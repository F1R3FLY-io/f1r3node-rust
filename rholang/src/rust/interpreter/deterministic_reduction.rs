use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use prost::Message;
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::operation_context::{self, CausalPath, OperationOrder};
use rspace_plus_plus::rspace::reporting_rspace::ReportPhase;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceAccountingObserver,
};
use rspace_plus_plus::rspace::trace::event::Produce;
use rspace_plus_plus::rspace::trace::Log;
use tokio::sync::{oneshot, Notify, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio::task::AbortHandle;
#[cfg(test)]
use tokio::task::JoinHandle;

use super::accounting::economic_failure::{
    classify_errors, EvaluationFailureSummary, FailureRecorder,
};
use super::accounting::phlo_execution::PhloFailure;
use super::accounting::RuntimeBudget;
use super::errors::InterpreterError;
use super::execution_space::{ExecutionBackend, ExecutionSpace};
use super::host_work::HostWorkBudget;
use super::rho_runtime::RhoISpace;

#[cfg(test)]
mod capability_tests;

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
        #[cfg(test)]
        if self.session.legacy_split {
            return self.split_legacy(count);
        }
        let step = self.next_step.fetch_add(1, Ordering::Relaxed);
        // Changed by C9 (DR-84): each child gets the one segment
        // (step, index + 1) instead of the pair (step, 1)·(index, 0).
        // Operations keep (step, 0), so the forward lexicographic order of
        // every generated path is unchanged and a split adds one segment.
        // let mut split_prefix = self.participant.clone();
        // split_prefix.push_back((step, 1));
        let children = (0..count)
            .map(|index| {
                // let mut participant = split_prefix.clone();
                // participant.push_back((index as u64, 0));
                let mut participant = self.participant.clone();
                let child = u64::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .expect("split child index fits in u64");
                participant.push_back((step, child));
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

    /// The legacy splitter, kept as the test oracle of C9 (DR-84): each child
    /// gets the pair (step, 1)·(index, 0).
    #[cfg(test)]
    fn split_legacy(&self, count: usize) -> Vec<Self> {
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

    pub fn host_work_budget(&self) -> Option<HostWorkBudget> { self.session.host_work.clone() }
}

pub fn current() -> Option<ReductionContext> {
    if INTERNAL_REDUCTION.try_with(|_| ()).is_ok() {
        None
    } else {
        REDUCTION_CONTEXT.try_with(Clone::clone).ok()
    }
}

pub(crate) fn record_evaluator_failures(errors: &[InterpreterError]) {
    if errors.is_empty() {
        return;
    }
    if let Some(context) = current() {
        let summary = classify_errors(errors, context.session.failure_work.as_ref())
            .unwrap_or_else(|_| EvaluationFailureSummary::single(PhloFailure::Platform));
        context.session.failures.record(summary);
    }
}

pub(crate) fn spawn_detached(
    future: impl Future<Output = Result<(), InterpreterError>> + Send + 'static,
) {
    let context = current().expect("detached reduction requires a reduction context");
    let child = context.split(1).pop().expect("one child context");
    spawn_detached_in_context(child, future);
}

pub(crate) fn spawn_detached_in_context(
    child: ReductionContext,
    future: impl Future<Output = Result<(), InterpreterError>> + Send + 'static,
) {
    let session = child.session.clone();
    let participant = child.participant.clone();
    let task_id = session.next_detached_id.fetch_add(1, Ordering::Relaxed);
    session.detached_count.fetch_add(1, Ordering::AcqRel);
    let (start, ready) = oneshot::channel();
    let task_session = session.clone();
    let mut detached_guard = DetachedGuard::new(task_session.clone(), task_id, participant.clone());
    let handle = tokio::spawn(scope(child.clone(), async move {
        if ready.await.is_ok() {
            if let Err(error) = future.await {
                record_evaluator_failures(std::slice::from_ref(&error));
                task_session
                    .detached_errors
                    .lock()
                    .expect("detached reduction errors lock")
                    .push((participant, error));
            }
        }
        detached_guard.finished = true;
        drop(detached_guard);
    }));
    let mut tasks = session
        .detached_tasks
        .lock()
        .expect("detached reduction tasks lock");
    if tasks.canceled {
        handle.abort();
    } else {
        tasks.handles.insert(task_id, handle.abort_handle());
    }
    drop(tasks);
    let _ = start.send(());
}

struct DetachedTasks {
    canceled: bool,
    handles: HashMap<u64, AbortHandle>,
}

struct DetachedGuard {
    session: Arc<ReductionSession>,
    task_id: u64,
    participant: ParticipantId,
    finished: bool,
}

impl DetachedGuard {
    fn new(session: Arc<ReductionSession>, task_id: u64, participant: ParticipantId) -> Self {
        Self {
            session,
            task_id,
            participant,
            finished: false,
        }
    }
}

impl Drop for DetachedGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.session
                .failures
                .record(EvaluationFailureSummary::single(PhloFailure::Platform));
            self.session
                .detached_errors
                .lock()
                .expect("detached reduction errors lock")
                .push((
                    self.participant.clone(),
                    InterpreterError::ReduceError("detached reduction task failed".to_string()),
                ));
        }
        self.session.complete(&self.participant);
        self.session
            .detached_tasks
            .lock()
            .expect("detached reduction tasks lock")
            .handles
            .remove(&self.task_id);
        if self.session.detached_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.session.detached_done.notify_waiters();
        }
    }
}

struct RootCancellationGuard {
    session: Arc<ReductionSession>,
    complete: bool,
}

impl Drop for RootCancellationGuard {
    fn drop(&mut self) {
        if !self.complete {
            let mut tasks = self
                .session
                .detached_tasks
                .lock()
                .expect("detached reduction tasks lock");
            tasks.canceled = true;
            for task in tasks.handles.values() {
                task.abort();
            }
        }
    }
}

pub fn reserve_host_work(
    dimension: HostWorkDimension,
    units: HostWorkUnits,
) -> Result<(), HostWorkReservationError> {
    let Some(context) = current() else {
        return Ok(());
    };
    let Some(budget) = context.host_work_budget() else {
        return Ok(());
    };
    budget.reserve(dimension, units).map(|_| ())
}

pub async fn scope<T>(context: ReductionContext, future: impl Future<Output = T>) -> T {
    REDUCTION_CONTEXT.scope(context, future).await
}

async fn internal_scope<T>(future: impl Future<Output = T>) -> T {
    INTERNAL_REDUCTION.scope((), future).await
}

#[cfg(test)]
pub(crate) struct ScopedJoinHandle<T> {
    inner: JoinHandle<T>,
}

#[cfg(test)]
impl<T> ScopedJoinHandle<T> {
    pub(crate) fn new(inner: JoinHandle<T>) -> Self { Self { inner } }
}

#[cfg(test)]
impl<T> Future for ScopedJoinHandle<T> {
    type Output = Result<T, tokio::task::JoinError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        std::pin::Pin::new(&mut self.inner).poll(context)
    }
}

#[cfg(test)]
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
    space: impl Into<ExecutionSpace>,
    budget: RuntimeBudget,
    coordinator: ReductionCoordinator,
    future: impl Future<Output = T>,
) -> T {
    root_with_host_work(space, budget, coordinator, None, future).await
}

pub async fn root_with_host_work<T>(
    space: impl Into<ExecutionSpace>,
    budget: RuntimeBudget,
    coordinator: ReductionCoordinator,
    host_work: Option<HostWorkBudget>,
    future: impl Future<Output = T>,
) -> T {
    if current().is_some() {
        return future.await;
    }
    root_with_observation(space, budget, coordinator, host_work, future)
        .await
        .0
}

pub(crate) async fn root_with_observation<T>(
    space: impl Into<ExecutionSpace>,
    budget: RuntimeBudget,
    coordinator: ReductionCoordinator,
    host_work: Option<HostWorkBudget>,
    future: impl Future<Output = T>,
) -> (T, EvaluationFailureSummary, Vec<InterpreterError>) {
    if let Some(context) = current() {
        let result = future.await;
        return (result, context.session.failures.snapshot(), Vec::new());
    }
    let session_id = budget.deploy_id();
    let evaluation_guard = coordinator.enter_evaluation().await;
    let session = Arc::new(ReductionSession::new(
        space.into(),
        budget,
        host_work,
        evaluation_guard,
    ));
    let mut cancellation = RootCancellationGuard {
        session: session.clone(),
        complete: false,
    };
    let context = ReductionContext::root(session.clone(), session_id);
    session.register(CausalPath::new());
    let guard = ParticipantGuard::new(session.clone(), CausalPath::new());
    let result = scope(context, future).await;
    drop(guard);
    session.wait_for_detached().await;
    if session.budget.native_execution_active() && !session.has_complete_cut() {
        session
            .failures
            .record(EvaluationFailureSummary::single(PhloFailure::Platform));
    }
    let mut errors = std::mem::take(
        &mut *session
            .detached_errors
            .lock()
            .expect("detached reduction errors lock"),
    );
    errors.sort_by(|left, right| left.0.cmp(&right.0));
    cancellation.complete = true;
    (
        result,
        session.failures.snapshot(),
        errors.into_iter().map(|(_, error)| error).collect(),
    )
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

    #[cfg(test)]
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

#[cfg(test)]
thread_local! {
    /// C9 (DR-84): sessions built on this thread while the flag is set split
    /// with the legacy two-segment scheme, for the schedule-equality test.
    pub(crate) static LEGACY_SPLIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct ReductionSession {
    #[cfg(test)]
    legacy_split: bool,
    space: ExecutionSpace,
    budget: RuntimeBudget,
    host_work: Option<HostWorkBudget>,
    failures: FailureRecorder,
    failure_work: Option<HostWorkBudget>,
    state: Mutex<SessionState>,
    evaluation_guard: Mutex<Option<OwnedRwLockReadGuard<()>>>,
    next_detached_id: AtomicU64,
    detached_count: AtomicUsize,
    detached_done: Notify,
    detached_tasks: Mutex<DetachedTasks>,
    detached_errors: Mutex<Vec<(ParticipantId, InterpreterError)>>,
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

impl Intent {
    fn reject(self, order: OperationOrder, error: RSpaceError) -> Completion {
        match self {
            Self::Produce { response, .. } => Completion::Produce {
                order,
                response,
                result: Err(error),
            },
            Self::Consume { response, .. } => Completion::Consume {
                order,
                response,
                result: Err(error),
            },
        }
    }
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
    async fn wait_for_detached(&self) {
        loop {
            let notified = self.detached_done.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.detached_count.load(Ordering::Acquire) == 0 {
                break;
            }
            notified.await;
        }
    }

    fn has_complete_cut(&self) -> bool {
        let state = self.state.lock().expect("reduction session lock");
        state.participants.is_empty() && state.intents.is_empty() && !state.driving
    }

    fn new(
        space: ExecutionSpace,
        budget: RuntimeBudget,
        host_work: Option<HostWorkBudget>,
        evaluation_guard: OwnedRwLockReadGuard<()>,
    ) -> Self {
        Self {
            #[cfg(test)]
            legacy_split: LEGACY_SPLIT.with(std::cell::Cell::get),
            space,
            budget,
            failure_work: host_work
                .as_ref()
                .map(|budget| HostWorkBudget::new(budget.limits())),
            host_work,
            failures: FailureRecorder::default(),
            state: Mutex::new(SessionState {
                participants: BTreeMap::new(),
                intents: BTreeMap::new(),
                driving: false,
            }),
            evaluation_guard: Mutex::new(Some(evaluation_guard)),
            next_detached_id: AtomicU64::new(0),
            detached_count: AtomicUsize::new(0),
            detached_done: Notify::new(),
            detached_tasks: Mutex::new(DetachedTasks {
                canceled: false,
                handles: HashMap::new(),
            }),
            detached_errors: Mutex::new(Vec::new()),
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

    async fn prepare(
        &self,
        order: OperationOrder,
        intent: Intent,
    ) -> Result<PreparedIntent, Completion> {
        let mut footprint = BTreeSet::new();
        match &intent {
            Intent::Produce { channel, data, .. } => {
                insert_channel(&mut footprint, channel);
                let joins = match self.space.get_joins(channel.clone()).await {
                    Ok(joins) => joins,
                    Err(error) => return Err(intent.reject(order, error)),
                };
                for join in joins {
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
        Ok(PreparedIntent {
            order,
            footprint,
            intent,
        })
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
        let mut completions = Vec::new();
        for (order, intent) in intents {
            match self.prepare(order, intent).await {
                Ok(intent) => prepared.push(intent),
                Err(completion) => completions.push(completion),
            }
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
    if budget.native_execution_active() {
        footprint.insert(vec![2, 0]);
    }
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
    execution: ExecutionSpace,
    coordinator: ReductionCoordinator,
}

impl DeterministicRSpace {
    pub fn new(inner: RhoISpace, coordinator: ReductionCoordinator) -> Self {
        Self {
            execution: scheduled_execution(inner.clone().into()),
            inner,
            coordinator,
        }
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

    async fn get_data_metered(
        &self,
        channel: &Par,
        meter: &(dyn SourceMeter + Sync),
    ) -> Result<Vec<Datum<ListParWithRandom>>, RSpaceError> {
        self.inner.get_data_metered(channel, meter).await
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
        if current().is_some_and(|context| context.session.budget.is_legacy()) {
            return self
                .inner
                .consume(channels, patterns, continuation, persistent, peeks)
                .await;
        }
        self.execution
            .consume(channels, patterns, continuation, persistent, peeks)
            .await
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
        if current().is_some_and(|context| context.session.budget.is_legacy()) {
            return self.inner.produce(channel, data, persistent).await;
        }
        self.execution.produce(channel, data, persistent).await
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

pub(crate) fn scheduled_execution(inner: ExecutionSpace) -> ExecutionSpace {
    ExecutionSpace::new(ScheduledExecution { inner })
}

struct ScheduledExecution {
    inner: ExecutionSpace,
}

#[async_trait]
impl ExecutionBackend for ScheduledExecution {
    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persistent: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<super::execution_space::ConsumeResult, RSpaceError> {
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
    ) -> Result<super::execution_space::ProduceResult, RSpaceError> {
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

    async fn get_joins(&self, channel: Par) -> Result<Vec<Vec<Par>>, RSpaceError> {
        self.inner.get_joins(channel).await
    }

    async fn is_replay(&self) -> bool { self.inner.is_replay().await }

    async fn update_produce(
        &self,
        original: &Produce,
        updated: Produce,
    ) -> Result<(), RSpaceError> {
        self.inner.update_produce(original, updated).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::Duration;

    use models::rhoapi::{CostAuthority, CostRegion};
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits, HostWorkUsage};
    use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
    use proptest::prelude::*;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use tokio::sync::Notify;

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::native_phlo_rules::{
        native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
        NativePhloExecutionContract, NativePhloRegionLimits,
    };
    use crate::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
    };
    use crate::rust::interpreter::accounting::NativeRuntimeConfig;
    use crate::rust::interpreter::execution_space::{ConsumeResult, ProduceResult};
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

    fn native_budget() -> RuntimeBudget {
        let descriptor = PhloScheduleV1 {
            protocol_version: 6,
            network: b"native-test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"atomic-REV",
            decimal_scale: 8,
            classes: NativePhloDimension::ALL
                .into_iter()
                .zip([0, 0, 1, 0])
                .zip([b"c".as_slice(), b"i", b"t", b"r"])
                .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
                .collect(),
            actual_price: 0,
            compatibility_rule: native_resource_compatibility_rule(),
        };
        let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
        let selected = binding.schedule();
        let permitted = [selected];
        let owners = [0];
        let controls = check_phlo_controls(
            selected.environment,
            0,
            u64::MAX,
            SignedPhloControls {
                limit: 1,
                price_ceiling: 0,
                required_owner_ceilings: &owners,
                permitted_schedules: &permitted,
            },
            selected,
            1,
        )
        .unwrap();
        let contract = NativePhloExecutionContract::new(controls, &binding).unwrap();
        let config = NativeRuntimeConfig::new(
            contract,
            NativeBudgetTraceLimits {
                attempts: 4,
                path_segments: 4,
                regions: NativePhloRegionLimits {
                    regions: 4,
                    encoded_authority_bytes: 4096,
                },
            },
            HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000))),
        );
        let budget = RuntimeBudget::new(Cost::create(1, "native global schedule"));
        budget.reset_for_native_execution(config).unwrap();
        budget
    }

    #[tokio::test]
    async fn native_selected_cut_rejects_unjoined_participant() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let (_, incomplete, _) = root_with_observation(
            reducer.space.clone(),
            native_budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {
                let children = current().expect("root reduction context").split(1);
                drop(children);
            },
        )
        .await;
        assert!(incomplete.contains(PhloFailure::Platform));

        let (_, complete, _) = root_with_observation(
            reducer.space.clone(),
            native_budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {},
        )
        .await;
        assert_eq!(complete, EvaluationFailureSummary::default());
    }

    #[tokio::test]
    async fn detached_reduction_is_drained_and_reports_its_failure() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let (result, summary, errors) = root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {
                spawn_detached(async {
                    tokio::task::yield_now().await;
                    Err(InterpreterError::UserAbortError)
                });
            },
        )
        .await;
        assert_eq!(result, ());
        assert_eq!(summary, EvaluationFailureSummary::single(PhloFailure::User));
        assert_eq!(errors, vec![InterpreterError::UserAbortError]);
    }

    #[tokio::test]
    async fn native_root_waits_for_detached_complete_cut() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let (_, summary, errors) = root_with_observation(
            reducer.space.clone(),
            native_budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {
                spawn_detached(async {
                    tokio::task::yield_now().await;
                    Ok(())
                });
            },
        )
        .await;
        assert_eq!(summary, EvaluationFailureSummary::default());
        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn canceled_root_aborts_its_detached_reduction() {
        struct MarkDrop(Arc<AtomicBool>);

        impl Drop for MarkDrop {
            fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
        }

        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let entered = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let entered_child = entered.clone();
        let dropped_child = dropped.clone();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            root_with_observation(
                reducer.space.clone(),
                reducer.metering.budget(),
                reducer.reduction_coordinator.clone(),
                None,
                async move {
                    spawn_detached(async move {
                        let _guard = MarkDrop(dropped_child);
                        entered_child.notify_one();
                        std::future::pending::<Result<(), InterpreterError>>().await
                    });
                    entered.notified().await;
                    std::future::pending::<()>().await;
                },
            ),
        )
        .await;
        assert!(result.is_err());
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached reduction should be aborted");
        tokio::time::timeout(
            Duration::from_secs(2),
            root_with_observation(
                reducer.space.clone(),
                reducer.metering.budget(),
                reducer.reduction_coordinator.clone(),
                None,
                async {},
            ),
        )
        .await
        .expect("canceled reduction should release its session");
    }

    #[tokio::test]
    async fn canceled_session_aborts_detached_child_before_first_poll() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let polled = Arc::new(AtomicBool::new(false));
        let child_polled = polled.clone();
        let (_, summary, errors) = tokio::time::timeout(
            Duration::from_secs(2),
            root_with_observation(
                reducer.space.clone(),
                reducer.metering.budget(),
                reducer.reduction_coordinator.clone(),
                None,
                async move {
                    let context = current().expect("reduction context");
                    let child = context.split(1).pop().expect("child context");
                    child
                        .session
                        .detached_tasks
                        .lock()
                        .expect("detached reduction tasks lock")
                        .canceled = true;
                    spawn_detached_in_context(child, async move {
                        child_polled.store(true, Ordering::SeqCst);
                        Ok(())
                    });
                },
            ),
        )
        .await
        .expect("canceled child must complete");
        assert!(!polled.load(Ordering::SeqCst));
        assert!(summary.contains(PhloFailure::Platform));
        assert_eq!(errors.len(), 1);
    }

    struct NearLimitBackend {
        first_channel: Par,
        admissions: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ExecutionBackend for NearLimitBackend {
        async fn consume(
            &self,
            _: Vec<Par>,
            _: Vec<BindPattern>,
            _: TaggedContinuation,
            _: bool,
            _: BTreeSet<i32>,
        ) -> Result<ConsumeResult, RSpaceError> {
            unreachable!()
        }

        async fn produce(
            &self,
            channel: Par,
            _: ListParWithRandom,
            _: bool,
        ) -> Result<ProduceResult, RSpaceError> {
            if channel == self.first_channel {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if self.admissions.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(None)
            } else {
                Err(RSpaceError::HostWorkRejected)
            }
        }

        async fn get_joins(&self, _: Par) -> Result<Vec<Vec<Par>>, RSpaceError> { Ok(Vec::new()) }

        async fn is_replay(&self) -> bool { false }

        async fn update_produce(&self, _: &Produce, _: Produce) -> Result<(), RSpaceError> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_disjoint_operations_reserve_near_limit_in_causal_order() {
        let budget = native_budget();
        let first_channel = models::rust::utils::new_gint_par(1, Vec::new(), false);
        let second_channel = models::rust::utils::new_gint_par(2, Vec::new(), false);
        let admissions = Arc::new(AtomicUsize::new(0));
        let space = scheduled_execution(ExecutionSpace::new(NearLimitBackend {
            first_channel: first_channel.clone(),
            admissions: admissions.clone(),
        }));
        let (first, second) = root(
            space.clone(),
            budget,
            ReductionCoordinator::default(),
            async {
                let parent = current().unwrap();
                let mut children = parent.split(2).into_iter();
                let first_context = children.next().unwrap();
                let second_context = children.next().unwrap();
                let first_space = space.clone();
                let first = tokio::spawn(scope(first_context.clone(), async move {
                    let _guard = ParticipantGuard::for_context(&first_context);
                    first_space
                        .produce(first_channel, ListParWithRandom::default(), false)
                        .await
                }));
                let second_space = space.clone();
                let second = tokio::spawn(scope(second_context.clone(), async move {
                    let _guard = ParticipantGuard::for_context(&second_context);
                    second_space
                        .produce(second_channel, ListParWithRandom::default(), false)
                        .await
                }));
                let result = (first.await.unwrap(), second.await.unwrap());
                parent.rejoin();
                result
            },
        )
        .await;
        assert!(first.is_ok());
        assert!(matches!(second, Err(RSpaceError::HostWorkRejected)));
        assert_eq!(admissions.load(Ordering::SeqCst), 2);
    }

    struct OrderedNativeBackend {
        seen: Arc<Mutex<Vec<Par>>>,
    }

    #[async_trait]
    impl ExecutionBackend for OrderedNativeBackend {
        async fn consume(
            &self,
            _: Vec<Par>,
            _: Vec<BindPattern>,
            _: TaggedContinuation,
            _: bool,
            _: BTreeSet<i32>,
        ) -> Result<ConsumeResult, RSpaceError> {
            unreachable!()
        }

        async fn produce(
            &self,
            channel: Par,
            _: ListParWithRandom,
            _: bool,
        ) -> Result<ProduceResult, RSpaceError> {
            self.seen.lock().expect("native order trace").push(channel);
            Ok(None)
        }

        async fn get_joins(&self, _: Par) -> Result<Vec<Vec<Par>>, RSpaceError> { Ok(Vec::new()) }

        async fn is_replay(&self) -> bool { false }

        async fn update_produce(&self, _: &Produce, _: Produce) -> Result<(), RSpaceError> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_recursive_frontiers_ignore_branch_arrival_order() {
        let channels =
            [1, 2, 3, 4].map(|value| models::rust::utils::new_gint_par(value, Vec::new(), false));
        for iteration in 0..8 {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let space = scheduled_execution(ExecutionSpace::new(OrderedNativeBackend {
                seen: seen.clone(),
            }));
            let expected = channels.clone();
            tokio::time::timeout(
                Duration::from_secs(2),
                root(
                    space.clone(),
                    native_budget(),
                    ReductionCoordinator::default(),
                    async {
                        let parent = current().expect("root reduction context");
                        let mut children = parent.split(2).into_iter();
                        let first_context = children.next().unwrap();
                        let second_context = children.next().unwrap();
                        let first_space = space.clone();
                        let first_channels = [channels[0].clone(), channels[2].clone()];
                        let first = tokio::spawn(scope(first_context.clone(), async move {
                            let _guard = ParticipantGuard::for_context(&first_context);
                            if iteration % 2 == 0 {
                                tokio::time::sleep(Duration::from_millis(2)).await;
                            }
                            for channel in first_channels {
                                first_space
                                    .produce(channel, ListParWithRandom::default(), false)
                                    .await?;
                                tokio::task::yield_now().await;
                            }
                            Ok::<(), RSpaceError>(())
                        }));
                        let second_space = space.clone();
                        let second_channels = [channels[1].clone(), channels[3].clone()];
                        let second = tokio::spawn(scope(second_context.clone(), async move {
                            let _guard = ParticipantGuard::for_context(&second_context);
                            if iteration % 2 == 1 {
                                tokio::time::sleep(Duration::from_millis(2)).await;
                            }
                            for channel in second_channels {
                                second_space
                                    .produce(channel, ListParWithRandom::default(), false)
                                    .await?;
                                tokio::task::yield_now().await;
                            }
                            Ok::<(), RSpaceError>(())
                        }));
                        first.await.unwrap().unwrap();
                        second.await.unwrap().unwrap();
                        parent.rejoin();
                    },
                ),
            )
            .await
            .expect("native frontier stalled");
            assert_eq!(*seen.lock().expect("native order trace"), expected);
        }
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_children_share_the_root_host_work_budget() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(16)));
        root_with_host_work(
            reducer.space.clone(),
            RuntimeBudget::new(Cost::create(100, "host work propagation")),
            ReductionCoordinator::default(),
            Some(host_work.clone()),
            async {
                let parent = current().expect("root reduction context");
                let children = parent.split(4);
                let mut handles = Vec::new();
                for child in children {
                    handles.push(tokio::spawn(scope(child.clone(), async move {
                        let _guard = ParticipantGuard::for_context(&child);
                        reserve_host_work(HostWorkDimension::ReductionSteps, HostWorkUnits::new(1))
                            .unwrap();
                    })));
                }
                for handle in handles {
                    handle.await.unwrap();
                }
                parent.rejoin();
            },
        )
        .await;

        assert_eq!(
            host_work.usage(HostWorkDimension::ReductionSteps),
            HostWorkUsage::new(4)
        );
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

    /// One step of a participant plan in the C9 path tests (DR-84): an
    /// operation, or a split whose children run their own plans.
    #[derive(Clone, Debug)]
    enum PathAction {
        Operation,
        Split(Vec<Vec<PathAction>>),
    }

    fn splitmix(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = *state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn path_plan(state: &mut u64, depth: usize) -> Vec<PathAction> {
        let length = usize::try_from(splitmix(state) % 5).expect("plan length fits in usize");
        let mut plan = Vec::with_capacity(length);
        for _ in 0..length {
            let split = depth > 0 && splitmix(state).is_multiple_of(3);
            plan.push(match split {
                false => PathAction::Operation,
                true => {
                    let count = usize::try_from(splitmix(state) % 3)
                        .expect("child count fits in usize")
                        + 1;
                    let mut children = Vec::with_capacity(count);
                    for _ in 0..count {
                        children.push(path_plan(state, depth - 1));
                    }
                    PathAction::Split(children)
                }
            });
        }
        plan
    }

    /// Runs `plan` from `context` and appends the participant path and every
    /// operation and descendant path that the plan generates.
    fn generated_paths(
        context: &ReductionContext,
        plan: &[PathAction],
        legacy: bool,
        paths: &mut Vec<CausalPath>,
    ) {
        paths.push(context.participant.clone());
        for action in plan {
            match action {
                PathAction::Operation => paths.push(context.next_operation().path),
                PathAction::Split(children) => {
                    let contexts = match legacy {
                        true => context.split_legacy(children.len()),
                        false => context.split(children.len()),
                    };
                    for (child, child_plan) in contexts.iter().zip(children) {
                        generated_paths(child, child_plan, legacy, paths);
                    }
                }
            }
        }
    }

    /// C9 (DR-84): on random operation and split plans, the compacted
    /// splitter and the legacy splitter generate paths in the same forward
    /// lexicographic order. Distinct steps of a plan get distinct paths, so
    /// the compaction is injective on generated paths. The shared-root
    /// comparison of `CausalPath` agrees with the vector order.
    #[tokio::test]
    async fn compacted_paths_preserve_vec_order() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let (mut total, mut deepest) = (0, 0);
        for seed in 0..64_u64 {
            let mut state = seed;
            let plan = path_plan(&mut state, 4);
            let ((compacted, legacy), _, _) = root_with_observation(
                reducer.space.clone(),
                native_budget(),
                reducer.reduction_coordinator.clone(),
                None,
                async {
                    let root = current().expect("root reduction context");
                    let compacted_root =
                        ReductionContext::root(root.session.clone(), root.session_id);
                    let legacy_root = ReductionContext::root(root.session.clone(), root.session_id);
                    let mut compacted = Vec::new();
                    let mut legacy = Vec::new();
                    generated_paths(&compacted_root, &plan, false, &mut compacted);
                    generated_paths(&legacy_root, &plan, true, &mut legacy);
                    (compacted, legacy)
                },
            )
            .await;
            assert_eq!(compacted.len(), legacy.len(), "seed {seed}");
            total += compacted.len();
            deepest = legacy.iter().map(CausalPath::len).fold(deepest, usize::max);
            let compacted_vectors = compacted.iter().map(CausalPath::to_vec).collect::<Vec<_>>();
            let legacy_vectors = legacy.iter().map(CausalPath::to_vec).collect::<Vec<_>>();
            for left in 0..compacted.len() {
                for right in 0..compacted.len() {
                    let expected = legacy_vectors[left].cmp(&legacy_vectors[right]);
                    assert_eq!(
                        expected == std::cmp::Ordering::Equal,
                        left == right,
                        "seed {seed}"
                    );
                    assert_eq!(
                        compacted_vectors[left].cmp(&compacted_vectors[right]),
                        expected,
                        "seed {seed}: {:?} {:?}",
                        compacted_vectors[left],
                        compacted_vectors[right],
                    );
                    assert_eq!(
                        compacted[left].cmp(&compacted[right]),
                        expected,
                        "seed {seed}"
                    );
                    assert_eq!(legacy[left].cmp(&legacy[right]), expected, "seed {seed}");
                }
            }
        }
        assert_eq!((total, deepest), (1_227, 9));
    }

    /// C9 (DR-84): `spawn_detached` splits one child for each detached
    /// reduction. A chain of 512 nested detached reductions gives an
    /// operation path of 513 segments, within the protocol-6 limit of 1,024
    /// path segments. The legacy splitter gave 1,025 segments.
    #[tokio::test]
    async fn funding_flow_depth_at_most_513() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let (depths, _, _) = root_with_observation(
            reducer.space.clone(),
            native_budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {
                let root = current().expect("root reduction context");
                let mut compacted = ReductionContext::root(root.session.clone(), root.session_id);
                let mut legacy = ReductionContext::root(root.session.clone(), root.session_id);
                for _ in 0..512 {
                    compacted = compacted.split(1).pop().expect("one compacted child");
                    legacy = legacy.split_legacy(1).pop().expect("one legacy child");
                }
                (
                    compacted.next_operation().path.len(),
                    legacy.next_operation().path.len(),
                )
            },
        )
        .await;
        assert_eq!(depths, (513, 1025));
    }
}
