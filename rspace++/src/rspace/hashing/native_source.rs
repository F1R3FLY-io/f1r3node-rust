use std::io::Write;
use std::mem::size_of;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::Serialize;

use super::blake2b256_hash::Blake2b256Hash;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::{
    BincodeByteSink, COST_CHANNEL_KEY_DOMAIN_V1, COST_EVENT_DOMAIN_V1, EventCodecEpoch,
    StableHashSerialize,
};
use crate::rspace::trace::event::{Consume, Produce};

type Result<T> = std::result::Result<T, RSpaceError>;

pub mod keys;
pub use keys::*;

pub trait SourceMeter {
    fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Result<()>;
}

impl<F: Fn(usize, usize, usize) -> Result<()>> SourceMeter for F {
    fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Result<()> {
        self(operations, scanned, backing)
    }
}

enum Output {
    Bytes { bytes: Vec<u8>, paid: usize },
    Hash(Blake2b<U32>),
}

struct Writer<'a, M> {
    meter: &'a M,
    output: Output,
    error: Option<RSpaceError>,
}

impl<M: SourceMeter> Writer<'_, M> {
    fn append(&mut self, input: &[u8]) -> Result<()> {
        self.meter.reserve(1, input.len(), 0)?;
        match &mut self.output {
            Output::Hash(hash) => hash.update(input),
            Output::Bytes { bytes, paid } => {
                let needed = bytes
                    .len()
                    .checked_add(input.len())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                if needed > *paid {
                    let next = needed
                        .max(paid.checked_mul(2).ok_or(RSpaceError::HostWorkRejected)?)
                        .max(8);
                    self.meter.reserve(1, bytes.len(), next)?;
                    bytes
                        .try_reserve_exact(next - bytes.len())
                        .map_err(|_| RSpaceError::HostWorkRejected)?;
                    *paid = next;
                }
                bytes.extend_from_slice(input);
            }
        }
        Ok(())
    }
}

impl<M: SourceMeter> BincodeByteSink for Writer<'_, M> {
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> { self.append(bytes) }
}

impl<M: SourceMeter> Write for Writer<'_, M> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.error.is_some() {
            return Err(std::io::ErrorKind::Other.into());
        }
        match self.append(bytes) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => {
                self.error = Some(error);
                Err(std::io::ErrorKind::Other.into())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn serialize<T: Serialize + ?Sized, M: SourceMeter>(
    value: &T,
    output: Output,
    meter: &M,
) -> Result<Output> {
    meter.reserve(1, 0, size_of::<bincode::ErrorKind>())?;
    let mut writer = Writer {
        meter,
        output,
        error: None,
    };
    let result = bincode::serialize_into(&mut writer, value);
    if let Some(error) = writer.error.take() {
        return Err(error);
    }
    if let Err(error) = result {
        return Err(RSpaceError::InterpreterError(error.to_string()));
    }
    Ok(writer.output)
}

fn stable_output<T: StableHashSerialize + ?Sized, M: SourceMeter>(
    value: &T,
    epoch: EventCodecEpoch,
    output: Output,
    meter: &M,
) -> Result<Output> {
    // The same initial serde-error allocation reservation as `serialize`.
    // Every subsequent chunk is charged by Writer before it reaches output.
    meter.reserve(1, 0, size_of::<bincode::ErrorKind>())?;
    let mut writer = Writer {
        meter,
        output,
        error: None,
    };
    match epoch {
        EventCodecEpoch::Legacy => value.stable_hash_write_legacy_metered(&mut writer)?,
        EventCodecEpoch::CostV1 => value.stable_hash_write_cost_v1_metered(&mut writer)?,
        EventCodecEpoch::CostV2 => value.stable_hash_write_metered(&mut writer)?,
    }
    Ok(writer.output)
}

/// A metered event leg. The authenticated epoch, never the payload, selects
/// the positional schema; Legacy refuses populated cost fields before output.
pub fn encode_for_epoch<T: StableHashSerialize + ?Sized>(
    epoch: EventCodecEpoch,
    value: &T,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>> {
    let contains_cost = value.stable_hash_contains_cost_metered(meter)?;
    if contains_cost && epoch == EventCodecEpoch::Legacy {
        return Err(RSpaceError::InterpreterError(
            "cost-bearing event leg is invalid in the Legacy codec epoch".into(),
        ));
    }
    if epoch == EventCodecEpoch::CostV1 {
        value.stable_hash_validate_cost_v1_metered(meter)?;
    }
    encode_validated_layout(value, epoch, meter)
}

/// Only callers holding the result of the immediately preceding complete
/// canonical validation may use this. It avoids decoding each raw EPathMap
/// key a second time when Route A builds a tagged cost-bearing channel key.
fn encode_validated_layout<T: StableHashSerialize + ?Sized>(
    value: &T,
    epoch: EventCodecEpoch,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>> {
    match stable_output(
        value,
        epoch,
        Output::Bytes {
            bytes: Vec::new(),
            paid: 0,
        },
        meter,
    )? {
        Output::Bytes { bytes, .. } => Ok(bytes),
        Output::Hash(_) => unreachable!(),
    }
}

fn hash_stable_layout<T: StableHashSerialize + ?Sized>(
    value: &T,
    epoch: EventCodecEpoch,
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash> {
    match stable_output(value, epoch, Output::Hash(Blake2b::<U32>::new()), meter)? {
        Output::Hash(hash) => {
            let mut bytes = vector(32, meter)?;
            bytes.extend_from_slice(&hash.finalize());
            Ok(Blake2b256Hash::from_bytes(bytes))
        }
        Output::Bytes { .. } => unreachable!(),
    }
}

/// Route A's single native key derivation: ordinary channels keep their
/// historical digest even in CostV1; cost-bearing channels get a disjoint
/// tagged preimage. The channel's complete canonical tree is validated first.
pub fn channel_hash_for_epoch<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channel: &C,
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash> {
    let contains_cost = channel.stable_hash_contains_cost_metered(meter)?;
    if contains_cost && epoch == EventCodecEpoch::Legacy {
        return Err(RSpaceError::InterpreterError(
            "cost-bearing channel is invalid in the Legacy codec epoch".into(),
        ));
    }
    if epoch == EventCodecEpoch::CostV1 {
        channel.stable_hash_validate_cost_v1_metered(meter)?;
    }
    if !contains_cost {
        return hash_stable_layout(channel, EventCodecEpoch::Legacy, meter);
    }
    let encoded = encode_validated_layout(channel, epoch, meter)?;
    let parts = [COST_CHANNEL_KEY_DOMAIN_V1, encoded.as_slice()];
    hash(parts.as_slice(), meter)
}

pub(crate) fn encode<T: Serialize + ?Sized>(
    value: &T,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>> {
    match serialize(
        value,
        Output::Bytes {
            bytes: Vec::new(),
            paid: 0,
        },
        meter,
    )? {
        Output::Bytes { bytes, .. } => Ok(bytes),
        Output::Hash(_) => unreachable!(),
    }
}

pub(crate) fn hash<T: Serialize + ?Sized>(
    value: &T,
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash> {
    match serialize(value, Output::Hash(Blake2b::<U32>::new()), meter)? {
        Output::Hash(hash) => {
            let mut bytes = vector(32, meter)?;
            bytes.extend_from_slice(&hash.finalize());
            Ok(Blake2b256Hash::from_bytes(bytes))
        }
        Output::Bytes { .. } => unreachable!(),
    }
}

pub(crate) fn channels_hash<C: Serialize>(
    channels: &[C],
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash> {
    let mut hashes = vector(channels.len(), meter)?;
    for channel in channels {
        hashes.push(hash(channel, meter)?);
    }
    sort(&mut hashes, |hash| &hash.0, meter)?;
    let mut digest = Blake2b::<U32>::new();
    for hash in hashes {
        meter.reserve(1, hash.0.len(), 0)?;
        digest.update(hash.0);
    }
    let mut bytes = vector(32, meter)?;
    bytes.extend_from_slice(&digest.finalize());
    Ok(Blake2b256Hash::from_bytes(bytes))
}

fn vector<T>(count: usize, meter: &impl SourceMeter) -> Result<Vec<T>> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

pub(crate) fn sort<T>(
    values: &mut [T],
    key: impl Fn(&T) -> &[u8],
    meter: &impl SourceMeter,
) -> Result<()> {
    let moves = size_of::<T>()
        .checked_mul(2)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(
        values.len(),
        values
            .len()
            .checked_mul(moves)
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )?;
    shared::rust::fallible_sort::sort(values, |a, b| {
        let a = key(a);
        let b = key(b);
        meter.reserve(
            1,
            a.len()
                .min(b.len())
                .checked_add(moves)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        Ok(a.cmp(b))
    })
}

pub fn produce<C: Serialize, A: Serialize>(
    channel: &C,
    data: &A,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<Produce> {
    let channel_hash = hash(channel, meter)?;
    let data = encode(data, meter)?;
    let persist = [u8::from(persistent)];
    let parts = [channel_hash.0.as_slice(), data.as_slice(), persist.as_slice()];
    let source_hash = hash(parts.as_slice(), meter)?;
    Ok(Produce::new(channel_hash, source_hash, persistent))
}

/// Native metered counterpart of `Produce::create_for_epoch`. Historical
/// `produce` stays untouched for explicitly context-free callers.
pub fn produce_for_epoch<C: StableHashSerialize, A: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channel: &C,
    data: &A,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<Produce> {
    let channel_hash = channel_hash_for_epoch(epoch, channel, meter)?;
    let data = encode_for_epoch(epoch, data, meter)?;
    let persist = [u8::from(persistent)];
    let source_hash = match epoch {
        EventCodecEpoch::Legacy => {
            let parts = [channel_hash.0.as_slice(), data.as_slice(), persist.as_slice()];
            hash(parts.as_slice(), meter)?
        }
        EventCodecEpoch::CostV1 | EventCodecEpoch::CostV2 => {
            let parts = [
                COST_EVENT_DOMAIN_V1,
                channel_hash.0.as_slice(),
                data.as_slice(),
                persist.as_slice(),
            ];
            hash(parts.as_slice(), meter)?
        }
    };
    Ok(Produce::new(channel_hash, source_hash, persistent))
}

pub fn clone_produce(source: &Produce, meter: &dyn SourceMeter) -> Result<Produce> {
    crate::rspace::native_backing::reserve_blocks_copy_and_cleanup(source, meter)?;
    Ok(source.clone())
}

pub fn consume<C: Serialize, P: Serialize, K: Serialize>(
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<Consume> {
    consume_with(channels, patterns, continuation, persistent, None, meter)
}

/// D-S1 (D-C2a, DR-96): a consume source and the store keys of its channels,
/// in channel order. The keys are the channel digests that the source
/// computes before it sorts them, so no channel is hashed twice.
pub fn consume_keys<C: Serialize, P: Serialize, K: Serialize>(
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<(Consume, Vec<StoreKey>)> {
    let mut keys = vector(channels.len(), meter)?;
    let source =
        consume_with(channels, patterns, continuation, persistent, Some(&mut keys), meter)?;
    Ok((source, keys))
}

fn consume_with<C: Serialize, P: Serialize, K: Serialize>(
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    mut keys: Option<&mut Vec<StoreKey>>,
    meter: &impl SourceMeter,
) -> Result<Consume> {
    let mut channel_hashes = vector(channels.len(), meter)?;
    for channel in channels {
        let digest = hash(channel, meter)?;
        if let Some(keys) = keys.as_deref_mut() {
            keys.push(StoreKey::from_digest(&digest, meter)?);
        }
        channel_hashes.push(digest);
    }
    sort(&mut channel_hashes, |hash| &hash.0, meter)?;
    let mut encoded_patterns = vector(patterns.len(), meter)?;
    for pattern in patterns {
        encoded_patterns.push(encode(pattern, meter)?);
    }
    sort(&mut encoded_patterns, Vec::as_slice, meter)?;
    let continuation = encode(continuation, meter)?;
    let persist = [u8::from(persistent)];
    let count = channels
        .len()
        .checked_add(patterns.len())
        .and_then(|n| n.checked_add(2))
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut parts = vector(count, meter)?;
    parts.extend(channel_hashes.iter().map(|hash| hash.0.as_slice()));
    parts.extend(encoded_patterns.iter().map(Vec::as_slice));
    parts.push(continuation.as_slice());
    parts.push(persist.as_slice());
    let source_hash = hash(&parts, meter)?;
    Ok(Consume {
        channel_hashes,
        hash: source_hash,
        persistent,
    })
}

/// Native metered counterpart of `Consume::create_for_epoch`; its sorted
/// channel keys and pattern bytes use the same epoch as its event preimage.
pub fn consume_for_epoch<C: StableHashSerialize, P: StableHashSerialize, K: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<Consume> {
    consume_for_epoch_with(epoch, channels, patterns, continuation, persistent, None, meter)
}

fn consume_for_epoch_with<
    C: StableHashSerialize,
    P: StableHashSerialize,
    K: StableHashSerialize,
>(
    epoch: EventCodecEpoch,
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    mut keys: Option<&mut Vec<StoreKey>>,
    meter: &impl SourceMeter,
) -> Result<Consume> {
    let mut channel_hashes = vector(channels.len(), meter)?;
    for channel in channels {
        let digest = channel_hash_for_epoch(epoch, channel, meter)?;
        if let Some(keys) = keys.as_deref_mut() {
            keys.push(StoreKey::from_digest(&digest, meter)?);
        }
        channel_hashes.push(digest);
    }
    sort(&mut channel_hashes, |hash| &hash.0, meter)?;
    let mut encoded_patterns = vector(patterns.len(), meter)?;
    for pattern in patterns {
        encoded_patterns.push(encode_for_epoch(epoch, pattern, meter)?);
    }
    sort(&mut encoded_patterns, Vec::as_slice, meter)?;
    let continuation = encode_for_epoch(epoch, continuation, meter)?;
    let persist = [u8::from(persistent)];
    let extra = usize::from(epoch != EventCodecEpoch::Legacy);
    let count = channels
        .len()
        .checked_add(patterns.len())
        .and_then(|n| n.checked_add(2 + extra))
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut parts = vector(count, meter)?;
    if epoch != EventCodecEpoch::Legacy {
        parts.push(COST_EVENT_DOMAIN_V1);
    }
    parts.extend(channel_hashes.iter().map(|hash| hash.0.as_slice()));
    parts.extend(encoded_patterns.iter().map(Vec::as_slice));
    parts.push(continuation.as_slice());
    parts.push(persist.as_slice());
    let source_hash = hash(&parts, meter)?;
    Ok(Consume {
        channel_hashes,
        hash: source_hash,
        persistent,
    })
}

pub fn consume_keys_for_epoch<
    C: StableHashSerialize,
    P: StableHashSerialize,
    K: StableHashSerialize,
>(
    epoch: EventCodecEpoch,
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<(Consume, Vec<StoreKey>)> {
    let mut keys = vector(channels.len(), meter)?;
    let source = consume_for_epoch_with(
        epoch,
        channels,
        patterns,
        continuation,
        persistent,
        Some(&mut keys),
        meter,
    )?;
    Ok((source, keys))
}

#[cfg(test)]
mod tests;
