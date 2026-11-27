use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::KeyValueStore;
use tracing::{Level, debug};

use super::cold_store::{ContinuationsLeaf, DataLeaf, JoinsLeaf};
use super::cold_store_codec::ColdStoreCodec;
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
use crate::rspace::history::prune_guard::PruneGuard;
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
    /// Coordination with the history pruner (issue #1061). Shared by Arc
    /// across every `HistoryRepositoryImpl` that refers to the same backing
    /// trie store; `do_checkpoint` and `reset` return a fresh repository
    /// instance but keep the same `prune_guard`.
    pub prune_guard: Arc<PruneGuard>,
    pub _marker: PhantomData<(C, P, A, K)>,
}

type ColdAction = (Blake2b256Hash, Option<PersistedData>);
const CHECKPOINT_PARALLEL_ACTIONS_THRESHOLD: usize = 256;

pub(super) struct CheckpointLeaf {
    key: ByteBuffer,
    value: ByteBuffer,
}

impl CheckpointLeaf {
    pub(super) fn into_pair(self) -> (ByteBuffer, ByteBuffer) { (self.key, self.value) }
}

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
            prune_guard: self.prune_guard.clone(),
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
                    CheckpointLeaf {
                        key: serialized_key,
                        value: serialized_value,
                    }
                })
                .collect();
            metrics::histogram!(HISTORY_CHECKPOINT_SERIALIZE_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(serialize_start.elapsed().as_secs_f64());
            metrics::histogram!(HISTORY_CHECKPOINT_SERIALIZED_BYTES_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(serialized_bytes as f64);

            let leaf_write_start = Instant::now();
            let write =
                if let Some(codec) = self.leaf_store.as_any().downcast_ref::<ColdStoreCodec>() {
                    codec.put_checkpoint_if_absent(serialized_cold_actions)
                } else {
                    self.leaf_store.put_if_absent(
                        serialized_cold_actions
                            .into_iter()
                            .map(CheckpointLeaf::into_pair)
                            .collect(),
                    )
                };
            write.expect("History Repository Impl: Failed to put if absent");
            metrics::histogram!(HISTORY_CHECKPOINT_LEAF_WRITE_TIME_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
                .record(leaf_write_start.elapsed().as_secs_f64());
        };

        // store everything related to history (history data, new root and populate
        // cache for new root).
        //
        // Hold the prune guard's shared side across `stage` (which reads
        // the store via `pending_writes` to decide which content-addressed
        // nodes to skip) and `checkpoint_writer.write` (which lands the
        // remaining nodes). If the sweep fires between those two steps,
        // it could delete a hash the stage decided to skip, leaving the
        // new state with a dangling pointer. See
        // `rspace::history::prune_guard` for the full rationale.
        let _prune_lock = self.prune_guard.enter_write();

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
        // Pin the new root before writing so the next sweep retains it even
        // if the DAG has not yet caught up to the block that will reference
        // it as a pre/post state.
        self.prune_guard.pin(new_root.clone());

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
            prune_guard: self.prune_guard.clone(),
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

        // Hold the sweep's shared side across pin + load. Without this a
        // sweep that took its pinned snapshot a moment before our `pin`
        // call would delete the root before `history_lock.reset` reads
        // it. Lock order (prune_guard shared → current_history) matches
        // `do_checkpoint`.
        let _prune_lock = self.prune_guard.enter_write();

        // Pin the target so an in-flight sweep that did not reach this
        // root via its caller-supplied set keeps its subtree. A reset is
        // issued ahead of executing or replaying the block anchored at
        // this root; without the pin the sweep could delete nodes that
        // the next read reaches.
        self.prune_guard.pin(root.clone());

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
            prune_guard: self.prune_guard.clone(),
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
        // Issue #1098 age-based pin eviction + observability. `begin_pass`
        // bumps the generation and drops pins older than the retention
        // window; `pins_evicted` feeds the structured log on the casper
        // side so operators can see the mark set contracting with pin age.
        let pass = self.prune_guard.begin_pass(PIN_RETENTION_PASSES);

        // Take the sweep's exclusive side briefly to force any in-flight
        // writers to finish their stage+pin+write window BEFORE we
        // snapshot pins. Without this a writer that has pinned its new
        // root but not yet committed its atomic `batched_put` write would
        // be in our pin snapshot without its subtree bytes being readable
        // yet. The lock-free mark walk below would then skip that
        // subtree, and the per-batch re-snapshot would also skip the pin
        // (already seen), so writer-committed nodes could still get
        // swept. Holding exclusive here is bounded by a single writer's
        // worst txn duration (sub-second under load), not the full pass.
        let initial_pins = {
            let _quiesce = self.prune_guard.enter_sweep();
            self.prune_guard.pinned_snapshot()
        };

        atomic_prune_via_store_batched(
            self.history_kv_store.as_ref(),
            self.prune_guard.as_ref(),
            retained_roots,
            initial_pins,
            pass,
        )
    }
}

/// How many passes a pin survives before age-based eviction drops it
/// (issue #1098). A pin's job is to protect the window between
/// `do_checkpoint`'s write and the block's appearance in the DAG
/// retention walk. That window is bounded by block-processing latency
/// (sub-second at steady state); two pass intervals (~10 min at the
/// testbed's cadence) is a generous safety margin. The LRU cap in
/// `prune_guard` is the harder bound against burst pressure.
const PIN_RETENTION_PASSES: u64 = 2;

/// Mark-and-sweep against a generic `KeyValueStore`.
///
/// # Lock contract (issues #1061, #1098)
///
/// The pre-#1098 helper held the `PruneGuard` exclusive side for the full
/// pass. A single ~41 s pass therefore stopped block processing for the
/// same ~41 s because every `do_checkpoint` waited for `enter_write` to
/// pair with the shared side of that lock. #1098 split that single
/// window into many short ones:
///
/// * The **initial mark walk** runs with no lock held. Writers can add nodes
///   concurrently; they cannot delete, so no node we read gets removed out from
///   under us. `initial_pins` is the snapshot our caller took under exclusive —
///   every pin in it has a committed write, so `get_one` returns full bytes for
///   every reachable child.
/// * Each **delete batch** takes the exclusive side briefly: scan a page
///   lock-free, acquire exclusive, re-snapshot pins, DFS-mark new pins that
///   appeared since the last batch, then delete unmarked keys from the page.
///   Typical per-batch hold time is a delete txn plus any new-pin DFS — orders
///   of magnitude shorter than a full-pass hold.
///
/// # Content-addressed race (issue #1061)
///
/// `RadixTreeImpl::pending_writes` SKIPS keys already in the store
/// (content-addressed writes are idempotent — the already-present bytes
/// equal the pending bytes). A sweep that deleted such a key between the
/// writer's `contains` check and its `put` of the other nodes would leave
/// the new state with a dangling pointer. Mutual exclusion between
/// writer-shared and sweep-exclusive closes that race: when the sweep
/// holds exclusive for a delete batch, no writer is in its stage+write
/// window; writers that completed earlier have pinned their new root,
/// and the batch's re-snapshot picks up every such pin before computing
/// the delete list.
fn atomic_prune_via_store_batched(
    store: &dyn KeyValueStore,
    guard: &crate::rspace::history::prune_guard::PruneGuard,
    caller_roots: &[Blake2b256Hash],
    initial_pins: Vec<Blake2b256Hash>,
    pass: crate::rspace::history::prune_guard::PassStart,
) -> Result<HistoryPruneResult, HistoryError> {
    use std::collections::HashSet;

    // Phase 1: initial mark walk. DFS from each caller root AND each
    // pinned root in the initial snapshot. No lock held. A pin from the
    // initial snapshot is guaranteed-landed because the caller took
    // exclusive to force writer quiescence before snapshotting.
    let mut mark: HashSet<Blake2b256Hash> = HashSet::new();
    let mut pins_seen: HashSet<Blake2b256Hash> = HashSet::new();
    for p in &initial_pins {
        pins_seen.insert(p.clone());
    }

    let mut seed: Vec<Blake2b256Hash> = Vec::with_capacity(caller_roots.len() + initial_pins.len());
    seed.extend_from_slice(caller_roots);
    seed.extend(initial_pins.iter().cloned());
    walk_and_mark(store, &seed, &mut mark)?;
    let marked_from_pins = marked_reachable_from(&mark, store, &initial_pins)?;

    // Phase 2: batched sweep. Scan a page without any lock, then acquire
    // exclusive for the batch: re-snapshot pins (any writer that pinned
    // between the last batch and now is now visible AND has committed its
    // write, because we just waited for all shared holders to drain), DFS
    // the new pins into the mark set, compute the delete list, delete.
    const SWEEP_PAGE: usize = 1024;
    let mut cursor: Option<Vec<u8>> = None;
    let mut swept = 0usize;
    let mut pins_mid_pass_added = 0usize;
    let mut batches = 0usize;
    loop {
        let page = store
            .scan_page(cursor.as_deref(), SWEEP_PAGE)
            .map_err(|e| HistoryError::RadixTreeError(RadixTreeError::KvStoreError(e)))?;
        if page.rows.is_empty() {
            break;
        }

        let _sweep_lock = guard.enter_sweep();
        batches += 1;

        let fresh_pins = guard.pinned_snapshot();
        let mut new_pins: Vec<Blake2b256Hash> = Vec::new();
        for p in fresh_pins {
            if pins_seen.insert(p.clone()) {
                new_pins.push(p);
            }
        }
        if !new_pins.is_empty() {
            pins_mid_pass_added += new_pins.len();
            // Under exclusive — every writer-committed node is readable.
            walk_and_mark(store, &new_pins, &mut mark)?;
        }

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
        drop(_sweep_lock);

        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    Ok(HistoryPruneResult {
        marked: mark.len(),
        swept,
        pins_initial: initial_pins.len(),
        pins_evicted: pass.pins_evicted,
        pins_mid_pass_added,
        marked_from_pins,
        batches,
    })
}

/// DFS from each root, adding every reachable node to `mark`. Stops at a
/// missing node (joiner / LFS-bootstrap case) — a hash we cannot read is
/// still marked (we will not sweep it) but its descendants are left
/// unexplored. Shared subtrees across roots are marked once because the
/// `HashSet::insert` short-circuits.
fn walk_and_mark(
    store: &dyn KeyValueStore,
    roots: &[Blake2b256Hash],
    mark: &mut std::collections::HashSet<Blake2b256Hash>,
) -> Result<(), HistoryError> {
    let mut frontier: Vec<Blake2b256Hash> = roots.to_vec();
    while let Some(hash) = frontier.pop() {
        if !mark.insert(hash.clone()) {
            continue;
        }
        let bytes = store.get_one(&hash.bytes()).map_err(HistoryError::from)?;
        let Some(serialized) = bytes else {
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
    Ok(())
}

/// Count how many of the already-marked nodes were reached from the given
/// roots. Used only for observability — tells operators whether the pins
/// are the dominant contributor to the mark set (the diagnosis from issue
/// #1098). A separate walk here is simpler than threading a per-origin
/// counter through `walk_and_mark`'s short-circuit; it runs in O(subtree)
/// against bytes that are already warm from the first walk.
fn marked_reachable_from(
    mark: &std::collections::HashSet<Blake2b256Hash>,
    store: &dyn KeyValueStore,
    roots: &[Blake2b256Hash],
) -> Result<usize, HistoryError> {
    use std::collections::HashSet;
    if roots.is_empty() {
        return Ok(0);
    }
    let mut reached: HashSet<Blake2b256Hash> = HashSet::new();
    let mut frontier: Vec<Blake2b256Hash> = roots.to_vec();
    while let Some(hash) = frontier.pop() {
        if !mark.contains(&hash) {
            continue;
        }
        if !reached.insert(hash.clone()) {
            continue;
        }
        let bytes = store.get_one(&hash.bytes()).map_err(HistoryError::from)?;
        let Some(serialized) = bytes else {
            continue;
        };
        let node = decode(serialized);
        for item in node {
            if let Item::NodePtr { ptr, .. } = item {
                frontier.push(Blake2b256Hash::from_bytes(ptr));
            }
        }
    }
    Ok(reached.len())
}

pub fn prepend_bytes(element: u8, bytes: &[u8]) -> Vec<u8> {
    let mut prefixed = Vec::with_capacity(bytes.len() + 1);
    prefixed.push(element);
    prefixed.extend_from_slice(bytes);
    prefixed
}
