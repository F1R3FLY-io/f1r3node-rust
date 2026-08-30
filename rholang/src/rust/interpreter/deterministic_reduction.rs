use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::reporting_rspace::ReportPhase;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceAccountingObserver,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::Log;
use tokio::sync::{oneshot, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use super::accounting::RuntimeBudget;
use super::rho_runtime::RhoISpace;

type ParticipantId = Vec<(u64, u64)>;

tokio::task_local! {
    static REDUCTION_CONTEXT: ReductionContext;
}

#[derive(Clone, Default)]
pub struct ReductionCoordinator {
    boundary: Arc<RwLock<()>>,
}

impl ReductionCoordinator {
    async fn enter_evaluation(&self) -> OwnedRwLockReadGuard<()> {
        self.boundary.clone().read_owned().await
    }

    async fn enter_boundary(&self) -> OwnedRwLockWriteGuard<()> {
        self.boundary.clone().write_owned().await
    }
}

#[derive(Clone)]
pub struct ReductionContext {
    session: Arc<ReductionSession>,
    session_id: [u8; 32],
    participant: ParticipantId,
    next_step: Arc<AtomicU64>,
}

impl ReductionContext {
    fn root(session: Arc<ReductionSession>, session_id: [u8; 32]) -> Self {
        Self {
            session,
            session_id,
            participant: Vec::new(),
            next_step: Arc::new(AtomicU64::new(0)),
        }
    }

    fn next_operation(&self) -> OperationOrder {
        let step = self.next_step.fetch_add(1, Ordering::Relaxed);
        let mut path = self.participant.clone();
        path.push((step, 0));
        OperationOrder {
            session: self.session_id,
            path,
        }
    }

    pub fn split(&self, count: usize) -> Vec<Self> {
        if count == 0 {
            return Vec::new();
        }
        let step = self.next_step.fetch_add(1, Ordering::Relaxed);
        let children = (0..count)
            .map(|index| {
                let mut participant = self.participant.clone();
                participant.push((step, 1));
                participant.push((index as u64, 0));
                Self {
                    session: self.session.clone(),
                    session_id: self.session_id,
                    participant,
                    next_step: Arc::new(AtomicU64::new(0)),
                }
            })
            .collect::<Vec<_>>();
        self.session.split(
            &self.participant,
            children
                .iter()
                .map(|child| child.participant.clone())
                .collect(),
        );
        children
    }

    pub fn rejoin(&self) { self.session.rejoin(self.participant.clone()); }
}

pub fn current() -> Option<ReductionContext> { REDUCTION_CONTEXT.try_with(Clone::clone).ok() }

pub async fn scope<T>(context: ReductionContext, future: impl Future<Output = T>) -> T {
    REDUCTION_CONTEXT.scope(context, future).await
}

pub async fn root<T>(
    space: RhoISpace,
    budget: RuntimeBudget,
    coordinator: ReductionCoordinator,
    future: impl Future<Output = T>,
) -> T {
    if current().is_some() {
        return future.await;
    }
    let session_id = budget.deploy_id();
    let evaluation_guard = coordinator.enter_evaluation().await;
    let session = Arc::new(ReductionSession::new(space, budget, evaluation_guard));
    let context = ReductionContext::root(session.clone(), session_id);
    session.register(Vec::new());
    let guard = ParticipantGuard::new(session, Vec::new());
    let result = scope(context, future).await;
    drop(guard);
    result
}

pub(crate) struct ParticipantGuard {
    session: Arc<ReductionSession>,
    participant: ParticipantId,
}

impl ParticipantGuard {
    fn new(session: Arc<ReductionSession>, participant: ParticipantId) -> Self {
        Self {
            session,
            participant,
        }
    }

    pub(crate) fn for_context(context: &ReductionContext) -> Self {
        Self::new(context.session.clone(), context.participant.clone())
    }
}

impl Drop for ParticipantGuard {
    fn drop(&mut self) { self.session.complete(&self.participant); }
}

enum ParticipantState {
    Running,
    Waiting(OperationOrder),
}

struct SessionState {
    participants: BTreeMap<ParticipantId, ParticipantState>,
    intents: BTreeMap<OperationOrder, Intent>,
    driving: bool,
}

struct ReductionSession {
    space: RhoISpace,
    budget: RuntimeBudget,
    state: Mutex<SessionState>,
    evaluation_guard: Mutex<Option<OwnedRwLockReadGuard<()>>>,
}

enum Intent {
    Produce {
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
        response: oneshot::Sender<
            Result<
                MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            >,
        >,
    },
    Consume {
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
        response: oneshot::Sender<
            Result<
                MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            >,
        >,
    },
}

enum Completion {
    Produce {
        order: OperationOrder,
        response: oneshot::Sender<
            Result<
                MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            >,
        >,
        result: Result<
            MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
            RSpaceError,
        >,
    },
    Consume {
        order: OperationOrder,
        response: oneshot::Sender<
            Result<
                MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
                RSpaceError,
            >,
        >,
        result: Result<
            MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
            RSpaceError,
        >,
    },
}

impl Completion {
    fn order(&self) -> &OperationOrder {
        match self {
            Self::Produce { order, .. } | Self::Consume { order, .. } => order,
        }
    }

    fn send(self) {
        match self {
            Self::Produce {
                response, result, ..
            } => {
                let _ = response.send(result);
            }
            Self::Consume {
                response, result, ..
            } => {
                let _ = response.send(result);
            }
        }
    }
}

struct PreparedIntent {
    order: OperationOrder,
    footprint: BTreeSet<Vec<u8>>,
    intent: Intent,
}

impl ReductionSession {
    fn new(
        space: RhoISpace,
        budget: RuntimeBudget,
        evaluation_guard: OwnedRwLockReadGuard<()>,
    ) -> Self {
        Self {
            space,
            budget,
            state: Mutex::new(SessionState {
                participants: BTreeMap::new(),
                intents: BTreeMap::new(),
                driving: false,
            }),
            evaluation_guard: Mutex::new(Some(evaluation_guard)),
        }
    }

    fn release_evaluation_guard(&self) {
        self.evaluation_guard
            .lock()
            .expect("reduction evaluation guard lock")
            .take();
    }

    fn register(&self, participant: ParticipantId) {
        self.state
            .lock()
            .expect("reduction session lock")
            .participants
            .insert(participant, ParticipantState::Running);
    }

    fn split(&self, parent: &ParticipantId, children: Vec<ParticipantId>) {
        let mut state = self.state.lock().expect("reduction session lock");
        state.participants.remove(parent);
        for child in children {
            state.participants.insert(child, ParticipantState::Running);
        }
    }

    fn rejoin(&self, parent: ParticipantId) {
        self.state
            .lock()
            .expect("reduction session lock")
            .participants
            .insert(parent, ParticipantState::Running);
    }

    fn complete(self: &Arc<Self>, participant: &ParticipantId) {
        let (start, quiescent) = {
            let mut state = self.state.lock().expect("reduction session lock");
            if let Some(ParticipantState::Waiting(order)) = state.participants.remove(participant) {
                state.intents.remove(&order);
            }
            (
                Self::ready_to_drive(&state),
                state.participants.is_empty() && state.intents.is_empty() && !state.driving,
            )
        };
        if quiescent {
            self.release_evaluation_guard();
        }
        if start {
            self.start_driver();
        }
    }

    fn ready_to_drive(state: &SessionState) -> bool {
        !state.driving
            && !state.intents.is_empty()
            && state
                .participants
                .values()
                .all(|participant| matches!(participant, ParticipantState::Waiting(_)))
    }

    fn start_driver(self: &Arc<Self>) {
        let should_start = {
            let mut state = self.state.lock().expect("reduction session lock");
            if !Self::ready_to_drive(&state) {
                false
            } else {
                state.driving = true;
                true
            }
        };
        if should_start {
            let session = self.clone();
            tokio::spawn(async move { session.drive().await });
        }
    }

    async fn submit_produce(
        self: &Arc<Self>,
        context: &ReductionContext,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<
        MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        let order = context.next_operation();
        let (response, receive) = oneshot::channel();
        self.submit(&context.participant, order, Intent::Produce {
            channel,
            data,
            persistent,
            response,
        });
        receive.await.map_err(|_| {
            RSpaceError::BugFoundError("deterministic produce was cancelled".to_string())
        })?
    }

    async fn submit_consume(
        self: &Arc<Self>,
        context: &ReductionContext,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<
        MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        let order = context.next_operation();
        let (response, receive) = oneshot::channel();
        self.submit(&context.participant, order, Intent::Consume {
            channels,
            patterns,
            continuation,
            persistent,
            peeks,
            response,
        });
        receive.await.map_err(|_| {
            RSpaceError::BugFoundError("deterministic consume was cancelled".to_string())
        })?
    }

    fn submit(
        self: &Arc<Self>,
        participant: &ParticipantId,
        order: OperationOrder,
        intent: Intent,
    ) {
        {
            let mut state = self.state.lock().expect("reduction session lock");
            let participant_state = state
                .participants
                .get_mut(participant)
                .expect("reduction participant must be registered");
            assert!(matches!(participant_state, ParticipantState::Running));
            *participant_state = ParticipantState::Waiting(order.clone());
            assert!(state.intents.insert(order, intent).is_none());
        }
        self.start_driver();
    }

    async fn prepare(&self, order: OperationOrder, intent: Intent) -> PreparedIntent {
        let mut footprint = BTreeSet::new();
        match &intent {
            Intent::Produce { channel, data, .. } => {
                insert_channel(&mut footprint, channel);
                for join in self.space.get_joins(channel.clone()).await {
                    for joined_channel in join {
                        insert_channel(&mut footprint, &joined_channel);
                    }
                }
                insert_authority(&mut footprint, data.cost_authority.as_ref(), &self.budget);
            }
            Intent::Consume {
                channels,
                continuation,
                ..
            } => {
                for channel in channels {
                    insert_channel(&mut footprint, channel);
                }
                insert_authority(
                    &mut footprint,
                    continuation.cost_authority.as_ref(),
                    &self.budget,
                );
            }
        }
        PreparedIntent {
            order,
            footprint,
            intent,
        }
    }

    async fn execute(&self, prepared: PreparedIntent) -> Completion {
        let PreparedIntent { order, intent, .. } = prepared;
        match intent {
            Intent::Produce {
                channel,
                data,
                persistent,
                response,
            } => {
                let result = operation_context::scope(
                    order.clone(),
                    self.space.produce(channel, data, persistent),
                )
                .await;
                Completion::Produce {
                    order,
                    response,
                    result,
                }
            }
            Intent::Consume {
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
                response,
            } => {
                let result = operation_context::scope(
                    order.clone(),
                    self.space
                        .consume(channels, patterns, continuation, persistent, peeks),
                )
                .await;
                Completion::Consume {
                    order,
                    response,
                    result,
                }
            }
        }
    }

    async fn drive(self: Arc<Self>) {
        let intents = {
            let mut state = self.state.lock().expect("reduction session lock");
            std::mem::take(&mut state.intents)
        };
        let mut prepared = Vec::with_capacity(intents.len());
        for (order, intent) in intents {
            prepared.push(self.prepare(order, intent).await);
        }
        let components = conflict_components(prepared);
        let mut component_futures = FuturesUnordered::new();
        for component in components {
            let session = self.clone();
            component_futures.push(async move {
                let mut completed = Vec::with_capacity(component.len());
                for intent in component {
                    completed.push(session.execute(intent).await);
                }
                completed
            });
        }
        let mut completions = Vec::new();
        while let Some(mut component) = component_futures.next().await {
            completions.append(&mut component);
        }
        completions.sort_by(|left, right| left.order().cmp(right.order()));
        {
            let mut state = self.state.lock().expect("reduction session lock");
            for completion in &completions {
                for participant in state.participants.values_mut() {
                    if matches!(participant, ParticipantState::Waiting(order) if order == completion.order())
                    {
                        *participant = ParticipantState::Running;
                    }
                }
            }
        }
        for completion in completions {
            completion.send();
        }
        let (restart, quiescent) = {
            let mut state = self.state.lock().expect("reduction session lock");
            state.driving = false;
            (
                Self::ready_to_drive(&state),
                state.participants.is_empty() && state.intents.is_empty(),
            )
        };
        if quiescent {
            self.release_evaluation_guard();
        }
        if restart {
            self.start_driver();
        }
    }
}

fn insert_channel(footprint: &mut BTreeSet<Vec<u8>>, channel: &Par) {
    let mut key = vec![0];
    key.extend(channel.encode_to_vec());
    footprint.insert(key);
}

fn insert_authority(
    footprint: &mut BTreeSet<Vec<u8>>,
    authority: Option<&models::rhoapi::CostAuthority>,
    budget: &RuntimeBudget,
) {
    if !budget.has_comm_accounting_scope() || budget.is_unmetered() {
        return;
    }
    match authority {
        Some(authority) if !authority.regions.is_empty() => {
            for region in &authority.regions {
                let mut key = vec![1];
                key.extend(&region.instance_id);
                footprint.insert(key);
            }
        }
        _ => {
            footprint.insert(vec![1, 0]);
        }
    }
}

fn conflict_components(mut intents: Vec<PreparedIntent>) -> Vec<Vec<PreparedIntent>> {
    intents.sort_by(|left, right| left.order.cmp(&right.order));
    let mut components: Vec<(BTreeSet<Vec<u8>>, Vec<PreparedIntent>)> = Vec::new();
    for intent in intents {
        let mut overlapping = Vec::new();
        for (index, (footprint, _)) in components.iter().enumerate() {
            if !footprint.is_disjoint(&intent.footprint) {
                overlapping.push(index);
            }
        }
        if overlapping.is_empty() {
            components.push((intent.footprint.clone(), vec![intent]));
            continue;
        }
        let first = overlapping[0];
        components[first].0.extend(intent.footprint.iter().cloned());
        components[first].1.push(intent);
        for index in overlapping.into_iter().skip(1).rev() {
            let (footprint, mut merged) = components.remove(index);
            components[first].0.extend(footprint);
            components[first].1.append(&mut merged);
        }
    }
    components
        .into_iter()
        .map(|(_, mut intents)| {
            intents.sort_by(|left, right| left.order.cmp(&right.order));
            intents
        })
        .collect()
}

#[derive(Clone)]
pub struct DeterministicRSpace {
    inner: RhoISpace,
    coordinator: ReductionCoordinator,
}

impl DeterministicRSpace {
    pub fn new(inner: RhoISpace, coordinator: ReductionCoordinator) -> Self {
        Self { inner, coordinator }
    }
}

#[async_trait]
impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> for DeterministicRSpace {
    fn set_accounting_observer(
        &self,
        observer: Option<
            Arc<
                dyn RSpaceAccountingObserver<
                    Par,
                    BindPattern,
                    ListParWithRandom,
                    TaggedContinuation,
                >,
            >,
        >,
    ) {
        self.inner.set_accounting_observer(observer);
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.create_checkpoint().await
    }

    async fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
        self.inner.get_data(channel).await
    }

    async fn get_waiting_continuations(
        &self,
        channels: Vec<Par>,
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
        self.inner.get_waiting_continuations(channels).await
    }

    async fn get_joins(&self, channel: Par) -> Vec<Vec<Par>> { self.inner.get_joins(channel).await }

    async fn remove_all_data(&self, channel: &Par) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.remove_all_data(channel).await
    }

    async fn remove_data_at(&self, channel: &Par, index: i32) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.remove_data_at(channel, index).await
    }

    async fn remove_data_at_recorded(
        &self,
        channel: &Par,
        index: i32,
        operation_id: &[u8],
    ) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner
            .remove_data_at_recorded(channel, index, operation_id)
            .await
    }

    async fn remove_all_continuations(&self, channels: Vec<Par>) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.remove_all_continuations(channels).await
    }

    async fn clear(&self) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.clear().await
    }

    async fn get_root(&self) -> Blake2b256Hash { self.inner.get_root().await }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.reset(root).await
    }

    async fn consume_result(
        &self,
        channel: Vec<Par>,
        pattern: Vec<BindPattern>,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.consume_result(channel, pattern).await
    }

    async fn to_map(
        &self,
    ) -> HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>> {
        self.inner.to_map().await
    }

    async fn create_soft_checkpoint(
        &self,
    ) -> SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.create_soft_checkpoint().await
    }

    async fn take_event_log(&self) -> Log {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.take_event_log().await
    }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.revert_to_soft_checkpoint(checkpoint).await
    }

    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<
        MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        match current() {
            Some(context) => {
                context
                    .session
                    .submit_consume(
                        &context,
                        channels,
                        patterns,
                        continuation,
                        persistent,
                        peeks,
                    )
                    .await
            }
            None => {
                self.inner
                    .consume(channels, patterns, continuation, persistent, peeks)
                    .await
            }
        }
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persistent: bool,
    ) -> Result<
        MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        match current() {
            Some(context) => {
                context
                    .session
                    .submit_produce(&context, channel, data, persistent)
                    .await
            }
            None => self.inner.produce(channel, data, persistent).await,
        }
    }

    async fn install(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.install(channels, patterns, continuation).await
    }

    async fn rig_and_reset(&self, start_root: Blake2b256Hash, log: Log) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.rig_and_reset(start_root, log).await
    }

    async fn rig(&self, log: Log) -> Result<(), RSpaceError> {
        let _boundary = self.coordinator.enter_boundary().await;
        self.inner.rig(log).await
    }

    async fn check_replay_data(&self) -> Result<(), RSpaceError> {
        self.inner.check_replay_data().await
    }

    async fn is_replay(&self) -> bool { self.inner.is_replay().await }

    async fn update_produce(&self, produce: Produce) { self.inner.update_produce(produce).await }

    async fn set_report_phase(&self, phase: ReportPhase) {
        self.inner.set_report_phase(phase).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use models::rhoapi::{CostAuthority, CostRegion};
    use proptest::prelude::*;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use tokio::sync::Notify;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::test_utils::persistent_store_tester::create_test_space;

    fn prepared(order: u64, keys: &[u8]) -> PreparedIntent {
        let (response, _receive) = oneshot::channel();
        PreparedIntent {
            order: OperationOrder {
                session: [7; 32],
                path: vec![(order, 0)],
            },
            footprint: keys.iter().map(|key| vec![*key]).collect(),
            intent: Intent::Produce {
                channel: Par::default(),
                data: ListParWithRandom::default(),
                persistent: false,
                response,
            },
        }
    }

    fn signature(intents: Vec<PreparedIntent>) -> Vec<Vec<u64>> {
        let mut components = conflict_components(intents)
            .into_iter()
            .map(|component| {
                component
                    .into_iter()
                    .map(|intent| intent.order.path[0].0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        components.sort();
        components
    }

    #[test]
    fn transitive_conflicts_form_one_component() {
        assert_eq!(
            signature(vec![
                prepared(0, &[1]),
                prepared(1, &[2]),
                prepared(2, &[1, 2]),
            ]),
            vec![vec![0, 1, 2]]
        );
    }

    #[test]
    fn disjoint_intents_remain_independent_components() {
        assert_eq!(
            signature(vec![
                prepared(0, &[1]),
                prepared(1, &[2]),
                prepared(2, &[3]),
            ]),
            vec![vec![0], vec![1], vec![2]]
        );
    }

    #[test]
    fn compound_authorities_conflict_on_every_shared_region() {
        let budget = RuntimeBudget::new(Cost::create(100, "shared authority region"));
        let _scope = budget.enter_comm_accounting_scope();
        let left = CostAuthority {
            regions: vec![
                CostRegion {
                    instance_id: vec![1; 32],
                    signature: None,
                },
                CostRegion {
                    instance_id: vec![2; 32],
                    signature: None,
                },
            ],
        };
        let right = CostAuthority {
            regions: vec![
                CostRegion {
                    instance_id: vec![2; 32],
                    signature: None,
                },
                CostRegion {
                    instance_id: vec![3; 32],
                    signature: None,
                },
            ],
        };
        let mut left_footprint = BTreeSet::new();
        let mut right_footprint = BTreeSet::new();
        insert_authority(&mut left_footprint, Some(&left), &budget);
        insert_authority(&mut right_footprint, Some(&right), &budget);
        assert!(!left_footprint.is_disjoint(&right_footprint));
    }

    #[tokio::test]
    async fn cancelled_root_keeps_checkpoint_boundary_closed_until_detached_children_finish() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let coordinator = ReductionCoordinator::default();
        let child_started = Arc::new(Notify::new());
        let release_child = Arc::new(Notify::new());
        let evaluation = {
            let coordinator = coordinator.clone();
            let space = reducer.space.clone();
            let child_started = child_started.clone();
            let release_child = release_child.clone();
            tokio::spawn(async move {
                root(
                    space,
                    RuntimeBudget::new(Cost::create(100, "cancelled reduction")),
                    coordinator,
                    async move {
                        let parent = current().expect("root reduction context");
                        let child = parent.split(1).pop().expect("child context");
                        tokio::spawn(scope(child.clone(), async move {
                            let _guard = ParticipantGuard::for_context(&child);
                            child_started.notify_one();
                            release_child.notified().await;
                        }));
                        std::future::pending::<()>().await;
                    },
                )
                .await;
            })
        };

        child_started.notified().await;
        evaluation.abort();
        let _ = evaluation.await;
        let boundary = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.enter_boundary().await })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!boundary.is_finished());
        release_child.notify_one();
        tokio::time::timeout(Duration::from_secs(1), boundary)
            .await
            .expect("checkpoint boundary remained blocked")
            .expect("checkpoint boundary task failed");
    }

    proptest! {
        #[test]
        fn component_partition_is_input_permutation_invariant(
            keys in prop::collection::vec(0_u8..6, 1..32)
        ) {
            let forward = keys
                .iter()
                .enumerate()
                .map(|(order, key)| prepared(order as u64, &[*key]))
                .collect::<Vec<_>>();
            let reverse = keys
                .iter()
                .enumerate()
                .rev()
                .map(|(order, key)| prepared(order as u64, &[*key]))
                .collect::<Vec<_>>();
            prop_assert_eq!(signature(forward), signature(reverse));
        }
    }
}
