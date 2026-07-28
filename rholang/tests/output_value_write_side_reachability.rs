//! # ★★ THE WRITE SIDE of `output_value` — what depth can a DEPLOY reach?
//!
//! `rholang/tests/replay_output_value_depth_ceiling.rs` proves the READ side:
//! a `Produce` whose `output_value` carries a depth-34 `Par` is accepted on
//! play and refused on replay. It obtains that byte string by **splicing** one
//! into the recorded log — legitimate, and its own header says so plainly:
//!
//! > *"Splicing proves the decode asymmetry; it does NOT prove reachability
//! > from a deploy."*
//!
//! That file then states the reachability answer in prose:
//!
//! > *"`output_value` is written from one site, gated on
//! > `non_deterministic_ops()`. All eight of those operations' return
//! > constructions were read and none exceeds depth 3 … **But the guard is
//! > those eight functions' return shapes, not a check.**"*
//!
//! ★★ **This file is that check.** Two things were wrong with leaving it as
//! prose, and they are different defects:
//!
//! 1. **The claim was hand-READ.** "Their return constructions were read" is a
//!    person's summary of eight function bodies. Nothing re-reads them.
//! 2. ★ **The denominator was hand-LISTED.** "All eight" is a literal. Add a
//!    ninth operation to [`non_deterministic_ops`] and the sentence is still
//!    grammatical, the fixture still passes, and the block-killing behaviour is
//!    live with no compile error anywhere. This is the same meta-defect the
//!    codec campaign found as `EXPR_INSTANCE_VARIANT_COUNT: usize = 36` — *a
//!    count nobody bumps.*
//!
//! Here the denominator **is** [`non_deterministic_ops`] itself, read from
//! production at run time, and every member must carry a row in
//! [`DISPOSITIONS`]. A ninth operation therefore fails this suite until
//! somebody decides its disposition — which is the only mechanism that makes
//! *"the guard is those eight functions' return shapes"* survive the ninth.
//!
//! ## The write path, traced (and it really is ONE site)
//!
//! ```text
//!   system process returns  Vec<Par>                     ← the only variable
//!         │
//!         ▼  dispatch.rs `dispatch_type`, gated on
//!            `non_deterministic_ops().contains(&body_ref)`
//!      output.iter().map(|p| p.encode_to_vec())           prost, UNBOUNDED
//!         │
//!         ▼  DispatchType::NonDeterministicCall(Vec<Vec<u8>>)
//!            reduce.rs `produce_inner`
//!      produce_event.mark_as_non_deterministic(bytes)     ← THE ONE WRITE
//!         │
//!         ▼  event.rs `Produce.output_value: Vec<Vec<u8>>`
//!            → block → gossip → validator → replay
//!      Par::decode(bytes)                                 prost, CAPPED at 100
//! ```
//!
//! `Produce::create` and `Produce::new` set `output_value: vec![]`;
//! `mark_as_non_deterministic` is the only function that sets it to anything
//! else, and `reduce.rs` is its only production caller. So the depth a deploy
//! can reach is exactly *the depth of the deepest `Par` any registered
//! non-deterministic operation returns* — nothing else contributes.
//!
//! ## What is measured, and in whose units
//!
//! The ceiling is stated in the bracket levels of `[[[…[0]…]]]`
//! (`models/tests/par_prost_depth_ceiling.rs::nested_list`), so the meter here
//! must agree with that convention or the comparison is meaningless.
//! [`max_par_nesting`] counts **`Par` nodes on the longest root-to-leaf chain,
//! minus one**, over `par_child_pars` — the generated, total child enumeration,
//! so no `Par`-bearing field can be silently skipped — and
//! [`the_depth_meter_agrees_with_the_ceiling_fixtures_units`] calibrates it
//! against `nested_list` before any verdict is read off it.
//!
//! ## Anti-vacuity — the false zero this file is built to avoid
//!
//! ★ The failure mode of a test like this is a **false zero**: the driving
//! program does not actually reach the operation, `output_value` stays `[]`,
//! the measured depth is 0, 0 < 33, and the suite is green while measuring
//! nothing. Every driven row therefore asserts that a NON-EMPTY `output_value`
//! was observed, and asserts the measured depth **equals** the recorded one
//! rather than merely clearing the ceiling — so a shape change in *either*
//! direction fires.
//!
//! **Recorded mutation ledger.** Each row was applied to this file's source,
//! run, and reverted; the control was re-run green between every pair. A guard
//! that cannot be shown red is a guard nobody has tested.
//!
//! | # | mutation | result | which assertion caught it |
//! |---|---|---|---|
//! | M0 | *none* — the control | **green** | — |
//! | M1 | delete the `OLLAMA_MODELS` row from [`DISPOSITIONS`] | **red** | `body_ref(s) [28] … have no row` |
//! | M2 | add a row for `DEV_NULL`, which is not a non-deterministic op | **red** | `… have a row … but are no longer in the set` |
//! | M3 | record `OLLAMA_MODELS` one level too LOW (1 → 0) | **red** | `wrote … depth 1; the register records 0` |
//! | M4 | record `GPT4` one level too HIGH (0 → 1) | **red** | `wrote … depth 0; the register records 1` |
//! | M5 | point `GPT4`'s program at a channel the op does not serve | **red** | `wrote NO output_value in 1 recorded produces` |
//! | M6 | make [`max_par_nesting`] return `0` unconditionally | **red** | calibration, *and* the depth-1 row |
//! | M7 | make the meter walk only the first-expr `EList` spine | **red** | `the meter missed depth carried under Send::data` |
//!
//! ★ M5 is the one that matters most: it is the **false zero**, and it is the
//! failure a version of this file without `produces_with_output > 0` would have
//! reported as a pass. ★ M7 is the second: a meter that only follows the
//! `nested_list` spine measures every one of these operations correctly *and*
//! would measure a deep `Par` returned under a `Send`, a `Match` or a `Receive`
//! as zero — green, and blind in exactly the direction that matters.
//!
//! ## ⚠ What this file does NOT do
//!
//! It changes no production code, bounds nothing, widens nothing, and adds no
//! feature gate. It records what the write side reaches **today** and refuses
//! to let that go unrecorded tomorrow. Whether to lift the READ ceiling so the
//! two sides agree at every depth is a change to the set of byte strings a node
//! accepts, and therefore F1r3node's coordinated decision — see
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md` §7.3.

use std::collections::{HashMap, HashSet};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, EList, Expr, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::par_children::par_child_pars;
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::chromadb_service::create_noop_chromadb_service;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::grpc_client_service::GrpcClientService;
use rholang::rust::interpreter::ollama_service::OllamaService;
use rholang::rust::interpreter::openai_service::{
    create_mock_openai_service, create_noop_openai_service, OpenAIMockConfig,
};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{non_deterministic_ops, BodyRefs};
use rholang::rust::interpreter::test_utils::resources::create_runtimes_with_services;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

// ---------------------------------------------------------------------------
// the ceiling this verdict is stated against
// ---------------------------------------------------------------------------

/// The last term depth a bare `Par::decode` accepts; the first it refuses is
/// `READ_CEILING + 1`.
///
/// Derived — never transcribed — by `models/tests/par_prost_depth_ceiling.rs`
/// as `D_max(0) = ⌊(100 − 1 − 0) / 3⌋ = 33`, and executed there against the
/// real decoder for nine envelopes. Restated here so that this file's verdict
/// names the number that actually binds it; the two files are kept honest by
/// [`the_depth_meter_agrees_with_the_ceiling_fixtures_units`], which rebuilds
/// that file's `nested_list` term and re-measures it with this file's meter.
const READ_CEILING: usize = 33;

// ---------------------------------------------------------------------------
// the depth meter — TOTAL over the `Par` grammar, and ITERATIVE
// ---------------------------------------------------------------------------

/// `Par` nodes on the longest root-to-leaf chain, minus one.
///
/// * **Total.** Children come from `par_child_pars`, the generated enumeration
///   that walks exprs, connectives, sends, bundles, receives, news, matches and
///   conditionals. A hand-written match here would be one more place to forget
///   a variant — the exact defect that left thirteen `ExprInstance` arms out of
///   the spatial matcher.
/// * **Iterative.** An explicit stack, so a deep subject can never abort the
///   *meter* and be mistaken for a shallow measurement.
/// * **Calibrated.** `max_par_nesting(nested_list(d)) == d`, asserted before
///   the meter is used for any verdict.
fn max_par_nesting(root: &Par) -> usize {
    let mut deepest = 0usize;
    let mut work: Vec<(&Par, usize)> = vec![(root, 0)];
    let mut children: Vec<&Par> = Vec::new();
    while let Some((par, depth)) = work.pop() {
        if depth > deepest {
            deepest = depth;
        }
        children.clear();
        par_child_pars(par, &mut children);
        work.reserve(children.len());
        for child in children.drain(..) {
            work.push((child, depth + 1));
        }
    }
    deepest
}

/// `[[[…[0]…]]]` with `depth` bracket levels — the same construction
/// `models/tests/par_prost_depth_ceiling.rs` and
/// `rholang/tests/replay_output_value_depth_ceiling.rs` build, so all three
/// files are talking about one term shape.
fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        par = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![par],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    par
}

// ---------------------------------------------------------------------------
// the register — its DENOMINATOR is production's own set
// ---------------------------------------------------------------------------

/// How a registered operation is exercised, or why it cannot be.
enum Drive {
    /// A Rholang program that reaches the operation, and the services it needs.
    ///
    /// The program is a real deploy evaluated on a real runtime: the value that
    /// lands in `output_value` is the one production puts there, not one this
    /// file constructs.
    Deploy {
        program: &'static str,
        services: fn() -> ExternalServices,
    },
    /// `non_deterministic_ops()` names the ref unconditionally, but the
    /// `Definition` that serves it is compiled out under the default feature
    /// set, so no dispatch entry exists and no deploy can reach it.
    ///
    /// ⚠ The row still exists. Dropping it would let the ref leave the register
    /// silently the day the feature turns on.
    CompiledOutWithoutFeature { feature: &'static str },
}

/// One registered non-deterministic operation and what it may write.
struct Disposition {
    /// The `ScalaBodyRef` the dispatcher gates on.
    body_ref: i64,
    /// The URN a deploy names it by.
    urn: &'static str,
    drive: Drive,
    /// ★ The `output_value` term depth this operation writes, in
    /// [`nested_list`] units.
    ///
    /// MEASURED by [`every_driven_op_writes_output_value_at_the_recorded_depth`]
    /// against the running interpreter — this number is an expectation the
    /// measurement must reproduce exactly, never a transcription of one.
    max_output_value_depth: usize,
    /// The shape that produces that depth, so a reader can see *why* the number
    /// is what it is without re-deriving it.
    shape: &'static str,
}

fn openai_services(config: OpenAIMockConfig) -> ExternalServices {
    ExternalServices {
        openai: create_mock_openai_service(config),
        ollama: std::sync::Arc::new(tokio::sync::Mutex::new(OllamaService::new_disabled())),
        grpc_client: GrpcClientService::new_noop(),
        openai_enabled: true,
        ollama_enabled: false,
        is_validator: true,
        chroma: create_noop_chromadb_service(),
    }
}

/// The Ollama mock answers all three of its operations from one configuration,
/// so one constructor serves all three rows.
fn ollama_services() -> ExternalServices {
    ExternalServices {
        openai: create_noop_openai_service(),
        ollama: std::sync::Arc::new(tokio::sync::Mutex::new(OllamaService::new_mock(
            "chat-reply".to_string(),
            "generate-reply".to_string(),
            vec!["model-a".to_string(), "model-b".to_string()],
        ))),
        grpc_client: GrpcClientService::new_noop(),
        openai_enabled: false,
        ollama_enabled: true,
        is_validator: true,
        chroma: create_noop_chromadb_service(),
    }
}

fn gpt4_services() -> ExternalServices {
    openai_services(OpenAIMockConfig::single_completion("gpt4-reply"))
}

fn dalle3_services() -> ExternalServices {
    openai_services(OpenAIMockConfig::single_dalle3(
        "https://example.invalid/i.png",
    ))
}

fn text_to_audio_services() -> ExternalServices {
    openai_services(OpenAIMockConfig::single_tts_audio(vec![0x49, 0x44, 0x33]))
}

/// `GrpcClientService::NoOp::tell` returns `Ok(())`, which is the arm an
/// observer node runs in production — so `grpc_tell` takes its success path and
/// writes the `output_value` it writes on a real node.
fn grpc_services() -> ExternalServices {
    ExternalServices {
        openai: create_noop_openai_service(),
        ollama: std::sync::Arc::new(tokio::sync::Mutex::new(OllamaService::new_disabled())),
        grpc_client: GrpcClientService::new_noop(),
        openai_enabled: false,
        ollama_enabled: false,
        is_validator: true,
        chroma: create_noop_chromadb_service(),
    }
}

/// ★★ **Every member of `non_deterministic_ops()` gets a row, and every row
/// names a member.** Both directions are asserted by
/// [`every_non_deterministic_op_carries_a_disposition`], so neither a new
/// operation nor a stale row can hide.
const DISPOSITIONS: &[Disposition] = &[
    Disposition {
        body_ref: BodyRefs::GPT4,
        urn: "rho:ai:gpt4",
        drive: Drive::Deploy {
            program: r#"new gpt4(`rho:ai:gpt4`), ack in { gpt4!("p", *ack) }"#,
            services: gpt4_services,
        },
        max_output_value_depth: 0,
        shape: "vec![RhoString::create_par(response)] — one Par, one GString expr, no child Par",
    },
    Disposition {
        body_ref: BodyRefs::DALLE3,
        urn: "rho:ai:dalle3",
        drive: Drive::Deploy {
            program: r#"new dalle3(`rho:ai:dalle3`), ack in { dalle3!("p", *ack) }"#,
            services: dalle3_services,
        },
        max_output_value_depth: 0,
        shape: "vec![RhoString::create_par(url)] — one Par, one GString expr",
    },
    Disposition {
        body_ref: BodyRefs::TEXT_TO_AUDIO,
        urn: "rho:ai:textToAudio",
        drive: Drive::Deploy {
            program: r#"new tts(`rho:ai:textToAudio`), ack in { tts!("p", *ack) }"#,
            services: text_to_audio_services,
        },
        max_output_value_depth: 0,
        shape: "vec![RhoByteArray::create_par(bytes)] — one Par, one GByteArray expr",
    },
    Disposition {
        body_ref: BodyRefs::OLLAMA_CHAT,
        urn: "rho:ollama:chat",
        drive: Drive::Deploy {
            program: r#"new chat(`rho:ollama:chat`), ack in { chat!("m", "p", *ack) }"#,
            services: ollama_services,
        },
        max_output_value_depth: 0,
        shape: "vec![RhoString::create_par(response)] — one Par, one GString expr",
    },
    Disposition {
        body_ref: BodyRefs::OLLAMA_GENERATE,
        urn: "rho:ollama:generate",
        drive: Drive::Deploy {
            program: r#"new gen(`rho:ollama:generate`), ack in { gen!("m", "p", *ack) }"#,
            services: ollama_services,
        },
        max_output_value_depth: 0,
        shape: "vec![RhoString::create_par(response)] — one Par, one GString expr",
    },
    Disposition {
        body_ref: BodyRefs::OLLAMA_MODELS,
        urn: "rho:ollama:models",
        drive: Drive::Deploy {
            program: r#"new models(`rho:ollama:models`), ack in { models!(*ack) }"#,
            services: ollama_services,
        },
        // ★ The DEEPEST of the eight, and still 32 levels clear of the ceiling.
        max_output_value_depth: 1,
        shape: "Par{EList{ps: [Par{GString}, …]}} — one collection level over scalar leaves",
    },
    Disposition {
        body_ref: BodyRefs::GRPC_TELL,
        urn: "rho:io:grpcTell",
        drive: Drive::Deploy {
            program: r#"new tell(`rho:io:grpcTell`) in { tell!("http://h", 1, "payload") }"#,
            services: grpc_services,
        },
        max_output_value_depth: 0,
        shape: "vec![Par::default()] — the empty Par; encodes to zero bytes",
    },
    Disposition {
        body_ref: BodyRefs::CHROMA_QUERY,
        urn: "rho:chroma:collection:entries:query",
        // `std_rho_chroma_processes()` is `vec![]` without the feature
        // (`rho_runtime.rs`), so ref 35 has no dispatch-table entry and a
        // deploy naming that URN cannot bind it. The ref stays in
        // `non_deterministic_ops()` either way, so the row stays too.
        drive: Drive::CompiledOutWithoutFeature {
            feature: "chromadb",
        },
        max_output_value_depth: 2,
        shape: "RhoList::create_par(entries) over CollectionEntry → Par (document ∥ metadata map)",
    },
];

// ---------------------------------------------------------------------------
// the driver
// ---------------------------------------------------------------------------

/// What one operation's deploy actually wrote.
struct Written {
    /// `Produce`s in the recorded log.
    produces: usize,
    /// `Produce`s whose `output_value` is non-empty. ★ Zero means the program
    /// never reached the operation, and is a test failure, not a measurement.
    produces_with_output: usize,
    /// One entry per byte string found in any `output_value`, in log order.
    depths: Vec<usize>,
    /// Any byte string that did not decode. Empty on every current row; a
    /// non-empty vector would itself be the finding.
    undecodable: Vec<String>,
    /// Errors the play evaluation reported. Empty on every current row.
    errors: Vec<String>,
}

/// Every `Produce` reachable in one log entry — COMM participants and the
/// standalone rows. Mirrors `replay_output_value_depth_ceiling.rs::produces_of`
/// so the two files harvest the same set.
fn produces_of(event: &Event) -> &[Produce] {
    match event {
        Event::Comm(comm) => &comm.produces,
        Event::IoEvent(IOEvent::Produce(produce)) => std::slice::from_ref(produce),
        Event::IoEvent(IOEvent::Consume(_)) => &[],
    }
}

/// Evaluate `program` on a play runtime carrying `services`, then read what the
/// run wrote into `output_value`.
async fn play_and_harvest(program: &str, services: ExternalServices) -> Written {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace store");
    let (mut runtime, _replay, _history): (
        RhoRuntimeImpl,
        RhoRuntimeImpl,
        std::sync::Arc<
            Box<
                dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    ) = create_runtimes_with_services(store, false, &mut Vec::new(), services).await;

    let phlo = Cost::create(i64::MAX, "output_value write-side reachability".to_string());
    let rand = Blake2b512Random::create_from_bytes(&[0x29; 32]);

    let result = runtime
        .evaluate(program, phlo, HashMap::new(), rand)
        .await
        .expect("play evaluation must run");
    let log = runtime.take_event_log().await;

    let mut written = Written {
        produces: 0,
        produces_with_output: 0,
        depths: Vec::new(),
        undecodable: Vec::new(),
        errors: result.errors.iter().map(|e| e.to_string()).collect(),
    };
    for event in log.iter() {
        for produce in produces_of(event) {
            written.produces += 1;
            if produce.output_value.is_empty() {
                continue;
            }
            written.produces_with_output += 1;
            written.depths.reserve(produce.output_value.len());
            for bytes in &produce.output_value {
                match Par::decode(&bytes[..]) {
                    Ok(par) => written.depths.push(max_par_nesting(&par)),
                    Err(e) => written.undecodable.push(e.to_string()),
                }
            }
        }
    }
    written
}

// ---------------------------------------------------------------------------
// 1. the meter is calibrated BEFORE any verdict is read off it
// ---------------------------------------------------------------------------

/// ★ The units check. [`READ_CEILING`] is stated in the bracket levels of
/// `models/tests/par_prost_depth_ceiling.rs::nested_list`; this asserts that
/// [`max_par_nesting`] counts the same thing, at the boundary and around it.
///
/// Without this, a meter that under-counted by a constant would make every
/// operation look further below the ceiling than it is, and the whole file
/// would be green and wrong.
#[test]
fn the_depth_meter_agrees_with_the_ceiling_fixtures_units() {
    for depth in [0usize, 1, 2, 3, 32, READ_CEILING, READ_CEILING + 1, 64] {
        let measured = max_par_nesting(&nested_list(depth));
        assert_eq!(
            measured, depth,
            "★ the depth meter disagrees with `nested_list`: a term built with \
             {depth} bracket levels measured {measured}. Every verdict in this \
             file compares a measured depth against READ_CEILING = \
             {READ_CEILING}, which is stated in `nested_list` units, so the two \
             must count the same thing."
        );
    }

    // The meter must also see depth through a field that is NOT the first-expr
    // EList spine — otherwise an operation returning a deep `Par` under a send,
    // a match, or a receive body would measure 0 and pass.
    let under_a_send = Par {
        sends: vec![models::rhoapi::Send {
            chan: Some(Par::default()),
            data: vec![nested_list(7)],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        }],
        ..Default::default()
    };
    assert_eq!(
        max_par_nesting(&under_a_send),
        8,
        "★ the meter missed depth carried under `Send::data`. It reads children \
         from the generated `par_child_pars`, so this failing means either that \
         enumeration or this expectation is wrong — not that the term is shallow."
    );

    println!(
        "  depth meter calibrated against nested_list at 0,1,2,3,32,33,34,64 and under Send::data"
    );
}

// ---------------------------------------------------------------------------
// 2. the denominator is production's, so a NINTH operation fails here
// ---------------------------------------------------------------------------

/// ★★ The bijection that replaces *"all eight of those operations"*.
///
/// `non_deterministic_ops()` is read from production at run time. A ninth
/// operation added there has no row and fails the first assertion; a row whose
/// `body_ref` is removed from the set fails the second. Neither can be
/// satisfied by editing a comment.
#[test]
fn every_non_deterministic_op_carries_a_disposition() {
    let registered: HashSet<i64> = non_deterministic_ops();
    assert!(
        !registered.is_empty(),
        "`non_deterministic_ops()` is empty; every assertion below would be \
         vacuously true"
    );

    let dispositioned: HashSet<i64> = DISPOSITIONS.iter().map(|d| d.body_ref).collect();
    assert_eq!(
        dispositioned.len(),
        DISPOSITIONS.len(),
        "two rows in DISPOSITIONS share a body_ref; one of them is shadowing \
         the other and is never checked"
    );

    let missing: Vec<i64> = {
        let mut v: Vec<i64> = registered.difference(&dispositioned).copied().collect();
        v.sort_unstable();
        v
    };
    assert!(
        missing.is_empty(),
        "★★ body_ref(s) {missing:?} are in `non_deterministic_ops()` and have no \
         row in DISPOSITIONS.\n\
         Every member of that set can write `output_value`, and what it writes \
         is decoded by a validator on replay with prost's {READ_CEILING}-level \
         ceiling. Add a row saying what this operation returns and how deep it \
         is — that is the decision this failure is asking for, and it is the \
         one thing that kept the ceiling unreachable for the previous eight."
    );

    let stale: Vec<i64> = {
        let mut v: Vec<i64> = dispositioned.difference(&registered).copied().collect();
        v.sort_unstable();
        v
    };
    assert!(
        stale.is_empty(),
        "body_ref(s) {stale:?} have a row in DISPOSITIONS but are no longer in \
         `non_deterministic_ops()`. A row that guards nothing makes the count \
         look larger than the set it covers."
    );

    println!(
        "  {} registered non-deterministic ops, {} dispositioned, bijective",
        registered.len(),
        DISPOSITIONS.len()
    );
}

// ---------------------------------------------------------------------------
// 3. the measurement — a real deploy, a real runtime, the real write site
// ---------------------------------------------------------------------------

/// ★★ **The answer to "can a deploy put a depth-34 `Par` into `output_value`?"**
///
/// For every driven row: run the deploy, harvest the recorded log's
/// `output_value`s, decode each byte string, and measure it.
///
/// The assertions, and why each one is there:
///
/// * `produces_with_output > 0` — ★ the anti-vacuity assertion. A program that
///   does not reach its operation writes nothing, and "nothing" measures 0,
///   which would clear the ceiling and mean nothing at all.
/// * `undecodable.is_empty()` — the play side writes what it can read back. A
///   failure here is not a fixture bug; it is the write/read asymmetry firing
///   on the write side's own bytes.
/// * `measured == recorded` — **equality, not `≤`**. A row that drifted in
///   either direction is a change in what a deploy can write, and that is
///   exactly the event this file exists to catch.
#[tokio::test(flavor = "current_thread")]
async fn every_driven_op_writes_output_value_at_the_recorded_depth() {
    let mut driven = 0usize;
    let mut deepest_seen = 0usize;

    for disposition in DISPOSITIONS {
        let Drive::Deploy { program, services } = &disposition.drive else {
            continue;
        };
        driven += 1;

        let written = play_and_harvest(program, services()).await;

        assert!(
            written.errors.is_empty(),
            "★ `{}` ({}) reported evaluation errors {:?}. The measurement below \
             would be of a run that did not happen.",
            disposition.urn,
            disposition.body_ref,
            written.errors
        );

        assert!(
            written.produces_with_output > 0,
            "★★ `{}` ({}) wrote NO `output_value` in {} recorded produces.\n\
             This is the false zero this test is built to refuse: an unwritten \
             `output_value` measures depth 0, 0 < {READ_CEILING}, and the \
             suite would be green while measuring nothing. Either the program \
             `{}` no longer reaches the operation, or the operation no longer \
             writes — and the second is a change to the write site.",
            disposition.urn,
            disposition.body_ref,
            written.produces,
            program
        );

        assert!(
            written.undecodable.is_empty(),
            "★★ `{}` ({}) wrote {} `output_value` byte string(s) that do not \
             decode: {:?}.\n\
             The play side just encoded these. A byte string this node wrote \
             and cannot read back IS the write/read asymmetry, arriving on the \
             write side's own output.",
            disposition.urn,
            disposition.body_ref,
            written.undecodable.len(),
            written.undecodable
        );

        let measured = written
            .depths
            .iter()
            .copied()
            .max()
            .expect("produces_with_output > 0 implies at least one byte string");
        assert_eq!(
            measured,
            disposition.max_output_value_depth,
            "★★ `{}` ({}) wrote `output_value` at term depth {measured}; the \
             register records {}.\n\
             Recorded shape: {}\n\
             The register is an expectation the interpreter must reproduce, not \
             a transcription of it. If the operation's return shape changed on \
             purpose, update the row AND check the new depth against the read \
             ceiling ({READ_CEILING}) — that check is the whole point of the row.",
            disposition.urn,
            disposition.body_ref,
            disposition.max_output_value_depth,
            disposition.shape
        );

        if measured > deepest_seen {
            deepest_seen = measured;
        }

        println!(
            "  {:<38} ref {:>3}  depth {}  ({} byte string(s) over {} produce(s))",
            disposition.urn,
            disposition.body_ref,
            measured,
            written.depths.len(),
            written.produces_with_output
        );
    }

    assert!(
        driven >= 7,
        "only {driven} operations were driven end-to-end; the register claims \
         more. A row that silently stopped being driven measures nothing."
    );
    println!(
        "  {driven} operations driven end-to-end; deepest `output_value` observed: {deepest_seen}"
    );
}

// ---------------------------------------------------------------------------
// 4. THE VERDICT — and the margin, stated rather than implied
// ---------------------------------------------------------------------------

/// ★★★ **The severity finding, made executable.**
///
/// Every registered operation's recorded `output_value` depth clears the read
/// ceiling, so **no deploy can drive the play/replay divergence today** — the
/// divergence proved by `replay_output_value_depth_ceiling.rs` needs a byte
/// string this write side cannot produce.
///
/// ⚠ The moment a row records a depth at or above [`READ_CEILING`], that
/// sentence stops being true and this test says so. It does not bound anything:
/// production is unchanged and the operation still writes whatever it writes.
/// What changes is that somebody is told, at the moment the row is written,
/// that the operation they just added can build a block no validator will
/// replay.
#[test]
fn the_recorded_write_depths_all_clear_the_read_ceiling() {
    assert!(
        !DISPOSITIONS.is_empty(),
        "DISPOSITIONS is empty; this test would be vacuously green"
    );

    let mut tightest = usize::MAX;
    let mut tightest_urn = "";
    for disposition in DISPOSITIONS {
        assert!(
            disposition.max_output_value_depth <= READ_CEILING,
            "★★★ `{}` ({}) records `output_value` depth {}, which the replay \
             decode REFUSES (it accepts at most {READ_CEILING}).\n\
             Shape: {}\n\
             A deploy reaching this operation now produces a block that plays \
             green and replays red — `InterpreterError::DecodeError` lands in \
             `EvaluateResult::errors`, the `is_failed != !eval_successful` \
             check in `casper/src/rust/rholang/replay_runtime.rs` returns \
             `ReplayFailure::ReplayStatusMismatch`, and \
             `InvalidBlock::InvalidTransaction` is SLASHABLE \
             (`casper/src/rust/block_status.rs::is_slashable`). The proposer is \
             honest and is slashed for it.",
            disposition.urn,
            disposition.body_ref,
            disposition.max_output_value_depth,
            disposition.shape
        );

        let margin = READ_CEILING - disposition.max_output_value_depth;
        if margin < tightest {
            tightest = margin;
            tightest_urn = disposition.urn;
        }

        // ★ A row that is not driven is still a row, and the verdict above
        // covers it — but on a RECORDED depth rather than a measured one. Say
        // which rows those are, so the difference is visible in the output
        // instead of having to be inferred from the two tests' line counts.
        if let Drive::CompiledOutWithoutFeature { feature } = &disposition.drive {
            println!(
                "  ⚠ `{}` ({}) is not driven here: its `Definition` is compiled out without \
                 feature `{feature}`, so the depth {} is RECORDED from its return construction \
                 and is re-measured only under `--features {feature}`",
                disposition.urn, disposition.body_ref, disposition.max_output_value_depth
            );
        }
    }

    println!(
        "  read ceiling {READ_CEILING}; tightest write-side margin {tightest} levels (`{tightest_urn}`)"
    );
    println!(
        "  ⇒ no registered non-deterministic operation can write an `output_value` \
         the validator's replay decode refuses"
    );
}
