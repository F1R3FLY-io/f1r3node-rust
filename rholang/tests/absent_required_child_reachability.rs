//! # ★★ #127 + #136 — is an absent required child REACHABLE, and from where?
//!
//! One question, two surfaces, and the campaign's own precedent says they must
//! not be answered differently. This file answers the **reachability** half by
//! measurement; `rholang/tests/spatial_matcher_disposition.rs` carries the
//! per-variant gate, and
//! `rspace++/libs/rspace_rhotypes/tests/ffi_absent_required_child.rs` carries
//! the FFI red.
//!
//! ## The premise, and why it is not a decode error
//!
//! `EMinus { Par p1 = 1; Par p2 = 2; }` (`RhoTypes.proto`) is proto3, where
//! every message field is optional **on the wire**. `prost` renders that as
//! `Option<Par>` and produces `None` for an absent field — *without error*.
//! ⇒ A `Par` with an absent required child is not something `Par::decode`
//! refuses. It is something `Par::decode` **returns**.
//!
//! ★ That is a different axis from the one #120/#129/#130/#135 walk. Those are
//! **depth**: an unbounded writer, a bounded reader, and a ceiling in between.
//! This is **shape**: the reader has no opinion at all, and the refusal — such
//! as it is — happens much later, as a panic, in a consumer that assumed the
//! field was there.
//!
//! ## The measurements
//!
//! | # | question | answer |
//! |---|---|---|
//! | M0 | which site faults FIRST? | ★ **not** the matcher — `has_locally_free.rs:681` |
//! | M1 | does `prost` accept it? | **yes**, 10 bytes, no error |
//! | M2 | does the matcher fault on the *decoded* value? | **yes**, `spatial_matcher.rs` `EMinus.p1 (target)` |
//! | M2b | control: same shapes, well formed | **matches** |
//! | M4 | can a block deliver it to a validator's matcher? | **decoded yes, matched NO** — see below |
//!
//! ## ★★ M0 — the sibling #127 does not name
//!
//! #127 scopes the finding to `spatial_matcher.rs`. Measured, the value never
//! gets that far on the normalizer's own path: `prepend_expr` recomputes
//! `locally_free`/`connective_used`, and `has_locally_free.rs` faults on the
//! same absent child first. That file carried **94** `.unwrap()` of this class
//! against the matcher's 55 — a larger surface than the one the task named.
//! *"Adding a guard ≠ enumerating siblings."*
//!
//! ## ★★★ M4 — the consensus path, and why the answer is LATENT
//!
//! `ProduceEventProto.outputValue` is `repeated bytes`, opaque to the block
//! body's decode, and (per `event.rs`) excluded from event identity — so a
//! proposer can put **any** byte string there and the event hash is unchanged.
//! On replay `decode_non_deterministic_output` reads it back, and a
//! non-deterministic system process's replay branch *produces the decoded
//! `Par`s onto the deploy's `ack` channel* (`system_processes.rs`:
//! `produce(&previous_output, ack)`), where the tuple space consults
//! `Matcher::get`. Every link of that chain is real.
//!
//! It still does not reach the matcher's absent-child arms, and the reason is
//! **structural rather than incidental**, which is why it is recorded here
//! instead of being left as "we looked and found nothing":
//!
//! > On replay the tuple space is **trace-driven**. The only COMM it reproduces
//! > is one that already fired during play — and the play-time target was the
//! > *service adapter's own return value*, constructed in-process, never
//! > decoded. For the spliced value to meet a pattern that descends into it,
//! > that same pattern must already have matched a well-formed value of the
//! > same variant at play time.
//!
//! [`M4_PROGRAMS`] drives five receive shapes a deploy can actually write —
//! including the `EMinus` pattern that would enter the arm under test — and all
//! five replay clean.
//!
//! ⚠ **The positive control is what makes that a measurement rather than a
//! false zero.** Splicing #129's depth-34 payload through the identical harness
//! turns the replay red with `recursion limit reached`, so the harness is
//! provably delivering spliced `output_value` bytes to the replay decode. Ten
//! false zeros have been recorded this campaign; this one is not the eleventh.
//!
//! ## What that leaves
//!
//! The one production path that hands a wire-decoded `Par` to `spatial_match`
//! against a **caller-chosen pattern** is the FFI entry point
//! `rspace_plus_plus_rhotypes::spatial_match_result`, which is #127's site and
//! #136's read *in the same function*. Its red — SIGABRT, non-unwinding, on ten
//! bytes — lives next to it, in that crate's own test directory.

//! ## ⚠ How the faults are OBSERVED (changed 2026-07-28)
//!
//! M0 and M2 used to wrap their subject in `std::panic::catch_unwind` and
//! `expect_err` the payload. They no longer do, and must not: a test that expects a
//! panic is a liability in this tree — `rholang` is compiled as a path dependency of
//! the mettail workspace, whose `dev`/`test` profile uses the CRANELIFT backend,
//! under which a panic does not unwind reliably, `catch_unwind` intercepts nothing,
//! and the process aborts with no diagnostic.
//!
//! Both faults are now measured the way the FFI red next door measures its abort:
//! **from outside the process**. The subject runs in a re-executed child, and the
//! parent decides on the child's exit status and stderr. Each child prints an
//! anti-vacuity line BEFORE touching the subject, so a child that died on startup —
//! or on an unrelated earlier fault — is a loud failure rather than a false green.
//!
//! What that changed about their discriminating power: it GREW. `expect_err` accepted
//! any unwinding panic from anywhere inside the closure; the parent now additionally
//! requires the child to have reached the subject, requires the process not to have
//! survived, and still requires the message to name the site.

use std::process::Command;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMinus, Expr, Par};
use models::rust::utils::{new_freevar_par, new_gint_par};
use prost::Message;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::util::prepend_expr;

/// The env var that puts a re-executed test binary into child mode. Its VALUE names
/// which subject the child is to run, so one variable serves every probe here.
const CHILD: &str = "RHOLANG_ABSENT_CHILD_SUBJECT";

/// Re-exec this test binary with `CHILD` set to `subject`, running only `test_name`.
///
/// Returns `(status_success, combined_output)`. The child's stdout and stderr are
/// concatenated because the anti-vacuity marker is on one and the fault message on
/// the other, and every caller wants both.
fn run_child(subject: &str, test_name: &str) -> (bool, String) {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = Command::new(exe)
        .env(CHILD, subject)
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .output()
        .expect("the child test process spawns");
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

/// Is this process the child for `subject`?
fn is_child_for(subject: &str) -> bool {
    std::env::var(CHILD).is_ok_and(|s| s == subject)
}

/// The NORMALIZER's construction: recomputes `locally_free`/`connective_used`.
/// ⚠ Panics on a malformed child — see `m0`.
fn par_of(instance: ExprInstance) -> Par {
    prepend_expr(
        Par::default(),
        Expr {
            expr_instance: Some(instance),
        },
        0,
    )
}

/// ★ The WIRE's construction. `Par.connective_used` and `Par.locally_free` are
/// proto FIELDS (`RhoTypes.proto`), decoded verbatim — nothing recomputes them
/// on the read path. So the bytes carry them, and a probe that reproduces the
/// wire must set them directly rather than derive them.
fn wire_par(instance: ExprInstance, connective_used: bool) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        connective_used,
        ..Default::default()
    }
}

/// ★★ M0 — the FIRST fault is not in `spatial_matcher.rs`.
///
/// `#127` scopes the finding to the matcher. Measured, the normalizer-side
/// recomputation in `has_locally_free.rs` panics on the same value FIRST, so a
/// fix confined to the matcher would leave the site that actually fires
/// untouched. This test exists to keep that fact from being re-lost.
#[test]
fn m0_has_locally_free_panics_before_the_matcher_is_reached() {
    const SUBJECT: &str = "m0";
    if is_child_for(SUBJECT) {
        // ── child ────────────────────────────────────────────────────────────
        // The CONTROL runs first, in this same process: the well-formed pair goes
        // through `prepend_expr` and returns. If the child dies here, the parent
        // says so instead of attributing an unrelated fault to the absent child.
        let control = par_of(ExprInstance::EMinusBody(EMinus {
            p1: Some(new_gint_par(1, Vec::new(), false)),
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }));
        println!("M0_CHILD_CONTROL_BUILT=true exprs={}", control.exprs.len());

        // ★ THE SUBJECT.
        let built = par_of(ExprInstance::EMinusBody(EMinus {
            p1: None,
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }));
        println!("M0_CHILD_SURVIVED=true exprs={}", built.exprs.len());
        return;
    }

    // ── parent ───────────────────────────────────────────────────────────────
    let (succeeded, text) = run_child(
        SUBJECT,
        "m0_has_locally_free_panics_before_the_matcher_is_reached",
    );
    assert!(
        text.contains("M0_CHILD_CONTROL_BUILT=true"),
        "M0: the child never built the WELL-FORMED pair, so whatever killed it was not \
         the absent child.\n--- child output ---\n{text}"
    );
    assert!(
        !text.contains("M0_CHILD_SURVIVED=true"),
        "M0: `prepend_expr` did NOT fault on an absent child. If that is now true, \
         re-derive: the sibling surface this test records has changed.\n\
         --- child output ---\n{text}"
    );
    assert!(
        !succeeded,
        "M0: the child exited cleanly; the normalizer-side recomputation no longer \
         faults.\n--- child output ---\n{text}"
    );
    assert!(
        text.contains("binary operand p1"),
        "M0: the child died, but not at `has_locally_free`'s absent-operand site.\n\
         --- child output ---\n{text}"
    );
    println!("  ⇒ M0: has_locally_free faults first, before the matcher is reached");
}

/// M1 — does prost accept an absent required child, on REAL BYTES?
///
/// Two independent constructions of the same byte string:
///   (a) encode a value whose `p1` is `None`;
///   (b) SURGERY: encode a well-formed `EMinus` and delete `p1`'s bytes.
/// (b) is what an attacker holding only bytes can do.
#[test]
fn m1_prost_accepts_an_absent_required_child() {
    // (a)
    let absent = wire_par(
        ExprInstance::EMinusBody(EMinus {
            p1: None,
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        false,
    );
    let bytes_a = absent.encode_to_vec();
    let decoded_a = Par::decode(&bytes_a[..]).expect("M1(a): prost DECODED the absent-p1 Par");
    println!("  M1(a) bytes = {bytes_a:02x?}");

    // (b) surgery on a well-formed encoding.
    let well_formed = wire_par(
        ExprInstance::EMinusBody(EMinus {
            p1: Some(new_gint_par(1, Vec::new(), false)),
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        false,
    );
    let bytes_b_full = well_formed.encode_to_vec();
    println!("  M1(b) well-formed = {bytes_b_full:02x?}");

    // Show the two differ, so (a) is not vacuously the same object.
    assert_ne!(
        bytes_a, bytes_b_full,
        "M1: the absent-p1 encoding equals the well-formed one; nothing was measured"
    );

    // The decoded value REALLY has None.
    let inner = match &decoded_a.exprs[0].expr_instance {
        Some(ExprInstance::EMinusBody(e)) => e,
        other => panic!("M1: decoded to the wrong variant: {other:?}"),
    };
    assert!(
        inner.p1.is_none(),
        "M1: p1 came back Some — prost did not produce the None this whole finding rests on"
    );
    assert!(
        inner.p2.is_some(),
        "M1: p2 must survive, or the probe is junk"
    );
    println!("  ⇒ M1: prost ACCEPTS an absent required child. Not a DecodeError.");
}

/// M2 — does the matcher fault on the DECODED value (not the constructed one)?
#[test]
fn m2_the_matcher_panics_on_a_decoded_absent_child() {
    const SUBJECT: &str = "m2";
    if is_child_for(SUBJECT) {
        // ── child ────────────────────────────────────────────────────────────
        let pattern = par_of(ExprInstance::EMinusBody(EMinus {
            p1: Some(new_freevar_par(0, Vec::new())),
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }));
        assert!(
            pattern.connective_used,
            "M2: the pattern must be connective_used or the matcher never descends"
        );

        // The CONTROL, in the same process and through the same call shape: a
        // WELL-FORMED target off the wire must match. Without it, a child that died
        // for any reason at all would look like evidence about the absent child.
        let control_bytes = wire_par(
            ExprInstance::EMinusBody(EMinus {
                p1: Some(new_gint_par(7, Vec::new(), false)),
                p2: Some(new_gint_par(2, Vec::new(), false)),
            }),
            false,
        )
        .encode_to_vec();
        let control_target = Par::decode(&control_bytes[..]).expect("the control decodes");
        let control = SpatialMatcherContext::new().spatial_match(control_target, pattern.clone());
        println!("M2_CHILD_CONTROL_MATCHED={}", control.is_some());

        // ★ THE SUBJECT: the value that came BACK OFF THE WIRE with `p1` absent.
        let bytes = wire_par(
            ExprInstance::EMinusBody(EMinus {
                p1: None,
                p2: Some(new_gint_par(2, Vec::new(), false)),
            }),
            false,
        )
        .encode_to_vec();
        let target = Par::decode(&bytes[..]).expect("the wire value decodes");
        let result = SpatialMatcherContext::new().spatial_match(target, pattern);
        println!("M2_CHILD_SURVIVED=true result={result:?}");
        return;
    }

    // ── parent ───────────────────────────────────────────────────────────────
    let (succeeded, text) = run_child(SUBJECT, "m2_the_matcher_panics_on_a_decoded_absent_child");
    assert!(
        text.contains("M2_CHILD_CONTROL_MATCHED=true"),
        "M2: the child's WELL-FORMED control did not match, so it never exercised the \
         `EMinus` arm and whatever killed it is not the absent child.\n\
         --- child output ---\n{text}"
    );
    assert!(
        !text.contains("M2_CHILD_SURVIVED=true"),
        "M2: the matcher did NOT fault; it answered. The premise of #127 is refuted and \
         the disposition must be re-derived.\n--- child output ---\n{text}"
    );
    assert!(
        !succeeded,
        "M2: the child exited cleanly on a target with an absent required child.\n\
         --- child output ---\n{text}"
    );
    assert!(
        text.contains("EMinus.p1"),
        "M2: the child died, but not at the site under test.\n--- child output ---\n{text}"
    );
    println!("  ⇒ M2: the fault is reached from a decoded value, at `EMinus.p1`");
}

/// M2b — the CONTROL: the same shapes, well formed, must not panic.
#[test]
fn m2b_control_the_well_formed_pair_matches() {
    let target = par_of(ExprInstance::EMinusBody(EMinus {
        p1: Some(new_gint_par(7, Vec::new(), false)),
        p2: Some(new_gint_par(2, Vec::new(), false)),
    }));
    let bytes = target.encode_to_vec();
    let target = Par::decode(&bytes[..]).expect("control decodes");

    let pattern = par_of(ExprInstance::EMinusBody(EMinus {
        p1: Some(new_freevar_par(0, Vec::new())),
        p2: Some(new_gint_par(2, Vec::new(), false)),
    }));

    let mut context = SpatialMatcherContext::new();
    let result = context.spatial_match(target, pattern);
    assert_eq!(
        result,
        Some(()),
        "M2b CONTROL: the well-formed pair must match, or M2's red is not attributable"
    );
    println!(
        "  ⇒ M2b control: well-formed pair MATCHES, binding {:?}",
        context.free_map.get(&0)
    );
}

// ===========================================================================
// M4 — REACHABILITY: the consensus wire path, driven end to end
// ===========================================================================

use std::collections::HashMap;

use models::rhoapi::{BindPattern, ListParWithRandom, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::chromadb_service::create_noop_chromadb_service;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::grpc_client_service::GrpcClientService;
use rholang::rust::interpreter::ollama_service::OllamaService;
use rholang::rust::interpreter::openai_service::create_noop_openai_service;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes_with_services;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

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

fn produces_of(event: &mut Event) -> &mut [Produce] {
    match event {
        Event::Comm(comm) => &mut comm.produces,
        Event::IoEvent(IOEvent::Produce(p)) => std::slice::from_mut(p),
        Event::IoEvent(IOEvent::Consume(_)) => &mut [],
    }
}

/// The malformed payload, byte-identical to M1's.
fn malformed_payload() -> Vec<u8> {
    wire_par(
        ExprInstance::EMinusBody(EMinus {
            p1: None,
            p2: Some(new_gint_par(2, Vec::new(), false)),
        }),
        true,
    )
    .encode_to_vec()
}

/// Receive shapes a DEPLOY can write, each aimed at a different way of
/// touching the spliced value.
const M4_PROGRAMS: &[(&str, &str)] = &[
    (
        "bare free variable — binds the whole term, descends into nothing",
        r#"new models(`rho:ollama:models`), ack in {
             models!(*ack) | for (@x <- ack) { Nil } }"#,
    ),
    (
        "EMINUS PATTERN — forces the matcher into the arm under test",
        r#"new models(`rho:ollama:models`), ack in {
             models!(*ack) | for (@{y - 2} <- ack) { Nil } }"#,
    ),
    (
        "bind then USE — the bound value is substituted and re-analysed",
        r#"new models(`rho:ollama:models`), ack in {
             models!(*ack) | for (@x <- ack) { @"sink"!(x) } }"#,
    ),
    (
        "bind then MATCH — the bound value becomes a match target",
        r#"new models(`rho:ollama:models`), ack in {
             models!(*ack) | for (@x <- ack) { match x { z - 2 => Nil  _ => Nil } } }"#,
    ),
    (
        "LIST pattern — descends one collection level",
        r#"new models(`rho:ollama:models`), ack in {
             models!(*ack) | for (@[a, b] <- ack) { Nil } }"#,
    ),
];

/// The #129 payload: a depth-34 nested list, which `Par::decode` REFUSES.
/// Used as the POSITIVE CONTROL — if splicing this does not turn the replay
/// red, the harness is not delivering the payload and every clean result above
/// is a false zero rather than a measurement.
fn elist(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

fn over_deep_payload() -> Vec<u8> {
    let mut par = new_gint_par(0, Vec::new(), false);
    for _ in 0..34 {
        par = elist(vec![par]);
    }
    par.encode_to_vec()
}

async fn drive_m4(label: &str, program: &str, payload: Vec<u8>) -> Vec<String> {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace store");
    let (mut play, replay, _h): (
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
    ) = create_runtimes_with_services(store, false, &mut Vec::new(), ollama_services()).await;

    let phlo = Cost::create(i64::MAX, "m4".to_string());
    let rand =
        crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&[0x5a; 32]);

    println!("\n── {label}");
    let play_result = play
        .evaluate(program, phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("play must run");
    println!("  play errors = {:?}", play_result.errors);
    let mut log = play.take_event_log().await;

    // ── the splice: exactly what a proposer writes into the block ───────────
    let mut with_output = 0usize;
    let mut spliced = 0usize;
    for event in log.iter_mut() {
        for produce in produces_of(event) {
            if !produce.output_value.is_empty() {
                with_output += 1;
                produce.output_value = vec![payload.clone()];
                spliced += 1;
            }
        }
    }
    println!("  produces carrying output_value = {with_output}; spliced = {spliced}");
    assert!(
        spliced > 0,
        "M4: nothing was spliced for {label} — the program never reached the op, so any \
         outcome below is unattributable"
    );

    replay.rig(log).await.expect("rig must accept the log");
    let replay_result = replay.evaluate(program, phlo, HashMap::new(), rand).await;
    match replay_result {
        Ok(r) => {
            let errors: Vec<String> = r.errors.iter().map(|e| e.to_string()).collect();
            println!("  ⇒ replay RETURNED. errors = {errors:?}");
            errors
        }
        Err(e) => {
            println!("  ⇒ replay ERRORED: {e:?}");
            vec![e.to_string()]
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn m4_the_replay_path_carries_a_malformed_par_from_the_block() {
    // ── ★ THE POSITIVE CONTROL, run FIRST ───────────────────────────────
    let control = drive_m4(
        "POSITIVE CONTROL: #129's depth-34 payload, which Par::decode REFUSES",
        M4_PROGRAMS[0].1,
        over_deep_payload(),
    )
    .await;
    assert!(
        control.iter().any(|e| e.contains("recursion limit")),
        "★★ M4 POSITIVE CONTROL FAILED: splicing a payload `Par::decode` is KNOWN to \
         refuse produced {control:?} instead of a recursion-limit error. The harness is \
         therefore NOT delivering spliced `output_value` bytes to the replay decode, and \
         every clean result below would be a FALSE ZERO. Fix the harness before reading \
         anything into the treatment cells."
    );

    for (label, program) in M4_PROGRAMS {
        let errors = drive_m4(label, program, malformed_payload()).await;
        println!("     [{label}] -> {errors:?}");
    }
}
