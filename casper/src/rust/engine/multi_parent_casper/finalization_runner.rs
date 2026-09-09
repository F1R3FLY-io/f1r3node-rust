//! Finalization runner — background task, RAII guard,
//! `compute_last_finalized_block`, `update_last_finalized_block`.
//!
//! The finality decision is `floor::floor_of_view`, shared with block floor
//! derivation. This module schedules concurrent evaluations and commits one
//! certified, state-preserving ledger successor atomically.
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use parking_lot::Mutex;
use prost::bytes::Bytes;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KvStoreError;

// Phase 7 (C-3): import the struct from its canonical sibling module
// instead of via the legacy shim — the previous import formed a circular
// path `engine::multi_parent_casper → engine::multi_parent_casper → engine::multi_parent_casper::types`.
use super::events::finalised_event;
use super::types::MultiParentCasperImpl;
use crate::rust::errors::CasperError;
use crate::rust::finality::finalization_schedule::FinalizationSchedule;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

const DIVERGENCE_ESCALATION_REFUSALS: u64 = 10;

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
    fn streak(&self) -> std::sync::MutexGuard<'_, HoldStreak> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn on_advance(&self) {
        *self.streak() = HoldStreak::default();
        if self.diverged.swap(false, Ordering::SeqCst) {
            tracing::info!(
                target: "f1r3fly.finalizer",
                "containment hold cleared after a containing floor was committed"
            );
        }
    }

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
                "finality divergence: rising derived floors do not preserve the committed LFB state"
            );
        }
    }

    pub fn diverged(&self) -> bool { self.diverged.load(Ordering::SeqCst) }
}

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
    Deferred,
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
        FinalizationWorkerOutcome::Deferred => {
            schedule.mark_deferred();
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
    pub(crate) deploy_lifecycle: Arc<crate::rust::finality::deploy_lifecycle::DeployLifecycle>,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicU64>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) protocol_version: i64,
    pub(crate) deploy_lifespan: i64,
    pub(crate) max_parent_depth: i32,
    pub(crate) shard_id: String,
    pub(crate) ftt: FtThreshold,
    pub(crate) finalization_schedule: Arc<FinalizationSchedule>,
    pub(crate) divergence_monitor: Arc<DivergenceMonitor>,
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
        deploy_lifecycle: this.deploy_lifecycle.clone(),
        runtime_manager: this.runtime_manager.clone(),
        event_publisher: this.event_publisher.clone(),
        finalization_in_progress: this.finalization_in_progress.clone(),
        enable_mergeable_channel_gc: this.casper_shard_conf.enable_mergeable_channel_gc,
        protocol_version: this.casper_shard_conf.casper_version,
        deploy_lifespan: this.casper_shard_conf.deploy_lifespan,
        max_parent_depth: this.casper_shard_conf.max_parent_depth,
        shard_id: this.casper_shard_conf.shard_name.clone(),
        // Exact ppm from the shard conf — the source of truth for the DECISION.
        ftt: FtThreshold::from_ppm(this.casper_shard_conf.fault_tolerance_threshold_ppm),
        finalization_schedule: this.finalization_schedule.clone(),
        divergence_monitor: this.divergence_monitor.clone(),
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
                Ok(Err(CasperError::BlockNotHeld(missing))) => {
                    tracing::debug!(
                        covered_through,
                        missing = %PrettyPrinter::build_string_bytes(&missing),
                        "finalizer worker deferred until the missing dependency is held"
                    );
                    FinalizationWorkerOutcome::Deferred
                }
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
    ctx.block_dag_storage
        .require_finalization_effect_projection(revision)?;
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
    let dag = ctx.block_dag_storage.get_representation()?;
    for block in blocks {
        let deploy_effect = effect_id(
            revision,
            block.block_hash.clone(),
            FinalizationEffectKind::DeployRemoval,
        );
        if !ctx
            .block_dag_storage
            .finalization_effect_completed(&deploy_effect)?
        {
            ctx.deploy_lifecycle
                .observe_block(
                    &dag,
                    &ctx.block_store,
                    &block,
                    ctx.deploy_lifespan,
                    crate::rust::finality::deploy_lifecycle::citability_horizon(
                        ctx.max_parent_depth,
                    ),
                    revision,
                )
                .await
                .map_err(|error| {
                    KvStoreError::IoError(format!(
                        "deploy lifecycle finalization effect failed: {error}"
                    ))
                })?;
            let mut terminal_deploy_signatures = HashSet::new();
            {
                let deploy_storage = ctx.deploy_storage.lock();
                for deploy in deploy_storage.read_all_for_protocol(ctx.protocol_version)? {
                    if dag.deploy_terminal(deploy.typed_deploy_id())?.is_some() {
                        terminal_deploy_signatures.insert(deploy.typed_deploy_id().clone());
                    }
                }
            }
            {
                let rejected = ctx
                    .rejected_deploy_buffer
                    .lock()
                    .map_err(|error| KvStoreError::LockError(error.to_string()))?;
                for deploy in rejected.read_all()? {
                    if dag.deploy_terminal(deploy.typed_deploy_id())?.is_some() {
                        terminal_deploy_signatures.insert(deploy.typed_deploy_id().clone());
                    }
                }
            }
            {
                let mut deploy_storage = ctx.deploy_storage.lock();
                for signature in &terminal_deploy_signatures {
                    match signature {
                        models::rust::deploy_id::DeployLookupId::Legacy(signature) => {
                            deploy_storage.remove_by_sig(signature.as_bytes())?;
                        }
                        models::rust::deploy_id::DeployLookupId::V6(deploy_id) => {
                            deploy_storage.remove_envelope_by_id(deploy_id.as_ref())?;
                        }
                    }
                }
            }
            let mut rejected = ctx
                .rejected_deploy_buffer
                .lock()
                .map_err(|error| KvStoreError::LockError(error.to_string()))?;
            for signature in &terminal_deploy_signatures {
                rejected.remove_by_id(signature)?;
            }
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
    }
    ctx.block_dag_storage
        .record_finalization_round_effects_completed_async(revision)
        .await?;
    Ok(())
}

async fn reconcile_finalization_effects(ctx: &FinalizationContext) -> Result<(), KvStoreError> {
    ctx.block_dag_storage.reconcile_finalization_projection()?;
    ctx.block_dag_storage
        .reconcile_finalization_effects_cursor_async()
        .await?;
    ctx.block_dag_storage
        .reconcile_finalization_effect_compaction_async()
        .await?;
    let mut effects_scan = ctx.block_dag_storage.pending_finalization_effect_scan()?;
    while let Some(FinalizationRecord {
        revision,
        finalized,
        ..
    }) = effects_scan.next_record()?
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
    let evaluation_generation = evaluation_base.dag_generation;
    let effect_ctx = ctx.clone();
    let wake_ctx = ctx.clone();
    let FinalizationContext {
        block_store,
        ftt,
        protocol_version,
        shard_id,
        finalization_schedule,
        divergence_monitor,
        ..
    } = ctx;
    let lfb_lookup_started = std::time::Instant::now();
    let dag = evaluation_base.dag;
    let last_finalized_block_hash = evaluation_base.head.block_hash.0.clone();
    let last_finalized_block_height = evaluation_base.head.block_number;
    let evaluation_head = evaluation_base.head;
    let certificate_context =
        match crate::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
            &dag,
            last_finalized_block_hash.clone(),
        ) {
            Ok(context) => {
                finalization_schedule.clear_missing_dependencies_through(evaluation_generation);
                context
            }
            Err(CasperError::BlockNotHeld(missing)) => {
                let parked = finalization_schedule.park_missing_dependency_if_current(
                    evaluation_generation,
                    missing.clone(),
                    || {
                        Ok::<_, CasperError>(
                            effect_ctx.block_dag_storage.current_generation()
                                == evaluation_generation
                                && dag.lookup(&missing)?.is_none(),
                        )
                    },
                )?;
                if !parked {
                    publish_finalization_request(effect_ctx, finalization_schedule.clone())?;
                }
                return Err(CasperError::BlockNotHeld(missing));
            }
            Err(error) => return Err(error),
        };
    let predecessor_post_state = dag
        .lookup_unsafe(&evaluation_head.block_hash.0)?
        .post_state_hash;
    let genesis_hash = effect_ctx
        .block_dag_storage
        .genesis_hash()?
        .ok_or_else(|| CasperError::RuntimeError("genesis hash is not initialized".to_string()))?;
    let approved_genesis = block_store
        .get(&genesis_hash)?
        .ok_or_else(|| CasperError::BlockNotHeld(genesis_hash.clone()))?;
    let witness_inputs = FinalizationWitnessInputs {
        protocol_version,
        shard_id,
        predecessor_certificate_digest: BlockHashSerde(Bytes::from(vec![
            0;
            models::rust::block_hash::LENGTH
        ])),
        predecessor_certificate_block_hash: BlockHashSerde(Bytes::from(vec![
            0;
            models::rust::block_hash::LENGTH
        ])),
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
    let evaluation_revision = evaluation_head.revision;
    let carrier_dag = dag.clone();
    let carrier_block_store = block_store.clone();
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let effect_ctx = effect_ctx.clone();
        let evaluation_head = evaluation_head.clone();
        let mut witness_inputs = witness_inputs.clone();
        let carrier_dag = carrier_dag.clone();
        let carrier_block_store = carrier_block_store.clone();
        let predecessor_post_state = predecessor_post_state.clone();
        let approved_genesis = approved_genesis.clone();
        async move {
            let effect_started = std::time::Instant::now();
            if evaluation_head.revision > 0 {
                let roots = witness_inputs
                    .latest_messages
                    .values()
                    .map(|hash| hash.0.clone())
                    .chain(std::iter::once(new_lfb.clone()))
                    .collect::<Vec<_>>();
                let mut remaining_work = models::rust::casper::protocol::casper_message::FinalizationCertificate::MAX_DAG_VISITS_PER_VERIFICATION;
                let support = carrier_dag.certified_support_closure(
                    &evaluation_head.block_hash.0,
                    roots,
                    models::rust::casper::protocol::casper_message::FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
                    &mut remaining_work,
                )?;
                let carrier =
                    crate::rust::finality::certificate::select_predecessor_certificate_carrier(
                        &support,
                        &new_lfb,
                        &evaluation_head.block_hash.0,
                        &predecessor_post_state,
                        witness_inputs.protocol_version,
                        &carrier_dag,
                        &carrier_block_store,
                        &approved_genesis,
                    )
                    .map_err(|error| KvStoreError::InvalidArgument(error.to_string()))?
                    .ok_or_else(|| {
                        KvStoreError::FinalizationCertificateCarrierPending {
                            expected_revision: evaluation_head.revision,
                            floor_hash: evaluation_head.block_hash.0.to_vec(),
                            certificate_digest: evaluation_head.certificate_digest.0.to_vec(),
                        }
                    })?;
                witness_inputs.predecessor_certificate_digest =
                    BlockHashSerde(carrier.certificate_digest);
                witness_inputs.predecessor_certificate_block_hash =
                    BlockHashSerde(carrier.block_hash);
            }
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

    let finalizer_started = std::time::Instant::now();
    let current = crate::rust::finality::floor::Floor {
        hash: last_finalized_block_hash.clone(),
        block_number: last_finalized_block_height,
    };
    let candidate = match crate::rust::finality::floor::floor_of_frozen_vote_projection(
        &dag,
        &block_store,
        &current,
        certificate_context
            .vote_projection()
            .eligible_latest_messages(),
        ftt,
    )
    .await?
    {
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
                "finalization deferred until the missing floor dependency is held"
            );
            None
        }
        crate::rust::finality::floor::FloorOfView::IncompatibilityHold { detail } => {
            tracing::debug!(
                detail,
                "finalization deferred for incompatible floor candidates"
            );
            None
        }
    };
    let mut new_finalized_hash_opt = None;
    if let Some(candidate) = candidate {
        let ft_value =
            crate::rust::safety::clique_oracle::CliqueOracle::normalized_fault_tolerance(
                &candidate.hash,
                &dag,
            )
            .await?;
        match new_lfb_found_effect((candidate.hash.clone(), ft_value)).await {
            Ok(()) => {
                divergence_monitor.on_advance();
                finalization_schedule.clear_parked_certificate_carrier(evaluation_revision);
                new_finalized_hash_opt = Some((candidate.hash, ft_value));
            }
            Err(KvStoreError::FinalizationCertificateCarrierPending {
                expected_revision, ..
            }) if expected_revision == evaluation_revision => {
                finalization_schedule.park_certificate_carrier(evaluation_revision);
                let current_head = wake_ctx.block_dag_storage.finalization_head()?;
                let base_changed = wake_ctx.block_dag_storage.current_generation()
                    != evaluation_generation
                    || current_head.as_ref().map(|head| head.revision) != Some(evaluation_revision);
                if base_changed
                    && finalization_schedule.take_parked_certificate_carrier(evaluation_revision)
                {
                    publish_finalization_request(wake_ctx, finalization_schedule.clone())?;
                }
            }
            Err(error) => return Err(CasperError::KvStoreError(error)),
        }
    };
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
    let dependency_ready = this.finalization_schedule.take_ready_missing_dependencies(
        this.block_dag_storage.current_generation(),
        &new_block.block_hash,
    );
    let parked_revision = if let (Some(head), Some(commitment)) = (
        this.block_dag_storage.finalization_head()?,
        new_block.header.finalized_floor.as_ref(),
    ) {
        let head_post_state = this
            .block_dag_storage
            .get_representation()?
            .lookup_unsafe(&head.block_hash.0)?
            .post_state_hash;
        (commitment.floor_hash == head.block_hash.0
            && commitment.floor_post_state_hash == head_post_state)
            .then_some(head.revision)
            .filter(|revision| {
                this.finalization_schedule
                    .take_parked_certificate_carrier(*revision)
            })
    } else {
        None
    };
    let due = finalization_due(
        this.casper_shard_conf.finalization_rate,
        new_block.body.state.block_number,
        this.recovery_sync_active
            .load(std::sync::atomic::Ordering::Acquire),
    );
    if due || parked_revision.is_some() || dependency_ready {
        if let Err(error) = request_finalization(this) {
            if let Some(revision) = parked_revision {
                this.finalization_schedule
                    .park_certificate_carrier(revision);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn finalization_due(finalization_rate: i32, block_number: i64, recovery_active: bool) -> bool {
    recovery_active || (finalization_rate > 0 && block_number % i64::from(finalization_rate) == 0)
}

pub(crate) fn request_finalization<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<(), CasperError> {
    publish_finalization_request(
        build_finalization_context(this),
        this.finalization_schedule.clone(),
    )
}

fn publish_finalization_request(
    ctx: FinalizationContext,
    schedule: Arc<FinalizationSchedule>,
) -> Result<(), CasperError> {
    let ticket = schedule.request().ok_or_else(|| {
        CasperError::RuntimeError("finalization request sequence exhausted".to_string())
    })?;
    start_finalization_dispatcher(ctx, schedule);
    tracing::debug!(ticket, "published finalization request");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    use block_storage::rust::finality::{FinalizationAppendOutcome, FinalizationLedger};
    use proptest::prelude::*;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rspace_plus_plus::rspace::rspace::RSpaceStore;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use shared::rust::store::key_value_store::KeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn effect_test_hash(value: u8) -> BlockHash {
        Bytes::from(vec![value; models::rust::block_hash::LENGTH])
    }

    async fn effect_entry_fixture() -> (
        FinalizationContext,
        FinalizationLedger,
        Arc<dyn KeyValueStore>,
    ) {
        let mut manager = InMemoryStoreManager::new();
        let block_dag_storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        let ledger = FinalizationLedger::create_from_kvm(&mut manager)
            .await
            .unwrap();
        ledger.ensure_genesis(effect_test_hash(0), 0).unwrap();
        let raw_ledger = manager
            .store(FinalizationLedger::STORE_NAME.to_string())
            .await
            .unwrap();
        let runtime_manager = RuntimeManager::create_with_store(
            RSpaceStore {
                history: Arc::new(InMemoryKeyValueStore::new()),
                roots: Arc::new(InMemoryKeyValueStore::new()),
                cold: Arc::new(InMemoryKeyValueStore::new()),
            },
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let ctx = FinalizationContext {
            block_dag_storage,
            block_store: KeyValueBlockStore::create_from_kvm(&mut manager)
                .await
                .unwrap(),
            deploy_storage: Arc::new(Mutex::new(
                KeyValueDeployStorage::new(&mut manager).await.unwrap(),
            )),
            rejected_deploy_buffer: Arc::new(std::sync::Mutex::new(
                KeyValueRejectedDeployBuffer::new(&mut manager)
                    .await
                    .unwrap(),
            )),
            deploy_lifecycle: Arc::new(Default::default()),
            runtime_manager: Arc::new(runtime_manager),
            event_publisher: F1r3flyEvents::new(),
            finalization_in_progress: Arc::new(AtomicU64::new(0)),
            enable_mergeable_channel_gc: false,
            protocol_version: 6,
            deploy_lifespan: 50,
            max_parent_depth: 100,
            shard_id: "root".to_string(),
            ftt: FtThreshold::from_ppm(100_000),
            finalization_schedule: Arc::new(FinalizationSchedule::new(2)),
            divergence_monitor: Arc::new(DivergenceMonitor::default()),
        };
        (ctx, ledger, raw_ledger)
    }

    fn append_effect_test_round(ledger: &FinalizationLedger) -> FinalizationRecord {
        let expected = ledger.head().unwrap().unwrap();
        let revision = expected.revision + 1;
        let hash = effect_test_hash(u8::try_from(revision).unwrap());
        let finalized = BTreeSet::from([BlockHashSerde(hash.clone())]);
        let mut supporting = finalized.clone();
        let carrier = if expected.revision == 0 {
            effect_test_hash(0)
        } else {
            supporting.insert(expected.block_hash.clone());
            expected.block_hash.0.clone()
        };
        let witness = FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            effect_test_hash(0),
            &expected,
            hash.clone(),
            expected.certificate_digest.0.clone(),
            carrier,
            i64::try_from(revision).unwrap(),
            effect_test_hash(200),
            1,
            1,
            BTreeMap::from([(
                ValidatorSerde(Bytes::from(vec![1; models::rust::validator::LENGTH])),
                BlockHashSerde(hash.clone()),
            )]),
            supporting,
            BlockHashSerde(effect_test_hash(201)),
            finalized.clone(),
        )
        .unwrap();
        ledger.persist_witness(&expected, &witness).unwrap();
        let record = FinalizationLedger::prepare_record(
            &expected,
            hash,
            i64::try_from(revision).unwrap(),
            1.0,
            finalized,
            &witness,
        )
        .unwrap();
        assert!(matches!(
            ledger.try_append(&expected, &record).unwrap(),
            FinalizationAppendOutcome::Committed(_)
        ));
        record
    }

    #[tokio::test]
    async fn effect_entry_rejects_append_after_projection_before_counter_or_block_access() {
        let (ctx, ledger, raw_ledger) = effect_entry_fixture().await;
        let mut captured = ctx
            .block_dag_storage
            .pending_finalization_effect_scan()
            .unwrap();
        let record = append_effect_test_round(&ledger);
        assert!(captured.next_record().unwrap().is_none());
        let finalized = HashSet::from([record.directly_finalized.0]);
        let before = raw_ledger.to_map().unwrap();

        for counter in [0, 1, u64::MAX] {
            ctx.finalization_in_progress
                .store(counter, Ordering::SeqCst);
            assert_eq!(
                apply_finalization_effects(&ctx, record.revision, &finalized).await,
                Err(KvStoreError::FinalizationProjectionPending {
                    revision: 1,
                    projected_revision: 0,
                })
            );
            assert_eq!(ctx.finalization_in_progress.load(Ordering::SeqCst), counter);
            assert_eq!(raw_ledger.to_map().unwrap(), before);
        }
        assert!(ctx
            .event_publisher
            .startup_buffer()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_empty());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn effect_entry_obeys_every_generated_projection_prefix(
            rounds in 1u8..12,
            requested in any::<u8>(),
            projected in any::<u8>(),
        ) {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async {
                let (ctx, ledger, raw_ledger) = effect_entry_fixture().await;
                for _ in 0..rounds {
                    append_effect_test_round(&ledger);
                }
                let requested = 1 + u64::from(requested % rounds);
                let projected = u64::from(projected % (rounds + 1));
                for revision in 1..=projected {
                    ledger.record_projection_completed(revision).unwrap();
                }
                let before = raw_ledger.to_map().unwrap();
                let finalized = HashSet::from([effect_test_hash(u8::try_from(requested).unwrap())]);
                let result = apply_finalization_effects(&ctx, requested, &finalized).await;
                if requested > projected {
                    prop_assert_eq!(result, Err(KvStoreError::FinalizationProjectionPending {
                        revision: requested,
                        projected_revision: projected,
                    }));
                } else {
                    prop_assert!(matches!(result, Err(KvStoreError::KeyNotFound(_))));
                }
                prop_assert_eq!(ctx.finalization_in_progress.load(Ordering::SeqCst), 0);
                prop_assert_eq!(raw_ledger.to_map().unwrap(), before);
                Ok(())
            })?;
        }
    }

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
    fn dependency_deferred_worker_neither_completes_nor_polls() {
        let schedule = FinalizationSchedule::new(2);
        assert_eq!(schedule.request(), Some(1));
        assert_eq!(schedule.next_coverage(), Some(1));
        schedule.mark_launched(1);
        assert_eq!(
            settle_finalization_worker(&schedule, 1, FinalizationWorkerOutcome::Deferred),
            None
        );
        assert!(!schedule.is_quiescent());
        assert_eq!(schedule.next_coverage(), None);
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
