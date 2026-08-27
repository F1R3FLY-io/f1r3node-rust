// See rspace/src/main/scala/coop/rchain/rspace/RSpace.scala

// NOTE: Manual marks are used instead of trace_i()/with_marks() because
// the functions are not async-compatible with Span trait's closure pattern.
// This matches Scala's Span[F].traceI() and withMarks() semantics.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::Instant;

pub static LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

use async_trait::async_trait;
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
use super::striped_locks::{self, ChannelLockGuard};
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

#[repr(C)]
#[derive(Clone)]
pub struct RSpace<C, P, A, K> {
    pub history_repository:
        Arc<std::sync::RwLock<Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>>>,
    // get_store() is called multiple times per produce()/consume() (produce_lock,
    // locked_produce, store_data, extract_produce_candidate) and never
    // contends a writer on that path — writes only happen at checkpoint/spawn
    // boundaries, replacing the whole pointer wholesale. std::sync::RwLock
    // still does atomic RMW on shared reader-count state per read-lock/unlock,
    // which scales badly under many concurrent readers on some platforms
    // (confirmed: isolated read-lock-only benchmark measured *negative*
    // scaling, concurrent slower than sequential — see issue #50 follow-up).
    // ArcSwap is lock-free for this "read-mostly, rare wholesale swap"
    // pattern: store() below is a single atomic pointer write, no
    // reader-count bookkeeping and no lock-release RMW to pay on the write
    // side either. get_store() uses load_full() (returns an owned Arc, not a
    // short-lived Guard, since callers keep the result across further work),
    // which still does one atomic refcount increment — cheaper than RwLock's
    // read-lock-plus-unlock pair, but not literally free.
    //
    // history_repository above has the identical RwLock<Arc<...>> shape but
    // is deliberately left alone: its only reader is spawn() (rare, not on
    // the produce/consume hot path), so it doesn't have this problem.
    pub store: Arc<arc_swap::ArcSwap<Box<dyn HotStore<C, P, A, K>>>>,
    installs: Arc<std::sync::Mutex<HashMap<Vec<C>, Install<P, K>>>>,
    event_log: Arc<std::sync::Mutex<Log>>,
    // Striped like phase_a/phase_b_locks below: NUM_LOCK_STRIPES independent
    // shards keyed by channel_hash(produce) % NUM_LOCK_STRIPES, instead of
    // one global std::sync::Mutex<BTreeMap>. The hot path (log_produce/
    // produce_counters) only ever contends the one shard its key hashes to.
    // Checkpoint boundaries (take/reset/restore_produce_counter below) lock
    // every shard for the whole operation, in the same fixed index order
    // every time, restoring the single global lock's point-in-time
    // linearization for this field specifically — not across it and
    // event_log, which remain two separate locks taken sequentially, same
    // as before this change (pre-existing, out of scope here).
    produce_counter: Arc<Vec<std::sync::Mutex<BTreeMap<Produce, i32>>>>,
    matcher: Arc<Box<dyn Match<P, A, K>>>,
    // Fixed-size striped locks replace the growing DashMap<u64, Mutex>.
    // See striped_locks.rs for the stripe count and hashing/lock scheme.
    // No DashMap entry() → no parking_lot shard contention per produce/consume.
    phase_a_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
    phase_b_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + std::hash::Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub fn get_store(&self) -> Arc<Box<dyn HotStore<C, P, A, K>>> { self.store.load_full() }

    async fn consume_lock(&self, channel_hashes: &[u64]) -> (ChannelLockGuard, ChannelLockGuard) {
        striped_locks::consume_lock(&self.phase_a_locks, &self.phase_b_locks, channel_hashes).await
    }

    // Split into three timed sub-phases (rather than the one combined
    // rspace.produce.lock_acquire_ns wrapping the whole function, measured by
    // the produce() caller) because phase_a wait, get_joins() work, and
    // phase_b wait are different costs that need to be told apart: the first
    // two are contention on the striped mutexes, the middle one is a
    // HotStore/history-repository read. See issue #50 follow-up.
    async fn produce_lock(&self, channel: &C) -> (ChannelLockGuard, ChannelLockGuard) {
        let phase_a_start = Instant::now();
        let channel_hash = striped_locks::channel_hash(channel);
        let phase_a = striped_locks::acquire_locks(&self.phase_a_locks, &[channel_hash]).await;
        metrics::counter!("rspace.produce.phase_a_wait_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(phase_a_start.elapsed().as_nanos() as u64);

        let joins_start = Instant::now();
        let store = self.get_store();
        let join_hashes: Vec<u64> = store
            .get_joins(channel)
            .into_iter()
            .flatten()
            .map(|ch| striped_locks::channel_hash(&ch))
            .collect();
        metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(joins_start.elapsed().as_nanos() as u64);

        let phase_b_start = Instant::now();
        let phase_b = striped_locks::acquire_locks(&self.phase_b_locks, &join_hashes).await;
        metrics::counter!("rspace.produce.phase_b_wait_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(phase_b_start.elapsed().as_nanos() as u64);

        (phase_a, phase_b)
    }

    fn new_striped_locks() -> Arc<Vec<Arc<tokio::sync::Mutex<()>>>> {
        striped_locks::new_striped_locks()
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
        // Span[F].withMarks("create-checkpoint") from Scala - works because this is NOT
        // async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "create-checkpoint").entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-create-checkpoint", "create_checkpoint");

        // Get changes with span
        let changes = {
            let _changes_span =
                tracing::info_span!(target: "f1r3fly.rspace", CHANGES_SPAN).entered();
            self.get_store().changes()
        };

        // Create history checkpoint with span
        let next_history = {
            let _history_span =
                tracing::info_span!(target: "f1r3fly.rspace", HISTORY_CHECKPOINT_SPAN).entered();
            self.get_history_repository().checkpoint(changes)
        };
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        let log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        self.reset_produce_counter();

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
        self.reset_produce_counter();

        // Striped locks are fixed-size and stateless (Mutex<()>); nothing to
        // clear on reset — they are reused across checkpoints.

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
        let curr_produce_counter = self.take_produce_counter();

        SoftCheckpoint {
            cache_snapshot,
            log: curr_event_log,
            produce_counter: curr_produce_counter,
        }
    }

    async fn take_event_log(&self) -> Log {
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        self.reset_produce_counter();
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

        // Invariant this relies on (unchanged from before the RwLock->ArcSwap
        // switch, now just more visible since there's no lock to obscure it):
        // no produce/consume can be concurrently in flight while store/
        // event_log/produce_counter are swapped here — callers already only
        // clone the store Arc and drop any lock immediately, so a reader that
        // started before this swap keeps running against the old store either
        // way. If that ever stops holding, this three-field update needs its
        // own coordination, not just three independent atomic swaps.
        self.store.store(Arc::new(hot_store));
        *self.event_log.lock().expect("event log lock") = checkpoint.log;
        self.restore_produce_counter(checkpoint.produce_counter);

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
            let channel_hashes: Vec<u64> = channels
                .iter()
                .map(|ch| striped_locks::channel_hash(ch))
                .collect();
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
        tracing::trace!(target: "f1r3fly.rspace.lock_order", seq = seq, op = "produce", hash = striped_locks::channel_hash(&channel), "lock acquired");
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
            store: Arc::new(arc_swap::ArcSwap::new(Arc::new(store))),
            matcher,
            installs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_log: Arc::new(std::sync::Mutex::new(Vec::new())),
            produce_counter: Arc::new(
                (0..striped_locks::NUM_LOCK_STRIPES)
                    .map(|_| std::sync::Mutex::new(BTreeMap::new()))
                    .collect(),
            ),
            phase_a_locks: Self::new_striped_locks(),
            phase_b_locks: Self::new_striped_locks(),
        }
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

    // Single-shard lookup for the hot path (log_produce): a given key always
    // hashes to the same shard, so per-key correctness holds regardless of
    // what any other shard is doing concurrently.
    fn produce_counter_shard(
        &self,
        produce_ref: &Produce,
    ) -> &std::sync::Mutex<BTreeMap<Produce, i32>> {
        let idx = (striped_locks::channel_hash(produce_ref) as usize) % self.produce_counter.len();
        &self.produce_counter[idx]
    }

    // Groups by shard first so a slice with repeats or same-shard keys locks
    // each needed shard only once, instead of once per produce_ref.
    fn produce_counters(&self, produce_refs: &[Produce]) -> BTreeMap<Produce, i32> {
        let mut by_shard: HashMap<usize, Vec<&Produce>> = HashMap::new();
        for p in produce_refs {
            let idx = (striped_locks::channel_hash(p) as usize) % self.produce_counter.len();
            by_shard.entry(idx).or_default().push(p);
        }

        let mut result = BTreeMap::new();
        for (idx, refs) in by_shard {
            let guard = self.produce_counter[idx]
                .lock()
                .expect("produce counter shard lock");
            for p in refs {
                result.insert(p.clone(), guard.get(p).copied().unwrap_or(0));
            }
        }
        result
    }

    // Locks every shard, in the same fixed index order every helper below
    // uses, before touching any of them, and holds all guards for the whole
    // operation. This is the equivalent of the former single global lock's
    // linearization point, restricted to this field: any produce() racing
    // take/reset/restore blocks on whichever shard it hashes to until the
    // full boundary operation finishes on every shard, so the result is a
    // consistent point-in-time snapshot across shards, not just per-key.
    //
    // (This does not make produce_counter atomic *with* event_log — those
    // remain two separate locks taken sequentially, same as before this
    // change; that cross-field ordering gap is pre-existing, not introduced
    // here, and is out of scope until event_log itself is addressed.)
    fn all_produce_counter_shards(&self) -> Vec<std::sync::MutexGuard<'_, BTreeMap<Produce, i32>>> {
        self.produce_counter
            .iter()
            .map(|shard| shard.lock().expect("produce counter shard lock"))
            .collect()
    }

    // Drains all shards into one combined map and leaves every shard empty,
    // atomically across the whole field (see all_produce_counter_shards).
    fn take_produce_counter(&self) -> BTreeMap<Produce, i32> {
        let mut guards = self.all_produce_counter_shards();
        let mut combined = BTreeMap::new();
        for guard in guards.iter_mut() {
            combined.extend(std::mem::take(&mut **guard));
        }
        combined
    }

    // Empties every shard atomically across the whole field.
    fn reset_produce_counter(&self) {
        let mut guards = self.all_produce_counter_shards();
        for guard in guards.iter_mut() {
            **guard = BTreeMap::new();
        }
    }

    // Partitions the incoming map by shard before taking any locks, then
    // installs each partition with one assignment per shard under all
    // guards held together — avoids the reset-then-insert window a
    // concurrent produce() could otherwise land in between.
    fn restore_produce_counter(&self, map: BTreeMap<Produce, i32>) {
        let mut partitioned: Vec<BTreeMap<Produce, i32>> = (0..self.produce_counter.len())
            .map(|_| BTreeMap::new())
            .collect();
        for (k, v) in map {
            let idx = (striped_locks::channel_hash(&k) as usize) % self.produce_counter.len();
            partitioned[idx].insert(k, v);
        }

        let mut guards = self.all_produce_counter_shards();
        for (guard, part) in guards.iter_mut().zip(partitioned) {
            **guard = part;
        }
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

        let commit_ok = match &options {
            // Cross-channel commit hook: a `where` guard on the consume
            // can veto even after every spatial bind matched. On false
            // we install the wk and leave the data alone, just as if
            // the spatial match itself had failed (plan §7.12).
            Some(data_candidates) => {
                let matched: Vec<A> = data_candidates.iter().map(|c| c.datum.a.clone()).collect();
                self.matcher.check_commit(continuation, &matched)
            }
            None => false,
        };

        match options {
            Some(data_candidates) if commit_ok => {
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
            _ => {
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
            |channels: Vec<C>| -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)> {
                // Arc-shared fetch: probing continuations no longer deep-clones
                // the continuation body on every produce.
                let continuations = self.get_store().get_continuations_arc(&channels);
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
            .push(Event::Comm(comm));
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
            .push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
    }

    fn log_produce(&self, produce_ref: &Produce, _channel: &C, _data: &A, persist: bool) {
        self.event_log
            .lock()
            .expect("event log lock")
            .push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
        if !persist {
            let mut counter = self
                .produce_counter_shard(produce_ref)
                .lock()
                .expect("produce counter shard lock");
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
                    datum: Datum { persist, .. },
                    removed_datum: _,
                    datum_index,
                } = consume_candidate;

                if !persist {
                    self.get_store().remove_datum(channel, *datum_index).ok()
                } else {
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
        // Same no-concurrent-produce/consume invariant as
        // revert_to_soft_checkpoint's store.store() call above.
        self.store.store(Arc::new(next_hot_store));
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
                    datum: Datum { persist, .. },
                    removed_datum: _,
                    datum_index,
                } = consume_candidate;

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
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        )
            -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
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
        let mut rng = rand::rng();
        let mut indexed_vec = t
            .into_iter()
            .enumerate()
            .map(|(i, d)| (d, i as i32))
            .collect::<Vec<_>>();
        indexed_vec.shuffle(&mut rng);
        indexed_vec
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::rspace::r#match::Match;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    // ── minimal types ─────────────────────────────────────────────────────────

    #[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
    struct Wildcard;

    #[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
    struct Cont;

    struct AlwaysMatch;

    impl Match<Wildcard, String, Cont> for AlwaysMatch {
        fn get(&self, _: &Wildcard, a: &String) -> Option<String> { Some(a.clone()) }
    }

    async fn make_rspace() -> RSpace<String, Wildcard, String, Cont> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        RSpace::create(store, Arc::new(Box::new(AlwaysMatch))).unwrap()
    }

    // Measures contention on the event_log mutex while N concurrent tasks call
    // produce() on separate channels. The log is pre-filled to PRE_FILL entries
    // before the concurrent phase so every insert starts with a large existing
    // log, making the mutex hold time long enough to observe.
    //
    // Observer runs on a dedicated OS thread (not a tokio task) because
    // std::sync::Mutex::lock() blocks the worker thread without yielding, so a
    // tokio observer would never be scheduled while producers hold the mutex.
    //
    // Passes when event_log uses O(1) append: hold time drops to ~10 ns and
    // the observer almost never catches the mutex held.

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn event_log_mutex_does_not_contend_under_concurrent_produces() {
        const TASKS: usize = 4;
        const OPS_PER_TASK: usize = 200;
        // Simulates mid-deploy state: the event_log grows to PRE_FILL entries
        // before the concurrent test starts. Each subsequent Vec::insert(0,..)
        // must shift all existing entries — O(PRE_FILL × sizeof(Event)) bytes.
        // At PRE_FILL=8000 and sizeof(Event)≈150 bytes on M1:
        //   shift ≈ 1.2 MB / 50 GB/s ≈ 24 μs per insert → detectable.
        const PRE_FILL: usize = 8_000;
        // Fraction of observer probes that find the mutex already held.
        const MAX_CONTENTION_RATE: f64 = 0.20;

        let rspace = make_rspace().await;

        // Observer on a dedicated OS thread: probes try_lock() in a spin loop.
        // Must be an OS thread, not a tokio task — std::sync::Mutex::lock()
        // blocks the worker thread without yielding, so a tokio observer would
        // never run while any producer holds the mutex.
        let event_log = rspace.event_log.clone();
        let running = Arc::new(AtomicBool::new(true));
        let total_probes = Arc::new(AtomicU64::new(0));
        let contended_probes = Arc::new(AtomicU64::new(0));

        {
            let event_log = event_log.clone();
            let running = running.clone();
            let total = total_probes.clone();
            let contended = contended_probes.clone();
            std::thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    total.fetch_add(1, Ordering::Relaxed);
                    if event_log.try_lock().is_err() {
                        contended.fetch_add(1, Ordering::Relaxed);
                    }
                    std::hint::spin_loop();
                }
            });
        }

        // Pre-fill: grow the log to PRE_FILL entries before the concurrent phase.
        // Counters are reset after so we measure only concurrent contention.
        for i in 0..PRE_FILL {
            rspace
                .produce(format!("prefill_{}", i), "datum".to_string(), false)
                .await
                .unwrap();
        }

        total_probes.store(0, Ordering::Relaxed);
        contended_probes.store(0, Ordering::Relaxed);

        // N concurrent producers, each on its own channel set.
        let handles: Vec<_> = (0..TASKS)
            .map(|i| {
                let s = rspace.clone();
                tokio::spawn(async move {
                    for j in 0..OPS_PER_TASK {
                        s.produce(format!("ch_{}_{}", i, j), "datum".to_string(), false)
                            .await
                            .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
        running.store(false, Ordering::Relaxed);

        let total = total_probes.load(Ordering::Relaxed);
        let contended = contended_probes.load(Ordering::Relaxed);
        let rate = if total > 0 {
            contended as f64 / total as f64
        } else {
            0.0
        };

        eprintln!(
            "event_log_contention: probes={total}  contended={contended}  rate={:.1}%  (threshold \
             <{:.0}%)",
            rate * 100.0,
            MAX_CONTENTION_RATE * 100.0,
        );

        assert!(
            rate < MAX_CONTENTION_RATE,
            "event_log mutex contention too high: {:.1}% of probes found the mutex held \
             (threshold {:.0}%). Root cause: all {} concurrent tasks share one \
             std::sync::Mutex<event_log> — every produce() call acquires it, blocking other \
             worker threads. Fix: per-task event logs merged at checkpoint, or a lock-free append \
             structure.",
            rate * 100.0,
            MAX_CONTENTION_RATE * 100.0,
            TASKS,
        );
    }

    // Mirrors the rholang-par benchmark: PAR_BRANCHES concurrent tokio tasks
    // each call produce() OPS_PER_BRANCH times on their own private channels,
    // all sharing one RSpace and therefore one event_log.
    //
    // The log grows naturally from zero to PAR_BRANCHES * OPS_PER_BRANCH entries.
    // Passes when event_log uses O(1) append.

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn par_branch_event_log_does_not_contend_at_rholang_par_scale() {
        const PAR_BRANCHES: usize = 32;
        const OPS_PER_BRANCH: usize = 500;
        const MAX_CONTENTION_RATE: f64 = 0.20;

        let rspace = make_rspace().await;

        let event_log = rspace.event_log.clone();
        let running = Arc::new(AtomicBool::new(true));
        let total_probes = Arc::new(AtomicU64::new(0));
        let contended_probes = Arc::new(AtomicU64::new(0));

        {
            let event_log = event_log.clone();
            let running = running.clone();
            let total = total_probes.clone();
            let contended = contended_probes.clone();
            std::thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    total.fetch_add(1, Ordering::Relaxed);
                    if event_log.try_lock().is_err() {
                        contended.fetch_add(1, Ordering::Relaxed);
                    }
                    std::hint::spin_loop();
                }
            });
        }

        // 32 par-branches, each producing on its own unique channels.
        // No matching happens, so contention comes purely from log growth.
        let handles: Vec<_> = (0..PAR_BRANCHES)
            .map(|i| {
                let s = rspace.clone();
                tokio::spawn(async move {
                    for j in 0..OPS_PER_BRANCH {
                        s.produce(format!("branch_{}_{}", i, j), "datum".to_string(), false)
                            .await
                            .unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
        running.store(false, Ordering::Relaxed);

        let total = total_probes.load(Ordering::Relaxed);
        let contended = contended_probes.load(Ordering::Relaxed);
        let rate = if total > 0 {
            contended as f64 / total as f64
        } else {
            0.0
        };

        eprintln!(
            "par_branch_contention: branches={PAR_BRANCHES}  ops_per_branch={OPS_PER_BRANCH}  \
             total_ops={}  probes={total}  contended={contended}  rate={:.1}%  (threshold <{:.0}%)",
            PAR_BRANCHES * OPS_PER_BRANCH,
            rate * 100.0,
            MAX_CONTENTION_RATE * 100.0,
        );

        assert!(
            rate < MAX_CONTENTION_RATE,
            "event_log mutex contention too high at {PAR_BRANCHES} par-branches: {:.1}% of probes \
             found the mutex held (threshold {:.0}%). Root cause: all {PAR_BRANCHES} par-branch \
             tasks share one std::sync::Mutex<event_log> and each produce() calls \
             Vec::insert(0,..) — O(n) shift where n grows to {} entries. Fix: per-branch event \
             logs merged at create_checkpoint, or replace insert(0,..) with push().",
            rate * 100.0,
            MAX_CONTENTION_RATE * 100.0,
            PAR_BRANCHES * OPS_PER_BRANCH,
        );
    }

    // Disambiguates two candidate explanations for the near-zero wall-clock
    // speedup measured end-to-end at rholang-par scale (bench_par_branches:
    // 0.88-0.96x at 32 branches against a worker-thread-bounded ideal — see
    // issue #50 follow-up):
    // (a) event_log/produce_counter themselves cost that much, or
    // (b) something else in produce()'s path (HotStore, the striped
    //     per-channel lock, tokio scheduling) is the real cost and these
    //     two locks are not the story despite being std::sync::Mutex.
    //
    // Calls log_produce() directly — the exact call log_produce() itself
    // takes (event_log.push then, for non-persist, produce_counter.insert)
    // with no produce_lock(), no HotStore, no get_store() call, no matcher
    // involved. Isolates these two locks from every other cost produce()
    // incurs, at the same branch/op counts as bench_par_branches.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn event_log_and_produce_counter_isolated_cost_at_rholang_par_scale() {
        const PAR_BRANCHES: usize = 32;
        const OPS_PER_BRANCH: usize = 5000;

        // Sequential baseline: identical total ops, one after another.
        let seq_rspace = make_rspace().await;
        let t_seq = Instant::now();
        for b in 0..PAR_BRANCHES {
            for i in 0..OPS_PER_BRANCH {
                let channel = format!("seq_{}_{}", b, i);
                let data = "datum".to_string();
                let produce_ref = Produce::create(&channel, &data, false);
                seq_rspace.log_produce(&produce_ref, &channel, &data, false);
            }
        }
        let seq_ms = t_seq.elapsed().as_millis().max(1);

        // Concurrent: PAR_BRANCHES tokio tasks, each on its own channel set,
        // all sharing one RSpace and therefore one event_log/produce_counter.
        let par_rspace = Arc::new(make_rspace().await);
        let t_par = Instant::now();
        let handles: Vec<_> = (0..PAR_BRANCHES)
            .map(|b| {
                let s = par_rspace.clone();
                tokio::spawn(async move {
                    for i in 0..OPS_PER_BRANCH {
                        let channel = format!("par_{}_{}", b, i);
                        let data = "datum".to_string();
                        let produce_ref = Produce::create(&channel, &data, false);
                        s.log_produce(&produce_ref, &channel, &data, false);
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
        let par_ms = t_par.elapsed().as_millis().max(1);

        // log_produce() is synchronous and the branch loops never await, so
        // each task holds its worker for the whole loop: at most num_workers
        // (further capped by the host's cores) branches run at once. That, not
        // PAR_BRANCHES, is the achievable ideal for the efficiency metric.
        let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
        let ideal = PAR_BRANCHES.min(num_workers).min(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(usize::MAX),
        );
        let speedup = seq_ms as f64 / par_ms as f64;
        let efficiency = speedup / ideal as f64 * 100.0;

        eprintln!(
            "event_log+produce_counter isolated cost: branches={PAR_BRANCHES} \
             ops_per_branch={OPS_PER_BRANCH} total_ops={} sequential={seq_ms}ms \
             concurrent={par_ms}ms speedup={:.2}x (ideal {ideal}x: {PAR_BRANCHES} branches on \
             {num_workers} workers) efficiency={:.1}%",
            PAR_BRANCHES * OPS_PER_BRANCH,
            speedup,
            efficiency,
        );

        // No assertion: this test is diagnostic, not a regression gate. It
        // reports the isolated cost of these two locks so it can be compared
        // against bench_par_branches' end-to-end number for the same
        // branch/op counts (see issue #50 investigation).
    }

    // With produce_counter now sharded (issue #50, part 1), this isolates
    // what's left: event_log alone, pushing directly to the field (bypassing
    // log_produce()/produce_counter entirely) at the same branch/op scale.
    // Confirms whether event_log on its own still accounts for the residual
    // near-zero speedup, and gives a before/after number for its own fix.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn event_log_isolated_cost_at_rholang_par_scale() {
        const PAR_BRANCHES: usize = 32;
        const OPS_PER_BRANCH: usize = 5000;

        // Sequential baseline: identical total ops, one after another.
        let seq_rspace = make_rspace().await;
        let t_seq = Instant::now();
        for b in 0..PAR_BRANCHES {
            for i in 0..OPS_PER_BRANCH {
                let channel = format!("seq_{}_{}", b, i);
                let data = "datum".to_string();
                let produce_ref = Produce::create(&channel, &data, false);
                seq_rspace
                    .event_log
                    .lock()
                    .expect("event log lock")
                    .push(Event::IoEvent(IOEvent::Produce(produce_ref)));
            }
        }
        let seq_ms = t_seq.elapsed().as_millis().max(1);

        // Concurrent: PAR_BRANCHES tokio tasks, all sharing one event_log.
        let par_rspace = Arc::new(make_rspace().await);
        let t_par = Instant::now();
        let handles: Vec<_> = (0..PAR_BRANCHES)
            .map(|b| {
                let s = par_rspace.clone();
                tokio::spawn(async move {
                    for i in 0..OPS_PER_BRANCH {
                        let channel = format!("par_{}_{}", b, i);
                        let data = "datum".to_string();
                        let produce_ref = Produce::create(&channel, &data, false);
                        s.event_log
                            .lock()
                            .expect("event log lock")
                            .push(Event::IoEvent(IOEvent::Produce(produce_ref)));
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
        let par_ms = t_par.elapsed().as_millis().max(1);

        let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
        let ideal = PAR_BRANCHES.min(num_workers).min(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(usize::MAX),
        );
        let speedup = seq_ms as f64 / par_ms as f64;
        let efficiency = speedup / ideal as f64 * 100.0;

        eprintln!(
            "event_log isolated cost (produce_counter excluded): branches={PAR_BRANCHES} \
             ops_per_branch={OPS_PER_BRANCH} total_ops={} sequential={seq_ms}ms \
             concurrent={par_ms}ms speedup={:.2}x (ideal {ideal}x: {PAR_BRANCHES} branches on \
             {num_workers} workers) efficiency={:.1}%",
            PAR_BRANCHES * OPS_PER_BRANCH,
            speedup,
            efficiency,
        );

        // Diagnostic, not a tight bound: absolute timings swing widely with
        // build profile (debug vs --release) and op count (see issue #50
        // follow-up), so this only catches catastrophic regressions back to
        // real lock-based contention, not general slowdowns.
        assert!(
            par_ms <= seq_ms.saturating_mul(5),
            "event_log regressed: concurrent ({par_ms}ms) more than 5x slower than sequential \
             ({seq_ms}ms) at {PAR_BRANCHES} branches x {OPS_PER_BRANCH} ops — see issue #50"
        );
    }

    // get_store() is called at least 3x per produce()/consume() (produce_lock,
    // locked_produce, store_data) with no writer ever contending it on the
    // hot path. Isolates just that call, at 2x the op count, as a regression
    // sentinel against reintroducing lock-based contention on this path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn get_store_read_lock_isolated_cost_at_rholang_par_scale() {
        const PAR_BRANCHES: usize = 32;
        const OPS_PER_BRANCH: usize = 5000;

        let seq_rspace = make_rspace().await;
        let t_seq = Instant::now();
        for _ in 0..(PAR_BRANCHES * OPS_PER_BRANCH) {
            let _ = seq_rspace.get_store();
            let _ = seq_rspace.get_store();
        }
        let seq_ms = t_seq.elapsed().as_millis().max(1);

        let par_rspace = Arc::new(make_rspace().await);
        let t_par = Instant::now();
        let handles: Vec<_> = (0..PAR_BRANCHES)
            .map(|_| {
                let s = par_rspace.clone();
                tokio::spawn(async move {
                    for _ in 0..OPS_PER_BRANCH {
                        let _ = s.get_store();
                        let _ = s.get_store();
                    }
                })
            })
            .collect();
        for h in handles {
            h.await.unwrap();
        }
        let par_ms = t_par.elapsed().as_millis().max(1);

        let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
        let ideal = PAR_BRANCHES.min(num_workers).min(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(usize::MAX),
        );
        let speedup = seq_ms as f64 / par_ms as f64;
        let efficiency = speedup / ideal as f64 * 100.0;

        eprintln!(
            "get_store() ArcSwap load isolated cost (2 calls/op): branches={PAR_BRANCHES} \
             ops_per_branch={OPS_PER_BRANCH} total_calls={} sequential={seq_ms}ms \
             concurrent={par_ms}ms speedup={:.2}x (ideal {ideal}x) efficiency={:.1}%",
            PAR_BRANCHES * OPS_PER_BRANCH * 2,
            speedup,
            efficiency,
        );

        // Diagnostic, not a tight bound: see the comment on
        // event_log_isolated_cost_at_rholang_par_scale above for why. This
        // threshold is deliberately generous — the pre-fix RwLock regressed
        // by more than 10x under concurrency, so 5x is a safe catastrophic-
        // regression floor without flaking on ordinary build/scale noise.
        assert!(
            par_ms <= seq_ms.saturating_mul(5),
            "get_store() regressed: concurrent ({par_ms}ms) more than 5x slower than sequential \
             ({seq_ms}ms) at {PAR_BRANCHES} branches x {OPS_PER_BRANCH} ops x2 calls/op — see \
             issue #50"
        );
    }
}
