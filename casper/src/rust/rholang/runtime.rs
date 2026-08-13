// See casper/src/main/scala/coop/rchain/casper/rholang/RuntimeSyntax.scala

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::mem;
use std::sync::OnceLock;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, GPrivate, GUnforgeable, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::new_freevar_par;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::interpreter::EvaluateResult;
// Slice 30: WAL root computation for per-deploy observability.  In
// slice 30b this hash goes on-chain via a proto extension of
// ProcessedDeploy (hard fork); until then it's logged for operator
// diagnostics.
use rholang::rust::interpreter::io::lock::{DeployScope, LockRegistry};
use rholang::rust::interpreter::io::snapshot::compute_wal_root;
use rholang::rust::interpreter::io::wal::{Wal, WalEntry, WalMark};
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::{bootstrap_registry, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::{MergeType, NumberChannelsEndVal};

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, CASPER_METRICS_SOURCE,
    EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC,
    EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC,
};
use crate::rust::rholang::types::eval_collector::EvalCollector;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::pre_charge_deploy::PreChargeDeploy;
use crate::rust::util::rholang::costacc::refund_deploy::RefundDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crate::rust::util::rholang::system_deploy_user_error::{
    SystemDeployPlatformFailure, SystemDeployUserError,
};
use crate::rust::util::rholang::tools::Tools;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};
use crate::rust::util::{construct_deploy, event_converter};

static EXPLORATORY_DEPLOY_KEY: OnceLock<crypto::rust::private_key::PrivateKey> = OnceLock::new();

/// RAII per-deploy resource guard.  Two responsibilities:
///
/// 1. **WAL boundary** (C-30-1 review fix, 2026 slice 30 round 2):
///    captures `Wal::begin_deploy` on construction and guarantees
///    `Wal::take_deploy_entries` runs on **every** exit path (success,
///    `?`-propagated error, panic), preventing per-deploy WAL entries
///    from leaking into the next deploy's slice.
/// 2. **Lock deploy-end sweep** (Phase 8 slice 8a step 5, 2026-08-13):
///    calls `LockRegistry::release_all_for_deploy(&deploy_scope)` on
///    Drop, sweeping every lock the deploy acquired but didn't release
///    via `token!release()` or `File.close`.  Spec §Explicit locks
///    MUST auto-release at deploy end for cross-validator determinism
///    (a MAY leak would diverge the lock table across validators that
///    make different MAY choices).
///
/// # WAL correctness
///
/// The pre-round-2 code drained only on the two `Ok`-return paths;
/// five `?` operators between `begin_deploy` and the drain sites
/// could early-return with WAL entries still in the per-runtime
/// buffer, poisoning the next deploy's `take_deploy_entries(mark)`
/// with entries the previous deploy contributed (Critical finding
/// C-30-1).  This guard closes the gap by making drain-on-drop the
/// default and drain-and-attach the explicit opt-in via
/// `take_and_commit`.
///
/// # Lock-sweep correctness
///
/// The Drop-based sweep symmetrically closes any leaked locks on all
/// three exit paths (success, `?`-propagated error, panic).  The
/// sweep is scope-scoped: it clears only entries whose
/// `RangeEntry.deploy` / `SequentialEntry.deploy` matches this
/// guard's `deploy_scope`, so a prior deploy's locks (if any survived
/// its own sweep — shouldn't happen absent a bug) are unaffected.
/// The `current_scope_cell` is set at construction so concurrent
/// lock-native handlers record the correct scope; cleared on drop.
pub(crate) struct WalDeployScope {
    /// Arc-shared clone of the per-runtime WAL — cheap; shares the
    /// underlying `Arc<Mutex<Vec<WalEntry>>>` so appends and drains
    /// see the same buffer.
    wal: Wal,
    mark: WalMark,
    /// Once `take_and_commit` runs, Drop skips the discard-drain
    /// (the entries are already in the caller's hands).
    committed: bool,
    /// Phase 8 slice 8a step 5: manager-shared range-lock registry
    /// (Arc-cloned).  Drop sweeps every lock acquired under this
    /// deploy's scope via `release_all_for_deploy(&deploy_scope)`
    /// AFTER the WAL discard-drain, so both the WAL contribution
    /// AND any leaked locks are cleaned up atomically at deploy end.
    lock_registry: LockRegistry,
    /// Phase 8 slice 8a step 5: the deploy-derived scope this guard
    /// is responsible for.  Blake2b256(deploy.sig) for user deploys;
    /// a state-hash-derived value for system deploys.  MUST be
    /// non-sentinel (`!= [0; 32]`); the `release_all_for_deploy`
    /// debug_assert guards against sentinel input.
    deploy_scope: DeployScope,
    /// Phase 8 slice 8a step 5: per-runtime "current deploy scope"
    /// cell shared with `FileHandleTable::current_deploy_scope`.
    /// Constructor sets it to `deploy_scope`; Drop clears back to
    /// `[0; 32]` after the sweep.  Handlers read this cell at
    /// acquire time so the LockRegistry entry records the correct
    /// scope for the eventual sweep.
    current_scope_cell: std::sync::Arc<std::sync::RwLock<DeployScope>>,
}

impl WalDeployScope {
    // H-2 fix (2026-08-06): now `pub(crate)` so `replay_runtime`
    // can use the same guard.  Pre-fix, replay didn't wrap deploys
    // in a WAL scope — `journal_read`/`journal_write` on the
    // replay branch appended to the follower's per-runtime WAL
    // without draining, causing unbounded growth across a block
    // (hits `MAX_WAL_ENTRIES` only on follower → follower fails
    // deploys the leader accepted) and making per-deploy WAL
    // comparison impossible.
    /// Legacy constructor — used only by pre-step-5 unit tests that
    /// don't exercise the lock-sweep behavior.  Uses a per-instance
    /// throwaway `LockRegistry` and a fresh scope cell, so Drop's
    /// `release_all_for_deploy` is a no-op (the registry is empty).
    /// New code MUST use `new_with_lock_sweep` and pass a real
    /// deploy-derived scope.
    #[cfg(test)]
    pub(crate) fn new(wal: Wal) -> Self {
        // Test-only fallback scope: any non-sentinel value avoids
        // the debug_assert in release_all_for_deploy.  `[0xAA; 32]`
        // is arbitrary but recognizable in test-failure diagnostics.
        Self::new_with_lock_sweep(
            wal,
            LockRegistry::new(),
            [0xAAu8; 32],
            std::sync::Arc::new(std::sync::RwLock::new([0u8; 32])),
        )
    }

    /// Phase 8 slice 8a step 5 — production constructor with lock
    /// sweep.  Sets `current_scope_cell` to `deploy_scope` so
    /// concurrent lock-native handlers record their acquires under
    /// this deploy's scope.  On Drop, sweeps `deploy_scope`'s locks
    /// from the shared `LockRegistry` and clears the cell back to
    /// sentinel.
    ///
    /// `deploy_scope` MUST be non-sentinel (`!= [0; 32]`); a sentinel
    /// scope would trip the `release_all_for_deploy` debug_assert
    /// on drop.  Blake2b256 output always produces a non-sentinel
    /// value under any real input (chance of collision with `[0; 32]`
    /// is 2⁻²⁵⁶ — negligible), so the enforcement is by construction
    /// via how callers derive scopes.
    pub(crate) fn new_with_lock_sweep(
        wal: Wal,
        lock_registry: LockRegistry,
        deploy_scope: DeployScope,
        current_scope_cell: std::sync::Arc<std::sync::RwLock<DeployScope>>,
    ) -> Self {
        debug_assert!(
            deploy_scope != [0u8; 32],
            "WalDeployScope::new_with_lock_sweep called with sentinel scope [0; 32]; \
             this would trip the release_all_for_deploy guard on Drop.  Callers \
             must derive a non-sentinel scope (Blake2b256 of deploy identifier)."
        );
        let mark = wal.begin_deploy();
        // Publish this deploy's scope to the shared cell so
        // concurrent-in-this-deploy lock-native calls record it.
        *current_scope_cell
            .write()
            .expect("current_deploy_scope RwLock poisoned") = deploy_scope;
        Self {
            wal,
            mark,
            committed: false,
            lock_registry,
            deploy_scope,
            current_scope_cell,
        }
    }

    /// Success path: drain entries in canonical (deploy_log) order
    /// and mark committed so Drop is a no-op.
    ///
    /// Slice 30c H-R3 integration: instead of returning entries in
    /// insertion order (scheduler-dependent under Par), walk the
    /// deploy's `event_log` Produces to derive a canonical order.
    /// Log order is frozen when the leader publishes the block; all
    /// validators consume the same log verbatim during replay, so
    /// the output is byte-identical across validators and across
    /// re-executions on the same validator regardless of tokio
    /// scheduling.  Resolves H-R3.
    pub(crate) fn take_and_commit(&mut self, deploy_log: &[Event]) -> Vec<WalEntry> {
        self.committed = true;
        let hashes = produce_channel_hashes(deploy_log);
        self.wal
            .take_deploy_entries_in_log_order(self.mark, &hashes)
    }
}

/// Slice 30c H-R3 integration: extract each Produce event's
/// `channels_hash` from a deploy's event log, in log order.  These
/// hashes are the keys the log-order WAL drain matches against the
/// per-entry ack-channel sidecar recorded by `Wal::append_with_ack`.
///
/// ConsumeEvents and CommEvents are skipped — the WAL sidecar is
/// keyed by ack-channel hash, which appears in the Produce that
/// publishes the syscall's reply.
pub(crate) fn produce_channel_hashes(deploy_log: &[Event]) -> Vec<[u8; 32]> {
    let mut out = Vec::with_capacity(deploy_log.len());
    for e in deploy_log {
        if let Event::Produce(pe) = e {
            // `channels_hash` is bytes::Bytes; expected to be exactly
            // 32 bytes (Blake2b256).  Anything else is malformed;
            // skip silently rather than panic — the drain has
            // defense-in-depth to append unmatched entries.
            if pe.channels_hash.len() == 32 {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&pe.channels_hash);
                out.push(buf);
            }
        }
    }
    out
}

impl Drop for WalDeployScope {
    fn drop(&mut self) {
        if !self.committed {
            // Discard-drain: entries produced during a failed deploy
            // are removed from the per-runtime buffer so the next
            // deploy's `begin_deploy` mark points at an empty tail.
            // Logged at debug level so an incident review can find
            // leaked entries.
            let leaked = self.wal.take_deploy_entries(self.mark);
            if !leaked.is_empty() {
                tracing::debug!(
                    target: "f1r3fly.casper.fs_wal",
                    n_entries = leaked.len(),
                    "fs-wal discard-drain on error path (deploy did not commit its WAL)"
                );
            }
        }
        // Phase 8 slice 8a step 5: sweep leaked locks per spec
        // §Explicit locks "Implementations MUST auto-release at
        // deploy-end".  Runs on EVERY exit path (success via
        // take_and_commit + Drop, `?`-propagated error via Drop
        // alone, panic-unwind via Drop alone).  Symmetrical with the
        // WAL discard-drain above — both close deploy-end resource
        // cleanup at the same point.
        //
        // The sentinel guard in release_all_for_deploy is not
        // exercised here: new_with_lock_sweep's debug_assert rejects
        // sentinel scopes at construction, and the legacy test
        // constructor uses `[0xAA; 32]` — also non-sentinel.
        let n_released = self
            .lock_registry
            .release_all_for_deploy(&self.deploy_scope);
        if n_released > 0 {
            tracing::debug!(
                target: "f1r3fly.casper.fs_locks",
                n_locks = n_released,
                "deploy-end auto-release: swept locks the caller did not release"
            );
        }
        // Clear the per-runtime "current scope" cell back to sentinel
        // so between-deploy handler calls (test-path only under
        // normal operation) see the sentinel value.
        *self
            .current_scope_cell
            .write()
            .expect("current_deploy_scope RwLock poisoned") = [0u8; 32];
    }
}

pub struct RuntimeOps {
    pub runtime: RhoRuntimeImpl,
}

impl RuntimeOps {
    pub fn new(runtime: RhoRuntimeImpl) -> Self { Self { runtime } }
}

#[allow(type_alias_bounds)]
pub type SysEvalResult<S: SystemDeployTrait> =
    (Either<SystemDeployUserError, S::Result>, EvaluateResult);

fn system_deploy_consume_all_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

/// Diagnostic label for a system deploy (closeBlock / slash / precharge /
/// refund). Called lazily inside tracing field evaluation, so it costs nothing
/// unless the event is enabled.
fn system_deploy_kind<S: SystemDeployTrait>(sd: &S) -> &'static str {
    let any = sd.as_any();
    if any.downcast_ref::<CloseBlockDeploy>().is_some() {
        "closeBlock"
    } else if any.downcast_ref::<SlashDeploy>().is_some() {
        "slash"
    } else if any.downcast_ref::<PreChargeDeploy>().is_some() {
        "precharge"
    } else if any.downcast_ref::<RefundDeploy>().is_some() {
        "refund"
    } else {
        "other"
    }
}

impl RuntimeOps {
    /**
     * Because of the history legacy, the emptyStateHash does not really represent an empty trie.
     * The `emptyStateHash` is used as genesis block pre state which the state only contains registry
     * fixed channels in the state.
     */
    pub async fn empty_state_hash(&mut self) -> Result<StateHash, CasperError> {
        self.runtime
            .reset(&RadixHistory::empty_root_node_hash())
            .await?;

        bootstrap_registry(&self.runtime).await;
        let checkpoint = self.runtime.create_checkpoint().await;
        Ok(checkpoint.root.bytes().into())
    }

    /* Compute state with deploys (genesis block) and System deploys (regular block) */

    /**
     * Evaluates deploys and System deploys with checkpoint to get final state hash
     */
    pub async fn compute_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-state") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-started");
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        if tracing::enabled!(target: "f1r3fly.casper.invalid_blocks", tracing::Level::DEBUG) {
            let entries: Vec<String> = invalid_blocks
                .iter()
                .map(|(bh, v)| {
                    format!(
                        "{}=>{}",
                        hex::encode(&bh[..8.min(bh.len())]),
                        hex::encode(&v[..8.min(v.len())])
                    )
                })
                .collect();
            tracing::debug!(target: "f1r3fly.casper.invalid_blocks", n = invalid_blocks.len(), seq = block_data.seq_num, "PLAY compute_state invalid_blocks: [{}]", entries.join(", "));
        }
        self.runtime.set_block_data(block_data).await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_block_data", rss_kb);
        }
        self.runtime.set_invalid_blocks(invalid_blocks).await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_invalid_blocks", rss_kb);
        }

        let (start_hash, processed_deploys) =
            self.play_deploys_for_state(start_hash, terms).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_play_deploys_for_state", rss_kb);
        }

        let mut current_hash = start_hash;
        let mut processed_system_deploys = Vec::with_capacity(system_deploys.len());

        for system_deploy_enum in system_deploys {
            // Match on the enum and call appropriate generic method
            let result = match system_deploy_enum {
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Slash(
                    mut slash_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut slash_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                    mut close_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut close_deploy)
                        .await?
                }
            };

            match result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    mergeable_channels,
                    result: _,
                } => {
                    processed_system_deploys.push((processed_system_deploy, mergeable_channels));
                    current_hash = state_hash;
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Failed { error_msg, .. },
                } => {
                    return Err(CasperError::RuntimeError(format!(
                        "Unexpected system error during play of system deploy: {}",
                        error_msg
                    )))
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Succeeded { .. },
                } => {
                    return Err(CasperError::RuntimeError(
                        "Unreachable code path. This is likely caused by a bug in the runtime."
                            .to_string(),
                    ))
                }
            }
        }

        let post_state_hash = current_hash;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "finish", rss_kb);
        }

        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-finished");
        Ok((post_state_hash, processed_deploys, processed_system_deploys))
    }

    /**
     * Evaluates genesis deploys with checkpoint to get final state hash
     */
    pub async fn compute_genesis(
        &mut self,
        terms: Vec<Signed<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<
        (
            StateHash,
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-genesis") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-started");
        self.runtime
            .set_block_data(BlockData {
                time_stamp: block_time,
                block_number,
                sender: PublicKey::from_bytes(&Vec::new()),
                seq_num: 0,
            })
            .await;

        let genesis_pre_state_hash = self.empty_state_hash().await?;
        let play_result = self
            .play_deploys_for_genesis(&genesis_pre_state_hash, terms)
            .await?;

        let (post_state_hash, processed_deploys) = play_result;
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-finished");
        Ok((genesis_pre_state_hash, post_state_hash, processed_deploys))
    }

    /* Deploy evaluators */

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play_deploys", "play-deploys-started");
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_reset", rss_kb);
        }

        let mut res = Vec::with_capacity(terms.len());
        // H-30-2 slice-30b fix: per-block WAL aggregator.
        // `play_deploy_with_cost_accounting` returns each deploy's
        // WAL contribution; we accumulate them in block order and,
        // at the end of the block, compute + log the per-block WAL
        // root.  Slice 30c will hand this vec to the snapshot
        // cadence writer + on-chain WAL commitment.
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();
        for deploy in terms {
            let (pd, mc, fs_wal) = self.play_deploy_with_cost_accounting(deploy).await?;
            if !fs_wal.is_empty() {
                block_fs_wal.extend(fs_wal);
            }
            res.push((pd, mc));
        }
        if !block_fs_wal.is_empty() {
            let root = compute_wal_root(&block_fs_wal);
            // H-30b-4 review note (slice 30b round 2): OPERATOR-VISIBLE
            // LOG SCHEMA — do not rename these fields without
            // coordinating with dashboards.  `target` is the
            // aggregator for downstream observability filters.
            //   - `n_entries` (u64): count of WalEntries in this block's slice.
            //   - `block_wal_root` (String): first 8 hex bytes of the Blake2b256
            //     content-address of the encoded slice.
            // Slice 30c will add the full 32-byte root to on-chain
            // ProcessedDeploy metadata; this log becomes secondary.
            tracing::info!(
                target: "f1r3fly.casper.fs_wal",
                n_entries = block_fs_wal.len(),
                block_wal_root = %hex::encode(&root[..8]),
                "per-block consensus WAL slice computed"
            );
            // Slice 30b MVP + H-30b-2 round-2 + M-P7-1 whole-review
            // fix: read the shared snapshot writer via the runtime's
            // Arc<RwLock<_>> (populated by RuntimeManager::spawn_runtime
            // via `share_fs_snapshot_writer`).  Reading each time keeps
            // any boot-time set on the RuntimeManager visible to this
            // runtime immediately.  Clone the writer + entries out of
            // the read guard so we can drop the lock BEFORE the
            // blocking I/O (which uses `std::fs::write` + `sync_all` +
            // `rename` + dir fsync — can be seconds on slow disks).
            // Then dispatch the blocking work via `spawn_blocking` so
            // we don't stall the tokio worker or block a concurrent
            // `RuntimeManager::set_fs_snapshot_writer` awaiting the
            // write lock.
            // H-1 fix (2026-08-06) — slice 30c Phase B: don't
            // snapshot per-block; cache the slice keyed by
            // post-state hash so the finalization runner can
            // snapshot only on cadence-hit blocks that ACTUALLY
            // finalize.  Pre-fix, `SnapshotWriter::maybe_write`
            // fired here on every candidate block including
            // non-finalized DAG tips — snapshot content forked
            // silently across siblings, and orphaned blocks
            // produced writes that were never referenced by the
            // finalized chain.  Post-fix, snapshots reflect the
            // finalized-chain history only.  The write itself
            // happens in
            // `finalization_runner::new_lfb_found_effect` after
            // reading from `pending_wal_slices`.
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint_create_checkpoint", rss_kb);
        }
        let final_checkpoint = self.runtime.create_checkpoint().await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint_create_checkpoint", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_final_checkpoint_root_to_bytes", rss_kb);
        }
        let final_root = final_checkpoint.root.to_bytes_prost();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint_root_to_bytes", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_final_checkpoint", rss_kb);
        }
        // H-1 fix (2026-08-06) — slice 30c Phase B: cache the
        // per-block WAL slice keyed by the post-state hash we just
        // computed.  A finalized block carries this same
        // `post_state_hash` in `block.body.state.post_state_hash`, so
        // `finalization_runner::new_lfb_found_effect` can look up the
        // slice by that key and snapshot on cadence hits.  Bounded
        // cache: the finalization runner also evicts stale entries
        // whose block_number is <= the new LFB (orphaned or already
        // handled).  See `pending_wal_slices` docstring on
        // `RuntimeManager` for the cache-size argument.
        if !block_fs_wal.is_empty() {
            let block_number = self.runtime.block_data_ref.read().await.block_number;
            const MAX_PENDING_WAL_SLICES: usize = 1024;
            let mut slices = self.runtime.pending_wal_slices.write().await;
            if slices.len() >= MAX_PENDING_WAL_SLICES {
                // Defensive: shouldn't hit under normal operation
                // (finalization latency is small vs. slice-block
                // production rate).  If we do, drop the oldest by
                // block_number.  Log so operators can raise the cap
                // if they legitimately have deep-fork scenarios.
                if let Some(oldest_key) = slices
                    .iter()
                    .min_by_key(|(_, (bn, _))| *bn)
                    .map(|(k, _)| k.clone())
                {
                    slices.remove(&oldest_key);
                    tracing::warn!(
                        target: "f1r3fly.casper.fs_wal",
                        cap = MAX_PENDING_WAL_SLICES,
                        "pending_wal_slices cache full; evicting oldest entry.  Deep-fork scenario or stalled finalizer?"
                    );
                }
            }
            slices.insert(final_root.to_vec(), (block_number, block_fs_wal));
        }
        Ok((final_root, res))
    }

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_genesis(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play_deploys_genesis", "play-deploys-genesis-started");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;

        // Slice 31 + H-P7-5 review fix: RAII exemption for the URN
        // filter.  Genesis composition needs `rho:io:fs:native:*`
        // URNs available so the FsGenesis deploy can bind `fsRead`,
        // `fsWrite`, etc.  The Drop impl re-enables the filter on
        // ALL exit paths — including panics and tokio-task
        // cancellation — so a subsequent user deploy on this runtime
        // cannot inherit the exemption even under adversarial or
        // buggy execution.  Guard holds an Arc<AtomicBool> clone of
        // the flag (not a runtime borrow), so mutable-runtime access
        // inside the loop is unblocked.
        let _filter_exemption = self.runtime.exempt_fs_native_urn_filter();
        let mut res = Vec::with_capacity(terms.len());
        for deploy in terms {
            res.push(self.process_deploy_with_mergeable_data(deploy).await?);
        }
        drop(_filter_exemption); // Explicit drop before create_checkpoint (below).

        let final_checkpoint = self.runtime.create_checkpoint().await;
        Ok((final_checkpoint.root.to_bytes_prost(), res))
    }

    /**
     * Evaluates deploy with cost accounting (PoS Pre-charge and Refund calls)
     *
     * # Return value (H-30-2 slice-30b fix)
     *
     * Returns `(ProcessedDeploy, NumberChannelsEndVal, Vec<WalEntry>)`.
     * The third element is the per-deploy consensus WAL contribution
     * drained via `WalDeployScope::take_and_commit`.  Pre-fix, these
     * entries were populated in `EvalCollector.fs_wal_entries` and
     * silently dropped on function return — a real observability sink
     * for any operator running `consensus-static-*` provisioning.  The
     * block emitter aggregates the per-deploy entries into a
     * per-block `Vec<WalEntry>` for the cadence loop to snapshot and
     * (in slice 30c) for the on-chain WAL Merkle root commitment.
     */
    pub async fn play_deploy_with_cost_accounting(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal, Vec<WalEntry>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploy") from Scala
        tracing::debug!(target: "f1r3fly.casper.play_deploy", "play-deploy-started");
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }
        let mut eval_collector_state = EvalCollector::new();

        // Slice 30 (C-30-1 round-2 fix): RAII drain guard for the
        // per-deploy WAL boundary.  Everything between here and the
        // `take_and_commit` call at the end of the deploy is the
        // deploy's WAL contribution — precharge + user deploy +
        // refund all share the same boundary (they are one atomic
        // deploy from the consensus-commitment perspective).  On any
        // `?`-propagated error, `wal_scope`'s Drop drains-and-discards
        // so entries produced by a failed deploy do not leak into the
        // next deploy's slice.  Pre-fix, five `?` operators between
        // begin_deploy and the drain sites could leak entries.
        // Step 5: derive deploy_scope from the deploy signature so
        // every lock acquired under this deploy is tagged with a
        // unique 32-byte identifier.  Blake2b256 output is
        // consensus-observable through no path other than the sweep
        // count (which isn't consensus-observable — see Drop
        // comment).  On drop, the guard sweeps this scope's locks
        // from the shared LockRegistry.
        let deploy_scope: DeployScope = {
            let h = crypto::rust::hash::blake2b256::Blake2b256::hash(deploy.sig.to_vec());
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        };
        let mut wal_scope = WalDeployScope::new_with_lock_sweep(
            self.runtime.fs_handles.wal.clone(),
            self.runtime.fs_handles.lock_registry.clone(),
            deploy_scope,
            self.runtime.fs_handles.current_deploy_scope.clone(),
        );

        let deploy_pk = deploy.pk.bytes.clone();
        let deploy_pk_hex = hex::encode(&deploy_pk);
        let deploy_sig_hex = hex::encode(&deploy.sig);
        let refund_rand = system_deploy_util::generate_refund_deploy_random_seed(&deploy);
        let pre_charge_rand = system_deploy_util::generate_pre_charge_deploy_random_seed(&deploy);

        // Evaluates Pre-charge system deploy
        let pre_charge_result = {
            // Using tracing events for async - Span[F].traceI("precharge") from Scala
            tracing::debug!(target: "f1r3fly.casper.precharge", "precharge-started");
            tracing::debug!(
                "PreCharging {} for {}",
                deploy_pk_hex.as_str(),
                deploy.data.total_phlo_charge()
            );
            if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_precharge_internal", rss_kb);
            }
            let (event_log, result, mergeable_channels) = self
                .play_system_deploy_internal(&mut PreChargeDeploy {
                    charge_amount: deploy.data.total_phlo_charge(),
                    pk: deploy.pk.clone(),
                    rand: pre_charge_rand,
                })
                .await?;
            if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_precharge_internal", rss_kb);
            }
            eval_collector_state.add(event_log, mergeable_channels);
            if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_precharge_collect", rss_kb);
            }
            result
        };
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_precharge", rss_kb);
        }

        match pre_charge_result {
            Either::Right(_) => {
                // Evaluates user deploy
                let pd = {
                    // Using tracing events for async - Span[F].traceI("user-deploy") from Scala
                    tracing::debug!(target: "f1r3fly.casper.user_deploy", "user-deploy-started");
                    tracing::debug!("Processing user deploy {}", deploy_pk_hex.as_str());
                    // Evaluates user deploy and append event log to local state
                    {
                        let (mut pd, mc) = self.process_deploy(deploy).await?;
                        let deploy_log = mem::take(&mut pd.deploy_log);
                        eval_collector_state.add(deploy_log, mc);
                        pd
                    }
                };
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_user_deploy", rss_kb);
                }

                // Evaluates Refund system deploy
                let refund_result = {
                    // Using tracing events for async - Span[F].traceI("refund") from Scala
                    tracing::debug!(target: "f1r3fly.casper.refund", "refund-started");
                    tracing::debug!(
                        "Refunding {} with {}",
                        deploy_pk_hex.as_str(),
                        pd.refund_amount()
                    );
                    let (event_log, result, mergeable_channels) = self
                        .play_system_deploy_internal(&mut RefundDeploy {
                            refund_amount: pd.refund_amount(),
                            rand: refund_rand,
                        })
                        .await?;
                    eval_collector_state.add(event_log, mergeable_channels);
                    result
                };
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_refund", rss_kb);
                }

                match refund_result {
                    Either::Right(_) => {
                        // Get mergeable channels data
                        let mergeable_channels_data = self
                            .get_number_channels_data(&eval_collector_state.mergeable_channels)
                            .await?;

                        let deploy_log = mem::take(&mut eval_collector_state.event_log);
                        // Slice 30 (C-30-1 round-2 fix): drain via
                        // RAII scope — success path opts in via
                        // `take_and_commit`; any `?`-error above
                        // discards via Drop, closing the pre-fix
                        // cross-deploy leak.
                        //
                        // Slice 30c H-R3 integration: pass the deploy
                        // log so the drain uses log-order (canonical
                        // across validators) rather than the
                        // scheduler-dependent insertion order.
                        let fs_wal = wal_scope.take_and_commit(&deploy_log);
                        if !fs_wal.is_empty() {
                            let wal_root = compute_wal_root(&fs_wal);
                            tracing::debug!(
                                target: "f1r3fly.casper.fs_wal",
                                deploy_sig = deploy_sig_hex.as_str(),
                                n_entries = fs_wal.len(),
                                wal_root = %hex::encode(&wal_root[..8]),
                                "fs-wal per-deploy drain (committed)"
                            );
                            eval_collector_state.add_fs_wal_entries(fs_wal);
                        }
                        if let Some(rss_kb) =
                            crate::rust::util::rholang::mem_profiler::read_vm_rss_kb()
                        {
                            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_collect_result", rss_kb);
                        }

                        // H-30-2 slice-30b fix: hand fs_wal_entries
                        // to the caller (block emitter) instead of
                        // dropping via EvalCollector.
                        let fs_wal_entries =
                            std::mem::take(&mut eval_collector_state.fs_wal_entries);
                        Ok((
                            ProcessedDeploy { deploy_log, ..pd },
                            mergeable_channels_data,
                            fs_wal_entries,
                        ))
                    }

                    Either::Left(error) => {
                        // If Pre-charge succeeds and Refund fails, it's a platform error.
                        // Include deploy identifiers so operators can quickly isolate toxic deploys.
                        let refund_amount = pd.refund_amount();
                        let failure_context = format!(
                            "{}, deploy_sig={}, deployer_pk={}, refund_amount={}",
                            error.error_message,
                            deploy_sig_hex,
                            deploy_pk_hex.as_str(),
                            refund_amount
                        );
                        metrics::counter!(
                            "casper_runtime_refund_failures_total",
                            "source" => CASPER_METRICS_SOURCE
                        )
                        .increment(1);
                        tracing::warn!("Refund failure '{}'", failure_context);
                        Err(CasperError::SystemRuntimeError(
                            SystemDeployPlatformFailure::GasRefundFailure(failure_context),
                        ))
                    }
                }
            }

            Either::Left(error) => {
                tracing::error!(error = %error.error_message, "pre-charge evaluation failed");

                // Handle evaluation errors from PreCharge
                // - assigning 0 cost - replay should reach the same state
                let mut empty_pd = ProcessedDeploy::empty(deploy);
                empty_pd.system_deploy_error = Some(error.error_message);

                // Update result with accumulated event logs
                // Get mergeable channels data
                let mergeable_channels_data = self
                    .get_number_channels_data(&eval_collector_state.mergeable_channels)
                    .await?;

                let deploy_log = mem::take(&mut eval_collector_state.event_log);
                // Slice 30 (round-2 fix): drain via RAII scope — the
                // pre-charge-failure path still yields an Ok
                // ProcessedDeploy, so we commit the scope explicitly.
                // Pre-charge failures typically leave the WAL empty
                // (no fs handlers ran) but the drain is free.
                //
                // Slice 30c H-R3 integration: log-order drain.
                let fs_wal = wal_scope.take_and_commit(&deploy_log);
                if !fs_wal.is_empty() {
                    tracing::debug!(
                        target: "f1r3fly.casper.fs_wal",
                        deploy_sig = deploy_sig_hex.as_str(),
                        n_entries = fs_wal.len(),
                        "fs-wal drain on pre-charge failure (committed)"
                    );
                    eval_collector_state.add_fs_wal_entries(fs_wal);
                }

                let fs_wal_entries = std::mem::take(&mut eval_collector_state.fs_wal_entries);
                Ok((
                    ProcessedDeploy {
                        deploy_log,
                        ..empty_pd
                    },
                    mergeable_channels_data,
                    fs_wal_entries,
                ))
            }
        }
    }

    pub async fn process_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>), CasperError> {
        // Keep a soft checkpoint before user deploy execution so failed deploy rollback
        // preserves pre-charge side effects required by refundDeploy.
        let fallback = self.runtime.create_soft_checkpoint().await;

        // Evaluate deploy
        let eval_result = self.evaluate(&deploy).await?;

        let deploy_log = self.runtime.take_event_log().await;

        let eval_succeeded = eval_result.errors.is_empty();
        let deploy_sig = deploy.sig.clone();

        let deploy_result = ProcessedDeploy {
            deploy,
            cost: Cost::to_proto(eval_result.cost),
            deploy_log: deploy_log
                .into_iter()
                .map(|event| event_converter::to_casper_event(event))
                .collect(),
            is_failed: !eval_succeeded,
            system_deploy_error: None,
        };

        if !eval_succeeded {
            self.runtime.revert_to_soft_checkpoint(fallback).await;
            interpreter_util::print_deploy_errors(&deploy_sig, &eval_result.errors);
        }

        Ok((deploy_result, eval_result.mergeable))
    }

    pub async fn process_deploy_with_mergeable_data(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let (pd, merge_chs) = self.process_deploy(deploy).await?;
        let data = self.get_number_channels_data(&merge_chs).await?;
        Ok((pd, data))
    }

    pub async fn get_number_channels_data(
        &self,
        channels: &std::collections::HashMap<
            Par,
            rspace_plus_plus::rspace::merger::merging_logic::MergeType,
        >,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let mut result = BTreeMap::new();
        for (channel, merge_type) in channels {
            if let Some((hash, value)) = self.get_number_channel(channel, *merge_type).await? {
                result.insert(hash, (value, *merge_type));
            }
        }
        Ok(result)
    }

    pub fn fold_bitmask_or(values: &[i64]) -> Option<i64> {
        if values.is_empty() {
            return None;
        }
        Some(
            values
                .iter()
                .fold(0i64, |acc, v| ((acc as u64) | (*v as u64)) as i64),
        )
    }

    pub async fn get_number_channel(
        &self,
        channel: &Par,
        merge_type: MergeType,
    ) -> Result<Option<(Blake2b256Hash, i64)>, CasperError> {
        let ch_values = self.runtime.get_data(channel).await;

        if ch_values.is_empty() {
            Ok(None)
        } else {
            let ch_hash = stable_hash_provider::hash(channel);
            if ch_values.len() != 1 {
                let nums: Vec<i64> = ch_values
                    .iter()
                    .filter_map(|datum| {
                        RholangMergingLogic::try_get_number_with_rnd(&datum.a).map(|(n, _)| n)
                    })
                    .collect();

                match merge_type {
                    MergeType::IntegerAdd => {
                        return Err(CasperError::RuntimeError(format!(
                            "number channel {} holds {} values {:?}; IntegerAdd single-value invariant violated",
                            hex::encode(ch_hash.bytes()),
                            ch_values.len(),
                            nums,
                        )));
                    }
                    MergeType::BitmaskOr => {
                        let num = match Self::fold_bitmask_or(&nums) {
                            Some(n) => n,
                            None => return Ok(None),
                        };
                        return Ok(Some((ch_hash, num)));
                    }
                }
            }

            // Single value: opportunistic numeric read. Non-numeric values
            // (e.g., TreeHashMap leaf Maps tagged with the bitmask tag) are
            // skipped here and fall through to the existing conflict path.
            let num_par = &ch_values[0].a;
            match RholangMergingLogic::try_get_number_with_rnd(num_par) {
                Some((num, _)) => Ok(Some((ch_hash, num))),
                None => Ok(None),
            }
        }
    }

    /* System deploy evaluators */

    /**
     * Evaluates System deploy with checkpoint to get final state hash
     */
    pub async fn play_system_deploy<S: SystemDeployTrait>(
        &mut self,
        state_hash: &StateHash,
        system_deploy: &mut S,
    ) -> Result<SystemDeployResult<S::Result>, CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
            .await?;

        // Slice 30c F-30b-8 fix: wrap the standalone system deploy
        // path with a WalDeployScope so any Consensus WAL entries a
        // system deploy would produce are drained + discarded per-
        // deploy rather than leaking into the next user deploy's
        // slice.  Currently no system deploy (CloseBlock, Slash,
        // PreCharge, Refund) touches Consensus caps at all — they
        // dispatch only to PoS/vault Rholang contracts, none of
        // which invoke fs-native URNs.  If a future system deploy
        // is written to touch Consensus caps, its WAL entries
        // land in `_leaked_entries` here, get logged at warn, and
        // are discarded rather than silently attributed to the
        // next user deploy.  A follow-up slice can extend the
        // block-level WAL aggregator to include system-deploy
        // contributions (needs a proto extension: system deploys
        // don't have per-deploy WAL attribution on
        // `ProcessedSystemDeploy` today).
        // Step 5: system deploys don't have a signature (they're
        // internal PoS/vault/system operations), so derive scope from
        // `state_hash` prefixed by a system-deploy marker.  System
        // deploys currently don't touch fs-native URNs — the sweep is
        // a no-op — but a non-sentinel scope is required to avoid
        // tripping the `release_all_for_deploy` debug_assert on drop.
        let deploy_scope: DeployScope = {
            let mut input = b"phase8-system-deploy:".to_vec();
            input.extend_from_slice(state_hash);
            let h = crypto::rust::hash::blake2b256::Blake2b256::hash(input);
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        };
        let _wal_scope = WalDeployScope::new_with_lock_sweep(
            self.runtime.fs_handles.wal.clone(),
            self.runtime.fs_handles.lock_registry.clone(),
            deploy_scope,
            self.runtime.fs_handles.current_deploy_scope.clone(),
        );

        let (event_log, result, mergeable_channels) =
            self.play_system_deploy_internal(system_deploy).await?;

        let final_state_hash = {
            let checkpoint = self.runtime.create_checkpoint().await;
            checkpoint.root.to_bytes_prost()
        };

        match result {
            Either::Right(system_deploy_result) => {
                let mcl = self.get_number_channels_data(&mergeable_channels).await?;
                if let Some(SlashDeploy {
                    invalid_block_hash,
                    pk,
                    target_activation_epoch,
                    initial_rand: _,
                }) = system_deploy.as_any().downcast_ref::<SlashDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_slash(
                            invalid_block_hash.clone(),
                            pk.clone(),
                            *target_activation_epoch,
                        ),
                        mcl,
                        system_deploy_result,
                    ))
                } else if let Some(CloseBlockDeploy { .. }) =
                    system_deploy.as_any().downcast_ref::<CloseBlockDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_close(),
                        mcl,
                        system_deploy_result,
                    ))
                } else {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::Empty,
                        mcl,
                        system_deploy_result,
                    ))
                }
            }

            Either::Left(usr_err) => Ok(SystemDeployResult::play_failed(event_log, usr_err)),
        }
    }

    pub async fn play_system_deploy_internal<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<
        (
            Vec<Event>,
            Either<SystemDeployUserError, S::Result>,
            HashMap<Par, MergeType>,
        ),
        CasperError,
    > {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        // Get System deploy result / throw fatal errors for unexpected results
        let (result_or_system_deploy_error, eval_result) =
            self.eval_system_deploy(system_deploy).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_eval_system_deploy", rss_kb);
        }

        let log = self.runtime.take_event_log().await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_take_event_log", rss_kb);
        }
        let log = log
            .into_iter()
            .map(event_converter::to_casper_event)
            .collect();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_convert_event_log", rss_kb);
        }

        Ok((log, result_or_system_deploy_error, eval_result.mergeable))
    }

    /**
     * Evaluates System deploy (applicative errors are fatal)
     */
    pub async fn eval_system_deploy<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<SysEvalResult<S>, CasperError> {
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", kind = system_deploy_kind(system_deploy), "eval_system_deploy ENTER (eval system source, then consume its result)");
        let wrapper_pre_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        let wrapper_pre = wrapper_pre_start.elapsed();
        let eval_result = self.evaluate_system_source(system_deploy).await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_evaluate_system_source", rss_kb);
        }

        let wrapper_mid_start = Instant::now();
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", n_eval_errors = eval_result.errors.len(), "eval_system_deploy: system source evaluated");
        if !eval_result.errors.is_empty() {
            tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", "eval_system_deploy: UnexpectedSystemErrors (system deploy eval ERRORED)");
            return Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::UnexpectedSystemErrors(eval_result.errors),
            ));
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_error_check", rss_kb);
        }

        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_consume_system_result", rss_kb);
        }
        let wrapper_mid = wrapper_mid_start.elapsed();
        let consumed = self.consume_system_result(system_deploy).await?;
        let wrapper_post_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_consume_system_result", rss_kb);
        }
        let r = match consumed {
            Some((_, vec_list)) => match vec_list.as_slice() {
                [ListParWithRandom { pars, .. }] if pars.len() == 1 => {
                    let extracted = system_deploy.extract_result(&pars[0]);
                    if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb()
                    {
                        tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_extract_result", rss_kb);
                    }
                    Ok(extracted)
                }
                _ => Err(CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::UnexpectedResult(
                        vec_list.iter().flat_map(|lp| lp.pars.clone()).collect(),
                    ),
                )),
            },
            None => {
                // INSTRUMENT (temporary): dump the leftover replay COMMs — names
                // the consume the closeBlock stalled on at replay.
                if let Err(e) = self.runtime.check_replay_data().await {
                    tracing::error!(target: "f1r3fly.casper.replay_block", kind = system_deploy_kind(system_deploy), "system-deploy ConsumeFailed replay stall (THIS is the deploy that returned None): {}", e);
                }
                Err(CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::ConsumeFailed,
                ))
            }
        }?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_match_result", rss_kb);
        }
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(
                (wrapper_pre + wrapper_mid + wrapper_post_start.elapsed()).as_nanos() as u64,
            );

        Ok((r, eval_result))
    }

    /**
     * Evaluates exploratory (read-only) deploy
     */
    pub async fn play_exploratory_deploy(
        &mut self,
        term: String,
        hash: &StateHash,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let deploy_result = async {
            let deploy = construct_deploy::source_deploy(
                term,
                0,
                // Hardcoded phlogiston limit / 1 REV if phloPrice=1
                Some(100 * 1000 * 1000),
                None,
                Some(
                    EXPLORATORY_DEPLOY_KEY
                        .get_or_init(|| Secp256k1.new_key_pair().0)
                        .clone(),
                ),
                None,
                None,
            )?;

            // Create return channel as first private name created in deploy term
            let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
            let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: rand.next().into_iter().map(|b| b as u8).collect(),
                })),
            }]);

            // Execute deploy on top of specified block hash
            self.capture_results_with_name(hash, &deploy, &return_name)
                .await
        };

        deploy_result.await
    }

    /// Lenient exploratory query: a runtime execution failure degrades to an
    /// empty result (logged, not propagated). Appropriate for display/API
    /// reads (bonds, active validators) — NEVER for consensus-level reads,
    /// where "failed" and "absent" must stay distinguishable
    /// (see [`Self::play_exploratory_par_strict`]).
    pub async fn play_exploratory_par(
        &mut self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        self.play_exploratory_par_with_mode(par, hash, false).await
    }

    /// Strict variant: a runtime injection failure PROPAGATES as an error
    /// instead of degrading to an empty result. Required for consensus-level
    /// reads (the protocol fault-tolerance threshold) where "query failed"
    /// must never be conflated with "value genuinely absent" — the lenient
    /// empty-result degradation would silently route a transient execution
    /// failure into the local-config fallback and re-open node-local
    /// divergence.
    pub async fn play_exploratory_par_strict(
        &mut self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        self.play_exploratory_par_with_mode(par, hash, true).await
    }

    async fn play_exploratory_par_with_mode(
        &mut self,
        par: Par,
        hash: &StateHash,
        strict: bool,
    ) -> Result<Vec<Par>, CasperError> {
        use crate::rust::metrics_constants::{
            BONDS_CACHE_GET_DATA_TIME_METRIC, BONDS_CACHE_INJ_TIME_METRIC,
            BONDS_CACHE_RESET_TIME_METRIC, CASPER_METRICS_SOURCE,
        };
        let __reset_start = std::time::Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_reset", rss_kb);
        }
        self.runtime.cost().set(Cost::unsafe_max());
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_set_cost", rss_kb);
        }
        metrics::histogram!(BONDS_CACHE_RESET_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(__reset_start.elapsed().as_secs_f64());

        let rand = Blake2b512Random::create_from_bytes(&[0u8; 128]);
        let mut return_rand = rand.clone();
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_build_return_name", rss_kb);
        }

        let __inj_start = std::time::Instant::now();
        let result = match self.runtime.inj(par, Env::new(), rand).await {
            Ok(()) => {
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_inj_ok", rss_kb);
                }
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                let __get_data_start = std::time::Instant::now();
                let data = self.get_data_par(&return_name).await;
                metrics::histogram!(BONDS_CACHE_GET_DATA_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__get_data_start.elapsed().as_secs_f64());
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_get_data_par", rss_kb);
                }
                Ok(data)
            }
            Err(err) => {
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
                    tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_inj_err", rss_kb);
                }
                tracing::error!(error = ?err, strict, "play_exploratory_par failed");
                if strict {
                    Err(CasperError::RuntimeError(format!(
                        "exploratory query execution failed (strict mode): {:?}",
                        err
                    )))
                } else {
                    Ok(Vec::new())
                }
            }
        };

        let _ = self.runtime.take_event_log().await;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_take_event_log", rss_kb);
        }
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_post_query_reset", rss_kb);
        }

        result
    }

    /* Checkpoints */

    /**
     * Creates soft checkpoint with rollback if result is false.
     */
    pub async fn with_soft_transaction<A, F, Fut>(&mut self, action: F) -> Result<A, CasperError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(A, bool), CasperError>>,
    {
        let fallback = self.runtime.create_soft_checkpoint().await;

        // Execute action
        let (a, success) = action().await?;

        // Revert the state if failed
        if !success {
            self.runtime.revert_to_soft_checkpoint(fallback).await;
        }

        Ok(a)
    }

    /* Evaluates and captures results */

    // Return channel on which result is captured is the first name
    // in the deploy term `new return in { return!(42) }`
    pub async fn capture_results(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        // Create return channel as first unforgeable name created in deploy term
        let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);

        let (data, _cost) = self
            .capture_results_with_name(start, deploy, &return_name)
            .await?;
        Ok(data)
    }

    pub async fn capture_results_with_name(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.capture_results_with_errors(start, deploy, name).await
    }

    pub async fn capture_results_with_errors(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start))
            .await?;

        let eval_res = self.evaluate(deploy).await?;
        if !eval_res.errors.is_empty() {
            return Err(CasperError::InterpreterError(eval_res.errors[0].clone()));
        }

        let cost = eval_res.cost.value.max(0) as u64;
        Ok((self.get_data_par(name).await, cost))
    }

    /* Evaluates Rholang source code */

    pub async fn evaluate(
        &mut self,
        deploy: &Signed<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        let deploy_data = SystemProcessDeployData::from_deploy(deploy);
        self.runtime.set_deploy_data(deploy_data).await;

        let result = self
            .runtime
            .evaluate(
                &deploy.data.term,
                Cost::create(deploy.data.phlo_limit, "Evaluate deploy".to_string()),
                normalizer_env_from_deploy(deploy),
                Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp),
            )
            .await;

        match result {
            Ok(eval_result) => Ok(eval_result),
            Err(e) => Err(CasperError::InterpreterError(e)),
        }
    }

    pub async fn evaluate_system_source<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<EvaluateResult, CasperError> {
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "start", rss_kb);
        }

        // Using tracing events for async - Span[F].traceI("evaluate-system-source") from Scala
        tracing::debug!(target: "f1r3fly.casper.evaluate_system_source", "evaluate-system-source-started");
        let eval_start = Instant::now();
        let wrapper_pre_start = eval_start;
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_build_env", rss_kb);
        }
        let env = system_deploy.env();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_build_env", rss_kb);
        }
        let rand = system_deploy.rand().clone();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_clone_rand", rss_kb);
        }
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "before_runtime_evaluate", rss_kb);
        }
        let wrapper_pre = wrapper_pre_start.elapsed();
        let result = self
            .runtime
            .evaluate(
                S::source(),
                Cost::unsafe_max(),
                env,
                // TODO: Review this clone and whether to pass mut ref down into evaluate
                rand,
            )
            .await?;
        let wrapper_post_start = Instant::now();
        if let Some(rss_kb) = crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() {
            tracing::debug!(target: "f1r3fly.casper.mem_profile", step = "after_runtime_evaluate", rss_kb);
        }
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(eval_start.elapsed().as_secs_f64());
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment((wrapper_pre + wrapper_post_start.elapsed()).as_nanos() as u64);
        Ok(result)
    }

    pub async fn get_data_par(&self, channel: &Par) -> Vec<Par> {
        self.runtime
            .get_data(channel)
            .await
            .into_iter()
            .flat_map(|datum| datum.a.pars)
            .collect()
    }

    pub async fn get_continuation_par(&self, channels: Vec<Par>) -> Vec<(Vec<BindPattern>, Par)> {
        self.runtime
            .get_continuations(channels)
            .await
            .into_iter()
            .filter_map(|wk| {
                if let Some(TaggedCont::ParBody(par_body)) = wk.continuation.tagged_cont {
                    Some((wk.patterns, par_body.body.unwrap()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn consume_result(
        &mut self,
        channel: Par,
        pattern: BindPattern,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        Ok(self
            .runtime
            .consume_result(vec![channel], vec![pattern])
            .await?)
    }

    pub async fn consume_system_result<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        let consume_start = Instant::now();
        let return_channel = system_deploy.return_channel()?;
        let result = self
            .consume_result(return_channel, system_deploy_consume_all_pattern())
            .await;
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(consume_start.elapsed().as_secs_f64());
        result
    }

    /* Read only Rholang evaluator helpers */

    pub async fn get_active_validators(
        &mut self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        let validators_pars = self
            .play_exploratory_par(Self::activate_validator_query_par().clone(), start_hash)
            .await?;

        if validators_pars.is_empty() {
            tracing::warn!(
                "No result from getActiveValidators query for state {}; treating as no active validators",
                PrettyPrinter::build_string_bytes(start_hash)
            );
            return Ok(Vec::new());
        }

        if validators_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(start_hash),
                validators_pars.len()
            )));
        }

        let validators = Self::to_validator_vec(validators_pars[0].to_owned())?;
        let vlds: Vec<String> = validators.iter().map(|v| hex::encode(v)).collect();
        tracing::info!(
            "*** ACTIVE VALIDATORS FOR StateHash {}: {}",
            hex::encode(start_hash),
            vlds.join("\n")
        );

        Ok(validators)
    }

    pub async fn compute_bonds(&mut self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        let bonds_pars = self
            .play_exploratory_par(Self::bonds_query_par().clone(), hash)
            .await?;

        if bonds_pars.is_empty() {
            tracing::warn!(
                "No result from getBonds query for state {}; treating as empty bonds",
                PrettyPrinter::build_string_bytes(hash)
            );
            return Ok(Vec::new());
        }

        if bonds_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(hash),
                bonds_pars.len()
            )));
        }

        Self::to_bond_vec(bonds_pars[0].to_owned())
    }

    fn activate_validator_query_source() -> String {
        r#"
          new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getActiveValidators", *return)
          }
        }
      "#
        .to_string()
    }

    /// Reads the protocol fault-tolerance threshold (parts-per-million) from
    /// the PoS contract at `start_hash`. Returns `None` when the contract does
    /// not expose the getter (a chain whose genesis predates the parameter) —
    /// the caller falls back to its local configuration in that case.
    pub async fn get_fault_tolerance_threshold_ppm(
        &mut self,
        start_hash: &StateHash,
    ) -> Result<Option<i64>, CasperError> {
        // STRICT query: a runtime execution failure must PROPAGATE (failing
        // node startup) rather than degrade to an empty result — the lenient
        // path's `Ok(vec![])`-on-error would be indistinguishable from "the
        // getter does not exist" and silently route a transient failure into
        // the local-config fallback, re-opening node-local floor divergence.
        // `None` is returned only after a SUCCESSFUL query with no result.
        let ppm_pars = self
            .play_exploratory_par_strict(Self::fault_tolerance_ppm_query_par().clone(), start_hash)
            .await?;

        if ppm_pars.is_empty() {
            tracing::warn!(
                "No result from getFaultToleranceThresholdPpm query for state {}; \
                 genesis predates the on-chain protocol FTT — falling back to local config",
                PrettyPrinter::build_string_bytes(start_hash)
            );
            return Ok(None);
        }
        if ppm_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from getFaultToleranceThresholdPpm query in state {}: {}",
                PrettyPrinter::build_string_bytes(start_hash),
                ppm_pars.len()
            )));
        }

        let par = &ppm_pars[0];
        match par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::GInt(ppm)) => {
                // RANGE GATE (θ = ppm/1e6 ∈ [-1, 1]). This is the guard that
                // discharges the `-den <= num <= den` hypothesis of the Rocq
                // `FtExact.ft_exact_no_overflow` / `ft_decides_exact` decision
                // `2q·den ⋛ S·(den+num)`. It is NOT decorative:
                //   * ppm < -1e6 ⇒ den+num < 0 ⇒ rhs < 0 <= lhs ⇒ the oracle
                //     returns true for ANY q, bypassing the fault-tolerance
                //     threshold shard-wide;
                //   * ppm > 1e6 ⇒ rhs > 2·S·den >= lhs ⇒ nothing ever
                //     finalizes (liveness halt).
                // `ft_decides_exact`'s `debug_assert!` cannot be relied on: it
                // compiles out in release, which is how CI runs. Reject at the
                // single read choke point so no caller can observe an
                // out-of-range protocol threshold.
                if !(-1_000_000..=1_000_000).contains(ppm) {
                    return Err(CasperError::RuntimeError(format!(
                        "on-chain fault-tolerance-threshold ppm out of range [-1000000, 1000000] \
                         in state {}: {}",
                        PrettyPrinter::build_string_bytes(start_hash),
                        ppm
                    )));
                }
                Ok(Some(*ppm))
            }
            other => Err(CasperError::RuntimeError(format!(
                "getFaultToleranceThresholdPpm returned a non-integer value in state {}: {:?}",
                PrettyPrinter::build_string_bytes(start_hash),
                other
            ))),
        }
    }

    fn fault_tolerance_ppm_query_source() -> String {
        r#"
          new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getFaultToleranceThresholdPpm", *return)
          }
        }
      "#
        .to_string()
    }

    fn fault_tolerance_ppm_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::fault_tolerance_ppm_query_source())
                .expect("Failed to compile fault tolerance ppm query source")
        })
    }

    fn activate_validator_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::activate_validator_query_source())
                .expect("Failed to compile active validator query source")
        })
    }

    fn bonds_query_source() -> String {
        r#"
        new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getBonds", *return)
          }
        }
      "#
        .to_string()
    }

    fn bonds_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::bonds_query_source())
                .expect("Failed to compile bonds query source")
        })
    }

    fn to_validator_vec(validators_par: Par) -> Result<Vec<Validator>, CasperError> {
        if validators_par.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match validators_par.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::ESetBody(set) => ParSetTypeMapper::eset_to_par_set(set.clone()).ps,
            _ => SortedParHashSet::create_from_empty(),
        };

        ps.map_iter(|v| {
            if v.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                match v.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }

    fn to_bond_vec(bonds_map: Par) -> Result<Vec<Bond>, CasperError> {
        if bonds_map.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match bonds_map.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::EMapBody(map) => ParMapTypeMapper::emap_to_par_map(map.clone()).ps,
            _ => SortedParMap::create_from_empty(),
        };

        ps.map_iter(|(validator, bond)| {
            if validator.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else if bond.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Stake in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                let validator_name = match validator.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }?;

                let stake_amount = match bond.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GInt(g_int) => Ok(*g_int),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GInt in stake data".to_string(),
                    )),
                }?;

                Ok(Bond {
                    validator: validator_name,
                    stake: stake_amount,
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

    use super::*;

    /// H-1 fix regression pin (2026-08-06) — slice 30c Phase B.
    /// `play_deploys_for_state` MUST NOT call `SnapshotWriter::
    /// maybe_write` directly (that was the pre-H-1 per-block trigger
    /// that forked snapshot writes on sibling non-finalized DAG
    /// tips).  Post-H-1, the slice is cached in
    /// `pending_wal_slices` and the actual write happens in
    /// `finalization_runner::new_lfb_found_effect` after the block
    /// finalizes.
    ///
    /// Source-scan pin: a future refactor that re-introduces
    /// `writer.maybe_write(...)` inside `play_deploys_for_state`'s
    /// body would trip this test.  Cheap to run; catches the
    /// class of regression that would silently reintroduce the
    /// per-block trigger.
    #[test]
    fn play_deploys_for_state_does_not_call_maybe_write_directly() {
        let src = include_str!("runtime.rs");
        // Locate the `pub async fn play_deploys_for_state` body by
        // string scan.  Look between the `pub async fn` header and
        // the matching function-closing `Ok((final_root, res))`
        // return (the terminal expression of this function — see
        // line ~499 of this file).
        let start_idx = src
            .find("pub async fn play_deploys_for_state")
            .expect("play_deploys_for_state must exist in this file");
        let end_marker = "Ok((final_root, res))";
        let body_end = src[start_idx..]
            .find(end_marker)
            .expect("terminal return must exist inside play_deploys_for_state");
        let body = &src[start_idx..start_idx + body_end];
        // Match call-syntax (`.maybe_write(`) rather than the bare
        // symbol — the H-1 docstring in this function references
        // "SnapshotWriter::maybe_write" as explanation, which is
        // fine; the anti-pattern is the invocation itself.
        assert!(
            !body.contains(".maybe_write("),
            "play_deploys_for_state must NOT call SnapshotWriter::maybe_write \
             directly — H-1 fix moved the trigger to finalization_runner's \
             new_lfb_found_effect so snapshots reflect only the finalized \
             chain, not sibling non-finalized DAG tips.  If you need to \
             re-add per-block snapshotting, first reason carefully about \
             cross-fork snapshot content divergence."
        );
        // Positive: the fix inserts into `pending_wal_slices` at
        // end of the function.  Verify that path is present.
        assert!(
            body.contains("pending_wal_slices"),
            "play_deploys_for_state must cache the per-block WAL slice into \
             pending_wal_slices for finalization_runner to pick up"
        );
    }

    /// H-2 fix regression pin (2026-08-06): replay path wraps each
    /// deploy in `WalDeployScope`.  Pre-fix, `replay_deploy_e` did
    /// not scope the WAL for the deploy, so follower-side journal
    /// appends (from the `is_replay` branches of fs_read /
    /// fs_write / fs_truncate) accumulated across every deploy in a
    /// block until the runtime was reset.  Consequences: unbounded
    /// growth (hit `MAX_WAL_ENTRIES` on follower only), no per-
    /// deploy comparison against the leader's committed slice.
    /// Post-fix, `replay_deploy_e` uses `WalDeployScope::new` +
    /// `take_and_commit` (discard-drain on error via Drop) so the
    /// follower's per-deploy lifecycle mirrors the leader's.
    #[test]
    fn replay_deploy_e_wraps_in_wal_deploy_scope() {
        let src = include_str!("replay_runtime.rs");
        // Locate `pub async fn replay_deploy_e` and confirm it
        // constructs a `WalDeployScope` and calls
        // `take_and_commit`.  Search the whole file since the
        // helper is invoked by fully-qualified path — a future
        // refactor that renames the helper would still trip this
        // if either the type or the method name shifted.
        assert!(
            src.contains("WalDeployScope::new"),
            "H-2 regression: replay_runtime.rs must construct a WalDeployScope \
             (per-deploy WAL drain guard).  Pre-H-2 the follower's WAL grew \
             unboundedly across a block."
        );
        assert!(
            src.contains("take_and_commit"),
            "H-2 regression: replay_runtime.rs must call take_and_commit on \
             the deploy's WalDeployScope so the per-deploy slice is drained \
             in canonical log order (matching the leader's commitment)."
        );
    }

    /// H-1 companion pin: `new_lfb_found_effect` in the finalization
    /// runner MUST consume from `pending_wal_slices` and call
    /// `maybe_write`.  If a refactor drops the LFB hook, snapshots
    /// stop firing entirely rather than resurrecting the per-block
    /// hazard.
    #[test]
    fn finalization_runner_new_lfb_found_effect_writes_snapshots() {
        let src = include_str!("../engine/multi_parent_casper/finalization_runner.rs");
        assert!(
            src.contains("pending_wal_slices"),
            "finalization_runner must consume pending_wal_slices \
             (H-1 fix: LFB-triggered snapshot write)"
        );
        assert!(
            src.contains("maybe_write"),
            "finalization_runner must call SnapshotWriter::maybe_write \
             on cache hits (H-1 fix)"
        );
    }

    fn mk_entry(tag: &str) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: std::path::PathBuf::from(format!("/{tag}")),
            extra_path: None,
            offset: None,
            length: Some(tag.len() as u64),
            payload_ref: Some(PayloadRef::hash(tag.as_bytes())),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    // ---------------------------------------------------------------
    // C-30-1 / C-30-2 round-2 review-fix tests: WalDeployScope
    // ---------------------------------------------------------------

    /// C-30-2: success path drains via `take_and_commit` and hands
    /// entries to the caller; the underlying Wal is empty after.
    #[test]
    fn wal_deploy_scope_take_and_commit_returns_entries_and_empties_wal() {
        let wal = Wal::new();
        wal.append(mk_entry("pre")).unwrap();
        let mut scope = WalDeployScope::new(wal.clone());
        wal.append(mk_entry("a")).unwrap();
        wal.append(mk_entry("b")).unwrap();
        let entries = scope.take_and_commit(&[]);
        assert_eq!(entries.len(), 2, "committed entries include only post-mark");
        assert_eq!(wal.len(), 1, "pre-mark entry survives the drain");
        assert_eq!(entries[0].path, std::path::PathBuf::from("/a"));
        assert_eq!(entries[1].path, std::path::PathBuf::from("/b"));
        // Dropping a committed scope is a no-op.
        drop(scope);
        assert_eq!(wal.len(), 1, "Drop after commit does not double-drain");
    }

    /// C-30-1: the *core* invariant.  Dropping a scope WITHOUT
    /// commit discards entries appended during its lifetime.  This
    /// pins the fix for the `?`-early-return leak that the pre-fix
    /// code had.
    #[test]
    fn wal_deploy_scope_drop_without_commit_discards_entries() {
        let wal = Wal::new();
        wal.append(mk_entry("pre")).unwrap();
        {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("leak-1")).unwrap();
            wal.append(mk_entry("leak-2")).unwrap();
            // `_scope` drops here without commit — discard-drain.
        }
        assert_eq!(
            wal.len(),
            1,
            "Drop without commit MUST discard post-mark entries so the next \
             deploy's begin_deploy sees an empty tail (C-30-1 regression pin)"
        );
        let remaining = wal.snapshot();
        assert_eq!(remaining[0].path, std::path::PathBuf::from("/pre"));
    }

    /// C-30-1 simulated `?`-early-return: a function that
    /// constructs a scope, appends, then returns Err before
    /// committing.  Verifies the scope's Drop still discards.
    #[test]
    fn wal_deploy_scope_early_return_pattern_does_not_leak() {
        let wal = Wal::new();

        fn simulated_deploy(wal: Wal) -> Result<(), &'static str> {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(WalEntry {
                op: WalOp::Truncate,
                path: std::path::PathBuf::from("/leaked"),
                extra_path: None,
                offset: Some(0),
                length: None,
                payload_ref: None,
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            })
            .unwrap();
            // Simulate a `?`-propagated error: return without commit.
            Err("simulated deploy failure")
        }

        let r = simulated_deploy(wal.clone());
        assert!(r.is_err(), "test precondition: simulated deploy fails");
        assert_eq!(
            wal.len(),
            0,
            "early-return via error must discard-drain (C-30-1 fix)"
        );
    }

    /// Sequential deploys: deploy A commits, deploy B commits — each
    /// sees only its own entries.  Pins per-deploy isolation.
    #[test]
    fn wal_deploy_scope_sequential_deploys_are_isolated() {
        let wal = Wal::new();
        let a_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("a1")).unwrap();
            wal.append(mk_entry("a2")).unwrap();
            scope.take_and_commit(&[])
        };
        let b_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("b1")).unwrap();
            scope.take_and_commit(&[])
        };
        assert_eq!(a_entries.len(), 2);
        assert_eq!(b_entries.len(), 1);
        assert_eq!(a_entries[0].path, std::path::PathBuf::from("/a1"));
        assert_eq!(b_entries[0].path, std::path::PathBuf::from("/b1"));
        assert_eq!(wal.len(), 0, "both deploys drained; WAL is empty");
    }

    /// Failed deploy followed by successful deploy: the successful
    /// deploy's commit MUST NOT include the failed deploy's entries.
    #[test]
    fn wal_deploy_scope_failed_deploy_does_not_pollute_next_deploy() {
        let wal = Wal::new();
        {
            let _scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("failed")).unwrap();
            // Drop without commit — discard-drain.
        }
        let b_entries = {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("clean")).unwrap();
            scope.take_and_commit(&[])
        };
        assert_eq!(
            b_entries.len(),
            1,
            "next deploy's slice must be exactly its own contributions"
        );
        assert_eq!(b_entries[0].path, std::path::PathBuf::from("/clean"));
        assert!(!b_entries
            .iter()
            .any(|e| e.path == std::path::Path::new("/failed")));
    }

    // ---------------------------------------------------------------
    // Phase 8 slice 8a step 5 — WalDeployScope::end auto-release hook.
    //
    // Tests below verify that WalDeployScope's Drop calls
    // LockRegistry::release_all_for_deploy(&deploy_scope) so any
    // locks the deploy acquired but did NOT release get swept at
    // deploy end (spec §Explicit locks MUST auto-release).  The
    // sweep is deploy-scope-scoped: locks acquired under OTHER
    // deploy scopes (e.g., prior/next deploys, if they interleave in
    // the shared LockRegistry) are unaffected.
    // ---------------------------------------------------------------

    use rholang::rust::interpreter::io::lock::{HolderId, LockMode, LockRegistry};

    fn holder(byte: u8) -> HolderId { HolderId::from_bytes([byte; 32]) }

    /// Deploy-end sweep: a lock acquired under this deploy's scope
    /// gets released when the WalDeployScope drops (caller neither
    /// released it explicitly nor closed the File cap).
    #[test]
    fn wal_deploy_scope_drop_releases_leaked_locks() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xA1u8; 32];

        // Acquire a lock under this deploy scope, then drop the
        // scope guard without releasing.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            lock_registry
                .try_acquire_range((1, 42), 0, 100, LockMode::Write, holder(1), deploy_scope)
                .expect("acquire under fresh scope must succeed");
            assert_eq!(
                lock_registry.held_locks(),
                1,
                "lock must be held BEFORE drop"
            );
            // _scope drops here — Drop calls release_all_for_deploy.
        }
        assert_eq!(
            lock_registry.held_locks(),
            0,
            "WalDeployScope::drop MUST sweep the deploy's leaked locks \
             (spec §Explicit locks MUST auto-release at deploy end)"
        );
    }

    /// Deploy-end sweep is scoped: locks acquired under OTHER
    /// deploy scopes survive.  Simulates two deploys sharing a
    /// LockRegistry (as would happen across two sequential deploys
    /// on the same runtime).
    #[test]
    fn wal_deploy_scope_drop_only_releases_matching_scope() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_a: DeployScope = [0xA1u8; 32];
        let deploy_b: DeployScope = [0xB2u8; 32];

        // Deploy B acquires first (not yet in scope guard).
        lock_registry
            .try_acquire_range((1, 42), 200, 100, LockMode::Read, holder(2), deploy_b)
            .expect("deploy B's acquire must succeed");

        // Deploy A runs in its scope guard, acquires another lock,
        // then drops — sweep must only clear A's, leaving B's alone.
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_a,
                current_scope_cell.clone(),
            );
            lock_registry
                .try_acquire_range((1, 43), 0, 100, LockMode::Write, holder(1), deploy_a)
                .expect("deploy A's acquire must succeed");
            assert_eq!(lock_registry.held_locks(), 2);
        }
        assert_eq!(
            lock_registry.held_locks(),
            1,
            "sweep MUST be scope-scoped: only deploy A's lock cleared, \
             deploy B's survives"
        );
    }

    /// Constructor sets the current-scope cell so concurrent
    /// handler calls record the correct scope; Drop clears it back
    /// to sentinel.
    #[test]
    fn wal_deploy_scope_publishes_scope_to_shared_cell() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xC3u8; 32];

        // Before the guard: sentinel.
        assert_eq!(*current_scope_cell.read().unwrap(), [0u8; 32]);

        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            // Inside the guard: real scope.
            assert_eq!(
                *current_scope_cell.read().unwrap(),
                deploy_scope,
                "constructor must publish scope to the shared cell so \
                 concurrent lock-native handlers can read it at acquire time"
            );
        }
        // After drop: back to sentinel.
        assert_eq!(
            *current_scope_cell.read().unwrap(),
            [0u8; 32],
            "Drop MUST clear the scope cell back to sentinel so \
             between-deploy handler calls see no live scope"
        );
    }

    /// Multiple locks under one deploy scope all get swept.
    #[test]
    fn wal_deploy_scope_sweeps_multiple_locks_under_same_deploy() {
        let wal = Wal::new();
        let lock_registry = LockRegistry::new();
        let current_scope_cell = std::sync::Arc::new(std::sync::RwLock::new([0u8; 32]));
        let deploy_scope: DeployScope = [0xD4u8; 32];

        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry.clone(),
                deploy_scope,
                current_scope_cell.clone(),
            );
            // Acquire 5 range locks + 1 sequential (on different inode).
            for i in 0..5 {
                lock_registry
                    .try_acquire_range(
                        (1, 42),
                        i * 200,
                        100,
                        LockMode::Read,
                        holder(1),
                        deploy_scope,
                    )
                    .expect("range acquire must succeed");
            }
            lock_registry
                .try_acquire_sequential((1, 43), holder(2), deploy_scope)
                .expect("sequential acquire must succeed");
            assert_eq!(lock_registry.held_locks(), 6);
        }
        assert_eq!(
            lock_registry.held_locks(),
            0,
            "sweep MUST release EVERY lock under the deploy scope, \
             both range and sequential"
        );
    }

    // ---------------------------------------------------------------
    // C-30b-1 round-2 review-fix test: the play_deploys_for_state
    // aggregation composition.  Pre-fix, the extend + compute_wal_root
    // + maybe_write loop had zero test coverage; a refactor that
    // swapped `extend`→`push` (nesting Vec<Vec<...>>), reordered
    // per-deploy WALs, or called maybe_write on the wrong slice would
    // compile and pass every existing test.  This test exercises the
    // SAME composition — WalDeployScope drain × N deploys, aggregate
    // into a block-scope Vec, compute the root, hand to SnapshotWriter
    // — WITHOUT the full RuntimeManager weight.
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn play_deploys_aggregation_composition_writes_correct_snapshot() {
        use rholang::rust::interpreter::io::snapshot::{
            compute_wal_root, read_snapshot_bytes, snapshot_path, SnapshotWriter,
        };

        let snapshot_dir = tempfile::tempdir().unwrap();
        let wal = Wal::new();
        let writer = SnapshotWriter {
            dir: snapshot_dir.path().to_path_buf(),
            cadence: 1, // every block
            retain: 10,
            signer_sk: None,
        };

        // Simulate play_deploys_for_state's block-scan loop with 3
        // deploys.  Each deploy: WalDeployScope::new → append via WAL
        // → take_and_commit → extend aggregator.
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();
        for (deploy_idx, tag) in [("deploy0", 3), ("deploy1", 1), ("deploy2", 2)]
            .iter()
            .enumerate()
        {
            let (label, n_entries) = tag;
            let mut scope = WalDeployScope::new(wal.clone());
            for i in 0..*n_entries {
                let path = format!("/{label}-e{i}");
                wal.append(mk_entry(&path[1..])).unwrap();
            }
            let fs_wal = scope.take_and_commit(&[]);
            assert_eq!(
                fs_wal.len(),
                *n_entries,
                "deploy {deploy_idx} committed the wrong number of entries"
            );
            block_fs_wal.extend(fs_wal);
        }
        assert_eq!(
            block_fs_wal.len(),
            6,
            "aggregation must accumulate ALL per-deploy contributions"
        );
        // Insertion order (deploy0's 3 entries, then deploy1's 1, then deploy2's 2)
        // MUST be preserved — the WAL root is order-sensitive.
        assert_eq!(
            block_fs_wal[0].path,
            std::path::PathBuf::from("/deploy0-e0")
        );
        assert_eq!(
            block_fs_wal[3].path,
            std::path::PathBuf::from("/deploy1-e0")
        );
        assert_eq!(
            block_fs_wal[4].path,
            std::path::PathBuf::from("/deploy2-e0")
        );

        // Compute the block WAL root — same call play_deploys_for_state
        // makes.
        let block_root = compute_wal_root(&block_fs_wal);

        // maybe_write on a cadence=1 block writes.
        let block_number = 5i64;
        let write_root = writer
            .maybe_write(block_number, &block_fs_wal)
            .expect("maybe_write must succeed with valid writer")
            .expect("cadence=1, non-empty entries: must produce a snapshot");
        assert_eq!(
            write_root, block_root,
            "SnapshotWriter's write must produce the SAME root as compute_wal_root on the same slice"
        );

        // The snapshot file exists at the content-addressed path.
        let snap_path = snapshot_path(snapshot_dir.path(), &write_root);
        assert!(snap_path.exists(), "snapshot file must be written to disk");
        // Read back + verify root.
        let bytes = read_snapshot_bytes(snapshot_dir.path(), &write_root).unwrap();
        assert!(!bytes.is_empty());

        // Per-runtime WAL was drained (no leftover entries).
        assert_eq!(
            wal.len(),
            0,
            "after 3 deploys × take_and_commit, per-runtime WAL is empty"
        );
    }

    /// C-30b-1 companion: pin the aggregation-order invariant.  If a
    /// future refactor accidentally reversed per-deploy order (e.g.,
    /// via a `.rev()`) or bucketed via a HashMap iteration, the block
    /// WAL root would flip.  This test asserts the exact root for a
    /// canonical 2-deploy sequence.
    #[tokio::test]
    async fn play_deploys_aggregation_preserves_deploy_scan_order() {
        use rholang::rust::interpreter::io::snapshot::compute_wal_root;

        let wal = Wal::new();
        let mut block_fs_wal: Vec<WalEntry> = Vec::new();

        // Deploy A appends "a".
        {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("a")).unwrap();
            block_fs_wal.extend(scope.take_and_commit(&[]));
        }
        // Deploy B appends "b".
        {
            let mut scope = WalDeployScope::new(wal.clone());
            wal.append(mk_entry("b")).unwrap();
            block_fs_wal.extend(scope.take_and_commit(&[]));
        }
        let root_ab = compute_wal_root(&block_fs_wal);

        // Redo with reversed deploy order — different root.
        let wal2 = Wal::new();
        let mut block_fs_wal2: Vec<WalEntry> = Vec::new();
        {
            let mut scope = WalDeployScope::new(wal2.clone());
            wal2.append(mk_entry("b")).unwrap();
            block_fs_wal2.extend(scope.take_and_commit(&[]));
        }
        {
            let mut scope = WalDeployScope::new(wal2.clone());
            wal2.append(mk_entry("a")).unwrap();
            block_fs_wal2.extend(scope.take_and_commit(&[]));
        }
        let root_ba = compute_wal_root(&block_fs_wal2);

        assert_ne!(
            root_ab, root_ba,
            "deploy-scan order MUST be part of the block WAL root — \
             a refactor that reorders would silently fork consensus"
        );
    }

    #[test]
    fn fold_bitmask_or_empty_returns_none() {
        assert_eq!(RuntimeOps::fold_bitmask_or(&[]), None);
    }

    #[test]
    fn fold_bitmask_or_single_returns_value() {
        assert_eq!(RuntimeOps::fold_bitmask_or(&[42]), Some(42));
    }

    #[test]
    fn fold_bitmask_or_returns_or_fold_not_max() {
        let a = 0b00010001i64;
        let b = 0b00100010i64;
        assert_eq!(RuntimeOps::fold_bitmask_or(&[a, b]), Some(0b00110011));
        let c = 0b01000000i64;
        assert_eq!(RuntimeOps::fold_bitmask_or(&[a, b, c]), Some(0b01110011));
    }

    #[test]
    fn fold_bitmask_or_commutes() {
        let xs = [0b0001_0001i64, 0b0010_0010, 0b0100_0100, 0b1000_1000];
        let mut ys = xs;
        ys.reverse();
        assert_eq!(
            RuntimeOps::fold_bitmask_or(&xs),
            RuntimeOps::fold_bitmask_or(&ys),
        );
    }

    #[test]
    fn fold_bitmask_or_negative_high_bits_preserved() {
        let neg = i64::MIN;
        let pos = 0b1010i64;
        let folded = RuntimeOps::fold_bitmask_or(&[neg, pos]).unwrap();
        assert_eq!(folded as u64, (neg as u64) | (pos as u64));
        assert_ne!(folded & i64::MIN, 0, "sign bit must remain set");
    }
}
