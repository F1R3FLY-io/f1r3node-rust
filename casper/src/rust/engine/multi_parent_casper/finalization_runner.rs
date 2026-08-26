//! Finalization runner — background task, RAII guard,
//! `compute_last_finalized_block`, `update_last_finalized_block`.
//!
//! Phase 3 (Commit 2): extracted from `engine::multi_parent_casper`.
//! The functions here are reachable via:
//!   * `MultiParentCasper::last_finalized_block` (mod.rs) →
//!     `compute_last_finalized_block` (here)
//!   * `block_admission::admit_handle_valid_block` (block_admission.rs) →
//!     `self.update_last_finalized_block` (inherent method here)
//!   * background task spawned by `update_last_finalized_block` →
//!     `run_queued_finalizer` → `compute_last_finalized_block`

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, FinalizationWitnessInputs,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::finality::{
    FinalizationEffectId, FinalizationEffectKind, FinalizationRecord,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::validator::ValidatorSerde;
// Phase 9 (A-3): deploy_storage uses parking_lot::Mutex.
use parking_lot::Mutex;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KvStoreError;

// Phase 7 (C-3): import the struct from its canonical sibling module
// instead of via the legacy shim — the previous import formed a circular
// path `engine::multi_parent_casper → engine::multi_parent_casper → engine::multi_parent_casper::types`.
use super::events::finalised_event;
use super::types::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::finality::finalization_schedule::FinalizationSchedule;
use crate::rust::finality::finalizer::Finalizer;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// RAII guard that ensures the finalization flag is reset on drop.
/// This prevents the flag from being stuck in `true` state if the async block
/// panics or returns early via `?` operator.
struct FinalizationGuard<'a>(&'a AtomicU64);

impl Drop for FinalizationGuard<'_> {
    fn drop(&mut self) {
        let previous = self.0.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous > 0);
    }
}

struct FinalizationDispatcherGuard(Arc<FinalizationSchedule>);

impl Drop for FinalizationDispatcherGuard {
    fn drop(&mut self) { self.0.clear_dispatcher(); }
}

#[derive(Clone, Copy)]
enum FinalizationWorkerOutcome {
    Succeeded,
    Failed,
}

fn settle_finalization_worker(
    schedule: &FinalizationSchedule,
    covered_through: u64,
    outcome: FinalizationWorkerOutcome,
) -> Option<std::time::Duration> {
    match outcome {
        FinalizationWorkerOutcome::Succeeded => {
            schedule.mark_succeeded(covered_through);
            None
        }
        FinalizationWorkerOutcome::Failed => schedule.mark_failed(covered_through),
    }
}

/// Phase 8 (PO-3): bundles the 9 service handles + tuning flags that
/// `compute_last_finalized_block` and `run_queued_finalizer` need. Avoids
/// the previous 9-/11-arg signatures (silenced by
/// `#[allow(clippy::too_many_arguments)]`). The struct is `Clone` because
/// the finalization-effect closure captures by move into a
/// `FnMut + Send + Sync`.
#[derive(Clone)]
pub(crate) struct FinalizationContext {
    pub(crate) block_dag_storage: BlockDagKeyValueStorage,
    pub(crate) block_store: KeyValueBlockStore,
    pub(crate) deploy_storage: Arc<Mutex<KeyValueDeployStorage>>,
    pub(crate) rejected_deploy_buffer: Arc<std::sync::Mutex<KeyValueRejectedDeployBuffer>>,
    /// Cosigner-metadata sidecar (keyed by primary signature). Drained in
    /// lockstep with `deploy_storage` when a block's deploys are finalized, so
    /// compound-deploy metadata stays bounded after canonical inclusion. Under
    /// sealed-floor, deploys are retained through accept and purged only at
    /// finalization, so this drain was relocated here from block admission.
    pub(crate) pending_cosigner_metadata: Arc<
        Mutex<
            std::collections::HashMap<prost::bytes::Bytes, super::types::PendingCosignerMetadata>,
        >,
    >,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicU64>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) ftt: FtThreshold,
    pub(crate) finalizer_conf: crate::rust::casper_conf::FinalizerConf,
}

/// Build a `FinalizationContext` from a `MultiParentCasperImpl`. Single
/// source of truth for the context clone — previously duplicated at
/// `traits::last_finalized_block` and the trigger site in
/// `finalization_runner::run_finalization`. Replaces both literal
/// constructions so adding/renaming a context field is one edit.
pub(crate) fn build_finalization_context<
    T: comm::rust::transport::transport_layer::TransportLayer + Send + Sync,
>(
    this: &crate::rust::engine::multi_parent_casper::types::MultiParentCasperImpl<T>,
) -> FinalizationContext {
    FinalizationContext {
        block_dag_storage: this.block_dag_storage.clone(),
        block_store: this.block_store.clone(),
        deploy_storage: this.deploy_storage.clone(),
        rejected_deploy_buffer: this.rejected_deploy_buffer.clone(),
        pending_cosigner_metadata: this.pending_cosigner_metadata.clone(),
        runtime_manager: this.runtime_manager.clone(),
        event_publisher: this.event_publisher.clone(),
        finalization_in_progress: this.finalization_in_progress.clone(),
        enable_mergeable_channel_gc: this.casper_shard_conf.enable_mergeable_channel_gc,
        // Exact ppm from the shard conf — the source of truth for the DECISION.
        ftt: FtThreshold::from_ppm(this.casper_shard_conf.fault_tolerance_threshold_ppm),
        finalizer_conf: this.casper_shard_conf.finalizer_conf.clone(),
    }
}

#[tracing::instrument(level = "info", skip_all)]
async fn run_finalization_dispatcher(
    ctx: FinalizationContext,
    schedule: Arc<FinalizationSchedule>,
) {
    let _dispatcher_guard = FinalizationDispatcherGuard(schedule.clone());
    loop {
        let Some(covered_through) = schedule.next_coverage() else {
            if schedule.release_dispatcher_or_reacquire() {
                continue;
            }
            return;
        };
        let permit = match schedule.acquire_worker().await {
            Ok(permit) => permit,
            Err(error) => {
                tracing::error!("finalization dispatcher stopped: {}", error);
                schedule.make_retry_ready(covered_through);
                schedule.release_dispatcher_or_reacquire();
                return;
            }
        };
        schedule.mark_launched(covered_through);
        let worker_ctx = ctx.clone();
        let retry_ctx = ctx.clone();
        let worker_schedule = schedule.clone();
        let handle = tokio::spawn(async move {
            tracing::info!(
                target: "f1r3fly.casper",
                covered_through,
                "finalizer-worker-started"
            );
            let result = compute_last_finalized_block(worker_ctx).await;
            drop(permit);
            tracing::info!(
                target: "f1r3fly.casper",
                covered_through,
                "finalizer-worker-finished"
            );
            result
        });
        tokio::spawn(async move {
            let outcome = match handle.await {
                Ok(Ok(_)) => FinalizationWorkerOutcome::Succeeded,
                Ok(Err(error)) => {
                    tracing::warn!(covered_through, "finalizer worker failed: {:?}", error);
                    FinalizationWorkerOutcome::Failed
                }
                Err(error) => {
                    tracing::error!(covered_through, "finalizer worker panicked: {}", error);
                    FinalizationWorkerOutcome::Failed
                }
            };
            let retry_delay =
                settle_finalization_worker(&worker_schedule, covered_through, outcome);
            if let Some(retry_delay) = retry_delay {
                tokio::time::sleep(retry_delay).await;
                if worker_schedule.make_retry_ready(covered_through) {
                    start_finalization_dispatcher(retry_ctx, worker_schedule);
                }
            }
        });
    }
}

fn start_finalization_dispatcher(ctx: FinalizationContext, schedule: Arc<FinalizationSchedule>) {
    if !schedule.try_start_dispatcher() {
        return;
    }
    let restart_ctx = ctx.clone();
    let restart_schedule = schedule.clone();
    let handle = tokio::spawn(run_finalization_dispatcher(ctx, schedule));
    tokio::spawn(async move {
        if let Err(error) = handle.await {
            tracing::error!("finalization dispatcher panicked: {}", error);
            start_finalization_dispatcher(restart_ctx, restart_schedule);
        }
    });
}

fn effect_id(
    revision: u64,
    block_hash: BlockHash,
    kind: FinalizationEffectKind,
) -> FinalizationEffectId {
    FinalizationEffectId {
        revision,
        block_hash: block_hash.into(),
        kind,
    }
}

async fn apply_finalization_effects(
    ctx: &FinalizationContext,
    revision: u64,
    finalized_set: &HashSet<BlockHash>,
) -> Result<(), KvStoreError> {
    ctx.finalization_in_progress
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            current.checked_add(1)
        })
        .map_err(|_| {
            KvStoreError::InvalidArgument(
                "concurrent finalization effect counter exhausted".to_string(),
            )
        })?;
    let _guard = FinalizationGuard(ctx.finalization_in_progress.as_ref());
    let mut blocks = finalized_set
        .iter()
        .map(|block_hash| {
            ctx.block_store.get(block_hash)?.ok_or_else(|| {
                KvStoreError::KeyNotFound(format!(
                    "finalized block {} not present in store",
                    PrettyPrinter::build_string_bytes(block_hash)
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    blocks.sort_by(|left, right| {
        left.body
            .state
            .block_number
            .cmp(&right.body.state.block_number)
            .then_with(|| left.block_hash.cmp(&right.block_hash))
    });

    for block in blocks {
        let deploys = block
            .body
            .deploys
            .iter()
            .map(|processed| processed.deploy.clone())
            .collect::<Vec<_>>();
        let deploy_signatures = deploys
            .iter()
            .map(|deploy| prost::bytes::Bytes::from(deploy.sig.to_vec()))
            .collect::<Vec<_>>();

        let deploy_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::DeployRemoval,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&deploy_effect)?
        {
            ctx.deploy_storage.lock().remove(deploys.clone())?;
            ctx.rejected_deploy_buffer
                .lock()
                .map_err(|error| KvStoreError::LockError(error.to_string()))?
                .remove(deploys)?;
            ctx.block_dag_storage
                .record_finalization_effect(deploy_effect)?;
        }

        let cosigner_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::CosignerRemoval,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&cosigner_effect)?
        {
            let mut sidecar = ctx.pending_cosigner_metadata.lock();
            for signature in deploy_signatures {
                sidecar.remove(&signature);
            }
            drop(sidecar);
            ctx.block_dag_storage
                .record_finalization_effect(cosigner_effect)?;
        }

        let cache_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::RuntimeCacheEviction,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&cache_effect)?
        {
            ctx.runtime_manager
                .remove_block_index_cache(&block.block_hash);
            ctx.block_dag_storage
                .record_finalization_effect(cache_effect)?;
        }

        if !ctx.enable_mergeable_channel_gc {
            tracing::debug!(
                "Mergeable channel GC disabled; retaining mergeable data for finalized block {} (sender={}, seq={})",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                PrettyPrinter::build_string_bytes(&block.sender),
                block.seq_num
            );
        }

        let event_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::FinalizedEvent,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&event_effect)?
        {
            ctx.event_publisher
                .publish(finalised_event(&block))
                .map_err(|error| KvStoreError::IoError(error.to_string()))?;
            ctx.block_dag_storage
                .record_finalization_effect(event_effect)?;
        }

        // Fileio H-1 fix (slice 30c Phase B): LFB-triggered fs_wal
        // snapshot write per newly-finalized block.  Post-merge
        // (2026-08-26) folded into apply_finalization_effects's per-
        // effect idempotency machinery instead of relying on
        // `pending_wal_slices.remove(...)` semantics — a
        // finalization-round retry after partial write finds this
        // effect already recorded and skips.  On cache miss (this
        // validator didn't compute the block; e.g., joining
        // validator that received it from a peer without re-
        // executing), record the receipt anyway so retries don't
        // loop and downstream peer catch-up fills the snapshot gap.
        //
        // Lock-ordering audit (2026-08-26): safe.  This branch
        // acquires `pending_wal_slices.write().await` (tokio
        // RwLock, released before) then `record_finalization_effect`
        // (which briefly grabs FinalizationLedger's `append_lock`
        // — a parking_lot::Mutex held only across a single store
        // put).  Reverse-order path does not exist: the only other
        // pending_wal_slices writer is `runtime.rs::play_deploys_for_state`
        // which never touches `append_lock` (block-processing vs.
        // finalization subsystems are disjoint).  The atomic
        // wrapper `record_directly_finalized_certified_atomic`
        // releases its own `global_lock` before invoking this
        // callback (see block_dag_key_value_storage.rs:2261-2303),
        // so no shared lock is held across the awaits below.
        let snapshot_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::WalSnapshotWrite,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&snapshot_effect)?
        {
            let writer_opt = ctx.runtime_manager.fs_snapshot_writer.read().await.clone();
            if let Some(writer) = writer_opt {
                let post_state_hash = block.body.state.post_state_hash.to_vec();
                let bn = block.body.state.block_number;
                let cache_entry = {
                    let mut slices = ctx.runtime_manager.pending_wal_slices.write().await;
                    slices.remove(&post_state_hash)
                };
                match cache_entry {
                    Some((_cached_bn, slice)) => {
                        let block_hash_pretty =
                            PrettyPrinter::build_string_bytes(&block.block_hash);
                        let writer_clone = writer.clone();
                        let snapshot_result = tokio::task::spawn_blocking(move || {
                            writer_clone.maybe_write(bn, &slice)
                        })
                        .await;
                        match snapshot_result {
                            Ok(Ok(_)) => {}
                            Ok(Err(error)) => tracing::warn!(
                                target: "f1r3fly.casper.fs_wal",
                                block_number = bn,
                                block_hash = %block_hash_pretty,
                                error = %error,
                                "LFB-triggered snapshot write failed"
                            ),
                            Err(je) => tracing::warn!(
                                target: "f1r3fly.casper.fs_wal",
                                block_number = bn,
                                block_hash = %block_hash_pretty,
                                error = %je,
                                "LFB-triggered snapshot spawn_blocking failed"
                            ),
                        }
                    }
                    None => {
                        tracing::debug!(
                            target: "f1r3fly.casper.fs_wal",
                            block_number = bn,
                            block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
                            "no cached WAL slice for finalized block \
                             (this validator did not compute it; \
                             peer catch-up will cover snapshot needs)"
                        );
                    }
                }
                // Evict stale entries whose block_number is <= this
                // finalized block's — either now-finalized (handled
                // above) or orphaned (never will be).
                let mut slices = ctx.runtime_manager.pending_wal_slices.write().await;
                slices.retain(|_, (cached_bn, _)| *cached_bn > bn);
            }
            // Record the receipt regardless of writer_opt / cache
            // presence: on retry we want to skip this effect
            // regardless of whether it did any real work.
            ctx.block_dag_storage
                .record_finalization_effect(snapshot_effect)?;
        }
    }
    ctx.block_dag_storage
        .record_finalization_round_effects_completed(revision)?;
    Ok(())
}

async fn reconcile_finalization_effects(ctx: &FinalizationContext) -> Result<(), KvStoreError> {
    ctx.block_dag_storage.reconcile_finalization_projection()?;
    ctx.block_dag_storage
        .reconcile_finalization_effect_compaction()?;
    for FinalizationRecord {
        revision,
        finalized,
        ..
    } in ctx
        .block_dag_storage
        .pending_finalization_effect_records()?
    {
        let finalized = finalized.into_iter().map(|hash| hash.0).collect();
        apply_finalization_effects(ctx, revision, &finalized).await?;
    }
    Ok(())
}

pub(crate) async fn compute_last_finalized_block(
    ctx: FinalizationContext,
) -> Result<BlockMessage, CasperError> {
    loop {
        match compute_last_finalized_block_once(ctx.clone()).await {
            Err(CasperError::KvStoreError(KvStoreError::StaleFinalization { .. })) => {
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

async fn compute_last_finalized_block_once(
    ctx: FinalizationContext,
) -> Result<BlockMessage, CasperError> {
    reconcile_finalization_effects(&ctx).await?;
    let evaluation_base = ctx.block_dag_storage.capture_finalization_base()?;
    let effect_ctx = ctx.clone();
    let FinalizationContext {
        block_store,
        ftt,
        finalizer_conf,
        ..
    } = ctx;
    let finalizer_conf = &finalizer_conf;
    let lfb_lookup_started = std::time::Instant::now();
    let dag = evaluation_base.dag;
    let last_finalized_block_hash = evaluation_base.head.block_hash.0.clone();
    let last_finalized_block_height = evaluation_base.head.block_number;
    let evaluation_head = evaluation_base.head;
    let certificate_context =
        crate::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
            &dag,
            last_finalized_block_hash.clone(),
        )?;
    let witness_inputs = FinalizationWitnessInputs {
        fault_tolerance_numerator: ftt.num,
        fault_tolerance_denominator: ftt.den,
        authority_context_digest: BlockHashSerde(certificate_context.digest().clone()),
        latest_messages: certificate_context
            .vote_projection()
            .exact_latest_messages()
            .iter()
            .map(|(validator, block_hash)| {
                (
                    ValidatorSerde(validator.clone()),
                    BlockHashSerde(block_hash.clone()),
                )
            })
            .collect(),
    };
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let effect_ctx = effect_ctx.clone();
        let evaluation_head = evaluation_head.clone();
        let witness_inputs = witness_inputs.clone();
        async move {
            let effect_started = std::time::Instant::now();
            let callback_ctx = effect_ctx.clone();
            effect_ctx
                .block_dag_storage
                .record_directly_finalized_certified_atomic(
                    &evaluation_head,
                    new_lfb,
                    ft_value,
                    witness_inputs,
                    move |revision, finalized_set| {
                        let callback_ctx = callback_ctx.clone();
                        let finalized_set = finalized_set.clone();
                        async move {
                            // Post-cost-accounted-rho-merge polish
                            // (2026-08-26): the fs_wal snapshot logic
                            // (H-1 fix, slice 30c Phase B) is now folded
                            // into `apply_finalization_effects` as a
                            // fifth per-effect step (WalSnapshotWrite).
                            // The atomic callback thus reduces to a
                            // single call — same shape as any other
                            // caller of `apply_finalization_effects`.
                            apply_finalization_effects(&callback_ctx, revision, &finalized_set)
                                .await
                        }
                    },
                )
                .await?;
            tracing::debug!(
                target: "f1r3fly.casper.finalizer.effect.timing",
                "record_directly_finalized_total_ms={}",
                effect_started.elapsed().as_millis()
            );
            Ok(())
        }
    };

    // Run finalizer
    let finalizer_started = std::time::Instant::now();
    let new_finalized_hash_opt = Finalizer::run_with_context(
        &dag,
        ftt,
        &last_finalized_block_hash,
        last_finalized_block_height,
        &certificate_context,
        new_lfb_found_effect,
        finalizer_conf,
    )
    .await?;
    let finalizer_ms = finalizer_started.elapsed().as_millis();
    let new_lfb_found = new_finalized_hash_opt.is_some();

    // Get the final LFB hash (either new or existing)
    let final_lfb_hash = new_finalized_hash_opt
        .map(|(hash, _ft)| hash)
        .unwrap_or(last_finalized_block_hash);

    // Return the finalized block
    let read_started = std::time::Instant::now();
    // P2-7: surface missing LFB as a typed error instead of panicking.
    let block_message = block_store.get(&final_lfb_hash)?.ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "final last-finalized block {} not present in store",
            PrettyPrinter::build_string_bytes(&final_lfb_hash)
        ))
    })?;
    tracing::debug!(
        target: "f1r3fly.casper.lfb.timing",
        "last_finalized_block timing: finalizer_ms={}, read_block_ms={}, total_ms={}, new_lfb_found={}",
        finalizer_ms,
        read_started.elapsed().as_millis(),
        lfb_lookup_started.elapsed().as_millis(),
        new_lfb_found
    );
    Ok(block_message)
}

/// Trigger a finalization run if the new block crosses the finalization
/// rate boundary. Free function (matches the rest of `engine/multi_parent_casper/*`
/// module idiom — every other operation in this sub-module is a free
/// function taking `this: &MultiParentCasperImpl<T>`).
///
/// Promoted from inherent `impl` block during the engine::multi_parent_casper
/// refactor (Phase-3+ cleanup). Caller:
/// `block_admission::admit_handle_valid_block`.
pub(crate) async fn update_last_finalized_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    new_block: &BlockMessage,
) -> Result<(), CasperError> {
    if finalization_due(
        this.casper_shard_conf.finalization_rate,
        new_block.body.state.block_number,
        this.recovery_sync_active
            .load(std::sync::atomic::Ordering::Acquire),
    ) {
        request_finalization(this)?;
    }
    Ok(())
}

fn finalization_due(finalization_rate: i32, block_number: i64, recovery_active: bool) -> bool {
    recovery_active || (finalization_rate > 0 && block_number % i64::from(finalization_rate) == 0)
}

pub(crate) fn request_finalization<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<(), CasperError> {
    let ticket = this.finalization_schedule.request().ok_or_else(|| {
        CasperError::RuntimeError("finalization request sequence exhausted".to_string())
    })?;
    let ctx = build_finalization_context(this);
    start_finalization_dispatcher(ctx, this.finalization_schedule.clone());
    tracing::debug!(ticket, "published finalization request");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::*;

    #[test]
    fn failed_worker_is_retryable_instead_of_completed() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        assert_eq!(
            settle_finalization_worker(&schedule, 1, FinalizationWorkerOutcome::Failed),
            Some(Duration::from_millis(25))
        );
        assert!(!schedule.is_quiescent());
        assert!(schedule.make_retry_ready(1));
        assert_eq!(schedule.next_coverage(), Some(1));
    }

    #[test]
    fn successful_worker_completes_its_coverage() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        assert_eq!(
            settle_finalization_worker(&schedule, 1, FinalizationWorkerOutcome::Succeeded),
            None
        );
        assert!(schedule.is_quiescent());
    }

    #[test]
    fn recovery_requests_finalization_even_when_periodic_finalization_is_disabled() {
        assert!(finalization_due(0, 7, true));
        assert!(finalization_due(-1, 7, true));
    }

    proptest! {
        #[test]
        fn recovery_finalization_is_independent_of_height_and_rate(
            rate in i32::MIN..=i32::MAX,
            height in i64::MIN..=i64::MAX,
        ) {
            prop_assert!(finalization_due(rate, height, true));
        }
    }
}
