// Construction and wiring: history repository + hot store creation, the
// play/replay pairing, and spawn.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use serde::Serialize;

use super::{RSpace, RSpaceStore};
use crate::rspace::errors::{HistoryRepositoryError, RSpaceError};
// Disabled by the merge of feature/cost-accounted-rho (lint fix): F1R3Lang's
// `EpochHistoryReader` replaced the `Box<dyn HistoryReader<Blake2b256Hash, ..>>`
// that named this type, so it was unused at HEAD too.
// use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use crate::rspace::history::history_reader::EpochHistoryReader;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::hot_store::{HotStore, HotStoreInstances};
use crate::rspace::logging::BasicLogger;
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::striped_locks;

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
{
    /**
     * Creates [[RSpace]] from [[HistoryRepository]] and [[HotStore]].
     */
    pub fn apply(
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Box<dyn HotStore<C, P, A, K>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> RSpace<C, P, A, K> {
        Self::apply_for_epoch(EventCodecEpoch::Legacy, history_repository, store, matcher)
    }

    pub fn apply_for_epoch(
        epoch: EventCodecEpoch,
        history_repository: Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>,
        store: Box<dyn HotStore<C, P, A, K>>,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> RSpace<C, P, A, K> {
        RSpace {
            epoch,
            history_repository: Arc::new(std::sync::RwLock::new(history_repository)),
            store: Arc::new(arc_swap::ArcSwap::new(Arc::new(store))),
            matcher,
            accounting_observer: Arc::new(std::sync::RwLock::new(None)),
            installs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_log: Arc::new(std::sync::Mutex::new(Vec::new())),
            ordered_event_log: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            event_log_reservations: Arc::new(std::sync::Mutex::new(
                super::trace_log::EventLogReservations::default(),
            )),
            resting_checkpoint_barrier: Arc::new(tokio::sync::RwLock::new(())),
            produce_counter: Arc::new(
                (0..striped_locks::NUM_LOCK_STRIPES)
                    .map(|_| std::sync::Mutex::new(BTreeMap::new()))
                    .collect(),
            ),
            phase_a_locks: striped_locks::new_striped_locks(),
            phase_b_locks: striped_locks::new_striped_locks(),
            step_observer: None,
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
            + ColdStoreDecode
            + 'static,
        P: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        A: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
        K: Clone + Debug + Default + Send + Sync + Serialize + ColdStoreDecode + 'static,
    {
        Self::create_for_epoch(EventCodecEpoch::Legacy, store, matcher)
    }

    pub fn create_for_epoch(
        epoch: EventCodecEpoch,
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> Result<RSpace<C, P, A, K>, HistoryRepositoryError>
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
    {
        let setup = Self::create_history_repo_for_epoch(epoch, store)?;
        let (history_reader, store) = setup;
        let space = Self::apply_for_epoch(epoch, Arc::new(history_reader), store, matcher);
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
        Self::create_with_replay_for_epoch(EventCodecEpoch::Legacy, store, matcher)
    }

    pub fn create_with_replay_for_epoch(
        epoch: EventCodecEpoch,
        store: RSpaceStore,
        matcher: Arc<Box<dyn Match<P, A, K>>>,
    ) -> Result<(RSpace<C, P, A, K>, ReplayRSpace<C, P, A, K>), HistoryRepositoryError>
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
    {
        let setup = Self::create_history_repo_for_epoch(epoch, store)?;
        let (history_repo, store) = setup;
        let history_repo_arc = Arc::new(history_repo);

        // Play
        let space = Self::apply_for_epoch(epoch, history_repo_arc.clone(), store, matcher.clone());
        // Replay
        let history_reader =
            history_repo_arc.get_history_reader_for_epoch(&history_repo_arc.root(), epoch)?;
        let replay_store = HotStoreInstances::create_from_hr(history_reader.base());
        let replay = ReplayRSpace::apply_with_logger_for_epoch(
            epoch,
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
        Self::create_history_repo_for_epoch(EventCodecEpoch::Legacy, store)
    }

    pub fn create_history_repo_for_epoch(
        epoch: EventCodecEpoch,
        store: RSpaceStore,
    ) -> Result<
        (
            Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
            Box<dyn HotStore<C, P, A, K>>,
        ),
        HistoryRepositoryError,
    >
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
    {
        let history_repo =
            HistoryRepositoryInstances::lmdb_repository(store.history, store.roots, store.cold)?;

        let history_reader =
            history_repo.get_history_reader_for_epoch(&history_repo.root(), epoch)?;

        let hot_store = HotStoreInstances::create_from_hr(history_reader.base());

        Ok((history_repo, hot_store))
    }

    pub fn spawn(&self) -> Result<Self, RSpaceError>
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
    {
        self.spawn_for_epoch(self.epoch)
    }

    pub fn spawn_for_epoch(&self, epoch: EventCodecEpoch) -> Result<Self, RSpaceError>
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
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
        let rspace =
            RSpace::apply_for_epoch(epoch, Arc::new(next_history), hot_store, self.matcher.clone());
        rspace.restore_installs()?;

        // Mark the completion of spawn operation
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-spawn", "spawn");
        Ok(rspace)
    }

    pub(super) fn create_new_hot_store(&self, history_reader: EpochHistoryReader<C, P, A, K>)
    where
        C: ColdStoreDecode,
        P: ColdStoreDecode,
        A: ColdStoreDecode,
        K: ColdStoreDecode,
    {
        let next_hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        // No in-flight produce/consume can be concurrent with this swap —
        // callers already only clone the store Arc and drop any lock
        // immediately, so a reader that started before this swap keeps
        // running against the old store either way.
        self.store.store(Arc::new(next_hot_store));
    }
}
