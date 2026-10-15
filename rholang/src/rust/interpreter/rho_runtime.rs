// See rholang/src/main/scala/coop/rchain/rholang/interpreter/RhoRuntime.scala
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance::{EMapBody, GByteArray};
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, Bundle, Expr, ListParWithRandom, Par, TaggedContinuation, Var};
use models::rust::block_hash::BlockHash;
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::new_freevar_par;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace_interface::IReplayRSpace;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::trace::Log;
use rspace_plus_plus::rspace::tuplespace_interface::Tuplespace;

use super::accounting::_cost;
use super::accounting::cost_accounting::CostAccounting;
use super::accounting::costs::Cost;
use super::accounting::has_cost::HasCost;
use super::dispatch::{RhoDispatch, RholangAndScalaDispatcher};
use super::env::Env;
use super::errors::InterpreterError;
use super::interpreter::{EvaluateResult, Interpreter, InterpreterImpl};
use super::merging::mergeable_tags::bitmask_or_tag;
use super::reduce::DebruijnInterpreter;
use super::registry::registry_bootstrap::ast;
use super::storage::charging_rspace::ChargingRSpace;
use super::substitute::Substitute;
use super::system_processes::{
    Arity, BlockData, BodyRef, Definition, DeployData, InvalidBlocks, Name, ProcessContext,
    Remainder, RhoDispatchMap,
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
     * The function would execute the par regardless setting cost which would possibly cause
     * [[coop.rchain.rholang.interpreter.errors.OutOfPhlogistonsError]]. Because of that, use this
     * function in some situation which is not cost sensitive.
     *
     * This function would change the state in the runtime.
     *
     * Ideally, this function should be removed or hack the runtime without cost accounting in the future .
     * @param par [[coop.rchain.models.Par]] for the execution
     * @param env additional env for execution
     * @param rand random seed for rholang execution
     * @return
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
    pub cost: _cost,
    pub block_data_ref: Arc<tokio::sync::RwLock<BlockData>>,
    pub invalid_blocks_param: InvalidBlocks,
    pub deploy_data_ref: Arc<tokio::sync::RwLock<DeployData>>,
    pub(crate) merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
    /// Per-runtime FS handle table — fds allocated by `fs_open` live
    /// here, visible to downstream `fs_read` / `fs_close` / etc.
    /// Shared with every FS native handler's `FsProcesses` dispatch
    /// surface via `Arc` under the hood, so a single Arc-bump at
    /// construction time threads the same table through every
    /// handler.  Public so test harnesses + the soft-checkpoint
    /// fd-snapshot wiring can inspect and manipulate the table
    /// directly.
    pub fs_handles: super::io::handle_table::FileHandleTable,
    /// Stack of file-fd counter snapshots captured at soft-checkpoint
    /// time.  On revert we pop the innermost snapshot and truncate
    /// the fd table to it, freeing every fd allocated after the
    /// checkpoint (spec §Fd-table lifecycle).  Stack (not single
    /// slot) so nested `create_soft_checkpoint` calls preserve the
    /// outer marks — H4/M1 review fix (slice 29 round 2).  Pre-fix
    /// design was `Option<u64>` which silently dropped the outer
    /// mark on the inner `create`.
    fs_snapshot_stack: Arc<std::sync::Mutex<Vec<u64>>>,
    /// Stack of dir-stream fd-counter snapshots captured at soft-
    /// checkpoint time.  Companion to `fs_snapshot_stack` — same
    /// push-on-create / pop-on-revert / clear-on-reset semantics,
    /// applied to `fs_handles.dir_handles`.  Kept as its own stack
    /// (rather than a tuple in `fs_snapshot_stack`) so a revert that
    /// touches only one table doesn't accidentally pop the other's
    /// mark.  Streaming-backing slice Step 4 (2026-08-25).
    dir_fs_snapshot_stack: Arc<std::sync::Mutex<Vec<u64>>>,
    /// Stack of consensus-WAL length marks captured at soft-
    /// checkpoint time.  On revert we pop the innermost mark and
    /// `fs_handles.wal.truncate_to(mark)`, discarding any WAL
    /// entries appended during the failed deploy.  Prevents
    /// divergence where a leader's reverted-but-journaled write
    /// would be replayed by followers.  H-29-1 review fix; nested-
    /// stack semantics from the H4/M1 round-2 fix.
    wal_snapshot_stack: Arc<std::sync::Mutex<Vec<super::io::wal::WalMark>>>,
    /// Slice 30b (H-30b-2 round-2 fix): optional snapshot writer,
    /// configured at boot from `storage.consensus-fs-snapshot-
    /// {cadence,dir}`.  `None` inside the RwLock when the operator
    /// has no consensus-static provisioning.  Public so test
    /// harnesses can inspect it directly; writers go through
    /// [`set_fs_snapshot_writer`] which acquires the write guard.
    ///
    /// `play_deploys_for_state` reads via `.read().await` on every
    /// call; many runtimes can read concurrently.  Only boot-time
    /// set is a writer.
    pub fs_snapshot_writer: Arc<tokio::sync::RwLock<Option<super::io::snapshot::SnapshotWriter>>>,
}

impl RhoRuntimeImpl {
    fn new(
        reducer: Arc<DebruijnInterpreter>,
        cost: _cost,
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
            dir_fs_snapshot_stack: Arc::new(std::sync::Mutex::new(Vec::new())),
            wal_snapshot_stack: Arc::new(std::sync::Mutex::new(Vec::new())),
            fs_snapshot_writer: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Boot-time setter for the optional consensus-WAL snapshot
    /// writer.  `None` disables snapshot persistence (default).
    /// `Some(writer)` enables cadence-based snapshot writes to the
    /// writer's configured directory via `writer.maybe_write(block,
    /// &entries)`.
    ///
    /// Acquires the write guard synchronously; callers hold the
    /// guard only across the single `*guard = writer` assignment.
    pub async fn set_fs_snapshot_writer(
        &self,
        writer: Option<super::io::snapshot::SnapshotWriter>,
    ) {
        *self.fs_snapshot_writer.write().await = writer;
    }

    pub fn get_cost_log(&self) -> Vec<Cost> { self.cost.get_log() }

    pub fn clear_cost_log(&self) { self.cost.clear_log() }

    /// Enable the rho:io:fs:native:* URN filter.  Every subsequent
    /// `new x(`rho:io:fs:native:...`)` inside a deploy returns
    /// `ReduceError` from `eval_new`.  This is the default state.
    /// Idempotent.
    pub fn enable_fs_native_urn_filter(&self) {
        self.reducer
            .filter_fs_native_urns
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Disable the rho:io:fs:native:* URN filter for the duration
    /// of a genesis-composition run (or a test harness).  MUST be
    /// re-enabled via `enable_fs_native_urn_filter` after the
    /// scope exits — leaving it disabled would expose raw fs
    /// syscalls to every subsequent user deploy on this runtime.
    /// Prefer [`exempt_fs_native_urn_filter`] when a lexical scope
    /// bounds the exemption; it uses RAII to re-enable on Drop.
    pub fn disable_fs_native_urn_filter(&self) {
        self.reducer
            .filter_fs_native_urns
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// RAII exemption guard for the `rho:io:fs:native:*` URN
    /// filter.  Disables the filter on construction and re-enables
    /// on Drop — including panics and tokio-task cancellation.
    /// Caller holds the returned guard for the lifetime of the
    /// exemption scope; dropping it immediately after construction
    /// re-enables the filter before any enclosed code sees it off.
    pub fn exempt_fs_native_urn_filter(&self) -> FsNativeUrnFilterExemption {
        self.disable_fs_native_urn_filter();
        FsNativeUrnFilterExemption {
            flag: self.reducer.filter_fs_native_urns.clone(),
        }
    }

    /// Introspection helper for tests and diagnostics.  Returns
    /// `true` iff `rho:io:fs:native:*` URN resolution is blocked
    /// in `eval_new`.
    pub fn fs_native_urn_filter_enabled(&self) -> bool {
        self.reducer
            .filter_fs_native_urns
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub async fn set_report_phase(
        &self,
        phase: rspace_plus_plus::rspace::reporting_rspace::ReportPhase,
    ) {
        self.reducer.space.set_report_phase(phase).await
    }
}

impl RhoRuntime for RhoRuntimeImpl {
    async fn evaluate(
        &self,
        term: &str,
        initial_phlo: Cost,
        normalizer_env: HashMap<String, Par>,
        rand: Blake2b512Random,
    ) -> Result<EvaluateResult, InterpreterError> {
        let start = Instant::now();
        let i = InterpreterImpl::new(self.cost.clone(), self.merge_chs.clone());
        let reducer = &self.reducer;
        let res = i
            .inj_attempt(reducer, term, initial_phlo, normalizer_env, rand)
            .await;
        metrics::histogram!(EVALUATE_TIME_METRIC, "source" => RUNTIME_METRICS_SOURCE)
            .record(start.elapsed().as_secs_f64());
        res
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
        // Streaming-backing slice Step 4: mirror the file-fd stack for
        // dir-stream fds so a reverted deploy sweeps stream fds it
        // opened between checkpoint and revert.  Same nested-stack
        // semantics as fs_snapshot_stack — the inner create pushes on
        // top of the outer mark; each revert pops one.
        {
            let mut stack = self.dir_fs_snapshot_stack.lock().unwrap();
            stack.push(self.fs_handles.dir_handles.snapshot_next_fd());
        }
        // H-29-1 review fix: snapshot the consensus WAL length
        // alongside the fd counters so revert can truncate the WAL
        // back to this mark too.  Keeps leader/follower WAL byte-
        // identity even across reverted deploys.
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
        // Streaming-backing slice Step 4: symmetric pop + truncate for
        // the dir-stream fd table.  Unbalanced revert (no matching
        // create) is a no-op, same defensive posture as the file-fd
        // stack.
        let dir_snap = { self.dir_fs_snapshot_stack.lock().unwrap().pop() };
        if let Some(s) = dir_snap {
            self.fs_handles.dir_handles.truncate_to(s).await;
        }
        // H-29-1: pop the WAL mark and truncate.  Same unbalanced-no-op
        // posture as the fd stacks.
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
        // PB-M-13 / Slice 28: seed FileHandleTable::next_fd from the
        // state root.  Every block boundary triggers a reset via this
        // path (see `casper::rholang::runtime::play_deploys_for_state`,
        // `play_deploys_for_genesis`, `play_system_deploy`), and every
        // validator resetting to the same root computes the same
        // watermark — so fd values captured by the leader are
        // reproducibly replayable by followers.
        //
        // **Consensus commitment**: fd values are consensus-observable
        // via Rholang tuplespace state (`fdP` cells inside File
        // agents).  This seed derivation is therefore an implicit
        // consensus commitment — any future change to
        // `seed_next_fd_from_state_hash`'s derivation constants or
        // hash algorithm is a hard fork.
        //
        // **Aliasing prevention**: a fresh runtime spawned per block
        // (via `RuntimeManager::spawn_runtime`) starts fd allocation
        // from the state-hash-derived watermark, NOT from `next_fd =
        // 1`.  Two independent runtimes at the same state hash
        // allocate identical fd sequences (leader/follower replay).
        self.fs_handles.seed_next_fd_from_state_hash(&root.bytes());
        // Streaming-backing slice (2026-08-25): seed the dir-stream
        // fd counter from the same state hash.  Same PB-M-13 aliasing
        // threat as file fds — dir-stream fd values flow through the
        // tuplespace as GInt, so a joining validator (or restart) that
        // allocated fresh dir-stream fds starting from 1 could alias a
        // stream fd a prior lifetime stashed in tuplespace state.
        self.fs_handles
            .dir_handles
            .seed_next_fd_from_state_hash(&root.bytes());
        // H-29-F2 review fix (defense in depth): clear the consensus
        // WAL on reset.  All correctness paths drain the WAL per-
        // deploy via `Wal::take_deploy_entries`; this clear guarantees
        // that if a caller resets to a state root without first
        // draining, the follower observes an empty WAL — no ghost
        // entries from an earlier block leak into the next.
        self.fs_handles.wal.clear();
        // M6 round-2 fix: also clear stashed checkpoint marks so a
        // subsequent revert doesn't pop a stale mark (which would
        // truncate the fd table to a pre-reset watermark or the WAL
        // to a length below the cleared zero).  A reset semantically
        // means "start fresh at this state root"; leaving a mark
        // stashed is inconsistent with that.
        self.fs_snapshot_stack.lock().unwrap().clear();
        self.dir_fs_snapshot_stack.lock().unwrap().clear();
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
    fn cost(&self) -> &_cost { &self.cost }
}

/// RAII guard returned by
/// [`RhoRuntimeImpl::exempt_fs_native_urn_filter`].  Owns an Arc
/// clone of the filter flag (not a borrow of the runtime) so the
/// caller can still exercise `&mut self` on the runtime during the
/// exemption.  The filter re-enables on Drop — including panics
/// and tokio-task cancellation.
#[must_use = "the exemption ends when the guard is dropped; letting it drop \
              immediately after construction re-enables the filter and \
              the enclosed code sees the filter ON"]
pub struct FsNativeUrnFilterExemption {
    flag: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FsNativeUrnFilterExemption {
    fn drop(&mut self) { self.flag.store(true, std::sync::atomic::Ordering::Release); }
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
    ]
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

/// Build `Definition` rows for every entry in the `FS_HANDLERS`
/// distributed slice.  Each definition adapts the typed per-handler
/// `dispatch` fn-pointer (which takes an `FsProcesses` + the triple
/// `(args, is_replay, previous)`) into the `Definition` handler
/// contract (which takes a `ProcessContext` and returns a per-call
/// inner closure).
///
/// The adaptation strategy: construct ONE `FsProcesses` instance up
/// front, Arc-clone it into each per-handler closure.  This gives
/// every fs native URN access to the same `FileHandleTable` + mode
/// + metering surface — critical for state continuity across
/// handler invocations (fds opened by `fs_open` must be seen by
/// `fs_read`, etc.).  Mode defaults to `Consensus` and metering to
/// `NoopMetering` for the Wave 4 posture; later slices rewire these
/// when the real cost-accounted-rho API lands.
///
/// `fs_remove_dir` is trait-exempt (DD-RemoveDirReplyShape complexity)
/// and NOT in `FS_HANDLERS`; it gets a dedicated `Definition` row
/// built by `dispatch_table_creator` using the `FsProcesses` handle
/// returned alongside the trait-handler Vec.  Both paths share the
/// SAME `FsProcesses` instance so state continuity (fd table, mode,
/// metering) spans the trait-registered 27 handlers plus the
/// trait-exempt one.
///
/// Called by `dispatch_table_creator` to register every fs native
/// URN into the runtime's dispatch map.  Phase-scoped visibility
/// is enforced inside the reducer by `filter_fs_native_urns`
/// (slice 5.32): user deploys get a `ReduceError`; genesis gets
/// unfiltered access via the toggle in `play_deploys_for_genesis`
/// (slice 5.33).
fn fs_handlers_to_definitions(
    dispatcher: RhoDispatch,
    space: RhoISpace,
    fs_handles: super::io::handle_table::FileHandleTable,
) -> (
    Vec<Definition>,
    super::io::handler_trait::fs_processes::FsProcesses,
) {
    use super::accounting::noop::{Metering, NoopMetering};
    use super::io::handler_trait::fs_processes::FsProcesses;
    use super::io::handler_trait::FS_HANDLERS;
    use super::io::{ConsensusMode, FS_NATIVE_URN_PREFIX_VERSIONED as FS_NATIVE_URN_PREFIX};

    let fs_metering: Arc<dyn Metering> = Arc::new(NoopMetering);
    let fs_processes = FsProcesses::new(
        dispatcher,
        space,
        fs_handles,
        ConsensusMode::Consensus,
        fs_metering,
    );

    let defs = FS_HANDLERS
        .iter()
        .map(|entry| {
            let fs_processes = fs_processes.clone();
            let dispatch = entry.dispatch;
            Definition {
                urn: format!("{FS_NATIVE_URN_PREFIX}{}", entry.urn_suffix),
                fixed_channel: (entry.fixed_channel)(),
                arity: entry.arity as Arity,
                body_ref: entry.body_ref,
                handler: Box::new(move |_ctx| {
                    let fs_processes = fs_processes.clone();
                    Box::new(move |args| dispatch(fs_processes.clone(), args))
                }),
                remainder: None,
            }
        })
        .collect();

    (defs, fs_processes)
}

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
) -> RhoDispatchMap {
    let mut dispatch_table = HashMap::new();

    // Build the process chain - always include all processes
    // AI processes must always be registered for replay compatibility.
    // When OpenAI is disabled, the NoOp service handles calls gracefully.
    let mut all_processes: Vec<Definition> = std_system_processes();
    all_processes.extend(std_rho_crypto_processes());
    all_processes.extend(std_rho_ai_processes());
    all_processes.extend(std_rho_chroma_processes());

    // File I/O native URNs — one Definition per entry in the
    // FS_HANDLERS distributed slice.  Registration is unconditional
    // (same posture as the stdio / crypto / ai processes); phase-
    // scoped visibility is enforced inside the reducer by
    // `filter_fs_native_urns` (slice 5.32).  User deploys attempting
    // to bind these URNs via `new x(\`rho:io:fs:native:1.0.0/...\`)`
    // get a `ReduceError`; genesis composition toggles the filter
    // off (slice 5.33's `play_deploys_for_genesis`).
    //
    // The dispatcher clone threaded here is the same `RhoDispatch`
    // instance that every other Definition's handler receives
    // through its `ProcessContext`, so the fs native handlers see
    // the same reducer / space / dispatcher as the rest of the
    // system-processes layer.
    let (fs_defs, fs_processes) =
        fs_handlers_to_definitions(dispatcher.clone(), space.clone(), fs_handles);
    all_processes.extend(fs_defs);

    // Trait-exempt fs_remove_dir handler: slice 5.43 registered the
    // URN + fixed_channel + proc_defs so FsGenesis composition could
    // resolve the URN at genesis-time; slice 5.44 added a stub
    // replying FSERR_UNSUPPORTED so user-held Dir caps got a
    // well-formed error.  Slice 5.142 swaps the stub for the real
    // handler ported in slices 5.136–5.141 (DD-RemoveDirReplyShape
    // with its 4 divergence reply shapes).  The `fs_processes`
    // handle is shared with the 27 trait-registered handlers, so
    // state continuity (fd table, mode, metering) spans both.
    all_processes.push(Definition {
        urn: format!("{}removeDir", super::io::FS_NATIVE_URN_PREFIX_VERSIONED),
        fixed_channel: FixedChannels::fs_remove_dir(),
        arity: 5,
        body_ref: BodyRefs::FS_REMOVE_DIR,
        handler: Box::new(move |_ctx| {
            let fs_processes = fs_processes.clone();
            Box::new(move |args| {
                let fs_processes = fs_processes.clone();
                Box::pin(async move { fs_processes.fs_remove_dir(args).await })
            })
        }),
        remainder: None,
    });

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

async fn setup_reducer(
    charging_rspace: RhoISpace,
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
    cost: _cost,
    fs_handles: super::io::handle_table::FileHandleTable,
) -> Arc<DebruijnInterpreter> {
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

    let replay_dispatch_table = dispatch_table_creator(
        charging_rspace.clone(),
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
    );

    let dispatcher = Arc::new(RholangAndScalaDispatcher {
        _dispatch_table: replay_dispatch_table,
        reducer: reducer_cell.clone(),
    });

    let reducer = Arc::new(DebruijnInterpreter {
        space: charging_rspace.clone(),
        dispatcher: dispatcher.clone(),
        urn_map,
        merge_chs,
        mergeable_tags,
        cost: cost.clone(),
        substitute: Substitute { cost: cost.clone() },
        single_term_evaluations: Arc::new(AtomicU64::new(0)),
        yielded_single_term_evaluations: Arc::new(AtomicU64::new(0)),
        spawned_eval_tasks: Arc::new(AtomicU64::new(0)),
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
            urn_map.insert(key, value);
        });

    let mut proc_defs: Vec<(Par, i32, Option<Var>, i64)> = combined_processes
        .iter()
        .map(|process| process.to_proc_defs())
        .collect();

    // File I/O native URNs — must land in `urn_map` (so the reducer's
    // `eval_new` can resolve `new x(`rho:io:fs:native:1.0.0/...`)` to
    // the handler's fixed_channel bundle) AND in `proc_defs` (so
    // `introduce_system_process` installs the per-channel reader that
    // the dispatcher drives via `body_ref`).  Slice 5.34 wired the fs
    // handlers into `dispatch_table_creator` but omitted this half
    // of the registration, so genesis composition (where the filter
    // is toggled off and FsGenesis tries to bind the raw primitives)
    // tripped at `urn_map.contains_key(urn) == false`.
    //
    // Fields read from each `FsHandlerEntry`: `urn_suffix`,
    // `fixed_channel`, `arity`, `body_ref`.  The handler closure is
    // NOT touched here — it lives only on the Definition produced by
    // `fs_handlers_to_definitions` for the dispatch table.
    for entry in super::io::handler_trait::FS_HANDLERS.iter() {
        let urn = format!(
            "{}{}",
            super::io::FS_NATIVE_URN_PREFIX_VERSIONED,
            entry.urn_suffix
        );
        let fixed_channel: Par = (entry.fixed_channel)();
        let bundle: Par = Par::default().with_bundles(vec![Bundle {
            body: Some(fixed_channel.clone()),
            write_flag: true,
            read_flag: false,
        }]);
        urn_map.insert(urn, bundle);
        proc_defs.push((fixed_channel, entry.arity as Arity, None, entry.body_ref));
    }

    // Trait-exempt FS native URN: `fs_remove_dir` is intentionally
    // NOT in `FS_HANDLERS` (its four divergence reply shapes don't
    // fit the `FsHandler` trait — see `handler_trait::fs_handler`
    // docstring).  The real dispatcher wiring lands at a future
    // Wave 4 handler slice.  Register the URN here so FsGenesis
    // composition (slice 5.36) can bind `new fsRemoveDir(
    // `rho:io:fs:native:1.0.0/removeDir`)` without tripping
    // `eval_new`'s "No value set for URN" check.  Dir.rho sends to
    // this channel inside its `removeDir` method only fire when a
    // user-held Dir cap invokes removeDir — not during genesis
    // composition.
    {
        let fixed_channel = FixedChannels::fs_remove_dir();
        let bundle: Par = Par::default().with_bundles(vec![Bundle {
            body: Some(fixed_channel.clone()),
            write_flag: true,
            read_flag: false,
        }]);
        urn_map.insert(
            format!("{}removeDir", super::io::FS_NATIVE_URN_PREFIX_VERSIONED),
            bundle,
        );
        // Arity 5 = (rootCanon, rel, recursive, cmode, ack); matches
        // the Dir.rho call-site `fsRemoveDir!(canonRoot, joined, b,
        // cmode, *retCh)`.
        proc_defs.push((fixed_channel, 5 as Arity, None, BodyRefs::FS_REMOVE_DIR));
    }

    (
        block_data_ref,
        invalid_blocks,
        deploy_data_ref,
        urn_map,
        proc_defs,
    )
}

pub async fn create_rho_env<T>(
    mut rspace: T,
    merge_chs: Arc<tokio::sync::RwLock<HashMap<Par, MergeType>>>,
    mergeable_tags: Arc<HashMap<Par, MergeType>>,
    extra_system_processes: &mut Vec<Definition>,
    cost: _cost,
    external_services: ExternalServices,
    fs_handles: super::io::handle_table::FileHandleTable,
) -> Result<
    (
        Arc<DebruijnInterpreter>,
        Arc<tokio::sync::RwLock<BlockData>>,
        InvalidBlocks,
        Arc<tokio::sync::RwLock<DeployData>>,
    ),
    InterpreterError,
>
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
    if let Some(tag_par) = bitmask_or_tag(&mergeable_tags)? {
        tracing::debug!(
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

    let res = introduce_system_process(vec![&mut rspace], proc_defs).await;
    assert!(res.iter().all(|s| s.is_none()));

    let charging_rspace: RhoISpace = Arc::new(Box::new(ChargingRSpace::charging_rspace(
        rspace,
        cost.clone(),
    )));

    // Use services from ExternalServices
    let openai_service = external_services.openai.clone();
    let ollama_service = external_services.ollama.clone();
    let grpc_client_service = external_services.grpc_client.clone();
    let chromadb_service = external_services.chroma.clone();
    let reducer = setup_reducer(
        charging_rspace,
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
        cost,
        fs_handles,
    )
    .await;

    Ok((reducer, block_data_ref, invalid_blocks, deploy_data_ref))
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
) -> Result<RhoRuntimeImpl, InterpreterError>
where
    T: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>
        + Clone
        + Send
        + Sync
        + 'static,
{
    let cost = CostAccounting::empty_cost();
    let merge_chs = Arc::new(tokio::sync::RwLock::new(HashMap::<Par, MergeType>::new()));
    // One FS handle table per runtime, shared by the dispatch-table-
    // side `FsProcesses` (via a clone) and exposed as a public field
    // on `RhoRuntimeImpl` for test harnesses + the (yet-to-land)
    // soft-checkpoint fd-snapshot wiring.  FileHandleTable is
    // Arc-backed internally so clones are cheap and point at the
    // same underlying state.
    let fs_handles = super::io::handle_table::FileHandleTable::new();

    let rho_env = create_rho_env(
        rspace,
        merge_chs.clone(),
        mergeable_tags,
        extra_system_processes,
        cost.clone(),
        external_services,
        fs_handles.clone(),
    )
    .await?;

    let (reducer, block_ref, invalid_blocks, deploy_ref) = rho_env;
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

    Ok(runtime)
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
) -> Result<RhoRuntimeImpl, InterpreterError>
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
) -> Result<RhoRuntimeImpl, InterpreterError>
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
) -> Result<(RhoRuntimeImpl, RhoRuntimeImpl), InterpreterError>
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
    .await?;

    let replay_rho_runtime = create_replay_rho_runtime(
        replay_space,
        mergeable_tags,
        init_registry,
        additional_system_processes,
        external_services,
    )
    .await?;

    Ok((rho_runtime, replay_rho_runtime))
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
) -> Result<RhoRuntimeImpl, InterpreterError> {
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
        RSpace::create(stores, matcher).unwrap();

    create_rho_runtime(
        space,
        mergeable_tags,
        init_registry,
        additional_system_processes,
        external_services,
    )
    .await
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::interpreter::io::handler_trait::{
        EXPECTED_MIGRATED_HANDLER_COUNT, FS_HANDLERS,
    };
    use crate::rust::interpreter::io::FS_NATIVE_URN_PREFIX as FS_NATIVE_URN_FILTER_PREFIX;
    use crate::rust::interpreter::matcher::r#match::Matcher;

    async fn minimal_dispatch_and_space() -> (RhoDispatch, RhoISpace) {
        let reducer_cell = Arc::new(std::sync::OnceLock::new());
        let dispatcher: RhoDispatch = Arc::new(RholangAndScalaDispatcher {
            _dispatch_table: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            reducer: reducer_cell,
        });

        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let space = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
            store,
            Arc::new(Box::new(Matcher)),
        )
        .unwrap();
        let rspace: RhoISpace = Arc::new(Box::new(space));

        (dispatcher, rspace)
    }

    /// Registration-count regression gate: walking `FS_HANDLERS` via
    /// `fs_handlers_to_definitions` must produce exactly
    /// `EXPECTED_MIGRATED_HANDLER_COUNT` rows.  This is distinct from
    /// `fs_handlers::fs_handlers_count_matches_migrated_pinned` (which
    /// pins the slice length): it defends against a regression in
    /// `fs_handlers_to_definitions` itself — e.g., a `.filter(...)`
    /// chained in, a stray `.take(N)`, or a short-circuit that drops
    /// entries after `FsProcesses` construction.
    #[tokio::test]
    async fn fs_handlers_to_definitions_count_matches_registry() {
        let (dispatcher, space) = minimal_dispatch_and_space().await;
        let (defs, _fs_processes) = fs_handlers_to_definitions(
            dispatcher,
            space,
            crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
        );
        assert_eq!(
            defs.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
            "fs_handlers_to_definitions produced {} definitions but \
             EXPECTED_MIGRATED_HANDLER_COUNT is {}.  A regression in \
             the FS_HANDLERS → Definition mapping is dropping entries \
             — handler dispatch will silently no-op for the missing \
             URNs.",
            defs.len(),
            EXPECTED_MIGRATED_HANDLER_COUNT,
        );
    }

    /// Every produced URN must be unique.  Two entries sharing a
    /// `urn_suffix` would collide in `RhoDispatchMap`, clobbering one
    /// handler at registration — a silent dispatch regression.
    #[tokio::test]
    async fn fs_handlers_to_definitions_urns_unique() {
        let (dispatcher, space) = minimal_dispatch_and_space().await;
        let (defs, _fs_processes) = fs_handlers_to_definitions(
            dispatcher,
            space,
            crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
        );
        let mut seen = std::collections::HashSet::new();
        for def in &defs {
            assert!(
                seen.insert(def.urn.clone()),
                "duplicate FS native URN `{}` in fs_handlers_to_definitions \
                 output — two FS_HANDLERS entries share a `urn_suffix`, \
                 which would clobber one handler at RhoDispatchMap \
                 registration.",
                def.urn,
            );
        }
    }

    /// Every produced URN must start with the shared
    /// `io::FS_NATIVE_URN_PREFIX` ("rho:io:fs:native:").  The reducer's
    /// `filter_fs_native_urns` check in `eval_new` tests
    /// `urn.starts_with(FS_NATIVE_URN_PREFIX)` — a regression where
    /// the local versioned prefix in `fs_handlers_to_definitions`
    /// drifts to something that no longer starts with the shared
    /// prefix would silently bypass the filter: user deploys could
    /// bind the FS native URNs directly, defeating the phase-scoped
    /// visibility gate (slices 5.32/5.33/5.35).
    #[tokio::test]
    async fn fs_handlers_to_definitions_urns_match_filter_prefix() {
        let (dispatcher, space) = minimal_dispatch_and_space().await;
        let (defs, _fs_processes) = fs_handlers_to_definitions(
            dispatcher,
            space,
            crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
        );
        for def in &defs {
            assert!(
                def.urn.starts_with(FS_NATIVE_URN_FILTER_PREFIX),
                "FS native URN `{}` does not start with the shared \
                 filter prefix `{}` — the reducer's \
                 `filter_fs_native_urns` check in `eval_new` would \
                 fail to reject this URN in user deploys, silently \
                 bypassing phase-scoped visibility.",
                def.urn,
                FS_NATIVE_URN_FILTER_PREFIX,
            );
        }
    }

    /// Every registered URN suffix in `FS_HANDLERS` must be reachable
    /// from the Definition output.  Walking FS_HANDLERS and matching
    /// against produced URNs confirms the mapping is total — no entry
    /// is silently dropped between `FS_HANDLERS.iter()` and the
    /// returned Vec.
    #[tokio::test]
    async fn fs_handlers_to_definitions_covers_every_registry_entry() {
        let (dispatcher, space) = minimal_dispatch_and_space().await;
        let (defs, _fs_processes) = fs_handlers_to_definitions(
            dispatcher,
            space,
            crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
        );
        let def_urns: std::collections::HashSet<&str> =
            defs.iter().map(|d| d.urn.as_str()).collect();
        for entry in FS_HANDLERS.iter() {
            let expected_urn = format!("rho:io:fs:native:1.0.0/{}", entry.urn_suffix);
            assert!(
                def_urns.contains(expected_urn.as_str()),
                "FS_HANDLERS entry `{}` (urn_suffix=`{}`) is missing \
                 from fs_handlers_to_definitions output — the \
                 FS_HANDLERS → Definition mapping is not total.  \
                 Expected URN: `{}`",
                entry.name,
                entry.urn_suffix,
                expected_urn,
            );
        }
    }

    /// Trait-exempt `fs_remove_dir` URN must appear in `urn_map`
    /// after `setup_maps_and_refs` runs.  Slice 5.43 added the
    /// registration so FsGenesis composition can bind
    /// `new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`)` without
    /// tripping eval_new's "No value set for URN" check.  Pinning the
    /// urn_map entry here catches a regression that removes the
    /// explicit registration (which is NOT auto-generated from
    /// FS_HANDLERS — the handler is trait-exempt).
    #[test]
    fn setup_maps_and_refs_registers_fs_remove_dir_urn() {
        let (_, _, _, urn_map, _) = setup_maps_and_refs(&Vec::new());
        let expected_urn = format!(
            "{}removeDir",
            crate::rust::interpreter::io::FS_NATIVE_URN_PREFIX_VERSIONED
        );
        assert!(
            urn_map.contains_key(&expected_urn),
            "urn_map missing `{expected_urn}`.  Trait-exempt fs_remove_dir \
             registration (slice 5.43) was removed — FsGenesis composition \
             will trip `BugFoundError` on `new fsRemoveDir(`...`)` at \
             genesis-time.  See `setup_maps_and_refs` for the explicit \
             registration site."
        );
    }

    /// Trait-exempt `fs_remove_dir` proc_def must carry arity 5 (so
    /// `fsRemoveDir!(rootCanon, rel, recursive, cmode, ack)` matches
    /// the system-process reader) and `BodyRefs::FS_REMOVE_DIR` =
    /// 58 (so the dispatcher routes to the stub handler registered
    /// in `dispatch_table_creator`).  Slice 5.44 added the stub
    /// handler to prevent a user-held Dir cap from hitting
    /// "dispatch: no function for 58" when invoking `removeDir` at
    /// state-execution; this test catches a regression that reverts
    /// the proc_def entry or renumbers the body_ref.
    #[test]
    fn setup_maps_and_refs_registers_fs_remove_dir_proc_def() {
        let (_, _, _, _, proc_defs) = setup_maps_and_refs(&Vec::new());
        let expected_fixed_channel = FixedChannels::fs_remove_dir();
        let found = proc_defs
            .iter()
            .find(|(fc, _, _, br)| *fc == expected_fixed_channel && *br == BodyRefs::FS_REMOVE_DIR);
        let Some((_, arity, remainder, body_ref)) = found else {
            panic!(
                "proc_defs missing an entry with fixed_channel = \
                 FixedChannels::fs_remove_dir() AND body_ref = \
                 BodyRefs::FS_REMOVE_DIR ({}).  Trait-exempt \
                 fs_remove_dir registration (slice 5.43) was removed; \
                 Dir.rho's `fsRemoveDir!(...)` would hit \"dispatch: \
                 no function for {}\".",
                BodyRefs::FS_REMOVE_DIR,
                BodyRefs::FS_REMOVE_DIR,
            );
        };
        assert_eq!(
            *arity, 5,
            "fs_remove_dir proc_def arity must be 5 to match \
             Dir.rho's `fsRemoveDir!(canonRoot, rel, recursive, \
             cmode, *retCh)` call site; got {}",
            arity
        );
        assert!(
            remainder.is_none(),
            "fs_remove_dir proc_def remainder must be None (no \
             rest-pattern); got {remainder:?}"
        );
        assert_eq!(*body_ref, BodyRefs::FS_REMOVE_DIR);
    }

    /// Every `FS_HANDLERS` entry's `arity` (declared `usize`) must
    /// fit in the `Arity` type (currently `i32`) WITHOUT truncation
    /// on the `as Arity` cast performed by `fs_handlers_to_definitions`
    /// (slice 5.31) and `setup_maps_and_refs` (slice 5.43).  Rust's
    /// `as` conversion on an out-of-range `usize → i32` silently
    /// wraps (two's-complement) rather than panicking.  Current
    /// handler arities fall in `[1, 7]` — far from `i32::MAX` — but a
    /// defensive pin keeps the compiler-silent wrap from surfacing as
    /// a hard-to-diagnose "dispatcher mismatch on arity" at runtime.
    ///
    /// If this test fires, either the `FsHandlerEntry::arity` field
    /// gained an entry out of `[0, i32::MAX]`, OR the `Arity` type
    /// alias was renarrowed (e.g., `i16`).  In either case the
    /// dispatcher's arity match would silently see the wrapped
    /// value.  Fix: widen `Arity` to accommodate, or audit the new
    /// entry.
    #[test]
    fn fs_handlers_arity_fits_in_arity_type() {
        let max_arity: usize = Arity::MAX as usize;
        for entry in FS_HANDLERS.iter() {
            assert!(
                entry.arity <= max_arity,
                "FS_HANDLERS entry `{}` (urn_suffix = `{}`) has \
                 arity = {}, which overflows the dispatcher's Arity \
                 type (max = {}).  The `as Arity` cast in \
                 fs_handlers_to_definitions + setup_maps_and_refs \
                 would silently wrap this value, producing a \
                 negative arity in the Definition.  Widen the Arity \
                 type alias in system_processes.rs or correct the \
                 entry.",
                entry.name,
                entry.urn_suffix,
                entry.arity,
                max_arity,
            );
        }
    }

    /// `fs_remove_dir` is trait-exempt — its four divergence reply
    /// shapes don't fit the `FsHandler` trait (see
    /// `handler_trait::fs_handler` docstring "Trait-exempt handler
    /// (fs_remove_dir)").  It MUST NOT appear in `FS_HANDLERS`
    /// because the trait-exempt `FsProcesses::fs_remove_dir` method
    /// (ported in slices 5.136-5.141, URN-registered in slice 5.142)
    /// is registered separately in `dispatch_table_creator`.  If both
    /// were registered, the dispatcher's `HashMap<body_ref, handler>`
    /// insert would silently clobber one with the other — depending
    /// on insertion order, callers might get either the trait-based
    /// response or the trait-exempt handler's divergence-reply-shape
    /// response, with no compile-time or load-time warning.
    ///
    /// This test catches a regression where someone adds a
    /// `fs_remove_dir` entry to `FS_HANDLERS` without first removing
    /// the explicit trait-exempt registration in
    /// `dispatch_table_creator`.  Pins both axes:
    /// `urn_suffix == "removeDir"` AND `body_ref ==
    /// BodyRefs::FS_REMOVE_DIR` — either match would collide.
    #[test]
    fn fs_remove_dir_stays_trait_exempt_in_fs_handlers() {
        for entry in FS_HANDLERS.iter() {
            assert_ne!(
                entry.urn_suffix, "removeDir",
                "FS_HANDLERS contains an entry with urn_suffix = \
                 \"removeDir\" (name = `{}`).  fs_remove_dir is \
                 trait-exempt; the explicit trait-exempt registration \
                 in `dispatch_table_creator` would collide at the \
                 dispatcher's body_ref HashMap, silently clobbering \
                 one handler with the other.  Either (a) remove the \
                 new FS_HANDLERS entry if fs_remove_dir still needs \
                 the four divergence reply shapes, or (b) if the \
                 handler is refactored to fit the FsHandler trait, \
                 remove the explicit trait-exempt registration in \
                 `dispatch_table_creator` and this test.",
                entry.name,
            );
            assert_ne!(
                entry.body_ref,
                BodyRefs::FS_REMOVE_DIR,
                "FS_HANDLERS contains an entry with body_ref = \
                 BodyRefs::FS_REMOVE_DIR ({}) (name = `{}`, \
                 urn_suffix = `{}`).  fs_remove_dir is trait-exempt; \
                 the body_ref slot is reserved for the explicit \
                 trait-exempt registration (slice 5.142) and must \
                 not be reused.",
                BodyRefs::FS_REMOVE_DIR,
                entry.name,
                entry.urn_suffix,
            );
        }
    }
}
