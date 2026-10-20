//! Added by DR-132: native evaluation without the deterministic scheduler.
//! Native branches race, as on `dev`. The recorder names each native
//! operation by its journal row, and native replay finds each row by its
//! RSpace source.
//!
//! FF-1, FF-4 and FF-8 failed before DR-132. They were written first.
// Changed by DR-132 (D2-B): the stall rule closes the risk of FF-6, so FF-6
// runs.
// //! FF-1, FF-4 and FF-8 failed before DR-132. They were written first. FF-6
// //! shows a residual risk that DR-132 leaves open (D2-A), so it is ignored.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::checkpoint::{Checkpoint, SoftCheckpoint};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceAccountingObserver,
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use rspace_plus_plus::rspace::trace::Log;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::observation_construction;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::reduce::{spawn_counted, with_counted_evaluation};
use crate::rust::interpreter::rho_runtime::{create_rho_env, test_accounting_observer};

const LIMIT: u64 = 1_000_000;
const WEIGHTS: [u64; 4] = [2, 3, 5, 7];
const SEED: &[u8] = b"dr-132-racing-evaluation";
const PROBE_DEADLINE: Duration = Duration::from_secs(2);
const PAUSE_DEADLINE: Duration = Duration::from_secs(1);
const FORGERY_DEADLINE: Duration = Duration::from_secs(10);

const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
    events: 1_000,
    source_entries: 10_000,
    source_bytes: 10_000_000,
    telemetry_items: 1_000,
    telemetry_bytes: 1_000_000,
};

fn integer(value: i64) -> Par {
    Par {
        exprs: vec![models::rhoapi::Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

fn funded_data(value: i64, seed: u8, auth: &CostAuthority) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![integer(value)],
        random_state: vec![seed; 17],
        cost_authority: Some(auth.clone()),
        ..Default::default()
    }
}

fn free_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![models::rust::utils::new_freevar_par(0, Vec::new())],
        free_count: 1,
        remainder: None,
    }
}

fn register_produce(
    budget: &RuntimeBudget,
    channel: &Par,
    data: &ListParWithRandom,
    auth: &CostAuthority,
) {
    let source = Produce::create(channel, data, false);
    let introduction = observation_construction::produce_introduction(&source, channel, data, auth)
        .expect("the produce introduction is measured");
    budget
        .register_introduction_authority(introduction.event_id, introduction.kind, auth)
        .expect("the produce authority registers");
}

fn register_consume(
    budget: &RuntimeBudget,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    auth: &CostAuthority,
) {
    let source = Consume::create(&channels.to_vec(), &patterns.to_vec(), continuation, false);
    let introduction = observation_construction::consume_introduction(
        &source,
        channels,
        patterns,
        continuation,
        auth,
    )
    .expect("the consume introduction is measured");
    budget
        .register_introduction_authority(introduction.event_id, introduction.kind, auth)
        .expect("the consume authority registers");
}

async fn empty_play_space() -> RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    play.create_checkpoint().await.expect("the pre-state");
    play
}

/// Added by DR-132 (FF-1): a space below the deterministic scheduler that
/// counts the produce and consume calls in flight. Each call waits, without a
/// lock, until a second call is in flight or the deadline passes.
#[derive(Clone)]
struct InFlightProbe {
    inner: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    in_flight: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

impl InFlightProbe {
    async fn enter(&self) {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(current, Ordering::SeqCst);
        let deadline = tokio::time::Instant::now() + PROBE_DEADLINE;
        while self.in_flight.load(Ordering::SeqCst) < 2 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn leave(&self) { self.in_flight.fetch_sub(1, Ordering::SeqCst); }
}

#[async_trait]
impl ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> for InFlightProbe {
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
        self.inner.set_accounting_observer(observer)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError> {
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
        self.inner.remove_all_data(channel).await
    }

    async fn remove_data_at(&self, channel: &Par, index: i32) -> Result<(), RSpaceError> {
        self.inner.remove_data_at(channel, index).await
    }

    async fn remove_data_at_recorded(
        &self,
        channel: &Par,
        index: i32,
        operation_id: &[u8],
    ) -> Result<(), RSpaceError> {
        self.inner
            .remove_data_at_recorded(channel, index, operation_id)
            .await
    }

    async fn remove_all_continuations(&self, channels: Vec<Par>) -> Result<(), RSpaceError> {
        self.inner.remove_all_continuations(channels).await
    }

    async fn clear(&self) -> Result<(), RSpaceError> { self.inner.clear().await }

    async fn get_root(&self) -> Blake2b256Hash { self.inner.get_root().await }

    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError> {
        self.inner.reset(root).await
    }

    async fn consume_result(
        &self,
        channel: Vec<Par>,
        pattern: Vec<BindPattern>,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError> {
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
        self.inner.create_soft_checkpoint().await
    }

    async fn take_event_log(&self) -> Log { self.inner.take_event_log().await }

    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    ) -> Result<(), RSpaceError> {
        self.inner.revert_to_soft_checkpoint(checkpoint).await
    }

    async fn consume(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<
        MaybeConsumeResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        self.enter().await;
        let result = self
            .inner
            .consume(channels, patterns, continuation, persist, peeks)
            .await;
        self.leave();
        result
    }

    async fn produce(
        &self,
        channel: Par,
        data: ListParWithRandom,
        persist: bool,
    ) -> Result<
        MaybeProduceResult<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        RSpaceError,
    > {
        self.enter().await;
        let result = self.inner.produce(channel, data, persist).await;
        self.leave();
        result
    }

    async fn install(
        &self,
        channels: Vec<Par>,
        patterns: Vec<BindPattern>,
        continuation: TaggedContinuation,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, RSpaceError> {
        self.inner.install(channels, patterns, continuation).await
    }

    async fn rig_and_reset(&self, start_root: Blake2b256Hash, log: Log) -> Result<(), RSpaceError> {
        self.inner.rig_and_reset(start_root, log).await
    }

    async fn rig(&self, log: Log) -> Result<(), RSpaceError> { self.inner.rig(log).await }

    async fn check_replay_data(&self) -> Result<(), RSpaceError> {
        self.inner.check_replay_data().await
    }

    async fn is_replay(&self) -> bool { self.inner.is_replay().await }

    async fn update_produce(&self, produce: Produce) -> () {
        self.inner.update_produce(produce).await
    }
}

/// Added by DR-132 (FF-1, failing first): native branches race, as on `dev`.
/// Under the deterministic scheduler a native frontier ran one operation at a
/// time, so the probe never saw two calls in flight.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_parallel_branches_run_concurrently() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let probe = InFlightProbe {
        inner: empty_play_space().await,
        in_flight: Arc::new(AtomicUsize::new(0)),
        maximum: Arc::new(AtomicUsize::new(0)),
    };
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
    budget
        .reset_for_native_execution(config(LIMIT, WEIGHTS))
        .expect("the play budget resets");
    let (reducer, _, _, _) = create_rho_env(
        probe.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
        crate::rust::interpreter::io::handle_table::FileHandleTable::new(),
    )
    .await
    .expect("the play environment starts");
    let parsed = Compiler::source_to_adt(r#"@"a"!(1) | @"b"!(2)"#).expect("the term parses");
    let scope = budget.enter_comm_accounting_scope();
    let played = reducer
        .inj_with_observation(parsed, Blake2b512Random::create_from_bytes(SEED))
        .await;
    drop(scope);
    played.0.expect("the native play succeeds");
    assert_eq!(
        probe.maximum.load(Ordering::SeqCst),
        2,
        "the two native sends run concurrently"
    );
}

/// Added by DR-132 (FF-4, failing first): with no outer name, the recorder
/// names each native operation by its journal row, and the ordered log binds
/// to the rows. Before DR-132 only the deterministic scheduler set a name, so
/// a native operation outside it failed with "native operation has no context".
#[tokio::test]
async fn native_play_names_each_operation_by_its_journal_row() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let auth = authority(3);
    let (a, b) = (integer(1), integer(2));
    let first = funded_data(10, 1, &auth);
    let second = funded_data(20, 2, &auth);
    let pattern = free_pattern();
    let continuation = TaggedContinuation {
        cost_authority: Some(auth.clone()),
        ..Default::default()
    };
    let play = empty_play_space().await;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
        .expect("the budget resets");
    let _scope = budget.enter_comm_accounting_scope();
    register_produce(&budget, &a, &first, &auth);
    register_produce(&budget, &b, &second, &auth);
    register_consume(
        &budget,
        std::slice::from_ref(&a),
        std::slice::from_ref(&pattern),
        &continuation,
        &auth,
    );
    play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
    assert!(play
        .produce(a.clone(), first, false)
        .await
        .expect("the first produce is recorded")
        .is_none());
    assert!(play
        .produce(b, second, false)
        .await
        .expect("the second produce is recorded")
        .is_none());
    assert!(play
        .consume(vec![a], vec![pattern], continuation, false, BTreeSet::new())
        .await
        .expect("the consume is recorded")
        .is_some());
    let recording = budget
        .native_budget_recording()
        .expect("the recording reads")
        .expect("the play has a budget recording");
    let rows = budget
        .native_operation_recording()
        .expect("the journal reads")
        .expect("the play has an operation journal");
    let names: Vec<_> = rows
        .iter()
        .map(|row| recording.paths.segments(row.occurrence.path))
        .collect();
    assert_eq!(names, vec![vec![(0, 0)], vec![(1, 0)], vec![(2, 0)]]);
    let log = play.create_soft_checkpoint().await.log;
    with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
        contract.check_operation_journal(recording.session, &recording, rows, journal_limits())
    })
    .expect("the journal checks")
    .bind_trace(log.into(), TRACE_LIMITS)
    .expect("the ordered log binds to the rows");
}

/// Added by DR-132 (FF-8): an observer that holds the row of a firing produce
/// open. It pauses in `observe_operation_finish` of the first produce that
/// matches, and it records whether another operation starts during the pause.
struct PausingObserver {
    inner:
        Arc<dyn RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation>>,
    paused: AtomicBool,
    pause_done: AtomicBool,
    started_during_pause: AtomicBool,
    release: (Mutex<bool>, Condvar),
}

impl PausingObserver {
    fn new(
        inner: Arc<
            dyn RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
        >,
    ) -> Self {
        Self {
            inner,
            paused: AtomicBool::new(false),
            pause_done: AtomicBool::new(false),
            started_during_pause: AtomicBool::new(false),
            release: (Mutex::new(false), Condvar::new()),
        }
    }

    fn release_pause(&self) {
        let (released, wake) = &self.release;
        *released.lock().expect("the release flag") = true;
        wake.notify_all();
    }
}

impl RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for PausingObserver
{
    fn replay_operation_directive(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<
        Option<rspace_plus_plus::rspace::rspace_interface::ReplayOperationDirective>,
        RSpaceError,
    > {
        self.inner.replay_operation_directive(source)
    }

    fn observe_operation_start(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<Option<OperationOrder>, RSpaceError> {
        if self.paused.load(Ordering::SeqCst) {
            self.started_during_pause.store(true, Ordering::SeqCst);
            self.release_pause();
        }
        self.inner.observe_operation_start(source, channels, joins)
    }

    fn observe_operation_finish(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        let firing = matches!(
            (&source, completion),
            (
                RSpaceOperationSource::Produce(_),
                RSpaceOperationCompletion::Matched
            )
        );
        if firing && !self.pause_done.swap(true, Ordering::SeqCst) {
            self.paused.store(true, Ordering::SeqCst);
            let (released, wake) = &self.release;
            let guard = released.lock().expect("the release flag");
            let _ = wake
                .wait_timeout_while(guard, PAUSE_DEADLINE, |released| !*released)
                .expect("the pause waits");
            self.paused.store(false, Ordering::SeqCst);
        }
        self.inner.observe_operation_finish(source, completion);
    }

    fn observe_produce(
        &self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_produce(source, channel, data, persistent)
    }

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_consume(source, channels, patterns, continuation, persistent, peeks)
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        data: &[(&ListParWithRandom, bool)],
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_comm(comm, continuation, continuation_persistent, data)
    }
}

/// Added by DR-132 (FF-8, failing first): a produce on `a` waits for the open
/// row of a produce on `b` that fires the join `[a, b]`. Before DR-132 the
/// produce on `b` held phase B only on the joins of `b`. After its COMM removed
/// the join from `a`, a produce on `a` read no joins, took no phase-B lock and
/// started inside the open footprint `{a, b}`. The recorder then failed with
/// "native conflicting operations overlap outside channel guards". Under the
/// deterministic scheduler a native frontier ran one operation at a time, so
/// the window was unreachable.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_produce_waits_for_the_open_join_footprint_of_another_produce() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let auth = authority(3);
    let (a, b) = (integer(1), integer(2));
    let early = funded_data(10, 1, &auth);
    let firing = funded_data(20, 2, &auth);
    let late = funded_data(30, 3, &auth);
    let patterns = vec![free_pattern(), free_pattern()];
    let continuation = TaggedContinuation {
        cost_authority: Some(auth.clone()),
        ..Default::default()
    };
    let play = empty_play_space().await;
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
        .expect("the budget resets");
    let _scope = budget.enter_comm_accounting_scope();
    let join = vec![a.clone(), b.clone()];
    register_consume(&budget, &join, &patterns, &continuation, &auth);
    register_produce(&budget, &a, &early, &auth);
    register_produce(&budget, &b, &firing, &auth);
    register_produce(&budget, &a, &late, &auth);
    let observer = Arc::new(PausingObserver::new(test_accounting_observer(
        budget.clone(),
    )));
    play.set_accounting_observer(Some(observer.clone()));
    let session = budget.deploy_id();
    let name = move |row: u64| OperationOrder {
        session,
        path: vec![(row, 0)].into(),
    };
    assert!(operation_context::scope(
        name(0),
        play.consume(join.clone(), patterns, continuation, false, BTreeSet::new())
    )
    .await
    .expect("the join waits")
    .is_none());
    assert!(
        operation_context::scope(name(1), play.produce(a.clone(), early, false))
            .await
            .expect("the datum on a is stored")
            .is_none()
    );
    let fired = tokio::spawn({
        let play = play.clone();
        let b = b.clone();
        operation_context::scope(name(2), async move { play.produce(b, firing, false).await })
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !observer.paused.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the firing produce reaches its pause"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let stored = tokio::spawn({
        let play = play.clone();
        operation_context::scope(name(3), async move { play.produce(a, late, false).await })
    });
    let fired = fired.await.expect("the firing task joins");
    let stored = stored.await.expect("the late task joins");
    assert!(
        fired.expect("the produce on b fires the join").is_some(),
        "the join fires"
    );
    assert!(
        !observer.started_during_pause.load(Ordering::SeqCst),
        "the produce on a started inside the open footprint of the produce on b"
    );
    assert!(
        stored
            .expect("the produce on a is recorded after the join completes")
            .is_none(),
        "the late datum on a is stored"
    );
}

// Changed by DR-132 (D2-B): FF-6 runs its operations as counted tasks of one
// evaluation, so the stall rule can end the wait, and it is no longer
// ignored. The original test called the session without an evaluation, so
// the session could not tell that every task waits. Its body stays below.
// /// Added by DR-132 (FF-6): a forged journal can put the produce of a body
// /// before the consume that runs the body. The term is
// /// `for (_ <- @"a") { @"a"!(2) } | @"a"!(1)`. The forged journal records the
// /// body's `@"a"!(2)` as row 0, the consume as row 1 with row 0 as its channel
// /// predecessor, and `@"a"!(1)` as row 2. A play of the three operations in
// /// that order records this journal, so it passes `check_operation_journal`
// /// and `bind_trace`. Replay of the term then stops: the consume waits for row
// /// 0, `@"a"!(1)` waits for the consume, and only the body of the consume sends
// /// `@"a"!(2)`. The user left this stall out of DR-132 (D2-A), so the test is
// /// ignored. It fails at its deadline.
// #[ignore = "DR-132 D2-A: a forged journal can stall native replay"]
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn forged_journal_inversion_reaches_a_verdict() {
//     let auth = authority(3);
//     let a = integer(1);
//     let one = funded_data(1, 1, &auth);
//     let two = funded_data(2, 2, &auth);
//     let pattern = free_pattern();
//     let continuation = TaggedContinuation {
//         cost_authority: Some(auth.clone()),
//         ..Default::default()
//     };
//     let play = empty_play_space().await;
//     let history = play.get_history_repository();
//     let budget = RuntimeBudget::new(Cost::unsafe_max());
//     budget
//         .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
//         .expect("the budget resets");
//     let _scope = budget.enter_comm_accounting_scope();
//     register_produce(&budget, &a, &two, &auth);
//     register_consume(
//         &budget,
//         std::slice::from_ref(&a),
//         std::slice::from_ref(&pattern),
//         &continuation,
//         &auth,
//     );
//     register_produce(&budget, &a, &one, &auth);
//     play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
//     assert!(play
//         .produce(a.clone(), two.clone(), false)
//         .await
//         .expect("the forged body datum is stored")
//         .is_none());
//     assert!(play
//         .consume(
//             vec![a.clone()],
//             vec![pattern.clone()],
//             continuation.clone(),
//             false,
//             BTreeSet::new(),
//         )
//         .await
//         .expect("the forged consume fires on the body datum")
//         .is_some());
//     assert!(play
//         .produce(a.clone(), one.clone(), false)
//         .await
//         .expect("the top-level datum is stored")
//         .is_none());
//     let recording = budget
//         .native_budget_recording()
//         .expect("the recording reads")
//         .expect("the play has a budget recording");
//     let rows = budget
//         .native_operation_recording()
//         .expect("the journal reads")
//         .expect("the play has an operation journal");
//     assert_eq!(rows[1].predecessors.as_ref(), &[0]);
//     assert_eq!(rows[2].predecessors.as_ref(), &[1]);
//     let log = play.create_soft_checkpoint().await.log;
//     let trace = with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
//         contract.check_operation_journal(recording.session, &recording, rows, journal_limits())
//     })
//     .expect("the forged journal checks")
//     .bind_trace(log.into(), TRACE_LIMITS)
//     .expect("the forged log binds to the rows");
//     let replay = Arc::new(
//         trace
//             .into_session(history, Arc::new(Box::new(Matcher)))
//             .expect("the replay session opens"),
//     );
//     let verdict = tokio::time::timeout(FORGERY_DEADLINE, async {
//         let consume = tokio::spawn({
//             let replay = replay.clone();
//             let (a, pattern, continuation, auth) = (
//                 a.clone(),
//                 pattern.clone(),
//                 continuation.clone(),
//                 auth.clone(),
//             );
//             async move {
//                 replay
//                     .consume(
//                         vec![a],
//                         vec![pattern],
//                         continuation,
//                         false,
//                         BTreeSet::new(),
//                         &auth,
//                     )
//                     .await
//                     .map(|fired| fired.is_some())
//             }
//         });
//         let produce = tokio::spawn({
//             let replay = replay.clone();
//             let (a, one, auth) = (a.clone(), one.clone(), auth.clone());
//             async move {
//                 replay
//                     .produce(a, one, false, &auth)
//                     .await
//                     .map(|fired| fired.is_some())
//             }
//         });
//         let consumed = consume.await.expect("the consume task joins")?;
//         let produced = produce.await.expect("the produce task joins")?;
//         if consumed || produced {
//             replay.produce(a.clone(), two.clone(), false, &auth).await?;
//         }
//         replay.check_complete().await
//     })
//     .await
//     .expect("the replay of the forged journal reaches a verdict");
//     assert!(verdict.is_err(), "the replay rejects the forged journal");
// }

/// Added by DR-132 (D2-B): the operations of
/// `for (_ <- @"a") { @"a"!(2) } | @"a"!(1)`: the top-level send of 1, the
/// receive, and the send of 2 by the body of the receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Send,
    Receive,
    Body,
}

type Session = NativeRuntimeReplaySession<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

type History = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync
            + 'static,
    >,
>;

/// Added by DR-132 (D2-B): one play of the operations of FF-6 in a chosen
/// order. Play records a journal for that order, and the journal passes
/// `check_operation_journal` and `bind_trace`. Only the orders in which the
/// body's send follows the receive come from an honest evaluation.
struct Inversion {
    a: Par,
    one: ListParWithRandom,
    two: ListParWithRandom,
    pattern: BindPattern,
    continuation: TaggedContinuation,
    auth: CostAuthority,
    history: History,
    recording: NativeBudgetRecording,
    rows: Arc<[NativeOperationRecord]>,
    log: Log,
}

impl Inversion {
    async fn play(order: [Step; 3]) -> Self {
        let auth = authority(3);
        let a = integer(1);
        let one = funded_data(1, 1, &auth);
        let two = funded_data(2, 2, &auth);
        let pattern = free_pattern();
        let continuation = TaggedContinuation {
            cost_authority: Some(auth.clone()),
            ..Default::default()
        };
        let play = empty_play_space().await;
        let history = play.get_history_repository();
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
            .expect("the budget resets");
        let _scope = budget.enter_comm_accounting_scope();
        register_produce(&budget, &a, &two, &auth);
        register_consume(
            &budget,
            std::slice::from_ref(&a),
            std::slice::from_ref(&pattern),
            &continuation,
            &auth,
        );
        register_produce(&budget, &a, &one, &auth);
        play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
        for step in order {
            match step {
                Step::Send => {
                    play.produce(a.clone(), one.clone(), false)
                        .await
                        .expect("the send plays");
                }
                Step::Body => {
                    play.produce(a.clone(), two.clone(), false)
                        .await
                        .expect("the body's send plays");
                }
                Step::Receive => {
                    play.consume(
                        vec![a.clone()],
                        vec![pattern.clone()],
                        continuation.clone(),
                        false,
                        BTreeSet::new(),
                    )
                    .await
                    .expect("the receive plays");
                }
            }
        }
        let recording = budget
            .native_budget_recording()
            .expect("the recording reads")
            .expect("the play has a budget recording");
        let rows = budget
            .native_operation_recording()
            .expect("the journal reads")
            .expect("the play has an operation journal");
        let log = play.create_soft_checkpoint().await.log;
        Self {
            a,
            one,
            two,
            pattern,
            continuation,
            auth,
            history,
            recording,
            rows,
            log,
        }
    }

    /// A fresh replay session of the recorded journal.
    fn session(&self) -> Arc<Session> {
        let trace = with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
            contract.check_operation_journal(
                self.recording.session,
                &self.recording,
                Arc::clone(&self.rows),
                journal_limits(),
            )
        })
        .expect("the journal checks")
        .bind_trace(self.log.clone().into(), TRACE_LIMITS)
        .expect("the log binds to the rows");
        Arc::new(
            trace
                .into_session(self.history.clone(), Arc::new(Box::new(Matcher)))
                .expect("the replay session opens"),
        )
    }

    /// The receive, and the body's send when the receive fires.
    fn receive(
        &self,
        replay: &Arc<Session>,
    ) -> impl Future<Output = Result<(), InterpreterError>> + Send + 'static {
        let replay = Arc::clone(replay);
        let (a, two, pattern, continuation, auth) = (
            self.a.clone(),
            self.two.clone(),
            self.pattern.clone(),
            self.continuation.clone(),
            self.auth.clone(),
        );
        async move {
            let fired = replay
                .consume(
                    vec![a.clone()],
                    vec![pattern],
                    continuation,
                    false,
                    BTreeSet::new(),
                    &auth,
                )
                .await?;
            if fired.is_some() {
                replay.produce(a, two, false, &auth).await?;
            }
            Ok(())
        }
    }

    /// The top-level send, and the body's send when the send fires the
    /// receive.
    fn send(
        &self,
        replay: &Arc<Session>,
    ) -> impl Future<Output = Result<(), InterpreterError>> + Send + 'static {
        let replay = Arc::clone(replay);
        let (a, one, two, auth) = (
            self.a.clone(),
            self.one.clone(),
            self.two.clone(),
            self.auth.clone(),
        );
        async move {
            let fired = replay.produce(a.clone(), one, false, &auth).await?;
            if fired.is_some() {
                replay.produce(a, two, false, &auth).await?;
            }
            Ok(())
        }
    }

    /// Replays the term with the receive and the send as counted tasks of
    /// one evaluation, as racing evaluation runs them. It returns the errors
    /// of the tasks, the completion check and the session, or no verdict at
    /// the deadline.
    async fn replay(
        &self,
    ) -> Result<(Vec<InterpreterError>, Result<(), RSpaceError>, Arc<Session>), String> {
        let replay = self.session();
        let (receive, send) = (self.receive(&replay), self.send(&replay));
        let ((), errors) = tokio::time::timeout(
            FORGERY_DEADLINE,
            with_counted_evaluation(replay.evaluation_progress(), async move {
                spawn_counted(receive);
                spawn_counted(send);
            }),
        )
        .await
        .map_err(|_| "no verdict within the deadline".to_owned())?
        .map_err(|error| format!("the evaluation does not begin: {error}"))?;
        let complete = replay.check_complete().await;
        Ok((errors, complete, replay))
    }
}

/// Added by DR-132 (FF-6, D2-B): a forged journal can put the produce of a
/// body before the consume that runs the body. The forged journal records the
/// body's `@"a"!(2)` as row 0, the consume as row 1 with row 0 as its channel
/// predecessor, and `@"a"!(1)` as row 2. A play of the three operations in
/// that order records this journal, so it passes the checker. In replay the
/// receive waits for row 0, the send waits for the receive, and only the body
/// of the receive sends `@"a"!(2)`. Every live task waits, so replay declares
/// a stall and rejects the journal, as dev rejects a log that its replay
/// cannot consume.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_journal_inversion_reaches_a_verdict() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let inversion = Inversion::play([Step::Body, Step::Receive, Step::Send]).await;
    assert_eq!(inversion.rows[1].predecessors.as_ref(), &[0]);
    assert_eq!(inversion.rows[2].predecessors.as_ref(), &[1]);
    let (errors, complete, replay) = inversion
        .replay()
        .await
        .expect("the replay of the forged journal reaches a verdict");
    assert!(complete.is_err(), "the replay rejects the forged journal");
    let errors = format!("{errors:?}");
    assert!(errors.contains("native replay stalled"), "{errors}");
    assert!(replay.progress_stalled());
}

/// Added by DR-132 (D2-B): the single-root form of FF-6. The root runs the
/// receive itself, so it is the only live task. Its own park finds every
/// live task parked and declares the stall.
#[tokio::test]
async fn forged_journal_inversion_stalls_a_single_root() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let inversion = Inversion::play([Step::Body, Step::Receive, Step::Send]).await;
    let replay = inversion.session();
    let receive = inversion.receive(&replay);
    let (received, errors) = tokio::time::timeout(
        FORGERY_DEADLINE,
        with_counted_evaluation(replay.evaluation_progress(), receive),
    )
    .await
    .expect("the single root reaches a verdict")
    .expect("the evaluation begins");
    let error = format!("{received:?}");
    assert!(error.contains("native replay stalled"), "{error}");
    assert!(errors.is_empty(), "no task was spawned: {errors:?}");
    assert!(replay.check_complete().await.is_err());
    assert!(replay.progress_stalled());
}

/// Added by DR-132 (P-6, D2-B): the runs of each order on each runtime.
const P6_RUNS: usize = 4;

/// Added by DR-132 (P-6, D2-B): every order of the three operations of FF-6
/// plays, and its journal passes the checker. Replay of each journal reaches
/// its verdict before the deadline, on a current-thread runtime and on
/// runtimes with 2 and 8 workers, over repeated runs, so the verdict does not
/// depend on the schedule. An order in which the body's send precedes the
/// receive, or in which the receive or the send waits on that send, forms a
/// cycle with the term, and replay rejects it with a stall. The two honest
/// orders are accepted.
#[test]
fn forged_order_reaches_its_verdict() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let orders = [
        ([Step::Send, Step::Receive, Step::Body], true),
        ([Step::Receive, Step::Send, Step::Body], true),
        ([Step::Body, Step::Receive, Step::Send], false),
        ([Step::Body, Step::Send, Step::Receive], false),
        ([Step::Receive, Step::Body, Step::Send], false),
        ([Step::Send, Step::Body, Step::Receive], false),
    ];
    let runtimes = [
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime"),
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime with 2 workers"),
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8)
            .enable_all()
            .build()
            .expect("a runtime with 8 workers"),
    ];
    let mut failed = Vec::new();
    for (order, honest) in orders {
        let inversion = runtimes[0].block_on(Inversion::play(order));
        for (index, runtime) in runtimes.iter().enumerate() {
            for run in 0..P6_RUNS {
                match runtime.block_on(inversion.replay()) {
                    Ok((errors, complete, replay)) => {
                        let accepted = errors.is_empty() && complete.is_ok();
                        let stalled = replay.progress_stalled()
                            && format!("{errors:?}").contains("native replay stalled");
                        if accepted != honest || stalled == honest {
                            failed.push(format!(
                                "{order:?} on runtime {index}, run {run}: accepted {accepted}, \
                                 stalled {stalled}, errors {errors:?}"
                            ));
                        }
                    }
                    Err(error) => {
                        failed.push(format!("{order:?} on runtime {index}, run {run}: {error}"))
                    }
                }
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Added by DR-132 (D2-B, review m2): a wait that is dropped while it holds
/// a registration leaves it, even after a panic poisoned the state lock. The
/// receive of FF-6 parks as one of two live tasks, a panic poisons the lock,
/// and the receive is dropped. The parked count returns to zero, no stall is
/// declared, and the word is idle after the evaluation.
#[tokio::test]
async fn a_dropped_wait_leaves_its_registration_on_a_poisoned_lock() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let inversion = Inversion::play([Step::Body, Step::Receive, Step::Send]).await;
    let replay = inversion.session();
    let receive = inversion.receive(&replay);
    let observed = Arc::clone(&replay);
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let ((), errors) = tokio::time::timeout(
        FORGERY_DEADLINE,
        with_counted_evaluation(replay.evaluation_progress(), async move {
            spawn_counted(async move {
                let _ = released.await;
                Ok(())
            });
            let mut receive = Box::pin(receive);
            let mut polls = 0;
            while observed.progress_counts().1 == 0 {
                assert!(
                    futures::poll!(receive.as_mut()).is_pending(),
                    "the receive waits on row 0"
                );
                polls += 1;
                assert!(polls < 1_000, "the receive never parked");
                tokio::task::yield_now().await;
            }
            assert_eq!(observed.progress_counts(), (2, 1));
            observed.poison_state();
            drop(receive);
            assert_eq!(
                observed.progress_counts(),
                (2, 0),
                "the dropped wait left its registration"
            );
            release.send(()).expect("the other task waits");
        }),
    )
    .await
    .expect("the evaluation ends")
    .expect("the evaluation begins");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(!replay.progress_stalled());
    assert!(replay.progress_idle());
}

/// Added by DR-132 (D2-B): the source text of the production files of the
/// native evaluation path, without full-line comments.
fn production_code(path: &str) -> String {
    let text = std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR")))
        .expect("the source file reads");
    text.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Added by DR-132 (D2-B, review m3): the stall rule counts every task that
/// can run a native operation (invariant I5). The evaluator spawns tasks
/// only through `spawn_in_dispatch_scope`, and no file of the native
/// evaluation path joins or selects futures, so no task waits on another
/// task outside the counted waits. A new spawn or join must count first.
#[test]
fn native_evaluation_spawns_only_counted_tasks() {
    let session = format!(
        "{}/../rspace++/src/rspace/replay_rspace/native_session",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut files = vec![
        "src/rust/interpreter/reduce.rs".to_owned(),
        "src/rust/interpreter/dispatch.rs".to_owned(),
        "src/rust/interpreter/contract_call.rs".to_owned(),
        "src/rust/interpreter/system_processes.rs".to_owned(),
        "../rspace++/src/rspace/replay_rspace/native_session.rs".to_owned(),
    ];
    let mut directories = vec![std::path::PathBuf::from(session)];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).expect("the session directory reads") {
            let path = entry.expect("a directory entry").path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name == "tests" || name == "tests.rs" {
                continue;
            }
            if path.is_dir() {
                directories.push(path);
            } else if name.ends_with(".rs") {
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .map(|relative| relative.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| {
                        format!(
                            "../{}",
                            path.to_string_lossy()
                                .split_once("rspace++/")
                                .map(|(_, tail)| format!("rspace++/{tail}"))
                                .expect("a path inside rspace++")
                        )
                    });
                files.push(relative);
            }
        }
    }
    let forbidden = [
        "join!(",
        "select!(",
        "join_all",
        "try_join",
        "futures::future::select",
        "FuturesUnordered",
        "buffer_unordered",
        "spawn_blocking",
        "thread::spawn",
        "tokio::spawn",
    ];
    let mut found = Vec::new();
    for file in &files {
        let mut code = production_code(file);
        if file.ends_with("interpreter/reduce.rs") {
            let start = code
                .find("fn spawn_in_dispatch_scope<F>")
                .expect("reduce.rs spawns through spawn_in_dispatch_scope");
            let end = start + code[start..].find("\n}\n").expect("the function ends");
            assert_eq!(code[start..end].matches("tokio::spawn").count(), 3);
            code.replace_range(start..end, "");
        }
        for pattern in forbidden {
            if code.contains(pattern) {
                found.push(format!("{file}: {pattern}"));
            }
        }
    }
    assert!(files.len() > 6, "the session files were found: {files:?}");
    assert!(found.is_empty(), "uncounted concurrency: {found:#?}");
}

/// Added by DR-132 (D2-B, review M2): the order of the critical sections
/// that the Loom model checks. `wait_ready` registers its notification before
/// it takes the state lock, so no change between its check and its await is
/// lost. `changed` clears the parked count inside the critical section of the
/// change and notifies after the unlock.
#[test]
fn stall_critical_sections_keep_their_order() {
    let code = production_code(
        "src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay.rs",
    );
    let body = |signature: &str| -> &str {
        let start = code.find(signature).expect("the function exists");
        let end = start + code[start..].find("\n    }\n").expect("the function ends");
        &code[start..end]
    };
    let wait = body("async fn wait_ready(&self");
    let position = |text: &str, pattern: &str| {
        text.find(pattern)
            .unwrap_or_else(|| panic!("`{pattern}` is missing"))
    };
    assert!(position(wait, ".notified()") < position(wait, ".enable()"));
    assert!(position(wait, ".enable()") < position(wait, ".state.lock()"));
    assert!(position(wait, ".state.lock()") < position(wait, "check_and_park("));
    assert!(position(wait, "check_and_park(") < position(wait, "notification.await"));
    let changed = body("fn changed(&self");
    assert!(position(changed, "after_change(") < position(changed, "drop(state)"));
    assert!(position(changed, "drop(state)") < position(changed, "notify_waiters()"));
}
