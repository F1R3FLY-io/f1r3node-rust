// P4 integration tests of the replay of space operations (P4 design D.5, the leaf
// rrs-consensus-replay-consult, readiness review S10).
//
// Play and replay run on one in-memory store, as a validator and a block producer share a
// history. Each test plays deploys with space operations, then replays them from the same start
// root with the play logs, and requires the same roots, the same costs and a replay that
// consumes the complete logs. Two protocols run, as in the ordinary regression suite:
//
// * Protocol D creates a checkpoint after each deploy, in play and in replay.
// * Protocol B is the block protocol of Casper. Play takes the event log of each deploy and
//   creates one checkpoint for the block, and replay does the same.
//
// A tampered log must fail with the typed code `ReplayDivergence`, and a reporting runtime must
// replay the space operations like a replay runtime. A deploy that fails, also when it runs out
// of phlo inside a space transaction, must replay to the same error and the same cost (decision
// 36).

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use reified_strategies::{gint, gstring};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
use rholang::rust::interpreter::rho_runtime::{
    create_replay_rho_runtime, RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::hot_store::HotStoreInstances;
use rspace_plus_plus::rspace::reporting_rspace::{ReportingEvent, ReportingRspace};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-replay-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-replay-payer";
const PHLO: i64 = 100_000_000;

/// A deploy that creates a space and two names of it. The handle goes to `@"rrs-handle"` and
/// the names go to `@"rrs-names"`.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    @"rrs-handle"!(*queue) |
    for (a <- queue!?("gensym")) {
      for (b <- queue!?("gensym")) {
        @"rrs-names"!(*a, *b)
      }
    }
  }
}
"#;

/// `body` with the handle of the setup space bound to `queue` and its names bound to `a` and
/// `b`.
fn with_space(body: &str) -> String {
    format!("for (queue <<- @\"rrs-handle\") {{ for (a, b <<- @\"rrs-names\") {{\n{body}\n}} }}")
}

/// A deploy of a test: its label (the source of its random state) and its source.
struct Program {
    label: String,
    source: String,
}

fn program(label: &str, source: String) -> Program {
    Program {
        label: label.to_owned(),
        source,
    }
}

/// The result of one played deploy.
struct Played {
    log: Vec<Event>,
    root: Option<Blake2b256Hash>,
    cost: i64,
}

fn error_codes(errors: &[InterpreterError]) -> Vec<String> {
    let mut codes = Vec::with_capacity(errors.len());
    for error in errors {
        push_codes(error, &mut codes);
    }
    codes
}

fn push_codes(error: &InterpreterError, codes: &mut Vec<String>) {
    match error {
        InterpreterError::AggregateError { interpreter_errors } => {
            for inner in interpreter_errors {
                push_codes(inner, codes);
            }
        }
        InterpreterError::ReduceError(message) => codes.push(
            message
                .strip_prefix("reified:")
                .and_then(|rest| rest.split(':').next())
                .map(str::to_owned)
                .unwrap_or_else(|| message.clone()),
        ),
        other => codes.push(format!("{other:?}")),
    }
}

/// A play runtime, a replay runtime and the history of one in-memory store.
struct Node {
    play: RhoRuntimeImpl,
    replay: RhoRuntimeImpl,
    history: RhoHistoryRepository,
}

async fn node() -> Node {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    let (play, replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
    for runtime in [&play, &replay] {
        fund(runtime);
    }
    Node {
        play,
        replay,
        history,
    }
}

fn fund(runtime: &RhoRuntimeImpl) {
    runtime
        .cost
        .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
}

async fn evaluate(runtime: &RhoRuntimeImpl, program: &Program) -> EvaluateResult {
    evaluate_with_phlo(runtime, program, PHLO).await
}

async fn evaluate_with_phlo(
    runtime: &RhoRuntimeImpl,
    program: &Program,
    phlo: i64,
) -> EvaluateResult {
    evaluate_injected(runtime, program, &HashMap::new(), phlo).await
}

/// The URI of an injected name.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

/// Evaluates `program` with the phlo limit `phlo`. `injections` binds the URIs of the injected
/// names of the program.
async fn evaluate_injected(
    runtime: &RhoRuntimeImpl,
    program: &Program,
    injections: &HashMap<String, Par>,
    phlo: i64,
) -> EvaluateResult {
    runtime
        .evaluate(
            &program.source,
            Cost::create(phlo, program.label.clone()),
            injections.clone(),
            Blake2b512Random::create_from_bytes(program.label.as_bytes()),
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{}: the evaluation returns no result: {error:?}",
                program.label
            )
        })
}

/// Plays `programs` in order. With `per_deploy`, each deploy ends with a checkpoint
/// (protocol D). Otherwise play takes the log of each deploy and the caller creates the block
/// checkpoint (protocol B).
async fn play(runtime: &mut RhoRuntimeImpl, programs: &[Program], per_deploy: bool) -> Vec<Played> {
    let mut played = Vec::with_capacity(programs.len());
    for program in programs {
        let result = evaluate(runtime, program).await;
        assert!(
            result.errors.is_empty(),
            "{}: the play fails: {:?}",
            program.label,
            result.errors
        );
        let (log, root) = match per_deploy {
            true => {
                let checkpoint = runtime.create_checkpoint().await;
                (checkpoint.log, Some(checkpoint.root))
            }
            false => (runtime.take_event_log().await, None),
        };
        played.push(Played {
            log,
            root,
            cost: result.cost.value,
        });
    }
    played
}

/// Replays `programs` with the logs of `played`, from the current root of `runtime`. It checks
/// the costs, the complete use of each log, and with `per_deploy` the root of each deploy.
async fn replay(
    runtime: &mut RhoRuntimeImpl,
    programs: &[Program],
    played: &[Played],
    per_deploy: bool,
) {
    for (program, deploy) in programs.iter().zip(played) {
        runtime
            .rig(deploy.log.clone())
            .await
            .expect("rig the replay runtime with the play log");
        let result = evaluate(runtime, program).await;
        assert!(
            result.errors.is_empty(),
            "{}: the replay fails: {:?}",
            program.label,
            result.errors
        );
        runtime.take_event_log().await;
        runtime.check_replay_data().await.unwrap_or_else(|error| {
            panic!("{}: the replay leaves events: {error:?}", program.label)
        });
        assert_eq!(
            result.cost.value, deploy.cost,
            "{}: the replay cost",
            program.label
        );
        if per_deploy {
            let root = runtime.create_checkpoint().await.root;
            runtime.take_event_log().await;
            assert_eq!(
                Some(root),
                deploy.root.clone(),
                "{}: the replay root",
                program.label
            );
        }
    }
}

/// Plays and replays `programs` under protocol D and protocol B, each on a fresh node, and
/// returns the play logs of protocol D.
async fn play_replay_equal(programs: &[Program]) -> Vec<Vec<Event>> {
    let mut deploys = node().await;
    let start = deploys.play.get_root().await;
    let played = play(&mut deploys.play, programs, true).await;
    deploys
        .replay
        .reset(&start)
        .await
        .expect("reset the replay runtime to the start root");
    replay(&mut deploys.replay, programs, &played, true).await;
    let mut block = node().await;
    let start = block.play.get_root().await;
    let played_block = play(&mut block.play, programs, false).await;
    let block_checkpoint = block.play.create_checkpoint().await;
    assert!(
        block_checkpoint.log.is_empty(),
        "play took the log of each deploy"
    );
    block
        .replay
        .reset(&start)
        .await
        .expect("reset the replay runtime to the start root");
    replay(&mut block.replay, programs, &played_block, false).await;
    let replay_root = block.replay.create_checkpoint().await.root;
    assert_eq!(
        replay_root, block_checkpoint.root,
        "the block root of the replay"
    );
    assert_eq!(
        Some(block_checkpoint.root),
        played.last().and_then(|deploy| deploy.root.clone()),
        "both protocols reach one post-state"
    );
    played.into_iter().map(|deploy| deploy.log).collect()
}

/// The space of the setup of a play runtime.
async fn setup_space(runtime: &RhoRuntimeImpl) -> (NativeSpace, Vec<Par>) {
    let handle = runtime.get_data(&gstring("rrs-handle")).await;
    let handle = handle
        .first()
        .and_then(|datum| datum.a.pars.first())
        .expect("the setup stores the handle")
        .clone();
    let space = match classify_space_ref(&handle) {
        Ok(SpaceRef::Native(space)) => space,
        other => panic!("the handle is not a native space: {other:?}"),
    };
    let names = runtime.get_data(&gstring("rrs-names")).await;
    let names = names
        .first()
        .expect("the setup stores the names")
        .a
        .pars
        .clone();
    (space, names)
}

fn data_channel(space: NativeSpace, channel: &Par) -> Par {
    let key = CollectionKey::of_channel(channel).expect("a channel has a collection key");
    namespace::gprivate(namespace::data_channel(space, &key))
}

fn counter_channel(space: NativeSpace, channel: &Par) -> Par {
    let key = CollectionKey::of_channel(channel).expect("a channel has a collection key");
    namespace::gprivate(namespace::collection_counter(space, &key))
}

/// The number of COMMs of `log` whose consume is on `channel`.
fn comms_on(log: &[Event], channel: &Par) -> usize {
    let key = hash(channel);
    log.iter().filter(|event| is_comm_on(event, &key)).count()
}

/// The number of space COMMs of `log`: COMMs whose consume is on a Reified channel of
/// `channels`.
fn space_comms(log: &[Event], channels: &[Par]) -> usize {
    channels.iter().map(|channel| comms_on(log, channel)).sum()
}

async fn values(runtime: &RhoRuntimeImpl, channel: &str) -> Vec<Vec<Par>> {
    runtime
        .get_data(&gstring(channel))
        .await
        .into_iter()
        .map(|datum| datum.a.pars)
        .collect()
}

/// The step of the phlo sweep, in phlo.
const SWEEP_STEP: usize = 37;

/// The phlo limit of the divergent pair of the sweep.
const DIVERGENT_PHLO: i64 = 100_000;

/// A deploy as Casper compares its play and its replay: the error codes, which are empty for a
/// successful deploy, and the cost.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    codes: Vec<String>,
    cost: i64,
}

impl Outcome {
    fn of(result: &EvaluateResult) -> Self {
        Self {
            codes: error_codes(&result.errors),
            cost: result.cost.value,
        }
    }
}

/// A deploy of the sweep: its program, its injected names, and the root that each of its plays
/// and replays starts from.
struct SweepDeploy<'a> {
    program: &'a Program,
    injections: &'a HashMap<String, Par>,
    root: &'a Blake2b256Hash,
}

/// The plays of the sweep by phlo limit: the outcome and the event log of each.
type Plays = BTreeMap<i64, (Outcome, Vec<Event>)>;

/// Plays `deploy` with the limit `phlo` as Casper plays a user deploy: a soft checkpoint, the
/// evaluation, the event log, and the revert of a failed deploy.
async fn play_limit(
    runtime: &mut RhoRuntimeImpl,
    deploy: &SweepDeploy<'_>,
    phlo: i64,
) -> (Outcome, Vec<Event>) {
    runtime
        .reset(deploy.root)
        .await
        .expect("reset the play runtime to the seeded root");
    let fallback = runtime.create_soft_checkpoint().await;
    let result = evaluate_injected(runtime, deploy.program, deploy.injections, phlo).await;
    let log = runtime.take_event_log().await;
    if !result.errors.is_empty() {
        runtime.revert_to_soft_checkpoint(fallback).await;
    }
    (Outcome::of(&result), log)
}

/// Replays `deploy` with the limit `phlo` and the play log `log` as Casper replays a user
/// deploy: the rig, a soft checkpoint, the evaluation, and the revert of a failed deploy. It
/// returns the outcome and whether the replay used every recorded event.
async fn replay_limit(
    runtime: &mut RhoRuntimeImpl,
    deploy: &SweepDeploy<'_>,
    phlo: i64,
    log: Vec<Event>,
) -> (Outcome, bool) {
    runtime
        .reset(deploy.root)
        .await
        .expect("reset the replay runtime to the seeded root");
    runtime
        .rig(log)
        .await
        .expect("rig the replay runtime with the play log");
    let fallback = runtime.create_soft_checkpoint().await;
    let result = evaluate_injected(runtime, deploy.program, deploy.injections, phlo).await;
    runtime.take_event_log().await;
    if !result.errors.is_empty() {
        runtime.revert_to_soft_checkpoint(fallback).await;
    }
    let complete = runtime.check_replay_data().await.is_ok();
    (Outcome::of(&result), complete)
}

/// The error codes of the play of `deploy` with the limit `phlo`. Each limit plays once, and
/// `plays` keeps its outcome and its log.
async fn played_codes(
    plays: &mut Plays,
    runtime: &mut RhoRuntimeImpl,
    deploy: &SweepDeploy<'_>,
    phlo: i64,
) -> Vec<String> {
    if let Some((outcome, _)) = plays.get(&phlo) {
        return outcome.codes.clone();
    }
    let played = play_limit(runtime, deploy, phlo).await;
    let codes = played.0.codes.clone();
    plays.insert(phlo, played);
    codes
}

/// A limit in `(low, high]` at which the outcome of play changes: the codes of play at the
/// limit differ from its codes one phlo below the limit. Play has other codes at `low` than at
/// `high`, and the bisection keeps that true of its bounds.
async fn outcome_change(
    plays: &mut Plays,
    runtime: &mut RhoRuntimeImpl,
    deploy: &SweepDeploy<'_>,
    mut low: i64,
    mut high: i64,
) -> i64 {
    let below = played_codes(plays, runtime, deploy, low).await;
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        match played_codes(plays, runtime, deploy, middle).await == below {
            true => low = middle,
            false => high = middle,
        }
    }
    high
}

mod space_replay_consult {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_equal() {
        // The waiters and the drained records use their own collections, so the persistent
        // receive and the persistent send never meet as a divergent pair.
        let mut parks = String::from("use queue { ");
        for _ in 0..FIRE_ROUND_CAP + 2 {
            parks.push_str(r#"for (@v <- @"capped") { @"rrs-capped"!(v) } | "#);
        }
        parks.push_str("Nil }");
        let programs = vec![
            program("rrs-p4-replay-setup", SETUP.to_owned()),
            program(
                "rrs-p4-replay-enqueue",
                with_space("a!(1) | a!(2) | b!(10)"),
            ),
            program(
                "rrs-p4-replay-dequeue",
                with_space(r#"for (@x <- a) { for (@y <- a) { @"rrs-out"!(x, y) } } | a!(3)"#),
            ),
            program(
                "rrs-p4-replay-persistent",
                with_space(
                    r#"b!!(20) | for (@z <- b) { @"rrs-b"!(z) } | for (@w <- b) { @"rrs-b"!(w) }"#,
                ),
            ),
            program(
                "rrs-p4-replay-drain-fill",
                with_space(r#"use queue { @"drain"!(1) | @"drain"!(2) | @"drain"!(3) }"#),
            ),
            program(
                "rrs-p4-replay-drain",
                with_space(r#"use queue { for (@d <= @"drain") { @"rrs-drained"!(d) } }"#),
            ),
            program("rrs-p4-replay-capped-park", with_space(&parks)),
            program(
                "rrs-p4-replay-capped-send",
                with_space(r#"use queue { @"capped"!!(8) }"#),
            ),
            program(
                "rrs-p4-replay-two-spaces",
                with_space(
                    r#"
new factory(`rho:space:hashmap:queue`) in {
  for (other <- factory!?("default")) {
    for (c <- other!?("gensym")) {
      c!(1) | c!(2) | b!(3) | b!(4) | for (@x <- c) { b!(x) } | for (@y <- b) { c!(y) } |
      use other { @"jobs"!(5) | for (@z <- @"jobs") { b!(z) } }
    }
  }
}
"#,
                ),
            ),
        ];
        let logs = play_replay_equal(&programs).await;
        // The replay is not vacuous: the play logs hold space COMMs.
        let mut runtime = node().await;
        play(&mut runtime.play, &programs[..1], true).await;
        let (space, names) = setup_space(&runtime.play).await;
        let mut channels: Vec<Par> = names
            .iter()
            .flat_map(|name| [data_channel(space, name), counter_channel(space, name)])
            .collect();
        channels.push(namespace::gprivate(namespace::space_counter(space)));
        for name in [gstring("drain"), gstring("capped")] {
            channels.push(data_channel(space, &name));
            channels.push(counter_channel(space, &name));
        }
        for (program, log) in programs.iter().zip(&logs).skip(1) {
            assert!(
                space_comms(log, &channels) >= 1,
                "{}: the deploy logs space COMMs",
                program.label
            );
        }
        assert_eq!(
            comms_on(&logs[5], &data_channel(space, &gstring("drain"))),
            3,
            "the persistent receive drains the three records in one operation"
        );
        let capped = gstring("capped");
        assert_eq!(
            comms_on(&logs[7], &data_channel(space, &capped)),
            FIRE_ROUND_CAP + 2,
            "the capped send fires every waiter"
        );
        assert_eq!(
            comms_on(&logs[7], &counter_channel(space, &capped)),
            2,
            "the capped send re-issues once"
        );
        // A failed deploy replays to the same failure and the same cost: Casper replays every
        // deploy of a block and rejects the block on a status or cost mismatch
        // (casper/src/rust/rholang/replay_runtime.rs, `run_user_deploy`). The first deploy
        // aborts after space operations. The second is a divergent pair that runs until the phlo
        // is spent, so its last space transaction fails at commit.
        let failing = [
            (
                program(
                    "rrs-p4-replay-abort",
                    with_space(
                        r#"for (_ <- queue!?("produce", *a, [1])) { a!(2) | @"rrs-x"!(3; k=1) }"#,
                    ),
                ),
                PHLO,
                "ParamsUnsupported",
            ),
            (
                program(
                    "rrs-p4-replay-divergent",
                    with_space("for (@x <= b) { Nil } | b!!(1)"),
                ),
                100_000,
                "OutOfPhlogistonsError",
            ),
        ];
        for (failing, phlo, code) in failing {
            let mut deploys = node().await;
            play(&mut deploys.play, &programs[..2], true).await;
            let root = deploys.play.get_root().await;
            let fallback = deploys.play.create_soft_checkpoint().await;
            let played = evaluate_with_phlo(&deploys.play, &failing, phlo).await;
            let log = deploys.play.take_event_log().await;
            deploys.play.revert_to_soft_checkpoint(fallback).await;
            assert_eq!(error_codes(&played.errors), vec![code], "{}", failing.label);
            deploys
                .replay
                .reset(&root)
                .await
                .expect("reset the replay runtime");
            deploys
                .replay
                .rig(log)
                .await
                .expect("rig the replay runtime");
            let replayed = evaluate_with_phlo(&deploys.replay, &failing, phlo).await;
            assert!(
                !replayed.errors.is_empty(),
                "{}: the replay fails as the play failed",
                failing.label
            );
            assert_eq!(
                replayed.cost.value,
                played.cost.value,
                "{}: the replay charges the play cost (replay errors: {:?})",
                failing.label,
                error_codes(&replayed.errors)
            );
            assert_eq!(
                error_codes(&replayed.errors),
                vec![code],
                "{}: the replay fails the same way",
                failing.label
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_equal_handle_calls() {
        let calls = with_space(
            r#"
for (n <- queue!?("gensym")) {
  for (@c <- queue!?("config")) {
    for (@k <- queue!?("qualifier")) {
      for (@e <- queue!?("frobnicate")) {
        for (@p <- queue!?("produce", *a, [1, 2])) {
          for (@x, @y <- a) {
            @"rrs-out"!(k, e, p, x, y) | n!(c)
          }
        }
      }
    }
  }
} |
new ret in { queue!(1, "gensym") | queue!(*ret, ["gensym"]) } |
new spaceAgent(`rho:space:getSpaceAgent`), equal(`rho:assert:assertEqual`) in {
  for (@capability <- spaceAgent!?(*queue)) {
    for (@same <- equal!?(capability, capability)) { @"rrs-system"!(same) }
  }
}
"#,
        );
        let programs = vec![
            program("rrs-p4-replay-calls-setup", SETUP.to_owned()),
            program("rrs-p4-replay-calls", calls),
            program(
                "rrs-p4-replay-calls-persistent",
                with_space(
                    r#"for (_ <- queue!?("produce", *b, [3]; persistent=true)) { for (@v <- b) { @"rrs-v"!(v) } }"#,
                ),
            ),
        ];
        let logs = play_replay_equal(&programs).await;
        let handle_op = namespace::gprivate(namespace::op_channel(namespace::HANDLE_OP));
        assert!(
            comms_on(&logs[1], &handle_op) >= 7,
            "the calls reach HANDLE_OP: {}",
            comms_on(&logs[1], &handle_op)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_peek_and_parked() {
        // Continuations parked by an earlier deploy fire in a later deploy, peeks keep their
        // records, and a produce on a peek waiter offers its datum again (E4 to E7).
        let programs = vec![
            program("rrs-p4-replay-parked-setup", SETUP.to_owned()),
            program(
                "rrs-p4-replay-parked-park",
                with_space(r#"for (@x <<- a) { @"rrs-peeked"!(x) }"#),
            ),
            program(
                "rrs-p4-replay-parked-park-2",
                with_space(
                    r#"for (@y <- a) { @"rrs-taken"!(y) } | for (@z <= b) { @"rrs-always"!(z) }"#,
                ),
            ),
            program(
                "rrs-p4-replay-parked-wake",
                with_space("a!(1) | b!(2) | b!(3)"),
            ),
            program("rrs-p4-replay-parked-store", with_space("a!(4) | a!(5)")),
            program(
                "rrs-p4-replay-parked-peek",
                with_space(
                    r#"for (@p <<- a) { @"rrs-peek-a"!(p) } | for (@q <<- a) { @"rrs-peek-b"!(q) }"#,
                ),
            ),
        ];
        play_replay_equal(&programs).await;
        // The play itself delivered every value.
        let mut runtime = node().await;
        play(&mut runtime.play, &programs, true).await;
        assert_eq!(values(&runtime.play, "rrs-peeked").await, vec![vec![gint(
            1
        )]]);
        assert_eq!(values(&runtime.play, "rrs-taken").await, vec![vec![gint(
            1
        )]]);
        let mut always: Vec<Vec<Par>> = values(&runtime.play, "rrs-always").await;
        always.sort_by_key(|pars| format!("{pars:?}"));
        assert_eq!(always, vec![vec![gint(2)], vec![gint(3)]]);
        assert_eq!(values(&runtime.play, "rrs-peek-a").await.len(), 1);
        assert_eq!(values(&runtime.play, "rrs-peek-b").await.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_divergence_typed() {
        let mut runtime = node().await;
        let setup = [program("rrs-p4-divergence-setup", SETUP.to_owned())];
        play(&mut runtime.play, &setup, true).await;
        let (space, names) = setup_space(&runtime.play).await;
        let a = names.first().expect("the setup makes names").clone();
        let seed = [program(
            "rrs-p4-divergence-seed",
            with_space("a!(1) | a!(2)"),
        )];
        let seeded = play(&mut runtime.play, &seed, true).await;
        let root = seeded
            .last()
            .and_then(|deploy| deploy.root.clone())
            .expect("the seed has a root");
        let dequeue = [program(
            "rrs-p4-divergence-dequeue",
            with_space(r#"for (@x <- a) { @"rrs-x"!(x) }"#),
        )];
        let played = play(&mut runtime.play, &dequeue, true).await;
        let log = played.first().expect("one played deploy").log.clone();
        assert_eq!(comms_on(&log, &data_channel(space, &a)), 1);
        assert_eq!(comms_on(&log, &counter_channel(space, &a)), 1);
        // The unchanged log replays.
        runtime
            .replay
            .reset(&root)
            .await
            .expect("reset the replay runtime");
        replay(&mut runtime.replay, &dequeue, &played, true).await;
        // A log without the COMM of the data channel, or without the COMM of the counter
        // bump, makes the space operation diverge. The replay names the divergence with its
        // own code. The divergence comes at commit, and the operations before the unpaired fire
        // stay applied (decision 36). Without the data COMM, the bump before the data fire
        // pairs, so the replay uses every recorded event. Without the bump COMM, the
        // transaction fails at the bump, so the data COMM stays unused.
        for (tampered, leaves_unused) in [
            (data_channel(space, &a), false),
            (counter_channel(space, &a), true),
        ] {
            let key = hash(&tampered);
            let log: Vec<Event> = log
                .iter()
                .filter(|event| !is_comm_on(event, &key))
                .cloned()
                .collect();
            runtime
                .replay
                .reset(&root)
                .await
                .expect("reset the replay runtime");
            runtime
                .replay
                .rig(log)
                .await
                .expect("rig the replay runtime with the tampered log");
            let result = evaluate(&runtime.replay, &dequeue[0]).await;
            assert_eq!(
                error_codes(&result.errors),
                vec!["ReplayDivergence"],
                "a missing space COMM is a typed divergence"
            );
            assert_eq!(
                runtime.replay.check_replay_data().await.is_err(),
                leaves_unused,
                "the tampered replay leaves recorded events unused exactly when the bump COMM is \
                 missing"
            );
            runtime.replay.take_event_log().await;
        }
        // A replay of another program with the same log also fails: the host pairing leaves
        // the recorded events unused.
        runtime
            .replay
            .reset(&root)
            .await
            .expect("reset the replay runtime");
        runtime
            .replay
            .rig(log)
            .await
            .expect("rig the replay runtime");
        let other = program(
            "rrs-p4-divergence-dequeue",
            with_space(r#"for (@x <- a) { @"rrs-y"!(x) }"#),
        );
        evaluate(&runtime.replay, &other).await;
        assert!(runtime.replay.check_replay_data().await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_out_of_phlo_sweep_replays_equal() {
        // Negative control of decision 36. Casper replays each deploy of a block and rejects the
        // block when the failure or the cost of a deploy differs from play
        // (casper/src/rust/rholang/replay_runtime.rs, `run_user_deploy`). The sweep plays the
        // target deploy with each limit from 0 to its full cost in steps of 37, and with the
        // limits L-1, L and L+1 at each limit L where the outcome of play changes. The deploy
        // names the space through injected names, so it makes no host COMM: a mismatch can only
        // come from the replay of its space transactions, not from host replay.
        let mut node = node().await;
        let setup = [
            program("rrs-p4-sweep-setup", SETUP.to_owned()),
            program(
                "rrs-p4-sweep-seed",
                with_space(r#"for (@w <- b) { @"rrs-w"!(w) } | a!(0)"#),
            ),
        ];
        let seeded = play(&mut node.play, &setup, true).await;
        let root = seeded
            .last()
            .and_then(|deploy| deploy.root.clone())
            .expect("the seed has a root");
        let (_, names) = setup_space(&node.play).await;
        let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
        let injections = HashMap::from([(injected("a"), a), (injected("b"), b)]);
        let target = program(
            "rrs-p4-sweep-target",
            format!(
                "new a(`{}`), b(`{}`) in {{ b!(1) | a!(2) }}",
                injected("a"),
                injected("b")
            ),
        );
        let deploy = SweepDeploy {
            program: &target,
            injections: &injections,
            root: &root,
        };
        let (full, _) = play_limit(&mut node.play, &deploy, PHLO).await;
        assert!(
            full.codes.is_empty(),
            "the target deploy succeeds with enough phlo: {:?}",
            full.codes
        );

        let mut plays = Plays::new();
        let mut limits: Vec<i64> = (0..=full.cost).step_by(SWEEP_STEP).collect();
        if limits.last() != Some(&full.cost) {
            limits.push(full.cost);
        }
        for limit in &limits {
            played_codes(&mut plays, &mut node.play, &deploy, *limit).await;
        }
        let mut cases: BTreeSet<i64> = limits.iter().copied().collect();
        let mut changes = Vec::new();
        for pair in limits.windows(2) {
            let [low, high] = [pair[0], pair[1]];
            let changed = played_codes(&mut plays, &mut node.play, &deploy, low).await
                != played_codes(&mut plays, &mut node.play, &deploy, high).await;
            if changed {
                let change = outcome_change(&mut plays, &mut node.play, &deploy, low, high).await;
                for limit in [change - 1, change, change + 1] {
                    played_codes(&mut plays, &mut node.play, &deploy, limit).await;
                    cases.insert(limit);
                }
                changes.push(change);
            }
        }
        assert!(
            changes.contains(&full.cost),
            "play succeeds from the full cost and fails one phlo below it: changes {changes:?}, \
             full cost {}",
            full.cost
        );

        let mut mismatches = Vec::new();
        let mut failed = 0usize;
        for limit in &cases {
            let (played, log) = plays.remove(limit).expect("every case is played");
            if !played.codes.is_empty() {
                failed += 1;
            }
            let (replayed, complete) = replay_limit(&mut node.replay, &deploy, *limit, log).await;
            if replayed != played || (played.codes.is_empty() && !complete) {
                mismatches.push(format!(
                    "{} at {limit}: play {played:?}, replay {replayed:?}, every event used: \
                     {complete}",
                    target.label
                ));
            }
        }
        assert!(failed > 0, "the sweep plays the target deploy out of phlo");

        // The divergent pair of `space_queue_replay_equal` runs until its phlo is spent.
        let divergent = program(
            "rrs-p4-sweep-divergent",
            format!(
                "new b(`{}`) in {{ for (@x <= b) {{ Nil }} | b!!(1) }}",
                injected("b")
            ),
        );
        let deploy = SweepDeploy {
            program: &divergent,
            injections: &injections,
            root: &root,
        };
        let (played, log) = play_limit(&mut node.play, &deploy, DIVERGENT_PHLO).await;
        assert_eq!(
            played.codes,
            vec!["OutOfPhlogistonsError"],
            "the divergent pair runs until the phlo is spent"
        );
        let (replayed, _) = replay_limit(&mut node.replay, &deploy, DIVERGENT_PHLO, log).await;
        if replayed != played {
            mismatches.push(format!(
                "{} at {DIVERGENT_PHLO}: play {played:?}, replay {replayed:?}",
                divergent.label
            ));
        }
        assert!(
            mismatches.is_empty(),
            "{} of {} deploys replay to another outcome than play ({failed} sweep limits fail \
             in play):\n{}",
            mismatches.len(),
            cases.len() + 1,
            mismatches.join("\n")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_reporting_equal() {
        let mut runtime = node().await;
        let setup = [program("rrs-p4-reporting-setup", SETUP.to_owned())];
        play(&mut runtime.play, &setup, true).await;
        let (space, names) = setup_space(&runtime.play).await;
        let a = names.first().expect("the setup makes names").clone();
        let root = runtime.play.get_root().await;
        let deploys = [program(
            "rrs-p4-reporting-ops",
            with_space(
                r#"a!(1) | for (@x <- a) { @"rrs-x"!(x) } | for (@p <<- b) { Nil } | b!(2) |
for (@k <- queue!?("qualifier")) { @"rrs-k"!(k) }"#,
            ),
        )];
        let played = play(&mut runtime.play, &deploys, true).await;
        let deploy = played.first().expect("one played deploy");
        // A reporting runtime over the same history replays the deploy.
        let reader = runtime
            .history
            .get_history_reader(&root)
            .expect("read the history at the root");
        let reporting = ReportingRspace::apply(
            runtime.history.clone(),
            Arc::new(HotStoreInstances::create_from_hr(reader.base())),
            Arc::new(Box::new(Matcher)),
        );
        let mut reporter = create_replay_rho_runtime(
            reporting.clone(),
            Arc::new(default_mergeable_tags()),
            false,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        fund(&reporter);
        reporter
            .reset(&root)
            .await
            .expect("reset the reporting runtime");
        replay(&mut reporter, &deploys, &played, false).await;
        // The report holds every COMM of the play log, the space COMMs with their routed
        // channels and their records included. A checkpoint clears the report, so the test
        // reads the report first.
        let report = reporting.get_report().expect("read the report");
        let reported_root = reporter.create_checkpoint().await.root;
        assert_eq!(
            Some(reported_root),
            deploy.root.clone(),
            "the reporting replay reaches the play root"
        );
        let comms: Vec<_> = report
            .iter()
            .flat_map(|batch| batch.events.iter())
            .filter_map(|event| match event {
                ReportingEvent::ReportingComm(comm) => Some(comm),
                ReportingEvent::ReportingProduce(_) | ReportingEvent::ReportingConsume(_) => None,
            })
            .collect();
        let played_comms = deploy
            .log
            .iter()
            .filter(|event| matches!(event, Event::Comm(_)))
            .count();
        assert_eq!(comms.len(), played_comms, "the report lists every COMM");
        let data = data_channel(space, &a);
        let space_comms: Vec<_> = comms
            .iter()
            .filter(|comm| comm.consume.channels == [data.clone()])
            .collect();
        assert_eq!(space_comms.len(), 1, "the dequeue on a is reported");
        let [delivered] = space_comms[0].produces.as_slice() else {
            panic!("the reported space COMM binds one record");
        };
        // As for a host COMM, the report holds the matched data: the payload without the
        // envelope of the record.
        assert_eq!(delivered.channel, data);
        assert_eq!(delivered.data.pars, vec![gint(1)]);
        assert!(
            record::unwrap_datum(&delivered.data).is_err(),
            "no envelope is reported"
        );
        let produce_count = deploy
            .log
            .iter()
            .filter(|event| matches!(event, Event::IoEvent(IOEvent::Produce(_))))
            .count();
        let reported_produces = report
            .iter()
            .flat_map(|batch| batch.events.iter())
            .filter(|event| matches!(event, ReportingEvent::ReportingProduce(_)))
            .count();
        assert_eq!(
            reported_produces, produce_count,
            "the report lists every produce"
        );
        assert_eq!(values(&reporter, "rrs-x").await, vec![vec![gint(1)]]);
    }
}
