use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
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
// Changed by DR-132 (P18): the deterministic scheduler is gone (D3-A).
// use crate::rust::interpreter::deterministic_reduction::scheduled_execution;
// Changed by G3 (DR-135, S6): the replay backend maps a skipped
// introduction as play does.
// use crate::rust::interpreter::execution_space::{
//     ConsumeResult, EvaluationProgress, ExecutionBackend, ExecutionSpace, ProduceResult,
// };
use crate::rust::interpreter::execution_space::{
    refused_stores_nothing, ConsumeResult, EvaluationProgress, ExecutionBackend, ExecutionSpace,
    ProduceResult,
};
use crate::rust::interpreter::spaces::agent::ParSpaceStore;

// Added by decision 90 (DR-132): the readiness wait of the space transactions of Reified
// RSpaces. A descendant of `trace::replay` reads the journal and the counted park.
// Changed by decision 95: the session epoch calls the check and the wait
// (`operations.rs`).
mod space_readiness;

type Session = NativeRuntimeReplaySession<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

struct NativeExecution {
    session: Arc<Session>,
    budget: RuntimeBudget,
}

impl Session {
    pub(crate) fn execution_space(self: &Arc<Self>, budget: RuntimeBudget) -> ExecutionSpace {
        // Changed by DR-132 (P18): the native execution space runs without
        // the scheduler wrapper.
        // scheduled_execution(ExecutionSpace::new(NativeExecution {
        //     session: self.clone(),
        //     budget,
        // }))
        ExecutionSpace::new(NativeExecution {
            session: self.clone(),
            budget,
        })
    }

    /// The space store of this session for the Reified RSpaces runtime of native replay
    /// (decision 47). A space operation gets its introduction record from `budget`, as
    /// `NativeExecution` gives one to a host operation (DR-118). `installed` holds the keys of
    /// the channels that the session installs for its system processes (R2).
    pub(crate) fn space_store(
        self: &Arc<Self>,
        budget: RuntimeBudget,
        installed: BTreeSet<Blake2b256Hash>,
    ) -> ParSpaceStore {
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
                // Changed by decision 89 (DR-118): the resolved introduction is the
                // record, as for a host operation.
                // budget
                //     .introduction_authority(identity, kind)
                //     .map_err(Into::into)
                budget.introduction(identity, kind).map_err(Into::into)
            }),
            installed,
            // Added by decision 90 (DR-132): the readiness wait of the transactions over the
            // journal of this session.
            // Disabled by decision 95: the session epoch gives the readiness check and the
            // wait (`space_transaction_ready` and `wait_space_change` in `operations.rs`).
            // Arc::new(space_readiness::SpaceReadiness::new(self.replay.clone())),
        );
        SpaceStoreHandle::new(host)
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
        // Changed by G3 (DR-135, S6): a skipped introduction stores nothing,
        // and the run continues, as in play (R8). The result is mapped below.
        // self.session
        //     .inner
        //     .consume_with_authority(
        let result = self
            .session
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
            // Changed by G3 (DR-135, S6): the result is mapped below.
            // .await
            .await;
        // Added by G3 (DR-135, S6).
        refused_stores_nothing(result)
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
            // Changed by G3 (DR-135, S6): a skipped introduction stores
            // nothing, and the run continues, as in play (R8).
            // .await?;
            .await;
        let result = refused_stores_nothing(result)?;
        // Added by DR-114 (E2): a returned produce may carry a record only when
        // it fires a non-persistent call of a recorded process. Otherwise a
        // block could hand a forged record to the user's code.
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
        Ok(result)
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        self.session.get_joins(&channel).await
    }

    async fn is_replay(&self) -> bool { true }

    /// Added by DR-132 (D2-B): native replay counts the tasks of each
    /// evaluation for its stall rule.
    fn evaluation_progress(&self) -> Option<Arc<dyn EvaluationProgress>> {
        Some(self.session.evaluation_progress())
    }

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
