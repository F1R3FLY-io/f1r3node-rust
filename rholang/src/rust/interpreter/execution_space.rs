use std::collections::BTreeSet;
use std::ops::Deref;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::rspace_interface::{MaybeConsumeResult, MaybeProduceResult};
use rspace_plus_plus::rspace::trace::event::Produce;

use super::rho_runtime::RhoISpace;

pub type ConsumeResult =
    MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
pub type ProduceResult =
    MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError>;

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError>;

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError>;
    async fn is_replay(&self) -> bool;
    async fn update_produce(&self, original: &Produce, updated: Produce)
        -> Result<(), RSpaceError>;

    /// Added by DR-132 (D2-B): the evaluation progress of native replay. No
    /// other backend counts the tasks of an evaluation.
    fn evaluation_progress(&self) -> Option<Arc<dyn EvaluationProgress>> { None }
}

/// Added by DR-132 (D2-B): the live tasks of one evaluation, which native
/// replay counts for its stall rule (`trace/replay/progress.rs`).
pub trait EvaluationProgress: Send + Sync {
    /// Starts an evaluation with the root as its one live task.
    fn begin(&self) -> Result<(), RSpaceError>;
    /// Counts a task before its spawn.
    fn enter(&self);
    /// A counted task ended. When every remaining live task waits on a
    /// recorded operation, replay declares the stall.
    fn finish(&self);
    /// The evaluation is cancelled, so no later finish declares a stall.
    fn close(&self);
    /// The identity of the replay. A waiter compares it with the progress
    /// of its task's evaluation.
    fn identity(&self) -> usize;
}

#[derive(Clone)]
pub struct ExecutionSpace(Arc<dyn ExecutionBackend>);

impl ExecutionSpace {
    pub fn new(backend: impl ExecutionBackend + 'static) -> Self { Self(Arc::new(backend)) }
}

impl Deref for ExecutionSpace {
    type Target = dyn ExecutionBackend;

    fn deref(&self) -> &Self::Target { self.0.as_ref() }
}

impl From<RhoISpace> for ExecutionSpace {
    fn from(space: RhoISpace) -> Self { Self::new(LegacyExecution(space)) }
}

struct LegacyExecution(RhoISpace);

/// Added by G3 (DR-135, S4): RSpace refuses an introduction whose owner
/// cannot pay with `RSpaceError::IntroductionRefused`, after it recorded the
/// refusal and stored nothing (R8). The reducer sees an operation that
/// stored nothing. Only a run with owner funding refuses an introduction.
// Changed by G3 (DR-135, S6): the replay backend maps a skipped
// introduction with this function too.
// fn refused_stores_nothing<T>(
pub(crate) fn refused_stores_nothing<T>(
    result: Result<Option<T>, RSpaceError>,
) -> Result<Option<T>, RSpaceError> {
    match result {
        Err(RSpaceError::IntroductionRefused) => Ok(None),
        other => other,
    }
}

#[async_trait]
impl ExecutionBackend for LegacyExecution {
    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError> {
        // Changed by G3 (DR-135, S4): an introduction that its owner cannot
        // pay stores nothing, and the run continues (R8).
        // self.0
        //     .consume(channels, patterns, continuation, persistent, peeks)
        //     .await
        refused_stores_nothing(
            self.0
                .consume(channels, patterns, continuation, persistent, peeks)
                .await,
        )
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        // Changed by G3 (DR-135, S4): an introduction that its owner cannot
        // pay stores nothing, and the run continues (R8).
        // self.0.produce(channel, data, persistent).await
        refused_stores_nothing(self.0.produce(channel, data, persistent).await)
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        Ok(self.0.get_joins(channel).await)
    }

    async fn is_replay(&self) -> bool { self.0.is_replay().await }

    async fn update_produce(&self, _: &Produce, updated: Produce) -> Result<(), RSpaceError> {
        self.0.update_produce(updated).await;
        Ok(())
    }
}
