use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeCandidateIdentity;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, RestingCOMM, COMM};
use serde::Serialize;

use super::recording::recording_error;
use super::InterpreterError;

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
    Resting(NativeRestingSource),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRestingSource {
    pub consume: NativeConsumeSource,
    pub continuation_index: i32,
    pub ordered_produces: Arc<[NativeProduceSource]>,
    pub ordered_data_indices: Arc<[i32]>,
    pub peeks: Arc<[i32]>,
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
    fn capture(source: &Consume) -> Result<Self, InterpreterError> {
        let mut channels = Vec::with_capacity(source.channel_hashes.len());
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

impl NativeRestingSource {
    fn capture(source: &RestingCOMM) -> Result<Self, InterpreterError> {
        let count = source.ordered_produces.len();
        if source.consume.channel_hashes.len() != count
            || source.ordered_data_indices.len() != count
            || source.continuation_index < 0
            || source.ordered_data_indices.iter().any(|index| *index < 0)
            || source
                .peeks
                .iter()
                .any(|index| usize::try_from(*index).map_or(true, |index| index >= count))
        {
            return Err(recording_error(
                "resting source has invalid ordered address",
            ));
        }
        let consume = NativeConsumeSource::capture(&source.consume)?;
        let mut produces = Vec::with_capacity(count);
        for (channel, produce) in source
            .consume
            .channel_hashes
            .iter()
            .zip(&source.ordered_produces)
        {
            if channel.0.as_slice() != produce.channel_hash.0.as_slice() {
                return Err(recording_error(
                    "resting source ordered channel differs from waiter",
                ));
            }
            produces.push(NativeProduceSource::capture(produce)?);
        }
        let mut indices = Vec::with_capacity(count);
        indices.extend_from_slice(&source.ordered_data_indices);
        let mut peeks = Vec::with_capacity(source.peeks.len());
        peeks.extend(source.peeks.iter().copied());
        Ok(Self {
            consume,
            continuation_index: source.continuation_index,
            ordered_produces: produces.into(),
            ordered_data_indices: indices.into(),
            peeks: peeks.into(),
        })
    }

    fn matches(&self, actual: &RestingCOMM) -> bool {
        if self.continuation_index != actual.continuation_index
            || self.ordered_produces.len() != actual.ordered_produces.len()
            || self.ordered_data_indices.len() != actual.ordered_data_indices.len()
            || self.peeks.len() != actual.peeks.len()
            || !self.consume.matches(&actual.consume)
        {
            return false;
        }
        for (expected, actual) in self.ordered_produces.iter().zip(&actual.ordered_produces) {
            if !expected.matches(actual) {
                return false;
            }
        }
        for (expected, actual) in self
            .ordered_data_indices
            .iter()
            .zip(&actual.ordered_data_indices)
        {
            if expected != actual {
                return false;
            }
        }
        for (expected, actual) in self.peeks.iter().zip(&actual.peeks) {
            if expected != actual {
                return false;
            }
        }
        true
    }
}

impl NativeOperationSource {
    // Added by the merge of feature/cost-accounted-rho (clippy gate): only the
    // tests call this Legacy wrapper. Production calls `capture_for_epoch`.
    #[cfg(test)]
    pub(super) fn capture(source: RSpaceOperationSource<'_>) -> Result<Self, InterpreterError> {
        Self::capture_for_epoch(source, EventCodecEpoch::Legacy)
    }

    pub(super) fn capture_for_epoch(
        source: RSpaceOperationSource<'_>,
        epoch: EventCodecEpoch,
    ) -> Result<Self, InterpreterError> {
        match source {
            RSpaceOperationSource::Produce(source) => {
                Ok(Self::Produce(NativeProduceSource::capture(source)?))
            }
            RSpaceOperationSource::Consume(source) => {
                Ok(Self::Consume(NativeConsumeSource::capture(source)?))
            }
            RSpaceOperationSource::Resting(source) if epoch == EventCodecEpoch::CostV2 => {
                Ok(Self::Resting(NativeRestingSource::capture(source)?))
            }
            RSpaceOperationSource::Resting(_) => {
                Err(recording_error("Resting source requires CostV2"))
            }
        }
    }

    pub(super) fn matches(&self, source: RSpaceOperationSource<'_>) -> bool {
        match (self, source) {
            (Self::Produce(expected), RSpaceOperationSource::Produce(actual)) => {
                expected.matches(actual)
            }
            (Self::Consume(expected), RSpaceOperationSource::Consume(actual)) => {
                expected.matches(actual)
            }
            (Self::Resting(expected), RSpaceOperationSource::Resting(actual)) => {
                expected.matches(actual)
            }
            _ => false,
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

    pub(super) fn capture(source: &COMM) -> Result<Self, InterpreterError> {
        let mut produces = Vec::with_capacity(source.produces.len());
        for produce in &source.produces {
            produces.push(NativeProduceSource::capture(produce)?);
        }
        let mut peeks = Vec::with_capacity(source.peeks.len());
        peeks.extend(source.peeks.iter().copied());
        let mut repetitions = Vec::with_capacity(source.times_repeated.len());
        for (produce, count) in &source.times_repeated {
            repetitions.push((NativeProduceSource::capture(produce)?, *count));
        }
        Ok(Self {
            consume: NativeConsumeSource::capture(&source.consume)?,
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

pub(super) fn channel_bytes(channel: &impl Serialize) -> Result<Arc<[u8]>, InterpreterError> {
    bincode::serialize(channel)
        .map(Arc::from)
        .map_err(|_| recording_error("native operation channel does not serialize"))
}
