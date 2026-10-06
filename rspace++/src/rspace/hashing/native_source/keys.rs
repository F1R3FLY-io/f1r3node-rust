//! D-S1 (D-C2a, DR-96): the keys of the native session store.
//!
//! A channel's store key is the Blake2b-256 digest of its bincode encoding,
//! the digest that already keys the channel's history leaves. Two channels
//! have the same key exactly when they are equal, up to a Blake2b collision:
//! the hand-written `PartialEq` and `Hash` of every rhoapi type with a
//! `locally_free` field ignore that field, and the field always encodes as
//! empty bytes (`models/build.rs`, `serialize_as_empty_bytes`). A group of
//! channels, the channels of a continuation or of a join, is keyed by the
//! digest of its channel keys in channel order, so `[a, b]` and `[b, a]`
//! remain different groups, as in the legacy store whose continuation key is
//! the channel vector. A digest collision is handled by D-C2e.

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::Serialize;

use super::{Output, Result, SourceMeter, serialize, sort, vector};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// The key of a channel, or of a channel group, in the native session store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreKey(pub [u8; 32]);

impl StoreKey {
    /// The key of a channel whose digest is already computed, such as
    /// `Produce::channel_hash`.
    pub fn from_digest(digest: &Blake2b256Hash, meter: &impl SourceMeter) -> Result<Self> {
        meter.reserve(1, 32, 0)?;
        let bytes: [u8; 32] = digest
            .0
            .as_slice()
            .try_into()
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        Ok(Self(bytes))
    }

    /// The store shard of the key.
    pub fn shard(&self) -> usize { usize::from(self.0[0]) }
}

fn finish(digest: Blake2b<U32>, meter: &impl SourceMeter) -> Result<StoreKey> {
    meter.reserve(1, 32, 0)?;
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&digest.finalize());
    Ok(StoreKey(bytes))
}

/// The key of a channel: the digest of its bincode encoding, with the bytes
/// of `hash` (`native_source.rs`) but no heap copy.
pub fn channel_key<C: Serialize + ?Sized>(
    channel: &C,
    meter: &impl SourceMeter,
) -> Result<StoreKey> {
    match serialize(channel, Output::Hash(Blake2b::<U32>::new()), meter)? {
        Output::Hash(digest) => finish(digest, meter),
        Output::Bytes { .. } => unreachable!(),
    }
}

/// The domain separator of group keys.
pub const GROUP_KEY_DOMAIN: &[u8] = b"f1r3fly native store channel group v1";

/// The key of a channel group: the digest of the domain separator and the
/// channel keys in channel order. The keys have a fixed width, so groups of
/// different lengths hash different inputs.
pub fn group_key(keys: &[StoreKey], meter: &impl SourceMeter) -> Result<StoreKey> {
    meter.reserve(1, GROUP_KEY_DOMAIN.len(), 0)?;
    let mut digest = Blake2b::<U32>::new();
    digest.update(GROUP_KEY_DOMAIN);
    for key in keys {
        meter.reserve(1, key.0.len(), 0)?;
        digest.update(key.0);
    }
    finish(digest, meter)
}

/// D-C2d (DR-96): the history projection of a channel group from the store
/// keys of its channels. A channel's store key is the digest that
/// `channels_hash` (`native_source.rs`) computes for the channel, so the
/// projection has the same bytes, and no channel is serialized again. The
/// digests are sorted, so the projection does not depend on the channel order.
pub fn channels_hash_from_keys(keys: &[StoreKey], meter: &impl SourceMeter) -> Result<[u8; 32]> {
    let mut sorted = vector(keys.len(), meter)?;
    for key in keys {
        meter.reserve(1, key.0.len(), 0)?;
        sorted.push(*key);
    }
    sort(&mut sorted, |key| &key.0[..], meter)?;
    let mut digest = Blake2b::<U32>::new();
    for key in &sorted {
        meter.reserve(1, key.0.len(), 0)?;
        digest.update(key.0);
    }
    meter.reserve(1, 32, 0)?;
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&digest.finalize());
    Ok(bytes)
}

/// The keys of a channel group: the key of each channel, in channel order,
/// and the group key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupKeys {
    pub channels: Vec<StoreKey>,
    pub group: StoreKey,
}

impl GroupKeys {
    /// The group keys of channels whose keys are already computed, such as
    /// the keys that `consume_keys` returns.
    pub fn from_channel_keys(channels: Vec<StoreKey>, meter: &impl SourceMeter) -> Result<Self> {
        let group = group_key(&channels, meter)?;
        Ok(Self { channels, group })
    }

    /// The group keys of `channels`, with one digest per channel.
    pub fn build<C: Serialize>(channels: &[C], meter: &impl SourceMeter) -> Result<Self> {
        let mut keys = vector(channels.len(), meter)?;
        for channel in channels {
            keys.push(channel_key(channel, meter)?);
        }
        Self::from_channel_keys(keys, meter)
    }
}

/// The keys of the join groups that one operation reads, indexed by the
/// position of the group in the operation's joins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationKeys {
    pub groups: Vec<GroupKeys>,
}

impl OperationKeys {
    /// One digest per channel per group, computed once for the operation.
    pub fn build<C: Serialize>(joins: &[Vec<C>], meter: &impl SourceMeter) -> Result<Self> {
        let mut groups = vector(joins.len(), meter)?;
        for join in joins {
            groups.push(GroupKeys::build(join, meter)?);
        }
        Ok(Self { groups })
    }
}
