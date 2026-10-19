//! Finalization runner — the one writer of the LFB.
//!
//! The LFB DECISION does not live here: it is `floor::floor_of_view` — the
//! one finality clock, shared with every block operation's floor
//! derivation. This module hosts the runner around it: pacing
//! (`finalization_rate`), the queued-run loop with its timeout backstop, the
//! finalization effects (`record_directly_finalized`, event publication)
//! applied when the clock advances, and the deploy-lifecycle work that
//! follows each cycle.
//!
//! Every cycle runs under `MultiParentCasperImpl::finalizer_cycle`, so the
//! LFB has exactly one writer. A cycle starts from two places:
//!   * `block_admission::admit_handle_valid_block` →
//!     `update_last_finalized_block`, which spawns `run_queued_finalizer`
//!     when the lock is free and queues a rerun when it is held;
//!   * `MultiParentCasperImpl::finalize`, which waits for the lock.
//!
//! Reads of the LFB never compute it: `MultiParentCasper::last_finalized_block`
//! returns the persisted one.
//!
//! ── DO NOT re-add a finalization-time rejected-deploy-buffer purge ──────────
//!
//! There is deliberately NO `purge_finalized_deploys_from_buffer` here, and no
//! `rejected_deploy_buffer` handle in `FinalizationContext`. This looks like the
//! well-known "the casper_engine split dropped the DL-1 finalization purge"
//! regression. It is NOT. It was re-derived and MEASURED during the 2026-07-15
//! dev merge, and the purge is actively harmful against the current recovery
//! design:
//!
//!   run `casper::mod batch2::map_cell_convergence_spec::three_writers_converge_under_load`
//!     - purge present  ⇒ FAILS, deterministically, same keys every run:
//!                        "MISSING 2 of 9 keys: [(\"v1_0\", 1), (\"v2_0\", 2)]"
//!     - purge absent   ⇒ PASSES (308s)
//!
//! Why: `v1_0`/`v2_0` are the round-0 keep-one LOSERS. Their writes are supposed
//! to be re-proposed out of the per-node rejected-deploy buffer (record-driven
//! recovery). A finalization-time purge of `block.body.rejected_deploys` evicts
//! exactly those entries, so the losers can never be recovered and their writes
//! are lost permanently — silently, since the merge itself is still "valid".
//!
//! The double-apply hazard the purge originally guarded against is handled
//! ELSEWHERE now: `block_creator` drops already-canonical sigs at ADMISSION
//! (`remove_by_sig` + `canonical_won_sigs` + the `rejected_in_scope` exemption),
//! which is a strictly better place for it — a deploy stops being re-proposable
//! when it lands canonically, not when some later block finalizes.
//!
//! If you are here because a static diff told you a fix went missing: it didn't.
//! Re-run the test above before restoring anything.
//!
//! Deploy-STORAGE release keys on the deploy-lifecycle register's
//! write-once terminal verdicts, never on the finality marker itself: after
//! each cycle, `settle_after_cycle` writes every verdict the persisted LFB
//! decides and removes the pool copy of every terminal sig.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KvStoreError;

// Phase 7 (C-3): import the struct from its canonical sibling module
// instead of via the legacy shim — the previous import formed a circular
// path `engine::multi_parent_casper → engine::multi_parent_casper → engine::multi_parent_casper::types`.
use super::events::finalised_event;
use super::types::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Consecutive containment refusals of strictly rising derived floors
/// against one pinned LFB before the hold is declared a finality
/// divergence. Deliberately a constant, not configuration (the floor
/// walk-depth report sets the precedent): there is no per-topology trade
/// here — no operator wants detection later, and earlier than the
/// transient-recovery bound is false positives for every deployment
/// equally. The one legitimate transient — a rejected-then-recovered
/// deploy whose re-homed carrier has not finalized yet — resolves within a
/// few floor advances or not at all.
const DIVERGENCE_ESCALATION_REFUSALS: u64 = 10;

/// Detects a finality DIVERGENCE from the containment gate's refusals.
///
/// The gate itself is the last line of finality safety: it refuses to
/// designate a state that is missing effects settled under the current
/// LFB. This monitor makes a persistent refusal LOUD. The discriminator is
/// structural, not temporal: refusals whose derived floors keep RISING
/// against one pinned LFB mean the shard is finalizing without this node
/// (in CI instance ucc-i6 a wedged node logged 399 such refusals, derived
/// rising h294 -> h472, over 14.5 silent minutes). A class-A stall never
/// trips it — its derivations do not rise — and a transient re-homing
/// clears it by advancing. Escalation is one ERROR plus the
/// `finality.divergence.detected` counter; proposing is deliberately NOT
/// halted (a wedged node's proposals follow the estimator, not its LFB,
/// and remained valid throughout the observed incident — halting would
/// shrink the honest proposer set exactly when certification needs it).
pub struct DivergenceMonitor {
    state: std::sync::Mutex<HoldStreak>,
    diverged: AtomicBool,
}

#[derive(Default)]
struct HoldStreak {
    pinned_lfb: Option<BlockHash>,
    refusals: u64,
    first_derived_number: i64,
    latest_derived_number: i64,
}

impl Default for DivergenceMonitor {
    fn default() -> Self {
        Self {
            state: std::sync::Mutex::new(HoldStreak::default()),
            diverged: AtomicBool::new(false),
        }
    }
}

impl DivergenceMonitor {
    /// Lock the streak state, recovering from poison: the monitor is
    /// telemetry — a panic elsewhere while the lock was held must never
    /// escalate into a finalizer panic. The recovered state is at worst a
    /// stale streak, which the next advance or hold rewrites.
    fn streak(&self) -> std::sync::MutexGuard<'_, HoldStreak> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A containing floor was adopted: any streak is over. If the monitor
    /// had escalated, the divergence resolved itself (a re-homed carrier
    /// finalized and containment passed) — say so at INFO.
    pub fn on_advance(&self) {
        *self.streak() = HoldStreak::default();
        if self.diverged.swap(false, Ordering::SeqCst) {
            tracing::info!(
                target: "f1r3fly.finalizer",
                "containment hold cleared: a containing floor was adopted; \
                 the finality divergence resolved"
            );
        }
    }

    /// The gate refused a strictly higher derived floor.
    pub fn on_containment_hold(&self, current_lfb: &BlockHash, derived_number: i64) {
        let escalate = {
            let mut streak = self.streak();
            if streak.pinned_lfb.as_ref() != Some(current_lfb) {
                *streak = HoldStreak {
                    pinned_lfb: Some(current_lfb.clone()),
                    refusals: 0,
                    first_derived_number: derived_number,
                    latest_derived_number: derived_number,
                };
            }
            streak.refusals += 1;
            streak.latest_derived_number = streak.latest_derived_number.max(derived_number);
            streak.refusals >= DIVERGENCE_ESCALATION_REFUSALS
                && streak.latest_derived_number > streak.first_derived_number
        };
        if escalate && !self.diverged.swap(true, Ordering::SeqCst) {
            metrics::counter!(
                crate::rust::metrics_constants::FINALITY_DIVERGENCE_DETECTED_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
            tracing::error!(
                target: "f1r3fly.finalizer",
                pinned_lfb = %PrettyPrinter::build_string_bytes(current_lfb),
                latest_refused_floor = derived_number,
                "FINALITY DIVERGENCE: the shard keeps finalizing floors whose \
                 state is missing effects settled under this node's LFB. This \
                 node's finalized read surface is frozen and will not recover \
                 without operator action (resync from a peer snapshot); its \
                 block production continues and remains valid"
            );
        }
    }

    /// True while an escalated divergence stands unresolved.
    pub fn diverged(&self) -> bool { self.diverged.load(Ordering::SeqCst) }
}

/// RAII guard that ensures the finalization flag is reset on drop.
/// This prevents the flag from being stuck in `true` state if the async block
/// panics or returns early via `?` operator.
struct FinalizationGuard<'a>(&'a AtomicBool);

impl Drop for FinalizationGuard<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
}

/// Phase 8 (PO-3): bundles the 9 service handles + tuning flags that
/// `compute_last_finalized_block` and `run_queued_finalizer` need. Avoids
/// the previous 9-/11-arg signatures (silenced by
/// `#[allow(clippy::too_many_arguments)]`). The struct is `Clone` because
/// the finalization-effect closure captures by move into a
/// `FnMut + Send + Sync`.
#[derive(Clone)]
pub(crate) struct FinalizationContext {
    pub(crate) observer: Option<crate::rust::soak_observer::ObserverBinding>,
    pub(crate) block_dag_storage: BlockDagKeyValueStorage,
    pub(crate) block_store: KeyValueBlockStore,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicBool>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) ftt: crate::rust::safety::clique_oracle::FtThreshold,
    pub(crate) divergence_monitor: Arc<DivergenceMonitor>,
    pub(crate) deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    pub(crate) deploy_lifespan: i64,
    pub(crate) citability_horizon: Option<i64>,
}

/// Build a `FinalizationContext` from a `MultiParentCasperImpl`. Single
/// source of truth for the field-by-field clone — previously duplicated at
/// `traits::last_finalized_block` and the trigger site in
/// `finalization_runner::run_finalization`. Replaces both literal
/// constructions so adding/renaming a context field is one edit.
pub(crate) fn build_finalization_context<
    T: comm::rust::transport::transport_layer::TransportLayer + Send + Sync,
>(
    this: &crate::rust::engine::multi_parent_casper::types::MultiParentCasperImpl<T>,
) -> FinalizationContext {
    FinalizationContext {
        observer: this.observer.get().cloned(),
        block_dag_storage: this.block_dag_storage.clone(),
        block_store: this.block_store.clone(),
        runtime_manager: this.runtime_manager.clone(),
        event_publisher: this.event_publisher.clone(),
        finalization_in_progress: this.finalization_in_progress.clone(),
        enable_mergeable_channel_gc: this.casper_shard_conf.enable_mergeable_channel_gc,
        // Exact ppm from the shard conf — the source of truth for the DECISION.
        ftt: crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
            this.casper_shard_conf.fault_tolerance_threshold_ppm,
        ),
        divergence_monitor: this.divergence_monitor.clone(),
        deploy_storage: this.deploy_storage.clone(),
        deploy_lifespan: this.casper_shard_conf.deploy_lifespan,
        citability_horizon: crate::rust::finality::deploy_lifecycle::citability_horizon(
            this.casper_shard_conf.max_parent_depth,
        ),
    }
}

/// Runs finalizer cycles until no rerun is queued. `held` is the
/// `finalizer_cycle` lock: holding it is what makes this the one LFB writer.
#[tracing::instrument(level = "info", skip_all)]
pub(crate) async fn run_queued_finalizer(
    ctx: FinalizationContext,
    finalizer_cycle: Arc<tokio::sync::Mutex<()>>,
    held: tokio::sync::OwnedMutexGuard<()>,
    finalizer_task_queued: Arc<AtomicBool>,
) {
    tracing::info!(target: "f1r3fly.casper", "finalizer-run-started");

    // Backstop only: floor-of-view rides the persisted floor/frontier
    // caches, so a cycle exceeding this is a stall to surface, not pace.
    let finalizer_blocking_timeout = std::time::Duration::from_secs(15);
    let ctx = &ctx;
    let queued: &AtomicBool = &finalizer_task_queued;
    hold_finalizer_cycles(&finalizer_cycle, held, queued, move || {
        drive_finalizer_runs(
            finalizer_blocking_timeout,
            queued,
            move || compute_last_finalized_block(ctx.clone()),
            move |advanced| settle_after_cycle(ctx.clone(), advanced),
        )
    })
    .await;
    tracing::info!(target: "f1r3fly.casper", "finalizer-run-finished");
}

/// Runs `cycles` with the lock held, then releases it. A trigger that lands
/// after the cycles' last check of `queued` but before the release finds the
/// lock held and only sets the flag, so the flag is re-checked here once the
/// lock is free: taken up if the lock can be re-acquired, and otherwise left
/// to the cycle that now holds it.
pub(crate) async fn hold_finalizer_cycles<R, RFut>(
    lock: &Arc<tokio::sync::Mutex<()>>,
    mut held: tokio::sync::OwnedMutexGuard<()>,
    queued: &AtomicBool,
    mut cycles: R,
) where
    R: FnMut() -> RFut,
    RFut: std::future::Future<Output = ()>,
{
    loop {
        cycles().await;
        drop(held);
        if !queued.load(Ordering::SeqCst) {
            return;
        }
        match lock.clone().try_lock_owned() {
            Ok(next) => held = next,
            Err(_) => return,
        }
    }
}

/// The queued-run loop of `run_queued_finalizer`, generic over its two steps
/// so its cycle accounting is testable without a casper instance. `compute`
/// runs under the timeout and reports whether the LFB advanced; `after` runs
/// untimed once each cycle with that answer, so a slow step after the
/// advance can never be abandoned midway and overlap the next cycle.
pub(crate) async fn drive_finalizer_runs<E, F, Fut, A, AFut>(
    timeout: std::time::Duration,
    finalizer_task_queued: &AtomicBool,
    mut compute: F,
    mut after: A,
) where
    E: std::fmt::Debug,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, E>>,
    A: FnMut(bool) -> AFut,
    AFut: std::future::Future<Output = ()>,
{
    use crate::rust::metrics_constants::{
        CASPER_METRICS_SOURCE, FINALIZER_RUN_FAILURES_METRIC, FINALIZER_RUN_RERUNS_METRIC,
        FINALIZER_RUN_TIMEOUTS_METRIC, FINALIZER_RUN_TIME_METRIC,
    };
    loop {
        let cycle_started = std::time::Instant::now();
        let outcome = tokio::time::timeout(timeout, compute()).await;
        metrics::histogram!(FINALIZER_RUN_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(cycle_started.elapsed().as_secs_f64());
        let advanced = match outcome {
            Ok(Ok(advanced)) => advanced,
            Ok(Err(err)) => {
                metrics::counter!(FINALIZER_RUN_FAILURES_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .increment(1);
                tracing::warn!("finalizer-run failed: {:?}", err);
                false
            }
            Err(_) => {
                metrics::counter!(FINALIZER_RUN_TIMEOUTS_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .increment(1);
                tracing::warn!(
                    "finalizer-run timed out after {:?}; skipping this cycle to avoid blocking propose",
                    timeout
                );
                false
            }
        };
        after(advanced).await;

        if finalizer_task_queued.swap(false, Ordering::SeqCst) {
            metrics::counter!(FINALIZER_RUN_RERUNS_METRIC, "source" => CASPER_METRICS_SOURCE)
                .increment(1);
            tracing::debug!("finalizer-run-queued; continuing finalizer loop");
            continue;
        }
        return;
    }
}

/// The deploy-lifecycle work after one cycle, on the blocking pool. The
/// settle and the release run every cycle, not only on an advance: both are
/// idempotent, and an advance persisted by a cycle that then failed, timed
/// out or crashed is otherwise never settled until the next advance. The
/// carrier prune is retention only, so it waits for an advance.
async fn settle_after_cycle(ctx: FinalizationContext, advanced: bool) {
    match tokio::task::spawn_blocking(move || settle_blocking(&ctx, advanced)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!(
            target: "f1r3fly.casper.lifecycle",
            error = %e,
            "deploy-lifecycle settle failed; the next cycle retries"
        ),
        Err(join_err) => tracing::error!(
            target: "f1r3fly.casper.lifecycle",
            "deploy-lifecycle settle task terminated abnormally: {}",
            join_err
        ),
    }
}

fn settle_blocking(ctx: &FinalizationContext, advanced: bool) -> Result<(), CasperError> {
    use crate::rust::finality::deploy_lifecycle::{record_observe_timing, release_settled, settle};

    let dag = ctx.block_dag_storage.get_representation()?;
    let settle_started = std::time::Instant::now();
    let settled = settle(
        &dag,
        &ctx.block_store,
        ctx.deploy_lifespan,
        ctx.citability_horizon,
    );
    let settle_ms = settle_started.elapsed().as_millis();
    let prune_ms = if advanced {
        let prune_started = std::time::Instant::now();
        let lfb_height = dag.block_number_unsafe(&dag.last_finalized_block())?;
        if let Err(e) = dag.prune_carriers_below(lfb_height - ctx.deploy_lifespan) {
            tracing::warn!("carrier-index prune failed (retention only): {}", e);
        }
        Some(prune_started.elapsed().as_millis())
    } else {
        None
    };
    record_observe_timing(settle_ms, prune_ms);
    release_settled(&dag, &ctx.deploy_storage)?;
    settled.map(|_| ())
}

/// A finalization trigger arrived while a run is in progress: queue one
/// follow-up run. Repeated triggers coalesce into that one rerun.
pub(crate) fn note_finalizer_busy(finalizer_task_queued: &AtomicBool) {
    metrics::counter!(
        crate::rust::metrics_constants::FINALIZER_RUN_QUEUED_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .increment(1);
    if !finalizer_task_queued.swap(true, Ordering::SeqCst) {
        tracing::debug!("Finalizer already running; queued follow-up finalization run");
    }
}

/// One LFB decision and, on an advance, its finalization effects. Returns
/// whether the LFB advanced.
pub(crate) async fn compute_last_finalized_block(
    ctx: FinalizationContext,
) -> Result<bool, CasperError> {
    let FinalizationContext {
        observer,
        block_dag_storage,
        block_store,
        runtime_manager,
        event_publisher,
        finalization_in_progress,
        enable_mergeable_channel_gc,
        ftt,
        divergence_monitor,
        ..
    } = ctx;
    let observation_operation = observer.as_ref().and_then(|observer| observer.operation());
    let lfb_lookup_started = std::time::Instant::now();
    // Get current LFB hash and height
    let dag = block_dag_storage.get_representation()?;
    let last_finalized_block_hash = dag.last_finalized_block();
    let last_finalized_block_height = dag.lookup_unsafe(&last_finalized_block_hash)?.block_number;

    // Keep effect closure FnMut-compatible by cloning captured state on each invocation.
    let block_dag_storage_for_effect = block_dag_storage.clone();
    let block_store_for_effect = block_store.clone();
    let runtime_manager_for_effect = runtime_manager.clone();
    let event_publisher_for_effect = event_publisher.clone();
    let finalization_in_progress_for_effect = finalization_in_progress.clone();

    // Create simple finalization effect closure
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let block_dag_storage = block_dag_storage_for_effect.clone();
        let block_store = block_store_for_effect.clone();
        let runtime_manager = runtime_manager_for_effect.clone();
        let event_publisher = event_publisher_for_effect.clone();
        let finalization_in_progress = finalization_in_progress_for_effect.clone();
        async move {
            let effect_started = std::time::Instant::now();
            let directly_finalized_hash = new_lfb.clone();
            block_dag_storage
                .record_directly_finalized(new_lfb.clone(), ft_value, |finalized_set: &HashSet<BlockHash>| {
                    let finalized_set = finalized_set.clone();
                    let directly_finalized_hash = directly_finalized_hash.clone();
                    let block_store = block_store.clone();
                    let runtime_manager = runtime_manager.clone();
                    let event_publisher = event_publisher.clone();
                    let finalization_in_progress = finalization_in_progress.clone();
                    Box::pin(async move {
                        let process_finalized_started = std::time::Instant::now();
                        // Use RAII guard to ensure flag is reset even if we return early or panic
                        finalization_in_progress.store(true, Ordering::SeqCst);
                        let _guard = FinalizationGuard(finalization_in_progress.as_ref());
                        tracing::debug!("Finalization started for {} blocks", finalized_set.len());

                        // process_finalized
                        for block_hash in &finalized_set {
                            // P2-7: a finalized hash should always be in the
                            // store, but a panic here would crash the
                            // finalization runner. Surface as a typed error.
                            let block = block_store.get(block_hash)?.ok_or_else(|| {
                                KvStoreError::KeyNotFound(format!(
                                    "finalized block {} not present in store",
                                    PrettyPrinter::build_string_bytes(block_hash)
                                ))
                            })?;

                            // Remove block index from cache
                            runtime_manager.remove_block_index_cache(block_hash);

                            // Keep mergeable data on finalization to preserve deterministic
                            // parent-state reconstruction. Safe deletion is handled only by
                            // reachability-based background GC when enabled.
                            if !enable_mergeable_channel_gc {
                                tracing::debug!(
                                    "Mergeable channel GC disabled; retaining mergeable data for finalized block {} (sender={}, seq={})",
                                    PrettyPrinter::build_string_bytes(&block.block_hash),
                                    PrettyPrinter::build_string_bytes(&block.sender),
                                    block.seq_num
                                );
                            }

                            for processed in &block.body.deploys {
                                tracing::info!(
                                    target: "f1r3fly.casper.deploy_lifecycle",
                                    event = "finalized_inclusion",
                                    deploy_sig = %hex::encode(&processed.deploy.sig),
                                    block_hash = %hex::encode(&block.block_hash),
                                    block_number = block.body.state.block_number,
                                    sender = %hex::encode(&block.sender),
                                    failed = processed.is_failed,
                                    directly_finalized = block_hash == &directly_finalized_hash,
                                    "deploy lifecycle"
                                );
                            }
                            for rejected in &block.body.rejected_deploys {
                                tracing::info!(
                                    target: "f1r3fly.casper.deploy_lifecycle",
                                    event = "finalized_rejection",
                                    deploy_sig = %hex::encode(&rejected.sig),
                                    block_hash = %hex::encode(&block.block_hash),
                                    block_number = block.body.state.block_number,
                                    carrier = %hex::encode(&rejected.carrier),
                                    duplicate = rejected.duplicate,
                                    directly_finalized = block_hash == &directly_finalized_hash,
                                    "deploy lifecycle"
                                );
                            }

                            // Publish BlockFinalised event for each newly finalized block
                            event_publisher
                                .publish(finalised_event(&block))
                                .map_err(|e| KvStoreError::IoError(e.to_string()))?;
                        }

                        // Guard will reset finalization_in_progress flag on drop
                        tracing::debug!("Finalization completed");
                        tracing::debug!(
                            target: "f1r3fly.casper.finalizer.effect.timing",
                            "Finalization effect timing: finalized_blocks={}, process_finalized_ms={}",
                            finalized_set.len(),
                            process_finalized_started.elapsed().as_millis()
                        );

                        Ok(())
                    })
                })
                .await?;
            tracing::debug!(
                target: "f1r3fly.casper.finalizer.effect.timing",
                "record_directly_finalized_total_ms={}",
                effect_started.elapsed().as_millis()
            );
            Ok(())
        }
    };

    // ONE finality clock: the LFB is the floor of the live view — the same
    // derivation, candidate soundness, and exact `>= θ` decision every block
    // operation uses (deliberately unifying away the old Finalizer's strict
    // `> θ` LFB decision: an LFB lagging the floors at the exact threshold
    // boundary would be a second clock). The derived floor advances the LFB
    // only when it CAPTURES the current one — the same state-monotonicity
    // predicate floor candidacy runs — so the read surface can never
    // designate a state missing settled content.
    let finalizer_started = std::time::Instant::now();
    let current = crate::rust::finality::floor::Floor {
        hash: last_finalized_block_hash.clone(),
        block_number: last_finalized_block_height,
    };
    let derivation =
        crate::rust::finality::floor::floor_of_view(&dag, &block_store, &current, ftt).await;
    if let Some(observer) = &observer {
        use crate::rust::finality::floor::FloorOfView;
        use crate::rust::soak_observer::{EventKind, FloorOutcome};
        let (outcome, hash, height) = match &derivation {
            Ok(FloorOfView::Advance(floor)) => (
                FloorOutcome::Advance,
                Some(floor.hash.as_ref()),
                Some(floor.block_number),
            ),
            Ok(FloorOfView::ContainmentHold { derived }) => (
                FloorOutcome::ContainmentHold,
                Some(derived.hash.as_ref()),
                Some(derived.block_number),
            ),
            Ok(FloorOfView::AbsenceHold { missing }) => {
                (FloorOutcome::AbsenceHold, Some(missing.as_ref()), None)
            }
            Ok(FloorOfView::NoAdvance) => (FloorOutcome::NoAdvance, None, None),
            Ok(FloorOfView::IncompatibilityHold { .. }) => {
                (FloorOutcome::IncompatibilityHold, None, None)
            }
            Err(_) => (FloorOutcome::Failed, None, None),
        };
        observer.emit(
            EventKind::LiveDerivation,
            observation_operation,
            Some(outcome),
            hash,
            height,
            Some(derivation.is_ok()),
        );
    }
    let new_lfb_opt = match derivation? {
        // `on_advance` is NOT called here: the alarm tracks the
        // PERSISTED LFB, so the streak clears only after the adoption
        // effect below succeeds. Clearing on derivation alone would let
        // repeated effect failures suppress the divergence alarm.
        crate::rust::finality::floor::FloorOfView::Advance(floor) => Some(floor),
        crate::rust::finality::floor::FloorOfView::NoAdvance => None,
        crate::rust::finality::floor::FloorOfView::ContainmentHold { derived } => {
            divergence_monitor
                .on_containment_hold(&last_finalized_block_hash, derived.block_number);
            None
        }
        crate::rust::finality::floor::FloorOfView::AbsenceHold { missing } => {
            tracing::debug!(
                missing = %PrettyPrinter::build_string_bytes(&missing),
                "finalizer holds this cycle: the floor walk needs a block \
                 this node does not hold. Nothing requests it here, so the \
                 hold lasts until catch-up fetches it for another reason"
            );
            None
        }
        crate::rust::finality::floor::FloorOfView::IncompatibilityHold { detail } => {
            tracing::debug!(
                detail,
                "finalizer holds this cycle: incompatible majority-agreement \
                     candidates under a negative fault-tolerance threshold; the \
                     next merge spanning both branches reconciles them"
            );
            None
        }
    };

    let new_lfb_found = new_lfb_opt.is_some();
    if let Some(new_lfb) = new_lfb_opt {
        let ft_value =
            crate::rust::safety::clique_oracle::CliqueOracle::normalized_fault_tolerance(
                &new_lfb.hash,
                &dag,
            )
            .await
            .map_err(CasperError::from)?;
        // `floor_of_view` only ever returns an adoption that CAPTURES the
        // current LFB, so `extends_previous_lfb` is true by construction —
        // emitted anyway for parity with soak dashboards that alarm on it.
        tracing::info!(
            target: "f1r3fly.casper.finalization_lifecycle",
            event = "candidate_finalized",
            candidate_hash = %hex::encode(&new_lfb.hash),
            candidate_height = new_lfb.block_number,
            previous_lfb_hash = %hex::encode(&last_finalized_block_hash),
            previous_lfb_height = last_finalized_block_height,
            extends_previous_lfb = true,
            fault_tolerance = ft_value,
            "finalization lifecycle"
        );
        if let Some(observer) = &observer {
            observer.emit(
                crate::rust::soak_observer::EventKind::EffectAttempt,
                observation_operation,
                None,
                Some(&new_lfb.hash),
                Some(new_lfb.block_number),
                None,
            );
        }
        let effect_result = new_lfb_found_effect((new_lfb.hash.clone(), ft_value)).await;
        if let Some(observer) = &observer {
            observer.emit(
                crate::rust::soak_observer::EventKind::EffectReturn,
                observation_operation,
                None,
                Some(&new_lfb.hash),
                Some(new_lfb.block_number),
                Some(effect_result.is_ok()),
            );
        }
        effect_result.map_err(CasperError::KvStoreError)?;
        divergence_monitor.on_advance();
    }
    tracing::debug!(
        target: "f1r3fly.casper.lfb.timing",
        "last_finalized_block timing: finalizer_ms={}, total_ms={}, new_lfb_found={}",
        finalizer_started.elapsed().as_millis(),
        lfb_lookup_started.elapsed().as_millis(),
        new_lfb_found
    );
    Ok(new_lfb_found)
}

/// The persisted LFB's block.
pub(crate) fn read_last_finalized_block(
    block_dag_storage: &BlockDagKeyValueStorage,
    block_store: &KeyValueBlockStore,
) -> Result<BlockMessage, CasperError> {
    let lfb_hash = block_dag_storage
        .get_representation()?
        .last_finalized_block();
    block_store.get(&lfb_hash)?.ok_or_else(|| {
        CasperError::RuntimeError(format!(
            "last finalized block {} not present in store",
            PrettyPrinter::build_string_bytes(&lfb_hash)
        ))
    })
}

impl<T: TransportLayer + Send + Sync> MultiParentCasperImpl<T> {
    /// Runs finalizer cycles now, waiting for any cycle already in flight,
    /// and returns the LFB they leave.
    pub async fn finalize(&self) -> Result<BlockMessage, CasperError> {
        let held = self.finalizer_cycle.clone().lock_owned().await;
        run_queued_finalizer(
            build_finalization_context(self),
            self.finalizer_cycle.clone(),
            held,
            self.finalizer_task_queued.clone(),
        )
        .await;
        read_last_finalized_block(&self.block_dag_storage, &self.block_store)
    }
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
    if this.casper_shard_conf.finalization_rate <= 0 {
        return Ok(());
    }

    if new_block.body.state.block_number % this.casper_shard_conf.finalization_rate as i64 == 0 {
        let Ok(held) = this.finalizer_cycle.clone().try_lock_owned() else {
            note_finalizer_busy(&this.finalizer_task_queued);
            return Ok(());
        };

        let ctx = build_finalization_context(this);
        let finalizer_cycle = this.finalizer_cycle.clone();
        let finalizer_task_queued = this.finalizer_task_queued.clone();

        // Capture the JoinHandle so a panic inside `run_queued_finalizer`
        // surfaces in the logs instead of being silently dropped. The cycle
        // lock is released when the task ends, panicking or not.
        let handle = tokio::spawn(async move {
            run_queued_finalizer(ctx, finalizer_cycle, held, finalizer_task_queued).await;
        });
        tokio::spawn(async move {
            if let Err(join_err) = handle.await {
                tracing::error!("Finalization task terminated abnormally: {}", join_err);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod divergence_monitor_tests {
    use super::*;

    fn lfb(byte: u8) -> BlockHash { BlockHash::from(vec![byte; 32]) }

    /// The ucc-i6 shape in miniature: one pinned LFB, refusals with rising
    /// derived floors — divergence at the threshold, not one tick sooner.
    #[test]
    fn rising_refusals_on_one_pin_escalate_at_the_threshold() {
        let monitor = DivergenceMonitor::default();
        let pin = lfb(1);
        for round in 0..DIVERGENCE_ESCALATION_REFUSALS - 1 {
            monitor.on_containment_hold(&pin, 294 + round as i64);
            assert!(
                !monitor.diverged(),
                "must not escalate before the threshold"
            );
        }
        monitor.on_containment_hold(&pin, 294 + DIVERGENCE_ESCALATION_REFUSALS as i64);
        assert!(
            monitor.diverged(),
            "rising refusals past the threshold are a divergence"
        );
    }

    /// A transient hold — a rejected-then-recovered deploy whose re-homed
    /// carrier finalizes — advances before the threshold and never escalates.
    #[test]
    fn a_hold_that_advances_never_escalates() {
        let monitor = DivergenceMonitor::default();
        let pin = lfb(2);
        for round in 0..DIVERGENCE_ESCALATION_REFUSALS - 1 {
            monitor.on_containment_hold(&pin, 300 + round as i64);
        }
        monitor.on_advance();
        assert!(!monitor.diverged());
        monitor.on_containment_hold(&pin, 400);
        assert!(
            !monitor.diverged(),
            "the advance must have reset the streak"
        );
    }

    /// Refusals whose derived floor does NOT rise are not a divergence —
    /// the shard is not finalizing without us, the derivation is just stuck
    /// (a class-A stall surfaces through the heartbeat, not here).
    #[test]
    fn a_pinned_derivation_never_escalates() {
        let monitor = DivergenceMonitor::default();
        let pin = lfb(3);
        for _ in 0..DIVERGENCE_ESCALATION_REFUSALS * 3 {
            monitor.on_containment_hold(&pin, 294);
        }
        assert!(
            !monitor.diverged(),
            "a non-rising derivation is not a divergence"
        );
    }

    /// The streak is keyed to the pinned LFB: an adoption through another
    /// path re-pins and restarts the count.
    #[test]
    fn a_new_pin_restarts_the_streak() {
        let monitor = DivergenceMonitor::default();
        for round in 0..DIVERGENCE_ESCALATION_REFUSALS - 1 {
            monitor.on_containment_hold(&lfb(4), 294 + round as i64);
        }
        for round in 0..DIVERGENCE_ESCALATION_REFUSALS - 1 {
            monitor.on_containment_hold(&lfb(5), 294 + round as i64);
            assert!(
                !monitor.diverged(),
                "the new pin must not inherit the old streak"
            );
        }
    }

    /// An escalated divergence that later heals (a containing floor is
    /// adopted) clears the bit and can escalate again on a fresh streak.
    #[test]
    fn recovery_clears_the_divergence_and_rearms() {
        let monitor = DivergenceMonitor::default();
        let pin = lfb(6);
        for round in 0..=DIVERGENCE_ESCALATION_REFUSALS {
            monitor.on_containment_hold(&pin, 294 + round as i64);
        }
        assert!(monitor.diverged());
        monitor.on_advance();
        assert!(
            !monitor.diverged(),
            "adoption of a containing floor resolves it"
        );
        for round in 0..=DIVERGENCE_ESCALATION_REFUSALS {
            monitor.on_containment_hold(&lfb(7), 500 + round as i64);
        }
        assert!(monitor.diverged(), "a fresh streak must escalate again");
    }
}

#[cfg(test)]
mod finalizer_run_metric_tests {
    use std::time::Duration;

    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};

    use super::*;
    use crate::rust::metrics_constants::{
        FINALIZER_RUN_FAILURES_METRIC, FINALIZER_RUN_QUEUED_METRIC, FINALIZER_RUN_RERUNS_METRIC,
        FINALIZER_RUN_TIMEOUTS_METRIC, FINALIZER_RUN_TIME_METRIC,
    };

    /// One snapshot per test: the debugging recorder drains histogram
    /// samples on every snapshot, so a second snapshot would see none.
    struct Recorded(std::collections::HashMap<String, (u64, usize)>);

    impl Recorded {
        fn take(snapshotter: &Snapshotter) -> Self {
            let mut values = std::collections::HashMap::new();
            for (key, (_, _, value)) in snapshotter.snapshot().into_hashmap() {
                let entry = values.entry(key.key().name().to_owned()).or_insert((0, 0));
                match value {
                    DebugValue::Counter(c) => entry.0 += c,
                    DebugValue::Histogram(s) => entry.1 += s.len(),
                    _ => {}
                }
            }
            Recorded(values)
        }

        fn counter(&self, name: &str) -> u64 { self.0.get(name).map_or(0, |v| v.0) }

        fn samples(&self, name: &str) -> usize { self.0.get(name).map_or(0, |v| v.1) }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn each_cycle_records_its_duration_and_outcome() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);

        let queued = AtomicBool::new(false);
        let mut cycle = 0u32;
        let mut advances = Vec::new();
        drive_finalizer_runs(
            Duration::from_millis(50),
            &queued,
            || {
                cycle += 1;
                let this_cycle = cycle;
                if this_cycle <= 2 {
                    queued.store(true, Ordering::SeqCst);
                }
                async move {
                    match this_cycle {
                        1 => Err::<bool, &str>("floor unavailable"),
                        2 => {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            Ok(true)
                        }
                        _ => Ok(true),
                    }
                }
            },
            |advanced| {
                advances.push(advanced);
                async {}
            },
        )
        .await;

        let recorded = Recorded::take(&snapshotter);
        assert_eq!(cycle, 3, "a trigger during cycles 1 and 2 gives two reruns");
        assert_eq!(
            advances,
            vec![false, false, true],
            "every cycle runs the after-step once; a failed or timed-out \
             cycle reports no advance"
        );
        assert_eq!(recorded.samples(FINALIZER_RUN_TIME_METRIC), 3);
        assert_eq!(recorded.counter(FINALIZER_RUN_FAILURES_METRIC), 1);
        assert_eq!(recorded.counter(FINALIZER_RUN_TIMEOUTS_METRIC), 1);
        assert_eq!(recorded.counter(FINALIZER_RUN_RERUNS_METRIC), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_run_with_no_queued_trigger_makes_one_cycle_and_no_rerun() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);

        let queued = AtomicBool::new(false);
        drive_finalizer_runs(
            Duration::from_millis(50),
            &queued,
            || async { Ok::<bool, &str>(false) },
            |_| async {},
        )
        .await;

        let recorded = Recorded::take(&snapshotter);
        assert_eq!(recorded.samples(FINALIZER_RUN_TIME_METRIC), 1);
        assert_eq!(recorded.counter(FINALIZER_RUN_RERUNS_METRIC), 0);
        assert_eq!(recorded.counter(FINALIZER_RUN_FAILURES_METRIC), 0);
        assert_eq!(recorded.counter(FINALIZER_RUN_TIMEOUTS_METRIC), 0);
    }

    /// A trigger that lands after a cycle's last check of `queued`, while the
    /// lock is still held, must still get a cycle once the lock is released.
    #[tokio::test(flavor = "current_thread")]
    async fn a_trigger_after_the_last_queued_check_still_runs_a_cycle() {
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let queued = AtomicBool::new(false);
        let held = lock.clone().try_lock_owned().expect("the lock starts free");
        let mut cycles = 0u32;
        hold_finalizer_cycles(&lock, held, &queued, || {
            cycles += 1;
            if cycles == 1 {
                assert!(
                    lock.clone().try_lock_owned().is_err(),
                    "the trigger must find the lock held"
                );
                note_finalizer_busy(&queued);
            } else {
                queued.swap(false, Ordering::SeqCst);
            }
            async {}
        })
        .await;

        assert_eq!(cycles, 2, "the late trigger must get its own cycle");
        assert!(!queued.load(Ordering::SeqCst));
        assert!(lock.try_lock().is_ok(), "the lock must end free");
    }

    #[test]
    fn every_trigger_during_a_run_is_counted_but_queues_one_rerun() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);

        let queued = AtomicBool::new(false);
        note_finalizer_busy(&queued);
        note_finalizer_busy(&queued);
        note_finalizer_busy(&queued);

        let recorded = Recorded::take(&snapshotter);
        assert!(queued.load(Ordering::SeqCst));
        assert_eq!(recorded.counter(FINALIZER_RUN_QUEUED_METRIC), 3);
    }
}
