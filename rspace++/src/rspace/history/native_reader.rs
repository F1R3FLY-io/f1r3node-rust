use std::fmt;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

mod framing;
use framing::{MAX_NODE_BYTES, NodeStep, node_step};
pub use framing::{NativeLeafKind, NativeRecords};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeReadCharge {
    pub operations: usize,
    pub scanned_bytes: usize,
    pub backing_bytes: usize,
}

pub trait NativeReadMeter {
    type Error;
    fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error>;
}

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
    Overflow,
    Allocation,
}

#[derive(Debug)]
pub enum NativeReadError<E> {
    Host(E),
    Consumer(E),
    Store(KvStoreError),
    Invalid(NativeReadFault),
}

impl<E: fmt::Display> fmt::Display for NativeReadError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(f, "native history host reservation: {error}"),
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

fn reserve<M: NativeReadMeter + ?Sized>(
    meter: &M,
    operations: usize,
    scanned_bytes: usize,
    backing_bytes: usize,
) -> Result<(), NativeReadError<M::Error>> {
    meter
        .reserve(NativeReadCharge {
            operations,
            scanned_bytes,
            backing_bytes,
        })
        .map_err(NativeReadError::Host)
}

fn lookup_credit<M: NativeReadMeter + ?Sized>(
    meter: &M,
    key: &[u8],
) -> Result<(), NativeReadError<M::Error>> {
    reserve(meter, 4, key.len() * 2, key.len() + 8)
}

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

    fn leaf_pointer<M: NativeReadMeter + ?Sized>(
        &self,
        lookup: &[u8],
        scratch: &mut Vec<u8>,
        meter: &M,
    ) -> Result<Option<[u8; 32]>, NativeReadError<M::Error>> {
        let mut remaining = lookup;
        let mut pointer = self.root;
        let mut first = true;
        while !remaining.is_empty() {
            scratch.clear();
            scratch.extend_from_slice(&pointer);
            lookup_credit(meter, scratch)?;
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
                        reserve(meter, 3 + bytes.len() / 34, bytes.len() * 3, 0)?;
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

    pub fn with_records<M, T>(
        &self,
        kind: NativeLeafKind,
        channel_hash: &[u8; 32],
        meter: &M,
        consume: impl FnOnce(NativeRecords<'_>) -> Result<T, M::Error>,
    ) -> Result<Option<T>, NativeReadError<M::Error>>
    where
        M: NativeReadMeter + ?Sized,
    {
        reserve(meter, 4, 40, 40)?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(40)
            .map_err(|_| NativeReadError::Invalid(NativeReadFault::Allocation))?;
        let mut lookup = [0; 33];
        lookup[0] = kind.prefix();
        lookup[1..].copy_from_slice(channel_hash);
        let Some(pointer) = self.leaf_pointer(&lookup, &mut scratch, meter)? else {
            return Ok(None);
        };
        let mut consume = Some(consume);
        for serialized in [false, true] {
            scratch.clear();
            if serialized {
                scratch.extend_from_slice(&32_u64.to_le_bytes());
            }
            scratch.extend_from_slice(&pointer);
            lookup_credit(meter, &scratch)?;
            let mut present = false;
            let mut result = Err(NativeReadError::Invalid(NativeReadFault::MissingLeaf));
            self.leaves
                .with_value(&scratch, &mut |bytes| {
                    let Some(bytes) = bytes else {
                        return Ok(());
                    };
                    present = true;
                    result = (|| {
                        let scanned = bytes
                            .len()
                            .checked_mul(3)
                            .ok_or(NativeReadError::Invalid(NativeReadFault::Overflow))?;
                        reserve(meter, 4 + bytes.len() / 8, scanned, 0)?;
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
