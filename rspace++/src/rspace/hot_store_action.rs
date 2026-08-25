use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    DataLeaf, PersistedData, PersistedDataKind, PersistedDataView,
};
use crate::rspace::internal::{Datum, WaitingContinuation};

// See rspace/src/main/scala/coop/rchain/rspace/HotStoreAction.scala
// PartialEq and Eq are needed for hot_store_spec tests
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotStoreAction<C: Clone, P: Clone, A: Clone, K: Clone> {
    Insert(InsertAction<C, P, A, K>),
    Delete(DeleteAction<C>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InsertAction<C: Clone, P: Clone, A: Clone, K: Clone> {
    InsertData(InsertData<C, A>),
    InsertCanonicalData(InsertCanonicalData),
    InsertJoins(InsertJoins<C>),
    InsertContinuations(InsertContinuations<C, P, K>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertData<C: Clone, A: Clone> {
    pub channel: C,
    pub data: Vec<Datum<A>>,
}

/// A checkpoint-ready `PersistedData::Data` value derived directly from
/// resident canonical datum identities. `encoded_persisted_data` includes the
/// frozen enum and leaf framing; it is not an ACT image or alternate format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertCanonicalData {
    pub hash: Blake2b256Hash,
    pub encoded_persisted_data: Vec<u8>,
    pub datum_count: u64,
}

impl InsertCanonicalData {
    /// Compatibility constructor for callers that already hold the frozen
    /// `Vec<Vec<u8>>` leaf payload. PathMap-native checkpoint egress bypasses
    /// this adapter and constructs `encoded_persisted_data` in one pass.
    pub fn from_encoded_datums(
        hash: Blake2b256Hash,
        encoded_datums: Vec<u8>,
        datum_count: u64,
    ) -> Self {
        Self {
            hash,
            encoded_persisted_data: PersistedData::Data(DataLeaf {
                bytes: encoded_datums,
            })
            .into_legacy_bincode(),
            datum_count,
        }
    }

    pub fn encoded_datums(&self) -> &[u8] {
        let view = PersistedDataView::parse(&self.encoded_persisted_data)
            .expect("canonical DATA action must contain a valid persisted envelope");
        assert_eq!(
            view.kind(),
            PersistedDataKind::Data,
            "canonical DATA action must contain the DATA persisted variant"
        );
        assert!(
            view.trailing().is_empty(),
            "canonical DATA action must not contain trailing bytes"
        );
        view.payload()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertJoins<C: Clone> {
    pub channel: C,
    pub joins: Vec<Vec<C>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertContinuations<C: Clone, P: Clone, K: Clone> {
    pub channels: Vec<C>,
    pub continuations: Vec<WaitingContinuation<P, K>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteAction<C: Clone> {
    DeleteData(DeleteData<C>),
    DeleteCanonicalData(DeleteCanonicalData),
    DeleteJoins(DeleteJoins<C>),
    DeleteContinuations(DeleteContinuations<C>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteData<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteCanonicalData {
    pub hash: Blake2b256Hash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteJoins<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteContinuations<C: Clone> {
    pub channels: Vec<C>,
}
