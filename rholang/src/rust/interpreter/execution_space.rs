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
        self.0
            .consume(channels, patterns, continuation, persistent, peeks)
            .await
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        self.0.produce(channel, data, persistent).await
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
