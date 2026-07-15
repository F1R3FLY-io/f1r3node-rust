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

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, KeyValueDagRepresentation,
};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
// Phase 9 (A-3): deploy_storage uses parking_lot::Mutex.
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
use crate::rust::finality::finalizer::Finalizer;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub(crate) fn purge_finalized_deploys_from_buffer(
    buffer: &mut KeyValueRejectedDeployBuffer,
    deploy_sigs: &[Vec<u8>],
) {
    for sig in deploy_sigs {
        let _ = buffer.remove_by_sig(sig);
    }
}

fn lookup_deploy_by_sig(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    sig: &Bytes,
) -> Result<Option<Signed<DeployData>>, KvStoreError> {
    let Some(block_hash) = dag.lookup_by_deploy_id(&sig.to_vec())? else {
        return Ok(None);
    };
    let Some(block) = block_store.get(&block_hash)? else {
        return Ok(None);
    };
    Ok(block
        .body
        .deploys
        .iter()
        .find(|processed| processed.deploy.sig == *sig)
        .map(|processed| processed.deploy.clone()))
}

fn preferred_clean_event(
    clean_events: &[(i64, BlockHash, bool)],
    latest_rejection_height: Option<i64>,
) -> Option<(i64, BlockHash, bool)> {
    let clean_after_rejection = clean_events.iter().filter(|(height, _, _)| {
        latest_rejection_height
            .map(|reject_height| *height > reject_height)
            .unwrap_or(true)
    });

    if let Some(clean_canonical) = clean_after_rejection
        .clone()
        .filter(|(_, _, is_canonical)| *is_canonical)
        .cloned()
        .max_by(|(a_height, _, _), (b_height, _, _)| a_height.cmp(b_height))
    {
        return Some(clean_canonical);
    }

    clean_after_rejection.cloned().max_by(
        |(a_height, _, a_canonical), (b_height, _, b_canonical)| {
            a_canonical.cmp(b_canonical).then(a_height.cmp(b_height))
        },
    )
}

fn deploy_cleanup_sets_for_finalized_frontier(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    finalized_lfb: &BlockHash,
    finalized_set: &HashSet<BlockHash>,
) -> Result<(Vec<Signed<DeployData>>, Vec<Signed<DeployData>>), KvStoreError> {
    struct CleanupState {
        deploy: Signed<DeployData>,
        clean_events: Vec<(i64, BlockHash, bool)>,
        failed_event: Option<(i64, BlockHash)>,
        rejected_event: Option<(i64, BlockHash)>,
    }

    let mut included: HashMap<Bytes, Signed<DeployData>> = HashMap::new();
    let mut rejected_sigs: HashSet<Bytes> = HashSet::new();

    for block_hash in finalized_set {
        let block = block_store.get(block_hash)?.ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized block {} not present in store",
                PrettyPrinter::build_string_bytes(block_hash)
            ))
        })?;

        for processed in &block.body.deploys {
            let sig = processed.deploy.sig.clone();
            included
                .entry(sig.clone())
                .or_insert_with(|| processed.deploy.clone());
        }
        for rejected in &block.body.rejected_deploys {
            rejected_sigs.insert(rejected.sig.clone());
        }
    }

    for sig in rejected_sigs {
        if included.contains_key(&sig) {
            continue;
        }
        if let Some(deploy) = lookup_deploy_by_sig(dag, block_store, &sig)? {
            included.insert(sig, deploy);
        }
    }

    let mut states: HashMap<Bytes, CleanupState> = included
        .into_iter()
        .map(|(sig, deploy)| {
            (sig, CleanupState {
                deploy,
                clean_events: Vec::new(),
                failed_event: None,
                rejected_event: None,
            })
        })
        .collect();

    let lfb_height = dag.block_number(finalized_lfb).ok_or_else(|| {
        KvStoreError::KeyNotFound(format!(
            "finalized LFB {} has no block number",
            PrettyPrinter::build_string_bytes(finalized_lfb)
        ))
    })?;
    let scan_floor = states
        .values()
        .map(|state| state.deploy.data.valid_after_block_number.max(0))
        .min()
        .map(|oldest| oldest.min((lfb_height - deploy_lifespan).max(0)))
        .unwrap_or((lfb_height - deploy_lifespan).max(0));

    let mut lfb_ancestor_set: HashSet<BlockHash> = HashSet::new();
    let mut lfb_frontier: Vec<BlockHash> = vec![finalized_lfb.clone()];
    while let Some(block_hash) = lfb_frontier.pop() {
        if !lfb_ancestor_set.insert(block_hash.clone()) {
            continue;
        }
        let height = dag.block_number(&block_hash).ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized ancestor {} has no block number",
                PrettyPrinter::build_string_bytes(&block_hash)
            ))
        })?;
        if height < scan_floor {
            continue;
        }
        let block = block_store.get(&block_hash)?.ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized ancestor {} not present in store",
                PrettyPrinter::build_string_bytes(&block_hash)
            ))
        })?;
        for parent in &block.header.parents_hash_list {
            if !lfb_ancestor_set.contains(parent) {
                lfb_frontier.push(parent.clone());
            }
        }
    }

    let mut finalized_history: HashSet<BlockHash> =
        dag.finalized_blocks_set.iter().cloned().collect();
    finalized_history.extend(finalized_set.iter().cloned());
    for block_hash in finalized_history {
        if lfb_ancestor_set.contains(&block_hash) {
            continue;
        }
        let Some(height) = dag.block_number(&block_hash) else {
            continue;
        };
        if height < scan_floor {
            continue;
        }
        let Some(block) = block_store.get(&block_hash)? else {
            continue;
        };
        for processed in &block.body.deploys {
            if !processed.is_failed {
                if let Some(state) = states.get_mut(&processed.deploy.sig) {
                    state.clean_events.push((height, block_hash.clone(), true));
                }
            }
        }
    }

    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut frontier: Vec<BlockHash> = vec![finalized_lfb.clone()];
    while let Some(block_hash) = frontier.pop() {
        if !visited.insert(block_hash.clone()) {
            continue;
        }

        let height = dag.block_number(&block_hash).ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized ancestor {} has no block number",
                PrettyPrinter::build_string_bytes(&block_hash)
            ))
        })?;
        if height < scan_floor {
            continue;
        }

        let block = block_store.get(&block_hash)?.ok_or_else(|| {
            KvStoreError::KeyNotFound(format!(
                "finalized ancestor {} not present in store",
                PrettyPrinter::build_string_bytes(&block_hash)
            ))
        })?;
        for parent in &block.header.parents_hash_list {
            if !visited.contains(parent) {
                frontier.push(parent.clone());
            }
        }

        let is_canonical =
            block_hash == *finalized_lfb || dag.is_in_main_chain(&block_hash, finalized_lfb)?;
        for processed in &block.body.deploys {
            if let Some(state) = states.get_mut(&processed.deploy.sig) {
                if processed.is_failed {
                    if state
                        .failed_event
                        .as_ref()
                        .map(|(h, _)| height > *h)
                        .unwrap_or(true)
                    {
                        state.failed_event = Some((height, block_hash.clone()));
                    }
                } else {
                    state
                        .clean_events
                        .push((height, block_hash.clone(), is_canonical));
                }
            }
        }

        for rejected in &block.body.rejected_deploys {
            if let Some(state) = states.get_mut(&rejected.sig) {
                if state
                    .rejected_event
                    .as_ref()
                    .map(|(h, _)| height > *h)
                    .unwrap_or(true)
                {
                    state.rejected_event = Some((height, block_hash.clone()));
                }
            }
        }
    }

    let mut terminal = Vec::new();
    let mut recoverable = Vec::new();

    for state in states.into_values() {
        let canonical_block = |block: &BlockHash| -> Result<bool, KvStoreError> {
            Ok(block == finalized_lfb || dag.is_in_main_chain(block, finalized_lfb)?)
        };

        let latest_rejection_height = state.rejected_event.as_ref().map(|(height, _)| *height);
        let clean_canonical = preferred_clean_event(&state.clean_events, latest_rejection_height);

        let mut failed_canonical: Option<(i64, BlockHash)> = None;
        if let Some((failed_height, failed_block)) = &state.failed_event {
            if canonical_block(failed_block)? {
                let mut keep = true;
                if clean_canonical
                    .as_ref()
                    .map(|(_, _, clean_is_canonical)| *clean_is_canonical)
                    .unwrap_or(false)
                {
                    keep = false;
                }
                if latest_rejection_height
                    .map(|reject_height| *failed_height <= reject_height)
                    .unwrap_or(false)
                {
                    keep = false;
                }
                if keep {
                    failed_canonical = Some((*failed_height, failed_block.clone()));
                }
            }
        }

        let clean_finalized_height: Option<i64> = match (&clean_canonical, &failed_canonical) {
            (Some((ch, _, _)), Some((fh, _))) if ch > fh => Some(*ch),
            (Some((ch, _, _)), None) => Some(*ch),
            _ => None,
        };
        let failed_finalized: bool = match (&clean_canonical, &failed_canonical) {
            (Some((ch, _, _)), Some((fh, _))) => fh > ch,
            (None, Some(_)) => true,
            _ => false,
        };
        let expired = lfb_height > state.deploy.data.valid_after_block_number + deploy_lifespan
            && clean_finalized_height.is_none()
            && state.rejected_event.is_none();

        if failed_finalized || clean_finalized_height.is_some() || expired {
            terminal.push(state.deploy);
        } else {
            recoverable.push(state.deploy);
        }
    }

    Ok((terminal, recoverable))
}

// Phase 13 (TC-1): the previous `FINALIZER_BLOCKING_TIMEOUT = 15s`
// constant is now `CasperShardConf::finalizer_blocking_timeout`,
// passed in via `FinalizationContext::finalizer_blocking_timeout`.

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
    pub(crate) deploy_storage: Arc<Mutex<KeyValueDeployStorage>>,
    /// Held under `std::sync::Mutex` (see `types.rs`), NOT the parking_lot
    /// `Mutex` aliased in this module — accessed only synchronously.
    pub(crate) rejected_deploy_buffer: Arc<
        std::sync::Mutex<
            block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer,
        >,
    >,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicBool>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) ftt: FtThreshold,
    pub(crate) finalizer_conf: crate::rust::casper_conf::FinalizerConf,
    pub(crate) finalizer_blocking_timeout: std::time::Duration,
    pub(crate) deploy_lifespan: i64,
}

/// Build a `FinalizationContext` from a `MultiParentCasperImpl`. Single
/// source of truth for the 10-field clone — previously duplicated at
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
        runtime_manager: this.runtime_manager.clone(),
        event_publisher: this.event_publisher.clone(),
        finalization_in_progress: this.finalization_in_progress.clone(),
        enable_mergeable_channel_gc: this.casper_shard_conf.enable_mergeable_channel_gc,
        // Exact ppm from the shard conf — the source of truth for the DECISION.
        ftt: FtThreshold::from_ppm(this.casper_shard_conf.fault_tolerance_threshold_ppm),
        finalizer_conf: this.casper_shard_conf.finalizer_conf.clone(),
        finalizer_blocking_timeout: this.casper_shard_conf.finalizer_blocking_timeout,
        deploy_lifespan: this.casper_shard_conf.deploy_lifespan,
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

    let finalizer_blocking_timeout = ctx.finalizer_blocking_timeout;
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
        deploy_storage,
        rejected_deploy_buffer,
        runtime_manager,
        event_publisher,
        finalization_in_progress,
        enable_mergeable_channel_gc,
        ftt,
        finalizer_conf,
        finalizer_blocking_timeout: _,
        deploy_lifespan,
    } = ctx;
    let finalizer_conf = &finalizer_conf;
    let lfb_lookup_started = std::time::Instant::now();
    // Get current LFB hash and height
    let dag = block_dag_storage.get_representation()?;
    let last_finalized_block_hash = dag.last_finalized_block();
    let last_finalized_block_height = dag.lookup_unsafe(&last_finalized_block_hash)?.block_number;

    // Keep effect closure FnMut-compatible by cloning captured state on each invocation.
    let block_dag_storage_for_effect = block_dag_storage.clone();
    let block_store_for_effect = block_store.clone();
    let deploy_storage_for_effect = deploy_storage.clone();
    let rejected_deploy_buffer_for_effect = rejected_deploy_buffer.clone();
    let runtime_manager_for_effect = runtime_manager.clone();
    let event_publisher_for_effect = event_publisher.clone();
    let finalization_in_progress_for_effect = finalization_in_progress.clone();

    // Create simple finalization effect closure
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let block_dag_storage = block_dag_storage_for_effect.clone();
        let block_store = block_store_for_effect.clone();
        let deploy_storage = deploy_storage_for_effect.clone();
        let rejected_deploy_buffer = rejected_deploy_buffer_for_effect.clone();
        let runtime_manager = runtime_manager_for_effect.clone();
        let event_publisher = event_publisher_for_effect.clone();
        let finalization_in_progress = finalization_in_progress_for_effect.clone();
        async move {
            let effect_started = std::time::Instant::now();
            let block_dag_storage_for_callbacks = block_dag_storage.clone();
            let finalized_lfb_for_cleanup = new_lfb.clone();
            block_dag_storage
                .record_directly_finalized(new_lfb.clone(), ft_value, move |finalized_set: &HashSet<BlockHash>| {
                    let finalized_set = finalized_set.clone();
                    let block_store = block_store.clone();
                    let deploy_storage = deploy_storage.clone();
                    let rejected_deploy_buffer = rejected_deploy_buffer.clone();
                    let runtime_manager = runtime_manager.clone();
                    let event_publisher = event_publisher.clone();
                    let finalization_in_progress = finalization_in_progress.clone();
                    let block_dag_storage_for_callback = block_dag_storage_for_callbacks.clone();
                    let finalized_lfb_for_cleanup = finalized_lfb_for_cleanup.clone();
                    Box::pin(async move {
                        let process_finalized_started = std::time::Instant::now();
                        // Use RAII guard to ensure flag is reset even if we return early or panic
                        finalization_in_progress.store(true, Ordering::SeqCst);
                        let _guard = FinalizationGuard(finalization_in_progress.as_ref());
                        tracing::debug!("Finalization started for {} blocks", finalized_set.len());

                        let dag_for_cleanup = block_dag_storage_for_callback.get_representation()?;
                        let (terminal_deploys, recoverable_deploys) =
                            deploy_cleanup_sets_for_finalized_frontier(
                                &dag_for_cleanup,
                                &block_store,
                                deploy_lifespan,
                                &finalized_lfb_for_cleanup,
                                &finalized_set,
                            )?;
                        let mut deploys_to_remove = terminal_deploys.clone();
                        deploys_to_remove.extend(recoverable_deploys.clone());
                        if !deploys_to_remove.is_empty() {
                            deploy_storage.lock().remove(deploys_to_remove)?;
                        }
                        {
                            let mut buffer_guard =
                                rejected_deploy_buffer.lock().map_err(|_| {
                                    KvStoreError::LockError(
                                        "Failed to acquire rejected_deploy_buffer lock".to_string(),
                                    )
                                })?;
                            let terminal_sigs: Vec<Vec<u8>> =
                                terminal_deploys.iter().map(|d| d.sig.to_vec()).collect();
                            purge_finalized_deploys_from_buffer(
                                &mut *buffer_guard,
                                &terminal_sigs,
                            );
                            if !recoverable_deploys.is_empty() {
                                buffer_guard.add(recoverable_deploys.clone())?;
                            }
                        }
                        tracing::info!(
                            target: "f1r3fly.casper.recovery",
                            "Finalization deploy cleanup: terminal={}, recoverable={}",
                            terminal_deploys.len(),
                            recoverable_deploys.len()
                        );

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
                            let deploys_count = block.body.deploys.len();
                            let finalized_set_str = PrettyPrinter::build_string_hashes(
                                &finalized_set.iter().map(|h| h.to_vec()).collect::<Vec<_>>(),
                            );
                            let removed_deploy_msg = format!(
                                "Observed {} deploys while finalizing block {}.",
                                deploys_count, finalized_set_str
                            );
                            tracing::info!("{}", removed_deploy_msg);

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

                            // Publish BlockFinalised event for each newly finalized block
                            event_publisher
                                .publish(finalised_event(&block))
                                .map_err(|e| KvStoreError::IoError(e.to_string()))?;
                        }

                        // Guard will reset finalization_in_progress flag on drop
                        tracing::debug!("Finalization completed");
                        tracing::debug!(
                            target: "f1r3fly.finalizer.effect.timing",
                            "Finalization effect timing: finalized_blocks={}, process_finalized_ms={}",
                            finalized_set.len(),
                            process_finalized_started.elapsed().as_millis()
                        );

                        Ok(())
                    })
                })
                .await?;
            tracing::debug!(
                target: "f1r3fly.finalizer.effect.timing",
                "record_directly_finalized_total_ms={}",
                effect_started.elapsed().as_millis()
            );
            Ok(())
        }
    };

    // Run finalizer
    let finalizer_started = std::time::Instant::now();
    let new_finalized_hash_opt = Finalizer::run(
        &dag,
        ftt,
        last_finalized_block_height,
        new_lfb_found_effect,
        finalizer_conf,
    )
    .await
    .map_err(CasperError::KvStoreError)?;
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
        target: "f1r3fly.last_finalized_block.timing",
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
mod tests {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;
    use crate::rust::util::construct_deploy;

    #[tokio::test]
    async fn purge_removes_terminal_deploys_and_keeps_recoverable_deploys() {
        let mut kvm = InMemoryStoreManager::new();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("in-memory rejected-deploy buffer");

        let terminal =
            construct_deploy::source_deploy("@1!(1)".to_string(), 1, None, None, None, None, None)
                .expect("terminal deploy");
        let recoverable =
            construct_deploy::source_deploy("@2!(2)".to_string(), 2, None, None, None, None, None)
                .expect("recoverable deploy");
        let survivor =
            construct_deploy::source_deploy("@3!(3)".to_string(), 3, None, None, None, None, None)
                .expect("survivor deploy");

        buffer
            .add(vec![
                terminal.clone(),
                recoverable.clone(),
                survivor.clone(),
            ])
            .expect("seed buffer");
        assert!(
            buffer.contains_sig(&terminal.sig).expect("contains"),
            "terminal seeded"
        );
        assert!(
            buffer.contains_sig(&recoverable.sig).expect("contains"),
            "recoverable seeded"
        );
        assert!(
            buffer.contains_sig(&survivor.sig).expect("contains"),
            "survivor seeded"
        );

        let terminal_sigs: Vec<Vec<u8>> = vec![terminal.sig.to_vec()];
        purge_finalized_deploys_from_buffer(&mut buffer, &terminal_sigs);

        assert!(
            !buffer.contains_sig(&terminal.sig).expect("contains"),
            "a terminal deploy must be purged from the buffer"
        );
        assert!(
            buffer.contains_sig(&recoverable.sig).expect("contains"),
            "a recoverable merge-rejected deploy must remain available for re-proposal"
        );
        assert!(
            buffer.contains_sig(&survivor.sig).expect("contains"),
            "an unrelated buffered deploy must survive the finalization purge"
        );
    }

    #[tokio::test]
    async fn cleanup_uses_new_lfb_before_persisted_lfb_moves() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy = construct_deploy::source_deploy(
            "@1!(1)".to_string(),
            1,
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let child = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store.put_block_message(&child).expect("store child");
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&child, InsertMode::Normal)
            .expect("insert child");

        let dag = dag_storage.get_representation().expect("dag");
        assert_eq!(dag.last_finalized_block(), genesis.block_hash);

        let finalized_set: HashSet<BlockHash> = HashSet::from([child.block_hash.clone()]);
        let (terminal, recoverable) = deploy_cleanup_sets_for_finalized_frontier(
            &dag,
            &block_store,
            50,
            &child.block_hash,
            &finalized_set,
        )
        .expect("cleanup sets");

        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].sig, deploy.sig);
        assert!(recoverable.is_empty());
    }

    #[tokio::test]
    async fn cleanup_recovers_deploy_when_rejection_finalizes_after_clean_block() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy = construct_deploy::source_deploy(
            "@2!(2)".to_string(),
            1,
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let main = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let clean = block_implicits::get_random_block(
            Some(1),
            Some(2),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let mut rejection = block_implicits::get_random_block(
            Some(2),
            Some(1),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![main.block_hash.clone(), clean.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        rejection.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy.sig.clone(),
        }];
        let tail_1 = block_implicits::get_random_block(
            Some(3),
            Some(1),
            None,
            None,
            None,
            None,
            Some(3),
            Some(vec![rejection.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let tail_2 = block_implicits::get_random_block(
            Some(4),
            Some(1),
            None,
            None,
            None,
            None,
            Some(4),
            Some(vec![tail_1.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store.put_block_message(&main).expect("store main");
        block_store.put_block_message(&clean).expect("store clean");
        block_store
            .put_block_message(&rejection)
            .expect("store rejection");
        block_store
            .put_block_message(&tail_1)
            .expect("store tail 1");
        block_store
            .put_block_message(&tail_2)
            .expect("store tail 2");
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&main, InsertMode::Normal)
            .expect("insert main");
        dag_storage
            .insert(&clean, InsertMode::Normal)
            .expect("insert clean");
        dag_storage
            .insert(&rejection, InsertMode::Normal)
            .expect("insert rejection");
        dag_storage
            .insert(&tail_1, InsertMode::Normal)
            .expect("insert tail 1");
        dag_storage
            .insert(&tail_2, InsertMode::Normal)
            .expect("insert tail 2");
        dag_storage
            .record_directly_finalized(clean.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("finalize clean");

        let dag = dag_storage.get_representation().expect("dag");
        let finalized_set: HashSet<BlockHash> = HashSet::from([
            rejection.block_hash.clone(),
            tail_1.block_hash.clone(),
            tail_2.block_hash.clone(),
        ]);
        let (terminal, recoverable) = deploy_cleanup_sets_for_finalized_frontier(
            &dag,
            &block_store,
            1,
            &tail_2.block_hash,
            &finalized_set,
        )
        .expect("cleanup sets");

        assert!(terminal.is_empty());
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].sig, deploy.sig);
    }

    #[tokio::test]
    async fn cleanup_recovers_previously_finalized_clean_when_later_rejected() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy = construct_deploy::source_deploy(
            "@4!(4)".to_string(),
            1,
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let clean = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let main = block_implicits::get_random_block(
            Some(1),
            Some(2),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let mut rejection = block_implicits::get_random_block(
            Some(2),
            Some(2),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![main.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        rejection.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy.sig.clone(),
        }];

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store.put_block_message(&clean).expect("store clean");
        block_store.put_block_message(&main).expect("store main");
        block_store
            .put_block_message(&rejection)
            .expect("store rejection");
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&clean, InsertMode::Normal)
            .expect("insert clean");
        dag_storage
            .insert(&main, InsertMode::Normal)
            .expect("insert main");
        dag_storage
            .insert(&rejection, InsertMode::Normal)
            .expect("insert rejection");
        dag_storage
            .record_directly_finalized(clean.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("finalize clean");

        let dag = dag_storage.get_representation().expect("dag");
        let finalized_set: HashSet<BlockHash> = HashSet::from([rejection.block_hash.clone()]);
        let (terminal, recoverable) = deploy_cleanup_sets_for_finalized_frontier(
            &dag,
            &block_store,
            50,
            &rejection.block_hash,
            &finalized_set,
        )
        .expect("cleanup sets");

        assert!(terminal.is_empty());
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].sig, deploy.sig);
    }

    #[tokio::test]
    async fn cleanup_recovers_canonical_clean_when_duplicate_rejected() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy = construct_deploy::source_deploy(
            "@3!(3)".to_string(),
            1,
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let clean_main = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let empty_main = block_implicits::get_random_block(
            Some(2),
            Some(1),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![clean_main.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let duplicate_clean = block_implicits::get_random_block(
            Some(2),
            Some(2),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![clean_main.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let mut rejection = block_implicits::get_random_block(
            Some(3),
            Some(1),
            None,
            None,
            None,
            None,
            Some(3),
            Some(vec![
                empty_main.block_hash.clone(),
                duplicate_clean.block_hash.clone(),
            ]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        rejection.body.rejected_deploys = vec![RejectedDeploy {
            sig: deploy.sig.clone(),
        }];

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store
            .put_block_message(&clean_main)
            .expect("store clean main");
        block_store
            .put_block_message(&empty_main)
            .expect("store empty main");
        block_store
            .put_block_message(&duplicate_clean)
            .expect("store duplicate clean");
        block_store
            .put_block_message(&rejection)
            .expect("store rejection");
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&clean_main, InsertMode::Normal)
            .expect("insert clean main");
        dag_storage
            .insert(&empty_main, InsertMode::Normal)
            .expect("insert empty main");
        dag_storage
            .insert(&duplicate_clean, InsertMode::Normal)
            .expect("insert duplicate clean");
        dag_storage
            .insert(&rejection, InsertMode::Normal)
            .expect("insert rejection");

        let dag = dag_storage.get_representation().expect("dag");
        let finalized_set: HashSet<BlockHash> = HashSet::from([rejection.block_hash.clone()]);
        let (terminal, recoverable) = deploy_cleanup_sets_for_finalized_frontier(
            &dag,
            &block_store,
            50,
            &rejection.block_hash,
            &finalized_set,
        )
        .expect("cleanup sets");

        assert!(terminal.is_empty());
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].sig, deploy.sig);
    }
}
