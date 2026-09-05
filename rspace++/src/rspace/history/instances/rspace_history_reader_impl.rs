use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;

use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::{StableHashSerialize, hash, hash_from_vec};
use crate::rspace::history::cold_store::{
    ColdLeafAddress, ColdLeafLocation, ColdLeafStoreError, PERSISTED_DATA_HEADER_LEN,
    PersistedDataKind, PersistedDataView, load_cold_leaf,
};
use crate::rspace::history::history::History;
use crate::rspace::history::history_reader::{HistoryReader, HistoryReaderBase};
use crate::rspace::history::history_repository::{PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use crate::rspace::history::history_repository_impl::prepend_bytes;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::metrics_constants::{
    HISTORY_FETCH_DATA_CALLS_METRIC, HISTORY_FETCH_DATA_DESERIALIZE_NS_METRIC,
    HISTORY_FETCH_DATA_LEAF_GET_NS_METRIC, HISTORY_FETCH_DATA_LEGACY_FALLBACK_METRIC,
    HISTORY_FETCH_DATA_TIME_NS_METRIC, HISTORY_FETCH_DATA_TRIE_READ_NS_METRIC,
    RSPACE_METRICS_SOURCE,
};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::serializers::serializers::{decode_continuations, decode_datums, decode_joins};

#[derive(Clone)]
pub struct RSpaceHistoryReaderImpl<C, P, A, K> {
    target_history: Arc<Box<dyn History>>,
    leaf_store: Arc<dyn KeyValueStore>,
    _marker: PhantomData<(C, P, A, K)>,
}

#[derive(Debug)]
struct FetchedPersistedData {
    bytes: Vec<u8>,
    kind: PersistedDataKind,
    payload: Range<usize>,
}

impl FetchedPersistedData {
    fn payload(&self) -> &[u8] { &self.bytes[self.payload.clone()] }
}

impl<C, P, A, K> RSpaceHistoryReaderImpl<C, P, A, K> {
    pub fn new(target_history: Box<dyn History>, leaf_store: Arc<dyn KeyValueStore>) -> Self {
        RSpaceHistoryReaderImpl {
            target_history: Arc::new(target_history),
            leaf_store,
            _marker: PhantomData,
        }
    }

    /** Fetch data on a hash pointer */
    fn fetch_data(
        &self,
        prefix: u8,
        key: &Blake2b256Hash,
    ) -> Result<Option<FetchedPersistedData>, HistoryError> {
        let __fetch_start = std::time::Instant::now();
        metrics::counter!(HISTORY_FETCH_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);

        let __trie_start = std::time::Instant::now();
        let history_key = prepend_bytes(prefix, &key.bytes());
        let read_bytes = self.target_history.read(history_key.clone())?;
        metrics::counter!(HISTORY_FETCH_DATA_TRIE_READ_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__trie_start.elapsed().as_nanos() as u64);

        let result = match read_bytes {
            Some(ref bytes) => {
                let __leaf_start = std::time::Instant::now();
                let address = ColdLeafAddress::from_history_prefix(
                    &history_key,
                    Blake2b256Hash::from_bytes(bytes.clone()),
                )
                .map_err(cold_leaf_history_error)?;
                let loaded = load_cold_leaf(self.leaf_store.as_ref(), &address)
                    .map_err(cold_leaf_history_error)?;
                if loaded.location() != ColdLeafLocation::Typed {
                    metrics::counter!(HISTORY_FETCH_DATA_LEGACY_FALLBACK_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                }
                metrics::counter!(HISTORY_FETCH_DATA_LEAF_GET_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(__leaf_start.elapsed().as_nanos() as u64);

                let __deser_start = std::time::Instant::now();
                let store_value_bytes = loaded.into_bytes();
                let view = PersistedDataView::parse_canonical(&store_value_bytes)
                    .map_err(|error| HistoryError::DecodeError(error.to_string()))?;
                let kind = view.kind();
                let payload_end = PERSISTED_DATA_HEADER_LEN + view.payload().len();
                metrics::counter!(HISTORY_FETCH_DATA_DESERIALIZE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(__deser_start.elapsed().as_nanos() as u64);
                Ok(Some(FetchedPersistedData {
                    bytes: store_value_bytes,
                    kind,
                    payload: PERSISTED_DATA_HEADER_LEN..payload_end,
                }))
            }
            None => Ok(None),
        };
        metrics::counter!(HISTORY_FETCH_DATA_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__fetch_start.elapsed().as_nanos() as u64);
        result
    }
}

fn cold_leaf_history_error(error: ColdLeafStoreError) -> HistoryError {
    match error {
        ColdLeafStoreError::Store(error) => HistoryError::KvStoreError(error),
        error => HistoryError::DecodeError(error.to_string()),
    }
}

impl<C, P, A, K> HistoryReader<Blake2b256Hash, C, P, A, K> for RSpaceHistoryReaderImpl<C, P, A, K>
where
    C: Clone + ColdStoreDecode + StableHashSerialize + 'static + Sync + Send,
    P: Clone + ColdStoreDecode + 'static + Sync + Send,
    A: Clone + ColdStoreDecode + 'static + Sync + Send,
    K: Clone + ColdStoreDecode + 'static + Sync + Send,
{
    fn root(&self) -> Blake2b256Hash { self.target_history.root() }

    fn get_data_proj(&self, key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, HistoryError> {
        match self.fetch_data(PREFIX_DATUM, key)? {
            Some(data) if data.kind == PersistedDataKind::Data => {
                decode_datums(data.payload()).map_err(|e| HistoryError::DecodeError(e.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for data at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn get_data_proj_binary(&self, key: &Blake2b256Hash) -> Result<Vec<Vec<u8>>, HistoryError> {
        match self.fetch_data(PREFIX_DATUM, key)? {
            Some(data) if data.kind == PersistedDataKind::Data => {
                bincode::deserialize(data.payload())
                    .map_err(|error| HistoryError::DecodeError(error.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for data at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn get_continuations_proj(
        &self,
        key: &Blake2b256Hash,
    ) -> Result<Vec<WaitingContinuation<P, K>>, HistoryError> {
        match self.fetch_data(PREFIX_KONT, key)? {
            Some(data) if data.kind == PersistedDataKind::Continuations => {
                decode_continuations(data.payload())
                    .map_err(|e| HistoryError::DecodeError(e.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for continuations at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn get_continuations_proj_binary(
        &self,
        key: &Blake2b256Hash,
    ) -> Result<Vec<Vec<u8>>, HistoryError> {
        match self.fetch_data(PREFIX_KONT, key)? {
            Some(data) if data.kind == PersistedDataKind::Continuations => {
                bincode::deserialize(data.payload())
                    .map_err(|error| HistoryError::DecodeError(error.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for continuations at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn get_joins_proj(&self, key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, HistoryError> {
        match self.fetch_data(PREFIX_JOINS, key)? {
            Some(data) if data.kind == PersistedDataKind::Joins => {
                decode_joins(data.payload()).map_err(|e| HistoryError::DecodeError(e.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for joins at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn get_joins_proj_binary(&self, key: &Blake2b256Hash) -> Result<Vec<Vec<u8>>, HistoryError> {
        match self.fetch_data(PREFIX_JOINS, key)? {
            Some(data) if data.kind == PersistedDataKind::Joins => {
                bincode::deserialize(data.payload())
                    .map_err(|error| HistoryError::DecodeError(error.to_string()))
            }
            Some(p) => Err(HistoryError::ActionError(format!(
                "Found unexpected {:?} leaf while looking for joins at key {:?}",
                p.kind, key
            ))),
            None => Ok(Vec::new()),
        }
    }

    fn base(&self) -> Box<dyn HistoryReaderBase<C, P, A, K>> {
        struct HistoryReaderBaseImpl<C, P, A, K> {
            outer: Arc<RSpaceHistoryReaderImpl<C, P, A, K>>,
        }

        impl<C, P, A, K> HistoryReaderBase<C, P, A, K> for HistoryReaderBaseImpl<C, P, A, K>
        where
            C: Clone + ColdStoreDecode + StableHashSerialize + 'static + Sync + Send,
            P: Clone + ColdStoreDecode + 'static + Sync + Send,
            A: Clone + ColdStoreDecode + 'static + Sync + Send,
            K: Clone + ColdStoreDecode + 'static + Sync + Send,
        {
            fn get_data_canonical(&self, key: &C) -> Vec<Datum<A>> {
                self.outer
                    .get_data_proj(&hash(key))
                    .expect("Failed to get data proj")
            }

            fn get_continuations_canonical(&self, key: &[C]) -> Vec<WaitingContinuation<P, K>> {
                self.outer
                    .get_continuations_proj(&hash_from_vec(&key.to_vec()))
                    .expect("Failed to get continuations proj")
            }

            fn get_joins_canonical(&self, key: &C) -> Vec<Vec<C>> {
                self.outer
                    .get_joins_proj(&hash(key))
                    .expect("Failed to get joins proj")
            }
        }

        let outer_arc = Arc::new(self.clone());
        Box::new(HistoryReaderBaseImpl { outer: outer_arc })
    }

    fn get_data_proj_generic(&self, key: &C) -> Vec<Datum<A>> {
        self.get_data_proj(&hash(key))
            .expect("Failed to get data proj")
    }

    fn get_continuations_proj_generic(&self, key: &Vec<C>) -> Vec<WaitingContinuation<P, K>> {
        self.get_continuations_proj(&hash_from_vec(key))
            .expect("Failed to get continuations proj")
    }

    fn get_joins_proj_generic(&self, key: &C) -> Vec<Vec<C>> {
        self.get_joins_proj(&hash(key))
            .expect("Failed to get joins proj")
    }
}
