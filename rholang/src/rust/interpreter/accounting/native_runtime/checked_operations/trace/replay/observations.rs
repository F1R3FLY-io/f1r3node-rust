use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use super::*;
use crate::rust::interpreter::accounting::observation_construction;

fn construction_error(error: impl std::fmt::Display) -> NativeReplayError {
    NativeReplayError::Construction(error.to_string())
}

impl NativeReplayReservation {
    fn authenticate_introduction_source(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<(), NativeReplayError> {
        if self.stage != ObservationStage::Introduction {
            return Err(NativeReplayError::Stage);
        }
        let channels = match &self.operation().source {
            NativeOperationSource::Produce(_) => 1,
            NativeOperationSource::Consume(source) => source.channels.len(),
        };
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationOperations,
            channels.saturating_add(1),
        )?;
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationBytes,
            channels.saturating_mul(32).saturating_add(64),
        )?;
        if !self.operation().source.matches(source) {
            return Err(NativeReplayError::Source);
        }
        Ok(())
    }

    pub fn observe_rho_produce(
        &mut self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        introduction_authority: &CostAuthority,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_introduction_source(RSpaceOperationSource::Produce(source))?;
        let observed = observation_construction::produce_introduction(
            source,
            channel,
            data,
            introduction_authority,
        )
        .map_err(construction_error)?
        .into_native()?;
        self.observe_introduction(&observed)
    }

    pub fn observe_rho_consume(
        &mut self,
        source: &Consume,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
        peeks: &std::collections::BTreeSet<i32>,
        introduction_authority: &CostAuthority,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_introduction_source(RSpaceOperationSource::Consume(source))?;
        let expected = self
            .operation()
            .consume_peeks
            .as_ref()
            .ok_or(NativeReplayError::Source)?;
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationOperations,
            peeks.len().saturating_add(1),
        )?;
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationBytes,
            peeks.len().saturating_mul(size_of::<i32>()),
        )?;
        if peeks.len() != expected.len() || !peeks.iter().eq(expected.iter()) {
            return Err(NativeReplayError::Source);
        }
        let observed = observation_construction::consume_introduction(
            source,
            channels,
            patterns,
            continuation,
            introduction_authority,
        )
        .map_err(construction_error)?
        .into_native()?;
        self.observe_introduction(&observed)
    }

    pub fn observe_rho_comm(
        &mut self,
        source: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        data: &[(&ListParWithRandom, bool)],
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_comm_source(source)?;
        let observed =
            observation_construction::comm(source, continuation, continuation_persistent, data)
                .map_err(construction_error)?
                .into_native()?;
        let link = self
            .operation()
            .comm
            .as_ref()
            .ok_or(NativeReplayError::Stage)?
            .observation;
        self.observe(link, &observed, ObservationStage::Complete)
    }
}
