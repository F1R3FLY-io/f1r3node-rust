// See rspace/src/main/scala/coop/rchain/rspace/RSpace.scala

// NOTE: Manual marks are used instead of trace_i()/with_marks() because
// the functions are not async-compatible with Span trait's closure pattern.
// This matches Scala's Span[F].traceI() and withMarks() semantics.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Instant;

pub static LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

use async_trait::async_trait;
use dashmap::DashMap;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;

use super::checkpoint::SoftCheckpoint;
use super::errors::{HistoryRepositoryError, RSpaceError};
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::history::history_reader::HistoryReader;
use super::history::instances::radix_history::RadixHistory;
use super::logging::BasicLogger;
use super::r#match::Match;
use super::metrics_constants::{
    CHANGES_SPAN, CONSUME_COMM_LABEL, HISTORY_CHECKPOINT_SPAN, LOCKED_CONSUME_SPAN,
    LOCKED_PRODUCE_SPAN, PRODUCE_COMM_LABEL, RESET_SPAN, REVERT_SOFT_CHECKPOINT_SPAN,
    RSPACE_METRICS_SOURCE,
};
use super::replay_rspace::ReplayRSpace;
use super::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceCandidate, MaybeProduceResult, RSpaceResult,
};
use super::trace::Log;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use crate::rspace::checkpoint::Checkpoint;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::hot_store::{HotStore, HotStoreInstances};
use crate::rspace::internal::*;
use crate::rspace::space_matcher::SpaceMatcher;

#[derive(Clone)]
pub struct RSpaceStore {
    pub history: Arc<dyn KeyValueStore>,
    pub roots: Arc<dyn KeyValueStore>,
    pub cold: Arc<dyn KeyValueStore>,
}

/// Ordering policy for matching candidate data/continuations in the COMM path.
///
/// `Random` (the default for all production runtimes) realizes the tuple space's
/// non-deterministic matching: when several candidates are eligible, one is chosen
/// by an entropy shuffle and the chosen index is recorded so replay can reproduce
/// it. `Deterministic` skips the shuffle (identity order), so a fresh PLAY is
/// bit-identical across nodes. It exists for finalized-state re-execution at the
/// seal, which has no single proposer to freeze the random choice and therefore
/// needs every node to derive the same state from the same finalized deploys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchOrder {
    Random,
    Deterministic,
}

#[repr(C)]
#[derive(Clone)]
pub struct RSpace<C, P, A, K> {
    pub history_repository:
        Arc<std::sync::RwLock<Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>>>,
    pub store: Arc<std::sync::RwLock<Arc<Box<dyn HotStore<C, P, A, K>>>>>,
    installs: Arc<std::sync::Mutex<HashMap<Vec<C>, Install<P, K>>>>,
    event_log: Arc<std::sync::Mutex<Log>>,
    produce_counter: Arc<std::sync::Mutex<BTreeMap<Produce, i32>>>,
    matcher: Arc<Box<dyn Match<P, A>>>,
    phase_a_locks: Arc<DashMap<u64, Arc<tokio::sync::Mutex<()>>>>,
    phase_b_locks: Arc<DashMap<u64, Arc<tokio::sync::Mutex<()>>>>,
    current_deploy_sig: Arc<std::sync::RwLock<Option<Vec<u8>>>>,
    this_exec_produces: Arc<std::sync::RwLock<HashSet<String>>>,
    match_order: MatchOrder,
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + std::hash::Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub fn get_store(&self) -> Arc<Box<dyn HotStore<C, P, A, K>>> {
        self.store.read().expect("store read lock").clone()
    }

    pub fn set_current_deploy_sig(&self, sig: Vec<u8>) {
        *self
            .current_deploy_sig
            .write()
            .expect("current_deploy_sig write lock") = Some(sig);
        self.this_exec_produces
            .write()
            .expect("this_exec_produces write lock")
            .clear();
    }

    pub fn current_deploy_sig_short(&self) -> String {
        match self
            .current_deploy_sig
            .read()
            .expect("current_deploy_sig read lock")
            .as_ref()
        {
            Some(sig) => {
                let h = hex::encode(sig);
                h[..16.min(h.len())].to_string()
            }
            None => "none".to_string(),
        }
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
    C: Clone + Debug + Default + Serialize + std::hash::Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
}

#[async_trait]
impl<C, P, A, K> ISpace<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + std::hash::Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "create-checkpoint").entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-create-checkpoint", "create_checkpoint");

        let hsid = Arc::as_ptr(&self.get_store()) as usize;
        tracing::debug!(
            target: "f1r3fly.rspace.checkpoint",
            path = "play",
            phase = "entry",
            thread = ?std::thread::current().id(),
            hsid = %format!("{:x}", hsid),
            "create_checkpoint entry",
        );

        let changes = {
            let _changes_span =
                tracing::info_span!(target: "f1r3fly.rspace", CHANGES_SPAN).entered();
            self.get_store().changes()
        };
        tracing::debug!(
            target: "f1r3fly.rspace.checkpoint",
            path = "play",
            phase = "changes",
            changes_count = changes.len(),
            "create_checkpoint hot-store changes",
        );

        // Create history checkpoint with span
        let next_history = {
            let _history_span =
                tracing::info_span!(target: "f1r3fly.rspace", HISTORY_CHECKPOINT_SPAN).entered();
            self.get_history_repository().checkpoint(changes)
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
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-create-checkpoint", "create_checkpoint");

        Ok(Checkpoint {
            root: self.get_history_repository().root(),
            log,
        })
    }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
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
        _channel: Vec<C>,
        _pattern: Vec<P>,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        panic!("\nERROR: RSpace consume_result should not be called here");
    }

    async fn get_data(&self, channel: &C) -> Vec<Datum<A>> { self.get_store().get_data(channel) }

    async fn get_waiting_continuations(&self, channels: Vec<C>) -> Vec<WaitingContinuation<P, K>> {
        self.get_store().get_continuations(&channels)
    }

    async fn get_joins(&self, channel: C) -> Vec<Vec<C>> { self.get_store().get_joins(&channel) }

    async fn clear(&self) -> Result<(), RSpaceError> {
        self.reset(&RadixHistory::empty_root_node_hash()).await
    }

    async fn get_root(&self) -> Blake2b256Hash { self.get_history_repository().root() }

    async fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> { self.get_store().to_map() }

    async fn create_soft_checkpoint(&self) -> SoftCheckpoint<C, P, A, K> {
        let cache_snapshot = self.get_store().snapshot();
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let curr_produce_counter =
            std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));

        SoftCheckpoint {
            cache_snapshot,
            log: curr_event_log,
            produce_counter: curr_produce_counter,
        }
    }

    async fn take_event_log(&self) -> Log {
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let _ = std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));
        curr_event_log
    }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError> {
        let _span =
            tracing::info_span!(target: "f1r3fly.rspace", REVERT_SOFT_CHECKPOINT_SPAN).entered();
        let history = self.get_history_repository();
        let history_reader = history.get_history_reader(&history.root())?;
        let hot_store = HotStoreInstances::create_from_hs_and_hr(
            checkpoint.cache_snapshot,
            history_reader.base(),
        );

        *self.store.write().expect("store write lock") = Arc::new(hot_store);
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
        if channels.is_empty() {
            panic!("RUST ERROR: channels can't be empty");
        } else if channels.len() != patterns.len() {
            panic!("RUST ERROR: channels.length must equal patterns.length");
        } else {
            let consume_ref = Consume::create(&channels, &patterns, &continuation, persist);

            let lock_start = Instant::now();
            let channel_hashes: Vec<u64> =
                channels.iter().map(|ch| Self::channel_hash(ch)).collect();
            let _lock_guard = self.consume_lock(&channel_hashes).await;
            let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::SeqCst);
            tracing::trace!(target: "f1r3fly.rspace.lock_order", seq = seq, op = "consume", hashes = ?channel_hashes, "lock acquired");
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
        let produce_ref = Produce::create(&channel, &data, persist);

        let lock_start = Instant::now();
        let _lock_guard = self.produce_lock(&channel).await;
        let seq = LOCK_SEQUENCE.fetch_add(1, AtomicOrdering::SeqCst);
        tracing::trace!(target: "f1r3fly.rspace.lock_order", seq = seq, op = "produce", hash = Self::channel_hash(&channel), "lock acquired");
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
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    /**
     * Creates [[RSpace]] from [[HistoryRepository]] and [[HotStore]].
     */
    pub fn apply(
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Box<dyn HotStore<C, P, A, K>>,
        matcher: Arc<Box<dyn Match<P, A>>>,
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
            current_deploy_sig: Arc::new(std::sync::RwLock::new(None)),
            this_exec_produces: Arc::new(std::sync::RwLock::new(HashSet::new())),
            match_order: MatchOrder::Random,
        }
    }

    pub fn create(
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A>>>,
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
            + for<'a> Deserialize<'a>
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    {
        let setup = Self::create_history_repo(store).unwrap();
        let (history_reader, store) = setup;
        let space = Self::apply(Arc::new(history_reader), store, matcher);
        Ok(space)
    }

    pub fn create_with_replay(
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A>>>,
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
            + for<'a> Deserialize<'a>
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    {
        let setup = Self::create_history_repo(store).unwrap();
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
            + for<'a> Deserialize<'a>
            + Eq
            + Hash
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    {
        let history_repo =
            HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)?;

        let history_reader = history_repo.get_history_reader(&history_repo.root())?;

        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());

        Ok((history_repo, hot_store))
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
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-consume", "locked_consume");

        let t0 = Instant::now();
        self.log_consume(consume_ref, channels, patterns, continuation, persist, peeks);
        metrics::counter!("rspace.consume.log_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let t1 = Instant::now();
        let mut channel_to_indexed_data = self.fetch_channel_to_index_data(channels);
        metrics::counter!("rspace.consume.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        let t2 = Instant::now();
        let zipped: Vec<(C, P)> = channels
            .iter()
            .cloned()
            .zip(patterns.iter().cloned())
            .collect();
        let options: Option<Vec<ConsumeCandidate<C, A>>> = self
            .extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
            .into_iter()
            .collect();
        metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t2.elapsed().as_nanos() as u64);

        let wk = WaitingContinuation {
            patterns: patterns.to_vec(),
            continuation: continuation.clone(),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        match options {
            Some(data_candidates) => {
                let t3 = Instant::now();
                let produce_counters_closure =
                    |produces: &[Produce]| self.produce_counters(produces);

                self.log_comm(
                    channels,
                    &wk,
                    COMM::new(
                        &data_candidates,
                        consume_ref.clone(),
                        peeks.clone(),
                        produce_counters_closure,
                    ),
                    "comm.consume",
                );
                self.store_persistent_data(&data_candidates, peeks);
                metrics::counter!("rspace.consume.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                Ok(self.wrap_result(channels, &wk, consume_ref, &data_candidates))
            }
            None => {
                let t3 = Instant::now();
                self.store_waiting_continuation(channels.to_vec(), wk);
                metrics::counter!("rspace.consume.store_continuation_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                Ok(None)
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
    fn fetch_channel_to_index_data(&self, channels: &[C]) -> HashMap<C, Vec<(Datum<A>, i32)>> {
        let mut map = HashMap::with_capacity(channels.len());
        for c in channels {
            let data = self.get_store().get_data(c);
            let shuffled_data = self.shuffle_with_index(data);
            map.insert(c.clone(), shuffled_data);
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
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-produce", "locked_produce");

        let t0 = Instant::now();
        let grouped_channels = self.get_store().get_joins(&channel);
        metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        self.log_produce(produce_ref, &channel, &data, persist);

        let t1 = Instant::now();
        let extracted = self.extract_produce_candidate(grouped_channels, channel.clone(), Datum {
            a: data.clone(),
            persist,
            source: produce_ref.clone(),
        });
        metrics::counter!("rspace.produce.extract_candidate_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        match extracted {
            Some(produce_candidate) => {
                let t2 = Instant::now();
                let result =
                    Ok(self
                        .process_match_found(produce_candidate)
                        .map(|consume_result| {
                            (consume_result.0, consume_result.1, produce_ref.clone())
                        }));
                metrics::counter!("rspace.produce.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t2.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-produce", "locked_produce");
                result
            }
            None => {
                let t2 = Instant::now();
                let result = Ok(self.store_data(channel, data, persist, produce_ref.clone()));
                metrics::counter!("rspace.produce.store_data_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t2.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-produce", "locked_produce");
                result
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
        let fetch_matching_continuations =
            |channels: Vec<C>| -> Vec<(WaitingContinuation<P, K>, i32)> {
                let continuations = self.get_store().get_continuations(&channels);
                self.shuffle_with_index(continuations)
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
        let fetch_matching_data = |channel| -> (C, Vec<(Datum<A>, i32)>) {
            let data_vec = self.get_store().get_data(&channel);
            let mut shuffled_data = self.shuffle_with_index(data_vec);
            if channel == bat_channel {
                shuffled_data.insert(0, (data.clone(), -1));
            }
            (channel, shuffled_data)
        };

        self.run_matcher_for_channels(
            grouped_channels,
            fetch_matching_continuations,
            fetch_matching_data,
        )
    }

    fn process_match_found(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let ProduceCandidate {
            channels,
            continuation,
            continuation_index,
            data_candidates,
        } = pc;

        let WaitingContinuation {
            patterns: _patterns,
            continuation: _cont,
            persist,
            peeks,
            source: consume_ref,
        } = &continuation;

        let produce_counters_closure = |produces: &[Produce]| self.produce_counters(produces);
        self.log_comm(
            &channels,
            &continuation,
            COMM::new(
                &data_candidates,
                consume_ref.clone(),
                peeks.clone(),
                produce_counters_closure,
            ),
            "comm.produce",
        );

        if !persist {
            self.get_store()
                .remove_continuation(&channels, continuation_index);
        }

        self.remove_matched_datum_and_join(&channels, &data_candidates);

        self.wrap_result(&channels, &continuation, consume_ref, &data_candidates)
    }

    fn log_comm(&self, _channels: &[C], _wk: &WaitingContinuation<P, K>, comm: COMM, label: &str) {
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
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-spawn", "spawn");

        let history_repo = self.get_history_repository();
        let next_history = history_repo.reset(&history_repo.root())?;
        let history_reader = next_history.get_history_reader(&next_history.root())?;
        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        let rspace = RSpace::apply(Arc::new(next_history), hot_store, self.matcher.clone());
        rspace.restore_installs();

        // Mark the completion of spawn operation
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-spawn", "spawn");
        Ok(rspace)
    }

    /// Like [`spawn`](Self::spawn) but the spawned space matches candidates in a
    /// deterministic (identity) order instead of an entropy shuffle. Used for
    /// finalized-state re-execution at the seal, where every node must derive the
    /// same state from the same finalized deploys with no proposer to freeze a
    /// random match choice.
    pub fn spawn_deterministic(&self) -> Result<Self, RSpaceError> {
        let mut rspace = self.spawn()?;
        rspace.match_order = MatchOrder::Deterministic;
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

    fn store_data(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: Produce,
    ) -> MaybeProduceResult<C, P, A, K> {
        let hsid = Arc::as_ptr(&self.get_store()) as usize;
        let existing = self.get_store().get_data(&channel);
        let pre_count = existing.len();
        tracing::trace!(
            target: "f1r3fly.rspace.hotstore",
            op = "produce",
            path = "play",
            thread = ?std::thread::current().id(),
            hsid = %format!("{:x}", hsid),
            deploy_sig = %self.current_deploy_sig_short(),
            channel_hash = %hex::encode(produce_ref.channel_hash.bytes()),
            produce_hash = %hex::encode(produce_ref.hash.bytes()),
            persistent = persist,
            pre_count,
            "store_data invoked from deploy execution",
        );
        if pre_count >= 1 {
            let existing_sources: Vec<String> = existing
                .iter()
                .map(|d| hex::encode(d.source.hash.bytes()))
                .collect();
            let new_hash = hex::encode(produce_ref.hash.bytes());
            let this_exec = self
                .this_exec_produces
                .read()
                .expect("this_exec_produces read lock");
            let existing_provenance: Vec<&str> = existing_sources
                .iter()
                .map(|h| {
                    if this_exec.contains(h) {
                        "this-exec"
                    } else {
                        "inherited-from-pre-state"
                    }
                })
                .collect();
            tracing::debug!(
                target: "f1r3fly.rspace.multidatum",
                path = "play",
                thread = ?std::thread::current().id(),
                hsid = %format!("{:x}", hsid),
                deploy_sig = %self.current_deploy_sig_short(),
                channel_hash = %hex::encode(produce_ref.channel_hash.bytes()),
                pre_count,
                post_count = pre_count + 1,
                new_produce_hash = %new_hash,
                existing_produce_hashes = ?existing_sources,
                existing_provenance = ?existing_provenance,
                channel_par = ?channel,
                value = ?data,
                "multi-datum transition on channel",
            );
        }
        self.this_exec_produces
            .write()
            .expect("this_exec_produces write lock")
            .insert(hex::encode(produce_ref.hash.bytes()));
        self.get_store().put_datum(&channel, Datum {
            a: data,
            persist,
            source: produce_ref,
        });

        None
    }

    fn store_persistent_data(
        &self,
        data_candidates: &Vec<ConsumeCandidate<C, A>>,
        _peeks: &BTreeSet<i32>,
    ) -> Option<Vec<()>> {
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by(|a, b| b.datum_index.cmp(&a.datum_index));
        let results: Vec<_> = sorted_candidates
            .into_iter()
            .rev()
            .map(|consume_candidate| {
                let ConsumeCandidate {
                    channel,
                    datum: Datum {
                        persist, source, ..
                    },
                    removed_datum: _,
                    datum_index,
                } = consume_candidate;

                if !persist {
                    tracing::trace!(
                        target: "f1r3fly.rspace.hotstore",
                        op = "consume",
                        path = "play",
                        site = "store_persistent_data",
                        deploy_sig = %self.current_deploy_sig_short(),
                        channel_hash = %hex::encode(source.channel_hash.bytes()),
                        produce_hash = %hex::encode(source.hash.bytes()),
                        datum_index = *datum_index,
                        persistent = *persist,
                        "store_persistent_data removed datum",
                    );
                    self.get_store().remove_datum(channel, *datum_index).ok()
                } else {
                    tracing::trace!(
                        target: "f1r3fly.rspace.hotstore",
                        op = "consume",
                        path = "play",
                        site = "store_persistent_data",
                        skip = "persistent",
                        deploy_sig = %self.current_deploy_sig_short(),
                        channel_hash = %hex::encode(source.channel_hash.bytes()),
                        produce_hash = %hex::encode(source.hash.bytes()),
                        datum_index = *datum_index,
                        "persistent datum not removed",
                    );
                    Some(())
                }
            })
            .collect();

        if results.iter().any(|res| res.is_none()) {
            None
        } else {
            Some(results.into_iter().flatten().collect())
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
            self.locked_install_internal(channels, install.patterns, install.continuation, true)
                .unwrap();
        }
    }

    fn locked_install_internal(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        record_install: bool,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        if channels.len() != patterns.len() {
            panic!("RUST ERROR: channels.length must equal patterns.length");
        } else {
            let consume_ref = Consume::create(&channels, &patterns, &continuation, true);
            let mut channel_to_indexed_data = self.fetch_channel_to_index_data(&channels);
            let zipped: Vec<(C, P)> = channels
                .iter()
                .cloned()
                .zip(patterns.iter().cloned())
                .collect();
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
                            patterns,
                            continuation,
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
    }

    fn wrap_result(
        &self,
        channels: &[C],
        wk: &WaitingContinuation<P, K>,
        _consume_ref: &Consume,
        data_candidates: &Vec<ConsumeCandidate<C, A>>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let cont_result = ContResult {
            continuation: wk.continuation.clone(),
            persistent: wk.persist,
            channels: channels.to_vec(),
            patterns: wk.patterns.clone(),
            peek: !wk.peeks.is_empty(),
        };

        let rspace_results = data_candidates
            .iter()
            .map(|data_candidate| RSpaceResult {
                channel: data_candidate.channel.clone(),
                matched_datum: data_candidate.datum.a.clone(),
                removed_datum: data_candidate.removed_datum.clone(),
                persistent: data_candidate.datum.persist,
            })
            .collect();

        Some((cont_result, rspace_results))
    }

    fn remove_matched_datum_and_join(
        &self,
        channels: &[C],
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Option<Vec<()>> {
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by(|a, b| b.datum_index.cmp(&a.datum_index));
        let results: Vec<_> = sorted_candidates
            .into_iter()
            .rev()
            .map(|consume_candidate| {
                let ConsumeCandidate {
                    channel,
                    datum: Datum {
                        persist, source, ..
                    },
                    removed_datum: _,
                    datum_index,
                } = consume_candidate;

                tracing::trace!(
                    target: "f1r3fly.rspace.hotstore",
                    op = "consume",
                    path = "play",
                    site = "remove_matched_datum_and_join",
                    deploy_sig = %self.current_deploy_sig_short(),
                    channel_hash = %hex::encode(source.channel_hash.bytes()),
                    produce_hash = %hex::encode(source.hash.bytes()),
                    datum_index = *datum_index,
                    persistent = *persist,
                    "remove_datum on matched join",
                );
                if *datum_index >= 0 &&
                    !persist &&
                    self.get_store()
                        .remove_datum(channel, *datum_index)
                        .is_err()
                {
                    return None;
                }
                self.get_store().remove_join(channel, channels);

                Some(())
            })
            .collect();

        if results.iter().any(|res| res.is_none()) {
            None
        } else {
            Some(results.into_iter().flatten().collect())
        }
    }

    fn run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(Vec<C>) -> Vec<(WaitingContinuation<P, K>, i32)>,
        fetch_matching_data: impl Fn(C) -> (C, Vec<(Datum<A>, i32)>),
    ) -> MaybeProduceCandidate<C, P, A, K> {
        let mut remaining = grouped_channels;

        loop {
            match remaining.split_first() {
                Some((channels, rest)) => {
                    let t_cont = Instant::now();
                    let match_candidates = fetch_matching_continuations(channels.to_vec());
                    metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t_cont.elapsed().as_nanos() as u64);
                    metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
                        .increment(match_candidates.len() as u64);

                    let t_data = Instant::now();
                    let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
                        .iter()
                        .map(|c| fetch_matching_data(c.clone()))
                        .collect();
                    metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
                        .increment(t_data.elapsed().as_nanos() as u64);

                    let t_match = Instant::now();
                    let first_match = self.extract_first_match(
                        &self.matcher,
                        channels.to_vec(),
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

    fn shuffle_with_index<D>(&self, t: Vec<D>) -> Vec<(D, i32)> {
        let mut indexed_vec = t
            .into_iter()
            .enumerate()
            .map(|(i, d)| (d, i as i32))
            .collect::<Vec<_>>();
        // Deterministic mode (finalized-state seal): keep identity order so a fresh
        // PLAY is bit-identical across nodes. Random mode (production): shuffle the
        // match order, recording the chosen index for replay.
        if self.match_order == MatchOrder::Random {
            let mut rng = rand::rng();
            indexed_vec.shuffle(&mut rng);
        }
        indexed_vec
    }
}
