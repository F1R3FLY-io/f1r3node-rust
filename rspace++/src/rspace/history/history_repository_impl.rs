use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;
use tracing::{Level, debug};

use super::cold_store::{ContinuationsLeaf, DataLeaf, JoinsLeaf};
use super::history_action::{DeleteAction, HistoryAction, InsertAction};
use super::history_reader::HistoryReader;
use super::history_repository::{HistoryPruneResult, PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use super::radix_tree::{Item, decode};
use crate::rspace::errors::{HistoryError, RadixTreeError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use crate::rspace::history::checkpoint_writer::CheckpointWriter;
use crate::rspace::history::cold_store::PersistedData;
use crate::rspace::history::history::History;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::history::root_repository::RootRepository;
use crate::rspace::hot_store_action::DeleteAction::{DeleteContinuations, DeleteData, DeleteJoins};
use crate::rspace::hot_store_action::HotStoreAction;
use crate::rspace::hot_store_action::InsertAction::{InsertContinuations, InsertData, InsertJoins};
use crate::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieDeleteAction, TrieDeleteConsume, TrieDeleteJoins, TrieDeleteProduce,
    TrieInsertAction, TrieInsertConsume, TrieInsertJoins, TrieInsertProduce,
};
use crate::rspace::metrics_constants::{
    HISTORY_CHECKPOINT_ACTIONS_METRIC, HISTORY_CHECKPOINT_HISTORY_LOCK_WAIT_TIME_METRIC,
    HISTORY_CHECKPOINT_HISTORY_PROCESS_TIME_METRIC, HISTORY_CHECKPOINT_LEAF_WRITE_TIME_METRIC,
    HISTORY_CHECKPOINT_PARTITION_TIME_METRIC, HISTORY_CHECKPOINT_ROOT_COMMIT_TIME_METRIC,
    HISTORY_CHECKPOINT_SERIALIZE_TIME_METRIC, HISTORY_CHECKPOINT_SERIALIZED_BYTES_METRIC,
    HISTORY_CHECKPOINT_STORAGE_ACTIONS_TIME_METRIC, HISTORY_CHECKPOINT_TIME_METRIC,
    HISTORY_LOCK_CHECKPOINT_SITE, HISTORY_LOCK_READER_SITE, HISTORY_LOCK_RESET_SITE,
    HISTORY_LOCK_ROOT_SITE, HISTORY_REPO_CURRENT_HISTORY_LOCK_CALLS_METRIC,
    HISTORY_REPO_CURRENT_HISTORY_LOCK_WAIT_NS_METRIC, HISTORY_REPO_ROOTS_LOCK_CALLS_METRIC,
    HISTORY_REPO_ROOTS_LOCK_WAIT_NS_METRIC, HISTORY_RSPACE_METRICS_SOURCE, LockSiteMetrics,
    ROOTS_LOCK_CONTAINS_ROOT_SITE, ROOTS_LOCK_RECORD_ROOT_SITE, ROOTS_LOCK_RESET_SITE,
};
use crate::rspace::serializers::serializers::{encode_continuations, encode_datums, encode_joins};
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

// See rspace/src/main/scala/coop/rchain/rspace/history/HistoryRepositoryImpl.
// scala
pub struct HistoryRepositoryImpl<C, P, A, K> {
    pub current_history: Arc<Mutex<Box<dyn History>>>,
    pub roots_repository: Arc<Mutex<RootRepository>>,
    pub checkpoint_writer: Arc<dyn CheckpointWriter>,
    pub leaf_store: Arc<dyn KeyValueStore>,
    /// The LMDB-backed trie-node store (`rspace/history`). Carried here so
    /// `HistoryRepository::history_store` can hand it out to the
    /// rspace-history pruner without reaching through `current_history`'s
    /// mutex. Shared by reference across checkpoints — a fresh
    /// `HistoryRepositoryImpl` from `checkpoint` / `do_checkpoint` / `reset`
    /// keeps the same underlying store.
    pub history_kv_store: Arc<dyn KeyValueStore>,
    pub rspace_exporter: Arc<dyn RSpaceExporter>,
    pub rspace_importer: Arc<dyn RSpaceImporter>,
    pub _marker: PhantomData<(C, P, A, K)>,
}

type ColdAction = (Blake2b256Hash, Option<PersistedData>);
const CHECKPOINT_PARALLEL_ACTIONS_THRESHOLD: usize = 256;

// Timed lock helpers (issue-146/147: measuring whether these blocking
// `std::sync::Mutex`es serialize concurrent PRECHARGE/REFUND system-deploy
// execution the way the now-removed LmdbKeyValueStore mutex used to
// serialize history reads).
struct TimedGuard<'a, T: ?Sized> {
    guard: Option<MutexGuard<'a, T>>,
    acquired: Instant,
    site: &'static LockSiteMetrics,
}

impl<T: ?Sized> Deref for TimedGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.guard
            .as_deref()
            .expect("TimedGuard holds its lock until drop")
    }
}

impl<T: ?Sized> Drop for TimedGuard<'_, T> {
    fn drop(&mut self) {
        let held = self.acquired.elapsed();
        drop(self.guard.take());
        metrics::counter!(self.site.hold_ns, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .increment(held.as_nanos() as u64);
    }
}

fn timed_lock<'a, T: ?Sized>(
    m: &'a Mutex<T>,
    wait_metric: &'static str,
    calls_metric: &'static str,
    site: &'static LockSiteMetrics,
    message: &str,
) -> TimedGuard<'a, T> {
    let start = Instant::now();
    let guard = m.lock().expect(message);
    let acquired = Instant::now();
    let wait_ns = acquired.duration_since(start).as_nanos() as u64;
    metrics::counter!(wait_metric, "source" => HISTORY_RSPACE_METRICS_SOURCE).increment(wait_ns);
    metrics::counter!(calls_metric, "source" => HISTORY_RSPACE_METRICS_SOURCE).increment(1);
    metrics::counter!(site.wait_ns, "source" => HISTORY_RSPACE_METRICS_SOURCE).increment(wait_ns);
    metrics::counter!(site.calls, "source" => HISTORY_RSPACE_METRICS_SOURCE).increment(1);
    TimedGuard {
        guard: Some(guard),
        acquired,
        site,
    }
}

fn lock_current_history<'a>(
    m: &'a Mutex<Box<dyn History>>,
    site: &'static LockSiteMetrics,
) -> TimedGuard<'a, Box<dyn History>> {
    timed_lock(
        m,
        HISTORY_REPO_CURRENT_HISTORY_LOCK_WAIT_NS_METRIC,
        HISTORY_REPO_CURRENT_HISTORY_LOCK_CALLS_METRIC,
        site,
        "History Repository Impl: Unable to acquire history lock",
    )
}

fn lock_roots_repository<'a>(
    m: &'a Mutex<RootRepository>,
    site: &'static LockSiteMetrics,
) -> TimedGuard<'a, RootRepository> {
    timed_lock(
        m,
        HISTORY_REPO_ROOTS_LOCK_WAIT_NS_METRIC,
        HISTORY_REPO_ROOTS_LOCK_CALLS_METRIC,
        site,
        "History Repository Impl: Unable to acquire roots repository lock",
    )
}

impl<C, P, A, K> HistoryRepositoryImpl<C, P, A, K>
where
    C: Clone + Send + Sync + Serialize,
    P: Clone + Send + Sync + Serialize,
    A: Clone + Send + Sync + Serialize,
    K: Clone + Send + Sync + Serialize,
{
    fn checkpoint_noop_clone(
        &self,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>
    where
        C: for<'a> Deserialize<'a> + 'static,
        P: for<'a> Deserialize<'a> + 'static,
        A: for<'a> Deserialize<'a> + 'static,
        K: for<'a> Deserialize<'a> + 'static,
    {
        Box::new(HistoryRepositoryImpl {
            current_history: self.current_history.clone(),
            roots_repository: self.roots_repository.clone(),
            checkpoint_writer: self.checkpoint_writer.clone(),
            leaf_store: self.leaf_store.clone(),
            history_kv_store: self.history_kv_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        })
    }

    fn checkpoint_parallel_actions_threshold() -> usize { CHECKPOINT_PARALLEL_ACTIONS_THRESHOLD }

    fn should_parallelize_checkpoint_actions(actions_len: usize) -> bool {
        actions_len >= Self::checkpoint_parallel_actions_threshold()
    }

    fn measure(&self, actions: &Vec<HotStoreAction<C, P, A, K>>) {
        if !tracing::enabled!(Level::DEBUG) {
            return;
        }
        for p in self.compute_measure(actions) {
            debug!("{}", p);
        }
    }

    fn compute_measure(&self, actions: &Vec<HotStoreAction<C, P, A, K>>) -> Vec<String> {
        actions
            .into_par_iter()
            .map(|action| match action {
                HotStoreAction::Insert(InsertData(i)) => {
                    let key = hash(&i.channel).bytes();
                    let data = encode_datums(&i.data);
                    format!("{};insert-data;{};{}", hex::encode(key), data.len(), i.data.len())
                }
                HotStoreAction::Insert(InsertContinuations(i)) => {
                    let key = hash_from_vec(&i.channels).bytes();
                    let data = encode_continuations(&i.continuations);
                    format!(
                        "{};insert-continuation;{};{}",
                        hex::encode(key),
                        data.len(),
                        i.continuations.len()
                    )
                }
                HotStoreAction::Insert(InsertJoins(i)) => {
                    let key = hash(&i.channel).bytes();
                    let data = encode_joins(&i.joins);
                    format!("{};insert-join;{};", hex::encode(key), data.len())
                }
                HotStoreAction::Delete(DeleteData(d)) => {
                    let key = hash(&d.channel).bytes();
                    format!("{};delete-data;0", hex::encode(key))
                }
                HotStoreAction::Delete(DeleteContinuations(d)) => {
                    let key = hash_from_vec(&d.channels).bytes();
                    format!("{};delete-continuation;0", hex::encode(key))
                }
                HotStoreAction::Delete(DeleteJoins(d)) => {
                    let key = hash(&d.channel).bytes();
                    format!("{};delete-join;0", hex::encode(key))
                }
            })
            .collect()
    }

    fn calculate_storage_actions(
        &self,
        action: &HotStoreTrieAction<C, P, A, K>,
    ) -> (ColdAction, HistoryAction) {
        match action {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertProduce(i)) => {
                let data = encode_datums(&i.data);
                let data_leaf = DataLeaf { bytes: data };
                let data_leaf_encoded = bincode::serialize(&data_leaf)
                    .expect("History Repository Impl: Unable to serialize DataLeaf");
                let data_hash = Blake2b256Hash::new(&data_leaf_encoded);

                (
                    (data_hash.clone(), Some(PersistedData::Data(data_leaf))),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_DATUM, &i.hash.bytes()),
                        hash: data_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertConsume(i)) => {
                let data = encode_continuations(&i.continuations);
                let continuations_leaf = ContinuationsLeaf { bytes: data };
                let continuations_leaf_encoded = bincode::serialize(&continuations_leaf)
                    .expect("History Repository Impl: Unable to serialize ContinuationsLeaf");
                let continuations_hash = Blake2b256Hash::new(&continuations_leaf_encoded);

                (
                    (
                        continuations_hash.clone(),
                        Some(PersistedData::Continuations(continuations_leaf)),
                    ),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_KONT, &i.hash.bytes()),
                        hash: continuations_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertJoins(i)) => {
                let data = encode_joins(&i.joins);
                let joins_leaf = JoinsLeaf { bytes: data };
                let joins_leaf_encoded = bincode::serialize(&joins_leaf)
                    .expect("History Repository Impl: Unable to serialize JoinsLeaf");
                let joins_hash = Blake2b256Hash::new(&joins_leaf_encoded);

                (
                    (joins_hash.clone(), Some(PersistedData::Joins(joins_leaf))),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_JOINS, &i.hash.bytes()),
                        hash: joins_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(i)) => {
                // Sort data before serializing for deterministic hashing regardless of
                // insertion order
                let mut sorted_data = i.data.clone();
                sorted_data.sort();
                let data =
                    bincode::serialize(&sorted_data).expect("Failed to serialize Vec<Vec<u8>>");
                let data_leaf = DataLeaf { bytes: data };
                let data_leaf_encoded = bincode::serialize(&data_leaf)
                    .expect("History Repository Impl: Unable to serialize DataLeaf");
                let data_hash = Blake2b256Hash::new(&data_leaf_encoded);

                (
                    (data_hash.clone(), Some(PersistedData::Data(data_leaf))),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_DATUM, &i.hash.bytes()),
                        hash: data_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(i)) => {
                // Sort continuations before serializing for deterministic hashing regardless of
                // insertion order
                let mut sorted_continuations = i.continuations.clone();
                sorted_continuations.sort();
                let data = bincode::serialize(&sorted_continuations)
                    .expect("Failed to serialize Vec<Vec<u8>>");
                let continuations_leaf = ContinuationsLeaf { bytes: data };
                let continuations_leaf_encoded = bincode::serialize(&continuations_leaf)
                    .expect("History Repository Impl: Unable to serialize ContinuationsLeaf");
                let continuations_hash = Blake2b256Hash::new(&continuations_leaf_encoded);

                (
                    (
                        continuations_hash.clone(),
                        Some(PersistedData::Continuations(continuations_leaf)),
                    ),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_KONT, &i.hash.bytes()),
                        hash: continuations_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(i)) => {
                // Sort joins before serializing for deterministic hashing regardless of
                // insertion order
                let mut sorted_joins = i.joins.clone();
                sorted_joins.sort();
                let data =
                    bincode::serialize(&sorted_joins).expect("Failed to serialize Vec<Vec<u8>>");
                let joins_leaf = JoinsLeaf { bytes: data };
                let joins_leaf_encoded = bincode::serialize(&joins_leaf)
                    .expect("History Repository Impl: Unable to serialize JoinsLeaf");
                let joins_hash = Blake2b256Hash::new(&joins_leaf_encoded);

                (
                    (joins_hash.clone(), Some(PersistedData::Joins(joins_leaf))),
                    HistoryAction::Insert(InsertAction {
                        key: prepend_bytes(PREFIX_JOINS, &i.hash.bytes()),
                        hash: joins_hash,
                    }),
                )
            }
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(d)) => (
                (d.hash.clone(), None),
                HistoryAction::Delete(DeleteAction {
                    key: prepend_bytes(PREFIX_DATUM, &d.hash.bytes()),
                }),
            ),
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(d)) => (
                (d.hash.clone(), None),
                HistoryAction::Delete(DeleteAction {
                    key: prepend_bytes(PREFIX_KONT, &d.hash.bytes()),
                }),
            ),
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(d)) => (
                (d.hash.clone(), None),
                HistoryAction::Delete(DeleteAction {
                    key: prepend_bytes(PREFIX_JOINS, &d.hash.bytes()),
                }),
            ),
        }
    }

    fn transform(
        &self,
        hot_store_action: HotStoreAction<C, P, A, K>,
    ) -> HotStoreTrieAction<C, P, A, K> {
        match hot_store_action {
            HotStoreAction::Insert(InsertData(i)) => {
                let key = hash(&i.channel);
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertProduce(
                    TrieInsertProduce::new(key, i.data),
                ))
            }
            HotStoreAction::Insert(InsertContinuations(i)) => {
                let key = hash_from_vec(&i.channels);
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertConsume(
                    TrieInsertConsume::new(key, i.continuations),
                ))
            }
            HotStoreAction::Insert(InsertJoins(i)) => {
                let key = hash(&i.channel);
                HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertJoins(
                    TrieInsertJoins::new(key, i.joins),
                ))
            }
            HotStoreAction::Delete(DeleteData(d)) => {
                let key = hash(&d.channel);
                HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(
                    TrieDeleteProduce::new(key),
                ))
            }
            HotStoreAction::Delete(DeleteContinuations(d)) => {
                let key = hash_from_vec(&d.channels);
                HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(
                    TrieDeleteConsume::new(key),
                ))
            }
            HotStoreAction::Delete(DeleteJoins(d)) => {
                let key = hash(&d.channel);
                HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(
                    TrieDeleteJoins::new(key),
                ))
            }
        }
    }
}

impl<C, P, A, K> HistoryRepository<C, P, A, K> for HistoryRepositoryImpl<C, P, A, K>
where
    C: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    P: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    A: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
    K: Clone + Send + Sync + Serialize + for<'a> Deserialize<'a> + 'static,
{
    fn checkpoint(
        &self,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static> {
        if actions.is_empty() {
            return self.checkpoint_noop_clone();
        }

        self.measure(&actions);

        let trie_actions: Vec<_> = if Self::should_parallelize_checkpoint_actions(actions.len()) {
            actions
                .into_par_iter()
                .map(|action| self.transform(action))
                .collect()
        } else {
            actions
                .into_iter()
                .map(|action| self.transform(action))
                .collect()
        };

        self.do_checkpoint(trie_actions)
    }

    fn do_checkpoint(
        &self,
        trie_actions: Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static> {
        let checkpoint_start = Instant::now();
        metrics::histogram!(HISTORY_CHECKPOINT_ACTIONS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(trie_actions.len() as f64);
        if trie_actions.is_empty() {
            let next = self.checkpoint_noop_clone();
            metrics::histogram!(HISTORY_CHECKPOINT_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(checkpoint_start.elapsed().as_secs_f64());
            return next;
        }

        let storage_actions_start = Instant::now();
        let storage_actions: Vec<(ColdAction, HistoryAction)> =
            if Self::should_parallelize_checkpoint_actions(trie_actions.len()) {
                trie_actions
                    .par_iter()
                    .map(|a| self.calculate_storage_actions(a))
                    .collect()
            } else {
                trie_actions
                    .iter()
                    .map(|a| self.calculate_storage_actions(a))
                    .collect()
            };

        metrics::histogram!(HISTORY_CHECKPOINT_STORAGE_ACTIONS_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(storage_actions_start.elapsed().as_secs_f64());
        let partition_start = Instant::now();
        let mut cold_actions: Vec<(Blake2b256Hash, PersistedData)> = Vec::new();
        let mut history_actions: Vec<HistoryAction> = Vec::with_capacity(storage_actions.len());
        for ((key, maybe_data), history) in storage_actions {
            if let Some(data) = maybe_data {
                cold_actions.push((key, data));
            }
            history_actions.push(history);
        }
        metrics::histogram!(HISTORY_CHECKPOINT_PARTITION_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(partition_start.elapsed().as_secs_f64());

        // store cold data
        {
            let serialize_start = Instant::now();
            let mut serialized_bytes = 0u64;
            let serialized_cold_actions = cold_actions
                .into_iter()
                .map(|(key, value)| {
                    let serialized_key = bincode::serialize(&key)
                        .expect("History Respository Impl: Failed to serialize");
                    let serialized_value = bincode::serialize(&value)
                        .expect("History Respository Impl: Failed to serialize");
                    serialized_bytes = serialized_bytes
                        .saturating_add(serialized_key.len() as u64)
                        .saturating_add(serialized_value.len() as u64);
                    (serialized_key, serialized_value)
                })
                .collect();
            metrics::histogram!(HISTORY_CHECKPOINT_SERIALIZE_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(serialize_start.elapsed().as_secs_f64());
            metrics::histogram!(HISTORY_CHECKPOINT_SERIALIZED_BYTES_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(serialized_bytes as f64);

            let leaf_write_start = Instant::now();
            self.leaf_store
                .put_if_absent(serialized_cold_actions)
                .expect("History Repository Impl: Failed to put if absent");
            metrics::histogram!(HISTORY_CHECKPOINT_LEAF_WRITE_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(leaf_write_start.elapsed().as_secs_f64());
        };

        // store everything related to history (history data, new root and populate
        // cache for new root)
        let (new_history, nodes, lock_wait, process_time) = {
            let lock_start = Instant::now();
            let history_lock =
                lock_current_history(&self.current_history, &HISTORY_LOCK_CHECKPOINT_SITE);
            let lock_wait = lock_start.elapsed();
            let process_start = Instant::now();
            let (new_history, nodes) = history_lock.stage(history_actions).unwrap();
            (new_history, nodes, lock_wait, process_start.elapsed())
        };
        metrics::histogram!(HISTORY_CHECKPOINT_HISTORY_LOCK_WAIT_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(lock_wait.as_secs_f64());
        metrics::histogram!(HISTORY_CHECKPOINT_HISTORY_PROCESS_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(process_time.as_secs_f64());

        let new_root = new_history.root();
        let commit_start = Instant::now();
        self.checkpoint_writer
            .write(nodes, &new_root)
            .expect("History Repository Impl: Unable to write checkpoint");
        metrics::histogram!(HISTORY_CHECKPOINT_ROOT_COMMIT_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(commit_start.elapsed().as_secs_f64());

        let next = Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(new_history)),
            roots_repository: self.roots_repository.clone(),
            checkpoint_writer: self.checkpoint_writer.clone(),
            leaf_store: self.leaf_store.clone(),
            history_kv_store: self.history_kv_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        });
        metrics::histogram!(HISTORY_CHECKPOINT_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
            .record(checkpoint_start.elapsed().as_secs_f64());
        next
    }

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        debug!("[HistoryRepositoryImpl] reset to {}", root);

        // The roots lock is released before the history lock is taken. This is
        // safe because roots are only added, never removed, so a root that
        // passes validate_root stays valid until history.reset uses it.
        lock_roots_repository(&self.roots_repository, &ROOTS_LOCK_RESET_SITE)
            .validate_root(root)?;

        let history_lock = lock_current_history(&self.current_history, &HISTORY_LOCK_RESET_SITE);
        let next = history_lock.reset(root)?;

        Ok(Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(next)),
            roots_repository: self.roots_repository.clone(),
            checkpoint_writer: self.checkpoint_writer.clone(),
            leaf_store: self.leaf_store.clone(),
            history_kv_store: self.history_kv_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        }))
    }

    fn history(&self) -> Arc<Mutex<Box<dyn History>>> { self.current_history.clone() }

    fn exporter(&self) -> Arc<dyn RSpaceExporter> { self.rspace_exporter.clone() }

    fn importer(&self) -> Arc<dyn RSpaceImporter> { self.rspace_importer.clone() }

    fn get_history_reader(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>, HistoryError> {
        let history_lock = lock_current_history(&self.current_history, &HISTORY_LOCK_READER_SITE);
        let history_repo = history_lock.reset(state_hash)?;
        Ok(Box::new(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone())))
    }

    fn get_history_reader_struct(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<RSpaceHistoryReaderImpl<C, P, A, K>, HistoryError> {
        let history_lock = lock_current_history(&self.current_history, &HISTORY_LOCK_READER_SITE);
        let history_repo = history_lock.reset(state_hash)?;
        Ok(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone()))
    }

    fn root(&self) -> Blake2b256Hash {
        let history_lock = lock_current_history(&self.current_history, &HISTORY_LOCK_ROOT_SITE);
        history_lock.root()
    }

    fn record_root(&self, root: &Blake2b256Hash) -> Result<(), HistoryError> {
        let roots_repo =
            lock_roots_repository(&self.roots_repository, &ROOTS_LOCK_RECORD_ROOT_SITE);
        roots_repo.commit(root).map_err(HistoryError::from)
    }

    fn contains_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError> {
        let roots_repo =
            lock_roots_repository(&self.roots_repository, &ROOTS_LOCK_CONTAINS_ROOT_SITE);
        roots_repo.contains_root(root).map_err(HistoryError::from)
    }

    fn history_store(&self) -> Arc<dyn KeyValueStore> { self.history_kv_store.clone() }

    fn prune_history_atomic(
        &self,
        retained_roots: &[Blake2b256Hash],
    ) -> Result<HistoryPruneResult, HistoryError> {
        atomic_prune_via_store(self.history_kv_store.as_ref(), retained_roots)
    }
}

/// Mark-and-sweep against a generic `KeyValueStore`. Correct for any
/// backend because `KeyValueStore::put` / `delete` are the only writers on
/// the trie-node store, and LMDB's single-writer rule inside the backing
/// env serialises them against our sweep's own writes. In-memory backends
/// serialise through their internal `RwLock`, which gives the same
/// guarantee. Not held across await points by the pruner, so the stall is
/// bounded to the pass duration.
///
/// A truly atomic implementation would wrap the whole pass in one LMDB
/// write transaction so even the mark-walk reads see pre-sweep state;
/// that requires exposing a scoped `RwTxn` handle through the
/// `KeyValueStore` trait, which is a bigger surface change. The current
/// implementation leans on two facts to stay correct without that:
///
/// 1. Reads via `KeyValueStore::get_one` during the mark walk see a consistent
///    view because LMDB read transactions are snapshot-isolated even against
///    concurrent writers, and our mark walk opens a fresh read transaction per
///    `get_one` — the "worst" a racing writer can do is add more nodes we then
///    also mark via their root (because the root is in `retained_roots`).
/// 2. The delete loop iterates keys we already collected into memory, so a
///    writer landing between iteration and `delete` can only add nodes, not
///    remove ones we were about to touch. A newly-added node whose hash
///    collides with a key we're about to delete would be a content-addressed
///    collision, which means the new node's bytes equal the dead node's bytes —
///    the new block tolerates the deletion because its next write re-inserts
///    the same bytes via `put_one_if_absent` on the content-addressed path.
///
/// If a stronger guarantee is needed later (e.g., to accommodate backends
/// without content-addressed idempotent writes), swap this helper to a
/// `with_write_txn` scope method on `KeyValueStore`.
fn atomic_prune_via_store(
    store: &dyn KeyValueStore,
    retained_roots: &[Blake2b256Hash],
) -> Result<HistoryPruneResult, HistoryError> {
    use std::collections::HashSet;

    // Phase 1: mark. DFS from each retained root; stop at a missing node
    // (joiner / LFS-bootstrap case where the subtree has not been imported
    // yet). Shared subtrees across roots are marked once because the
    // `HashSet::insert` short-circuits.
    let mut mark: HashSet<Blake2b256Hash> = HashSet::new();
    for root in retained_roots {
        let mut frontier: Vec<Blake2b256Hash> = vec![root.clone()];
        while let Some(hash) = frontier.pop() {
            if !mark.insert(hash.clone()) {
                continue;
            }
            let bytes = store.get_one(&hash.bytes()).map_err(HistoryError::from)?;
            let Some(serialized) = bytes else {
                // A subtree not yet imported (or already swept by a
                // previous pass). Correct behaviour is to stop the walk
                // here — we cannot mark nodes we cannot read. Future
                // passes will see the full subtree once it is imported.
                continue;
            };
            let node = decode(serialized);
            for item in node {
                if let Item::NodePtr { ptr, .. } = item {
                    frontier.push(Blake2b256Hash::from_bytes(ptr));
                }
                // `Item::Leaf` values point into the cold store, not into
                // history. Never follow them from here.
            }
        }
    }
    let marked = mark.len();

    // Phase 2: sweep. Paginate the env so we never materialise more than
    // one page worth of keys at once (~32 KiB for 32-byte hashes at the
    // default 1024 limit). `delete` is a batched write that LMDB
    // serialises against other writers for the duration of the delete
    // transaction — long enough for correctness, short enough not to
    // stall the chain for the whole pass.
    const SWEEP_PAGE: usize = 1024;
    let mut cursor: Option<Vec<u8>> = None;
    let mut swept = 0usize;
    loop {
        let page = store
            .scan_page(cursor.as_deref(), SWEEP_PAGE)
            .map_err(|e| HistoryError::RadixTreeError(RadixTreeError::KvStoreError(e)))?;
        let to_delete: Vec<Vec<u8>> = page
            .rows
            .iter()
            .filter_map(|(key, _)| {
                let hash = Blake2b256Hash::from_bytes(key.clone());
                (!mark.contains(&hash)).then(|| key.clone())
            })
            .collect();
        if !to_delete.is_empty() {
            swept += store
                .delete(to_delete)
                .map_err(|e| HistoryError::RadixTreeError(RadixTreeError::KvStoreError(e)))?;
        }
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    Ok(HistoryPruneResult { marked, swept })
}

pub fn prepend_bytes(element: u8, bytes: &[u8]) -> Vec<u8> {
    let mut prefixed = Vec::with_capacity(bytes.len() + 1);
    prefixed.push(element);
    prefixed.extend_from_slice(bytes);
    prefixed
}
