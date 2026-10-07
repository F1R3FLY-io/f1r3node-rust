use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::replay_rspace::native_session::space::NativeSpaceHost;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::space_store_handle::SpaceStoreHandle;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::NativeRuntimeReplaySession;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::deterministic_reduction::scheduled_execution;
use crate::rust::interpreter::execution_space::{
    ConsumeResult, ExecutionBackend, ExecutionSpace, ProduceResult,
};
use crate::rust::interpreter::spaces::agent::ParSpaceStore;

type Session = NativeRuntimeReplaySession<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

struct NativeExecution {
    session: Arc<Session>,
    budget: RuntimeBudget,
}

impl Session {
    pub(crate) fn execution_space(self: &Arc<Self>, budget: RuntimeBudget) -> ExecutionSpace {
        scheduled_execution(ExecutionSpace::new(NativeExecution {
            session: self.clone(),
            budget,
        }))
    }

    /// The space store of this session for the Reified RSpaces runtime of native replay
    /// (decision 47). A space operation gets its introduction authority from `budget`, as
    /// `NativeExecution` gives one to a host operation.
    pub(crate) fn space_store(
        self: &Arc<Self>,
        budget: RuntimeBudget,
    ) -> Result<ParSpaceStore, RSpaceError> {
        let host = NativeSpaceHost::new(
            Arc::clone(self),
            |session: &Self| &session.inner,
            Arc::new(move |source: RSpaceOperationSource<'_>| {
                let (identity, kind) = match source {
                    RSpaceOperationSource::Produce(produce) => (
                        produce_introduction_identity(produce),
                        AuthorityByteEventKind::ProduceIntroduction,
                    ),
                    RSpaceOperationSource::Consume(consume) => (
                        consume_introduction_identity(consume),
                        AuthorityByteEventKind::ConsumeIntroduction,
                    ),
                };
                budget
                    .introduction_authority(identity, kind)
                    .map_err(Into::into)
            }),
        )?;
        Ok(SpaceStoreHandle::new(host))
    }
}

#[async_trait]
impl ExecutionBackend for NativeExecution {
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
