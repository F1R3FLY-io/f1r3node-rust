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

#[async_trait::async_trait]
impl NativeOperationEpoch<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for SessionEpoch
{
    type Ticket = NativeReplayReservation;

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
        let mut participants = Vec::with_capacity(data.len());
        participants.extend(
            data.iter()
                .map(|candidate| (candidate.datum.a.as_ref(), candidate.datum.persist)),
        );
        decision(self.observe_rho_comm(source, continuation, persistent, &participants))
    }

    fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError> {
        let trace = &self.replay.inner.trace;
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
        let mut recorded = None;
        for candidate in &comm.produces {
            if candidate.hash == source.hash
                && candidate.channel_hash == source.channel_hash
                && candidate.persistent == source.persistent
            {
                recorded = Some(candidate);
                break;
            }
        }
        if recorded.is_none() {
            if let Some(IOEvent::Produce(introduction)) = trace.introduction(self.slot) {
                recorded = Some(introduction);
            }
        }
        let recorded = recorded.ok_or_else(|| error(NativeReplayError::Source))?;
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
