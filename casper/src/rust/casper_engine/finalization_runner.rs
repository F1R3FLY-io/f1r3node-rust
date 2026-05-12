//! Finalization runner — background task, RAII guard,
//! `compute_last_finalized_block`, `update_last_finalized_block`.
//!
//! Phase 3 (Commit 2): extracted from `multi_parent_casper_impl.rs`.
//! The functions here are reachable via:
//!   * `MultiParentCasper::last_finalized_block` (mod.rs) →
//!     `compute_last_finalized_block` (here)
//!   * `block_admission::admit_handle_valid_block` (block_admission.rs) →
//!     `self.update_last_finalized_block` (inherent method here)
//!   * background task spawned by `update_last_finalized_block` →
//!     `run_queued_finalizer` → `compute_last_finalized_block`

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Phase 9 (A-3): deploy_storage uses parking_lot::Mutex.
use parking_lot::Mutex;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::errors::CasperError;
use crate::rust::finality::finalizer::Finalizer;
// Phase 7 (C-3): import the struct from its canonical sibling module
// instead of via the legacy shim — the previous import formed a circular
// path `casper_engine → multi_parent_casper_impl → casper_engine::types`.
use super::events::finalised_event;
use super::types::MultiParentCasperImpl;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

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
    pub(crate) runtime_manager: Arc<tokio::sync::Mutex<RuntimeManager>>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicBool>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) fault_tolerance_threshold: f32,
    pub(crate) finalizer_conf: crate::rust::casper_conf::FinalizerConf,
    pub(crate) finalizer_blocking_timeout: std::time::Duration,
}

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
        runtime_manager,
        event_publisher,
        finalization_in_progress,
        enable_mergeable_channel_gc,
        fault_tolerance_threshold,
        finalizer_conf,
        finalizer_blocking_timeout: _,
    } = ctx;
    let finalizer_conf = &finalizer_conf;
    let lfb_lookup_started = std::time::Instant::now();
    // Get current LFB hash and height
    let dag = block_dag_storage
        .get_representation()
        .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
    let last_finalized_block_hash = dag.last_finalized_block();
    let last_finalized_block_height = dag.lookup_unsafe(&last_finalized_block_hash)?.block_number;

    // Keep effect closure FnMut-compatible by cloning captured state on each invocation.
    let block_dag_storage_for_effect = block_dag_storage.clone();
    let block_store_for_effect = block_store.clone();
    let deploy_storage_for_effect = deploy_storage.clone();
    let runtime_manager_for_effect = runtime_manager.clone();
    let event_publisher_for_effect = event_publisher.clone();
    let finalization_in_progress_for_effect = finalization_in_progress.clone();

    // Create simple finalization effect closure
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let block_dag_storage = block_dag_storage_for_effect.clone();
        let block_store = block_store_for_effect.clone();
        let deploy_storage = deploy_storage_for_effect.clone();
        let runtime_manager = runtime_manager_for_effect.clone();
        let event_publisher = event_publisher_for_effect.clone();
        let finalization_in_progress = finalization_in_progress_for_effect.clone();
        async move {
            let effect_started = std::time::Instant::now();
            block_dag_storage
                .record_directly_finalized(new_lfb.clone(), ft_value, |finalized_set: &HashSet<BlockHash>| {
                    let finalized_set = finalized_set.clone();
                    let block_store = block_store.clone();
                    let deploy_storage = deploy_storage.clone();
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
                            let deploys: Vec<_> = block
                                .body
                                .deploys
                                .iter()
                                .map(|pd| pd.deploy.clone())
                                .collect();

                            // Remove block deploys from persistent store.
                            // Phase 9 (A-3): parking_lot::Mutex — no poison.
                            let deploys_count = deploys.len();
                            deploy_storage.lock().remove(deploys)?;
                            let finalized_set_str = PrettyPrinter::build_string_hashes(
                                &finalized_set.iter().map(|h| h.to_vec()).collect::<Vec<_>>(),
                            );
                            let removed_deploy_msg = format!(
                                "Removed {} deploys from deploy history as we finalized block {}.",
                                deploys_count, finalized_set_str
                            );
                            tracing::info!("{}", removed_deploy_msg);

                            // Remove block index from cache
                            runtime_manager
                                .lock()
                                .await
                                .remove_block_index_cache(block_hash);

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
        fault_tolerance_threshold,
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

impl<T: TransportLayer + Send + Sync> MultiParentCasperImpl<T> {
    /// P2-15 / Phase 3: inherent method invoked by
    /// `block_admission::admit_handle_valid_block` (cross-sub-module call).
    /// Promoted to `pub(crate)` for the cross-sub-module access.
    pub(crate) async fn update_last_finalized_block(
        &self,
        new_block: &BlockMessage,
    ) -> Result<(), CasperError> {
        if self.casper_shard_conf.finalization_rate <= 0 {
            return Ok(());
        }

        if new_block.body.state.block_number % self.casper_shard_conf.finalization_rate as i64 == 0
        {
            if self
                .finalizer_task_in_progress
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                if !self.finalizer_task_queued.swap(true, Ordering::SeqCst) {
                    tracing::debug!("Finalizer already running; queued follow-up finalization run");
                }
                return Ok(());
            }

            let ctx = FinalizationContext {
                block_dag_storage: self.block_dag_storage.clone(),
                block_store: self.block_store.clone(),
                deploy_storage: self.deploy_storage.clone(),
                runtime_manager: self.runtime_manager.clone(),
                event_publisher: self.event_publisher.clone(),
                finalization_in_progress: self.finalization_in_progress.clone(),
                enable_mergeable_channel_gc: self.casper_shard_conf.enable_mergeable_channel_gc,
                fault_tolerance_threshold: self.casper_shard_conf.fault_tolerance_threshold,
                finalizer_conf: self.casper_shard_conf.finalizer_conf.clone(),
                finalizer_blocking_timeout: self.casper_shard_conf.finalizer_blocking_timeout,
            };
            let finalizer_task_in_progress = self.finalizer_task_in_progress.clone();
            let finalizer_task_queued = self.finalizer_task_queued.clone();

            tokio::spawn(async move {
                run_queued_finalizer(ctx, finalizer_task_in_progress, finalizer_task_queued).await;
            });
        }
        Ok(())
    }
}
