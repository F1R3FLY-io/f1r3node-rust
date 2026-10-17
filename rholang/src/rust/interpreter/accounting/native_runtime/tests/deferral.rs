//! Added by DR-132 (F4): native-budget variants of dev's deferral specs
//! (`rholang/tests/deferred_dispatch_spec.rs` and
//! `rholang/tests/deferred_dispatch_memory_spec.rs`), and the loops of F2 and
//! F3 of the DR-132 plan.
//!
//! Each test plays a term with native phlo and replays its checked trace
//! through the native replay session, the path of protocol-6 validators. The
//! heap tests measure the transient heap of the play and of the replay, each
//! on its own runtime. A chain of continuation bodies (the tail recursion), of
//! system-process replies (F2) or of persistent re-issues (F3) holds no heap
//! for each step, because each step runs as a task of the per-call scope.

use std::collections::HashMap;
use std::time::Duration;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::Log;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::test_support::{with_live_heap, LiveHeap};
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::interpreter::EvaluateResult;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::rho_runtime::{
    create_native_replay_env, create_rho_env, RhoHistoryRepository,
};

type PlaySpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

const LIMIT: u64 = u64::MAX;
const WEIGHTS: [u64; 4] = [2, 3, 5, 7];
const SEED: &[u8] = b"dr-132-native-deferral";
const WORKERS: usize = 4;
/// The iterations of dev's `deferred_dispatch_memory_spec`.
const ITERATIONS: usize = 10_000;
/// The stored data of the F3 loop. Each re-issue of the persistent consume
/// reads the remaining data of the channel, so the play takes time in the
/// square of this count.
const STORED_DATA: usize = 2_000;
/// The fill of the F3 pre-state stores this many data in each deploy, as
/// `large_channel.rs` does.
const FILL_CHUNK: usize = 1_000;
/// The transient-heap limit of dev's `deferred_dispatch_memory_spec`. Before
/// dev's fix, 10,000 iterations held about 313 MB.
const TRANSIENT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
    events: 1_000_000,
    source_entries: 1_000_000,
    source_bytes: 1 << 30,
    telemetry_items: 1_000_000,
    telemetry_bytes: 1 << 30,
};

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(SEED) }

fn funded_budget() -> RuntimeBudget {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(SEED, Sig::Ground(vec![7]));
    budget
        .reset_for_native_execution(config(LIMIT, WEIGHTS))
        .expect("the native budget resets");
    budget
}

/// A native play: its result and the evidence and state that replay needs.
struct Played {
    term: Par,
    result: Result<(), InterpreterError>,
    recording: NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    log: Log,
    history: RhoHistoryRepository,
}

/// A space whose pre-state is checkpointed, with the store manager that keeps
/// its stores.
async fn empty_space() -> (PlaySpace, InMemoryStoreManager) {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    play.create_checkpoint().await.expect("the pre-state");
    (play, stores)
}

/// Plays `term` with native phlo on `play`, and then checkpoints. With a
/// `heap`, it measures the transient heap of the evaluation alone, and not of
/// the evidence copies and the checkpoint after it. The result keeps the
/// reducer.
async fn native_play(
    play: &PlaySpace,
    term: &str,
    heap: Option<&'static LiveHeap>,
) -> (Played, usize, impl Sized) {
    let parsed = Compiler::source_to_adt(term).expect("the term parses");
    let history = play.get_history_repository();
    let budget = funded_budget();
    let (reducer, _, _, _) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .expect("the play environment starts");
    let scope = budget.enter_comm_accounting_scope();
    let evaluation = reducer.inj_with_observation(parsed.clone(), rand());
    let ((result, _), transient) = match heap {
        Some(heap) => heap.transient(evaluation).await,
        None => (evaluation.await, 0),
    };
    drop(scope);
    let recording = budget
        .native_budget_recording()
        .expect("the play recording reads")
        .expect("the play has a budget recording");
    let operations = budget
        .native_operation_recording()
        .expect("the play journal reads")
        .expect("the play has an operation journal");
    let log = play.create_soft_checkpoint().await.log;
    play.create_checkpoint().await.expect("the post-state");
    let played = Played {
        term: parsed,
        result,
        recording,
        operations,
        log,
        history,
    };
    (played, transient, (reducer, budget))
}

/// Replays `played` through the native replay session. The session must
/// complete every recorded operation. With a `heap`, it measures the
/// transient heap of the evaluation alone. The result keeps the replay
/// environment.
async fn native_replay(
    played: &Played,
    heap: Option<&'static LiveHeap>,
) -> (EvaluateResult, usize, impl Sized) {
    let trace = with_contract(LIMIT, WEIGHTS, |contract| {
        contract.check_operation_journal(
            played.recording.session,
            &played.recording,
            Arc::clone(&played.operations),
            journal_limits(),
        )
    })
    .expect("the play journal checks")
    .bind_trace(played.log.clone().into(), TRACE_LIMITS)
    .expect("the play trace binds");
    let mut environment = create_native_replay_env(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        funded_budget(),
        ExternalServices::noop(),
    )
    .await
    .expect("the replay environment starts");
    let evaluation = environment.evaluate(played.term.clone(), rand());
    let (replayed, transient) = match heap {
        Some(heap) => heap.transient(evaluation).await,
        None => (evaluation.await, 0),
    };
    let replayed = replayed.expect("the native replay runs");
    environment
        .check_complete()
        .await
        .expect("the replay completes every recorded operation");
    (replayed, transient, environment)
}

/// Plays `term` after `fill` on a new space, and then replays the play. Play
/// and replay each run on a measured runtime. Each fill deploy is played
/// before the measured play, so its data are in the pre-state of the play and
/// of the replay.
fn measured_play_and_replay(fill: &[String], term: &str) -> (Played, usize, EvaluateResult, usize) {
    let (played, play_heap, retained) = with_live_heap(WORKERS, |heap| async move {
        let (play, stores) = empty_space().await;
        for deploy in fill {
            let (filled, _, _) = native_play(&play, deploy, None).await;
            filled.result.expect("the fill deploy succeeds");
        }
        let (played, transient, reducer) = native_play(&play, term, Some(heap)).await;
        (played, transient, (play, stores, reducer))
    });
    let played_ref = &played;
    let (replayed, replay_heap, environment) = with_live_heap(WORKERS, |heap| async move {
        native_replay(played_ref, Some(heap)).await
    });
    drop((retained, environment));
    (played, play_heap, replayed, replay_heap)
}

/// Asserts that replay accepts `played`, uses its phlo, and that neither play
/// nor replay held `TRANSIENT_LIMIT_BYTES` of transient heap.
fn assert_bounded_heap(what: &str, fill: &[String], term: &str) {
    let (played, play_heap, replayed, replay_heap) = measured_play_and_replay(fill, term);
    eprintln!("{what}: play {play_heap} transient bytes, replay {replay_heap} transient bytes");
    assert!(
        played.result.is_ok(),
        "{what}: play failed: {:?}",
        played.result
    );
    assert!(
        replayed.errors.is_empty(),
        "{what}: replay errors: {:?}",
        replayed.errors
    );
    assert!(played.recording.used > 0, "{what}: play charges phlo");
    assert_eq!(
        replayed.native_phlo_usage,
        Some(played.recording.used),
        "{what}: replay uses the phlo of play"
    );
    assert!(
        play_heap < TRANSIENT_LIMIT_BYTES,
        "{what}: play held {play_heap} transient bytes"
    );
    assert!(
        replay_heap < TRANSIENT_LIMIT_BYTES,
        "{what}: replay held {replay_heap} transient bytes"
    );
}

/// The native variant of dev's
/// `tail_recursive_contract_play_and_replay_do_not_hold_per_iteration_heap`.
/// Each body of the contract runs as a task of the per-call scope.
#[test]
fn native_tail_recursion_holds_no_heap_for_each_iteration() {
    let term = format!(
        "new loop in {{ contract loop(@n) = {{ if (n <= 0) {{ Nil }} else {{ loop!(n - 1) }} }} | loop!({ITERATIONS}) }}"
    );
    assert_bounded_heap("tail recursion", &[], &term);
}

/// F2: a system process replies to a persistent receive, and the body of the
/// receive calls the system process again. On dev the dispatcher evaluates a
/// body inline, so each reply would run the next body inside the previous
/// system-process call. `eval_continuation` runs each body as a task of the
/// per-call scope, so the chain does not nest.
#[test]
fn native_system_process_reply_chain_holds_no_heap_for_each_reply() {
    let term = format!(
        r#"new hash(`rho:crypto:blake2b256Hash`), reply, count in {{
  count!({ITERATIONS}) |
  for (@digest <= reply) {{
    for (@n <- count) {{ if (n > 0) {{ count!(n - 1) | hash!(digest, *reply) }} }}
  }} |
  hash!("seed".toUtf8Bytes(), *reply)
}}"#
    );
    assert_bounded_heap("system-process reply chain", &[], &term);
}

/// F3: a persistent consume over stored data re-issues itself after each
/// COMM. `run_parallel_dispatches` runs each re-issue as a task of the
/// per-call scope and returns, so no re-issue parks its parent.
#[test]
fn native_persistent_consume_over_stored_data_holds_no_heap_for_each_datum() {
    let fill: Vec<String> = (0..STORED_DATA)
        .step_by(FILL_CHUNK)
        .map(|start| {
            (start..(start + FILL_CHUNK).min(STORED_DATA))
                .map(|index| format!("@\"data\"!({index})"))
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect();
    assert_bounded_heap(
        "persistent consume over stored data",
        &fill,
        "for (_ <= @\"data\") { Nil }",
    );
}

/// The native variant of dev's
/// `fan_out_play_and_replay_charge_the_same_cost_in_every_round`: racing
/// play and native replay charge the same phlo, and every round charges the
/// same phlo.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_fan_out_play_and_replay_charge_the_same_phlo_in_every_round() {
    let sends = (0..16)
        .map(|i| format!("loop!({})", 200 + i))
        .collect::<Vec<_>>()
        .join(" | ");
    let term = format!(
        "new loop, done in {{ contract loop(@n) = {{ if (n <= 0) {{ done!(n) }} else {{ loop!(n - 1) }} }} | for (_ <= done) {{ Nil }} | {sends} }}"
    );
    let mut phlo = Vec::with_capacity(3);
    for _ in 0..3 {
        let (play, _stores) = empty_space().await;
        let (played, _, _reducer) = native_play(&play, &term, None).await;
        assert!(played.result.is_ok(), "play failed: {:?}", played.result);
        let (replayed, _, _environment) = native_replay(&played, None).await;
        assert!(
            replayed.errors.is_empty(),
            "replay errors: {:?}",
            replayed.errors
        );
        assert_eq!(replayed.native_phlo_usage, Some(played.recording.used));
        phlo.push(played.recording.used);
    }
    assert!(
        phlo.windows(2).all(|pair| pair[0] == pair[1]),
        "fan-out phlo differs between rounds: {phlo:?}"
    );
}

/// The native variant of dev's
/// `error_in_deferred_body_is_reported_by_the_evaluation`: a failure in a
/// body that runs as a task of the scope reaches the result of play, and
/// replay reports the same failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_error_in_a_deferred_body_is_reported_by_play_and_replay() {
    let term = "new c in { contract c(@x) = { if (x) { Nil } else { Nil } } | c!(1) }";
    let (play, _stores) = empty_space().await;
    let (played, _, _reducer) = native_play(&play, term, None).await;
    let error = match &played.result {
        Err(error @ InterpreterError::IfConditionTypeError { .. }) => error.clone(),
        other => panic!("unexpected play result: {other:?}"),
    };
    let (replayed, _, _environment) = native_replay(&played, None).await;
    assert_eq!(replayed.errors, vec![error]);
    assert_eq!(replayed.native_phlo_usage, Some(played.recording.used));
}

/// The native variant of dev's `dropped_evaluation_aborts_deferred_dispatches`:
/// when the play is dropped, the per-call scope closes and aborts its tasks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_dropped_play_aborts_its_deferred_tasks() {
    let (play, _stores) = empty_space().await;
    let budget = funded_budget();
    let (reducer, _, _, _) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .expect("the play environment starts");
    let parsed = Compiler::source_to_adt(
        "new loop in { contract loop(@n) = { if (n <= 0) { Nil } else { loop!(n - 1) } } | loop!(1000000000) }",
    )
    .expect("the term parses");
    reducer.reset_eval_work_stats();
    let scope = budget.enter_comm_accounting_scope();
    let evaluation = reducer.inj_with_observation(parsed, rand());
    assert!(
        tokio::time::timeout(Duration::from_millis(500), evaluation)
            .await
            .is_err(),
        "the loop must still run when the play is dropped"
    );
    drop(scope);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let stopped_at = reducer.eval_work_stats().single_term_evaluations;
    assert!(stopped_at > 0, "the loop did not start");

    tokio::time::sleep(Duration::from_millis(500)).await;
    let later = reducer.eval_work_stats().single_term_evaluations;
    assert_eq!(
        later, stopped_at,
        "deferred tasks kept running after the play was dropped"
    );
}
