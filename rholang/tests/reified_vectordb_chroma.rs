//! The suite of the recorded VectorDB processes `rho:vectordb:embed` and `rho:vectordb:ann`, of
//! the `config` reply of a mirrored VectorDB space (P6 step 7a) and of the mirror publication
//! (P6 step 7b). Design v3 4.9, plan decisions 57, 58, 66 to 69, 98 and 104 to 106.
//!
//! Changed by decisions 98 and 114 (DR-114 oracle mode): a play publishes the net writes of each
//! commit, whatever the result of its deploy, and replay publishes nothing. Nothing repairs the
//! mirror, so the suite has no reconciler tests. Before, the suite also covered the mirror
//! journal, its publication on the final result, and the mirror reconciler (P6 step 7c).
//!
//! `embed` and `ann` ask node-local services: an embedder and the ChromaDB mirror of a space. The
//! dispatcher records the whole reply of a call on its produce event, and replay produces the
//! recorded reply without a service call, so two nodes with different services compute the same
//! state. The tests install a mock embedder and a fake mirror (`support/backends.rs`).
//!
//! Changed by decision 113: only a build with the feature `chromadb` registers `embed` and `ann`,
//! so their tests run only in that build. The `config` test and the mirror tests run in every
//! build.
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
//! | `vectordb_embed_out_of_phlo_rejects_output` | One phlo below the usage of a call, the reply does not appear, the trigger keeps the record and no produce is marked failed (DR-114, G-A), two plays agree, and native replay meets the same denial. |
//! | `vectordb_record_room_bounds_embed_replies` | The record room bounds the successes of `embed`: a second success past the room, or a first one a byte past it, becomes `EXT_OUTPUT_TOO_LARGE`, and a failure takes no room (decisions 68 and 105). |
//! | `vectordb_record_room_bounds_ann_replies` | With no room, the answer of `ann` becomes `EXT_OUTPUT_TOO_LARGE`, and a failure keeps its code (decisions 68 and 105). |
//! | `vectordb_replay_rejects_a_call_without_its_record` | Native replay of a log without the record of an `embed` call fails closed (decision 68). |
//! | `vectordb_ann_replies_and_codes` | A numeric query, a text query, every reply code, the collection and the channel digest that the mirror receives, the drop of a malformed id, and native replay. |
//! | `vectordb_ann_replay_equal` | Native replay with another mirror reproduces the advisory answers and a recorded outage, and asks no mirror and no embedder. |
//! | `vectordb_ann_ids_feed_the_exact_query` | Across two deploys, the advisory ids, with a stale id among them, restrict an exact query, and the exact order decides. |
//! | `vectordb_embed_and_ann_reject_params_and_persistent_sends` | A call with params aborts with `ParamsUnsupported`, and a persistent call fails at its fire with `SystemProcessShapeError` (base rule P2, decision 106), before any service call. |
//! | `vectordb_config_reports_the_mirror` | The `config` reply of a mirrored space names its backend and its collection, and an unmirrored space has no such entry. |
//! | `mirror_publishes_a_successful_deploy` | Each commit of a successful deploy publishes one batch: an ensure and an upsert of its record with a vector, with its id, embedding, channel digest, persistence and document. A record without a vector and an unmirrored space are not mirrored. |
//! | `mirror_publishes_the_commits_of_a_failed_deploy` | A space error and an out-of-phlo deploy publish the upsert of the store that committed before the failure, and no delete (decision 98). |
//! | `mirror_rule_e_coalesces` | A peek writes nothing (Rule E), a consume of a non-persistent record deletes it, and a persistent record stays. |
//! | `mirror_temp_never_mirrored` | The factory refuses `mirror` with `temp` and with `seq` (`InvalidParam`, gate 9), so nothing reaches the mirror. |
//! | `mirror_never_blocks_deploy` | With a stalled mirror and a full queue, or a mirror that refuses every write, deploys succeed at once, and a refused batch is dropped (decision 114). |
//! | `mirror_publishes_each_commit_of_a_deploy_in_order` | A record that one transaction stores and a later transaction of the same deploy consumes gives an upsert and then a delete, in commit order, and leaves no record in the mirror. |
//! | `mirror_fired_send_is_never_mirrored` | A send that fires a parked waiter never enters the collection, so nothing reaches the mirror. |
//! | `mirror_collection_distance_follows_the_metric` | The collections of spaces with the metrics `cosine`, `dot` and `euclidean` have the distances `cosine`, `ip` and `l2`. |
//! | `mirror_exploratory_evaluation_publishes_its_commits` | An exploratory dry run publishes the commit of its store, so the mirror keeps the record after the revert (an orphan, decision 98). |
//! | `mirror_replay_publishes_nothing` | Native replay certification of two deploys publishes nothing to the mirror of the replaying node (decision 98). |
//! | `prop_mirror_tracks_the_writes_of_every_play` | After each step of a generated chain of stores, takes, peeks, failing deploys and exploratory dry runs, the new calls of the mirror are the writes of the commits of the step. Native replay of the plays publishes nothing (ChromaMirror.tla, NetWritesExact, ReplayNeverPublishes and OnePublicationPerCommit). |
//! | `prop_vectordb_services_replay_reproduces_the_recorded_outputs` | Generated `embed` and `ann` calls replay with other services: replay reproduces play with a complete journal, and it asks no service (ConsensusIndependentOfMirror, AdvisoryRecorded). |
//!
//! Retired by decision 114: the rows of the reconciler tests `mirror_reconcile_removes_orphans`,
//! `mirror_restart_rescans_reports`, `mirror_reconcile_repairs_a_dirty_collection`,
//! `mirror_reconciler_follows_the_finalized_blocks` and
//! `prop_mirror_reconcile_repairs_the_finalized_view`.
//!
//! Changed by decision 104: replay makes no check of an `ann` call. The config read writes no
//! journal row (DR-131), so certification stays complete. Before, decision 67 made replay repeat
//! the checks of play, and native replay that skipped the config read failed the completeness
//! check.

// Added by decision 113: only a chromadb build registers `embed` and `ann`, so the helpers of
// their tests are unused in the default build.
#![cfg_attr(not(feature = "chromadb"), allow(dead_code, unused_imports))]

#[path = "support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use proptest::prelude::*;
use prost::Message;
use rholang::rust::interpreter::errors::InterpreterError;
// Added by decision 105: the failure replies of `embed` and `ann` use the base codes.
use rholang::rust::interpreter::io::errors::FserrCode;
use rholang::rust::interpreter::io::response;
use rholang::rust::interpreter::reified::handle::MIRROR_ENTRY;
// Retired by decision 114: the mirror is an oracle store, and nothing repairs it.
// use rholang::rust::interpreter::reified::mirror_reconciler::{self, reconcile, Finalized};
// Changed by decisions 105 and 114: the base codes replace the Reified codes, and no reconciler
// reads the services.
// use rholang::rust::interpreter::reified::vectordb_services::{
//     channel_digest, collection_name, current as current_services, Neighbour, ServiceReplyCode,
//     MAX_TEXTS, MAX_TEXT_BYTES,
// };
use rholang::rust::interpreter::reified::vectordb_services::{
    channel_digest, collection_name, Neighbour, MAX_TEXTS, MAX_TEXT_BYTES,
};
use rholang::rust::interpreter::spaces::mirror::{MirrorMetric, MirrorRecord};
use rholang::rust::interpreter::spaces::namespace::NativeSpace;
use rholang::rust::interpreter::spaces::record;
use rholang::rust::interpreter::spaces::types::config::SpaceConfig;
// Retired by decision 114: only the reconciler tests named a root, an event or a write mode.
// use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};
// use shared::rust::shared::f1r3fly_event::F1r3flyEvent;
// use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
// use support::backends::{
//     self, services, EmbedMode, FakeMirror, MirrorCall, MockEmbedder, WriteMode,
// };
use support::backends::{self, services, EmbedMode, FakeMirror, MirrorCall, MockEmbedder};
// Changed by decision 98: the tracking property certifies failed plays, which have no root.
// use support::native::{
//     assert_reproduces, certify, committed_evidence, finish_injected, native_play,
//     native_play_with_room, replay_environment_from, Certification, NativeChain, Played,
//     AMPLE_LIMIT, OUT,
// };
use support::native::{
    assert_certified, assert_reproduces, certify, committed_evidence, finish_injected, native_play,
    native_play_with_room, replay_environment_from, Certification, NativeChain, Played,
    AMPLE_LIMIT, OUT,
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

// Retired by decision 114: only the watermark of the `ann` reply was an integer.
// fn int(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

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

// Retired by decision 105: the failure replies of `embed` and `ann` use the base codes.
// /// The reply `[false, code, detail]` of `code`.
// fn failed(code: ServiceReplyCode) -> Par {
//     list(vec![
//         flag(false),
//         string(code.code()),
//         string(code.detail()),
//     ])
// }

/// Added by decision 105: the failure reply `[false, code, "<process>: <text>"]` of the process
/// `process`, as `native_failure_reply` builds it.
fn failure(process: &str, code: &'static str, text: &str) -> Par {
    response::err(FserrCode(code), format!("{process}: {text}"))
}

/// The `ann` reply of `neighbours`.
///
/// Changed by decision 114: the reply carries no watermark. Before, the reply ended with the
/// watermark `watermark`.
// fn answer(neighbours: &[Neighbour], watermark: i64) -> Par {
fn answer(neighbours: &[Neighbour]) -> Par {
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
        // int(watermark),
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
///
/// Changed by decision 105: a refused call replies `EXT_BAD_ARG`, and a failure of the model or a
/// node without services replies `EXT_FAILED`. Before, each failure had its own Reified code.
// Added by decision 113: only a chromadb build registers `embed`.
#[cfg(feature = "chromadb")]
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
    // Added by decision 105: the reply of a refused call.
    let bad_embed = failure("vectordb_embed", "EXT_BAD_ARG", "invalid argument");
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
        // Changed by decision 105: every refused call replies `EXT_BAD_ARG`.
        // ("over-count", failed(ServiceReplyCode::EmbeddingRateLimited)),
        // ("over-bytes", failed(ServiceReplyCode::EmbeddingRateLimited)),
        // ("empty-list", failed(ServiceReplyCode::BadArgument)),
        // ("number", failed(ServiceReplyCode::BadArgument)),
        // ("mixed", failed(ServiceReplyCode::BadArgument)),
        ("over-count", bad_embed.clone()),
        ("over-bytes", bad_embed.clone()),
        ("empty-list", bad_embed.clone()),
        ("number", bad_embed.clone()),
        ("mixed", bad_embed),
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
    // Changed by decision 105: every failure of the model replies `EXT_FAILED`.
    // for (mode, code) in [
    //     (
    //         Some(EmbedMode::NonFinite),
    //         ServiceReplyCode::NonFiniteEmbedding,
    //     ),
    //     (
    //         Some(EmbedMode::Unavailable),
    //         ServiceReplyCode::EmbeddingUnavailable,
    //     ),
    //     (
    //         Some(EmbedMode::Short),
    //         ServiceReplyCode::EmbeddingUnavailable,
    //     ),
    //     (None, ServiceReplyCode::EmbeddingUnavailable),
    // ] {
    let model_failed = failure("vectordb_embed", "EXT_FAILED", "external service failed");
    for mode in [
        Some(EmbedMode::NonFinite),
        Some(EmbedMode::Unavailable),
        Some(EmbedMode::Short),
        None,
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
        // assert_eq!(replies(&played).get("call"), Some(&failed(code)), "{label}");
        assert_eq!(replies(&played).get("call"), Some(&model_failed), "{label}");
        lock.replace(services(&replayed_with, &FakeMirror::down()));
        assert_certifies(&label, &played, &term, AMPLE_LIMIT).await;
    }
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");
}

/// Native replay produces the recorded reply of play with another embedder, which it never
/// calls.
// Added by decision 113: only a chromadb build registers `embed`.
#[cfg(feature = "chromadb")]
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

/// The records of the produce events of `log`: the decoded `output_value` of each produce that
/// carries one, in log order.
fn produce_records(log: &[Event]) -> Vec<Vec<Par>> {
    log.iter()
        .filter_map(|event| match event {
            Event::IoEvent(IOEvent::Produce(produce)) if !produce.output_value.is_empty() => Some(
                produce
                    .output_value
                    .iter()
                    .map(|bytes| Par::decode(bytes.as_slice()).expect("a record decodes"))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// One phlo below the usage of a call, the reply does not appear. Native funded execution keeps
/// the record of the reply on the produce that triggered the call and raises the cause (DR-114
/// of `feature/cost-accounted-rho`, gap G-A), so the deploy fails with `OutOfPhlogistonsError`
/// and no produce copy is marked failed. Two plays agree, and native replay with another
/// embedder, which it never asks, meets the same denial (decision 68).
// Changed by decision 89 (DR-114, decision 68): the record-and-replay mechanism of external
// service calls reached this branch, so `embed` moved to it. The old test expected the error
// `ProduceFailureWithOutput` and a produce marked failed, and it could not certify the play:
//
// let mut runs = Vec::with_capacity(2);
// for _ in 0..2 {
//     let played = native_play(&term, used - 1).await;
//     match played.result.errors.as_slice() {
//         [InterpreterError::ProduceFailureWithOutput {
//             cause,
//             output_not_produced,
//         }] => {
//             assert!(
//                 matches!(cause.as_ref(), InterpreterError::OutOfPhlogistonsError),
//                 "the cause: {cause:?}"
//             );
//             assert_eq!(output_not_produced.len(), 1, "the reply is the output");
//         }
//         errors => panic!("unexpected errors: {errors:?}"),
//     }
//     assert!(played.out.is_empty(), "the reply does not appear");
//     let failed_calls: Vec<_> = played
//         .log
//         .iter()
//         .filter_map(|event| match event {
//             Event::IoEvent(IOEvent::Produce(produce)) if produce.failed => Some(produce),
//             _ => None,
//         })
//         .collect();
//     assert_eq!(failed_calls.len(), 1, "the call is marked failed");
//     assert!(failed_calls[0].output_value.is_empty());
//     runs.push((
//         format!("{:?}", played.result.errors),
//         played.root.clone(),
//         played.log.clone(),
//     ));
// }
// assert_eq!(runs[0], runs[1], "the rejection is deterministic");
// Added by decision 113: only a chromadb build registers `embed`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_out_of_phlo_rejects_output() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let replayed_with = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let term = format!(r#"new embed(`rho:vectordb:embed`) in {{ embed!("{OUT}", "hello") }}"#);
    let reply = ok(vec![vector32(&embedder.vector("hello")), string("mock-a")]);
    let full = native_play(&term, AMPLE_LIMIT).await;
    assert_played("embed full", &full);
    assert_eq!(full.out, vec![vec![reply.clone()]]);
    let used = full
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget")
        .used;
    let mut runs = Vec::with_capacity(2);
    for _ in 0..2 {
        let played = native_play(&term, used - 1).await;
        assert_eq!(
            played.result.errors,
            vec![InterpreterError::OutOfPhlogistonsError],
            "the deploy fails with the cause"
        );
        assert!(played.out.is_empty(), "the reply does not appear");
        assert!(
            played.log.iter().all(|event| !matches!(
                event,
                Event::IoEvent(IOEvent::Produce(produce)) if produce.failed
            )),
            "no produce copy is marked failed"
        );
        assert_eq!(
            produce_records(&played.log),
            vec![vec![reply.clone()]],
            "the trigger of the call keeps the record"
        );
        runs.push(played);
    }
    assert_eq!(
        (&runs[0].root, &runs[0].log),
        (&runs[1].root, &runs[1].log),
        "the rejection is deterministic"
    );
    lock.replace(services(&replayed_with, &FakeMirror::down()));
    assert_certifies("embed out of phlo", &runs[1], &term, used - 1).await;
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");
    drop(lock);
}

/// The record room of a deploy bounds the successes of `embed` (decision 68, DR-114 decision 5).
/// A success takes the encoded length of its reply and 64 bytes, and a failure takes nothing.
/// With the room of one reply, the first of two sequential calls succeeds and the second fails
/// with `EXT_OUTPUT_TOO_LARGE`. One byte less, and the first call fails already. A failure before
/// a success leaves the room of the success. Native replay with another embedder, which it never
/// asks, reproduces each play.
///
/// Changed by decision 105: the failures use the base codes `EXT_OUTPUT_TOO_LARGE` and
/// `EXT_BAD_ARG`. Before, they were the Reified codes `OutputTooLarge` and `BadArgument`.
// Added by decision 113: only a chromadb build registers `embed`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_record_room_bounds_embed_replies() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let replayed_with = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let reply = ok(vec![vector32(&embedder.vector("hello")), string("mock-a")]);
    let room = u64::try_from(reply.encoded_len()).expect("a small reply") + 64;
    let sequential = |first: &str| {
        format!(
            r#"new embed(`rho:vectordb:embed`) in {{
  for (@r <- embed!?({first})) {{
    @"{OUT}"!(["first", r]) |
    for (@s <- embed!?("hello")) {{ @"{OUT}"!(["second", s]) }}
  }}
}}"#
        )
    };
    // Changed by decision 105: the base code of a reply past the record room.
    // let too_large = failed(ServiceReplyCode::OutputTooLarge);
    let too_large = failure(
        "vectordb_embed",
        "EXT_OUTPUT_TOO_LARGE",
        "external output exceeds the record room",
    );
    let cases = [
        (
            "one reply",
            sequential(r#""hello""#),
            room,
            reply.clone(),
            too_large.clone(),
        ),
        (
            "one byte less",
            sequential(r#""hello""#),
            room - 1,
            too_large.clone(),
            too_large,
        ),
        (
            "a failure first",
            sequential("42"),
            room,
            // failed(ServiceReplyCode::BadArgument),
            failure("vectordb_embed", "EXT_BAD_ARG", "invalid argument"),
            reply,
        ),
    ];
    let mut played_runs = Vec::with_capacity(cases.len());
    for (label, term, room, first, second) in cases {
        let played = native_play_with_room(&term, AMPLE_LIMIT, room).await;
        assert_played(label, &played);
        let expected: BTreeMap<String, Par> =
            [("first".to_string(), first), ("second".to_string(), second)].into();
        assert_eq!(replies(&played), expected, "{label}");
        played_runs.push((label, term, played));
    }
    let asked = embedder.calls();
    lock.replace(services(&replayed_with, &FakeMirror::down()));
    for (label, term, played) in &played_runs {
        assert_certifies(label, played, term, AMPLE_LIMIT).await;
    }
    assert_eq!(replayed_with.calls(), 0, "native replay asks no embedder");
    assert_eq!(
        embedder.calls(),
        asked,
        "native replay asks no embedder of play"
    );
    drop(lock);
}

/// The record room bounds the successes of `ann` too (decision 68). With no room, the advisory
/// answer of the mirror becomes `EXT_OUTPUT_TOO_LARGE`, and a call on a space that is not a
/// VectorDB space keeps its own failure. Native replay with a mirror that is down, which it never
/// asks, reproduces the play.
///
/// Changed by decisions 105 and 114: the failures use the base codes, and the answer carries no
/// watermark. Before, the failures were `OutputTooLarge` and `NotAVectorDbSpace`.
// Added by decision 113: only a chromadb build registers `ann`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_record_room_bounds_ann_replies() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)], 7);
    let mirror = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)]);
    let lock = backends::install(services(&embedder, &mirror)).await;
    let term = ann_program(&[
        ("vector", r#"*m, *mn, [1.0, 0.0], {"k": 1}"#.to_string()),
        ("nil", r#"Nil, *mn, [1.0, 0.0], {"k": 1}"#.to_string()),
    ]);
    let full = native_play(&term, AMPLE_LIMIT).await;
    assert_played("ann with the room of production", &full);
    assert_eq!(
        ann_replies(&full).get("vector"),
        // Some(&answer(&[neighbour(&[1; 32], 0.25)], 7)),
        Some(&answer(&[neighbour(&[1; 32], 0.25)])),
        "the room of production holds the answer"
    );
    let played = native_play_with_room(&term, AMPLE_LIMIT, 0).await;
    assert_played("ann without room", &played);
    // Changed by decision 105: the base codes of the two failures.
    // let expected: BTreeMap<String, Par> = [
    //     (
    //         "vector".to_string(),
    //         failed(ServiceReplyCode::OutputTooLarge),
    //     ),
    //     (
    //         "nil".to_string(),
    //         failed(ServiceReplyCode::NotAVectorDbSpace),
    //     ),
    // ]
    // .into();
    let expected: BTreeMap<String, Par> = [
        (
            "vector".to_string(),
            failure(
                "vectordb_ann",
                "EXT_OUTPUT_TOO_LARGE",
                "external output exceeds the record room",
            ),
        ),
        (
            "nil".to_string(),
            failure("vectordb_ann", "EXT_BAD_ARG", "invalid argument"),
        ),
    ]
    .into();
    assert_eq!(ann_replies(&played), expected);
    let replay_mirror = FakeMirror::down();
    lock.replace(services(&embedder, &replay_mirror));
    assert_certifies("ann without room", &played, &term, AMPLE_LIMIT).await;
    assert!(
        replay_mirror.queries().is_empty(),
        "native replay asks no mirror"
    );
    drop(lock);
}

/// Removes every record from `log`, on all copies: the produce events, the produces of each COMM
/// and the keys of its repetition counts. The copies stay equal, so the record placement check
/// of the trace binding accepts the log (DR-114, E1).
fn strip_records(log: &mut [Event]) {
    let clear = |produce: &mut Produce| {
        produce.output_value.clear();
        produce.is_deterministic = true;
    };
    for event in log.iter_mut() {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => clear(produce),
            Event::Comm(comm) => {
                comm.produces.iter_mut().for_each(clear);
                comm.times_repeated = std::mem::take(&mut comm.times_repeated)
                    .into_iter()
                    .map(|(mut key, count)| {
                        clear(&mut key);
                        (key, count)
                    })
                    .collect();
            }
            Event::IoEvent(IOEvent::Consume(_)) => {}
        }
    }
}

/// Native replay of a block that holds a call of `embed` without its record fails closed. The
/// handler stops before it reads anything (`native_replayed_reply`, which the driver of DR-114
/// shares, decisions 68 and 116), so the operations of play stay incomplete, and the
/// certification rejects the block.
// Added by decision 113: only a chromadb build registers `embed`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_replay_rejects_a_call_without_its_record() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let lock = backends::install(services(&embedder, &FakeMirror::down())).await;
    let term = embed_program(&[("one", quoted("hello"))]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("embed", &played);
    assert_eq!(
        produce_records(&played.log).len(),
        1,
        "play records the reply"
    );
    let (recording, journal) = committed_evidence(&played);
    let mut log = played.log.clone();
    strip_records(&mut log);
    let environment = replay_environment_from(
        played.history.clone(),
        log,
        AMPLE_LIMIT,
        &recording,
        journal,
    )
    .await
    .expect("the log without its records binds");
    let certification = finish_injected(
        environment,
        &term,
        played.random.clone(),
        played.injections.clone(),
    )
    .await;
    let outcome = format!("{:?}", certification.evaluation);
    assert!(
        outcome.contains("native replay has incomplete operations"),
        "{outcome}"
    );
    drop(lock);
}

// ─── ann ─────────────────────────────────────────────────────────────────────────────────────

/// A numeric query, a text query and each reply code of `ann`. The mirror receives the
/// collection and the channel digest of the call, and the reply drops an id that is not 32 bytes
/// long. Native replay reproduces every reply with a mirror that it never asks.
///
/// Changed by decisions 105 and 114: each failed check of play replies `EXT_BAD_ARG`, a failure
/// of a service replies `EXT_FAILED`, and the answer carries no watermark. Before, each check had
/// its own Reified code.
// Added by decision 113: only a chromadb build registers `ann`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_ann_replies_and_codes() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    let first = [1; 32];
    let second = [2; 32];
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(
    //     vec![
    //         neighbour(&first, 0.25),
    //         neighbour(&second, 0.5),
    //         neighbour(&[3; 3], 0.75),
    //     ],
    //     7,
    // );
    let mirror = FakeMirror::new(vec![
        neighbour(&first, 0.25),
        neighbour(&second, 0.5),
        neighbour(&[3; 3], 0.75),
    ]);
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
    // Changed by decisions 105 and 114: the base codes, and no watermark.
    // let expected: BTreeMap<String, Par> = [
    //     (
    //         "vector",
    //         answer(&[neighbour(&first, 0.25), neighbour(&second, 0.5)], 7),
    //     ),
    //     ("text", answer(&[neighbour(&first, 0.25)], 7)),
    //     ("nil", failed(ServiceReplyCode::NotAVectorDbSpace)),
    //     ("queue", failed(ServiceReplyCode::NotAVectorDbSpace)),
    //     ("unmirrored", failed(ServiceReplyCode::MirrorDisabled)),
    //     ("dimensions", failed(ServiceReplyCode::DimensionMismatch)),
    //     ("not-numbers", failed(ServiceReplyCode::BadArgument)),
    //     ("beyond-f32", failed(ServiceReplyCode::BadArgument)),
    //     ("k-zero", failed(ServiceReplyCode::InvalidOption)),
    //     ("k-over", failed(ServiceReplyCode::InvalidOption)),
    //     ("extra-key", failed(ServiceReplyCode::InvalidOption)),
    //     ("not-a-map", failed(ServiceReplyCode::InvalidOption)),
    // ]
    // .into_iter()
    // .map(|(label, reply)| (label.to_string(), reply))
    // .collect();
    let bad_argument = failure("vectordb_ann", "EXT_BAD_ARG", "invalid argument");
    let expected: BTreeMap<String, Par> = [
        (
            "vector",
            answer(&[neighbour(&first, 0.25), neighbour(&second, 0.5)]),
        ),
        ("text", answer(&[neighbour(&first, 0.25)])),
        ("nil", bad_argument.clone()),
        ("queue", bad_argument.clone()),
        ("unmirrored", bad_argument.clone()),
        ("dimensions", bad_argument.clone()),
        ("not-numbers", bad_argument.clone()),
        ("beyond-f32", bad_argument.clone()),
        ("k-zero", bad_argument.clone()),
        ("k-over", bad_argument.clone()),
        ("extra-key", bad_argument.clone()),
        ("not-a-map", bad_argument),
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
    // let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)]);
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
    // Changed by decision 105: a failure of a service replies `EXT_FAILED`.
    // assert_eq!(
    //     found.get("text-dimensions"),
    //     Some(&failed(ServiceReplyCode::DimensionMismatch))
    // );
    // assert_eq!(
    //     found.get("outage"),
    //     Some(&failed(ServiceReplyCode::MirrorUnavailable))
    // );
    let service_failed = failure("vectordb_ann", "EXT_FAILED", "external service failed");
    assert_eq!(found.get("text-dimensions"), Some(&service_failed));
    assert_eq!(found.get("outage"), Some(&service_failed));
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("ann services", &played, &term, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
}

/// Native replay reproduces the advisory answers and a recorded outage with another mirror and
/// another embedder, which it never asks.
///
/// Changed by decisions 105 and 114: the outage replies `EXT_FAILED`, and the answers carry no
/// watermark. Before, the outage replied `MirrorUnavailable`.
// Added by decision 113: only a chromadb build registers `ann`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_ann_replay_equal() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let played_with = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)], 7);
    let played_with = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)]);
    let lock = backends::install(services(&embedder, &played_with)).await;
    let term = ann_program(&[
        ("vector", r#"*m, *mn, [1.0, 0.0], {"k": 1}"#.to_string()),
        ("text", r#"*m, *mn, "hello", {"k": 1}"#.to_string()),
    ]);
    let played = native_play(&term, AMPLE_LIMIT).await;
    assert_played("ann replay", &played);
    let expected: BTreeMap<String, Par> = [
        // ("vector", answer(&[neighbour(&[1; 32], 0.25)], 7)),
        // ("text", answer(&[neighbour(&[1; 32], 0.25)], 7)),
        ("vector", answer(&[neighbour(&[1; 32], 0.25)])),
        ("text", answer(&[neighbour(&[1; 32], 0.25)])),
    ]
    .into_iter()
    .map(|(label, reply)| (label.to_string(), reply))
    .collect();
    assert_eq!(ann_replies(&played), expected);
    let replayed_embedder = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
    // let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)]);
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
        // Some(&failed(ServiceReplyCode::MirrorUnavailable))
        Some(&failure(
            "vectordb_ann",
            "EXT_FAILED",
            "external service failed"
        ))
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
// Added by decision 113: only a chromadb build registers `ann`.
#[cfg(feature = "chromadb")]
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
    // Changed by decision 114: the mirror answers no watermark.
    // lock.replace(services(
    //     &embedder,
    //     &FakeMirror::new(
    //         vec![
    //             neighbour(&id_of(&[1.0, 1.0]), 0.25),
    //             neighbour(&stale, 0.375),
    //             neighbour(&id_of(&[1.0, 0.0]), 0.5),
    //         ],
    //         3,
    //     ),
    // ));
    lock.replace(services(
        &embedder,
        &FakeMirror::new(vec![
            neighbour(&id_of(&[1.0, 1.0]), 0.25),
            neighbour(&stale, 0.375),
            neighbour(&id_of(&[1.0, 0.0]), 0.5),
        ]),
    ));
    // Changed by decision 114: the reply of `ann` is `[true, ids, distances]`, with no watermark.
    // Before, the arm of the match read:
    //         [true, ids, _, _] => {{ for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) {{ @"{OUT}"!(x) }} }}
    let query = format!(
        r#"new ann(`rho:vectordb:ann`) in {{
  for (m <<- @"rrs-ids-handle") {{ for (mn <<- @"rrs-ids-name") {{
    for (@r <- ann!?(*m, *mn, [1.0, 0.0], {{"k": 3}})) {{
      match r {{
        [true, ids, _] => {{ for (@x <- mn ~ ([1.0, 0.0]; ids=ids, k=3)) {{ @"{OUT}"!(x) }} }}
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
    // let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)], 11);
    let replayed_with = FakeMirror::new(vec![neighbour(&[9; 32], 0.125)]);
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("ids query", &played, &query, AMPLE_LIMIT).await;
    assert!(
        replayed_with.queries().is_empty(),
        "native replay asks no mirror"
    );
}

/// A call with params aborts with `ParamsUnsupported` (I-8), and a persistent call fails at its
/// fire under base rule P2 (`reduce.rs`, DR-114), before any service call.
///
/// Changed by decision 106: the persistent sends fail as every recorded process of the base
/// fails. Before, rule A21 aborted them with `PersistentOpCall`.
// Added by decision 113: only a chromadb build registers `embed` and `ann`.
#[cfg(feature = "chromadb")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vectordb_embed_and_ann_reject_params_and_persistent_sends() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)], 7);
    let mirror = FakeMirror::new(vec![neighbour(&[1; 32], 0.25)]);
    let _lock = backends::install(services(&embedder, &mirror)).await;
    let persistent_send =
        r#"SystemProcessShapeError("a persistent send to a recorded system process")"#;
    for (label, source, code) in [
        (
            "embed-params",
            r#"new embed(`rho:vectordb:embed`), r in { embed!(*r, "hello"; k=1) }"#,
            "ParamsUnsupported",
        ),
        (
            "embed-persistent",
            r#"new embed(`rho:vectordb:embed`), r in { embed!!(*r, "hello") }"#,
            // Changed by decision 106: base rule P2 fails the send at its fire.
            // "PersistentOpCall",
            persistent_send,
        ),
        (
            "ann-params",
            r#"new ann(`rho:vectordb:ann`), r in { ann!(*r, Nil, Nil, [1.0, 0.0], {"k": 1}; k=1) }"#,
            "ParamsUnsupported",
        ),
        (
            "ann-persistent",
            r#"new ann(`rho:vectordb:ann`), r in { ann!!(*r, Nil, Nil, [1.0, 0.0], {"k": 1}) }"#,
            // Changed by decision 106: base rule P2 fails the send at its fire.
            // "PersistentOpCall",
            persistent_send,
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

/// Added by decision 98: one batch that the writer applied, as the mirror logged its calls. The
/// upserts and the deletes are empty when the batch held none.
#[derive(Clone, Debug, PartialEq)]
struct AppliedBatch {
    collection: String,
    metric: MirrorMetric,
    upserts: Vec<MirrorRecord>,
    deletes: Vec<[u8; 32]>,
}

/// Added by decision 98: the batches of the logged write calls `calls` of a mirror that applies
/// each write. Each batch starts with its `Ensure`, and an `Upsert` and then a `Delete` of the
/// same collection can follow (`apply` in `vectordb_services.rs`).
fn applied_batches(calls: &[MirrorCall]) -> Vec<AppliedBatch> {
    let mut batches: Vec<AppliedBatch> = Vec::with_capacity(calls.len());
    for call in calls {
        match (call, batches.last_mut()) {
            (MirrorCall::Ensure { collection, metric }, _) => batches.push(AppliedBatch {
                collection: collection.clone(),
                metric: *metric,
                upserts: Vec::new(),
                deletes: Vec::new(),
            }),
            (
                MirrorCall::Upsert {
                    collection,
                    records,
                },
                Some(batch),
            ) if *collection == batch.collection
                && batch.upserts.is_empty()
                && batch.deletes.is_empty() =>
            {
                batch.upserts = records.clone()
            }
            (MirrorCall::Delete { collection, ids }, Some(batch))
                if *collection == batch.collection && batch.deletes.is_empty() =>
            {
                batch.deletes = ids.clone()
            }
            (call, _) => panic!("a write call outside the order of a batch: {call:?} in {calls:?}"),
        }
    }
    batches
}

/// A successful deploy publishes the net writes of each commit of its mirrored space: one batch
/// for each stored record with a vector, which upserts its id, embedding, channel digest,
/// persistence and document. A record without a vector and the records of an unmirrored space
/// are not mirrored. Native replay certifies the play.
///
/// Changed by decision 98: each commit publishes its own batch. Before, the deploy published one
/// batch with all its upserts after its final result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_publishes_a_successful_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    // Changed by decision 98: each store commits alone, so each publishes its own batch, in
    // commit order.
    // assert_eq!(mirror.calls(), vec![
    //     MirrorCall::Ensure {
    //         collection: collection.clone(),
    //         metric: MirrorMetric::Cosine,
    //     },
    //     MirrorCall::Upsert {
    //         collection: collection.clone(),
    //         records: records.values().cloned().collect(),
    //     },
    // ]);
    let batches = applied_batches(&mirror.calls());
    assert_eq!(
        batches.len(),
        3,
        "one batch for each store with a vector: {batches:?}"
    );
    let mut published = BTreeMap::new();
    for batch in batches {
        assert_eq!(
            (batch.collection.as_str(), batch.metric),
            (collection.as_str(), MirrorMetric::Cosine),
            "the collection of the mirrored space"
        );
        assert!(batch.deletes.is_empty(), "a store deletes nothing");
        let [record] = <[MirrorRecord; 1]>::try_from(batch.upserts)
            .expect("each batch upserts the record of its store");
        published.insert(record.id, record);
    }
    assert_eq!(published, records, "the batches upsert each record once");
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
/// commits, so the store has published the upsert of the record when `tail` runs.
///
/// Changed by decision 98: the commit of the store publishes the upsert. Before, the journal
/// held the upsert until the final result.
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

/// A deploy that fails publishes the commits before its failure: a space error and an
/// out-of-phlo deploy each publish the upsert of their store, and nothing deletes it after the
/// revert. The same program with an ample budget publishes the same upsert. The phlo limit of the
/// out-of-phlo deploy is one unit below the usage of the control, so its failure comes after the
/// store has committed.
///
/// Changed by decision 98 (DR-114 oracle mode): a play publishes at each commit, whatever the
/// result of its deploy. Before, a failed deploy dropped its journal and published nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Renamed by decision 98: a failed deploy publishes its commits.
// async fn mirror_journal_drops_failed_deploy() {
async fn mirror_publishes_the_commits_of_a_failed_deploy() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let control_mirror = FakeMirror::new(Vec::new(), 0);
    let control_mirror = FakeMirror::new(Vec::new());
    let lock = backends::install(services(&embedder, &control_mirror)).await;
    let term = store_then(&send_peeked());
    let control = native_play(&term, AMPLE_LIMIT).await;
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

    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    backends::writer().flush().await;
    // Changed by decision 98: each failed deploy publishes the commit of its store.
    // assert_eq!(
    //     mirror.calls(),
    //     Vec::new(),
    //     "a failed deploy publishes nothing"
    // );
    let batches = applied_batches(&mirror.calls());
    assert_eq!(
        batches.len(),
        2,
        "the space error and then the out-of-phlo deploy publish their store: {batches:?}"
    );
    for batch in &batches {
        assert_eq!(
            (batch.collection.as_str(), batch.metric),
            (collection.as_str(), MirrorMetric::Cosine),
            "the collection of the space of the program"
        );
        assert_eq!(batch.upserts.len(), 1, "the upsert of the store");
        assert!(batch.deletes.is_empty(), "the revert deletes nothing");
    }
}

/// Rule E coalesces in each commit: a peek removes its record and stores it again in one
/// transaction, so it writes nothing. A consume of a non-persistent record deletes it, and a
/// persistent record that a receive matches stays. Native replay certifies both deploys.
///
/// Changed by decision 98: the net writes of each commit coalesce. Before, the journal of the
/// deploy coalesced them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mirror_rule_e_coalesces() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
/// succeed at once, and a batch that finds the queue full is dropped. With a mirror that refuses
/// every write, the batch fails after its retries, and the writer drops it too.
///
/// Changed by decision 114: nothing repairs the mirror, so the writer drops a batch and keeps no
/// dirty mark. Before, each such batch marked its collection dirty for the reconciler.
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
    // Retired by decision 114: the writer keeps no dirty marks.
    // assert_eq!(
    //     writer.dirty(),
    //     BTreeSet::from([collection.clone()]),
    //     "a batch found the queue full"
    // );
    stalled.release();
    writer.flush().await;
    // Changed by decision 114: the stalled task holds one batch and the queue one more, so the
    // third store is dropped.
    // assert!(!stalled
    //     .collection(&collection)
    //     .expect("the first batch reaches the mirror")
    //     .records
    //     .is_empty());
    let held = stalled
        .collection(&collection)
        .expect("the first batch reaches the mirror")
        .records;
    assert!(!held.is_empty(), "the first batch reaches the mirror");
    assert!(
        held.len() < 3,
        "a batch that finds the queue full is dropped: {held:?}"
    );
    // Retired by decision 114: the writer keeps no dirty marks.
    // assert_eq!(
    //     writer.take_dirty().into_keys().collect::<BTreeSet<_>>(),
    //     BTreeSet::from([collection])
    // );

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
    // Changed by decision 114: the writer drops the batch after its attempts.
    // assert_eq!(writer.dirty(), BTreeSet::from([collection]));
    assert_eq!(
        down.collection(&collection),
        None,
        "the dropped batch leaves no collection"
    );
}

/// An exploratory deploy publishes the commit of its store, and its writes do not reach the
/// state of the next deploy. The dry run reverts its record, but the mirror keeps it, an orphan
/// that the next deploy does not publish again.
///
/// Changed by decision 90 (C6): Casper runs an exploratory deploy as a native dry run, which
/// reverts its writes (DR-134 of the base, `dry_run.rs`). Before, the test injected the term
/// through `RhoRuntime::inj`, whose journal the next root evaluation dropped (FR-5').
///
/// Changed by decision 98 (DR-114 oracle mode): a dry run is a play, so it publishes at each
/// commit. Before, an exploratory deploy published nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Renamed by decision 98: a dry run publishes its commits.
// async fn mirror_exploratory_evaluation_publishes_nothing() {
async fn mirror_exploratory_evaluation_publishes_its_commits() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    // Added by decision 98: the batch of the dry run names the collection too.
    let collection = collection_name(&space.sid);
    chain
        .explore(r#"for (_, mn <<- @"rrs-explore-names") { mn!([1.0, 0.0]) }"#)
        .await
        .expect("the exploratory evaluation runs");
    // Changed by decision 90 (C6): the dry run reverts its record.
    // assert_eq!(
    //     chain.data(&data_channel(space, &mn)).await.len(),
    //     1,
    //     "the exploratory evaluation stores its record"
    // );
    assert_eq!(
        chain.data(&data_channel(space, &mn)).await.len(),
        0,
        "the exploratory dry run reverts its record"
    );
    backends::writer().flush().await;
    // Changed by decision 98: the dry run publishes the commit of its store.
    // assert_eq!(
    //     mirror.calls(),
    //     Vec::new(),
    //     "an exploratory evaluation publishes nothing"
    // );
    let [explored] = <[AppliedBatch; 1]>::try_from(applied_batches(&mirror.calls()))
        .expect("the dry run publishes one batch");
    assert_eq!(
        (explored.collection.as_str(), explored.metric),
        (collection.as_str(), MirrorMetric::Cosine),
        "the collection of the space"
    );
    assert!(explored.deletes.is_empty(), "the revert deletes nothing");
    let [orphan] = <[MirrorRecord; 1]>::try_from(explored.upserts)
        .expect("the dry run upserts the record of its store");
    assert_eq!(
        orphan.embedding,
        vec![1.0, 0.0],
        "the record of the dry run"
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
    // Changed by decision 90 (C6): the state holds only the record of the deploy, because the
    // dry run reverted its own record.
    // assert_eq!(records.len(), 2, "both records are stored");
    assert_eq!(records.len(), 1, "only the record of the deploy is stored");
    records.retain(|_, record| record.embedding == vec![0.0, 1.0]);
    // Moved by decision 98: the batch of the dry run reads the collection above.
    // let collection = collection_name(&space.sid);
    // Changed by decision 98: the calls of the dry run come first.
    // assert_eq!(
    //     mirror.calls(),
    //     vec![
    //         MirrorCall::Ensure {
    //             collection: collection.clone(),
    //             metric: MirrorMetric::Cosine,
    //         },
    //         MirrorCall::Upsert {
    //             collection,
    //             records: records.into_values().collect(),
    //         },
    //     ],
    //     "the next deploy publishes only its own record"
    // );
    let [_, sent] = <[AppliedBatch; 2]>::try_from(applied_batches(&mirror.calls()))
        .expect("the dry run and then the deploy publish one batch each");
    assert_eq!(
        sent,
        AppliedBatch {
            collection: collection.clone(),
            metric: MirrorMetric::Cosine,
            upserts: records.values().cloned().collect(),
            deletes: Vec::new(),
        },
        "the next deploy publishes only its own record"
    );
    let held = mirror
        .collection(&collection)
        .expect("the mirror collection")
        .records;
    assert_eq!(held.len(), 2, "the mirror keeps the record of the dry run");
    assert_eq!(
        held.get(&orphan.id),
        Some(&orphan),
        "the record of the dry run is an orphan of the mirror"
    );
}

/// Each commit of a deploy publishes its own batch, in commit order: a record that one
/// transaction stores and a later transaction of the same deploy consumes gives an upsert and
/// then a delete, and leaves no record in the mirror. The peek fires only after the store
/// commits, and the consume only after the peek, so the three transactions run in this order.
/// The peek writes nothing (Rule E). Native replay certifies the deploy.
///
/// Changed by decision 98: each commit publishes its batch before its hold drops, so the
/// batches keep the commit order. Before, the journal of the deploy kept only the last write of
/// each record, and the deploy published one delete.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Renamed by decision 98: each commit publishes its own batch.
// async fn mirror_journal_keeps_the_last_write_of_a_deploy() {
async fn mirror_publishes_each_commit_of_a_deploy_in_order() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    // Changed by decision 98: the store and then the consume publish, in commit order.
    // assert!(
    //     matches!(
    //         calls.as_slice(),
    //         [MirrorCall::Ensure { .. }, MirrorCall::Delete { ids, .. }] if ids.len() == 1
    //     ),
    //     "the last write of the record is its delete: {calls:?}"
    // );
    let [stored, consumed] = <[AppliedBatch; 2]>::try_from(applied_batches(&calls))
        .expect("a batch for the store and then one for the consume");
    let [record] = <[MirrorRecord; 1]>::try_from(stored.upserts.clone())
        .expect("the store upserts its record");
    assert_eq!(
        stored,
        AppliedBatch {
            collection: collection.clone(),
            metric: MirrorMetric::Cosine,
            upserts: vec![record.clone()],
            deletes: Vec::new(),
        },
        "the first batch is the upsert of the store"
    );
    assert_eq!(
        consumed,
        AppliedBatch {
            collection: collection.clone(),
            metric: MirrorMetric::Cosine,
            upserts: Vec::new(),
            deletes: vec![record.id],
        },
        "the second batch is the delete of the consume"
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
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = FakeMirror::new(Vec::new(), 0);
    let mirror = FakeMirror::new(Vec::new());
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

/// Native replay certification publishes nothing to the mirror of the replaying node: after the
/// certification of two deploys, that mirror has no call and no collection. The mirror of play
/// got the writes of both deploys, with the delete of the consumed record among them.
///
/// Changed by decision 98 (DR-114 oracle mode): only a play calls a service, and replay delivers
/// the records. Before, native replay published its own journal, so the mirror of the replaying
/// node got the calls and the records of the mirror of play.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// Renamed by decision 98: replay publishes nothing.
// async fn mirror_replay_publishes_its_own_journal() {
async fn mirror_replay_publishes_nothing() {
    let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
    // Changed by decision 114: the mirror answers no watermark.
    // let played_with = FakeMirror::new(Vec::new(), 0);
    let played_with = FakeMirror::new(Vec::new());
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
    // let replayed_with = FakeMirror::new(Vec::new(), 0);
    let replayed_with = FakeMirror::new(Vec::new());
    lock.replace(services(&embedder, &replayed_with));
    assert_certifies("replay setup", &first, &setup, AMPLE_LIMIT).await;
    assert_certifies("replay change", &second, &change, AMPLE_LIMIT).await;
    backends::writer().flush().await;
    // Changed by decision 98: native replay publishes nothing.
    // assert_eq!(replayed_with.calls(), played_calls);
    // assert_eq!(
    //     replayed_with.collection(&collection),
    //     played_with.collection(&collection)
    // );
    assert_eq!(
        replayed_with.calls(),
        Vec::new(),
        "native replay publishes nothing"
    );
    assert_eq!(
        replayed_with.collection(&collection),
        None,
        "the mirror of the replaying node has no collection"
    );
}

// ─── reconciler (step 7c) ────────────────────────────────────────────────────────────────────

/// A deploy that makes a mirrored space with the records `[1.0, 0.0]` and `[0.0, 1.0]` on one
/// name, and sends the space and the name on `rrs-reconcile-names`.
///
/// Changed by decision 114: only the tracking property reads it now. Before, the reconciler
/// tests read it too.
fn reconcile_setup() -> String {
    format!(
        r#"new vdb(`{VDB_URN}`) in {{
  for (m <- vdb!?({MIRRORED})) {{ for (mn <- m!?("gensym")) {{
    @"rrs-reconcile-names"!(*m, *mn) | mn!([1.0, 0.0]) | mn!([0.0, 1.0])
  }} }}
}}"#
    )
}

// Retired by decision 114: the mirror is an oracle store, and nothing repairs it (File I/O,
// oracle mode). The tests of the reconciler and their helpers follow.
// /// The deploy after [`reconcile_setup`]: it consumes `[1.0, 0.0]` and stores `[0.6, 0.8]`.
// const RECONCILE_CHANGE: &str = r#"for (_, mn <<- @"rrs-reconcile-names") {
//   for (@x <- mn ~ ([1.0, 0.0])) { mn!([0.6, 0.8]) }
// }"#;
//
// /// The expected mirror records of every payload that the reconciler tests store.
// fn reconcile_expected() -> Expected {
//     [
//         (vec![vector64(&[1.0, 0.0])], (vec![1.0, 0.0], None, false)),
//         (vec![vector64(&[0.0, 1.0])], (vec![0.0, 1.0], None, false)),
//         (
//             vec![vector64(&[0.6, 0.8])],
//             (vec![0.6f64 as f32, 0.8f64 as f32], None, false),
//         ),
//     ]
//     .into_iter()
//     .map(|(pars, record)| (format!("{pars:?}"), record))
//     .collect()
// }
//
// /// A record that no state holds, on the channel `digest`: the write of a fork.
// fn orphan(digest: &str) -> MirrorRecord {
//     MirrorRecord {
//         id: [0xEE; 32],
//         embedding: vec![0.5, 0.5],
//         channel: digest.to_string(),
//         persistent: false,
//         document: None,
//     }
// }
//
// /// The reconciler repairs a collection to the finalized state: it deletes an orphan that a
// /// fork wrote, adds a record that a later removal took out, and sets the watermark. A second
// /// pass at the same block leaves the collection, and a pass whose writes fail keeps the
// /// watermark and the divergence for the next pass.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn mirror_reconcile_removes_orphans() {
//     let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//     let mirror = FakeMirror::new(Vec::new(), 0);
//     let _lock = backends::install(services(&embedder, &mirror)).await;
//     let mut chain = NativeChain::new().await;
//     let setup = reconcile_setup();
//     let played = chain.play(&setup, "rrs-reconcile-setup", AMPLE_LIMIT).await;
//     assert_played("reconcile setup", &played);
//     backends::writer().flush().await;
//     let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//         .expect("a handle and a name");
//     let space = native(&m);
//     let collection = collection_name(&space.sid);
//     let digest = channel_digest(&key(&mn));
//     let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
//     let held = mirror
//         .collection(&collection)
//         .expect("the mirror collection");
//     assert_eq!(held.records, records, "the journal published both records");
//     assert_eq!(
//         held.space,
//         Some(space.sid),
//         "the journal registered the space"
//     );
//     assert_eq!(
//         held.channels,
//         BTreeMap::from([(digest.clone(), key(&mn).as_bytes().to_vec())]),
//         "the journal registered the channel"
//     );
//
//     let first = *records.keys().next().expect("a record");
//     let mut diverged = held.clone();
//     diverged.records.remove(&first);
//     diverged.records.insert(orphan(&digest).id, orphan(&digest));
//     mirror.put(&collection, diverged);
//     let state = chain.finalized_state();
//     let block = Finalized {
//         block_number: 1,
//         root: played.root.clone(),
//     };
//     let report = reconcile(&current_services(), state.as_ref(), &block).await;
//     assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
//     assert_eq!((report.deleted, report.added), (1, 1));
//     let repaired = mirror
//         .collection(&collection)
//         .expect("the mirror collection");
//     assert_eq!(
//         repaired.records, records,
//         "the mirror holds the finalized records"
//     );
//     assert_eq!(repaired.watermark, 1);
//
//     let report = reconcile(&current_services(), state.as_ref(), &block).await;
//     assert_eq!(report.current, BTreeSet::from([collection.clone()]));
//     assert!(report.repaired.is_empty() && report.failed.is_empty());
//
//     let mut diverged = repaired.clone();
//     diverged.records.insert(orphan(&digest).id, orphan(&digest));
//     mirror.put(&collection, diverged.clone());
//     mirror.set_writes(WriteMode::Refuse);
//     let next = Finalized {
//         block_number: 2,
//         root: played.root.clone(),
//     };
//     let report = reconcile(&current_services(), state.as_ref(), &next).await;
//     assert_eq!(report.failed, BTreeSet::from([collection.clone()]));
//     assert_eq!(
//         mirror.collection(&collection),
//         Some(diverged),
//         "a failed pass keeps the watermark and the orphan"
//     );
// }
//
// /// After a restart, the writer and its memory are gone, and the mirror keeps its registry. The
// /// reconciler rescans every registered channel: a mirror that lost every record of a collection
// /// gets them back from the finalized state.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn mirror_restart_rescans_reports() {
//     let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//     let mirror = FakeMirror::new(Vec::new(), 0);
//     let lock = backends::install(services(&embedder, &mirror)).await;
//     let mut chain = NativeChain::new().await;
//     let played = chain
//         .play(&reconcile_setup(), "rrs-reconcile-setup", AMPLE_LIMIT)
//         .await;
//     assert_played("restart setup", &played);
//     backends::writer().flush().await;
//     let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//         .expect("a handle and a name");
//     let space = native(&m);
//     let collection = collection_name(&space.sid);
//     let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
//     let mut lost = mirror
//         .collection(&collection)
//         .expect("the mirror collection");
//     lost.records.clear();
//     mirror.put(&collection, lost);
//     lock.replace(services(&embedder, &mirror));
//     let state = chain.finalized_state();
//     let block = Finalized {
//         block_number: 3,
//         root: played.root.clone(),
//     };
//     let report = reconcile(&current_services(), state.as_ref(), &block).await;
//     assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
//     assert_eq!((report.deleted, report.added), (0, 2));
//     let repaired = mirror
//         .collection(&collection)
//         .expect("the mirror collection");
//     assert_eq!(repaired.records, records);
//     assert_eq!(repaired.watermark, 3);
// }
//
// /// A batch that fails leaves its collection dirty with its channels. The next pass repairs the
// /// collection even at its watermark, from the state that holds the new record.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn mirror_reconcile_repairs_a_dirty_collection() {
//     let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//     let mirror = FakeMirror::new(Vec::new(), 0);
//     let _lock = backends::install(services(&embedder, &mirror)).await;
//     let mut chain = NativeChain::new().await;
//     let setup = reconcile_setup();
//     let first = chain.play(&setup, "rrs-reconcile-setup", AMPLE_LIMIT).await;
//     assert_played("dirty setup", &first);
//     let writer = backends::writer();
//     writer.flush().await;
//     let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//         .expect("a handle and a name");
//     let space = native(&m);
//     let collection = collection_name(&space.sid);
//     let state = chain.finalized_state();
//     let block = |root: &Blake2b256Hash| Finalized {
//         block_number: 1,
//         root: root.clone(),
//     };
//     let report = reconcile(&current_services(), state.as_ref(), &block(&first.root)).await;
//     assert_eq!(report.repaired, BTreeSet::from([collection.clone()]));
//
//     mirror.set_writes(WriteMode::Refuse);
//     let second = chain
//         .play(RECONCILE_CHANGE, "rrs-reconcile-change", AMPLE_LIMIT)
//         .await;
//     assert_played("dirty change", &second);
//     writer.flush().await;
//     assert_eq!(writer.dirty(), BTreeSet::from([collection.clone()]));
//     mirror.set_writes(WriteMode::Apply);
//     let report = reconcile(&current_services(), state.as_ref(), &block(&second.root)).await;
//     assert_eq!(
//         report.repaired,
//         BTreeSet::from([collection.clone()]),
//         "a dirty collection is repaired at its watermark"
//     );
//     assert_eq!((report.deleted, report.added), (1, 1));
//     assert!(
//         writer.dirty().is_empty(),
//         "the repair clears the dirty mark"
//     );
//     let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
//     assert_eq!(
//         mirror
//             .collection(&collection)
//             .expect("the mirror collection")
//             .records,
//         records
//     );
// }
//
// /// The reconciler of a node follows its `BlockFinalised` events: it maps each block to its
// /// post-state root, and the passes reach the newest finalized block.
// #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// async fn mirror_reconciler_follows_the_finalized_blocks() {
//     let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//     let mirror = FakeMirror::new(Vec::new(), 0);
//     let _lock = backends::install(services(&embedder, &mirror)).await;
//     let mut chain = NativeChain::new().await;
//     let first = chain
//         .play(&reconcile_setup(), "rrs-reconcile-setup", AMPLE_LIMIT)
//         .await;
//     assert_played("events setup", &first);
//     let second = chain
//         .play(RECONCILE_CHANGE, "rrs-reconcile-change", AMPLE_LIMIT)
//         .await;
//     assert_played("events change", &second);
//     backends::writer().flush().await;
//     let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//         .expect("a handle and a name");
//     let space = native(&m);
//     let collection = collection_name(&space.sid);
//     let mut lost = mirror
//         .collection(&collection)
//         .expect("the mirror collection");
//     lost.records.clear();
//     mirror.put(&collection, lost);
//
//     let state = chain.finalized_state();
//     state.finalize(&[1; 32], &first.root);
//     state.finalize(&[2; 32], &second.root);
//     let events = F1r3flyEvents::new();
//     mirror_reconciler::start(&events, state.clone());
//     for (hash, number) in [([1u8; 32], 1), ([2u8; 32], 2), ([9u8; 32], 9)] {
//         events
//             .publish(F1r3flyEvent::block_finalised(
//                 hex::encode(hash),
//                 number,
//                 0,
//                 Vec::new(),
//                 Vec::new(),
//                 Vec::new(),
//                 String::new(),
//                 0,
//             ))
//             .expect("the event is published");
//     }
//     let reached = tokio::time::timeout(CERTIFY_TIMEOUT, async {
//         loop {
//             let held = mirror
//                 .collection(&collection)
//                 .expect("the mirror collection");
//             if held.watermark == 2 {
//                 return held;
//             }
//             tokio::time::sleep(Duration::from_millis(20)).await;
//         }
//     })
//     .await
//     .expect("the passes reach the newest block that the node holds");
//     let records = mirror_records(&chain, space, &mn, &reconcile_expected()).await;
//     assert_eq!(
//         reached.records, records,
//         "the mirror holds the state of block 2"
//     );
// }

// ─── properties (step 8d) ────────────────────────────────────────────────────────────────────

/// The vectors that the generated chains store and query.
const VECTORS: [[f64; 2]; 4] = [[1.0, 0.0], [0.0, 1.0], [0.6, 0.8], [0.8, 0.6]];

/// The vector `VECTORS[index]` as Rholang source.
fn vector_source(index: usize) -> String {
    let [a, b] = VECTORS[index];
    format!("[{a:?}, {b:?}]")
}

// Retired by decision 114: only the retired repair property read the expected records.
// /// The expected mirror record of each vector of [`VECTORS`] and of the two records of
// /// [`reconcile_setup`].
// fn vectors_expected() -> Expected {
//     VECTORS
//         .iter()
//         .map(|[a, b]| {
//             (
//                 format!("{:?}", vec![vector64(&[*a, *b])]),
//                 (vec![*a as f32, *b as f32], None, false),
//             )
//         })
//         .collect()
// }

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
    /// An exploratory deploy that stores `VECTORS[v]`: a native dry run, which reverts its
    /// writes (decision 90, C6). Before, it was an evaluation through `RhoRuntime::inj`.
    /// Changed by decision 98: the commit of its store publishes the record, and the mirror
    /// keeps it after the revert.
    Explore(usize),
}

// Changed by decision 114: only the tracking property generates steps, and it always generates
// exploratory deploys. Before, the retired repair property generated chains without them.
// fn chain_step(explore: bool) -> BoxedStrategy<ChainStep> {
//     let index = 0..VECTORS.len();
//     match explore {
//         true => prop_oneof![
//             3 => index.clone().prop_map(ChainStep::Store),
//             2 => index.clone().prop_map(ChainStep::Take),
//             1 => Just(ChainStep::Peek),
//             1 => index.clone().prop_map(ChainStep::Fail),
//             1 => index.prop_map(ChainStep::Explore),
//         ]
//         .boxed(),
//         false => prop_oneof![
//             3 => index.clone().prop_map(ChainStep::Store),
//             2 => index.clone().prop_map(ChainStep::Take),
//             1 => Just(ChainStep::Peek),
//             1 => index.prop_map(ChainStep::Fail),
//         ]
//         .boxed(),
//     }
// }
fn chain_step() -> BoxedStrategy<ChainStep> {
    let index = 0..VECTORS.len();
    prop_oneof![
        3 => index.clone().prop_map(ChainStep::Store),
        2 => index.clone().prop_map(ChainStep::Take),
        1 => Just(ChainStep::Peek),
        1 => index.clone().prop_map(ChainStep::Fail),
        1 => index.prop_map(ChainStep::Explore),
    ]
    .boxed()
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

// Changed by decision 98 (DR-114 oracle mode): a play publishes at each commit, whatever the
// result of its deploy, and replay publishes nothing. The property that checked the publication
// of the successful deploys only (PublishOnlyOnSuccess) follows.
// /// The mirror follows the published deploys and nothing else (ChromaMirror.tla,
// /// PublishOnlyOnSuccess and NetWritesExact). After each step of a generated chain, the mirror
// /// holds the records that the state changes of the successful deploys give: a failed deploy
// /// publishes nothing, a peek writes nothing, a take deletes, and an exploratory evaluation
// /// publishes nothing. The test chain keeps the state changes of a failed deploy and of an
// /// exploratory evaluation (it reverts nothing, unlike the deploys of a block), so the oracle
// /// applies only the state changes of the successful deploys.
// #[test]
// fn prop_mirror_tracks_the_published_deploys() {
//     check(
//         runtime_config(),
//         proptest::collection::vec(chain_step(true), 1..6),
//         |steps| {
//             tokio_runtime().block_on(async {
//                 let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//                 let mirror = FakeMirror::new(Vec::new(), 0);
//                 let _lock = backends::install(services(&embedder, &mirror)).await;
//                 let mut chain = NativeChain::new().await;
//                 let played = chain
//                     .play(&reconcile_setup(), "rrs-track-setup", AMPLE_LIMIT)
//                     .await;
//                 assert_played("track setup", &played);
//                 let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//                     .expect("a handle and a name");
//                 let space = native(&m);
//                 let collection = collection_name(&space.sid);
//                 backends::writer().flush().await;
//                 let mut expected = state_ids(&chain, space, &mn).await;
//                 for (index, step) in steps.iter().enumerate() {
//                     let before = state_ids(&chain, space, &mn).await;
//                     let published = match step {
//                         ChainStep::Explore(_) => {
//                             // An exploratory evaluation publishes nothing, also when it fails (a
//                             // full collection refuses its store).
//                             if let Err(error) = chain.explore(&step_source(*step)).await {
//                                 drop(error);
//                             }
//                             false
//                         }
//                         _ => {
//                             let label = format!("rrs-track-{index}");
//                             let played = chain.play(&step_source(*step), &label, AMPLE_LIMIT).await;
//                             played.result.errors.is_empty()
//                         }
//                     };
//                     backends::writer().flush().await;
//                     let after = state_ids(&chain, space, &mn).await;
//                     if published {
//                         expected.extend(after.difference(&before).copied());
//                         expected.retain(|id| after.contains(id) || !before.contains(id));
//                     }
//                     let held = mirror_ids(&mirror, &collection);
//                     prop_assert_eq!(held, expected.clone(), "step {} {:?}", index, step);
//                 }
//                 Ok(())
//             })
//         },
//     );
// }

/// The mirror follows the writes of every play (ChromaMirror.tla: NetWritesExact,
/// ReplayNeverPublishes and OnePublicationPerCommit, rows W9, W13 and W14). After each step of a
/// generated chain, the new calls of the mirror are the batches of the commits of the step, and
/// each batch writes one record. In a played step, the upserts are the records that the step
/// added to the state, and the deletes are the records that it removed. So a peek writes
/// nothing, a take deletes, and a failed deploy publishes the commits before its failure. The
/// test chain keeps the state changes of a failed deploy, unlike the deploys of a block. An
/// exploratory dry run publishes the commit of its store and then reverts it, so the mirror
/// keeps a record that the state does not hold. Native replay of every play publishes nothing.
///
/// Changed by decision 90 (C6): an exploratory deploy is a native dry run, which reverts its
/// writes. Before, the test chain kept the state changes of an exploratory evaluation too.
///
/// Changed by decision 98: it replaces `prop_mirror_tracks_the_published_deploys`, which
/// checked that only a successful deploy published (PublishOnlyOnSuccess).
#[test]
fn prop_mirror_tracks_the_writes_of_every_play() {
    check(
        runtime_config(),
        proptest::collection::vec(chain_step(), 1..6),
        |steps| {
            tokio_runtime().block_on(async {
                let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
                let mirror = FakeMirror::new(Vec::new());
                let lock = backends::install(services(&embedder, &mirror)).await;
                let mut chain = NativeChain::new().await;
                let setup = reconcile_setup();
                let played = chain.play(&setup, "rrs-track-setup", AMPLE_LIMIT).await;
                assert_played("track setup", &played);
                let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
                    .expect("a handle and a name");
                let space = native(&m);
                let collection = collection_name(&space.sid);
                backends::writer().flush().await;
                let mut expected = state_ids(&chain, space, &mn).await;
                prop_assert_eq!(
                    mirror_ids(&mirror, &collection),
                    expected.clone(),
                    "the setup publishes its records"
                );
                let mut plays = Vec::with_capacity(steps.len() + 1);
                plays.push(("track setup".to_string(), setup, played));
                for (index, step) in steps.iter().enumerate() {
                    let before = state_ids(&chain, space, &mn).await;
                    let seen = mirror.calls().len();
                    let term = step_source(*step);
                    match step {
                        // A dry run fails when a full collection refuses its store.
                        ChainStep::Explore(_) => {
                            if let Err(error) = chain.explore(&term).await {
                                drop(error);
                            }
                        }
                        ChainStep::Store(_)
                        | ChainStep::Take(_)
                        | ChainStep::Peek
                        | ChainStep::Fail(_) => {
                            let label = format!("rrs-track-{index}");
                            let played = chain.play(&term, &label, AMPLE_LIMIT).await;
                            plays.push((label, term, played));
                        }
                    }
                    backends::writer().flush().await;
                    let after = state_ids(&chain, space, &mn).await;
                    let mut upserted = BTreeSet::new();
                    let mut deleted = BTreeSet::new();
                    for batch in applied_batches(&mirror.calls()[seen..]) {
                        prop_assert_eq!(
                            (batch.collection.as_str(), batch.metric),
                            (collection.as_str(), MirrorMetric::Cosine),
                            "step {} {:?}: the collection of the space",
                            index,
                            step
                        );
                        prop_assert_eq!(
                            batch.upserts.len() + batch.deletes.len(),
                            1,
                            "step {} {:?}: each commit writes one record",
                            index,
                            step
                        );
                        for record in &batch.upserts {
                            upserted.insert(record.id);
                            expected.insert(record.id);
                        }
                        for id in &batch.deletes {
                            deleted.insert(*id);
                            expected.remove(id);
                        }
                    }
                    match step {
                        ChainStep::Explore(_) => {
                            prop_assert_eq!(
                                &after,
                                &before,
                                "step {} {:?}: the dry run reverts its writes",
                                index,
                                step
                            );
                            prop_assert!(
                                deleted.is_empty(),
                                "step {} {:?}: a store deletes nothing",
                                index,
                                step
                            );
                            prop_assert!(
                                upserted.len() <= 1 && upserted.is_disjoint(&after),
                                "step {} {:?}: the dry run upserts at most its own record",
                                index,
                                step
                            );
                        }
                        ChainStep::Store(_)
                        | ChainStep::Take(_)
                        | ChainStep::Peek
                        | ChainStep::Fail(_) => {
                            prop_assert_eq!(
                                &upserted,
                                &after.difference(&before).copied().collect::<BTreeSet<_>>(),
                                "step {} {:?}: the upserts are the added records",
                                index,
                                step
                            );
                            prop_assert_eq!(
                                &deleted,
                                &before.difference(&after).copied().collect::<BTreeSet<_>>(),
                                "step {} {:?}: the deletes are the removed records",
                                index,
                                step
                            );
                        }
                    }
                    prop_assert_eq!(
                        mirror_ids(&mirror, &collection),
                        expected.clone(),
                        "step {} {:?}",
                        index,
                        step
                    );
                }
                let replayed_with = FakeMirror::new(Vec::new());
                lock.replace(services(&embedder, &replayed_with));
                for (label, term, played) in &plays {
                    let certification = certified(label, played, term, AMPLE_LIMIT).await;
                    let root = played.result.errors.is_empty().then_some(&played.root);
                    assert_certified(label, &played.result, root, &certification);
                }
                backends::writer().flush().await;
                prop_assert_eq!(
                    replayed_with.calls(),
                    Vec::new(),
                    "native replay publishes nothing"
                );
                Ok(())
            })
        },
    );
}

// Retired by decision 114: nothing repairs the mirror, so no reconciler pass runs, and
// ChromaMirror.tla has no invariant ReconciledEqualsFinalizedView.
// /// A reconciler pass repairs the mirror to the finalized view (ChromaMirror.tla,
// /// ReconciledEqualsFinalizedView). After a generated chain, the test removes some records of
// /// the mirror and adds orphans on the registered channel. A pass whose writes fail keeps the
// /// mirror and the watermark; a pass that finishes holds exactly the records of the finalized
// /// state and sets the watermark.
// #[test]
// fn prop_mirror_reconcile_repairs_the_finalized_view() {
//     let strategy = (
//         proptest::collection::vec(chain_step(false), 0..4),
//         proptest::collection::vec(proptest::bool::ANY, 6),
//         0usize..3,
//         proptest::bool::ANY,
//     );
//     check(
//         runtime_config(),
//         strategy,
//         |(steps, keep, orphans, refuse_first)| {
//             tokio_runtime().block_on(async {
//                 let embedder = MockEmbedder::new("mock-a", 2, 0.5, EmbedMode::Vectors);
//                 let mirror = FakeMirror::new(Vec::new(), 0);
//                 let _lock = backends::install(services(&embedder, &mirror)).await;
//                 let mut chain = NativeChain::new().await;
//                 let mut played = chain
//                     .play(&reconcile_setup(), "rrs-repair-setup", AMPLE_LIMIT)
//                     .await;
//                 assert_played("repair setup", &played);
//                 let [m, mn] = <[Par; 2]>::try_from(handles(&chain, "rrs-reconcile-names").await)
//                     .expect("a handle and a name");
//                 let space = native(&m);
//                 let collection = collection_name(&space.sid);
//                 let digest = channel_digest(&key(&mn));
//                 for (index, step) in steps.iter().enumerate() {
//                     played = chain
//                         .play(
//                             &step_source(*step),
//                             &format!("rrs-repair-{index}"),
//                             AMPLE_LIMIT,
//                         )
//                         .await;
//                 }
//                 backends::writer().flush().await;
//                 let finalized = mirror_records(&chain, space, &mn, &vectors_expected()).await;
//                 let mut diverged = mirror
//                     .collection(&collection)
//                     .expect("the setup made the collection");
//                 let ids: Vec<[u8; 32]> = diverged.records.keys().copied().collect();
//                 for (id, kept) in ids.iter().zip(keep.iter().cycle()) {
//                     if !kept {
//                         diverged.records.remove(id);
//                     }
//                 }
//                 for orphan_index in 0..orphans {
//                     let mut record = orphan(&digest);
//                     record.id = [0xE0 + u8::try_from(orphan_index).expect("a small index"); 32];
//                     diverged.records.insert(record.id, record);
//                 }
//                 let watermark = diverged.watermark;
//                 mirror.put(&collection, diverged.clone());
//                 let state = chain.finalized_state();
//                 if refuse_first {
//                     mirror.set_writes(WriteMode::Refuse);
//                     let block = Finalized {
//                         block_number: 1,
//                         root: played.root.clone(),
//                     };
//                     reconcile(&current_services(), state.as_ref(), &block).await;
//                     let kept = mirror.collection(&collection).expect("the collection");
//                     // A refused write may leave the deletes or the upserts applied, never the
//                     // watermark (MirrorBackend::set_watermark is a write of the registry).
//                     prop_assert_eq!(
//                         kept.watermark,
//                         watermark,
//                         "a failed pass keeps the watermark"
//                     );
//                     mirror.set_writes(WriteMode::Apply);
//                 }
//                 let block = Finalized {
//                     block_number: 2,
//                     root: played.root.clone(),
//                 };
//                 reconcile(&current_services(), state.as_ref(), &block).await;
//                 let repaired = mirror.collection(&collection).expect("the collection");
//                 prop_assert_eq!(
//                     &repaired.records,
//                     &finalized,
//                     "the mirror holds the finalized view"
//                 );
//                 prop_assert_eq!(repaired.watermark, 2);
//                 Ok(())
//             })
//         },
//     );
// }

/// A generated call of `embed` or `ann`.
#[derive(Clone, Debug)]
enum ServiceCall {
    /// `embed` of the texts.
    Embed(Vec<String>),
    /// `ann` of the vector `VECTORS[v]` with `k`.
    AnnVector(usize, u8),
    /// `ann` of a text with `k`.
    AnnText(String, u8),
    /// `ann` of a vector of three dimensions, which play refuses with `EXT_BAD_ARG`. Changed by
    /// decision 105: before, the code was `DimensionMismatch`.
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
    // Changed by decision 114: the mirror answers no watermark.
    // let mirror = match down {
    //     true => FakeMirror::down(),
    //     false => FakeMirror::new(
    //         answer
    //             .iter()
    //             .zip([0.125f32, 0.25, 0.375])
    //             .map(|(byte, distance)| neighbour(&[*byte; 32], distance))
    //             .collect(),
    //         u64::try_from(answer.len()).expect("a short answer"),
    //     ),
    // };
    let mirror = match down {
        true => FakeMirror::down(),
        false => FakeMirror::new(
            answer
                .iter()
                .zip([0.125f32, 0.25, 0.375])
                .map(|(byte, distance)| neighbour(&[*byte; 32], distance))
                .collect(),
        ),
    };
    (embedder, mirror)
}

/// The recorded outputs of `embed` and `ann` make consensus independent of the services of a
/// node (ChromaMirror.tla, ConsensusIndependentOfMirror and AdvisoryRecorded).
/// Generated calls play with one embedder and one mirror (or a mirror that is down), and native
/// replay certifies the play with another embedder and another mirror: replay reproduces the
/// result, the phlo and the root of play, and asks no service. Replay makes no check of an `ann`
/// call and delivers the record (decision 104). The config read writes no journal row (DR-131),
/// so the certification stays complete.
///
/// Changed by decision 104: before, the checks of an `ann` call were journaled native
/// operations, so replay made each check of play (decision 67).
// Added by decision 113: only a chromadb build registers `embed` and `ann`.
#[cfg(feature = "chromadb")]
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
                let played = native_play(&term, LARGE_LIMIT).await;
                let replayed_embedder = MockEmbedder::new("mock-b", 2, 9.0, EmbedMode::Vectors);
                // let replayed_with = FakeMirror::new(vec![neighbour(&[0xAA; 32], 0.5)], 99);
                let replayed_with = FakeMirror::new(vec![neighbour(&[0xAA; 32], 0.5)]);
                lock.replace(services(&replayed_embedder, &replayed_with));
                let certification =
                    tokio::time::timeout(CERTIFY_TIMEOUT, certify(&played, &term, LARGE_LIMIT))
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
