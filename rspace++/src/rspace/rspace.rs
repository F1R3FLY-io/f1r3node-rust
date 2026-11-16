// See rspace/src/main/scala/coop/rchain/rspace/RSpace.scala

// NOTE: Manual marks are used instead of trace_i()/with_marks() because
// the functions are not async-compatible with Span trait's closure pattern.
// This matches Scala's Span[F].traceI() and withMarks() semantics.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub static LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

use async_trait::async_trait;
use dashmap::DashMap;
use serde::Serialize;
use tracing::{Level, event};

use super::candidate_order::order_candidates_with_index;
use super::checkpoint::SoftCheckpoint;
use super::data_index::{CandidatePool, RSpaceDataIndex};
use super::errors::{HistoryRepositoryError, RSpaceError};
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::stable_hash_provider::StableHashSerialize;
use super::history::history_reader::HistoryReader;
use super::history::instances::radix_history::RadixHistory;
use super::hot_store::{
    AcceptedMatch, AcceptedMatchOutcome, ContinuationVersion, StoredContinuationCommit,
};
use super::logging::{BasicLogger, StepCommObserver};
use super::r#match::Match;
use super::metrics_constants::{
    CHANGES_SPAN, CONSUME_COMM_LABEL, HISTORY_CHECKPOINT_SPAN, LOCKED_CONSUME_SPAN,
    LOCKED_PRODUCE_SPAN, PRODUCE_COMM_LABEL, RESET_SPAN, RETAINED_AUTOMATON_CACHE_HIT_METRIC,
    RETAINED_AUTOMATON_CANDIDATES_METRIC, RETAINED_AUTOMATON_COMPILE_METRIC,
    RETAINED_AUTOMATON_COMPILE_NS_METRIC, RETAINED_AUTOMATON_DISTINCT_TERMINAL_PATHS_METRIC,
    RETAINED_AUTOMATON_FALLBACK_PATTERNS_METRIC, RETAINED_AUTOMATON_INDEXED_CANDIDATES_METRIC,
    RETAINED_AUTOMATON_INDEXED_PATTERNS_METRIC, RETAINED_AUTOMATON_PATTERNS_METRIC,
    RETAINED_AUTOMATON_SHARED_TERMINALS_METRIC, RETAINED_AUTOMATON_TERMINALS_METRIC,
    REVERT_SOFT_CHECKPOINT_SPAN, RSPACE_METRICS_SOURCE,
    RSPACE_PRODUCE_UNCONSUMED_DATUM_STORED_METRIC,
};
use super::replay_rspace::ReplayRSpace;
use super::retained_set_automaton::RetainedSetAutomaton;
use super::rho_path::RhoPathEncode;
use super::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceCandidate, MaybeProduceResult, RSpaceResult,
};
use super::trace::Log;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use super::util::unpack_option;
use crate::rspace::checkpoint::Checkpoint;
use crate::rspace::history::durable_history_repository::DurableHistoryRepositoryV1;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::hot_store::{HotStore, HotStoreInstances};
use crate::rspace::internal::*;
#[cfg(test)]
use crate::rspace::persistence::journal_owner_hooks;
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::serializers::serializers::CandidateOrderingBytes;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::state::rspace_state_authority::{
    AuthorityRouteV1, DurableBackendV1, DurableViewV1,
};

#[derive(Clone)]
pub struct RSpaceStore {
    authority: Arc<crate::rspace::state::rspace_state_authority::RSpaceStateAuthority>,
}

impl RSpaceStore {
    pub fn new(
        authority: Arc<crate::rspace::state::rspace_state_authority::RSpaceStateAuthority>,
    ) -> Self {
        Self { authority }
    }

    pub fn in_memory() -> Self {
        Self::new(crate::rspace::state::rspace_state_authority::RSpaceStateAuthority::in_memory())
    }

    pub fn authority(
        &self,
    ) -> Arc<crate::rspace::state::rspace_state_authority::RSpaceStateAuthority> {
        Arc::clone(&self.authority)
    }
}

struct CachedRetainedSetAutomaton {
    version: ContinuationVersion,
    automaton: RetainedSetAutomaton,
}

#[repr(C)]
#[derive(Clone)]
pub struct RSpace<C, P, A, K> {
    history_repository:
        Arc<std::sync::RwLock<Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>>>,
    pub store: Arc<std::sync::RwLock<Arc<Box<dyn HotStore<C, P, A, K>>>>>,
    installs: Arc<std::sync::Mutex<HashMap<Vec<C>, Install<P, K>>>>,
    event_log: Arc<std::sync::Mutex<Log>>,
    produce_counter: Arc<std::sync::Mutex<BTreeMap<Produce, i32>>>,
    matcher: Arc<Box<dyn Match<P, A, K>>>,
    phase_a_locks: Arc<DashMap<u64, Arc<tokio::sync::Mutex<()>>>>,
    phase_b_locks: Arc<DashMap<u64, Arc<tokio::sync::Mutex<()>>>>,
    /// Shared permits protect ordinary mutations; exclusive permits protect
    /// checkpoint/reset authority transitions without serializing mutations.
    operation_gate: Arc<tokio::sync::RwLock<()>>,
    /// Live single-step COMM observer (MeTTaIL reactive stepper). `None` in
    /// production: the `log_comm` seam then pays only one branch-predicted
    /// `is_none` check per COMM — zero allocation, no vtable. Installed via
    /// [`RSpace::set_step_observer`]. Not part of any FFI surface (RSpace
    /// has no `extern "C"` consumers); kept after the original production
    /// fields so their offsets are unchanged.
    step_observer: Option<Arc<dyn StepCommObserver<C, P, A, K>>>,
    retained_set_automata: Arc<DashMap<Vec<C>, CachedRetainedSetAutomaton>>,
    durable_view: Option<Arc<DurableViewV1<C, P, A, K>>>,
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    pub fn get_store(&self) -> Arc<Box<dyn HotStore<C, P, A, K>>> {
        self.store.read().expect("store read lock").clone()
    }

    fn channel_hash(channel: &C) -> u64 {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        channel.hash(&mut hasher);
        hasher.finish()
    }

    async fn acquire_locks(
        lock_map: &DashMap<u64, Arc<tokio::sync::Mutex<()>>>,
        keys: &[u64],
    ) -> ChannelLockGuard {
        let mut sorted_keys: Vec<u64> = keys.to_vec();
        sorted_keys.sort();
        sorted_keys.dedup();

        let mut held: Vec<HeldLock> = Vec::with_capacity(sorted_keys.len());
        for k in &sorted_keys {
            let lock = lock_map
                .entry(*k)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone();
            let guard = lock.lock_owned().await;
            held.push(HeldLock { _guard: guard });
        }

        ChannelLockGuard { _held: held }
    }

    async fn consume_lock(&self, channel_hashes: &[u64]) -> (ChannelLockGuard, ChannelLockGuard) {
        let phase_a = Self::acquire_locks(&self.phase_a_locks, channel_hashes).await;
        let phase_b = Self::acquire_locks(&self.phase_b_locks, channel_hashes).await;
        (phase_a, phase_b)
    }

    async fn produce_lock(&self, channel: &C) -> (ChannelLockGuard, ChannelLockGuard) {
        let channel_hash = Self::channel_hash(channel);
        let phase_a = Self::acquire_locks(&self.phase_a_locks, &[channel_hash]).await;

        let store = self.get_store();
        let join_hashes: Vec<u64> = store
            .get_joins(channel)
            .into_iter()
            .flatten()
            .map(|ch| Self::channel_hash(&ch))
            .collect();

        let phase_b = Self::acquire_locks(&self.phase_b_locks, &join_hashes).await;
        (phase_a, phase_b)
    }

    pub fn get_history_repository(
        &self,
    ) -> Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>> {
        self.history_repository
            .read()
            .expect("history read lock")
            .clone()
    }
}

struct HeldLock {
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

struct ChannelLockGuard {
    _held: Vec<HeldLock>,
}

impl<C, P, A, K> SpaceMatcher<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
}

#[async_trait]
impl<C, P, A, K> ISpace<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
        if let Some(view) = self.durable_view.as_deref() {
            return self.create_durable_checkpoint(view).await;
        }
        let _operation_guard = self.operation_gate.write().await;
        // Span[F].withMarks("create-checkpoint") from Scala - works because this is NOT
        // async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "create-checkpoint").entered();
        event!(Level::DEBUG, mark = "started-create-checkpoint", "create_checkpoint");

        // Get changes with span
        let changes = {
            let _changes_span =
                tracing::info_span!(target: "f1r3fly.rspace", CHANGES_SPAN).entered();
            self.get_store().checkpoint_changes()?
        };

        // Create history checkpoint with span
        let next_history = {
            let _history_span =
                tracing::info_span!(target: "f1r3fly.rspace", HISTORY_CHECKPOINT_SPAN).entered();
            self.get_history_repository().checkpoint(changes)?
        };
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        let log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let _ = std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));

        let history_reader = self
            .get_history_repository()
            .get_history_reader(&self.get_history_repository().root())?;

        self.create_new_hot_store(history_reader);
        self.restore_installs();

        // Mark the completion of create-checkpoint
        event!(Level::DEBUG, mark = "finished-create-checkpoint", "create_checkpoint");

        Ok(Checkpoint {
            root: self.get_history_repository().root(),
            log,
        })
    }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
        let _operation_guard = self.operation_gate.write().await;
        let _span = tracing::info_span!(target: "f1r3fly.rspace", RESET_SPAN).entered();
        let next_history = self.get_history_repository().reset(root)?;
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        *self.event_log.lock().expect("event log lock") = Vec::new();
        *self.produce_counter.lock().expect("produce counter lock") = BTreeMap::new();

        self.phase_a_locks.clear();
        self.phase_b_locks.clear();

        let history_reader = self.get_history_repository().get_history_reader(root)?;
        self.create_new_hot_store(history_reader);
        self.restore_installs();

        Ok(())
    }

    async fn consume_result(
        &self,
        channel: Vec<C>,
        pattern: Vec<P>,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        let consume_res = self
            .consume(channel, pattern, K::default(), false, BTreeSet::new())
            .await?;
        Ok(unpack_option(&consume_res))
    }

    async fn get_data(&self, channel: &C) -> Vec<Datum<A>> { self.get_store().get_data(channel) }

    async fn get_waiting_continuations(&self, channels: Vec<C>) -> Vec<WaitingContinuation<P, K>> {
        self.get_store().get_continuations(&channels)
    }

    async fn get_joins(&self, channel: C) -> Vec<Vec<C>> { self.get_store().get_joins(&channel) }

    async fn remove_all_data(&self, channel: &C) -> Result<(), RSpaceError> {
        let _operation_guard = self.operation_gate.read().await;
        let len = self.get_store().get_data(channel).len();
        for index in (0..len).rev() {
            self.get_store().remove_datum(channel, index as i32)?;
        }
        Ok(())
    }

    async fn remove_all_continuations(&self, channels: Vec<C>) -> Result<(), RSpaceError> {
        let _operation_guard = self.operation_gate.read().await;
        let len = self.get_store().get_continuations(&channels).len();
        for index in (0..len).rev() {
            let _ = self
                .get_store()
                .remove_continuation(&channels, index as i32);
        }
        Ok(())
    }

    async fn clear(&self) -> Result<(), RSpaceError> {
        self.reset(&RadixHistory::empty_root_node_hash()).await
    }

    async fn get_root(&self) -> Blake2b256Hash { self.get_history_repository().root() }

    async fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> { self.get_store().to_map() }

    async fn create_soft_checkpoint(&self) -> SoftCheckpoint<C, P, A, K> {
        let _operation_guard = self.operation_gate.write().await;
        let cache_snapshot = self.get_store().snapshot();
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let curr_produce_counter =
            std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));
        let pending = self
            .get_store()
            .pending_deltas()
            .expect("live hot store journal snapshot");
        let pending_data_delta = pending
            .data
            .expect("live hot store must expose a data journal for soft checkpoints");
        let pending_continuation_delta = pending
            .continuations
            .expect("live hot store must expose a continuation journal for soft checkpoints");
        let pending_continuation_content = pending
            .continuation_content
            .expect("live hot store must expose continuation content transitions");
        let pending_continuation_source_bytes = pending
            .continuation_source_bytes
            .expect("live hot store must expose continuation source bytes");
        let pending_join_delta = pending
            .joins
            .expect("live hot store must expose a join journal for soft checkpoints");

        SoftCheckpoint {
            cache_snapshot,
            log: curr_event_log,
            produce_counter: curr_produce_counter,
            pending_data_delta,
            pending_continuation_delta,
            pending_continuation_content,
            pending_continuation_source_bytes,
            pending_join_delta,
        }
    }

    async fn take_event_log(&self) -> Log {
        let _operation_guard = self.operation_gate.write().await;
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let _ = std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));
        curr_event_log
    }

    /// Forward the reactive single-step gate from the installed observer (if
    /// any). `None` in production (no observer) — the reducer then never
    /// pauses. The same `StepGate` the stepper `release_one`s after
    /// publishing each COMM event.
    fn step_gate(&self) -> Option<Arc<super::logging::StepGate>> {
        self.step_observer
            .as_ref()
            .and_then(|observer| observer.step_gate())
    }

    /// Forward a non-COMM structural reduction to the installed observer (if
    /// any) — the symmetric emit twin of `step_gate`. `None`-op in
    /// production (one branch-predicted `is_none` check, no allocation, no
    /// vtable). `redex` is borrowed; the observer clones only when it captures.
    fn observe_reduction(&self, redex: &C, kind: super::logging::ReductionKind) {
        if let Some(observer) = &self.step_observer {
            observer.observe_reduction(redex, kind);
        }
    }

    fn has_reduction_observer(&self) -> bool { self.step_observer.is_some() }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError> {
        let _operation_guard = self.operation_gate.write().await;
        let _span =
            tracing::info_span!(target: "f1r3fly.rspace", REVERT_SOFT_CHECKPOINT_SPAN).entered();
        let history = self.get_history_repository();
        let history_reader = history.get_history_reader(&history.root())?;
        let hot_store = HotStoreInstances::create_from_hs_and_hr(
            checkpoint.cache_snapshot,
            history_reader.base(),
        );

        *self.store.write().expect("store write lock") = Arc::new(hot_store);
        self.get_store().restore_pending_deltas(
            checkpoint.pending_data_delta,
            checkpoint.pending_continuation_delta,
            checkpoint.pending_continuation_content,
            checkpoint.pending_continuation_source_bytes,
            checkpoint.pending_join_delta,
        )?;
        self.retained_set_automata.clear();
        *self.event_log.lock().expect("event log lock") = checkpoint.log;
        *self.produce_counter.lock().expect("produce counter lock") = checkpoint.produce_counter;

        Ok(())
    }

    async fn consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let _operation_guard = self.operation_gate.read().await;
        // ★ A MALFORMED CONSUME IS A DECIDABLE NEGATIVE, NOT AN INVARIANT
        // VIOLATION — so it is REFUSED, through the `Result` this signature has
        // always carried, rather than aborting the process.
        //
        // The distinction that decides the disposition is whether the CALLER'S
        // OWN INPUTS can express the fault. Here they can: the two lengths are
        // right there in the arguments, and `ConsumeParams` puts `channels` and
        // `patterns` in two INDEPENDENT repeated protobuf fields
        // (`models/src/main/protobuf/RSpacePlusPlusTypes.proto`), so any wire
        // encoding at all can carry `channels.len() != patterns.len()`. That is
        // the opposite classification from an absent required child in a `Par`
        // (#127/#136), which no honest producer can emit and which therefore
        // stays an invariant.
        //
        // ⚠ CONSENSUS-VISIBLE. This turns "every node aborts" into "this call
        // fails". A validator that today dies would instead reject. It requires
        // a coordinated `Validate::version` bump, which is F1r3node's act, not
        // this file's — `casper/src/rust/validate.rs` compares versions for
        // EXACT equality with no activation-height machinery.
        //
        // ⚠ THE MESSAGES AND THE VARIANT ARE DELIBERATELY UNCHANGED from
        // `ReplayRSpace::locked_install_internal`, which already refused this
        // exact condition this exact way. Four sites now agree; before this
        // change three panicked and one returned `Err`, which was itself a
        // play/replay asymmetry (see the sibling table in
        // `rspace++/tests/consume_arity_refusal.rs`).
        if channels.is_empty() {
            Err(RSpaceError::BugFoundError("RUST ERROR: channels can't be empty".to_string()))
        } else if channels.len() != patterns.len() {
            Err(RSpaceError::BugFoundError(
                "RUST ERROR: channels.length must equal patterns.length".to_string(),
            ))
        } else {
            let consume_ref = Consume::create(&channels, &patterns, &continuation, persist);

            let lock_start = Instant::now();
            let channel_hashes: Vec<u64> =
                channels.iter().map(|ch| Self::channel_hash(ch)).collect();
            let _lock_guard = self.consume_lock(&channel_hashes).await;
            let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::SeqCst);
            tracing::trace!(target: "rspace.lock_order", seq = seq, op = "consume", hashes = ?channel_hashes, "lock acquired");
            metrics::counter!("rspace.consume.lock_acquire_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(lock_start.elapsed().as_nanos() as u64);

            metrics::counter!("rspace.consume.calls", "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            let start = Instant::now();
            let result = self.locked_consume(
                &channels,
                &patterns,
                &continuation,
                persist,
                &peeks,
                &consume_ref,
            );
            let duration = start.elapsed();
            metrics::histogram!("comm_consume_time_seconds", "source" => RSPACE_METRICS_SOURCE)
                .record(duration.as_secs_f64());
            result
        }
    }

    async fn produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        let _operation_guard = self.operation_gate.read().await;
        let produce_ref = Produce::create(&channel, &data, persist);

        let lock_start = Instant::now();
        let _lock_guard = self.produce_lock(&channel).await;
        let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::SeqCst);
        tracing::trace!(target: "rspace.lock_order", seq = seq, op = "produce", hash = Self::channel_hash(&channel), "lock acquired");
        metrics::counter!("rspace.produce.lock_acquire_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(lock_start.elapsed().as_nanos() as u64);

        metrics::counter!("rspace.produce.calls", "source" => RSPACE_METRICS_SOURCE).increment(1);
        let start = Instant::now();
        let result = self.locked_produce(channel, data, persist, &produce_ref);
        let duration = start.elapsed();
        metrics::histogram!("comm_produce_time_seconds", "source" => RSPACE_METRICS_SOURCE)
            .record(duration.as_secs_f64());
        result
    }

    async fn install(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        let _operation_guard = self.operation_gate.read().await;
        metrics::counter!("rspace.install.calls", "source" => RSPACE_METRICS_SOURCE).increment(1);
        let start = Instant::now();
        let result = self.locked_install_internal(channels, patterns, continuation, true);
        let duration = start.elapsed();
        metrics::histogram!("install_time_seconds", "source" => RSPACE_METRICS_SOURCE)
            .record(duration.as_secs_f64());
        result
    }

    async fn rig_and_reset(
        &self,
        _start_root: Blake2b256Hash,
        _log: Log,
    ) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace rig_and_reset should not be called here");
    }

    async fn rig(&self, _log: Log) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace rig should not be called here");
    }

    async fn check_replay_data(&self) -> Result<(), RSpaceError> {
        panic!("\nERROR: RSpace check_replay_data should not be called here");
    }

    async fn is_replay(&self) -> bool { false }

    async fn update_produce(&self, produce_ref: Produce) -> () {
        for event in self.event_log.lock().expect("event log lock").iter_mut() {
            match event {
                Event::IoEvent(IOEvent::Produce(produce)) => {
                    if produce.hash == produce_ref.hash {
                        *produce = produce_ref.clone();
                    }
                }

                Event::Comm(comm) => {
                    let COMM {
                        produces: _produces,
                        times_repeated: _times_repeated,
                        ..
                    } = comm;

                    let updated_comm = COMM {
                        produces: _produces
                            .iter()
                            .map(|p| {
                                if p.hash == produce_ref.hash {
                                    produce_ref.clone()
                                } else {
                                    p.clone()
                                }
                            })
                            .collect(),
                        times_repeated: _times_repeated
                            .iter()
                            .map(|(k, v)| {
                                if k.hash == produce_ref.hash {
                                    (produce_ref.clone(), v.clone())
                                } else {
                                    (k.clone(), v.clone())
                                }
                            })
                            .collect(),
                        ..comm.clone()
                    };

                    *comm = updated_comm;
                }

                _ => continue,
            }
        }
    }
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + RhoPathEncode
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    /**
     * Creates [[RSpace]] from [[HistoryRepository]] and [[HotStore]].
     */
    pub fn apply(
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Box<dyn HotStore<C, P, A, K>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> RSpace<C, P, A, K>
    where
        C: Clone + Debug + Ord + Hash,
        P: Clone + Debug,
        A: Clone + Debug,
        K: Clone + Debug,
    {
        RSpace {
            history_repository: Arc::new(std::sync::RwLock::new(history_repository)),
            store: Arc::new(std::sync::RwLock::new(Arc::new(store))),
            matcher,
            installs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_log: Arc::new(std::sync::Mutex::new(Vec::new())),
            produce_counter: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            phase_a_locks: Arc::new(DashMap::new()),
            phase_b_locks: Arc::new(DashMap::new()),
            operation_gate: Arc::new(tokio::sync::RwLock::new(())),
            step_observer: None,
            retained_set_automata: Arc::new(DashMap::new()),
            durable_view: None,
        }
    }

    /// Install (or clear) the live single-step COMM observer. Off (`None`) by
    /// default; the MeTTaIL reactive stepper sets `Some(..)` before running
    /// `inj`, and the `log_comm` seam then emits each committed COMM to it.
    /// Idempotent; pass `None` to detach.
    pub fn set_step_observer(&mut self, observer: Option<Arc<dyn StepCommObserver<C, P, A, K>>>) {
        self.step_observer = observer;
    }

    pub fn create(
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> Result<RSpace<C, P, A, K>, HistoryRepositoryError>
    where
        C: Clone
            + Debug
            + Default
            + Send
            + Sync
            + Serialize
            + Ord
            + Hash
            + ColdStoreDecode
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        if let AuthorityRouteV1::Durable(durable) = store.authority.route() {
            let (repository, hot_store, view) = Self::create_durable_view(durable)?;
            let mut space = Self::apply(Arc::new(repository), hot_store, matcher);
            space.durable_view = Some(Arc::new(view));
            return Ok(space);
        }
        let setup = Self::create_history_repo(store)?;
        let (history_reader, store) = setup;
        let space = Self::apply(Arc::new(history_reader), store, matcher);
        Ok(space)
    }

    pub fn create_with_replay(
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> Result<(RSpace<C, P, A, K>, ReplayRSpace<C, P, A, K>), HistoryRepositoryError>
    where
        C: Clone
            + Debug
            + Default
            + Send
            + Sync
            + Serialize
            + Ord
            + Hash
            + ColdStoreDecode
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        let setup = Self::create_history_repo(store)?;
        let (history_repo, store) = setup;
        let history_repo_arc = Arc::new(history_repo);

        // Play
        let space = Self::apply(history_repo_arc.clone(), store, matcher.clone());
        // Replay
        let history_reader: Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>> =
            history_repo_arc.get_history_reader(&history_repo_arc.root())?;
        let replay_store = HotStoreInstances::create_from_hr(history_reader.base());
        let replay = ReplayRSpace::apply_with_logger(
            history_repo_arc.clone(),
            Arc::new(replay_store),
            matcher.clone(),
            Box::new(BasicLogger::new()),
        );
        Ok((space, replay))
    }

    /**
     * Creates [[HistoryRepository]] and [[HotStore]].
     */
    pub fn create_history_repo(
        store: RSpaceStore,
    ) -> Result<
        (
            Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
            Box<dyn HotStore<C, P, A, K>>,
        ),
        HistoryRepositoryError,
    >
    where
        C: Clone
            + Debug
            + Default
            + Send
            + Sync
            + Serialize
            + ColdStoreDecode
            + Eq
            + Hash
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        if let AuthorityRouteV1::Durable(durable) = store.authority.route() {
            return Self::create_durable_history_repo(durable);
        }

        let history_repo = HistoryRepositoryInstances::repository(store.authority)?;

        let history_reader = history_repo.get_history_reader(&history_repo.root())?;

        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());

        Ok((history_repo, hot_store))
    }

    fn create_durable_history_repo(
        durable: &DurableBackendV1,
    ) -> Result<
        (
            Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
            Box<dyn HotStore<C, P, A, K>>,
        ),
        HistoryRepositoryError,
    >
    where
        C: Clone
            + Debug
            + Default
            + Send
            + Sync
            + Serialize
            + ColdStoreDecode
            + Eq
            + Hash
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        let slot = durable.owner_slot::<C, P, A, K>()?;
        let repository = DurableHistoryRepositoryV1::new(slot.published_core())?;
        let hot_store = slot.view_store(&repository.root())?;
        Ok((Box::new(repository), hot_store))
    }

    fn create_durable_view(
        durable: &DurableBackendV1,
    ) -> Result<
        (
            Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
            Box<dyn HotStore<C, P, A, K>>,
            DurableViewV1<C, P, A, K>,
        ),
        HistoryRepositoryError,
    >
    where
        C: Clone
            + Debug
            + Default
            + Send
            + Sync
            + Serialize
            + ColdStoreDecode
            + Eq
            + Hash
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        let slot = durable.owner_slot::<C, P, A, K>()?;
        let repository = DurableHistoryRepositoryV1::new(slot.published_core())?;
        let (base, hot_store) = slot.view_at(&repository.root())?;
        Ok((Box::new(repository), hot_store, DurableViewV1::new(slot, base)))
    }

    async fn create_durable_checkpoint(
        &self,
        view: &DurableViewV1<C, P, A, K>,
    ) -> Result<Checkpoint, RSpaceError> {
        let permit = Arc::clone(&self.operation_gate).write_owned().await;
        event!(Level::DEBUG, mark = "started-create-checkpoint", "create_checkpoint");
        let hot = self.get_store();
        let (root, _operation_guard) = view.checkpoint(permit, &self.operation_gate, &hot).await?;
        #[cfg(test)]
        journal_owner_hooks::run(
            journal_owner_hooks::JournalOwnerHookPoint::BeforeViewInstallation,
        );
        let (base, next_store) = view.view_at(&root)?;
        let next_history = self.get_history_repository().reset(&root)?;
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);
        let log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        self.produce_counter
            .lock()
            .expect("produce counter lock")
            .clear();
        *self.store.write().expect("store write lock") = Arc::new(next_store);
        self.retained_set_automata.clear();
        view.record_base(base);
        self.restore_installs();
        event!(Level::DEBUG, mark = "finished-create-checkpoint", "create_checkpoint");
        Ok(Checkpoint { root, log })
    }

    fn produce_counters(&self, produce_refs: &[Produce]) -> BTreeMap<Produce, i32> {
        produce_refs
            .iter()
            .cloned()
            .map(|p| {
                (
                    p.clone(),
                    self.produce_counter
                        .lock()
                        .expect("produce counter lock")
                        .get(&p)
                        .unwrap_or(&0)
                        .clone(),
                )
            })
            .collect()
    }

    fn locked_consume(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persist: bool,
        peeks: &BTreeSet<i32>,
        consume_ref: &Consume,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-consume") from Scala
        let _span = tracing::info_span!(target: "f1r3fly.rspace", LOCKED_CONSUME_SPAN).entered();
        event!(Level::DEBUG, mark = "started-locked-consume", "locked_consume");

        let t0 = Instant::now();
        self.log_consume(consume_ref, channels, patterns, continuation, persist, peeks);
        metrics::counter!("rspace.consume.log_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let wk = WaitingContinuation {
            patterns: Arc::new(patterns.to_vec()),
            continuation: Arc::new(continuation.clone()),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        loop {
            let t1 = Instant::now();
            let mut channel_to_indexed_data = self.fetch_channel_to_index_data(channels);
            metrics::counter!("rspace.consume.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t1.elapsed().as_nanos() as u64);

            let t2 = Instant::now();
            // Borrow-zip: no per-consume channel/pattern clones.
            let zipped: Vec<(&C, &P)> = channels.iter().zip(patterns.iter()).collect();
            // The complete, guard-aware candidate search (plan §7.12 / defect D1).
            // A `where` guard participates in SELECTION, not only approval.
            let options: Option<Vec<ConsumeCandidate<C, A>>> = self
                .extract_guarded_data_candidates(
                    &self.matcher,
                    &zipped,
                    continuation,
                    &mut channel_to_indexed_data,
                );
            metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t2.elapsed().as_nanos() as u64);

            match options {
                Some(data_candidates) => {
                    let t3 = Instant::now();
                    if !self.try_commit_data_match(&data_candidates)? {
                        // A checkpoint/reset or a concurrent accepted ticket
                        // changed the exact trie multiplicities after selection.
                        // Re-select from the current CoW root; never publish a
                        // COMM for the refused proposal and never cap retries.
                        continue;
                    }
                    let produce_counters_closure =
                        |produces: &[Produce]| self.produce_counters(produces);
                    self.log_comm(
                        channels,
                        &wk,
                        &data_candidates,
                        COMM::new(
                            &data_candidates,
                            consume_ref.clone(),
                            peeks.clone(),
                            produce_counters_closure,
                        ),
                        "comm.consume",
                    );
                    metrics::counter!("rspace.consume.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t3.elapsed().as_nanos() as u64);
                    event!(Level::DEBUG, mark = "finished-locked-consume", "locked_consume");
                    return Ok(self.wrap_result(channels, &wk, consume_ref, &data_candidates));
                }
                None => {
                    let t3 = Instant::now();
                    self.store_waiting_continuation(channels.to_vec(), wk);
                    metrics::counter!("rspace.consume.store_continuation_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t3.elapsed().as_nanos() as u64);
                    event!(Level::DEBUG, mark = "finished-locked-consume", "locked_consume");
                    return Ok(None);
                }
            }
        }
    }

    /*
     * Here, we create a cache of the data at each channel as
     * `channelToIndexedData` which is used for finding matches.  When a
     * speculative match is found, we can remove the matching datum from the
     * remaining data candidates in the cache.
     *
     * Put another way, this allows us to speculatively remove matching data
     * without affecting the actual store contents.
     */
    fn fetch_channel_to_index_data(&self, channels: &[C]) -> HashMap<C, CandidatePool<A>> {
        let mut map = HashMap::with_capacity(channels.len());
        for c in channels {
            map.insert(c.clone(), self.get_store().get_candidate_pool(c));
        }
        map
    }

    fn locked_produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: &Produce,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-produce") from Scala
        let _span = tracing::info_span!(target: "f1r3fly.rspace", LOCKED_PRODUCE_SPAN).entered();
        event!(Level::DEBUG, mark = "started-locked-produce", "locked_produce");

        self.log_produce(produce_ref, &channel, &data, persist);

        // Wrap the produced payload in an `Arc` once — the speculative
        // candidate below and the `store_data` fallthrough share it by
        // refcount (previously each took a deep copy).
        let data = Arc::new(data);

        let arrival = Datum {
            a: Arc::clone(&data),
            persist,
            source: produce_ref.clone(),
        };

        loop {
            let t0 = Instant::now();
            let grouped_channels = self.get_store().get_joins(&channel);
            metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t0.elapsed().as_nanos() as u64);

            let t1 = Instant::now();
            let extracted =
                self.extract_produce_candidate(grouped_channels, channel.clone(), arrival.clone());
            metrics::counter!("rspace.produce.extract_candidate_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t1.elapsed().as_nanos() as u64);

            match extracted {
                Some(produce_candidate) => {
                    let t2 = Instant::now();
                    // ★ The produced datum must be either CONSUMED or STORED —
                    // never neither. It rides in its own candidate pool under the
                    // distinct fresh address, so the fired COMM consumed it exactly
                    // when a candidate carries that address.
                    //
                    // With the guard-aware selector this is unreachable from any
                    // state the fixed matcher itself produced, and the reasoning is
                    // worth writing down because it is what makes the selector safe
                    // on this path: at quiescence no selection of RESTING data
                    // satisfies any waiting continuation (else the consume or
                    // produce that created that state would have fired it), and
                    // admissibility is monotone in the data — adding this datum can
                    // only create selections that CONTAIN it. So every selection
                    // available here contains the fresh datum.
                    //
                    // It is reachable from a state some OTHER matcher produced: a
                    // hot store restored from a checkpoint written before this fix
                    // can hold a resting datum stranded by defect D1 next to the
                    // continuation it satisfies, and a later produce of a datum
                    // that does not match spatially would then fire on the stranded
                    // pair. Dropping the produced message there would be silent
                    // message loss — no rule of the rho calculus discards a
                    // message — so it is stored instead. AFTER `process_match_found`,
                    // which removes resting data by exact trie path.
                    let comm_consumed_the_produced_datum = produce_candidate
                        .data_candidates
                        .iter()
                        .any(|candidate| candidate.address.is_fresh());

                    let Some(consume_result) = self.try_process_match_found(
                        produce_candidate,
                        Some((channel.clone(), arrival.clone())),
                    )?
                    else {
                        // The opaque continuation epoch or exact datum paths
                        // went stale between selection and publication. Retry
                        // against the current store; the arrival is still only
                        // an Arc-backed proposal and has not been lost.
                        continue;
                    };

                    if !comm_consumed_the_produced_datum {
                        tracing::debug!(
                            target: "f1r3fly.rspace",
                            "a COMM fired on resting data only; the arriving datum was added in \
                             the same PathMap transaction"
                        );
                        metrics::counter!(
                            RSPACE_PRODUCE_UNCONSUMED_DATUM_STORED_METRIC,
                            "source" => RSPACE_METRICS_SOURCE
                        )
                        .increment(1);
                    }

                    metrics::counter!("rspace.produce.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t2.elapsed().as_nanos() as u64);
                    event!(Level::DEBUG, mark = "finished-locked-produce", "locked_produce");
                    return Ok(Some((consume_result.0, consume_result.1, produce_ref.clone())));
                }
                None => {
                    let t2 = Instant::now();
                    let result = self.store_data(
                        channel.clone(),
                        Arc::clone(&data),
                        persist,
                        produce_ref.clone(),
                    );
                    metrics::counter!("rspace.produce.store_data_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t2.elapsed().as_nanos() as u64);
                    event!(Level::DEBUG, mark = "finished-locked-produce", "locked_produce");
                    return Ok(result);
                }
            }
        }
    }

    /*
     * Find produce candidate
     *
     * NOTE: On Rust side, we are NOT passing functions through. Instead just the
     * data. And then in 'run_matcher_for_channels' we call the functions
     * defined below
     */
    fn extract_produce_candidate(
        &self,
        grouped_channels: Vec<Vec<C>>,
        bat_channel: C,
        data: Datum<A>,
    ) -> MaybeProduceCandidate<C, P, A, K> {
        let store = self.get_store();
        // A produce with no waiting continuation cannot inspect resting data.
        // Open this potentially large, hash-ordered catalog only if retained
        // pruning or the spatial matcher actually requests it, and share the
        // one immutable pool across repeated occurrences of the same channel.
        let bat_channel_pool: OnceLock<CandidatePool<A>> = OnceLock::new();
        let fetch_matching_continuations = |channels: Vec<C>| {
            let (version, continuations) = store.get_continuations_versioned(&channels);
            if continuations.is_empty() {
                self.retained_set_automata.remove(&channels);
                return (version, Vec::new());
            }
            let resting_data =
                bat_channel_pool.get_or_init(|| store.get_candidate_pool(&bat_channel));
            let selection = self.retained_candidate_selection(
                &channels,
                &continuations,
                version.clone(),
                &bat_channel,
                &data.a,
                resting_data,
            );
            let mut ordered = self.shuffle_with_index(continuations);
            if let Some(selection) = selection {
                ordered.retain(|(_, index)| {
                    usize::try_from(*index)
                        .ok()
                        .and_then(|index| selection.get(index))
                        .copied()
                        .unwrap_or(false)
                });
            }
            (version, ordered)
        };

        /*
         * Here, we create a cache of the data at each channel as
         * `channelToIndexedData` which is used for finding matches.  When a
         * speculative match is found, we can remove the matching datum from
         * the remaining data candidates in the cache.
         *
         * Put another way, this allows us to speculatively remove matching data
         * without affecting the actual store contents.
         *
         * In this version, we also add the produced data directly to this cache.
         */
        let fetch_matching_data = |channel| -> (C, CandidatePool<A>) {
            let mut pool = if channel == bat_channel {
                bat_channel_pool
                    .get_or_init(|| store.get_candidate_pool(&bat_channel))
                    .clone()
            } else {
                store.get_candidate_pool(&channel)
            };
            if channel == bat_channel {
                assert!(
                    pool.prepend_fresh(data.clone()),
                    "a produced candidate must be prepended before selection"
                );
            }
            (channel, pool)
        };

        self.run_matcher_for_channels(
            grouped_channels,
            fetch_matching_continuations,
            fetch_matching_data,
        )
    }

    fn retained_candidate_selection(
        &self,
        channels: &[C],
        candidates: &[WaitingContinuation<P, K>],
        version: Option<ContinuationVersion>,
        bat_channel: &C,
        datum: &A,
        resting_data: &CandidatePool<A>,
    ) -> Option<Vec<bool>> {
        if candidates.is_empty() {
            self.retained_set_automata.remove(channels);
            return Some(Vec::new());
        }
        let datum_path = self.matcher.retained_datum_path(datum)?;
        let bind_positions = channels
            .iter()
            .enumerate()
            .filter_map(|(position, channel)| (channel == bat_channel).then_some(position))
            .collect::<Vec<_>>();
        if bind_positions.is_empty() {
            return None;
        }

        let automaton = match version {
            Some(version) => {
                if let Some(cached) = self.retained_set_automata.get(channels).filter(|cached| {
                    cached.version == version &&
                        cached.automaton.candidate_count() == candidates.len()
                }) {
                    metrics::counter!(
                        RETAINED_AUTOMATON_CACHE_HIT_METRIC,
                        "source" => RSPACE_METRICS_SOURCE
                    )
                    .increment(1);
                    return Some(cached.automaton.matching_candidates_with_resting(
                        &datum_path,
                        &bind_positions,
                        Some(resting_data.stored_availability()),
                    ));
                }
                let automaton = self.compile_retained_set_automaton(candidates);
                let selection = automaton.matching_candidates_with_resting(
                    &datum_path,
                    &bind_positions,
                    Some(resting_data.stored_availability()),
                );
                self.retained_set_automata
                    .insert(channels.to_vec(), CachedRetainedSetAutomaton { version, automaton });
                return Some(selection);
            }
            None => self.compile_retained_set_automaton(candidates),
        };
        Some(automaton.matching_candidates_with_resting(
            &datum_path,
            &bind_positions,
            Some(resting_data.stored_availability()),
        ))
    }

    fn compile_retained_set_automaton(
        &self,
        candidates: &[WaitingContinuation<P, K>],
    ) -> RetainedSetAutomaton {
        let started = Instant::now();
        let automaton = RetainedSetAutomaton::compile(&**self.matcher, candidates);
        let stats = automaton.stats();
        let increment = |metric: &'static str, value: u64| {
            metrics::counter!(metric, "source" => RSPACE_METRICS_SOURCE).increment(value);
        };
        let count = |value: usize| u64::try_from(value).unwrap_or(u64::MAX);
        increment(RETAINED_AUTOMATON_COMPILE_METRIC, 1);
        increment(
            RETAINED_AUTOMATON_COMPILE_NS_METRIC,
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        );
        increment(RETAINED_AUTOMATON_CANDIDATES_METRIC, count(stats.candidate_count));
        increment(
            RETAINED_AUTOMATON_INDEXED_CANDIDATES_METRIC,
            count(stats.indexed_candidate_count),
        );
        increment(RETAINED_AUTOMATON_PATTERNS_METRIC, count(stats.pattern_count));
        increment(RETAINED_AUTOMATON_INDEXED_PATTERNS_METRIC, count(stats.indexed_pattern_count));
        increment(RETAINED_AUTOMATON_FALLBACK_PATTERNS_METRIC, count(stats.fallback_pattern_count));
        increment(RETAINED_AUTOMATON_TERMINALS_METRIC, count(stats.terminal_count));
        increment(
            RETAINED_AUTOMATON_DISTINCT_TERMINAL_PATHS_METRIC,
            count(stats.distinct_terminal_path_count),
        );
        increment(RETAINED_AUTOMATON_SHARED_TERMINALS_METRIC, count(stats.shared_terminal_count));
        automaton
    }

    /// `E(S)` for this space — see
    /// [`SpaceMatcher::enumerate_enabled_rendezvous`], which is where the whole
    /// query lives. This binder supplies nothing but the two private handles
    /// (`matcher`, hot store); `ReplayRSpace::enabled_rendezvous` is the same
    /// three lines, so play and replay cannot enumerate differently.
    ///
    /// Read-only: no store mutation, no event log entry, no produce counter
    /// movement. Costs matcher time, not phlogiston.
    pub fn enabled_rendezvous(&self) -> Vec<ProduceCandidate<C, P, A, K>> {
        self.enumerate_enabled_rendezvous(&self.matcher, &self.get_store())
    }

    /// Fire the rendezvous `pc` names: remove its continuation (unless
    /// persistent), remove each selected stored datum by exact trie path, drop
    /// the joins, append the `COMM` event, and return the continuation and
    /// matched payloads for dispatch.
    ///
    /// # Why this is `pub`
    ///
    /// `locked_produce` reaches it with a candidate it just *searched for*. A
    /// speculative evaluator must fire a candidate it *named* — a specific
    /// member of [`RSpace::enabled_rendezvous`]'s answer, which for a
    /// persistent continuation or a join is generally not the one a search
    /// would return. Forcing the choice by trimming the store instead
    /// ("install a state in which the wanted match is the only one and let
    /// the search find it") is not sound: a persistent continuation or a
    /// join over a channel bound twice still admits several selections after
    /// any such trim.
    ///
    /// The alternative to exporting it is for the caller to reimplement exact
    /// datum addressing, continuation removal, join bookkeeping and event
    /// emission outside this crate, where they could drift from consensus
    /// behaviour silently. Every production caller still reaches this method
    /// through `locked_produce`.
    pub fn process_match_found(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        match self.try_process_match_found(pc, None) {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(
                    target: "f1r3fly.rspace",
                    %error,
                    "named rendezvous commit failed before publication"
                );
                None
            }
        }
    }

    fn try_process_match_found(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
        arrival: Option<(C, Datum<A>)>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let ProduceCandidate {
            channels,
            continuation,
            continuation_address,
            data_candidates,
        } = pc;

        if continuation_address.version.is_none() {
            return Err(RSpaceError::BugFoundError(
                "named rendezvous has no continuation freshness capability".to_string(),
            ));
        }

        let WaitingContinuation {
            patterns: _patterns,
            continuation: _cont,
            persist,
            peeks,
            source: consume_ref,
        } = &continuation;

        let selected_fresh = data_candidates
            .iter()
            .any(|candidate| candidate.address.is_fresh());
        if selected_fresh && arrival.is_none() {
            // `enabled_rendezvous` never manufactures Fresh addresses. Refuse
            // a forged or cross-regime ticket rather than consuming a waiting
            // continuation without the arriving datum it names.
            return Ok(None);
        }

        let mut datum_delta = AcceptedDatumChanges::from_candidates(&data_candidates)
            .map_err(super::data_index::DataIndexError::from)?;
        if !selected_fresh {
            if let Some((arrival_channel, arrival_datum)) = &arrival {
                let path = RSpaceDataIndex::datum_path(arrival_channel, arrival_datum);
                datum_delta
                    .record_addition(&path, super::bag::Multiplicity::ONE)
                    .map_err(super::data_index::DataIndexError::from)?;
            }
        }
        let required_persistent_paths = data_candidates
            .iter()
            .filter_map(|candidate| {
                (candidate.datum.persist).then(|| match &candidate.address {
                    DatumAddress::Stored(path) => Some(Arc::clone(path)),
                    DatumAddress::Fresh => None,
                })?
            })
            .collect();
        let join_channels = data_candidates
            .iter()
            .map(|candidate| candidate.channel.clone())
            .collect();
        let expected = StoredContinuationCommit {
            address: continuation_address,
            source: continuation.source.clone(),
            persist: *persist,
            peeks: peeks.clone(),
        };
        let transition = AcceptedMatch {
            channels: channels.clone(),
            continuation: Some(expected),
            datum_delta,
            required_persistent_paths,
            join_channels,
        };
        let authoritative_continuation = match self.get_store().apply_accepted_match(transition)? {
            AcceptedMatchOutcome::Committed {
                continuation: Some(continuation),
            } => continuation,
            AcceptedMatchOutcome::Committed { continuation: None } => {
                return Err(RSpaceError::BugFoundError(
                    "accepted named match returned no authoritative continuation".to_string(),
                ));
            }
            AcceptedMatchOutcome::Stale => return Ok(None),
        };

        let produce_counters_closure = |produces: &[Produce]| self.produce_counters(produces);
        self.log_comm(
            &channels,
            &authoritative_continuation,
            &data_candidates,
            COMM::new(
                &data_candidates,
                consume_ref.clone(),
                peeks.clone(),
                produce_counters_closure,
            ),
            "comm.produce",
        );

        Ok(self.wrap_result(&channels, &authoritative_continuation, consume_ref, &data_candidates))
    }

    fn log_comm(
        &self,
        channels: &[C],
        wk: &WaitingContinuation<P, K>,
        data_candidates: &[ConsumeCandidate<C, A>],
        comm: COMM,
        label: &str,
    ) {
        // Increment counter FIRST (matching Scala) using constants to avoid memory
        // leaks Labels are always "comm.consume" or "comm.produce" based on the
        // RSpace implementation
        match label {
            "comm.consume" => {
                metrics::counter!(CONSUME_COMM_LABEL, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
            }
            "comm.produce" => {
                metrics::counter!(PRODUCE_COMM_LABEL, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
            }
            _ => {
                // This should never happen, but log if it does
                tracing::warn!("Unexpected label in log_comm: {}", label);
            }
        }

        // Live single-step emit seam. `None` in production (one branch-predicted
        // `is_none`, no alloc/vtable). When a StepCommObserver is installed,
        // hand it the full COMM payload — the rendezvous channels, the consumed
        // data, and the firing waiting-continuation (patterns + continuation) —
        // extracted to slices so the observer can clone lock-free.
        if let Some(observer) = &self.step_observer {
            // Materialize through the Arc — observer-only path (None in
            // production), cost-identical to the earlier value-shaped clone.
            let consumed: Vec<A> = data_candidates
                .iter()
                .map(|candidate| (*candidate.datum.a).clone())
                .collect();
            observer.observe_comm(
                channels,
                &consumed,
                &wk.patterns,
                &wk.continuation,
                &comm,
                label,
            );
        }

        // Then update event log (RSpace-specific behavior)
        self.event_log
            .lock()
            .expect("event log lock")
            .insert(0, Event::Comm(comm));
    }

    fn log_consume(
        &self,
        consume_ref: &Consume,
        _channels: &[C],
        _patterns: &[P],
        _continuation: &K,
        _persist: bool,
        _peeks: &BTreeSet<i32>,
    ) {
        self.event_log
            .lock()
            .expect("event log lock")
            .insert(0, Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
    }

    fn log_produce(&self, produce_ref: &Produce, _channel: &C, _data: &A, persist: bool) {
        self.event_log
            .lock()
            .expect("event log lock")
            .insert(0, Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
        if !persist {
            let mut counter = self.produce_counter.lock().expect("produce counter lock");
            let current = counter.get(produce_ref).copied().unwrap_or(0);
            counter.insert(produce_ref.clone(), current + 1);
        }
    }

    pub fn spawn(&self) -> Result<Self, RSpaceError> {
        // Span[F].withMarks("spawn") from Scala - works because this is NOT async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "spawn").entered();
        event!(Level::DEBUG, mark = "started-spawn", "spawn");

        let history_repo = self.get_history_repository();
        let next_history = history_repo.reset(&history_repo.root())?;
        let history_reader = next_history.get_history_reader(&next_history.root())?;
        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        let rspace = RSpace::apply(Arc::new(next_history), hot_store, self.matcher.clone());
        rspace.restore_installs();

        // Mark the completion of spawn operation
        event!(Level::DEBUG, mark = "finished-spawn", "spawn");
        Ok(rspace)
    }

    /* RSpaceOps */

    fn store_waiting_continuation(
        &self,
        channels: Vec<C>,
        wc: WaitingContinuation<P, K>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let _ = self.get_store().put_continuation(&channels, wc);
        for channel in channels.iter() {
            self.get_store().put_join(channel, &channels);
        }
        None
    }

    // Takes the shared `Arc` payload — the datum stored is the same
    // allocation the speculative candidate borrowed (no copy).
    fn store_data(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        produce_ref: Produce,
    ) -> MaybeProduceResult<C, P, A, K> {
        self.get_store().put_datum(&channel, Datum {
            a: data,
            persist,
            source: produce_ref,
        });

        None
    }

    fn try_commit_data_match(
        &self,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<bool, RSpaceError> {
        let delta = AcceptedDatumChanges::from_candidates(data_candidates)
            .map_err(super::data_index::DataIndexError::from)?;
        let required_persistent_paths = data_candidates
            .iter()
            .filter_map(|candidate| {
                (candidate.datum.persist).then(|| match &candidate.address {
                    DatumAddress::Stored(path) => Some(Arc::clone(path)),
                    DatumAddress::Fresh => None,
                })?
            })
            .collect();
        let transition = AcceptedMatch {
            channels: Vec::new(),
            continuation: None,
            datum_delta: delta,
            required_persistent_paths,
            join_channels: Vec::new(),
        };
        match self.get_store().apply_accepted_match(transition)? {
            AcceptedMatchOutcome::Committed { continuation: None } => Ok(true),
            AcceptedMatchOutcome::Committed {
                continuation: Some(_),
            } => Err(RSpaceError::BugFoundError(
                "data-only accepted match returned a continuation".to_string(),
            )),
            AcceptedMatchOutcome::Stale => Ok(false),
        }
    }

    fn restore_installs(&self) {
        // Move out the install map to avoid cloning the whole structure on each
        // restore.
        let installs = {
            let mut installs_lock = self.installs.lock().unwrap();
            std::mem::take(&mut *installs_lock)
        };
        {
            let mut installs_lock = self.installs.lock().unwrap();
            installs_lock.reserve(installs.len());
        }

        for (channels, install) in installs {
            // ⚠ The arity refusal `locked_install_internal` grew below CANNOT
            // fire here: every entry in `installs` was put there BY that same
            // function, after its own check passed, so `channels.len() ==
            // patterns.len()` holds by construction on this path. Any `Err`
            // reaching this line is therefore a store fault, not a malformed
            // request, and `restore_installs` has no channel to carry it —
            // hence a named `expect` rather than a bare `unwrap`.
            self.locked_install_internal(channels, install.patterns, install.continuation, true)
                .expect(
                    "restore_installs: re-installing an entry this space itself recorded (its \
                     arity was already checked at install time)",
                );
        }
    }

    fn locked_install_internal(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        record_install: bool,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        // ★ Same decidable negative as `consume`, same refusal. `InstallParams`
        // is the second wire message that puts `channels` and `patterns` in two
        // independent repeated fields, so the fault is caller-expressible here
        // for exactly the same reason.
        //
        // ⚠ THIS SITE IS WHY THE FOUR MOVE TOGETHER. Its ReplaySpace twin,
        // `ReplayRSpace::locked_install_internal`, ALREADY returned this exact
        // `Err` with this exact message, so before this change `RSpace::install`
        // aborted the process where `ReplayRSpace::install` refused — a
        // play/replay asymmetry sitting in the tree, masked only because both of
        // this function's in-tree callers turn the `Err` back into a panic.
        // Converting play here makes the two agree.
        if channels.len() != patterns.len() {
            Err(RSpaceError::BugFoundError(
                "RUST ERROR: channels.length must equal patterns.length".to_string(),
            ))
        } else {
            let consume_ref = Consume::create(&channels, &patterns, &continuation, true);
            let mut channel_to_indexed_data = self.fetch_channel_to_index_data(&channels);
            // Borrow-zip: no per-install channel/pattern clones.
            let zipped: Vec<(&C, &P)> = channels.iter().zip(patterns.iter()).collect();
            let options: Option<Vec<ConsumeCandidate<C, A>>> = self
                .extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
                .into_iter()
                .collect();

            match options {
                None => {
                    if record_install {
                        self.installs
                            .lock()
                            .unwrap()
                            .insert(channels.clone(), Install {
                                patterns: patterns.clone(),
                                continuation: continuation.clone(),
                            });
                    }

                    self.get_store()
                        .install_continuation(&channels, WaitingContinuation {
                            patterns: Arc::new(patterns),
                            continuation: Arc::new(continuation),
                            persist: true,
                            peeks: BTreeSet::default(),
                            source: consume_ref,
                        });

                    for channel in channels.iter() {
                        self.get_store().install_join(channel, &channels);
                    }
                    Ok(None)
                }
                Some(_) => Err(RSpaceError::BugFoundError(
                    "RUST ERROR: Installing can be done only on startup".to_string(),
                )),
            }
        }
    }

    fn create_new_hot_store(
        &self,
        history_reader: Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>,
    ) {
        let next_hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        *self.store.write().expect("store write lock") = Arc::new(next_hot_store);
        self.retained_set_automata.clear();
    }

    // The public result stays value-shaped (`RSpaceResult<C, A>` /
    // `ContResult` unchanged) — this is the single per-fired-COMM
    // materialization boundary, cost-identical to the earlier value-shaped clones.
    // The multiplicative per-ATTEMPT copies died in the store/matcher hops; the
    // dispatch-side copy into the continuation env is stage-L2 territory
    // (the continuation-environment boundary is outside this transport change).
    fn wrap_result(
        &self,
        channels: &[C],
        wk: &WaitingContinuation<P, K>,
        _consume_ref: &Consume,
        data_candidates: &Vec<ConsumeCandidate<C, A>>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let cont_result = ContResult {
            continuation: (*wk.continuation).clone(),
            persistent: wk.persist,
            channels: channels.to_vec(),
            patterns: (*wk.patterns).clone(),
            peek: !wk.peeks.is_empty(),
        };

        let rspace_results = data_candidates
            .iter()
            .map(|data_candidate| RSpaceResult {
                channel: data_candidate.channel.clone(),
                matched_datum: (*data_candidate.datum.a).clone(),
                removed_datum: (*data_candidate.removed_datum).clone(),
                persistent: data_candidate.datum.persist,
            })
            .collect();

        Some((cont_result, rspace_results))
    }

    fn run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        ) -> (
            Option<ContinuationVersion>,
            Vec<(WaitingContinuation<P, K>, i32)>,
        ),
        fetch_matching_data: impl Fn(C) -> (C, CandidatePool<A>),
    ) -> MaybeProduceCandidate<C, P, A, K> {
        let mut remaining = grouped_channels;

        loop {
            match remaining.split_first() {
                Some((channels, rest)) => {
                    let t_cont = Instant::now();
                    let (continuation_version, match_candidates) =
                        fetch_matching_continuations(channels.to_vec());
                    metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t_cont.elapsed().as_nanos() as u64);
                    metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
                        .increment(match_candidates.len() as u64);

                    if match_candidates.is_empty() {
                        remaining = rest.to_vec();
                        continue;
                    }

                    let t_data = Instant::now();
                    let channel_to_indexed_data: HashMap<C, CandidatePool<A>> = channels
                        .iter()
                        .map(|c| fetch_matching_data(c.clone()))
                        .collect();
                    metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t_data.elapsed().as_nanos() as u64);

                    let t_match = Instant::now();
                    let first_match = self.extract_first_match(
                        &self.matcher,
                        channels.to_vec(),
                        continuation_version,
                        match_candidates,
                        channel_to_indexed_data,
                    );
                    metrics::counter!("rspace.matcher.extract_first_match_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t_match.elapsed().as_nanos() as u64);

                    match first_match {
                        Some(produce_candidate) => return Some(produce_candidate),
                        None => remaining = rest.to_vec(),
                    }
                }
                None => {
                    return None;
                }
            }
        }
    }

    /// A vector-backed candidate pool in THE canonical order — see
    /// [`crate::rspace::candidate_order`], which now owns it so the replay
    /// space can order its pools identically (the guard-aware selector's
    /// replay-equivalence argument depends on play and replay enumerating
    /// candidates in the same order).
    ///
    /// The name is historical: this never shuffled. A random shuffle would make
    /// equally valid matches diverge across validators, so the order is a hash
    /// of the serialized candidate, then the complete serialized identity to
    /// resolve a digest collision. Vector store index distinguishes only
    /// byte-identical repetitions and remains their removal address.
    /// Datum pools use `RSpaceDataIndex`'s `(hash, canonical datum identity
    /// bytes)` order instead.
    fn shuffle_with_index<D>(&self, t: Vec<D>) -> Vec<(D, i32)>
    where D: CandidateOrderingBytes {
        order_candidates_with_index(t)
    }
}
