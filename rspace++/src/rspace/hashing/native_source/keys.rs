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

use std::io::Write;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::Serialize;

use super::{Result, sort};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

/// The key of a channel, or of a channel group, in the native session store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreKey(pub [u8; 32]);

impl StoreKey {
    /// The key of a channel whose digest is already computed, such as
    /// `Produce::channel_hash`.
    pub fn from_digest(digest: &Blake2b256Hash) -> Result<Self> {
        let bytes: [u8; 32] = digest.0.as_slice().try_into().map_err(|_| {
            RSpaceError::BugFoundError(format!(
                "a store key digest has {} bytes, not 32",
                digest.0.len()
            ))
        })?;
        Ok(Self(bytes))
    }

    /// The store shard of the key.
    pub fn shard(&self) -> usize { usize::from(self.0[0]) }
}

fn finish(digest: Blake2b<U32>) -> StoreKey {
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&digest.finalize());
    StoreKey(bytes)
}

/// Changed by DR-131 S6: streams the bincode encoding into the digest, so
/// that a channel key needs no heap copy of the encoding.
struct DigestWriter(Blake2b<U32>);

impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// The key of a channel: the digest of its bincode encoding, with the bytes
/// of `stable_hash_provider::hash` but no heap copy.
pub fn channel_key<C: Serialize + ?Sized>(channel: &C) -> Result<StoreKey> {
    let mut writer = DigestWriter(Blake2b::<U32>::new());
    bincode::serialize_into(&mut writer, channel)
        .map_err(|error| RSpaceError::InterpreterError(error.to_string()))?;
    Ok(finish(writer.0))
}

/// The domain separator of group keys.
pub const GROUP_KEY_DOMAIN: &[u8] = b"f1r3fly native store channel group v1";

/// The key of a channel group: the digest of the domain separator and the
/// channel keys in channel order. The keys have a fixed width, so groups of
/// different lengths hash different inputs.
pub fn group_key(keys: &[StoreKey]) -> StoreKey {
    let mut digest = Blake2b::<U32>::new();
    digest.update(GROUP_KEY_DOMAIN);
    for key in keys {
        digest.update(key.0);
    }
    finish(digest)
}

/// D-C2d (DR-96): the history projection of a channel group from the store
/// keys of its channels. A channel's store key is the digest that
/// `stable_hash_provider::hash_from_vec` computes for the channel, so the
/// projection has the same bytes, and no channel is serialized again. The
/// digests are sorted, so the projection does not depend on the channel order.
pub fn channels_hash_from_keys(keys: &[StoreKey]) -> [u8; 32] {
    let mut sorted = keys.to_vec();
    sort(&mut sorted, |key| &key.0[..]);
    let mut digest = Blake2b::<U32>::new();
    for key in &sorted {
        digest.update(key.0);
    }
    let mut bytes = [0; 32];
    bytes.copy_from_slice(&digest.finalize());
    bytes
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
    pub fn from_channel_keys(channels: Vec<StoreKey>) -> Self {
        let group = group_key(&channels);
        Self { channels, group }
    }

    /// The group keys of `channels`, with one digest per channel.
    pub fn build<C: Serialize>(channels: &[C]) -> Result<Self> {
        let mut keys = Vec::with_capacity(channels.len());
        for channel in channels {
            keys.push(channel_key(channel)?);
        }
        Ok(Self::from_channel_keys(keys))
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
    pub fn build<C: Serialize>(joins: &[Vec<C>]) -> Result<Self> {
        let mut groups = Vec::with_capacity(joins.len());
        for join in joins {
            groups.push(GroupKeys::build(join)?);
        }
        Ok(Self { groups })
    }
}
