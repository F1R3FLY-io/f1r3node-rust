use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use shared::rust::store::key_value_store::KeyValueStore;
use tracing::{Level, debug};

use super::cold_store::{
    CanonicalCollectionKind, ColdLeafAddress, ColdLeafStoreError, ContinuationsLeaf, DataLeaf,
    JoinsLeaf, PersistedDataKind, PersistedRecordSequenceView,
};
use super::history_action::{DeleteAction, HistoryAction, InsertAction};
use super::history_reader::HistoryReader;
use super::history_repository::{PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use super::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;
use crate::rspace::errors::{HistoryError, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::{StableHashSerialize, hash, hash_from_vec};
use crate::rspace::history::cold_store::PersistedData;
use crate::rspace::history::history::History;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::history::roots_store::CurrentRoot;
use crate::rspace::hot_store_action::DeleteAction::{
    DeleteCanonicalCollection, DeleteCanonicalData, DeleteContinuations, DeleteData, DeleteJoins,
};
use crate::rspace::hot_store_action::HotStoreAction;
use crate::rspace::hot_store_action::InsertAction::{
    InsertCanonicalCollection, InsertCanonicalData, InsertContinuations, InsertData, InsertJoins,
};
use crate::rspace::hot_store_trie_action::{
    HotStoreTrieAction, TrieDeleteAction, TrieDeleteCanonicalCollection, TrieDeleteConsume,
    TrieDeleteJoins, TrieDeleteProduce, TrieInsertAction, TrieInsertCanonicalCollection,
    TrieInsertCanonicalProduce, TrieInsertConsume, TrieInsertJoins, TrieInsertProduce,
};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::serializers::serializers::{encode_continuations, encode_datums, encode_joins};
use crate::rspace::state::instances::rspace_importer_store::record_validator;
use crate::rspace::state::rspace_exporter::RSpaceExporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

// See rspace/src/main/scala/coop/rchain/rspace/history/HistoryRepositoryImpl.
// scala
pub struct HistoryRepositoryImpl<C, P, A, K> {
    current_history: Arc<Mutex<Box<dyn History>>>,
    authority: Arc<RSpaceStateAuthority>,
    leaf_store: Arc<dyn KeyValueStore>,
    rspace_exporter: Arc<dyn RSpaceExporter>,
    rspace_importer: Arc<dyn RSpaceImporter>,
    _marker: PhantomData<(C, P, A, K)>,
}

pub(crate) type ColdAction = Option<(ColdLeafAddress, Vec<u8>)>;
const CHECKPOINT_PARALLEL_ACTIONS_THRESHOLD: usize = 256;

/// Hash the frozen bincode-1 representation of a leaf whose sole field is a
/// `Vec<u8>` without materializing `[length prefix | bytes]` a second time.
///
/// `bincode::serialize(&DataLeaf { bytes })`, and likewise the continuation
/// and join leaf wrappers, is exactly an eight-byte little-endian element
/// count followed by the byte payload. Differential tests below bind this
/// transducer to bincode rather than relying on that statement alone.
fn hash_single_byte_vec_leaf(bytes: &[u8]) -> Blake2b256Hash {
    let len = u64::try_from(bytes.len())
        .expect("RSpace leaf byte length must fit the frozen bincode u64 length field");
    let len = len.to_le_bytes();
    Blake2b256Hash::new_from_chunks([len.as_slice(), bytes])
}

fn canonical_collection_prefix(kind: CanonicalCollectionKind) -> u8 {
    match kind {
        CanonicalCollectionKind::Joins => PREFIX_JOINS,
        CanonicalCollectionKind::Continuations => PREFIX_KONT,
    }
}

fn canonical_collection_label(kind: CanonicalCollectionKind) -> &'static str {
    match kind {
        CanonicalCollectionKind::Joins => "join",
        CanonicalCollectionKind::Continuations => "continuation",
    }
}

fn cold_leaf_history_error(error: ColdLeafStoreError) -> HistoryError {
    match error {
        ColdLeafStoreError::Store(error) => HistoryError::KvStoreError(error),
        error => HistoryError::ActionError(error.to_string()),
    }
}

impl<C, P, A, K> HistoryRepositoryImpl<C, P, A, K>
where
    C: Clone + Send + Sync + StableHashSerialize,
    P: Clone + Send + Sync + StableHashSerialize,
    A: Clone + Send + Sync + StableHashSerialize,
    K: Clone + Send + Sync + StableHashSerialize,
{
    pub(crate) fn new(
        history: Box<dyn History>,
        authority: Arc<RSpaceStateAuthority>,
        leaf_store: Arc<dyn KeyValueStore>,
        rspace_exporter: Arc<dyn RSpaceExporter>,
        rspace_importer: Arc<dyn RSpaceImporter>,
    ) -> Self {
        Self {
            current_history: Arc::new(Mutex::new(history)),
            authority,
            leaf_store,
            rspace_exporter,
            rspace_importer,
            _marker: PhantomData,
        }
    }

    fn checkpoint_noop_clone(
        &self,
    ) -> Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>
    where
        C: ColdStoreDecode + 'static,
        P: ColdStoreDecode + 'static,
        A: ColdStoreDecode + 'static,
        K: ColdStoreDecode + 'static,
    {
        Box::new(HistoryRepositoryImpl {
            current_history: self.current_history.clone(),
            authority: self.authority.clone(),
            leaf_store: self.leaf_store.clone(),
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
                HotStoreAction::Insert(InsertCanonicalData(i)) => {
                    let view = PersistedRecordSequenceView::parse_canonical(
                        i.encoded_persisted_data(),
                        PersistedDataKind::Data,
                        Some(i.datum_count()),
                    )
                    .expect("canonical DATA egress must construct a valid record sequence");
                    format!(
                        "{};insert-data;{};{}",
                        hex::encode(i.hash().bytes()),
                        view.payload().len(),
                        i.datum_count()
                    )
                }
                HotStoreAction::Insert(InsertCanonicalCollection(i)) => {
                    let view = PersistedRecordSequenceView::parse_canonical(
                        i.encoded_persisted_data(),
                        i.persisted_kind(),
                        Some(i.record_count()),
                    )
                    .expect("canonical collection egress must construct a valid record sequence");
                    format!(
                        "{};insert-{};{};{}",
                        hex::encode(i.hash().bytes()),
                        canonical_collection_label(i.kind()),
                        view.payload().len(),
                        i.record_count()
                    )
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
                HotStoreAction::Delete(DeleteCanonicalData(d)) => {
                    format!("{};delete-data;0", hex::encode(d.hash().bytes()))
                }
                HotStoreAction::Delete(DeleteCanonicalCollection(d)) => format!(
                    "{};delete-{};0",
                    hex::encode(d.hash().bytes()),
                    canonical_collection_label(d.kind())
                ),
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
}

pub(crate) fn calculate_storage_actions<C, P, A, K>(
    action: HotStoreTrieAction<C, P, A, K>,
) -> Result<(ColdAction, HistoryAction), HistoryError>
where
    C: Clone + Send + Sync + StableHashSerialize,
    P: Clone + Send + Sync + StableHashSerialize,
    A: Clone + Send + Sync + StableHashSerialize,
    K: Clone + Send + Sync + StableHashSerialize,
{
    let (kind, digest, encoded, history_action) = match action {
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertProduce(i)) => {
            let data = encode_datums(&i.data);
            let data_leaf = DataLeaf { bytes: data };
            let data_hash = hash_single_byte_vec_leaf(&data_leaf.bytes);
            let encoded = PersistedData::Data(data_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Data),
                data_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_DATUM, &i.hash.bytes()),
                    hash: data_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertCanonicalProduce(i)) => {
            let encoded = i.encoded_persisted_data;
            let view = PersistedRecordSequenceView::parse_canonical(
                &encoded,
                PersistedDataKind::Data,
                Some(i.datum_count),
            )
            .map_err(|error| HistoryError::ActionError(error.to_string()))?;
            let data_hash = Blake2b256Hash::new(view.leaf_encoding());

            (
                Some(PersistedDataKind::Data),
                data_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_DATUM, &i.hash.bytes()),
                    hash: data_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertCanonicalCollection(
            i,
        )) => {
            let encoded = i.encoded_persisted_data;
            let view = PersistedRecordSequenceView::parse_canonical(
                &encoded,
                i.kind.persisted_kind(),
                Some(i.record_count),
            )
            .map_err(|error| HistoryError::ActionError(error.to_string()))?;
            let leaf_hash = Blake2b256Hash::new(view.leaf_encoding());

            (
                Some(i.kind.persisted_kind()),
                leaf_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(canonical_collection_prefix(i.kind), &i.hash.bytes()),
                    hash: leaf_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertConsume(i)) => {
            let data = encode_continuations(&i.continuations);
            let continuations_leaf = ContinuationsLeaf { bytes: data };
            let continuations_hash = hash_single_byte_vec_leaf(&continuations_leaf.bytes);
            let encoded = PersistedData::Continuations(continuations_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Continuations),
                continuations_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_KONT, &i.hash.bytes()),
                    hash: continuations_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertJoins(i)) => {
            let data = encode_joins(&i.joins);
            let joins_leaf = JoinsLeaf { bytes: data };
            let joins_hash = hash_single_byte_vec_leaf(&joins_leaf.bytes);
            let encoded = PersistedData::Joins(joins_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Joins),
                joins_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_JOINS, &i.hash.bytes()),
                    hash: joins_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryProduce(i)) => {
            // Sort data before serializing for deterministic hashing regardless of
            // insertion order
            let mut sorted_data = i.data;
            sorted_data.sort();
            let data = bincode::serialize(&sorted_data)
                .map_err(|error| HistoryError::ActionError(error.to_string()))?;
            let data_leaf = DataLeaf { bytes: data };
            let data_hash = hash_single_byte_vec_leaf(&data_leaf.bytes);
            let encoded = PersistedData::Data(data_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Data),
                data_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_DATUM, &i.hash.bytes()),
                    hash: data_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryConsume(i)) => {
            // Sort continuations before serializing for deterministic hashing regardless of
            // insertion order
            let mut sorted_continuations = i.continuations;
            sorted_continuations.sort();
            let data = bincode::serialize(&sorted_continuations)
                .map_err(|error| HistoryError::ActionError(error.to_string()))?;
            let continuations_leaf = ContinuationsLeaf { bytes: data };
            let continuations_hash = hash_single_byte_vec_leaf(&continuations_leaf.bytes);
            let encoded = PersistedData::Continuations(continuations_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Continuations),
                continuations_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_KONT, &i.hash.bytes()),
                    hash: continuations_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertBinaryJoins(i)) => {
            // Sort joins before serializing for deterministic hashing regardless of
            // insertion order
            let mut sorted_joins = i.joins;
            sorted_joins.sort();
            let data = bincode::serialize(&sorted_joins)
                .map_err(|error| HistoryError::ActionError(error.to_string()))?;
            let joins_leaf = JoinsLeaf { bytes: data };
            let joins_hash = hash_single_byte_vec_leaf(&joins_leaf.bytes);
            let encoded = PersistedData::Joins(joins_leaf).into_legacy_bincode();

            (
                Some(PersistedDataKind::Joins),
                joins_hash.clone(),
                Some(encoded),
                HistoryAction::Insert(InsertAction {
                    key: prepend_bytes(PREFIX_JOINS, &i.hash.bytes()),
                    hash: joins_hash,
                }),
            )
        }
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteProduce(d)) => (
            None,
            d.hash.clone(),
            None,
            HistoryAction::Delete(DeleteAction {
                key: prepend_bytes(PREFIX_DATUM, &d.hash.bytes()),
            }),
        ),
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteConsume(d)) => (
            None,
            d.hash.clone(),
            None,
            HistoryAction::Delete(DeleteAction {
                key: prepend_bytes(PREFIX_KONT, &d.hash.bytes()),
            }),
        ),
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteJoins(d)) => (
            None,
            d.hash.clone(),
            None,
            HistoryAction::Delete(DeleteAction {
                key: prepend_bytes(PREFIX_JOINS, &d.hash.bytes()),
            }),
        ),
        HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteCanonicalCollection(
            d,
        )) => (
            None,
            d.hash.clone(),
            None,
            HistoryAction::Delete(DeleteAction {
                key: prepend_bytes(canonical_collection_prefix(d.kind), &d.hash.bytes()),
            }),
        ),
    };
    let cold_action = match (kind, encoded) {
        (Some(kind), Some(encoded)) => {
            Some((ColdLeafAddress::new(kind, digest).map_err(cold_leaf_history_error)?, encoded))
        }
        (None, None) => None,
        _ => {
            return Err(HistoryError::ActionError(
                "cold-leaf action kind and bytes disagree".to_string(),
            ));
        }
    };
    Ok((cold_action, history_action))
}

pub(crate) fn transform<C, P, A, K>(
    hot_store_action: HotStoreAction<C, P, A, K>,
) -> HotStoreTrieAction<C, P, A, K>
where
    C: Clone + Send + Sync + StableHashSerialize,
    P: Clone + Send + Sync + StableHashSerialize,
    A: Clone + Send + Sync + StableHashSerialize,
    K: Clone + Send + Sync + StableHashSerialize,
{
    match hot_store_action {
        HotStoreAction::Insert(InsertData(i)) => {
            let key = hash(&i.channel);
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertProduce(
                TrieInsertProduce::new(key, i.data),
            ))
        }
        HotStoreAction::Insert(InsertCanonicalData(i)) => {
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertCanonicalProduce({
                let (hash, encoded_persisted_data, datum_count) = i.into_parts();
                TrieInsertCanonicalProduce {
                    hash,
                    encoded_persisted_data,
                    datum_count,
                }
            }))
        }
        HotStoreAction::Insert(InsertCanonicalCollection(i)) => {
            let (kind, hash, encoded_persisted_data, record_count) = i.into_parts();
            HotStoreTrieAction::TrieInsertAction(TrieInsertAction::TrieInsertCanonicalCollection(
                TrieInsertCanonicalCollection {
                    kind,
                    hash,
                    encoded_persisted_data,
                    record_count,
                },
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
        HotStoreAction::Delete(DeleteCanonicalData(d)) => HotStoreTrieAction::TrieDeleteAction(
            TrieDeleteAction::TrieDeleteProduce(TrieDeleteProduce::new(d.into_hash())),
        ),
        HotStoreAction::Delete(DeleteCanonicalCollection(d)) => {
            let (kind, hash) = d.into_parts();
            HotStoreTrieAction::TrieDeleteAction(TrieDeleteAction::TrieDeleteCanonicalCollection(
                TrieDeleteCanonicalCollection { kind, hash },
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

impl<C, P, A, K> HistoryRepository<C, P, A, K> for HistoryRepositoryImpl<C, P, A, K>
where
    C: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    P: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    A: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
    K: Clone + Send + Sync + StableHashSerialize + ColdStoreDecode + 'static,
{
    fn checkpoint(
        &self,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        if actions.is_empty() {
            return Ok(self.checkpoint_noop_clone());
        }

        self.measure(&actions);

        let trie_actions: Vec<_> = if Self::should_parallelize_checkpoint_actions(actions.len()) {
            actions.into_par_iter().map(transform).collect()
        } else {
            actions.into_iter().map(transform).collect()
        };

        self.do_checkpoint(trie_actions)
    }

    fn checkpoint_at(
        &self,
        expected: &CurrentRoot,
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        let actual = self.authority.current_verified_root()?.ok_or_else(|| {
            HistoryError::ActionError("RSpace authority has no current root".into())
        })?;
        if &actual != expected {
            return Err(HistoryError::RootError(RootError::RootCatalogConflict {
                expected_generation: Some(expected.generation()),
                actual_generation: Some(actual.generation()),
            }));
        }
        self.checkpoint(actions)
    }

    fn current_authority_root(&self) -> Result<CurrentRoot, HistoryError> {
        self.authority
            .current_verified_root()?
            .ok_or_else(|| HistoryError::ActionError("RSpace authority has no current root".into()))
    }

    fn do_checkpoint(
        &self,
        trie_actions: Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        if trie_actions.is_empty() {
            return Ok(self.checkpoint_noop_clone());
        }

        let storage_actions: Vec<(ColdAction, HistoryAction)> =
            if Self::should_parallelize_checkpoint_actions(trie_actions.len()) {
                trie_actions
                    .into_par_iter()
                    .map(calculate_storage_actions)
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                trie_actions
                    .into_iter()
                    .map(calculate_storage_actions)
                    .collect::<Result<Vec<_>, _>>()?
            };

        let mut cold_actions = Vec::new();
        let mut history_actions: Vec<HistoryAction> = Vec::with_capacity(storage_actions.len());
        for (cold_action, history) in storage_actions {
            if let Some(cold_action) = cold_action {
                cold_actions.push(cold_action);
            }
            history_actions.push(history);
        }

        let history_lock = self
            .current_history
            .lock()
            .expect("History Repository Impl: Unable to acquire history lock");
        let (new_history, prepared) = history_lock.prepare_checkpoint(history_actions)?;
        let validator = record_validator::<C, P, A, K>();
        self.authority
            .commit_local_checkpoint(&prepared, cold_actions, &validator)?;
        drop(history_lock);

        Ok(Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(new_history)),
            authority: self.authority.clone(),
            leaf_store: self.leaf_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        }))
    }

    fn reset(
        &self,
        root: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>, HistoryError> {
        debug!("[HistoryRepositoryImpl] reset to {}", root);

        let known = self.authority.known_verified_root(root)?;
        let next = {
            let history_lock = self
                .current_history
                .lock()
                .expect("History Repository Impl: Unable to acquire history lock");
            history_lock.reset(root)?
        };
        self.authority.activate_known(known)?;

        Ok(Box::new(HistoryRepositoryImpl {
            current_history: Arc::new(Mutex::new(next)),
            authority: self.authority.clone(),
            leaf_store: self.leaf_store.clone(),
            rspace_exporter: self.rspace_exporter.clone(),
            rspace_importer: self.rspace_importer.clone(),
            _marker: PhantomData,
        }))
    }

    fn exporter(&self) -> Arc<dyn RSpaceExporter> { self.rspace_exporter.clone() }

    fn importer(&self) -> Arc<dyn RSpaceImporter> { self.rspace_importer.clone() }

    fn get_history_reader(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>, HistoryError> {
        let history_lock = self
            .current_history
            .lock()
            .expect("History Repository Impl: Unable to acquire history lock");
        let history_repo = history_lock.reset(state_hash)?;
        Ok(Box::new(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone())))
    }

    fn get_history_reader_struct(
        &self,
        state_hash: &Blake2b256Hash,
    ) -> Result<RSpaceHistoryReaderImpl<C, P, A, K>, HistoryError> {
        let history_lock = self
            .current_history
            .lock()
            .expect("History Repository Impl: Unable to acquire history lock");
        let history_repo = history_lock.reset(state_hash)?;
        Ok(RSpaceHistoryReaderImpl::new(history_repo, self.leaf_store.clone()))
    }

    fn root(&self) -> Blake2b256Hash {
        let history_lock = self
            .current_history
            .lock()
            .expect("History Repository Impl: Unable to acquire history lock");
        history_lock.root()
    }

    fn contains_verified_root(&self, root: &Blake2b256Hash) -> Result<bool, HistoryError> {
        self.authority
            .contains_verified_root(root)
            .map_err(HistoryError::from)
    }
}

pub fn prepend_bytes(element: u8, _bytes: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(element);
    bytes.extend(_bytes.iter().cloned());
    bytes
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    fn legacy_leaf_hash(leaf: &impl Serialize) -> Blake2b256Hash {
        let encoded = bincode::serialize(leaf).unwrap();
        Blake2b256Hash::new(&encoded)
    }

    #[test]
    fn chunked_leaf_hash_is_exactly_the_frozen_bincode_hash() {
        for bytes in [Vec::new(), vec![0], (0u8..=254).collect(), vec![0xa5; 65_537]] {
            let expected = hash_single_byte_vec_leaf(&bytes);
            assert_eq!(
                expected,
                legacy_leaf_hash(&DataLeaf {
                    bytes: bytes.clone()
                })
            );
            assert_eq!(
                expected,
                legacy_leaf_hash(&ContinuationsLeaf {
                    bytes: bytes.clone(),
                })
            );
            assert_eq!(expected, legacy_leaf_hash(&JoinsLeaf { bytes }));
        }
    }

    #[test]
    fn chunked_leaf_hash_negative_control_rejects_a_missing_length_prefix() {
        let bytes = b"length-prefix-is-consensus-significant";
        assert_ne!(hash_single_byte_vec_leaf(bytes), Blake2b256Hash::new(bytes));
    }
}
