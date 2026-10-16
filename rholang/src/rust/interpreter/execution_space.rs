use std::collections::BTreeSet;
use std::ops::Deref;
use std::sync::Arc;

use async_trait::async_trait;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::logging::{ReductionKind, StepGate};
use rspace_plus_plus::rspace::rspace_interface::{
    MaybeConsumeResult, MaybeProduceResult, ProduceCommitGuard,
};
use rspace_plus_plus::rspace::trace::event::Produce;

use super::accounting::RuntimeBudget;
use super::rho_runtime::RhoISpace;
use super::storage::charging_rspace::native_replay_meter;

pub type ConsumeResult =
    MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
pub type ProduceResult =
    MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    /// Authenticated event layout selected by the backing RSpace, never by
    /// process syntax or the presence of accounting fields in a payload.
    fn codec_epoch(&self) -> EventCodecEpoch;

    /// Live reduction observation is explicit for every execution backend.
    /// Native replay has a distinct event log and opts out deliberately.
    fn step_gate(&self) -> Option<Arc<StepGate>>;
    fn observe_reduction(&self, redex: &Par, kind: ReductionKind);

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

    async fn produce_guarded(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        guard: &dyn ProduceCommitGuard,
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

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Installs a host-owned replay lease beneath deterministic scheduling.
    /// The scheduler still chooses the operation order; only the terminal
    /// replay dispatch may consume this policy-derived finite lease.
    pub(crate) fn with_native_replay(space: RhoISpace, cost: RuntimeBudget) -> Self {
        Self::new(LegacyExecution {
            space,
            native_replay: Some(cost),
        })
    }
}

impl Deref for ExecutionSpace {
    type Target = dyn ExecutionBackend;

    fn deref(&self) -> &Self::Target { self.0.as_ref() }
}

impl From<RhoISpace> for ExecutionSpace {
    fn from(space: RhoISpace) -> Self {
        Self::new(LegacyExecution {
            space,
            native_replay: None,
        })
    }
}

struct LegacyExecution {
    space: RhoISpace,
    native_replay: Option<RuntimeBudget>,
}

impl LegacyExecution {
    async fn cost_v2_replay(&self) -> bool {
        self.native_replay
            .as_ref()
            .is_some_and(|cost| cost.native_event_codec_epoch() == Some(EventCodecEpoch::CostV2))
            && self.space.is_replay().await
    }
}

#[async_trait]
impl ExecutionBackend for LegacyExecution {
    fn codec_epoch(&self) -> EventCodecEpoch { self.space.codec_epoch() }

    fn step_gate(&self) -> Option<Arc<StepGate>> { self.space.step_gate() }

    fn observe_reduction(&self, redex: &Par, kind: ReductionKind) {
        self.space.observe_reduction(redex, kind);
    }

    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<ConsumeResult, RSpaceError> {
        if self.cost_v2_replay().await {
            let meter = native_replay_meter(self.native_replay.as_ref().expect("checked above"))?;
            return self
                .space
                .replay_consume_cost_v2_metered(
                    channels,
                    patterns,
                    continuation,
                    persistent,
                    peeks,
                    &meter,
                )
                .await;
        }
        self.space
            .consume(channels, patterns, continuation, persistent, peeks)
            .await
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<ProduceResult, RSpaceError> {
        if self.cost_v2_replay().await {
            let meter = native_replay_meter(self.native_replay.as_ref().expect("checked above"))?;
            return self
                .space
                .replay_produce_cost_v2_metered(channel, data, persistent, None, &meter)
                .await;
        }
        self.space.produce(channel, data, persistent).await
    }

    async fn produce_guarded(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        guard: &dyn ProduceCommitGuard,
    ) -> Result<ProduceResult, RSpaceError> {
        if self.cost_v2_replay().await {
            let meter = native_replay_meter(self.native_replay.as_ref().expect("checked above"))?;
            return self
                .space
                .replay_produce_cost_v2_metered(channel, data, persistent, Some(guard), &meter)
                .await;
        }
        self.space
            .produce_guarded(channel, data, persistent, guard)
            .await
    }

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        self.space.get_joins(channel).await
    }

    async fn is_replay(&self) -> bool { self.space.is_replay().await }

    async fn update_produce(&self, _: &Produce, updated: Produce) -> Result<(), RSpaceError> {
        self.space.update_produce(updated).await;
        Ok(())
    }
}
