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
//
// Changed by decision 90 (C6, decision 69): spaces have one execution model, native funded
// execution. So each deploy plays natively, and native replay of its own record certifies it
// from its pre-state, as validator replay certifies an offered deploy. Replay must give the
// errors of play, consume every journal row and use the phlo of play. A deploy that succeeds
// must also reach the root of play. The native session of the shared harness
// (`support::session`) runs and certifies the deploys of a test.
//
// Only protocol D has a native form. Casper checkpoints each offered deploy at its settlement
// root, and validator replay starts each deploy at its own execution root
// (`casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs`). So no
// deploy replays from the hot store of an earlier deploy, and protocol B is commented out below.
// A tampered log fails the trace binding of native replay. A reporting replay is a native replay
// whose session logs to a reporting logger, as Casper reports an offered block
// (`casper/src/rust/reporting_casper.rs`, `trace_offered`).

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

// Added by decision 90 (C6): the native session and the native certification of the shared
// harness.
#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
// Changed by decision 90 (C6): the reporting logger of a native replay shares its report.
// use std::sync::Arc;
use std::sync::{Arc, Mutex};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use reified_strategies::{gint, gstring};
// Disabled by decision 90 (C6): the native session funds its deploys and plays them natively.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
// Added by decision 90 (C6): the typed rejection of the trace binding.
use rholang::rust::interpreter::accounting::NativeOperationTraceError;
use rholang::rust::interpreter::errors::InterpreterError;
// Disabled by decision 90 (C6): the shared harness builds the native replay environment.
// use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
// Disabled by decision 90 (C6): the shared harness builds the native replay environment.
// use rholang::rust::interpreter::matcher::r#match::Matcher;
// use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::reified::costs::FIRE_ROUND_CAP;
use rholang::rust::interpreter::reified::scope::{classify_space_ref, SpaceRef};
// Changed by decision 90 (C6): the native session holds the runtime, and a native replay takes
// the reporting logger.
// use rholang::rust::interpreter::rho_runtime::{
//     create_replay_rho_runtime, RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl,
// };
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl, RhoSpaceLogger};
use rholang::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use rholang::rust::interpreter::spaces::record;
// Disabled by decision 90 (C6): the native session builds its runtime in the shared harness.
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
// Disabled by decision 90 (C6): a native replay reads the history of its pre-state.
// use rspace_plus_plus::rspace::hot_store::HotStoreInstances;
// Changed by decision 90 (C6): the reporting logger of a native replay collects the report.
// use rspace_plus_plus::rspace::reporting_rspace::{ReportingEvent, ReportingRspace};
use rspace_plus_plus::rspace::reporting_rspace::{ReportingEvent, ReportingLogger};
// Disabled by decision 90 (C6): the native session builds its runtime in the shared harness.
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
// Added by decision 90 (C6): the native session and the native certification.
use support::native::{
    assert_certified, assert_reproduces, certify, committed_evidence_of, finish_with, rejected,
    replay_environment_from, replay_environment_with_logger, NativeChain, Played, AMPLE_LIMIT,
};
use support::session::{Injections, Session};

/// True when `event` is a COMM whose consume is on the channel with the hash `key`.
fn is_comm_on(event: &Event, key: &Blake2b256Hash) -> bool {
    match event {
        Event::Comm(comm) => comm.consume.channel_hashes.as_slice() == std::slice::from_ref(key),
        Event::IoEvent(_) => false,
    }
}

// Disabled by decision 90 (C6): the native session funds its own deploy signature.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p4-replay-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p4-replay-payer";
// Changed by decision 90 (C6): a deploy plays natively, with the native phlo limit of the suites.
// const PHLO: i64 = 100_000_000;
const PHLO: u64 = AMPLE_LIMIT;

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

// Disabled by decision 90 (C6): a native deploy gives the `Deploy` of the session or the `Played`
// of the native harness.
// /// The result of one played deploy.
// struct Played {
//     log: Vec<Event>,
//     root: Option<Blake2b256Hash>,
//     cost: i64,
// }

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
        // Changed by Reified RSpaces (user decision of 2026-10-25, as DR-128): a user failure of
        // a space is a `SpaceUserError` with the text of its reduce error.
        // InterpreterError::ReduceError(message) => codes.push(
        InterpreterError::ReduceError(message) | InterpreterError::SpaceUserError(message) => codes
            .push(
                message
                    .strip_prefix("reified:")
                    .and_then(|rest| rest.split(':').next())
                    .map(str::to_owned)
                    .unwrap_or_else(|| message.clone()),
            ),
        other => codes.push(format!("{other:?}")),
    }
}

// Changed by decision 90 (decision 69): these helpers served only the retired
// `space_queue_legacy_replay_equal` (Option P), and DR-134 deleted the legacy meter.
//
// /// The labels of the storage charges of `ChargingRSpace` and of the legacy charges of the
// /// space jobs (decision 45, Option P).
// const STORAGE_LABELS: [&str; 6] = [
//     "produces storage",
//     "consume storage",
//     "consume storage refund",
//     "produces storage refund",
//     "event storage cost",
//     "comm event storage cost",
// ];
//
// /// The storage charges of a legacy log, in log order.
// fn storage_charges(log: &[Cost]) -> Vec<Cost> {
//     log.iter()
//         .filter(|charge| STORAGE_LABELS.contains(&charge.operation.as_ref()))
//         .cloned()
//         .collect()
// }
//
// /// Every charge of a legacy log, sorted by label and value.
// fn sorted_charges(log: &[Cost]) -> Vec<(String, i64)> {
//     let mut charges: Vec<(String, i64)> = log
//         .iter()
//         .map(|charge| (charge.operation.to_string(), charge.value))
//         .collect();
//     charges.sort();
//     charges
// }

// Changed by decision 90 (C6): the native session of the shared harness funds, plays and
// certifies each deploy. The legacy node, its funding and its evaluation helpers follow.
//
// /// A play runtime, a replay runtime and the history of one in-memory store.
// struct Node {
//     play: RhoRuntimeImpl,
//     replay: RhoRuntimeImpl,
//     history: RhoHistoryRepository,
// }
//
// async fn node() -> Node {
//     let mut manager = InMemoryStoreManager::new();
//     let stores = manager
//         .r_space_stores()
//         .await
//         .expect("create the in-memory RSpace stores");
//     let (play, replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
//     for runtime in [&play, &replay] {
//         fund(runtime);
//     }
//     Node {
//         play,
//         replay,
//         history,
//     }
// }
//
// fn fund(runtime: &RhoRuntimeImpl) {
//     runtime
//         .cost
//         .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
// }
//
// async fn evaluate(runtime: &RhoRuntimeImpl, program: &Program) -> EvaluateResult {
//     evaluate_with_phlo(runtime, program, PHLO).await
// }
//
// async fn evaluate_with_phlo(
//     runtime: &RhoRuntimeImpl,
//     program: &Program,
//     phlo: i64,
// ) -> EvaluateResult {
//     evaluate_injected(runtime, program, &HashMap::new(), phlo).await
// }

/// The URI of an injected name.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

// Disabled by decision 90 (C6): the native session and the native chain evaluate each deploy
// natively. The legacy evaluation follows.
//
// /// Evaluates `program` with the phlo limit `phlo`. `injections` binds the URIs of the injected
// /// names of the program.
// async fn evaluate_injected(
//     runtime: &RhoRuntimeImpl,
//     program: &Program,
//     injections: &HashMap<String, Par>,
//     phlo: i64,
// ) -> EvaluateResult {
//     runtime
//         .evaluate(
//             &program.source,
//             Cost::create(phlo, program.label.clone()),
//             injections.clone(),
//             Blake2b512Random::create_from_bytes(program.label.as_bytes()),
//         )
//         .await
//         .unwrap_or_else(|error| {
//             panic!(
//                 "{}: the evaluation returns no result: {error:?}",
//                 program.label
//             )
//         })
// }

// Changed by decision 90 (C6): the native session plays each deploy, and native replay of its
// own record certifies it from its pre-state (protocol D). Protocol B has no native form,
// because validator replay starts each offered deploy at its own execution root. The legacy
// play, the rigged replay and the two protocols follow.
//
// /// Plays `programs` in order. With `per_deploy`, each deploy ends with a checkpoint
// /// (protocol D). Otherwise play takes the log of each deploy and the caller creates the block
// /// checkpoint (protocol B).
// async fn play(runtime: &mut RhoRuntimeImpl, programs: &[Program], per_deploy: bool) -> Vec<Played> {
//     let mut played = Vec::with_capacity(programs.len());
//     for program in programs {
//         let result = evaluate(runtime, program).await;
//         assert!(
//             result.errors.is_empty(),
//             "{}: the play fails: {:?}",
//             program.label,
//             result.errors
//         );
//         let (log, root) = match per_deploy {
//             true => {
//                 let checkpoint = runtime.create_checkpoint().await;
//                 (checkpoint.log, Some(checkpoint.root))
//             }
//             false => (runtime.take_event_log().await, None),
//         };
//         played.push(Played {
//             log,
//             root,
//             cost: result.cost.value,
//         });
//     }
//     played
// }
//
// /// Replays `programs` with the logs of `played`, from the current root of `runtime`. It checks
// /// the costs, the complete use of each log, and with `per_deploy` the root of each deploy.
// async fn replay(
//     runtime: &mut RhoRuntimeImpl,
//     programs: &[Program],
//     played: &[Played],
//     per_deploy: bool,
// ) {
//     for (program, deploy) in programs.iter().zip(played) {
//         runtime
//             .rig(deploy.log.clone())
//             .await
//             .expect("rig the replay runtime with the play log");
//         let result = evaluate(runtime, program).await;
//         assert!(
//             result.errors.is_empty(),
//             "{}: the replay fails: {:?}",
//             program.label,
//             result.errors
//         );
//         runtime.take_event_log().await;
//         runtime.check_replay_data().await.unwrap_or_else(|error| {
//             panic!("{}: the replay leaves events: {error:?}", program.label)
//         });
//         assert_eq!(
//             result.cost.value, deploy.cost,
//             "{}: the replay cost",
//             program.label
//         );
//         if per_deploy {
//             let root = runtime.create_checkpoint().await.root;
//             runtime.take_event_log().await;
//             assert_eq!(
//                 Some(root),
//                 deploy.root.clone(),
//                 "{}: the replay root",
//                 program.label
//             );
//         }
//     }
// }
//
// /// Plays and replays `programs` under protocol D and protocol B, each on a fresh node, and
// /// returns the play logs of protocol D.
// async fn play_replay_equal(programs: &[Program]) -> Vec<Vec<Event>> {
//     let mut deploys = node().await;
//     let start = deploys.play.get_root().await;
//     let played = play(&mut deploys.play, programs, true).await;
//     deploys
//         .replay
//         .reset(&start)
//         .await
//         .expect("reset the replay runtime to the start root");
//     replay(&mut deploys.replay, programs, &played, true).await;
//     let mut block = node().await;
//     let start = block.play.get_root().await;
//     let played_block = play(&mut block.play, programs, false).await;
//     let block_checkpoint = block.play.create_checkpoint().await;
//     assert!(
//         block_checkpoint.log.is_empty(),
//         "play took the log of each deploy"
//     );
//     block
//         .replay
//         .reset(&start)
//         .await
//         .expect("reset the replay runtime to the start root");
//     replay(&mut block.replay, programs, &played_block, false).await;
//     let replay_root = block.replay.create_checkpoint().await.root;
//     assert_eq!(
//         replay_root, block_checkpoint.root,
//         "the block root of the replay"
//     );
//     assert_eq!(
//         Some(block_checkpoint.root),
//         played.last().and_then(|deploy| deploy.root.clone()),
//         "both protocols reach one post-state"
//     );
//     played.into_iter().map(|deploy| deploy.log).collect()
// }

/// Added by decision 90 (C6): plays `programs` in order on a new native session. Each deploy
/// must succeed, and the session certifies it by native replay of its own record from its
/// pre-state. Returns the session and the event log of each deploy.
async fn play_replay_equal(programs: &[Program]) -> (Session, Vec<Vec<Event>>) {
    let mut session = Session::new().await;
    let mut logs = Vec::with_capacity(programs.len());
    for program in programs {
        logs.push(session.run(&program.source, &program.label).await.log);
    }
    (session, logs)
}

/// Added by decision 90 (C6): plays `program` natively on `chain` with the phlo limit [`PHLO`].
/// The play must succeed, and native replay must certify it from its pre-state.
async fn play_certified(chain: &mut NativeChain, program: &Program) -> Played {
    let played = chain.play(&program.source, &program.label, PHLO).await;
    assert!(
        played.result.errors.is_empty(),
        "{}: the play fails: {:?}",
        program.label,
        played.result.errors
    );
    let certification = certify(&played, &program.source, PHLO).await;
    assert_reproduces(&program.label, &played, &certification);
    played
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

// Changed by decision 90 (C6): the limit of the divergent pair is a native phlo limit, as in the
// queue suite (`reified_queue.rs`).
// /// The phlo limit of the divergent pair of the sweep.
// const DIVERGENT_PHLO: i64 = 100_000;
/// The native phlo limit of the divergent pair. It stops the endless re-issue after a few rounds.
const DIVERGENT_PHLO: u64 = 20_000;

// Changed by decision 90 (C6): the cost of a native deploy is the phlo usage of its budget
// recording. The legacy outcome follows.
//
// /// A deploy as Casper compares its play and its replay: the error codes, which are empty for a
// /// successful deploy, and the cost.
// #[derive(Clone, Debug, PartialEq, Eq)]
// struct Outcome {
//     codes: Vec<String>,
//     cost: i64,
// }
//
// impl Outcome {
//     fn of(result: &EvaluateResult) -> Self {
//         Self {
//             codes: error_codes(&result.errors),
//             cost: result.cost.value,
//         }
//     }
// }
/// A native deploy as Casper compares its play and its replay: the error codes, which are empty
/// for a successful deploy, and the phlo usage of its budget recording.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    codes: Vec<String>,
    used: u64,
}

impl Outcome {
    fn of(result: &EvaluateResult) -> Self {
        Self {
            codes: error_codes(&result.errors),
            used: result
                .native_budget_recording
                .as_ref()
                .expect("native play records its budget")
                .used,
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

// Changed by decision 90 (C6): the sweep keeps the native play of each limit, which its
// certification replays. The legacy map follows.
//
// /// The plays of the sweep by phlo limit: the outcome and the event log of each.
// type Plays = BTreeMap<i64, (Outcome, Vec<Event>)>;
/// The native plays of the sweep by phlo limit: the outcome and the play of each.
type Plays = BTreeMap<u64, (Outcome, Played)>;

// Changed by decision 90 (C6): the sweep plays natively on a native chain, and native replay
// certifies each case from the seeded root. The legacy play, the rigged replay and the
// bisection over the legacy phlo limit follow.
//
// /// Plays `deploy` with the limit `phlo` as Casper plays a user deploy: a soft checkpoint, the
// /// evaluation, the event log, and the revert of a failed deploy.
// async fn play_limit(
//     runtime: &mut RhoRuntimeImpl,
//     deploy: &SweepDeploy<'_>,
//     phlo: i64,
// ) -> (Outcome, Vec<Event>) {
//     runtime
//         .reset(deploy.root)
//         .await
//         .expect("reset the play runtime to the seeded root");
//     let fallback = runtime.create_soft_checkpoint().await;
//     let result = evaluate_injected(runtime, deploy.program, deploy.injections, phlo).await;
//     let log = runtime.take_event_log().await;
//     if !result.errors.is_empty() {
//         runtime.revert_to_soft_checkpoint(fallback).await;
//     }
//     (Outcome::of(&result), log)
// }
//
// /// Replays `deploy` with the limit `phlo` and the play log `log` as Casper replays a user
// /// deploy: the rig, a soft checkpoint, the evaluation, and the revert of a failed deploy. It
// /// returns the outcome and whether the replay used every recorded event.
// async fn replay_limit(
//     runtime: &mut RhoRuntimeImpl,
//     deploy: &SweepDeploy<'_>,
//     phlo: i64,
//     log: Vec<Event>,
// ) -> (Outcome, bool) {
//     runtime
//         .reset(deploy.root)
//         .await
//         .expect("reset the replay runtime to the seeded root");
//     runtime
//         .rig(log)
//         .await
//         .expect("rig the replay runtime with the play log");
//     let fallback = runtime.create_soft_checkpoint().await;
//     let result = evaluate_injected(runtime, deploy.program, deploy.injections, phlo).await;
//     runtime.take_event_log().await;
//     if !result.errors.is_empty() {
//         runtime.revert_to_soft_checkpoint(fallback).await;
//     }
//     let complete = runtime.check_replay_data().await.is_ok();
//     (Outcome::of(&result), complete)
// }
//
// /// The error codes of the play of `deploy` with the limit `phlo`. Each limit plays once, and
// /// `plays` keeps its outcome and its log.
// async fn played_codes(
//     plays: &mut Plays,
//     runtime: &mut RhoRuntimeImpl,
//     deploy: &SweepDeploy<'_>,
//     phlo: i64,
// ) -> Vec<String> {
//     if let Some((outcome, _)) = plays.get(&phlo) {
//         return outcome.codes.clone();
//     }
//     let played = play_limit(runtime, deploy, phlo).await;
//     let codes = played.0.codes.clone();
//     plays.insert(phlo, played);
//     codes
// }
//
// /// A limit in `(low, high]` at which the outcome of play changes: the codes of play at the
// /// limit differ from its codes one phlo below the limit. Play has other codes at `low` than at
// /// `high`, and the bisection keeps that true of its bounds.
// async fn outcome_change(
//     plays: &mut Plays,
//     runtime: &mut RhoRuntimeImpl,
//     deploy: &SweepDeploy<'_>,
//     mut low: i64,
//     mut high: i64,
// ) -> i64 {
//     let below = played_codes(plays, runtime, deploy, low).await;
//     while high - low > 1 {
//         let middle = low + (high - low) / 2;
//         match played_codes(plays, runtime, deploy, middle).await == below {
//             true => low = middle,
//             false => high = middle,
//         }
//     }
//     high
// }

/// Plays `deploy` natively with the phlo limit `limit` from the seeded root. Native play
/// checkpoints each deploy, so each play first resets the chain to the seeded root. Casper settles
/// a failed deploy at its pre-state root, so no test reads the root of a failed play.
async fn play_limit(
    chain: &mut NativeChain,
    deploy: &SweepDeploy<'_>,
    limit: u64,
) -> (Outcome, Played) {
    chain.reset(deploy.root).await;
    let played = chain
        .play_with(
            &deploy.program.source,
            &deploy.program.label,
            limit,
            deploy.injections.clone(),
        )
        .await;
    (Outcome::of(&played.result), played)
}

/// Certifies `played`, the play of `deploy` with the phlo limit `limit`, by native replay from the
/// seeded root through its committed evidence, as validator replay does. Replay must give the
/// errors of play, consume every journal row and use the phlo of play. A deploy that succeeds
/// must also reach the root of play.
async fn replay_limit(deploy: &SweepDeploy<'_>, limit: u64, played: &Played) {
    let certification = certify(played, &deploy.program.source, limit).await;
    let root = played.result.errors.is_empty().then_some(&played.root);
    assert_certified(
        &format!("{} at {limit}", deploy.program.label),
        &played.result,
        root,
        &certification,
    );
}

/// The error codes of the play of `deploy` with the limit `limit`. Each limit plays once, and
/// `plays` keeps its outcome and its play.
async fn played_codes(
    plays: &mut Plays,
    chain: &mut NativeChain,
    deploy: &SweepDeploy<'_>,
    limit: u64,
) -> Vec<String> {
    if let Some((outcome, _)) = plays.get(&limit) {
        return outcome.codes.clone();
    }
    let played = play_limit(chain, deploy, limit).await;
    let codes = played.0.codes.clone();
    plays.insert(limit, played);
    codes
}

/// A limit in `(low, high]` at which the outcome of play changes: the codes of play at the
/// limit differ from its codes one phlo below the limit. Play has other codes at `low` than at
/// `high`, and the bisection keeps that true of its bounds.
async fn outcome_change(
    plays: &mut Plays,
    chain: &mut NativeChain,
    deploy: &SweepDeploy<'_>,
    mut low: u64,
    mut high: u64,
) -> u64 {
    let below = played_codes(plays, chain, deploy, low).await;
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        match played_codes(plays, chain, deploy, middle).await == below {
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
        // Changed by decision 90 (C6): the session of the deploys holds the setup space, so no
        // second runtime plays the setup. The legacy play of the setup follows.
        // let logs = play_replay_equal(&programs).await;
        // // The replay is not vacuous: the play logs hold space COMMs.
        // let mut runtime = node().await;
        // play(&mut runtime.play, &programs[..1], true).await;
        // let (space, names) = setup_space(&runtime.play).await;
        let (session, logs) = play_replay_equal(&programs).await;
        // The replay is not vacuous: the play logs hold space COMMs.
        let (space, names) = setup_space(&session.play).await;
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
        //
        // Changed by decision 90 (C6): validator replay certifies a failed offered deploy from its
        // pre-state (`execution/replay.rs`). Replay must give the errors of play, consume every
        // journal row and use the phlo of play. The native session certifies each deploy so.
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
                // Changed by decision 90 (C6): the divergent pair has the native limit of the
                // sweep.
                // 100_000,
                DIVERGENT_PHLO,
                "OutOfPhlogistonsError",
            ),
        ];
        for (failing, phlo, code) in failing {
            // Changed by decision 90 (C6): the native session plays the setup and the enqueue,
            // then plays the failed deploy, reverts it and certifies it by native replay. The
            // legacy play and the rigged replay follow.
            let (mut session, _) = play_replay_equal(&programs[..2]).await;
            let deploy = session
                .deploy_full(&failing.source, &failing.label, &Injections::new(), phlo)
                .await;
            assert_eq!(deploy.codes(), vec![code], "{}", failing.label);
            // let mut deploys = node().await;
            // play(&mut deploys.play, &programs[..2], true).await;
            // let root = deploys.play.get_root().await;
            // let fallback = deploys.play.create_soft_checkpoint().await;
            // let played = evaluate_with_phlo(&deploys.play, &failing, phlo).await;
            // let log = deploys.play.take_event_log().await;
            // deploys.play.revert_to_soft_checkpoint(fallback).await;
            // assert_eq!(error_codes(&played.errors), vec![code], "{}", failing.label);
            // deploys
            //     .replay
            //     .reset(&root)
            //     .await
            //     .expect("reset the replay runtime");
            // deploys
            //     .replay
            //     .rig(log)
            //     .await
            //     .expect("rig the replay runtime");
            // let replayed = evaluate_with_phlo(&deploys.replay, &failing, phlo).await;
            // assert!(
            //     !replayed.errors.is_empty(),
            //     "{}: the replay fails as the play failed",
            //     failing.label
            // );
            // assert_eq!(
            //     replayed.cost.value,
            //     played.cost.value,
            //     "{}: the replay charges the play cost (replay errors: {:?})",
            //     failing.label,
            //     error_codes(&replayed.errors)
            // );
            // assert_eq!(
            //     error_codes(&replayed.errors),
            //     vec![code],
            //     "{}: the replay fails the same way",
            //     failing.label
            // );
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
        // Changed by decision 90 (C6): the native session plays and certifies the deploys.
        // let logs = play_replay_equal(&programs).await;
        let (_, logs) = play_replay_equal(&programs).await;
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
        // Changed by decision 90 (C6): the native session plays and certifies the deploys, and its
        // runtime is the play runtime, so no second runtime plays them again. The legacy reads
        // follow.
        // play_replay_equal(&programs).await;
        // // The play itself delivered every value.
        // let mut runtime = node().await;
        // play(&mut runtime.play, &programs, true).await;
        // assert_eq!(values(&runtime.play, "rrs-peeked").await, vec![vec![gint(
        //     1
        // )]]);
        // assert_eq!(values(&runtime.play, "rrs-taken").await, vec![vec![gint(
        //     1
        // )]]);
        // let mut always: Vec<Vec<Par>> = values(&runtime.play, "rrs-always").await;
        // always.sort_by_key(|pars| format!("{pars:?}"));
        // assert_eq!(always, vec![vec![gint(2)], vec![gint(3)]]);
        // assert_eq!(values(&runtime.play, "rrs-peek-a").await.len(), 1);
        // assert_eq!(values(&runtime.play, "rrs-peek-b").await.len(), 1);
        let (session, _) = play_replay_equal(&programs).await;
        // The play itself delivered every value.
        let play = &session.play;
        assert_eq!(values(play, "rrs-peeked").await, vec![vec![gint(1)]]);
        assert_eq!(values(play, "rrs-taken").await, vec![vec![gint(1)]]);
        let mut always: Vec<Vec<Par>> = values(play, "rrs-always").await;
        always.sort_by_key(|pars| format!("{pars:?}"));
        assert_eq!(always, vec![vec![gint(2)], vec![gint(3)]]);
        assert_eq!(values(play, "rrs-peek-a").await.len(), 1);
        assert_eq!(values(play, "rrs-peek-b").await.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_divergence_typed() {
        // Changed by decision 90 (C6): the deploys play on a native session, which certifies each
        // deploy by native replay from its pre-state. Native replay keeps no event log. The trace
        // binding requires that the committed log is exactly the events of the journal rows
        // (`checked_operations/trace.rs`). So a log without a space COMM fails the binding with a
        // typed coverage rejection, before replay evaluates the deploy. Native replay of another
        // program with the same evidence fails closed, because replay authenticates the footprint
        // and the source of each operation. The legacy rigged replays follow.
        //
        // let mut runtime = node().await;
        // let setup = [program("rrs-p4-divergence-setup", SETUP.to_owned())];
        // play(&mut runtime.play, &setup, true).await;
        // let (space, names) = setup_space(&runtime.play).await;
        // let a = names.first().expect("the setup makes names").clone();
        // let seed = [program(
        //     "rrs-p4-divergence-seed",
        //     with_space("a!(1) | a!(2)"),
        // )];
        // let seeded = play(&mut runtime.play, &seed, true).await;
        // let root = seeded
        //     .last()
        //     .and_then(|deploy| deploy.root.clone())
        //     .expect("the seed has a root");
        // let dequeue = [program(
        //     "rrs-p4-divergence-dequeue",
        //     with_space(r#"for (@x <- a) { @"rrs-x"!(x) }"#),
        // )];
        // let played = play(&mut runtime.play, &dequeue, true).await;
        // let log = played.first().expect("one played deploy").log.clone();
        // assert_eq!(comms_on(&log, &data_channel(space, &a)), 1);
        // assert_eq!(comms_on(&log, &counter_channel(space, &a)), 1);
        // // The unchanged log replays.
        // runtime
        //     .replay
        //     .reset(&root)
        //     .await
        //     .expect("reset the replay runtime");
        // replay(&mut runtime.replay, &dequeue, &played, true).await;
        // // A log without the COMM of the data channel, or without the COMM of the counter
        // // bump, makes the space operation diverge. The replay names the divergence with its
        // // own code. The divergence comes at commit, and the operations before the unpaired fire
        // // stay applied (decision 36). Without the data COMM, the bump before the data fire
        // // pairs, so the replay uses every recorded event. Without the bump COMM, the
        // // transaction fails at the bump, so the data COMM stays unused.
        // for (tampered, leaves_unused) in [
        //     (data_channel(space, &a), false),
        //     (counter_channel(space, &a), true),
        // ] {
        //     let key = hash(&tampered);
        //     let log: Vec<Event> = log
        //         .iter()
        //         .filter(|event| !is_comm_on(event, &key))
        //         .cloned()
        //         .collect();
        //     runtime
        //         .replay
        //         .reset(&root)
        //         .await
        //         .expect("reset the replay runtime");
        //     runtime
        //         .replay
        //         .rig(log)
        //         .await
        //         .expect("rig the replay runtime with the tampered log");
        //     let result = evaluate(&runtime.replay, &dequeue[0]).await;
        //     assert_eq!(
        //         error_codes(&result.errors),
        //         vec!["ReplayDivergence"],
        //         "a missing space COMM is a typed divergence"
        //     );
        //     assert_eq!(
        //         runtime.replay.check_replay_data().await.is_err(),
        //         leaves_unused,
        //         "the tampered replay leaves recorded events unused exactly when the bump COMM is \
        //          missing"
        //     );
        //     runtime.replay.take_event_log().await;
        // }
        // // A replay of another program with the same log also fails: the host pairing leaves
        // // the recorded events unused.
        // runtime
        //     .replay
        //     .reset(&root)
        //     .await
        //     .expect("reset the replay runtime");
        // runtime
        //     .replay
        //     .rig(log)
        //     .await
        //     .expect("rig the replay runtime");
        // let other = program(
        //     "rrs-p4-divergence-dequeue",
        //     with_space(r#"for (@x <- a) { @"rrs-y"!(x) }"#),
        // );
        // evaluate(&runtime.replay, &other).await;
        // assert!(runtime.replay.check_replay_data().await.is_err());
        let mut session = Session::new().await;
        session.run(SETUP, "rrs-p4-divergence-setup").await;
        let (space, names) = setup_space(&session.play).await;
        let a = names.first().expect("the setup makes names").clone();
        session
            .run(&with_space("a!(1) | a!(2)"), "rrs-p4-divergence-seed")
            .await;
        // The pre-state of the dequeue, as a validator holds it before replay.
        let history = session.history();
        let dequeue = program(
            "rrs-p4-divergence-dequeue",
            with_space(r#"for (@x <- a) { @"rrs-x"!(x) }"#),
        );
        // `Session::run` certifies the unchanged record of the dequeue.
        let played = session.run(&dequeue.source, &dequeue.label).await;
        let log = played.log.clone();
        assert_eq!(comms_on(&log, &data_channel(space, &a)), 1);
        assert_eq!(comms_on(&log, &counter_channel(space, &a)), 1);
        let (recording, journal) = committed_evidence_of(&played.result);
        let coverage = NativeOperationTraceError::Coverage.to_string();
        for tampered in [data_channel(space, &a), counter_channel(space, &a)] {
            let key = hash(&tampered);
            let log: Vec<Event> = log
                .iter()
                .filter(|event| !is_comm_on(event, &key))
                .cloned()
                .collect();
            let rejection =
                replay_environment_from(history.clone(), log, PHLO, &recording, journal.clone())
                    .await
                    .err()
                    .expect("the trace binding rejects a log without a space COMM");
            assert!(
                rejection.starts_with("the trace binding rejects the log")
                    && rejection.ends_with(&coverage),
                "a missing space COMM is a typed coverage rejection: {rejection}"
            );
        }
        // Native replay of another program with the same evidence also fails.
        let environment = replay_environment_from(history, log, PHLO, &recording, journal)
            .await
            .expect("the committed evidence checks");
        let other = program(
            "rrs-p4-divergence-dequeue",
            with_space(r#"for (@x <- a) { @"rrs-y"!(x) }"#),
        );
        let certification = finish_with(
            environment,
            &other.source,
            Blake2b512Random::create_from_bytes(other.label.as_bytes()),
        )
        .await;
        assert!(
            rejected(&certification),
            "the certification rejects another program with the evidence of the dequeue"
        );
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
        // Changed by decision 90 (C6): validator replay certifies an offered deploy natively
        // (`execution/replay.rs`), so native replay certifies each case of the sweep, and the
        // limits are native phlo limits.
        // Changed by decision 90 (decision 69): the sweep has no legacy variant, because DR-134
        // deleted the legacy meter.
        // out_of_phlo_sweep_replays_equal(false).await;
        out_of_phlo_sweep_replays_equal().await;
    }

    // Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
    //
    // #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    // async fn space_queue_legacy_out_of_phlo_sweep_replays_equal() {
    //     // Decision 45, Option P. In a legacy deploy, each staged space operation pays the
    //     // charges of `ChargingRSpace`: the pre-charge in its callback, its post-charges in the
    //     // callback of the next operation, and the post-charges of the last operation after the
    //     // commit. The same sweep must replay every limit to the outcome of play, so a charge
    //     // that runs out of phlo stops play and replay at the same point.
    //     out_of_phlo_sweep_replays_equal(true).await;
    // }

    /// The sweep of `space_queue_out_of_phlo_sweep_replays_equal`. It ran in a legacy deploy when
    /// its former parameter `legacy` was true, until DR-134 deleted the legacy meter (decision 90,
    /// decision 69).
    // Changed by decision 90 (decision 69): the sweep has no legacy variant.
    // async fn out_of_phlo_sweep_replays_equal(legacy: bool) {
    //     let mut node = node().await;
    //     let _legacy = legacy.then(|| {
    //         (
    //             node.play.cost.enter_legacy_scope(),
    //             node.replay.cost.enter_legacy_scope(),
    //         )
    //     });
    async fn out_of_phlo_sweep_replays_equal() {
        // Changed by decision 90 (C6): the sweep plays natively on a native chain. Native replay
        // certifies each case from the seeded root through its committed evidence, as validator
        // replay certifies an offered deploy (`execution/replay.rs`). A case whose replay differs
        // from its play fails its certification, so the sweep collects no mismatches. The legacy
        // sweep follows.
        //
        // let mut node = node().await;
        // let setup = [
        //     program("rrs-p4-sweep-setup", SETUP.to_owned()),
        //     program(
        //         "rrs-p4-sweep-seed",
        //         with_space(r#"for (@w <- b) { @"rrs-w"!(w) } | a!(0)"#),
        //     ),
        // ];
        // let seeded = play(&mut node.play, &setup, true).await;
        // let root = seeded
        //     .last()
        //     .and_then(|deploy| deploy.root.clone())
        //     .expect("the seed has a root");
        // let (_, names) = setup_space(&node.play).await;
        // let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
        // let injections = HashMap::from([(injected("a"), a), (injected("b"), b)]);
        // let target = program(
        //     "rrs-p4-sweep-target",
        //     format!(
        //         "new a(`{}`), b(`{}`) in {{ b!(1) | a!(2) }}",
        //         injected("a"),
        //         injected("b")
        //     ),
        // );
        // let deploy = SweepDeploy {
        //     program: &target,
        //     injections: &injections,
        //     root: &root,
        // };
        // let (full, _) = play_limit(&mut node.play, &deploy, PHLO).await;
        // assert!(
        //     full.codes.is_empty(),
        //     "the target deploy succeeds with enough phlo: {:?}",
        //     full.codes
        // );
        //
        // let mut plays = Plays::new();
        // let mut limits: Vec<i64> = (0..=full.cost).step_by(SWEEP_STEP).collect();
        // if limits.last() != Some(&full.cost) {
        //     limits.push(full.cost);
        // }
        // for limit in &limits {
        //     played_codes(&mut plays, &mut node.play, &deploy, *limit).await;
        // }
        // let mut cases: BTreeSet<i64> = limits.iter().copied().collect();
        // let mut changes = Vec::new();
        // for pair in limits.windows(2) {
        //     let [low, high] = [pair[0], pair[1]];
        //     let changed = played_codes(&mut plays, &mut node.play, &deploy, low).await
        //         != played_codes(&mut plays, &mut node.play, &deploy, high).await;
        //     if changed {
        //         let change = outcome_change(&mut plays, &mut node.play, &deploy, low, high).await;
        //         for limit in [change - 1, change, change + 1] {
        //             played_codes(&mut plays, &mut node.play, &deploy, limit).await;
        //             cases.insert(limit);
        //         }
        //         changes.push(change);
        //     }
        // }
        // assert!(
        //     changes.contains(&full.cost),
        //     "play succeeds from the full cost and fails one phlo below it: changes {changes:?}, \
        //      full cost {}",
        //     full.cost
        // );
        //
        // let mut mismatches = Vec::new();
        // let mut failed = 0usize;
        // for limit in &cases {
        //     let (played, log) = plays.remove(limit).expect("every case is played");
        //     if !played.codes.is_empty() {
        //         failed += 1;
        //     }
        //     let (replayed, complete) = replay_limit(&mut node.replay, &deploy, *limit, log).await;
        //     if replayed != played || (played.codes.is_empty() && !complete) {
        //         mismatches.push(format!(
        //             "{} at {limit}: play {played:?}, replay {replayed:?}, every event used: \
        //              {complete}",
        //             target.label
        //         ));
        //     }
        // }
        // assert!(failed > 0, "the sweep plays the target deploy out of phlo");
        //
        // // The divergent pair of `space_queue_replay_equal` runs until its phlo is spent.
        // let divergent = program(
        //     "rrs-p4-sweep-divergent",
        //     format!(
        //         "new b(`{}`) in {{ for (@x <= b) {{ Nil }} | b!!(1) }}",
        //         injected("b")
        //     ),
        // );
        // let deploy = SweepDeploy {
        //     program: &divergent,
        //     injections: &injections,
        //     root: &root,
        // };
        // let (played, log) = play_limit(&mut node.play, &deploy, DIVERGENT_PHLO).await;
        // assert_eq!(
        //     played.codes,
        //     vec!["OutOfPhlogistonsError"],
        //     "the divergent pair runs until the phlo is spent"
        // );
        // let (replayed, _) = replay_limit(&mut node.replay, &deploy, DIVERGENT_PHLO, log).await;
        // if replayed != played {
        //     mismatches.push(format!(
        //         "{} at {DIVERGENT_PHLO}: play {played:?}, replay {replayed:?}",
        //         divergent.label
        //     ));
        // }
        // assert!(
        //     mismatches.is_empty(),
        //     "{} of {} deploys replay to another outcome than play ({failed} sweep limits fail \
        //      in play):\n{}",
        //     mismatches.len(),
        //     cases.len() + 1,
        //     mismatches.join("\n")
        // );
        let mut chain = NativeChain::new().await;
        let setup = [
            program("rrs-p4-sweep-setup", SETUP.to_owned()),
            program(
                "rrs-p4-sweep-seed",
                with_space(r#"for (@w <- b) { @"rrs-w"!(w) } | a!(0)"#),
            ),
        ];
        let mut seeded = None;
        for program in &setup {
            seeded = Some(play_certified(&mut chain, program).await.root);
        }
        let root = seeded.expect("the seed has a root");
        let names = chain
            .values("rrs-names")
            .await
            .into_iter()
            .next()
            .expect("the setup stores the names");
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
        let (full, _) = play_limit(&mut chain, &deploy, PHLO).await;
        assert!(
            full.codes.is_empty(),
            "the target deploy succeeds with enough phlo: {:?}",
            full.codes
        );

        let mut plays = Plays::new();
        let mut limits: Vec<u64> = (0..=full.used).step_by(SWEEP_STEP).collect();
        if limits.last() != Some(&full.used) {
            limits.push(full.used);
        }
        for limit in &limits {
            played_codes(&mut plays, &mut chain, &deploy, *limit).await;
        }
        let mut cases: BTreeSet<u64> = limits.iter().copied().collect();
        let mut changes = Vec::new();
        for pair in limits.windows(2) {
            let [low, high] = [pair[0], pair[1]];
            let changed = played_codes(&mut plays, &mut chain, &deploy, low).await
                != played_codes(&mut plays, &mut chain, &deploy, high).await;
            if changed {
                let change = outcome_change(&mut plays, &mut chain, &deploy, low, high).await;
                for limit in [change - 1, change, change + 1] {
                    played_codes(&mut plays, &mut chain, &deploy, limit).await;
                    cases.insert(limit);
                }
                changes.push(change);
            }
        }
        assert!(
            changes.contains(&full.used),
            "play succeeds from the full usage and fails one phlo below it: changes {changes:?}, \
             full usage {}",
            full.used
        );

        let mut failed = 0usize;
        for limit in &cases {
            let (played, native) = plays.remove(limit).expect("every case is played");
            if !played.codes.is_empty() {
                failed += 1;
            }
            replay_limit(&deploy, *limit, &native).await;
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
        let (played, native) = play_limit(&mut chain, &deploy, DIVERGENT_PHLO).await;
        assert_eq!(
            played.codes,
            vec!["OutOfPhlogistonsError"],
            "the divergent pair runs until the phlo is spent"
        );
        replay_limit(&deploy, DIVERGENT_PHLO, &native).await;
    }

    // Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
    //
    // #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    // async fn space_queue_legacy_replay_equal() {
    //     // Decision 45, Option P. A legacy deploy charges each staged space operation as
    //     // `ChargingRSpace` charges its host call. Play and replay make the same charges, so they
    //     // reach the same cost, the same legacy log and the same root. The deploys name the space
    //     // only through injected names, so they make no host operation: each COMM event of a log
    //     // is a space COMM, and the legacy log holds one COMM fee for each of them.
    //     //
    //     // In a legacy deploy the reducer charges its own evaluation steps (`send eval`,
    //     // `var eval`, `substitution`) as its parallel branches run, so their order in the log
    //     // depends on the schedule, in play as in replay (design report D.4, R1). The test
    //     // compares the storage charges in order, which only the space jobs make here, and every
    //     // charge as a multiset.
    //     let mut node = node().await;
    //     let _legacy = (
    //         node.play.cost.enter_legacy_scope(),
    //         node.replay.cost.enter_legacy_scope(),
    //     );
    //     let setup = [program("rrs-p45-legacy-setup", SETUP.to_owned())];
    //     let seeded = play(&mut node.play, &setup, true).await;
    //     let mut root = seeded
    //         .last()
    //         .and_then(|deploy| deploy.root.clone())
    //         .expect("the setup has a root");
    //     let (_, names) = setup_space(&node.play).await;
    //     let [a, b] = <[Par; 2]>::try_from(names).expect("the setup makes two names");
    //     let injections = HashMap::from([(injected("a"), a), (injected("b"), b)]);
    //     let bodies = [
    //         ("rrs-p45-legacy-sends", "a!(1) | a!(2) | b!(3)"),
    //         (
    //             "rrs-p45-legacy-receives",
    //             "for (@x <- a) { Nil } | for (@y <- b) { Nil }",
    //         ),
    //         ("rrs-p45-legacy-peek", "for (@x <<- a) { Nil }"),
    //         (
    //             "rrs-p45-legacy-persistent",
    //             "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
    //         ),
    //         ("rrs-p45-legacy-drain", "for (@d <= a) { Nil }"),
    //     ];
    //     for (label, body) in bodies {
    //         let deploy = program(
    //             label,
    //             format!(
    //                 "new a(`{}`), b(`{}`) in {{ {body} }}",
    //                 injected("a"),
    //                 injected("b")
    //             ),
    //         );
    //         node.play
    //             .reset(&root)
    //             .await
    //             .expect("reset the play runtime");
    //         let played = evaluate_injected(&node.play, &deploy, &injections, PHLO).await;
    //         assert!(
    //             played.errors.is_empty(),
    //             "{label}: play fails: {:?}",
    //             played.errors
    //         );
    //         let played_legacy = node.play.cost.get_log();
    //         let log = node.play.take_event_log().await;
    //         let next = node.play.create_checkpoint().await.root;
    //         let comms = log
    //             .iter()
    //             .filter(|event| matches!(event, Event::Comm(_)))
    //             .count();
    //         let fees = played_legacy
    //             .iter()
    //             .filter(|charge| charge.operation == "comm event storage cost")
    //             .count();
    //         assert!(comms > 0, "{label}: the deploy makes space COMMs");
    //         assert_eq!(fees, comms, "{label}: one COMM fee for each space COMM");
    //
    //         node.replay
    //             .reset(&root)
    //             .await
    //             .expect("reset the replay runtime");
    //         node.replay
    //             .rig(log)
    //             .await
    //             .expect("rig the replay runtime with the play log");
    //         let replayed = evaluate_injected(&node.replay, &deploy, &injections, PHLO).await;
    //         assert!(
    //             replayed.errors.is_empty(),
    //             "{label}: replay fails: {:?}",
    //             replayed.errors
    //         );
    //         let replayed_legacy = node.replay.cost.get_log();
    //         node.replay.take_event_log().await;
    //         node.replay
    //             .check_replay_data()
    //             .await
    //             .unwrap_or_else(|error| panic!("{label}: the replay leaves events: {error:?}"));
    //         assert_eq!(
    //             replayed.cost.value, played.cost.value,
    //             "{label}: the replay cost"
    //         );
    //         assert_eq!(
    //             storage_charges(&replayed_legacy),
    //             storage_charges(&played_legacy),
    //             "{label}: the storage charges of the space jobs, in order"
    //         );
    //         assert_eq!(
    //             sorted_charges(&replayed_legacy),
    //             sorted_charges(&played_legacy),
    //             "{label}: the same legacy charges"
    //         );
    //         let replayed_root = node.replay.create_checkpoint().await.root;
    //         node.replay.take_event_log().await;
    //         assert_eq!(replayed_root, next, "{label}: the replay root");
    //         root = next;
    //     }
    // }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_queue_replay_reporting_equal() {
        // Changed by decision 90 (C6): Casper reports an offered deploy by native replay of its own
        // record with a reporting logger (`reporting_casper.rs`, `trace_offered`). The session of
        // the replay logs each stored and matched operation, the space operations included, to the
        // logger. The legacy reporting runtime over the rigged play log follows.
        //
        // let mut runtime = node().await;
        // let setup = [program("rrs-p4-reporting-setup", SETUP.to_owned())];
        // play(&mut runtime.play, &setup, true).await;
        // let (space, names) = setup_space(&runtime.play).await;
        // let a = names.first().expect("the setup makes names").clone();
        // let root = runtime.play.get_root().await;
        // let deploys = [program(
        //     "rrs-p4-reporting-ops",
        //     with_space(
        //         r#"a!(1) | for (@x <- a) { @"rrs-x"!(x) } | for (@p <<- b) { Nil } | b!(2) |
        // for (@k <- queue!?("qualifier")) { @"rrs-k"!(k) }"#,
        //     ),
        // )];
        // let played = play(&mut runtime.play, &deploys, true).await;
        // let deploy = played.first().expect("one played deploy");
        // // A reporting runtime over the same history replays the deploy.
        // let reader = runtime
        //     .history
        //     .get_history_reader(&root)
        //     .expect("read the history at the root");
        // let reporting = ReportingRspace::apply(
        //     runtime.history.clone(),
        //     Arc::new(HotStoreInstances::create_from_hr(reader.base())),
        //     Arc::new(Box::new(Matcher)),
        // );
        // let mut reporter = create_replay_rho_runtime(
        //     reporting.clone(),
        //     Arc::new(default_mergeable_tags()),
        //     false,
        //     &mut Vec::new(),
        //     ExternalServices::noop(),
        // )
        // .await
        // .expect("create the reporting runtime");
        // fund(&reporter);
        // reporter
        //     .reset(&root)
        //     .await
        //     .expect("reset the reporting runtime");
        // replay(&mut reporter, &deploys, &played, false).await;
        // // The report holds every COMM of the play log, the space COMMs with their routed
        // // channels and their records included. A checkpoint clears the report, so the test
        // // reads the report first.
        // let report = reporting.get_report().expect("read the report");
        // let reported_root = reporter.create_checkpoint().await.root;
        // assert_eq!(
        //     Some(reported_root),
        //     deploy.root.clone(),
        //     "the reporting replay reaches the play root"
        // );
        // let comms: Vec<_> = report
        //     .iter()
        //     .flat_map(|batch| batch.events.iter())
        //     .filter_map(|event| match event {
        //         ReportingEvent::ReportingComm(comm) => Some(comm),
        //         ReportingEvent::ReportingProduce(_) | ReportingEvent::ReportingConsume(_) => None,
        //     })
        //     .collect();
        // let played_comms = deploy
        //     .log
        //     .iter()
        //     .filter(|event| matches!(event, Event::Comm(_)))
        //     .count();
        // assert_eq!(comms.len(), played_comms, "the report lists every COMM");
        // let data = data_channel(space, &a);
        // let space_comms: Vec<_> = comms
        //     .iter()
        //     .filter(|comm| comm.consume.channels == [data.clone()])
        //     .collect();
        // assert_eq!(space_comms.len(), 1, "the dequeue on a is reported");
        // let [delivered] = space_comms[0].produces.as_slice() else {
        //     panic!("the reported space COMM binds one record");
        // };
        // // As for a host COMM, the report holds the matched data: the payload without the
        // // envelope of the record.
        // assert_eq!(delivered.channel, data);
        // assert_eq!(delivered.data.pars, vec![gint(1)]);
        // assert!(
        //     record::unwrap_datum(&delivered.data).is_err(),
        //     "no envelope is reported"
        // );
        // let produce_count = deploy
        //     .log
        //     .iter()
        //     .filter(|event| matches!(event, Event::IoEvent(IOEvent::Produce(_))))
        //     .count();
        // let reported_produces = report
        //     .iter()
        //     .flat_map(|batch| batch.events.iter())
        //     .filter(|event| matches!(event, ReportingEvent::ReportingProduce(_)))
        //     .count();
        // assert_eq!(
        //     reported_produces, produce_count,
        //     "the report lists every produce"
        // );
        // assert_eq!(values(&reporter, "rrs-x").await, vec![vec![gint(1)]]);
        let mut session = Session::new().await;
        session.run(SETUP, "rrs-p4-reporting-setup").await;
        let (space, names) = setup_space(&session.play).await;
        let a = names.first().expect("the setup makes names").clone();
        // The pre-state of the deploy, as the reporting node holds it before replay.
        let history = session.history();
        let ops = program(
            "rrs-p4-reporting-ops",
            with_space(
                r#"a!(1) | for (@x <- a) { @"rrs-x"!(x) } | for (@p <<- b) { Nil } | b!(2) |
for (@k <- queue!?("qualifier")) { @"rrs-k"!(k) }"#,
            ),
        );
        let deploy = session.run(&ops.source, &ops.label).await;
        let root = session.checkpoint().await;
        // A native replay with a reporting logger replays the deploy from its pre-state, as the
        // reporting node replays each offered deploy of a block.
        let (recording, journal) = committed_evidence_of(&deploy.result);
        let events = Arc::new(Mutex::new(Vec::new()));
        let logger: RhoSpaceLogger = Box::new(ReportingLogger {
            report: Arc::new(Mutex::new(Vec::new())),
            soft_report: events.clone(),
        });
        let environment = replay_environment_with_logger(
            history,
            deploy.log.clone(),
            PHLO,
            &recording,
            journal,
            &mut Vec::new(),
            Some(logger),
        )
        .await
        .expect("the committed evidence checks");
        let certification = finish_with(
            environment,
            &ops.source,
            Blake2b512Random::create_from_bytes(ops.label.as_bytes()),
        )
        .await;
        assert_certified(&ops.label, &deploy.result, Some(&root), &certification);
        // The report holds every COMM of the play log, the space COMMs with their routed
        // channels and their records included.
        let report = events.lock().expect("read the report").clone();
        let comms: Vec<_> = report
            .iter()
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
            .filter(|event| matches!(event, ReportingEvent::ReportingProduce(_)))
            .count();
        assert_eq!(
            reported_produces, produce_count,
            "the report lists every produce"
        );
        // The report lists every consume, the parked and the matched space consumes included, as
        // the base checks for host consumes (DR-134).
        let consume_count = deploy
            .log
            .iter()
            .filter(|event| matches!(event, Event::IoEvent(IOEvent::Consume(_))))
            .count();
        let reported_consumes = report
            .iter()
            .filter(|event| matches!(event, ReportingEvent::ReportingConsume(_)))
            .count();
        assert_eq!(
            reported_consumes, consume_count,
            "the report lists every consume"
        );
        // The reporting replay reaches the root of play, so its state holds the value of play on
        // `@"rrs-x"`.
        assert_eq!(values(&session.play, "rrs-x").await, vec![vec![gint(1)]]);
    }
}
