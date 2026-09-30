use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use super::*;
use crate::rust::interpreter::accounting::observation_construction;

fn construction_error(error: RSpaceError) -> NativeReplayError {
    match error {
        RSpaceError::HostWorkRejected => {
            NativeReplayError::Host(InterpreterError::HostWorkRejected)
        }
        other => NativeReplayError::Construction(other.to_string()),
    }
}

impl NativeReplayReservation {
    fn authenticate_introduction_source(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<(), NativeReplayError> {
        if self.stage != ObservationStage::Introduction {
            return Err(NativeReplayError::Stage);
        }
        let meter = |operations, scanned, backing| {
            reserve_source(&self.replay.inner.host, operations, scanned, backing)
        };
        if !self
            .operation()
            .source
            .metered_matches(source, &meter)
            .map_err(|error| NativeReplayError::Host(error.into()))?
        {
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
        let meter = |operations, scanned, backing| {
            reserve_source(&self.replay.inner.host, operations, scanned, backing)
        };
        let observed = observation_construction::produce_introduction_metered(
            source,
            channel,
            data,
            introduction_authority,
            &meter,
        )
        .map_err(construction_error)?
        .into_native_metered(&meter)
        .map_err(construction_error)?;
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
            checked_add(peeks.len(), 1)?,
        )?;
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationBytes,
            checked_mul(peeks.len(), size_of::<i32>())?,
        )?;
        if peeks.len() != expected.len() || !peeks.iter().eq(expected.iter()) {
            return Err(NativeReplayError::Source);
        }
        let meter = |operations, scanned, backing| {
            reserve_source(&self.replay.inner.host, operations, scanned, backing)
        };
        let observed = observation_construction::consume_introduction_metered(
            source,
            channels,
            patterns,
            continuation,
            introduction_authority,
            &meter,
        )
        .map_err(construction_error)?
        .into_native_metered(&meter)
        .map_err(construction_error)?;
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
        let meter = |operations, scanned, backing| {
            reserve_source(&self.replay.inner.host, operations, scanned, backing)
        };
        let observed = observation_construction::comm_metered(
            source,
            continuation,
            continuation_persistent,
            data,
            &meter,
        )
        .map_err(construction_error)?
        .into_native_metered(&meter)
        .map_err(construction_error)?;
        let link = self
            .operation()
            .comm
            .as_ref()
            .ok_or(NativeReplayError::Stage)?
            .observation;
        self.observe(link, &observed, ObservationStage::Complete)
    }
}
