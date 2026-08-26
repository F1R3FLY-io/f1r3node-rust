// See rholang/src/main/scala/coop/rchain/rholang/interpreter/RhoRuntime.scala
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance::{EMapBody, GByteArray};
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, Bundle, CostAuthority, Expr, ListParWithRandom, Par, TaggedContinuation, Var,
};
use models::rust::block_hash::BlockHash;
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::new_freevar_par;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace_interface::IReplayRSpace;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceAccountingObserver};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use rspace_plus_plus::rspace::trace::Log;
use rspace_plus_plus::rspace::tuplespace_interface::Tuplespace;

use super::accounting::authority::ResourceMultiset;
use super::accounting::cost_accounting::CostAccounting;
use super::accounting::costs::Cost;
use super::accounting::has_cost::HasCost;
use super::accounting::{BillableTokenEvent, RuntimeBudget};
use super::dispatch::{RhoDispatch, RholangAndScalaDispatcher};
use super::env::Env;
use super::errors::InterpreterError;
use super::interpreter::{EvaluateResult, Interpreter, InterpreterImpl};
use super::reduce::DebruijnInterpreter;
use super::registry::registry_bootstrap::ast;
use super::substitute::Substitute;
use super::system_processes::{
    Arity, BlockData, BodyRef, Definition, DeployData, InvalidBlocks, Name, ProcessContext,
    Remainder, RhoDispatchMap, SystemProcesses,
};
use crate::rust::interpreter::chromadb_service::SharedChromaDBService;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::grpc_client_service::GrpcClientService;
use crate::rust::interpreter::metrics_constants::{
    CREATE_CHECKPOINT_TIME_METRIC, CREATE_SOFT_CHECKPOINT_TIME_METRIC, EVALUATE_TIME_METRIC,
    RUNTIME_CHECKPOINT_TOTAL_METRIC, RUNTIME_METRICS_SOURCE,
    RUNTIME_REVERT_SOFT_CHECKPOINT_TOTAL_METRIC, RUNTIME_SOFT_CHECKPOINT_TOTAL_METRIC,
    RUNTIME_TAKE_EVENT_LOG_EVENTS_TOTAL_METRIC, RUNTIME_TAKE_EVENT_LOG_LAST_EVENTS_METRIC,
    RUNTIME_TAKE_EVENT_LOG_TOTAL_METRIC,
};
use crate::rust::interpreter::ollama_service::SharedOllamaService;
use crate::rust::interpreter::openai_service::SharedOpenAIService;
use crate::rust::interpreter::system_processes::{BodyRefs, FixedChannels};

/*
 * This trait has been combined with the 'ReplayRhoRuntime' trait
*/
#[allow(async_fn_in_trait)]
pub trait RhoRuntime: HasCost {
    /**
     * Parse the rholang term into [[coop.rchain.models.Par]] and execute it with provided initial phlo.
     *
     * This function would change the state in the runtime.
     * @param term The rholang contract which would run on the runtime
     * @param initialPhlo initial cost for the this evaluation. If the phlo is not enough,
     *                    [[coop.rchain.rholang.interpreter.errors.OutOfPhlogistonsError]] would return.
     * @param normalizerEnv additional env for Par when parsing term into Par
     * @param rand random seed for rholang execution
     * @return
     */
    async fn evaluate(
        &self,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
    ) -> Result<EvaluateResult, InterpreterError>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/RhoRuntimeSyntax.scala
    async fn evaluate_with_env(
        &mut self,
        term: &str,
        normalizer_env: HashMap<String, Par>,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.evaluate_with_env_and_phlo(term, Cost::unsafe_max(), normalizer_env)
            .await
    }

    async fn evaluate_with_term(&mut self, term: &str) -> Result<EvaluateResult, InterpreterError> {
        self.evaluate_with_env_and_phlo(term, Cost::unsafe_max(), HashMap::new())
            .await
    }

    async fn evaluate_with_phlo(
        &mut self,
        term: &str,
        initial_phlo: Cost,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.evaluate_with_env_and_phlo(term, initial_phlo, HashMap::new())
            .await
    }

    async fn evaluate_with_env_and_phlo(
        &mut self,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
    ) -> Result<EvaluateResult, InterpreterError> {
        let rand = Blake2b512Random::create_from_length(128);
        let checkpoint = self.create_soft_checkpoint().await;
        match self
            .evaluate(term, initial_phlo, normalizer_env, rand)
            .await
        {
            Ok(eval_result) => {
                if !eval_result.errors.is_empty() {
                    self.revert_to_soft_checkpoint(checkpoint).await;
                    Ok(eval_result)
                } else {
                    Ok(eval_result)
                }
            }
            Err(err) => {
                self.revert_to_soft_checkpoint(checkpoint).await;
                Err(err)
            }
        }
    }

    /**
     * Inject an already-normalized process into the current runtime state.
     *
     * Ordinary user deploys must enter through `evaluate`, which constructs the
     * signed metered process and initializes the token budget. This lower-level
     * entry point is kept for tests, bootstrap, and system paths that have
     * already selected their budget mode explicitly.
     */
    async fn inj(
        &self,
        par: Par,
        env: Env<Par>,
        rand: Blake2b512Random,
    ) -> Result<(), InterpreterError>;

    /**
     * After some executions([[evaluate]]) on the runtime, you can create a soft checkpoint which is the changes
     * for the current state of the runtime. You can revert the changes by [[revertToSoftCheckpoint]]
     * @return
     */
    async fn create_soft_checkpoint(
        &mut self,
    ) -> SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    /// Drain and return runtime event log without cloning hot-store state.
    async fn take_event_log(&mut self) -> Log;

    /// Return current runtime root hash without creating a checkpoint.
    async fn get_root(&self) -> Blake2b256Hash;

    async fn revert_to_soft_checkpoint(
        &mut self,
        soft_checkpoint: SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> ();

    /**
     * Create a checkpoint for the runtime. All the changes which happened in the runtime would persistent in the disk
     * and result in a new stateHash for the new state.
     * @return
     */
    async fn create_checkpoint(&mut self) -> Checkpoint;

    /**
     * Reset the runtime to the specific state. Then you can operate some execution on the state.
     * @param root the target state hash to reset
     * @return
     */
    async fn reset(&mut self, root: &Blake2b256Hash) -> Result<(), InterpreterError>;

    /**
     * Consume the result in the rspace.
     *
     * This function would change the state in the runtime.
     * @param channel target channel for the consume
     * @param pattern pattern for the consume
     * @return
     */
    async fn consume_result(
        &mut self,
        channel: Vec<Par>,
        pattern: Vec<BindPattern>,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, InterpreterError>;

    /**
     * get data directly from history repository
     *
     * This function would not change the state in the runtime
     */
    async fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>>;

    async fn get_joins(&self, channel: Par) -> Vec<Vec<Par>>;

    /**
     * get continuation directly from history repository
     *
     * This function would not change the state in the runtime
     */
    async fn get_continuations(
        &self,
        channels: Vec<Par>,
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>>;

    /**
     * Set the runtime block data environment.
     */
    async fn set_block_data(&self, block_data: BlockData) -> ();

    /**
     * Set the runtime invalid blocks environment.
     */
    async fn set_invalid_blocks(&self, invalid_blocks: HashMap<BlockHash, Validator>) -> ();

    /**
     * Set the runtime deploy data environment.
     */
    async fn set_deploy_data(&self, deploy_data: DeployData) -> ();

    /**
     * Get the hot changes after some executions for the runtime.
     * Currently this is only for debug info mostly.
     */
    async fn get_hot_changes(
        &self,
    ) -> HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>>;

    /* Replay functions */

    async fn rig(&self, log: Log) -> Result<(), InterpreterError>;

    async fn check_replay_data(&self) -> Result<(), InterpreterError>;
}

/*
 * We use this struct for both normal and replay RhoRuntime instances
*/
#[derive(Clone)]
pub struct RhoRuntimeImpl {
    pub reducer: Arc<DebruijnInterpreter>,
    pub cost: RuntimeBudget,
    pub block_data_ref: Arc<tokio::sync::RwLock<BlockData>>,
    pub invalid_blocks_param: InvalidBlocks,
    pub deploy_data_ref: Arc<tokio::sync::RwLock<DeployData>>,
    pub merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
    /// Per-runtime File I/O fd table (spec §Phase 1).  Shared with every
    /// `FsProcesses` handler via `Arc` under the hood so fds opened by
    /// `fs_open` are visible to `fs_read` / `fs_close` etc.
    pub fs_handles: super::io::handle_table::FileHandleTable,
    /// Stack of fd-counter snapshots captured at soft-checkpoint
    /// time.  On revert we pop the innermost snapshot and truncate
    /// the fd table to it, freeing every fd allocated after the
    /// checkpoint (spec §Fd-table lifecycle).  Stack (not single
    /// slot) so nested `create_soft_checkpoint` calls preserve the
    /// outer marks — H4/M1 review fix (slice 29 round 2).  Pre-fix
    /// design was `Option<u64>` which silently dropped the outer
    /// mark on the inner `create`.
    fs_snapshot_stack: Arc<std::sync::Mutex<Vec<u64>>>,
    /// Stack of WAL length snapshots captured at soft-checkpoint
    /// time.  On revert we pop the innermost mark and truncate the
    /// WAL back to it, discarding any entries appended during the
    /// failed deploy.  Prevents divergence where a leader's
    /// reverted-but-journaled write would be replayed by followers.
    /// H-29-1 review fix; stack semantics from H4/M1 round-2 fix.
    wal_snapshot_stack: Arc<std::sync::Mutex<Vec<super::io::wal::WalMark>>>,
    /// Slice 30b (H-30b-2 round-2 fix): optional snapshot writer,
    /// configured at boot from `storage.consensus-fs-snapshot-
    /// {cadence,dir}`.  `None` (inside the RwLock) when the
    /// operator has no consensus-static provisioning.
    ///
    /// **Arc-shared with `RuntimeManager.fs_snapshot_writer`:**
    /// `RuntimeManager::spawn_runtime` clones ITS Arc into this
    /// field via `share_fs_snapshot_writer`, so every runtime
    /// spawned by a given manager reads from the SAME `RwLock`.
    /// A boot-time `set_fs_snapshot_writer` on the manager is
    /// immediately visible to every spawned runtime — closes the
    /// pre-fix H-30b-2 race where each spawn cached a per-runtime
    /// snapshot of the writer value.
    ///
    /// `play_deploys_for_state` reads via `.read().await` on every
    /// call.  The lock is a read-write lock (tokio's) so many
    /// runtimes can read concurrently; only boot-time set is a
    /// writer.
    pub fs_snapshot_writer: Arc<tokio::sync::RwLock<Option<super::io::snapshot::SnapshotWriter>>>,

    /// H-1 fix (2026-08-06) — slice 30c Phase B: per-block WAL slice
    /// cache, keyed by post-state hash (`Vec<u8>`).  Populated by
    /// `casper::rholang::runtime::play_deploys_for_state` after
    /// computing the per-block slice; consumed by the finalization
    /// runner's LFB-found effect.  Pre-H-1 the snapshot was written
    /// synchronously per candidate block (fork-prone); post-H-1 the
    /// slice is cached under the block's post-state hash and only
    /// snapshotted when the block actually finalizes and hits a
    /// cadence boundary.
    ///
    /// Same sharing pattern as `fs_snapshot_writer`:
    /// `RuntimeManager::spawn_runtime` calls
    /// `share_pending_wal_slices` so every spawned runtime writes
    /// into the manager's shared cache.
    pub pending_wal_slices: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<Vec<u8>, (i64, Vec<super::io::wal::WalEntry>)>,
        >,
    >,
}

impl RhoRuntimeImpl {
    #[allow(clippy::too_many_arguments)]
    fn new(
        reducer: Arc<DebruijnInterpreter>,
        cost: RuntimeBudget,
        block_data_ref: Arc<tokio::sync::RwLock<BlockData>>,
        invalid_blocks_param: InvalidBlocks,
        deploy_data_ref: Arc<tokio::sync::RwLock<DeployData>>,
        merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
        fs_handles: super::io::handle_table::FileHandleTable,
    ) -> RhoRuntimeImpl {
        RhoRuntimeImpl {
            reducer,
            cost,
            block_data_ref,
            invalid_blocks_param,
            deploy_data_ref,
            merge_chs,
            fs_handles,
            fs_snapshot_stack: Arc::new(std::sync::Mutex::new(Vec::new())),
            wal_snapshot_stack: Arc::new(std::sync::Mutex::new(Vec::new())),
            // Slice 30b: default None inside a fresh RwLock.  Boot
            // typically replaces via `share_fs_snapshot_writer`
            // (RuntimeManager path) so all sibling runtimes read the
            // same slot.
            fs_snapshot_writer: Arc::new(tokio::sync::RwLock::new(None)),
            // H-1 (2026-08-06): per-runtime empty cache by default.
            // `RuntimeManager::spawn_runtime` overwrites with a
            // shared handle so every spawned runtime writes into
            // the manager's cache for the finalization runner to
            // consume.
            pending_wal_slices: Arc::new(
                tokio::sync::RwLock::new(std::collections::HashMap::new()),
            ),
        }
    }

    /// Slice 30b boot hook: attach a snapshot writer to THIS
    /// runtime only.  Acquires the internal write lock.  Used by
    /// tests and standalone runtimes that aren't spawned via a
    /// `RuntimeManager`; production spawn goes through
    /// `share_fs_snapshot_writer` instead so multiple runtimes
    /// share the same Arc.
    pub async fn set_fs_snapshot_writer(
        &self,
        writer: Option<super::io::snapshot::SnapshotWriter>,
    ) {
        *self.fs_snapshot_writer.write().await = writer;
    }

    /// H-30b-2 round-2 fix: replace this runtime's writer slot
    /// with a shared `Arc<RwLock<...>>` so all runtimes spawned
    /// from the same `RuntimeManager` read live from the same
    /// source.  Called by `RuntimeManager::spawn_runtime`.
    pub fn share_fs_snapshot_writer(
        &mut self,
        shared: Arc<tokio::sync::RwLock<Option<super::io::snapshot::SnapshotWriter>>>,
    ) {
        self.fs_snapshot_writer = shared;
    }

    /// H-1 (2026-08-06) — slice 30c Phase B: share the manager's
    /// pending-WAL-slice cache so every spawned runtime writes its
    /// per-block WAL slice into a single map keyed by post-state
    /// hash.  The finalization runner reads from the same map when
    /// the LFB advances.
    pub fn share_pending_wal_slices(
        &mut self,
        shared: Arc<
            tokio::sync::RwLock<
                std::collections::HashMap<Vec<u8>, (i64, Vec<super::io::wal::WalEntry>)>,
            >,
        >,
    ) {
        self.pending_wal_slices = shared;
    }

    pub fn get_cost_log(&self) -> Vec<Cost> { self.cost.get_log() }

    pub fn get_cost_event_log(&self) -> Vec<BillableTokenEvent> { self.cost.get_event_log() }

    pub fn clear_cost_log(&self) { self.cost.clear_log() }

    pub fn clear_cost_event_log(&self) { self.cost.clear_event_log() }

    pub async fn evaluate_with_authority(
        &self,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
        authority_allocation: Option<ResourceMultiset<[u8; 32]>>,
    ) -> Result<EvaluateResult, InterpreterError> {
        let start = Instant::now();
        let interpreter = InterpreterImpl::new(self.cost.clone(), self.merge_chs.clone());
        let result = interpreter
            .inj_attempt(
                &self.reducer,
                term,
                initial_phlo,
                normalizer_env,
                rand,
                authority_allocation,
            )
            .await;
        metrics::histogram!(EVALUATE_TIME_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());
        result
    }

    /// Slice 31: enable the rho:io:fs:native:* URN filter.  Every
    /// subsequent `new x(rho:io:fs:native:.../*)` inside a deploy
    /// returns `ReduceError` from `eval_new`.  This is the default
    /// state.  Idempotent.
    pub fn enable_fs_native_urn_filter(&self) {
        self.reducer
            .filter_fs_native_urns
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Slice 31: disable the rho:io:fs:native:* URN filter for the
    /// duration of a genesis-composition run.  MUST be re-enabled
    /// via `enable_fs_native_urn_filter` after the genesis deploys
    /// complete — leaving it disabled would expose raw fs syscalls
    /// to every subsequent user deploy on this runtime.
    pub fn disable_fs_native_urn_filter(&self) {
        self.reducer
            .filter_fs_native_urns
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// H-P7-5 review fix (Phase 7 whole-review round): RAII exemption
    /// guard for the `rho:io:fs:native:*` URN filter.  Disables the
    /// filter on construction and re-enables on Drop — including
    /// panics and tokio-task cancellation.  See FsNativeUrnFilterExemption
    /// below for the guard's lifetime contract.
    pub fn exempt_fs_native_urn_filter(&self) -> FsNativeUrnFilterExemption {
        self.disable_fs_native_urn_filter();
        FsNativeUrnFilterExemption {
            flag: self.reducer.filter_fs_native_urns.clone(),
        }
    }

    /// Slice 31: introspection helper for tests and diagnostics.
    pub fn fs_native_urn_filter_enabled(&self) -> bool {
        self.reducer
            .filter_fs_native_urns
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

/// H-P7-5 review fix: RAII drop-guard for the
/// `rho:io:fs:native:*` URN-filter exemption granted by
/// `RhoRuntimeImpl::exempt_fs_native_urn_filter`.  Drop re-enables
/// the filter on all exit paths (normal return, `?`-error, panic
/// unwind).  Holds an `Arc<AtomicBool>` clone of the filter flag
/// (not a borrow of the runtime) so the caller can still exercise
/// `&mut self` on the runtime during the exemption.
#[must_use = "the exemption ends when the guard is dropped; letting it drop \
              immediately after construction re-enables the filter and \
              the enclosed code sees the filter ON"]
pub struct FsNativeUrnFilterExemption {
    flag: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FsNativeUrnFilterExemption {
    fn drop(&mut self) { self.flag.store(true, std::sync::atomic::Ordering::Release); }
}

impl RhoRuntime for RhoRuntimeImpl {
    async fn evaluate(
        &self,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
    ) -> Result<EvaluateResult, InterpreterError> {
        self.evaluate_with_authority(term, initial_phlo, normalizer_env, rand, None)
            .await
    }

    async fn inj(
        &self,
        par: Par,
        _env: Env<Par>,
        rand: Blake2b512Random,
    ) -> Result<(), InterpreterError> {
        let res = self.reducer.inj(par, rand).await;
        res
    }

    async fn create_soft_checkpoint(
        &mut self,
    ) -> SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        let start = Instant::now();
        let checkpoint = self.reducer.space.create_soft_checkpoint().await;
        // Snapshot the fd counter so an evaluation error can roll back
        // any opens issued during the deploy — spec §Phase 1 fd-table
        // lifecycle.  Monotonic counter guarantees no fd aliasing across
        // rollback boundaries.
        // H4/M1 review fix (round 2): PUSH onto a stack rather than
        // overwriting a single slot, so nested soft-checkpoints
        // preserve outer marks.  Revert POPs the innermost.
        {
            let mut stack = self.fs_snapshot_stack.lock().unwrap();
            stack.push(self.fs_handles.snapshot_next_fd());
        }
        // H-29-1 review fix: snapshot the consensus WAL length
        // alongside the fd counter so revert can truncate both.
        {
            let mut stack = self.wal_snapshot_stack.lock().unwrap();
            stack.push(self.fs_handles.wal.snapshot_mark());
        }
        metrics::histogram!(CREATE_SOFT_CHECKPOINT_TIME_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());
        metrics::counter!(RUNTIME_SOFT_CHECKPOINT_TOTAL_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .increment(1);
        checkpoint
    }

    async fn take_event_log(&mut self) -> Log {
        let log = self.reducer.space.take_event_log().await;
        let log_len = log.len() as u64;
        metrics::counter!(RUNTIME_TAKE_EVENT_LOG_TOTAL_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(
            RUNTIME_TAKE_EVENT_LOG_EVENTS_TOTAL_METRIC,
            "source" => RUNTIME_METRICS_SOURCE
        )
        .increment(log_len);
        metrics::gauge!(
            RUNTIME_TAKE_EVENT_LOG_LAST_EVENTS_METRIC,
            "source" => RUNTIME_METRICS_SOURCE
        )
        .set(log_len as f64);
        log
    }

    async fn get_root(&self) -> Blake2b256Hash { self.reducer.space.get_root().await }

    async fn revert_to_soft_checkpoint(
        &mut self,
        soft_checkpoint: SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> () {
        metrics::counter!(
            RUNTIME_REVERT_SOFT_CHECKPOINT_TOTAL_METRIC,
            "source" => RUNTIME_METRICS_SOURCE
        )
        .increment(1);
        // Roll back the fd table to the snapshot captured at
        // create_soft_checkpoint time.  Any fds opened during the failed
        // eval are closed and removed; the monotonic counter is not
        // rewound so stale fds observed by any caller reliably see
        // FSERR_CLOSED rather than aliasing a later open.
        // H4/M1 round-2 fix: POP the innermost snapshot from the stack
        // so nested checkpoints unwind correctly.  A revert without a
        // matching create is a no-op (defensive against unbalanced calls).
        let snap = { self.fs_snapshot_stack.lock().unwrap().pop() };
        if let Some(s) = snap {
            self.fs_handles.truncate_to(s).await;
        }
        // H-29-1: same stack semantics for WAL rollback.
        let wal_snap = { self.wal_snapshot_stack.lock().unwrap().pop() };
        if let Some(mark) = wal_snap {
            self.fs_handles.wal.truncate_to(mark);
        }
        self.reducer
            .space
            .revert_to_soft_checkpoint(soft_checkpoint)
            .await
            .unwrap()
    }

    async fn create_checkpoint(&mut self) -> Checkpoint {
        let start = Instant::now();
        let checkpoint = self.reducer.space.create_checkpoint().await.unwrap();
        metrics::histogram!(CREATE_CHECKPOINT_TIME_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());
        metrics::counter!(RUNTIME_CHECKPOINT_TOTAL_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .increment(1);
        checkpoint
    }

    async fn reset(&mut self, root: &Blake2b256Hash) -> Result<(), InterpreterError> {
        self.reducer.space.reset(root).await?;
        // Slice 28 (PB-M-13): seed FileHandleTable::next_fd from the
        // state root.  Every block boundary triggers a reset via
        // this path (see `casper::rholang::runtime::play_deploys_for_state`,
        // `play_deploys_for_genesis`, `play_system_deploy`,
        // `play_exploratory_par_with_mode`), and every validator
        // resetting to the same root computes the same watermark —
        // so fd values captured by the leader are reproducibly
        // replayable by followers.
        //
        // **Consensus commitment (M-28-F2 review note):** fd values
        // are consensus-observable via Rholang tuplespace state
        // (`fdP` cells inside File agents).  This seed derivation
        // is therefore an implicit consensus commitment — any
        // future change to `seed_next_fd_from_state_hash`'s
        // derivation constants or hash algorithm is a hard fork.
        //
        // **Aliasing-prevention claim (M-28-2 clarification):**
        // slice 28 prevents post-restart fd aliasing by ensuring
        // that a fresh runtime spawned per block (via
        // `RuntimeManager::spawn_runtime`) starts allocation from
        // the state-hash-derived watermark, NOT from `next_fd = 1`.
        // Two independent runtimes at the same state hash allocate
        // identical fd sequences (leader/follower replay).  Cross-
        // block aliasing risk is not "guaranteed impossible" but is
        // statistically negligible given the 44-bit entropy
        // derivation and 20-bit per-lifetime headroom (see
        // `handle_table.rs::FD_ENTROPY_HEADROOM_BITS` and
        // `seed_next_fd_from_state_hash`).
        //
        // **Side-effect note (M-28-F1):** `reset()` mutates
        // `fs_handles.next_fd` — a side-effect that predates slice
        // 28 (via `snapshot_next_fd` / `truncate_to` on soft
        // checkpoints).  All current callers are consensus-relevant
        // paths that spawn a fresh runtime for their scope, so the
        // seeding side-effect is contained.
        self.fs_handles.seed_next_fd_from_state_hash(&root.bytes());
        // Streaming-backing slice Step 3 review-fix (2026-08-25):
        // seed the dir-stream fd counter from the same state hash.
        // Same PB-M-13 aliasing threat as file fds: dir-stream fd
        // values flow through the tuplespace as GInt, so a joining
        // validator (or restart-after-crash) allocating fresh
        // dir-stream fds starting from 1 could alias a stream fd
        // that a prior lifetime stashed in the tuplespace state.
        // Missing this call means the streaming primitive is
        // sound within a single runtime lifetime but unsound
        // across a full restart — a real gap once Dir.rho starts
        // holding stream fds across block boundaries (Step 5).
        self.fs_handles
            .dir_handles
            .seed_next_fd_from_state_hash(&root.bytes());
        // H-29-F2 review fix (defense in depth): clear the consensus
        // WAL on reset.  All correctness paths already drain the WAL
        // per-deploy via `Wal::take_deploy_entries`; this clear
        // guarantees that if a caller resets to a state root without
        // first draining, the follower observes an empty WAL — no
        // ghost entries from an earlier block leak into the next.
        self.fs_handles.wal.clear();
        // M6 round-2 fix: also clear stashed checkpoint marks so a
        // subsequent revert doesn't pop a stale mark (which would
        // truncate the fd table to a pre-reset watermark or the WAL
        // to a length below the cleared zero).  A reset semantically
        // means "start fresh at this state root"; leaving a mark
        // stashed is inconsistent with that.
        self.fs_snapshot_stack.lock().unwrap().clear();
        self.wal_snapshot_stack.lock().unwrap().clear();
        Ok(())
    }

    async fn consume_result(
        &mut self,
        channel: Vec<Par>,
        pattern: Vec<BindPattern>,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, InterpreterError> {
        Ok(self.reducer.space.consume_result(channel, pattern).await?)
    }

    async fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
        self.reducer.space.get_data(channel).await
    }

    async fn get_joins(&self, channel: Par) -> Vec<Vec<Par>> {
        self.reducer.space.get_joins(channel).await
    }

    async fn get_continuations(
        &self,
        channels: Vec<Par>,
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
        self.reducer.space.get_waiting_continuations(channels).await
    }

    async fn set_block_data(&self, block_data: BlockData) -> () {
        let mut lock = self.block_data_ref.write().await;
        *lock = block_data;
    }

    async fn set_deploy_data(&self, deploy_data: DeployData) -> () {
        let mut lock = self.deploy_data_ref.write().await;
        *lock = deploy_data;
    }

    async fn set_invalid_blocks(&self, invalid_blocks: HashMap<BlockHash, Validator>) -> () {
        let invalid_blocks: Par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(EMapBody(ParMapTypeMapper::par_map_to_emap(
                ParMap::create_from_sorted_par_map(SortedParMap::create_from_map(
                    invalid_blocks
                        .into_iter()
                        .map(|(validator, block_hash)| {
                            (
                                Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(GByteArray(validator.into())),
                                }]),
                                Par::default().with_exprs(vec![Expr {
                                    expr_instance: Some(GByteArray(block_hash.into())),
                                }]),
                            )
                        })
                        .collect(),
                )),
            ))),
        }]);

        self.invalid_blocks_param.set_params(invalid_blocks).await
    }

    async fn get_hot_changes(
        &self,
    ) -> HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>> {
        self.reducer.space.to_map().await
    }

    async fn rig(&self, log: Log) -> Result<(), InterpreterError> {
        self.reducer.space.rig(log).await?;
        Ok(())
    }

    async fn check_replay_data(&self) -> Result<(), InterpreterError> {
        self.reducer.space.check_replay_data().await?;
        Ok(())
    }
}

impl HasCost for RhoRuntimeImpl {
    fn cost(&self) -> &RuntimeBudget { &self.cost }
}

pub type RhoTuplespace =
    Arc<Box<dyn Tuplespace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Send + Sync>>;

pub type RhoISpace =
    Arc<Box<dyn ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Send + Sync>>;

pub type RhoReplayISpace = Arc<
    Box<dyn IReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Send + Sync>,
>;

pub type RhoHistoryRepository = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync
            + 'static,
    >,
>;

pub type ISpaceAndReplay = (RhoISpace, RhoReplayISpace);

struct RhoCommObserver {
    budget: RuntimeBudget,
}

impl RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for RhoCommObserver
{
    fn observe_produce(
        &self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        if !self.budget.has_comm_accounting_scope() || self.budget.is_unmetered() {
            return Ok(());
        }
        let identity = super::accounting::byte_accounting::produce_introduction_identity(source);
        let authority = self
            .budget
            .introduction_authority(
                identity,
                super::accounting::authority::AuthorityByteEventKind::ProduceIntroduction,
            )
            .map_err(interpreter_error_to_rspace)?;
        let charge = super::accounting::byte_accounting::produce_introduction_charge(channel, data)
            .and_then(|charge| {
                charge.cost(super::accounting::byte_accounting::BYTE_COST_SCHEDULE_V1)
            })
            .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        self.budget
            .reserve_produce_introduction_identity(identity, &authority, charge, persistent)
            .map_err(interpreter_error_to_rspace)
    }

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
        persistent: bool,
        _peeks: &std::collections::BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        if !self.budget.has_comm_accounting_scope() || self.budget.is_unmetered() {
            return Ok(());
        }
        let identity = super::accounting::byte_accounting::consume_introduction_identity(source);
        let authority = self
            .budget
            .introduction_authority(
                identity,
                super::accounting::authority::AuthorityByteEventKind::ConsumeIntroduction,
            )
            .map_err(interpreter_error_to_rspace)?;
        let charge = super::accounting::byte_accounting::consume_introduction_charge(
            channels,
            patterns,
            continuation,
        )
        .and_then(|charge| charge.cost(super::accounting::byte_accounting::BYTE_COST_SCHEDULE_V1))
        .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        self.budget
            .reserve_consume_introduction_identity(identity, &authority, charge, persistent)
            .map_err(interpreter_error_to_rspace)
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        data: &[(&ListParWithRandom, bool)],
    ) -> Result<(), RSpaceError> {
        if !self.budget.has_comm_accounting_scope() || self.budget.is_unmetered() {
            return Ok(());
        }
        let bytes = comm.cost_identity().bytes();
        let identity: [u8; 32] = bytes
            .try_into()
            .map_err(|_| RSpaceError::BugFoundError("invalid COMM identity length".to_string()))?;
        let mut authorities = Vec::<&CostAuthority>::new();
        let mut persistent_regions = std::collections::BTreeSet::new();
        if let Some(authority) = continuation.cost_authority.as_ref() {
            authorities.push(authority);
            if continuation_persistent {
                for region in super::accounting::authority::authority_regions(authority)
                    .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?
                    .into_keys()
                {
                    persistent_regions.insert(region);
                }
            }
        }
        for (datum, persistent) in data {
            if let Some(authority) = datum.cost_authority.as_ref() {
                authorities.push(authority);
                if *persistent {
                    for region in super::accounting::authority::authority_regions(authority)
                        .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?
                        .into_keys()
                    {
                        persistent_regions.insert(region);
                    }
                }
            }
        }
        let authority = super::accounting::authority::merge_authorities(authorities)
            .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        let authority = super::accounting::authority::instantiate_persistent_regions(
            &authority,
            &persistent_regions,
            identity,
        )
        .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        let byte_cost = super::accounting::byte_accounting::comm_charge(comm, data)
            .and_then(|charge| {
                charge.cost(super::accounting::byte_accounting::BYTE_COST_SCHEDULE_V1)
            })
            .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
        self.budget
            .reserve_comm_authority_identity_with_byte_cost(identity, &authority, byte_cost)
            .map_err(interpreter_error_to_rspace)?;
        Ok(())
    }
}

fn interpreter_error_to_rspace(error: InterpreterError) -> RSpaceError {
    match error {
        InterpreterError::OutOfPhlogistonsError => RSpaceError::OutOfPhlogistons,
        other => RSpaceError::InterpreterError(other.to_string()),
    }
}

async fn introduce_system_process<T>(
    mut spaces: Vec<&mut T>,
    processes: Vec<(Name, Arity, Remainder, BodyRef)>,
) -> Vec<Option<(TaggedContinuation, Vec<ListParWithRandom>)>>
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
{
    let mut results: Vec<Option<(TaggedContinuation, Vec<ListParWithRandom>)>> = Vec::new();

    for (name, arity, remainder, body_ref) in processes {
        let channels = vec![name];
        let patterns = vec![BindPattern {
            patterns: (0..arity).map(|i| new_freevar_par(i, Vec::new())).collect(),
            remainder,
            free_count: arity,
        }];

        let continuation = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ScalaBodyRef(body_ref)),
            guard: None,
            cost_authority: None,
        };

        for space in &mut spaces {
            let result = space
                .install(channels.clone(), patterns.clone(), continuation.clone())
                .await;
            results.push(result.map_err(|err| panic!("{}", err)).unwrap());
        }
    }

    results
}

fn std_system_processes() -> Vec<Definition> {
    vec![
        // ------------------------------------------------------------------
        // Legacy stdio URNs (File I/O FIP §1122 — DEPRECATED as of
        // `rho:io:fs:1.0.0`; removal target `rho:io:fs:2.*`).
        //
        // TODO(rho:io:fs:2.0): remove these four legacy registrations.
        //
        // Preserved for one-shot debug printing.  New code should use
        // `fs!stdout()` / `fs!stderr()` (spec §842-844) which returns an
        // error tuple on failure.  Deprecation is documented in the FIP
        // itself; there is no runtime notification — legacy flat URNs
        // predate the Versioned Registry's caller-provided `notify`
        // channel pattern (no syntax slot for a notify argument on
        // `new x(`urn`)`), and an operator-log warning would spam
        // messages that neither the operator nor the deploy author can
        // act on.
        // ------------------------------------------------------------------
        Definition {
            urn: "rho:io:stdout".to_string(),
            fixed_channel: FixedChannels::stdout(),
            arity: 1,
            body_ref: BodyRefs::STDOUT,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().std_out(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:stdoutAck".to_string(),
            fixed_channel: FixedChannels::stdout_ack(),
            arity: 2,
            body_ref: BodyRefs::STDOUT_ACK,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().std_out_ack(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:stderr".to_string(),
            fixed_channel: FixedChannels::stderr(),
            arity: 1,
            body_ref: BodyRefs::STDERR,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().std_err(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:stderrAck".to_string(),
            fixed_channel: FixedChannels::stderr_ack(),
            arity: 2,
            body_ref: BodyRefs::STDERR_ACK,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().std_err_ack(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:block:data".to_string(),
            fixed_channel: FixedChannels::get_block_data(),
            arity: 1,
            body_ref: BodyRefs::GET_BLOCK_DATA,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .get_block_data(args, ctx.block_data.clone())
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:casper:invalidBlocks".to_string(),
            fixed_channel: FixedChannels::get_invalid_blocks(),
            arity: 1,
            body_ref: BodyRefs::GET_INVALID_BLOCKS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .invalid_blocks(args, &ctx.invalid_blocks)
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:vault:address".to_string(),
            fixed_channel: FixedChannels::vault_address(),
            arity: 3,
            body_ref: BodyRefs::VAULT_ADDRESS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().vault_address(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:system:deployerId:ops".to_string(),
            fixed_channel: FixedChannels::deployer_id_ops(),
            arity: 3,
            body_ref: BodyRefs::DEPLOYER_ID_OPS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().deployer_id_ops(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:registry:ops".to_string(),
            fixed_channel: FixedChannels::reg_ops(),
            arity: 3,
            body_ref: BodyRefs::REG_OPS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().registry_ops(args).await })
                })
            }),
            remainder: None,
        },
        // Versioned-registry helper URN; see the `registry_ops_v1`
        // handler in system_processes.rs. The legacy `rho:registry:ops`
        // above is intentionally left untouched.
        Definition {
            urn: "rho:registry:ops:1.0.0".to_string(),
            fixed_channel: FixedChannels::reg_ops_v1(),
            arity: 3,
            body_ref: BodyRefs::REG_OPS_V1,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().registry_ops_v1(args).await },
                    )
                })
            }),
            remainder: None,
        },
        // Unified URN-binding dispatcher. Serves both legacy URNs (via
        // ProcessContext::urn_map) and versioned URNs (by delegating to
        // the Rholang lookupVersion contract). Will be the single
        // dispatch point for eval_new once that refactor lands.
        Definition {
            urn: "rho:internal:registry_lookup".to_string(),
            fixed_channel: FixedChannels::registry_lookup(),
            arity: 2,
            body_ref: BodyRefs::REGISTRY_LOOKUP,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().registry_lookup(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "sys:authToken:ops".to_string(),
            fixed_channel: FixedChannels::sys_authtoken_ops(),
            arity: 3,
            body_ref: BodyRefs::SYS_AUTHTOKEN_OPS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().sys_auth_token_ops(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:grpcTell".to_string(),
            fixed_channel: FixedChannels::grpc_tell(),
            arity: 3,
            body_ref: BodyRefs::GRPC_TELL,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().grpc_tell(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:io:devNull".to_string(),
            fixed_channel: FixedChannels::dev_null(),
            arity: 1,
            body_ref: BodyRefs::DEV_NULL,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().dev_null(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:deploy:data".to_string(),
            fixed_channel: FixedChannels::deploy_data(),
            arity: 1,
            body_ref: BodyRefs::DEPLOY_DATA,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .get_deploy_data(args, ctx.deploy_data.clone())
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:execution:abort".to_string(),
            fixed_channel: FixedChannels::abort(),
            arity: 1,
            body_ref: BodyRefs::ABORT,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().abort(args).await })
                })
            }),
            remainder: None,
        },
        // ------------------------------------------------------------------
        // File I/O native primitives (rho:io:fs:native:1.0.0/*).
        //
        // Registered here for fixed-channel dispatch.  Slice 31
        // (2026-08-04) reinstated the URN filter as a phase-scoped
        // flag on the reducer (`DebruijnInterpreter::filter_fs_native_urns`):
        // ON by default so state-execution deploys reject fs-native
        // URN bindings with `ReduceError`; the genesis entry
        // (`casper::rholang::runtime::play_deploys_for_genesis`)
        // toggles it OFF for the duration of the FsGenesis batch and
        // back ON before returning.  User code can only reach the
        // filesystem via the `Fs` cap published at genesis.
        //
        // URN naming: the "rho:io:fs:native:1.0.0/" prefix and the
        // per-primitive suffixes below are duplicated in
        // `casper/src/rust/genesis/contracts/fs_genesis.rs::FS_NATIVE_URN_PREFIX`
        // and `FS_NATIVE_URN_SUFFIXES`.  A version bump or rename must
        // edit ALL THREE sites (this comment, the composed source, and
        // the constants).  The drift test at
        // `fs_genesis.rs::fs_native_urn_suffixes_covers_composed_source`
        // catches suffix-list-vs-composed-source mismatches but does
        // NOT catch mismatches with this file — cross-check by hand.
        // ------------------------------------------------------------------
        fs_native_def(
            "rho:io:fs:native:1.0.0/open",
            FixedChannels::fs_open(),
            // Slice 29 (PB-M-14): arity is 5 = (root, rel, mode, cmode, ack).
            // cmode plumbed to `FileHandle.cmode` for later WAL routing.
            5,
            BodyRefs::FS_OPEN,
            |sp, args| Box::pin(async move { sp.fs.fs_open(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/close",
            FixedChannels::fs_close(),
            2,
            BodyRefs::FS_CLOSE,
            |sp, args| Box::pin(async move { sp.fs.fs_close(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/read",
            FixedChannels::fs_read(),
            3,
            BodyRefs::FS_READ,
            |sp, args| Box::pin(async move { sp.fs.fs_read(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/readAt",
            FixedChannels::fs_read_at(),
            4,
            BodyRefs::FS_READ_AT,
            |sp, args| Box::pin(async move { sp.fs.fs_read_at(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/write",
            FixedChannels::fs_write(),
            3,
            BodyRefs::FS_WRITE,
            |sp, args| Box::pin(async move { sp.fs.fs_write(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/writeAt",
            FixedChannels::fs_write_at(),
            4,
            BodyRefs::FS_WRITE_AT,
            |sp, args| Box::pin(async move { sp.fs.fs_write_at(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/seek",
            FixedChannels::fs_seek(),
            4,
            BodyRefs::FS_SEEK,
            |sp, args| Box::pin(async move { sp.fs.fs_seek(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/tell",
            FixedChannels::fs_tell(),
            2,
            BodyRefs::FS_TELL,
            |sp, args| Box::pin(async move { sp.fs.fs_tell(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/size",
            FixedChannels::fs_size(),
            2,
            BodyRefs::FS_SIZE,
            |sp, args| Box::pin(async move { sp.fs.fs_size(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/truncate",
            FixedChannels::fs_truncate(),
            3,
            BodyRefs::FS_TRUNCATE,
            |sp, args| Box::pin(async move { sp.fs.fs_truncate(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/flush",
            FixedChannels::fs_flush(),
            2,
            BodyRefs::FS_FLUSH,
            |sp, args| Box::pin(async move { sp.fs.fs_flush(args).await }),
        ),
        // Arities include the trailing ack channel.  Per B1 fix, every
        // path-taking handler takes (rootCanon, rel, ...) — see handler
        // signatures in handlers.rs.
        fs_native_def(
            "rho:io:fs:native:1.0.0/stat",
            FixedChannels::fs_stat(),
            // Slice 26 (H-26-F1): arity is 4 = (rootCanon, rel, cmode, ack).
            // `cmode` controls whether the record omits host-transient
            // fields (mtime/ctime/atime/owner/group).
            4,
            BodyRefs::FS_STAT,
            |sp, args| Box::pin(async move { sp.fs.fs_stat(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/exists",
            FixedChannels::fs_exists(),
            3, // (rootCanon, rel, ack)
            BodyRefs::FS_EXISTS,
            |sp, args| Box::pin(async move { sp.fs.fs_exists(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/entries",
            FixedChannels::fs_entries(),
            // Slice 26 (H-26-F1): arity is 4 = (rootCanon, rel, cmode, ack).
            4,
            BodyRefs::FS_ENTRIES,
            |sp, args| Box::pin(async move { sp.fs.fs_entries(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/entriesStream",
            FixedChannels::fs_entries_stream(),
            3, // (rootCanon, rel, ack)
            BodyRefs::FS_ENTRIES_STREAM,
            |sp, args| Box::pin(async move { sp.fs.fs_entries_stream(args).await }),
        ),
        // Streaming-backing slice (2026-08-25) — per-fd directory-entries
        // streaming primitive.  Three natives replace the bulk
        // `entriesStream` stub above (kept for backwards compat until
        // Dir.rho swaps its consumer, Step 5): Open allocates a stream
        // fd, Next yields one entry per call, Close releases the fd.
        // Under Consensus mode each Next reply is D3-WAL-journaled
        // (Step 3); under Oracular the natives are best-effort.
        fs_native_def(
            "rho:io:fs:native:1.0.0/entriesStreamOpen",
            FixedChannels::fs_entries_stream_open(),
            4, // (rootCanon, rel, cmode, ack)
            BodyRefs::FS_ENTRIES_STREAM_OPEN,
            |sp, args| Box::pin(async move { sp.fs.fs_entries_stream_open(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/entriesStreamNext",
            FixedChannels::fs_entries_stream_next(),
            2, // (streamFd, ack) — cmode captured in DirHandle at open
            BodyRefs::FS_ENTRIES_STREAM_NEXT,
            |sp, args| Box::pin(async move { sp.fs.fs_entries_stream_next(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/entriesStreamClose",
            FixedChannels::fs_entries_stream_close(),
            2, // (streamFd, ack)
            BodyRefs::FS_ENTRIES_STREAM_CLOSE,
            |sp, args| Box::pin(async move { sp.fs.fs_entries_stream_close(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/rename",
            FixedChannels::fs_rename(),
            // Slice 26 (H-26-F1): arity is 6 = (fromRootCanon, fromRel,
            // toRootCanon, toRel, cmode, ack).
            6,
            BodyRefs::FS_RENAME,
            |sp, args| Box::pin(async move { sp.fs.fs_rename(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/copyFile",
            FixedChannels::fs_copy_file(),
            // Slice 26 (H-26-F1): arity is 6 = (fromRootCanon, fromRel,
            // toRootCanon, toRel, cmode, ack).
            6,
            BodyRefs::FS_COPY_FILE,
            |sp, args| Box::pin(async move { sp.fs.fs_copy_file(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/removeFile",
            FixedChannels::fs_remove_file(),
            // Slice 26 (H-26-F1): arity is 4 = (rootCanon, rel, cmode, ack).
            4,
            BodyRefs::FS_REMOVE_FILE,
            |sp, args| Box::pin(async move { sp.fs.fs_remove_file(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/removeDir",
            FixedChannels::fs_remove_dir(),
            // Slice 26 (H-26-F1): arity is 5 = (rootCanon, rel, recursive,
            // cmode, ack).
            5,
            BodyRefs::FS_REMOVE_DIR,
            |sp, args| Box::pin(async move { sp.fs.fs_remove_dir(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/chmod",
            FixedChannels::fs_chmod(),
            // Slice 26 (H-26-F1): arity is 5 = (rootCanon, rel, modeBits,
            // cmode, ack).
            5,
            BodyRefs::FS_CHMOD,
            |sp, args| Box::pin(async move { sp.fs.fs_chmod(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/chown",
            FixedChannels::fs_chown(),
            // Slice 26 (H-26-F1): arity is 6 = (rootCanon, rel, owner,
            // group, cmode, ack).  `cmode` short-circuits chown on
            // Consensus caps.
            6,
            BodyRefs::FS_CHOWN,
            |sp, args| Box::pin(async move { sp.fs.fs_chown(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/quarantine",
            FixedChannels::fs_quarantine(),
            3,
            BodyRefs::FS_QUARANTINE,
            |sp, args| Box::pin(async move { sp.fs.fs_quarantine(args).await }),
        ),
        // Phase 8 slice 8a — range-lock natives (fd-based after
        // review-2 fix, 2026-08-12).
        fs_native_def(
            "rho:io:fs:native:1.0.0/lockRange",
            FixedChannels::fs_lock_range(),
            // (fd, offset, length, mode, holder, cmode, ack)
            7,
            BodyRefs::FS_LOCK_RANGE,
            |sp, args| Box::pin(async move { sp.fs.fs_lock_range(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/lockSequential",
            FixedChannels::fs_lock_sequential(),
            // (fd, holder, cmode, ack)
            4,
            BodyRefs::FS_LOCK_SEQUENTIAL,
            |sp, args| Box::pin(async move { sp.fs.fs_lock_sequential(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/releaseLock",
            FixedChannels::fs_release_lock(),
            // (lockId, ack)
            2,
            BodyRefs::FS_RELEASE_LOCK,
            |sp, args| Box::pin(async move { sp.fs.fs_release_lock(args).await }),
        ),
        fs_native_def(
            "rho:io:fs:native:1.0.0/releaseAllForHolder",
            FixedChannels::fs_release_all_for_holder(),
            // (holder, ack)
            2,
            BodyRefs::FS_RELEASE_ALL_FOR_HOLDER,
            |sp, args| Box::pin(async move { sp.fs.fs_release_all_for_holder(args).await }),
        ),
    ]
}

/// Compact factory for the 22 File I/O native `Definition` rows.  Cuts
/// the boilerplate to two lines per row.
fn fs_native_def(
    urn: &'static str,
    fixed_channel: Name,
    arity: Arity,
    body_ref: BodyRef,
    call: fn(
        SystemProcesses,
        (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<Par>, InterpreterError>> + Send>,
    >,
) -> Definition {
    Definition {
        urn: urn.to_string(),
        fixed_channel,
        arity,
        body_ref,
        handler: Box::new(move |ctx| {
            Box::new(move |args| {
                let sp = ctx.system_processes.clone();
                call(sp, args)
            })
        }),
        remainder: None,
    }
}

fn std_rho_crypto_processes() -> Vec<Definition> {
    vec![
        Definition {
            urn: "rho:crypto:secp256k1Verify".to_string(),
            fixed_channel: FixedChannels::secp256k1_verify(),
            arity: 4,
            body_ref: BodyRefs::SECP256K1_VERIFY,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().secp256k1_verify(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:crypto:blake2b256Hash".to_string(),
            fixed_channel: FixedChannels::blake2b256_hash(),
            arity: 2,
            body_ref: BodyRefs::BLAKE2B256_HASH,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().blake2b256_hash(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:crypto:keccak256Hash".to_string(),
            fixed_channel: FixedChannels::keccak256_hash(),
            arity: 2,
            body_ref: BodyRefs::KECCAK256_HASH,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().keccak256_hash(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:crypto:sha256Hash".to_string(),
            fixed_channel: FixedChannels::sha256_hash(),
            arity: 2,
            body_ref: BodyRefs::SHA256_HASH,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().sha256_hash(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:crypto:ed25519Verify".to_string(),
            fixed_channel: FixedChannels::ed25519_verify(),
            arity: 4,
            body_ref: BodyRefs::ED25519_VERIFY,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().ed25519_verify(args).await })
                })
            }),
            remainder: None,
        },
    ]
}

fn std_rho_ai_processes() -> Vec<Definition> {
    vec![
        Definition {
            urn: "rho:ai:gpt4".to_string(),
            fixed_channel: FixedChannels::gpt4(),
            arity: 2,
            body_ref: BodyRefs::GPT4,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().gpt4(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:ai:dalle3".to_string(),
            fixed_channel: FixedChannels::dalle3(),
            arity: 2,
            body_ref: BodyRefs::DALLE3,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().dalle3(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:ai:textToAudio".to_string(),
            fixed_channel: FixedChannels::text_to_audio(),
            arity: 2,
            body_ref: BodyRefs::TEXT_TO_AUDIO,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().text_to_audio(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:ollama:chat".to_string(),
            fixed_channel: FixedChannels::ollama_chat(),
            arity: 3,
            body_ref: BodyRefs::OLLAMA_CHAT,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().ollama_chat(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:ollama:generate".to_string(),
            fixed_channel: FixedChannels::ollama_generate(),
            arity: 3,
            body_ref: BodyRefs::OLLAMA_GENERATE,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(
                        async move { ctx.system_processes.clone().ollama_generate(args).await },
                    )
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:ollama:models".to_string(),
            fixed_channel: FixedChannels::ollama_models(),
            arity: 1,
            body_ref: BodyRefs::OLLAMA_MODELS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().ollama_models(args).await })
                })
            }),
            remainder: None,
        },
    ]
}

#[cfg(feature = "chromadb")]
fn std_rho_chroma_processes() -> Vec<Definition> {
    vec![
        Definition {
            urn: "rho:chroma:collection:new".to_string(),
            fixed_channel: FixedChannels::chroma_create_collection(),
            arity: 4,
            body_ref: BodyRefs::CHROMA_CREATE_COLLECTION,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .chroma_create_collection(args)
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:chroma:collection:meta".to_string(),
            fixed_channel: FixedChannels::chroma_get_collection_meta(),
            arity: 2,
            body_ref: BodyRefs::CHROMA_GET_COLLECTION_META,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .chroma_get_collection_meta(args)
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:chroma:collection:entries:new".to_string(),
            fixed_channel: FixedChannels::chroma_upsert_entries(),
            arity: 3,
            body_ref: BodyRefs::CHROMA_UPSERT_ENTRIES,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .chroma_upsert_entries(args)
                            .await
                    })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:chroma:collection:entries:query".to_string(),
            fixed_channel: FixedChannels::chroma_query(),
            arity: 3,
            body_ref: BodyRefs::CHROMA_QUERY,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move { ctx.system_processes.clone().chroma_query(args).await })
                })
            }),
            remainder: None,
        },
        Definition {
            urn: "rho:chroma:collection:entries:delete".to_string(),
            fixed_channel: FixedChannels::chroma_delete_documents(),
            arity: 3,
            body_ref: BodyRefs::CHROMA_DELETE_DOCUMENTS,
            handler: Box::new(|ctx| {
                Box::new(move |args| {
                    let ctx = ctx.clone();
                    Box::pin(async move {
                        ctx.system_processes
                            .clone()
                            .chroma_delete_documents(args)
                            .await
                    })
                })
            }),
            remainder: None,
        },
    ]
}

#[cfg(not(feature = "chromadb"))]
fn std_rho_chroma_processes() -> Vec<Definition> { vec![] }

#[allow(clippy::too_many_arguments)]
fn dispatch_table_creator(
    space: RhoISpace,
    dispatcher: RhoDispatch,
    block_data: Arc<tokio::sync::RwLock<BlockData>>,
    invalid_blocks: InvalidBlocks,
    urn_map: Arc<HashMap<String, Par>>,
    deploy_data: Arc<tokio::sync::RwLock<DeployData>>,
    extra_system_processes: &mut Vec<Definition>,
    openai_service: SharedOpenAIService,
    ollama_service: SharedOllamaService,
    grpc_client_service: GrpcClientService,
    chromadb_service: SharedChromaDBService,
    fs_handles: super::io::handle_table::FileHandleTable,
    fs_mode: super::io::ConsensusMode,
    metering: super::metering::MeteredMachine,
) -> RhoDispatchMap {
    let mut dispatch_table = HashMap::new();

    // Build the process chain - always include all processes
    // AI processes must always be registered for replay compatibility.
    // When OpenAI is disabled, the NoOp service handles calls gracefully.
    let mut all_processes: Vec<Definition> = std_system_processes();
    all_processes.extend(std_rho_crypto_processes());
    all_processes.extend(std_rho_ai_processes());
    all_processes.extend(std_rho_chroma_processes());

    all_processes.append(extra_system_processes);

    for def in all_processes.iter_mut() {
        let tuple = def.to_dispatch_table(ProcessContext::create(
            space.clone(),
            dispatcher.clone(),
            block_data.clone(),
            invalid_blocks.clone(),
            deploy_data.clone(),
            urn_map.clone(),
            openai_service.clone(),
            ollama_service.clone(),
            grpc_client_service.clone(),
            chromadb_service.clone(),
            fs_handles.clone(),
            fs_mode,
            metering.clone(),
        ));

        dispatch_table.insert(tuple.0, tuple.1);
    }

    Arc::new(tokio::sync::RwLock::new(dispatch_table))
}

fn basic_processes() -> HashMap<String, Par> {
    let mut map = HashMap::new();

    map.insert(
        "rho:registry:lookup".to_string(),
        Par::default().with_bundles(vec![Bundle {
            body: Some(FixedChannels::reg_lookup()),
            write_flag: true,
            read_flag: false,
        }]),
    );

    map.insert(
        "rho:registry:insertArbitrary".to_string(),
        Par::default().with_bundles(vec![Bundle {
            body: Some(FixedChannels::reg_insert_random()),
            write_flag: true,
            read_flag: false,
        }]),
    );

    map.insert(
        "rho:registry:insertSigned:secp256k1".to_string(),
        Par::default().with_bundles(vec![Bundle {
            body: Some(FixedChannels::reg_insert_signed()),
            write_flag: true,
            read_flag: false,
        }]),
    );

    // TODO(cleanup): drop this entry once Step 5b lands the eval_new
    // desugaring for rho:lib:... URNs and the test surface migrates
    // to the public rho:registry:1.0.0 URN below. Kept for now so the
    // Step 3-5 tests keep passing while the public URN ships beside it.
    map.insert(
        "rho:registry:v1:internal".to_string(),
        Par::default().with_bundles(vec![Bundle {
            body: Some(FixedChannels::reg_v1_internal()),
            write_flag: true,
            read_flag: false,
        }]),
    );

    // Public versioned-registry entry point. Clients use
    // `new getReg(`rho:registry:1.0.0`), notify in { ... getReg!?(*notify) ... }`
    // to obtain a `bundle+{v1Api}` carrying the v1 API surface.
    map.insert(
        "rho:registry:1.0.0".to_string(),
        Par::default().with_bundles(vec![Bundle {
            body: Some(FixedChannels::reg_v1()),
            write_flag: true,
            read_flag: false,
        }]),
    );

    map
}

#[allow(clippy::too_many_arguments)]
async fn setup_reducer(
    rspace: RhoISpace,
    block_data_ref: Arc<tokio::sync::RwLock<BlockData>>,
    invalid_blocks: InvalidBlocks,
    deploy_data_ref: Arc<tokio::sync::RwLock<DeployData>>,
    extra_system_processes: &mut Vec<Definition>,
    urn_map: HashMap<String, Par>,
    merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    openai_service: SharedOpenAIService,
    ollama_service: SharedOllamaService,
    grpc_client_service: GrpcClientService,
    chromadb_service: SharedChromaDBService,
    fs_handles: super::io::handle_table::FileHandleTable,
    fs_mode: super::io::ConsensusMode,
    cost: RuntimeBudget,
) -> Arc<DebruijnInterpreter> {
    rspace.set_accounting_observer(Some(Arc::new(RhoCommObserver {
        budget: cost.clone(),
    })));
    let reducer_cell = Arc::new(std::sync::OnceLock::new());

    let temp_dispatcher = Arc::new(RholangAndScalaDispatcher {
        _dispatch_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        reducer: reducer_cell.clone(),
    });

    // Wrap urn_map in Arc up front so it can be shared between the
    // dispatch_table_creator (passes it into ProcessContext / SystemProcesses
    // for the upcoming registry_lookup handler) and the DebruijnInterpreter
    // (uses it directly in the eval_new fast path).
    let urn_map = Arc::new(urn_map);

    // Phase 9 slice 9b-i: create the MeteredMachine BEFORE building
    // the dispatch table so `FsProcesses` handlers (constructed inside
    // `dispatch_table_creator` via `ProcessContext::create ->
    // SystemProcesses::create -> FsProcesses::new`) get a clone.  Every
    // clone shares the same underlying budget via `Arc` internals, so
    // a handler-side charge decrements the same budget the reducer
    // observes.  The `substitute: Substitute { metering }` at the end
    // of this function consumes the last clone; every intermediate
    // consumer holds a `.clone()`.
    let metering = super::metering::MeteredMachine::new(cost.clone());

    let replay_dispatch_table = dispatch_table_creator(
        rspace.clone(),
        temp_dispatcher.clone(),
        block_data_ref,
        invalid_blocks,
        urn_map.clone(),
        deploy_data_ref,
        extra_system_processes,
        openai_service,
        ollama_service,
        grpc_client_service,
        chromadb_service,
        fs_handles,
        fs_mode,
        metering.clone(),
    );

    let dispatcher = Arc::new(RholangAndScalaDispatcher {
        _dispatch_table: replay_dispatch_table,
        reducer: reducer_cell.clone(),
    });

    let reducer = Arc::new(DebruijnInterpreter {
        space: rspace.clone(),
        dispatcher: dispatcher.clone(),
        urn_map,
        merge_chs,
        mergeable_tags,
        metering: metering.clone(),
        substitute: Substitute { metering },
        // Slice 31: default ON — genesis path toggles off per-batch.
        filter_fs_native_urns: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    });

    reducer_cell.set(Arc::downgrade(&reducer)).ok().unwrap();
    reducer
}

fn setup_maps_and_refs(
    extra_system_processes: &Vec<Definition>,
) -> (
    Arc<tokio::sync::RwLock<BlockData>>,
    InvalidBlocks,
    Arc<tokio::sync::RwLock<DeployData>>,
    HashMap<String, Name>,
    Vec<(Name, Arity, Remainder, BodyRef)>,
) {
    let block_data_ref = Arc::new(tokio::sync::RwLock::new(BlockData::empty()));
    let invalid_blocks = InvalidBlocks::new();
    let deploy_data_ref = Arc::new(tokio::sync::RwLock::new(DeployData::empty()));

    let system_binding = std_system_processes();
    let rho_crypto_binding = std_rho_crypto_processes();
    // Always include AI processes for replay compatibility.
    // When OpenAI is disabled, the NoOp service handles calls gracefully.
    let rho_ai_binding = std_rho_ai_processes();
    let rho_chroma_binding = std_rho_chroma_processes();

    let combined_processes = system_binding
        .iter()
        .chain(rho_crypto_binding.iter())
        .chain(rho_ai_binding.iter())
        .chain(extra_system_processes.iter())
        .chain(rho_chroma_binding.iter())
        .collect::<Vec<&Definition>>();

    let mut urn_map: HashMap<_, _> = basic_processes();
    combined_processes
        .iter()
        .map(|process| process.to_urn_map())
        .for_each(|(key, value)| {
            // Every URN — including `rho:io:fs:native:1.0.0/*` — is
            // registered in urn_map so it's dispatchable when
            // needed.  Slice 31 (2026-08-04) added phase-scoped
            // filtering at the reducer's `eval_new` level: the
            // fs-native family is REJECTED during user (state-
            // execution) deploys and PERMITTED during genesis
            // composition.  See DebruijnInterpreter::
            // filter_fs_native_urns and RhoRuntimeImpl::
            // enable_fs_native_urn_filter / disable_fs_native_urn_filter.
            urn_map.insert(key, value);
        });

    let proc_defs: Vec<(Par, i32, Option<Var>, i64)> = combined_processes
        .iter()
        .map(|process| process.to_proc_defs())
        .collect();

    (
        block_data_ref,
        invalid_blocks,
        deploy_data_ref,
        urn_map,
        proc_defs,
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn create_rho_env<T>(
    mut rspace: T,
    merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    extra_system_processes: &mut Vec<Definition>,
    cost: RuntimeBudget,
    external_services: ExternalServices,
) -> (
    Arc<DebruijnInterpreter>,
    Arc<tokio::sync::RwLock<BlockData>>,
    InvalidBlocks,
    Arc<tokio::sync::RwLock<DeployData>>,
    super::io::handle_table::FileHandleTable,
)
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let maps_and_refs = setup_maps_and_refs(extra_system_processes);
    let (block_data_ref, invalid_blocks, deploy_data_ref, mut urn_map, proc_defs) = maps_and_refs;

    // Expose the bitmask-OR mergeable tag to system contracts (Registry.rho)
    // via a URI binding. Genesis-defined tags are unforgeable names; they must
    // be created at runtime startup and threaded into both the merge engine's
    // tag registry and the URN map so contracts can bind them via
    // `bootstrapName(`rho:system:...`)`.
    for (tag_par, merge_type) in mergeable_tags.iter() {
        if let MergeType::BitmaskOr = merge_type {
            tracing::info!(
                target: "f1r3fly.merge.tag_check.validation",
                "URI binding inserted: rho:system:bitmaskMergeableTag -> Par(unforgeables={}, exprs={}, bundles={})",
                tag_par.unforgeables.len(),
                tag_par.exprs.len(),
                tag_par.bundles.len(),
            );
            urn_map.insert(
                "rho:system:bitmaskMergeableTag".to_string(),
                tag_par.clone(),
            );
        }
    }

    let res = introduce_system_process(vec![&mut rspace], proc_defs).await;
    assert!(res.iter().all(|s| s.is_none()));

    let raw_rspace: RhoISpace = Arc::new(Box::new(rspace));

    // Use services from ExternalServices
    let openai_service = external_services.openai.clone();
    let ollama_service = external_services.ollama.clone();
    let grpc_client_service = external_services.grpc_client.clone();
    let chromadb_service = external_services.chroma.clone();
    // Create the fd table ONCE at runtime setup — all 22 fs handlers
    // clone into their own ProcessContext but the underlying table is
    // Arc-shared so fds survive across dispatches within the runtime.
    let fs_handles = super::io::handle_table::FileHandleTable::new();
    // Slice 26 / H-26-F3 review fix: `fs_mode` is a per-runtime FALLBACK
    // that is only consulted if a native handler receives a caller
    // without a per-cap cmode arg — but slice 26's `resolve_cmode` now
    // rejects such calls with `FSERR_BAD_ARG` (C-26-F1 fail-closed),
    // so this value is effectively unreachable from the library-agent
    // dispatch path.  It remains here for future handlers that might
    // read `self.mode` directly.  `Default` returns `Consensus` — the
    // more restrictive mode — so any refactor that lands a handler
    // consulting `self.mode` fails closed.
    let fs_mode = super::io::ConsensusMode::default();

    let reducer = setup_reducer(
        raw_rspace,
        block_data_ref.clone(),
        invalid_blocks.clone(),
        deploy_data_ref.clone(),
        extra_system_processes,
        urn_map,
        merge_chs,
        mergeable_tags,
        openai_service,
        ollama_service,
        grpc_client_service,
        chromadb_service,
        fs_handles.clone(),
        fs_mode,
        cost,
    )
    .await;

    (
        reducer,
        block_data_ref,
        invalid_blocks,
        deploy_data_ref,
        fs_handles,
    )
}

// This is from Nassim Taleb's "Skin in the Game"
fn bootstrap_rand() -> Blake2b512Random {
    Blake2b512Random::create_from_bytes("Decentralization is based on the simple notion that it is easier to macrobull***t than microbull***t. \
         Decentralization reduces large structural asymmetries."
         .as_bytes())
}

pub async fn bootstrap_registry(runtime: &RhoRuntimeImpl) -> () {
    let rand = bootstrap_rand();
    let cost = runtime.cost().get();
    runtime
        .cost()
        .set(Cost::create(i64::MAX, "bootstrap registry".to_string()));
    runtime.inj(ast(), Env::new(), rand).await.unwrap();
    runtime.cost().set(Cost::create_from_cost(cost));
}

async fn create_runtime<T>(
    rspace: T,
    extra_system_processes: &mut Vec<Definition>,
    init_registry: bool,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    external_services: ExternalServices,
) -> RhoRuntimeImpl
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let cost = CostAccounting::empty_cost();
    let merge_chs = Arc::new(tokio::sync::RwLock::new(HashMap::<Par, MergeType>::new()));

    let rho_env = create_rho_env(
        rspace,
        merge_chs.clone(),
        mergeable_tags,
        extra_system_processes,
        cost.clone(),
        external_services,
    )
    .await;

    let (reducer, block_ref, invalid_blocks, deploy_ref, fs_handles) = rho_env;
    let mut runtime = RhoRuntimeImpl::new(
        reducer,
        cost,
        block_ref,
        invalid_blocks,
        deploy_ref,
        merge_chs,
        fs_handles,
    );

    if init_registry {
        bootstrap_registry(&runtime).await;
        runtime.create_checkpoint().await;
    }

    runtime
}

/// Creates a runtime for executing Rholang code.
///
/// # Parameters
///
/// - `rspace`: The rspace which the runtime would operate on
/// - `extra_system_processes`: Extra system rholang processes exposed to the runtime
///   which you can execute functions on
/// - `init_registry`: For a newly created rspace, you might need to bootstrap registry
///   in the runtime to use rholang registry normally. This is not the only thing you need
///   for rholang registry - after the bootstrap registry, you still need to insert registry
///   contract on the rspace. For an existing rspace which bootstrapped registry before, you
///   can skip this. For some test cases, you don't need the registry, then you can skip this
///   init process which can be faster.
/// - `mergeable_tags`: Map of tag `Par` to its merge strategy
/// - `external_services`: External services configuration (OpenAI, gRPC)
///
/// # Returns
///
/// A configured `RhoRuntimeImpl` instance ready for executing Rholang code.
#[tracing::instrument(
    name = "create-play-runtime",
    target = "f1r3fly.rholang.runtime",
    skip_all
)]
pub async fn create_rho_runtime<T>(
    rspace: T,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    init_registry: bool,
    extra_system_processes: &mut Vec<Definition>,
    external_services: ExternalServices,
) -> RhoRuntimeImpl
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    create_runtime(
        rspace,
        extra_system_processes,
        init_registry,
        mergeable_tags,
        external_services,
    )
    .await
}

/// Creates a replay runtime for executing Rholang code with replay capabilities.
///
/// # Parameters
///
/// - `rspace`: The replay rspace which the runtime operates on
/// - `extra_system_processes`: Same as `create_rho_runtime`
/// - `init_registry`: Same as `create_rho_runtime`
/// - `mergeable_tags`: Map of tag `Par` to its merge strategy
/// - `external_services`: External services configuration
///
/// # Returns
///
/// A configured `RhoRuntimeImpl` instance with replay capabilities.
#[tracing::instrument(
    name = "create-replay-runtime",
    target = "f1r3fly.rholang.runtime",
    skip_all
)]
pub async fn create_replay_rho_runtime<T>(
    rspace: T,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    init_registry: bool,
    extra_system_processes: &mut Vec<Definition>,
    external_services: ExternalServices,
) -> RhoRuntimeImpl
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    create_runtime(
        rspace,
        extra_system_processes,
        init_registry,
        mergeable_tags,
        external_services,
    )
    .await
}

pub(crate) async fn _create_runtimes<T, R>(
    space: T,
    replay_space: R,
    init_registry: bool,
    additional_system_processes: &mut Vec<Definition>,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    external_services: ExternalServices,
) -> (RhoRuntimeImpl, RhoRuntimeImpl)
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
    R: IReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let rho_runtime = create_rho_runtime(
        space,
        mergeable_tags.clone(),
        init_registry,
        additional_system_processes,
        external_services.clone(),
    )
    .await;

    let replay_rho_runtime = create_replay_rho_runtime(
        replay_space,
        mergeable_tags,
        init_registry,
        additional_system_processes,
        external_services,
    )
    .await;

    (rho_runtime, replay_rho_runtime)
}

#[tracing::instrument(
    name = "create-play-runtime",
    target = "f1r3fly.rholang.runtime.create-play",
    skip_all
)]
pub async fn create_runtime_from_kv_store(
    stores: RSpaceStore,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    init_registry: bool,
    additional_system_processes: &mut Vec<Definition>,
    matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>>,
    external_services: ExternalServices,
) -> RhoRuntimeImpl {
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
        RSpace::create(stores, matcher).unwrap();

    let runtime = create_rho_runtime(
        space,
        mergeable_tags,
        init_registry,
        additional_system_processes,
        external_services,
    )
    .await;

    runtime
}
