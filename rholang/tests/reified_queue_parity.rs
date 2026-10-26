//! The queue parity golden of P5 (P5 design v3, section A.1, prerequisite 3 of W0).
//!
//! P5 rewrites the engine that runs the `hashmap:queue` space: the spine S1 generalizes its
//! plans, and later batches add other disciplines. This suite pins what the engine does on a
//! fixed set of deploys, as captured at the commit of decision 45 (Option P). For each deploy
//! it pins every event identity in order (the Produce, Consume and COMM hashes, the peeks and
//! `times_repeated`), the phlo cost, the error codes, and the root after the deploy. A deploy of
//! a legacy budget pins its storage charges in order and all its legacy charges as a multiset.
//! P5 must leave every value unchanged for the queue.
//!
//! The scenarios are the FIFO order with a peek and its re-store (decision 45, Rule E), a
//! produce-side peek with its re-offer (decision 30), a persistent send past
//! `FIRE_ROUND_CAP` (I-21), the re-entry of decision 43, the divergent pair (A10), persistent
//! operations, and the deploys of a legacy budget (decision 45, Option P).
//!
//! The deploys name the space only through injected names, so they make no host operation
//! after the setup. In a legacy deploy, the reducer charges its own evaluation steps in the
//! order of its schedule (design report of decision 45, D.4). So the suite pins the storage
//! charges in order, which only the space jobs make, and every charge as a multiset.
//!
//! To capture the golden, set `RRS_CAPTURE_QUEUE_PARITY_OUT` to an absolute directory under
//! `target/`, run this suite, and assemble the fragments with
//! `scripts/verify-reified/merge-queue-parity-capture.py`. Without the variable, each test
//! compares its scenario with `test-vectors/reified-queue-parity-v1.json`.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use reified_strategies::gstring;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use serde_json::{json, Map, Value};

const DEPLOY_SIGNATURE: &[u8] = b"rrs-p5-parity-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-p5-parity-payer";
const PHLO: i64 = 100_000_000;

/// The phlo limit of the divergent pair. It stops the endless re-issue in about 100 ms.
const DIVERGENT_PHLO: i64 = 100_000;

/// The waiters of the capped send: two more than `FIRE_ROUND_CAP` (64) at the capture, so the
/// send runs past the cap. The count is a literal, so the deploys of the golden are fixed.
const CAPPED_WAITERS: usize = 66;

/// The waiters of the re-entry scenario: `FIRE_ROUND_CAP` (64) at the capture, so the first
/// round of the send ends at the cap.
const REENTRY_WAITERS: usize = 64;

/// The golden file, relative to the workspace root.
const CORPUS: &str = "test-vectors/reified-queue-parity-v1.json";

/// The environment variable that switches the suite to capture mode.
const CAPTURE_DIRECTORY: &str = "RRS_CAPTURE_QUEUE_PARITY_OUT";

/// A deploy that creates a queue space and two names of it. The names go to `@"rrs-names"`.
const SETUP: &str = r#"
new factory(`rho:space:hashmap:queue`) in {
  for (queue <- factory!?("default")) {
    for (a <- queue!?("gensym")) {
      for (b <- queue!?("gensym")) {
        @"rrs-names"!(*a, *b)
      }
    }
  }
}
"#;

/// The URI of an injected name.
fn injected(name: &str) -> String { format!("rho:rrs:test:{name}") }

/// The budget of the deploys of a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Budget {
    /// A token budget, the default of the runtime.
    Token,
    /// A legacy budget: the deploys run inside `RuntimeBudget::enter_legacy_scope`.
    Legacy,
}

/// One deploy of a scenario: its label (the source of its random state), its body over the
/// injected names `a` and `b`, and its phlo limit.
struct Step {
    label: String,
    body: String,
    phlo: i64,
}

fn step(label: &str, body: &str) -> Step {
    Step {
        label: label.to_owned(),
        body: body.to_owned(),
        phlo: PHLO,
    }
}

/// A play runtime over a fresh in-memory store, after the setup deploy, with the injected
/// names of the setup space.
struct Session {
    play: RhoRuntimeImpl,
    injections: HashMap<String, Par>,
}

impl Session {
    async fn new() -> Self {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        let (mut play, _replay, _history) = create_runtimes(stores, false, &mut Vec::new()).await;
        play.cost
            .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
        let setup = play
            .evaluate(
                SETUP,
                Cost::create(PHLO, "rrs-p5-parity-setup"),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(b"rrs-p5-parity-setup"),
            )
            .await
            .expect("the setup returns a result");
        assert!(
            setup.errors.is_empty(),
            "the setup fails: {:?}",
            setup.errors
        );
        play.create_checkpoint().await;
        let names = play.get_data(&gstring("rrs-names")).await;
        let pars = match <[_; 1]>::try_from(names) {
            Ok([datum]) => datum.a.pars,
            Err(data) => panic!("@\"rrs-names\" holds {} data instead of one", data.len()),
        };
        let [a, b] = <[Par; 2]>::try_from(pars).expect("the setup makes two names");
        let injections = HashMap::from([(injected("a"), a), (injected("b"), b)]);
        Self { play, injections }
    }

    fn source(step: &Step) -> String {
        format!(
            "new a(`{}`), b(`{}`) in {{ {} }}",
            injected("a"),
            injected("b"),
            step.body
        )
    }

    /// Runs `step` as Casper runs a user deploy: a soft checkpoint, the evaluation, the event
    /// log, and the revert of a failed deploy. Then it creates a checkpoint, so each record
    /// holds the root after its deploy.
    async fn deploy(&mut self, step: &Step) -> Deployed {
        let fallback = self.play.create_soft_checkpoint().await;
        let source = Self::source(step);
        let phlo = Cost::create(step.phlo, step.label.clone());
        let random = Blake2b512Random::create_from_bytes(step.label.as_bytes());
        let result = self
            .play
            .evaluate(&source, phlo, self.injections.clone(), random)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{}: the evaluation returns no result: {error:?}",
                    step.label
                )
            });
        let legacy = self.play.cost.get_log();
        let log = self.play.take_event_log().await;
        if !result.errors.is_empty() {
            self.play.revert_to_soft_checkpoint(fallback).await;
        }
        let root = self.play.create_checkpoint().await.root;
        Deployed {
            result,
            legacy,
            log,
            root,
        }
    }
}

/// The observations of one deploy.
struct Deployed {
    result: EvaluateResult,
    legacy: Vec<Cost>,
    log: Vec<Event>,
    root: Blake2b256Hash,
}

fn hex_of(hash: &Blake2b256Hash) -> String { hex::encode(hash.bytes()) }

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

/// The labels of the storage charges of `ChargingRSpace` and of the legacy charges of the
/// space jobs (decision 45, Option P).
const STORAGE_LABELS: [&str; 6] = [
    "produces storage",
    "consume storage",
    "consume storage refund",
    "produces storage refund",
    "event storage cost",
    "comm event storage cost",
];

fn charge_value(charge: &Cost) -> Value { json!([charge.operation.to_string(), charge.value]) }

/// The record of one deploy: the token-budget fields, and the legacy charges of a deploy of a
/// legacy budget.
fn record(step: &Step, deployed: &Deployed, budget: Budget) -> Value {
    let mut fields = Map::new();
    fields.insert("label".to_owned(), json!(step.label));
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
    fields.insert("cost".to_owned(), json!(deployed.result.cost.value));
    fields.insert(
        "errors".to_owned(),
        json!(error_codes(&deployed.result.errors)),
    );
    fields.insert("root".to_owned(), json!(hex_of(&deployed.root)));
    match budget {
        Budget::Token => {}
        Budget::Legacy => {
            let storage: Vec<Value> = deployed
                .legacy
                .iter()
                .filter(|charge| STORAGE_LABELS.contains(&charge.operation.as_ref()))
                .map(charge_value)
                .collect();
            let mut all: Vec<(String, i64)> = deployed
                .legacy
                .iter()
                .map(|charge| (charge.operation.to_string(), charge.value))
                .collect();
            all.sort();
            fields.insert("storage_charges".to_owned(), Value::Array(storage));
            fields.insert(
                "charges".to_owned(),
                Value::Array(
                    all.into_iter()
                        .map(|(label, value)| json!([label, value]))
                        .collect(),
                ),
            );
        }
    }
    Value::Object(fields)
}

/// Runs the deploys of a scenario and returns their records.
///
/// Each deploy runs as the plain evaluation of a user deploy.
async fn run_scenario(steps: &[Step], budget: Budget) -> Value {
    let mut plain = Session::new().await;
    let _legacy = (budget == Budget::Legacy).then(|| plain.play.cost.enter_legacy_scope());
    let mut records = Vec::with_capacity(steps.len());
    for step in steps {
        let deployed = plain.deploy(step).await;
        records.push(record(step, &deployed, budget));
    }
    Value::Array(records)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate lives inside the workspace")
        .to_path_buf()
}

/// Writes the scenario to the capture directory, or compares it with the golden file.
fn observe(scenario: &str, actual: Value) {
    match std::env::var_os(CAPTURE_DIRECTORY) {
        Some(directory) => write_fragment(Path::new(&directory), scenario, actual),
        None => verify_scenario(scenario, &actual),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_fifo_and_peek_restore() {
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
    observe(
        "fifo_and_peek_restore",
        run_scenario(&steps, Budget::Token).await,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_produce_side_peek_reoffer() {
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
    observe(
        "produce_side_peek_reoffer",
        run_scenario(&steps, Budget::Token).await,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_capped_persistent_send() {
    // A persistent send past FIRE_ROUND_CAP waiters: the agent commits the first rounds and
    // re-issues the rest as a new operation that keeps the order of its first round (I-21,
    // decision 43).
    let steps = [
        step("rrs-parity-capped-park", &parks_on_a(CAPPED_WAITERS)),
        step("rrs-parity-capped-send", "a!!(8)"),
    ];
    observe(
        "capped_persistent_send",
        run_scenario(&steps, Budget::Token).await,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_reentry() {
    // Decision 43: a body that the first round fires sends a younger datum, and the re-issued
    // persistent send re-enters ahead of it and fires the head waiter.
    let mut parks = String::from("for (@v <- a) { a!(99) }");
    for _ in 1..REENTRY_WAITERS {
        parks.push_str(" | for (@v <- a) { Nil }");
    }
    let steps = [
        step("rrs-parity-reentry-park", &parks),
        step("rrs-parity-reentry-park-one", "for (@1 <- a) { Nil }"),
        step("rrs-parity-reentry-send", "a!!(1)"),
    ];
    observe("reentry", run_scenario(&steps, Budget::Token).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_divergent_pair() {
    // A persistent receive and a persistent send on one collection re-issue after every round
    // (A10) until the phlo runs out.
    let steps = [Step {
        label: "rrs-parity-divergent".to_owned(),
        body: "for (@x <= a) { Nil } | a!!(1)".to_owned(),
        phlo: DIVERGENT_PHLO,
    }];
    observe("divergent_pair", run_scenario(&steps, Budget::Token).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_persistent_operations() {
    // A persistent send with two waiters on b, and a persistent receive that drains a.
    let steps = [
        step("rrs-parity-persistent-fill", "a!(1) | a!(2) | b!(3)"),
        step(
            "rrs-parity-persistent-send",
            "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
        ),
        step("rrs-parity-persistent-drain", "for (@d <= a) { Nil }"),
    ];
    observe(
        "persistent_operations",
        run_scenario(&steps, Budget::Token).await,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_parity_legacy_budget() {
    // Decision 45, Option P: the deploys of a legacy budget pay the legacy charges of the
    // equivalent host program.
    let steps = [
        step("rrs-parity-legacy-sends", "a!(1) | a!(2) | b!(3)"),
        step(
            "rrs-parity-legacy-receives",
            "for (@x <- a) { Nil } | for (@y <- b) { Nil }",
        ),
        step("rrs-parity-legacy-peek", "for (@x <<- a) { Nil }"),
        step(
            "rrs-parity-legacy-persistent",
            "b!!(4) | for (@z <- b) { Nil } | for (@w <- b) { Nil }",
        ),
        step("rrs-parity-legacy-drain", "for (@d <= a) { Nil }"),
    ];
    observe("legacy_budget", run_scenario(&steps, Budget::Legacy).await);
}
