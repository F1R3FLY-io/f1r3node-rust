//! Block admission, deploy intake, valid-block handling.
//!
//! Phase 3 Step 5 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use std::collections::HashSet;

use block_storage::rust::dag::block_dag_key_value_storage::{
    DeployId, InsertMode, KeyValueDagRepresentation,
};
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;
use crate::rust::casper::{Casper, CasperSnapshot, DeployError};
use crate::rust::deploy_admission::{
    AdmissionReservations, AdmissionRevision, FreshWindow, PendingDeploy, ReserveError,
};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    CASPER_METRICS_SOURCE, DEPLOY_ADMISSION_ACCEPTED_METRIC, DEPLOY_ADMISSION_CAPACITY_METRIC,
    DEPLOY_ADMISSION_PENDING_AHEAD_METRIC, DEPLOY_ADMISSION_REFUSED_METRIC,
};
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

pub(crate) fn admit_buffer_waits_on_dependency<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    this.casper_buffer_storage
        .get_parents(&BlockHashSerde(hash.clone()))
        .is_some_and(|parents| {
            parents
                .iter()
                .any(|parent| !admit_dag_contains(this, &BlockHash::from(parent.0.clone())))
        })
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
    // This fast path avoids parsing known deploys; reserve_deploy performs the authoritative check.
    if deploy_is_known(this, &deploy_id)? {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }

    // Create normalizer environment from deploy
    let normalizer_env = match normalizer_env_from_deploy(&deploy) {
        Ok(env) => env,
        Err(error) => return Ok(Either::Left(DeployError::parsing_error(error.to_string()))),
    };
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

fn deploy_is_known<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy_id: &DeployId,
) -> Result<bool, CasperError> {
    if this.deploy_storage.contains_sig(deploy_id)? {
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
        .lock()?
        .contains_sig(deploy_id)
        .map_err(Into::into)
}

fn reserve_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    let deploy_id = deploy.sig.to_vec();
    if this
        .block_dag_storage
        .deploy_canonical_appearance(&deploy_id)?
        .is_some()
    {
        return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
    }

    const MAX_REFRESH_ATTEMPTS: usize = 3;
    let mut refresh_changes = [None; MAX_REFRESH_ATTEMPTS];
    let shard_cap = this.casper_shard_conf.max_user_deploys_per_block as usize;
    for changed_component in &mut refresh_changes {
        let deploy_storage = this.deploy_storage.as_ref();
        let rejected_deploys = this.rejected_deploy_buffer.lock()?;
        if deploy_storage.contains_sig(&deploy_id)? || rejected_deploys.contains_sig(&deploy_id)? {
            return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
        }
        let mut capacity = this.deploy_admission.capacity(shard_cap, 1);
        let mut cache = this.deploy_admission.reservations.lock();
        let generation = this.block_dag_storage.current_generation();
        let lifecycle = this.block_dag_storage.deploy_status_revision();
        let dag = this.block_dag_storage.get_representation()?;
        if dag.deploy_terminal(&deploy_id)?.is_some()
            || dag.deploy_canonical_appearance(&deploy_id)?.is_some()
        {
            return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
        }
        let pool = deploy_storage.metadata_snapshot()?;
        let rejected = rejected_deploys.metadata_snapshot()?;
        let (earliest_valid_after, now_millis) =
            queue_bounds(&dag, this.casper_shard_conf.deploy_lifespan)?;
        let window = FreshWindow {
            earliest_valid_after,
            now_millis,
        };
        let revision = AdmissionRevision {
            pool: pool.generation,
            rejected: rejected.generation,
            dag: generation,
            lifecycle,
        };
        let monotonic = cache.window.is_some_and(|previous| {
            previous.earliest_valid_after <= earliest_valid_after
                && previous.now_millis <= now_millis
        });
        let reusable = lifecycle.is_some() && cache.revision == Some(revision) && monotonic;
        let incremental = lifecycle.is_some()
            && monotonic
            && cache.revision.is_some_and(|previous| {
                previous.rejected == revision.rejected
                    && previous.dag == generation
                    && previous.lifecycle == lifecycle
                    && pool.parent_generation == Some(previous.pool)
            });
        let is_fresh = |deploy: &PendingDeploy| -> Result<bool, KvStoreError> {
            Ok(deploy.valid_after_block_number > earliest_valid_after
                && deploy
                    .expiration_timestamp
                    .is_none_or(|expires| now_millis <= expires)
                && !rejected.entries.contains_key(&deploy.signature)
                && dag.deploy_terminal(&deploy.signature)?.is_none()
                && dag
                    .deploy_canonical_appearance(&deploy.signature)?
                    .is_none())
        };
        let book = if reusable || incremental {
            cache
                .book
                .as_ref()
                .expect("Admission reservation cache is missing")
                .clone()
        } else {
            let mut fresh = Vec::new();
            for deploy in pool.entries.values() {
                if is_fresh(deploy)? {
                    fresh.push(deploy.clone());
                }
            }
            AdmissionReservations::from_fresh(fresh, window).map_err(|error| {
                CasperError::RuntimeError(format!(
                    "Admission reservation refresh failed: {error:?}"
                ))
            })?
        };
        let mismatch = if generation != this.block_dag_storage.current_generation() {
            Some("dag")
        } else if lifecycle != this.block_dag_storage.deploy_status_revision() {
            Some("lifecycle")
        } else if pool.generation != deploy_storage.metadata_snapshot()?.generation {
            Some("pool")
        } else if rejected.generation != rejected_deploys.metadata_snapshot()?.generation {
            Some("rejected")
        } else {
            None
        };
        if let Some(component) = mismatch {
            *changed_component = Some(component);
            continue;
        }
        if incremental {
            cache.revision = None;
            for signature in &pool.removals {
                book.exclude(signature);
            }
            for deploy in &pool.additions {
                book.exclude(&deploy.signature);
                if is_fresh(deploy)? {
                    book.restore_accepted(deploy.clone(), window)
                        .map_err(|error| {
                            CasperError::RuntimeError(format!(
                                "Admission metadata update failed: {error:?}"
                            ))
                        })?;
                }
            }
        }
        cache.revision = Some(revision);
        cache.window = Some(window);
        cache.book = Some(book.clone());
        book.refresh_window(window);
        if capacity == 0 && book.occupied() == 0 && shard_cap > 0 {
            capacity = 1;
        }
        let metadata = PendingDeploy::from(&deploy);
        let pending_ahead = book.pending_ahead(&metadata);
        metrics::gauge!(DEPLOY_ADMISSION_PENDING_AHEAD_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(pending_ahead as f64);
        metrics::gauge!(DEPLOY_ADMISSION_CAPACITY_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(capacity as f64);
        let reservation = match book.try_reserve(metadata, capacity, window) {
            Ok(reservation) => reservation,
            Err(ReserveError::Duplicate) => {
                return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)))
            }
            Err(ReserveError::Full { .. }) => {
                metrics::counter!(DEPLOY_ADMISSION_REFUSED_METRIC, "source" => CASPER_METRICS_SOURCE)
                .increment(1);
                return Ok(Either::Left(DeployError::PoolBusy {
                    pending_ahead,
                    capacity,
                }));
            }
            Err(error) => {
                return Err(CasperError::RuntimeError(format!(
                    "Deploy reservation failed: {error:?}"
                )))
            }
        };
        cache.revision = None;
        if !deploy_storage.add_if_absent(deploy)? {
            return Ok(Either::Left(DeployError::duplicate_deploy(deploy_id)));
        }
        reservation.commit(window);
        if let Ok(updated) = deploy_storage.metadata_snapshot() {
            if updated.parent_generation == Some(revision.pool) {
                cache.revision = Some(AdmissionRevision {
                    pool: updated.generation,
                    ..revision
                });
            }
        }
        metrics::counter!(DEPLOY_ADMISSION_ACCEPTED_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        return Ok(Either::Right(deploy_id));
    }
    tracing::warn!(
        attempts = MAX_REFRESH_ATTEMPTS,
        first_changed_component_per_attempt = ?refresh_changes,
        "Deploy admission snapshot refresh exhausted"
    );
    Ok(Either::Left(DeployError::AdmissionBusy))
}

fn queue_bounds(
    dag: &KeyValueDagRepresentation,
    deploy_lifespan: i64,
) -> Result<(i64, i64), CasperError> {
    let next_block = dag.latest_block_number().checked_add(1).ok_or_else(|| {
        CasperError::RuntimeError("Next block height exceeds the supported range".to_string())
    })?;
    let earliest =
        crate::rust::util::deploy_window::earliest_valid_after(next_block, deploy_lifespan)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|_| {
            CasperError::RuntimeError("System time exceeds the supported range".to_string())
        })?;
    Ok((earliest, now))
}

fn deploy_is_fresh(
    dag: &KeyValueDagRepresentation,
    buffered: &HashSet<Bytes>,
    deploy: &Signed<DeployData>,
    earliest: i64,
    now: i64,
) -> Result<bool, KvStoreError> {
    Ok(deploy.data.valid_after_block_number > earliest
        && !deploy.data.is_expired_at(now)
        && !buffered.contains(&deploy.sig)
        && dag.deploy_terminal(&deploy.sig)?.is_none()
        && dag.deploy_canonical_appearance(&deploy.sig)?.is_none())
}

fn local_service_sample<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<Option<(usize, bool)>, CasperError> {
    if !this
        .validator_id
        .as_ref()
        .is_some_and(|id| id.public_key.bytes == block.sender)
    {
        return Ok(None);
    }
    let storage = this.deploy_storage.as_ref();
    let rejected = this.rejected_deploy_buffer.lock()?;
    let dag = this.block_dag_storage.get_representation()?;
    if dag.contains(&block.block_hash) {
        return Ok(None);
    }
    let (earliest, now) = queue_bounds(&dag, this.casper_shard_conf.deploy_lifespan)?;
    let buffered: HashSet<Bytes> = rejected
        .read_all()?
        .into_iter()
        .map(|deploy| deploy.sig)
        .collect();
    let mut included = HashSet::new();
    for processed in &block.body.deploys {
        let deploy = &processed.deploy;
        if storage.contains_sig(&deploy.sig)?
            && deploy_is_fresh(&dag, &buffered, deploy, earliest, now)?
        {
            included.insert(deploy.sig.clone());
        }
    }
    let had_demand = !included.is_empty()
        || storage.any(|deploy| {
            Ok(
                deploy.data.valid_after_block_number < block.body.state.block_number
                    && deploy_is_fresh(&dag, &buffered, deploy, earliest, now)?,
            )
        })?;
    Ok(Some((included.len(), had_demand)))
}

pub(crate) async fn admit_handle_valid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<KeyValueDagRepresentation, CasperError> {
    let service_sample = match local_service_sample(this, block) {
        Ok(sample) => sample,
        Err(error) => {
            tracing::warn!(%error, "Failed to measure local deploy inclusion capacity");
            None
        }
    };
    // Bug #17 / T-9.20: atomic (DAG insert, casper-buffer remove) pair
    // via the helper. See
    // docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
    //
    // User deploys are intentionally NOT purged from pending storage on mere
    // DAG acceptance. They are retained until the finalizer writes their
    // terminal verdict, so an accepted-but-orphaned deploy can be re-proposed
    // via the canonical-won record before it is lost.
    this.block_store.persist_block(block)?;
    let block_hash_serde = BlockHashSerde(block.block_hash.clone());
    let insert_start = std::time::Instant::now();
    let updated_dag = block_storage::rust::dag::buffer_dag_transition::atomic_insert_then_buffer(
        &this.block_dag_storage,
        block,
        InsertMode::Normal,
        &this.casper_buffer_storage,
        block_storage::rust::dag::buffer_dag_transition::BufferTransition::RemoveFromBuffer(
            block_hash_serde,
        ),
    )?;
    if let Some((included, had_demand)) = service_sample {
        this.deploy_admission
            .observe(block.seq_num, included, had_demand);
    }
    let insert_ms = insert_start.elapsed().as_millis();
    if insert_ms >= 1_000 {
        tracing::warn!(
            target: "f1r3fly.casper.lifecycle.timing",
            "DAG insert slow: block_number={}, insert_ms={}",
            block.body.state.block_number,
            insert_ms
        );
    }
    record_dag_cardinality_metrics(&updated_dag);

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

pub(crate) fn add_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    let deploy_id = deploy.sig.to_vec();
    if let Either::Left(error) = reserve_deploy(this, deploy.clone())? {
        return Ok(Either::Left(error));
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

    let storage = &this.deploy_storage;
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
pub(crate) async fn admit_list_pending_deploys<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<Vec<(Signed<DeployData>, bool)>, CasperError> {
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
        .lock()?
        .read_all()
        .map_err(|e| {
            CasperError::RuntimeError(format!("Failed to read rejected deploy buffer: {:?}", e))
        })?;
    let buffered_sigs: HashSet<Bytes> = rejected.iter().map(|d| d.sig.clone()).collect();

    let mut out: Vec<(Signed<DeployData>, bool)> = Vec::with_capacity(rejected.len());

    let fresh = this.deploy_storage.read_all().map_err(|e| {
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
            out.push((deploy, false));
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
            out.push((deploy, true));
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
