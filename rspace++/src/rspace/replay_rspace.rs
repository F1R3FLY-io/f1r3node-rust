// See /home/spreston/src/firefly/f1r3fly/rspace/src/main/scala/coop/rchain/
// rspace/ReplayRSpace.scala

// NOTE: Manual marks are used instead of trace_i() because async functions
// are not compatible with the current Span trait's FnOnce closure pattern.
// This matches Scala's Span[F].traceI() semantics for async operations.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::Serialize;

use super::candidate_order::order_candidates_with_index;
use super::checkpoint::SoftCheckpoint;
use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::hashing::stable_hash_provider::StableHashSerialize;
// Disabled by the merge of feature/cost-accounted-rho (lint fix):
// `create_new_hot_store` takes F1R3Lang's `EpochHistoryReader`, so no code here
// names this trait. It was unused at HEAD too.
// use super::history::history_reader::HistoryReader;
use super::history::instances::radix_history::RadixHistory;
use super::logging::{BasicLogger, RSpaceLogger};
use super::r#match::Match;
use super::metrics_constants::{
    CONSUME_COMM_LABEL, PRODUCE_COMM_LABEL, REPLAY_RSPACE_METRICS_SOURCE,
    REPLAY_WAITING_CONTINUATIONS_CHANNEL_DEPTH_METRIC,
    REPLAY_WAITING_CONTINUATIONS_ESTIMATE_METRIC,
    REPLAY_WAITING_CONTINUATIONS_MATCHED_TOTAL_METRIC,
    REPLAY_WAITING_CONTINUATIONS_STORED_TOTAL_METRIC,
};
use super::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceCandidate, MaybeProduceResult,
    ProduceCommitGuard, RSpaceAccountingObserver, RSpaceOperationCompletion, RSpaceOperationSource,
    RSpaceResult,
};
// Disabled by the merge of feature/cost-accounted-rho (DR-131): only the
// removed `get_data_cold_decode_metered` named this trait by its short name.
// The other bounds in this file use its full path.
// use super::serializers::cold_store_decode::ColdStoreDecode;
use super::striped_locks::{self, ChannelLockGuard};
use super::trace::Log;
use super::trace::event::{COMM, Consume, Event, IOEvent, Produce, recorded_removal_for_epoch};
use super::util::unpack_option;
use crate::rspace::checkpoint::Checkpoint;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::hot_store::{HotStore, HotStoreInstances};
use crate::rspace::internal::*;
use crate::rspace::space_matcher::SpaceMatcher;

mod native_directive;
mod native_candidate;
mod ordered_cursor;
pub mod native_epoch;
pub mod native_session;

#[repr(C)]
#[derive(Clone)]
pub struct ReplayRSpace<C, P, A, K> {
    epoch: crate::rspace::hashing::stable_hash_provider::EventCodecEpoch,
    pub history_repository:
        Arc<std::sync::RwLock<Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>>>,
    pub store: Arc<std::sync::RwLock<Arc<Box<dyn HotStore<C, P, A, K>>>>>,
    installs: Arc<Mutex<HashMap<Vec<C>, Install<P, K>>>>,
    event_log: Arc<Mutex<Log>>,
    produce_counter: Arc<Mutex<BTreeMap<Produce, i32>>>,
    matcher: Arc<Box<dyn Match<P, A, K>>>,
    accounting_observer:
        Arc<std::sync::RwLock<Option<Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>>>,
    pub replay_data: Arc<Mutex<MultisetMultiMap<IOEvent, COMM>>>,
    /// Authenticated CostV2 global event order. The legacy multiset remains
    /// the candidate index, never the permission to retire a v7 event.
    ordered_cursor: Arc<Mutex<ordered_cursor::OrderedReplayCursor>>,
    /// Serializes a CostV2 transition with checkpoint/reset and keeps the
    /// cursor position paired with the corresponding hot-store snapshot.
    ordered_replay_barrier: Arc<tokio::sync::RwLock<()>>,
    // Secondary index over `replay_data`: consume.hash -> produce.hash ->
    // recorded COMMs containing both. A persistent contract's single consume
    // key can hold one recorded COMM PER FIRE (100k+ for a hot loop), and
    // `locked_consume` previously cloned that entire multiset on every
    // replayed fire — O(n^2) COMM clones across a deploy's replay, which
    // turned seconds of execution into hours of validation (the PR #280
    // convergence stall). The index narrows each fire to the COMMs whose
    // produce is actually present in the hot store (almost always exactly
    // one). Kept strictly in sync with `replay_data` (rig populates, clears
    // clear, remove_bindings_for prunes); on any miss the caller falls back
    // to the full multiset scan, so behavior is unchanged in edge cases.
    consume_replay_index: Arc<Mutex<HashMap<Blake2b256Hash, HashMap<Blake2b256Hash, Vec<COMM>>>>>,
    logger: Arc<Mutex<Box<dyn RSpaceLogger<C, P, A, K>>>>,
    replay_waiting_continuations_estimate: Arc<AtomicI64>,
    // Fixed-size striped locks — mirrors the RSpace fix in rspace.rs (PR #72).
    // ReplayRSpace still had the pre-#72 growing DashMap<u64, Arc<Mutex>>
    // (unbounded, `entry()` shard contention on insert), which #72 never
    // ported here — see #43. See striped_locks.rs for the shared scheme.
    phase_a_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
    phase_b_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
}

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    pub fn get_store(&self) -> Arc<Box<dyn HotStore<C, P, A, K>>> {
        self.store.read().expect("store read lock").clone()
    }

    pub fn codec_epoch(&self) -> crate::rspace::hashing::stable_hash_provider::EventCodecEpoch {
        self.epoch
    }

    /// Build the historical IO-to-COMM candidate index. CostV2 uses this
    /// only after the distinct ordered cursor has been charged and installed;
    /// a Resting event has no IO leg and is never inserted into this index.
    fn rig_candidate_index(&self, log: &Log) -> Result<(), RSpaceError> {
        let (io_events, comm_events): (Vec<_>, Vec<_>) = log
            .iter()
            .partition(|event| matches!(event, Event::IoEvent(_)));
        let new_stuff: HashSet<_> = io_events.into_iter().collect();

        let replay_data = self.replay_data.lock().expect("replay data lock");
        replay_data.clear();
        let mut consume_index = self
            .consume_replay_index
            .lock()
            .expect("consume replay index lock");
        consume_index.clear();

        for event in comm_events {
            match event {
                Event::Comm(comm) => {
                    for produce in &comm.produces {
                        consume_index
                            .entry(comm.consume.hash.clone())
                            .or_default()
                            .entry(produce.hash.clone())
                            .or_default()
                            .push(comm.clone());
                    }
                    let comm_cloned = comm.clone();
                    let (consume, produces) = (comm_cloned.consume, comm_cloned.produces);
                    let produce_io_events: Vec<IOEvent> =
                        produces.into_iter().map(IOEvent::Produce).collect();

                    let mut io_events = produce_io_events.clone();
                    io_events.insert(0, IOEvent::Consume(consume));

                    for io_event in io_events {
                        let io_event_converted: Event = match io_event {
                            IOEvent::Produce(ref p) => Event::IoEvent(IOEvent::Produce(p.clone())),
                            IOEvent::Consume(ref c) => Event::IoEvent(IOEvent::Consume(c.clone())),
                        };

                        if new_stuff.contains(&io_event_converted) {
                            replay_data.add_binding(io_event, comm.clone());
                        }
                    }
                }
                Event::RestingComm(_)
                    if self.epoch ==
                        crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 => {}
                _ => return Err(RSpaceError::ProduceCommitProtocolViolation),
            }
        }
        Ok(())
    }

    /// `E(S)` for the replay space — see
    /// [`crate::rspace::space_matcher::SpaceMatcher::enumerate_enabled_rendezvous`],
    /// which is where the whole query lives. Byte-for-byte the same three lines
    /// as `RSpace::enabled_rendezvous`; the shared default method is what makes
    /// play and replay enumerate identically rather than agreeing by assertion.
    ///
    /// Read-only: no store mutation, no event log entry, no `replay_data`
    /// consumption. Replay's pools are the same canonical order play's are
    /// (`order_candidates_with_index`), so a state that replays to the same hot
    /// store enumerates to the same `E(S)` in the same order.
    pub fn enabled_rendezvous(&self) -> Result<Vec<ProduceCandidate<C, P, A, K>>, RSpaceError> {
        self.enumerate_enabled_rendezvous(&self.matcher, &self.get_store())
    }

    pub fn get_history_repository(
        &self,
    ) -> Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>> {
        self.history_repository
            .read()
            .expect("history read lock")
            .clone()
    }

    async fn consume_lock(&self, channel_hashes: &[u64]) -> (ChannelLockGuard, ChannelLockGuard) {
        striped_locks::consume_lock(&self.phase_a_locks, &self.phase_b_locks, channel_hashes).await
    }

    async fn produce_lock(
        &self,
        channel: &C,
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        let channel_hash = striped_locks::channel_hash(channel);
        let phase_a = striped_locks::acquire_locks(&self.phase_a_locks, &[channel_hash]).await;

        let store = self.get_store();
        let join_hashes: Vec<u64> = store
            .get_joins(channel)?
            .into_iter()
            .flatten()
            .map(|ch| striped_locks::channel_hash(&ch))
            .collect();

        let phase_b = striped_locks::acquire_locks(&self.phase_b_locks, &join_hashes).await;
        Ok((phase_a, phase_b))
    }

    fn new_striped_locks() -> Arc<Vec<Arc<tokio::sync::Mutex<()>>>> {
        striped_locks::new_striped_locks()
    }
}

impl<C, P, A, K> SpaceMatcher<C, P, A, K> for ReplayRSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
}

#[async_trait]
impl<C, P, A, K> ISpace<C, P, A, K> for ReplayRSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + StableHashSerialize
        + crate::rspace::serializers::cold_store_decode::ColdStoreDecode
        + std::hash::Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + crate::rspace::serializers::cold_store_decode::ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + crate::rspace::serializers::cold_store_decode::ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + crate::rspace::serializers::cold_store_decode::ColdStoreDecode
        + 'static
        + Sync
        + Send,
{
    fn codec_epoch(&self) -> crate::rspace::hashing::stable_hash_provider::EventCodecEpoch {
        self.epoch
    }

    fn set_accounting_observer(
        &self,
        observer: Option<Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>,
    ) {
        *self
            .accounting_observer
            .write()
            .expect("accounting observer write lock") = observer;
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
        let _ordered_replay_barrier = self.ordered_replay_barrier.write().await;
        self.check_replay_data().await?;

        let changes = self.get_store().changes();
        let next_history = self
            .get_history_repository()
            .checkpoint_for_epoch(self.epoch, changes)?;
        let history_reader =
            next_history.get_history_reader_for_epoch(&next_history.root(), self.epoch)?;
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        self.create_new_hot_store(history_reader);
        self.restore_installs()?;

        Ok(Checkpoint {
            root: self.get_history_repository().root(),
            log: Vec::new(),
        })
    }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
        let _ordered_replay_barrier = self.ordered_replay_barrier.write().await;
        if self
            .ordered_cursor
            .lock()
            .expect("ordered replay cursor lock")
            .is_pending()
        {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let next_history = self.get_history_repository().reset(root)?;
        let history_reader = next_history.get_history_reader_for_epoch(root, self.epoch)?;
        *self.history_repository.write().expect("history write lock") = Arc::new(next_history);

        *self.event_log.lock().expect("event log lock") = Vec::new();
        *self.produce_counter.lock().expect("produce counter lock") = BTreeMap::new();

        // Striped locks are fixed-size and stateless (Mutex<()>); nothing to
        // clear on reset — they are reused across checkpoints.

        self.create_new_hot_store(history_reader);
        self.restore_installs()?;
        self.replay_waiting_continuations_estimate
            .store(0, Ordering::Relaxed);
        self.ordered_cursor
            .lock()
            .expect("ordered replay cursor lock")
            .clear();
        metrics::gauge!(
            REPLAY_WAITING_CONTINUATIONS_ESTIMATE_METRIC,
            "source" => REPLAY_RSPACE_METRICS_SOURCE
        )
        .set(0.0);

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

    async fn get_data(&self, channel: &C) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.get_store().get_data(channel)
    }

    async fn get_waiting_continuations(
        &self,
        channels: Vec<C>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        self.get_store().get_continuations(&channels)
    }

    async fn get_joins(&self, channel: C) -> Result<Vec<Vec<C>>, RSpaceError> {
        self.get_store().get_joins(&channel)
    }

    async fn remove_all_data(&self, channel: &C) -> Result<(), RSpaceError> {
        let len = self.get_store().get_data(channel)?.len();
        for index in (0..len).rev() {
            self.get_store().remove_datum(channel, index as i32)?;
        }
        Ok(())
    }

    async fn remove_data_at(&self, channel: &C, index: i32) -> Result<(), RSpaceError> {
        self.get_store().remove_datum(channel, index)
    }

    async fn remove_data_at_recorded(
        &self,
        channel: &C,
        index: i32,
        operation_id: &[u8],
    ) -> Result<(), RSpaceError> {
        let channel_lock = striped_locks::channel_hash(channel);
        let _guard = self.consume_lock(&[channel_lock]).await;
        let data = self.get_store().get_data(channel)?;
        let datum = usize::try_from(index)
            .ok()
            .and_then(|index| data.get(index).cloned())
            .ok_or_else(|| {
                RSpaceError::BugFoundError(
                    "recorded replay removal references a missing datum".to_string(),
                )
            })?;
        let (consume, comm) =
            recorded_removal_for_epoch(self.epoch, channel, &datum.source, operation_id)?;
        let available = self
            .replay_data
            .lock()
            .expect("replay data lock")
            .map
            .get(&IOEvent::Consume(consume.clone()))
            .is_some_and(|bindings| bindings.iter().any(|binding| binding.0 == &comm));
        if !available {
            return Err(RSpaceError::BugFoundError(
                "recorded replay removal is absent from the proposer trace".to_string(),
            ));
        }
        self.get_store().remove_datum(channel, index)?;
        {
            let mut event_log = self.event_log.lock().expect("event log lock");
            event_log.push(Event::IoEvent(IOEvent::Consume(consume)));
            event_log.push(Event::Comm(comm.clone()));
        }
        self.remove_bindings_for(comm);
        Ok(())
    }

    async fn remove_all_continuations(&self, channels: Vec<C>) -> Result<(), RSpaceError> {
        let len = self.get_store().get_continuations(&channels)?.len();
        for index in (0..len).rev() {
            let _ = self
                .get_store()
                .remove_continuation(&channels, index as i32)?;
        }
        Ok(())
    }

    async fn clear(&self) -> Result<(), RSpaceError> {
        self.replay_data.lock().expect("replay data lock").clear();
        self.consume_replay_index
            .lock()
            .expect("consume replay index lock")
            .clear();
        self.reset(&RadixHistory::empty_root_node_hash()).await
    }

    async fn get_root(&self) -> Blake2b256Hash { self.get_history_repository().root() }

    async fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> { self.get_store().to_map() }

    async fn create_soft_checkpoint(&self) -> SoftCheckpoint<C, P, A, K> {
        let _ordered_replay_barrier = self.ordered_replay_barrier.write().await;
        let cache_snapshot = self.get_store().snapshot();
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let curr_produce_counter =
            std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));

        SoftCheckpoint {
            cache_snapshot,
            log: curr_event_log,
            produce_counter: curr_produce_counter,
            replay_cursor: self
                .ordered_cursor
                .lock()
                .expect("ordered replay cursor lock")
                .checkpoint()
                .map(|checkpoint| (checkpoint.generation, checkpoint.next)),
        }
    }

    async fn take_event_log(&self) -> Log {
        let _ordered_replay_barrier = self.ordered_replay_barrier.write().await;
        let curr_event_log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let _ = std::mem::take(&mut *self.produce_counter.lock().expect("produce counter lock"));
        curr_event_log
    }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError> {
        let _ordered_replay_barrier = self.ordered_replay_barrier.write().await;
        let history = self.get_history_repository();
        let history_reader = history.get_history_reader_for_epoch(&history.root(), self.epoch)?;
        let hot_store = HotStoreInstances::create_from_hs_and_hr(
            checkpoint.cache_snapshot,
            history_reader.base(),
        );

        self.ordered_cursor
            .lock()
            .expect("ordered replay cursor lock")
            .restore(
                checkpoint.replay_cursor.map(|(generation, next)| {
                    ordered_cursor::CursorCheckpoint { generation, next }
                }),
            )?;

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
        if self.epoch == crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        // ★★ THE SHARPER HALF OF THE PAIR. `RSpace::consume` and this function
        // must classify a malformed request IDENTICALLY, because a refusal that
        // happens during replay but not during play — or the reverse — is the
        // divergence class itself, not a mere inconsistency: replay would
        // report a mismatch against a play run that never saw one.
        //
        // Both now refuse, with the same variant and the same text, so the pair
        // is symmetric by construction. Before this change both panicked, which
        // was also symmetric — but symmetric in the mode where the node dies,
        // and a dead node cannot report a rejection.
        //
        // The classification argument is `RSpace::consume`'s, verbatim: the two
        // lengths are in the caller's own arguments, and `ConsumeParams`
        // carries them in independent repeated fields.
        if channels.is_empty() {
            Err(RSpaceError::BugFoundError("RUST ERROR: channels can't be empty".to_string()))
        } else if channels.len() != patterns.len() {
            Err(RSpaceError::BugFoundError(
                "RUST ERROR: channels.length must equal patterns.length".to_string(),
            ))
        } else {
            let consume_ref = Consume::create_for_epoch(
                self.epoch,
                &channels,
                &patterns,
                &continuation,
                persist,
            )?;
            let channel_hashes: Vec<u64> = channels
                .iter()
                .map(|ch| striped_locks::channel_hash(ch))
                .collect();
            let _lock_guard = self.consume_lock(&channel_hashes).await;

            metrics::counter!("replay_rspace.consume.calls", "source" => "rspace").increment(1);
            let start = Instant::now();
            let result =
                self.locked_consume(channels, patterns, continuation, persist, peeks, consume_ref);
            metrics::histogram!("replay_consume_time_seconds", "source" => "rspace")
                .record(start.elapsed().as_secs_f64());
            result
        }
    }

    async fn produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        if self.epoch == crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        self.produce_with_guard(channel, data, persist, None).await
    }

    async fn produce_guarded(
        &self,
        channel: C,
        data: A,
        persist: bool,
        guard: &dyn ProduceCommitGuard,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        if self.epoch == crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        self.produce_with_guard(channel, data, persist, Some(guard))
            .await
    }

    async fn install(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        self.locked_install_internal(channels, patterns, continuation)
    }

    async fn rig_and_reset(&self, start_root: Blake2b256Hash, log: Log) -> Result<(), RSpaceError> {
        if self.epoch == crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        self.rig(log).await?;
        self.reset(&start_root).await
    }

    async fn rig(&self, log: Log) -> Result<(), RSpaceError> {
        if self.epoch == crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        if log
            .iter()
            .any(|event| matches!(event, Event::RestingComm(_)))
        {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        self.rig_candidate_index(&log)
    }

    async fn check_replay_data(&self) -> Result<(), RSpaceError> {
        if !self
            .ordered_cursor
            .lock()
            .expect("ordered replay cursor lock")
            .is_complete()
        {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let replay_data = self.replay_data.lock().expect("replay data lock");
        if replay_data.is_empty() {
            Ok(())
        } else {
            let leftover: Vec<String> = replay_data
                .map
                .iter()
                .take(8)
                .map(|e| format!("{:?}", e.key()))
                .collect();
            Err(RSpaceError::UnusedCommEvent {
                leftover_count: replay_data.map.len(),
                leftover,
            })
        }
    }

    async fn is_replay(&self) -> bool { true }

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

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + StableHashSerialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    /**
     * Creates [[ReplayRSpace]] from [[HistoryRepository]] and [[HotStore]].
     */
    pub fn apply(
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Arc<Box<dyn HotStore<C, P, A, K>>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> ReplayRSpace<C, P, A, K>
    where
        C: Clone + Debug + Ord + Hash,
        P: Clone + Debug,
        A: Clone + Debug,
        K: Clone + Debug,
    {
        Self::apply_for_epoch(
            crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::Legacy,
            history_repository,
            store,
            matcher,
        )
    }

    pub fn apply_for_epoch(
        epoch: crate::rspace::hashing::stable_hash_provider::EventCodecEpoch,
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Arc<Box<dyn HotStore<C, P, A, K>>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> ReplayRSpace<C, P, A, K> {
        ReplayRSpace {
            epoch,
            history_repository: Arc::new(std::sync::RwLock::new(history_repository)),
            store: Arc::new(std::sync::RwLock::new(store)),
            matcher,
            accounting_observer: Arc::new(std::sync::RwLock::new(None)),
            installs: Arc::new(Mutex::new(HashMap::new())),
            event_log: Arc::new(Mutex::new(Vec::new())),
            produce_counter: Arc::new(Mutex::new(BTreeMap::new())),
            replay_data: Arc::new(Mutex::new(MultisetMultiMap::empty())),
            ordered_cursor: Arc::new(Mutex::new(ordered_cursor::OrderedReplayCursor::default())),
            ordered_replay_barrier: Arc::new(tokio::sync::RwLock::new(())),
            consume_replay_index: Arc::new(Mutex::new(HashMap::new())),
            logger: Arc::new(Mutex::new(Box::new(BasicLogger::new()))),
            replay_waiting_continuations_estimate: Arc::new(AtomicI64::new(0)),
            phase_a_locks: Self::new_striped_locks(),
            phase_b_locks: Self::new_striped_locks(),
        }
    }

    pub fn apply_with_logger(
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Arc<Box<dyn HotStore<C, P, A, K>>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        logger: Box<dyn RSpaceLogger<C, P, A, K>>,
    ) -> ReplayRSpace<C, P, A, K>
    where
        C: Clone + Debug + Ord + Hash,
        P: Clone + Debug,
        A: Clone + Debug,
        K: Clone + Debug,
    {
        Self::apply_with_logger_for_epoch(
            crate::rspace::hashing::stable_hash_provider::EventCodecEpoch::Legacy,
            history_repository,
            store,
            matcher,
            logger,
        )
    }

    pub fn apply_with_logger_for_epoch(
        epoch: crate::rspace::hashing::stable_hash_provider::EventCodecEpoch,
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Arc<Box<dyn HotStore<C, P, A, K>>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
        logger: Box<dyn RSpaceLogger<C, P, A, K>>,
    ) -> ReplayRSpace<C, P, A, K> {
        ReplayRSpace {
            epoch,
            history_repository: Arc::new(std::sync::RwLock::new(history_repository)),
            store: Arc::new(std::sync::RwLock::new(store)),
            matcher,
            accounting_observer: Arc::new(std::sync::RwLock::new(None)),
            installs: Arc::new(Mutex::new(HashMap::new())),
            event_log: Arc::new(Mutex::new(Vec::new())),
            produce_counter: Arc::new(Mutex::new(BTreeMap::new())),
            replay_data: Arc::new(Mutex::new(MultisetMultiMap::empty())),
            ordered_cursor: Arc::new(Mutex::new(ordered_cursor::OrderedReplayCursor::default())),
            ordered_replay_barrier: Arc::new(tokio::sync::RwLock::new(())),
            consume_replay_index: Arc::new(Mutex::new(HashMap::new())),
            logger: Arc::new(Mutex::new(logger)),
            replay_waiting_continuations_estimate: Arc::new(AtomicI64::new(0)),
            phase_a_locks: Self::new_striped_locks(),
            phase_b_locks: Self::new_striped_locks(),
        }
    }

    fn inc_replay_waiting_continuations(&self, channels: &[C]) -> Result<(), RSpaceError> {
        let depth = self.get_store().get_continuations(channels)?.len();
        self.inc_replay_waiting_continuations_depth(depth);
        Ok(())
    }

    fn inc_replay_waiting_continuations_depth(&self, depth: usize) {
        metrics::counter!(
            REPLAY_WAITING_CONTINUATIONS_STORED_TOTAL_METRIC,
            "source" => REPLAY_RSPACE_METRICS_SOURCE
        )
        .increment(1);
        let estimate = self
            .replay_waiting_continuations_estimate
            .fetch_add(1, Ordering::Relaxed) +
            1;
        metrics::gauge!(
            REPLAY_WAITING_CONTINUATIONS_ESTIMATE_METRIC,
            "source" => REPLAY_RSPACE_METRICS_SOURCE
        )
        .set(estimate as f64);
        metrics::histogram!(
            REPLAY_WAITING_CONTINUATIONS_CHANNEL_DEPTH_METRIC,
            "source" => REPLAY_RSPACE_METRICS_SOURCE
        )
        .record(depth as f64);
    }

    fn dec_replay_waiting_continuations(&self) {
        let mut current = self
            .replay_waiting_continuations_estimate
            .load(Ordering::Relaxed);
        loop {
            if current <= 0 {
                metrics::gauge!(
                    REPLAY_WAITING_CONTINUATIONS_ESTIMATE_METRIC,
                    "source" => REPLAY_RSPACE_METRICS_SOURCE
                )
                .set(0.0);
                return;
            }
            match self
                .replay_waiting_continuations_estimate
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => {
                    metrics::gauge!(
                        REPLAY_WAITING_CONTINUATIONS_ESTIMATE_METRIC,
                        "source" => REPLAY_RSPACE_METRICS_SOURCE
                    )
                    .set((current - 1) as f64);
                    return;
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn inc_replay_waiting_continuations_matched_total(&self) {
        metrics::counter!(
            REPLAY_WAITING_CONTINUATIONS_MATCHED_TOTAL_METRIC,
            "source" => REPLAY_RSPACE_METRICS_SOURCE
        )
        .increment(1);
    }

    fn mark_replay_waiting_continuation_match(&self) {
        if self
            .replay_waiting_continuations_estimate
            .load(Ordering::Relaxed) >
            0
        {
            self.dec_replay_waiting_continuations();
        }
        self.inc_replay_waiting_continuations_matched_total();
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

    #[inline]
    fn get_produce_count(&self, produce_ref: &Produce) -> i32 {
        *self
            .produce_counter
            .lock()
            .expect("produce counter lock")
            .get(produce_ref)
            .unwrap_or(&0)
    }

    fn locked_consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
        consume_ref: Consume,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-consume") from Scala - works because this is
        // NOT async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "locked-consume").entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-consume", "locked_consume");

        tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, persist, "replay.consume ENTER");

        let source = RSpaceOperationSource::Consume(&consume_ref);
        let observer = self.start_accounting_operation(source, &channels, &[])?;
        let result = self.locked_consume_observed(
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            consume_ref.clone(),
            observer.as_deref(),
        );
        if let Some(observer) = observer {
            observer
                .observe_operation_finish(source, RSpaceOperationCompletion::from_result(&result));
        }
        result
    }

    fn locked_consume_observed(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
        consume_ref: Consume,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        if let Some(directive) = observer
            .map(|observer| {
                observer.replay_operation_directive(RSpaceOperationSource::Consume(&consume_ref))
            })
            .transpose()?
            .flatten()
        {
            return self.replay_native_consume(
                channels,
                patterns,
                continuation,
                persist,
                peeks,
                consume_ref,
                observer,
                directive,
            );
        }
        Self::observe_consume(
            observer,
            &consume_ref,
            &channels,
            &patterns,
            &continuation,
            persist,
            &peeks,
        )?;

        let wk = WaitingContinuation {
            patterns: Arc::new(patterns.clone()),
            continuation: Arc::new(continuation),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        // Fast path: a persistent contract's consume key can hold one
        // recorded COMM per fire (100k+ for a hot loop), and the replayed
        // re-consume of such a contract runs BEFORE its next produce exists,
        // so the hot store holds no data for its channels. The old path
        // cloned the whole COMM multiset and ran the matcher over every
        // candidate against that empty data — deterministically None, at
        // O(n) clones per fire, O(n^2) per replay (the PR #280 convergence
        // stall). With zero store data the outcome is always
        // store_waiting_continuation, so take it directly. When data IS
        // present, narrow candidates through the consume/produce index;
        // any miss falls back to the full clone, preserving behavior.
        let mut present_produce_hashes: HashSet<Blake2b256Hash> = HashSet::new();
        for channel in &channels {
            present_produce_hashes.extend(
                self.get_store()
                    .get_data(channel)?
                    .into_iter()
                    .map(|d| d.source.hash),
            );
        }
        if present_produce_hashes.is_empty() {
            let recorded = self
                .replay_data
                .lock()
                .unwrap()
                .map
                .contains_key(&IOEvent::Consume(consume_ref.clone()));
            if recorded {
                tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, "replay.consume STALL-B: recorded COMM exists but no data in store yet (matcher would find no candidate)");
            } else {
                tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, "replay.consume STALL-A: no recorded COMM for this consume (validator did a consume absent from the proposer's trace -> execution diverged)");
            }
            let result = self.store_waiting_continuation(channels.clone(), wk.clone())?;
            self.log_consume(consume_ref, &channels, &patterns, &wk.continuation, persist, &peeks);
            return Ok(result);
        }
        let comms_option = {
            // Changed by the merge of feature/cost-accounted-rho (plan step
            // 1.9, CBR-048): the checkpoint merge e5102e945 lost the canonical
            // projection of dd11241da. A raw `Counter<COMM>` iteration follows
            // the hash map's process-local order, so two validators could try
            // the recorded COMMs in different orders.
            // ```text
            // let full = || {
            //     self.replay_data
            //         .lock()
            //         .unwrap()
            //         .map
            //         .get(&IOEvent::Consume(consume_ref.clone()))
            //         .map(|comms| {
            //             comms
            //                 .iter()
            //                 .map(|tuple| tuple.0.clone())
            //                 .collect::<Vec<_>>()
            //         })
            // };
            // ```
            let full = || {
                self.replay_data
                    .lock()
                    .unwrap()
                    .get_distinct_values_sorted(&IOEvent::Consume(consume_ref.clone()))
            };
            let narrowed: Option<Vec<COMM>> = {
                let index = self
                    .consume_replay_index
                    .lock()
                    .expect("consume replay index lock");
                index.get(&consume_ref.hash).map(|by_produce| {
                    let mut out: Vec<COMM> = Vec::new();
                    for hash in &present_produce_hashes {
                        if let Some(comms) = by_produce.get(hash) {
                            for comm in comms {
                                if !out.contains(comm) {
                                    out.push(comm.clone());
                                }
                            }
                        }
                    }
                    // Added by the merge of feature/cost-accounted-rho (plan
                    // step 1.9, CBR-048): the narrowed list is upstream's new
                    // path, and its order follows the store's data order. It
                    // holds distinct recorded COMMs, so it takes the same
                    // canonical order as the full projection.
                    out.sort_unstable();
                    out
                })
            };
            match narrowed {
                Some(comms) if !comms.is_empty() => Some(comms),
                _ => full(),
            }
        };
        match comms_option {
            None => {
                tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, "replay.consume STALL-A: no recorded COMM for this consume (validator did a consume absent from the proposer's trace -> execution diverged)");
                let result = self.store_waiting_continuation(channels.clone(), wk.clone())?;
                self.log_consume(
                    consume_ref.clone(),
                    &channels,
                    &patterns,
                    &wk.continuation,
                    persist,
                    &peeks,
                );
                Ok(result)
            }
            Some(comms_list) => {
                match self.get_comm_and_consume_candidates(
                    channels.clone(),
                    patterns,
                    // The continuation the commit guard is evaluated against.
                    // `wk` already owns it behind the `Arc` built above.
                    &wk.continuation,
                    comms_list.clone(),
                )? {
                    None => {
                        tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, n_recorded_comms = comms_list.len(), "replay.consume STALL-B: recorded COMM exists but matching produce data not present (produce diverged / missing in merged pre-state)");
                        let result =
                            self.store_waiting_continuation(channels.clone(), wk.clone())?;
                        self.log_consume(
                            consume_ref.clone(),
                            &channels,
                            &wk.patterns,
                            &wk.continuation,
                            persist,
                            &peeks,
                        );
                        Ok(result)
                    }
                    Some((_, (data_candidates, permit))) => {
                        let produce_counters_closure =
                            |produces: &[Produce]| self.produce_counters(produces);

                        let comm_ref = COMM::new(
                            &data_candidates,
                            consume_ref.clone(),
                            peeks.clone(),
                            produce_counters_closure,
                        );

                        assert!(
                            comms_list.contains(&comm_ref),
                            "{}",
                            format!(
                                "COMM Event {:?} was not contained in the trace {:?}",
                                comm_ref, comms_list
                            )
                        );

                        Self::observe_comm(
                            observer,
                            &comm_ref,
                            &wk.continuation,
                            wk.persist,
                            &data_candidates,
                        )?;
                        for candidate in &data_candidates {
                            self.get_store().get_joins(&candidate.channel)?;
                        }
                        let result = permit.commit(|| -> Result<_, RSpaceError> {
                            self.store_persistent_data(&data_candidates)?;
                            self.log_consume(
                                consume_ref.clone(),
                                &channels,
                                &wk.patterns,
                                &wk.continuation,
                                persist,
                                &peeks,
                            );
                            self.log_comm(
                                &data_candidates,
                                &channels,
                                wk.clone(),
                                comm_ref.clone(),
                                "comm.consume",
                            );
                            self.remove_bindings_for(comm_ref);
                            Ok(self.wrap_result(channels, wk, consume_ref.clone(), data_candidates))
                        })??;
                        tracing::trace!(target: "f1r3fly.rspace.ops", channels = ?consume_ref.channel_hashes, "replay.consume OK: COMM reproduced");
                        Ok(result)
                    }
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
    fn fetch_channel_to_index_data(
        &self,
        channels: &[C],
    ) -> Result<HashMap<C, Vec<(Datum<A>, i32)>>, RSpaceError> {
        let mut map = HashMap::with_capacity(channels.len());
        for c in channels {
            let data = self.get_store().get_data(c)?;
            // THE canonical candidate order — the same one the play space uses
            // (`crate::rspace::candidate_order`). Replay used raw store order
            // here; see `run_matcher_consume` for why the two must agree.
            map.insert(c.clone(), order_candidates_with_index(data));
        }
        Ok(map)
    }

    fn get_comm_and_consume_candidates(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: &K,
        comms: Vec<COMM>,
    ) -> Result<
        Option<(COMM, (Vec<ConsumeCandidate<C, A>>, super::r#match::PreparedCommit))>,
        RSpaceError,
    > {
        let run_matcher = |comm: COMM| {
            self.run_matcher_consume(channels.clone(), patterns.clone(), continuation, comm)
        };

        self.get_comm_or_candidate(comms, run_matcher)
    }

    /// Rebuild, under replay, the candidate selection the recorded `comm` says
    /// the play space made.
    ///
    /// ★ Play and replay must agree on the SELECTION, not merely on the COMM.
    /// `COMM::new` sorts its produce refs, so two selections that permute the
    /// same data across binds — `for(@x <- c; @y <- c)` fed by two data on `c`
    /// — build the SAME COMM and slip past the trace assertion while binding
    /// the receive's variables the other way round, which diverges the
    /// post-state silently. Two things make that unreachable, and both are
    /// required:
    ///
    /// 1. Changed by the complete guard search (defect D1, user decision 4 of
    ///    2026-10-16): replay keeps only the recorded data and orders them by
    ///    THE canonical candidate order. Play draws its pools in random order
    ///    (DR-129), but E1 orders the pool of each channel that the join names
    ///    more than once by the same key, and the pool of a channel that the
    ///    join names once keeps only data equal to the recorded datum (equal
    ///    source hashes mean equal data). So replay walks the recorded data in
    ///    play's order, and
    /// 2. the selection is made by the same guard-aware search the play space
    ///    uses, so a guard that discriminates between two assignments decides
    ///    the same way on both sides (this path did not consult the guard at
    ///    all before — it did not need to while a guard rejection could only
    ///    ever mean "no COMM at all", which defect D1's fix changes).
    ///
    /// The COMM filter below only REMOVES candidates, so replay's pool, in the
    /// order of point 1, is a subsequence of play's, and the first admissible
    /// selection of the recorded data is the same in both. See
    /// [`crate::rspace::space_matcher::SpaceMatcher::extract_prepared_data_candidates`].
    fn run_matcher_consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: &K,
        comm: COMM,
    ) -> Result<Option<(Vec<ConsumeCandidate<C, A>>, super::r#match::PreparedCommit)>, RSpaceError>
    {
        let mut channel_to_indexed_data_list: Vec<(C, Vec<(Datum<A>, i32)>)> = Vec::new();

        for c in &channels {
            let data = self.get_store().get_data(c)?;
            let filtered_data: Vec<(Datum<A>, i32)> = order_candidates_with_index(data)
                .into_iter()
                .filter(|(d, i)| self.matches(comm.clone(), (d.clone(), *i)))
                .collect();
            channel_to_indexed_data_list.push((c.clone(), filtered_data));
        }

        let mut channel_to_indexed_data_map: HashMap<C, Vec<(Datum<A>, i32)>> =
            channel_to_indexed_data_list.into_iter().collect();

        // Borrow-zip: no per-attempt channel/pattern clones.
        let pairs: Vec<(&C, &P)> = channels.iter().zip(patterns.iter()).collect();

        Ok(self.extract_prepared_data_candidates(
            &self.matcher,
            &pairs,
            continuation,
            &mut channel_to_indexed_data_map,
        ))
    }

    async fn produce_with_guard(
        &self,
        channel: C,
        data: A,
        persist: bool,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        let produce_ref = Produce::create_for_epoch(self.epoch, &channel, &data, persist)?;
        let lock_guard = self.produce_lock(&channel).await?;
        metrics::counter!("replay_rspace.produce.calls", "source" => "rspace").increment(1);
        let start = Instant::now();
        let data = Arc::new(data);
        let result = self.locked_produce(
            channel.clone(),
            Arc::clone(&data),
            persist,
            produce_ref.clone(),
            guard,
        );
        metrics::histogram!("replay_produce_time_seconds", "source" => "rspace")
            .record(start.elapsed().as_secs_f64());
        drop(lock_guard);
        let result = result?;
        // Disabled by the merge of feature/cost-accounted-rho (plan step 1.9):
        // 1bd18074e notified here because its commit closures only counted the
        // produce. In the merged code every commit closure calls `log_produce`,
        // which notifies after the last fallible step of the commit and before
        // the COMM is logged. A second notification here made the reporting
        // space record each produce twice.
        // self.notify_produce(produce_ref, &channel, &data, persist);
        Ok(result)
    }

    fn locked_produce(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        produce_ref: Produce,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-produce") from Scala - works because this is
        // NOT async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "locked-produce").entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-produce", "locked_produce");

        let grouped_channels = self.get_store().get_joins(&channel)?;
        let source = RSpaceOperationSource::Produce(&produce_ref);
        let observer = self.start_accounting_operation(
            source,
            std::slice::from_ref(&channel),
            &grouped_channels,
        )?;
        let result = self.locked_produce_observed(
            channel,
            data,
            persist,
            produce_ref.clone(),
            grouped_channels,
            observer.as_deref(),
            guard,
        );
        if let Some(observer) = observer {
            observer
                .observe_operation_finish(source, RSpaceOperationCompletion::from_result(&result));
        }
        result
    }

    fn locked_produce_observed(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        produce_ref: Produce,
        grouped_channels: Vec<Vec<C>>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        if let Some(directive) = observer
            .map(|observer| {
                observer.replay_operation_directive(RSpaceOperationSource::Produce(&produce_ref))
            })
            .transpose()?
            .flatten()
        {
            return self.replay_native_produce(
                channel,
                (*data).clone(),
                persist,
                produce_ref,
                grouped_channels,
                observer,
                directive,
                guard,
            );
        }
        Self::observe_produce(observer, &produce_ref, &channel, &data, persist)?;

        tracing::trace!(target: "f1r3fly.rspace.ops", channel = ?produce_ref.channel_hash, persist, "replay.produce ENTER");
        // O(1) hash lookup. `IOEvent` derives `Hash`/`Eq`, and `Produce`'s
        // manual impls hash/compare on `self.hash` only — the metadata
        // fields (`is_deterministic`, `output_value`, `failed`) are
        // documented as non-identity, so this is semantically identical
        // to a hash-only linear scan over the map.
        //
        // Changed by the merge of feature/cost-accounted-rho (plan step 1.9,
        // CBR-048): the checkpoint merge e5102e945 lost the canonical
        // projection of dd11241da, as in the consume path.
        // ```text
        // let comms_option = self
        //     .replay_data
        //     .lock()
        //     .unwrap()
        //     .map
        //     .get(&IOEvent::Produce(produce_ref.clone()))
        //     .map(|comms| {
        //         comms
        //             .iter()
        //             .map(|tuple| tuple.0.clone())
        //             .collect::<Vec<_>>()
        //     });
        // ```
        let comms_option = self
            .replay_data
            .lock()
            .unwrap()
            .get_distinct_values_sorted(&IOEvent::Produce(produce_ref.clone()));
        match comms_option {
            None => {
                tracing::trace!(target: "f1r3fly.rspace.ops", channel = ?produce_ref.channel_hash, "replay.produce: stored (no recorded COMM for this produce)");
                super::rspace_interface::commit_produce(guard, || {
                    let result = self.store_data(
                        channel.clone(),
                        data.clone(),
                        persist,
                        produce_ref.clone(),
                    )?;
                    self.log_produce(produce_ref, &channel, &data, persist);
                    Ok(result)
                })?
            }
            Some(comms) => {
                match self.get_comm_or_produce_candidate(
                    channel.clone(),
                    data.clone(),
                    persist,
                    comms.clone(),
                    produce_ref.clone(),
                    grouped_channels,
                )? {
                    Some((comm, pc)) => {
                        tracing::trace!(target: "f1r3fly.rspace.ops", channel = ?produce_ref.channel_hash, "replay.produce OK: matched a recorded COMM (fired a waiting consume)");
                        // Changed by the merge of feature/cost-accounted-rho
                        // (clippy gate): the arm returned this expression
                        // through a `let result` binding.
                        // ```text
                        // let result = self
                        //     .handle_match(
                        //         pc,
                        //         comms,
                        //         &channel,
                        //         &data,
                        //         persist,
                        //         &produce_ref,
                        //         observer,
                        //         guard,
                        //     )
                        //     .map(|matched| {
                        //         matched.map(|consume_result| {
                        //             let p = comm
                        //                 .produces
                        //                 .into_iter()
                        //                 .find(|p| p.hash == produce_ref.hash);
                        //             (
                        //                 consume_result.0,
                        //                 consume_result.1,
                        //                 p.unwrap_or_else(|| produce_ref.clone()),
                        //             )
                        //         })
                        //     });
                        // result
                        // ```
                        self.handle_match(
                            pc,
                            comms,
                            &channel,
                            &data,
                            persist,
                            &produce_ref,
                            observer,
                            guard,
                        )
                        .map(|matched| {
                            matched.map(|consume_result| {
                                let p = comm
                                    .produces
                                    .into_iter()
                                    .find(|p| p.hash == produce_ref.hash);
                                (
                                    consume_result.0,
                                    consume_result.1,
                                    p.unwrap_or_else(|| produce_ref.clone()),
                                )
                            })
                        })
                    }
                    None => {
                        tracing::trace!(target: "f1r3fly.rspace.ops", channel = ?produce_ref.channel_hash, "replay.produce: stored (recorded COMM exists but no matching consume waiting yet)");
                        super::rspace_interface::commit_produce(guard, || {
                            let result = self.store_data(
                                channel.clone(),
                                data.clone(),
                                persist,
                                produce_ref.clone(),
                            )?;
                            self.log_produce(produce_ref, &channel, &data, persist);
                            Ok(result)
                        })?
                    }
                }
            }
        }
    }

    fn get_comm_or_produce_candidate(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        comms: Vec<COMM>,
        produce_ref: Produce,
        grouped_channels: Vec<Vec<C>>,
    ) -> Result<Option<(COMM, ProduceCandidate<C, P, A, K>)>, RSpaceError> {
        let run_matcher =
            |comm: COMM| -> Result<Option<ProduceCandidate<C, P, A, K>>, RSpaceError> {
                self.run_matcher_produce(
                    channel.clone(),
                    data.clone(),
                    persist,
                    comm,
                    produce_ref.clone(),
                    grouped_channels.clone(),
                )
            };

        self.get_comm_or_candidate(comms, run_matcher)
    }

    fn run_matcher_produce(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        comm: COMM,
        produce_ref: Produce,
        grouped_channels: Vec<Vec<C>>,
    ) -> Result<Option<ProduceCandidate<C, P, A, K>>, RSpaceError> {
        self.run_matcher_for_channels(
            grouped_channels,
            |channels| {
                // THE canonical candidate order, as in the play space — the
                // `extract_first_match` this feeds now makes a guard-aware
                // selection, so the two sides must enumerate alike (see
                // `run_matcher_consume`).
                let continuations = self.get_store().get_continuations_arc(&channels)?;
                Ok(order_candidates_with_index(continuations)
                    .into_iter()
                    .filter(|(wc, _)| comm.consume == wc.source)
                    .collect::<Vec<_>>())
            },
            |c| {
                let store_data = self.get_store().get_data(&c)?;
                let datum_tuples = order_candidates_with_index(store_data);

                let mut result = datum_tuples;
                if c == channel {
                    result.insert(
                        0,
                        (
                            Datum {
                                a: Arc::clone(&data),
                                persist,
                                source: produce_ref.clone(),
                            },
                            -1,
                        ),
                    );
                }

                Ok((
                    c.clone(),
                    result
                        .into_iter()
                        .filter(|(datum, _)| {
                            comm.produces.contains(&datum.source) &&
                                (datum.persist ||
                                    *comm.times_repeated.get(&datum.source).unwrap_or(&0) ==
                                        self.pending_produce_count(
                                            &datum.source,
                                            &produce_ref,
                                            persist,
                                        ))
                        })
                        .collect(),
                ))
            },
        )
    }

    fn matches(&self, comm: COMM, datum_with_index: (Datum<A>, i32)) -> bool {
        let datum = datum_with_index.0;
        let x = comm.produces.contains(&datum.source);

        x && self.was_repeated_enough_times(comm, datum)
    }

    fn was_repeated_enough_times(&self, comm: COMM, datum: Datum<A>) -> bool {
        if !datum.persist {
            let x = *comm.times_repeated.get(&datum.source).unwrap_or(&0) ==
                self.get_produce_count(&datum.source);
            x
        } else {
            true
        }
    }

    fn pending_produce_count(&self, source: &Produce, pending: &Produce, persist: bool) -> i32 {
        self.get_produce_count(source) + i32::from(!persist && source == pending)
    }

    fn handle_match(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
        comms: Vec<COMM>,
        channel: &C,
        data: &A,
        produced_persistently: bool,
        produce_ref: &Produce,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let ProduceCandidate {
            channels,
            continuation,
            continuation_index,
            data_candidates,
            commit,
        } = pc;

        let WaitingContinuation {
            patterns: _patterns,
            continuation: _cont,
            persist,
            peeks,
            source: consume_ref,
        } = &continuation;

        let produce_counters_closure = |produces: &[Produce]| {
            let mut counters = self.produce_counters(produces);
            if !produced_persistently && produces.contains(produce_ref) {
                *counters.entry(produce_ref.clone()).or_insert(0) += 1;
            }
            counters
        };
        let comm_ref = COMM::new(
            &data_candidates,
            consume_ref.clone(),
            peeks.clone(),
            produce_counters_closure,
        );

        assert!(
            comms.contains(&comm_ref),
            "COMM Event {:?} was not contained in the trace {:?}",
            comm_ref,
            comms
        );

        Self::observe_comm(observer, &comm_ref, _cont, *persist, &data_candidates)?;
        for candidate in &data_candidates {
            self.get_store().get_joins(&candidate.channel)?;
        }
        let permit = commit.unwrap_or_default();
        let result = permit.commit_with(guard, || -> Result<_, RSpaceError> {
            if !persist {
                self.get_store()
                    .remove_continuation(&channels, continuation_index)?;
            }
            self.remove_matched_datum_and_join(&channels, &data_candidates)?;
            self.log_produce(produce_ref.clone(), channel, data, produced_persistently);
            self.log_comm(
                &data_candidates,
                &channels,
                continuation.clone(),
                comm_ref.clone(),
                "comm.produce",
            );
            self.mark_replay_waiting_continuation_match();
            self.remove_bindings_for(comm_ref);
            Ok(self.wrap_result(
                channels,
                continuation.clone(),
                consume_ref.clone(),
                data_candidates,
            ))
        })??;
        Ok(result)
    }

    fn start_accounting_operation(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[C],
        joins: &[Vec<C>],
    ) -> Result<Option<Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>, RSpaceError> {
        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
        if let Some(observer) = &observer {
            observer.observe_operation_start(source, channels, joins)?;
        }
        Ok(observer)
    }

    fn observe_comm(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        comm: &COMM,
        continuation: &K,
        continuation_persistent: bool,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        match observer {
            Some(observer) => {
                let data = data_candidates
                    .iter()
                    .map(|candidate| (candidate.datum.a.as_ref(), candidate.datum.persist))
                    .collect::<Vec<_>>();
                observer.observe_comm(comm, continuation, continuation_persistent, &data)
            }
            None => Ok(()),
        }
    }

    fn observe_produce(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        source: &Produce,
        channel: &C,
        data: &A,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        match observer {
            Some(observer) => observer.observe_produce(source, channel, data, persistent),
            None => Ok(()),
        }
    }

    fn observe_consume(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        source: &Consume,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        match observer {
            Some(observer) => observer.observe_consume(
                source,
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
            ),
            None => Ok(()),
        }
    }

    fn remove_bindings_for(&self, comm_ref: COMM) {
        {
            let mut index = self
                .consume_replay_index
                .lock()
                .expect("consume replay index lock");
            if let Some(by_produce) = index.get_mut(&comm_ref.consume.hash) {
                for produce in &comm_ref.produces {
                    if let Some(comms) = by_produce.get_mut(&produce.hash) {
                        if let Some(pos) = comms.iter().position(|c| c == &comm_ref) {
                            comms.remove(pos);
                        }
                        if comms.is_empty() {
                            by_produce.remove(&produce.hash);
                        }
                    }
                }
                if by_produce.is_empty() {
                    index.remove(&comm_ref.consume.hash);
                }
            }
        }
        let replay_data = self.replay_data.lock().expect("replay data lock");
        replay_data.remove_binding_in_place(&IOEvent::Consume(comm_ref.consume.clone()), &comm_ref);

        for produce_ref in comm_ref.produces.iter() {
            replay_data.remove_binding_in_place(&IOEvent::Produce(produce_ref.clone()), &comm_ref);
        }
    }

    pub fn log_comm(
        &self,
        data_candidates: &Vec<ConsumeCandidate<C, A>>,
        channels: &Vec<C>,
        wk: WaitingContinuation<P, K>,
        comm: COMM,
        label: &str,
    ) {
        // Record metrics using constants to avoid memory leaks
        let metric_label = match label {
            "comm.consume" => CONSUME_COMM_LABEL,
            "comm.produce" => PRODUCE_COMM_LABEL,
            _ => {
                // This should never happen, but guards against future errors.
                tracing::warn!("log_comm called with unexpected dynamic label: {}", label);
                "" // Return an empty string to avoid creating a metric
            }
        };

        if !metric_label.is_empty() {
            metrics::counter!(metric_label, "source" => REPLAY_RSPACE_METRICS_SOURCE).increment(1);
        }

        // Call logger for reporting events
        if let Ok(logger_guard) = self.logger.lock() {
            logger_guard.log_comm(data_candidates, channels, wk, comm, label);
        }
    }

    pub fn log_consume(
        &self,
        consume_ref: Consume,
        channels: &Vec<C>,
        patterns: &Vec<P>,
        continuation: &K,
        persist: bool,
        peeks: &BTreeSet<i32>,
    ) {
        // Call logger for reporting events
        if let Ok(logger_guard) = self.logger.lock() {
            logger_guard.log_consume(consume_ref, channels, patterns, continuation, persist, peeks);
        }
    }

    pub fn log_produce(&self, produce_ref: Produce, channel: &C, data: &A, persist: bool) {
        self.notify_produce(produce_ref.clone(), channel, data, persist);
        self.increment_produce_counter(&produce_ref, persist);
    }

    fn notify_produce(&self, produce_ref: Produce, channel: &C, data: &A, persist: bool) {
        // Call logger for reporting events
        if let Ok(logger_guard) = self.logger.lock() {
            logger_guard.log_produce(produce_ref, channel, data, persist);
        }
    }

    fn increment_produce_counter(&self, produce_ref: &Produce, persist: bool) {
        if !persist {
            let mut counter = self.produce_counter.lock().expect("produce counter lock");
            let current = counter.get(produce_ref).copied().unwrap_or(0);
            counter.insert(produce_ref.clone(), current + 1);
        }
    }

    // Disabled by the merge of feature/cost-accounted-rho (plan step 1.3):
    // theirs counts a produce when the operation starts and calls this
    // rollback after a failed match. Here a produce is counted only inside
    // the commit closures (`log_produce` in `commit_produce` and
    // `permit.commit_with`), and the COMM check adds the pending count
    // itself (`produce_counters_closure` in `handle_match`). A rollback
    // would undo a count that never happened, and the `times_repeated`
    // check would then reject valid blocks.
    // ```text
    // fn rollback_produce_counter(&self, produce_ref: &Produce, persist: bool) {
    //     if !persist {
    //         let mut counter = self.produce_counter.lock().expect("produce counter lock");
    //         match counter.get(produce_ref).copied().unwrap_or(0) {
    //             0 => {}
    //             1 => {
    //                 counter.remove(produce_ref);
    //             }
    //             count => {
    //                 counter.insert(produce_ref.clone(), count - 1);
    //             }
    //         }
    //     }
    // }
    // ```

    fn get_comm_or_candidate<Candidate>(
        &self,
        comms: Vec<COMM>,
        run_matcher: impl Fn(COMM) -> Result<Option<Candidate>, RSpaceError>,
    ) -> Result<Option<(COMM, Candidate)>, RSpaceError> {
        for comm in comms {
            if let Some(candidate) = run_matcher(comm.clone())? {
                return Ok(Some((comm, candidate)));
            }
        }
        Ok(None)
    }

    // This function may need to clear 'replay_data'
    pub fn spawn(&self) -> Result<Self, RSpaceError>
    where
        C: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        P: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        A: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        K: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
    {
        self.spawn_for_epoch(self.epoch)
    }

    pub fn spawn_for_epoch(
        &self,
        epoch: crate::rspace::hashing::stable_hash_provider::EventCodecEpoch,
    ) -> Result<Self, RSpaceError>
    where
        C: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        P: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        A: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        K: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
    {
        // Span[F].withMarks("spawn") from Scala - works because this is NOT
        // async
        let _span = tracing::info_span!(target: "f1r3fly.rspace", "spawn").entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-spawn", "spawn");

        let history_repo = self.get_history_repository();
        let next_history = history_repo.reset(&history_repo.root())?;
        let history_reader =
            next_history.get_history_reader_for_epoch(&next_history.root(), epoch)?;
        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        let rspace = Self::apply_for_epoch(
            epoch,
            Arc::new(next_history),
            Arc::new(hot_store),
            self.matcher.clone(),
        );
        rspace.restore_installs()?;

        // Mark the completion of spawn operation
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-spawn", "spawn");
        Ok(rspace)
    }

    /* RSpaceOps */

    fn store_waiting_continuation(
        &self,
        channels: Vec<C>,
        wc: WaitingContinuation<P, K>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        self.get_store().get_continuations(&channels)?;
        for channel in &channels {
            self.get_store().get_joins(channel)?;
        }
        if self.get_store().put_continuation(&channels, wc.clone())? {
            self.inc_replay_waiting_continuations(&channels)?;
        }
        for channel in channels.iter() {
            self.get_store().put_join(channel, &channels)?;
        }
        Ok(None)
    }

    fn store_data(
        &self,
        channel: C,
        data: Arc<A>,
        persist: bool,
        produce_ref: Produce,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        self.get_store().put_datum(&channel, Datum {
            a: data,
            persist,
            source: produce_ref,
        })?;

        Ok(None)
    }

    fn store_persistent_data(
        &self,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        // Highest index first: removing an earlier same-channel datum would
        // shift the indices of the remaining candidates.
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
        let store = self.get_store();
        for consume_candidate in sorted_candidates {
            if !consume_candidate.datum.persist {
                store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index)?;
            }
        }
        Ok(())
    }

    fn restore_installs(&self) -> Result<(), RSpaceError> {
        let installs = self.installs.lock().unwrap().clone();
        for (channels, install) in installs {
            self.locked_install_internal(channels, install.patterns, install.continuation)?;
        }
        Ok(())
    }

    fn locked_install_internal(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError> {
        if channels.len() != patterns.len() {
            Err(RSpaceError::BugFoundError(
                "RUST ERROR: channels.length must equal patterns.length".to_string(),
            ))
        } else {
            let consume_ref =
                Consume::create_for_epoch(self.epoch, &channels, &patterns, &continuation, true)?;
            let mut channel_to_indexed_data = self.fetch_channel_to_index_data(&channels)?;
            // Borrow-zip: no per-install channel/pattern clones.
            let zipped: Vec<(&C, &P)> = channels.iter().zip(patterns.iter()).collect();
            let options: Option<Vec<ConsumeCandidate<C, A>>> = self
                .extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
                .into_iter()
                .collect();

            match options {
                None => {
                    self.installs
                        .lock()
                        .unwrap()
                        .insert(channels.clone(), Install {
                            patterns: patterns.clone(),
                            continuation: continuation.clone(),
                        });

                    let store = self.get_store();
                    store.install_continuation(&channels, WaitingContinuation {
                        patterns: Arc::new(patterns),
                        continuation: Arc::new(continuation),
                        persist: true,
                        peeks: BTreeSet::default(),
                        source: consume_ref,
                    });

                    for channel in channels.iter() {
                        store.install_join(channel, &channels);
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
        history_reader: crate::rspace::history::history_reader::EpochHistoryReader<C, P, A, K>,
    ) where
        C: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        P: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        A: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
        K: crate::rspace::serializers::cold_store_decode::ColdStoreDecode,
    {
        let next_hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        *self.store.write().expect("store write lock") = Arc::new(next_hot_store);
    }

    // The public result stays value-shaped — the single per-fired-COMM
    // materialization boundary (`Arc::unwrap_or_clone` takes the value when
    // uniquely owned, clones otherwise — same semantics as the earlier
    // value-shaped moves/clones).
    fn wrap_result(
        &self,
        channels: Vec<C>,
        wk: WaitingContinuation<P, K>,
        _consume_ref: Consume,
        data_candidates: Vec<ConsumeCandidate<C, A>>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let cont_result = ContResult {
            continuation: Arc::unwrap_or_clone(wk.continuation),
            persistent: wk.persist,
            channels,
            patterns: Arc::unwrap_or_clone(wk.patterns),
            peek: !wk.peeks.is_empty(),
        };

        let rspace_results = data_candidates
            .into_iter()
            .map(|data_candidate| RSpaceResult {
                channel: data_candidate.channel,
                matched_datum: Arc::unwrap_or_clone(data_candidate.datum.a),
                removed_datum: Arc::unwrap_or_clone(data_candidate.removed_datum),
                persistent: data_candidate.datum.persist,
            })
            .collect();

        Some((cont_result, rspace_results))
    }

    fn remove_matched_datum_and_join(
        &self,
        channels: &[C],
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        // Preserve the same-channel candidate indices until each removal.
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
        let store = self.get_store();
        for consume_candidate in sorted_candidates {
            let channel = &consume_candidate.channel;
            let datum_index = consume_candidate.datum_index;

            if datum_index >= 0 && !consume_candidate.datum.persist {
                store.remove_datum(channel, datum_index)?;
            }
            store.remove_join(channel, channels)?;
        }
        Ok(())
    }

    fn run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        ) -> Result<
            Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
            RSpaceError,
        >,
        fetch_matching_data: impl Fn(C) -> Result<(C, Vec<(Datum<A>, i32)>), RSpaceError>,
    ) -> Result<MaybeProduceCandidate<C, P, A, K>, RSpaceError> {
        let mut remaining = grouped_channels;

        loop {
            match remaining.split_first() {
                Some((channels, rest)) => {
                    let match_candidates = fetch_matching_continuations(channels.to_vec())?;
                    let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
                        .iter()
                        .map(|c| fetch_matching_data(c.clone()))
                        .collect::<Result<_, _>>()?;

                    let first_match = self.extract_first_match(
                        &self.matcher,
                        channels.to_vec(),
                        match_candidates,
                        channel_to_indexed_data,
                    );

                    match first_match {
                        Some(produce_candidate) => return Ok(Some(produce_candidate)),
                        None => remaining = rest.to_vec(),
                    }
                }
                None => {
                    return Ok(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Regression guard for the phase_a_locks/phase_b_locks fix ported from
    // RSpace (PR #72 "Per-channel locks → fixed striped locks") — see
    // f1r3node-rust#43. Before this fix, ReplayRSpace still used a growing
    // DashMap<u64, Arc<Mutex>> (the pre-#72 pattern RSpace moved away from),
    // which PR #72 explicitly flagged as an untouched residual: "Replay path
    // still times out at forks >= 16". Mirrors
    // `par_branch_event_log_does_not_contend_at_rholang_par_scale` in
    // rspace.rs, but drives ReplayRSpace::produce() so it exercises
    // produce_lock/phase_a_locks/phase_b_locks instead of the event log.

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::rspace::r#match::Match;
    use crate::rspace::rspace::RSpace;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    #[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
    struct Wildcard;

    impl crate::rspace::hashing::stable_hash_provider::StableHashSerialize for Wildcard {}

    impl crate::rspace::serializers::cold_store_decode::ColdStoreDecode for Wildcard {
        fn cold_decode_prefix(
            bytes: &[u8],
        ) -> Result<
            (Self, usize),
            crate::rspace::serializers::cold_store_decode::ColdStoreDecodeError,
        > {
            crate::rspace::serializers::cold_store_decode::legacy_prefix(bytes)
        }
    }

    struct AlwaysMatch;

    impl Match<Wildcard, String, String> for AlwaysMatch {
        fn get(&self, _: &Wildcard, a: &String) -> Option<String> { Some(a.clone()) }
    }

    async fn make_replay_rspace() -> ReplayRSpace<String, Wildcard, String, String> {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let (_play, replay) = RSpace::<String, Wildcard, String, String>::create_with_replay(
            store,
            Arc::new(Box::new(AlwaysMatch)),
        )
        .unwrap();
        replay
    }

    // 32 par-branches, each calling produce() on its own private channels —
    // mirrors the rholang-par benchmark's replay side. Regression guard for
    // the documented symptom "replay path still times out at forks >= 16"
    // (PR #72's own follow-up list, f1r3node-rust#43): before this fix,
    // ReplayRSpace acquired phase locks through an unbounded
    // DashMap<u64, Arc<Mutex>> (the pre-#72 pattern RSpace already moved
    // away from). A solo-vs-concurrent speedup ratio isn't a reliable
    // assertion here — on the in-memory test store a single produce() is
    // low-single-digit microseconds, so tokio task-spawn/scheduling
    // overhead dominates the timing at that scale and swamps any lock
    // signal. What's actually being guarded is forward progress: this
    // bound (30s for 16,000 ops) is generous by roughly two orders of
    // magnitude relative to observed run time, so it fails on a hang or
    // reintroduced serialization, not on ordinary timing noise.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn par_branch_replay_produce_completes_at_rholang_par_scale() {
        const PAR_BRANCHES: usize = 32;
        const OPS_PER_BRANCH: usize = 500;
        const MAX_WALL_CLOCK: std::time::Duration = std::time::Duration::from_secs(30);

        let rspace = make_replay_rspace().await;

        let t0 = std::time::Instant::now();
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
        let elapsed = t0.elapsed();

        eprintln!(
            "replay_par_branch_scaling: branches={PAR_BRANCHES}  ops_per_branch={OPS_PER_BRANCH}  \
             elapsed={:.3}s  (bound {:.0}s)",
            elapsed.as_secs_f64(),
            MAX_WALL_CLOCK.as_secs_f64(),
        );

        assert!(
            elapsed < MAX_WALL_CLOCK,
            "ReplayRSpace produce() at {PAR_BRANCHES} par-branches took {:.2}s (bound {:.0}s) — \
             regression back to unbounded per-channel locking, the pre-#72 pattern that made \
             replay time out at forks >= 16.",
            elapsed.as_secs_f64(),
            MAX_WALL_CLOCK.as_secs_f64(),
        );
    }
}
