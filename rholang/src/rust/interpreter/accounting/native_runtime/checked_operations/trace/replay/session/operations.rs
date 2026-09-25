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
        use crate::rust::interpreter::accounting::native_runtime::clone_backing;
        clone_backing::inspect(channel, &self.host).map_err(error)?;
        clone_backing::inspect(data, &self.host).map_err(error)
    }

    fn prepare_consume_source(
        &self,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
    ) -> Result<(), RSpaceError> {
        use crate::rust::interpreter::accounting::native_runtime::clone_backing;
        clone_backing::inspect_slice(channels, &self.host).map_err(error)?;
        clone_backing::inspect_slice(patterns, &self.host).map_err(error)?;
        clone_backing::inspect(continuation, &self.host).map_err(error)
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
    type Authority = CostAuthority;
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
        authority: &CostAuthority,
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
        authority: &CostAuthority,
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
        if !self
            .operation()
            .source
            .matches(RSpaceOperationSource::Produce(source))
        {
            return Err(error(NativeReplayError::Source));
        }
        let comm = trace
            .comm(self.slot)
            .ok_or_else(|| error(NativeReplayError::CommSource))?;
        work(
            host,
            HostWorkDimension::VerificationOperations,
            comm.produces.len().saturating_add(1),
        )
        .map_err(error)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            comm.produces.len().saturating_mul(96),
        )
        .map_err(error)?;
        let recorded = comm
            .produces
            .iter()
            .find(|candidate| {
                candidate.hash == source.hash
                    && candidate.channel_hash == source.channel_hash
                    && candidate.persistent == source.persistent
            })
            .or_else(|| match trace.introduction(self.slot) {
                Some(IOEvent::Produce(source)) => Some(source),
                _ => None,
            })
            .ok_or_else(|| error(NativeReplayError::Source))?;
        work(
            host,
            HostWorkDimension::VerificationOperations,
            recorded.output_value.len().saturating_add(1),
        )
        .map_err(error)?;
        work(
            host,
            HostWorkDimension::SearchStateBytes,
            128usize.saturating_add(
                recorded
                    .output_value
                    .len()
                    .saturating_mul(size_of::<Vec<u8>>())
                    .saturating_mul(2),
            ),
        )
        .map_err(error)?;
        for bytes in &recorded.output_value {
            work(
                host,
                HostWorkDimension::SearchStateBytes,
                bytes.len().saturating_mul(2),
            )
            .map_err(error)?;
            work(host, HostWorkDimension::VerificationBytes, bytes.len()).map_err(error)?;
        }
        Ok(recorded.clone())
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
        self.inner
            .produce(channel, data, persistent, authority)
            .await
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
        self.inner
            .consume(
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
                authority,
            )
            .await
    }
}

#[cfg(test)]
mod tests;
