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

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
// DISABLED (2026-08-07 dev merge) — only the commented-out purge used this:
// use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
// DISABLED (2026-08-07 dev merge): `RejectedDeploy` was used only by the
// commented-out purge fn.
use models::rust::casper::protocol::casper_message::BlockMessage;
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
use crate::rust::finality::finalizer::Finalizer;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

// DISABLED (2026-08-07 dev merge) — finalization-time rejected-buffer purge.
//
// This purge (and every handle/plumbing line feeding it, all commented out
// below with this same tag) was restored on this branch when the DL-1 hazard
// was understood against the OLD recovery design. The 2026-07-15 dev-merge
// measurement (see the module header above, "DO NOT re-add …") showed that
// under the record-driven recovery design adopted from dev, the purge is
// actively harmful: it evicts the round-0 keep-one LOSERS from the buffer
// before recovery can re-propose them, losing their writes permanently
// (`three_writers_converge_under_load`: purge present ⇒ FAILS, purge absent
// ⇒ PASSES). The DL-1 double-apply hazard this purge guarded is enforced at
// ADMISSION instead (`block_creator` `canonical_won_sigs` + `remove_by_sig`
// + the `rejected_in_scope` exemption, pinned by
// `interpreter_util::backstop_tests`). Kept commented out, not deleted, per
// the repo's disable-by-commenting rule; re-enabling requires re-running the
// measurement in the module header first.
//
// /// Purge a finalized block's deploys from the per-node rejected-deploy buffer: BOTH the
// /// block's own included deploys (`deploy_sigs`) AND every signature it lists in
// /// `rejected_deploys`. The rejected ones are definitively lost to the canonical chain and
// /// must never be re-proposed from this node's buffer; the included ones have landed
// /// canonically and no longer need re-proposal.
// pub(crate) fn purge_finalized_deploys_from_buffer(
//     buffer: &mut KeyValueRejectedDeployBuffer,
//     deploy_sigs: &[Vec<u8>],
//     rejected_deploys: &[RejectedDeploy],
// ) {
//     for sig in deploy_sigs {
//         let _ = buffer.remove_by_sig(sig);
//     }
//     for rd in rejected_deploys {
//         let _ = buffer.remove_by_sig(&rd.sig);
//     }
// }

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
    // DISABLED (2026-08-07 dev merge) — the buffer handle existed only to feed
    // the finalization-time purge (see the module header + the commented-out
    // `purge_finalized_deploys_from_buffer` above). Deliberately absent per
    // dev's measured design: the buffer must SURVIVE finalization so
    // record-driven recovery can re-propose keep-one losers.
    // pub(crate) rejected_deploy_buffer: Arc<
    //     std::sync::Mutex<
    //         block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer,
    //     >,
    // >,
    pub(crate) runtime_manager: Arc<RuntimeManager>,
    pub(crate) event_publisher: F1r3flyEvents,
    pub(crate) finalization_in_progress: Arc<AtomicBool>,
    pub(crate) enable_mergeable_channel_gc: bool,
    pub(crate) ftt: FtThreshold,
    pub(crate) finalizer_conf: crate::rust::casper_conf::FinalizerConf,
    pub(crate) finalizer_blocking_timeout: std::time::Duration,
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
        pending_cosigner_metadata: this.pending_cosigner_metadata.clone(),
        // DISABLED (2026-08-07 dev merge): no buffer handle — see FinalizationContext.
        // rejected_deploy_buffer: this.rejected_deploy_buffer.clone(),
        runtime_manager: this.runtime_manager.clone(),
        event_publisher: this.event_publisher.clone(),
        finalization_in_progress: this.finalization_in_progress.clone(),
        enable_mergeable_channel_gc: this.casper_shard_conf.enable_mergeable_channel_gc,
        // Exact ppm from the shard conf — the source of truth for the DECISION.
        ftt: FtThreshold::from_ppm(this.casper_shard_conf.fault_tolerance_threshold_ppm),
        finalizer_conf: this.casper_shard_conf.finalizer_conf.clone(),
        finalizer_blocking_timeout: this.casper_shard_conf.finalizer_blocking_timeout,
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
        pending_cosigner_metadata,
        // DISABLED (2026-08-07 dev merge): rejected_deploy_buffer removed from the context.
        runtime_manager,
        event_publisher,
        finalization_in_progress,
        enable_mergeable_channel_gc,
        ftt,
        finalizer_conf,
        finalizer_blocking_timeout: _,
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
    let pending_cosigner_metadata_for_effect = pending_cosigner_metadata.clone();
    // DISABLED (2026-08-07 dev merge): buffer clone fed only the purge.
    // let rejected_deploy_buffer_for_effect = rejected_deploy_buffer.clone();
    let runtime_manager_for_effect = runtime_manager.clone();
    let event_publisher_for_effect = event_publisher.clone();
    let finalization_in_progress_for_effect = finalization_in_progress.clone();

    // Create simple finalization effect closure
    let new_lfb_found_effect = move |(new_lfb, ft_value): (BlockHash, f32)| {
        let block_dag_storage = block_dag_storage_for_effect.clone();
        let block_store = block_store_for_effect.clone();
        let deploy_storage = deploy_storage_for_effect.clone();
        let pending_cosigner_metadata = pending_cosigner_metadata_for_effect.clone();
        // DISABLED (2026-08-07 dev merge): buffer clone fed only the purge.
        // let rejected_deploy_buffer = rejected_deploy_buffer_for_effect.clone();
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
                    let pending_cosigner_metadata = pending_cosigner_metadata.clone();
                    // DISABLED (2026-08-07 dev merge): buffer clone fed only the purge.
                    // let rejected_deploy_buffer = rejected_deploy_buffer.clone();
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
                            let deploy_sigs_for_buffer: Vec<Vec<u8>> =
                                deploys.iter().map(|d| d.sig.to_vec()).collect();
                            deploy_storage.lock().remove(deploys)?;

                            // Drain the cosigner-metadata sidecar in lockstep
                            // with `deploy_storage` (both keyed by primary
                            // signature). Relocated here from the acceptance-time
                            // purge so the sidecar stays bounded under sealed-floor
                            // record-driven recovery: deploys are retained through
                            // accept and removed only once finalized, so their
                            // compound-deploy metadata must be released here too.
                            {
                                let mut sidecar = pending_cosigner_metadata.lock();
                                for sig in &deploy_sigs_for_buffer {
                                    sidecar.remove(&prost::bytes::Bytes::from(sig.clone()));
                                }
                            }

                            // DISABLED (2026-08-07 dev merge) — the buffer purge
                            // that used to run here. The prior rationale ("the
                            // casper_engine split dropped this purge … IntegerAdd
                            // invariant violation") was true against the OLD
                            // recovery design; under dev's record-driven recovery
                            // (adopted by this merge) the purge is MEASURED
                            // harmful — see the module header: it evicts keep-one
                            // losers before re-proposal, and DL-1 is enforced at
                            // admission instead.
                            //
                            // {
                            //     let mut buffer_guard =
                            //         rejected_deploy_buffer.lock().map_err(|_| {
                            //             KvStoreError::LockError(
                            //                 "Failed to acquire rejected_deploy_buffer lock"
                            //                     .to_string(),
                            //             )
                            //         })?;
                            //     purge_finalized_deploys_from_buffer(
                            //         &mut *buffer_guard,
                            //         &deploy_sigs_for_buffer,
                            //         &block.body.rejected_deploys,
                            //     );
                            // }
                            let finalized_set_str = PrettyPrinter::build_string_hashes(
                                &finalized_set.iter().map(|h| h.to_vec()).collect::<Vec<_>>(),
                            );
                            let removed_deploy_msg = format!(
                                "Removed {} deploys from deploy history as we finalized block {}.",
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

    // Run finalizer
    let finalizer_started = std::time::Instant::now();
    let new_finalized_hash_opt = Finalizer::run(
        &dag,
        ftt,
        &last_finalized_block_hash,
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

// DISABLED (2026-08-07 dev merge) — the DL-1 purge unit test, commented out
// together with `purge_finalized_deploys_from_buffer` (see the rationale at
// the commented-out fn above and the module header). The DL-1 contract is
// now pinned admission-side by `interpreter_util::backstop_tests`, which the
// deploy-lifecycle gate runs.
// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::rust::util::construct_deploy;
//     use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
//
//     /// DL-1 (deploy-lifecycle finalization purge). Finalizing a block must remove from THIS
//     /// node's rejected-deploy buffer BOTH the deploys the block INCLUDED (they landed
//     /// canonically) AND the deploys it listed as REJECTED (definitively lost) — while
//     /// leaving unrelated buffered deploys intact for later re-proposal. This is exactly the
//     /// purge the casper_engine split dropped, which re-proposed already-finalized deploys and
//     /// double-applied them (the `three_writers_converge_under_load` regression). Previously
//     /// covered only indirectly through that convergence test; this pins the contract directly.
//     #[tokio::test]
//     async fn purge_removes_included_and_rejected_deploys_and_keeps_others() {
//         let mut kvm = InMemoryStoreManager::new();
//         let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm)
//             .await
//             .expect("in-memory rejected-deploy buffer");
//
//         // Three distinct deploys — distinct terms + timestamps ⇒ distinct signatures.
//         let included =
//             construct_deploy::source_deploy("@1!(1)".to_string(), 1, None, None, None, None, None)
//                 .expect("included deploy");
//         let rejected =
//             construct_deploy::source_deploy("@2!(2)".to_string(), 2, None, None, None, None, None)
//                 .expect("rejected deploy");
//         let survivor =
//             construct_deploy::source_deploy("@3!(3)".to_string(), 3, None, None, None, None, None)
//                 .expect("survivor deploy");
//
//         buffer
//             .add(vec![included.clone(), rejected.clone(), survivor.clone()])
//             .expect("seed buffer");
//         assert!(buffer.contains_sig(&included.sig).expect("contains"), "included seeded");
//         assert!(buffer.contains_sig(&rejected.sig).expect("contains"), "rejected seeded");
//         assert!(buffer.contains_sig(&survivor.sig).expect("contains"), "survivor seeded");
//
//         // The finalized block INCLUDED `included` and lists `rejected` in body.rejected_deploys.
//         let included_sigs: Vec<Vec<u8>> = vec![included.sig.to_vec()];
//         let rejected_deploys = vec![RejectedDeploy { sig: rejected.sig.clone() }];
//
//         purge_finalized_deploys_from_buffer(&mut buffer, &included_sigs, &rejected_deploys);
//
//         assert!(
//             !buffer.contains_sig(&included.sig).expect("contains"),
//             "an included (now-finalized) deploy must be purged from the buffer"
//         );
//         assert!(
//             !buffer.contains_sig(&rejected.sig).expect("contains"),
//             "a body.rejected_deploys sig must be purged (definitively lost, never re-proposed)"
//         );
//         assert!(
//             buffer.contains_sig(&survivor.sig).expect("contains"),
//             "an unrelated buffered deploy must survive the finalization purge (still re-proposable)"
//         );
//     }
// }
//
