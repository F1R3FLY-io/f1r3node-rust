use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
use rspace_plus_plus::rspace::reporting_rspace::ReportPhase;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceAccountingObserver,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::Log;
use tokio::sync::{oneshot, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio::task::JoinHandle;

use super::accounting::RuntimeBudget;
use super::rho_runtime::RhoISpace;

type ParticipantId = CausalPath;

tokio::task_local! {
    static REDUCTION_CONTEXT: ReductionContext;
    static INTERNAL_REDUCTION: ();
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
            participant: CausalPath::new(),
            next_step: Arc::new(AtomicU64::new(0)),
        }
    }

    fn next_operation(&self) -> OperationOrder {
        let step = self.next_step.fetch_add(1, Ordering::Relaxed);
        let mut path = self.participant.clone();
        path.push_back((step, 0));
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
        let mut split_prefix = self.participant.clone();
        split_prefix.push_back((step, 1));
        let children = (0..count)
            .map(|index| {
                let mut participant = split_prefix.clone();
                participant.push_back((index as u64, 0));
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

pub fn current() -> Option<ReductionContext> {
    if INTERNAL_REDUCTION.try_with(|_| ()).is_ok() {
        None
    } else {
        REDUCTION_CONTEXT.try_with(Clone::clone).ok()
    }
}

pub async fn scope<T>(context: ReductionContext, future: impl Future<Output = T>) -> T {
    REDUCTION_CONTEXT.scope(context, future).await
}

async fn internal_scope<T>(future: impl Future<Output = T>) -> T {
    INTERNAL_REDUCTION.scope((), future).await
}

pub(crate) struct ScopedJoinHandle<T> {
    inner: JoinHandle<T>,
}

impl<T> ScopedJoinHandle<T> {
    pub(crate) fn new(inner: JoinHandle<T>) -> Self { Self { inner } }
}

impl<T> Future for ScopedJoinHandle<T> {
    type Output = Result<T, tokio::task::JoinError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        std::pin::Pin::new(&mut self.inner).poll(context)
    }
}

impl<T> Drop for ScopedJoinHandle<T> {
    fn drop(&mut self) { self.inner.abort(); }
}

struct DirectExecutionGuard {
    session: Arc<ReductionSession>,
    participant: ParticipantId,
}

impl DirectExecutionGuard {
    fn new(session: Arc<ReductionSession>, participant: ParticipantId) -> Self {
        Self {
            session,
            participant,
        }
    }
}

impl Drop for DirectExecutionGuard {
    fn drop(&mut self) { self.session.finish_direct(&self.participant); }
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
    session.register(CausalPath::new());
    let guard = ParticipantGuard::new(session, CausalPath::new());
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

#[derive(Debug)]
enum ParticipantState {
    Running,
    ExecutingDirect,
    Waiting(OperationOrder),
}

#[derive(Debug, Eq, PartialEq)]
enum DriverPoll {
    CompletedInline,
    Transferred,
}

async fn poll_driver_once(future: impl Future<Output = ()> + Send + 'static) -> DriverPoll {
    let mut future = Some(Box::pin(future));
    std::future::poll_fn(|context| {
        let mut driver = future.take().expect("driver must be polled once");
        match driver.as_mut().poll(context) {
            Poll::Ready(()) => Poll::Ready(DriverPoll::CompletedInline),
            Poll::Pending => {
                tokio::spawn(driver);
                Poll::Ready(DriverPoll::Transferred)
            }
        }
    })
    .await
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

    fn claim_direct_state(state: &mut SessionState, participant: &ParticipantId) -> bool {
        if state.driving || !state.intents.is_empty() || state.participants.len() != 1 {
            return false;
        }
        let Some(participant_state) = state.participants.get_mut(participant) else {
            return false;
        };
        if !matches!(participant_state, ParticipantState::Running) {
            return false;
        }
        *participant_state = ParticipantState::ExecutingDirect;
        true
    }

    fn claim_direct(&self, participant: &ParticipantId) -> bool {
        Self::claim_direct_state(
            &mut self.state.lock().expect("reduction session lock"),
            participant,
        )
    }

    fn finish_direct(&self, participant: &ParticipantId) {
        let mut state = self.state.lock().expect("reduction session lock");
        if matches!(
            state.participants.get(participant),
            Some(ParticipantState::ExecutingDirect)
        ) {
            state
                .participants
                .insert(participant.clone(), ParticipantState::Running);
        }
    }

    fn release_frontier(state: &mut SessionState, orders: &[OperationOrder]) {
        assert!(
            state.intents.is_empty(),
            "driver received a new intent before it released the completed frontier"
        );
        for order in orders {
            for participant in state.participants.values_mut() {
                if matches!(participant, ParticipantState::Waiting(waiting) if waiting == order) {
                    *participant = ParticipantState::Running;
                }
            }
        }
        state.driving = false;
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
                Self::claim_driver(&mut state),
                state.participants.is_empty() && state.intents.is_empty() && !state.driving,
            )
        };
        if quiescent {
            self.release_evaluation_guard();
        }
        if start {
            self.spawn_driver();
        }
    }

    fn frontier_ready(state: &SessionState) -> bool {
        !state.intents.is_empty()
            && state
                .participants
                .values()
                .all(|participant| matches!(participant, ParticipantState::Waiting(_)))
    }

    fn claim_driver(state: &mut SessionState) -> bool {
        if state.driving || !Self::frontier_ready(state) {
            return false;
        }
        state.driving = true;
        true
    }

    fn spawn_driver(self: &Arc<Self>) {
        let session = self.clone();
        tokio::spawn(internal_scope(async move { session.drive().await }));
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
        if self.claim_direct(&context.participant) {
            let _direct = DirectExecutionGuard::new(self.clone(), context.participant.clone());
            return internal_scope(operation_context::scope(
                order,
                self.space.produce(channel, data, persistent),
            ))
            .await;
        }
        let (response, receive) = oneshot::channel();
        let drive = self.submit(&context.participant, order, Intent::Produce {
            channel,
            data,
            persistent,
            response,
        });
        if drive {
            let _ = poll_driver_once(internal_scope(self.clone().drive())).await;
        }
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
        if self.claim_direct(&context.participant) {
            let _direct = DirectExecutionGuard::new(self.clone(), context.participant.clone());
            return internal_scope(operation_context::scope(
                order,
                self.space
                    .consume(channels, patterns, continuation, persistent, peeks),
            ))
            .await;
        }
        let (response, receive) = oneshot::channel();
        let drive = self.submit(&context.participant, order, Intent::Consume {
            channels,
            patterns,
            continuation,
            persistent,
            peeks,
            response,
        });
        if drive {
            let _ = poll_driver_once(internal_scope(self.clone().drive())).await;
        }
        receive.await.map_err(|_| {
            RSpaceError::BugFoundError("deterministic consume was cancelled".to_string())
        })?
    }

    fn submit(
        self: &Arc<Self>,
        participant: &ParticipantId,
        order: OperationOrder,
        intent: Intent,
    ) -> bool {
        let mut state = self.state.lock().expect("reduction session lock");
        let participant_state = state
            .participants
            .get_mut(participant)
            .expect("reduction participant must be registered");
        assert!(
            matches!(participant_state, ParticipantState::Running),
            "participant {participant:?} submitted {order:?} while {participant_state:?}"
        );
        *participant_state = ParticipantState::Waiting(order.clone());
        assert!(state.intents.insert(order, intent).is_none());
        Self::claim_driver(&mut state)
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
            let orders = completions
                .iter()
                .map(|completion| completion.order().clone())
                .collect::<Vec<_>>();
            Self::release_frontier(&mut state, &orders);
        }
        for completion in completions {
            completion.send();
        }
        let quiescent = {
            let state = self.state.lock().expect("reduction session lock");
            state.participants.is_empty() && state.intents.is_empty() && !state.driving
        };
        if quiescent {
            self.release_evaluation_guard();
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
    use std::sync::atomic::AtomicBool;
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
                path: vec![(order, 0)].into(),
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
                    .map(|intent| intent.order.path.to_vec()[0].0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        components.sort();
        components
    }

    fn lifecycle_state(live: usize, waiting: usize) -> SessionState {
        let mut participants = BTreeMap::new();
        let mut intents = BTreeMap::new();
        for index in 0..live {
            let participant = CausalPath::from(vec![(index as u64, 0)]);
            if index < waiting {
                let order = OperationOrder {
                    session: [11; 32],
                    path: CausalPath::from(vec![(index as u64, 1)]),
                };
                participants.insert(participant, ParticipantState::Waiting(order.clone()));
                let (response, _receive) = oneshot::channel();
                intents.insert(order, Intent::Produce {
                    channel: Par::default(),
                    data: ListParWithRandom::default(),
                    persistent: false,
                    response,
                });
            } else {
                participants.insert(participant, ParticipantState::Running);
            }
        }
        SessionState {
            participants,
            intents,
            driving: false,
        }
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
    async fn ready_driver_completes_in_the_callers_poll() {
        let ran = Arc::new(AtomicBool::new(false));
        let ran_driver = ran.clone();
        let result = poll_driver_once(async move {
            ran_driver.store(true, Ordering::Relaxed);
        })
        .await;
        assert_eq!(result, DriverPoll::CompletedInline);
        assert!(ran.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn pending_driver_transfers_before_the_caller_can_yield() {
        let release = Arc::new(Notify::new());
        let finished = Arc::new(Notify::new());
        let result = {
            let release = release.clone();
            let finished = finished.clone();
            poll_driver_once(async move {
                release.notified().await;
                finished.notify_one();
            })
            .await
        };
        assert_eq!(result, DriverPoll::Transferred);
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), finished.notified())
            .await
            .expect("transferred driver did not complete");
    }

    #[tokio::test]
    async fn internal_driver_scope_cannot_submit_an_external_intent() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        root(
            reducer.space.clone(),
            RuntimeBudget::new(Cost::create(100, "internal driver scope")),
            ReductionCoordinator::default(),
            async {
                assert!(current().is_some());
                internal_scope(async { assert!(current().is_none()) }).await;
                assert!(current().is_some());
            },
        )
        .await;
    }

    #[tokio::test]
    async fn cancelled_root_aborts_children_before_checkpoint_boundary_opens() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let coordinator = ReductionCoordinator::default();
        let child_started = Arc::new(Notify::new());
        let release_child = Arc::new(Notify::new());
        let child_mutated = Arc::new(AtomicBool::new(false));
        let evaluation = {
            let coordinator = coordinator.clone();
            let space = reducer.space.clone();
            let child_started = child_started.clone();
            let release_child = release_child.clone();
            let child_mutated = child_mutated.clone();
            tokio::spawn(async move {
                root(
                    space,
                    RuntimeBudget::new(Cost::create(100, "cancelled reduction")),
                    coordinator,
                    async move {
                        let parent = current().expect("root reduction context");
                        let child = parent.split(1).pop().expect("child context");
                        let child_handle =
                            ScopedJoinHandle::new(tokio::spawn(scope(child.clone(), async move {
                                let _guard = ParticipantGuard::for_context(&child);
                                child_started.notify_one();
                                release_child.notified().await;
                                child_mutated.store(true, Ordering::Relaxed);
                            })));
                        std::future::pending::<()>().await;
                        drop(child_handle);
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
        tokio::time::timeout(Duration::from_secs(1), boundary)
            .await
            .expect("checkpoint boundary remained blocked")
            .expect("checkpoint boundary task failed");
        release_child.notify_one();
        tokio::task::yield_now().await;
        assert!(!child_mutated.load(Ordering::Relaxed));
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

        #[test]
        fn exactly_one_driver_claims_each_complete_frontier(
            live in 1_usize..16,
            waiting_seed in 0_usize..64,
        ) {
            let waiting = waiting_seed % (live + 1);
            let mut state = lifecycle_state(live, waiting);
            let expected = waiting == live;
            prop_assert_eq!(ReductionSession::claim_driver(&mut state), expected);
            prop_assert_eq!(state.driving, expected);
            prop_assert!(!ReductionSession::claim_driver(&mut state));
        }

        #[test]
        fn direct_execution_claims_only_a_single_running_participant(
            live in 1_usize..16,
            waiting_seed in 0_usize..64,
        ) {
            let waiting = waiting_seed % (live + 1);
            let mut state = lifecycle_state(live, waiting);
            let participant = CausalPath::from(vec![(0, 0)]);
            let expected = live == 1 && waiting == 0;
            prop_assert_eq!(
                ReductionSession::claim_direct_state(&mut state, &participant),
                expected,
            );
            prop_assert!(!ReductionSession::claim_direct_state(&mut state, &participant));
        }

        #[test]
        fn completed_frontier_releases_driver_before_participants_resume(
            live in 1_usize..16,
        ) {
            let mut state = lifecycle_state(live, live);
            let orders = state
                .participants
                .values()
                .filter_map(|participant| match participant {
                    ParticipantState::Waiting(order) => Some(order.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            state.intents.clear();
            state.driving = true;
            ReductionSession::release_frontier(&mut state, &orders);
            prop_assert!(!state.driving);
            prop_assert!(state
                .participants
                .values()
                .all(|participant| matches!(participant, ParticipantState::Running)));
            let first = CausalPath::from(vec![(0, 0)]);
            prop_assert_eq!(
                ReductionSession::claim_direct_state(&mut state, &first),
                live == 1,
            );
        }
    }
}
