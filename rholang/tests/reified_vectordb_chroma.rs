//! The suite of the recorded VectorDB processes `rho:vectordb:embed` and `rho:vectordb:ann`, and
//! of the `config` reply of a mirrored VectorDB space (P6 step 7a, design v3 4.9; plan decisions
//! 57, 58 and 66 to 69).
//!
//! `embed` and `ann` ask node-local services: an embedder and the ChromaDB mirror of a space. The
//! dispatcher records the whole reply of a call on its produce event, and replay produces the
//! recorded reply without a service call, so two nodes with different services compute the same
//! state. The tests install a mock embedder and a fake mirror (`support/backends.rs`).
//!
//! Every test runs in the latest execution model (plan decision 69): native play at the v6
//! limits, then native replay certification of the play with other services
//! (`support/native.rs`). A certification reproduces the errors, the complete operation journal,
//! the phlo usage, the authority events, the byte observations and the root of play.
//!
//! | Test | What it checks |
//! |---|---|
//! | `vectordb_embed_replies_and_codes` | One text, a list, the quota boundaries and each reply code, that only checked calls reach the model, and native replay of each reply. |
//! | `vectordb_embed_replay_equal` | Native replay with another embedder produces the reply of play and calls no embedder. |
//! | `vectordb_embed_out_of_phlo_rejects_output` | One phlo below the usage of a call, the reply does not appear, the call is marked failed, and two plays agree. |
//! | `vectordb_ann_replies_and_codes` | A numeric query, a text query, every reply code, the collection and the channel digest that the mirror receives, the drop of a malformed id, and native replay. |
//! | `vectordb_ann_replay_equal` | Native replay with another mirror reproduces the advisory answers and a recorded outage, and asks no mirror and no embedder. |
//! | `vectordb_ann_ids_feed_the_exact_query` | Across two deploys, the advisory ids, with a stale id among them, restrict an exact query, and the exact order decides. |
//! | `vectordb_embed_and_ann_reject_params_and_persistent_sends` | A call with params aborts with `ParamsUnsupported`, and a persistent call with `PersistentOpCall`, before any service call. |
//! | `vectordb_config_reports_the_mirror` | The `config` reply of a mirrored space names its backend and its collection, and an unmirrored space has no such entry. |
//!
//! Plan decision 67 needs no test of its own here: the config read of an `ann` call is a
//! journaled native operation, so native replay that skips it fails the completeness check
//! ("native replay has incomplete operations"), and every replay test fails.

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::reified::handle::MIRROR_ENTRY;
use rholang::rust::interpreter::reified::vectordb_services::{
    channel_digest, collection_name, Neighbour, ServiceReplyCode, MAX_TEXTS, MAX_TEXT_BYTES,
};
use rholang::rust::interpreter::spaces::namespace::NativeSpace;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use support::backends::{self, services, EmbedMode, FakeMirror, MockEmbedder};
use support::native::{
    assert_reproduces, certify, native_play, Certification, NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::session::{config_channel, data_channel, error_codes, key, native};

// ─── Values ──────────────────────────────────────────────────────────────────────────────────

fn expr(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Par::default()
    }
}

fn flag(value: bool) -> Par { expr(ExprInstance::GBool(value)) }

fn string(value: &str) -> Par { expr(ExprInstance::GString(value.to_string())) }

fn int(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

fn float32(value: f32) -> Par { expr(ExprInstance::GFloat32(value.to_bits())) }

fn bytes(value: &[u8]) -> Par { expr(ExprInstance::GByteArray(value.to_vec())) }

fn double(value: f64) -> Par { expr(ExprInstance::GDouble(value.to_bits())) }

fn list(values: Vec<Par>) -> Par {
    expr(ExprInstance::EListBody(EList {
        ps: values,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

fn vector32(values: &[f32]) -> Par { list(values.iter().copied().map(float32).collect()) }

fn vector64(values: &[f64]) -> Par { list(values.iter().copied().map(double).collect()) }

/// The reply `[true, values…]`.
fn ok(values: Vec<Par>) -> Par {
    let mut parts = vec![flag(true)];
    parts.extend(values);
    list(parts)
}

/// The reply `[false, code, detail]` of `code`.
fn failed(code: ServiceReplyCode) -> Par {
    list(vec![
        flag(false),
        string(code.code()),
        string(code.detail()),
    ])
}

/// The `ann` reply of `neighbours` and `watermark`.
fn answer(neighbours: &[Neighbour], watermark: i64) -> Par {
    ok(vec![
        list(
            neighbours
                .iter()
                .map(|neighbour| bytes(&neighbour.id))
                .collect(),
        ),
        list(
            neighbours
                .iter()
                .map(|neighbour| float32(neighbour.distance))
                .collect(),
        ),
        int(watermark),
    ])
}

fn neighbour(id: &[u8], distance: f32) -> Neighbour {
    Neighbour {
        id: id.to_vec(),
        distance,
    }
}

// ─── Programs ────────────────────────────────────────────────────────────────────────────────

/// The VectorDB family of the HashMap store.
const VDB_URN: &str = "rho:space:hashmap:vectordb";

/// The constructor of a mirrored VectorDB space of four records of two dimensions.
const MIRRORED: &str = r#""default"; capacity=4, dimensions=2, mirror="chroma""#;

/// The constructor of an unmirrored VectorDB space.
const UNMIRRORED: &str = r#""default"; capacity=4, dimensions=2"#;

/// The native phlo limit of the programs with long texts.
const LARGE_LIMIT: u64 = 100 * AMPLE_LIMIT;

/// The longest time that a native replay certification may take. A certification of these
/// programs takes well under a second, so a hang fails the test instead of blocking the suite.
const CERTIFY_TIMEOUT: Duration = Duration::from_secs(120);

/// The calls `{process}!?(arguments)` of `calls`, each sending `[label, reply]` on [`OUT`].
fn calls(process: &str, calls: &[(&str, String)]) -> String {
    calls
        .iter()
        .map(|(label, arguments)| {
            format!(r#"for (@r <- {process}!?({arguments})) {{ @"{OUT}"!(["{label}", r]) }}"#)
        })
        .collect::<Vec<_>>()
        .join(" |\n  ")
}

/// A deploy of `embed` calls.
fn embed_program(embed_calls: &[(&str, String)]) -> String {
    format!(
        "new embed(`rho:vectordb:embed`) in {{\n  {}\n}}",
        calls("embed", embed_calls)
    )
}

/// A deploy that creates a mirrored VectorDB space `m` with a name `mn`, sends them on [`OUT`]
/// under the labels `m` and `mn`, and then makes the `ann` calls of `ann_calls`.
fn ann_program(ann_calls: &[(&str, String)]) -> String {
    format!(
        r#"new ann(`rho:vectordb:ann`), vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{
    for (mn <- m!?("gensym")) {{
      @"{OUT}"!(["m", *m]) | @"{OUT}"!(["mn", *mn]) |
      {}
    }}
  }}
}}"#,
        calls("ann", ann_calls)
    )
}

/// The pairs `[label, value]` on [`OUT`] of `played`, by label.
fn replies(played: &Played) -> BTreeMap<String, Par> {
    let mut found = BTreeMap::new();
    for pars in &played.out {
        let [pair] = pars.as_slice() else {
            panic!("{OUT:?} holds a datum of {} values", pars.len());
        };
        let Some(ExprInstance::EListBody(EList { ps, .. })) =
            pair.exprs.first().and_then(|e| e.expr_instance.clone())
        else {
            panic!("{OUT:?} holds a value that is not a pair: {pair:?}");
        };
        let [label, value] = <[Par; 2]>::try_from(ps).expect("a pair [label, value]");
        let Some(ExprInstance::GString(label)) =
            label.exprs.first().and_then(|e| e.expr_instance.clone())
        else {
            panic!("a label that is not a string");
        };
        assert!(
            found.insert(label.clone(), value).is_none(),
            "two values for {label}"
        );
    }
    found
}

/// The pairs of `played` without the space and the name of [`ann_program`], which it sends
/// under the labels `m` and `mn`.
fn ann_replies(played: &Played) -> BTreeMap<String, Par> {
    let mut found = replies(played);
    found.remove("m");
    found.remove("mn");
    found
}

/// The space and the name that [`ann_program`] sends on [`OUT`].
fn space_and_name(played: &Played) -> (NativeSpace, Par) {
    let found = replies(played);
    (
        native(found.get("m").expect("the space")),
        found.get("mn").expect("the name").clone(),
    )
}

/// Asserts that `played` succeeds.
fn assert_played(label: &str, played: &Played) {
    assert!(
        played.result.errors.is_empty(),
        "{label}: native play fails: {:?}",
        played.result.errors
    );
}

/// Certifies `played` by native replay of `term` within [`CERTIFY_TIMEOUT`].
async fn certified(label: &str, played: &Played, term: &str, limit: u64) -> Certification {
    tokio::time::timeout(CERTIFY_TIMEOUT, certify(played, term, limit))
        .await
        .unwrap_or_else(|_| panic!("{label}: native replay does not finish"))
}

/// Certifies `played` and asserts that native replay reproduces it.
async fn assert_certifies(label: &str, played: &Played, term: &str, limit: u64) {
    let certification = certified(label, played, term, limit).await;
    assert_reproduces(label, played, &certification);
}

/// The only value of the only datum of `data`.
fn sole(data: Vec<Vec<Par>>) -> Par {
    match <[Vec<Par>; 1]>::try_from(data) {
        Ok([pars]) => match <[Par; 1]>::try_from(pars) {
            Ok([value]) => value,
            Err(pars) => panic!("a datum of {} values", pars.len()),
        },
        Err(data) => panic!("{} data instead of one", data.len()),
    }
}

// ─── embed ───────────────────────────────────────────────────────────────────────────────────

fn quoted(text: &str) -> String { format!("{text:?}") }

fn quoted_list(texts: &[String]) -> String {
    format!(
        "[{}]",
        texts
            .iter()
            .map(|text| quoted(text))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// One text, a list, the boundaries of the quota and each reply code of `embed`. The quota and
/// the argument checks refuse a call before the model runs. Native replay reproduces every reply
/// with an embedder that it never asks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_replies_and_codes() {
    let embedder = MockEmbedder::new("mock-a", 3, 0.5, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let at_count: Vec<String> = (0..MAX_TEXTS).map(|index| format!("t{index}")).collect();
    let over_count: Vec<String> = (0..=MAX_TEXTS).map(|index| format!("t{index}")).collect();
    let at_bytes = "x".repeat(MAX_TEXT_BYTES);
    let over_bytes = "x".repeat(MAX_TEXT_BYTES + 1);
    let term = embed_program(&[
        ("one", quoted("hello")),
        ("list", r#"["a", "bc"]"#.to_string()),
        ("at-count", quoted_list(&at_count)),
        ("at-bytes", quoted(&at_bytes)),
        ("over-count", quoted_list(&over_count)),
        ("over-bytes", quoted(&over_bytes)),
        ("empty-list", "[]".to_string()),
        ("number", "42".to_string()),
        ("mixed", r#"["a", 1]"#.to_string()),
    ]);
    let played = native_play(&term, LARGE_LIMIT).await;
    assert_played("embed codes", &played);
    let model = string("mock-a");
    let at_count_vectors = list(
        at_count
            .iter()
            .map(|text| vector32(&embedder.vector(text)))
            .collect(),
    );
    let expected: BTreeMap<String, Par> = [
        (
            "one",
            ok(vec![vector32(&embedder.vector("hello")), model.clone()]),
        ),
        (
            "list",
            ok(vec![
                list(vec![
                    vector32(&embedder.vector("a")),
                    vector32(&embedder.vector("bc")),
                ]),
                model.clone(),
            ]),
        ),
        ("at-count", ok(vec![at_count_vectors, model.clone()])),
        (
            "at-bytes",
            ok(vec![vector32(&embedder.vector(&at_bytes)), model.clone()]),
        ),
        ("over-count", failed(ServiceReplyCode::EmbeddingRateLimited)),
        ("over-bytes", failed(ServiceReplyCode::EmbeddingRateLimited)),
        ("empty-list", failed(ServiceReplyCode::BadArgument)),
        ("number", failed(ServiceReplyCode::BadArgument)),
        ("mixed", failed(ServiceReplyCode::BadArgument)),
    ]
    .into_iter()
    .map(|(label, reply)| (label.to_string(), reply))
    .collect();
    assert_eq!(replies(&played), expected);
    assert_eq!(
        embedder.calls(),
        4,
        "only the calls within the quota and with texts reach the model"
    );
    let replayed_with = MockEmbedder::new("mock-b", 3, 9.0, EmbedMode::Vectors);
    lock.replace(services(&replayed_with, &FakeMirror::down()));
    assert_certifies("embed codes", &played, &term, LARGE_LIMIT).await;
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");

    // The failures of the model, and a node without services.
    let term = embed_program(&[("call", r#"["a", "bc"]"#.to_string())]);
    for (mode, code) in [
        (
            Some(EmbedMode::NonFinite),
            ServiceReplyCode::NonFiniteEmbedding,
        ),
        (
            Some(EmbedMode::Unavailable),
            ServiceReplyCode::EmbeddingUnavailable,
        ),
        (
            Some(EmbedMode::Short),
            ServiceReplyCode::EmbeddingUnavailable,
        ),
        (None, ServiceReplyCode::EmbeddingUnavailable),
    ] {
        match mode {
            Some(mode) => lock.replace(services(
                &MockEmbedder::new("mock-c", 3, 0.5, mode),
                &FakeMirror::down(),
            )),
            None => lock.remove(),
        }
        let label = format!("embed {mode:?}");
        let played = native_play(&term, AMPLE_LIMIT).await;
        assert_played(&label, &played);
        assert_eq!(replies(&played).get("call"), Some(&failed(code)), "{label}");
        lock.replace(services(&replayed_with, &FakeMirror::down()));
        assert_certifies(&label, &played, &term, AMPLE_LIMIT).await;
    }
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");
}

/// Native replay produces the recorded reply of play with another embedder, which it never
/// calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_replay_equal() {
    let played_with = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let replayed_with = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
    let lock = backends::install(services(&played_with, &FakeMirror::down())).await;
    let term = embed_program(&[
        ("one", quoted("hello")),
        ("list", r#"["a", "bc"]"#.to_string()),
    ]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("embed replay", &played);
    let expected: BTreeMap<String, Par> = [
        (
            "one",
            ok(vec![
                vector32(&played_with.vector("hello")),
                string("mock-a"),
            ]),
        ),
        (
            "list",
            ok(vec![
                list(vec![
                    vector32(&played_with.vector("a")),
                    vector32(&played_with.vector("bc")),
                ]),
                string("mock-a"),
            ]),
        ),
    ]
    .into_iter()
    .map(|(label, reply)| (label.to_string(), reply))
    .collect();
    assert_eq!(replies(&played), expected);
    lock.replace(services(&replayed_with, &FakeMirror::down()));
    assert_certifies("embed replay", &played, &term, AMPLE_LIMIT).await;
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");
}

/// One phlo below the usage of a call, the reply does not appear: the produce of the reply
/// fails, the deploy fails with the output that it did not produce, and the event log marks the
/// call failed. Two plays agree.
///
/// Native replay of this play is the work of the record-and-replay mechanism of external service
/// calls, which the cost-accounting owner implements (plan decision 68, task 10904): replay
/// refuses to dispatch a call whose recorded produce is marked failed (`reduce.rs:704-706`), so
/// the journaled reply attempt of play stays unconsumed ("native replay has incomplete
/// operations"). When the mechanism merges, `embed` and `ann` move to it and this test certifies
/// the rejection by native replay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_out_of_phlo_rejects_output() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let term = format!(r#"new embed(`rho:vectordb:embed`) in {{ embed!("{OUT}", "hello") }}"#);
    let full = native_play(&term, AMPLE_LIMIT).await;
    assert_played("embed full", &full);
    assert_eq!(full.out, vec![vec![ok(vec![
        vector32(&embedder.vector("hello")),
        string("mock-a")
    ])]]);
    let used = full
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget")
        .used;
    let mut runs = Vec::with_capacity(2);
    for _ in 0..2 {
        let played = native_play(&term, used - 1).await;
        match played.result.errors.as_slice() {
            [InterpreterError::ProduceFailureWithOutput {
                cause,
                output_not_produced,
            }] => {
                assert!(
                    matches!(cause.as_ref(), InterpreterError::OutOfPhlogistonsError),
                    "the cause: {cause:?}"
                );
                assert_eq!(output_not_produced.len(), 1, "the reply is the output");
            }
            errors => panic!("unexpected errors: {errors:?}"),
        }
        assert!(played.out.is_empty(), "the reply does not appear");
        let failed_calls: Vec<_> = played
            .log
            .iter()
            .filter_map(|event| match event {
                Event::IoEvent(IOEvent::Produce(produce)) if produce.failed => Some(produce),
                _ => None,
            })
            .collect();
        assert_eq!(failed_calls.len(), 1, "the call is marked failed");
        assert!(failed_calls[0].output_value.is_empty());
        runs.push((
            format!("{:?}", played.result.errors),
            played.root.clone(),
            played.log.clone(),
        ));
    }
    assert_eq!(runs[0], runs[1], "the rejection is deterministic");
    drop(lock);
}

// ─── ann ─────────────────────────────────────────────────────────────────────────────────────

/// A numeric query, a text query and each reply code of `ann`. The mirror receives the
/// collection and the channel digest of the call, and the reply drops an id that is not 32 bytes
/// long. Native replay reproduces every reply with a mirror that it never asks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_ann_replies_and_codes() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let first = [1; 32];
    let second = [2; 32];
    let mirror = FakeMirror::new(
        vec![
            neighbour(&first, 0.25),
            neighbour(&second, 0.5),
            neighbour(&[3; 3], 0.75),
        ],
        7,
    );
    let lock = backends::install(services(&embedder, &mirror)).await;
    // Three deploys: one call of `ann` costs about 16.6 M VerificationBytes in native replay at
    // the v6 limits, so one deploy holds about a dozen calls (verification.md, section 18).
    let queries_term = ann_program(&[
        ("vector", r#"*m, *mn, [1.0, 0.0], {"k": 3}"#.to_string()),
        ("text", r#"*m, *mn, "hello", {"k": 1}"#.to_string()),
        (
            "dimensions",
            r#"*m, *mn, [1.0, 0.0, 0.0], {"k": 1}"#.to_string(),
        ),
        (
            "not-numbers",
            r#"*m, *mn, [1.0, "x"], {"k": 1}"#.to_string(),
        ),
        (
            "beyond-f32",
            r#"*m, *mn, [1e300, 0.0], {"k": 1}"#.to_string(),
        ),
    ]);
    let options_term = ann_program(&[
        ("k-zero", r#"*m, *mn, [1.0, 0.0], {"k": 0}"#.to_string()),
        ("k-over", r#"*m, *mn, [1.0, 0.0], {"k": 5}"#.to_string()),
        (
            "extra-key",
            r#"*m, *mn, [1.0, 0.0], {"k": 1, "x": 2}"#.to_string(),
        ),
        ("not-a-map", r#"*m, *mn, [1.0, 0.0], [1]"#.to_string()),
    ]);
    let spaces_term = format!(
        r#"new ann(`rho:vectordb:ann`), vdb(`{VDB_URN}`), queue(`rho:space:hashmap:queue`) in {{
  for (u <- vdb!?({UNMIRRORED})) {{ for (q <- queue!?("default")) {{
    for (un <- u!?("gensym")) {{
      {}
    }}
  }} }}
}}"#,
        calls("ann", &[
            ("nil", r#"Nil, *un, [1.0, 0.0], {"k": 1}"#.to_string()),
            ("queue", r#"*q, *un, [1.0, 0.0], {"k": 1}"#.to_string()),
            ("unmirrored", r#"*u, *un, [1.0, 0.0], {"k": 1}"#.to_string()),
        ])
    );
    let queries = native_play(&queries_term, AMPLE_LIMIT).await;
    assert_played("ann queries", &queries);
    let (space, name) = space_and_name(&queries);
    let mut asked = mirror.queries();
    let options = native_play(&options_term, AMPLE_LIMIT).await;
    assert_played("ann options", &options);
    let spaces = native_play(&spaces_term, AMPLE_LIMIT).await;
    assert_played("ann spaces", &spaces);
    let mut found = ann_replies(&queries);
    found.extend(ann_replies(&options));
    found.extend(replies(&spaces));
    let expected: BTreeMap<String, Par> = [
        (
            "vector",
            answer(&[neighbour(&first, 0.25), neighbour(&second, 0.5)], 7),
        ),
        ("text", answer(&[neighbour(&first, 0.25)], 7)),
        ("nil", failed(ServiceReplyCode::NotAVectorDbSpace)),
        ("queue", failed(ServiceReplyCode::NotAVectorDbSpace)),
        ("unmirrored", failed(ServiceReplyCode::MirrorDisabled)),
        ("dimensions", failed(ServiceReplyCode::DimensionMismatch)),
        ("not-numbers", failed(ServiceReplyCode::BadArgument)),
        ("beyond-f32", failed(ServiceReplyCode::BadArgument)),
        ("k-zero", failed(ServiceReplyCode::InvalidOption)),
        ("k-over", failed(ServiceReplyCode::InvalidOption)),
        ("extra-key", failed(ServiceReplyCode::InvalidOption)),
        ("not-a-map", failed(ServiceReplyCode::InvalidOption)),
    ]
    .into_iter()
    .map(|(label, reply)| (label.to_string(), reply))
    .collect();
    assert_eq!(found, expected, "the malformed id is dropped");
    assert_eq!(
        mirror.queries().len(),
        asked.len(),
        "a call that fails a check never reaches the mirror"
    );
    asked.sort_by_key(|query| query.k);
    assert_eq!(
        asked.len(),
        2,
        "the checked calls reach the mirror: {asked:?}"
    );
    for query in &asked {
        assert_eq!(query.collection, collection_name(&space.sid));
        assert_eq!(query.channel, channel_digest(&key(&name)));
    }
    assert_eq!(asked[0].vector, embedder.vector("hello"), "the text query");
    assert_eq!(asked[1].vector, vec![1.0, 0.0]);
    assert_eq!(asked[1].k, 3);
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    lock.replace(services(&embedder, &replayed_with));
    for (label, played, term) in [
        ("ann queries", &queries, &queries_term),
        ("ann options", &options, &options_term),
        ("ann spaces", &spaces, &spaces_term),
    ] {
        assert_certifies(label, played, term, AMPLE_LIMIT).await;
    }
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );

    // A text whose embedding has other dimensions than the space, and a mirror outage.
    lock.replace(services(
        &MockEmbedder::new("mock-c", 3, 0.5, EmbedMode::Vectors),
        &FakeMirror::down(),
    ));
    let term = ann_program(&[
        (
            "text-dimensions",
            r#"*m, *mn, "hello", {"k": 1}"#.to_string(),
        ),
        ("outage", r#"*m, *mn, [1.0, 0.0], {"k": 1}"#.to_string()),
    ]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("ann services", &played);
    let found = ann_replies(&played);
    assert_eq!(
        found.get("text-dimensions"),
        Some(&failed(ServiceReplyCode::DimensionMismatch))
    );
    assert_eq!(
        found.get("outage"),
        Some(&failed(ServiceReplyCode::MirrorUnavailable))
    );
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("ann services", &played, &term, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
}

/// Native replay reproduces the advisory answers and a recorded outage with another mirror and
/// another embedder, which it never asks.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_ann_replay_equal() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let played_with = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)], 7);
    let lock = backends::install(services(&embedder, &played_with)).await;
    let term = ann_program(&[
        ("vector", r#"*m, *mn, [1.0, 0.0], {"k": 1}"#.to_string()),
        ("text", r#"*m, *mn, "hello", {"k": 1}"#.to_string()),
    ]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("ann replay", &played);
    let expected: BTreeMap<String, Par> = [
        ("vector", answer(&[neighbour(&[1; 32], 0.25)], 7)),
        ("text", answer(&[neighbour(&[1; 32], 0.25)], 7)),
    ]
    .into_iter()
    .map(|(label, reply)| (label.to_string(), reply))
    .collect();
    assert_eq!(ann_replies(&played), expected);
    let replayed_embedder = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    lock.replace(services(&replayed_embedder, &replayed_with));
    assert_certifies("ann replay", &played, &term, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
    assert_eq!(
        replayed_embedder.calls(),
        0,
        "native replay asks no embedder"
    );

    // A recorded outage replays as an outage with a mirror that answers.
    lock.replace(services(&embedder, &FakeMirror::down()));
    let term = ann_program(&[("outage", r#"*m, *mn, [1.0, 0.0], {"k": 1}"#.to_string())]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("ann outage", &played);
    assert_eq!(
        ann_replies(&played).get("outage"),
        Some(&failed(ServiceReplyCode::MirrorUnavailable))
    );
    lock.replace(services(&replayed_embedder, &replayed_with));
    assert_certifies("ann outage", &played, &term, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
}

/// The advisory ids of `ann` restrict an exact query with `ids=`. The first deploy stores the
/// records, and the second asks the mirror and queries. A stale id among the ids selects
/// nothing, a record outside them is never delivered, and the exact order decides, not the
/// order of the mirror. Native replay certifies the second deploy from the root of the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_ann_ids_feed_the_exact_query() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let lock = backends::without_services().await;
    let mut chain = NativeChain::new().await;
    let setup = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{
    for (mn <- m!?("gensym")) {{
      @"rrs-ids-handle"!(*m) | @"rrs-ids-name"!(*mn) |
      mn!([1.0, 0.0]) | mn!([0.0, 1.0]) | mn!([1.0, 1.0])
    }}
  }}
}}"#
    );
    let played = chain.play(&setup, "rrs-ids-setup", AMPLE_LIMIT).await;
    assert_played("ids setup", &played);
    let handle = sole(chain.values("rrs-ids-handle").await);
    let name = sole(chain.values("rrs-ids-name").await);
    let space = native(&handle);
    let ids: BTreeMap<String, Vec<u8>> = chain
        .data(&data_channel(space, &name))
        .await
        .into_iter()
        .map(|datum| {
            let decoded = record::unwrap_vdb_datum(&datum.a).expect("a VectorDB record");
            (
                format!("{:?}", decoded.datum.pars),
                datum.source.hash.bytes(),
            )
        })
        .collect();
    assert_eq!(ids.len(), 3, "the three records are stored");
    let id_of = |values: &[f64]| {
        ids.get(&format!("{:?}", vec![vector64(values)]))
            .expect("the record of the vector")
            .clone()
    };
    let stale = [0xAB; 32];
    lock.replace(services(
        &embedder,
        &FakeMirror::new(
            vec![
                neighbour(&id_of(&[1.0, 1.0]), 0.25),
                neighbour(&stale, 0.375),
                neighbour(&id_of(&[1.0, 0.0]), 0.5),
            ],
            3,
        ),
    ));
    let query = format!(
        r#"new ann(`rho:vectordb:ann`) in {{
  for (m <<- @"rrs-ids-handle") {{ for (mn <<- @"rrs-ids-name") {{
    for (@r <- ann!?(*m, *mn, [1.0, 0.0], {{"k": 3}})) {{
      match r {{
        [true, ids, _, _] => {{ for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) {{ @"{OUT}"!(x) }} }}
      }}
    }}
  }} }}
}}"#
    );
    let played = chain.play(&query, "rrs-ids-query", AMPLE_LIMIT).await;
    assert_played("ids query", &played);
    assert_eq!(
        played.out,
        vec![vec![list(vec![
            vector64(&[1.0, 0.0]),
            vector64(&[1.0, 1.0])
        ])]],
        "the live listed records, in the exact order"
    );
    let left: Vec<Vec<Par>> = chain
        .data(&data_channel(space, &name))
        .await
        .into_iter()
        .map(|datum| {
            record::unwrap_vdb_datum(&datum.a)
                .expect("a VectorDB record")
                .datum
                .pars
        })
        .collect();
    assert_eq!(
        left,
        vec![vec![vector64(&[0.0, 1.0])]],
        "the record outside the ids stays"
    );
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("ids query", &played, &query, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
}

/// A call with params aborts with `ParamsUnsupported` (I-8), and a persistent call with
/// `PersistentOpCall` (A21), before any service call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_and_ann_reject_params_and_persistent_sends() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)], 7);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    for (label, source, code) in [
        (
            "embed-params",
            r#"new embed(`rho:vectordb:embed`), r in { embed!(*r, "hello"; k=1) }"#,
            "ParamsUnsupported",
        ),
        (
            "embed-persistent",
            r#"new embed(`rho:vectordb:embed`), r in { embed!!(*r, "hello") }"#,
            "PersistentOpCall",
        ),
        (
            "ann-params",
            r#"new ann(`rho:vectordb:ann`), r in { ann!(*r, Nil, Nil, [1.0, 0.0], {"k": 1}; k=1) }"#,
            "ParamsUnsupported",
        ),
        (
            "ann-persistent",
            r#"new ann(`rho:vectordb:ann`), r in { ann!!(*r, Nil, Nil, [1.0, 0.0], {"k": 1}) }"#,
            "PersistentOpCall",
        ),
    ] {
        let played = native_play(source, AMPLE_LIMIT).await;
        assert_eq!(
            error_codes(&played.result.errors),
            vec![code.to_string()],
            "{label}"
        );
    }
    assert_eq!(embedder.calls(), 0, "no embedder call");
    assert!(mirror.queries().is_empty(), "no mirror call");
}

// ─── config ──────────────────────────────────────────────────────────────────────────────────

/// The `config` reply of a mirrored space holds the stored config map and the entry `mirror`,
/// which names the backend and the collection. An unmirrored space replies its stored map.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_config_reports_the_mirror() {
    let mut chain = NativeChain::new().await;
    let term = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (u <- vdb!?({UNMIRRORED})) {{
    @"rrs-config-handles"!(*m, *u) |
    for (@c <- m!?("config")) {{ @"{OUT}"!(["m", c]) }} |
    for (@c <- u!?("config")) {{ @"{OUT}"!(["u", c]) }}
  }} }}
}}"#
    );
    let played = chain.play(&term, "rrs-config", AMPLE_LIMIT).await;
    assert_played("config", &played);
    let handles = chain.values("rrs-config-handles").await;
    let [handles] = handles.as_slice() else {
        panic!("one datum of handles");
    };
    let [m, u] = handles.as_slice() else {
        panic!("two handles");
    };
    let found = replies(&played);
    for (tag, handle, mirrored) in [("m", m, true), ("u", u, false)] {
        let space = native(handle);
        let reply = found.get(tag).expect("a config reply");
        let stored = chain.data(&config_channel(space)).await;
        let [stored] = stored.as_slice() else {
            panic!("{tag}: one config record");
        };
        let stored = record::config_map(&stored.a)
            .expect("the config record decodes")
            .clone();
        let config = SpaceConfig::from_par(&stored).expect("the stored config");
        assert_eq!(config.params.contains_key("mirror"), mirrored, "{tag}");
        let entries = map_entries(reply);
        let mirror = entries.get(MIRROR_ENTRY).cloned();
        let mut without_mirror = entries.clone();
        without_mirror.remove(MIRROR_ENTRY);
        assert_eq!(
            without_mirror,
            map_entries(&stored),
            "{tag}: the reply keeps the stored entries"
        );
        match mirrored {
            true => {
                let expected: BTreeMap<String, Par> = [
                    ("backend".to_string(), string("chroma")),
                    (
                        "collection".to_string(),
                        string(&format!("rrs-{}", hex::encode(&space.sid[..16]))),
                    ),
                ]
                .into_iter()
                .collect();
                assert_eq!(
                    map_entries(&mirror.expect("the mirror entry")),
                    expected,
                    "{tag}: the mirror entry"
                );
            }
            false => assert_eq!(mirror, None, "{tag}: no mirror entry"),
        }
    }
    assert_certifies("config", &played, &term, AMPLE_LIMIT).await;
}

/// The entries of a map with string keys.
fn map_entries(par: &Par) -> BTreeMap<String, Par> {
    let Some(ExprInstance::EMapBody(map)) = par.exprs.first().and_then(|e| e.expr_instance.clone())
    else {
        panic!("not a map: {par:?}");
    };
    map.kvs
        .into_iter()
        .map(|pair| {
            let key = pair.key.expect("a key");
            let Some(ExprInstance::GString(key)) =
                key.exprs.first().and_then(|e| e.expr_instance.clone())
            else {
                panic!("a key that is not a string");
            };
            (key, pair.value.expect("a value"))
        })
        .collect()
}
