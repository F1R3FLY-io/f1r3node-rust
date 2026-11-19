use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;

use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use crate::rspace::errors::{HistoryError, HistoryRepositoryError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::checkpoint_writer::KvCheckpointWriter;
use crate::rspace::history::history::{History, HistoryInstances};
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::history::history_repository_impl::HistoryRepositoryImpl;
use crate::rspace::history::prune_guard::PruneGuard;
use crate::rspace::history::root_repository::RootRepository;
use crate::rspace::history::roots_store::RootsStoreInstances;
use crate::rspace::hot_store_action::HotStoreAction;
use crate::rspace::hot_store_trie_action::HotStoreTrieAction;
use crate::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use crate::rspace::state::instances::rspace_importer_store::RSpaceImporterStore;
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

// See rspace/src/main/scala/coop/rchain/rspace/history/HistoryRepository.scala
pub trait HistoryRepository<C: Clone, P: Clone, A: Clone, K: Clone>: Send + Sync {
    fn checkpoint(
        &self,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>;

    fn do_checkpoint(
        &self,
        actions: Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>;

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError>;

    fn history(&self) -> Arc<Mutex<Box<dyn History>>>;

    fn exporter(&self) -> Arc<dyn RSpaceExporter>;

    fn importer(&self) -> Arc<dyn RSpaceImporter>;

    fn get_history_reader(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>, HistoryError>;

    fn get_history_reader_struct(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<RSpaceHistoryReaderImpl<C, P, A, K>, HistoryError>;

    fn root(&self) -> Blake2b256Hash;

    /// Record a root hash in the roots store so that subsequent `reset` calls
    /// can find it via `validate_root`. This is needed during
    /// LFS bootstrap to register `emptyStateHashFixed` before genesis replay.
    fn record_root(&self, root: &Blake2b256Hash) -> Result<(), HistoryError>;

    /// Pure lookup: returns true if the root is recorded in the store.
    /// Companion to `record_root` for joiner-side LFS sync to check whether
    /// a root has already been imported before fetching from peers.
    fn contains_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError>;

    /// Raw access to the LMDB-backed trie-node store. Used by rspace-history
    /// pruning (issue #923, Option D) to walk every node hash in the env
    /// via `scan_page` + `delete`, after a mark walk from each retained
    /// root has built the reachable-hash set. Exposes trie nodes only —
    /// cold-store datums/continuations and the roots store are separate
    /// environments and not accessible here.
    fn history_store(&self) -> Arc<dyn KeyValueStore>;

    /// Atomic mark-and-sweep of the trie-node store against `retained_roots`.
    ///
    /// Issue #1061: the implementation holds the rspace-level `PruneGuard`
    /// exclusive side for the whole pass so no runtime `do_checkpoint` or
    /// `reset` can run its content-addressed `contains` + `put` window
    /// while the sweep is deleting. Pre-#1061 the helper leaned on LMDB
    /// per-write serialisation alone and lost nodes to the race between
    /// the writer's `contains` check (which skips keys already in the
    /// store) and its subsequent `put`.
    ///
    /// `retained_roots` is the caller-supplied set of roots whose subtrees
    /// MUST survive. The implementation unions this with its `PruneGuard`
    /// pinned-roots snapshot (recently-committed checkpoints + recent
    /// reset targets) before the mark walk, so roots a writer committed
    /// before the DAG caught up also survive. The casper-side caller
    /// (`prune_history_once`) builds the caller set from
    /// `compute_gc_retention_roots` plus the empty root.
    ///
    /// Returns a pair of counters so operators can see the pass's work.
    fn prune_history_atomic(
        &self,
        retained_roots: &[Blake2b256Hash],
    ) -> Result<HistoryPruneResult, HistoryError>;
}

/// Counters for one atomic prune pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryPruneResult {
    /// Trie nodes reachable from the retained roots at pass time.
    pub marked: usize,
    /// Trie nodes deleted (keys present in the env, not in the mark set).
    pub swept: usize,
}

pub struct HistoryRepositoryInstances<C, P, A, K> {
    _marker: PhantomData<(C, P, A, K)>,
}

pub const PREFIX_DATUM: u8 = 0x00;
pub const PREFIX_KONT: u8 = 0x01;
pub const PREFIX_JOINS: u8 = 0x02;

impl<C, P, A, K> HistoryRepositoryInstances<C, P, A, K>
where
    C: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    P: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    A: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    K: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
{
    pub fn lmdb_repository(
        history_key_value_store: Arc<dyn KeyValueStore>,
        roots_key_value_store: Arc<dyn KeyValueStore>,
        cold_key_value_store: Arc<dyn KeyValueStore>,
    ) -> Result<
        Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>,
        HistoryRepositoryError,
    > {
        // Roots store
        let roots_repository = RootRepository {
            roots_store: Box::new(RootsStoreInstances::roots_store(roots_key_value_store.clone())),
        };

        let current_root = roots_repository.current_root()?;

        tracing::debug!(
            "[HistoryRepository] lmdbRepository initialized with root={}",
            current_root
        );

        // History store
        let history = HistoryInstances::create(current_root, history_key_value_store.clone())?;

        // Cold store
        // let cold_store =
        // ColdStoreInstances::cold_store(cold_key_value_store.clone());

        // RSpace importer/exporter / directly operates on Store (lmdb)
        let exporter = RSpaceExporterStore::create(
            history_key_value_store.clone(),
            cold_key_value_store.clone(),
            roots_key_value_store.clone(),
        );
        let importer = RSpaceImporterStore::create(
            history_key_value_store.clone(),
            cold_key_value_store.clone(),
            roots_key_value_store.clone(),
        );

        Ok(Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(Box::new(history))),
            roots_repository: Arc::new(Mutex::new(roots_repository)),
            checkpoint_writer: Arc::new(KvCheckpointWriter {
                history: history_key_value_store.clone(),
                roots: roots_key_value_store,
            }),
            leaf_store: cold_key_value_store,
            history_kv_store: history_key_value_store,
            rspace_exporter: Arc::new(exporter),
            rspace_importer: Arc::new(importer),
            prune_guard: Arc::new(PruneGuard::new()),
            _marker: PhantomData,
        }))
    }
}
