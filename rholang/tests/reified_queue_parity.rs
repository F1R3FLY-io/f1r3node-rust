//! The queue parity golden of P5 (P5 design v3, section A.1, prerequisite 3 of W0).
//!
//! P5 rewrites the engine that runs the `hashmap:queue` space: the spine S1 generalizes its
//! plans, and later batches add other disciplines. This suite pins what the engine does on a
//! fixed set of deploys, as captured at the commit of decision 45 (Option P). For each deploy
//! it pins every event identity in order (the Produce, Consume and COMM hashes, the peeks and
//! `times_repeated`), the phlo cost, the error codes, and the root after the deploy. A deploy of
//! a legacy budget pinned its storage charges in order and all its legacy charges as a multiset,
//! until DR-134 deleted the legacy meter. Decision 90 (decision 69) retires that scenario
//! (`queue_parity_legacy_budget`), and the golden keeps its record. P5 must leave every value
//! unchanged for the queue.
//!
//! The scenarios are the FIFO order with a peek and its re-store (decision 45, Rule E), a
//! produce-side peek with its re-offer (decision 30), a persistent send past
//! `FIRE_ROUND_CAP` (I-21), the re-entry of decision 43, the divergent pair (A10), persistent
//! operations, and, until decision 90 (decision 69) retired it, the deploys of a legacy budget
//! (decision 45, Option P).
//!
//! The deploys name the space only through injected names, so they make no host operation
//! after the setup. In a legacy deploy, the reducer charged its own evaluation steps in the
//! order of its schedule (design report of decision 45, D.4). So the retired scenario pinned the
//! storage charges in order, which only the space jobs made, and every charge as a multiset.
//!
//! To capture the golden, set `RRS_CAPTURE_QUEUE_PARITY_OUT` to an absolute directory under
//! `target/`, run this suite, and assemble the fragments with
//! `scripts/verify-reified/merge-queue-parity-capture.py`. Without the variable, each test
// Changed by decision 112: the golden of this tree is v2. The text before:
// //! compares its scenario with `test-vectors/reified-queue-parity-v1.json`.
//! compares its scenario with `test-vectors/reified-queue-parity-v2.json`.
//!
//! Changed by decision 90 (C6, decision 69): spaces have one execution model, native funded
//! execution. So each deploy plays natively in the native session of the shared harness
//! (`support::session`). Native replay of its own record certifies each deploy from its
//! pre-state, as validator replay certifies an offered deploy. The cost of a record is the
//! native phlo usage of its deploy, and the setup space comes from `Session::setup`.
//!
//! Changed by decision 93: racing evaluation gives arrival-order logs (DR-132). So each scenario
//! runs three times on a runtime with two worker threads and three times on a runtime with eight.
//! Each run replays its own records, and every run must give the same records. A record keeps
//! only the observations that every interleaving gives: the state of the collections, the cost,
//! the error codes, and the number of events of each kind.
//!
//! In a scenario of [`Order::Arrival`], a deploy runs parallel processes on one collection. Their
//! ords follow their arrival, and so do the event identities and the roots. So the records of
//! such a scenario keep no log and no root. A step can also free other observations ([`Free`]).
//! The parallel sends of the persistent operations take their places in the queue in the order
//! of their arrival. In the persistent send, the send fires the parked second receive, or that
//! receive takes the stored send. In the divergent pair, the operation that arrives second
//! re-issues after each round. Each of these two races decides the number of events of each kind.
//! In the re-entry send, the datum 99 arrives before or after the re-issued send, and the cost
//! follows that race.

// Disabled by decision 90 (C6): the native session reads the names of the setup space.
// #[path = "../../models/tests/support/reified_strategies.rs"]
// mod reified_strategies;

// Added by decision 90 (C6): the native session of the shared harness.
#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

// Disabled by decision 90 (C6): the native session seeds and funds each deploy.
// use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
// Disabled by decision 90 (C6): the native session reads the names of the setup space.
// use reified_strategies::gstring;
// Disabled by decision 90 (C6): the native session seeds and funds each deploy.
// use rholang::rust::interpreter::accounting::costs::Cost;
// use rholang::rust::interpreter::accounting::Sig;
use prost::Message;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
// Disabled by decision 90 (C6): the native session builds its runtime in the shared harness.
// use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
// use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
// Disabled by decision 90 (C6): the native session builds its runtime in the shared harness.
// use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
// use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use serde_json::{json, Map, Value};
// Added by decision 90 (C6): the native session of the shared harness.
use support::session::{counter_channel, counter_of, Injections, Session, Space, PHLO};

// Disabled by decision 90 (C6): the native session funds each deploy, and `PHLO` of the shared
// harness is the native phlo limit of a deploy.
// const DEPLOY_SIGNATURE: &[u8] = b"rrs-p5-parity-deploy";
// const FUNDING_SIGNATURE: &[u8] = b"rrs-p5-parity-payer";
// const PHLO: i64 = 100_000_000;

// Changed by decision 90 (C6): the limit of the divergent pair is a native phlo limit, as in the
// queue suite (`reified_queue.rs`).
// /// The phlo limit of the divergent pair. It stops the endless re-issue in about 100 ms.
// const DIVERGENT_PHLO: i64 = 100_000;
/// The native phlo limit of the divergent pair. It stops the endless re-issue after a few rounds.
const DIVERGENT_PHLO: u64 = 20_000;

/// The waiters of the capped send: two more than `FIRE_ROUND_CAP` (64) at the capture, so the
/// send runs past the cap. The count is a literal, so the deploys of the golden are fixed.
const CAPPED_WAITERS: usize = 66;

/// The waiters of the re-entry scenario: `FIRE_ROUND_CAP` (64) at the capture, so the first
/// round of the send ends at the cap.
const REENTRY_WAITERS: usize = 64;

/// The golden file, relative to the workspace root.
// Changed by decision 112: the fields change the vector of this tree, and decision 120 keeps v1
// frozen as evidence for the masked comparison (ruling 6). The text before:
// const CORPUS: &str = "test-vectors/reified-queue-parity-v1.json";
const CORPUS: &str = "test-vectors/reified-queue-parity-v2.json";

/// The environment variable that switches the suite to capture mode.
const CAPTURE_DIRECTORY: &str = "RRS_CAPTURE_QUEUE_PARITY_OUT";

/// Added by decision 93: the runs of each scenario on each runtime of [`RACING_WORKERS`].
const RACING_RUNS: usize = 3;

// Changed by decision 111: the runtimes also hold the current-thread runtime, as the base runs
// its racing tests (`random_candidate_order.rs`, P-1 and P-6 of DR-132).
/// The worker threads of the runtimes of each scenario, where 0 is the current-thread runtime
/// (decisions 93 and 111). No scenario needs the current-thread runtime to pass.
const RACING_WORKERS: [usize; 3] = [0, 2, 8];

/// Added by decision 90 (C6): the factory of the setup space.
const QUEUE_FACTORY: &str = "rho:space:hashmap:queue";

// Disabled by decision 90 (C6): the native session creates the setup space with
// `Session::setup` of the shared harness, which also publishes the handle.
// /// A deploy that creates a queue space and two names of it. The names go to `@"rrs-names"`.
// const SETUP: &str = r#"
// new factory(`rho:space:hashmap:queue`) in {
//   for (queue <- factory!?("default")) {
//     for (a <- queue!?("gensym")) {
//       for (b <- queue!?("gensym")) {
//         @"rrs-names"!(*a, *b)
//       }
//     }
//   }
// }
// "#;

/// The URI of an injected name.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

/// The budget of the deploys of a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Budget {
    /// A token budget, the default of the runtime.
    Token,
    // Changed by decision 90 (decision 69): DR-134 deleted the legacy meter and its scope, so no
    // deploy runs with a legacy budget.
    // /// A legacy budget: the deploys run inside `RuntimeBudget::enter_legacy_scope`.
    // Legacy,
}

/// One deploy of a scenario: its label (the source of its random state), its body over the
/// injected names `a` and `b`, and its phlo limit.
struct Step {
    label: String,
    body: String,
    // Changed by decision 90 (C6): the phlo limit of a deploy is a native phlo limit.
    // phlo: i64,
    phlo: u64,
    // Added by decision 93: the observations of the deploy that racing evaluation leaves free.
    free: &'static [Free],
}

fn step(label: &str, body: &str) -> Step {
    Step {
        label: label.to_owned(),
        body: body.to_owned(),
        phlo: PHLO,
        // Added by decision 93: by default, every interleaving gives each observation.
        free: &[],
    }
}

/// Added by decision 93: the observations of a step that racing evaluation leaves free.
impl Step {
    /// The step with the free observations `free`.
    fn with_free(self, free: &'static [Free]) -> Self { Self { free, ..self } }

    /// Whether racing evaluation leaves `observation` of the deploy free.
    fn frees(&self, observation: Free) -> bool { self.free.contains(&observation) }
}

// Changed by decision 90 (C6): the native session of the shared harness plays and certifies
// each deploy ([`run_scenario`]). The legacy session follows.
//
// /// A play runtime over a fresh in-memory store, after the setup deploy, with the injected
// /// names of the setup space.
// struct Session {
//     play: RhoRuntimeImpl,
//     injections: HashMap<String, Par>,
// }
//
// impl Session {
//     async fn new() -> Self {
//         let mut manager = InMemoryStoreManager::new();
//         let stores = manager
//             .r_space_stores()
//             .await
//             .expect("create the in-memory RSpace stores");
//         let (mut play, _replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
//         play.cost
//             .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
//         let setup = play
//             .evaluate(
//                 SETUP,
//                 Cost::create(PHLO, "rrs-p5-parity-setup"),
//                 HashMap::new(),
//                 Blake2b512Random::create_from_bytes(b"rrs-p5-parity-setup"),
//             )
//             .await
//             .expect("the setup returns a result");
//         assert!(
//             setup.errors.is_empty(),
//             "the setup fails: {:?}",
//             setup.errors
//         );
//         play.create_checkpoint().await;
//         let names = play.get_data(&gstring("rrs-names")).await;
//         let pars = match <[_; 1]>::try_from(names) {
//             Ok([datum]) => datum.a.pars,
//             Err(data) => panic!("@\"rrs-names\" holds {} data instead of one", data.len()),
//         };
//         let [a, b] = <[Par; 2]>::try_from(pars).expect("the setup makes two names");
//         let injections = HashMap::from([(injected("a"), a), (injected("b"), b)]);
//         Self { play, injections }
//     }
//
//     fn source(step: &Step) -> String {
//         format!(
//             "new a(`{}`), b(`{}`) in {{ {} }}",
//             injected("a"),
//             injected("b"),
//             step.body
//         )
//     }
//
//     /// Runs `step` as Casper runs a user deploy: a soft checkpoint, the evaluation, the event
//     /// log, and the revert of a failed deploy. Then it creates a checkpoint, so each record
//     /// holds the root after its deploy.
//     async fn deploy(&mut self, step: &Step) -> Deployed {
//         let fallback = self.play.create_soft_checkpoint().await;
//         let source = Self::source(step);
//         let phlo = Cost::create(step.phlo, step.label.clone());
//         let random = Blake2b512Random::create_from_bytes(step.label.as_bytes());
//         let result = self
//             .play
//             .evaluate(&source, phlo, self.injections.clone(), random)
//             .await
//             .unwrap_or_else(|error| {
//                 panic!(
//                     "{}: the evaluation returns no result: {error:?}",
//                     step.label
//                 )
//             });
//         let legacy = self.play.cost.get_log();
//         let log = self.play.take_event_log().await;
//         if !result.errors.is_empty() {
//             self.play.revert_to_soft_checkpoint(fallback).await;
//         }
//         let root = self.play.create_checkpoint().await.root;
//         Deployed {
//             result,
//             legacy,
//             log,
//             root,
//         }
//     }
// }

/// Added by decision 90 (C6): the source of `step`, with the injected names `a` and `b` of the
/// setup space.
fn source(step: &Step) -> String {
    format!(
        "new a(`{}`), b(`{}`) in {{ {} }}",
        injected("a"),
        injected("b"),
        step.body
    )
}

/// Added by decision 93: the order of the processes of the deploys of a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// Each deploy runs one process on the space at a time. So every run gives the same log and
    /// the same roots.
    Program,
    /// Some deploy runs parallel processes on one collection, and their order follows their
    /// arrival (DR-132). So do the ords of the records, the event identities and the roots.
    Arrival,
}

/// Added by decision 93: an observation of a deploy that racing evaluation can change from run to
/// run (DR-132). The record of the deploy holds what every interleaving gives instead, and the
/// replay of each run certifies the observation in that run (`Session::deploy_full`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Free {
    /// The order of the records and the parked continuations of each collection. The record holds
    /// each collection as a multiset.
    Places,
    /// The number of events of each kind. The record holds the number of COMM events.
    Kinds,
    /// The cost. The record holds no cost.
    Cost,
}

/// The observations of one deploy.
struct Deployed {
    result: EvaluateResult,
    // Changed by decision 90 (decision 69): only a legacy budget read its log.
    // legacy: Vec<Cost>,
    log: Vec<Event>,
    root: Blake2b256Hash,
    // Added by decision 93: the collections of the setup space after the deploy.
    state: Value,
}

fn hex_of(hash: &Blake2b256Hash) -> String { hex::encode(hash.bytes()) }

/// Added by decision 93: the kind of one event.
fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::IoEvent(IOEvent::Produce(_)) => "PRODUCE",
        Event::IoEvent(IOEvent::Consume(_)) => "CONSUME",
        Event::Comm(_) => "COMM",
    }
}

/// Added by decision 93: the number of events of each kind in `log`. Racing evaluation gives
/// arrival-order logs (DR-132), so a record of [`Order::Arrival`] keeps its log as a multiset of
/// kinds.
fn kinds_json(log: &[Event]) -> Value {
    let mut kinds = Map::new();
    for event in log {
        let count = kinds.entry(event_kind(event)).or_insert(json!(0));
        *count = json!(count.as_u64().expect("a kind count is a number") + 1);
    }
    Value::Object(kinds)
}

/// Added by decision 93: the collections of the names `a` and `b` of the setup space. Each holds
/// its records, as payload and persistence, and its parked continuations, as persistence and
/// peek, in the order of their ords. It also holds the value of its counter. The ords follow the
/// arrival of parallel processes, so the state keeps no ord. When the deploy frees the places of
/// the records ([`Free::Places`]), each collection holds its records and its parked continuations
/// as multisets.
async fn state_json(session: &Session, space: &Space, places_free: bool) -> Value {
    let mut state = Map::new();
    for (name, channel) in [("a", &space.a), ("b", &space.b)] {
        let mut records: Vec<String> = session
            .records(space.space, channel)
            .await
            .into_iter()
            .map(|(_, payload, persistent)| {
                let payload: Vec<String> = payload
                    .iter()
                    .map(|par| hex::encode(par.encode_to_vec()))
                    .collect();
                format!("[{}] persistent={persistent}", payload.join(","))
            })
            .collect();
        let mut parked: Vec<String> = session
            .parked(space.space, channel)
            .await
            .into_iter()
            .map(|(_, persistent, peek)| format!("persistent={persistent} peek={peek}"))
            .collect();
        if places_free {
            records.sort_unstable();
            parked.sort_unstable();
        }
        let counter = counter_or_zero(session, space, channel).await;
        state.insert(
            name.to_owned(),
            json!({ "records": records, "parked": parked, "counter": counter }),
        );
    }
    Value::Object(state)
}

/// Added by decision 93: the value of the counter of the collection of `name`, or 0 before its
/// first operation.
async fn counter_or_zero(session: &Session, space: &Space, name: &Par) -> i64 {
    let data = session.data(&counter_channel(space.space, name)).await;
    match data.is_empty() {
        true => 0,
        false => counter_of(&data),
    }
}

fn produce_text(produce: &Produce) -> String {
    let outputs: Vec<String> = produce.output_value.iter().map(hex::encode).collect();
    format!(
        "{} on {} persistent={} deterministic={} failed={} output=[{}]",
        hex_of(&produce.hash),
        hex_of(&produce.channel_hash),
        produce.persistent,
        produce.is_deterministic,
        produce.failed,
        outputs.join(",")
    )
}

fn consume_text(consume: &Consume) -> String {
    let channels: Vec<String> = consume.channel_hashes.iter().map(hex_of).collect();
    format!(
        "{} on [{}] persistent={}",
        hex_of(&consume.hash),
        channels.join(","),
        consume.persistent
    )
}

fn comm_text(comm: &COMM) -> String {
    let produces: Vec<String> = comm.produces.iter().map(produce_text).collect();
    let peeks: Vec<String> = comm.peeks.iter().map(i32::to_string).collect();
    let repeated: Vec<String> = comm
        .times_repeated
        .iter()
        .map(|(produce, count)| format!("{}={count}", hex_of(&produce.hash)))
        .collect();
    format!(
        "COMM {} with [{}] peeks=[{}] repeated=[{}]",
        consume_text(&comm.consume),
        produces.join("; "),
        peeks.join(","),
        repeated.join(",")
    )
}

/// The identity of one event, with every field of the event.
fn event_text(event: &Event) -> String {
    match event {
        Event::IoEvent(IOEvent::Produce(produce)) => format!("PRODUCE {}", produce_text(produce)),
        Event::IoEvent(IOEvent::Consume(consume)) => format!("CONSUME {}", consume_text(consume)),
        Event::Comm(comm) => comm_text(comm),
    }
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

// Changed by decision 90 (decision 69): only the records of a legacy budget held legacy charges,
// and DR-134 deleted the legacy meter.
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
// fn charge_value(charge: &Cost) -> Value { json!([charge.operation.to_string(), charge.value]) }

/// The record of one deploy: the token-budget fields. A deploy of a legacy budget also held its
/// legacy charges until DR-134 deleted the legacy meter (decision 90, decision 69).
///
/// Changed by decision 93: a record keeps only the observations that every interleaving of its
/// deploy gives. A scenario of [`Order::Arrival`] keeps no log and no root, and a step can free
/// other observations ([`Free`]).
fn record(step: &Step, deployed: &Deployed, budget: Budget, order: Order) -> Value {
    let mut fields = Map::new();
    fields.insert("label".to_owned(), json!(step.label));
    // Changed by decision 93: only a scenario of program order keeps the log and the root of each
    // deploy.
    match order {
        Order::Program => {
            fields.insert(
                "events".to_owned(),
                Value::Array(
                    deployed
                        .log
                        .iter()
                        .map(|event| Value::String(event_text(event)))
                        .collect(),
                ),
            );
            fields.insert("root".to_owned(), json!(hex_of(&deployed.root)));
        }
        Order::Arrival => {}
    }
    // Changed by decision 90 (C6): the cost of a record is the native phlo usage of its deploy.
    // Changed by decision 93: a race of the deploy can decide the cost, and then the record holds
    // no cost. The legacy cost follows.
    // fields.insert("cost".to_owned(), json!(deployed.result.cost.value));
    if !step.frees(Free::Cost) {
        fields.insert(
            "native_phlo_usage".to_owned(),
            json!(deployed.result.native_phlo_usage),
        );
    }
    // Added by decision 93: the number of events of each kind, or the number of COMM events when
    // a race of the deploy decides the kinds.
    match step.frees(Free::Kinds) {
        true => {
            let comms = deployed
                .log
                .iter()
                .filter(|event| matches!(event, Event::Comm(_)))
                .count();
            fields.insert("comm_events".to_owned(), json!(comms));
        }
        false => {
            fields.insert("event_kinds".to_owned(), kinds_json(&deployed.log));
        }
    }
    // Added by decision 93: the state of the collections after the deploy.
    fields.insert("state".to_owned(), deployed.state.clone());
    fields.insert(
        "errors".to_owned(),
        json!(error_codes(&deployed.result.errors)),
    );
    match budget {
        // Changed by decision 90 (decision 69): no budget is a legacy budget, because DR-134
        // deleted the legacy meter.
        // Budget::Legacy => {
        //     let storage: Vec<Value> = deployed
        //         .legacy
        //         .iter()
        //         .filter(|charge| STORAGE_LABELS.contains(&charge.operation.as_ref()))
        //         .map(charge_value)
        //         .collect();
        //     let mut all: Vec<(String, i64)> = deployed
        //         .legacy
        //         .iter()
        //         .map(|charge| (charge.operation.to_string(), charge.value))
        //         .collect();
        //     all.sort();
        //     fields.insert("storage_charges".to_owned(), Value::Array(storage));
        //     fields.insert(
        //         "charges".to_owned(),
        //         Value::Array(
        //             all.into_iter()
        //                 .map(|(label, value)| json!([label, value]))
        //                 .collect(),
        //         ),
        //     );
        // }
        Budget::Token => {}
    }
    Value::Object(fields)
}

// Changed by decision 90 (C6): a native session runs the deploys of a scenario. The legacy
// runner follows.
//
// /// Runs the deploys of a scenario and returns their records.
// ///
// /// Each deploy runs as the plain evaluation of a user deploy.
// async fn run_scenario(steps: &[Step], budget: Budget) -> Value {
//     let mut plain = Session::new().await;
//     let _legacy = (budget == Budget::Legacy).then(|| plain.play.cost.enter_legacy_scope());
//     let mut records = Vec::with_capacity(steps.len());
//     for step in steps {
//         let deployed = plain.deploy(step).await;
//         records.push(record(step, &deployed, budget));
//     }
//     Value::Array(records)
// }
/// Runs the deploys of a scenario on a new native session after the setup, and returns their
/// records. Each deploy runs as Casper runs an offered deploy, and native replay of its own
/// record certifies it (`Session::deploy_full`). The root of each record is the checkpoint
/// root after its deploy, so a failed deploy has the root before it.
async fn run_scenario(steps: &[Step], budget: Budget, order: Order) -> Value {
    let mut session = Session::new().await;
    let space = session.setup(QUEUE_FACTORY, "\"default\"").await;
    let injections: Injections = HashMap::from([
        (injected("a"), space.a.clone()),
        (injected("b"), space.b.clone()),
    ]);
    let mut records = Vec::with_capacity(steps.len());
    for step in steps {
        let deploy = session
            .deploy_full(&source(step), &step.label, &injections, step.phlo)
            .await;
        let root = session.checkpoint().await;
        let deployed = Deployed {
            result: deploy.result,
            log: deploy.log,
            root,
            state: state_json(&session, &space, step.frees(Free::Places)).await,
        };
        records.push(record(step, &deployed, budget, order));
    }
    Value::Array(records)
}

// Changed by decision 111: 0 workers give the current-thread runtime, as in the base.
/// The runtime of a scenario: the current-thread runtime for 0 workers, else a multi-thread
/// runtime with `workers` worker threads (decisions 93 and 111).
fn racing_runtime(workers: usize) -> tokio::runtime::Runtime {
    match workers {
        0 => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread racing runtime"),
        _ => tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
            .expect("a racing runtime"),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate lives inside the workspace")
        .to_path_buf()
}

// Changed by decision 93: each scenario runs on the racing runtimes, and every run must give
// the same records. The legacy observation of one run follows.
//
// /// Writes the scenario to the capture directory, or compares it with the golden file.
// fn observe(scenario: &str, actual: Value) {
//     match std::env::var_os(CAPTURE_DIRECTORY) {
//         Some(directory) => write_fragment(Path::new(&directory), scenario, actual),
//         None => verify_scenario(scenario, &actual),
//     }
// }
/// Runs the scenario `steps` [`RACING_RUNS`] times on a runtime with each worker count of
/// [`RACING_WORKERS`]. Every run must give the records of the first run. Then it writes those
/// records to the capture directory, or compares them with the golden file.
fn observe(scenario: &str, steps: &[Step], budget: Budget, order: Order) {
    let mut runs = Vec::with_capacity(RACING_WORKERS.len() * RACING_RUNS);
    for workers in RACING_WORKERS {
        let runtime = racing_runtime(workers);
        for run in 0..RACING_RUNS {
            runs.push((
                format!("{workers} workers, run {run}"),
                runtime.block_on(run_scenario(steps, budget, order)),
            ));
        }
    }
    let ((first_label, first), rest) = runs.split_first().expect("each scenario runs");
    let mut mismatches = Vec::new();
    for (label, actual) in rest {
        compare(
            &format!("{scenario} ({label})"),
            first,
            actual,
            &mut mismatches,
        );
    }
    assert!(
        mismatches.is_empty(),
        "the runs of {scenario} differ from its first run ({first_label}):\n{}",
        mismatches.join("\n")
    );
    match std::env::var_os(CAPTURE_DIRECTORY) {
        Some(directory) => write_fragment(Path::new(&directory), scenario, first.clone()),
        None => verify_scenario(scenario, first),
    }
}

fn write_fragment(directory: &Path, scenario: &str, actual: Value) {
    let target = workspace_root().join("target");
    let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    let inside_target = directory.is_absolute()
        && directory
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
        && (directory.starts_with(&target) || directory.starts_with(&canonical_target));
    assert!(
        inside_target,
        "{CAPTURE_DIRECTORY} must be an absolute path under {}, got {}",
        target.display(),
        directory.display()
    );
    std::fs::create_dir_all(directory).expect("create the capture directory");
    let mut document = Map::new();
    document.insert(scenario.to_owned(), actual);
    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .expect("serialize the capture fragment");
    text.push('\n');
    std::fs::write(directory.join(format!("{scenario}.json")), text)
        .expect("write the capture fragment");
}

fn verify_scenario(scenario: &str, actual: &Value) {
    let path = workspace_root().join(CORPUS);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; capture the golden with {CAPTURE_DIRECTORY}",
            path.display()
        )
    });
    let corpus: Value = serde_json::from_str(&text).expect("parse the queue parity golden");
    let expected = corpus
        .get("scenarios")
        .and_then(|scenarios| scenarios.get(scenario))
        .unwrap_or_else(|| panic!("{CORPUS} has no scenario {scenario}"));
    let mut mismatches = Vec::new();
    compare(scenario, expected, actual, &mut mismatches);
    assert!(
        mismatches.is_empty(),
        "the queue differs from the parity golden {CORPUS}:\n{}",
        mismatches.join("\n")
    );
}

fn compare(path: &str, expected: &Value, actual: &Value, mismatches: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for key in left
                .keys()
                .chain(right.keys().filter(|key| !left.contains_key(*key)))
            {
                match (left.get(key), right.get(key)) {
                    (Some(l), Some(r)) => compare(&format!("{path}.{key}"), l, r, mismatches),
                    (Some(_), None) => mismatches.push(format!("{path}.{key}: missing")),
                    (None, Some(_)) => mismatches.push(format!("{path}.{key}: unexpected")),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                mismatches.push(format!(
                    "{path}: {} entries expected, {} found",
                    left.len(),
                    right.len()
                ));
            }
            for (index, (l, r)) in left.iter().zip(right).enumerate() {
                compare(&format!("{path}[{index}]"), l, r, mismatches);
            }
        }
        (l, r) if l == r => {}
        (l, r) => mismatches.push(format!("{path}: expected {l}, found {r}")),
    }
}

/// `count` waiters on `a` with the body `Nil`, as one parallel term.
fn parks_on_a(count: usize) -> String {
    let mut parks = String::with_capacity(count * 32 + 3);
    for _ in 0..count {
        parks.push_str("for (@v <- a) { Nil } | ");
    }
    parks.push_str("Nil");
    parks
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_fifo_and_peek_restore() {
#[test]
fn queue_parity_fifo_and_peek_restore() {
    // FIFO enqueue and dequeue, and a consume-side peek of the head: the COMM removes the
    // record, and the re-store puts it back at its place (decision 45, Rule E).
    let steps = [
        step("rrs-parity-fifo-enqueue-5", "a!(5)"),
        step("rrs-parity-fifo-enqueue-6", "a!(6)"),
        step("rrs-parity-fifo-peek-1", "for (@x <<- a) { Nil }"),
        step("rrs-parity-fifo-peek-2", "for (@y <<- a) { Nil }"),
        step(
            "rrs-parity-fifo-dequeue",
            "for (@z <- a) { for (@w <- a) { Nil } }",
        ),
    ];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe(
    //     "fifo_and_peek_restore",
    //     run_scenario(&steps, Budget::Token).await,
    // );
    observe(
        "fifo_and_peek_restore",
        &steps,
        Budget::Token,
        Order::Program,
    );
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_produce_side_peek_reoffer() {
#[test]
fn queue_parity_produce_side_peek_reoffer() {
    // A send fires a peek waiter and offers the datum to the next waiter in the same
    // transaction (decision 30). With no waiter behind the peek waiter, the re-offer stores
    // the datum.
    let steps = [
        step("rrs-parity-reoffer-peek", "for (@x <<- b) { Nil }"),
        step("rrs-parity-reoffer-take", "for (@y <- b) { Nil }"),
        step("rrs-parity-reoffer-send", "b!(5)"),
        step("rrs-parity-reoffer-peek-a", "for (@x <<- a) { Nil }"),
        step("rrs-parity-reoffer-store", "a!(6)"),
    ];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe(
    //     "produce_side_peek_reoffer",
    //     run_scenario(&steps, Budget::Token).await,
    // );
    observe(
        "produce_side_peek_reoffer",
        &steps,
        Budget::Token,
        Order::Program,
    );
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_capped_persistent_send() {
#[test]
fn queue_parity_capped_persistent_send() {
    // A persistent send past FIRE_ROUND_CAP waiters: the agent commits the first rounds and
    // re-issues the rest as a new operation that keeps the order of its first round (I-21,
    // decision 43).
    let steps = [
        step("rrs-parity-capped-park", &parks_on_a(CAPPED_WAITERS)),
        step("rrs-parity-capped-send", "a!!(8)"),
    ];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe(
    //     "capped_persistent_send",
    //     run_scenario(&steps, Budget::Token).await,
    // );
    observe(
        "capped_persistent_send",
        &steps,
        Budget::Token,
        Order::Arrival,
    );
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_reentry() {
#[test]
fn queue_parity_reentry() {
    // Decision 43: a body that the first round fires sends a younger datum, and the re-issued
    // persistent send re-enters ahead of it and fires the head waiter.
    let mut parks = String::from("for (@v <- a) { a!(99) }");
    for _ in 1..REENTRY_WAITERS {
        parks.push_str(" | for (@v <- a) { Nil }");
    }
    let steps = [
        step("rrs-parity-reentry-park", &parks),
        step("rrs-parity-reentry-park-one", "for (@1 <- a) { Nil }"),
        // Changed by decision 93: the datum 99 of the fired body arrives before or after the
        // re-issued send, and the cost follows that race.
        // step("rrs-parity-reentry-send", "a!!(1)"),
        step("rrs-parity-reentry-send", "a!!(1)").with_free(&[Free::Cost]),
    ];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe("reentry", run_scenario(&steps, Budget::Token).await);
    observe("reentry", &steps, Budget::Token, Order::Arrival);
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_divergent_pair() {
#[test]
fn queue_parity_divergent_pair() {
    // A persistent receive and a persistent send on one collection re-issue after every round
    // (A10) until the phlo runs out.
    // Changed by decision 93: the operation of the pair that arrives second re-issues after each
    // round, so the race decides the number of events of each kind.
    // let steps = [Step {
    //     label: "rrs-parity-divergent".to_owned(),
    //     body: "for (@x <= a) { Nil } | a!!(1)".to_owned(),
    //     phlo: DIVERGENT_PHLO,
    // }];
    let steps = [Step {
        label: "rrs-parity-divergent".to_owned(),
        body: "for (@x <= a) { Nil } | a!!(1)".to_owned(),
        phlo: DIVERGENT_PHLO,
        free: &[Free::Kinds],
    }];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe("divergent_pair", run_scenario(&steps, Budget::Token).await);
    observe("divergent_pair", &steps, Budget::Token, Order::Arrival);
}

// Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_persistent_operations() {
#[test]
fn queue_parity_persistent_operations() {
    // A persistent send with two waiters on b, and a persistent receive that drains a.
    let steps = [
        // Changed by decision 93: the parallel sends take their places in the queue in the order
        // of their arrival.
        // step("rrs-parity-persistent-fill", "a!(1) | a!(2) | b!(3)"),
        step("rrs-parity-persistent-fill", "a!(1) | a!(2) | b!(3)").with_free(&[Free::Places]),
        // Changed by decision 93: the records of `a` keep the places of the parallel sends of the
        // fill. The send fires the parked second receive, or the second receive takes the stored
        // send, and the number of events of each kind follows that race.
        // step(
        //     "rrs-parity-persistent-send",
        //     "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
        // ),
        step(
            "rrs-parity-persistent-send",
            "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
        )
        .with_free(&[Free::Places, Free::Kinds]),
        step("rrs-parity-persistent-drain", "for (@d <= a) { Nil }"),
    ];
    // Changed by decision 93: the scenario runs on the racing runtimes of `observe`.
    // observe(
    //     "persistent_operations",
    //     run_scenario(&steps, Budget::Token).await,
    // );
    observe(
        "persistent_operations",
        &steps,
        Budget::Token,
        Order::Arrival,
    );
}

// Disabled by decision 90 (decision 69): Option P (the legacy charges of decision 45) is removed with the legacy meter (DR-134).
//
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn queue_parity_legacy_budget() {
//     // Decision 45, Option P: the deploys of a legacy budget pay the legacy charges of the
//     // equivalent host program.
//     let steps = [
//         step("rrs-parity-legacy-sends", "a!(1) | a!(2) | b!(3)"),
//         step(
//             "rrs-parity-legacy-receives",
//             "for (@x <- a) { Nil } | for (@y <- b) { Nil }",
//         ),
//         step("rrs-parity-legacy-peek", "for (@x <<- a) { Nil }"),
//         step(
//             "rrs-parity-legacy-persistent",
//             "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
//         ),
//         step("rrs-parity-legacy-drain", "for (@d <= a) { Nil }"),
//     ];
//     observe("legacy_budget", run_scenario(&steps, Budget::Legacy).await);
// }
