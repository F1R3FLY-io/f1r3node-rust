//! Block admission, deploy intake, valid-block handling.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::{
    DeployId, InsertMode, KeyValueDagRepresentation,
};
// Added by DR-134 (DR-66): a pending deploy is a legacy deploy or an offered
// envelope.
use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeFormat};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;
use crate::rust::casper::{Casper, CasperSnapshot, DeployError};
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::genesis_resource_policy::OFFERED_PRODUCTION_READY;
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

// Changed by DR-134 (DR-66 items 2 and 5): only offered-funded deploys are
// accepted, on every shard. A body-only deploy is refused before any check;
// the body-only admission of the legacy meter is deleted.
pub(crate) fn admit_deploy<T: TransportLayer + Send + Sync>(
    _this: &MultiParentCasperImpl<T>,
    _deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    Err(CasperError::RuntimeError(
        "new deploys require offered-funded cost accounting".to_string(),
    ))
}

pub(crate) fn admit_deploy_envelope<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    envelope: DeployEnvelope,
    adopted_policy: &crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    // Changed by DR-99 (joined-node policy adoption): the policy identity is
    // the genesis post-state, which differs from an LFS-joined node's anchor.
    // The submitted policy must be the one this node adopted at start.
    // if !OFFERED_PRODUCTION_READY
    //     || !adopted_policy.offered_funded_v6_active()
    //     || adopted_policy.genesis().genesis_root()
    //         != &this.approved_block.body.state.post_state_hash
    // {
    if !OFFERED_PRODUCTION_READY
        || !adopted_policy.offered_funded_v6_active()
        || this.adopted_resource_policy.as_ref().is_none_or(|own| {
            own.genesis().genesis_root() != adopted_policy.genesis().genesis_root()
        })
    {
        return Err(CasperError::RuntimeError(
            "offered-funded deploy admission is not active under the current genesis policy"
                .to_string(),
        ));
    }
    if envelope.format() != DeployEnvelopeFormat::OfferedFunded {
        return Err(CasperError::RuntimeError(
            "only offered-funded envelopes use funded admission".to_string(),
        ));
    }
    let deploy_id = envelope.identity().as_bytes().to_vec();
    if deploy_is_known(this, &deploy_id)? {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }
    let normalizer_env = normalizer_env_from_envelope(&envelope);
    if let Err(interpreter_error) = interpreter_util::mk_term(&envelope.body().term, normalizer_env)
    {
        return Ok(Either::Left(DeployError::parsing_error(format!(
            "Error in parsing term: \n{}",
            interpreter_error
        ))));
    }
    if !this
        .deploy_storage
        .lock()
        .add_envelope_if_absent(&envelope)?
    {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }
    if this.casper_shard_conf.deploy_heartbeat_wake_enabled {
        if let Some(signal) = this.heartbeat_signal_ref.get() {
            signal.trigger_wake();
        }
    }
    Ok(Either::Right(deploy_id))
}

fn deploy_is_known<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy_id: &DeployId,
) -> Result<bool, CasperError> {
    if this.deploy_storage.lock().contains_sig(deploy_id)? {
        return Ok(true);
    }
    if let Ok(id) = models::rust::deploy_id::DeployIdV6::try_from(deploy_id.as_slice()) {
        if this.deploy_storage.lock().contains_envelope_id(&id)? {
            return Ok(true);
        }
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
        .contains_sig(deploy_id)
        .map_err(Into::into)
}

pub(crate) async fn admit_handle_valid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<KeyValueDagRepresentation, CasperError> {
    // Bug #17 / T-9.20: atomic (DAG insert, casper-buffer remove) pair
    // via the helper. See
    // docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
    //
    // Sealed-floor (record-driven recovery): user deploys are intentionally
    // NOT purged from pending storage on mere DAG acceptance. They are
    // retained through accept and removed only once finalized (see
    // finalization_runner), so an accepted-but-orphaned deploy can be
    // re-proposed via the canonical-won record before it is lost.
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
        let mut storage = this.deploy_storage.lock();
        for sig in &terminalized {
            storage.remove_by_sig(sig)?;
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

fn stored_deploy_is_pending_for_snapshot(
    snapshot: &CasperSnapshot,
    latest_block_number: i64,
    earliest_block_number: i64,
    current_time_millis: i64,
    deploy: &Signed<DeployData>,
) -> bool {
    let block_expired = deploy.data.valid_after_block_number <= earliest_block_number;
    let time_expired = deploy.data.is_expired_at(current_time_millis);
    if block_expired || time_expired {
        return false;
    }

    let is_future = super::events::pending_deploy_is_future_for_next_block(
        latest_block_number,
        deploy.data.valid_after_block_number,
    );
    let already_in_scope = snapshot.deploys_in_scope.contains(&deploy.sig)
        && !snapshot.rejected_in_scope.contains(&deploy.sig);
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
    deploy: &Signed<DeployData>,
) -> bool {
    let block_expired = deploy.data.valid_after_block_number <= earliest_block_number;
    let time_expired = deploy.data.is_expired_at(current_time_millis);
    if block_expired || time_expired {
        return false;
    }

    let already_in_scope = snapshot.deploys_in_scope.contains(&deploy.sig)
        && !snapshot.rejected_in_scope.contains(&deploy.sig);
    !already_in_scope
}

/// Added by DR-134 (DR-66): `stored_deploy_is_queued` for an offered
/// envelope. An envelope keys the scope by its deploy id, as the block creator
/// does.
fn stored_envelope_is_queued(
    snapshot: &CasperSnapshot,
    earliest_block_number: i64,
    current_time_millis: i64,
    envelope: &DeployEnvelope,
) -> bool {
    let body = envelope.body();
    let block_expired = body.valid_after_block_number <= earliest_block_number;
    let time_expired = body.is_expired_at(current_time_millis);
    if block_expired || time_expired {
        return false;
    }

    let identity = Bytes::copy_from_slice(envelope.identity().as_bytes());
    let already_in_scope = snapshot.deploys_in_scope.contains(&identity)
        && !snapshot.rejected_in_scope.contains(&identity);
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

    storage
        .any(|deploy| {
            Ok(stored_deploy_is_pending_for_snapshot(
                snapshot,
                latest_block_number,
                earliest_block_number,
                current_time_millis,
                deploy,
            ))
        })
        .map_err(|e| CasperError::RuntimeError(format!("Failed to scan deploy storage: {:?}", e)))
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
// Changed by DR-134 (DR-66): the listing returns legacy deploys and offered
// envelopes; on protocol 6 the pool holds only offered envelopes.
// pub(crate) async fn admit_list_pending_deploys<T: TransportLayer + Send + Sync>(
//     this: &MultiParentCasperImpl<T>,
// ) -> Result<Vec<(Signed<DeployData>, bool)>, CasperError> {
pub(crate) async fn admit_list_pending_deploys<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<(PendingDeployCandidate, bool)>, CasperError> {
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
    let buffered_sigs: HashSet<Bytes> = rejected.iter().map(|d| d.sig.clone()).collect();

    // Changed by DR-134 (DR-66): the entries carry both formats.
    // let mut out: Vec<(Signed<DeployData>, bool)> = Vec::with_capacity(rejected.len());
    let mut out: Vec<(PendingDeployCandidate, bool)> = Vec::with_capacity(rejected.len());

    let fresh = this.deploy_storage.lock().read_all().map_err(|e| {
        CasperError::RuntimeError(format!("Failed to read deploy storage: {:?}", e))
    })?;
    for deploy in fresh {
        if buffered_sigs.contains(&deploy.sig) {
            continue;
        }
        if stored_deploy_is_queued(
            &snapshot,
            earliest_block_number,
            current_time_millis,
            &deploy,
        ) {
            // Changed by DR-134 (DR-66).
            // out.push((deploy, false));
            out.push((PendingDeployCandidate::Legacy(deploy), false));
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
            // Changed by DR-134 (DR-66).
            // out.push((deploy, true));
            out.push((PendingDeployCandidate::Legacy(deploy), true));
        }
    }

    // Added by DR-134 (DR-66): the offered envelopes of both pools, under the
    // rules of the legacy deploys above. An envelope in both pools is emitted
    // once, as rejected.
    let envelope_limits = models::rust::cost_protocol_limits::offered_funded_v6_limits().envelope;
    let rejected_envelopes = this
        .rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all_envelopes(envelope_limits)
        .map_err(|e| {
            CasperError::RuntimeError(format!(
                "Failed to read rejected deploy buffer envelopes: {:?}",
                e
            ))
        })?;
    let buffered_ids: HashSet<Vec<u8>> = rejected_envelopes
        .iter()
        .map(|envelope| envelope.identity().as_bytes().to_vec())
        .collect();
    let fresh_envelopes = this
        .deploy_storage
        .lock()
        .read_all_envelopes(envelope_limits)
        .map_err(|e| {
            CasperError::RuntimeError(format!("Failed to read deploy storage envelopes: {:?}", e))
        })?;
    out.reserve(fresh_envelopes.len() + rejected_envelopes.len());
    for candidate in fresh_envelopes {
        let PendingDeployCandidate::Envelope(envelope) = candidate else {
            continue;
        };
        if buffered_ids.contains(envelope.identity().as_bytes()) {
            continue;
        }
        if stored_envelope_is_queued(
            &snapshot,
            earliest_block_number,
            current_time_millis,
            &envelope,
        ) {
            out.push((PendingDeployCandidate::Envelope(envelope), false));
        }
    }
    for envelope in rejected_envelopes {
        if stored_envelope_is_queued(
            &snapshot,
            earliest_block_number,
            current_time_millis,
            &envelope,
        ) {
            out.push((PendingDeployCandidate::Envelope(envelope), true));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::stored_deploy_is_pending_for_snapshot;
    use crate::rust::casper::test_helpers::TestCasperWithSnapshot;
    use crate::rust::util::construct_deploy;

    #[test]
    fn rejected_in_scope_storage_deploy_remains_pending() {
        let snapshot = TestCasperWithSnapshot::create_empty_snapshot();
        let deploy = construct_deploy::basic_deploy_data(91, None, Some("test".to_string()))
            .expect("deploy");

        assert!(stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &deploy,
        ));

        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        assert!(!stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &deploy,
        ));

        snapshot.rejected_in_scope.insert(deploy.sig.clone());
        assert!(stored_deploy_is_pending_for_snapshot(
            &snapshot,
            20,
            -1,
            deploy.data.time_stamp,
            &deploy,
        ));
    }
}
