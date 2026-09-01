//! Block admission, deploy intake, valid-block handling.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::{
    CertifiedAdmissionOutcome, CertifiedSenderAuthority, DeployId, InsertMode,
    KeyValueDagRepresentation,
};
use block_storage::rust::deploy::pending_deploy::PendingDeploy;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId, LegacyDeploySignature};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;
use crate::rust::casper::{Casper, CasperSnapshot, DeployError};
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
    let deploy_id = deploy.sig.to_vec();
    if this.casper_shard_conf.casper_version >= 6 {
        return Ok(Either::Left(DeployError::parsing_error(
            "protocol-v6 admission requires authenticated v6.1 authorization".to_string(),
        )));
    }
    // This fast path avoids parsing known deploys; reserve_deploy performs the authoritative check.
    if deploy_is_known(
        this,
        &DeployLookupId::Legacy(LegacyDeploySignature::new(deploy_id.clone())),
    )? {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }

    // Create normalizer environment from deploy
    let normalizer_env = normalizer_env_from_deploy(&deploy);
    let parse_started_at = std::time::Instant::now();

    // Try to parse the deploy term
    match interpreter_util::mk_term(&deploy.data.term, normalizer_env) {
        Err(interpreter_error) => {
            tracing::debug!(
                target: "f1r3fly.casper.deploy.timing",
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
            let deploy_result = add_deploy(this, deploy)?;
            tracing::debug!(
                target: "f1r3fly.casper.deploy.timing",
                parse_ms = parse_elapsed_ms,
                add_deploy_ms = add_started_at.elapsed().as_millis(),
                "Deploy parse/add completed"
            );
            Ok(deploy_result)
        }
    }
}

/// Multi-signature-aware admission. Validates the deploy via
/// `normalizer_env_from_cosigned_deploy` (so `rho:system:cosigners`
/// reflects the full cosigner list), enforces the configured
/// `max_cosigners_per_deploy` cap at the ingress boundary, then stores
/// the legacy `Signed<DeployData>` shape in the standard
/// the canonical protocol-v6 envelope in `KeyValueDeployStorage`.
pub(crate) fn admit_deploy_cosigned<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    use models::rust::normalizer_env::normalizer_env_from_cosigned_deploy;
    if this.casper_shard_conf.casper_version >= 6 && !cosigned.is_envelope_bound() {
        return Ok(Either::Left(DeployError::parsing_error(
            "protocol-v6 admission requires authenticated v6.1 authorization".to_string(),
        )));
    }
    if this.casper_shard_conf.casper_version < 6 && cosigned.is_envelope_bound() {
        return Ok(Either::Left(DeployError::parsing_error(
            "pre-v6 admission cannot accept protocol-v6 authorization".to_string(),
        )));
    }
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
            let deploy_result = add_deploy_cosigned(this, cosigned)?;
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_elapsed_ms,
                add_deploy_ms = add_started_at.elapsed().as_millis(),
                "Deploy parse/add completed (multi-sig path)"
            );
            Ok(deploy_result)
        }
    }
}

fn deploy_is_known<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy_id: &DeployLookupId,
) -> Result<bool, CasperError> {
    let in_pool = match deploy_id {
        DeployLookupId::Legacy(signature) => this
            .deploy_storage
            .lock()
            .contains_sig(signature.as_bytes())?,
        DeployLookupId::V6(deploy_id) => this
            .deploy_storage
            .lock()
            .contains_envelope(deploy_id.as_ref())?,
    };
    if in_pool {
        return Ok(true);
    }
    if this
        .block_dag_storage
        .deploy_canonical_appearance(deploy_id)?
        .is_some()
    {
        return Ok(true);
    }
    this.rejected_deploy_buffer
        .lock()
        .map_err(|error| CasperError::LockError(error.to_string()))?
        .contains_id(deploy_id)
        .map_err(Into::into)
}

fn reserve_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<bool, CasperError> {
    let deploy_id = deploy.sig.to_vec();
    let typed_deploy_id = DeployLookupId::Legacy(LegacyDeploySignature::new(deploy_id.clone()));
    if this
        .block_dag_storage
        .deploy_canonical_appearance(&typed_deploy_id)?
        .is_some()
    {
        return Ok(false);
    }

    let mut deploy_storage = this.deploy_storage.lock();
    deploy_storage.add_if_absent(deploy).map_err(Into::into)
}

fn reserve_deploy_envelope<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    envelope: crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> Result<bool, CasperError> {
    let deploy_id = envelope
        .envelope_commitment()
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    let typed_deploy_id = DeployLookupId::V6(
        DeployIdV6::try_from(deploy_id.as_ref())
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?,
    );
    if this
        .block_dag_storage
        .deploy_canonical_appearance(&typed_deploy_id)?
        .is_some()
    {
        return Ok(false);
    }
    let mut deploy_storage = this.deploy_storage.lock();
    let rejected_deploys = this
        .rejected_deploy_buffer
        .lock()
        .map_err(|error| CasperError::LockError(error.to_string()))?;
    if rejected_deploys.contains_id(&typed_deploy_id)? {
        return Ok(false);
    }
    deploy_storage
        .add_envelope_if_absent(envelope)
        .map_err(Into::into)
}

pub(crate) async fn admit_handle_valid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    certificate: &CertifiedSenderAuthority,
    outcome: &CertifiedAdmissionOutcome,
) -> Result<KeyValueDagRepresentation, CasperError> {
    // Bug #17 / T-9.20: atomic (DAG insert, casper-buffer remove) pair
    // via the helper. See
    // docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
    //
    // Sealed-floor (record-driven recovery): user deploys are intentionally
    // NOT purged from pending storage on mere DAG acceptance. They are
    // retained through accept and removed only after the deploy-lifecycle
    // register writes a floor-terminal verdict, so an accepted-but-orphaned
    // deploy can be re-proposed before it is lost.
    let block_hash_serde = BlockHashSerde(block.block_hash.clone());
    let updated_dag = block_storage::rust::dag::buffer_dag_transition::atomic_insert_then_buffer(
        &this.block_dag_storage,
        block,
        InsertMode::Normal,
        certificate,
        outcome,
        &this.casper_buffer_storage,
        block_storage::rust::dag::buffer_dag_transition::BufferTransition::RemoveFromBuffer(
            block_hash_serde,
        ),
    )?;
    record_dag_cardinality_metrics(&updated_dag);
    let finalization_revision = this
        .block_dag_storage
        .finalization_head()?
        .map(|head| head.revision)
        .unwrap_or(0);

    // Advance the deploy-lifecycle register: the insert above already
    // ingested the block's body into the lifecycle event rows; this bumps
    // the register's clocks and evaluates the sigs whose thresholds
    // crossed. A crash between insert and this step only delays a verdict
    // (the schedule re-arms from the persisted open rows).
    let terminalized = this
        .deploy_lifecycle
        .observe_block(
            &updated_dag,
            &this.block_store,
            block,
            this.casper_shard_conf.deploy_lifespan,
            crate::rust::finality::deploy_lifecycle::citability_horizon(
                this.casper_shard_conf.max_parent_depth,
            ),
            finalization_revision,
        )
        .await?;

    // Release the proposer's pool copy of every sig the register just
    // settled. This is the ONLY deploy-pool eviction on the finality path:
    // the register is what re-evaluates as the floor advances, so it is
    // the only component that can name the moment a deploy stops being
    // re-proposable. Keying this on the finality marker instead destroys
    // work — a marked block can still be orphaned, and an orphaned carrier
    // yields no rejection record, so the pool copy is its only route back
    // into a live branch. Non-owners simply do not hold the sig (deploys
    // never gossip) and the removal is a no-op.
    if !terminalized.is_empty() {
        {
            let mut storage = this.deploy_storage.lock();
            for sig in &terminalized {
                match sig {
                    models::rust::deploy_id::DeployLookupId::Legacy(signature) => {
                        storage.remove_by_sig(signature.as_bytes())?;
                    }
                    models::rust::deploy_id::DeployLookupId::V6(deploy_id) => {
                        storage.remove_envelope_by_id(deploy_id.as_ref())?;
                    }
                }
            }
        }
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
/// in the canonical protocol-v6 envelope store.
pub(crate) fn add_deploy_cosigned<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    let is_compound = cosigned.is_compound();
    let deploy_id = cosigned
        .envelope_commitment()
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    let legacy_signed = cosigned.as_legacy_signed_ref();
    if !reserve_deploy_envelope(this, cosigned)? {
        return Ok(Either::Left(DeployError::duplicate_deploy(
            deploy_id.to_vec(),
        )));
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
    Ok(Either::Right(deploy_id.to_vec()))
}

pub(crate) fn add_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    let deploy_id = deploy.sig.to_vec();
    if !reserve_deploy(this, deploy.clone())? {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }

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

    Ok(Either::Right(deploy_id))
}

fn stored_deploy_is_pending_for_snapshot(
    snapshot: &CasperSnapshot,
    latest_block_number: i64,
    earliest_block_number: i64,
    current_time_millis: i64,
    deploy: &PendingDeploy,
) -> bool {
    let block_expired = deploy.data().valid_after_block_number <= earliest_block_number;
    let time_expired = deploy.data().is_expired_at(current_time_millis);
    if block_expired || time_expired {
        return false;
    }

    let is_future = super::events::pending_deploy_is_future_for_next_block(
        latest_block_number,
        deploy.data().valid_after_block_number,
    );
    let already_in_scope = snapshot.deploys_in_scope.contains(deploy.typed_deploy_id())
        && !snapshot
            .rejected_in_scope
            .contains(deploy.typed_deploy_id());
    !is_future && !already_in_scope
}

/// Whether a stored deploy is still WAITING to land, for the reporting API.
///
/// Deliberately weaker than `stored_deploy_is_pending_for_snapshot`, which
/// answers "may the proposer put this in the NEXT block". The two differ on
/// one clause: a deploy whose `valid_after_block_number` is ahead of the tip
/// is not proposable yet, but it is submitted, queued, and will land once the
/// chain reaches its window — so it is pending to a caller asking "where is my
/// deploy". Using the proposer predicate here would report it as absent.
///
/// The three exclusions are the ones that mean "will never land, or already
/// did": the validity window closed on block height, the expiration timestamp
/// passed, or the deploy is already in the merge scope.
fn stored_deploy_is_queued(
    snapshot: &CasperSnapshot,
    earliest_block_number: i64,
    current_time_millis: i64,
    deploy: &PendingDeploy,
) -> bool {
    let block_expired = deploy.data().valid_after_block_number <= earliest_block_number;
    let time_expired = deploy.data().is_expired_at(current_time_millis);
    if block_expired || time_expired {
        return false;
    }

    let already_in_scope = snapshot.deploys_in_scope.contains(deploy.typed_deploy_id())
        && !snapshot
            .rejected_in_scope
            .contains(deploy.typed_deploy_id());
    !already_in_scope
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
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        latest_block_number,
        snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
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

    Ok(storage
        .read_all_for_protocol(snapshot.on_chain_state.shard_conf.casper_version)?
        .iter()
        .any(|deploy| {
            stored_deploy_is_pending_for_snapshot(
                snapshot,
                latest_block_number,
                earliest_block_number,
                current_time_millis,
                deploy,
            )
        }))
}

/// C15 / Arch-3: extracted from `Casper::list_pending_deploys` in
/// `dispatch.rs`. The dispatch module hosts only thin trait delegates; the
/// read-and-pair body lives with the other deploy-pool helpers here.
///
/// Returns a bulk snapshot of pending deploys from both `deploy_storage`
/// (fresh, not yet proposed) and `rejected_deploy_buffer` (recovering
/// after a merge conflict). Each entry is paired with an `is_rejected`
/// flag: `false` for fresh, `true` for the recovery backlog.
///
/// Fresh deploys are filtered by the same predicate as
/// `fresh_local_deploy_stats` and `has_pending_deploys_in_storage_for_snapshot`:
/// a deploy already in a block, one whose `valid_after_block_number` is
/// ahead of the tip, a block-expired one, and a time-expired one are each
/// excluded. A signature that sits in both pools is emitted once, with
/// `is_rejected = true` (the buffer dedups storage in its first clause).
pub(crate) async fn admit_list_pending_deploys<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<(Cosigned<DeployData>, bool)>, CasperError> {
    let snapshot = this.get_snapshot().await?;
    let latest_block_number = snapshot.dag.latest_block_number();
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        latest_block_number,
        snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
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

    let rejected = this
        .rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all()
        .map_err(|e| {
            CasperError::RuntimeError(format!("Failed to read rejected deploy buffer: {:?}", e))
        })?;
    let buffered_sigs: HashSet<DeployLookupId> = rejected
        .iter()
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect();

    let mut out: Vec<(Cosigned<DeployData>, bool)> = Vec::with_capacity(rejected.len());

    let fresh = this
        .deploy_storage
        .lock()
        .read_all_for_protocol(snapshot.on_chain_state.shard_conf.casper_version)?;
    for deploy in fresh {
        if buffered_sigs.contains(deploy.typed_deploy_id()) {
            continue;
        }
        if stored_deploy_is_queued(
            &snapshot,
            earliest_block_number,
            current_time_millis,
            &deploy,
        ) {
            out.push((deploy.into_envelope(), false));
        }
    }

    // The recovery backlog gets the same test as the fresh pool. A deploy
    // whose window has closed while it sat here can never land, so reporting
    // it as pending is the same wrong answer in the other pool — the buffer
    // is only purged when a proposal runs, so a node that is not proposing
    // would report it indefinitely.
    for deploy in rejected {
        if stored_deploy_is_queued(
            &snapshot,
            earliest_block_number,
            current_time_millis,
            &deploy,
        ) {
            out.push((deploy.into_envelope(), true));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use block_storage::rust::deploy::pending_deploy::PendingDeploy;

    use super::stored_deploy_is_pending_for_snapshot;
    use crate::rust::casper::test_helpers::TestCasperWithSnapshot;
    use crate::rust::util::construct_deploy;

    #[test]
    fn rejected_in_scope_storage_deploy_remains_pending() {
        let snapshot = TestCasperWithSnapshot::create_empty_snapshot();
        let deploy = construct_deploy::basic_deploy_data(91, None, Some("test".to_string()))
            .expect("deploy");
        let pending = PendingDeploy::from_legacy(deploy.clone()).expect("pending deploy");

        assert!(stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &pending,
        ));

        snapshot
            .deploys_in_scope
            .insert(pending.typed_deploy_id().clone());
        assert!(!stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &pending,
        ));

        snapshot
            .rejected_in_scope
            .insert(pending.typed_deploy_id().clone());
        assert!(stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &pending,
        ));
    }
}
