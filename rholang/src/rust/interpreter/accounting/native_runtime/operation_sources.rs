use std::io::Write;
use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
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

pub(super) fn allocate<T>(
    count: usize,
    budget: &HostWorkBudget,
) -> Result<Vec<T>, InterpreterError> {
    work(
        budget,
        HostWorkDimension::VerificationOperations,
        count.saturating_add(1),
    )?;
    work(
        budget,
        HostWorkDimension::SearchStateBytes,
        count.saturating_mul(size_of::<T>()).saturating_mul(2),
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
                    source.channel_hashes.len().saturating_mul(64),
                )?;
                Ok(Self::Consume(NativeConsumeSource::capture(source, budget)?))
            }
        }
    }

    pub(super) fn matches(&self, source: RSpaceOperationSource<'_>) -> bool {
        match (self, source) {
            (Self::Produce(a), RSpaceOperationSource::Produce(b)) => a.matches(b),
            (Self::Consume(a), RSpaceOperationSource::Consume(b)) => a.matches(b),
            _ => false,
        }
    }
}

impl NativeCommSource {
    pub(super) fn reserve_comparison(
        &self,
        budget: &HostWorkBudget,
    ) -> Result<(), InterpreterError> {
        let producers = self.produces.len().saturating_add(self.repetitions.len());
        work(
            budget,
            HostWorkDimension::VerificationOperations,
            producers
                .saturating_add(self.consume.channels.len())
                .saturating_add(self.peeks.len())
                .saturating_add(1),
        )?;
        work(
            budget,
            HostWorkDimension::VerificationBytes,
            producers
                .saturating_mul(65)
                .saturating_add(self.consume.channels.len().saturating_mul(32))
                .saturating_add(
                    self.peeks
                        .len()
                        .saturating_add(self.repetitions.len())
                        .saturating_mul(4),
                )
                .saturating_add(33),
        )
    }

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

    pub(super) fn capture(
        source: &COMM,
        budget: &HostWorkBudget,
    ) -> Result<Self, InterpreterError> {
        let count = source
            .produces
            .len()
            .saturating_add(source.times_repeated.len());
        work(
            budget,
            HostWorkDimension::VerificationBytes,
            count
                .saturating_mul(65)
                .saturating_add(source.consume.channel_hashes.len().saturating_mul(32)),
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
                bytes.len().saturating_mul(4).saturating_add(16),
            )?;
            if self.bytes.len().saturating_add(bytes.len()) > self.bytes.capacity() {
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
