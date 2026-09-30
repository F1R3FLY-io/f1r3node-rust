use std::io::Write;
use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeCandidateIdentity;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use serde::Serialize;

use super::recording::{recording_error, work};
use super::{HostWorkBudget, InterpreterError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeProduceSource {
    pub channel: [u8; 32],
    pub hash: [u8; 32],
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeConsumeSource {
    pub channels: Arc<[[u8; 32]]>,
    pub hash: [u8; 32],
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeOperationSource {
    Produce(NativeProduceSource),
    Consume(NativeConsumeSource),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCommSource {
    pub consume: NativeConsumeSource,
    pub produces: Arc<[NativeProduceSource]>,
    pub peeks: Arc<[i32]>,
    pub repetitions: Arc<[(NativeProduceSource, i32)]>,
}

fn hash(bytes: &[u8]) -> Result<[u8; 32], InterpreterError> {
    bytes
        .try_into()
        .map_err(|_| recording_error("native operation source hash has invalid length"))
}

fn checked_add(left: usize, right: usize) -> Result<usize, InterpreterError> {
    left.checked_add(right)
        .ok_or(InterpreterError::HostWorkRejected)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, InterpreterError> {
    left.checked_mul(right)
        .ok_or(InterpreterError::HostWorkRejected)
}

fn metered_bytes(left: &[u8], right: &[u8], meter: &dyn SourceMeter) -> Result<bool, RSpaceError> {
    let bytes = left
        .len()
        .checked_add(right.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(1, bytes, 0)?;
    Ok(left == right)
}

pub(super) fn allocate<T>(
    count: usize,
    budget: &HostWorkBudget,
) -> Result<Vec<T>, InterpreterError> {
    work(
        budget,
        HostWorkDimension::VerificationOperations,
        checked_add(count, 1)?,
    )?;
    work(
        budget,
        HostWorkDimension::SearchStateBytes,
        checked_mul(checked_mul(count, size_of::<T>())?, 2)?,
    )?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    Ok(result)
}

impl NativeProduceSource {
    fn capture(source: &Produce) -> Result<Self, InterpreterError> {
        Ok(Self {
            channel: hash(&source.channel_hash.0)?,
            hash: hash(&source.hash.0)?,
            persistent: source.persistent,
        })
    }

    fn matches(&self, source: &Produce) -> bool {
        self.channel.as_slice() == source.channel_hash.0
            && self.hash.as_slice() == source.hash.0
            && self.persistent == source.persistent
    }

    fn metered_matches(
        &self,
        source: &Produce,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        if !metered_bytes(&self.channel, &source.channel_hash.0, meter)? {
            return Ok(false);
        }
        if !metered_bytes(&self.hash, &source.hash.0, meter)? {
            return Ok(false);
        }
        meter.reserve(1, 2, 0)?;
        Ok(self.persistent == source.persistent)
    }
}

impl NativeConsumeSource {
    fn capture(source: &Consume, budget: &HostWorkBudget) -> Result<Self, InterpreterError> {
        let mut channels = allocate(source.channel_hashes.len(), budget)?;
        for channel in &source.channel_hashes {
            channels.push(hash(&channel.0)?);
        }
        Ok(Self {
            channels: channels.into(),
            hash: hash(&source.hash.0)?,
            persistent: source.persistent,
        })
    }

    fn matches(&self, source: &Consume) -> bool {
        self.hash.as_slice() == source.hash.0
            && self.persistent == source.persistent
            && self.channels.len() == source.channel_hashes.len()
            && self
                .channels
                .iter()
                .zip(&source.channel_hashes)
                .all(|(a, b)| a.as_slice() == b.0)
    }

    fn metered_matches(
        &self,
        source: &Consume,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        if !metered_bytes(&self.hash, &source.hash.0, meter)? {
            return Ok(false);
        }
        meter.reserve(1, 2, 0)?;
        if self.persistent != source.persistent {
            return Ok(false);
        }
        meter.reserve(1, size_of::<usize>() * 2, 0)?;
        if self.channels.len() != source.channel_hashes.len() {
            return Ok(false);
        }
        for (expected, actual) in self.channels.iter().zip(&source.channel_hashes) {
            if !metered_bytes(expected, &actual.0, meter)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl NativeOperationSource {
    pub(super) fn capture(
        source: RSpaceOperationSource<'_>,
        budget: &HostWorkBudget,
    ) -> Result<Self, InterpreterError> {
        work(budget, HostWorkDimension::VerificationBytes, 128)?;
        match source {
            RSpaceOperationSource::Produce(source) => {
                Ok(Self::Produce(NativeProduceSource::capture(source)?))
            }
            RSpaceOperationSource::Consume(source) => {
                work(
                    budget,
                    HostWorkDimension::VerificationBytes,
                    checked_mul(source.channel_hashes.len(), 64)?,
                )?;
                Ok(Self::Consume(NativeConsumeSource::capture(source, budget)?))
            }
        }
    }

    pub(super) fn metered_matches(
        &self,
        source: RSpaceOperationSource<'_>,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        match (self, source) {
            (Self::Produce(expected), RSpaceOperationSource::Produce(actual)) => {
                expected.metered_matches(actual, meter)
            }
            (Self::Consume(expected), RSpaceOperationSource::Consume(actual)) => {
                expected.metered_matches(actual, meter)
            }
            _ => {
                meter.reserve(1, 0, 0)?;
                Ok(false)
            }
        }
    }
}

impl NativeCommSource {
    pub(super) fn matches(&self, source: &COMM) -> bool {
        self.consume.matches(&source.consume)
            && self.produces.len() == source.produces.len()
            && self
                .produces
                .iter()
                .zip(&source.produces)
                .all(|(a, b)| a.matches(b))
            && self.peeks.len() == source.peeks.len()
            && self.peeks.iter().eq(source.peeks.iter())
            && self.repetitions.len() == source.times_repeated.len()
            && self
                .repetitions
                .iter()
                .zip(&source.times_repeated)
                .all(|((a, n), (b, m))| n == m && a.matches(b))
    }

    pub(super) fn metered_matches(
        &self,
        source: &COMM,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        if !self.consume.metered_matches(&source.consume, meter)? {
            return Ok(false);
        }
        meter.reserve(1, size_of::<usize>() * 2, 0)?;
        if self.produces.len() != source.produces.len() {
            return Ok(false);
        }
        for (expected, actual) in self.produces.iter().zip(&source.produces) {
            if !expected.metered_matches(actual, meter)? {
                return Ok(false);
            }
        }
        meter.reserve(1, size_of::<usize>() * 2, 0)?;
        if self.peeks.len() != source.peeks.len() {
            return Ok(false);
        }
        for (expected, actual) in self.peeks.iter().zip(&source.peeks) {
            meter.reserve(1, size_of::<i32>() * 2, 0)?;
            if expected != actual {
                return Ok(false);
            }
        }
        meter.reserve(1, size_of::<usize>() * 2, 0)?;
        if self.repetitions.len() != source.times_repeated.len() {
            return Ok(false);
        }
        for ((expected, count), (actual, actual_count)) in
            self.repetitions.iter().zip(&source.times_repeated)
        {
            meter.reserve(1, size_of::<i32>() * 2, 0)?;
            if count != actual_count || !expected.metered_matches(actual, meter)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn capture(
        source: &COMM,
        budget: &HostWorkBudget,
    ) -> Result<Self, InterpreterError> {
        let count = checked_add(source.produces.len(), source.times_repeated.len())?;
        work(
            budget,
            HostWorkDimension::VerificationBytes,
            checked_add(
                checked_mul(count, 65)?,
                checked_mul(source.consume.channel_hashes.len(), 32)?,
            )?,
        )?;
        let mut produces = allocate(source.produces.len(), budget)?;
        for produce in &source.produces {
            produces.push(NativeProduceSource::capture(produce)?);
        }
        let mut peeks = allocate(source.peeks.len(), budget)?;
        peeks.extend(source.peeks.iter().copied());
        let mut repetitions = allocate(source.times_repeated.len(), budget)?;
        for (produce, count) in &source.times_repeated {
            repetitions.push((NativeProduceSource::capture(produce)?, *count));
        }
        Ok(Self {
            consume: NativeConsumeSource::capture(&source.consume, budget)?,
            produces: produces.into(),
            peeks: peeks.into(),
            repetitions: repetitions.into(),
        })
    }
}

impl NativeCandidateIdentity for NativeCommSource {
    fn matches_consume(&self, source: &Consume) -> bool { self.consume.matches(source) }

    fn matches_produce(&self, source: &Produce) -> bool {
        self.produces
            .iter()
            .any(|expected| expected.matches(source))
    }

    fn repetition(&self, source: &Produce) -> Option<i32> {
        self.repetitions
            .iter()
            .find_map(|(expected, count)| expected.matches(source).then_some(*count))
    }

    fn matches_comm(&self, source: &COMM) -> bool { self.matches(source) }

    fn metered_matches_consume(
        &self,
        source: &Consume,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        self.consume.metered_matches(source, meter)
    }

    fn metered_matches_produce(
        &self,
        source: &Produce,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        for expected in self.produces.iter() {
            if expected.metered_matches(source, meter)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn metered_repetition(
        &self,
        source: &Produce,
        meter: &dyn SourceMeter,
    ) -> Result<Option<i32>, RSpaceError> {
        for (expected, count) in self.repetitions.iter() {
            if expected.metered_matches(source, meter)? {
                return Ok(Some(*count));
            }
        }
        Ok(None)
    }

    fn metered_matches_comm(
        &self,
        source: &COMM,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        self.metered_matches(source, meter)
    }
}

struct ChannelWriter<'a> {
    bytes: Vec<u8>,
    budget: &'a HostWorkBudget,
}

impl Write for ChannelWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let reserve = || -> Result<(), InterpreterError> {
            work(self.budget, HostWorkDimension::VerificationOperations, 1)?;
            work(
                self.budget,
                HostWorkDimension::VerificationBytes,
                bytes.len(),
            )?;
            work(
                self.budget,
                HostWorkDimension::SearchStateBytes,
                checked_add(checked_mul(bytes.len(), 4)?, 16)?,
            )?;
            if checked_add(self.bytes.len(), bytes.len())? > self.bytes.capacity() {
                work(
                    self.budget,
                    HostWorkDimension::VerificationBytes,
                    self.bytes.len(),
                )?;
            }
            Ok(())
        };
        reserve().map_err(std::io::Error::other)?;
        self.bytes
            .try_reserve(bytes.len())
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

pub(super) fn channel_bytes(
    channel: &impl Serialize,
    budget: &HostWorkBudget,
) -> Result<Arc<[u8]>, InterpreterError> {
    let mut writer = ChannelWriter {
        bytes: Vec::new(),
        budget,
    };
    bincode::serialize_into(&mut writer, channel)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    Ok(writer.bytes.into())
}
