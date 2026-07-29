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
    ) -> Result<Outcome<u64>, Never> {
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
    ) -> Result<Outcome<u64>, Never> {
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
    ) -> Result<Outcome<u64>, Never> {
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
    ) -> Result<Outcome<u64>, Never> {
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
    ) -> Result<Outcome<u64>, Never> {
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
        other => panic!("drive_configuration_gate: unknown {GATE_ARM}={other:?}"),
    };
    println!(
        "{OK_MARKER} arm={arm} value={value} descends={} combines={}",
        visits.descends, visits.combines
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
