//! Finalization runner — background task, RAII guard,
//! `compute_last_finalized_block`, `update_last_finalized_block`.
//!
//! The LFB DECISION does not live here: it is `floor::floor_of_view` — the
//! one finality clock, shared with every block operation's floor
//! derivation. This module hosts only the runner scaffolding around it:
//! pacing (`finalization_rate`), the queued-run loop with its timeout
//! backstop, and the finalization effects (`record_directly_finalized`,
//! event publication) applied when the clock advances.
//!
//! Phase 3 (Commit 2): extracted from `engine::multi_parent_casper`.
//! The functions here are reachable via:
//!   * `MultiParentCasper::last_finalized_block` (mod.rs) →
//!     `compute_last_finalized_block` (here)
//!   * `block_admission::admit_handle_valid_block` (block_admission.rs) →
//!     `self.update_last_finalized_block` (inherent method here)
//!   * background task spawned by `update_last_finalized_block` →
//!     `run_queued_finalizer` → `compute_last_finalized_block`
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
//! Deploy-STORAGE release does not happen in this module at all: the
//! deploy-lifecycle register names the moment a deploy stops being
//! re-proposable (its write-once terminal verdict), and block admission
//! releases the pool copy against exactly that list.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
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
    pub(crate) block_dag_storage: BlockDagKeyValueStorage,
    pub(crate) block_store: KeyValueBlockStore,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicBool>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) ftt: crate::rust::safety::clique_oracle::FtThreshold,
    pub(crate) divergence_monitor: Arc<DivergenceMonitor>,
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
    }
}

#[tracing::instrument(level = "info", skip_all)]
pub(crate) async fn run_queued_finalizer(
    ctx: FinalizationContext,
    finalizer_task_in_progress: Arc<AtomicBool>,
    finalizer_task_queued: Arc<AtomicBool>,
) {
    let _task_guard = FinalizationGuard(finalizer_task_in_progress.as_ref());
    tracing::info!(target: "f1r3fly.casper", "finalizer-run-started");

    // Backstop only: floor-of-view rides the persisted floor/frontier
    // caches, so a cycle exceeding this is a stall to surface, not pace.
    let finalizer_blocking_timeout = std::time::Duration::from_secs(15);
    loop {
        match tokio::time::timeout(
            finalizer_blocking_timeout,
            compute_last_finalized_block(ctx.clone()),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                tracing::warn!("finalizer-run failed: {:?}", err);
            }
            Err(_) => {
                tracing::warn!(
                    "finalizer-run timed out after {:?}; skipping this cycle to avoid blocking propose",
                    finalizer_blocking_timeout
                );
            }
        }

        if finalizer_task_queued.swap(false, Ordering::SeqCst) {
            tracing::debug!("finalizer-run-queued; continuing finalizer loop");
            continue;
        }

        tracing::info!(target: "f1r3fly.casper", "finalizer-run-finished");
        return;
    }
}

pub(crate) async fn compute_last_finalized_block(
    ctx: FinalizationContext,
) -> Result<BlockMessage, CasperError> {
    let FinalizationContext {
        block_dag_storage,
        block_store,
        runtime_manager,
        event_publisher,
        finalization_in_progress,
        enable_mergeable_channel_gc,
        ftt,
        divergence_monitor,
    } = ctx;
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

                            // Deploy-storage removal deliberately does NOT
                            // happen here: node-local finalization of a
                            // block is not evidence its deploys' effects
                            // are canonical. The lifecycle register's
                            // terminal verdicts drive the release, at
                            // block admission.

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
    let new_lfb_opt =
        match crate::rust::finality::floor::floor_of_view(&dag, &block_store, &current, ftt).await?
        {
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
                     this node does not hold; catch-up delivers it"
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
    let final_lfb_hash = if let Some(new_lfb) = new_lfb_opt {
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
        new_lfb_found_effect((new_lfb.hash.clone(), ft_value))
            .await
            .map_err(CasperError::KvStoreError)?;
        divergence_monitor.on_advance();
        new_lfb.hash
    } else {
        last_finalized_block_hash
    };
    let finalizer_ms = finalizer_started.elapsed().as_millis();

    // Deploy-pool release is NOT done here. The register
    // (`finality::deploy_lifecycle`) is the one component that re-evaluates
    // as the floor advances, so it is the only component that can name the
    // moment a deploy stops being re-proposable; block admission releases
    // the pool copy against exactly its write-once terminal list. The
    // finality marker is never a release edge: a marked block can still be
    // excluded from every future cone, and an orphaned carrier's pool copy
    // is its only route back into a live branch.

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
    if this.casper_shard_conf.finalization_rate <= 0 {
        return Ok(());
    }

    if new_block.body.state.block_number % this.casper_shard_conf.finalization_rate as i64 == 0 {
        if this
            .finalizer_task_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            if !this.finalizer_task_queued.swap(true, Ordering::SeqCst) {
                tracing::debug!("Finalizer already running; queued follow-up finalization run");
            }
            return Ok(());
        }

        let ctx = build_finalization_context(this);
        let finalizer_task_in_progress = this.finalizer_task_in_progress.clone();
        let finalizer_task_queued = this.finalizer_task_queued.clone();

        // Capture the JoinHandle so a panic inside `run_queued_finalizer`
        // surfaces in the logs instead of being silently dropped. The
        // RAII `FinalizationGuard` in run_queued_finalizer prevents the
        // in-progress flag from sticking even if the task panics, so
        // there's no deadlock — but silent panics mask real bugs.
        let handle = tokio::spawn(async move {
            run_queued_finalizer(ctx, finalizer_task_in_progress, finalizer_task_queued).await;
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
