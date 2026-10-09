use std::collections::BTreeSet;

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::internal::ConsumeCandidate;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeCandidateIdentity, NativeOperationEpoch, NativeOperationPublication,
    NativeOperationTicket, NativeReplayDecision,
};
use rspace_plus_plus::rspace::rspace_interface::{MaybeConsumeResult, MaybeProduceResult};
use rspace_plus_plus::rspace::trace::event::{Consume, IOEvent, Produce};

use super::*;
use crate::rust::interpreter::accounting::IntroductionRecord;

fn decision(
    result: Result<NativeBudgetReplayDecision, NativeReplayError>,
) -> Result<NativeReplayDecision, RSpaceError> {
    result
        .map(|result| match result {
            NativeBudgetReplayDecision::Accepted { .. } => NativeReplayDecision::Granted,
            NativeBudgetReplayDecision::Denied => NativeReplayDecision::Denied,
        })
        .map_err(error)
}

/// D-E3 (DR-110): the charge of the produce source preparation. The source
/// pass hashes the channel and encodes the data with bincode, and its writer
/// reserves only the bytes that it writes (`native_source::produce`). Each
/// value is released once after the operation. A block inspection prepays one
/// traversal, so each value gets two inspections: the source pass and the
/// release.
fn charge_produce_source(
    host: &HostWorkBudget,
    channel: &Par,
    data: &ListParWithRandom,
) -> Result<(), InterpreterError> {
    use crate::rust::interpreter::accounting::native_runtime::clone_backing;
    // Changed by D-O1 (DR-110): a per-level inspection charges each inline
    // byte three times, which paid the source pass and the release.
    // clone_backing::inspect(channel, &self.host).map_err(error)?;
    // clone_backing::inspect(data, &self.host).map_err(error)
    clone_backing::inspect_blocks(channel, host)?;
    clone_backing::inspect_blocks(data, host)?;
    // Added by D-E3 (DR-110): the release of each value.
    clone_backing::inspect_blocks(channel, host)?;
    clone_backing::inspect_blocks(data, host)
}

/// D-E3 (DR-110): the charge of the consume source preparation. The source
/// pass hashes each channel and encodes each pattern and the continuation
/// with bincode, and its writer reserves only the bytes that it writes
/// (`native_source::consume_keys`). Each value is released once after the
/// operation, so each value gets two block inspections, as in the produce.
fn charge_consume_source(
    host: &HostWorkBudget,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
) -> Result<(), InterpreterError> {
    use crate::rust::interpreter::accounting::native_runtime::clone_backing;
    // Changed by D-O1 (DR-110): a per-level inspection charges each inline
    // byte three times, which paid the source pass and the release.
    // clone_backing::inspect_slice(channels, &self.host).map_err(error)?;
    // clone_backing::inspect_slice(patterns, &self.host).map_err(error)?;
    // clone_backing::inspect(continuation, &self.host).map_err(error)
    clone_backing::inspect_blocks_slice(channels, host)?;
    clone_backing::inspect_blocks_slice(patterns, host)?;
    clone_backing::inspect_blocks(continuation, host)?;
    // Added by D-E3 (DR-110): the release of each value.
    clone_backing::inspect_blocks_slice(channels, host)?;
    clone_backing::inspect_blocks_slice(patterns, host)?;
    clone_backing::inspect_blocks(continuation, host)
}

#[async_trait::async_trait]
impl NativeOperationEpoch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for SessionEpoch
{
    type Ticket = NativeReplayReservation;

    fn prepare_produce_source(
        &self,
        channel: &Par,
        data: &ListParWithRandom,
    ) -> Result<(), RSpaceError> {
        charge_produce_source(&self.host, channel, data).map_err(error)
    }

    fn prepare_consume_source(
        &self,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
    ) -> Result<(), RSpaceError> {
        charge_consume_source(&self.host, channels, patterns, continuation).map_err(error)
    }

    async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), RSpaceError> {
        self.replay.wait_ready(source).await.map_err(error)
    }

    fn begin_operation(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<Self::Ticket>, RSpaceError> {
        self.replay.reserve_ready(source).map_err(error)
    }
}

impl NativeOperationPublication for NativeReplayPublication {
    fn publish(&mut self) { self.publish_in_place(); }
}

impl NativeOperationTicket<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for NativeReplayReservation
{
    // Changed by D-F2 (DR-118): the resolved introduction is the whole record,
    // so the observers can reuse the reducer's measurement.
    // type Authority = CostAuthority;
    type Authority = IntroductionRecord;
    type Publication = NativeReplayPublication;

    fn outcome(&self) -> NativeReplayOutcome { self.expected_outcome() }

    fn candidate_identity(&self) -> Option<&dyn NativeCandidateIdentity> {
        self.operation()
            .comm
            .as_ref()
            .map(|row| row.source.as_ref() as &dyn NativeCandidateIdentity)
    }

    fn authenticate_footprint(
        &mut self,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<(), RSpaceError> {
        NativeReplayReservation::authenticate_footprint(self, channels, joins).map_err(error)
    }

    fn observe_produce(
        &mut self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        // Changed by D-F2 (DR-118): the resolved introduction record.
        // authority: &CostAuthority,
        authority: &IntroductionRecord,
    ) -> Result<NativeReplayDecision, RSpaceError> {
        decision(self.observe_rho_produce(source, channel, data, authority))
    }

    fn observe_consume(
        &mut self,
        source: &Consume,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
        peeks: &BTreeSet<i32>,
        // Changed by D-F2 (DR-118): the resolved introduction record.
        // authority: &CostAuthority,
        authority: &IntroductionRecord,
    ) -> Result<NativeReplayDecision, RSpaceError> {
        decision(self.observe_rho_consume(
            source,
            channels,
            patterns,
            continuation,
            peeks,
            authority,
        ))
    }

    fn observe_comm(
        &mut self,
        source: &COMM,
        continuation: &TaggedContinuation,
        persistent: bool,
        data: &[ConsumeCandidate<Par, ListParWithRandom>],
    ) -> Result<NativeReplayDecision, RSpaceError> {
        let mut participants = allocate(data.len(), &self.replay.inner.host).map_err(error)?;
        participants.extend(
            data.iter()
                .map(|candidate| (&candidate.datum.a, candidate.datum.persist)),
        );
        decision(self.observe_rho_comm(source, continuation, persistent, &participants))
    }

    fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError> {
        let trace = &self.replay.inner.trace;
        let host = &self.replay.inner.host;
        let meter =
            |operations, scanned, backing| reserve_source(host, operations, scanned, backing);
        if !self
            .operation()
            .source
            .metered_matches(RSpaceOperationSource::Produce(source), &meter)?
        {
            return Err(error(NativeReplayError::Source));
        }
        let comm = trace
            .comm(self.slot)
            .ok_or_else(|| error(NativeReplayError::CommSource))?;
        let mut recorded = None;
        for candidate in &comm.produces {
            let scanned = candidate
                .hash
                .0
                .len()
                .checked_add(source.hash.0.len())
                .and_then(|bytes| bytes.checked_add(candidate.channel_hash.0.len()))
                .and_then(|bytes| bytes.checked_add(source.channel_hash.0.len()))
                .and_then(|bytes| bytes.checked_add(2))
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter(1, scanned, 0)?;
            if candidate.hash == source.hash
                && candidate.channel_hash == source.channel_hash
                && candidate.persistent == source.persistent
            {
                recorded = Some(candidate);
                break;
            }
        }
        if recorded.is_none() {
            meter(1, 0, 0)?;
            if let Some(IOEvent::Produce(introduction)) = trace.introduction(self.slot) {
                recorded = Some(introduction);
            }
        }
        let recorded = recorded.ok_or_else(|| error(NativeReplayError::Source))?;
        rspace_plus_plus::rspace::hashing::native_source::clone_produce(recorded, &meter)
    }

    fn prepare(self, outcome: NativeReplayOutcome) -> Result<Self::Publication, RSpaceError> {
        self.prepare_completion(outcome).map_err(error)
    }
}

impl NativeRuntimeReplaySession<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    pub async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        authority: &CostAuthority,
    ) -> Result<
        MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        // Changed by D-F2 (DR-118): the session resolves a record. This entry
        // takes only an authority, so its record has no measurement and the
        // observer walks the datum.
        // self.inner
        //     .produce(channel, data, persistent, authority)
        //     .await
        let record = IntroductionRecord {
            authority: authority.clone(),
            measurement: None,
        };
        self.inner.produce(channel, data, persistent, &record).await
    }

    pub async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
        authority: &CostAuthority,
    ) -> Result<
        MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        // Changed by D-F2 (DR-118): the session resolves a record. This entry
        // takes only an authority, so its record has no measurement and the
        // observer walks the continuation.
        // self.inner
        //     .consume(
        //         channels,
        //         patterns,
        //         continuation,
        //         persistent,
        //         peeks,
        //         authority,
        //     )
        //     .await
        let record = IntroductionRecord {
            authority: authority.clone(),
            measurement: None,
        };
        self.inner
            .consume(channels, patterns, continuation, persistent, peeks, &record)
            .await
    }
}

#[cfg(test)]
mod tests;
