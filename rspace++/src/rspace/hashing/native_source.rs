use std::convert::Infallible;

use serde::Serialize;

use super::blake2b256_hash::Blake2b256Hash;
use super::stable_hash_provider::{hash, hash_consume};
use crate::rspace::errors::RSpaceError;
use crate::rspace::trace::event::Consume;

type Result<T> = std::result::Result<T, RSpaceError>;

pub mod keys;
pub use keys::*;

/// Changed by DR-131 S6: sorts by a byte key with the heap sort of
/// `fallible_sort` (D-F), so that equal keys keep the order that the sort
/// gave them before host work was removed.
pub(crate) fn sort<T>(values: &mut [T], key: impl Fn(&T) -> &[u8]) {
    let Ok(()) =
        shared::rust::fallible_sort::sort(values, |a, b| Ok::<_, Infallible>(key(a).cmp(key(b))));
}

/// D-S1 (D-C2a, DR-96): a consume source and the store keys of its channels,
/// in channel order. The keys are the channel digests that the source
/// computes before it sorts them, so no channel is hashed twice. The source
/// equals `Consume::create`.
pub fn consume_keys<C: Serialize, P: Serialize, K: Serialize>(
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
) -> Result<(Consume, Vec<StoreKey>)> {
    let mut keys = Vec::with_capacity(channels.len());
    let mut channel_hashes = Vec::with_capacity(channels.len());
    for channel in channels {
        let digest = hash(channel);
        keys.push(StoreKey::from_digest(&digest)?);
        channel_hashes.push(digest);
    }
    channel_hashes.sort();
    let encoded_channels = channel_hashes.iter().map(Blake2b256Hash::bytes).collect();
    let source_hash = hash_consume(encoded_channels, patterns, continuation, persistent);
    Ok((
        Consume {
            channel_hashes,
            hash: source_hash,
            persistent,
        },
        keys,
    ))
}

#[cfg(test)]
mod tests;
