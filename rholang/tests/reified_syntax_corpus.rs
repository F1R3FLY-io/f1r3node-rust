// P3b tests of the dual-frontend corpus of the Reified RSpaces syntax (plan "Frontend-neutral
// syntax layer", decisions 18 and 21, P3 audit §4).
//
// Changed by decision 112: the corpus of this tree is v2. The text before:
// // `test-vectors/reified-syntax-corpus-v1.json` pairs each source text with the prost bytes of
// `test-vectors/reified-syntax-corpus-v2.json` pairs each source text with the prost bytes of
// its normalized Par. The tree-sitter normalizers produce these bytes now, and the MeTTaIL
// lowerer must produce the same bytes after P12. The corpus holds only keyword-direction rows:
// a row that uses `use` or `free` as an identifier is a tree-sitter regression, not a
// frontend contract (P3 audit §5, risk 10).
//
// `reified_syntax_corpus_bytes_match` compares the normalizer with the corpus. The model check
// is independent of the normalizer: it decodes the corpus bytes with the P2 decoders and
// compares every construct with a model that this file writes by hand.
//
// Capture: set `RRS_CAPTURE_REIFIED_SYNTAX_OUT` to an absolute directory under `target/`, and
// set `RRS_CAPTURE_REVISION`, `RRS_CAPTURE_PARSER_REVISION` and `RRS_CAPTURE_COMMAND`. The
// bytes test then writes the corpus file there instead of comparing.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use models::rhoapi::expr::ExprInstance;
// Changed by decision 112: the Reified syntax is a set of protobuf fields, so the walker reads
// the fields with the decoders of the fields. The text before:
// ```
// use models::rhoapi::{ETuple, Expr, New, Par, Receive, ReceiveBind};
// use models::rust::reified::builders::{push_process_expr, query_spec, send_params, use_block};
// use models::rust::reified::encoding::{
//     decode_free_value, decode_process_tuple, decode_query_source, marker_of, split_trailing_params,
//     FreeTheory, Marker, NewSpace, QuerySpec, ReifiedConstruct, SendParams, UseBlock,
// };
// ```
use models::rhoapi::{Expr, New, Par, Receive, ReceiveBind};
use models::rust::reified::builders::{push_use_block, query_spec, send_params, use_block};
use models::rust::reified::encoding::{
    check_query_spec, check_send_params, decode_free_value, decode_new_space, decode_use_block,
    FreeTheory, Marker, NewSpace, QuerySpec, ReifiedConstruct, SendParams, UseBlock,
};
use models::rust::reified::params::SendParam;
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::utils::new_freevar_par;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::RngSeed;
use prost::Message;
use reified_strategies::{
    bound_var, contains_marker, expr_par, gbool, gint, gstring, list, new_term, send, sorted, tuple,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::util::prepend_new;
use rholang_parser::ast::{AnnProc, Bind, Hyperparam, Name, Proc, SendType};
use rholang_parser::{RholangParser, SourcePos};
use serde_json::{Map, Value};

// Changed by decision 112: the fields change the expected prost bytes of each Reified form, so
// the corpus takes its v2 file and name, and decision 120 keeps v1 frozen as evidence for the
// masked comparison (ruling 6). The text before:
// ```
// const CORPUS_PATH: &str = "test-vectors/reified-syntax-corpus-v1.json";
// const CORPUS_NAME: &str = "reified_syntax_corpus_v1";
// ```
const CORPUS_PATH: &str = "test-vectors/reified-syntax-corpus-v2.json";
const CORPUS_NAME: &str = "reified_syntax_corpus_v2";
const CORPUS_RULE: &str = "Each entry pairs a source text with the hex of the prost bytes of its normalized Par. Every frontend of the Reified RSpaces syntax must normalize the source to exactly these bytes.";
const CAPTURE_ENV: &str = "RRS_CAPTURE_REIFIED_SYNTAX_OUT";
const PROVENANCE: [(&str, &str); 3] = [
    ("captured_at_revision", "RRS_CAPTURE_REVISION"),
    ("parser_revision", "RRS_CAPTURE_PARSER_REVISION"),
    ("capture_command", "RRS_CAPTURE_COMMAND"),
];
const ENTRY_COUNT: usize = 44;
const PROPERTY_SEED: u64 = 0x0E0D_2026_1005_0302;

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rholang crate has a workspace parent")
        .to_path_buf()
}

fn compile(source: &str) -> Par {
    Compiler::source_to_adt(source)
        .unwrap_or_else(|error| panic!("{source} must normalize, but failed with {error:?}"))
}

fn gdouble(value: f64) -> Par {
    expr_par(ExprInstance::GDouble(value.to_bits()), Vec::new(), false)
}

// Changed by decision 112: a use block is a field of its process. The text before:
// fn process(tuple: ETuple) -> Par { push_process_expr(Par::default(), tuple) }
fn process(block: models::rhoapi::UseBlock) -> Par { push_use_block(Par::default(), block) }

fn new_par(new: New) -> Par { prepend_new(Par::default(), new) }

fn two_sends(first: Par, second: Par) -> Par { sorted(concatenate_pars(first, second)) }

/// The receive `for (@x <- source) { Nil }`: one bind with one free variable.
fn receive_once(source: Par) -> Par {
    let locally_free = source.locally_free.clone();
    Par {
        receives: vec![Receive {
            binds: vec![ReceiveBind {
                patterns: vec![new_freevar_par(0, Vec::new())],
                source: Some(source),
                remainder: None,
                free_count: 1,
                cost_signature: None,
                query_spec: None,
            }],
            body: Some(Par::default()),
            persistent: false,
            peek: false,
            bind_count: 1,
            locally_free: locally_free.clone(),
            connective_used: false,
            condition: None,
        }],
        locally_free,
        ..Par::default()
    }
}

fn named_map(named: Vec<(&str, Par)>) -> BTreeMap<String, Par> {
    named
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn use_model(space: Par, body: Par) -> ReifiedConstruct {
    ReifiedConstruct::Use(UseBlock { space, body })
}

fn new_space_model(annotations: Vec<Option<Par>>, new: New) -> ReifiedConstruct {
    ReifiedConstruct::NewSpace(NewSpace { annotations, new })
}

fn params_model(positional: Vec<Par>, named: Vec<(&str, Par)>) -> ReifiedConstruct {
    ReifiedConstruct::Params(SendParams {
        positional,
        named: named_map(named),
    })
}

// Changed by decision 112: the channel of a query is the source of its bind, so the model holds
// the payload and the named params. The byte-exact test of the corpus covers the channel. The
// text before:
// ```
// fn query_model(channel: Par, payload: Vec<Par>, named: Vec<(&str, Par)>) -> ReifiedConstruct {
//     ReifiedConstruct::Query(QuerySpec {
//         channel,
//         payload,
//         named: named_map(named),
//     })
// }
// ```
fn query_model(_channel: Par, payload: Vec<Par>, named: Vec<(&str, Par)>) -> ReifiedConstruct {
    ReifiedConstruct::Query(QuerySpec {
        payload,
        named: named_map(named),
    })
}

/// The named params `named` as send params of the builders.
fn named_params(named: Vec<(&str, Par)>) -> Vec<SendParam> {
    named
        .into_iter()
        .map(|(key, value)| SendParam::Named(key.to_owned(), value))
        .collect()
}

// Retired by decision 112: the params are a field of the send and the query is a field of the
// bind, so the model rows use the two helpers below. The text before:
// ```
// /// The trailing PARAMS element of a send with the named params `named`.
// fn params_element(named: Vec<(&str, Par)>) -> Par {
//     send_params(named_params(named))
//         .expect("the params of a row are valid")
//         .expect("a row with named params has a PARAMS element")
// }
//
// /// The bind source `channel ~ (payload…; named…)` of a receive query.
// fn query_source(channel: Par, payload: Vec<Par>, named: Vec<(&str, Par)>) -> Par {
//     query_spec(channel, payload, named_params(named)).expect("the query of a row is valid")
// }
// ```

/// The send `chan!(data…; named…)`.
fn send_named(chan: Par, data: Vec<Par>, named: Vec<(&str, Par)>) -> Par {
    let params = send_params(named_params(named))
        .expect("the params of a row are valid")
        .expect("a row with named params has params");
    let mut par = send(chan, data);
    for send in &mut par.sends {
        send.params = Some(params.clone());
    }
    par
}

/// The receive `for (@x <- channel ~ (payload…; named…)) { Nil }`: one bind with one free
/// variable and a query.
fn receive_query(channel: Par, payload: Vec<Par>, named: Vec<(&str, Par)>) -> Par {
    let query = query_spec(payload, named_params(named)).expect("the query of a row is valid");
    let mut par = receive_once(channel);
    for receive in &mut par.receives {
        for bind in &mut receive.binds {
            bind.query_spec = Some(query.clone());
        }
    }
    par
}

fn free_model(name: &str, args: Vec<Par>) -> ReifiedConstruct {
    ReifiedConstruct::Free(FreeTheory {
        name: name.to_owned(),
        args,
    })
}

/// The 44 corpus sources, in corpus order, each with the Reified constructs of its normalized
/// Par. The model is written from the syntax contract, not from the normalizer.
fn corpus_table() -> Vec<(&'static str, Vec<ReifiedConstruct>)> {
    let x_one = || send(gstring("x"), vec![gint(1)]);
    let k_one = || params_model(Vec::new(), vec![("k", gint(1))]);
    let two_binders = || {
        two_sends(
            send(bound_var(1), vec![gint(1)]),
            send(bound_var(0), vec![gint(2)]),
        )
    };
    let query_x_five = || query_model(gstring("x"), vec![gint(5)], Vec::new());
    let query_x_one = || query_model(gstring("x"), vec![gint(1)], Vec::new());
    vec![
        (r#"use @"space" { @"x"!(1) }"#, vec![use_model(
            gstring("space"),
            x_one(),
        )]),
        (r#"use @"space" { Nil }"#, vec![use_model(
            gstring("space"),
            Par::default(),
        )]),
        ("new s in { use s { s!(1) } }", vec![use_model(
            bound_var(0),
            send(bound_var(0), vec![gint(1)]),
        )]),
        (r#"new s, t in { use s { use t { @"x"!(1) } } }"#, vec![
            use_model(bound_var(1), process(use_block(bound_var(0), x_one()))),
            use_model(bound_var(0), x_one()),
        ]),
        (
            r#"new s in { use s { new x in { x!(1) } } | @"y"!(2) }"#,
            vec![use_model(
                bound_var(0),
                new_par(new_term(1, Vec::new(), send(bound_var(0), vec![gint(1)]))),
            )],
        ),
        ("new s in { new x : s in { x!(1) } }", vec![
            new_space_model(
                vec![Some(bound_var(0))],
                new_term(1, Vec::new(), send(bound_var(0), vec![gint(1)])),
            ),
        ]),
        ("new s in { new x : s, y in { x!(1) | y!(2) } }", vec![
            new_space_model(
                vec![Some(bound_var(0)), None],
                new_term(2, Vec::new(), two_binders()),
            ),
        ]),
        (r#"new x : @"space", y in { x!(1) | y!(2) }"#, vec![
            new_space_model(
                vec![Some(gstring("space")), None],
                new_term(2, Vec::new(), two_binders()),
            ),
        ]),
        (
            "new s in { new a, b : s, out(`rho:io:stdout`) in { b!(1) | out!(2) } }",
            vec![new_space_model(
                vec![None, Some(bound_var(0))],
                new_term(2, vec!["rho:io:stdout".to_owned()], two_binders()),
            )],
        ),
        (
            "new s, t in { new x : s, y : t in { x!(1) | y!(2) } }",
            vec![new_space_model(
                vec![Some(bound_var(1)), Some(bound_var(0))],
                new_term(2, Vec::new(), two_binders()),
            )],
        ),
        (r#"@"factory"!(free Nat())"#, vec![free_model(
            "Nat",
            Vec::new(),
        )]),
        (r#"@"factory"!(free Th(1, "two"))"#, vec![free_model(
            "Th",
            vec![gint(1), gstring("two")],
        )]),
        (
            r#"@"factory"!(free Int(), free String(), free Bool(), free Any())"#,
            ["Int", "String", "Bool", "Any"]
                .into_iter()
                .map(|name| free_model(name, Vec::new()))
                .collect(),
        ),
        (r#"@"x"!(1; 2)"#, vec![params_model(
            vec![gint(2)],
            Vec::new(),
        )]),
        (r#"@"x"!(1; priority=2)"#, vec![params_model(
            Vec::new(),
            vec![("priority", gint(2))],
        )]),
        (r#"@"x"!(1; 2, k=3)"#, vec![params_model(
            vec![gint(2)],
            vec![("k", gint(3))],
        )]),
        (r#"@"x"!(1; b=2, a=1)"#, vec![params_model(
            Vec::new(),
            vec![("a", gint(1)), ("b", gint(2))],
        )]),
        (r#"@"x"!(1; a=1, b=2)"#, vec![params_model(
            Vec::new(),
            vec![("a", gint(1)), ("b", gint(2))],
        )]),
        (r#"@"x"!(1;)"#, Vec::new()),
        (r#"@"x"!(1)"#, Vec::new()),
        (r#"@"x"!(; k=1)"#, vec![k_one()]),
        (r#"@"x"!!(1; k=1)"#, vec![k_one()]),
        (r#"@"x"!?(1; k=1)."#, vec![k_one()]),
        (r#"@"x"!m(1; k=1)."#, vec![k_one()]),
        (r#"for (y <- @"x"!?(1; k=1)) { Nil }"#, vec![k_one()]),
        (r#"for (y <- @"x"!m(1; k=1)) { Nil }"#, vec![k_one()]),
        (
            r#"new factory in { for (space <- factory!?("default", free Nat(); capacity=4)) { Nil } }"#,
            vec![
                free_model("Nat", Vec::new()),
                params_model(Vec::new(), vec![("capacity", gint(4))]),
            ],
        ),
        (r#"for (y <- @"x" ~ 5) { Nil }"#, vec![query_x_five()]),
        (r#"for (y <- @"x" ~ (5;)) { Nil }"#, vec![query_x_five()]),
        (r#"for (y <- @"x" ~ (1, 2)) { Nil }"#, vec![query_model(
            gstring("x"),
            vec![tuple(vec![gint(1), gint(2)])],
            Vec::new(),
        )]),
        (r#"for (y <- @"x" ~ (1, 2;)) { Nil }"#, vec![query_model(
            gstring("x"),
            vec![gint(1), gint(2)],
            Vec::new(),
        )]),
        (
            r#"for (@doc <- @"db" ~ ([0.5, 0.25]; metric="cosine", threshold=0.8, k=3)) { Nil }"#,
            vec![query_model(
                gstring("db"),
                vec![list(vec![gdouble(0.5), gdouble(0.25)])],
                vec![
                    ("k", gint(3)),
                    ("metric", gstring("cosine")),
                    ("threshold", gdouble(0.8)),
                ],
            )],
        ),
        (r#"for (@task <- @"tasks" ~ (; priority=0)) { Nil }"#, vec![
            query_model(gstring("tasks"), Vec::new(), vec![("priority", gint(0))]),
        ]),
        (r#"new z in { for (y <- @"x" ~ *z) { y!(1) } }"#, vec![
            query_model(gstring("x"), vec![bound_var(0)], Vec::new()),
        ]),
        (
            r#"for (@a <- @"x" ~ 1 & @b <- @"y" where a == b) { Nil }"#,
            vec![query_x_one()],
        ),
        (
            r#"for (@a <- @"x" ~ 1; @b <- @"y" ~ (; k=2)) { Nil }"#,
            vec![
                query_x_one(),
                query_model(gstring("y"), Vec::new(), vec![("k", gint(2))]),
            ],
        ),
        // The P4 documentation rows: a hashmap:queue space at run time.
        (
            r#"new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { use space { @"jobs"!(1) | for (@x <- @"jobs") { Nil } } } }"#,
            vec![use_model(
                bound_var(0),
                sorted(concatenate_pars(
                    send(gstring("jobs"), vec![gint(1)]),
                    receive_once(gstring("jobs")),
                )),
            )],
        ),
        (
            r#"new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { new box : space in { box!(1) | for (@x <- box) { Nil } } } }"#,
            vec![new_space_model(
                vec![Some(bound_var(0))],
                new_term(
                    1,
                    Vec::new(),
                    sorted(concatenate_pars(
                        send(bound_var(0), vec![gint(1)]),
                        receive_once(bound_var(0)),
                    )),
                ),
            )],
        ),
        (
            r#"new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?("default")) { for (_ <- space!?("produce", "jobs", [1]; persistent=true)) { Nil } } }"#,
            vec![params_model(Vec::new(), vec![("persistent", gbool(true))])],
        ),
        (
            r#"new hmq(`rho:space:hashmap:queue`) in { for (space <- hmq!?(free Nat())) { Nil } }"#,
            vec![free_model("Nat", Vec::new())],
        ),
        // The P6 documentation rows: VectorDB spaces in chapter 21 and the ChromaDB mirror in
        // docs/rholang/chromadb.md.
        (
            r#"new vdb(`rho:space:hashmap:vectordb`) in { for (space <- vdb!?("default"; capacity=8, dimensions=2)) { use space { @"docs"!([1, 0]) | @"docs"!([0, 1]) | for (@v <- @"docs" ~ ([1, 0])) { Nil } } } }"#,
            vec![
                params_model(Vec::new(), vec![
                    ("capacity", gint(8)),
                    ("dimensions", gint(2)),
                ]),
                use_model(
                    bound_var(0),
                    sorted(concatenate_pars(
                        two_sends(
                            send(gstring("docs"), vec![list(vec![gint(1), gint(0)])]),
                            send(gstring("docs"), vec![list(vec![gint(0), gint(1)])]),
                        ),
                        // Changed by decision 112: the query is a field of the bind. The text
                        // before:
                        // ```
                        // receive_once(query_source(
                        //     gstring("docs"),
                        //     vec![list(vec![gint(1), gint(0)])],
                        //     Vec::new(),
                        // )),
                        // ```
                        receive_query(
                            gstring("docs"),
                            vec![list(vec![gint(1), gint(0)])],
                            Vec::new(),
                        ),
                    )),
                ),
                query_model(
                    gstring("docs"),
                    vec![list(vec![gint(1), gint(0)])],
                    Vec::new(),
                ),
            ],
        ),
        (
            r#"new vdb(`rho:space:hashmap:vectordb`) in { for (space <- vdb!?("default"; capacity=8, dimensions=2, metric="euclidean")) { use space { @"docs"!("origin"; embedding=[0, 0]) | @"docs"!("far"; embedding=[3, 4]) | for (@top <- @"docs" ~ ([0, 0]; radius=5, k=2, scores=true)) { Nil } } } }"#,
            vec![
                params_model(Vec::new(), vec![
                    ("capacity", gint(8)),
                    ("dimensions", gint(2)),
                    ("metric", gstring("euclidean")),
                ]),
                use_model(
                    bound_var(0),
                    sorted(concatenate_pars(
                        two_sends(
                            // Changed by decision 112: the params are a field of the send. The
                            // text before:
                            // ```
                            // send(gstring("docs"), vec![
                            //     gstring("origin"),
                            //     params_element(vec![("embedding", list(vec![gint(0), gint(0)]))]),
                            // ]),
                            // send(gstring("docs"), vec![
                            //     gstring("far"),
                            //     params_element(vec![("embedding", list(vec![gint(3), gint(4)]))]),
                            // ]),
                            // ```
                            send_named(gstring("docs"), vec![gstring("origin")], vec![(
                                "embedding",
                                list(vec![gint(0), gint(0)]),
                            )]),
                            send_named(gstring("docs"), vec![gstring("far")], vec![(
                                "embedding",
                                list(vec![gint(3), gint(4)]),
                            )]),
                        ),
                        // Changed by decision 112: the query is a field of the bind. The text
                        // before:
                        // ```
                        // receive_once(query_source(
                        //     gstring("docs"),
                        //     vec![list(vec![gint(0), gint(0)])],
                        //     vec![("radius", gint(5)), ("k", gint(2)), ("scores", gbool(true))],
                        // )),
                        // ```
                        receive_query(gstring("docs"), vec![list(vec![gint(0), gint(0)])], vec![
                            ("radius", gint(5)),
                            ("k", gint(2)),
                            ("scores", gbool(true)),
                        ]),
                    )),
                ),
                params_model(Vec::new(), vec![(
                    "embedding",
                    list(vec![gint(0), gint(0)]),
                )]),
                params_model(Vec::new(), vec![(
                    "embedding",
                    list(vec![gint(3), gint(4)]),
                )]),
                query_model(gstring("docs"), vec![list(vec![gint(0), gint(0)])], vec![
                    ("k", gint(2)),
                    ("radius", gint(5)),
                    ("scores", gbool(true)),
                ]),
            ],
        ),
        (
            r#"new vdb(`rho:space:hashmap:vectordb`) in { for (m <- vdb!?("default"; capacity=4, dimensions=2, mirror="chroma")) { for (mn <- m!?("gensym")) { @"my-space"!(*m) | @"my-name"!(*mn) | mn!([1.0, 0.0]) | mn!([0.0, 1.0]) | mn!([1.0, 1.0]) } } }"#,
            vec![params_model(Vec::new(), vec![
                ("capacity", gint(4)),
                ("dimensions", gint(2)),
                ("mirror", gstring("chroma")),
            ])],
        ),
        (
            // Changed by decision 114: the ann reply is [true, ids, distances], with no watermark.
            // r#"new ann(`rho:vectordb:ann`) in { for (m <<- @"my-space") { for (mn <<- @"my-name") { for (@r <- ann!?(*m, *mn, [1.0, 0.0], {"k": 3})) { match r { [true, ids, _, _] => { for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) { @"result"!(x) } } [false, code, _] => { @"result"!(code) } } } } } }"#,
            r#"new ann(`rho:vectordb:ann`) in { for (m <<- @"my-space") { for (mn <<- @"my-name") { for (@r <- ann!?(*m, *mn, [1.0, 0.0], {"k": 3})) { match r { [true, ids, _] => { for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) { @"result"!(x) } } [false, code, _] => { @"result"!(code) } } } } } }"#,
            vec![query_model(
                bound_var(3),
                vec![list(vec![gdouble(1.0), gdouble(0.0)])],
                vec![("ids", bound_var(0)), ("k", gint(3))],
            )],
        ),
    ]
}

// ---------------------------------------------------------------------------
// The corpus file
// ---------------------------------------------------------------------------

struct CorpusEntry {
    source: String,
    expected_prost_hex: String,
}

fn read_corpus(root: &Path) -> (Value, Vec<CorpusEntry>) {
    let text = fs::read_to_string(root.join(CORPUS_PATH)).unwrap_or_else(|error| {
        panic!("read {CORPUS_PATH}: {error}; capture it with {CAPTURE_ENV}")
    });
    let corpus: Value = serde_json::from_str(&text).expect("parse the Reified syntax corpus");
    let entries = corpus
        .get("entries")
        .and_then(Value::as_array)
        .expect("the corpus has an entries array")
        .iter()
        .map(|entry| {
            let field = |key: &str| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("a corpus entry has no string {key}: {entry}"))
                    .to_owned()
            };
            match entry.as_object().map(Map::len) {
                Some(2) => {}
                other => panic!("a corpus entry has exactly two fields, found {other:?}: {entry}"),
            }
            CorpusEntry {
                source: field("source"),
                expected_prost_hex: field("expected_prost_hex"),
            }
        })
        .collect();
    (corpus, entries)
}

fn capture(root: &Path, out_dir: &Path) {
    let target = root.join("target");
    let canonical_target = target.canonicalize().unwrap_or_else(|_| target.clone());
    let inside_target = out_dir.is_absolute()
        && out_dir
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
        && (out_dir.starts_with(&target) || out_dir.starts_with(&canonical_target));
    assert!(
        inside_target,
        "{CAPTURE_ENV} must be an absolute path under {}, got {}",
        target.display(),
        out_dir.display()
    );
    let entries: Vec<Value> = corpus_table()
        .into_iter()
        .map(|(source, _)| {
            let mut entry = Map::with_capacity(2);
            entry.insert("source".to_owned(), Value::from(source));
            entry.insert(
                "expected_prost_hex".to_owned(),
                Value::from(hex::encode(compile(source).encode_to_vec())),
            );
            Value::Object(entry)
        })
        .collect();
    let mut corpus = Map::with_capacity(PROVENANCE.len() + 3);
    corpus.insert("name".to_owned(), Value::from(CORPUS_NAME));
    for (key, variable) in PROVENANCE {
        let value = std::env::var(variable)
            .unwrap_or_else(|_| panic!("set {variable} when capturing the corpus"));
        corpus.insert(key.to_owned(), Value::from(value));
    }
    corpus.insert("rule".to_owned(), Value::from(CORPUS_RULE));
    corpus.insert("entries".to_owned(), Value::Array(entries));
    fs::create_dir_all(out_dir).expect("create the capture directory");
    let rendered = serde_json::to_string_pretty(&Value::Object(corpus)).expect("render the corpus");
    let file_name = Path::new(CORPUS_PATH)
        .file_name()
        .expect("the corpus path names a file");
    fs::write(out_dir.join(file_name), format!("{rendered}\n")).expect("write the corpus");
}

#[test]
fn reified_syntax_corpus_bytes_match() {
    let root = workspace_root();
    if let Some(out_dir) = std::env::var_os(CAPTURE_ENV) {
        capture(&root, Path::new(&out_dir));
        return;
    }
    let (corpus, entries) = read_corpus(&root);
    assert_eq!(
        corpus.get("name").and_then(Value::as_str),
        Some(CORPUS_NAME)
    );
    for (key, _) in PROVENANCE {
        assert!(
            corpus
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            "the corpus has no {key}"
        );
    }
    let table = corpus_table();
    assert_eq!(table.len(), ENTRY_COUNT);
    let sources: Vec<&str> = entries.iter().map(|entry| entry.source.as_str()).collect();
    let listed: Vec<&str> = table.iter().map(|(source, _)| *source).collect();
    assert_eq!(
        sources, listed,
        "the corpus holds exactly the listed sources, in order"
    );
    let mut failures = Vec::new();
    for entry in &entries {
        let actual = hex::encode(compile(&entry.source).encode_to_vec());
        if actual != entry.expected_prost_hex {
            failures.push(format!(
                "{}: expected {}, actual {actual}",
                entry.source, entry.expected_prost_hex
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus entries changed:\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// The model check: decode the corpus bytes with the P2 decoders
// ---------------------------------------------------------------------------

// Retired by decision 112: each Reified construct is now a field or a value, so the position of
// a construct follows from the type of its field. The walker below reads the fields with the
// decoders of the fields. The text before:
// ```
// /// The position of a `Par` in its parent, for the position rules of decision 18.
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// enum Slot {
//     /// A process: USE and NEWSPACE tuples are recognized here.
//     Process,
//     /// The last data element of a send: a PARAMS tuple is recognized here.
//     TrailingData,
//     /// The source of a receive bind: a QUERY tuple is recognized here.
//     BindSource,
//     /// Any other position: a FREE tuple is a value anywhere.
//     Value,
// }
//
// fn tuple_holder(tuple: &ETuple) -> Par {
//     Par {
//         exprs: vec![Expr {
//             expr_instance: Some(ExprInstance::ETupleBody(tuple.clone())),
//         }],
//         locally_free: tuple.locally_free.clone(),
//         connective_used: tuple.connective_used,
//         ..Par::default()
//     }
// }
//
// /// Decodes the marker tuple `tuple` with the P2 decoder of its marker, checks its position and
// /// returns the construct and the slot of each element after the head.
// fn decode_marker_tuple(
//     marker: Marker,
//     tuple: &ETuple,
//     holder: &Par,
//     slot: Slot,
// ) -> Result<(ReifiedConstruct, Vec<Slot>), String> {
//     let sole = holder.exprs.len() == 1;
//     let (construct, slots) = match (marker, slot) {
//         (Marker::Use, Slot::Process) => (
//             decode_process_tuple(tuple)
//                 .map_err(|error| error.to_string())?
//                 .into(),
//             vec![Slot::Value, Slot::Process],
//         ),
//         (Marker::NewSpace, Slot::Process) => (
//             decode_process_tuple(tuple)
//                 .map_err(|error| error.to_string())?
//                 .into(),
//             vec![Slot::Value, Slot::Process],
//         ),
//         (Marker::Params, Slot::TrailingData) if sole => (
//             ReifiedConstruct::Params(
//                 split_trailing_params(&[tuple_holder(tuple)])
//                     .map_err(|error| error.to_string())?
//                     .ok_or("a PARAMS tuple decodes to no params")?,
//             ),
//             vec![Slot::Value, Slot::Value],
//         ),
//         (Marker::Query, Slot::BindSource) if sole => (
//             ReifiedConstruct::Query(
//                 decode_query_source(&tuple_holder(tuple))
//                     .map_err(|error| error.to_string())?
//                     .ok_or("a QUERY tuple decodes to no query")?,
//             ),
//             vec![Slot::Value, Slot::Value, Slot::Value],
//         ),
//         (Marker::Free, _) => (
//             ReifiedConstruct::Free(
//                 decode_free_value(&tuple_holder(tuple))
//                     .map_err(|error| error.to_string())?
//                     .ok_or("a FREE tuple decodes to no theory")?,
//             ),
//             vec![Slot::Value, Slot::Value],
//         ),
//         (marker, slot) => {
//             return Err(format!("a {marker} tuple in the {slot:?} position"));
//         }
//     };
//     Ok((construct, slots))
// }
//
// fn walk_expr(
//     expr: &Expr,
//     holder: &Par,
//     slot: Slot,
//     out: &mut Vec<ReifiedConstruct>,
// ) -> Result<(), String> {
//     match &expr.expr_instance {
//         Some(ExprInstance::ETupleBody(tuple)) => match tuple.ps.first().and_then(marker_of) {
//             Some(marker) => {
//                 let (construct, slots) = decode_marker_tuple(marker, tuple, holder, slot)?;
//                 out.push(construct);
//                 for (element, element_slot) in tuple.ps.iter().skip(1).zip(slots) {
//                     walk(element, element_slot, out)?;
//                 }
//                 Ok(())
//             }
//             None => walk_all(&tuple.ps, out),
//         },
//         Some(ExprInstance::EListBody(list)) => walk_all(&list.ps, out),
//         Some(ExprInstance::ESetBody(set)) => walk_all(&set.ps, out),
//         Some(ExprInstance::EMapBody(map)) => {
//             for pair in &map.kvs {
//                 for part in [&pair.key, &pair.value].into_iter().flatten() {
//                     walk(part, Slot::Value, out)?;
//                 }
//             }
//             Ok(())
//         }
//         Some(ExprInstance::EMethodBody(method)) => {
//             if let Some(target) = &method.target {
//                 walk(target, Slot::Value, out)?;
//             }
//             walk_all(&method.arguments, out)
//         }
//         Some(ExprInstance::EEqBody(eq)) => {
//             for part in [&eq.p1, &eq.p2].into_iter().flatten() {
//                 walk(part, Slot::Value, out)?;
//             }
//             Ok(())
//         }
//         _ => Ok(()),
//     }
// }
//
// fn walk_all(pars: &[Par], out: &mut Vec<ReifiedConstruct>) -> Result<(), String> {
//     for par in pars {
//         walk(par, Slot::Value, out)?;
//     }
//     Ok(())
// }
//
// /// Collects every Reified construct of `par`, decoded by the P2 decoders, and checks that each
// /// marker tuple is in its position.
// fn walk(par: &Par, slot: Slot, out: &mut Vec<ReifiedConstruct>) -> Result<(), String> {
//     for expr in &par.exprs {
//         walk_expr(expr, par, slot, out)?;
//     }
//     for send in &par.sends {
//         if let Some(chan) = &send.chan {
//             walk(chan, Slot::Value, out)?;
//         }
//         let last = send.data.len().checked_sub(1);
//         for (index, datum) in send.data.iter().enumerate() {
//             let datum_slot = match Some(index) == last {
//                 true => Slot::TrailingData,
//                 false => Slot::Value,
//             };
//             walk(datum, datum_slot, out)?;
//         }
//     }
//     for receive in &par.receives {
//         for bind in &receive.binds {
//             walk_all(&bind.patterns, out)?;
//             if let Some(source) = &bind.source {
//                 walk(source, Slot::BindSource, out)?;
//             }
//         }
//         if let Some(body) = &receive.body {
//             walk(body, Slot::Process, out)?;
//         }
//         if let Some(condition) = &receive.condition {
//             walk(condition, Slot::Value, out)?;
//         }
//     }
//     for new in &par.news {
//         if let Some(body) = &new.p {
//             walk(body, Slot::Process, out)?;
//         }
//     }
//     for matched in &par.matches {
//         if let Some(target) = &matched.target {
//             walk(target, Slot::Value, out)?;
//         }
//         for case in &matched.cases {
//             if let Some(pattern) = &case.pattern {
//                 walk(pattern, Slot::Value, out)?;
//             }
//             if let Some(body) = &case.source {
//                 walk(body, Slot::Process, out)?;
//             }
//         }
//     }
//     Ok(())
// }
// ```

fn walk_all(pars: &[Par], out: &mut Vec<ReifiedConstruct>) -> Result<(), String> {
    for par in pars {
        walk(par, out)?;
    }
    Ok(())
}

fn walk_expr(expr: &Expr, out: &mut Vec<ReifiedConstruct>) -> Result<(), String> {
    match &expr.expr_instance {
        Some(ExprInstance::EFreeBody(free)) => {
            let holder = Par {
                exprs: vec![expr.clone()],
                ..Par::default()
            };
            let decoded = decode_free_value(&holder)
                .map_err(|error| error.to_string())?
                .ok_or("a FREE value decodes to no theory")?;
            walk_all(&free.args, out)?;
            out.push(ReifiedConstruct::Free(decoded));
            Ok(())
        }
        Some(ExprInstance::ETupleBody(tuple)) => walk_all(&tuple.ps, out),
        Some(ExprInstance::EListBody(list)) => walk_all(&list.ps, out),
        Some(ExprInstance::ESetBody(set)) => walk_all(&set.ps, out),
        Some(ExprInstance::EMapBody(map)) => {
            for pair in &map.kvs {
                for part in [&pair.key, &pair.value].into_iter().flatten() {
                    walk(part, out)?;
                }
            }
            Ok(())
        }
        Some(ExprInstance::EMethodBody(method)) => {
            if let Some(target) = &method.target {
                walk(target, out)?;
            }
            walk_all(&method.arguments, out)
        }
        Some(ExprInstance::EEqBody(eq)) => {
            for part in [&eq.p1, &eq.p2].into_iter().flatten() {
                walk(part, out)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Collects every Reified construct of `par`, decoded by the decoders of the fields.
fn walk(par: &Par, out: &mut Vec<ReifiedConstruct>) -> Result<(), String> {
    for expr in &par.exprs {
        walk_expr(expr, out)?;
    }
    for block in &par.use_blocks {
        let decoded = decode_use_block(block).map_err(|error| error.to_string())?;
        walk(&decoded.space, out)?;
        walk(&decoded.body, out)?;
        out.push(ReifiedConstruct::Use(decoded));
    }
    for send in &par.sends {
        if let Some(chan) = &send.chan {
            walk(chan, out)?;
        }
        walk_all(&send.data, out)?;
        if let Some(params) = &send.params {
            check_send_params(params).map_err(|error| error.to_string())?;
            walk_all(&params.positional, out)?;
            for value in params.named.values() {
                walk(value, out)?;
            }
            out.push(ReifiedConstruct::Params(params.clone()));
        }
    }
    for receive in &par.receives {
        for bind in &receive.binds {
            walk_all(&bind.patterns, out)?;
            if let Some(source) = &bind.source {
                walk(source, out)?;
            }
            if let Some(query) = &bind.query_spec {
                check_query_spec(query).map_err(|error| error.to_string())?;
                walk_all(&query.payload, out)?;
                for value in query.named.values() {
                    walk(value, out)?;
                }
                out.push(ReifiedConstruct::Query(query.clone()));
            }
        }
        if let Some(body) = &receive.body {
            walk(body, out)?;
        }
        if let Some(condition) = &receive.condition {
            walk(condition, out)?;
        }
    }
    for new in &par.news {
        match new.space_types.as_slice() {
            [] => {
                if let Some(body) = &new.p {
                    walk(body, out)?;
                }
            }
            _ => {
                let decoded = decode_new_space(new).map_err(|error| error.to_string())?;
                for annotation in decoded.annotations.iter().flatten() {
                    walk(annotation, out)?;
                }
                if let Some(body) = &decoded.new.p {
                    walk(body, out)?;
                }
                out.push(ReifiedConstruct::NewSpace(decoded));
            }
        }
    }
    for matched in &par.matches {
        if let Some(target) = &matched.target {
            walk(target, out)?;
        }
        for case in &matched.cases {
            if let Some(pattern) = &case.pattern {
                walk(pattern, out)?;
            }
            if let Some(body) = &case.source {
                walk(body, out)?;
            }
        }
    }
    Ok(())
}

/// The constructs of `par` in a canonical order, so that two lists compare as multisets.
fn canonical(mut constructs: Vec<ReifiedConstruct>) -> Vec<ReifiedConstruct> {
    constructs.sort_by_key(|construct| format!("{construct:?}"));
    constructs
}

#[test]
fn reified_syntax_corpus_decodes_to_source_model() {
    let (_, entries) = read_corpus(&workspace_root());
    let table = corpus_table();
    assert_eq!(entries.len(), table.len());
    let mut failures = Vec::new();
    for (entry, (source, model)) in entries.iter().zip(table) {
        assert_eq!(entry.source, source);
        let bytes = hex::decode(&entry.expected_prost_hex).expect("the corpus holds hex");
        let par = Par::decode(bytes.as_slice()).expect("the corpus holds prost Par bytes");
        let mut decoded = Vec::new();
        // Changed by decision 112: the walker reads the fields and takes no position. The text
        // before:
        // match walk(&par, Slot::Process, &mut decoded) {
        match walk(&par, &mut decoded) {
            Ok(()) if canonical(decoded.clone()) == canonical(model.clone()) => {}
            Ok(()) => failures.push(format!(
                "{source}: decoded {decoded:?}, but the model is {model:?}"
            )),
            Err(error) => failures.push(format!("{source}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------
// Coverage: every construct of the syntax contract has a corpus row
// ---------------------------------------------------------------------------

/// A Reified construct of the syntax contract, read from the parsed AST of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Construct {
    UseNamedSpace,
    UseQuotedSpace,
    NestedUse,
    AnnotationName,
    AnnotationQuotedName,
    AnnotationBesideUriBinder,
    FreeWithoutArgs,
    FreeWithArgs,
    ParamsOnSend,
    ParamsOnPersistentSend,
    ParamsOnSyncSend,
    ParamsOnMethodSugar,
    ParamsOnSendReceiveSource,
    ParamsOnMethodSource,
    PositionalParam,
    NamedParam,
    ParamsWithoutData,
    EmptyParamsSection,
    QueryBare,
    QueryGroupWithoutParams,
    QueryGroupWithParams,
    QueryParamsOnly,
    QueryPayloadList,
    QueryInJoin,
    QueryWithGuard,
    QueryInReceipts,
}

const CONSTRUCTS: [Construct; 26] = [
    Construct::UseNamedSpace,
    Construct::UseQuotedSpace,
    Construct::NestedUse,
    Construct::AnnotationName,
    Construct::AnnotationQuotedName,
    Construct::AnnotationBesideUriBinder,
    Construct::FreeWithoutArgs,
    Construct::FreeWithArgs,
    Construct::ParamsOnSend,
    Construct::ParamsOnPersistentSend,
    Construct::ParamsOnSyncSend,
    Construct::ParamsOnMethodSugar,
    Construct::ParamsOnSendReceiveSource,
    Construct::ParamsOnMethodSource,
    Construct::PositionalParam,
    Construct::NamedParam,
    Construct::ParamsWithoutData,
    Construct::EmptyParamsSection,
    Construct::QueryBare,
    Construct::QueryGroupWithoutParams,
    Construct::QueryGroupWithParams,
    Construct::QueryParamsOnly,
    Construct::QueryPayloadList,
    Construct::QueryInJoin,
    Construct::QueryWithGuard,
    Construct::QueryInReceipts,
];

/// The character of the single-line `source` at the 1-based position `pos`.
fn char_at(source: &str, pos: SourcePos) -> Option<char> {
    source
        .lines()
        .nth(pos.line.checked_sub(1)?)?
        .chars()
        .nth(pos.col.checked_sub(1)?)
}

/// The first character at or after `pos` that is not whitespace.
fn next_token_char(source: &str, pos: SourcePos) -> Option<char> {
    let line = source.lines().nth(pos.line.checked_sub(1)?)?;
    line.chars()
        .skip(pos.col.checked_sub(1)?)
        .find(|character| !character.is_whitespace())
}

/// True when `inputs` starts with the method-name literal of the `!m` sugar: a string
/// literal whose source text is not quoted.
fn is_method_sugar(source: &str, inputs: &[AnnProc]) -> bool {
    match inputs.first() {
        Some(AnnProc {
            proc: Proc::StringLiteral(_),
            span,
        }) => char_at(source, span.start) != Some('"'),
        _ => false,
    }
}

fn params_constructs(params: &[Hyperparam], found: &mut BTreeSet<Construct>) {
    if params.is_empty() {
        found.insert(Construct::EmptyParamsSection);
    }
    for param in params {
        found.insert(match param {
            Hyperparam::Positional(_) => Construct::PositionalParam,
            Hyperparam::Named { .. } => Construct::NamedParam,
        });
    }
}

fn bind_constructs(
    source: &str,
    bind: &Bind,
    receipt_binds: usize,
    guarded: bool,
    receipts: usize,
    found: &mut BTreeSet<Construct>,
) {
    if let Some(params) = bind.source_params() {
        params_constructs(params, found);
        let inputs = bind.input().unwrap_or_default();
        found.insert(match (params.is_empty(), is_method_sugar(source, inputs)) {
            (false, true) => Construct::ParamsOnMethodSource,
            (false, false) => Construct::ParamsOnSendReceiveSource,
            (true, _) => Construct::EmptyParamsSection,
        });
    }
    if let Some(query) = bind.query_spec() {
        match (&query.params, query.payload.as_slice()) {
            (Some(params), payload) => {
                params_constructs(params, found);
                found.insert(Construct::QueryGroupWithParams);
                if payload.is_empty() {
                    found.insert(Construct::QueryParamsOnly);
                }
            }
            (None, [element]) => {
                found.insert(match next_token_char(source, element.span.end) {
                    Some(';') => Construct::QueryGroupWithoutParams,
                    _ => Construct::QueryBare,
                });
            }
            (None, _) => {}
        }
        if query.payload.len() > 1 {
            found.insert(Construct::QueryPayloadList);
        }
        if receipt_binds > 1 {
            found.insert(Construct::QueryInJoin);
        }
        if guarded {
            found.insert(Construct::QueryWithGuard);
        }
        if receipts > 1 {
            found.insert(Construct::QueryInReceipts);
        }
    }
}

/// The Reified constructs of one corpus row, read from its v3 parse.
fn row_constructs(source: &str) -> BTreeSet<Construct> {
    let parser = RholangParser::new();
    let procs = match parser.parse(source) {
        validated::Validated::Good(procs) => procs,
        validated::Validated::Fail(failure) => panic!("{source} must parse: {failure:?}"),
    };
    let mut found = BTreeSet::new();
    for root in &procs {
        for node in root.iter_preorder_dfs() {
            match node.proc {
                Proc::UseBlock { space, proc } => {
                    found.insert(match space {
                        Name::NameVar(_) => Construct::UseNamedSpace,
                        Name::Quote(_) => Construct::UseQuotedSpace,
                    });
                    if proc
                        .iter_preorder_dfs()
                        .any(|inner| matches!(inner.proc, Proc::UseBlock { .. }))
                    {
                        found.insert(Construct::NestedUse);
                    }
                }
                Proc::New { decls, .. } => {
                    for decl in decls {
                        match &decl.space {
                            Some(Name::NameVar(_)) => found.insert(Construct::AnnotationName),
                            Some(Name::Quote(_)) => found.insert(Construct::AnnotationQuotedName),
                            None => false,
                        };
                    }
                    if decls.iter().any(|decl| decl.space.is_some())
                        && decls.iter().any(|decl| decl.uri.is_some())
                    {
                        found.insert(Construct::AnnotationBesideUriBinder);
                    }
                }
                Proc::FreeTheory { args, .. } => {
                    found.insert(match args.is_empty() {
                        true => Construct::FreeWithoutArgs,
                        false => Construct::FreeWithArgs,
                    });
                }
                Proc::Send {
                    send_type,
                    inputs,
                    params: Some(params),
                    ..
                } => {
                    params_constructs(params, &mut found);
                    if !params.is_empty() {
                        found.insert(match send_type {
                            SendType::Single => Construct::ParamsOnSend,
                            SendType::Multiple => Construct::ParamsOnPersistentSend,
                        });
                        if inputs.is_empty() {
                            found.insert(Construct::ParamsWithoutData);
                        }
                    }
                }
                Proc::SendSync {
                    inputs,
                    params: Some(params),
                    ..
                } => {
                    params_constructs(params, &mut found);
                    if !params.is_empty() {
                        found.insert(match is_method_sugar(source, inputs) {
                            true => Construct::ParamsOnMethodSugar,
                            false => Construct::ParamsOnSyncSend,
                        });
                    }
                }
                Proc::ForComprehension { receipts, .. } => {
                    for receipt in receipts {
                        for bind in receipt.iter() {
                            bind_constructs(
                                source,
                                bind,
                                receipt.len(),
                                receipt.guard.is_some(),
                                receipts.len(),
                                &mut found,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
    found
}

#[test]
fn reified_syntax_corpus_covers_every_construct() {
    let (_, entries) = read_corpus(&workspace_root());
    assert_eq!(entries.len(), ENTRY_COUNT);
    let mut covered = BTreeSet::new();
    for entry in &entries {
        covered.extend(row_constructs(&entry.source));
    }
    let missing: Vec<&Construct> = CONSTRUCTS
        .iter()
        .filter(|construct| !covered.contains(construct))
        .collect();
    assert!(missing.is_empty(), "no corpus row has {missing:?}");
    let markers: BTreeSet<Marker> = corpus_table()
        .iter()
        .flat_map(|(_, model)| model.iter().map(ReifiedConstruct::marker))
        .collect();
    assert_eq!(markers, Marker::ALL.into_iter().collect::<BTreeSet<_>>());
}

// ---------------------------------------------------------------------------
// The invalid rows: each syntax form fails in the documented way
// ---------------------------------------------------------------------------

const INVALID_CORPUS_PATH: &str = "test-vectors/reified-syntax-invalid-v1.json";
const INVALID_CORPUS_NAME: &str = "reified-syntax-invalid-v1";

/// The syntax forms of the section "Grammar forms" of `syntax-contract.md`. Each form has at
/// least one invalid row (P10 design v2, section 2.3, review item A5).
const SYNTAX_FORMS: [&str; 6] = [
    "use block",
    "space annotation",
    "theory reference",
    "params",
    "query",
    "bare float",
];

/// One invalid row: the syntax form, the source text, and a text that the debug form of the
/// compile error contains.
struct InvalidEntry {
    form: String,
    source: String,
    expected_error: String,
}

fn read_invalid_corpus(root: &Path) -> Vec<InvalidEntry> {
    let text = fs::read_to_string(root.join(INVALID_CORPUS_PATH))
        .unwrap_or_else(|error| panic!("read {INVALID_CORPUS_PATH}: {error}"));
    let corpus: Value = serde_json::from_str(&text).expect("parse the invalid syntax corpus");
    assert_eq!(
        corpus.get("name").and_then(Value::as_str),
        Some(INVALID_CORPUS_NAME)
    );
    corpus
        .get("entries")
        .and_then(Value::as_array)
        .expect("the invalid corpus has an entries array")
        .iter()
        .map(|entry| {
            let field = |key: &str| {
                entry
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("an invalid entry has no string {key}: {entry}"))
                    .to_owned()
            };
            match entry.as_object().map(Map::len) {
                Some(3) => {}
                other => {
                    panic!("an invalid entry has exactly three fields, found {other:?}: {entry}")
                }
            }
            InvalidEntry {
                form: field("form"),
                source: field("source"),
                expected_error: field("expected_error"),
            }
        })
        .collect()
}

/// Each invalid row fails to compile with its documented error: the variant or the reason of
/// a parser error, the message of a normalizer error, or `ParserError` for a text that the
/// grammar does not accept. Every syntax form of the contract has a row.
#[test]
fn reified_syntax_invalid_rows_fail() {
    let entries = read_invalid_corpus(&workspace_root());
    let mut failures = Vec::new();
    for entry in &entries {
        match Compiler::source_to_adt(&entry.source) {
            Ok(_) => failures.push(format!("{}: compiled", entry.source)),
            Err(error) => {
                let shown = format!("{error:?}");
                println!("invalid row {}: {}: {shown}", entry.form, entry.source);
                if !shown.contains(&entry.expected_error) {
                    failures.push(format!(
                        "{}: expected {}, actual {shown}",
                        entry.source, entry.expected_error
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} invalid rows failed otherwise:\n{}",
        failures.len(),
        entries.len(),
        failures.join("\n")
    );
    let forms: BTreeSet<&str> = entries.iter().map(|entry| entry.form.as_str()).collect();
    assert_eq!(forms, SYNTAX_FORMS.into_iter().collect::<BTreeSet<_>>());
}

// ---------------------------------------------------------------------------
// Ordinary programs have no marker
// ---------------------------------------------------------------------------

/// The names of the generated programs. `use` and `free` are ordinary names here.
const NAMES: [&str; 4] = ["a", "b", "use", "free"];

fn arb_data() -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        (0i64..100).prop_map(|value| value.to_string()),
        "[a-z]{0,4}".prop_map(|value| format!("\"{value}\"")),
        any::<bool>().prop_map(|value| value.to_string()),
        select(NAMES.to_vec()).prop_map(|name| format!("*{name}")),
        Just("Nil".to_owned()),
    ];
    leaf.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..3).prop_map(|items| format!("[{}]", items.join(", "))),
            vec(inner.clone(), 2..4).prop_map(|items| format!("({})", items.join(", "))),
            vec(("[a-z]{1,3}", inner), 0..3).prop_map(|pairs| {
                let pairs: Vec<String> = pairs
                    .into_iter()
                    .map(|(key, value)| format!("\"{key}\": {value}"))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }),
        ]
    })
    .boxed()
}

fn arb_process() -> BoxedStrategy<String> {
    let data = || vec(arb_data(), 0..3).prop_map(|items| items.join(", "));
    let leaf = prop_oneof![
        Just("Nil".to_owned()),
        (select(NAMES.to_vec()), data()).prop_map(|(name, data)| format!("{name}!({data})")),
        (select(NAMES.to_vec()), data()).prop_map(|(name, data)| format!("{name}!!({data})")),
        (select(NAMES.to_vec()), data()).prop_map(|(name, data)| format!("{name}!?({data}).")),
        (select(NAMES.to_vec()), data()).prop_map(|(name, data)| format!("{name}!get({data}).")),
    ];
    leaf.prop_recursive(3, 16, 2, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(left, right)| format!("{left} | {right}")),
            (select(NAMES.to_vec()), inner.clone())
                .prop_map(|(name, body)| format!("for (_ <- {name}) {{ {body} }}")),
            (select(NAMES.to_vec()), inner.clone())
                .prop_map(|(name, body)| format!("for (@v <= {name}) {{ {body} }}")),
            (select(NAMES.to_vec()), inner.clone())
                .prop_map(|(name, body)| format!("{name}!?(1); {{ {body} }}")),
            inner.prop_map(|body| format!("new c in {{ c!(0) | {body} }}")),
        ]
    })
    .boxed()
}

proptest! {
    #![proptest_config(property_config())]

    /// A program without Reified syntax normalizes to a Par without a marker name, also when
    /// it uses `use` and `free` as ordinary names.
    #[test]
    fn reified_syntax_corpus_ordinary_programs_have_no_markers(body in arb_process()) {
        let source = format!("new a, b, use, free in {{ {body} }}");
        let par = Compiler::source_to_adt(&source);
        prop_assert!(par.is_ok(), "{} must normalize: {:?}", source, par);
        let par = par.expect("the program normalizes");
        prop_assert!(!contains_marker(&par), "{} normalized to a marker", source);
    }
}
