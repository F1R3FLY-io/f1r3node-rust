//! Finalization runner — the one writer of the LFB.
//!
//! The LFB DECISION does not live here: it is `floor::floor_of_view` — the
//! one finality clock, shared with every block operation's floor
//! derivation. This module hosts the runner around it: pacing
//! (`finalization_rate`), the finalizer task with its timeout backstop, the
//! finalization effects (`record_directly_finalized`, event publication)
//! applied when the clock advances, and the deploy-lifecycle work that
//! follows each cycle.
//!
//! Every cycle runs on the casper instance's one `Finalizer` task, one cycle at
//! a time, so the LFB has exactly one writer. A cycle is requested from two
//! places:
//!   * `block_admission::admit_handle_valid_block` →
//!     `update_last_finalized_block`, which only wakes the task;
//!   * `MultiParentCasperImpl::finalize`, which waits for a cycle that started
//!     after the call.
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
use crate::rust::casper::CasperShardConf;
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

/// What a finalizer cycle reads and writes: handles shared with the casper
/// instance the finalizer serves, and that instance's shard conf.
#[derive(Clone)]
pub struct FinalizationContext {
    /// Read every cycle, because the observer is attached after the casper
    /// instance, and so after its finalizer, is built.
    pub observer: Arc<std::sync::OnceLock<crate::rust::soak_observer::ObserverBinding>>,
    pub block_dag_storage: BlockDagKeyValueStorage,
    pub block_store: KeyValueBlockStore,
    pub runtime_manager: Arc<RuntimeManager>,
    pub event_publisher: F1r3flyEvents,
    pub divergence_monitor: Arc<DivergenceMonitor>,
    pub deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    pub finalized_bond_cache: super::bond_cache::FinalizedBondCache,
    pub shard_conf: CasperShardConf,
}

impl FinalizationContext {
    /// The exact threshold, never the `f32` display field: it decides finality.
    fn ftt(&self) -> crate::rust::safety::clique_oracle::FtThreshold {
        crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
            self.shard_conf.fault_tolerance_threshold_ppm,
        )
    }

    fn citability_horizon(&self) -> Option<i64> {
        crate::rust::finality::deploy_lifecycle::citability_horizon(
            self.shard_conf.max_parent_depth,
        )
    }
}

/// Backstop only: floor-of-view rides the persisted floor/frontier caches, so a
/// compute exceeding this is a stall to surface, not pace.
const FINALIZER_COMPUTE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

type CycleFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
type Cycle = Box<dyn Fn() -> CycleFuture + Send>;

/// The one finalizer task of a casper instance. Its cycles run one at a time,
/// which is what makes it the one LFB writer. `Notify` holds at most one
/// permit, so a trigger is never lost and a burst of triggers coalesces into
/// one cycle. Dropping the handle stops the task.
pub struct Finalizer {
    wake: Arc<tokio::sync::Notify>,
    requested: Arc<std::sync::atomic::AtomicU64>,
    in_cycle: Arc<AtomicBool>,
    /// The `requested` value each completed cycle read when it started.
    completed: tokio::sync::watch::Receiver<u64>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl Finalizer {
    /// Starts the finalizer task of the casper instance `ctx` belongs to.
    pub fn start(ctx: FinalizationContext) -> Self { Self::spawn(finalizer_cycle(ctx)) }

    fn spawn(cycle: Cycle) -> Self {
        let wake = Arc::new(tokio::sync::Notify::new());
        let requested = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let in_cycle = Arc::new(AtomicBool::new(false));
        let (completed_tx, completed) = tokio::sync::watch::channel(0);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        tokio::spawn(finalizer_loop(
            cycle,
            wake.clone(),
            requested.clone(),
            in_cycle.clone(),
            completed_tx,
            stopped,
        ));
        Self {
            wake,
            requested,
            in_cycle,
            completed,
            _stop: stop,
        }
    }

    /// Asks for a cycle and returns at once.
    pub(crate) fn trigger(&self) {
        if self.in_cycle.load(Ordering::SeqCst) {
            metrics::counter!(
                crate::rust::metrics_constants::FINALIZER_RUN_QUEUED_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
        }
        self.requested.fetch_add(1, Ordering::SeqCst);
        self.wake.notify_one();
    }

    /// Asks for a cycle and waits until a cycle that started after this call
    /// has completed.
    pub(crate) async fn run(&self) -> Result<(), CasperError> {
        let ticket = self.requested.fetch_add(1, Ordering::SeqCst) + 1;
        self.wake.notify_one();
        self.completed
            .clone()
            .wait_for(|started_with| *started_with >= ticket)
            .await
            .map(|_| ())
            .map_err(|_| CasperError::RuntimeError("the finalizer task has stopped".to_string()))
    }
}

/// A cycle reads `requested` before it starts, so a cycle that completes with a
/// value at or above a caller's ticket started after that caller asked.
async fn finalizer_loop(
    cycle: Cycle,
    wake: Arc<tokio::sync::Notify>,
    requested: Arc<std::sync::atomic::AtomicU64>,
    in_cycle: Arc<AtomicBool>,
    completed: tokio::sync::watch::Sender<u64>,
    mut stopped: tokio::sync::oneshot::Receiver<()>,
) {
    use crate::rust::metrics_constants::{CASPER_METRICS_SOURCE, FINALIZER_RUN_RERUNS_METRIC};
    loop {
        tokio::select! {
            _ = wake.notified() => {}
            _ = &mut stopped => return,
        }
        let started_with = requested.load(Ordering::SeqCst);
        in_cycle.store(true, Ordering::SeqCst);
        // A subtask, so a panicking cycle costs that cycle, not finality.
        if let Err(join_err) = tokio::spawn(cycle()).await {
            tracing::error!(
                target: "f1r3fly.casper",
                "finalizer cycle terminated abnormally: {}",
                join_err
            );
        }
        in_cycle.store(false, Ordering::SeqCst);
        if requested.load(Ordering::SeqCst) > started_with {
            metrics::counter!(FINALIZER_RUN_RERUNS_METRIC, "source" => CASPER_METRICS_SOURCE)
                .increment(1);
        }
        completed.send_replace(started_with);
    }
}

fn finalizer_cycle(ctx: FinalizationContext) -> Cycle {
    Box::new(move || {
        let ctx = ctx.clone();
        Box::pin(async move {
            run_cycle(
                FINALIZER_COMPUTE_TIMEOUT,
                || compute_last_finalized_block(&ctx),
                |advanced| settle_after_cycle(ctx.clone(), advanced),
            )
            .await
        })
    })
}

/// One finalizer cycle, generic over its two steps so its accounting is
/// testable without a casper instance. `compute` runs under the timeout and
/// reports whether the LFB advanced; `after` runs untimed with that answer, so
/// a slow step after the advance is never abandoned midway.
pub(crate) async fn run_cycle<E, F, Fut, A, AFut>(
    timeout: std::time::Duration,
    compute: F,
    after: A,
) where
    E: std::fmt::Debug,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<bool, E>>,
    A: FnOnce(bool) -> AFut,
    AFut: std::future::Future<Output = ()>,
{
    use crate::rust::metrics_constants::{
        CASPER_METRICS_SOURCE, FINALIZER_RUN_FAILURES_METRIC, FINALIZER_RUN_TIMEOUTS_METRIC,
        FINALIZER_RUN_TIME_METRIC,
    };
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
}

/// The work after one cycle that reads the persisted LFB, on the blocking
/// pool: the bond-cache refresh, the deploy-verdict settle and the pool
/// release. They run every cycle, not only on an advance: each is idempotent,
/// and an advance persisted by a cycle that then failed, timed out or crashed
/// is otherwise not seen until the next advance. The carrier prune is
/// retention only, so it waits for an advance.
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
    use crate::rust::finality::deploy_lifecycle::{record_settle_timing, release_settled, settle};

    if let Err(e) = ctx.finalized_bond_cache.refresh(&ctx.block_dag_storage) {
        tracing::error!(
            target: "f1r3fly.casper",
            error = %e,
            "finalized bond cache refresh failed; the next cycle retries"
        );
    }
    let dag = ctx.block_dag_storage.get_representation()?;
    let settle_started = std::time::Instant::now();
    let settled = settle(
        &dag,
        &ctx.block_store,
        ctx.shard_conf.deploy_lifespan,
        ctx.citability_horizon(),
    );
    let settle_ms = settle_started.elapsed().as_millis();
    let prune_ms = if advanced {
        let prune_started = std::time::Instant::now();
        let lfb_height = dag.block_number_unsafe(&dag.last_finalized_block())?;
        if let Err(e) = dag.prune_carriers_below(lfb_height - ctx.shard_conf.deploy_lifespan) {
            tracing::warn!("carrier-index prune failed (retention only): {}", e);
        }
        Some(prune_started.elapsed().as_millis())
    } else {
        None
    };
    record_settle_timing(settle_ms, prune_ms);
    release_settled(&dag, &ctx.deploy_storage)?;
    settled.map(|_| ())
}

/// One LFB decision and, on an advance, its finalization effects. Returns
/// whether the LFB advanced.
///
/// The decision is `floor_of_view` — the same derivation and exact `>= θ` test
/// every block operation uses. It advances the LFB only to a floor that
/// CAPTURES the current one, so the read surface never designates a state
/// missing settled content.
pub(crate) async fn compute_last_finalized_block(
    ctx: &FinalizationContext,
) -> Result<bool, CasperError> {
    use crate::rust::finality::floor::{floor_of_view, Floor};

    let started = std::time::Instant::now();
    let observer = ctx.observer.get().cloned();
    let operation = observer.as_ref().and_then(|observer| observer.operation());
    let observed = observer.as_ref().map(|observer| (observer, operation));
    let dag = ctx.block_dag_storage.get_representation()?;
    let current_hash = dag.last_finalized_block();
    let current = Floor {
        block_number: dag.lookup_unsafe(&current_hash)?.block_number,
        hash: current_hash,
    };

    let derivation = floor_of_view(&dag, &ctx.block_store, &current, ctx.ftt()).await;
    if let Some((observer, operation)) = observed {
        emit_derivation(observer, operation, &derivation);
    }
    let adopted = match adoption(ctx, &current, derivation?) {
        Some(new_lfb) => {
            adopt(ctx, &dag, &current, &new_lfb, observed).await?;
            true
        }
        None => false,
    };
    tracing::debug!(
        target: "f1r3fly.casper.lfb.timing",
        "last_finalized_block timing: total_ms={}, new_lfb_found={}",
        started.elapsed().as_millis(),
        adopted
    );
    Ok(adopted)
}

fn emit_derivation(
    observer: &crate::rust::soak_observer::ObserverBinding,
    operation: Option<u64>,
    derivation: &Result<crate::rust::finality::floor::FloorOfView, CasperError>,
) {
    use crate::rust::finality::floor::FloorOfView;
    use crate::rust::soak_observer::{EventKind, FloorOutcome};
    let (outcome, hash, height) = match derivation {
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
        operation,
        Some(outcome),
        hash,
        height,
        Some(derivation.is_ok()),
    );
}

/// The floor this cycle adopts, if any. A hold is reported here and adopts
/// nothing.
fn adoption(
    ctx: &FinalizationContext,
    current: &crate::rust::finality::floor::Floor,
    derivation: crate::rust::finality::floor::FloorOfView,
) -> Option<crate::rust::finality::floor::Floor> {
    use crate::rust::finality::floor::FloorOfView;
    match derivation {
        FloorOfView::Advance(floor) => Some(floor),
        FloorOfView::NoAdvance => None,
        FloorOfView::ContainmentHold { derived } => {
            ctx.divergence_monitor
                .on_containment_hold(&current.hash, derived.block_number);
            None
        }
        FloorOfView::AbsenceHold { missing } => {
            tracing::debug!(
                missing = %PrettyPrinter::build_string_bytes(&missing),
                "finalizer holds this cycle: the floor walk needs a block \
                 this node does not hold. Nothing requests it here, so the \
                 hold lasts until catch-up fetches it for another reason"
            );
            None
        }
        FloorOfView::IncompatibilityHold { detail } => {
            tracing::debug!(
                detail,
                "finalizer holds this cycle: incompatible majority-agreement \
                 candidates under a negative fault-tolerance threshold; the \
                 next merge spanning both branches reconciles them"
            );
            None
        }
    }
}

/// Persists `new_lfb` and applies the effects of everything it finalizes. The
/// divergence alarm tracks the PERSISTED LFB, so its streak clears only once
/// the persist succeeds.
async fn adopt(
    ctx: &FinalizationContext,
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    current: &crate::rust::finality::floor::Floor,
    new_lfb: &crate::rust::finality::floor::Floor,
    observed: Option<(&crate::rust::soak_observer::ObserverBinding, Option<u64>)>,
) -> Result<(), CasperError> {
    use crate::rust::soak_observer::EventKind;
    let ft_value = crate::rust::safety::clique_oracle::CliqueOracle::normalized_fault_tolerance(
        &new_lfb.hash,
        dag,
    )
    .await
    .map_err(CasperError::from)?;
    // `extends_previous_lfb` is true by construction (an adoption captures the
    // current LFB); soak dashboards alarm on the field, so it stays.
    tracing::info!(
        target: "f1r3fly.casper.finalization_lifecycle",
        event = "candidate_finalized",
        candidate_hash = %hex::encode(&new_lfb.hash),
        candidate_height = new_lfb.block_number,
        previous_lfb_hash = %hex::encode(&current.hash),
        previous_lfb_height = current.block_number,
        extends_previous_lfb = true,
        fault_tolerance = ft_value,
        "finalization lifecycle"
    );
    let emit = |kind, ok| {
        if let Some((observer, operation)) = observed {
            observer.emit(
                kind,
                operation,
                None,
                Some(&new_lfb.hash),
                Some(new_lfb.block_number),
                ok,
            );
        }
    };
    emit(EventKind::EffectAttempt, None);
    let effect_started = std::time::Instant::now();
    let persisted = ctx
        .block_dag_storage
        .record_directly_finalized(new_lfb.hash.clone(), ft_value, |finalized| {
            process_finalized(ctx, finalized.clone(), &new_lfb.hash)
        })
        .await;
    emit(EventKind::EffectReturn, Some(persisted.is_ok()));
    persisted.map_err(CasperError::KvStoreError)?;
    tracing::debug!(
        target: "f1r3fly.casper.finalizer.effect.timing",
        "record_directly_finalized_total_ms={}",
        effect_started.elapsed().as_millis()
    );
    ctx.divergence_monitor.on_advance();
    Ok(())
}

/// The effects of newly finalized blocks: the runtime's block-index cache
/// entry is dropped and `BlockFinalised` is published for each.
async fn process_finalized(
    ctx: &FinalizationContext,
    finalized: HashSet<BlockHash>,
    directly_finalized: &BlockHash,
) -> Result<(), KvStoreError> {
    let started = std::time::Instant::now();
    for block_hash in &finalized {
        let block = ctx.block_store.get(block_hash)?.ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized block {} not present in store",
                PrettyPrinter::build_string_bytes(block_hash)
            ))
        })?;
        ctx.runtime_manager.remove_block_index_cache(block_hash);
        log_finalized_deploys(&block, block_hash == directly_finalized);
        ctx.event_publisher
            .publish(finalised_event(&block))
            .map_err(|e| KvStoreError::IoError(e.to_string()))?;
    }
    tracing::debug!(
        target: "f1r3fly.casper.finalizer.effect.timing",
        "Finalization effect timing: finalized_blocks={}, process_finalized_ms={}",
        finalized.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}

fn log_finalized_deploys(block: &BlockMessage, directly_finalized: bool) {
    for processed in &block.body.deploys {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "finalized_inclusion",
            deploy_sig = %hex::encode(&processed.deploy.sig),
            block_hash = %hex::encode(&block.block_hash),
            block_number = block.body.state.block_number,
            sender = %hex::encode(&block.sender),
            failed = processed.is_failed,
            directly_finalized,
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
            directly_finalized,
            "deploy lifecycle"
        );
    }
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
    /// Waits for a finalizer cycle that started after this call to complete,
    /// and returns the LFB it leaves.
    pub async fn finalize(&self) -> Result<BlockMessage, CasperError> {
        self.finalizer.run().await?;
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
        this.finalizer.trigger();
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
    use std::sync::atomic::AtomicU32;
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

        let mut advances = Vec::new();
        for outcome in 0..3 {
            run_cycle(
                Duration::from_millis(50),
                || async move {
                    match outcome {
                        0 => Err::<bool, &str>("floor unavailable"),
                        1 => {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            Ok(true)
                        }
                        _ => Ok(true),
                    }
                },
                |advanced| {
                    advances.push(advanced);
                    async {}
                },
            )
            .await;
        }

        let recorded = Recorded::take(&snapshotter);
        assert_eq!(
            advances,
            vec![false, false, true],
            "every cycle runs the after-step once; a failed or timed-out \
             cycle reports no advance"
        );
        assert_eq!(recorded.samples(FINALIZER_RUN_TIME_METRIC), 3);
        assert_eq!(recorded.counter(FINALIZER_RUN_FAILURES_METRIC), 1);
        assert_eq!(recorded.counter(FINALIZER_RUN_TIMEOUTS_METRIC), 1);
    }

    /// A mock cycle: counts its runs, holds each run until `gate` gets a
    /// permit, and then panics on the runs listed in `panic_on`.
    struct Probe {
        runs: Arc<AtomicU32>,
        started: Arc<tokio::sync::Notify>,
        gate: Arc<tokio::sync::Semaphore>,
        panic_on: &'static [u32],
    }

    impl Probe {
        fn new(panic_on: &'static [u32]) -> Self {
            Self {
                runs: Arc::new(AtomicU32::new(0)),
                started: Arc::new(tokio::sync::Notify::new()),
                gate: Arc::new(tokio::sync::Semaphore::new(0)),
                panic_on,
            }
        }

        fn cycle(&self) -> Cycle {
            let (runs, started, gate, panic_on) = (
                self.runs.clone(),
                self.started.clone(),
                self.gate.clone(),
                self.panic_on,
            );
            Box::new(move || {
                let (runs, started, gate) = (runs.clone(), started.clone(), gate.clone());
                Box::pin(async move {
                    let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                    started.notify_one();
                    gate.acquire().await.expect("gate open").forget();
                    assert!(!panic_on.contains(&run), "cycle {run} panics");
                })
            })
        }

        fn runs(&self) -> u32 { self.runs.load(Ordering::SeqCst) }
    }

    async fn settle_task() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_burst_of_triggers_during_a_cycle_gives_exactly_one_more_cycle() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let probe = Probe::new(&[]);
        let finalizer = Finalizer::spawn(probe.cycle());

        finalizer.trigger();
        probe.started.notified().await;
        for _ in 0..3 {
            finalizer.trigger();
        }
        probe.gate.add_permits(2);
        settle_task().await;

        let recorded = Recorded::take(&snapshotter);
        assert_eq!(
            probe.runs(),
            2,
            "three triggers during a cycle coalesce into one more cycle"
        );
        assert_eq!(recorded.counter(FINALIZER_RUN_QUEUED_METRIC), 3);
        assert_eq!(recorded.counter(FINALIZER_RUN_RERUNS_METRIC), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn finalize_waits_for_a_cycle_that_started_after_the_call() {
        let probe = Probe::new(&[]);
        let finalizer = Finalizer::spawn(probe.cycle());

        finalizer.trigger();
        probe.started.notified().await;
        let run = finalizer.run();
        tokio::pin!(run);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut run)
                .await
                .is_err(),
            "the call must not return while the cycle that started before it runs"
        );
        probe.gate.add_permits(1);
        probe.started.notified().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut run)
                .await
                .is_err(),
            "the call must not return when the cycle that started before it completes"
        );
        probe.gate.add_permits(1);
        run.await.expect("the finalizer task is running");
        assert_eq!(probe.runs(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_panicking_cycle_does_not_stop_the_loop() {
        let probe = Probe::new(&[1]);
        let finalizer = Finalizer::spawn(probe.cycle());
        probe.gate.add_permits(2);

        finalizer.trigger();
        probe.started.notified().await;
        finalizer
            .run()
            .await
            .expect("the task survives a panicking cycle");
        assert_eq!(probe.runs(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_task_ends_when_the_finalizer_is_dropped() {
        let probe = Probe::new(&[]);
        probe.gate.add_permits(1);
        let finalizer = Finalizer::spawn(probe.cycle());
        finalizer.run().await.expect("first cycle");
        assert!(
            Arc::strong_count(&probe.runs) > 1,
            "the running task holds the cycle"
        );

        drop(finalizer);
        settle_task().await;
        assert_eq!(
            Arc::strong_count(&probe.runs),
            1,
            "the task must drop its cycle once the finalizer is dropped"
        );
    }
}
