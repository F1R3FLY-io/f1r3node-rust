// Construction and wiring: history repository + hot store creation, the
// play/replay pairing, and spawn.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{RSpace, RSpaceStore};
use crate::rspace::errors::{HistoryRepositoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::history::history_repository::{HistoryRepository, HistoryRepositoryInstances};
use crate::rspace::hot_store::{HotStore, HotStoreInstances};
use crate::rspace::logging::BasicLogger;
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::striped_locks;

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
    ) -> RSpace<C, P, A, K> {
        RSpace {
            history_repository: Arc::new(std::sync::RwLock::new(history_repository)),
            store: Arc::new(std::sync::RwLock::new(Arc::new(store))),
            matcher,
            installs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            event_log: Arc::new(std::sync::Mutex::new(Vec::new())),
            produce_counter: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            phase_a_locks: striped_locks::new_striped_locks(),
            phase_b_locks: striped_locks::new_striped_locks(),
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

    pub(super) fn create_new_hot_store(
        &self,
        history_reader: Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>,
    ) {
        let next_hot_store = HotStoreInstances::create_from_hr(history_reader.base());
        *self.store.write().expect("store write lock") = Arc::new(next_hot_store);
    }
}
