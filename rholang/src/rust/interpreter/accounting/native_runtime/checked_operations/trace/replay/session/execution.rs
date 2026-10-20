use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::logging::{ReductionKind, StepGate};
use rspace_plus_plus::rspace::rspace_interface::ProduceCommitGuard;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::NativeRuntimeReplaySession;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::deterministic_reduction::{scheduled_execution, ReductionCoordinator};
use crate::rust::interpreter::execution_space::{
    ConsumeResult, ExecutionBackend, ExecutionSpace, ProduceResult,
};

type Session = NativeRuntimeReplaySession<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

struct NativeExecution {
    session: Arc<Session>,
    budget: RuntimeBudget,
}

impl Session {
    pub(crate) fn execution_space(
        self: &Arc<Self>,
        budget: RuntimeBudget,
        coordinator: ReductionCoordinator,
    ) -> ExecutionSpace {
        scheduled_execution(ExecutionSpace::new(NativeExecution {
            session: self.clone(),
            budget: budget.clone(),
        }), coordinator, budget)
    }
}

#[async_trait]
impl ExecutionBackend for NativeExecution {
    fn codec_epoch(&self) -> rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch {
        self.budget.native_event_codec_epoch().expect("native replay budget must bind its codec epoch")
    }

    fn step_gate(&self) -> Option<Arc<StepGate>> {
        // NativeReplaySession wraps ReplayRSpace, which has no live
        // StepCommObserver or step gate. Its deterministic COMM evidence is
        // recorded by the replay log and checked by the native trace instead.
        None
    }

    fn observe_reduction(&self, _redex: &Par, _kind: ReductionKind) {
        // ReplayRSpace intentionally has no live reduction observer. These
        // diagnostic events are not part of native consensus charge evidence.
    }

    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError> {
        self.session
            .inner
            .consume_with_authority(
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
                // Changed by D-F2 (DR-118): the resolved introduction is the
                // record, so the observer can reuse the reducer's measurement.
                // |source| {
                //     self.budget
                //         .introduction_authority(
                //             consume_introduction_identity(source),
                //             AuthorityByteEventKind::ConsumeIntroduction,
                //         )
                //         .map_err(Into::into)
                // },
                |source| {
                    self.budget
                        .introduction(
                            consume_introduction_identity(source),
                            AuthorityByteEventKind::ConsumeIntroduction,
                        )
                        .map_err(Into::into)
                },
            )
            .await
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        // Changed by DR-114: the result is checked before it is returned.
        let result = self
            .session
            .inner
            // Changed by D-F2 (DR-118): the resolved introduction is the record,
            // so the observer can reuse the reducer's measurement.
            // .produce_with_authority(channel, data, persistent, |source| {
            //     self.budget
            //         .introduction_authority(
            //             produce_introduction_identity(source),
            //             AuthorityByteEventKind::ProduceIntroduction,
            //         )
            //         .map_err(Into::into)
            // })
            .produce_with_authority(channel, data, persistent, |source| {
                self.budget
                    .introduction(
                        produce_introduction_identity(source),
                        AuthorityByteEventKind::ProduceIntroduction,
                    )
                    .map_err(Into::into)
            })
            .await?;
        // Changed by the cost-accounted-rho merge (R1-10): the E2 check of
        // DR-114 moved into `reject_forged_recorded_output`, which the guarded
        // produce calls too.
        reject_forged_recorded_output(&result, persistent)?;
        Ok(result)
    }

    async fn produce_guarded(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        guard: &dyn ProduceCommitGuard,
    ) -> Result<ProduceResult, RSpaceError> {
        // Changed by the cost-accounted-rho merge (R1-10): the guarded produce
        // resolves the whole introduction record, as the unguarded produce
        // does (D-F2, DR-118), and it checks its result for a forged record
        // (DR-114, E2).
        // self.session
        //     .inner
        //     .produce_with_authority_guarded(
        //         channel,
        //         data,
        //         persistent,
        //         |source| {
        //             self.budget
        //                 .introduction_authority(
        //                     produce_introduction_identity(source),
        //                     AuthorityByteEventKind::ProduceIntroduction,
        //                 )
        //                 .map_err(Into::into)
        //         },
        //         guard,
        //     )
        //     .await
        let result = self
            .session
            .inner
            .produce_with_authority_guarded(
                channel,
                data,
                persistent,
                |source| {
                    self.budget
                        .introduction(
                            produce_introduction_identity(source),
                            AuthorityByteEventKind::ProduceIntroduction,
                        )
                        .map_err(Into::into)
                },
                guard,
            )
            .await?;
        reject_forged_recorded_output(&result, persistent)?;
        Ok(result)
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        self.session.get_joins(&channel).await
    }

    async fn is_replay(&self) -> bool { true }

    async fn update_produce(
        &self,
        original: &Produce,
        updated: Produce,
    ) -> Result<(), RSpaceError> {
        self.session
            .inner
            .validate_produce_update(original, &updated)
            .await
    }
}

/// Added by DR-114 (E2): a returned produce may carry a record only when it
/// fires a non-persistent call of a recorded process. Otherwise a block could
/// hand a forged record to the user's code. Both produces of native replay
/// call this check (R1-10).
fn reject_forged_recorded_output(
    result: &ProduceResult,
    persistent: bool,
) -> Result<(), RSpaceError> {
    if let Some((continuation, _, produce)) = result.as_ref() {
        let recorded_call = !persistent
            && matches!(
                continuation.continuation.tagged_cont.as_ref(),
                Some(models::rhoapi::tagged_continuation::TaggedCont::ScalaBodyRef(body_ref))
                    if crate::rust::interpreter::system_processes::is_native_recorded_op(*body_ref)
            );
        if !produce.output_value.is_empty() && !recorded_call {
            return Err(RSpaceError::InterpreterError(
                "native replay found a recorded output on an unrecorded call".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
