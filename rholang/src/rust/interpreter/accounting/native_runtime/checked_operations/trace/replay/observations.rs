// Changed by D-F2 (DR-118): the introduction observers take a record.
// use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

use super::*;
use crate::rust::interpreter::accounting::{observation_construction, IntroductionRecord};

fn construction_error(error: RSpaceError) -> NativeReplayError {
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
        if !self.operation().source.matches(source) {
            return Err(NativeReplayError::Source);
        }
        Ok(())
    }

    // Changed by D-F2 (DR-118): the observer receives the introduction record
    // and reuses the reducer's measurement when the record has one.
    // pub fn observe_rho_produce(
    //     &mut self,
    //     source: &Produce,
    //     channel: &Par,
    //     data: &ListParWithRandom,
    //     introduction_authority: &CostAuthority,
    // ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
    //     self.authenticate_introduction_source(RSpaceOperationSource::Produce(source))?;
    //     let meter = |operations, scanned, backing| {
    //         reserve_source(&self.replay.inner.host, operations, scanned, backing)
    //     };
    //     let observed = observation_construction::produce_introduction_metered(
    //         source,
    //         channel,
    //         data,
    //         introduction_authority,
    //         &meter,
    //     )
    //     .map_err(construction_error)?
    //     .into_native_metered(&meter)
    //     .map_err(construction_error)?;
    //     self.observe_introduction(&observed)
    // }
    pub fn observe_rho_produce(
        &mut self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        introduction: &IntroductionRecord,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_introduction_source(RSpaceOperationSource::Produce(source))?;
        let observed = observation_construction::produce_introduction_recorded(
            source,
            channel,
            data,
            introduction,
        )
        .map_err(construction_error)?
        .into_native()
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
        // Changed by D-F2 (DR-118): the observer receives the introduction
        // record and reuses the reducer's measurement when it has one.
        // introduction_authority: &CostAuthority,
        introduction: &IntroductionRecord,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_introduction_source(RSpaceOperationSource::Consume(source))?;
        let expected = self
            .operation()
            .consume_peeks
            .as_ref()
            .ok_or(NativeReplayError::Source)?;
        if peeks.len() != expected.len() || !peeks.iter().eq(expected.iter()) {
            return Err(NativeReplayError::Source);
        }
        // Changed by D-F2 (DR-118): the recorded form reuses the measurement.
        // let observed = observation_construction::consume_introduction_metered(
        //     source,
        //     channels,
        //     patterns,
        //     continuation,
        //     introduction_authority,
        //     &meter,
        // )
        let observed = observation_construction::consume_introduction_recorded(
            source,
            channels,
            patterns,
            continuation,
            introduction,
        )
        .map_err(construction_error)?
        .into_native()
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
        // DR-101: the bound deployment resolves residue seals as play did. An
        // unbound replay resolves none, so a play that resolved one fails closed.
        let residue = match self.replay.inner.authority.as_ref() {
            Some(binding) => binding
                .residue_context()
                .map_err(NativeReplayError::Interpreter)?,
            None => crate::rust::interpreter::accounting::authority::ResidueContext::system(),
        };
        let observed = observation_construction::comm(
            source,
            continuation,
            continuation_persistent,
            data,
            &residue,
        )
        .map_err(construction_error)?
        .into_native()
        .map_err(construction_error)?;
        let link = self
            .operation()
            .comm
            .as_ref()
            .ok_or(NativeReplayError::Stage)?
            .observation;
        // Changed by G3 (DR-135, S6): a granted COMM attempt keeps its
        // transferee funded (A5). A retry links an earlier attempt, which
        // play checked when it granted it.
        // self.observe(link, &observed, ObservationStage::Complete)
        let decision = self.observe(link, &observed, ObservationStage::Complete)?;
        if let (
            NativeBudgetReplayDecision::Accepted { .. },
            NativeObservationLink::Attempt(index),
        ) = (&decision, link)
        {
            self.check_transferee(index, continuation, data)?;
        }
        Ok(decision)
    }
}
