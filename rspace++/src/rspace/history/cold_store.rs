use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;

pub(crate) const PERSISTED_DATA_HEADER_LEN: usize =
    std::mem::size_of::<u32>() + std::mem::size_of::<u64>();
const PERSISTED_DATA_TAG_LEN: usize = std::mem::size_of::<u32>();

/// The frozen bincode-1 discriminant of an RSpace cold-store leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistedDataKind {
    Joins,
    Data,
    Continuations,
}

impl PersistedDataKind {
    const fn discriminant(self) -> u32 {
        match self {
            Self::Joins => 0,
            Self::Data => 1,
            Self::Continuations => 2,
        }
    }

    pub(crate) fn write_header(self, payload_len: u64, output: &mut Vec<u8>) {
        output.extend_from_slice(&self.discriminant().to_le_bytes());
        output.extend_from_slice(&payload_len.to_le_bytes());
    }

    fn from_discriminant(discriminant: u32) -> Result<Self, PersistedDataFrameError> {
        match discriminant {
            0 => Ok(Self::Joins),
            1 => Ok(Self::Data),
            2 => Ok(Self::Continuations),
            value => Err(PersistedDataFrameError::InvalidDiscriminant(value)),
        }
    }
}

/// A validated borrowed view of the non-recursive bincode cold-store envelope.
///
/// Bincode 1 permits trailing bytes, so this cursor deliberately exposes but
/// does not reject them. `leaf_encoding` is the exact hash authority (the
/// encoded single-field leaf without the outer enum tag); `payload` is the
/// already-encoded datum, continuation, or join collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistedDataView<'a> {
    kind: PersistedDataKind,
    leaf_encoding: &'a [u8],
    payload: &'a [u8],
    trailing: &'a [u8],
}

impl<'a> PersistedDataView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, PersistedDataFrameError> {
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

    pub fn kind(self) -> PersistedDataKind { self.kind }

    pub fn leaf_encoding(self) -> &'a [u8] { self.leaf_encoding }

    pub fn payload(self) -> &'a [u8] { self.payload }

    pub fn trailing(self) -> &'a [u8] { self.trailing }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistedDataFrameError {
    TruncatedHeader { available: usize },
    InvalidDiscriminant(u32),
    LengthOverflow(u64),
    TruncatedPayload { declared: u64, available: usize },
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
        }
    }
}

impl std::error::Error for PersistedDataFrameError {}

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

    fn typed(kind: PersistedDataKind, bytes: Vec<u8>) -> PersistedData {
        match kind {
            PersistedDataKind::Joins => PersistedData::Joins(JoinsLeaf { bytes }),
            PersistedDataKind::Data => PersistedData::Data(DataLeaf { bytes }),
            PersistedDataKind::Continuations => {
                PersistedData::Continuations(ContinuationsLeaf { bytes })
            }
        }
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

                let view = PersistedDataView::parse(&actual).unwrap();
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
                PersistedDataView::parse(&bytes).is_ok(),
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
        let view = PersistedDataView::parse(&encoded).unwrap();
        assert_eq!(view.leaf_encoding(), leaf);
        assert_ne!(view.leaf_encoding(), encoded);
    }
}
