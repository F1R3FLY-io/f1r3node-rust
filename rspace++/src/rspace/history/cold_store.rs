use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

pub(crate) const PERSISTED_DATA_HEADER_LEN: usize =
    std::mem::size_of::<u32>() + std::mem::size_of::<u64>();
const PERSISTED_DATA_TAG_LEN: usize = std::mem::size_of::<u32>();

/// The frozen bincode-1 discriminant of an RSpace cold-store leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PersistedDataKind {
    Joins,
    Data,
    Continuations,
}

const COLD_LEAF_ADDRESS_DOMAIN: &[u8; 4] = b"RSLF";
const COLD_LEAF_ADDRESS_VERSION: u8 = 1;
const COLD_LEAF_DIGEST_LEN: usize = 32;
pub const COLD_LEAF_ADDRESS_LEN: usize =
    COLD_LEAF_ADDRESS_DOMAIN.len() + 1 + 1 + COLD_LEAF_DIGEST_LEN;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColdLeafAddress {
    kind: PersistedDataKind,
    digest: Blake2b256Hash,
}

impl ColdLeafAddress {
    pub fn new(
        kind: PersistedDataKind,
        digest: Blake2b256Hash,
    ) -> Result<Self, ColdLeafStoreError> {
        if digest.0.len() != COLD_LEAF_DIGEST_LEN {
            return Err(ColdLeafStoreError::InvalidDigestLength {
                available: digest.0.len(),
            });
        }
        Ok(Self { kind, digest })
    }

    pub fn from_history_prefix(
        prefix: &[u8],
        digest: Blake2b256Hash,
    ) -> Result<Self, ColdLeafStoreError> {
        if prefix.len() != 1 + COLD_LEAF_DIGEST_LEN {
            return Err(ColdLeafStoreError::InvalidHistoryPrefixLength {
                available: prefix.len(),
            });
        }
        Self::new(PersistedDataKind::from_history_tag(prefix[0])?, digest)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ColdLeafStoreError> {
        if bytes.len() != COLD_LEAF_ADDRESS_LEN {
            return Err(ColdLeafStoreError::InvalidAddressLength {
                available: bytes.len(),
            });
        }
        if &bytes[..COLD_LEAF_ADDRESS_DOMAIN.len()] != COLD_LEAF_ADDRESS_DOMAIN {
            return Err(ColdLeafStoreError::InvalidAddressDomain);
        }
        let version = bytes[COLD_LEAF_ADDRESS_DOMAIN.len()];
        if version != COLD_LEAF_ADDRESS_VERSION {
            return Err(ColdLeafStoreError::UnsupportedAddressVersion(version));
        }
        let tag_offset = COLD_LEAF_ADDRESS_DOMAIN.len() + 1;
        let kind = PersistedDataKind::from_physical_tag(bytes[tag_offset])?;
        let digest = Blake2b256Hash::from_bytes(bytes[tag_offset + 1..].to_vec());
        Self::new(kind, digest)
    }

    pub fn kind(&self) -> PersistedDataKind { self.kind }

    pub fn digest(&self) -> &Blake2b256Hash { &self.digest }

    pub fn into_digest(self) -> Blake2b256Hash { self.digest }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(COLD_LEAF_ADDRESS_LEN);
        bytes.extend_from_slice(COLD_LEAF_ADDRESS_DOMAIN);
        bytes.push(COLD_LEAF_ADDRESS_VERSION);
        bytes.push(self.kind.physical_tag());
        bytes.extend_from_slice(&self.digest.0);
        bytes
    }

    fn legacy_raw_key(&self) -> Vec<u8> { self.digest.0.clone() }

    fn legacy_bincode_key(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(std::mem::size_of::<u64>() + COLD_LEAF_DIGEST_LEN);
        bytes.extend_from_slice(&(COLD_LEAF_DIGEST_LEN as u64).to_le_bytes());
        bytes.extend_from_slice(&self.digest.0);
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColdLeafLocation {
    Typed,
    LegacyRawDigest,
    LegacyBincodeDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedColdLeaf {
    address: ColdLeafAddress,
    bytes: Vec<u8>,
    location: ColdLeafLocation,
}

impl LoadedColdLeaf {
    pub fn address(&self) -> &ColdLeafAddress { &self.address }

    pub fn bytes(&self) -> &[u8] { &self.bytes }

    pub fn into_bytes(self) -> Vec<u8> { self.bytes }

    pub fn location(&self) -> ColdLeafLocation { self.location }
}

/// The two PathMap-authoritative grouped collections that can emit their
/// frozen cold-store representation without projecting typed vectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalCollectionKind {
    Joins,
    Continuations,
}

impl CanonicalCollectionKind {
    pub(crate) const fn persisted_kind(self) -> PersistedDataKind {
        match self {
            Self::Joins => PersistedDataKind::Joins,
            Self::Continuations => PersistedDataKind::Continuations,
        }
    }
}

impl PersistedDataKind {
    const fn discriminant(self) -> u32 {
        match self {
            Self::Joins => 0,
            Self::Data => 1,
            Self::Continuations => 2,
        }
    }

    pub(crate) fn header(self, payload_len: u64) -> [u8; PERSISTED_DATA_HEADER_LEN] {
        let mut header = [0; PERSISTED_DATA_HEADER_LEN];
        header[..PERSISTED_DATA_TAG_LEN].copy_from_slice(&self.discriminant().to_le_bytes());
        header[PERSISTED_DATA_TAG_LEN..].copy_from_slice(&payload_len.to_le_bytes());
        header
    }

    pub(crate) fn write_header(self, payload_len: u64, output: &mut Vec<u8>) {
        output.extend_from_slice(&self.header(payload_len));
    }

    fn from_discriminant(discriminant: u32) -> Result<Self, PersistedDataFrameError> {
        match discriminant {
            0 => Ok(Self::Joins),
            1 => Ok(Self::Data),
            2 => Ok(Self::Continuations),
            value => Err(PersistedDataFrameError::InvalidDiscriminant(value)),
        }
    }

    const fn physical_tag(self) -> u8 {
        match self {
            Self::Data => 0,
            Self::Continuations => 1,
            Self::Joins => 2,
        }
    }

    fn from_physical_tag(tag: u8) -> Result<Self, ColdLeafStoreError> {
        match tag {
            0 => Ok(Self::Data),
            1 => Ok(Self::Continuations),
            2 => Ok(Self::Joins),
            value => Err(ColdLeafStoreError::InvalidPhysicalKindTag(value)),
        }
    }

    fn from_history_tag(tag: u8) -> Result<Self, ColdLeafStoreError> {
        match tag {
            0 => Ok(Self::Data),
            1 => Ok(Self::Continuations),
            2 => Ok(Self::Joins),
            value => Err(ColdLeafStoreError::InvalidHistoryKindTag(value)),
        }
    }
}

pub fn validate_cold_leaf<'a>(
    address: &ColdLeafAddress,
    bytes: &'a [u8],
) -> Result<PersistedRecordSequenceView<'a>, ColdLeafStoreError> {
    let view = PersistedRecordSequenceView::parse_canonical(bytes, address.kind, None)
        .map_err(|error| ColdLeafStoreError::InvalidEnvelope(error.to_string()))?;
    let actual = Blake2b256Hash::new(view.leaf_encoding());
    if actual != address.digest {
        return Err(ColdLeafStoreError::DigestMismatch {
            expected: address.digest.clone(),
            actual,
        });
    }
    Ok(view)
}

pub fn load_cold_leaf(
    store: &dyn KeyValueStore,
    address: &ColdLeafAddress,
) -> Result<LoadedColdLeaf, ColdLeafStoreError> {
    let candidates = [
        (address.encode(), ColdLeafLocation::Typed),
        (address.legacy_raw_key(), ColdLeafLocation::LegacyRawDigest),
        (address.legacy_bincode_key(), ColdLeafLocation::LegacyBincodeDigest),
    ];
    for (key, location) in candidates {
        if let Some(bytes) = store.get_one(&key)? {
            validate_cold_leaf(address, &bytes)?;
            return Ok(LoadedColdLeaf {
                address: address.clone(),
                bytes,
                location,
            });
        }
    }
    Err(ColdLeafStoreError::MissingObject(address.clone()))
}

pub fn put_cold_leaves_if_absent_verified(
    store: &dyn KeyValueStore,
    leaves: Vec<(ColdLeafAddress, Vec<u8>)>,
) -> Result<(), ColdLeafStoreError> {
    let mut unique = std::collections::BTreeMap::new();
    for (address, bytes) in leaves {
        validate_cold_leaf(&address, &bytes)?;
        match unique.entry(address) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(bytes);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &bytes => {}
            std::collections::btree_map::Entry::Occupied(entry) => {
                return Err(ColdLeafStoreError::ContentCollision(entry.key().clone()));
            }
        }
    }

    let entries = unique
        .into_iter()
        .map(|(address, bytes)| (address.encode(), bytes))
        .collect();
    store.put_if_absent_or_verify_equal(entries)?;
    Ok(())
}

/// A validated borrowed view of the non-recursive bincode cold-store envelope.
///
/// `leaf_encoding` is the exact hash authority (the encoded single-field leaf
/// without the outer enum tag); `payload` is the already-encoded datum,
/// continuation, or join collection. Authoritative storage paths use
/// [`Self::parse_canonical`]. The private compatibility parser exists only so
/// executable differentials can pin bincode 1's trailing-byte behavior without
/// making that permissive language available to production callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistedDataView<'a> {
    kind: PersistedDataKind,
    leaf_encoding: &'a [u8],
    payload: &'a [u8],
    trailing: &'a [u8],
}

impl<'a> PersistedDataView<'a> {
    fn parse_bincode_compatible(bytes: &'a [u8]) -> Result<Self, PersistedDataFrameError> {
        if bytes.len() < PERSISTED_DATA_HEADER_LEN {
            return Err(PersistedDataFrameError::TruncatedHeader {
                available: bytes.len(),
            });
        }

        let discriminant = u32::from_le_bytes(
            bytes[..PERSISTED_DATA_TAG_LEN]
                .try_into()
                .expect("the persisted-data tag range has four bytes"),
        );
        let kind = PersistedDataKind::from_discriminant(discriminant)?;
        let payload_len_u64 = u64::from_le_bytes(
            bytes[PERSISTED_DATA_TAG_LEN..PERSISTED_DATA_HEADER_LEN]
                .try_into()
                .expect("the persisted-data length range has eight bytes"),
        );
        let payload_len = usize::try_from(payload_len_u64)
            .map_err(|_| PersistedDataFrameError::LengthOverflow(payload_len_u64))?;
        let payload_end = PERSISTED_DATA_HEADER_LEN
            .checked_add(payload_len)
            .ok_or(PersistedDataFrameError::LengthOverflow(payload_len_u64))?;
        if payload_end > bytes.len() {
            return Err(PersistedDataFrameError::TruncatedPayload {
                declared: payload_len_u64,
                available: bytes.len() - PERSISTED_DATA_HEADER_LEN,
            });
        }

        Ok(Self {
            kind,
            leaf_encoding: &bytes[PERSISTED_DATA_TAG_LEN..payload_end],
            payload: &bytes[PERSISTED_DATA_HEADER_LEN..payload_end],
            trailing: &bytes[payload_end..],
        })
    }

    /// Parse a value produced by the canonical writer rather than a merely
    /// bincode-compatible value. Bincode 1 admits trailing bytes, but a
    /// canonical cold-store value has exactly one envelope and no suffix.
    pub fn parse_canonical(bytes: &'a [u8]) -> Result<Self, PersistedDataFrameError> {
        let view = Self::parse_bincode_compatible(bytes)?;
        if view.trailing.is_empty() {
            Ok(view)
        } else {
            Err(PersistedDataFrameError::TrailingBytes {
                available: view.trailing.len(),
            })
        }
    }

    pub fn kind(self) -> PersistedDataKind { self.kind }

    pub fn leaf_encoding(self) -> &'a [u8] { self.leaf_encoding }

    pub fn payload(self) -> &'a [u8] { self.payload }

    pub fn trailing(self) -> &'a [u8] { self.trailing }

    /// Validate the frozen `Vec<Vec<u8>>` payload without decoding a record or
    /// allocating an entry projection.
    pub fn validate_records(
        self,
        expected_kind: PersistedDataKind,
        expected_count: Option<u64>,
    ) -> Result<PersistedRecordSequenceView<'a>, PersistedDataFrameError> {
        if self.kind != expected_kind {
            return Err(PersistedDataFrameError::UnexpectedKind {
                expected: expected_kind,
                actual: self.kind,
            });
        }
        PersistedRecordSequenceView::validate(self, expected_count)
    }
}

/// Validated, allocation-free view of a frozen `Vec<Vec<u8>>` leaf payload.
///
/// Construction proves that `record_count` length-prefixed records consume the
/// payload exactly. Iteration can therefore return borrowed record bodies with
/// no fallible branch beyond ordinary iterator termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistedRecordSequenceView<'a> {
    envelope: PersistedDataView<'a>,
    record_count: u64,
}

impl<'a> PersistedRecordSequenceView<'a> {
    pub fn parse_canonical(
        bytes: &'a [u8],
        expected_kind: PersistedDataKind,
        expected_count: Option<u64>,
    ) -> Result<Self, PersistedDataFrameError> {
        PersistedDataView::parse_canonical(bytes)?.validate_records(expected_kind, expected_count)
    }

    fn validate(
        envelope: PersistedDataView<'a>,
        expected_count: Option<u64>,
    ) -> Result<Self, PersistedDataFrameError> {
        let payload = envelope.payload;
        if payload.len() < std::mem::size_of::<u64>() {
            return Err(PersistedDataFrameError::TruncatedRecordCount {
                available: payload.len(),
            });
        }
        let record_count = u64::from_le_bytes(
            payload[..std::mem::size_of::<u64>()]
                .try_into()
                .expect("the record-count range has eight bytes"),
        );
        let mut cursor = std::mem::size_of::<u64>();
        for index in 0..record_count {
            let available = payload.len().saturating_sub(cursor);
            if available < std::mem::size_of::<u64>() {
                return Err(PersistedDataFrameError::TruncatedRecordLength { index, available });
            }
            let declared = u64::from_le_bytes(
                payload[cursor..cursor + std::mem::size_of::<u64>()]
                    .try_into()
                    .expect("the record-length range has eight bytes"),
            );
            cursor += std::mem::size_of::<u64>();
            let length = usize::try_from(declared)
                .map_err(|_| PersistedDataFrameError::RecordLengthOverflow { index, declared })?;
            let end = cursor
                .checked_add(length)
                .ok_or(PersistedDataFrameError::RecordLengthOverflow { index, declared })?;
            if end > payload.len() {
                return Err(PersistedDataFrameError::TruncatedRecord {
                    index,
                    declared,
                    available: payload.len().saturating_sub(cursor),
                });
            }
            cursor = end;
        }
        if cursor != payload.len() {
            return Err(PersistedDataFrameError::TrailingRecordBytes {
                available: payload.len() - cursor,
            });
        }
        if let Some(expected) = expected_count {
            if record_count != expected {
                return Err(PersistedDataFrameError::RecordCountMismatch {
                    expected,
                    actual: record_count,
                });
            }
        }
        Ok(Self {
            envelope,
            record_count,
        })
    }

    pub fn kind(self) -> PersistedDataKind { self.envelope.kind }

    pub fn leaf_encoding(self) -> &'a [u8] { self.envelope.leaf_encoding }

    pub fn payload(self) -> &'a [u8] { self.envelope.payload }

    pub const fn record_count(self) -> u64 { self.record_count }

    pub fn records(self) -> PersistedRecordIter<'a> {
        PersistedRecordIter {
            payload: self.envelope.payload,
            cursor: std::mem::size_of::<u64>(),
            remaining: self.record_count,
        }
    }
}

pub struct PersistedRecordIter<'a> {
    payload: &'a [u8],
    cursor: usize,
    remaining: u64,
}

impl<'a> Iterator for PersistedRecordIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let length = u64::from_le_bytes(
            self.payload[self.cursor..self.cursor + std::mem::size_of::<u64>()]
                .try_into()
                .expect("validated record length is present"),
        );
        self.cursor += std::mem::size_of::<u64>();
        let length = usize::try_from(length).expect("validated record length fits usize");
        let end = self.cursor + length;
        let record = &self.payload[self.cursor..end];
        self.cursor = end;
        self.remaining -= 1;
        Some(record)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.remaining)
            .expect("a validated in-memory record count must fit usize");
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for PersistedRecordIter<'_> {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistedDataFrameError {
    TruncatedHeader {
        available: usize,
    },
    InvalidDiscriminant(u32),
    LengthOverflow(u64),
    TruncatedPayload {
        declared: u64,
        available: usize,
    },
    TrailingBytes {
        available: usize,
    },
    UnexpectedKind {
        expected: PersistedDataKind,
        actual: PersistedDataKind,
    },
    TruncatedRecordCount {
        available: usize,
    },
    RecordCountMismatch {
        expected: u64,
        actual: u64,
    },
    TruncatedRecordLength {
        index: u64,
        available: usize,
    },
    RecordLengthOverflow {
        index: u64,
        declared: u64,
    },
    TruncatedRecord {
        index: u64,
        declared: u64,
        available: usize,
    },
    TrailingRecordBytes {
        available: usize,
    },
}

impl std::fmt::Display for PersistedDataFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedHeader { available } => write!(
                f,
                "persisted-data header is truncated: expected {PERSISTED_DATA_HEADER_LEN} bytes, \
                 found {available}"
            ),
            Self::InvalidDiscriminant(value) => {
                write!(f, "persisted-data discriminant {value} is outside the frozen 0..=2 range")
            }
            Self::LengthOverflow(value) => {
                write!(f, "persisted-data payload length {value} does not fit this platform")
            }
            Self::TruncatedPayload {
                declared,
                available,
            } => write!(
                f,
                "persisted-data payload is truncated: declared {declared} bytes, found {available}"
            ),
            Self::TrailingBytes { available } => {
                write!(f, "canonical persisted-data value has {available} trailing bytes")
            }
            Self::UnexpectedKind { expected, actual } => {
                write!(f, "persisted-data kind {actual:?} differs from expected {expected:?}")
            }
            Self::TruncatedRecordCount { available } => write!(
                f,
                "persisted record count is truncated: expected 8 bytes, found {available}"
            ),
            Self::RecordCountMismatch { expected, actual } => {
                write!(f, "persisted record count {actual} differs from action metadata {expected}")
            }
            Self::TruncatedRecordLength { index, available } => write!(
                f,
                "persisted record {index} length is truncated: expected 8 bytes, found {available}"
            ),
            Self::RecordLengthOverflow { index, declared } => {
                write!(f, "persisted record {index} length {declared} does not fit this platform")
            }
            Self::TruncatedRecord {
                index,
                declared,
                available,
            } => write!(
                f,
                "persisted record {index} is truncated: declared {declared} bytes, found \
                 {available}"
            ),
            Self::TrailingRecordBytes { available } => {
                write!(f, "persisted record sequence has {available} trailing bytes")
            }
        }
    }
}

impl std::error::Error for PersistedDataFrameError {}

#[derive(Clone, Debug, PartialEq)]
pub enum ColdLeafStoreError {
    InvalidDigestLength {
        available: usize,
    },
    InvalidAddressLength {
        available: usize,
    },
    InvalidAddressDomain,
    UnsupportedAddressVersion(u8),
    InvalidPhysicalKindTag(u8),
    InvalidHistoryPrefixLength {
        available: usize,
    },
    InvalidHistoryKindTag(u8),
    InvalidEnvelope(String),
    DigestMismatch {
        expected: Blake2b256Hash,
        actual: Blake2b256Hash,
    },
    MissingObject(ColdLeafAddress),
    ContentCollision(ColdLeafAddress),
    Store(KvStoreError),
}

impl std::fmt::Display for ColdLeafStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDigestLength { available } => write!(
                f,
                "cold-leaf digest must contain {COLD_LEAF_DIGEST_LEN} bytes, found {available}"
            ),
            Self::InvalidAddressLength { available } => write!(
                f,
                "cold-leaf address must contain {COLD_LEAF_ADDRESS_LEN} bytes, found {available}"
            ),
            Self::InvalidAddressDomain => write!(f, "cold-leaf address has the wrong domain"),
            Self::UnsupportedAddressVersion(version) => {
                write!(f, "cold-leaf address version {version} is unsupported")
            }
            Self::InvalidPhysicalKindTag(tag) => {
                write!(f, "cold-leaf physical kind tag {tag} is invalid")
            }
            Self::InvalidHistoryPrefixLength { available } => write!(
                f,
                "history leaf prefix must contain {} bytes, found {available}",
                1 + COLD_LEAF_DIGEST_LEN
            ),
            Self::InvalidHistoryKindTag(tag) => {
                write!(f, "history leaf kind tag {tag} is invalid")
            }
            Self::InvalidEnvelope(error) => write!(f, "cold-leaf envelope is invalid: {error}"),
            Self::DigestMismatch { expected, actual } => {
                write!(f, "cold-leaf digest mismatch: expected {expected}, computed {actual}")
            }
            Self::MissingObject(address) => write!(
                f,
                "cold-leaf object is missing for {:?} at {}",
                address.kind, address.digest
            ),
            Self::ContentCollision(address) => write!(
                f,
                "cold-leaf address collision for {:?} at {}",
                address.kind, address.digest
            ),
            Self::Store(error) => write!(f, "cold-leaf store error: {error}"),
        }
    }
}

impl std::error::Error for ColdLeafStoreError {}

impl From<KvStoreError> for ColdLeafStoreError {
    fn from(error: KvStoreError) -> Self { Self::Store(error) }
}

// See rspace/src/main/scala/coop/rchain/rspace/history/ColdStore.scala
pub struct ColdStoreInstances;

impl ColdStoreInstances {
    pub fn cold_store(store: Arc<Mutex<Box<dyn KeyValueStore>>>) -> Box<dyn KeyValueStore> {
        let store_lock = store
            .lock()
            .expect("Radix History: Failed to acquire lock on store");
        store_lock.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PersistedData {
    Joins(JoinsLeaf),
    Data(DataLeaf),
    Continuations(ContinuationsLeaf),
}

impl PersistedData {
    /// Consume a typed leaf and reuse its payload allocation for the exact
    /// frozen bincode-1 envelope. This is equivalent to
    /// `bincode::serialize(&self)` but never retains a second complete payload
    /// buffer at the integration boundary.
    pub(crate) fn into_legacy_bincode(self) -> Vec<u8> {
        let (kind, mut payload) = match self {
            Self::Joins(leaf) => (PersistedDataKind::Joins, leaf.bytes),
            Self::Data(leaf) => (PersistedDataKind::Data, leaf.bytes),
            Self::Continuations(leaf) => (PersistedDataKind::Continuations, leaf.bytes),
        };
        let payload_len = u64::try_from(payload.len())
            .expect("a Rust Vec length must fit the frozen bincode u64 length field");
        let old_len = payload.len();
        let new_len = old_len
            .checked_add(PERSISTED_DATA_HEADER_LEN)
            .expect("persisted-data encoded length overflow");
        payload.reserve_exact(PERSISTED_DATA_HEADER_LEN);
        payload.resize(new_len, 0);
        payload.copy_within(0..old_len, PERSISTED_DATA_HEADER_LEN);
        payload[..PERSISTED_DATA_TAG_LEN].copy_from_slice(&kind.discriminant().to_le_bytes());
        payload[PERSISTED_DATA_TAG_LEN..PERSISTED_DATA_HEADER_LEN]
            .copy_from_slice(&payload_len.to_le_bytes());
        payload
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinsLeaf {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataLeaf {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContinuationsLeaf {
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    fn typed(kind: PersistedDataKind, bytes: Vec<u8>) -> PersistedData {
        match kind {
            PersistedDataKind::Joins => PersistedData::Joins(JoinsLeaf { bytes }),
            PersistedDataKind::Data => PersistedData::Data(DataLeaf { bytes }),
            PersistedDataKind::Continuations => {
                PersistedData::Continuations(ContinuationsLeaf { bytes })
            }
        }
    }

    fn addressed(kind: PersistedDataKind, records: &[Vec<u8>]) -> (ColdLeafAddress, Vec<u8>) {
        let encoded = typed(kind, bincode::serialize(records).unwrap()).into_legacy_bincode();
        let digest = Blake2b256Hash::new(
            PersistedDataView::parse_canonical(&encoded)
                .unwrap()
                .leaf_encoding(),
        );
        (ColdLeafAddress::new(kind, digest).unwrap(), encoded)
    }

    #[test]
    fn physical_address_codec_is_versioned_and_independent_of_bincode_discriminants() {
        let digest = Blake2b256Hash::from_bytes((0u8..32).collect());
        for (kind, physical_tag) in [
            (PersistedDataKind::Data, 0),
            (PersistedDataKind::Continuations, 1),
            (PersistedDataKind::Joins, 2),
        ] {
            let address = ColdLeafAddress::new(kind, digest.clone()).unwrap();
            let mut expected = b"RSLF".to_vec();
            expected.extend_from_slice(&[1, physical_tag]);
            expected.extend_from_slice(&digest.0);
            assert_eq!(address.encode(), expected);
            assert_eq!(ColdLeafAddress::decode(&expected).unwrap(), address);
        }
        assert_ne!(
            PersistedDataKind::Data.physical_tag() as u32,
            PersistedDataKind::Data.discriminant()
        );
    }

    #[test]
    fn typed_writes_preserve_equal_payloads_at_all_three_kind_addresses() {
        let store = InMemoryKeyValueStore::new();
        let records = vec![b"same-record".to_vec()];
        let leaves =
            [PersistedDataKind::Data, PersistedDataKind::Continuations, PersistedDataKind::Joins]
                .into_iter()
                .map(|kind| addressed(kind, &records))
                .collect::<Vec<_>>();
        assert!(
            leaves
                .windows(2)
                .all(|pair| pair[0].0.digest == pair[1].0.digest)
        );

        put_cold_leaves_if_absent_verified(&store, leaves.clone()).unwrap();
        assert_eq!(store.to_map().unwrap().len(), 3);
        for (address, expected) in leaves {
            let loaded = load_cold_leaf(&store, &address).unwrap();
            assert_eq!(loaded.location(), ColdLeafLocation::Typed);
            assert_eq!(loaded.bytes(), expected);
        }
    }

    #[test]
    fn typed_corruption_is_not_downgraded_to_a_valid_legacy_candidate() {
        let store = InMemoryKeyValueStore::new();
        let (address, valid) = addressed(PersistedDataKind::Data, &[b"record".to_vec()]);
        let mut corrupt = valid.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        store
            .put(vec![(address.encode(), corrupt), (address.legacy_raw_key(), valid)])
            .unwrap();

        assert!(matches!(
            load_cold_leaf(&store, &address),
            Err(ColdLeafStoreError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn raw_and_frozen_bincode_legacy_candidates_are_validated_in_order() {
        let store = InMemoryKeyValueStore::new();
        let (raw, raw_bytes) = addressed(PersistedDataKind::Data, &[b"raw".to_vec()]);
        let (bincode, bincode_bytes) = addressed(PersistedDataKind::Joins, &[b"bincode".to_vec()]);
        assert_eq!(bincode.legacy_bincode_key(), bincode::serialize(bincode.digest()).unwrap());
        store
            .put(vec![
                (raw.legacy_raw_key(), raw_bytes),
                (bincode.legacy_bincode_key(), bincode_bytes),
            ])
            .unwrap();

        assert_eq!(
            load_cold_leaf(&store, &raw).unwrap().location(),
            ColdLeafLocation::LegacyRawDigest
        );
        assert_eq!(
            load_cold_leaf(&store, &bincode).unwrap().location(),
            ColdLeafLocation::LegacyBincodeDigest
        );
    }

    #[test]
    fn typed_put_requires_exact_equality_for_an_existing_address() {
        let store = InMemoryKeyValueStore::new();
        let (address, valid) = addressed(PersistedDataKind::Data, &[b"record".to_vec()]);
        store
            .put_one(address.encode(), b"different-existing-bytes".to_vec())
            .unwrap();

        assert!(matches!(
            put_cold_leaves_if_absent_verified(&store, vec![(address, valid)]),
            Err(ColdLeafStoreError::Store(KvStoreError::InvalidArgument(_)))
        ));
    }

    #[test]
    fn direct_envelope_is_exactly_the_frozen_bincode_image() {
        for kind in
            [PersistedDataKind::Joins, PersistedDataKind::Data, PersistedDataKind::Continuations]
        {
            for payload in [Vec::new(), vec![0], (0u8..=254).collect(), vec![0xa5; 65_537]] {
                let expected = bincode::serialize(&typed(kind, payload.clone())).unwrap();
                let actual = typed(kind, payload.clone()).into_legacy_bincode();
                assert_eq!(actual, expected);

                let view = PersistedDataView::parse_bincode_compatible(&actual).unwrap();
                assert_eq!(PersistedDataView::parse_canonical(&actual).unwrap(), view);
                assert_eq!(view.kind(), kind);
                assert_eq!(view.payload(), payload);
                assert_eq!(view.leaf_encoding(), &actual[PERSISTED_DATA_TAG_LEN..]);
                assert!(view.trailing().is_empty());
            }
        }
    }

    #[test]
    fn borrowed_envelope_cursor_matches_bincode_admission() {
        let valid = bincode::serialize(&PersistedData::Data(DataLeaf {
            bytes: (0u8..=63).collect(),
        }))
        .unwrap();
        let mut corpus = Vec::new();
        for end in 0..=valid.len() {
            corpus.push(valid[..end].to_vec());
        }
        for offset in 0..PERSISTED_DATA_HEADER_LEN {
            for bit in 0..8 {
                let mut mutated = valid.clone();
                mutated[offset] ^= 1 << bit;
                corpus.push(mutated);
            }
        }
        let mut trailing = valid.clone();
        trailing.extend_from_slice(b"accepted-trailing-bincode-bytes");
        corpus.push(trailing);
        let mut hostile_len = valid.clone();
        hostile_len[PERSISTED_DATA_TAG_LEN..PERSISTED_DATA_HEADER_LEN]
            .copy_from_slice(&u64::MAX.to_le_bytes());
        corpus.push(hostile_len);

        for bytes in corpus {
            assert_eq!(
                PersistedDataView::parse_bincode_compatible(&bytes).is_ok(),
                bincode::deserialize::<PersistedData>(&bytes).is_ok(),
                "admission differs for {} bytes: {}",
                bytes.len(),
                hex::encode(bytes)
            );
        }
    }

    #[test]
    fn leaf_hash_range_excludes_only_the_outer_discriminant() {
        let encoded = bincode::serialize(&PersistedData::Joins(JoinsLeaf {
            bytes: b"leaf-payload".to_vec(),
        }))
        .unwrap();
        let leaf = bincode::serialize(&JoinsLeaf {
            bytes: b"leaf-payload".to_vec(),
        })
        .unwrap();
        let view = PersistedDataView::parse_bincode_compatible(&encoded).unwrap();
        assert_eq!(view.leaf_encoding(), leaf);
        assert_ne!(view.leaf_encoding(), encoded);
    }

    #[test]
    fn canonical_cursor_rejects_the_trailing_bytes_that_bincode_admits() {
        let mut encoded = bincode::serialize(&PersistedData::Data(DataLeaf {
            bytes: b"canonical-payload".to_vec(),
        }))
        .unwrap();
        encoded.extend_from_slice(b"noncanonical-suffix");

        assert!(bincode::deserialize::<PersistedData>(&encoded).is_ok());
        assert!(PersistedDataView::parse_bincode_compatible(&encoded).is_ok());
        assert_eq!(
            PersistedDataView::parse_canonical(&encoded),
            Err(PersistedDataFrameError::TrailingBytes { available: 19 })
        );
    }

    #[test]
    fn record_sequence_cursor_borrows_every_exact_record() {
        let records = vec![Vec::new(), vec![0], vec![0, 0, 1], b"record".to_vec()];
        for kind in
            [PersistedDataKind::Joins, PersistedDataKind::Data, PersistedDataKind::Continuations]
        {
            let encoded = typed(kind, bincode::serialize(&records).unwrap()).into_legacy_bincode();
            let view = PersistedRecordSequenceView::parse_canonical(
                &encoded,
                kind,
                Some(records.len() as u64),
            )
            .unwrap();
            assert_eq!(view.kind(), kind);
            assert_eq!(view.record_count(), records.len() as u64);
            assert_eq!(
                view.records().collect::<Vec<_>>(),
                records.iter().map(Vec::as_slice).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn record_sequence_cursor_rejects_every_malformed_frame_shape() {
        let envelope = |payload: Vec<u8>| {
            PersistedData::Joins(JoinsLeaf { bytes: payload }).into_legacy_bincode()
        };

        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(vec![0; 7]),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::TruncatedRecordCount { available: 7 })
        );

        let empty = envelope(0u64.to_le_bytes().to_vec());
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(&empty, PersistedDataKind::Data, None,),
            Err(PersistedDataFrameError::UnexpectedKind {
                expected: PersistedDataKind::Data,
                actual: PersistedDataKind::Joins,
            })
        );
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(&empty, PersistedDataKind::Joins, Some(1),),
            Err(PersistedDataFrameError::RecordCountMismatch {
                expected: 1,
                actual: 0,
            })
        );

        let mut missing_length = 1u64.to_le_bytes().to_vec();
        missing_length.extend_from_slice(&[0xaa; 7]);
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(missing_length),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::TruncatedRecordLength {
                index: 0,
                available: 7,
            })
        );

        let mut overflowing = 1u64.to_le_bytes().to_vec();
        overflowing.extend_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(overflowing),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::RecordLengthOverflow {
                index: 0,
                declared: u64::MAX,
            })
        );

        let mut truncated = 1u64.to_le_bytes().to_vec();
        truncated.extend_from_slice(&3u64.to_le_bytes());
        truncated.extend_from_slice(&[1, 2]);
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(truncated),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::TruncatedRecord {
                index: 0,
                declared: 3,
                available: 2,
            })
        );

        let mut too_few = 2u64.to_le_bytes().to_vec();
        too_few.extend_from_slice(&1u64.to_le_bytes());
        too_few.push(7);
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(too_few),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::TruncatedRecordLength {
                index: 1,
                available: 0,
            })
        );

        let mut trailing = 0u64.to_le_bytes().to_vec();
        trailing.push(9);
        assert_eq!(
            PersistedRecordSequenceView::parse_canonical(
                &envelope(trailing),
                PersistedDataKind::Joins,
                None,
            ),
            Err(PersistedDataFrameError::TrailingRecordBytes { available: 1 })
        );
    }
}
