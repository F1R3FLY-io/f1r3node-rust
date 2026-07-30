//! # The driver's own guards, watched RED on every run
//!
//! `models/src/rust/rholang/drive.rs` rejects a malformed machine
//! configuration in three places. Until this file existed, none of them had
//! ever been SHOWN to reject anything: they were assertions that had only ever
//! been satisfied, which is indistinguishable from assertions that accept
//! everything. A checker never seen red is not evidence.
//!
//! The real instances cannot supply the missing evidence — they are correct, and
//! *"the machine is well-formed, so a malformed configuration cannot be
//! reproduced"* would retire the obligation for every guard whose subject is
//! working, which is all of them. So this file grows its own malformed
//! visitors, each violating exactly one clause.
//!
//! ## ⚠ Why child processes, and never `#[should_panic]`
//!
//! This workspace forbids a test that *expects a panic*: a `panic!` that fails
//! to unwind aborts printing nothing, and a stack overflow is a `SIGSEGV` that
//! is not catchable at all. So every negative subject runs in a **child
//! process** and the parent reads the child's **exit status** and **output** —
//! the same mechanism `rholang/tests/stack_depth_gate.rs` uses. The bonus is
//! that the parent can then assert on the diagnostic *text*, which is what
//! makes "it failed" into "it failed for the stated reason".
//!
//! ## ★ Why each negative subject is asserted in a profile-aware way
//!
//! `drive.rs` splits its checks deliberately:
//!
//! | check | compiled when | this file's subject |
//! |---|---|---|
//! | Invariant 1 — local suspension well-formedness | `debug_assertions` | `region_underfull`, `region_overfull` |
//! | Invariant 2 — the deficit invariant `V + D + C − A == 1` | `debug_assertions` | `pops_nothing` |
//! | the **final** configuration (`V == 1`, `work` drained) | **always** | `pops_nothing`, in release |
//!
//! So `pops_nothing` is the subject that pins both ends of one invariant: in a
//! debug build the running check catches it mid-flight, and in a release build —
//! where that check does not exist — the unconditional final-configuration
//! assertion catches it at the end. The parent requires the message that
//! belongs to the profile it was compiled for, so **each message is pinned in
//! the profile where it is the one that fires**, and neither can rot unnoticed.
//!
//! ## The positive subjects
//!
//! `well_formed` and `early_exit` are here for the same reason the negative
//! ones are: a gate made only of rejections cannot tell "correctly rejects the
//! malformed" from "rejects everything". `early_exit` additionally *counts
//! nodes visited*, because [`Outcome::Done`] exists to make a traversal stop —
//! a `Done` that returned the right answer after walking the whole term would
//! satisfy an equality assertion while failing at its only job.

use std::process::{Command, Stdio};

use models::rust::rholang::drive::{drive, Outcome, Step, Traversal};

// ===========================================================================
// §A  The subject machine: a unary chain, summed
// ===========================================================================

/// One node of a `depth`-long chain. `Copy`, as [`Traversal::Node`] requires.
#[derive(Clone, Copy)]
struct Chain(usize);

/// The only continuation: add one to the child's value.
enum ChainKont {
    /// Consumes ONE value.
    Succ,
    /// Consumes TWO — used only by the malformed `region_underfull` visitor, to
    /// declare an arity its `descend` does not supply children for.
    Pair,
}

/// A traversal cannot fail, but [`Traversal::Err`] must be inhabited-or-not by
/// *some* type; `Infallible` says "no error is reachable" in the type system
/// rather than in a comment.
type Never = std::convert::Infallible;

/// Nodes visited, so `early_exit` can show that it stopped rather than merely
/// answering correctly.
#[derive(Default)]
struct Visits {
    descends: usize,
    combines: usize,
}

/// The WELL-FORMED machine: `drive` over `Chain(n)` yields `n + 1`.
struct WellFormed;

impl Traversal for WellFormed {
    type Node<'t> = Chain;
    type Val = u64;
    type Kont<'t> = ChainKont;
    type State = Visits;
    type Err = Never;

    fn descend<'t>(
        &mut self,
        st: &mut Visits,
        node: Chain,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node.0 {
            0 => {
                vals.push(1);
                Ok(())
            }
            n => {
                work.push(Step::Combine(ChainKont::Succ));
                work.push(Step::Descend(Chain(n - 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Visits,
        kont: ChainKont,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, Chain>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        let popped = pop_for(&kont, vals);
        Ok(Outcome::Value(popped + 1))
    }

    fn arity(kont: &ChainKont) -> usize {
        match kont {
            ChainKont::Succ => 1,
            ChainKont::Pair => 2,
        }
    }
}

/// Pop what a continuation declares, and return the first child's value.
fn pop_for(kont: &ChainKont, vals: &mut Vec<u64>) -> u64 {
    match kont {
        ChainKont::Succ => vals.pop().expect("drive: value stack underflow"),
        ChainKont::Pair => {
            let b = vals.pop().expect("drive: value stack underflow");
            let _a = vals.pop().expect("drive: value stack underflow");
            b
        }
    }
}

/// EARLY EXIT. Identical to [`WellFormed`], except that the continuation which
/// first sees a value `>= EARLY_AT` reports it as the answer.
///
/// ★ This is the mechanism [`Outcome::Done`] exists for. `models/src/lib.rs`'s
/// `impl PartialEq for Par` is a `&&` chain that stops at the first unequal
/// field; a post-order fold with no early exit walks both terms to completion,
/// which would be a performance-correctness regression against the code it
/// replaces. The gate proves the driver really stops, by counting.
struct EarlyExit;

/// The value at which [`EarlyExit`] breaks. Small, so the *unvisited* remainder
/// of the chain is large and the count is unambiguous.
const EARLY_AT: u64 = 4;

impl Traversal for EarlyExit {
    type Node<'t> = Chain;
    type Val = u64;
    type Kont<'t> = ChainKont;
    type State = Visits;
    type Err = Never;

    fn descend<'t>(
        &mut self,
        st: &mut Visits,
        node: Chain,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node.0 {
            0 => {
                vals.push(1);
                Ok(())
            }
            n => {
                work.push(Step::Combine(ChainKont::Succ));
                work.push(Step::Descend(Chain(n - 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Visits,
        kont: ChainKont,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, Chain>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        let v = pop_for(&kont, vals) + 1;
        if v >= EARLY_AT {
            Ok(Outcome::Done(v))
        } else {
            Ok(Outcome::Value(v))
        }
    }

    fn arity(kont: &ChainKont) -> usize {
        match kont {
            ChainKont::Succ => 1,
            ChainKont::Pair => 2,
        }
    }
}

/// MALFORMED — `arity()` says 1, `combine` pops 0.
///
/// Leaves exactly one obligation unpopped per combine. In debug this violates
/// Invariant 2 at the next loop head; in release it survives to the drained
/// work stack and is caught by the unconditional final-configuration assertion.
struct PopsNothing;

impl Traversal for PopsNothing {
    type Node<'t> = Chain;
    type Val = u64;
    type Kont<'t> = ChainKont;
    type State = Visits;
    type Err = Never;

    fn descend<'t>(
        &mut self,
        st: &mut Visits,
        node: Chain,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node.0 {
            0 => {
                vals.push(1);
                Ok(())
            }
            n => {
                work.push(Step::Combine(ChainKont::Succ));
                work.push(Step::Descend(Chain(n - 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Visits,
        _kont: ChainKont,
        _vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, Chain>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        // ⚠ THE DEFECT: declares arity 1 and pops nothing.
        Ok(Outcome::Value(1))
    }

    fn arity(kont: &ChainKont) -> usize {
        match kont {
            ChainKont::Succ => 1,
            ChainKont::Pair => 2,
        }
    }
}

/// MALFORMED — a continuation of arity 2 pushed with only ONE child.
///
/// Invariant 1's `avail >= arity` clause. This is the defect the hand-written
/// machines' "push the `Combine` first, then exactly `arity` children" rule
/// existed to catch, and which the driver's right-to-left scan has to keep
/// catching now that it also admits nested continuations.
struct RegionUnderfull;

impl Traversal for RegionUnderfull {
    type Node<'t> = Chain;
    type Val = u64;
    type Kont<'t> = ChainKont;
    type State = Visits;
    type Err = Never;

    fn descend<'t>(
        &mut self,
        st: &mut Visits,
        node: Chain,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node.0 {
            0 => {
                vals.push(1);
                Ok(())
            }
            n => {
                // ⚠ THE DEFECT: `Pair` has arity 2, and gets one child.
                work.push(Step::Combine(ChainKont::Pair));
                work.push(Step::Descend(Chain(n - 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Visits,
        kont: ChainKont,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, Chain>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        Ok(Outcome::Value(pop_for(&kont, vals) + 1))
    }

    fn arity(kont: &ChainKont) -> usize {
        match kont {
            ChainKont::Succ => 1,
            ChainKont::Pair => 2,
        }
    }
}

/// MALFORMED — a branching `descend` that ALSO pushes a value.
///
/// Invariant 1's leaf/branch exclusivity clause: a step produces exactly one
/// value, so a call that pushed work has already delegated that production.
struct RegionOverfull;

impl Traversal for RegionOverfull {
    type Node<'t> = Chain;
    type Val = u64;
    type Kont<'t> = ChainKont;
    type State = Visits;
    type Err = Never;

    fn descend<'t>(
        &mut self,
        st: &mut Visits,
        node: Chain,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node.0 {
            0 => {
                vals.push(1);
                Ok(())
            }
            n => {
                work.push(Step::Combine(ChainKont::Succ));
                work.push(Step::Descend(Chain(n - 1)));
                // ⚠ THE DEFECT: a branching descend also produces a value.
                vals.push(99);
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Visits,
        kont: ChainKont,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, Chain>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        Ok(Outcome::Value(pop_for(&kont, vals) + 1))
    }

    fn arity(kont: &ChainKont) -> usize {
        match kont {
            ChainKont::Succ => 1,
            ChainKont::Pair => 2,
        }
    }
}

// ===========================================================================
// §B  The child harness
// ===========================================================================

/// Chain length every arm is driven at. Long enough that `early_exit` leaves a
/// large unvisited remainder, short enough to be instant.
const CHAIN: usize = 64;

const GATE_ARM: &str = "DRIVE_GATE_ARM";

/// The child entry point's full test path, as libtest's `--exact` wants it.
const CHILD: &str = "gate_child";

/// Printed by a child that ran to completion, so the parent can tell "the
/// subject succeeded" from "the subject never ran".
const OK_MARKER: &str = "DRIVE_GATE_OK";

fn run_arm(arm: &str) {
    let mut visits = Visits::default();
    let (value, visits) = match arm {
        "well_formed" => {
            let v = drive(&mut WellFormed, &mut visits, Step::Descend(Chain(CHAIN)))
                .expect("well_formed cannot fail");
            (v, visits)
        }
        "early_exit" => {
            let v = drive(&mut EarlyExit, &mut visits, Step::Descend(Chain(CHAIN)))
                .expect("early_exit cannot fail");
            (v, visits)
        }
        "pops_nothing" => {
            let v = drive(&mut PopsNothing, &mut visits, Step::Descend(Chain(CHAIN)))
                .expect("pops_nothing cannot fail");
            (v, visits)
        }
        "region_underfull" => {
            let v = drive(
                &mut RegionUnderfull,
                &mut visits,
                Step::Descend(Chain(CHAIN)),
            )
            .expect("region_underfull cannot fail");
            (v, visits)
        }
        "region_overfull" => {
            let v = drive(&mut RegionOverfull, &mut visits, Step::Descend(Chain(CHAIN)))
                .expect("region_overfull cannot fail");
            (v, visits)
        }
        // ★★ The `Outcome::Tail` arms run on their own `Trail` state, so they are
        // dispatched before the `Visits` arms rather than inside them.
        tail_arm if tail_arm.starts_with("tail_") => return run_tail_arm(tail_arm),
        other => panic!("drive_configuration_gate: unknown {GATE_ARM}={other:?}"),
    };
    println!(
        "{OK_MARKER} arm={arm} value={value} descends={} combines={}",
        visits.descends, visits.combines
    );
}

/// The `Outcome::Tail` arms. Separate from [`run_arm`] because their
/// [`Traversal::State`] is a [`Trail`] rather than a [`Visits`] — the byte trail
/// IS the subject.
fn run_tail_arm(arm: &str) {
    // ⚠ `Step<'_, T>` is parameterised by the visitor, so the root cannot be one
    // shared binding across arms — each arm builds its own.
    fn root<T>() -> Step<'static, T>
    where
        T: Traversal<Node<'static> = SeqNode>,
    {
        Step::Descend(SeqNode::Seq {
            len: SEQ_LEN,
            cursor: 0,
        })
    }
    let mut st = Trail::default();
    let value = match arm {
        "tail_resumable" => drive(&mut Resumable, &mut st, root()).expect("cannot fail"),
        "tail_arity_lies" => drive(&mut TailArityLies, &mut st, root()).expect("cannot fail"),
        "tail_region_underfull" => {
            drive(&mut TailRegionUnderfull, &mut st, root()).expect("cannot fail")
        }
        "tail_forever" => drive(&mut TailsForever, &mut st, root()).expect("cannot fail"),
        "tail_leaves_two_values" => {
            drive(&mut TailLeavesTwoValues, &mut st, root()).expect("cannot fail")
        }
        "tail_undeclared" => drive(&mut TailUndeclared, &mut st, root()).expect("cannot fail"),
        other => panic!("drive_configuration_gate: unknown tail arm {other:?}"),
    };
    println!(
        "{OK_MARKER} arm={arm} value={value} descends={} combines={} tails={} trail={}",
        st.descends,
        st.combines,
        st.tails,
        String::from_utf8_lossy(&st.bytes)
    );
}

/// The child entry point.
///
/// ⚠ A NO-OP when its environment is absent, because `cargo test --
/// --include-ignored` and `cargo nextest run --run-ignored all` execute every
/// `#[ignore]`d test. A child that *required* its environment would fail the
/// suite for a reason unrelated to the property. Skipping is right here
/// precisely because this test is a mechanism; the assertions are in its
/// callers.
#[test]
#[ignore = "child process of the drive configuration gate; driven via DRIVE_GATE_ARM"]
fn gate_child() {
    let Ok(arm) = std::env::var(GATE_ARM) else {
        println!("gate_child: no {GATE_ARM} — not a child invocation, nothing to do");
        return;
    };
    run_arm(&arm);
}

/// What a child invocation produced.
struct Child {
    ok: bool,
    output: String,
}

fn run_child(arm: &str) -> Child {
    let exe = std::env::current_exe().expect("drive_configuration_gate: current_exe");
    let out = Command::new(exe)
        // `--nocapture` so the child's panic message reaches its own stderr
        // verbatim instead of being folded into libtest's captured summary.
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env(GATE_ARM, arm)
        .stdin(Stdio::null())
        .output()
        .expect("drive_configuration_gate: failed to run the child");
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Child {
        ok: out.status.success(),
        output,
    }
}

/// Parse `descends=` / `combines=` back out of a successful child's marker.
fn counts(output: &str) -> (usize, usize) {
    let field = |key: &str| -> usize {
        output
            .split_whitespace()
            .find_map(|tok| tok.strip_prefix(key))
            .unwrap_or_else(|| panic!("no {key} in child output:\n{output}"))
            .parse()
            .expect("a count")
    };
    (field("descends="), field("combines="))
}

// ===========================================================================
// §C  The positive subjects — what makes the rejections meaningful
// ===========================================================================

#[test]
fn the_driver_runs_a_well_formed_traversal_to_completion() {
    let child = run_child("well_formed");
    assert!(
        child.ok,
        "CONTROL FAILED: the well-formed traversal did not complete. Every rejection \
         asserted below is meaningless until this passes — a driver that rejected \
         EVERYTHING would satisfy them all.\n{}",
        child.output
    );
    assert!(
        child.output.contains(&format!("{OK_MARKER} arm=well_formed")),
        "the child exited 0 without running the subject:\n{}",
        child.output
    );
    assert!(
        child
            .output
            .contains(&format!("value={}", CHAIN as u64 + 1)),
        "a {CHAIN}-long chain summed to the wrong value; the driver's post-order is \
         wrong:\n{}",
        child.output
    );
    let (descends, combines) = counts(&child.output);
    assert_eq!(
        (descends, combines),
        (CHAIN + 1, CHAIN),
        "a full walk of a {CHAIN}-long chain is {} descends and {CHAIN} combines",
        CHAIN + 1
    );
}

/// ★ [`Outcome::Done`] must actually STOP the machine.
#[test]
fn early_exit_stops_the_walk_rather_than_finishing_it() {
    let child = run_child("early_exit");
    assert!(
        child.ok,
        "the early-exit traversal did not complete:\n{}",
        child.output
    );
    assert!(
        child.output.contains(&format!("value={EARLY_AT}")),
        "early exit returned the wrong value; `Outcome::Done(v)` must return exactly `v`:\n{}",
        child.output
    );

    let (descends, combines) = counts(&child.output);
    // ★ The chain is walked all the way DOWN before any combine runs — every
    // `descend` pushes `[Combine(Succ), Descend(n-1)]`, so all `CHAIN + 1`
    // descents happen first and the `CHAIN` continuations pile up behind them.
    // What `Outcome::Done` saves is therefore the ASCENT, and the descents are
    // not a defect in the measurement.
    //
    // ⚠ The count is `EARLY_AT - 1`, not `EARLY_AT`, and this assertion caught
    // that off-by-one when it was written the other way. The LEAF already
    // produces value 1 without a combine, so the combine that first sees value
    // `v` is the `(v - 1)`-th: 1 -> 2 -> 3 -> 4 is three combines to reach 4.
    let expected_combines = EARLY_AT as usize - 1;
    assert_eq!(
        combines, expected_combines,
        "early exit ran {combines} combines; it must run exactly {expected_combines} — the \
         leaf produces value 1 with no combine, so reaching {EARLY_AT} takes \
         {EARLY_AT} - 1 ascending steps."
    );
    assert!(
        combines < descends,
        "VACUOUS: early exit ran {combines} combines against {descends} descends, so it \
         did not demonstrably stop early."
    );
    // The real statement: strictly fewer combines than the full walk needs.
    let (_, full_combines) = counts(&run_child("well_formed").output);
    assert!(
        combines < full_combines,
        "★ EARLY EXIT IS NOT EXITING. It ran {combines} combines where the complete walk \
         runs {full_combines}. `Outcome::Done` must make `drive` return immediately and \
         drop the pending obligations — without that, a comparison traversal walks both \
         terms to completion and is a performance-correctness REGRESSION against the \
         short-circuiting `&&` chain in `models/src/lib.rs`'s `impl PartialEq for Par`."
    );
}

// ===========================================================================
// §D  The rejections
// ===========================================================================

/// The message fragment every `drive.rs` configuration diagnostic carries.
const COMMON: &str = "drive: MALFORMED";

fn assert_rejected(arm: &str, expected: &[&str]) {
    let child = run_child(arm);
    assert!(
        !child.ok,
        "★ THE GUARD ACCEPTED A MALFORMED CONFIGURATION. Arm {arm:?} ran to completion; \
         `drive.rs` must reject it. A checker that accepts everything is \
         indistinguishable from no checker.\n{}",
        child.output
    );
    assert!(
        child.output.contains(COMMON),
        "arm {arm:?} failed, but not with a `drive.rs` configuration diagnostic — so it \
         failed for a reason that is not its subject, and this assertion has shown \
         nothing:\n{}",
        child.output
    );
    for fragment in expected {
        assert!(
            child.output.contains(fragment),
            "arm {arm:?} was rejected, but the diagnostic did not contain {fragment:?}. \
             The message a maintainer will read is part of the guard.\n{}",
            child.output
        );
    }
}

/// ★★ THE SUBJECT THAT PINS BOTH ENDS OF INVARIANT 2.
///
/// In a **debug** build the running deficit check catches it mid-flight; in a
/// **release** build, where that check is not compiled, the unconditional
/// final-configuration assertion catches it on the drained work stack. Each
/// message is required in the profile where it is the one that fires, so
/// neither can rot unnoticed.
#[test]
fn a_kont_that_pops_fewer_values_than_its_arity_claims_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "pops_nothing",
            &["DEFICIT INVARIANT", "V + D + C - A == 1", "Invariant 2"],
        );
    } else {
        assert_rejected(
            "pops_nothing",
            &[
                "MALFORMED FINAL CONFIGURATION",
                "obligation that was never popped",
                "checked UNCONDITIONALLY",
            ],
        );
    }
}

/// Invariant 1, `avail >= arity`. `debug_assertions`-only by design, so the
/// release expectation is only that the malformed machine does not silently
/// produce an answer.
#[test]
fn a_continuation_pushed_with_too_few_children_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "region_underfull",
            &["declares arity 2", "Invariant 1"],
        );
    } else {
        let child = run_child("region_underfull");
        assert!(
            !child.ok,
            "arm `region_underfull` produced an answer. Invariant 1 is \
             `debug_assertions`-only, so in release the underflow surfaces from the \
             visitor's own `vals.pop()` instead — but it must still not SUCCEED.\n{}",
            child.output
        );
    }
}

/// Invariant 1, leaf/branch exclusivity. `debug_assertions`-only by design.
///
/// ⚠ In release this defect is genuinely SILENT — the extra value rides the
/// stack to the end, where the unconditional final-configuration assertion
/// catches it. That is exactly why that one assertion is not a `debug_assert`.
#[test]
fn a_branching_descend_that_also_pushes_a_value_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "region_overfull",
            &["must push", "NO value", "Invariant 1"],
        );
    } else {
        assert_rejected(
            "region_overfull",
            &["MALFORMED FINAL CONFIGURATION", "checked UNCONDITIONALLY"],
        );
    }
}

// ===========================================================================
// §E  ★★ `Outcome::Tail` — the resumable subject, and the four RED legs
// ===========================================================================
//
// `Tail` shipped with no production client: its client is the wire encoder, a
// separate stage. A mechanism with no client is dead code, and this workspace has
// floors against exactly that — so it ships with an **executed** client here, and
// that client is the control every rejection below depends on.
//
// ## ★ Why this subject and not a second copy of `Chain`
//
// `Tail` exists for a traversal whose parent discovers its next obligation only
// *after* the previous child completes, because **bytes sit between the
// children**. `Chain` is a post-order fold: it knows its one child up front and
// has no use for resumption. So the subject here writes a real byte trail —
// `[`, then `,` before each item, then `]` — into `Traversal::State`, and the
// assertion on that trail is what makes it a genuine interleaved-I/O traversal
// rather than a `Tail` that could have been an `Outcome::Value`.
//
// ⚠ And it demonstrates `Tail`'s own cost honestly: a `Tail` produces no value,
// so the partial sum has to be parked in `State` between resumptions. That is the
// reason `Tail` is NOT the fix for a byte-movement gap — see `drive.rs`'s
// correction — and the client shows it rather than asserting it.

/// One node of the resumable subject.
#[derive(Clone, Copy)]
enum SeqNode {
    /// A leaf carrying its own value.
    Leaf(u64),
    /// `len` items, `cursor` of them already done. ★ The cursor is the *field
    /// program position*, and advancing it is the termination contract.
    Seq { len: u32, cursor: u32 },
}

/// The resumable subject's continuation: resume `Seq` after the item at `cursor`.
#[derive(Clone, Copy)]
struct SeqResume {
    len: u32,
    cursor: u32,
    /// Declared arity. `1` for the well-formed visitor; the `tail_arity_lies`
    /// visitor sets `2` so `arity()` disagrees with what `combine` pops.
    arity: usize,
}

/// How many items the resumable sequence has. Long enough that a tail chain is
/// unmistakable, short enough to be instant.
const SEQ_LEN: u32 = 8;

/// What the resumable traversal writes as it goes, so "bytes between children" is
/// observable rather than claimed.
#[derive(Default)]
struct Trail {
    bytes: Vec<u8>,
    /// ★ The accumulator a `Tail` forces into `State`: a `Tail` produces no
    /// value, so the sum of the items already seen has nowhere else to live.
    acc: u64,
    descends: usize,
    combines: usize,
    tails: usize,
}

/// ★★ THE CONTROL: a well-formed resumable traversal, with bytes between its
/// children and one `Tail` per item after the first.
struct Resumable;

impl Traversal for Resumable {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    /// ★ The instance's own statement of its resumption bound: a sequence of
    /// `SEQ_LEN` items needs at most `SEQ_LEN` resumptions, which is the length
    /// of its field program. Exactly the `arity` discipline, one level up.
    const MAX_TAILS_PER_DESCENT: usize = SEQ_LEN as usize;

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                st.bytes.push(b'L');
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                if cursor == 0 {
                    st.bytes.push(b'[');
                }
                // The SEPARATOR — the byte that sits between two children and is
                // the entire reason this traversal cannot push its region at once.
                st.bytes.push(b',');
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 1,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        for _ in 0..kont.arity {
            st.acc += vals.pop().expect("drive: value stack underflow");
        }
        if kont.cursor + 1 == kont.len {
            st.bytes.push(b']');
            let total = std::mem::take(&mut st.acc);
            Ok(Outcome::Value(total))
        } else {
            st.tails += 1;
            // ★ STRICTLY ADVANCED: `cursor + 1 > cursor`. This is the termination
            // contract discharged, and it is the only reason the machine halts.
            Ok(Outcome::Tail(SeqNode::Seq {
                len: kont.len,
                cursor: kont.cursor + 1,
            }))
        }
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

/// RED: a `Tail` from a `Kont` whose `combine` popped fewer values than its
/// `arity()` claims. Invariant 2's cross-check, on the tail path.
struct TailArityLies;

impl Traversal for TailArityLies {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    const MAX_TAILS_PER_DESCENT: usize = SEQ_LEN as usize;

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                // ⚠ Declares arity 2 while pushing ONE child, so Invariant 1
                // would also object — which is why this visitor pushes TWO
                // children and has `combine` pop only one. The defect under test
                // is the POP count, not the region.
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 2,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        // ⚠ THE DEFECT: pops ONE where `arity()` says TWO, then tails.
        st.acc += vals.pop().expect("drive: value stack underflow");
        if kont.cursor + 1 == kont.len {
            Ok(Outcome::Value(std::mem::take(&mut st.acc)))
        } else {
            st.tails += 1;
            Ok(Outcome::Tail(SeqNode::Seq {
                len: kont.len,
                cursor: kont.cursor + 1,
            }))
        }
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

/// RED: a `Tail` whose suspension region was pushed UNDER-FULL — `arity` 2 with
/// one child. Invariant 1, on the tail path.
struct TailRegionUnderfull;

impl Traversal for TailRegionUnderfull {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    const MAX_TAILS_PER_DESCENT: usize = SEQ_LEN as usize;

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                // ⚠ THE DEFECT: arity 2, one child.
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 2,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        for _ in 0..kont.arity {
            st.acc += vals.pop().expect("drive: value stack underflow");
        }
        st.tails += 1;
        Ok(Outcome::Tail(SeqNode::Seq {
            len: kont.len,
            cursor: kont.cursor + 1,
        }))
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

/// ★★ RED: `Tail`ing FOREVER without advancing the cursor.
///
/// The termination obligation `Outcome::Tail` introduces, and the one defect in
/// this file that **both invariants accept**. `Δ(V + D + C − A) = 0` on every tail
/// and Invariant 1 is satisfied by every region, at every loop head, forever. Only
/// `MAX_TAILS_PER_DESCENT` stops it.
struct TailsForever;

impl Traversal for TailsForever {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    const MAX_TAILS_PER_DESCENT: usize = SEQ_LEN as usize;

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 1,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        st.acc += vals.pop().expect("drive: value stack underflow");
        st.tails += 1;
        // ⚠ THE DEFECT: the cursor does NOT advance. Every invariant holds.
        Ok(Outcome::Tail(SeqNode::Seq {
            len: kont.len,
            cursor: kont.cursor,
        }))
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

/// RED: a `Tail` chain that drains `work` leaving `V != 1` — caught by the
/// **unconditional** final-configuration assertion, so this is the tail-path
/// subject that fires in a RELEASE build.
///
/// The mechanism: the last resume reports `Outcome::Value` *without popping*, so
/// the item's value stays on the stack alongside the answer.
struct TailLeavesTwoValues;

impl Traversal for TailLeavesTwoValues {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    const MAX_TAILS_PER_DESCENT: usize = SEQ_LEN as usize;

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 1,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        if kont.cursor + 1 == kont.len {
            // ⚠ THE DEFECT: reports a value WITHOUT popping its child's, so the
            // work stack drains with two values on `vals`.
            Ok(Outcome::Value(std::mem::take(&mut st.acc)))
        } else {
            st.acc += vals.pop().expect("drive: value stack underflow");
            st.tails += 1;
            Ok(Outcome::Tail(SeqNode::Seq {
                len: kont.len,
                cursor: kont.cursor + 1,
            }))
        }
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

/// RED: a traversal that tails **without declaring itself resumable at all** —
/// `MAX_TAILS_PER_DESCENT` left at its fail-closed default of 0.
///
/// ★ This is what makes the default a guard rather than a formality: a refactor
/// that introduces a `Tail` into a traversal that never contemplated one is
/// rejected on the FIRST tail, not after an arbitrary budget.
struct TailUndeclared;

impl Traversal for TailUndeclared {
    type Node<'t>
        = SeqNode
    where
        Self: 't;
    type Val = u64;
    type Kont<'t>
        = SeqResume
    where
        Self: 't;
    type State = Trail;
    type Err = Never;

    // ⚠ NO `MAX_TAILS_PER_DESCENT`. The default is 0.

    fn descend<'t>(
        &mut self,
        st: &mut Trail,
        node: SeqNode,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<u64>,
    ) -> Result<(), Never> {
        st.descends += 1;
        match node {
            SeqNode::Leaf(v) => {
                vals.push(v);
                Ok(())
            }
            SeqNode::Seq { len, cursor } => {
                work.push(Step::Combine(SeqResume {
                    len,
                    cursor,
                    arity: 1,
                }));
                work.push(Step::Descend(SeqNode::Leaf(u64::from(cursor) + 1)));
                Ok(())
            }
        }
    }

    fn combine<'t>(
        &mut self,
        st: &mut Trail,
        kont: SeqResume,
        vals: &mut Vec<u64>,
    ) -> Result<Outcome<u64, SeqNode>, Never>
    where
        Self: 't,
    {
        st.combines += 1;
        st.acc += vals.pop().expect("drive: value stack underflow");
        st.tails += 1;
        Ok(Outcome::Tail(SeqNode::Seq {
            len: kont.len,
            cursor: kont.cursor + 1,
        }))
    }

    fn arity(kont: &SeqResume) -> usize {
        kont.arity
    }
}

// ===========================================================================
// §F  ★★ The `Outcome::Tail` assertions
// ===========================================================================

/// Pull one `key=` field out of a child's marker line.
fn field(output: &str, key: &str) -> String {
    output
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix(key))
        .unwrap_or_else(|| panic!("no {key} in child output:\n{output}"))
        .to_string()
}

/// ★★★ **THE CONTROL.** Every rejection in this section is vacuous until this
/// passes — and it is also `Outcome::Tail`'s only executed client until the wire
/// encoder is converted, which is what keeps the mechanism from being dead code.
#[test]
fn a_well_formed_tail_chain_completes_and_writes_bytes_between_its_children() {
    let child = run_child("tail_resumable");
    assert!(
        child.ok,
        "★ CONTROL FAILED: the well-formed RESUMABLE traversal did not complete. Every \
         `Outcome::Tail` rejection asserted below is meaningless until this passes, and \
         `Tail` itself would be a mechanism with no executed client.\n{}",
        child.output
    );
    assert!(
        child
            .output
            .contains(&format!("{OK_MARKER} arm=tail_resumable")),
        "the child exited 0 without running the subject:\n{}",
        child.output
    );

    // The items are `1 ..= SEQ_LEN`, so the answer is the triangular number.
    let expected: u64 = (1..=u64::from(SEQ_LEN)).sum();
    assert_eq!(
        field(&child.output, "value="),
        expected.to_string(),
        "the resumable traversal summed to the wrong value; a `Tail` must resume the SAME \
         node's remaining program, and the accumulator parked in `State` must survive every \
         resumption:\n{}",
        child.output
    );

    // ★ The tail count is the subject. `SEQ_LEN` items means `SEQ_LEN - 1`
    // resumptions: the last item's resume reports `Outcome::Value` instead.
    assert_eq!(
        field(&child.output, "tails="),
        (SEQ_LEN - 1).to_string(),
        "VACUOUS: the traversal completed without taking {} `Outcome::Tail`(s), so it did \
         not exercise the mechanism at all:\n{}",
        SEQ_LEN - 1,
        child.output
    );

    // ★★ The byte trail is what makes this an INTERLEAVED-I/O traversal rather
    // than a post-order fold wearing a `Tail`. A separator before every item, a
    // bracket at each end, and the item markers strictly between them: a visitor
    // that could have pushed its whole region at once could not produce this.
    let trail = field(&child.output, "trail=");
    let expected_trail = format!("[{}]", ",L".repeat(SEQ_LEN as usize));
    assert_eq!(
        trail, expected_trail,
        "★ the byte trail is the evidence that bytes sit BETWEEN the children. Expected \
         {expected_trail:?} — an opening bracket, then a separator before each of the \
         {SEQ_LEN} items, then a closing bracket — and got {trail:?}. A traversal that \
         emitted all its separators up front would not need `Tail` and this gate would be \
         testing nothing.\n{}",
        child.output
    );
}

/// Invariant 2's arity cross-check, on the tail path. `debug_assertions`-only.
#[test]
fn a_tail_from_a_kont_that_popped_fewer_values_than_its_arity_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "tail_arity_lies",
            &["DEFICIT INVARIANT", "V + D + C - A == 1", "Invariant 2"],
        );
    } else {
        assert_rejected(
            "tail_arity_lies",
            &["MALFORMED FINAL CONFIGURATION", "checked UNCONDITIONALLY"],
        );
    }
}

/// Invariant 1 on the tail path: the suspension region was pushed under-full.
#[test]
fn a_tail_whose_suspension_region_was_pushed_under_full_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected("tail_region_underfull", &["declares arity 2", "Invariant 1"]);
    } else {
        let child = run_child("tail_region_underfull");
        assert!(
            !child.ok,
            "arm `tail_region_underfull` produced an answer. Invariant 1 is \
             `debug_assertions`-only, so in release the underflow surfaces from the \
             visitor's own `vals.pop()` instead — but it must still not SUCCEED.\n{}",
            child.output
        );
    }
}

/// ★★★ **THE TERMINATION BACKSTOP** — the one defect in this file that both
/// invariants accept.
///
/// `Δ(V + D + C − A) = 0` on every tail and Invariant 1 holds for every region, at
/// every loop head, forever. A machine that spins here is *perfectly well-formed*
/// by both stated invariants, which is precisely why `Outcome::Tail` needed a
/// third obligation and why that obligation needed its own executed RED.
#[test]
fn a_tail_chain_that_never_advances_its_cursor_is_stopped_by_the_bound() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "tail_forever",
            &[
                "TAIL BOUND",
                "MAX_TAILS_PER_DESCENT",
                "strictly ADVANCED",
                "conservation laws, not progress measures",
            ],
        );
    } else {
        // ⚠ Honest in release: the bound is `debug_assertions`-only by design, so
        // an unadvancing tail chain in a release build does NOT terminate. The
        // release statement is therefore about the CONTRACT and the debug build is
        // where it is enforced — exactly as Invariant 1 is. Asserting "it hangs"
        // is not a test one can write, so this leg asserts the honest thing: that
        // the bound is the debug mechanism and the release build is not claimed to
        // catch it.
        println!(
            "tail_forever: NOT ATTEMPTED in release. `MAX_TAILS_PER_DESCENT` is checked \
             under `debug_assertions` only — it is a programming-error backstop, not a \
             data-dependent one, and paying a multiply and a branch per tail in release to \
             catch a defect no input can cause is the wrong trade. The executed rejection \
             is this test in a DEBUG build; running the arm here would hang the suite."
        );
    }
}

/// ★ The tail-path subject that fires in a RELEASE build, via the one assertion
/// `drive.rs` compiles unconditionally.
#[test]
fn a_tail_chain_that_drains_the_work_stack_with_two_values_is_rejected() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "tail_leaves_two_values",
            &["DEFICIT INVARIANT", "Invariant 2"],
        );
    } else {
        assert_rejected(
            "tail_leaves_two_values",
            &[
                "MALFORMED FINAL CONFIGURATION",
                "obligation that was never popped",
                "checked UNCONDITIONALLY",
            ],
        );
    }
}

/// ★ The fail-closed default: a traversal that tails without ever declaring
/// itself resumable is rejected on its FIRST tail.
#[test]
fn a_tail_from_a_traversal_that_declared_no_bound_is_rejected_immediately() {
    if cfg!(debug_assertions) {
        assert_rejected(
            "tail_undeclared",
            &[
                "TAIL BOUND",
                "MAX_TAILS_PER_DESCENT",
                "fail-closed default",
            ],
        );
        // ★ "Immediately" is the property, so it is asserted rather than implied:
        // the diagnostic must report exactly ONE tail taken.
        let child = run_child("tail_undeclared");
        assert!(
            child.output.contains("has taken 1 `Outcome::Tail`(s)"),
            "the undeclared-bound rejection did not fire on the FIRST tail. A default of 0 \
             exists so that the very first `Tail` is caught; a default that allowed a few \
             would be a formality.\n{}",
            child.output
        );
    } else {
        println!(
            "tail_undeclared: NOT ATTEMPTED in release — same reason as `tail_forever`; the \
             bound is `debug_assertions`-only and the arm would hang."
        );
    }
}
