use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    CanonicalCollectionKind, ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData,
    PersistedDataFrameError, PersistedDataKind, PersistedDataView, PersistedRecordSequenceView,
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
    InsertCanonicalCollection(InsertCanonicalCollection),
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
    hash: Blake2b256Hash,
    encoded_persisted_data: Vec<u8>,
    datum_count: u64,
}

/// Checkpoint-ready continuation or join leaf emitted directly from its
/// authoritative PathMap endpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertCanonicalCollection {
    kind: CanonicalCollectionKind,
    hash: Blake2b256Hash,
    encoded_persisted_data: Vec<u8>,
    record_count: u64,
}

impl InsertCanonicalData {
    /// Compatibility constructor for callers that already hold the frozen
    /// `Vec<Vec<u8>>` leaf payload. PathMap-native checkpoint egress bypasses
    /// this adapter and constructs `encoded_persisted_data` in one pass.
    pub fn try_from_encoded_datums(
        hash: Blake2b256Hash,
        encoded_datums: Vec<u8>,
        datum_count: u64,
    ) -> Result<Self, PersistedDataFrameError> {
        let action = Self {
            hash,
            encoded_persisted_data: PersistedData::Data(DataLeaf {
                bytes: encoded_datums,
            })
            .into_legacy_bincode(),
            datum_count,
        };
        PersistedRecordSequenceView::parse_canonical(
            &action.encoded_persisted_data,
            PersistedDataKind::Data,
            Some(action.datum_count),
        )?;
        Ok(action)
    }

    pub(crate) fn from_trusted_parts(
        hash: Blake2b256Hash,
        encoded_persisted_data: Vec<u8>,
        datum_count: u64,
    ) -> Self {
        Self {
            hash,
            encoded_persisted_data,
            datum_count,
        }
    }

    pub fn hash(&self) -> &Blake2b256Hash { &self.hash }

    pub fn encoded_persisted_data(&self) -> &[u8] { &self.encoded_persisted_data }

    pub const fn datum_count(&self) -> u64 { self.datum_count }

    pub fn encoded_datums(&self) -> &[u8] {
        let view = PersistedDataView::parse_canonical(&self.encoded_persisted_data)
            .expect("canonical DATA action must contain a valid persisted envelope");
        assert_eq!(
            view.kind(),
            PersistedDataKind::Data,
            "canonical DATA action must contain the DATA persisted variant"
        );
        view.payload()
    }

    pub(crate) fn into_parts(self) -> (Blake2b256Hash, Vec<u8>, u64) {
        (self.hash, self.encoded_persisted_data, self.datum_count)
    }
}

impl InsertCanonicalCollection {
    pub fn try_from_encoded_joins(
        hash: Blake2b256Hash,
        encoded_joins: Vec<u8>,
        record_count: u64,
    ) -> Result<Self, PersistedDataFrameError> {
        Self::try_from_encoded_records(
            CanonicalCollectionKind::Joins,
            hash,
            PersistedData::Joins(JoinsLeaf {
                bytes: encoded_joins,
            })
            .into_legacy_bincode(),
            record_count,
        )
    }

    pub fn try_from_encoded_continuations(
        hash: Blake2b256Hash,
        encoded_continuations: Vec<u8>,
        record_count: u64,
    ) -> Result<Self, PersistedDataFrameError> {
        Self::try_from_encoded_records(
            CanonicalCollectionKind::Continuations,
            hash,
            PersistedData::Continuations(ContinuationsLeaf {
                bytes: encoded_continuations,
            })
            .into_legacy_bincode(),
            record_count,
        )
    }

    fn try_from_encoded_records(
        kind: CanonicalCollectionKind,
        hash: Blake2b256Hash,
        encoded_persisted_data: Vec<u8>,
        record_count: u64,
    ) -> Result<Self, PersistedDataFrameError> {
        PersistedRecordSequenceView::parse_canonical(
            &encoded_persisted_data,
            kind.persisted_kind(),
            Some(record_count),
        )?;
        Ok(Self {
            kind,
            hash,
            encoded_persisted_data,
            record_count,
        })
    }

    pub(crate) fn from_trusted_parts(
        kind: CanonicalCollectionKind,
        hash: Blake2b256Hash,
        encoded_persisted_data: Vec<u8>,
        record_count: u64,
    ) -> Self {
        Self {
            kind,
            hash,
            encoded_persisted_data,
            record_count,
        }
    }

    pub fn persisted_kind(&self) -> PersistedDataKind { self.kind.persisted_kind() }

    pub(crate) const fn kind(&self) -> CanonicalCollectionKind { self.kind }

    pub fn hash(&self) -> &Blake2b256Hash { &self.hash }

    pub fn encoded_persisted_data(&self) -> &[u8] { &self.encoded_persisted_data }

    pub const fn record_count(&self) -> u64 { self.record_count }

    pub fn encoded_records(&self) -> &[u8] {
        let view = PersistedRecordSequenceView::parse_canonical(
            &self.encoded_persisted_data,
            self.kind.persisted_kind(),
            Some(self.record_count),
        )
        .expect("trusted canonical collection action contains valid records");
        view.payload()
    }

    pub(crate) fn into_parts(self) -> (CanonicalCollectionKind, Blake2b256Hash, Vec<u8>, u64) {
        (self.kind, self.hash, self.encoded_persisted_data, self.record_count)
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
    DeleteCanonicalCollection(DeleteCanonicalCollection),
    DeleteJoins(DeleteJoins<C>),
    DeleteContinuations(DeleteContinuations<C>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteData<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteCanonicalData {
    hash: Blake2b256Hash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteCanonicalCollection {
    kind: CanonicalCollectionKind,
    hash: Blake2b256Hash,
}

impl DeleteCanonicalData {
    pub fn new(hash: Blake2b256Hash) -> Self { Self { hash } }

    pub fn hash(&self) -> &Blake2b256Hash { &self.hash }

    pub(crate) fn into_hash(self) -> Blake2b256Hash { self.hash }
}

impl DeleteCanonicalCollection {
    pub(crate) fn new(kind: CanonicalCollectionKind, hash: Blake2b256Hash) -> Self {
        Self { kind, hash }
    }

    pub fn persisted_kind(&self) -> PersistedDataKind { self.kind.persisted_kind() }

    pub fn joins(hash: Blake2b256Hash) -> Self { Self::new(CanonicalCollectionKind::Joins, hash) }

    pub fn continuations(hash: Blake2b256Hash) -> Self {
        Self::new(CanonicalCollectionKind::Continuations, hash)
    }

    pub(crate) const fn kind(&self) -> CanonicalCollectionKind { self.kind }

    pub fn hash(&self) -> &Blake2b256Hash { &self.hash }

    pub(crate) fn into_parts(self) -> (CanonicalCollectionKind, Blake2b256Hash) {
        (self.kind, self.hash)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteJoins<C: Clone> {
    pub channel: C,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteContinuations<C: Clone> {
    pub channels: Vec<C>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash() -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![0x5a; 32]) }

    fn one_record() -> Vec<u8> { bincode::serialize(&vec![b"record".to_vec()]).unwrap() }

    #[test]
    fn validated_canonical_action_constructors_accept_exact_record_sequences() {
        let data = InsertCanonicalData::try_from_encoded_datums(hash(), one_record(), 1).unwrap();
        assert_eq!(data.datum_count(), 1);
        assert_eq!(data.encoded_datums(), one_record());

        let joins =
            InsertCanonicalCollection::try_from_encoded_joins(hash(), one_record(), 1).unwrap();
        assert_eq!(joins.persisted_kind(), PersistedDataKind::Joins);
        assert_eq!(joins.record_count(), 1);
        assert_eq!(joins.encoded_records(), one_record());

        let continuations =
            InsertCanonicalCollection::try_from_encoded_continuations(hash(), one_record(), 1)
                .unwrap();
        assert_eq!(continuations.persisted_kind(), PersistedDataKind::Continuations);
        assert_eq!(continuations.record_count(), 1);
        assert_eq!(continuations.encoded_records(), one_record());
    }

    #[test]
    fn validated_canonical_action_constructors_reject_count_mismatches() {
        for error in [
            InsertCanonicalData::try_from_encoded_datums(hash(), one_record(), 2).unwrap_err(),
            InsertCanonicalCollection::try_from_encoded_joins(hash(), one_record(), 2).unwrap_err(),
            InsertCanonicalCollection::try_from_encoded_continuations(hash(), one_record(), 2)
                .unwrap_err(),
        ] {
            assert_eq!(error, PersistedDataFrameError::RecordCountMismatch {
                expected: 2,
                actual: 1,
            });
        }
    }

    #[test]
    fn validated_canonical_action_constructors_reject_malformed_inner_frames() {
        let truncated_count = vec![0; 7];
        assert_eq!(
            InsertCanonicalData::try_from_encoded_datums(hash(), truncated_count, 0),
            Err(PersistedDataFrameError::TruncatedRecordCount { available: 7 })
        );

        let mut truncated_record = 1_u64.to_le_bytes().to_vec();
        truncated_record.extend_from_slice(&4_u64.to_le_bytes());
        truncated_record.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            InsertCanonicalCollection::try_from_encoded_joins(hash(), truncated_record, 1),
            Err(PersistedDataFrameError::TruncatedRecord {
                index: 0,
                declared: 4,
                available: 3,
            })
        );

        let mut trailing = 0_u64.to_le_bytes().to_vec();
        trailing.push(0xff);
        assert_eq!(
            InsertCanonicalCollection::try_from_encoded_continuations(hash(), trailing, 0),
            Err(PersistedDataFrameError::TrailingRecordBytes { available: 1 })
        );
    }
}
