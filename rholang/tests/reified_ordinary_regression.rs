use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::casper::{
    event_proto, CommEventProto, ConsumeEventProto, EventProto, PeekProto, ProduceEventProto,
};
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};
use models::rust::casper::protocol::casper_message::{
    CommEvent, ConsumeEvent, Event as CasperEvent, Peek, ProduceEvent,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
use proptest::prelude::*;
use proptest::sample::{select, subsequence};
use proptest::strategy::{Union, ValueTree};
use proptest::test_runner::{RngSeed, TestRunner};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
    NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{NativeRuntimeConfig, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_runtime::{RhoHistoryRepository, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use serde_json::{json, Map, Value};

const CAPTURE_DIRECTORY: &str = "RRS_CAPTURE_ORDINARY_OUT";
const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    // Changed by decision 112: the fields change the vector of this tree, and decision 120 keeps
    // v1 frozen as evidence for the masked comparison (ruling 6). The text before:
    // "/../test-vectors/ordinary-execution-v1.json"
    "/../test-vectors/ordinary-execution-v2.json"
);
const DEPLOY_SIGNATURE: &[u8] = b"rrs-ordinary-execution-v1-deploy";
const FUNDING_SIGNATURE: &[u8] = b"rrs-ordinary-execution-v1-payer";
const EVALUATION_PHLO: i64 = 100_000_000;
const NATIVE_LIMIT: u64 = 1_000_000;
const NATIVE_WEIGHTS: [u64; 4] = [2, 3, 5, 7];

const R1_CONSUME_DATA: &str =
    r#"@"rrs-r1-consume-a"!(1) | @"rrs-r1-consume-b"!(2) | @"rrs-r1-consume-c"!(3)"#;
const R1_CONSUME_RECEIVE: &str = r#"for (@x <- @"rrs-r1-consume-a" & @y <- @"rrs-r1-consume-b" & @z <- @"rrs-r1-consume-c") { @"rrs-r1-consume-out"!(x + y + z) }"#;
const R1_CONSUME_CHANNELS: [&str; 3] = ["rrs-r1-consume-a", "rrs-r1-consume-b", "rrs-r1-consume-c"];
const R1_PRODUCE_RECEIVE: &str = r#"for (@x <- @"rrs-r1-produce-a" & @y <- @"rrs-r1-produce-b" & @z <- @"rrs-r1-produce-c") { @"rrs-r1-produce-out"!(x + y + z) }"#;
const R1_PRODUCE_SENDS: [&str; 3] = [
    r#"@"rrs-r1-produce-c"!(3)"#,
    r#"@"rrs-r1-produce-a"!(1)"#,
    r#"@"rrs-r1-produce-b"!(2)"#,
];
const R1_PRODUCE_CHANNELS: [&str; 3] = ["rrs-r1-produce-a", "rrs-r1-produce-b", "rrs-r1-produce-c"];

const R2_DATA_FIRST_SEND: &str = r#"@"rrs-r2-data-first"!(42)"#;
const R2_DATA_FIRST_PEEK: &str =
    r#"for (@x <<- @"rrs-r2-data-first") { @"rrs-r2-data-first-seen"!(x) }"#;
const R2_CONTINUATION_FIRST_PEEK: &str =
    r#"for (@x <<- @"rrs-r2-continuation-first") { @"rrs-r2-continuation-first-seen"!(x) }"#;
const R2_CONTINUATION_FIRST_SEND: &str = r#"@"rrs-r2-continuation-first"!(42)"#;

const R3_SAME_CHANNEL_JOIN: &str =
    r#"for (@[y, 6] <- @"rrs-r3-greedy" & @[1, x] <- @"rrs-r3-greedy") { Nil }"#;
// The witness of greedy joins needs `[1, 6]` before the other datum in the
// canonical candidate order, so that the bind `[1, x]` takes it first. The
// order follows the source hashes of the two produces (DR-75). Since DR-129
// play picks at random again, and E1 keeps this order for a channel that a join
// names twice, as here (`order_repeated_channels`). The data
// `[1, 5]` were the witness under the digest order before the merge of
// feature/cost-accounted-rho at 4d67b5ccc. Under the canonical order, `[1, 1]`
// and `[1, 2]` are witnesses, and `[1, 0]`, `[1, 3]`, `[1, 4]`, `[1, 5]`,
// `[1, 7]`, `[1, 8]` and `[1, 9]` are not (measured at 4d67b5ccc).
// At 702496775 the source hashes of the two produces changed, as section R1
// shows. Only `[1, 8]` and `[1, 9]` are witnesses there, and `[1, 0]` to
// `[1, 5]` and `[1, 7]` are not (measured in a clean export of 702496775).
// Changed by the merge of feature/cost-accounted-rho at 702496775: the datum
// `[1, 1]` no longer makes the greedy matcher fail.
// const R3_GREEDY_DATA: &str = r#"@"rrs-r3-greedy"!([1, 1]) | @"rrs-r3-greedy"!([1, 6])"#;
const R3_GREEDY_DATA: &str = r#"@"rrs-r3-greedy"!([1, 8]) | @"rrs-r3-greedy"!([1, 6])"#;
const R3_GREEDY_RECEIVE: &str = r#"for (@left, @right <- @"rrs-r3-greedy-names") { for (@[y, 6] <- @left & @[1, x] <- @right) { @"rrs-r3-greedy-out"!(x, y) } } | @"rrs-r3-greedy-names"!("rrs-r3-greedy", "rrs-r3-greedy")"#;
const R3_CONTROL_DATA: &str = r#"@"rrs-r3-control"!([1, 9]) | @"rrs-r3-control"!([7, 6])"#;
const R3_CONTROL_RECEIVE: &str = r#"for (@left, @right <- @"rrs-r3-control-names") { for (@[y, 6] <- @left & @[1, x] <- @right) { @"rrs-r3-control-out"!(x, y) } } | @"rrs-r3-control-names"!("rrs-r3-control", "rrs-r3-control")"#;

const R4_PROGRAM: &str = r#"@"rrs-r4-data"!(7, "seven") | @"rrs-r4-data"!!(8) | for (@a <- @"rrs-r4-wait") { @"rrs-r4-never"!(a) } | for (@b <- @"rrs-r4-join-left" & @c <- @"rrs-r4-join-right") { @"rrs-r4-never"!(b, c) }"#;

const R7_TERMS: [(&str, &str); 20] = [
    (
        "send_many_arguments",
        r#"@"rrs-r7-send"!(1, "two", true, [3, 4], (5, 6))"#,
    ),
    ("persistent_send", r#"@"rrs-r7-persistent"!!(7)"#),
    (
        "receive_with_patterns",
        r#"for (@[x, y], _ <- @"rrs-r7-receive") { @"rrs-r7-out"!(x + y) }"#,
    ),
    (
        "receive_with_remainder",
        r#"for (@first, ...@rest <- @"rrs-r7-remainder") { @"rrs-r7-out"!(first, rest) }"#,
    ),
    (
        "join",
        r#"for (@x <- @"rrs-r7-left" & @y <- @"rrs-r7-right") { @"rrs-r7-out"!(x, y) }"#,
    ),
    (
        "persistent_and_peek_receive",
        r#"for (@x <= @"rrs-r7-persistent-receive") { Nil } | for (@y <<- @"rrs-r7-peek") { Nil }"#,
    ),
    (
        "new_names",
        r#"new a, b, c in { a!(*b) | b!(*c) | for (@x <- c) { Nil } }"#,
    ),
    (
        "new_with_uri",
        r#"new stdout(`rho:io:stdout`) in { stdout!("rrs-r7") }"#,
    ),
    (
        "match_cases",
        r#"match [1, 2] {
  [a, b] => @"rrs-r7-out"!(a + b)
  _ => Nil
}"#,
    ),
    (
        "bundles",
        r#"new w, r, n, rw in { @"rrs-r7-bundle"!(bundle+ {*w}, bundle- {*r}, bundle0 {*n}, bundle {*rw}) }"#,
    ),
    (
        "map",
        r#"@"rrs-r7-map"!({"k": 1, 2: [3], "nested": {"z": Nil}})"#,
    ),
    ("set", r#"@"rrs-r7-set"!(Set(3, 1, 2), Set())"#),
    ("list", r#"@"rrs-r7-list"!([1, [2, 3], []])"#),
    ("tuple", r#"@"rrs-r7-tuple"!((1, 2), (1, 2, 3))"#),
    (
        "method_calls",
        r#"@"rrs-r7-method"!([1, 2, 3].nth(1), "abc".length(), {"a": 1}.get("a"), "x".toUtf8Bytes())"#,
    ),
    (
        "quote_and_eval",
        r#"for (x <- @"rrs-r7-quote") { *x | @{*x | Nil}!(1) }"#,
    ),
    (
        "contract",
        r#"contract @"rrs-r7-add"(@a, @b, ret) = { ret!(a + b) }"#,
    ),
    (
        "tuple_process",
        r#"for (@x, @y <- @"rrs-r7-pair") { (x, y) }"#,
    ),
    (
        "operators",
        r#"@"rrs-r7-ops"!(1 + 2 * 3 - 4 / 2 % 3, not true or false and true, "a" ++ "b", 1 < 2, 2 <= 2, 3 > 1, 4 >= 4, 1 == 1, 1 != 2, -5)"#,
    ),
    (
        "if_else",
        r#"if (1 < 2) { @"rrs-r7-if"!(1) } else { @"rrs-r7-if"!(2) }"#,
    ),
];

const R8_PROGRAM: &str = r#"
@"rrs-r8-data"!(1, "one") |
@"rrs-r8-data"!(2, "two") |
@"rrs-r8-persistent-data"!!([3, 4]) |
for (@x <- @"rrs-r8-wait") { @"rrs-r8-never"!(x) } |
for (@a <- @"rrs-r8-join-left" & @b <- @"rrs-r8-join-right") { @"rrs-r8-never"!(a, b) } |
for (@y <= @"rrs-r8-persistent-wait") { Nil } |
for (@z <<- @"rrs-r8-peek-wait") { Nil } |
@"rrs-r8-consumed"!(5) |
for (@w <- @"rrs-r8-consumed") { @"rrs-r8-result"!(w * 2) } |
new private in { private!({"k": Set(5)}) | for (@v <- private & @u <- @"rrs-r8-join-right") { Nil } }
"#;

const R9_PROGRAM: &str = r#"
new loop, result, stdout(`rho:io:stdout`) in {
  contract loop(@n, @acc) = {
    match n {
      0 => result!(acc)
      _ => loop!(n - 1, acc + n)
    }
  } |
  loop!(4, 0) |
  for (@total <- result) {
    @"rrs-r9-total"!(total) |
    stdout!(["rrs-r9", total])
  } |
  @"rrs-r9-left"!(1) |
  @"rrs-r9-right"!(2) |
  for (@l <- @"rrs-r9-left" & @r <- @"rrs-r9-right") { @"rrs-r9-joined"!(l + r) } |
  @"rrs-r9-peek"!("seen") |
  for (@s <<- @"rrs-r9-peek") { @"rrs-r9-peeked"!(s) } |
  @"rrs-r9-persistent"!!(10) |
  for (@p <- @"rrs-r9-persistent") { @"rrs-r9-persistent-seen"!(p) }
}
"#;

const DC_CANDIDATES: usize = 6;
const DC_DATA_CHANNEL: &str = "rrs-dc-data";
const DC_DATA_SOURCE: &str = r#"@"rrs-dc-data"!(0) | @"rrs-dc-data"!(1) | @"rrs-dc-data"!(2) | @"rrs-dc-data"!(3) | @"rrs-dc-data"!(4) | @"rrs-dc-data"!(5)"#;
const DC_TAKE_SOURCE: &str = r#"for (@x <- @"rrs-dc-data") { Nil }"#;
const DC_CONTINUATION_CHANNEL: &str = "rrs-dc-cont";
const DC_CONTINUATION_SOURCE: &str = r#"for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(0, x) } | for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(1, x) } | for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(2, x) } | for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(3, x) } | for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(4, x) } | for (@x <- @"rrs-dc-cont") { @"rrs-dc-fired"!(5, x) }"#;

struct CostProgram {
    name: &'static str,
    source: &'static str,
    phlo: i64,
    native_limit: u64,
    /// Added by decision 93: the journal rows of the program follow the arrival order of its
    /// parallel processes. A persistent operation that fires a waiting partner is issued again
    /// (dev's `dispatchAndRun`, `reduce.rs` `continue_produce_process` and
    /// `continue_consume_process`), and a loop stops at the first operation that its budget
    /// refuses. Racing evaluation decides that order (DR-132), so the corpus keeps no row count
    /// and no COMM row of such a program.
    rows_follow_arrival: bool,
}

const R6_PROGRAMS: [CostProgram; 11] = [
    CostProgram {
        name: "send",
        source: r#"@"rrs-r6-send"!(1)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "receive",
        source: r#"for (@x <- @"rrs-r6-receive") { Nil }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "persistent_send",
        source: r#"@"rrs-r6-persistent-send"!!(1) | for (@x <- @"rrs-r6-persistent-send") { @"rrs-r6-persistent-send-out"!(x) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: true,
    },
    CostProgram {
        name: "persistent_receive",
        source: r#"for (@x <= @"rrs-r6-persistent-receive") { @"rrs-r6-persistent-receive-out"!(x) } | @"rrs-r6-persistent-receive"!(1) | @"rrs-r6-persistent-receive"!(2)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: true,
    },
    CostProgram {
        name: "join",
        source: r#"@"rrs-r6-join-left"!(1) | @"rrs-r6-join-right"!(2) | for (@x <- @"rrs-r6-join-left" & @y <- @"rrs-r6-join-right") { @"rrs-r6-join-out"!(x + y) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "peek",
        source: r#"@"rrs-r6-peek"!(1) | for (@x <<- @"rrs-r6-peek") { @"rrs-r6-peek-out"!(x) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "new",
        source: r#"new a, b in { a!(1) | for (@x <- a) { b!(x) } }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "match",
        source: r#"match [1, 2, 3] {
  [a, b, c] => @"rrs-r6-match"!(a + b + c)
  _ => Nil
}"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "arithmetic",
        source: r#"@"rrs-r6-arithmetic"!((7 * 6 - 2) / 4 % 3 + 10)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "system_process",
        source: r#"new stdout(`rho:io:stdout`) in { stdout!("rrs-r6-system-process") }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
        rows_follow_arrival: false,
    },
    CostProgram {
        name: "out_of_phlo",
        source: r#"new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) }"#,
        phlo: 20_000,
        native_limit: 20_000,
        rows_follow_arrival: true,
    },
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate lives inside the workspace")
        .to_path_buf()
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        other => other,
    }
}

fn observe(fragment: &str, section: &str, actual: Value) {
    let actual = canonical(actual);
    match std::env::var_os(CAPTURE_DIRECTORY) {
        Some(directory) => write_fragment(Path::new(&directory), fragment, section, actual),
        None => verify_section(section, &actual),
    }
}

fn write_fragment(directory: &Path, fragment: &str, section: &str, actual: Value) {
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
    document.insert(section.to_owned(), actual);
    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .expect("serialize the capture fragment");
    text.push('\n');
    std::fs::write(directory.join(format!("{fragment}.json")), text)
        .expect("write the capture fragment");
}

fn verify_section(section: &str, actual: &Value) {
    let text = std::fs::read_to_string(CORPUS).unwrap_or_else(|error| {
        panic!("read {CORPUS}: {error}; capture the corpus with {CAPTURE_DIRECTORY}")
    });
    let corpus: Value = serde_json::from_str(&text).expect("parse the ordinary execution corpus");
    let expected = corpus
        .get(section)
        .unwrap_or_else(|| panic!("{CORPUS} has no section {section}"));
    let mut mismatches = Vec::new();
    compare(section, expected, actual, &mut mismatches);
    assert!(
        mismatches.is_empty(),
        "HEAD ordinary execution differs from {CORPUS}:\n{}",
        mismatches.join("\n")
    );
}

fn compare(path: &str, expected: &Value, actual: &Value, mismatches: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys: BTreeSet<&String> = expected.keys().chain(actual.keys()).collect();
            for key in keys {
                let child = format!("{path}.{key}");
                match (expected.get(key), actual.get(key)) {
                    (Some(expected), Some(actual)) => compare(&child, expected, actual, mismatches),
                    (Some(expected), None) => mismatches.push(format!(
                        "{child}: expected {expected} but HEAD produced no value"
                    )),
                    (None, Some(actual)) => mismatches.push(format!(
                        "{child}: expected no value but HEAD produced {actual}"
                    )),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(expected_items), Value::Array(actual_items))
            if expected_items.len() == actual_items.len() =>
        {
            for (index, (expected, actual)) in expected_items.iter().zip(actual_items).enumerate() {
                compare(&format!("{path}[{index}]"), expected, actual, mismatches);
            }
        }
        _ if expected == actual => {}
        _ => mismatches.push(format!(
            "{path}: expected {expected} but HEAD produced {actual}"
        )),
    }
}

fn sorted(values: Vec<Value>) -> Vec<Value> {
    let mut values: Vec<Value> = values.into_iter().map(canonical).collect();
    values.sort_by_cached_key(|value| value.to_string());
    values
}

fn hex_list<T: AsRef<[u8]>>(items: &[T]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| Value::String(hex::encode(item)))
            .collect(),
    )
}

fn string_par(value: &str) -> Par { new_gstring_par(value.to_owned(), Vec::new(), false) }

fn seed(label: &str) -> Blake2b512Random { Blake2b512Random::create_from_bytes(label.as_bytes()) }

fn channel_hash_hex(channel: &Par) -> String { hex::encode(hash(channel).0) }

fn channel_labels(names: &[&str]) -> Value {
    Value::Object(
        names
            .iter()
            .map(|name| (channel_hash_hex(&string_par(name)), Value::from(*name)))
            .collect(),
    )
}

fn pretty(par: &Par) -> String { PrettyPrinter::new().build_string_from_message(par) }

fn pretty_channel(par: &Par) -> String { PrettyPrinter::new().build_channel_string(par) }

fn pretty_channels(channels: &[Par]) -> Vec<String> {
    channels.iter().map(pretty_channel).collect()
}

fn errors_json(errors: &[InterpreterError]) -> Value {
    Value::Array(
        errors
            .iter()
            .map(|error| Value::String(format!("{error:?}")))
            .collect(),
    )
}

// The renderers destructure without `..`, so a new event, datum, or proto field fails to compile.
fn produce_json(produce: &Produce) -> Value {
    let Produce {
        channel_hash,
        hash: produce_hash,
        persistent,
        is_deterministic,
        output_value,
        failed,
    } = produce;
    json!({
        "kind": "produce",
        "channel_hash": hex::encode(&channel_hash.0),
        "hash": hex::encode(&produce_hash.0),
        "persistent": persistent,
        "is_deterministic": is_deterministic,
        "output_value": hex_list(output_value),
        "failed": failed,
    })
}

fn consume_json(consume: &Consume) -> Value {
    let Consume {
        channel_hashes,
        hash: consume_hash,
        persistent,
    } = consume;
    json!({
        "kind": "consume",
        "channel_hashes": channel_hashes
            .iter()
            .map(|channel| hex::encode(&channel.0))
            .collect::<Vec<_>>(),
        "hash": hex::encode(&consume_hash.0),
        "persistent": persistent,
    })
}

fn comm_json(comm: &COMM) -> Value {
    let COMM {
        consume,
        produces,
        peeks,
        times_repeated,
    } = comm;
    json!({
        "kind": "comm",
        "consume": consume_json(consume),
        "produces": produces.iter().map(produce_json).collect::<Vec<_>>(),
        "peeks": peeks.iter().copied().collect::<Vec<_>>(),
        "times_repeated": times_repeated
            .iter()
            .map(|(produce, count)| json!([hex::encode(&produce.hash.0), count]))
            .collect::<Vec<_>>(),
    })
}

fn event_json(event: &Event) -> Value {
    match event {
        Event::Comm(comm) => comm_json(comm),
        Event::IoEvent(IOEvent::Produce(produce)) => produce_json(produce),
        Event::IoEvent(IOEvent::Consume(consume)) => consume_json(consume),
    }
}

fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::Comm(_) => "comm",
        Event::IoEvent(IOEvent::Produce(_)) => "produce",
        Event::IoEvent(IOEvent::Consume(_)) => "consume",
    }
}

fn events_json(log: &[Event]) -> Value { Value::Array(log.iter().map(event_json).collect()) }

fn kinds_json(log: &[Event]) -> Value {
    Value::Array(
        log.iter()
            .map(|event| Value::from(event_kind(event)))
            .collect(),
    )
}

// Added by decision 93: racing evaluation gives arrival-order logs (DR-132), so no test compares
// the order of a log of parallel processes across runs. Such a log is compared as a multiset: its
// events, or its event kinds, in one canonical order.
fn arrival_events_json(log: &[Event]) -> Value {
    let mut events: Vec<Value> = log.iter().map(event_json).collect();
    events.sort_by_cached_key(|event| event.to_string());
    Value::Array(events)
}

fn arrival_kinds_json(log: &[Event]) -> Value {
    let mut kinds: Vec<&'static str> = log.iter().map(event_kind).collect();
    kinds.sort_unstable();
    Value::Array(kinds.into_iter().map(Value::from).collect())
}

fn datum_json(datum: &Datum<ListParWithRandom>) -> Value {
    let Datum { a, persist, source } = datum;
    json!({
        "a": hex::encode(a.encode_to_vec()),
        "persist": persist,
        "source": produce_json(source),
    })
}

fn waiting_continuation_json(
    waiting: &WaitingContinuation<BindPattern, TaggedContinuation>,
) -> Value {
    let WaitingContinuation {
        patterns,
        continuation,
        persist,
        peeks,
        source,
    } = waiting;
    json!({
        "patterns": patterns
            .iter()
            .map(|pattern| hex::encode(pattern.encode_to_vec()))
            .collect::<Vec<_>>(),
        "continuation": hex::encode(continuation.encode_to_vec()),
        "persist": persist,
        "peeks": peeks.iter().copied().collect::<Vec<_>>(),
        "source": consume_json(source),
    })
}

fn produce_event_proto_json(proto: &ProduceEventProto) -> Value {
    let ProduceEventProto {
        channels_hash,
        hash: produce_hash,
        persistent,
        times_repeated,
        is_deterministic,
        output_value,
        failed,
    } = proto;
    json!({
        "channels_hash": hex::encode(channels_hash),
        "hash": hex::encode(produce_hash),
        "persistent": persistent,
        "times_repeated": times_repeated,
        "is_deterministic": is_deterministic,
        "output_value": hex_list(output_value),
        "failed": failed,
    })
}

fn consume_event_proto_json(proto: &ConsumeEventProto) -> Value {
    let ConsumeEventProto {
        channels_hashes,
        hash: consume_hash,
        persistent,
    } = proto;
    json!({
        "channels_hashes": hex_list(channels_hashes),
        "hash": hex::encode(consume_hash),
        "persistent": persistent,
    })
}

fn comm_event_proto_json(proto: &CommEventProto) -> Value {
    let CommEventProto {
        consume,
        produces,
        peeks,
    } = proto;
    json!({
        "consume": consume.as_ref().map(consume_event_proto_json),
        "produces": produces.iter().map(produce_event_proto_json).collect::<Vec<_>>(),
        "peeks": peeks
            .iter()
            .map(|PeekProto { channel_index }| *channel_index)
            .collect::<Vec<_>>(),
    })
}

fn event_proto_json(proto: &EventProto) -> Value {
    let EventProto { event_instance } = proto;
    match event_instance {
        Some(event_proto::EventInstance::Produce(produce)) => {
            json!({ "produce": produce_event_proto_json(produce) })
        }
        Some(event_proto::EventInstance::Consume(consume)) => {
            json!({ "consume": consume_event_proto_json(consume) })
        }
        Some(event_proto::EventInstance::Comm(comm)) => {
            json!({ "comm": comm_event_proto_json(comm) })
        }
        None => Value::Null,
    }
}

fn produce_event_json(event: &ProduceEvent) -> Value {
    let ProduceEvent {
        channels_hash,
        hash: produce_hash,
        persistent,
        times_repeated,
        is_deterministic,
        output_value,
        failed,
    } = event;
    json!({
        "channels_hash": hex::encode(channels_hash),
        "hash": hex::encode(produce_hash),
        "persistent": persistent,
        "times_repeated": times_repeated,
        "is_deterministic": is_deterministic,
        "output_value": hex_list(output_value),
        "failed": failed,
    })
}

fn consume_event_json(event: &ConsumeEvent) -> Value {
    let ConsumeEvent {
        channels_hashes,
        hash: consume_hash,
        persistent,
    } = event;
    json!({
        "channels_hashes": hex_list(channels_hashes),
        "hash": hex::encode(consume_hash),
        "persistent": persistent,
    })
}

fn casper_event_json(event: &CasperEvent) -> Value {
    match event {
        CasperEvent::Produce(produce) => json!({ "produce": produce_event_json(produce) }),
        CasperEvent::Consume(consume) => json!({ "consume": consume_event_json(consume) }),
        CasperEvent::Comm(CommEvent {
            consume,
            produces,
            peeks,
        }) => json!({
            "comm": {
                "consume": consume_event_json(consume),
                "produces": produces.iter().map(produce_event_json).collect::<Vec<_>>(),
                "peeks": peeks
                    .iter()
                    .map(|Peek { channel_index }| *channel_index)
                    .collect::<Vec<_>>(),
            }
        }),
    }
}

fn produce_event_proto(produce: &Produce, times_repeated: i32) -> ProduceEventProto {
    ProduceEventProto {
        channels_hash: produce.channel_hash.to_bytes_prost(),
        hash: produce.hash.to_bytes_prost(),
        persistent: produce.persistent,
        times_repeated,
        is_deterministic: produce.is_deterministic,
        output_value: produce
            .output_value
            .iter()
            .cloned()
            .map(Into::into)
            .collect(),
        failed: produce.failed,
    }
}

fn consume_event_proto(consume: &Consume) -> ConsumeEventProto {
    ConsumeEventProto {
        channels_hashes: consume
            .channel_hashes
            .iter()
            .map(Blake2b256Hash::to_bytes_prost)
            .collect(),
        hash: consume.hash.to_bytes_prost(),
        persistent: consume.persistent,
    }
}

fn event_proto(event: &Event) -> EventProto {
    let event_instance = match event {
        Event::Comm(comm) => event_proto::EventInstance::Comm(CommEventProto {
            consume: Some(consume_event_proto(&comm.consume)),
            produces: comm
                .produces
                .iter()
                .map(|produce| {
                    produce_event_proto(produce, *comm.times_repeated.get(produce).unwrap_or(&0))
                })
                .collect(),
            peeks: comm
                .peeks
                .iter()
                .map(|channel_index| PeekProto {
                    channel_index: *channel_index,
                })
                .collect(),
        }),
        Event::IoEvent(IOEvent::Produce(produce)) => {
            event_proto::EventInstance::Produce(produce_event_proto(produce, 0))
        }
        Event::IoEvent(IOEvent::Consume(consume)) => {
            event_proto::EventInstance::Consume(consume_event_proto(consume))
        }
    };
    EventProto {
        event_instance: Some(event_instance),
    }
}

fn casper_produce(produce: &Produce, times_repeated: i32) -> ProduceEvent {
    ProduceEvent {
        channels_hash: produce.channel_hash.to_bytes_prost(),
        hash: produce.hash.to_bytes_prost(),
        persistent: produce.persistent,
        times_repeated,
        is_deterministic: produce.is_deterministic,
        output_value: produce
            .output_value
            .iter()
            .cloned()
            .map(Into::into)
            .collect(),
        failed: produce.failed,
    }
}

fn casper_consume(consume: &Consume) -> ConsumeEvent {
    ConsumeEvent {
        channels_hashes: consume
            .channel_hashes
            .iter()
            .map(Blake2b256Hash::to_bytes_prost)
            .collect(),
        hash: consume.hash.to_bytes_prost(),
        persistent: consume.persistent,
    }
}

fn casper_event(event: &Event) -> CasperEvent {
    match event {
        Event::Comm(COMM {
            consume,
            produces,
            peeks,
            times_repeated,
        }) => CasperEvent::Comm(CommEvent {
            consume: casper_consume(consume),
            produces: produces
                .iter()
                .map(|produce| casper_produce(produce, *times_repeated.get(produce).unwrap_or(&0)))
                .collect(),
            peeks: peeks
                .iter()
                .map(|channel_index| Peek {
                    channel_index: *channel_index,
                })
                .collect(),
        }),
        Event::IoEvent(IOEvent::Produce(produce)) => {
            CasperEvent::Produce(casper_produce(produce, 0))
        }
        Event::IoEvent(IOEvent::Consume(consume)) => CasperEvent::Consume(casper_consume(consume)),
    }
}

struct Runtimes {
    play: RhoRuntimeImpl,
    replay: RhoRuntimeImpl,
    history: RhoHistoryRepository,
}

async fn fresh_runtimes() -> Runtimes {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("create in-memory rspace stores");
    let (play, replay, history) = create_runtimes(stores, false, &mut Vec::new()).await;
    for runtime in [&play, &replay] {
        runtime
            .cost
            .set_deploy_signature_funded(DEPLOY_SIGNATURE, Sig::Ground(FUNDING_SIGNATURE.to_vec()));
    }
    Runtimes {
        play,
        replay,
        history,
    }
}

async fn evaluate(runtime: &mut RhoRuntimeImpl, source: &str, label: &str) -> Vec<Event> {
    let result = runtime
        .evaluate(
            source,
            Cost::create(EVALUATION_PHLO, label.to_owned()),
            HashMap::new(),
            seed(label),
        )
        .await
        .unwrap_or_else(|error| panic!("{label} evaluation failed: {error:?}"));
    assert!(
        result.errors.is_empty(),
        "{label} reported errors: {:?}",
        result.errors
    );
    runtime.take_event_log().await
}

fn produces_in(log: &[Event]) -> Vec<Produce> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) => Some(produce.clone()),
            _ => None,
        })
        .collect()
}

fn comms_in(log: &[Event]) -> Vec<&COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm),
            _ => None,
        })
        .collect()
}

fn single_comm(log: &[Event]) -> &COMM {
    match comms_in(log).as_slice() {
        [comm] => comm,
        other => panic!("expected exactly one COMM, found {}", other.len()),
    }
}

fn receive_bind_channels(source: &str) -> Vec<Par> {
    let par = Compiler::source_to_adt(source).expect("normalize the receive");
    match par.receives.as_slice() {
        [receive] => receive
            .binds
            .iter()
            .map(|bind| bind.source.clone().expect("receive bind has a source"))
            .collect(),
        other => panic!("expected one receive, found {}", other.len()),
    }
}

fn aliased_join_patterns(source: &str) -> Vec<BindPattern> {
    let par = Compiler::source_to_adt(source).expect("normalize the aliasing receive");
    let outer = match par.receives.as_slice() {
        [outer] => outer,
        other => panic!("expected one aliasing receive, found {}", other.len()),
    };
    let body = outer
        .body
        .as_ref()
        .expect("the aliasing receive has a body");
    match body.receives.as_slice() {
        [join] => join
            .binds
            .iter()
            .map(|bind| BindPattern {
                patterns: bind.patterns.clone(),
                remainder: bind.remainder,
                free_count: bind.free_count,
            })
            .collect(),
        other => panic!("expected one aliased join, found {}", other.len()),
    }
}

fn candidate(
    channel: &Par,
    source: &Produce,
    index: i32,
) -> ConsumeCandidate<Par, ListParWithRandom> {
    ConsumeCandidate {
        channel: channel.clone(),
        datum: Datum {
            a: ListParWithRandom::default(),
            persist: source.persistent,
            source: source.clone(),
        },
        removed_datum: ListParWithRandom::default(),
        datum_index: index,
    }
}

fn produce_channel_order(produces: &[Produce]) -> Vec<String> {
    produces
        .iter()
        .map(|produce| hex::encode(&produce.channel_hash.0))
        .collect()
}

fn r1_section(
    channel_names: &[&str],
    bind_channels: &[Par],
    evaluated: &[Produce],
    comm: &COMM,
    logs: Value,
    // Added by decision 93: true when one evaluation ran the produces in parallel, so that their
    // evaluation order follows their arrival (DR-132).
    evaluation_races: bool,
) -> Value {
    let channel_of = |produce: &Produce| {
        bind_channels
            .iter()
            .find(|channel| hash(*channel) == produce.channel_hash)
            .expect("every COMM produce belongs to a bound channel")
            .clone()
    };
    let source_of = |channel: &Par| {
        let channel_hash = hash(channel);
        evaluated
            .iter()
            .find(|produce| produce.channel_hash == channel_hash)
            .expect("every bound channel received one produce")
            .clone()
    };
    let bind_candidates: Vec<_> = bind_channels
        .iter()
        .enumerate()
        .map(|(index, channel)| candidate(channel, &source_of(channel), index as i32))
        .collect();
    let evaluation_candidates: Vec<_> = evaluated
        .iter()
        .enumerate()
        .map(|(index, produce)| candidate(&channel_of(produce), produce, index as i32))
        .collect();
    let rebuilt_from_binds = COMM::new(
        &bind_candidates,
        comm.consume.clone(),
        comm.peeks.clone(),
        |_| comm.times_repeated.clone(),
    );
    let rebuilt_from_evaluation = COMM::new(
        &evaluation_candidates,
        comm.consume.clone(),
        comm.peeks.clone(),
        |_| comm.times_repeated.clone(),
    );
    assert_eq!(comm_json(&rebuilt_from_binds), comm_json(comm));
    assert_eq!(comm_json(&rebuilt_from_evaluation), comm_json(comm));
    let comm_order = produce_channel_order(&comm.produces);
    let evaluation_order = produce_channel_order(evaluated);
    let bind_order: Vec<String> = bind_channels.iter().map(channel_hash_hex).collect();
    // Changed by decision 93: racing evaluation decides the order of parallel produces, so that
    // order can equal the canonical COMM order. Each run then checks that the parallel produces
    // are the produces of the COMM, and the corpus keeps the evaluation order of sequential steps
    // only.
    // assert_ne!(
    //     evaluation_order, comm_order,
    //     "the evaluation order must differ from the canonical COMM order"
    // );
    match evaluation_races {
        true => {
            let mut arrived = evaluation_order.clone();
            arrived.sort_unstable();
            let mut canonical = comm_order.clone();
            canonical.sort_unstable();
            assert_eq!(
                arrived, canonical,
                "the parallel produces are the produces of the COMM"
            );
        }
        false => assert_ne!(
            evaluation_order, comm_order,
            "the evaluation order must differ from the canonical COMM order"
        ),
    }
    assert_ne!(
        bind_order, comm_order,
        "the bind order must differ from the canonical COMM order"
    );
    // json!({
    //     "channels": channel_labels(channel_names),
    //     "evaluation_order": evaluation_order,
    //     "bind_order": bind_order,
    //     "comm_produce_order": comm_order,
    //     "comm_equals_comm_new_canonical_sort": true,
    //     "comm": comm_json(comm),
    //     "logs": logs,
    // })
    let mut section = json!({
        "channels": channel_labels(channel_names),
        "bind_order": bind_order,
        "comm_produce_order": comm_order,
        "comm_equals_comm_new_canonical_sort": true,
        "comm": comm_json(comm),
        "logs": logs,
    });
    if !evaluation_races {
        section["evaluation_order"] = json!(evaluation_order);
    }
    section
}

async fn r1_consume_triggered() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let data_log = evaluate(&mut play, R1_CONSUME_DATA, "rrs-r1-consume-data").await;
    let receive_log = evaluate(&mut play, R1_CONSUME_RECEIVE, "rrs-r1-consume-receive").await;
    let evaluated = produces_in(&data_log);
    assert_eq!(evaluated.len(), 3, "the data step stores three produces");
    let comm = single_comm(&receive_log);
    let bind_channels = receive_bind_channels(R1_CONSUME_RECEIVE);
    let mut names = R1_CONSUME_CHANNELS.to_vec();
    names.push("rrs-r1-consume-out");
    r1_section(
        &names,
        &bind_channels,
        &evaluated,
        comm,
        // Changed by decision 93: the three sends of the data step run in parallel, so its log
        // follows their arrival, and the corpus keeps it as a multiset.
        // json!({
        //     "data": {
        //         "kinds": kinds_json(&data_log),
        //         "events": events_json(&data_log),
        //     },
        json!({
            "data": {
                "kinds": arrival_kinds_json(&data_log),
                "events": arrival_events_json(&data_log),
            },
            "receive": {
                "kinds": kinds_json(&receive_log),
                "events": events_json(&receive_log),
            },
        }),
        true,
    )
}

async fn r1_produce_triggered() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let receive_log = evaluate(&mut play, R1_PRODUCE_RECEIVE, "rrs-r1-produce-receive").await;
    assert!(
        comms_in(&receive_log).is_empty(),
        "the receive waits for data"
    );
    let mut send_logs = Vec::with_capacity(R1_PRODUCE_SENDS.len());
    for (index, source) in R1_PRODUCE_SENDS.iter().enumerate() {
        send_logs.push(evaluate(&mut play, source, &format!("rrs-r1-produce-send-{index}")).await);
    }
    let bind_channels = receive_bind_channels(R1_PRODUCE_RECEIVE);
    let bound: BTreeSet<Blake2b256Hash> = bind_channels.iter().map(hash).collect();
    let evaluated: Vec<Produce> = send_logs
        .iter()
        .flat_map(|log| produces_in(log))
        .filter(|produce| bound.contains(&produce.channel_hash))
        .collect();
    assert_eq!(
        evaluated.len(),
        3,
        "each send step evaluates one bound produce"
    );
    let triggering_log = send_logs.last().expect("three send steps");
    let comm = single_comm(triggering_log);
    let mut names = R1_PRODUCE_CHANNELS.to_vec();
    names.push("rrs-r1-produce-out");
    let sends: Vec<Value> = R1_PRODUCE_SENDS
        .iter()
        .zip(&send_logs)
        .map(|(source, log)| {
            json!({
                "source": source,
                "kinds": kinds_json(log),
                "events": events_json(log),
            })
        })
        .collect();
    r1_section(
        &names,
        &bind_channels,
        &evaluated,
        comm,
        json!({
            "receive": {
                "kinds": kinds_json(&receive_log),
                "events": events_json(&receive_log),
            },
            "sends": sends,
        }),
        false,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r1_comm_produce_sort() {
    let consume_triggered = r1_consume_triggered().await;
    let produce_triggered = r1_produce_triggered().await;
    observe(
        "r1",
        "r1_comm_produce_sort",
        json!({
            "consume_triggered": consume_triggered,
            "produce_triggered": produce_triggered,
        }),
    );
}

fn r2_reproduce_section(
    channel: &Par,
    original: &Produce,
    peek_log: &[Event],
    remaining: &[Datum<ListParWithRandom>],
) -> Value {
    let comm = single_comm(peek_log);
    assert_eq!(
        comm.peeks,
        BTreeSet::from([0]),
        "the peek COMM records bind 0"
    );
    let comm_position = peek_log
        .iter()
        .position(|event| matches!(event, Event::Comm(_)))
        .expect("the peek step logs a COMM");
    let channel_hash = hash(channel);
    let reproduced: Vec<&Produce> = peek_log[comm_position + 1..]
        .iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if produce.channel_hash == channel_hash => {
                Some(produce)
            }
            _ => None,
        })
        .collect();
    let reproduce = match reproduced.as_slice() {
        [reproduce] => *reproduce,
        other => panic!(
            "expected one re-produce after the peek COMM, found {}",
            other.len()
        ),
    };
    assert_eq!(
        reproduce.hash, original.hash,
        "the reducer re-produces the peeked datum unchanged"
    );
    let datum = match remaining {
        [datum] => datum,
        other => panic!(
            "expected exactly one datum after the peek, found {}",
            other.len()
        ),
    };
    assert_eq!(datum.source.hash, original.hash);
    json!({
        "comm_peeks": comm.peeks.iter().copied().collect::<Vec<_>>(),
        "reproduce_hash_equals_original": true,
        "reproduce": produce_json(reproduce),
        "remaining_data_count": remaining.len(),
        "remaining_datum": {
            "pars": datum.a.pars.iter().map(pretty).collect::<Vec<_>>(),
            "persist": datum.persist,
            "source": produce_json(&datum.source),
        },
    })
}

async fn r2_data_first() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let channel = string_par("rrs-r2-data-first");
    let send_log = evaluate(&mut play, R2_DATA_FIRST_SEND, "rrs-r2-data-first-send").await;
    let original = match produces_in(&send_log).as_slice() {
        [produce] => produce.clone(),
        other => panic!("expected one produce, found {}", other.len()),
    };
    let peek_log = evaluate(&mut play, R2_DATA_FIRST_PEEK, "rrs-r2-data-first-peek").await;
    let remaining = play.get_data(&channel).await;
    let seen = play.get_data(&string_par("rrs-r2-data-first-seen")).await;
    assert_eq!(seen.len(), 1, "the peek body runs once");
    json!({
        "channels": channel_labels(&["rrs-r2-data-first", "rrs-r2-data-first-seen"]),
        "send": { "kinds": kinds_json(&send_log), "events": events_json(&send_log) },
        // Changed by decision 93 (DR-132): the peek COMM runs its body and its re-produce in
        // parallel, so this log is a multiset. The text before:
        // "peek": { "kinds": kinds_json(&peek_log), "events": events_json(&peek_log) },
        "peek": {
            "kinds": arrival_kinds_json(&peek_log),
            "events": arrival_events_json(&peek_log),
        },
        "outcome": r2_reproduce_section(&channel, &original, &peek_log, &remaining),
        "seen_data_count": seen.len(),
    })
}

async fn r2_continuation_first() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let channel = string_par("rrs-r2-continuation-first");
    let peek_log = evaluate(
        &mut play,
        R2_CONTINUATION_FIRST_PEEK,
        "rrs-r2-continuation-first-peek",
    )
    .await;
    let waiting = play.get_continuations(vec![channel.clone()]).await;
    let waiting = match waiting.as_slice() {
        [waiting] => waiting.clone(),
        other => panic!("expected one waiting peek, found {}", other.len()),
    };
    assert_eq!(
        waiting.peeks,
        BTreeSet::from([0]),
        "the waiting consume carries peek 0"
    );
    let send_log = evaluate(
        &mut play,
        R2_CONTINUATION_FIRST_SEND,
        "rrs-r2-continuation-first-send",
    )
    .await;
    let original = produces_in(&send_log)
        .first()
        .cloned()
        .expect("the send step logs its produce first");
    let remaining = play.get_data(&channel).await;
    let seen = play
        .get_data(&string_par("rrs-r2-continuation-first-seen"))
        .await;
    assert_eq!(seen.len(), 1, "the peek body runs once");
    assert!(
        play.get_continuations(vec![channel.clone()])
            .await
            .is_empty(),
        "the linear peek continuation is consumed"
    );
    json!({
        "channels": channel_labels(&[
            "rrs-r2-continuation-first",
            "rrs-r2-continuation-first-seen",
        ]),
        "peek": { "kinds": kinds_json(&peek_log), "events": events_json(&peek_log) },
        "waiting_peeks": waiting.peeks.iter().copied().collect::<Vec<_>>(),
        "waiting_persist": waiting.persist,
        // Changed by decision 93 (DR-132): the peek COMM runs its body and its re-produce in
        // parallel, so this log is a multiset. The text before:
        // "send": { "kinds": kinds_json(&send_log), "events": events_json(&send_log) },
        "send": {
            "kinds": arrival_kinds_json(&send_log),
            "events": arrival_events_json(&send_log),
        },
        "outcome": r2_reproduce_section(&channel, &original, &send_log, &remaining),
        "seen_data_count": seen.len(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r2_peek_removes_and_reproduces() {
    let data_first = r2_data_first().await;
    let continuation_first = r2_continuation_first().await;
    observe(
        "r2",
        "r2_peek_removes_and_reproduces",
        json!({
            "data_first": data_first,
            "continuation_first": continuation_first,
        }),
    );
}

fn bincode_hash<T: serde::Serialize>(record: &T) -> Blake2b256Hash {
    Blake2b256Hash::new(&bincode::serialize(record).expect("bincode-serialize a candidate"))
}

// The hash of the source produce or consume of a stored candidate.
trait CandidateSourceHash {
    fn source_hash(&self) -> &Blake2b256Hash;
}

impl<A: Clone> CandidateSourceHash for Datum<A> {
    fn source_hash(&self) -> &Blake2b256Hash { &self.source.hash }
}

impl<P: Clone, K: Clone> CandidateSourceHash for WaitingContinuation<P, K> {
    fn source_hash(&self) -> &Blake2b256Hash { &self.source.hash }
}

// The host's candidate ranking, computed in the test with the rule of
// candidate_order.rs (DR-75 of feature/cost-accounted-rho, the base since the
// merge at 4d67b5ccc): ascending (source hash, Blake2b256(bincode(record)),
// index). The digest decides only between equal source hashes. Each entry is
// the digest and the store index of a record.
fn canonical_rank<T: serde::Serialize + CandidateSourceHash>(
    records: &[T],
) -> Vec<(Blake2b256Hash, usize)> {
    let mut keyed: Vec<(Blake2b256Hash, Blake2b256Hash, usize)> = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.source_hash().clone(), bincode_hash(record), index))
        .collect();
    keyed.sort();
    keyed
        .into_iter()
        .map(|(_, digest, index)| (digest, index))
        .collect()
}

// Disabled by the merge of feature/cost-accounted-rho at 4d67b5ccc: the host
// no longer ranks candidates by their digest alone (DR-75), so canonical_rank
// replaces this oracle.
// fn bincode_rank<T: serde::Serialize>(records: &[T]) -> Vec<(Blake2b256Hash, usize)> {
//     let mut keyed: Vec<(Blake2b256Hash, usize)> = records
//         .iter()
//         .enumerate()
//         .map(|(index, record)| (bincode_hash(record), index))
//         .collect();
//     keyed.sort();
//     keyed
// }

fn deterministic_order(data: &[Datum<ListParWithRandom>]) -> Vec<usize> {
    canonical_rank(data)
        .into_iter()
        .map(|(_, index)| index)
        .collect()
}

fn greedy_assignment(matches: &[Vec<bool>]) -> Option<Vec<usize>> {
    let mut used = vec![false; matches.first().map_or(0, Vec::len)];
    let mut assignment = Vec::with_capacity(matches.len());
    for row in matches {
        let chosen = row
            .iter()
            .enumerate()
            .find(|(datum, matched)| **matched && !used[*datum])
            .map(|(datum, _)| datum)?;
        used[chosen] = true;
        assignment.push(chosen);
    }
    Some(assignment)
}

fn backtracking_assignment(matches: &[Vec<bool>], used: &mut [bool], bind: usize) -> bool {
    match matches.get(bind) {
        None => true,
        Some(row) => (0..row.len()).any(|datum| {
            if !row[datum] || used[datum] {
                return false;
            }
            used[datum] = true;
            let found = backtracking_assignment(matches, used, bind + 1);
            used[datum] = false;
            found
        }),
    }
}

async fn r3_join(
    channel_name: &str,
    output_name: &str,
    data_source: &str,
    receive_source: &str,
    label: &str,
) -> (Value, bool) {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let channel = string_par(channel_name);
    let data_log = evaluate(&mut play, data_source, &format!("{label}-data")).await;
    let stored = play.get_data(&channel).await;
    assert_eq!(
        stored.len(),
        2,
        "both data are stored before the join arrives"
    );
    let order = deterministic_order(&stored);
    let ordered: Vec<&Datum<ListParWithRandom>> =
        order.iter().map(|index| &stored[*index]).collect();
    let patterns = aliased_join_patterns(receive_source);
    let matches: Vec<Vec<bool>> = patterns
        .iter()
        .map(|pattern| {
            ordered
                .iter()
                .map(|datum| {
                    Match::<BindPattern, ListParWithRandom, TaggedContinuation>::get(
                        &Matcher, pattern, &datum.a,
                    )
                    .is_some()
                })
                .collect()
        })
        .collect();
    let greedy = greedy_assignment(&matches);
    let mut used = vec![false; ordered.len()];
    let backtracking = backtracking_assignment(&matches, &mut used, 0);
    let receive_log = evaluate(&mut play, receive_source, &format!("{label}-receive")).await;
    let aliased = vec![hash(&channel), hash(&channel)];
    let join_consumes = receive_log
        .iter()
        .filter(|event| {
            matches!(event, Event::IoEvent(IOEvent::Consume(consume)) if consume.channel_hashes == aliased)
        })
        .count();
    assert_eq!(join_consumes, 1, "the aliased join reaches the space once");
    let comm_count = comms_in(&receive_log)
        .into_iter()
        .filter(|comm| comm.consume.channel_hashes == aliased)
        .count();
    assert_eq!(
        comm_count,
        usize::from(greedy.is_some()),
        "HEAD's join matcher agrees with the greedy first-match simulation"
    );
    let remaining = play.get_data(&channel).await;
    let waiting = play
        .get_continuations(vec![channel.clone(), channel.clone()])
        .await;
    let output = play.get_data(&string_par(output_name)).await;
    if let [waiting] = waiting.as_slice() {
        assert_eq!(
            waiting.patterns, patterns,
            "the stored join keeps the normalized bind order"
        );
    }
    (
        json!({
            "channels": channel_labels(&[
                channel_name,
                output_name,
                &format!("{channel_name}-names"),
            ]),
            // Changed by decision 93: the two sends of the data step and the two processes of the
            // receive step run in parallel, so both logs follow their arrival (DR-132), and the
            // corpus keeps them as multisets.
            // "data": { "kinds": kinds_json(&data_log), "events": events_json(&data_log) },
            "data": { "kinds": arrival_kinds_json(&data_log), "events": arrival_events_json(&data_log) },
            "deterministic_data_order": ordered
                .iter()
                .map(|datum| datum.a.pars.iter().map(pretty).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            "normalized_bind_patterns": patterns
                .iter()
                .map(|pattern| pattern.patterns.iter().map(pretty).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            "match_matrix": matches,
            "greedy_assignment": greedy,
            "backtracking_assignment_exists": backtracking,
            // "receive": { "kinds": kinds_json(&receive_log), "events": events_json(&receive_log) },
            "receive": {
                "kinds": arrival_kinds_json(&receive_log),
                "events": arrival_events_json(&receive_log),
            },
            "comm_count": comm_count,
            "remaining_data_count": remaining.len(),
            "waiting_continuation_count": waiting.len(),
            "output_data_count": output.len(),
        }),
        backtracking,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r3_greedy_join_first_match() {
    let same_channel = match Compiler::source_to_adt(R3_SAME_CHANNEL_JOIN) {
        Err(error @ InterpreterError::ReceiveOnSameChannelsError { .. }) => format!("{error:?}"),
        other => panic!("HEAD rejects a syntactic same-channel join, got {other:?}"),
    };
    let (greedy, greedy_backtracking) = r3_join(
        "rrs-r3-greedy",
        "rrs-r3-greedy-out",
        R3_GREEDY_DATA,
        R3_GREEDY_RECEIVE,
        "rrs-r3-greedy",
    )
    .await;
    assert!(
        greedy_backtracking,
        "a backtracking matcher would satisfy the greedy join"
    );
    assert_eq!(
        greedy["comm_count"],
        json!(0),
        "HEAD's greedy matcher leaves the join unmatched: order {} patterns {} matrix {}",
        greedy["deterministic_data_order"],
        greedy["normalized_bind_patterns"],
        greedy["match_matrix"]
    );
    assert_eq!(greedy["remaining_data_count"], json!(2));
    assert_eq!(greedy["waiting_continuation_count"], json!(1));
    let (control, _) = r3_join(
        "rrs-r3-control",
        "rrs-r3-control-out",
        R3_CONTROL_DATA,
        R3_CONTROL_RECEIVE,
        "rrs-r3-control",
    )
    .await;
    assert_eq!(control["comm_count"], json!(1), "the control join matches");
    assert_eq!(control["remaining_data_count"], json!(0));
    assert_eq!(control["output_data_count"], json!(1));
    observe(
        "r3",
        "r3_greedy_join_first_match",
        json!({
            "syntactic_same_channel_join": {
                "source": R3_SAME_CHANNEL_JOIN,
                "normalization": same_channel,
            },
            "greedy": greedy,
            "control": control,
        }),
    );
}

// Measures the witness of the aliased join for `R3_GREEDY_DATA`. Each line reports one datum
// `[1, k]`. A datum is a witness when the greedy matcher finds no communication and a
// backtracking assignment exists. The reveal batch runs this helper with `--ignored`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measures the r3 witnesses, run it with --ignored"]
async fn reified_ordinary_regression_r3_measure_witnesses() {
    for k in 0..=9 {
        let data = format!(r#"@"rrs-r3-greedy"!([1, {k}]) | @"rrs-r3-greedy"!([1, 6])"#);
        let (measured, backtracking) = r3_join(
            "rrs-r3-greedy",
            "rrs-r3-greedy-out",
            &data,
            R3_GREEDY_RECEIVE,
            "rrs-r3-greedy",
        )
        .await;
        println!(
            "r3 witness [1, {k}]: comm_count {} backtracking {backtracking} order {} matrix {}",
            measured["comm_count"], measured["deterministic_data_order"], measured["match_matrix"],
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r4_leaf_bytes() {
    let Runtimes {
        mut play, history, ..
    } = fresh_runtimes().await;
    let empty_root = play.get_root().await;
    let log = evaluate(&mut play, R4_PROGRAM, "rrs-r4").await;
    let checkpoint = play.create_checkpoint().await;
    let reader = history
        .get_history_reader(&checkpoint.root)
        .expect("read the checkpointed history");
    let data_channel = string_par("rrs-r4-data");
    let wait_channel = string_par("rrs-r4-wait");
    let left_channel = string_par("rrs-r4-join-left");
    let right_channel = string_par("rrs-r4-join-right");
    let data_key = hash(&data_channel);
    let data_leaf = reader
        .get_data_proj_binary(&data_key)
        .expect("read the data leaf");
    assert_eq!(data_leaf.len(), 2, "the data leaf holds both data");
    let continuation_leaf = |channels: Vec<Par>| {
        let key = hash_from_vec(&channels);
        let leaf = reader
            .get_continuations_proj_binary(&key)
            .expect("read a continuation leaf");
        assert_eq!(
            leaf.len(),
            1,
            "each continuation leaf holds one continuation"
        );
        json!({
            "channels": pretty_channels(&channels),
            "key": hex::encode(&key.0),
            "leaf": hex_list(&leaf),
        })
    };
    let joins_leaf = |channel: &Par| {
        let key = hash(channel);
        let leaf = reader
            .get_joins_proj_binary(&key)
            .expect("read a joins leaf");
        assert!(!leaf.is_empty(), "every waiting channel has a joins leaf");
        json!({
            "channel": pretty_channel(channel),
            "key": hex::encode(&key.0),
            "leaf": hex_list(&leaf),
        })
    };
    let section = json!({
        "program": R4_PROGRAM,
        "empty_root": hex::encode(&empty_root.0),
        "checkpoint_root": hex::encode(&checkpoint.root.0),
        // Changed by decision 93: the processes of the program run in parallel, so its log
        // follows their arrival (DR-132), and the corpus keeps its kinds as a multiset.
        // "kinds": kinds_json(&log),
        "kinds": arrival_kinds_json(&log),
        "data": {
            "channel": pretty_channel(&data_channel),
            "key": hex::encode(&data_key.0),
            "leaf": hex_list(&data_leaf),
        },
        "continuations": [
            continuation_leaf(vec![wait_channel.clone()]),
            continuation_leaf(vec![left_channel.clone(), right_channel.clone()]),
        ],
        "joins": [
            joins_leaf(&wait_channel),
            joins_leaf(&left_channel),
            joins_leaf(&right_channel),
        ],
        "data_channel_joins_leaf": hex_list(
            &reader
                .get_joins_proj_binary(&data_key)
                .expect("read the data channel joins leaf")
        ),
    });
    observe("r4", "r4_leaf_bytes", section);
}

fn comm_sort_key(produce: &Produce) -> (Vec<u8>, Vec<u8>, bool) {
    (
        produce.channel_hash.0.clone(),
        produce.hash.0.clone(),
        produce.persistent,
    )
}

#[test]
fn reified_ordinary_regression_r5_event_struct_and_proto_bytes() {
    let channel_alpha = string_par("rrs-r5-alpha");
    let channel_beta = string_par("rrs-r5-beta");
    let datum_alpha = ListParWithRandom {
        pars: vec![new_gint_par(7, Vec::new(), false)],
        random_state: seed("rrs-r5-alpha").to_bytes(),
        cost_authority: None,
        cost_stack: None,
    };
    let datum_beta = ListParWithRandom {
        pars: vec![string_par("beta")],
        random_state: seed("rrs-r5-beta").to_bytes(),
        cost_authority: None,
        cost_stack: None,
    };
    let pattern_alpha = BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    };
    let pattern_beta = BindPattern {
        patterns: vec![string_par("beta")],
        remainder: None,
        free_count: 0,
    };
    let continuation = TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(new_gint_par(1, Vec::new(), false)),
            random_state: seed("rrs-r5-body").to_bytes(),
        })),
        guard: None,
        cost_authority: None,
    };
    let channels = vec![channel_alpha.clone(), channel_beta.clone()];
    let patterns = vec![pattern_alpha, pattern_beta];

    let produce_linear = Produce::create(&channel_alpha, &datum_alpha, false);
    let produce_persistent = Produce::create(&channel_beta, &datum_beta, true);
    let consume = Consume::create(&channels, &patterns, &continuation, false);
    let datum = Datum::create(&channel_alpha, datum_alpha.clone(), false);
    let waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &continuation,
        false,
        BTreeSet::from([1]),
    );
    assert_eq!(datum.source.hash, produce_linear.hash);
    assert_eq!(waiting.source, consume);

    let mut canonical_sources = [
        (
            channel_alpha.clone(),
            datum_alpha.clone(),
            produce_linear.clone(),
        ),
        (
            channel_beta.clone(),
            datum_beta.clone(),
            produce_persistent.clone(),
        ),
    ];
    canonical_sources.sort_by_key(|(_, _, source)| comm_sort_key(source));
    let candidates: Vec<ConsumeCandidate<Par, ListParWithRandom>> = canonical_sources
        .iter()
        .rev()
        .enumerate()
        .map(|(index, (channel, data, source))| ConsumeCandidate {
            channel: channel.clone(),
            datum: Datum {
                a: data.clone(),
                persist: source.persistent,
                source: source.clone(),
            },
            removed_datum: data.clone(),
            datum_index: index as i32,
        })
        .collect();
    let comm = COMM::new(
        &candidates,
        consume.clone(),
        BTreeSet::from([1]),
        |produces| {
            produces
                .iter()
                .map(|produce| (produce.clone(), i32::from(!produce.persistent)))
                .collect()
        },
    );
    let candidate_order: Vec<String> = candidates
        .iter()
        .map(|candidate| hex::encode(&candidate.datum.source.hash.0))
        .collect();
    let canonical_order: Vec<String> = canonical_sources
        .iter()
        .map(|(_, _, source)| hex::encode(&source.hash.0))
        .collect();
    let comm_order: Vec<String> = comm
        .produces
        .iter()
        .map(|produce| hex::encode(&produce.hash.0))
        .collect();
    assert_ne!(candidate_order, canonical_order);
    assert_eq!(comm_order, canonical_order);

    let bincode_hex = |bytes: Vec<u8>| hex::encode(bytes);
    let stable_hex = |value: Blake2b256Hash| hex::encode(value.0);
    let produce_section = |produce: &Produce| {
        json!({
            "fields": produce_json(produce),
            "bincode": bincode_hex(bincode::serialize(produce).expect("bincode-serialize Produce")),
            "io_event_bincode": bincode_hex(
                bincode::serialize(&IOEvent::Produce(produce.clone()))
                    .expect("bincode-serialize IOEvent::Produce")
            ),
            "blake2b256_bincode": stable_hex(hash(produce)),
        })
    };
    let events = [
        (
            "produce",
            Event::IoEvent(IOEvent::Produce(produce_linear.clone())),
        ),
        ("consume", Event::IoEvent(IOEvent::Consume(consume.clone()))),
        ("comm", Event::Comm(comm.clone())),
    ];
    let proto: Map<String, Value> = events
        .iter()
        .map(|(name, event)| {
            let direct = event_proto(event);
            let converted = casper_event(event);
            let direct_bytes = direct.encode_to_vec();
            assert_eq!(
                converted.to_proto().encode_to_vec(),
                direct_bytes,
                "models' Event::to_proto agrees with the directly constructed {name} message"
            );
            assert_eq!(
                CasperEvent::from_proto(direct.clone()).expect("decode the event message"),
                converted
            );
            (
                (*name).to_owned(),
                json!({
                    "message": event_proto_json(&direct),
                    "casper_event": casper_event_json(&converted),
                    "prost": hex::encode(&direct_bytes),
                }),
            )
        })
        .collect();
    let section = json!({
        "produce_linear": produce_section(&produce_linear),
        "produce_persistent": produce_section(&produce_persistent),
        "consume": {
            "fields": consume_json(&consume),
            "bincode": bincode_hex(bincode::serialize(&consume).expect("bincode-serialize Consume")),
            "io_event_bincode": bincode_hex(
                bincode::serialize(&IOEvent::Consume(consume.clone()))
                    .expect("bincode-serialize IOEvent::Consume")
            ),
            "blake2b256_bincode": stable_hex(hash(&consume)),
        },
        "comm": {
            "fields": comm_json(&comm),
            "bincode": bincode_hex(bincode::serialize(&comm).expect("bincode-serialize COMM")),
            "cost_identity": stable_hex(comm.cost_identity()),
            "blake2b256_bincode": stable_hex(hash(&comm)),
            "candidate_order": candidate_order,
            "canonical_order": canonical_order,
        },
        "datum": {
            "fields": datum_json(&datum),
            "bincode": bincode_hex(bincode::serialize(&datum).expect("bincode-serialize Datum")),
        },
        "waiting_continuation": {
            "fields": waiting_continuation_json(&waiting),
            "bincode": bincode_hex(
                bincode::serialize(&waiting).expect("bincode-serialize WaitingContinuation")
            ),
        },
        "proto_messages": proto,
        "proto_construction": "direct",
    });
    observe("r5", "r5_event_struct_and_proto_bytes", section);
}

fn native_config(limit: u64) -> NativeRuntimeConfig {
    let names: [&[u8]; 4] = [b"c", b"i", b"t", b"r"];
    let descriptor = PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(NATIVE_WEIGHTS)
            .zip(names)
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: 0,
        compatibility_rule: native_resource_compatibility_rule(),
    };
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("bind the native phlo schedule");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [0_u64];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: 0,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        limit,
    )
    .expect("check the native phlo controls");
    let contract =
        NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
            .expect("build the native contract");
    NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4096,
        regions: NativePhloRegionLimits {
            regions: 1000,
            encoded_authority_bytes: 10_000_000,
        },
    })
}

// Changed by decision 93: the corpus keeps the journal rows of a program only when they do not
// follow the arrival order of its processes (`CostProgram::rows_follow_arrival`). Journal order is
// start order (DR-132), so it keeps the COMM rows as a multiset.
// fn native_json(result: &EvaluateResult) -> Value {
//     let comm_identity_counts: Vec<usize> = result
//         .native_operation_recording
//         .as_ref()
//         .map(|rows| {
//             rows.iter()
//                 .filter_map(|row| row.comm.as_ref())
//                 .map(|comm| comm.source.produces.len() + comm.source.repetitions.len())
//                 .collect()
//         })
//         .unwrap_or_default();
//     json!({
//         "native_phlo_usage": result.native_phlo_usage,
//         "errors": errors_json(&result.errors),
//         "operation_count": result
//             .native_operation_recording
//             .as_ref()
//             .map(|rows| rows.len()),
//         "comm_identity_counts": comm_identity_counts,
//     })
// }
fn native_json(result: &EvaluateResult, rows_follow_arrival: bool) -> Value {
    let mut comm_identity_counts: Vec<usize> = result
        .native_operation_recording
        .as_ref()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.comm.as_ref())
                .map(|comm| comm.source.produces.len() + comm.source.repetitions.len())
                .collect()
        })
        .unwrap_or_default();
    comm_identity_counts.sort_unstable();
    let mut native = json!({
        "native_phlo_usage": result.native_phlo_usage,
        "errors": errors_json(&result.errors),
    });
    if !rows_follow_arrival {
        native["operation_count"] = json!(result
            .native_operation_recording
            .as_ref()
            .map(|rows| rows.len()));
        native["comm_identity_counts"] = json!(comm_identity_counts);
    }
    native
}

async fn r6_program(program: &CostProgram) -> Value {
    let label = format!("rrs-r6-{}", program.name);
    let standard = {
        let Runtimes { play, .. } = fresh_runtimes().await;
        let result = play
            .evaluate(
                program.source,
                Cost::create(program.phlo, label.clone()),
                HashMap::new(),
                seed(&label),
            )
            .await
            .unwrap_or_else(|error| panic!("{label} standard evaluation failed: {error:?}"));
        json!({
            "cost": result.cost.value,
            "errors": errors_json(&result.errors),
            "quantitative_byte_cost": result.quantitative_byte_cost,
        })
    };
    let native = {
        let Runtimes { mut play, .. } = fresh_runtimes().await;
        let result = play
            .evaluate_with_native_phlo(
                program.source,
                HashMap::new(),
                seed(&label),
                None,
                native_config(program.native_limit),
            )
            .await
            .unwrap_or_else(|error| panic!("{label} native evaluation failed: {error:?}"));
        // Changed by decision 93: the journal rows of an arrival-ordered program follow racing
        // evaluation (`CostProgram::rows_follow_arrival`).
        // native_json(&result)
        native_json(&result, program.rows_follow_arrival)
    };
    json!({
        "source": program.source,
        "phlo_limit": program.phlo,
        "native_limit": program.native_limit,
        "native_weights": NATIVE_WEIGHTS,
        "standard": standard,
        "native": native,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r6_cost_vectors() {
    let mut programs = Map::new();
    for program in &R6_PROGRAMS {
        programs.insert(program.name.to_owned(), r6_program(program).await);
    }
    let comm_programs = R6_PROGRAMS
        .iter()
        .filter(|program| {
            programs[program.name]["native"]["comm_identity_counts"]
                .as_array()
                .is_some_and(|counts| !counts.is_empty())
        })
        .count();
    assert!(
        comm_programs >= 4,
        "the native path observes COMMs in several programs"
    );
    assert!(
        programs["out_of_phlo"]["standard"]["errors"]
            .to_string()
            .contains("OutOfPhlogistonsError"),
        "the out-of-phlo deploy runs out of phlo on the standard path"
    );
    observe("r6", "r6_cost_vectors", json!({ "programs": programs }));
}

#[test]
fn reified_ordinary_regression_r7_term_prost_and_bincode_bytes() {
    let terms: Map<String, Value> = R7_TERMS
        .iter()
        .map(|(name, source)| {
            let par = Compiler::source_to_adt(source)
                .unwrap_or_else(|error| panic!("normalize {name}: {error:?}"));
            let bincode_bytes = bincode::serialize(&par).expect("bincode-serialize the Par");
            let stable_hash = hash(&par);
            assert_eq!(stable_hash, Blake2b256Hash::new(&bincode_bytes));
            (
                (*name).to_owned(),
                json!({
                    "source": source,
                    "prost": hex::encode(par.encode_to_vec()),
                    "bincode": hex::encode(&bincode_bytes),
                    "stable_hash": hex::encode(&stable_hash.0),
                }),
            )
        })
        .collect();
    observe(
        "r7",
        "r7_term_prost_and_bincode_bytes",
        json!({ "terms": terms }),
    );
}

fn pattern_json(pattern: &BindPattern) -> Value {
    let BindPattern {
        patterns,
        remainder,
        free_count,
    } = pattern;
    json!({
        "patterns": patterns.iter().map(pretty).collect::<Vec<_>>(),
        "remainder": remainder
            .as_ref()
            .map(|var| PrettyPrinter::new().build_string_from_var(var)),
        "free_count": free_count,
    })
}

fn continuation_state_json(continuation: &TaggedContinuation) -> Value {
    let TaggedContinuation {
        tagged_cont,
        guard,
        cost_authority,
    } = continuation;
    let body = match tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body,
            random_state: _,
        })) => json!({ "par": body.as_ref().map(pretty) }),
        Some(TaggedCont::ScalaBodyRef(reference)) => json!({ "system_process": reference }),
        None => Value::Null,
    };
    json!({
        "body": body,
        "guard": guard.as_ref().map(pretty),
        "has_cost_authority": cost_authority.is_some(),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r8_state_dump() {
    assert!(
        std::env::var_os("PRETTY_PRINTER_OUTPUT_TRIM_AFTER").is_none(),
        "the state dump requires untrimmed pretty printing"
    );
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let log = evaluate(&mut play, R8_PROGRAM, "rrs-r8").await;
    let snapshot = play.create_soft_checkpoint().await;
    let state = snapshot.cache_snapshot;
    let data = sorted(
        state
            .data
            .iter()
            .flat_map(|shard| shard.iter())
            .filter(|(_, data)| !data.is_empty())
            .map(|(channel, data)| {
                json!({
                    "channel": pretty_channel(channel),
                    "data": sorted(
                        data.iter()
                            .map(|datum| {
                                json!({
                                    "pars": datum.a.pars.iter().map(pretty).collect::<Vec<_>>(),
                                    "persist": datum.persist,
                                })
                            })
                            .collect()
                    ),
                })
            })
            .collect(),
    );
    let continuations = sorted(
        state
            .continuations
            .iter()
            .flat_map(|shard| shard.iter())
            .filter(|(_, waiting)| !waiting.is_empty())
            .map(|(channels, waiting)| {
                json!({
                    "channels": pretty_channels(channels),
                    "continuations": sorted(
                        waiting
                            .iter()
                            .map(|waiting| {
                                json!({
                                    "patterns": waiting.patterns.iter().map(pattern_json).collect::<Vec<_>>(),
                                    "continuation": continuation_state_json(&waiting.continuation),
                                    "persist": waiting.persist,
                                    "peeks": waiting.peeks.iter().copied().collect::<Vec<_>>(),
                                })
                            })
                            .collect()
                    ),
                })
            })
            .collect(),
    );
    let joins = sorted(
        state
            .joins
            .iter()
            .flat_map(|shard| shard.iter())
            .filter(|(_, joins)| !joins.is_empty())
            .map(|(channel, joins)| {
                let mut rendered: Vec<Vec<String>> =
                    joins.iter().map(|join| pretty_channels(join)).collect();
                rendered.sort();
                json!({
                    "channel": pretty_channel(channel),
                    "joins": rendered,
                })
            })
            .collect(),
    );
    observe(
        "r8",
        "r8_state_dump",
        json!({
            "program": R8_PROGRAM,
            // Changed by decision 93: the processes of the program run in parallel, so its log
            // follows their arrival (DR-132), and the corpus keeps its kinds as a multiset.
            // "kinds": kinds_json(&log),
            "kinds": arrival_kinds_json(&log),
            "data": data,
            "continuations": continuations,
            "joins": joins,
        }),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reified_ordinary_regression_r9_play_replay_equality() {
    let Runtimes {
        mut play,
        mut replay,
        ..
    } = fresh_runtimes().await;
    let start_root = play.get_root().await;
    let rand = seed("rrs-r9");
    let played = play
        .evaluate(
            R9_PROGRAM,
            Cost::create(EVALUATION_PHLO, "rrs-r9"),
            HashMap::new(),
            rand.clone(),
        )
        .await
        .expect("play evaluation returns a result");
    assert!(
        played.errors.is_empty(),
        "play reported errors: {:?}",
        played.errors
    );
    let play_checkpoint = play.create_checkpoint().await;
    replay
        .reset(&start_root)
        .await
        .expect("reset the replay runtime to the start root");
    replay
        .rig(play_checkpoint.log.clone())
        .await
        .expect("rig the replay runtime with the play log");
    let replayed = replay
        .evaluate(
            R9_PROGRAM,
            Cost::create(EVALUATION_PHLO, "rrs-r9"),
            HashMap::new(),
            rand,
        )
        .await
        .expect("replay evaluation returns a result");
    assert!(
        replayed.errors.is_empty(),
        "replay reported errors: {:?}",
        replayed.errors
    );
    replay
        .check_replay_data()
        .await
        .expect("the replay consumes the complete play log");
    let replay_checkpoint = replay.create_checkpoint().await;
    assert_eq!(
        replay_checkpoint.root, play_checkpoint.root,
        "replay reaches the play root"
    );
    assert_eq!(
        played.cost.value, replayed.cost.value,
        "replay charges the play cost"
    );
    observe(
        "r9",
        "r9_play_replay_equality",
        json!({
            "program": R9_PROGRAM,
            "start_root": hex::encode(&start_root.0),
            "play_root": hex::encode(&play_checkpoint.root.0),
            "replay_root_equals_play_root": true,
            "play_cost": played.cost.value,
            // Changed by decision 93: racing evaluation decides which processes meet first, so
            // the length and the kinds of the play log differ between runs of the clean base
            // (DR-132). Each run rigs its own log and checks the root, the cost and the complete
            // use of the log above.
            // "event_log_length": play_checkpoint.log.len(),
            // "event_kinds": kinds_json(&play_checkpoint.log),
        }),
    );
}

fn departed<T: serde::Serialize + Clone>(before: &[T], after: &[T]) -> T {
    assert_eq!(
        after.len() + 1,
        before.len(),
        "exactly one candidate leaves per step"
    );
    let stayed: BTreeSet<Blake2b256Hash> = after.iter().map(bincode_hash).collect();
    let gone: Vec<&T> = before
        .iter()
        .filter(|record| !stayed.contains(&bincode_hash(*record)))
        .collect();
    match gone.as_slice() {
        [record] => (*record).clone(),
        other => panic!("expected one departed candidate, found {}", other.len()),
    }
}

fn hash_hex_list(hashes: &[Blake2b256Hash]) -> Value {
    Value::Array(
        hashes
            .iter()
            .map(|hash| Value::String(hex::encode(&hash.0)))
            .collect(),
    )
}

fn identity_order(ranked: &[(Blake2b256Hash, usize)]) -> bool {
    ranked
        .iter()
        .enumerate()
        .all(|(position, (_, index))| position == *index)
}

async fn deterministic_data_candidates() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let channel = string_par(DC_DATA_CHANNEL);
    evaluate(&mut play, DC_DATA_SOURCE, "rrs-dc-data").await;
    let stored = play.get_data(&channel).await;
    assert_eq!(stored.len(), DC_CANDIDATES, "the data step stores six data");
    let ranked = canonical_rank(&stored);
    assert!(
        !identity_order(&ranked),
        "the ranking must differ from the store order"
    );
    let mut remaining = stored.clone();
    let mut taken = Vec::with_capacity(DC_CANDIDATES);
    for take in 0..DC_CANDIDATES {
        let log = evaluate(&mut play, DC_TAKE_SOURCE, &format!("rrs-dc-take-{take}")).await;
        let after = play.get_data(&channel).await;
        let datum = departed(&remaining, &after);
        let consumed: Vec<&Blake2b256Hash> = single_comm(&log)
            .produces
            .iter()
            .map(|produce| &produce.hash)
            .collect();
        assert_eq!(
            consumed,
            vec![&datum.source.hash],
            "the COMM consumes the departed datum"
        );
        taken.push(bincode_hash(&datum));
        remaining = after;
    }
    let ranked_hashes: Vec<Blake2b256Hash> = ranked.iter().map(|(hash, _)| hash.clone()).collect();
    // Changed by decision 89 (DR-129): play picks at random among candidates again, as on dev,
    // so the consumes take the data in some order. Each datum departs exactly once, and the
    // observation records the taken data sorted.
    // assert_eq!(
    //     taken, ranked_hashes,
    //     "consumes take data in the canonical candidate order"
    // );
    let taken = sorted_hashes(taken);
    assert_eq!(
        taken,
        sorted_hashes(ranked_hashes.clone()),
        "the consumes take each datum once"
    );
    json!({
        "source": DC_DATA_SOURCE,
        "take_source": DC_TAKE_SOURCE,
        "ranked_hashes": hash_hex_list(&ranked_hashes),
        "values": ranked
            .iter()
            .map(|(_, index)| stored[*index].a.pars.iter().map(pretty).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        // "taken": hash_hex_list(&taken),
        "taken_sorted": hash_hex_list(&taken),
    })
}

/// The hashes `hashes` in ascending order: the order-independent view of the candidates that
/// a sequence of COMMs took (decision 89, DR-129).
fn sorted_hashes(mut hashes: Vec<Blake2b256Hash>) -> Vec<Blake2b256Hash> {
    hashes.sort();
    hashes
}

async fn deterministic_continuation_candidates() -> Value {
    let Runtimes { mut play, .. } = fresh_runtimes().await;
    let channel = string_par(DC_CONTINUATION_CHANNEL);
    evaluate(&mut play, DC_CONTINUATION_SOURCE, "rrs-dc-cont").await;
    let waiting = play.get_continuations(vec![channel.clone()]).await;
    assert_eq!(waiting.len(), DC_CANDIDATES, "six receives wait");
    let ranked = canonical_rank(&waiting);
    assert!(
        !identity_order(&ranked),
        "the ranking must differ from the store order"
    );
    let mut remaining = waiting.clone();
    let mut sends = Vec::with_capacity(DC_CANDIDATES);
    let mut fired = Vec::with_capacity(DC_CANDIDATES);
    for value in 0..DC_CANDIDATES {
        let send = format!(r#"@"{DC_CONTINUATION_CHANNEL}"!({value})"#);
        let log = evaluate(&mut play, &send, &format!("rrs-dc-send-{value}")).await;
        let after = play.get_continuations(vec![channel.clone()]).await;
        let continuation = departed(&remaining, &after);
        assert_eq!(
            single_comm(&log).consume,
            continuation.source,
            "the COMM fires the departed continuation"
        );
        fired.push(bincode_hash(&continuation));
        sends.push(send);
        remaining = after;
    }
    let ranked_hashes: Vec<Blake2b256Hash> = ranked.iter().map(|(hash, _)| hash.clone()).collect();
    // Changed by decision 89 (DR-129): play picks at random among candidates again, as on dev,
    // so the produces fire the continuations in some order. Each continuation fires exactly
    // once, and the observation records the fired continuations sorted.
    // assert_eq!(
    //     fired, ranked_hashes,
    //     "produces fire continuations in the canonical candidate order"
    // );
    let fired = sorted_hashes(fired);
    assert_eq!(
        fired,
        sorted_hashes(ranked_hashes.clone()),
        "the produces fire each continuation once"
    );
    json!({
        "source": DC_CONTINUATION_SOURCE,
        "sends": sends,
        "ranked_hashes": hash_hex_list(&ranked_hashes),
        "bodies": ranked
            .iter()
            .map(|(_, index)| continuation_state_json(&waiting[*index].continuation))
            .collect::<Vec<_>>(),
        // "fired": hash_hex_list(&fired),
        "fired_sorted": hash_hex_list(&fired),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deterministic_candidates_hash_unchanged() {
    assert!(
        std::env::var_os("PRETTY_PRINTER_OUTPUT_TRIM_AFTER").is_none(),
        "the candidate dump requires untrimmed pretty printing"
    );
    let data = deterministic_data_candidates().await;
    let continuations = deterministic_continuation_candidates().await;
    observe(
        "dc",
        "deterministic_candidates",
        json!({
            "data": data,
            "continuations": continuations,
        }),
    );
}

// The ordinary play/replay property (regression design, section 3). Every
// generated case terminates by construction: a receive body uses only
// channels above the levels of the receive's channels (S1), no channel
// carries both `!!` and `<=` (S2), and no received variable is a channel
// (S3). Each deploy must replay with no errors, pass `check_replay_data`,
// reach the play root, and charge the play cost.

const PROPERTY_SEED: u64 = 0x0E0_5EED_2026_1004; // house seed
const PROPERTY_CASES: u32 = 64;
const PROPERTY_PHLO: i64 = 10_000_000;
const PUBLIC_LEVELS: u8 = 3;
const PUBLIC_PER_LEVEL: u8 = 2;
const SINK_COUNT: u8 = 2;
const SINK_LEVEL: u8 = 3;
const MAX_DEPTH: u8 = 3;
const MAX_JOIN_WIDTH: usize = 3;
const MUTANT_SEND: &str = r#" | @"rrs-prop-mutant"!(0)"#;

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: PROPERTY_CASES,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn property_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build the property runtime")
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum GenChan {
    Public { level: u8, index: u8 },
    Sink(u8),
    Fresh,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenArrow {
    Linear,
    Peek,
    Persistent,
}

#[derive(Clone, Debug)]
enum GenArg {
    Int(u8),
    Str(bool),
    Pair(Box<GenArg>, Box<GenArg>),
    Var(u8),
    Name,
}

#[derive(Clone, Copy, Debug)]
enum GenPat {
    Bind,
    Wildcard,
    Int(u8),
    Str,
    HeadZero,
    Both,
}

#[derive(Clone, Debug)]
struct GenBind {
    pats: Vec<GenPat>,
    chan: GenChan,
}

#[derive(Clone, Debug)]
enum GenTerm {
    Send {
        chan: GenChan,
        persistent: bool,
        args: Vec<GenArg>,
    },
    Receive {
        arrow: GenArrow,
        binds: Vec<GenBind>,
        body: Vec<GenTerm>,
    },
    New {
        inner: Vec<GenTerm>,
    },
    Aliased {
        level: u8,
        index: u8,
        arrow: GenArrow,
        left: Vec<GenPat>,
        right: Vec<GenPat>,
        body: Vec<GenTerm>,
    },
}

#[derive(Clone, Debug)]
struct OrdinaryCase {
    seed: u32,
    seed_arrow: GenArrow,
    seed_persistent: bool,
    deploys: Vec<Vec<GenTerm>>,
}

fn gen_arrow() -> impl Strategy<Value = GenArrow> {
    prop_oneof![
        3 => Just(GenArrow::Linear),
        2 => Just(GenArrow::Peek),
        1 => Just(GenArrow::Persistent),
    ]
}

fn gen_arity() -> impl Strategy<Value = usize> {
    prop_oneof![4 => Just(1_usize), 1 => Just(2_usize)]
}

fn gen_leaf_arg() -> impl Strategy<Value = GenArg> {
    prop_oneof![
        3 => (0_u8..=2).prop_map(GenArg::Int),
        1 => any::<bool>().prop_map(GenArg::Str),
        2 => any::<u8>().prop_map(GenArg::Var),
        1 => Just(GenArg::Name),
    ]
}

fn gen_arg() -> impl Strategy<Value = GenArg> {
    prop_oneof![
        3 => (0_u8..=2).prop_map(GenArg::Int),
        1 => any::<bool>().prop_map(GenArg::Str),
        2 => any::<u8>().prop_map(GenArg::Var),
        1 => Just(GenArg::Name),
        1 => (gen_leaf_arg(), gen_leaf_arg())
            .prop_map(|(left, right)| GenArg::Pair(Box::new(left), Box::new(right))),
    ]
}

fn gen_pat() -> impl Strategy<Value = GenPat> {
    prop_oneof![
        4 => Just(GenPat::Bind),
        2 => Just(GenPat::Wildcard),
        1 => (0_u8..=1).prop_map(GenPat::Int),
        1 => Just(GenPat::Str),
        1 => Just(GenPat::HeadZero),
        1 => Just(GenPat::Both),
    ]
}

fn gen_pats() -> impl Strategy<Value = Vec<GenPat>> {
    gen_arity().prop_flat_map(|arity| prop::collection::vec(gen_pat(), arity))
}

fn gen_args() -> impl Strategy<Value = Vec<GenArg>> {
    gen_arity().prop_flat_map(|arity| prop::collection::vec(gen_arg(), arity))
}

// The public channels at levels `lo..=2`, with room for `extra` more.
fn public_channels(lo: u8, extra: usize) -> Vec<GenChan> {
    let levels = usize::from(PUBLIC_LEVELS.saturating_sub(lo));
    let mut channels = Vec::with_capacity(levels * usize::from(PUBLIC_PER_LEVEL) + extra);
    for level in lo..PUBLIC_LEVELS {
        for index in 0..PUBLIC_PER_LEVEL {
            channels.push(GenChan::Public { level, index });
        }
    }
    channels
}

fn sink_channels() -> Vec<GenChan> { (0..SINK_COUNT).map(GenChan::Sink).collect() }

fn chan_level(chan: GenChan, lo: u8) -> u8 {
    match chan {
        GenChan::Public { level, .. } => level,
        GenChan::Sink(_) => SINK_LEVEL,
        GenChan::Fresh => lo,
    }
}

fn gen_send_over(channels: Vec<GenChan>) -> BoxedStrategy<GenTerm> {
    (select(channels), prop::bool::weighted(0.2), gen_args())
        .prop_map(|(chan, persistent, args)| GenTerm::Send {
            chan,
            persistent,
            args,
        })
        .boxed()
}

fn gen_send(lo: u8, fresh: bool) -> BoxedStrategy<GenTerm> {
    let mut channels = public_channels(lo, usize::from(SINK_COUNT) + 1);
    channels.extend(sink_channels());
    if fresh {
        channels.push(GenChan::Fresh);
    }
    gen_send_over(channels)
}

fn gen_receive(lo: u8, depth: u8, fresh: bool) -> BoxedStrategy<GenTerm> {
    let mut channels = public_channels(lo, 1);
    if fresh {
        channels.push(GenChan::Fresh);
    }
    let width = channels.len().min(MAX_JOIN_WIDTH);
    (gen_arrow(), subsequence(channels, 1..=width))
        .prop_flat_map(move |(arrow, chans)| {
            let top = chans
                .iter()
                .map(|chan| chan_level(*chan, lo))
                .max()
                .expect("a receive binds at least one channel");
            let binds: Vec<_> = chans
                .into_iter()
                .map(|chan| gen_pats().prop_map(move |pats| GenBind { pats, chan }))
                .collect();
            (Just(arrow), binds, gen_body(top + 1, depth + 1))
        })
        .prop_map(|(arrow, binds, body)| GenTerm::Receive { arrow, binds, body })
        .boxed()
}

fn gen_body(lo: u8, depth: u8) -> BoxedStrategy<Vec<GenTerm>> {
    match lo >= SINK_LEVEL || depth >= MAX_DEPTH {
        true => prop::collection::vec(gen_send_over(sink_channels()), 1..=2).boxed(),
        false => prop::collection::vec(gen_term(lo, depth, false), 1..=2).boxed(),
    }
}

fn gen_new(lo: u8, depth: u8) -> BoxedStrategy<GenTerm> {
    prop::collection::vec(gen_term(lo, depth + 1, true), 1..=2)
        .prop_map(|inner| GenTerm::New { inner })
        .boxed()
}

fn gen_term(lo: u8, depth: u8, fresh: bool) -> BoxedStrategy<GenTerm> {
    let mut options = Vec::with_capacity(3);
    options.push((4, gen_send(lo, fresh)));
    options.push((3, gen_receive(lo, depth, fresh)));
    if depth < MAX_DEPTH {
        options.push((1, gen_new(lo, depth)));
    }
    Union::new_weighted(options).boxed()
}

fn gen_aliased() -> BoxedStrategy<GenTerm> {
    (
        0..PUBLIC_LEVELS,
        0..PUBLIC_PER_LEVEL,
        gen_arrow(),
        gen_pats(),
        gen_pats(),
    )
        .prop_flat_map(|(level, index, arrow, left, right)| {
            (
                Just((level, index, arrow, left, right)),
                gen_body(level + 1, 1),
            )
        })
        .prop_map(
            |((level, index, arrow, left, right), body)| GenTerm::Aliased {
                level,
                index,
                arrow,
                left,
                right,
                body,
            },
        )
        .boxed()
}

fn gen_top_term() -> BoxedStrategy<GenTerm> {
    Union::new_weighted(vec![(8, gen_term(0, 0, false)), (1, gen_aliased())]).boxed()
}

fn ordinary_case() -> impl Strategy<Value = OrdinaryCase> {
    (
        0_u32..1_000_000,
        gen_arrow(),
        any::<bool>(),
        prop::collection::vec(prop::collection::vec(gen_top_term(), 1..=4), 1..=2),
    )
        .prop_map(
            |(seed, seed_arrow, seed_persistent, deploys)| OrdinaryCase {
                seed,
                seed_arrow,
                seed_persistent,
                deploys,
            },
        )
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ResolvedChan {
    Public { level: u8, index: u8 },
    Sink(u8),
    Fresh(usize),
}

#[derive(Debug)]
enum ResolvedArg {
    Int(u8),
    Str(bool),
    Pair(Box<ResolvedArg>, Box<ResolvedArg>),
    Var(usize),
    Name(usize),
    Zero,
}

#[derive(Clone, Copy, Debug)]
enum ResolvedPat {
    Bind(usize),
    Wildcard,
    Int(u8),
    Str,
    HeadZero(usize),
    Both(usize, usize),
}

#[derive(Debug)]
struct ResolvedBind {
    pats: Vec<ResolvedPat>,
    chan: ResolvedChan,
}

#[derive(Debug)]
enum ResolvedTerm {
    Send {
        chan: ResolvedChan,
        persistent: bool,
        args: Vec<ResolvedArg>,
    },
    Receive {
        arrow: GenArrow,
        binds: Vec<ResolvedBind>,
        body: Vec<ResolvedTerm>,
    },
    New {
        binder: usize,
        inner: Vec<ResolvedTerm>,
    },
    Aliased {
        alias: usize,
        level: u8,
        index: u8,
        arrow: GenArrow,
        left: Vec<ResolvedPat>,
        right: Vec<ResolvedPat>,
        body: Vec<ResolvedTerm>,
    },
}

// Pass 1 of the renderer: ids in pre-order across deploys for variables
// `x{n}`, `new` binders `n{n}` and aliases.
#[derive(Default)]
struct Resolver {
    variables: usize,
    binders: usize,
    aliases: usize,
}

impl Resolver {
    fn variable(&mut self) -> usize {
        self.variables += 1;
        self.variables - 1
    }

    fn binder(&mut self) -> usize {
        self.binders += 1;
        self.binders - 1
    }

    fn alias(&mut self) -> usize {
        self.aliases += 1;
        self.aliases - 1
    }

    fn pats(&mut self, pats: &[GenPat], scope: &mut Vec<usize>) -> Vec<ResolvedPat> {
        let mut resolved = Vec::with_capacity(pats.len());
        for pat in pats {
            resolved.push(match pat {
                GenPat::Bind => {
                    let variable = self.variable();
                    scope.push(variable);
                    ResolvedPat::Bind(variable)
                }
                GenPat::Wildcard => ResolvedPat::Wildcard,
                GenPat::Int(value) => ResolvedPat::Int(*value),
                GenPat::Str => ResolvedPat::Str,
                GenPat::HeadZero => {
                    let variable = self.variable();
                    scope.push(variable);
                    ResolvedPat::HeadZero(variable)
                }
                GenPat::Both => {
                    let first = self.variable();
                    let second = self.variable();
                    scope.extend([first, second]);
                    ResolvedPat::Both(first, second)
                }
            });
        }
        resolved
    }

    fn arg(arg: &GenArg, scope: &[usize], binders: &[usize]) -> ResolvedArg {
        match arg {
            GenArg::Int(value) => ResolvedArg::Int(*value),
            GenArg::Str(first) => ResolvedArg::Str(*first),
            GenArg::Pair(left, right) => ResolvedArg::Pair(
                Box::new(Self::arg(left, scope, binders)),
                Box::new(Self::arg(right, scope, binders)),
            ),
            GenArg::Var(choice) => match scope.len() {
                0 => ResolvedArg::Zero,
                len => ResolvedArg::Var(scope[usize::from(*choice) % len]),
            },
            GenArg::Name => match binders.last() {
                Some(binder) => ResolvedArg::Name(*binder),
                None => ResolvedArg::Zero,
            },
        }
    }

    fn chan(chan: GenChan, binders: &[usize]) -> ResolvedChan {
        match chan {
            GenChan::Public { level, index } => ResolvedChan::Public { level, index },
            GenChan::Sink(index) => ResolvedChan::Sink(index),
            GenChan::Fresh => {
                ResolvedChan::Fresh(*binders.last().expect("a fresh channel lies inside a new"))
            }
        }
    }

    fn terms(
        &mut self,
        terms: &[GenTerm],
        scope: &mut Vec<usize>,
        binders: &mut Vec<usize>,
    ) -> Vec<ResolvedTerm> {
        let mut resolved = Vec::with_capacity(terms.len());
        for term in terms {
            resolved.push(self.term(term, scope, binders));
        }
        resolved
    }

    fn term(
        &mut self,
        term: &GenTerm,
        scope: &mut Vec<usize>,
        binders: &mut Vec<usize>,
    ) -> ResolvedTerm {
        match term {
            GenTerm::Send {
                chan,
                persistent,
                args,
            } => ResolvedTerm::Send {
                chan: Self::chan(*chan, binders),
                persistent: *persistent,
                args: args
                    .iter()
                    .map(|arg| Self::arg(arg, scope, binders))
                    .collect(),
            },
            GenTerm::Receive { arrow, binds, body } => {
                let outer = scope.len();
                let mut resolved = Vec::with_capacity(binds.len());
                for bind in binds {
                    let pats = self.pats(&bind.pats, scope);
                    resolved.push(ResolvedBind {
                        pats,
                        chan: Self::chan(bind.chan, binders),
                    });
                }
                let body = self.terms(body, scope, binders);
                scope.truncate(outer);
                ResolvedTerm::Receive {
                    arrow: *arrow,
                    binds: resolved,
                    body,
                }
            }
            GenTerm::New { inner } => {
                let binder = self.binder();
                binders.push(binder);
                let inner = self.terms(inner, scope, binders);
                binders.pop();
                ResolvedTerm::New { binder, inner }
            }
            GenTerm::Aliased {
                level,
                index,
                arrow,
                left,
                right,
                body,
            } => {
                let alias = self.alias();
                let mut aliased_scope = Vec::with_capacity(left.len() + right.len());
                let left = self.pats(left, &mut aliased_scope);
                let right = self.pats(right, &mut aliased_scope);
                let body = self.terms(body, &mut aliased_scope, binders);
                ResolvedTerm::Aliased {
                    alias,
                    level: *level,
                    index: *index,
                    arrow: *arrow,
                    left,
                    right,
                    body,
                }
            }
        }
    }
}

// Pass 1 for a whole case. Deploy 0 starts with the seed pair, which takes
// `x0`.
fn resolve_case(case: &OrdinaryCase) -> Vec<Vec<ResolvedTerm>> {
    let mut resolver = Resolver::default();
    let seed_channel = ResolvedChan::Public { level: 0, index: 0 };
    let seed_variable = resolver.variable();
    let mut deploys = Vec::with_capacity(case.deploys.len());
    for (position, terms) in case.deploys.iter().enumerate() {
        let mut resolved = Vec::with_capacity(terms.len() + 2);
        if position == 0 {
            resolved.push(ResolvedTerm::Send {
                chan: seed_channel,
                persistent: case.seed_persistent,
                args: vec![ResolvedArg::Int(0)],
            });
            resolved.push(ResolvedTerm::Receive {
                arrow: case.seed_arrow,
                binds: vec![ResolvedBind {
                    pats: vec![ResolvedPat::Bind(seed_variable)],
                    chan: seed_channel,
                }],
                body: vec![ResolvedTerm::Send {
                    chan: ResolvedChan::Sink(0),
                    persistent: false,
                    args: vec![ResolvedArg::Var(seed_variable)],
                }],
            });
        }
        let mut scope = Vec::new();
        let mut binders = Vec::new();
        resolved.extend(resolver.terms(terms, &mut scope, &mut binders));
        deploys.push(resolved);
    }
    deploys
}

// Pass 2 (S2): every channel that carries a persistent receive, across all
// deploys of the case.
fn collect_persistent_receives(terms: &[ResolvedTerm], keys: &mut BTreeSet<ResolvedChan>) {
    for term in terms {
        match term {
            ResolvedTerm::Send { .. } => {}
            ResolvedTerm::Receive { arrow, binds, body } => {
                match arrow {
                    GenArrow::Persistent => keys.extend(binds.iter().map(|bind| bind.chan)),
                    GenArrow::Linear | GenArrow::Peek => {}
                }
                collect_persistent_receives(body, keys);
            }
            ResolvedTerm::New { inner, .. } => collect_persistent_receives(inner, keys),
            ResolvedTerm::Aliased {
                level,
                index,
                arrow,
                body,
                ..
            } => {
                match arrow {
                    GenArrow::Persistent => {
                        keys.insert(ResolvedChan::Public {
                            level: *level,
                            index: *index,
                        });
                    }
                    GenArrow::Linear | GenArrow::Peek => {}
                }
                collect_persistent_receives(body, keys);
            }
        }
    }
}

fn persistent_receive_channels(deploys: &[Vec<ResolvedTerm>]) -> BTreeSet<ResolvedChan> {
    let mut keys = BTreeSet::new();
    for terms in deploys {
        collect_persistent_receives(terms, &mut keys);
    }
    keys
}

// Pass 3: printing. A persistent send prints `!!` only when no persistent
// receive shares its channel (S2).
fn send_is_persistent(
    chan: &ResolvedChan,
    persistent: bool,
    keys: &BTreeSet<ResolvedChan>,
) -> bool {
    persistent && !keys.contains(chan)
}

fn chan_text(chan: ResolvedChan) -> String {
    match chan {
        ResolvedChan::Public { level, index } => format!(r#"@"rrs-prop-c{level}{index}""#),
        ResolvedChan::Sink(index) => format!(r#"@"rrs-prop-out{index}""#),
        ResolvedChan::Fresh(binder) => format!("n{binder}"),
    }
}

fn arg_text(arg: &ResolvedArg) -> String {
    match arg {
        ResolvedArg::Int(value) => value.to_string(),
        ResolvedArg::Str(true) => r#""a""#.to_owned(),
        ResolvedArg::Str(false) => r#""b""#.to_owned(),
        ResolvedArg::Pair(left, right) => format!("[{}, {}]", arg_text(left), arg_text(right)),
        ResolvedArg::Var(variable) => format!("x{variable}"),
        ResolvedArg::Name(binder) => format!("*n{binder}"),
        ResolvedArg::Zero => "0".to_owned(),
    }
}

fn pat_text(pat: ResolvedPat) -> String {
    match pat {
        ResolvedPat::Bind(variable) => format!("@x{variable}"),
        ResolvedPat::Wildcard => "_".to_owned(),
        ResolvedPat::Int(value) => format!("@{value}"),
        ResolvedPat::Str => r#"@"a""#.to_owned(),
        ResolvedPat::HeadZero(variable) => format!("@[0, x{variable}]"),
        ResolvedPat::Both(first, second) => format!("@[x{first}, x{second}]"),
    }
}

fn pats_text(pats: &[ResolvedPat]) -> String {
    pats.iter()
        .copied()
        .map(pat_text)
        .collect::<Vec<_>>()
        .join(", ")
}

fn arrow_text(arrow: GenArrow) -> &'static str {
    match arrow {
        GenArrow::Linear => "<-",
        GenArrow::Peek => "<<-",
        GenArrow::Persistent => "<=",
    }
}

fn terms_text(terms: &[ResolvedTerm], keys: &BTreeSet<ResolvedChan>) -> String {
    terms
        .iter()
        .map(|term| term_text(term, keys))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn term_text(term: &ResolvedTerm, keys: &BTreeSet<ResolvedChan>) -> String {
    match term {
        ResolvedTerm::Send {
            chan,
            persistent,
            args,
        } => {
            let bang = match send_is_persistent(chan, *persistent, keys) {
                true => "!!",
                false => "!",
            };
            let args = args.iter().map(arg_text).collect::<Vec<_>>().join(", ");
            format!("{}{bang}({args})", chan_text(*chan))
        }
        ResolvedTerm::Receive { arrow, binds, body } => {
            let binds = binds
                .iter()
                .map(|bind| {
                    format!(
                        "{} {} {}",
                        pats_text(&bind.pats),
                        arrow_text(*arrow),
                        chan_text(bind.chan)
                    )
                })
                .collect::<Vec<_>>()
                .join(" & ");
            format!("for ({binds}) {{ {} }}", terms_text(body, keys))
        }
        ResolvedTerm::New { binder, inner } => {
            format!("new n{binder} in {{ {} }}", terms_text(inner, keys))
        }
        ResolvedTerm::Aliased {
            alias,
            level,
            index,
            arrow,
            left,
            right,
            body,
        } => {
            let arrow = arrow_text(*arrow);
            format!(
                r#"for (@l{alias}, @r{alias} <- @"rrs-prop-alias{alias}") {{ for ({} {arrow} @l{alias} & {} {arrow} @r{alias}) {{ {} }} }} | @"rrs-prop-alias{alias}"!("rrs-prop-c{level}{index}", "rrs-prop-c{level}{index}")"#,
                pats_text(left),
                pats_text(right),
                terms_text(body, keys)
            )
        }
    }
}

fn render_case(case: &OrdinaryCase) -> Vec<String> {
    let deploys = resolve_case(case);
    let keys = persistent_receive_channels(&deploys);
    deploys
        .iter()
        .map(|terms| terms_text(terms, &keys))
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayMutation {
    None,
    ReplaySeed,
    ExtraSend,
}

struct PlayReplaySummary {
    comms: usize,
}

struct PlayedDeploy {
    root: Blake2b256Hash,
    log: Vec<Event>,
    cost: i64,
    errors: Value,
}

struct BlockDeploy {
    log: Vec<Event>,
    cost: i64,
    errors: Value,
}

// The replay source and seed of one deploy under `mutation`.
fn replay_input(
    source: &str,
    deploy_label: &str,
    mutation: ReplayMutation,
) -> (String, Blake2b512Random) {
    match mutation {
        ReplayMutation::None => (source.to_owned(), seed(deploy_label)),
        ReplayMutation::ReplaySeed => (source.to_owned(), seed(&format!("{deploy_label}-mutant"))),
        ReplayMutation::ExtraSend => (format!("{source}{MUTANT_SEND}"), seed(deploy_label)),
    }
}

async fn play_deploy(
    play: &RhoRuntimeImpl,
    source: &str,
    deploy_label: &str,
    context: &str,
) -> Result<EvaluateResult, String> {
    let result = play
        .evaluate(
            source,
            Cost::create(PROPERTY_PHLO, deploy_label.to_owned()),
            HashMap::new(),
            seed(deploy_label),
        )
        .await
        .map_err(|error| format!("{context}: play evaluation failed: {error:?}"))?;
    match result.errors.as_slice() {
        [] => Ok(result),
        errors => Err(format!("{context}: play reported errors: {errors:?}")),
    }
}

async fn replay_deploy(
    replay: &RhoRuntimeImpl,
    source: &str,
    deploy_label: &str,
    mutation: ReplayMutation,
    log: &[Event],
    play_errors: &Value,
    context: &str,
) -> Result<EvaluateResult, String> {
    replay
        .rig(log.to_vec())
        .await
        .map_err(|error| format!("{context}: rig failed: {error:?}"))?;
    let (replay_source, replay_seed) = replay_input(source, deploy_label, mutation);
    let result = replay
        .evaluate(
            &replay_source,
            Cost::create(PROPERTY_PHLO, deploy_label.to_owned()),
            HashMap::new(),
            replay_seed,
        )
        .await
        .map_err(|error| format!("{context}: replay evaluation failed: {error:?}"))?;
    let errors = errors_json(&result.errors);
    match errors == *play_errors {
        true => Ok(result),
        false => Err(format!(
            "{context}: replay errors {errors} differ from play errors {play_errors}"
        )),
    }
}

// Protocol D: play and replay each create a checkpoint after every deploy.
async fn replay_with_deploy_checkpoints(
    sources: &[String],
    label: &str,
    mutation: ReplayMutation,
) -> Result<PlayReplaySummary, String> {
    let Runtimes {
        mut play,
        mut replay,
        ..
    } = fresh_runtimes().await;
    let start_root = play.get_root().await;
    let mut played = Vec::with_capacity(sources.len());
    let mut comms = 0;
    for (index, source) in sources.iter().enumerate() {
        let context = format!("protocol D, deploy {index}");
        let deploy_label = format!("{label}-{index}");
        let result = play_deploy(&play, source, &deploy_label, &context).await?;
        let checkpoint = play.create_checkpoint().await;
        comms += comms_in(&checkpoint.log).len();
        played.push(PlayedDeploy {
            root: checkpoint.root,
            log: checkpoint.log,
            cost: result.cost.value,
            errors: errors_json(&result.errors),
        });
    }
    replay
        .reset(&start_root)
        .await
        .map_err(|error| format!("protocol D: reset the replay runtime: {error:?}"))?;
    for (index, (source, deploy)) in sources.iter().zip(&played).enumerate() {
        let context = format!("protocol D, deploy {index}");
        let deploy_label = format!("{label}-{index}");
        let result = replay_deploy(
            &replay,
            source,
            &deploy_label,
            mutation,
            &deploy.log,
            &deploy.errors,
            &context,
        )
        .await?;
        replay
            .check_replay_data()
            .await
            .map_err(|error| format!("{context}: check_replay_data failed: {error:?}"))?;
        let root = replay.create_checkpoint().await.root;
        // rrs-finding-head-replay-checkpoint-produce-counter: RSpace::create_checkpoint resets the produce counter (ispace_impl.rs:82-83), ReplayRSpace::create_checkpoint keeps it (replay_rspace.rs:166-181).
        replay.take_event_log().await;
        if root != deploy.root {
            return Err(format!(
                "{context}: replay root {} differs from play root {}",
                hex::encode(&root.0),
                hex::encode(&deploy.root.0)
            ));
        }
        if result.cost.value != deploy.cost {
            return Err(format!(
                "{context}: replay cost {} differs from play cost {}",
                result.cost.value, deploy.cost
            ));
        }
    }
    Ok(PlayReplaySummary { comms })
}

// Protocol B: Casper's block protocol, the rule that validators run. Play
// takes the event log of each deploy (casper runtime.rs:565) and creates one
// checkpoint for the block. Replay resets once, discards its event log after
// each deploy (replay_runtime.rs:192-193, 599), and creates one checkpoint.
async fn replay_with_block_checkpoint(
    sources: &[String],
    label: &str,
    mutation: ReplayMutation,
) -> Result<(), String> {
    let Runtimes {
        mut play,
        mut replay,
        ..
    } = fresh_runtimes().await;
    let start_root = play.get_root().await;
    let mut played = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let context = format!("protocol B, deploy {index}");
        let deploy_label = format!("{label}-{index}");
        let result = play_deploy(&play, source, &deploy_label, &context).await?;
        let log = play.take_event_log().await;
        played.push(BlockDeploy {
            log,
            cost: result.cost.value,
            errors: errors_json(&result.errors),
        });
    }
    let block_context = format!("protocol B, block of {} deploys", sources.len());
    let block = play.create_checkpoint().await;
    match block.log.as_slice() {
        [] => {}
        events => {
            return Err(format!(
                "{block_context}: the play checkpoint log holds {} events",
                events.len()
            ))
        }
    }
    replay
        .reset(&start_root)
        .await
        .map_err(|error| format!("protocol B: reset the replay runtime: {error:?}"))?;
    for (index, (source, deploy)) in sources.iter().zip(&played).enumerate() {
        let context = format!("protocol B, deploy {index}");
        let deploy_label = format!("{label}-{index}");
        let result = replay_deploy(
            &replay,
            source,
            &deploy_label,
            mutation,
            &deploy.log,
            &deploy.errors,
            &context,
        )
        .await?;
        replay.take_event_log().await;
        replay
            .check_replay_data()
            .await
            .map_err(|error| format!("{context}: check_replay_data failed: {error:?}"))?;
        if result.cost.value != deploy.cost {
            return Err(format!(
                "{context}: replay cost {} differs from play cost {}",
                result.cost.value, deploy.cost
            ));
        }
    }
    let root = replay.create_checkpoint().await.root;
    match root == block.root {
        true => Ok(()),
        false => Err(format!(
            "{block_context}: replay root {} differs from play block root {}",
            hex::encode(&root.0),
            hex::encode(&block.root.0)
        )),
    }
}

// Both protocols must pass. The summary is the COMM count of protocol D.
async fn play_replay_equal(
    sources: &[String],
    label: &str,
    mutation: ReplayMutation,
) -> Result<PlayReplaySummary, String> {
    let summary = replay_with_deploy_checkpoints(sources, label, mutation).await?;
    replay_with_block_checkpoint(sources, label, mutation).await?;
    Ok(summary)
}

proptest! {
    #![proptest_config(property_config())]

    #[test]
    fn prop_ordinary_play_replay_equal(case in ordinary_case()) {
        let sources = render_case(&case);
        let label = format!("rrs-prop-ordinary-{}", case.seed);
        let summary = property_runtime()
            .block_on(play_replay_equal(&sources, &label, ReplayMutation::None))
            .map_err(|failure| TestCaseError::fail(format!("{failure}\nprogram: {sources:#?}")))?;
        prop_assert!(summary.comms >= 1, "the case fired no COMM: {:#?}", sources);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_play_replay_negative_control_detects_mutants() {
    let sources = [R9_PROGRAM.to_owned(), R8_PROGRAM.to_owned()];
    let label = "rrs-prop-negative-control";
    let summary = play_replay_equal(&sources, label, ReplayMutation::None)
        .await
        .unwrap_or_else(|failure| panic!("the unmutated replay must pass: {failure}"));
    assert!(summary.comms >= 1, "the control programs fire COMMs");
    for mutation in [ReplayMutation::ReplaySeed, ReplayMutation::ExtraSend] {
        match play_replay_equal(&sources, label, mutation).await {
            Err(failure) => println!("{mutation:?} detected: {failure}"),
            Ok(_) => panic!("the property accepts the {mutation:?} mutant"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Construct {
    LinearSend,
    PersistentSend,
    LinearReceive,
    PeekReceive,
    PersistentReceive,
    Join2,
    Join3,
    AliasedJoin,
    New,
    FreshChannel,
    NameArg,
    VarArg,
    PairArg,
    ListPattern,
    LiteralPattern,
    WildcardPattern,
    Depth3,
    TwoDeploys,
}

const CONSTRUCTS: [Construct; 18] = [
    Construct::LinearSend,
    Construct::PersistentSend,
    Construct::LinearReceive,
    Construct::PeekReceive,
    Construct::PersistentReceive,
    Construct::Join2,
    Construct::Join3,
    Construct::AliasedJoin,
    Construct::New,
    Construct::FreshChannel,
    Construct::NameArg,
    Construct::VarArg,
    Construct::PairArg,
    Construct::ListPattern,
    Construct::LiteralPattern,
    Construct::WildcardPattern,
    Construct::Depth3,
    Construct::TwoDeploys,
];

fn receive_construct(arrow: GenArrow) -> Construct {
    match arrow {
        GenArrow::Linear => Construct::LinearReceive,
        GenArrow::Peek => Construct::PeekReceive,
        GenArrow::Persistent => Construct::PersistentReceive,
    }
}

fn chan_constructs(chan: ResolvedChan, found: &mut BTreeSet<Construct>) {
    match chan {
        ResolvedChan::Fresh(_) => {
            found.insert(Construct::FreshChannel);
        }
        ResolvedChan::Public { .. } | ResolvedChan::Sink(_) => {}
    }
}

fn arg_constructs(arg: &ResolvedArg, found: &mut BTreeSet<Construct>) {
    match arg {
        ResolvedArg::Pair(left, right) => {
            found.insert(Construct::PairArg);
            arg_constructs(left, found);
            arg_constructs(right, found);
        }
        ResolvedArg::Var(_) => {
            found.insert(Construct::VarArg);
        }
        ResolvedArg::Name(_) => {
            found.insert(Construct::NameArg);
        }
        ResolvedArg::Int(_) | ResolvedArg::Str(_) | ResolvedArg::Zero => {}
    }
}

fn pat_constructs(pats: &[ResolvedPat], found: &mut BTreeSet<Construct>) {
    for pat in pats {
        match pat {
            ResolvedPat::HeadZero(_) | ResolvedPat::Both(..) => {
                found.insert(Construct::ListPattern);
            }
            ResolvedPat::Int(_) | ResolvedPat::Str => {
                found.insert(Construct::LiteralPattern);
            }
            ResolvedPat::Wildcard => {
                found.insert(Construct::WildcardPattern);
            }
            ResolvedPat::Bind(_) => {}
        }
    }
}

// Depth counts as in the generator: a receive body and the terms inside a
// `new` lie one level deeper, and an aliased join's body lies at depth 1.
fn term_constructs(
    terms: &[ResolvedTerm],
    depth: u8,
    keys: &BTreeSet<ResolvedChan>,
    found: &mut BTreeSet<Construct>,
) {
    for term in terms {
        if depth >= MAX_DEPTH {
            found.insert(Construct::Depth3);
        }
        match term {
            ResolvedTerm::Send {
                chan,
                persistent,
                args,
            } => {
                found.insert(match send_is_persistent(chan, *persistent, keys) {
                    true => Construct::PersistentSend,
                    false => Construct::LinearSend,
                });
                chan_constructs(*chan, found);
                for arg in args {
                    arg_constructs(arg, found);
                }
            }
            ResolvedTerm::Receive { arrow, binds, body } => {
                found.insert(receive_construct(*arrow));
                match binds.len() {
                    2 => {
                        found.insert(Construct::Join2);
                    }
                    3 => {
                        found.insert(Construct::Join3);
                    }
                    _ => {}
                }
                for bind in binds {
                    chan_constructs(bind.chan, found);
                    pat_constructs(&bind.pats, found);
                }
                term_constructs(body, depth + 1, keys, found);
            }
            ResolvedTerm::New { inner, .. } => {
                found.insert(Construct::New);
                term_constructs(inner, depth + 1, keys, found);
            }
            ResolvedTerm::Aliased {
                arrow,
                left,
                right,
                body,
                ..
            } => {
                found.insert(Construct::AliasedJoin);
                found.insert(receive_construct(*arrow));
                pat_constructs(left, found);
                pat_constructs(right, found);
                term_constructs(body, 1, keys, found);
            }
        }
    }
}

fn case_constructs(case: &OrdinaryCase) -> BTreeSet<Construct> {
    let deploys = resolve_case(case);
    let keys = persistent_receive_channels(&deploys);
    let mut found = BTreeSet::new();
    if deploys.len() == 2 {
        found.insert(Construct::TwoDeploys);
    }
    for terms in &deploys {
        term_constructs(terms, 0, &keys, &mut found);
    }
    found
}

#[test]
fn ordinary_program_generator_covers_every_construct() {
    let mut runner = TestRunner::new(property_config());
    let strategy = ordinary_case();
    let mut seen = BTreeSet::new();
    for _ in 0..PROPERTY_CASES {
        let case = strategy
            .new_tree(&mut runner)
            .expect("generate an ordinary case")
            .current();
        seen.extend(case_constructs(&case));
    }
    let missing: Vec<Construct> = CONSTRUCTS
        .iter()
        .copied()
        .filter(|construct| !seen.contains(construct))
        .collect();
    assert!(
        missing.is_empty(),
        "the generator misses {missing:?} in {PROPERTY_CASES} cases"
    );
}
