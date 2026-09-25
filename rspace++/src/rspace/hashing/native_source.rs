use std::io::Write;
use std::mem::size_of;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::Serialize;

use super::blake2b256_hash::Blake2b256Hash;
use crate::rspace::errors::RSpaceError;
use crate::rspace::trace::event::{Consume, Produce};

type Result<T> = std::result::Result<T, RSpaceError>;

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
    meter.reserve(1, 0, 0)?;
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

fn encode<T: Serialize + ?Sized>(value: &T, meter: &impl SourceMeter) -> Result<Vec<u8>> {
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

fn hash<T: Serialize + ?Sized>(value: &T, meter: &impl SourceMeter) -> Result<Blake2b256Hash> {
    match serialize(value, Output::Hash(Blake2b::<U32>::new()), meter)? {
        Output::Hash(hash) => {
            let mut bytes = vector(32, meter)?;
            bytes.extend_from_slice(&hash.finalize());
            Ok(Blake2b256Hash::from_bytes(bytes))
        }
        Output::Bytes { .. } => unreachable!(),
    }
}

fn vector<T>(count: usize, meter: &impl SourceMeter) -> Result<Vec<T>> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count.saturating_add(1), bytes, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

fn sort<T>(values: &mut [T], key: impl Fn(&T) -> &[u8], meter: &impl SourceMeter) -> Result<()> {
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

pub fn consume<C: Serialize, P: Serialize, K: Serialize>(
    channels: &[C],
    patterns: &[P],
    continuation: &K,
    persistent: bool,
    meter: &impl SourceMeter,
) -> Result<Consume> {
    let mut channel_hashes = vector(channels.len(), meter)?;
    for channel in channels {
        channel_hashes.push(hash(channel, meter)?);
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

#[cfg(test)]
mod tests;
