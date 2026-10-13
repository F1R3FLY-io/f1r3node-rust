//! The examples of Reified RSpaces (P10 step S2; plan decisions 85 and 86, design v2 section 2.3).
//!
//! Each example of `examples/reified_rspaces/` runs here. Every deploy of an example plays
//! natively at the v6 limits (decision 79), certifies by native replay and reproduces its play.
//! Then the values on the result channels of the example must equal its expected values. An
//! expected value is a Rholang literal, which the compiler normalizes, so the comparison is
//! between canonical pars. An example that shows an abort runs its last deploy to that abort,
//! and the harness checks only the error code of that deploy.
//!
//! Each example prints one record line: its deploy files, its expected and actual values, the
//! phlo of each deploy and the post-state root (leaf 9224). The table of this file holds the
//! expected values (design v2, deviation D-B). `MANIFEST.toml` is the index that doc_lint reads,
//! and doc_lint checks that each example of the manifest names this file and its file name.

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;

use models::rhoapi::Par;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::reified::vectordb_services::Neighbour;
use support::backends::{self, services, EmbedMode, FakeMirror, MirrorCall, MockEmbedder};
use support::native::{assert_reproduces, certify, host_work, NativeChain, AMPLE_LIMIT};
use support::session::{error_codes, Injections};

const LIMIT: u64 = AMPLE_LIMIT;

/// The expected outcome of an example.
enum Expect {
    /// The datums of each result channel. A datum is the argument list of one send, written as
    /// the Rholang literals of the arguments.
    Values(&'static [(&'static str, &'static [&'static str])]),
    /// The error code of the last deploy. Every earlier deploy succeeds.
    Error(&'static str),
}

/// The services of an example.
enum Services {
    /// The example uses no external service.
    None,
    /// The example calls `rho:vectordb:embed`. The harness installs the fake embedder, whose
    /// vector of a text has two values: `0.5 + len(text)` and `1.5 + len(text)`.
    Embedder,
    /// The example stores records with a ChromaDB mirror in its first deploy, and its last
    /// deploy asks `rho:vectordb:ann` for the neighbours of `query`. The harness installs a fake
    /// mirror, and before the last deploy it installs a fake whose neighbours are the published
    /// records, nearest to `query` first, as ChromaDB would answer.
    Mirror { query: [f32; 2] },
}

/// One example: its name in `MANIFEST.toml`, its deploys in order, its services and its
/// expected outcome.
struct Example {
    name: &'static str,
    deploys: &'static [(&'static str, &'static str)],
    services: Services,
    expect: Expect,
}

/// The neighbours of `query` among the records that the deploys published to `mirror`, by
/// cosine distance, nearest first.
fn published_neighbours(mirror: &FakeMirror, query: [f32; 2]) -> Vec<Neighbour> {
    let collection = mirror
        .calls()
        .into_iter()
        .find_map(|call| match call {
            MirrorCall::Ensure { collection, .. } => Some(collection),
            _ => None,
        })
        .expect("the first deploy ensured a mirror collection");
    let held = mirror
        .collection(&collection)
        .expect("the mirror holds the collection");
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mut neighbours: Vec<Neighbour> = held
        .records
        .values()
        .map(|record| {
            let dot: f32 = record
                .embedding
                .iter()
                .zip(query.iter())
                .map(|(a, b)| a * b)
                .sum();
            let distance = 1.0 - dot / (norm(&record.embedding) * norm(&query));
            Neighbour {
                id: record.id.to_vec(),
                distance,
            }
        })
        .collect();
    neighbours.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)));
    neighbours
}

macro_rules! deploy {
    ($file:literal) => {
        (
            $file,
            include_str!(concat!("../../examples/reified_rspaces/", $file)),
        )
    };
}

/// The datums of the literal argument lists `datums`, sorted as `NativeChain::values` sorts them.
fn expected(datums: &[&str]) -> Vec<Vec<Par>> {
    let mut values: Vec<Vec<Par>> = datums
        .iter()
        .map(|arguments| {
            let source = format!("@\"rrs.examples.expected\"!({arguments})");
            let par = Compiler::source_to_adt(&source).expect("the expected values parse");
            assert_eq!(par.sends.len(), 1, "one send for {arguments}");
            par.sends[0].data.clone()
        })
        .collect();
    values.sort_by_cached_key(|pars| format!("{pars:?}"));
    values
}

/// The datums `datums` in Rholang syntax, for messages.
fn pretty_datums(datums: &[Vec<Par>]) -> String {
    let shown: Vec<String> = datums
        .iter()
        .map(|pars| {
            let args: Vec<String> = pars
                .iter()
                .map(|par| PrettyPrinter::new().build_string_from_message(par))
                .collect();
            format!("({})", args.join(", "))
        })
        .collect();
    format!("[{}]", shown.join(", "))
}

/// Runs `example` and checks its outcome. Returns the record line of the example and the
/// post-state root of its last deploy, in hex.
async fn run(example: &Example) -> (String, String) {
    let embedder = MockEmbedder::new("rrs-examples", 2, 0.5, EmbedMode::Vectors);
    let first_mirror = FakeMirror::new(Vec::new(), 0);
    let lock = match example.services {
        Services::None => None,
        Services::Embedder | Services::Mirror { .. } => {
            Some(backends::install(services(&embedder, &first_mirror)).await)
        }
    };
    let mut chain = NativeChain::new().await;
    let mut phlo = Vec::with_capacity(example.deploys.len());
    let mut root = String::new();
    for (index, (file, source)) in example.deploys.iter().enumerate() {
        let label = format!("{} {}", example.name, file);
        let last = index + 1 == example.deploys.len();
        if let (Services::Mirror { query }, true, Some(lock)) = (&example.services, last, &lock) {
            backends::writer().flush().await;
            let neighbours = published_neighbours(&first_mirror, *query);
            lock.replace(services(&embedder, &FakeMirror::new(neighbours, 1)));
        }
        let played = chain
            .play_with(source, &label, LIMIT, host_work(), Injections::new())
            .await;
        match (&example.expect, last) {
            (Expect::Error(code), true) => {
                assert_eq!(
                    error_codes(&played.result.errors),
                    vec![code.to_string()],
                    "{label}: the error codes"
                );
            }
            _ => assert!(
                played.result.errors.is_empty(),
                "{label}: {:?}",
                played.result.errors
            ),
        }
        let certification = certify(&played, source, LIMIT).await;
        assert_reproduces(&label, &played, &certification);
        phlo.push(played.result.native_phlo_usage);
        root = hex::encode(played.root.bytes());
    }
    let files: Vec<&str> = example.deploys.iter().map(|(file, _)| *file).collect();
    let mut outcome = Vec::new();
    if let Expect::Values(channels) = &example.expect {
        for (channel, datums) in channels.iter() {
            let mut actual = chain.values(channel).await;
            actual.sort_by_cached_key(|pars| format!("{pars:?}"));
            let expected = expected(datums);
            assert!(
                actual == expected,
                "{}: the values on {channel}: actual {}, expected {}",
                example.name,
                pretty_datums(&actual),
                pretty_datums(&expected)
            );
            outcome.push(format!("{channel} = {}", pretty_datums(&actual)));
        }
    } else if let Expect::Error(code) = &example.expect {
        outcome.push(format!("error {code}"));
    }
    let record = format!(
        "example {}: files {:?}; expected and actual {}; phlo {:?}; root {}",
        example.name,
        files,
        outcome.join(", "),
        phlo,
        root
    );
    println!("{record}");
    (record, root)
}

/// The directory of the example files.
fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reified_rspaces")
}

macro_rules! values {
    ($($channel:literal => [$($datum:literal),* $(,)?]),* $(,)?) => {
        Expect::Values(&[$(($channel, &[$($datum),*])),*])
    };
}

const QUEUE_FIFO: Example = Example {
    name: "queue-fifo",
    deploys: &[deploy!("queue_fifo.rho")],
    services: Services::None,
    expect: values!("rrs.example.queue" => [r#"["A", "B", "C"]"#]),
};
const QUEUE_REJECTS_A_SEND_PARAM: Example = Example {
    name: "queue-rejects-a-send-param",
    deploys: &[deploy!("queue_rejects_a_send_param.rho")],
    services: Services::None,
    expect: Expect::Error("UnknownParam"),
};
const QUEUE_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "queue-peeks-and-persistence",
    deploys: &[deploy!("queue_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.queue.aspects" => [
        r#"["peeked", "A"]"#,
        r#"["taken", "A"]"#,
        r#"["drained", "A"]"#,
        r#"["drained", "B"]"#,
    ]),
};
const STACK_LIFO: Example = Example {
    name: "stack-lifo",
    deploys: &[deploy!("stack_lifo.rho")],
    services: Services::None,
    expect: values!("rrs.example.stack" => [r#"["C", "B", "A"]"#]),
};
const STACK_REJECTS_A_QUERY: Example = Example {
    name: "stack-rejects-a-query",
    deploys: &[deploy!("stack_rejects_a_query.rho")],
    services: Services::None,
    expect: Expect::Error("QuerySpecUnsupported"),
};
const STACK_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "stack-peeks-and-persistence",
    deploys: &[deploy!("stack_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.stack.aspects" => [
        r#"["peeked", "B"]"#,
        r#"["taken", "B"]"#,
        r#"["drained", "B"]"#,
        r#"["drained", "A"]"#,
    ]),
};
const SET_IDEMPOTENT: Example = Example {
    name: "set-idempotent",
    deploys: &[deploy!("set_idempotent.rho")],
    services: Services::None,
    expect: values!("rrs.example.set" => [r#""a""#, r#""probe""#]),
};
const SET_REJECTS_A_SEND_PARAM: Example = Example {
    name: "set-rejects-a-send-param",
    deploys: &[deploy!("set_rejects_a_send_param.rho")],
    services: Services::None,
    expect: Expect::Error("UnknownParam"),
};
const SET_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "set-peeks-and-persistence",
    deploys: &[deploy!("set_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.set.aspects" => [
        r#"["peeked", "a"]"#,
        r#"["taken", "a"]"#,
        r#"["left", "p"]"#,
        r#"["again", "p"]"#,
    ]),
};
const CELL_EXACTLY_ONCE: Example = Example {
    name: "cell-exactly-once",
    deploys: &[deploy!("cell_exactly_once.rho")],
    services: Services::None,
    expect: values!("rrs.example.cell" => ["1", "2"]),
};
const CELL_OCCUPIED: Example = Example {
    name: "cell-occupied",
    deploys: &[deploy!("cell_occupied.rho")],
    services: Services::None,
    expect: Expect::Error("CellOccupied"),
};
const CELL_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "cell-peeks-and-persistence",
    deploys: &[deploy!("cell_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.cell.aspects" => [
        r#"["peeked", 1]"#,
        r#"["taken", 1]"#,
        r#"["persistent", 2]"#,
        r#"["again", 2]"#,
    ]),
};
const BAG_BASICS: Example = Example {
    name: "bag-basics",
    deploys: &[deploy!("bag_basics.rho")],
    services: Services::None,
    expect: values!("rrs.example.bag" => [r#"["matched", 2]"#, r#"["rest", 4]"#]),
};
const BAG_REJECTS_A_QUERY: Example = Example {
    name: "bag-rejects-a-query",
    deploys: &[deploy!("bag_rejects_a_query.rho")],
    services: Services::None,
    expect: Expect::Error("QuerySpecUnsupported"),
};
const BAG_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "bag-peeks-and-persistence",
    deploys: &[deploy!("bag_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.bag.aspects" => [
        r#"["peeked", 5]"#,
        r#"["first", 5]"#,
        r#"["second", 5]"#,
        r#"["persistent", 7]"#,
        r#"["again", 7]"#,
    ]),
};
const PRIORITY_QUEUE: Example = Example {
    name: "priority-queue",
    deploys: &[deploy!("priority_queue.rho")],
    services: Services::None,
    expect: values!("rrs.example.pq" => [r#"["zero", "two", "low"]"#, r#""lane two""#]),
};
const PRIORITY_QUEUE_REJECTS_A_LEVEL: Example = Example {
    name: "priority-queue-rejects-a-level",
    deploys: &[deploy!("priority_queue_rejects_a_level.rho")],
    services: Services::None,
    expect: Expect::Error("InvalidParam"),
};
const PRIORITY_QUEUE_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "priority-queue-peeks-and-persistence",
    deploys: &[deploy!("priority_queue_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.pq.aspects" => [
        r#"["peeked", "urgent"]"#,
        r#"["taken", "urgent"]"#,
        r#"["drained", "x"]"#,
        r#"["drained", "x"]"#,
    ]),
};
const PATHMAP_PREFIX_AGGREGATION: Example = Example {
    name: "pathmap-prefix-aggregation",
    deploys: &[deploy!("pathmap_prefix_aggregation.rho")],
    services: Services::None,
    expect: values!("rrs.example.pathmap" => [
        r#"{|[2, "hi"]|}"#,
        r#"{|[2, "hello"]|}"#,
        r#"{|[3, "there"]|}"#,
        "[2, 42]",
    ]),
};
const PATHMAP_HIERARCHICAL_SPACES: Example = Example {
    name: "pathmap-hierarchical-spaces",
    deploys: &[deploy!("pathmap_hierarchical_spaces.rho")],
    services: Services::None,
    expect: values!("rrs.example.pathmap.tree" => [
        r#"["south", 20]"#,
        r#"["prefix", ["north", 10]]"#,
        r#"["prefix", ["north", 11]]"#,
    ]),
};
const ARRAY_INDEXED_SPACE: Example = Example {
    name: "array-indexed-space",
    deploys: &[deploy!("array_indexed_space.rho")],
    services: Services::None,
    expect: values!("rrs.example.array" => [r#"["x", "y"]"#]),
};
const ARRAY_OUT_OF_NAMES: Example = Example {
    name: "array-out-of-names",
    deploys: &[deploy!("array_out_of_names.rho")],
    services: Services::None,
    expect: Expect::Error("OutOfNames"),
};
const VECTOR_INDEXED_SPACE: Example = Example {
    name: "vector-indexed-space",
    deploys: &[deploy!("vector_indexed_space.rho")],
    services: Services::None,
    expect: values!("rrs.example.vector" => ["10"]),
};
const HASHSET_DEDUP_SPACES: Example = Example {
    name: "hashset-dedup-spaces",
    deploys: &[deploy!("hashset_dedup_spaces.rho")],
    services: Services::None,
    expect: values!("rrs.example.hashset" => [
        r#""set took a unit""#,
        r#""bag took a unit""#,
        r#""bag took a second unit""#,
        r#"["set", "seq"]"#,
    ]),
};
const HASHSET_REJECTS_A_PAYLOAD: Example = Example {
    name: "hashset-rejects-a-payload",
    deploys: &[deploy!("hashset_rejects_a_payload.rho")],
    services: Services::None,
    expect: Expect::Error("PayloadNotEmpty"),
};
const HASHSET_REJECTS_A_PATTERN: Example = Example {
    name: "hashset-rejects-a-pattern",
    deploys: &[deploy!("hashset_rejects_a_pattern.rho")],
    services: Services::None,
    expect: Expect::Error("PatternNotEmpty"),
};
const SPACE_QUALIFIER_DEFAULT: Example = Example {
    name: "space-qualifier-default",
    deploys: &[
        deploy!("space_qualifier_default_1.rho"),
        deploy!("space_qualifier_default_2.rho"),
    ],
    services: Services::None,
    expect: values!("rrs.example.qualifier.default" => [
        r#"["default", {"theme": "dark", "lang": "en"}]"#,
    ]),
};
const SPACE_QUALIFIER_TEMP: Example = Example {
    name: "space-qualifier-temp",
    deploys: &[deploy!("space_qualifier_temp.rho")],
    services: Services::None,
    expect: values!("rrs.example.qualifier.temp" => [r#"["temp", "session-token"]"#]),
};
const SPACE_QUALIFIER_SEQ: Example = Example {
    name: "space-qualifier-seq",
    deploys: &[deploy!("space_qualifier_seq.rho")],
    services: Services::None,
    expect: values!("rrs.example.qualifier.seq" => [r#"["seq", "local data"]"#]),
};
const SPACE_QUALIFIERS_SEQ_SENT: Example = Example {
    name: "space-qualifiers-seq-sent",
    deploys: &[deploy!("space_qualifiers_seq_sent.rho")],
    services: Services::None,
    expect: Expect::Error("SeqSent"),
};
const SEQ_REJECTS_CONCURRENT_USES: Example = Example {
    name: "seq-rejects-concurrent-uses",
    deploys: &[deploy!("seq_rejects_concurrent_uses.rho")],
    services: Services::None,
    expect: Expect::Error("SeqConcurrent"),
};
const THEORY_VALIDATED_SPACE: Example = Example {
    name: "theory-validated-space",
    deploys: &[deploy!("theory_validated_space.rho")],
    services: Services::None,
    expect: values!("rrs.example.theory" => ["49"]),
};
const THEORY_REFUSES_A_VALUE: Example = Example {
    name: "theory-refuses-a-value",
    deploys: &[deploy!("theory_refuses_a_value.rho")],
    services: Services::None,
    expect: Expect::Error("TheoryRefuted"),
};
const GET_SPACE_AGENT: Example = Example {
    name: "get-space-agent",
    deploys: &[deploy!("get_space_agent.rho")],
    services: Services::None,
    expect: values!("rrs.example.agent" => ["[true, true]"]),
};
const VECTORDB_QUERIES: Example = Example {
    name: "vectordb-queries",
    deploys: &[deploy!("vectordb_queries.rho")],
    services: Services::None,
    expect: values!("rrs.example.vectordb" => [
        r#"["defaults", [1, 0]]"#,
        r#"["dot", [3, 4]]"#,
        r#"["threshold", [0, 1]]"#,
        r#"["top two", [[1, 0], [3, 4]]]"#,
    ]),
};
const VECTORDB_LIST_SCORES: Example = Example {
    name: "vectordb-list-scores",
    deploys: &[deploy!("vectordb_list_scores.rho")],
    services: Services::None,
    expect: values!("rrs.example.vectordb.list" => [r#"[[0.0, "origin"], [5.0, "far"]]"#]),
};
const VECTORDB_MULTI_TOPIC: Example = Example {
    name: "vectordb-multi-topic",
    deploys: &[deploy!("vectordb_multi_topic.rho")],
    services: Services::None,
    expect: values!("rrs.example.vectordb.topics" => [
        r#"["sports", "football"]"#,
        r#"["music", "violin"]"#,
    ]),
};
const VECTORDB_DIMENSION_MISMATCH: Example = Example {
    name: "vectordb-dimension-mismatch",
    deploys: &[deploy!("vectordb_dimension_mismatch.rho")],
    services: Services::None,
    expect: Expect::Error("DimensionMismatch"),
};
const VECTORDB_CAPACITY_EXCEEDED: Example = Example {
    name: "vectordb-capacity-exceeded",
    deploys: &[deploy!("vectordb_capacity_exceeded.rho")],
    services: Services::None,
    expect: Expect::Error("CapacityExceeded"),
};
const VECTORDB_PEEKS_AND_PERSISTENCE: Example = Example {
    name: "vectordb-peeks-and-persistence",
    deploys: &[deploy!("vectordb_peeks_and_persistence.rho")],
    services: Services::None,
    expect: values!("rrs.example.vectordb.aspects" => [
        r#"["peeked", [1, 0]]"#,
        r#"["first", [1, 0]]"#,
        r#"["second", [1, 0]]"#,
        r#"["persistent", [0, 1]]"#,
        r#"["again", [0, 1]]"#,
    ]),
};
const VECTOR_OPS: Example = Example {
    name: "vector-ops",
    deploys: &[deploy!("vector_ops.rho")],
    services: Services::None,
    expect: values!("rrs.example.vector.ops" => [
        "[[true, [4, 6]], [true, [3, 6]], [true, 11], [true, 5.0], [true, 5.0]]",
    ]),
};
const DETERMINISM_DEMO: Example = Example {
    name: "determinism-demo",
    deploys: &[deploy!("determinism_demo.rho")],
    services: Services::None,
    expect: values!("rrs.example.determinism" => [r#""green""#]),
};
const MULTI_SPACE_ISOLATION: Example = Example {
    name: "multi-space-isolation",
    deploys: &[deploy!("multi_space_isolation.rho")],
    services: Services::None,
    expect: values!("rrs.example.isolation" => [
        r#"["left", "left mail"]"#,
        r#"["right", "right mail"]"#,
    ]),
};
const CROSS_SPACE_JOIN: Example = Example {
    name: "cross-space-join",
    deploys: &[deploy!("cross_space_join.rho")],
    services: Services::None,
    expect: Expect::Error("CrossSpaceJoin"),
};
const SPACE_ANNOTATIONS: Example = Example {
    name: "space-annotations",
    deploys: &[deploy!("space_annotations.rho")],
    services: Services::None,
    expect: values!("rrs.example.annotations" => [r#"["one", "two"]"#]),
};
const HANDLE_METHODS: Example = Example {
    name: "handle-methods",
    deploys: &[deploy!("handle_methods.rho")],
    services: Services::None,
    expect: values!("rrs.example.handle" => [r#"["default", "hashmap", "bag", "stored by produce"]"#]),
};
const FACTORY_REJECTS_A_BAD_ARGUMENT: Example = Example {
    name: "factory-rejects-a-bad-argument",
    deploys: &[deploy!("factory_rejects_a_bad_argument.rho")],
    services: Services::None,
    expect: Expect::Error("BadConstructorArgument"),
};
const FACTORY_REJECTS_AN_UNKNOWN_THEORY: Example = Example {
    name: "factory-rejects-an-unknown-theory",
    deploys: &[deploy!("factory_rejects_an_unknown_theory.rho")],
    services: Services::None,
    expect: Expect::Error("UnknownTheory"),
};
const FACTORY_REJECTS_AN_UNKNOWN_PARAM: Example = Example {
    name: "factory-rejects-an-unknown-param",
    deploys: &[deploy!("factory_rejects_an_unknown_param.rho")],
    services: Services::None,
    expect: Expect::Error("UnknownParam"),
};
const FACTORY_REJECTS_A_MISSING_PARAM: Example = Example {
    name: "factory-rejects-a-missing-param",
    deploys: &[deploy!("factory_rejects_a_missing_param.rho")],
    services: Services::None,
    expect: Expect::Error("MissingParam"),
};
const FACTORY_REJECTS_AN_INVALID_PARAM: Example = Example {
    name: "factory-rejects-an-invalid-param",
    deploys: &[deploy!("factory_rejects_an_invalid_param.rho")],
    services: Services::None,
    expect: Expect::Error("InvalidParam"),
};
const FACTORY_REJECTS_A_POSITIONAL_PARAM: Example = Example {
    name: "factory-rejects-a-positional-param",
    deploys: &[deploy!("factory_rejects_a_positional_param.rho")],
    services: Services::None,
    expect: Expect::Error("PositionalParamsUnsupported"),
};
const FACTORY_REJECTS_A_FORGED_PARAMS_TUPLE: Example = Example {
    name: "factory-rejects-a-forged-params-tuple",
    deploys: &[deploy!("factory_rejects_a_forged_params_tuple.rho")],
    services: Services::None,
    expect: Expect::Error("Malformed"),
};
const FACTORY_REJECTS_A_PERSISTENT_CALL: Example = Example {
    name: "factory-rejects-a-persistent-call",
    deploys: &[deploy!("factory_rejects_a_persistent_call.rho")],
    services: Services::None,
    expect: Expect::Error("PersistentFactoryCall"),
};
const SYSTEM_OP_REJECTS_A_PERSISTENT_CALL: Example = Example {
    name: "system-op-rejects-a-persistent-call",
    deploys: &[deploy!("system_op_rejects_a_persistent_call.rho")],
    services: Services::None,
    expect: Expect::Error("PersistentOpCall"),
};
const SYSTEM_OP_REJECTS_PARAMS: Example = Example {
    name: "system-op-rejects-params",
    deploys: &[deploy!("system_op_rejects_params.rho")],
    services: Services::None,
    expect: Expect::Error("ParamsUnsupported"),
};
const HANDLE_REJECTS_A_PERSISTENT_CALL: Example = Example {
    name: "handle-rejects-a-persistent-call",
    deploys: &[deploy!("handle_rejects_a_persistent_call.rho")],
    services: Services::None,
    expect: Expect::Error("PersistentHandleCall"),
};
const TEMP_SPACE_ENDS_WITH_ITS_DEPLOY: Example = Example {
    name: "temp-space-ends-with-its-deploy",
    deploys: &[
        deploy!("temp_space_ends_with_its_deploy_1.rho"),
        deploy!("temp_space_ends_with_its_deploy_2.rho"),
    ],
    services: Services::None,
    expect: Expect::Error("DanglingHandle"),
};
const FIPS_EXAMPLE: Example = Example {
    name: "fips-example",
    deploys: &[deploy!("fips_example.rho")],
    services: Services::None,
    expect: values!("rrs.example.fips" => [
        r#"["joins", 3, 7]"#,
        r#"["forged", 5]"#,
        r#"["agent", true]"#,
    ]),
};
const SPECULATIVE_EXECUTION: Example = Example {
    name: "speculative-execution",
    deploys: &[deploy!("speculative_execution.rho")],
    services: Services::None,
    expect: values!("rrs.example.speculative" => [
        r#"["committed", 700]"#,
        r#"["rolled back", 700]"#,
    ]),
};
const VECTORDB_EMBED: Example = Example {
    name: "vectordb-embed",
    deploys: &[deploy!("vectordb_embed.rho")],
    services: Services::Embedder,
    expect: values!("rrs.example.embed" => [r#"[true, [12.5f32, 13.5f32], "rrs-examples"]"#]),
};
const VECTORDB_MIRROR: Example = Example {
    name: "vectordb-mirror",
    deploys: &[
        deploy!("vectordb_mirror_1.rho"),
        deploy!("vectordb_mirror_2.rho"),
    ],
    services: Services::Mirror { query: [1.0, 0.0] },
    expect: values!("result" => ["[[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]"]),
};
const AGENT_HASHMAP_BAG: Example = Example {
    name: "agent-hashmap-bag",
    deploys: &[deploy!("agent_hashmap_bag.rho")],
    services: Services::None,
    expect: values!("agentHashmapBag.demo" => ["3"]),
};
const AGENT_WRAPPER_FORWARDS_TO_NATIVE: Example = Example {
    name: "agent-wrapper-forwards-to-native",
    deploys: &[deploy!("agent_wrapper_forwards_to_native.rho")],
    services: Services::None,
    expect: values!("rrs.example.wrapper" => [r#""work""#]),
};
const AGENT_SPACE_REJECTS_A_PERSISTENT_SEND: Example = Example {
    name: "agent-space-rejects-a-persistent-send",
    deploys: &[deploy!("agent_space_rejects_a_persistent_send.rho")],
    services: Services::None,
    expect: Expect::Error("PersistentSendOnAgentSpace"),
};
const AGENT_SPACE_REJECTS_PARAMS: Example = Example {
    name: "agent-space-rejects-params",
    deploys: &[deploy!("agent_space_rejects_params.rho")],
    services: Services::None,
    expect: Expect::Error("ParamsUnsupportedOnAgentSpace"),
};
const AGENT_SPACE_REJECTS_A_QUERY: Example = Example {
    name: "agent-space-rejects-a-query",
    deploys: &[deploy!("agent_space_rejects_a_query.rho")],
    services: Services::None,
    expect: Expect::Error("QuerySpecUnsupportedOnAgentSpace"),
};
const AGENT_SPACE_REJECTS_A_SPANNING_GUARD: Example = Example {
    name: "agent-space-rejects-a-spanning-guard",
    deploys: &[deploy!("agent_space_rejects_a_spanning_guard.rho")],
    services: Services::None,
    expect: Expect::Error("AgentGuardSpansJoin"),
};
const AGENT_SPACE_REJECTS_A_DEEP_PATTERN: Example = Example {
    name: "agent-space-rejects-a-deep-pattern",
    deploys: &[deploy!("agent_space_rejects_a_deep_pattern.rho")],
    services: Services::None,
    expect: Expect::Error("AgentPatternTooDeep"),
};
const AGENT_CAPABILITY_REJECTS_A_PERSISTENT_CALL: Example = Example {
    name: "agent-capability-rejects-a-persistent-call",
    deploys: &[deploy!("agent_capability_rejects_a_persistent_call.rho")],
    services: Services::None,
    expect: Expect::Error("PersistentCapabilityCall"),
};

/// One test function for each example, and the list `EXAMPLES` of the same examples, so the
/// list and the test functions cannot differ.
macro_rules! example_tests {
    ($($test:ident => $example:ident),* $(,)?) => {
        /// Every example of the table, in the order of `MANIFEST.toml`.
        const EXAMPLES: &[&Example] = &[$(&$example),*];
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn $test() { run(&$example).await; }
        )*
    };
}

example_tests! {
    example_queue_fifo => QUEUE_FIFO,
    example_queue_rejects_a_send_param => QUEUE_REJECTS_A_SEND_PARAM,
    example_queue_peeks_and_persistence => QUEUE_PEEKS_AND_PERSISTENCE,
    example_stack_lifo => STACK_LIFO,
    example_stack_rejects_a_query => STACK_REJECTS_A_QUERY,
    example_stack_peeks_and_persistence => STACK_PEEKS_AND_PERSISTENCE,
    example_set_idempotent => SET_IDEMPOTENT,
    example_set_rejects_a_send_param => SET_REJECTS_A_SEND_PARAM,
    example_set_peeks_and_persistence => SET_PEEKS_AND_PERSISTENCE,
    example_cell_exactly_once => CELL_EXACTLY_ONCE,
    example_cell_occupied => CELL_OCCUPIED,
    example_cell_peeks_and_persistence => CELL_PEEKS_AND_PERSISTENCE,
    example_bag_basics => BAG_BASICS,
    example_bag_rejects_a_query => BAG_REJECTS_A_QUERY,
    example_bag_peeks_and_persistence => BAG_PEEKS_AND_PERSISTENCE,
    example_priority_queue => PRIORITY_QUEUE,
    example_priority_queue_rejects_a_level => PRIORITY_QUEUE_REJECTS_A_LEVEL,
    example_priority_queue_peeks_and_persistence => PRIORITY_QUEUE_PEEKS_AND_PERSISTENCE,
    example_pathmap_prefix_aggregation => PATHMAP_PREFIX_AGGREGATION,
    example_pathmap_hierarchical_spaces => PATHMAP_HIERARCHICAL_SPACES,
    example_array_indexed_space => ARRAY_INDEXED_SPACE,
    example_array_out_of_names => ARRAY_OUT_OF_NAMES,
    example_vector_indexed_space => VECTOR_INDEXED_SPACE,
    example_hashset_dedup_spaces => HASHSET_DEDUP_SPACES,
    example_hashset_rejects_a_payload => HASHSET_REJECTS_A_PAYLOAD,
    example_hashset_rejects_a_pattern => HASHSET_REJECTS_A_PATTERN,
    example_space_qualifier_default => SPACE_QUALIFIER_DEFAULT,
    example_space_qualifier_temp => SPACE_QUALIFIER_TEMP,
    example_space_qualifier_seq => SPACE_QUALIFIER_SEQ,
    example_space_qualifiers_seq_sent => SPACE_QUALIFIERS_SEQ_SENT,
    example_seq_rejects_concurrent_uses => SEQ_REJECTS_CONCURRENT_USES,
    example_theory_validated_space => THEORY_VALIDATED_SPACE,
    example_theory_refuses_a_value => THEORY_REFUSES_A_VALUE,
    example_get_space_agent => GET_SPACE_AGENT,
    example_vectordb_queries => VECTORDB_QUERIES,
    example_vectordb_list_scores => VECTORDB_LIST_SCORES,
    example_vectordb_multi_topic => VECTORDB_MULTI_TOPIC,
    example_vectordb_dimension_mismatch => VECTORDB_DIMENSION_MISMATCH,
    example_vectordb_capacity_exceeded => VECTORDB_CAPACITY_EXCEEDED,
    example_vectordb_peeks_and_persistence => VECTORDB_PEEKS_AND_PERSISTENCE,
    example_vector_ops => VECTOR_OPS,
    example_determinism_demo => DETERMINISM_DEMO,
    example_multi_space_isolation => MULTI_SPACE_ISOLATION,
    example_cross_space_join => CROSS_SPACE_JOIN,
    example_space_annotations => SPACE_ANNOTATIONS,
    example_handle_methods => HANDLE_METHODS,
    example_factory_rejects_a_bad_argument => FACTORY_REJECTS_A_BAD_ARGUMENT,
    example_factory_rejects_an_unknown_theory => FACTORY_REJECTS_AN_UNKNOWN_THEORY,
    example_factory_rejects_an_unknown_param => FACTORY_REJECTS_AN_UNKNOWN_PARAM,
    example_factory_rejects_a_missing_param => FACTORY_REJECTS_A_MISSING_PARAM,
    example_factory_rejects_an_invalid_param => FACTORY_REJECTS_AN_INVALID_PARAM,
    example_factory_rejects_a_positional_param => FACTORY_REJECTS_A_POSITIONAL_PARAM,
    example_factory_rejects_a_forged_params_tuple => FACTORY_REJECTS_A_FORGED_PARAMS_TUPLE,
    example_factory_rejects_a_persistent_call => FACTORY_REJECTS_A_PERSISTENT_CALL,
    example_system_op_rejects_a_persistent_call => SYSTEM_OP_REJECTS_A_PERSISTENT_CALL,
    example_system_op_rejects_params => SYSTEM_OP_REJECTS_PARAMS,
    example_handle_rejects_a_persistent_call => HANDLE_REJECTS_A_PERSISTENT_CALL,
    example_temp_space_ends_with_its_deploy => TEMP_SPACE_ENDS_WITH_ITS_DEPLOY,
    example_fips_example => FIPS_EXAMPLE,
    example_speculative_execution => SPECULATIVE_EXECUTION,
    example_vectordb_embed => VECTORDB_EMBED,
    example_vectordb_mirror => VECTORDB_MIRROR,
    example_agent_hashmap_bag => AGENT_HASHMAP_BAG,
    example_agent_wrapper_forwards_to_native => AGENT_WRAPPER_FORWARDS_TO_NATIVE,
    example_agent_space_rejects_a_persistent_send => AGENT_SPACE_REJECTS_A_PERSISTENT_SEND,
    example_agent_space_rejects_params => AGENT_SPACE_REJECTS_PARAMS,
    example_agent_space_rejects_a_query => AGENT_SPACE_REJECTS_A_QUERY,
    example_agent_space_rejects_a_spanning_guard => AGENT_SPACE_REJECTS_A_SPANNING_GUARD,
    example_agent_space_rejects_a_deep_pattern => AGENT_SPACE_REJECTS_A_DEEP_PATTERN,
    example_agent_capability_rejects_a_persistent_call => AGENT_CAPABILITY_REJECTS_A_PERSISTENT_CALL,
}

/// The table of this file names each `.rho` file of the example directory exactly once.
#[test]
fn examples_cover_the_directory() {
    let mut in_table = BTreeSet::new();
    for example in EXAMPLES {
        for (file, _) in example.deploys {
            assert!(in_table.insert(*file), "{file} is in two examples");
        }
    }
    let on_disk: BTreeSet<String> = std::fs::read_dir(examples_dir())
        .expect("read the example directory")
        .map(|entry| {
            entry
                .expect("read a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".rho"))
        .collect();
    let in_table: BTreeSet<String> = in_table.into_iter().map(str::to_owned).collect();
    assert_eq!(in_table, on_disk);
}

/// The post-state root of the replay trace of the specification (section 11, Determinism).
const REPLAY_TRACE_ROOT: &str = "36cf8734055ee8a51bccea4ceeb5f29133f6767f04bafb6dc6aedea3abfcd8c9";

/// The replay trace of the specification (section 11, Determinism): the example `queue-fifo`
/// plays natively at the v6 limits, native replay reproduces its play, and its post-state root
/// is the recorded root of the specification. A change of the root changes the trace of the
/// specification too, so the specification must record the new root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn determinism_replay_trace_root_is_pinned() {
    let (_, root) = run(&QUEUE_FIFO).await;
    assert_eq!(
        root, REPLAY_TRACE_ROOT,
        "the root of the replay trace of specification section 11"
    );
}

/// The negative control of leaf 9760: a changed expected value fails the comparison.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[should_panic(expected = "the values on")]
async fn examples_reject_a_changed_expectation() {
    run(&Example {
        name: "the changed expectation",
        deploys: &[deploy!("queue_fifo.rho")],
        services: Services::None,
        expect: Expect::Values(&[("rrs.example.queue", &["\"Z\""])]),
    })
    .await;
}
