//! Block admission, deploy intake, valid-block handling.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use block_storage::rust::dag::block_dag_key_value_storage::{
    DeployId, InsertMode, KeyValueDagRepresentation,
};
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;
use crate::rust::casper::{CasperSnapshot, DeployError};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::interpreter_util;

pub(crate) fn admit_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    admit_buffer_contains(this, hash) || admit_dag_contains(this, hash)
}

pub(crate) fn admit_dag_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    // Bootstrap-window safety (P1-2): if the DAG representation is not yet
    // initialized (no approved/last-finalized block), report `false` rather
    // than panicking. Returning `false` here matches the trait's
    // pre-existing semantics for "block not present".
    match this.block_dag_storage.get_representation() {
        Ok(dag) => dag.contains(hash),
        Err(_) => false,
    }
}

pub(crate) fn admit_buffer_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    let block_hash_serde = BlockHashSerde(hash.clone());
    this.casper_buffer_storage.contains(&block_hash_serde)
}

pub(crate) fn admit_get_approved_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<&BlockMessage, CasperError> {
    Ok(&this.approved_block)
}

pub(crate) fn admit_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    // Create normalizer environment from deploy
    let normalizer_env = normalizer_env_from_deploy(&deploy);
    let parse_started_at = std::time::Instant::now();

    // Try to parse the deploy term
    match interpreter_util::mk_term(&deploy.data.term, normalizer_env) {
        Err(interpreter_error) => {
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_started_at.elapsed().as_millis(),
                "Deploy parse failed"
            );
            Ok(Either::Left(DeployError::parsing_error(format!(
                "Error in parsing term: \n{}",
                interpreter_error
            ))))
        }
        Ok(_parsed_term) => {
            let parse_elapsed_ms = parse_started_at.elapsed().as_millis();
            let add_started_at = std::time::Instant::now();
            let deploy_id = add_deploy(this, deploy)?;
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_elapsed_ms,
                add_deploy_ms = add_started_at.elapsed().as_millis(),
                "Deploy parse/add completed"
            );
            Ok(Either::Right(deploy_id))
        }
    }
}

/// Multi-signature-aware admission. Validates the deploy via
/// `normalizer_env_from_cosigned_deploy` (so `rho:system:cosigners`
/// reflects the full cosigner list), enforces the configured
/// `max_cosigners_per_deploy` cap at the ingress boundary, then stores
/// the legacy `Signed<DeployData>` shape in the standard
/// `KeyValueDeployStorage` AND mirrors the cosigner extras +
/// primary_phlo_share into the `pending_cosigner_metadata` sidecar map
/// (keyed by primary signature). The sidecar is consulted by the
/// proposer-side `block_creator` to reconstruct the full Cosigned
/// envelope when handing deploys off to the runtime fan-out.
pub(crate) fn admit_deploy_cosigned<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    use models::rust::normalizer_env::normalizer_env_from_cosigned_deploy;
    let normalizer_env = normalizer_env_from_cosigned_deploy(&cosigned);
    let parse_started_at = std::time::Instant::now();
    match interpreter_util::mk_term(&cosigned.data.term, normalizer_env) {
        Err(interpreter_error) => {
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_started_at.elapsed().as_millis(),
                "Deploy parse failed (multi-sig path)"
            );
            Ok(Either::Left(DeployError::parsing_error(format!(
                "Error in parsing term: \n{}",
                interpreter_error
            ))))
        }
        Ok(_parsed_term) => {
            let max_cosigners = this.casper_shard_conf.max_cosigners_per_deploy as usize;
            if cosigned.signers().len() > max_cosigners {
                return Ok(Either::Left(DeployError::parsing_error(format!(
                    "Cosigner cap exceeded at ingress: {} signers > limit {}",
                    cosigned.signers().len(),
                    max_cosigners
                ))));
            }
            let parse_elapsed_ms = parse_started_at.elapsed().as_millis();
            let add_started_at = std::time::Instant::now();
            let deploy_id = add_deploy_cosigned(this, cosigned)?;
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_elapsed_ms,
                add_deploy_ms = add_started_at.elapsed().as_millis(),
                "Deploy parse/add completed (multi-sig path)"
            );
            Ok(Either::Right(deploy_id))
        }
    }
}

pub(crate) async fn admit_handle_valid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<KeyValueDagRepresentation, CasperError> {
    // Bug #17 / T-9.20: atomic (DAG insert, casper-buffer remove) pair
    // via the helper. The deploy-storage purge below runs OUTSIDE the
    // (DAG, buffer) critical section because deploy storage lives in a
    // third LMDB env with its own atomicity story. See
    // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
    let block_hash_serde = BlockHashSerde(block.block_hash.clone());
    let updated_dag = block_storage::rust::dag::buffer_dag_transition::atomic_insert_then_buffer(
        &this.block_dag_storage,
        block,
        InsertMode::Normal,
        &this.casper_buffer_storage,
        block_storage::rust::dag::buffer_dag_transition::BufferTransition::RemoveFromBuffer(
            block_hash_serde,
        ),
    )?;
    record_dag_cardinality_metrics(&updated_dag);

    // Remove user deploys from pending deploy storage as soon as the block is
    // accepted into the DAG.
    let deploys: Vec<_> = block
        .body
        .deploys
        .iter()
        .map(|pd| pd.deploy.clone())
        .collect();
    if !deploys.is_empty() {
        let deploys_count = deploys.len();
        let block_hash = PrettyPrinter::build_string_bytes(&block.block_hash);
        let block_number = block.body.state.block_number;
        // Drain the cosigner-metadata sidecar in lockstep with the legacy
        // deploy_storage. Both keyed by primary signature; identical key set
        // by construction (sidecar is only populated by `add_deploy_cosigned`,
        // which always also writes to `deploy_storage`).
        {
            let mut sidecar = this.pending_cosigner_metadata.lock();
            for deploy in &deploys {
                sidecar.remove(&deploy.sig);
            }
        }
        // Phase 9 (A-3): `deploy_storage` is `parking_lot::Mutex` — no
        // poison propagation, so `.lock()` returns the guard directly.
        this.deploy_storage.lock().remove(deploys)?;

        tracing::debug!(
            "Removed {} deploys from pending pool for accepted block {} at {}.",
            deploys_count,
            block_hash,
            block_number
        );
    }

    // Publish BlockAdded event
    this.event_publisher
        .publish(super::events::added_event(block))?;

    // Update last finalized block if needed
    super::finalization_runner::update_last_finalized_block(this, block).await?;

    // Wake heartbeat immediately when a new peer block is accepted.
    if let Some(validator_id) = &this.validator_id {
        if block.sender != validator_id.public_key.bytes {
            if let Some(signal) = this.heartbeat_signal_ref.get() {
                tracing::debug!(
                    "Triggering heartbeat wake for accepted peer block {}",
                    PrettyPrinter::build_string_bytes(&block.block_hash)
                );
                signal.trigger_wake();
            }
        }
    }

    Ok(updated_dag)
}

/// Multi-sig variant of `add_deploy`. Stores the legacy `Signed<DeployData>`
/// in the standard pool and mirrors the cosigner metadata into the
/// `pending_cosigner_metadata` sidecar so the proposer can reconstruct the
/// canonical Cosigned envelope at deploy selection time.
pub(crate) fn add_deploy_cosigned<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> Result<DeployId, CasperError> {
    let is_compound = cosigned.is_compound();
    // Extract cosigner metadata BEFORE consuming the envelope for storage.
    let metadata = if is_compound {
        let cosigners_proto: Vec<models::casper::CompoundSigner> = cosigned
            .signers()
            .iter()
            .skip(1)
            .map(|c| models::casper::CompoundSigner {
                pk: c.pk.bytes.clone().into(),
                sig: c.sig.clone(),
                sig_algorithm: c.sig_algorithm.name(),
            })
            .collect();
        Some(cosigners_proto)
    } else {
        None
    };
    let legacy_signed = cosigned.into_legacy_signed_unchecked();
    let primary_sig = legacy_signed.sig.clone();

    // Store in the legacy pool (selection-by-primary-signer semantics).
    this.deploy_storage
        .lock()
        .add(vec![legacy_signed.clone()])?;

    // Mirror cosigner extras into the sidecar map for proposer-side
    // reconstruction. Only populated for compound deploys; single-signer
    // deploys are uniquely identified by primary sig in the legacy pool.
    if let Some(cosigners) = metadata {
        this.pending_cosigner_metadata.lock().insert(
            primary_sig.clone(),
            super::types::PendingCosignerMetadata { cosigners },
        );
    }

    let deploy_info = PrettyPrinter::build_string_signed_deploy_data(&legacy_signed);
    tracing::info!(
        "Received (multi-sig path; is_compound={}): {}",
        is_compound,
        deploy_info
    );
    if this.casper_shard_conf.deploy_heartbeat_wake_enabled {
        if let Some(signal) = this.heartbeat_signal_ref.get() {
            signal.trigger_wake();
        }
    }
    Ok(primary_sig.to_vec())
}

pub(crate) fn add_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<DeployId, CasperError> {
    // Add deploy to storage. Phase 9 (A-3): parking_lot::Mutex.
    this.deploy_storage.lock().add(vec![deploy.clone()])?;

    // Log the received deploy
    let deploy_info = PrettyPrinter::build_string_signed_deploy_data(&deploy);
    tracing::info!("Received {}", deploy_info);

    // Wake the heartbeat immediately so it picks up the new deploy without
    // waiting for the next timer tick (up to check_interval seconds).
    // Phase 8 (C-4): operator-controlled via CasperShardConf rather than a
    // hardcoded predicate.
    if this.casper_shard_conf.deploy_heartbeat_wake_enabled {
        if let Some(signal) = this.heartbeat_signal_ref.get() {
            tracing::debug!("Triggering heartbeat wake for immediate block proposal");
            signal.trigger_wake();
        } else {
            tracing::debug!("No heartbeat signal available (heartbeat may be disabled)");
        }
    }

    // Return deploy signature as DeployId
    Ok(deploy.sig.to_vec())
}

/// C15 / Arch-3: extracted from `Casper::has_pending_deploys_in_storage_for_snapshot`
/// in `dispatch.rs`. The dispatch module is intended to host thin
/// trait delegates (one-line `super::<module>::<fn>` calls); the
/// 60-line body of this method belongs with the other admission /
/// deploy-pool helpers in this module.
pub(crate) async fn admit_has_pending_deploys_in_storage_for_snapshot<
    T: TransportLayer + Send + Sync,
>(
    this: &MultiParentCasperImpl<T>,
    snapshot: &CasperSnapshot,
) -> Result<bool, CasperError> {
    let latest_block_number = snapshot.dag.latest_block_number();
    let earliest_block_number =
        latest_block_number - snapshot.on_chain_state.shard_conf.deploy_lifespan;
    // Pre-epoch system clock (operationally impossible on modern
    // systems, but per-correctness directive: handle the corner). A
    // silent zero would make every deploy's `is_expired_at(0)` return
    // false (treating all timestamps as future), masking a corrupt
    // clock. Propagate as a typed `CasperError::RuntimeError` so the
    // call site fails loudly instead of silently corrupting deploy
    // expiration evaluation.
    let current_time_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|e| {
            CasperError::RuntimeError(format!(
                "system clock is before UNIX_EPOCH ({}); cannot evaluate \
                 deploy expiration",
                e
            ))
        })?;

    // Phase 9 (A-3): `deploy_storage` is `parking_lot::Mutex`.
    let storage = this.deploy_storage.lock();
    if !storage.non_empty().map_err(|e| {
        CasperError::RuntimeError(format!("Failed to query deploy storage: {:?}", e))
    })? {
        return Ok(false);
    }

    storage
        .any(|deploy| {
            let block_expired = deploy.data.valid_after_block_number <= earliest_block_number;
            let time_expired = deploy.data.is_expired_at(current_time_millis);
            if block_expired || time_expired {
                return Ok(false);
            }

            // `pending_deploy_is_future_for_next_block` is `pub(super)`
            // in `events`; the call resolves because `block_admission` is
            // a sibling sub-module of `events` under `engine::multi_parent_casper`.
            let is_future = super::events::pending_deploy_is_future_for_next_block(
                latest_block_number,
                deploy.data.valid_after_block_number,
            );
            let already_in_scope = snapshot.deploys_in_scope.contains(&deploy.sig);
            Ok(!is_future && !already_in_scope)
        })
        .map_err(|e| CasperError::RuntimeError(format!("Failed to scan deploy storage: {:?}", e)))
}
