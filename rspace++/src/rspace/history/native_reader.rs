use std::fmt;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

mod framing;
mod records;
use framing::{MAX_NODE_BYTES, NodeStep, node_step};
pub use framing::{NativeLeafKind, NativeRecords};
// Added by DR-131 P2a: the per-record decoding through the cold-store decode
// boundary. DR-131 S6 wired it in place of the typed decoder.
pub use records::{decode_continuation, decode_datum, decode_join};

use crate::rspace::serializers::cold_store_decode::ColdStoreDecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReadFault {
    Truncated,
    Length,
    DuplicateIndex,
    NodeSize,
    LeafKind,
    NodeHash,
    LeafHash,
    MissingNode,
    MissingLeaf,
    TypedRecord,
}

#[derive(Debug)]
pub enum NativeReadError<E> {
    Consumer(E),
    Store(KvStoreError),
    Invalid(NativeReadFault),
}

/// DR-131 S6: a record that the cold-store decoder rejects is a typed-record
/// fault of the native history, as a record that the typed decoder rejected
/// was.
impl<E> From<ColdStoreDecodeError> for NativeReadError<E> {
    fn from(_: ColdStoreDecodeError) -> Self { Self::Invalid(NativeReadFault::TypedRecord) }
}

impl<E: fmt::Display> fmt::Display for NativeReadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Consumer(error) => write!(f, "native history consumer: {error}"),
            Self::Store(error) => write!(f, "native history store: {error}"),
            Self::Invalid(error) => write!(f, "native history: {error:?}"),
        }
    }
}

pub struct NativeHistoryReader<'a> {
    root: [u8; 32],
    nodes: &'a dyn KeyValueStore,
    leaves: &'a dyn KeyValueStore,
}

fn digest(bytes: &[u8]) -> [u8; 32] { Blake2b::<U32>::digest(bytes).into() }

impl<'a> NativeHistoryReader<'a> {
    pub fn new(
        root: [u8; 32],
        nodes: &'a dyn KeyValueStore,
        leaves: &'a dyn KeyValueStore,
    ) -> Self {
        Self {
            root,
            nodes,
            leaves,
        }
    }

    fn leaf_pointer<E>(
        &self,
        lookup: &[u8],
        scratch: &mut Vec<u8>,
    ) -> Result<Option<[u8; 32]>, NativeReadError<E>> {
        let mut remaining = lookup;
        let mut pointer = self.root;
        let mut first = true;
        while !remaining.is_empty() {
            scratch.clear();
            scratch.extend_from_slice(&pointer);
            let mut result = Err(NativeReadError::Invalid(NativeReadFault::MissingNode));
            self.nodes
                .with_value(scratch, &mut |bytes| {
                    result = (|| {
                        let Some(bytes) = bytes else {
                            return if first && pointer == digest(&[]) {
                                Ok(NodeStep::Absent)
                            } else {
                                Err(NativeReadError::Invalid(NativeReadFault::MissingNode))
                            };
                        };
                        if bytes.len() > MAX_NODE_BYTES {
                            return Err(NativeReadError::Invalid(NativeReadFault::NodeSize));
                        }
                        if digest(bytes) != pointer {
                            return Err(NativeReadError::Invalid(NativeReadFault::NodeHash));
                        }
                        node_step(bytes, remaining).map_err(NativeReadError::Invalid)
                    })();
                    Ok(())
                })
                .map_err(NativeReadError::Store)?;
            match result? {
                NodeStep::Absent => return Ok(None),
                NodeStep::Leaf(value) => return Ok(Some(value)),
                NodeStep::Child { hash, consumed } => {
                    pointer = hash;
                    remaining = &remaining[consumed..];
                    first = false;
                }
            }
        }
        Ok(None)
    }

    pub fn with_records<T, E>(
        &self,
        kind: NativeLeafKind,
        channel_hash: &[u8; 32],
        consume: impl FnOnce(NativeRecords<'_>) -> Result<T, E>,
    ) -> Result<Option<T>, NativeReadError<E>> {
        let mut scratch = Vec::with_capacity(40);
        let mut lookup = [0; 33];
        lookup[0] = kind.prefix();
        lookup[1..].copy_from_slice(channel_hash);
        let Some(pointer) = self.leaf_pointer(&lookup, &mut scratch)? else {
            return Ok(None);
        };
        let mut consume = Some(consume);
        for serialized in [false, true] {
            scratch.clear();
            if serialized {
                scratch.extend_from_slice(&32_u64.to_le_bytes());
            }
            scratch.extend_from_slice(&pointer);
            let mut present = false;
            let mut result = Err(NativeReadError::Invalid(NativeReadFault::MissingLeaf));
            self.leaves
                .with_value(&scratch, &mut |bytes| {
                    let Some(bytes) = bytes else {
                        return Ok(());
                    };
                    present = true;
                    result = (|| {
                        let (records, hash_bytes) =
                            NativeRecords::parse(bytes, kind).map_err(NativeReadError::Invalid)?;
                        if digest(hash_bytes) != pointer {
                            return Err(NativeReadError::Invalid(NativeReadFault::LeafHash));
                        }
                        consume.take().expect("one borrowed leaf consumer")(records)
                            .map(Some)
                            .map_err(NativeReadError::Consumer)
                    })();
                    Ok(())
                })
                .map_err(NativeReadError::Store)?;
            if present {
                return result;
            }
        }
        Err(NativeReadError::Invalid(NativeReadFault::MissingLeaf))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use tests::allocations::measure as measure_allocations;
