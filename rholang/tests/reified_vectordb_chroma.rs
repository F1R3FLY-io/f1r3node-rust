//! The suite of the recorded VectorDB processes `rho:vectordb:embed` and `rho:vectordb:ann`, of
//! the `config` reply of a mirrored VectorDB space (P6 step 7a), of the mirror journal and its
//! publication (P6 step 7b), and of the mirror reconciler (P6 step 7c). Design v3 4.9, plan
//! decisions 57, 58 and 66 to 70.
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
//! | `mirror_publishes_a_successful_deploy` | A successful deploy publishes one upsert for each record with a vector, with its id, embedding, channel digest, persistence and document. A record without a vector and an unmirrored space are not mirrored. |
//! | `mirror_journal_drops_failed_deploy` | A space error, an out-of-phlo deploy and a host-work rejection publish nothing, also after a transaction of the space has committed. |
//! | `mirror_rule_e_coalesces` | A peek writes nothing (Rule E), a consume of a non-persistent record deletes it, and a persistent record stays. |
//! | `mirror_temp_never_mirrored` | The factory refuses `mirror` with `temp` and with `seq` (`InvalidParam`, gate 9), so nothing reaches the mirror. |
//! | `mirror_never_blocks_deploy` | With a stalled mirror and a full queue, or a mirror that refuses every write, deploys succeed at once and the collection is dirty. |
//! | `mirror_journal_keeps_the_last_write_of_a_deploy` | A record that one transaction stores and a later transaction of the same deploy consumes leaves no record in the mirror. |
//! | `mirror_fired_send_is_never_mirrored` | A send that fires a parked waiter never enters the collection, so nothing reaches the mirror. |
//! | `mirror_collection_distance_follows_the_metric` | The collections of spaces with the metrics `cosine`, `dot` and `euclidean` have the distances `cosine`, `ip` and `l2`. |
//! | `mirror_exploratory_evaluation_publishes_nothing` | An evaluation without a deploy result (`RhoRuntime::inj`) publishes nothing, and its journal does not reach the next deploy. |
//! | `mirror_reconcile_removes_orphans` | A pass deletes an orphan, adds a missing record and sets the watermark. A pass at the watermark leaves the collection, and a failed pass keeps the watermark. |
//! | `mirror_restart_rescans_reports` | After a restart, a pass repairs every channel that the metadata registry names. |
//! | `mirror_reconcile_repairs_a_dirty_collection` | A failed batch leaves its collection dirty, and the next pass repairs it at its watermark. |
//! | `mirror_reconciler_follows_the_finalized_blocks` | The reconciler maps the `BlockFinalised` events to post-state roots, and its passes reach the newest block. |
//! | `mirror_replay_publishes_its_own_journal` | Native replay certification publishes the journal of each replayed deploy to the mirror of the replaying node. |
//! | `prop_mirror_tracks_the_published_deploys` | After each step of a generated chain of stores, takes, peeks, failing deploys and exploratory evaluations, the mirror holds the records of the state that a published deploy stored (ChromaMirror.tla, PublishOnlyOnSuccess and NetWritesExact). |
//! | `prop_mirror_reconcile_repairs_the_finalized_view` | After a generated chain and a mirror with missing records and orphans, a pass that fails keeps the mirror and the watermark, and a pass that finishes holds the finalized view (ReconciledEqualsFinalizedView). |
//! | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` | Generated `embed` and `ann` calls replay with other services: replay reproduces play with a complete journal, so it makes each check of play, and it asks no service (ConsensusIndependentOfMirror, AdvisoryRecorded, EqualHostWork). |
//!
//! Plan decision 67 needs no test of its own here: the config read of an `ann` call is a
//! journaled native operation, so native replay that skips it fails the completeness check
//! ("native replay has incomplete operations"), and every replay test fails.

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits;
use models::rust::host_work::{HostWorkDimension, HostWorkLimit};
use proptest::prelude::*;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::reified::handle::MIRROR_ENTRY;
use rholang::rust::interpreter::reified::mirror_reconciler::{self, reconcile, Finalized};
use rholang::rust::interpreter::reified::vectordb_services::{
    channel_digest, collection_name, current as current_services, Neighbour, ServiceReplyCode,
    MAX_TEXTS, MAX_TEXT_BYTES,
};
use rholang::rust::interpreter::spaces::mirror::{MirrorMetric, MirrorRecord};
use rholang::rust::interpreter::spaces::namespace::NativeSpace;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use shared::rust::shared::f1r3fly_event::F1r3flyEvent;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use support::backends::{
    self, services, EmbedMode, FakeMirror, MirrorCall, MockEmbedder, WriteMode,
};
use support::native::{
    assert_reproduces, certify, certify_with, host_work, native_play, native_play_result,
    native_play_with, Certification, NativeChain, Played, AMPLE_LIMIT, OUT,
};
use support::props::{check, runtime_config, tokio_runtime};
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
///
/// The calls within the quota and the refused calls run as two deploys. As one deploy, its
/// certification used 260,960,438 `VerificationBytes` of the v6 limit 2^28 (measured in P7
/// step 3). Each installed system process adds about 1.28 * 10^6 units to the build of the native
/// replay environment, so the six HashSet factories of P7 took the one deploy over the limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_replies_and_codes() {
    let embedder = MockEmbedder::new("mock-a", 3, 0.5, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let at_count: Vec<String> = (0..MAX_TEXTS).map(|index| format!("t{index}")).collect();
    let over_count: Vec<String> = (0..=MAX_TEXTS).map(|index| format!("t{index}")).collect();
    let at_bytes = "x".repeat(MAX_TEXT_BYTES);
    let over_bytes = "x".repeat(MAX_TEXT_BYTES + 1);
    // Changed by P7 step 3: two deploys, see the comment of the test.
    // let term = embed_program(&[
    //     ("one", quoted("hello")),
    //     ("list", r#"["a", "bc"]"#.to_string()),
    //     ("at-count", quoted_list(&at_count)),
    //     ("at-bytes", quoted(&at_bytes)),
    //     ("over-count", quoted_list(&over_count)),
    //     ("over-bytes", quoted(&over_bytes)),
    //     ("empty-list", "[]".to_string()),
    //     ("number", "42".to_string()),
    //     ("mixed", r#"["a", 1]"#.to_string()),
    // ]);
    // let played = native_play(&term, LARGE_LIMIT).await;
    // assert_played("embed codes", &played);
    let within = embed_program(&[
        ("one", quoted("hello")),
        ("list", r#"["a", "bc"]"#.to_string()),
        ("at-count", quoted_list(&at_count)),
        ("at-bytes", quoted(&at_bytes)),
    ]);
    let refused = embed_program(&[
        ("over-count", quoted_list(&over_count)),
        ("over-bytes", quoted(&over_bytes)),
        ("empty-list", "[]".to_string()),
        ("number", "42".to_string()),
        ("mixed", r#"["a", 1]"#.to_string()),
    ]);
    let mut played = Vec::with_capacity(2);
    for (label, term) in [("embed calls", within), ("embed codes", refused)] {
        let deploy = native_play(&term, LARGE_LIMIT).await;
        assert_played(label, &deploy);
        played.push((label, term, deploy));
    }
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
    // assert_eq!(replies(&played), expected);
    let mut replied = BTreeMap::new();
    for (_, _, deploy) in &played {
        replied.extend(replies(deploy));
    }
    assert_eq!(replied, expected);
    assert_eq!(
        embedder.calls(),
        4,
        "only the calls within the quota and with texts reach the model"
    );
    let replayed_with = MockEmbedder::new("mock-b", 3, 9.0, EmbedMode::Vectors);
    lock.replace(services(&replayed_with, &FakeMirror::down()));
    // assert_certifies("embed codes", &played, &term, LARGE_LIMIT).await;
    for (label, term, deploy) in &played {
        assert_certifies(label, deploy, term, LARGE_LIMIT).await;
    }
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

// ─── mirror (step 7b) ────────────────────────────────────────────────────────────────────────

/// The constructor of a mirrored VectorDB space whose collection holds two records.
const MIRRORED_TWO: &str = r#""default"; capacity=2, dimensions=2, mirror="chroma""#;

/// The expected mirror record of a stored record: its embedding, its document and its
/// persistence, by the Debug form of its payload.
type Expected = BTreeMap<String, (Vec<f32>, Option<String>, bool)>;

/// The mirror records that the data of the collection `name` of `space` give with `expected`,
/// by record id. A stored record whose payload is not in `expected` has no embedding.
async fn mirror_records(
    chain: &NativeChain,
    space: NativeSpace,
    name: &Par,
    expected: &Expected,
) -> BTreeMap<[u8; 32], MirrorRecord> {
    let channel = channel_digest(&key(name));
    let mut records = BTreeMap::new();
    for datum in chain.data(&data_channel(space, name)).await {
        let decoded = record::unwrap_vdb_datum(&datum.a).expect("a VectorDB record");
        let Some((embedding, document, persistent)) =
            expected.get(&format!("{:?}", decoded.datum.pars))
        else {
            assert_eq!(
                decoded.embedding, None,
                "a record outside `expected` has no vector"
            );
            continue;
        };
        let id: [u8; 32] = datum
            .source
            .hash
            .bytes()
            .try_into()
            .expect("a 32-byte record id");
        assert_eq!(datum.persist, *persistent, "the persistence of the record");
        records.insert(id, MirrorRecord {
            id,
            embedding: embedding.clone(),
            channel: channel.clone(),
            persistent: *persistent,
            document: document.clone(),
        });
    }
    records
}

/// The handles and the names that a setup deploy sends on `channel`.
async fn handles(chain: &NativeChain, channel: &str) -> Vec<Par> {
    let mut values = chain.values(channel).await;
    assert_eq!(values.len(), 1, "one datum on {channel}");
    values.remove(0)
}

/// A successful deploy publishes the net writes of its mirrored space: one upsert for each
/// record with a vector, with its id, embedding, channel digest, persistence and document. A
/// record without a vector and the records of an unmirrored space are not mirrored. Native
/// replay certifies the play.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_publishes_a_successful_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let term = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (u <- vdb!?({UNMIRRORED})) {{
    for (mn <- m!?("gensym")) {{ for (un <- u!?("gensym")) {{
      @"rrs-mirror-handles"!(*m, *u, *mn, *un) |
      mn!([1.0, 0.0]) | mn!("doc"; embedding=[0.6, 0.8]) | mn!!([0.0, 1.0]) | mn!("plain") |
      un!([1.0, 0.0])
    }} }}
  }} }}
}}"#
    );
    let played = chain.play(&term, "rrs-mirror-publish", AMPLE_LIMIT).await;
    assert_played("mirror publish", &played);
    backends::writer().flush().await;
    let [m, u, mn, un] = <[Par; 4]>::try_from(handles(&chain, "rrs-mirror-handles").await)
        .expect("two handles and two names");
    let (mirrored, unmirrored) = (native(&m), native(&u));
    let expected: Expected = [
        (vec![vector64(&[1.0, 0.0])], (vec![1.0, 0.0], None, false)),
        (
            vec![string("doc")],
            (
                vec![0.6f64 as f32, 0.8f64 as f32],
                Some("doc".to_string()),
                false,
            ),
        ),
        (vec![vector64(&[0.0, 1.0])], (vec![0.0, 1.0], None, true)),
    ]
    .into_iter()
    .map(|(pars, record)| (format!("{pars:?}"), record))
    .collect();
    let records = mirror_records(&chain, mirrored, &mn, &expected).await;
    assert_eq!(records.len(), 3, "three records have a vector");
    let collection = collection_name(&mirrored.sid);
    assert_eq!(mirror.calls(), vec![
        MirrorCall::Ensure {
            collection: collection.clone(),
            metric: MirrorMetric::Cosine,
        },
        MirrorCall::Upsert {
            collection: collection.clone(),
            records: records.values().cloned().collect(),
        },
    ]);
    let held = mirror
        .collection(&collection)
        .expect("the mirror collection");
    assert_eq!(held.metric, MirrorMetric::Cosine);
    assert_eq!(held.records, records);
    assert_eq!(
        mirror.collection(&collection_name(&unmirrored.sid)),
        None,
        "an unmirrored space has no collection"
    );
    assert_eq!(
        chain.data(&data_channel(unmirrored, &un)).await.len(),
        1,
        "the unmirrored space stores its record"
    );
    assert_certifies("mirror publish", &played, &term, AMPLE_LIMIT).await;
}

/// A program that sends its mirrored space on [`OUT`] under the label `m`, stores a record,
/// and then runs `tail` in the body of a peek of the record. The peek fires only after the store
/// commits, so the journal holds the upsert of the record when `tail` runs.
fn store_then(tail: &str) -> String {
    format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"{OUT}"!(["m", *m]) | mn!([1.0, 0.0]) |
    for (@x <<- mn) {{ {tail} }}
  }} }}
}}"#
    )
}

/// The tail of [`store_then`] that sends the peeked record on [`OUT`] under the label `x`.
fn send_peeked() -> String { format!(r#"@"{OUT}"!(["x", x])"#) }

/// The mirror collection of the space of a [`store_then`] play.
fn store_then_collection(played: &Played) -> String {
    collection_name(&native(replies(played).get("m").expect("the space")).sid)
}

/// A deploy that fails drops its journal, also after a transaction of a mirrored space has
/// committed: a space error, an out-of-phlo deploy and a host-work rejection publish nothing.
/// The same program with an ample budget publishes its upsert. Each budget is one unit below
/// the usage of the control, so each failure comes after the store has committed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_journal_drops_failed_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let control_mirror = FakeMirror::new(Vec::new(), 0);
    let lock = backends::install(services(&embedder, &control_mirror)).await;
    let term = store_then(&send_peeked());
    let host = host_work();
    let control = native_play_with(&term, AMPLE_LIMIT, host.clone()).await;
    assert_played("mirror control", &control);
    backends::writer().flush().await;
    let collection = store_then_collection(&control);
    assert!(
        matches!(
            control_mirror.calls().as_slice(),
            [MirrorCall::Ensure { .. }, MirrorCall::Upsert { collection: upserted, records }]
                if *upserted == collection && records.len() == 1
        ),
        "the control publishes its upsert: {:?}",
        control_mirror.calls()
    );
    let used = control
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget")
        .used;
    let verification = host.usage(HostWorkDimension::VerificationBytes).get();

    let mirror = FakeMirror::new(Vec::new(), 0);
    lock.replace(services(&embedder, &mirror));
    let error = native_play(&store_then("mn!([1.0, 2.0, 3.0])"), AMPLE_LIMIT).await;
    assert_eq!(error_codes(&error.result.errors), vec![
        "DimensionMismatch".to_string()
    ]);
    let out_of_phlo = native_play(&term, used - 1).await;
    assert!(
        out_of_phlo
            .result
            .errors
            .iter()
            .any(|error| matches!(error, InterpreterError::OutOfPhlogistonsError)),
        "out of phlo: {:?}",
        out_of_phlo.result.errors
    );
    let mut limits = offered_funded_v6_host_work_limits();
    limits.set(
        HostWorkDimension::VerificationBytes,
        HostWorkLimit::new(verification - 1),
    );
    // The last reservation of `VerificationBytes` comes after the last operation, so every
    // transaction has committed and the journal is complete when the check rejects the deploy.
    let rejected = native_play_result(&term, AMPLE_LIMIT, HostWorkBudget::new(limits)).await;
    assert!(
        matches!(rejected, Err(InterpreterError::HostWorkRejected)),
        "the host-work check rejects the deploy: {rejected:?}"
    );
    backends::writer().flush().await;
    assert_eq!(
        mirror.calls(),
        Vec::new(),
        "a failed deploy publishes nothing"
    );
}

/// Rule E coalesces in the journal: a peek removes its record and stores it again, so it
/// writes nothing. A consume of a non-persistent record deletes it, and a persistent record
/// that a receive matches stays. Native replay certifies both deploys.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_rule_e_coalesces() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let setup = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{
    for (n1 <- m!?("gensym")) {{ for (n2 <- m!?("gensym")) {{ for (n3 <- m!?("gensym")) {{
      @"rrs-rule-e-names"!(*m, *n1, *n2, *n3) |
      n1!([1.0, 0.0]) | n2!([0.0, 1.0]) | n3!!([0.6, 0.8])
    }} }} }}
  }}
}}"#
    );
    let first = chain.play(&setup, "rrs-rule-e-setup", AMPLE_LIMIT).await;
    assert_played("rule E setup", &first);
    backends::writer().flush().await;
    let [m, n1, n2, n3] = <[Par; 4]>::try_from(handles(&chain, "rrs-rule-e-names").await)
        .expect("a handle and three names");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let setup_calls = mirror.calls();
    let before = mirror
        .collection(&collection)
        .expect("the mirror collection");
    assert_eq!(before.records.len(), 3, "the setup mirrors three records");
    let removed = mirror_records(
        &chain,
        space,
        &n2,
        &[(
            format!("{:?}", vec![vector64(&[0.0, 1.0])]),
            (vec![0.0, 1.0], None, false),
        )]
        .into_iter()
        .collect(),
    )
    .await;
    let [removed] = <[[u8; 32]; 1]>::try_from(removed.into_keys().collect::<Vec<_>>())
        .expect("one record of n2");
    let reads = format!(
        r#"for (_, n1, n2, n3 <<- @"rrs-rule-e-names") {{
  for (@y <<- n1) {{ @"{OUT}"!(["peek", y]) }} |
  for (@x <- n2 ~ ([0.0, 1.0])) {{ @"{OUT}"!(["take", x]) }} |
  for (@z <- n3 ~ ([0.6, 0.8])) {{ @"{OUT}"!(["persistent", z]) }}
}}"#
    );
    let second = chain.play(&reads, "rrs-rule-e-reads", AMPLE_LIMIT).await;
    assert_played("rule E reads", &second);
    backends::writer().flush().await;
    assert_eq!(replies(&second).len(), 3, "the three receives fire");
    let mut expected_calls = setup_calls;
    expected_calls.extend([
        MirrorCall::Ensure {
            collection: collection.clone(),
            metric: MirrorMetric::Cosine,
        },
        MirrorCall::Delete {
            collection: collection.clone(),
            ids: vec![removed],
        },
    ]);
    assert_eq!(mirror.calls(), expected_calls, "only the consume writes");
    let mut after = before.records.clone();
    after.remove(&removed);
    assert_eq!(
        mirror
            .collection(&collection)
            .expect("the mirror collection")
            .records,
        after
    );
    assert_eq!(
        chain.data(&data_channel(space, &n1)).await.len(),
        1,
        "the peek keeps n1"
    );
    assert_eq!(
        chain.data(&data_channel(space, &n3)).await.len(),
        1,
        "n3 is persistent"
    );
    assert_certifies("rule E setup", &first, &setup, AMPLE_LIMIT).await;
    assert_certifies("rule E reads", &second, &reads, AMPLE_LIMIT).await;
}

/// The factory refuses `mirror` with the qualifiers `temp` and `seq`, so no ephemeral space has a
/// mirror. Gate 9 refuses both with `InvalidParam` on `mirror` (P7 steps 1 and 2 make `temp` and
/// `seq` available in native execution, design v3 QT-1). The unit test
/// `reified_factory_gate_9_needs_the_default_qualifier_and_an_indexed_metric` checks gate 9. A
/// refused call creates no space, so nothing reaches the mirror.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_temp_never_mirrored() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    // Changed by P7 step 1 (design v3 QT-1): gate 9 refuses `temp` with `mirror`.
    // for qualifier in ["temp", "seq"] {
    // Changed by P7 step 2 (design v3 QT-1): gate 9 refuses `seq` with `mirror` too.
    // for (qualifier, code) in [("temp", "InvalidParam"), ("seq", "QualifierUnavailable")] {
    for (qualifier, code) in [("temp", "InvalidParam"), ("seq", "InvalidParam")] {
        let term = format!(
            r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?("{qualifier}"; capacity=4, dimensions=2, mirror="chroma")) {{
    for (mn <- m!?("gensym")) {{ mn!([1.0, 0.0]) }}
  }}
}}"#
        );
        let played = native_play(&term, AMPLE_LIMIT).await;
        assert_eq!(
            error_codes(&played.result.errors),
            vec![code.to_string()],
            "{qualifier}"
        );
    }
    backends::writer().flush().await;
    assert_eq!(
        mirror.calls(),
        Vec::new(),
        "no ephemeral space writes to the mirror"
    );
}

/// Publication never waits. With a stalled mirror and a queue of one batch, three deploys
/// succeed at once, and a batch that finds the queue full marks the collection dirty. With a
/// mirror that refuses every write, the batch fails after its retries, and the collection is
/// dirty too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_never_blocks_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let stalled = FakeMirror::stalled();
    let lock = backends::install(backends::services_with_queue(&embedder, &stalled, 1)).await;
    let writer = backends::writer();
    let mut chain = NativeChain::new().await;
    let setup = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-stall-names"!(*m, *mn) | mn!([1.0, 0.0])
  }} }}
}}"#
    );
    let send = |vector: &str| format!(r#"for (_, mn <<- @"rrs-stall-names") {{ mn!({vector}) }}"#);
    let deploys = [
        (setup, "rrs-stall-1"),
        (send("[0.0, 1.0]"), "rrs-stall-2"),
        (send("[0.6, 0.8]"), "rrs-stall-3"),
    ];
    for (term, label) in &deploys {
        let played = tokio::time::timeout(CERTIFY_TIMEOUT, chain.play(term, label, AMPLE_LIMIT))
            .await
            .unwrap_or_else(|_| panic!("{label}: the deploy waits for the mirror"));
        assert_played(label, &played);
    }
    let [m, _] = <[Par; 2]>::try_from(handles(&chain, "rrs-stall-names").await)
        .expect("a handle and a name");
    let collection = collection_name(&native(&m).sid);
    assert_eq!(
        writer.dirty(),
        BTreeSet::from([collection.clone()]),
        "a batch found the queue full"
    );
    stalled.release();
    writer.flush().await;
    assert!(!stalled
        .collection(&collection)
        .expect("the first batch reaches the mirror")
        .records
        .is_empty());
    assert_eq!(
        writer.take_dirty().into_keys().collect::<BTreeSet<_>>(),
        BTreeSet::from([collection])
    );

    let down = FakeMirror::down();
    lock.replace(services(&embedder, &down));
    let writer = backends::writer();
    let played = native_play(&store_then(&send_peeked()), AMPLE_LIMIT).await;
    assert_played("mirror down", &played);
    writer.flush().await;
    let collection = store_then_collection(&played);
    assert_eq!(
        down.calls(),
        vec![
            MirrorCall::Ensure {
                collection: collection.clone(),
                metric: MirrorMetric::Cosine,
            };
            3
        ],
        "each of the 3 attempts fails at its first call"
    );
    assert_eq!(writer.dirty(), BTreeSet::from([collection]));
}

/// An evaluation without a deploy result publishes nothing, and its journal does not reach the
/// next deploy: Casper's exploratory deploys inject a term through `RhoRuntime::inj`, and the
/// next root evaluation drops the journal that no result settled (FR-5').
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_exploratory_evaluation_publishes_nothing() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let setup = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-explore-names"!(*m, *mn)
  }} }}
}}"#
    );
    let played = chain.play(&setup, "rrs-explore-setup", AMPLE_LIMIT).await;
    assert_played("explore setup", &played);
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-explore-names").await)
        .expect("a handle and a name");
    let space = native(&m);
    chain
        .explore(r#"for (_, mn <<- @"rrs-explore-names") { mn!([1.0, 0.0]) }"#)
        .await
        .expect("the exploratory evaluation runs");
    assert_eq!(
        chain.data(&data_channel(space, &mn)).await.len(),
        1,
        "the exploratory evaluation stores its record"
    );
    backends::writer().flush().await;
    assert_eq!(
        mirror.calls(),
        Vec::new(),
        "an exploratory evaluation publishes nothing"
    );
    let send = r#"for (_, mn <<- @"rrs-explore-names") { mn!([0.0, 1.0]) }"#;
    let played = chain.play(send, "rrs-explore-send", AMPLE_LIMIT).await;
    assert_played("explore send", &played);
    backends::writer().flush().await;
    let expected: Expected = [
        (vec![vector64(&[1.0, 0.0])], (vec![1.0, 0.0], None, false)),
        (vec![vector64(&[0.0, 1.0])], (vec![0.0, 1.0], None, false)),
    ]
    .into_iter()
    .map(|(pars, record)| (format!("{pars:?}"), record))
    .collect();
    let mut records = mirror_records(&chain, space, &mn, &expected).await;
    assert_eq!(records.len(), 2, "both records are stored");
    records.retain(|_, record| record.embedding == vec![0.0, 1.0]);
    let collection = collection_name(&space.sid);
    assert_eq!(
        mirror.calls(),
        vec![
            MirrorCall::Ensure {
                collection: collection.clone(),
                metric: MirrorMetric::Cosine,
            },
            MirrorCall::Upsert {
                collection,
                records: records.into_values().collect(),
            },
        ],
        "the next deploy publishes only its own record"
    );
}

/// The journal of a deploy keeps the last write of each record: a record that one transaction
/// stores and a later transaction of the same deploy consumes leaves no record in the mirror. The
/// peek fires only after the store commits, and the consume only after the peek, so the three
/// transactions run in this order. Native replay certifies the deploy.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_journal_keeps_the_last_write_of_a_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let term = store_then(&format!(
        r#"for (@y <- mn ~ ([1.0, 0.0])) {{ @"{OUT}"!(["x", y]) }}"#
    ));
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("mirror last write", &played);
    assert_eq!(
        replies(&played).get("x"),
        Some(&vector64(&[1.0, 0.0])),
        "the consume takes the stored record"
    );
    backends::writer().flush().await;
    let collection = store_then_collection(&played);
    let calls = mirror.calls();
    assert!(
        matches!(
            calls.as_slice(),
            [MirrorCall::Ensure { .. }, MirrorCall::Delete { ids, .. }] if ids.len() == 1
        ),
        "the last write of the record is its delete: {calls:?}"
    );
    assert_eq!(
        mirror
            .collection(&collection)
            .expect("the mirror collection")
            .records,
        BTreeMap::new(),
        "the consumed record is not in the mirror"
    );
    assert_certifies("mirror last write", &played, &term, AMPLE_LIMIT).await;
}

/// A send that fires a parked waiter never enters the collection, so it never reaches the
/// mirror: the first deploy parks a receive, and the send of the second deploy fires it with the
/// incoming datum. Native replay certifies both deploys.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_fired_send_is_never_mirrored() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let park = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-fired-names"!(*m, *mn) |
    for (@x <- mn ~ ([1.0, 0.0])) {{ @"{OUT}"!(x) }}
  }} }}
}}"#
    );
    let first = chain.play(&park, "rrs-fired-park", AMPLE_LIMIT).await;
    assert_played("fired park", &first);
    let send = r#"for (_, mn <<- @"rrs-fired-names") { mn!([1.0, 0.0]) }"#;
    let second = chain.play(send, "rrs-fired-send", AMPLE_LIMIT).await;
    assert_played("fired send", &second);
    assert_eq!(
        second.out,
        vec![vec![vector64(&[1.0, 0.0])]],
        "the send fires the waiter"
    );
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-fired-names").await)
        .expect("a handle and a name");
    assert!(
        chain.data(&data_channel(native(&m), &mn)).await.is_empty(),
        "the incoming datum is never stored"
    );
    backends::writer().flush().await;
    assert_eq!(mirror.calls(), Vec::new(), "nothing reaches the mirror");
    assert_certifies("fired park", &first, &park, AMPLE_LIMIT).await;
    assert_certifies("fired send", &second, send, AMPLE_LIMIT).await;
}

/// The distance of a mirror collection follows the metric of its space: `cosine` (the default),
/// `dot` and `euclidean` give `cosine`, `ip` and `l2`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_collection_distance_follows_the_metric() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let term = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (c <- vdb!?({MIRRORED})) {{
    for (d <- vdb!?("default"; capacity=4, dimensions=2, metric="dot", mirror="chroma")) {{
      for (e <- vdb!?("default"; capacity=4, dimensions=2, metric="euclidean", mirror="chroma")) {{
        @"rrs-metric-handles"!(*c, *d, *e) |
        for (cn <- c!?("gensym")) {{ cn!([1.0, 0.0]) }} |
        for (dn <- d!?("gensym")) {{ dn!([1.0, 0.0]) }} |
        for (en <- e!?("gensym")) {{ en!([1.0, 0.0]) }}
      }}
    }}
  }}
}}"#
    );
    let played = chain.play(&term, "rrs-metric", AMPLE_LIMIT).await;
    assert_played("mirror metric", &played);
    backends::writer().flush().await;
    let handles = handles(&chain, "rrs-metric-handles").await;
    let calls = mirror.calls();
    for (handle, metric) in handles.iter().zip([
        MirrorMetric::Cosine,
        MirrorMetric::InnerProduct,
        MirrorMetric::L2,
    ]) {
        let collection = collection_name(&native(handle).sid);
        assert!(
            calls.contains(&MirrorCall::Ensure {
                collection: collection.clone(),
                metric,
            }),
            "{metric:?}: the collection is made with its distance: {calls:?}"
        );
        let held = mirror
            .collection(&collection)
            .expect("the mirror collection");
        assert_eq!(held.metric, metric);
        assert_eq!(held.records.len(), 1, "{metric:?}: one record");
    }
    assert_certifies("mirror metric", &played, &term, AMPLE_LIMIT).await;
}

/// Native replay certification publishes the journal of the replayed deploy to the mirror of
/// the replaying node: certifying two deploys in order gives that mirror the calls and the
/// records of the mirror of play.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_replay_publishes_its_own_journal() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let played_with = FakeMirror::new(Vec::new(), 0);
    let lock = backends::install(services(&embedder, &played_with)).await;
    let mut chain = NativeChain::new().await;
    let setup = format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED_TWO})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-replay-names"!(*m, *mn) | mn!([1.0, 0.0]) | mn!("doc"; embedding=[0.0, 1.0])
  }} }}
}}"#
    );
    let first = chain.play(&setup, "rrs-replay-setup", AMPLE_LIMIT).await;
    assert_played("replay setup", &first);
    let change = format!(
        r#"for (_, mn <<- @"rrs-replay-names") {{
  for (@x <- mn ~ ([1.0, 0.0])) {{ @"{OUT}"!(x) | mn!([0.6, 0.8]) }}
}}"#
    );
    let second = chain.play(&change, "rrs-replay-change", AMPLE_LIMIT).await;
    assert_played("replay change", &second);
    assert_eq!(second.out, vec![vec![vector64(&[1.0, 0.0])]]);
    backends::writer().flush().await;
    let [m, _] = <[Par; 2]>::try_from(handles(&chain, "rrs-replay-names").await)
        .expect("a handle and a name");
    let collection = collection_name(&native(&m).sid);
    let played_calls = played_with.calls();
    assert!(
        played_calls
            .iter()
            .any(|call| matches!(call, MirrorCall::Delete { .. })),
        "the second deploy deletes the consumed record: {played_calls:?}"
    );
    let replayed_with = FakeMirror::new(Vec::new(), 0);
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("replay setup", &first, &setup, AMPLE_LIMIT).await;
    assert_certifies("replay change", &second, &change, AMPLE_LIMIT).await;
    backends::writer().flush().await;
    assert_eq!(replayed_with.calls(), played_calls);
    assert_eq!(
        replayed_with.collection(&collection),
        played_with.collection(&collection)
    );
}

// ─── reconciler (step 7c) ────────────────────────────────────────────────────────────────────

/// A deploy that makes a mirrored space with the records `[1.0, 0.0]` and `[0.0, 1.0]` on one
/// name, and sends the space and the name on `rrs-reconcile-names`.
fn reconcile_setup() -> String {
    format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-reconcile-names"!(*m, *mn) | mn!([1.0, 0.0]) | mn!([0.0, 1.0])
  }} }}
}}"#
    )
}

/// The deploy after [`reconcile_setup`]: it consumes `[1.0, 0.0]` and stores `[0.6, 0.8]`.
const RECONCILE_CHANGE: &str = r#"for (_, mn <<- @"rrs-reconcile-names") {
  for (@x <- mn ~ ([1.0, 0.0])) { mn!([0.6, 0.8]) }
}"#;

/// The expected mirror records of every payload that the reconciler tests store.
fn reconcile_expected() -> Expected {
    [
        (vec![vector64(&[1.0, 0.0])], (vec![1.0, 0.0], None, false)),
        (vec![vector64(&[0.0, 1.0])], (vec![0.0, 1.0], None, false)),
        (
            vec![vector64(&[0.6, 0.8])],
            (vec![0.6f64 as f32, 0.8f64 as f32], None, false),
        ),
    ]
    .into_iter()
    .map(|(pars, record)| (format!("{pars:?}"), record))
    .collect()
}

/// A record that no state holds, on the channel `digest`: the write of a fork.
fn orphan(digest: &str) -> MirrorRecord {
    MirrorRecord {
        id: [0xEE; 32],
        embedding: vec![0.5, 0.5],
        channel: digest.to_string(),
        persistent: false,
        document: None,
    }
}

/// The reconciler repairs a collection to the finalized state: it deletes an orphan that a
/// fork wrote, adds a record that a later removal took out, and sets the watermark. A second
/// pass at the same block leaves the collection, and a pass whose writes fail keeps the
/// watermark and the divergence for the next pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_reconcile_removes_orphans() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let setup = reconcile_setup();
    let played = chain.play(&setup, "rrs-reconcile-setup", AMPLE_LIMIT).await;
    assert_played("reconcile setup", &played);
    backends::writer().flush().await;
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
        .expect("a handle and a name");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let digest = channel_digest(&key(&mn));
    let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
    let held = mirror
        .collection(&collection)
        .expect("the mirror collection");
    assert_eq!(held.records, records, "the journal published both records");
    assert_eq!(
        held.space,
        Some(space.sid),
        "the journal registered the space"
    );
    assert_eq!(
        held.channels,
        BTreeMap::from([(digest.clone(), key(&mn).as_bytes().to_vec())]),
        "the journal registered the channel"
    );

    let first = *records.keys().next().expect("a record");
    let mut diverged = held.clone();
    diverged.records.remove(&first);
    diverged.records.insert(orphan(&digest).id, orphan(&digest));
    mirror.put(&collection, diverged);
    let state = chain.finalized_state();
    let block = Finalized {
        block_number: 1,
        root: played.root.clone(),
    };
    let report = reconcile(&current_services(), state.as_ref(), &block).await;
    assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
    assert_eq!((report.deleted, report.added), (1, 1));
    let repaired = mirror
        .collection(&collection)
        .expect("the mirror collection");
    assert_eq!(
        repaired.records, records,
        "the mirror holds the finalized records"
    );
    assert_eq!(repaired.watermark, 1);

    let report = reconcile(&current_services(), state.as_ref(), &block).await;
    assert_eq!(report.current, BTreeSet::from([collection.clone()]));
    assert!(report.repaired.is_empty() && report.failed.is_empty());

    let mut diverged = repaired.clone();
    diverged.records.insert(orphan(&digest).id, orphan(&digest));
    mirror.put(&collection, diverged.clone());
    mirror.set_writes(WriteMode::Refuse);
    let next = Finalized {
        block_number: 2,
        root: played.root.clone(),
    };
    let report = reconcile(&current_services(), state.as_ref(), &next).await;
    assert_eq!(report.failed, BTreeSet::from([collection.clone()]));
    assert_eq!(
        mirror.collection(&collection),
        Some(diverged),
        "a failed pass keeps the watermark and the orphan"
    );
}

/// After a restart, the writer and its memory are gone, and the mirror keeps its registry. The
/// reconciler rescans every registered channel: a mirror that lost every record of a collection
/// gets them back from the finalized state.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_restart_rescans_reports() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let played = chain
        .play(&reconcile_setup(), "rrs-reconcile-setup", AMPLE_LIMIT)
        .await;
    assert_played("restart setup", &played);
    backends::writer().flush().await;
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
        .expect("a handle and a name");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
    let mut lost = mirror
        .collection(&collection)
        .expect("the mirror collection");
    lost.records.clear();
    mirror.put(&collection, lost);
    lock.replace(services(&embedder, &mirror));
    let state = chain.finalized_state();
    let block = Finalized {
        block_number: 3,
        root: played.root.clone(),
    };
    let report = reconcile(&current_services(), state.as_ref(), &block).await;
    assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
    assert_eq!((report.deleted, report.added), (0, 2));
    let repaired = mirror
        .collection(&collection)
        .expect("the mirror collection");
    assert_eq!(repaired.records, records);
    assert_eq!(repaired.watermark, 3);
}

/// A batch that fails leaves its collection dirty with its channels. The next pass repairs the
/// collection even at its watermark, from the state that holds the new record.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_reconcile_repairs_a_dirty_collection() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let setup = reconcile_setup();
    let first = chain.play(&setup, "rrs-reconcile-setup", AMPLE_LIMIT).await;
    assert_played("dirty setup", &first);
    let writer = backends::writer();
    writer.flush().await;
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
        .expect("a handle and a name");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let state = chain.finalized_state();
    let block = |root: &Blake2b256Hash| Finalized {
        block_number: 1,
        root: root.clone(),
    };
    let report = reconcile(&current_services(), state.as_ref(), &block(&first.root)).await;
    assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));

    mirror.set_writes(WriteMode::Refuse);
    let second = chain
        .play(RECONCILE_CHANGE, "rrs-reconcile-change", AMPLE_LIMIT)
        .await;
    assert_played("dirty change", &second);
    writer.flush().await;
    assert_eq!(writer.dirty(), BTreeSet::from([collection.clone()]));
    mirror.set_writes(WriteMode::Apply);
    let report = reconcile(&current_services(), state.as_ref(), &block(&second.root)).await;
    assert_eq!(
        report.repaired,
        BTreeSet::from([collection.clone()]),
        "a dirty collection is repaired at its watermark"
    );
    assert_eq!((report.deleted, report.added), (1, 1));
    assert!(
        writer.dirty().is_empty(),
        "the repair clears the dirty mark"
    );
    let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
    assert_eq!(
        mirror
            .collection(&collection)
            .expect("the mirror collection")
            .records,
        records
    );
}

/// The reconciler of a node follows its `BlockFinalised` events: it maps each block to its
/// post-state root, and the passes reach the newest finalized block.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_reconciler_follows_the_finalized_blocks() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = FakeMirror::new(Vec::new(), 0);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let mut chain = NativeChain::new().await;
    let first = chain
        .play(&reconcile_setup(), "rrs-reconcile-setup", AMPLE_LIMIT)
        .await;
    assert_played("events setup", &first);
    let second = chain
        .play(RECONCILE_CHANGE, "rrs-reconcile-change", AMPLE_LIMIT)
        .await;
    assert_played("events change", &second);
    backends::writer().flush().await;
    let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
        .expect("a handle and a name");
    let space = native(&m);
    let collection = collection_name(&space.sid);
    let mut lost = mirror
        .collection(&collection)
        .expect("the mirror collection");
    lost.records.clear();
    mirror.put(&collection, lost);

    let state = chain.finalized_state();
    state.finalize(&[1; 32], &first.root);
    state.finalize(&[2; 32], &second.root);
    let events = F1r3flyEvents::new();
    mirror_reconciler::start(&events, state.clone());
    for (hash, number) in [([1u8; 32], 1), ([2u8; 32], 2), ([9u8; 32], 9)] {
        events
            .publish(F1r3flyEvent::block_finalised(
                hex::encode(hash),
                number,
                0,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                String::new(),
                0,
            ))
            .expect("the event is published");
    }
    let reached = tokio::time::timeout(CERTIFY_TIMEOUT, async {
        loop {
            let held = mirror
                .collection(&collection)
                .expect("the mirror collection");
            if held.watermark == 2 {
                return held;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the passes reach the newest block that the node holds");
    let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
    assert_eq!(
        reached.records, records,
        "the mirror holds the state of block 2"
    );
}

// ─── properties (step 8d) ────────────────────────────────────────────────────────────────────

/// The vectors that the generated chains store and query.
const VECTORS: [[f64; 2]; 4] = [[1.0, 0.0], [0.0, 1.0], [0.6, 0.8], [0.8, 0.6]];

/// The vector `VECTORS[index]` as Rholang source.
fn vector_source(index: usize) -> String {
    let [a, b] = VECTORS[index];
    format!("[{a:?}, {b:?}]")
}

/// The expected mirror record of each vector of [`VECTORS`] and of the two records of
/// [`reconcile_setup`].
fn vectors_expected() -> Expected {
    VECTORS
        .iter()
        .map(|[a, b]| {
            (
                format!("{:?}", vec![vector64(&[*a, *b])]),
                (vec![*a as f32, *b as f32], None, false),
            )
        })
        .collect()
}

/// A step of a generated chain on the mirrored name of [`reconcile_setup`].
#[derive(Clone, Copy, Debug)]
enum ChainStep {
    /// A deploy that stores `VECTORS[v]`.
    Store(usize),
    /// A deploy that takes the record nearest to `VECTORS[v]`, or parks its receive.
    Take(usize),
    /// A deploy that peeks a record: Rule E removes it and stores it again.
    Peek,
    /// A deploy that stores `VECTORS[v]` and then fails with a vector of three dimensions.
    Fail(usize),
    /// An exploratory evaluation (`RhoRuntime::inj`) that stores `VECTORS[v]`.
    Explore(usize),
}

fn chain_step(explore: bool) -> BoxedStrategy<ChainStep> {
    let index = 0..VECTORS.len();
    match explore {
        true => prop_oneof![
            3 => index.clone().prop_map(ChainStep::Store),
            2 => index.clone().prop_map(ChainStep::Take),
            1 => Just(ChainStep::Peek),
            1 => index.clone().prop_map(ChainStep::Fail),
            1 => index.prop_map(ChainStep::Explore),
        ]
        .boxed(),
        false => prop_oneof![
            3 => index.clone().prop_map(ChainStep::Store),
            2 => index.clone().prop_map(ChainStep::Take),
            1 => Just(ChainStep::Peek),
            1 => index.prop_map(ChainStep::Fail),
        ]
        .boxed(),
    }
}

/// The source of the deploy of `step`.
fn step_source(step: ChainStep) -> String {
    let names = r#"for (_, mn <<- @"rrs-reconcile-names") "#;
    match step {
        ChainStep::Store(v) | ChainStep::Explore(v) => {
            format!("{names}{{ mn!({}) }}", vector_source(v))
        }
        ChainStep::Take(v) => {
            format!(
                "{names}{{ for (@x <- mn ~ ({})) {{ Nil }} }}",
                vector_source(v)
            )
        }
        ChainStep::Peek => format!("{names}{{ for (@x <<- mn) {{ Nil }} }}"),
        ChainStep::Fail(v) => format!(
            "{names}{{ mn!({}) | for (@x <<- mn) {{ mn!([1.0, 2.0, 3.0]) }} }}",
            vector_source(v)
        ),
    }
}

/// The ids of the records on the name `name` of `space` in the state of `chain`.
async fn state_ids(chain: &NativeChain, space: NativeSpace, name: &Par) -> BTreeSet<[u8; 32]> {
    chain
        .data(&data_channel(space, name))
        .await
        .into_iter()
        .map(|datum| {
            <[u8; 32]>::try_from(datum.source.hash.bytes().as_slice()).expect("a 32-byte record id")
        })
        .collect()
}

/// The ids of the records of `collection` in `mirror`.
fn mirror_ids(mirror: &FakeMirror, collection: &str) -> BTreeSet<[u8; 32]> {
    mirror
        .collection(collection)
        .map(|held| held.records.keys().copied().collect())
        .unwrap_or_default()
}

/// The mirror follows the published deploys and nothing else (ChromaMirror.tla,
/// PublishOnlyOnSuccess and NetWritesExact). After each step of a generated chain, the mirror
/// holds the records that the state changes of the successful deploys give: a failed deploy
/// publishes nothing, a peek writes nothing, a take deletes, and an exploratory evaluation
/// publishes nothing. The test chain keeps the state changes of a failed deploy and of an
/// exploratory evaluation (it reverts nothing, unlike the deploys of a block), so the oracle
/// applies only the state changes of the successful deploys.
#[test]
fn prop_mirror_tracks_the_published_deploys() {
    check(
        runtime_config(),
        proptest::collection::vec(chain_step(true), 1..6),
        |steps| {
            tokio_runtime().block_on(async {
                let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
                let mirror = FakeMirror::new(Vec::new(), 0);
                let _lock = backends::install(services(&embedder, &mirror)).await;
                let mut chain = NativeChain::new().await;
                let played = chain
                    .play(&reconcile_setup(), "rrs-track-setup", AMPLE_LIMIT)
                    .await;
                assert_played("track setup", &played);
                let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
                    .expect("a handle and a name");
                let space = native(&m);
                let collection = collection_name(&space.sid);
                backends::writer().flush().await;
                let mut expected = state_ids(&chain, space, &mn).await;
                for (index, step) in steps.iter().enumerate() {
                    let before = state_ids(&chain, space, &mn).await;
                    let published = match step {
                        ChainStep::Explore(_) => {
                            // An exploratory evaluation publishes nothing, also when it fails (a
                            // full collection refuses its store).
                            if let Err(error) = chain.explore(&step_source(*step)).await {
                                drop(error);
                            }
                            false
                        }
                        _ => {
                            let label = format!("rrs-track-{index}");
                            let played = chain.play(&step_source(*step), &label, AMPLE_LIMIT).await;
                            played.result.errors.is_empty()
                        }
                    };
                    backends::writer().flush().await;
                    let after = state_ids(&chain, space, &mn).await;
                    if published {
                        expected.extend(after.difference(&before).copied());
                        expected.retain(|id| after.contains(id) || !before.contains(id));
                    }
                    let held = mirror_ids(&mirror, &collection);
                    prop_assert_eq!(held, expected.clone(), "step {} {:?}", index, step);
                }
                Ok(())
            })
        },
    );
}

/// A reconciler pass repairs the mirror to the finalized view (ChromaMirror.tla,
/// ReconciledEqualsFinalizedView). After a generated chain, the test removes some records of
/// the mirror and adds orphans on the registered channel. A pass whose writes fail keeps the
/// mirror and the watermark; a pass that finishes holds exactly the records of the finalized
/// state and sets the watermark.
#[test]
fn prop_mirror_reconcile_repairs_the_finalized_view() {
    let strategy = (
        proptest::collection::vec(chain_step(false), 0..4),
        proptest::collection::vec(proptest::bool::ANY, 6),
        0usize..3,
        proptest::bool::ANY,
    );
    check(
        runtime_config(),
        strategy,
        |(steps, keep, orphans, refuse_first)| {
            tokio_runtime().block_on(async {
                let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
                let mirror = FakeMirror::new(Vec::new(), 0);
                let _lock = backends::install(services(&embedder, &mirror)).await;
                let mut chain = NativeChain::new().await;
                let mut played = chain
                    .play(&reconcile_setup(), "rrs-repair-setup", AMPLE_LIMIT)
                    .await;
                assert_played("repair setup", &played);
                let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
                    .expect("a handle and a name");
                let space = native(&m);
                let collection = collection_name(&space.sid);
                let digest = channel_digest(&key(&mn));
                for (index, step) in steps.iter().enumerate() {
                    played = chain
                        .play(
                            &step_source(*step),
                            &format!("rrs-repair-{index}"),
                            AMPLE_LIMIT,
                        )
                        .await;
                }
                backends::writer().flush().await;
                let finalized = mirror_records(&chain, space, &mn, &vectors_expected()).await;
                let mut diverged = mirror
                    .collection(&collection)
                    .expect("the setup made the collection");
                let ids: Vec<[u8; 32]> = diverged.records.keys().copied().collect();
                for (id, kept) in ids.iter().zip(keep.iter().cycle()) {
                    if !kept {
                        diverged.records.remove(id);
                    }
                }
                for orphan_index in 0..orphans {
                    let mut record = orphan(&digest);
                    record.id = [0xE0 + u8::try_from(orphan_index).expect("a small index"); 32];
                    diverged.records.insert(record.id, record);
                }
                let watermark = diverged.watermark;
                mirror.put(&collection, diverged.clone());
                let state = chain.finalized_state();
                if refuse_first {
                    mirror.set_writes(WriteMode::Refuse);
                    let block = Finalized {
                        block_number: 1,
                        root: played.root.clone(),
                    };
                    reconcile(&current_services(), state.as_ref(), &block).await;
                    let kept = mirror.collection(&collection).expect("the collection");
                    // A refused write may leave the deletes or the upserts applied, never the
                    // watermark (MirrorBackend::set_watermark is a write of the registry).
                    prop_assert_eq!(
                        kept.watermark,
                        watermark,
                        "a failed pass keeps the watermark"
                    );
                    mirror.set_writes(WriteMode::Apply);
                }
                let block = Finalized {
                    block_number: 2,
                    root: played.root.clone(),
                };
                reconcile(&current_services(), state.as_ref(), &block).await;
                let repaired = mirror.collection(&collection).expect("the collection");
                prop_assert_eq!(
                    &repaired.records,
                    &finalized,
                    "the mirror holds the finalized view"
                );
                prop_assert_eq!(repaired.watermark, 2);
                Ok(())
            })
        },
    );
}

/// A generated call of `embed` or `ann`.
#[derive(Clone, Debug)]
enum ServiceCall {
    /// `embed` of the texts.
    Embed(Vec<String>),
    /// `ann` of the vector `VECTORS[v]` with `k`.
    AnnVector(usize, u8),
    /// `ann` of a text with `k`.
    AnnText(String, u8),
    /// `ann` of a vector of three dimensions (DimensionMismatch).
    AnnWrongDimensions(u8),
}

fn service_call() -> impl Strategy<Value = ServiceCall> {
    let text = "[a-z]{1,8}";
    let k = proptest::sample::select(vec![0u8, 1, 2, 5]);
    prop_oneof![
        proptest::collection::vec(text, 0..3).prop_map(ServiceCall::Embed),
        (0..VECTORS.len(), k.clone()).prop_map(|(v, k)| ServiceCall::AnnVector(v, k)),
        (text, k.clone()).prop_map(|(text, k)| ServiceCall::AnnText(text, k)),
        k.prop_map(ServiceCall::AnnWrongDimensions),
    ]
}

/// A deploy that creates a mirrored VectorDB space `m` with a name `mn`, and then makes `calls`,
/// each sending `[label, reply]` on [`OUT`].
fn services_program(service_calls: &[ServiceCall]) -> String {
    let parts: Vec<String> = service_calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let (process, arguments) = match call {
                ServiceCall::Embed(texts) => ("embed", quoted_list(texts)),
                ServiceCall::AnnVector(v, k) => (
                    "ann",
                    format!(r#"*m, *mn, {}, {{"k": {k}}}"#, vector_source(*v)),
                ),
                ServiceCall::AnnText(text, k) => {
                    ("ann", format!(r#"*m, *mn, {}, {{"k": {k}}}"#, quoted(text)))
                }
                ServiceCall::AnnWrongDimensions(k) => {
                    ("ann", format!(r#"*m, *mn, [1.0, 2.0, 3.0], {{"k": {k}}}"#))
                }
            };
            format!(r#"for (@r <- {process}!?({arguments})) {{ @"{OUT}"!(["c{index}", r]) }}"#)
        })
        .collect();
    format!(
        r#"new embed(`rho:vectordb:embed`), ann(`rho:vectordb:ann`), vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{
    for (mn <- m!?("gensym")) {{
      @"{OUT}"!(["m", *m]) | @"{OUT}"!(["mn", *mn]){}{}
    }}
  }}
}}"#,
        if parts.is_empty() { "" } else { " |\n      " },
        parts.join(" |\n      ")
    )
}

/// The services of a node: its embedder and its mirror, with the neighbours of `answer` or down.
fn played_services(answer: &[u8], down: bool) -> (Arc<MockEmbedder>, Arc<FakeMirror>) {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let mirror = match down {
        true => FakeMirror::down(),
        false => FakeMirror::new(
            answer
                .iter()
                .zip([0.125f32, 0.25, 0.375])
                .map(|(byte, distance)| neighbour(&[*byte; 32], distance))
                .collect(),
            u64::try_from(answer.len()).expect("a short answer"),
        ),
    };
    (embedder, mirror)
}

/// The recorded outputs of `embed` and `ann` make consensus independent of the services of a
/// node (ChromaMirror.tla, ConsensusIndependentOfMirror, AdvisoryRecorded and EqualHostWork).
/// Generated calls play with one embedder and one mirror (or a mirror that is down), and native
/// replay certifies the play with another embedder and another mirror: replay reproduces the
/// result, the phlo and the root of play, and asks no service. The checks of an `ann` call are
/// journaled native operations, so replay makes each check of play: otherwise the journal of
/// play is incomplete (decision 67). The total host work differs by design, because replay
/// certification also verifies the committed evidence.
#[test]
fn prop_vectordb_services_replay_reproduces_the_recorded_outputs() {
    let strategy = (
        proptest::collection::vec(service_call(), 1..4),
        proptest::collection::vec(1u8..9, 0..3),
        proptest::bool::weighted(0.25),
    );
    check(
        runtime_config(),
        strategy,
        |(service_calls, answer, down)| {
            tokio_runtime().block_on(async {
                let term = services_program(&service_calls);
                let (embedder, mirror) = played_services(&answer, down);
                let lock = backends::install(services(&embedder, &mirror)).await;
                let played = native_play_with(&term, LARGE_LIMIT, host_work()).await;
                let replayed_embedder = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
                let replayed_with = FakeMirror::new(vec![neighbour(&[0xAA; 32], 0.5)], 99);
                lock.replace(services(&replayed_embedder, &replayed_with));
                let certification = tokio::time::timeout(
                    CERTIFY_TIMEOUT,
                    certify_with(&played, &term, LARGE_LIMIT, host_work()),
                )
                .await
                .map_err(|_| TestCaseError::fail("native replay does not finish"))?;
                assert_reproduces("services replay", &played, &certification);
                prop_assert!(
                    replayed_with.queries().is_empty(),
                    "native replay asks no mirror"
                );
                prop_assert_eq!(
                    replayed_embedder.calls(),
                    0,
                    "native replay asks no embedder"
                );
                Ok(())
            })
        },
    );
}
