//! # ★★ The `prost` read ceiling as a CONSENSUS fault, made executable
//!
//! `models/tests/par_prost_depth_ceiling.rs` shows that a `Par` of nesting
//! depth 34 encodes and does not decode. On its own that is a client
//! inconvenience. **This file is the leg that makes it a consensus fault**, and
//! the distinction is the whole point:
//!
//! > **The proposer never decodes. The validator always decodes.**
//!
//! Same binary, same program, same term, same seed — and the run differs in one
//! bool.
//!
//! ## The asymmetry, traced through production code
//!
//! * **Play.** `Produce::create` (`rspace++/src/rspace/trace/event.rs`) sets
//!   `output_value: vec![]`. `RSpace::locked_produce` returns exactly that
//!   freshly-created `Produce`, so when `produce_inner` (`reduce.rs:919-927`)
//!   hands `produce_event.output_value` to `continue_produce_process`, the
//!   vector is **empty** — the `.map(Par::decode)` at `reduce.rs:1065` iterates
//!   nothing. Then the bytes are *written* into the block.
//! * **Replay.** `ReplayRSpace::locked_produce`
//!   (`rspace++/src/rspace/replay_rspace.rs`) returns
//!   `comm.produces.into_iter().find(|p| p.hash == produce_ref.hash)` — the
//!   `Produce` **from the trace**, whose `output_value` came from the block.
//!   The same `reduce.rs:1065` now decodes real bytes.
//!
//! `Err(DecodeError)` becomes an entry in `EvaluateResult::errors`, which makes
//! `eval_successful = false` (`casper/src/rust/rholang/replay_runtime.rs:427`),
//! which fails the `processed_deploy.is_failed != !eval_successful` check at
//! `:443` and returns `CasperError::ReplayFailure`. **The proposer builds a
//! block no validator can replay.**
//!
//! ## Why the bytes get that far in the first place
//!
//! `ProduceEventProto.outputValue` is `repeated bytes`
//! (`models/src/main/protobuf/CasperMessage.proto:393`), not a nested message.
//! Decoding the block body therefore does **not** descend into those bytes and
//! cannot reject them; and because the eventual `Par::decode` is a fresh
//! top-level decode, it gets `prost`'s full 100-level budget and sits at the
//! **bare** ceiling — 33 accepted, 34 refused. The block is well-formed; only
//! replay is not.
//!
//! ## ⚠ Not reachable today — and what the guard actually is
//!
//! `output_value` is written from one site, gated on `non_deterministic_ops()`.
//! All eight of those operations' return constructions were read and none
//! exceeds depth 3, the deepest sitting behind a non-default feature. **But the
//! guard is those eight functions' return shapes, not a check.** A ninth
//! operation that echoes a caller-supplied `Par` makes this live with no other
//! change — which is precisely the mutation this file performs on the recorded
//! log.
//!
//! ## What this file does NOT do
//!
//! It does not bound the write side, does not widen what a node accepts, and
//! changes no production code. Widening the read side changes the set of byte
//! strings a node accepts and needs a coordinated version bump — F1r3node's
//! decision. See `docs/design/audits/theta-depth-traversals-2026-07-26.md`
//! §7.3, whose instruction this discharges: the ceiling *"must be surfaced
//! before it is discovered by a validator."*
//!
//! ## Method
//!
//! 1. Run [`PROGRAM`] on the play runtime and take its event log.
//! 2. Assert every `Produce` in that log carries `output_value == []` — the
//!    executable form of *"the proposer never decodes"*.
//! 3. Splice `nested_list(d).encode_to_vec()` into `output_value`, exactly as a
//!    block produced by a node with a ninth non-deterministic op would carry
//!    it. `Produce`'s `PartialEq`/`Hash`/`Ord` are **hash-only** by design
//!    (`event.rs`: *"metadata fields like `is_deterministic`, `output_value`,
//!    and `failed` ... must NOT affect identity"*), so the splice cannot
//!    perturb event identity and the rig still matches.
//! 4. `rig` the mutated log into the replay runtime and evaluate the same
//!    program with the same seed.
//!
//! [`PROGRAM`] is shaped so the send that fires the COMM is emitted from a
//! continuation, which puts the COMM on the **produce** side (only
//! `continue_produce_process` receives a non-empty `previous_output`;
//! `continue_consume_process` is always handed `Vec::new()` at `reduce.rs`).
//! The whole file runs on a `current_thread` runtime so the interleaving is not
//! the scheduler's choice; the outcome was confirmed identical over 25
//! consecutive runs before it was committed.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent, Produce};

/// The last nesting depth a bare `Par::decode` accepts, and the first it
/// refuses. Pinned — with its arithmetic and its eight sibling envelopes — by
/// `models/tests/par_prost_depth_ceiling.rs`.
const LAST_DECODING_DEPTH: usize = 33;
const FIRST_REFUSED_DEPTH: usize = 34;

/// A COMM fired by a **produce**: `ch!(42)` is emitted by the continuation of
/// the `boot` COMM, so the receive on `ch` is already installed when it lands.
/// That routes the trace's `output_value` through `continue_produce_process`,
/// which is the decode site this file is about.
const PROGRAM: &str = r#"
    new ch, boot in {
      for (@x <- ch) { @"sink"!(x) } |
      boot!(0) |
      for (@_ <- boot) { ch!(42) }
    }
"#;

// ---------------------------------------------------------------------------
// term construction — ITERATIVE, so the builder is never the constraint
// ---------------------------------------------------------------------------

fn elist(ps: Vec<Par>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    }
}

/// `[[[…[0]…]]]` with `depth` bracket levels — byte-identical to the fixture
/// `models/tests/par_prost_depth_ceiling.rs` builds, so the two files cannot
/// disagree about which term the boundary is about.
fn nested_list(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p
}

// ---------------------------------------------------------------------------
// the driver
// ---------------------------------------------------------------------------

/// What one play-then-replay pass observed.
struct Pass {
    /// `is_replay` as the play runtime's own space reports it. Must be `false`.
    play_is_replay: bool,
    /// `is_replay` as the replay runtime's own space reports it. Must be `true`.
    replay_is_replay: bool,
    play_errors: Vec<InterpreterError>,
    replay_errors: Vec<InterpreterError>,
    /// `Produce`s found in the recorded log, and how many carried an EMPTY
    /// `output_value` before the splice. The proposer writes nothing, so these
    /// must be equal.
    produces_recorded: usize,
    produces_empty_before_splice: usize,
    /// How many `output_value` slots the splice filled.
    spliced: usize,
}

/// Every `Produce` reachable in one log entry: the COMM's participants, and the
/// standalone `IoEvent::Produce` rows. `rig` reads both.
fn produces_of(event: &mut Event) -> &mut [Produce] {
    match event {
        Event::Comm(comm) => &mut comm.produces,
        Event::IoEvent(IOEvent::Produce(produce)) => std::slice::from_mut(produce),
        Event::IoEvent(IOEvent::Consume(_)) => &mut [],
    }
}

/// Play [`PROGRAM`], splice `splice_depth` into the recorded log's
/// `output_value`s (when `Some`), then replay it.
async fn play_then_replay(splice_depth: Option<usize>) -> Pass {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace store");
    let (mut runtime, replay_runtime, _history) =
        create_runtimes(store, false, &mut Vec::new()).await;

    // ★ THE ONE BOOL, read from the two spaces themselves rather than assumed.
    // It is what `produce_inner` threads into `continue_produce_process`.
    let play_is_replay = runtime.reducer.space.is_replay().await;
    let replay_is_replay = replay_runtime.reducer.space.is_replay().await;

    let phlo = Cost::create(i64::MAX, "replay output_value depth ceiling".to_string());
    let rand = Blake2b512Random::create_from_bytes(&[0x77; 32]);

    // ── PLAY ────────────────────────────────────────────────────────────────
    let play = runtime
        .evaluate(PROGRAM, phlo.clone(), HashMap::new(), rand.clone())
        .await
        .expect("play evaluation must run");
    let mut log = runtime.take_event_log().await;

    // ── the block's `outputValue`, as a node with a ninth op would write it ──
    let payload = splice_depth.map(|d| nested_list(d).encode_to_vec());
    let mut produces_recorded = 0usize;
    let mut produces_empty_before_splice = 0usize;
    let mut spliced = 0usize;
    for event in log.iter_mut() {
        for produce in produces_of(event) {
            produces_recorded += 1;
            if produce.output_value.is_empty() {
                produces_empty_before_splice += 1;
            }
            if let Some(bytes) = &payload {
                produce.output_value = vec![bytes.clone()];
                spliced += 1;
            }
        }
    }

    // ── REPLAY ──────────────────────────────────────────────────────────────
    replay_runtime
        .rig(log)
        .await
        .expect("the replay rig must accept the recorded log");
    let replay = replay_runtime
        .evaluate(PROGRAM, phlo, HashMap::new(), rand)
        .await
        .expect("replay evaluation must run");

    Pass {
        play_is_replay,
        replay_is_replay,
        play_errors: play.errors,
        replay_errors: replay.errors,
        produces_recorded,
        produces_empty_before_splice,
        spliced,
    }
}

/// `true` iff `errors` contains a decode failure attributable to `prost`'s
/// recursion limit — the error KIND, not merely "something failed".
///
/// `InterpreterError::DecodeError` carries `prost::DecodeError::to_string()`,
/// whose `Display` ends in the root cause. Matching the suffix rather than
/// `is_err()` keeps a truncated buffer or a wrong field number from passing as
/// this finding.
fn has_recursion_limit_decode_error(errors: &[InterpreterError]) -> bool {
    errors.iter().any(|e| match e {
        InterpreterError::DecodeError(message) => message.ends_with("recursion limit reached"),
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// ★★ the leg
// ---------------------------------------------------------------------------

/// ★★ **Red on replay, green on play — same binary, same term, one bool.**
///
/// The 2×2 this asserts:
///
/// | | depth 33 | depth 34 |
/// |---|---|---|
/// | play (`is_replay = false`) | green | **green** |
/// | replay (`is_replay = true`) | green | **RED — `DecodeError(… recursion limit reached)`** |
///
/// The depth-33 row is the adjacent control: it is the deepest term the wire
/// carries, it goes through the identical splice-and-rig machinery, and it
/// replays clean. Without it a red cell proves only that *something* about deep
/// terms upsets replay; with it, the red cell is the boundary.
#[tokio::test(flavor = "current_thread")]
async fn the_replay_decode_of_output_value_is_the_consensus_class_member() {
    // ── no splice: the machinery itself must be quiet ───────────────────────
    let baseline = play_then_replay(None).await;
    assert!(
        !baseline.play_is_replay && baseline.replay_is_replay,
        "the two runtimes do not differ in `is_replay` ({} vs {}). Every claim \
         below is about that one bool; if both spaces report the same thing, the \
         fixture is comparing a runtime with itself.",
        baseline.play_is_replay,
        baseline.replay_is_replay
    );
    assert!(
        baseline.produces_recorded > 0,
        "the play run recorded no `Produce` at all, so the splice below would have \
         nothing to write into and the whole test would be vacuously green. The \
         program is not firing the COMM it is written to fire."
    );
    assert_eq!(
        baseline.produces_empty_before_splice, baseline.produces_recorded,
        "★ {} of {} recorded `Produce`s already carried a non-empty \
         `output_value`. `Produce::create` sets `output_value: vec![]`, and 'the \
         proposer never decodes' rests on that: if the play path is now writing \
         payloads, the asymmetry this file documents has changed shape and the \
         audit's §7.3 analysis must be re-derived before anything here is trusted.",
        baseline.produces_recorded - baseline.produces_empty_before_splice,
        baseline.produces_recorded
    );
    assert!(
        baseline.play_errors.is_empty() && baseline.replay_errors.is_empty(),
        "the unspliced program does not play-and-replay cleanly (play: {:?}, \
         replay: {:?}), so a red cell below could not be attributed to the splice",
        baseline.play_errors,
        baseline.replay_errors
    );

    // ── the CONTROL: depth 33, the deepest the wire carries ─────────────────
    let control = play_then_replay(Some(LAST_DECODING_DEPTH)).await;
    assert!(
        control.spliced > 0,
        "the depth-{LAST_DECODING_DEPTH} control spliced nothing"
    );
    assert!(
        control.play_errors.is_empty(),
        "depth {LAST_DECODING_DEPTH} on the PLAY side: {:?}",
        control.play_errors
    );
    assert!(
        control.replay_errors.is_empty(),
        "★ depth {LAST_DECODING_DEPTH} must REPLAY CLEAN — it is the last depth a \
         bare `Par::decode` accepts. It failed with {:?}. Either the read ceiling \
         moved down (this node now rejects byte strings it used to accept — a \
         consensus-visible narrowing) or the splice is malformed, in which case \
         the depth-{FIRST_REFUSED_DEPTH} result below proves nothing.",
        control.replay_errors
    );

    // ── the DEFECT: depth 34 ────────────────────────────────────────────────
    let over = play_then_replay(Some(FIRST_REFUSED_DEPTH)).await;
    assert_eq!(
        over.spliced, control.spliced,
        "the depth-{FIRST_REFUSED_DEPTH} pass spliced a different number of \
         `output_value` slots than the depth-{LAST_DECODING_DEPTH} control, so the \
         two are not the same experiment"
    );
    assert!(
        over.play_errors.is_empty(),
        "★★ depth {FIRST_REFUSED_DEPTH} must be GREEN ON PLAY: {:?}. The proposer \
         does not decode `output_value` — it writes it. A red cell here would mean \
         the play path has started decoding, which would make the block \
         unbuildable rather than unreplayable, and would change the defect's \
         class.",
        over.play_errors
    );
    assert!(
        has_recursion_limit_decode_error(&over.replay_errors),
        "★★ depth {FIRST_REFUSED_DEPTH} must be RED ON REPLAY with a recursion-limit \
         `DecodeError`, and it was not: {:?}.\n\
         \n\
         If the replay is now CLEAN, the bare read ceiling has moved up and this \
         node accepts byte strings it used to reject — a consensus-visible \
         widening that needs a coordinated version bump (audit §7.3). If it failed \
         for some OTHER reason, the fixture stopped reaching `reduce.rs:1065` and \
         is no longer measuring the decode.",
        over.replay_errors
    );

    println!(
        "  the one bool: play is_replay={} / replay is_replay={}",
        over.play_is_replay, over.replay_is_replay
    );
    println!(
        "  {} recorded produces, all with output_value=[] before the splice; {} \
         spliced per pass",
        baseline.produces_recorded, over.spliced
    );
    println!(
        "  depth {LAST_DECODING_DEPTH}: play GREEN, replay GREEN   |   depth \
         {FIRST_REFUSED_DEPTH}: play GREEN, replay RED (recursion limit reached)"
    );
}
