use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;

use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use crate::rspace::errors::{HistoryError, HistoryRepositoryError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::checkpoint_writer::KvCheckpointWriter;
use crate::rspace::history::cold_gc::{AuditReport, ColdGc, NoopColdGc, SweepReport};
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

    /// Cold-store GC retention-set advance (issue #1181).
    ///
    /// Called by the casper GC driver immediately BEFORE
    /// `prune_history_atomic`. The implementation delegates to its
    /// `ColdGc`, which diffs `new_retention_set` against its persisted
    /// record of the previous pass's retention set, invokes `on_expire`
    /// for each retiring root, and persists the new set atomically.
    ///
    /// Returns the retiring root hashes for driver-side observability.
    /// A `NoopColdGc` implementation returns an empty Vec and performs
    /// no side effects.
    ///
    /// Must run BEFORE `prune_history_atomic` so each retiring root's
    /// trie nodes are still readable when `on_expire` walks them.
    fn cold_gc_advance_retention(
        &self,
        new_retention_set: &[Blake2b256Hash],
    ) -> Result<Vec<Blake2b256Hash>, HistoryError>;

    /// Audit the cold-GC refcount invariant (issue #1181, Phase 2).
    /// Reads the implementation's retention-set record, mark-walks
    /// every root in it, and compares the resulting leaf counts
    /// against the refcount store.
    ///
    /// Returns an [`AuditReport`][crate::rspace::history::cold_gc::AuditReport]
    /// with divergence counts and a bounded sample of specific
    /// divergences for operator debugging. A `NoopColdGc`
    /// implementation returns an empty report and performs no walks.
    ///
    /// Intended as a periodic canary; run frequency is up to the
    /// casper driver. Can be costly on a large retention window
    /// because it walks every retained root.
    fn cold_gc_audit(&self) -> Result<AuditReport, HistoryError>;

    /// Delete every entry in the cold leaf store whose key no
    /// retained root references (issue #1181, Phase 4). Operates in
    /// place: LMDB does not shrink the file, but freed pages are
    /// reused by future writes, so the cold store stops growing
    /// unboundedly. Returns a [`SweepReport`] for driver-side
    /// metrics; a `NoopColdGc` implementation returns the default
    /// (zero) report.
    ///
    /// Must be called only when the cold-GC retention state is
    /// consistent (an `advance_retention` pass with a non-empty
    /// retention set has run at least once). The implementation
    /// enforces this: an empty retention store causes a skipped
    /// report rather than a destructive sweep.
    fn cold_gc_sweep(&self) -> Result<SweepReport, HistoryError>;
}

/// Counters for one atomic prune pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryPruneResult {
    /// Trie nodes reachable from the retained roots at pass time.
    pub marked: usize,
    /// Trie nodes deleted (keys present in the env, not in the mark set).
    pub swept: usize,
    /// Issue #1098 observability. Pinned roots in the initial snapshot,
    /// after age-based eviction at `begin_pass`.
    pub pins_initial: usize,
    /// Pinned roots evicted by age at the start of this pass.
    pub pins_evicted: usize,
    /// Pinned roots that appeared between delete batches during the pass
    /// (writer committed a checkpoint mid-sweep). Each such root triggers
    /// an under-exclusive DFS into the mark set.
    pub pins_mid_pass_added: usize,
    /// Trie nodes reached by walking from the initial-snapshot pins alone.
    /// Compare with `marked` to see how much of the mark set the pins
    /// contribute vs. the caller's DAG walk.
    pub marked_from_pins: usize,
    /// Number of delete batches (brief exclusive-lock windows) the pass
    /// executed. A high count with low per-batch stalls is the shape the
    /// #1098 fix aims for; the pre-fix code held one long batch instead.
    pub batches: usize,
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
        Self::lmdb_repository_with_cold_gc(
            history_key_value_store,
            roots_key_value_store,
            cold_key_value_store,
            Arc::new(NoopColdGc),
        )
    }

    /// Construct the repository with an explicit `ColdGc` implementation.
    /// Callers that opt into cold-store GC bookkeeping (issue #1181)
    /// pass an `LmdbColdGc` wired to dedicated sub-DBs; everyone else
    /// uses [`lmdb_repository`][Self::lmdb_repository] which delegates
    /// here with a `NoopColdGc`.
    pub fn lmdb_repository_with_cold_gc(
        history_key_value_store: Arc<dyn KeyValueStore>,
        roots_key_value_store: Arc<dyn KeyValueStore>,
        cold_key_value_store: Arc<dyn KeyValueStore>,
        cold_gc: Arc<dyn ColdGc>,
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

        // Adopt the cold-GC implementation's PruneGuard if it has
        // one, so `do_checkpoint`'s `enter_write` and the sweep's
        // `enter_sweep` fence against the SAME lock. Without this,
        // the sweep would hold a lock the checkpoint writer does
        // not acquire, letting a fresh `leaf_store.put_if_absent`
        // race the sweep's delete batch (issue #1181, Phase 4 race
        // fix). `NoopColdGc` returns `None` and the repository
        // falls back to a private guard.
        let prune_guard = cold_gc
            .prune_guard()
            .unwrap_or_else(|| Arc::new(PruneGuard::new()));

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
            prune_guard,
            cold_gc,
            _marker: PhantomData,
        }))
    }
}
