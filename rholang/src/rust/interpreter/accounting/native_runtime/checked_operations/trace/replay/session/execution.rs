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
                |source| {
                    self.budget
                        .introduction_authority(
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
        self.session
            .inner
            .produce_with_authority(channel, data, persistent, |source| {
                self.budget
                    .introduction_authority(
                        produce_introduction_identity(source),
                        AuthorityByteEventKind::ProduceIntroduction,
                    )
                    .map_err(Into::into)
            })
            .await
    }

    async fn produce_guarded(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        guard: &dyn ProduceCommitGuard,
    ) -> Result<ProduceResult, RSpaceError> {
        self.session
            .inner
            .produce_with_authority_guarded(
                channel,
                data,
                persistent,
                |source| {
                    self.budget
                        .introduction_authority(
                            produce_introduction_identity(source),
                            AuthorityByteEventKind::ProduceIntroduction,
                        )
                        .map_err(Into::into)
                },
                guard,
            )
            .await
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

#[cfg(test)]
mod tests;
