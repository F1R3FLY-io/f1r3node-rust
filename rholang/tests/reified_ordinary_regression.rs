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
use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits, HostWorkUsages};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{NativeRuntimeConfig, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
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
    "/../test-vectors/ordinary-execution-v1.json"
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
const R3_GREEDY_DATA: &str = r#"@"rrs-r3-greedy"!([1, 5]) | @"rrs-r3-greedy"!([1, 6])"#;
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

struct CostProgram {
    name: &'static str,
    source: &'static str,
    phlo: i64,
    native_limit: u64,
}

const R6_PROGRAMS: [CostProgram; 11] = [
    CostProgram {
        name: "send",
        source: r#"@"rrs-r6-send"!(1)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "receive",
        source: r#"for (@x <- @"rrs-r6-receive") { Nil }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "persistent_send",
        source: r#"@"rrs-r6-persistent-send"!!(1) | for (@x <- @"rrs-r6-persistent-send") { @"rrs-r6-persistent-send-out"!(x) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "persistent_receive",
        source: r#"for (@x <= @"rrs-r6-persistent-receive") { @"rrs-r6-persistent-receive-out"!(x) } | @"rrs-r6-persistent-receive"!(1) | @"rrs-r6-persistent-receive"!(2)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "join",
        source: r#"@"rrs-r6-join-left"!(1) | @"rrs-r6-join-right"!(2) | for (@x <- @"rrs-r6-join-left" & @y <- @"rrs-r6-join-right") { @"rrs-r6-join-out"!(x + y) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "peek",
        source: r#"@"rrs-r6-peek"!(1) | for (@x <<- @"rrs-r6-peek") { @"rrs-r6-peek-out"!(x) }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "new",
        source: r#"new a, b in { a!(1) | for (@x <- a) { b!(x) } }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "match",
        source: r#"match [1, 2, 3] {
  [a, b, c] => @"rrs-r6-match"!(a + b + c)
  _ => Nil
}"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "arithmetic",
        source: r#"@"rrs-r6-arithmetic"!((7 * 6 - 2) / 4 % 3 + 10)"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "system_process",
        source: r#"new stdout(`rho:io:stdout`) in { stdout!("rrs-r6-system-process") }"#,
        phlo: EVALUATION_PHLO,
        native_limit: NATIVE_LIMIT,
    },
    CostProgram {
        name: "out_of_phlo",
        source: r#"new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) }"#,
        phlo: 20_000,
        native_limit: 20_000,
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

fn usages_json(usages: &HostWorkUsages) -> Value {
    Value::Object(
        HostWorkDimension::ALL
            .into_iter()
            .map(|dimension| {
                (
                    format!("{dimension:?}"),
                    Value::from(usages.get(dimension).get()),
                )
            })
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
    assert_ne!(
        evaluation_order, comm_order,
        "the evaluation order must differ from the canonical COMM order"
    );
    assert_ne!(
        bind_order, comm_order,
        "the bind order must differ from the canonical COMM order"
    );
    json!({
        "channels": channel_labels(channel_names),
        "evaluation_order": evaluation_order,
        "bind_order": bind_order,
        "comm_produce_order": comm_order,
        "comm_equals_comm_new_canonical_sort": true,
        "comm": comm_json(comm),
        "logs": logs,
    })
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
        json!({
            "data": {
                "kinds": kinds_json(&data_log),
                "events": events_json(&data_log),
            },
            "receive": {
                "kinds": kinds_json(&receive_log),
                "events": events_json(&receive_log),
            },
        }),
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
        "peek": { "kinds": kinds_json(&peek_log), "events": events_json(&peek_log) },
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
        "send": { "kinds": kinds_json(&send_log), "events": events_json(&send_log) },
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

fn deterministic_order(data: &[Datum<ListParWithRandom>]) -> Vec<usize> {
    let mut keyed: Vec<(Blake2b256Hash, usize)> = data
        .iter()
        .enumerate()
        .map(|(index, datum)| {
            let bytes = bincode::serialize(datum).expect("bincode-serialize the datum");
            (Blake2b256Hash::new(&bytes), index)
        })
        .collect();
    keyed.sort();
    keyed.into_iter().map(|(_, index)| index).collect()
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
            "data": { "kinds": kinds_json(&data_log), "events": events_json(&data_log) },
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
            "receive": { "kinds": kinds_json(&receive_log), "events": events_json(&receive_log) },
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
        "kinds": kinds_json(&log),
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

fn native_config(limit: u64, host_work: HostWorkBudget) -> NativeRuntimeConfig {
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
        NativePhloExecutionContract::new(controls, &binding).expect("build the native contract");
    NativeRuntimeConfig::new(
        contract,
        NativeBudgetTraceLimits {
            attempts: 100_000,
            path_segments: 4096,
            regions: NativePhloRegionLimits {
                regions: 1000,
                encoded_authority_bytes: 10_000_000,
            },
        },
        host_work,
    )
}

fn native_json(result: &EvaluateResult, host_work: &HostWorkBudget) -> Value {
    let comm_identity_counts: Vec<usize> = result
        .native_operation_recording
        .as_ref()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.comm.as_ref())
                .map(|comm| comm.source.produces.len() + comm.source.repetitions.len())
                .collect()
        })
        .unwrap_or_default();
    json!({
        "native_phlo_usage": result.native_phlo_usage,
        "errors": errors_json(&result.errors),
        "operation_count": result
            .native_operation_recording
            .as_ref()
            .map(|rows| rows.len()),
        "comm_identity_counts": comm_identity_counts,
        "host_work_usage": usages_json(&host_work.usages()),
        "host_work_rejection": format!("{:?}", host_work.rejection()),
    })
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
    let host_work = {
        let Runtimes { play, .. } = fresh_runtimes().await;
        let (result, report) = play
            .evaluate_with_host_work(
                program.source,
                Cost::create(program.phlo, label.clone()),
                HashMap::new(),
                seed(&label),
                HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)),
            )
            .await
            .unwrap_or_else(|error| panic!("{label} host-work evaluation failed: {error:?}"));
        json!({
            "cost": result.cost.value,
            "errors": errors_json(&result.errors),
            "host_work_usage": usages_json(&report.usages),
            "host_work_rejection": format!("{:?}", report.rejection),
        })
    };
    let native = {
        let Runtimes { mut play, .. } = fresh_runtimes().await;
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let result = play
            .evaluate_with_native_phlo(
                program.source,
                HashMap::new(),
                seed(&label),
                None,
                native_config(program.native_limit, budget.clone()),
            )
            .await
            .unwrap_or_else(|error| panic!("{label} native evaluation failed: {error:?}"));
        native_json(&result, &budget)
    };
    json!({
        "source": program.source,
        "phlo_limit": program.phlo,
        "native_limit": program.native_limit,
        "native_weights": NATIVE_WEIGHTS,
        "standard": standard,
        "host_work": host_work,
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
        "the host-work-metered native path observes COMMs in several programs"
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
            "kinds": kinds_json(&log),
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
            "event_log_length": play_checkpoint.log.len(),
            "event_kinds": kinds_json(&play_checkpoint.log),
        }),
    );
}
