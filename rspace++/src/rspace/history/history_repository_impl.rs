use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;
use tracing::{Level, debug};

use super::cold_store::{ContinuationsLeaf, DataLeaf, JoinsLeaf};
use super::history_action::{DeleteAction, HistoryAction, InsertAction};
use super::history_reader::HistoryReader;
use super::history_repository::{PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use super::native_reader::NativeHistoryReader;
use crate::rspace::errors::{HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use crate::rspace::history::cold_store::PersistedData;
use crate::rspace::history::history::History;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::history::native_checkpoint::{NativeCheckpoint, NativeCheckpointBacking};
use crate::rspace::history::root_repository::RootRepository;
use crate::rspace::hot_store_action::DeleteAction::{DeleteContinuations, DeleteData, DeleteJoins};
use crate::rspace::hot_store_action::InsertAction::{InsertContinuations, InsertData, InsertJoins};
use crate::rspace::hot_store_action::{HotStoreAction, NativeExportAction};
use crate::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieDeleteAction, TrieDeleteConsume, TrieDeleteJoins, TrieDeleteProduce,
    TrieInsertAction, TrieInsertConsume, TrieInsertJoins, TrieInsertProduce,
};
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::metrics_constants::{
    HISTORY_REPO_CURRENT_HISTORY_LOCK_CALLS_METRIC,
    HISTORY_REPO_CURRENT_HISTORY_LOCK_WAIT_NS_METRIC, HISTORY_REPO_ROOTS_LOCK_CALLS_METRIC,
    HISTORY_REPO_ROOTS_LOCK_WAIT_NS_METRIC, HISTORY_RSPACE_METRICS_SOURCE,
};
use crate::rspace::serializers::serializers::{encode_continuations, encode_datums, encode_joins};
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

// See rspace/src/main/scala/coop/rchain/rspace/history/HistoryRepositoryImpl.
// scala
pub struct HistoryRepositoryImpl<C, P, A, K> {
    pub current_history: Arc<Mutex<Box<dyn History>>>,
    pub roots_repository: Arc<Mutex<RootRepository>>,
    pub leaf_store: Arc<dyn KeyValueStore>,
    pub node_store: Arc<dyn KeyValueStore>,
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
fn lock_current_history(
    m: &Mutex<Box<dyn History>>,
) -> std::sync::MutexGuard<'_, Box<dyn History>> {
    let start = Instant::now();
    let guard = m
        .lock()
        .expect("History Repository Impl: Unable to acquire history lock");
    metrics::counter!(HISTORY_REPO_CURRENT_HISTORY_LOCK_WAIT_NS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
        .increment(start.elapsed().as_nanos() as u64);
    metrics::counter!(HISTORY_REPO_CURRENT_HISTORY_LOCK_CALLS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
        .increment(1);
    guard
}

fn lock_roots_repository(m: &Mutex<RootRepository>) -> std::sync::MutexGuard<'_, RootRepository> {
    let start = Instant::now();
    let guard = m
        .lock()
        .expect("History Repository Impl: Unable to acquire roots repository lock");
    metrics::counter!(HISTORY_REPO_ROOTS_LOCK_WAIT_NS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
        .increment(start.elapsed().as_nanos() as u64);
    metrics::counter!(HISTORY_REPO_ROOTS_LOCK_CALLS_METRIC, "source" => HISTORY_RSPACE_METRICS_SOURCE)
        .increment(1);
    guard
}

impl<C, P, A, K> HistoryRepositoryImpl<C, P, A, K>
where
    C: Clone + Send + Sync + Serialize,
    P: Clone + Send + Sync + Serialize,
    A: Clone + Send + Sync + Serialize,
    K: Clone + Send + Sync + Serialize,
{
    /// D-C3 (D-S3, DR-97): stages a native checkpoint whose cold and history
    /// actions `prepare` builds: the atomic-root check, the radix
    /// preparation, the root reservation and the backing, in the order of
    /// the owned checkpoint.
    fn stage_native_checkpoint(
        &self,
        prepare: impl FnOnce() -> Result<NativeCheckpoint, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<NativeCheckpoint, RSpaceError> {
        meter.reserve(1, 0, 0)?;
        if !self
            .roots_repository
            .lock()
            .expect("root lock")
            .supports_atomic_record()
        {
            return Err(RSpaceError::InterpreterError(
                "native checkpoint requires atomic root recording".to_owned(),
            ));
        }
        let mut prepared = prepare()?;
        meter.reserve(8, 0, 0)?;
        if !prepared.history_actions.is_empty() {
            let history = self.current_history.lock().expect("history lock");
            prepared.prepared_history =
                Some(history.prepare_native(std::mem::take(&mut prepared.history_actions), meter)?);
        }
        if let Some(staged) = &prepared.prepared_history {
            let root = staged.next.root_ref();
            self.roots_repository
                .lock()
                .expect("root lock")
                .reserve_native_commit(root, meter)?;
            meter.reserve(1, root.0.len(), root.0.len())?;
            prepared.root = Some(root.clone());
        }
        let result_backing = size_of::<HistoryRepositoryImpl<C, P, A, K>>()
            .checked_add(if prepared.prepared_history.is_some() {
                size_of::<Mutex<Box<dyn History>>>()
                    .checked_add(2 * size_of::<usize>())
                    .and_then(|bytes| bytes.checked_add(align_of::<Mutex<Box<dyn History>>>() * 2))
                    .ok_or(RSpaceError::HostWorkRejected)?
            } else {
                0
            })
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(2, 0, result_backing)?;
        prepared.backing = Some(NativeCheckpointBacking {
            history: self.current_history.clone(),
            roots: self.roots_repository.clone(),
            leaves: self.leaf_store.clone(),
            nodes: self.node_store.clone(),
        });
        Ok(prepared)
    }

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
            leaf_store: self.leaf_store.clone(),
            node_store: self.node_store.clone(),
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
    // Changed by D-C3 (D-S3, DR-97): the owned and the borrowed checkpoints
    // share the staging in `stage_native_checkpoint`; the body below moved
    // there unchanged, with the preparation as its closure.
    // fn prepare_native_checkpoint(
    //     &self,
    //     actions: Vec<HotStoreAction<C, P, A, K>>,
    //     meter: &dyn SourceMeter,
    // ) -> Result<NativeCheckpoint, RSpaceError> {
    //     meter.reserve(1, 0, 0)?;
    //     if !self
    //         .roots_repository
    //         .lock()
    //         .expect("root lock")
    //         .supports_atomic_record()
    //     {
    //         return Err(RSpaceError::InterpreterError(
    //             "native checkpoint requires atomic root recording".to_owned(),
    //         ));
    //     }
    //     let mut prepared = NativeCheckpoint::prepare(actions, meter)?;
    //     meter.reserve(8, 0, 0)?;
    //     if !prepared.history_actions.is_empty() {
    //         let history = self.current_history.lock().expect("history lock");
    //         prepared.prepared_history =
    //             Some(history.prepare_native(std::mem::take(&mut
    // prepared.history_actions), meter)?);     }
    //     if let Some(staged) = &prepared.prepared_history {
    //         let root = staged.next.root_ref();
    //         self.roots_repository
    //             .lock()
    //             .expect("root lock")
    //             .reserve_native_commit(root, meter)?;
    //         meter.reserve(1, root.0.len(), root.0.len())?;
    //         prepared.root = Some(root.clone());
    //     }
    //     let result_backing = size_of::<HistoryRepositoryImpl<C, P, A, K>>()
    //         .checked_add(if prepared.prepared_history.is_some() {
    //             size_of::<Mutex<Box<dyn History>>>()
    //                 .checked_add(2 * size_of::<usize>())
    //                 .and_then(|bytes| bytes.checked_add(align_of::<Mutex<Box<dyn
    // History>>>() * 2))                 .ok_or(RSpaceError::HostWorkRejected)?
    //         } else {
    //             0
    //         })
    //         .ok_or(RSpaceError::HostWorkRejected)?;
    //     meter.reserve(2, 0, result_backing)?;
    //     prepared.backing = Some(NativeCheckpointBacking {
    //         history: self.current_history.clone(),
    //         roots: self.roots_repository.clone(),
    //         leaves: self.leaf_store.clone(),
    //         nodes: self.node_store.clone(),
    //     });
    //     Ok(prepared)
    // }
    fn prepare_native_checkpoint(
        &self,
        actions: Vec<HotStoreAction<C, P, A, K>>,
        meter: &dyn SourceMeter,
    ) -> Result<NativeCheckpoint, RSpaceError> {
        self.stage_native_checkpoint(|| NativeCheckpoint::prepare(actions, meter), meter)
    }

    fn prepare_native_checkpoint_borrowed(
        &self,
        actions: &[NativeExportAction<'_, C, A, Arc<WaitingContinuation<P, K>>>],
        meter: &dyn SourceMeter,
    ) -> Result<NativeCheckpoint, RSpaceError> {
        self.stage_native_checkpoint(|| NativeCheckpoint::prepare_borrowed(actions, meter), meter)
    }

    fn commit_native_checkpoint(
        &self,
        prepared: NativeCheckpoint,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, RSpaceError> {
        let backing = prepared.backing.as_ref().ok_or_else(|| {
            RSpaceError::InterpreterError("native checkpoint has no history backing".to_owned())
        })?;
        if !Arc::ptr_eq(&backing.history, &self.current_history) ||
            !Arc::ptr_eq(&backing.roots, &self.roots_repository) ||
            !Arc::ptr_eq(&backing.leaves, &self.leaf_store) ||
            !Arc::ptr_eq(&backing.nodes, &self.node_store)
        {
            return Err(RSpaceError::InterpreterError(
                "native checkpoint belongs to another history repository".to_owned(),
            ));
        }
        let Some(staged_history) = prepared.prepared_history else {
            if !prepared.history_actions.is_empty() {
                return Err(RSpaceError::InterpreterError(
                    "native checkpoint has no prepared history".to_owned(),
                ));
            }
            return Ok(self.checkpoint_noop_clone());
        };
        let new_root = prepared.root.ok_or_else(|| {
            RSpaceError::InterpreterError("native checkpoint has no prepared root".to_owned())
        })?;
        if staged_history.next.root_ref() != &new_root {
            return Err(RSpaceError::InterpreterError(
                "native checkpoint root differs from prepared history".to_owned(),
            ));
        }
        let new_repository: Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static> =
            Box::new(HistoryRepositoryImpl {
                current_history: Arc::new(Mutex::new(staged_history.next)),
                roots_repository: self.roots_repository.clone(),
                leaf_store: self.leaf_store.clone(),
                node_store: self.node_store.clone(),
                rspace_exporter: self.rspace_exporter.clone(),
                rspace_importer: self.rspace_importer.clone(),
                _marker: PhantomData,
            });
        self.leaf_store.put_if_absent(prepared.cold_actions)?;
        if !staged_history.writes.is_empty() {
            staged_history
                .store
                .put(staged_history.writes)
                .map_err(HistoryError::from)?;
        }
        self.roots_repository
            .lock()
            .expect("root lock")
            .commit_native(&new_root)
            .map_err(HistoryError::from)?;
        Ok(new_repository)
    }

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
        if trie_actions.is_empty() {
            return self.checkpoint_noop_clone();
        }

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

        let mut cold_actions: Vec<(Blake2b256Hash, PersistedData)> = Vec::new();
        let mut history_actions: Vec<HistoryAction> = Vec::with_capacity(storage_actions.len());
        for ((key, maybe_data), history) in storage_actions {
            if let Some(data) = maybe_data {
                cold_actions.push((key, data));
            }
            history_actions.push(history);
        }

        // save new root for state after checkpoint
        let store_root = |root| {
            let roots_repo_lock = lock_roots_repository(&self.roots_repository);
            roots_repo_lock.commit(root)
        };

        // store cold data
        {
            let serialized_cold_actions = cold_actions
                .into_iter()
                .map(|(key, value)| {
                    let serialized_key = bincode::serialize(&key)
                        .expect("History Respository Impl: Failed to serialize");
                    let serialized_value = bincode::serialize(&value)
                        .expect("History Respository Impl: Failed to serialize");
                    (serialized_key, serialized_value)
                })
                .collect();

            self.leaf_store
                .put_if_absent(serialized_cold_actions)
                .expect("History Repository Impl: Failed to put if absent");
        };

        // store everything related to history (history data, new root and populate
        // cache for new root)
        let new_history = {
            let history_lock = lock_current_history(&self.current_history);
            history_lock.process(history_actions).unwrap()
        };

        let new_root = new_history.root();
        store_root(&new_root).expect("History Repository Impl: Unable to store root");

        ();

        Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(new_history)),
            roots_repository: self.roots_repository.clone(),
            leaf_store: self.leaf_store.clone(),
            node_store: self.node_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        })
    }

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        debug!("[HistoryRepositoryImpl] reset to {}", root);

        let roots_lock = lock_roots_repository(&self.roots_repository);
        roots_lock.validate_and_set_current_root(root.clone())?;

        let history_lock = lock_current_history(&self.current_history);
        let next = history_lock.reset(root)?;

        Ok(Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(next)),
            roots_repository: self.roots_repository.clone(),
            leaf_store: self.leaf_store.clone(),
            node_store: self.node_store.clone(),
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
        let history_lock = lock_current_history(&self.current_history);
        let history_repo = history_lock.reset(state_hash)?;
        Ok(Box::new(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone())))
    }

    fn get_current_history_reader_native(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>, RSpaceError> {
        let history_lock = lock_current_history(&self.current_history);
        let history = history_lock.snapshot_native(meter)?;
        let bytes = std::mem::size_of::<RSpaceHistoryReaderImpl<C, P, A, K>>()
            .checked_add(std::mem::size_of::<Box<dyn History>>())
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<usize>()))
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(3, 0, bytes)?;
        Ok(Box::new(RSpaceHistoryReaderImpl::new(history, self.leaf_store.clone())))
    }

    fn get_history_reader_struct(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<RSpaceHistoryReaderImpl<C, P, A, K>, HistoryError> {
        let history_lock = lock_current_history(&self.current_history);
        let history_repo = history_lock.reset(state_hash)?;
        Ok(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone()))
    }

    fn root(&self) -> Blake2b256Hash {
        let history_lock = lock_current_history(&self.current_history);
        history_lock.root()
    }

    fn native_history_reader(&self, state_hash: [u8; 32]) -> NativeHistoryReader<'_> {
        NativeHistoryReader::new(state_hash, self.node_store.as_ref(), self.leaf_store.as_ref())
    }

    fn record_root(&self, root: &Blake2b256Hash) -> Result<(), HistoryError> {
        let roots_repo = lock_roots_repository(&self.roots_repository);
        roots_repo.commit(root).map_err(HistoryError::from)
    }

    fn contains_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError> {
        let roots_repo = lock_roots_repository(&self.roots_repository);
        roots_repo.contains_root(root).map_err(HistoryError::from)
    }
}

pub fn prepend_bytes(element: u8, _bytes: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(element);
    bytes.extend(_bytes.iter().cloned());
    bytes
}
