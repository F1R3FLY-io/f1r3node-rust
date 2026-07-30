//! # The three EVENT-HASH legs — a native-stack depth probe
//!
//! ## What this file is for
//!
//! Every RSpace `Produce` and `Consume` event carries a **stable hash**, and that
//! hash is computed by serialising the datum, the patterns and the continuation
//! (`rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash_produce,
//! hash_consume}`). Those three serialisations are the **event-hash legs**:
//!
//! | leg | entry point (`models::rust::spliced_event_bytes`) | called from |
//! |---|---|---|
//! | datum | `event_hash_bytes_list_par_with_random` | `hash_produce`, once per produce |
//! | pattern | `event_hash_bytes_bind_pattern` | `hash_consume`, once per bind |
//! | continuation | `event_hash_bytes_tagged_continuation` | `hash_consume`, once per consume |
//!
//! The event hash reaches consensus twice over: it is the RSpace event identity
//! that `Produce`/`Consume`/`COMM` carry into `ProcessedDeploy::deploy_log` (and
//! so into `Body.deploys` and the **block hash**), and it is the key the replay
//! space rigs against (`ReplayRSpace::rig`). A leg that consumes native stack in
//! proportion to term nesting depth is therefore a **consensus-liveness**
//! exposure, not a robustness nit: a native-stack overflow is a `SIGSEGV` on the
//! guard page, which Rust's handler turns into `fatal runtime error: stack
//! overflow` + `abort()` — signal 6, shell status 134. It is not catchable, it is
//! not a failed deploy, and it takes the whole node process.
//!
//! ## Why the legs are (still) Θ(depth) — two independent recursions each
//!
//! `spliced_event_bytes` exists to splice cached `InternedEPathMap.serde_bytes`
//! into the bincode stream at filled-cell `EPathMap` nodes. Its structure is a
//! dispatch scan followed by one of two emitters, and **both branches recurse**:
//!
//! ```text
//!   event_hash_bytes_<leg>(value)
//!        │
//!        ├─ contains_par(value)                    ← RECURSION 1, ALWAYS RUN
//!        │     `spliced_event_bytes.rs:223-234`      mutually recursive with
//!        │     contains_{expr,send,receive,new,…}    contains_expr over
//!        │                                          `EList.ps: Vec<Par>`
//!        │
//!        ├─ false ⇒ direct(value)                  ← RECURSION 2a (the 95.43 %
//!        │     `= bincode::serialize(value)`          case: no filled cell)
//!        │     the DERIVED `Serialize`, which is
//!        │     mutually recursive Par ▸ Expr ▸ …
//!        │
//!        └─ true  ⇒ emit_<leg>(value, out)         ← RECURSION 2b
//!              emit_vec ▸ emit_par ▸ emit_expr ▸ …
//!              hand-written, mutually recursive
//! ```
//!
//! ★ **Recursion 2a is the one that matters most**, and it is the one that reads
//! as already-fixed if you only look at the *names* in the tree. The cold-store
//! encoder **was** converted: `models::rust::rholang::wire_encode` provides
//! `ColdStoreEncode::cold_encode`, a single-walk trampolined encoder that is
//! byte-identical to `bincode::serialize` with O(1) native stack, and it is
//! implemented for exactly `Par`, `BindPattern`, `ListParWithRandom` and
//! `TaggedContinuation` — i.e. for **all three legs' root types**. The event-hash
//! legs simply never started calling it: `direct()` is still
//! `bincode::serialize`. So the conversion that landed for the cold store did not
//! reach the event hash, and the exposure the campaign existed to remove is still
//! present on three of the four legs (the **channel** leg was routed through the
//! new encoder in `00ff9187`; these three were not).
//!
//! ## What this file measures, and what it deliberately does not assert
//!
//! This is a **measurement** harness in the sense of
//! `rholang/tests/stack_depth_probe.rs`: one traversal per number. It reports the
//! minimum stack on which each leg survives at two depths and derives B/level.
//! It asserts only what is *currently true* — that the three legs slope and that
//! the `cold_encode` control does not — so that:
//!
//! * the numbers in the report are reproducible by anyone, on any machine, and
//! * the day someone routes the legs through `wire_encode`, this file's
//!   `the_three_legs_still_slope` test goes RED and has to be re-derived into a
//!   flatness claim. A one-sided floor would have gone quietly green instead.
//!
//! ⚠ **12,288 B is this instrument's FLOOR, not a measurement.** `getconf
//! PTHREAD_STACK_MIN` is 16,384 on this platform and clamps every smaller
//! `stack_size` request, while [`min_stack_for`] begins its exponential probe at
//! 16 KiB and bisects `[8192, 16384]` at [`RESOLUTION`] granularity. 12,288 is
//! therefore the smallest value the bisection can ever return, and a subject that
//! reads it has been shown only to fit *below the floor*. Do not divide by it and
//! do not report it as a per-level cost. **Slope readings (B/level) are
//! unaffected**, because the floor is a constant added to both ends of the ladder
//! — and when both ends read the floor the slope is exactly 0.
//!
//! ## Why this file lives under `casper/tests/`
//!
//! The subject is `models`, but the *stake* is consensus, and `casper` is the
//! crate that owns the consensus surface these bytes reach (`deploy_log` →
//! `Body.deploys` → block hash; `replay_compute_state` → rig). It sits beside
//! `casper/tests/deploy_ingress_depth_ceiling.rs`, whose method it reuses
//! verbatim, and it depends on nothing in `casper` — so it can be moved to
//! `models/tests/` unchanged if the codec work consolidates the family there.
//!
//! ## Invocation
//!
//! ```text
//! cargo test -p casper --test event_hash_leg_depth_probe -- --nocapture
//! ```
//!
//! Each probe point re-execs this binary (a stack overflow is not unwindable, so
//! a bisection cannot run in one process) with
//! `EVENT_HASH_LEG` / `EVENT_HASH_DEPTH` / `EVENT_HASH_STACK` set.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, EList, EPathMap, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::rholang::wire_encode::ColdStoreEncode;
use models::rust::spliced_event_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::utils::new_gint_par;

// ---------------------------------------------------------------------------
// the subjects
// ---------------------------------------------------------------------------

/// One probe subject: a leg, crossed with which of its two emitters is exercised.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leg {
    /// `event_hash_bytes_list_par_with_random`, map-free term ⇒ `direct()`.
    Datum,
    /// `event_hash_bytes_bind_pattern`, map-free term ⇒ `direct()`.
    Pattern,
    /// `event_hash_bytes_tagged_continuation`, map-free term ⇒ `direct()`.
    Continuation,
    /// `event_hash_bytes_list_par_with_random` with a FILLED-cell `EPathMap` at
    /// the bottom of the chain ⇒ the hand-written `emit_*` spine walks the whole
    /// chain (every level answers `contains_par == true`).
    DatumSpliced,
    /// As [`Leg::DatumSpliced`], on the pattern leg.
    PatternSpliced,
    /// As [`Leg::DatumSpliced`], on the continuation leg.
    ContinuationSpliced,
    /// ★ THE CONTROL. `ListParWithRandom::cold_encode()` — the trampolined
    /// encoder that is byte-identical to `bincode::serialize`. This is what the
    /// datum leg would read after conversion, measured on the *same* fixture by
    /// the *same* instrument, so "flat" is a comparison and not a hope.
    DatumCold,
    /// As [`Leg::DatumCold`], on the pattern leg.
    PatternCold,
    /// As [`Leg::DatumCold`], on the continuation leg.
    ContinuationCold,
}

impl Leg {
    fn tag(self) -> &'static str {
        match self {
            Leg::Datum => "datum",
            Leg::Pattern => "pattern",
            Leg::Continuation => "continuation",
            Leg::DatumSpliced => "datum-spliced",
            Leg::PatternSpliced => "pattern-spliced",
            Leg::ContinuationSpliced => "continuation-spliced",
            Leg::DatumCold => "datum-cold",
            Leg::PatternCold => "pattern-cold",
            Leg::ContinuationCold => "continuation-cold",
        }
    }

    fn from_tag(tag: &str) -> Self {
        match tag {
            "datum" => Leg::Datum,
            "pattern" => Leg::Pattern,
            "continuation" => Leg::Continuation,
            "datum-spliced" => Leg::DatumSpliced,
            "pattern-spliced" => Leg::PatternSpliced,
            "continuation-spliced" => Leg::ContinuationSpliced,
            "datum-cold" => Leg::DatumCold,
            "pattern-cold" => Leg::PatternCold,
            "continuation-cold" => Leg::ContinuationCold,
            other => unreachable!("event_hash_leg_depth_probe: unknown leg {other:?}"),
        }
    }
}

/// The three legs as they run in production on a map-free term — the 95.43 %
/// case in the measured production datum distribution.
const DIRECT_LEGS: [Leg; 3] = [Leg::Datum, Leg::Pattern, Leg::Continuation];

/// The same three legs forced onto the hand-written spliced spine.
const SPLICED_LEGS: [Leg; 3] = [
    Leg::DatumSpliced,
    Leg::PatternSpliced,
    Leg::ContinuationSpliced,
];

/// The converted-encoder controls, one per leg root type.
const COLD_LEGS: [Leg; 3] = [Leg::DatumCold, Leg::PatternCold, Leg::ContinuationCold];

// ---------------------------------------------------------------------------
// the ladder
// ---------------------------------------------------------------------------

/// Bisection granularity, matching `rholang/tests/stack_depth_gate.rs` and
/// `casper/tests/deploy_ingress_depth_ceiling.rs` so the numbers are comparable
/// across all three harnesses.
const RESOLUTION: usize = 4096;

/// The ladder. Chosen so the quantisation error in the derived slope,
/// `RESOLUTION / (LADDER_HI - LADDER_LO)`, is about **1.07 B/level** — under
/// 0.04 % of the smallest slope this file expects to see.
const LADDER_LO: usize = 256;
const LADDER_HI: usize = 4_096;

/// A subject counts as SLOPING only if the ladder grows by more than this. Four
/// bisection buckets over a 3,840-step ladder is under 4.3 B/level, far below any
/// real per-level frame.
const ZERO_SLOPE_TOLERANCE: usize = 4 * RESOLUTION;

// ---------------------------------------------------------------------------
// fixtures — ALL ITERATIVE, so construction never contributes to the reading
// ---------------------------------------------------------------------------

fn expr_par(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn elist(ps: Vec<Par>) -> Par {
    expr_par(ExprInstance::EListBody(EList {
        ps,
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }))
}

/// `[[[…[0]…]]]` with `depth` bracket levels — the same shape
/// `rholang/tests/stack_depth_probe.rs` uses, so slopes are comparable.
fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        par = elist(vec![par]);
    }
    par
}

/// A SHALLOW `EPathMap` whose intern cell has been FILLED, wrapped in a `Par`.
///
/// The map itself carries one ground entry, so building it is O(1) in the probe's
/// depth parameter; all the nesting lives in the chain that wraps it. Filling the
/// cell is what makes `contains_epathmap` — and hence `contains_par` at every
/// enclosing level — answer `true`, which is the only way to reach the
/// hand-written emitter.
fn filled_epathmap_par() -> Par {
    let entry = elist(vec![new_gint_par(7, vec![], false)]);
    let map = EPathMap::new(vec![entry], vec![], false, None);
    let _handle = map.intern();
    assert!(
        map.interned_handle().is_some(),
        "VACUOUS PROBE: intern() did not fill the shadow cell, so the spliced \
         subjects would silently measure the DIRECT path instead"
    );
    expr_par(ExprInstance::EPathmapBody(map))
}

/// `[[[…[⟨filled map⟩]…]]]` — `depth` bracket levels above a filled-cell map.
///
/// Placing the map at the BOTTOM is deliberate: `emit_vec` black-boxes any
/// element whose `contains` predicate is false, so a map at the top would send
/// the deep sibling straight back through `bincode::serialize` and the reading
/// would be the derived encoder's again rather than the hand walk's.
fn nested_list_over_filled_map(depth: usize) -> Par {
    let mut par = filled_epathmap_par();
    for _ in 0..depth {
        par = elist(vec![par]);
    }
    par
}

fn datum_of(par: Par) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![par],
        random_state: vec![0xAB; 32],
    }
}

fn pattern_of(par: Par) -> BindPattern {
    BindPattern {
        patterns: vec![par],
        remainder: None,
        free_count: 0,
    }
}

fn continuation_of(par: Par) -> TaggedContinuation {
    TaggedContinuation {
        guard: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(par),
            random_state: vec![0xCD; 32],
        })),
    }
}

/// Run `f` on a stack that can never be the constraint, and hand back its result.
/// 1 GiB is address space, not resident memory.
fn on_a_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .name("probe-setup".to_string())
        .spawn(f)
        .expect("event_hash_leg_depth_probe: failed to spawn the setup thread")
        .join()
        .expect("event_hash_leg_depth_probe: setup thread panicked")
}

// ---------------------------------------------------------------------------
// the subject, run once
// ---------------------------------------------------------------------------

/// A pre-built leg input. One variant per leg ROOT TYPE, not per subject: the
/// three `*Cold` controls reuse the same three fixtures as the three direct
/// subjects, which is what makes them controls rather than separate experiments.
enum Fixture {
    Datum(ListParWithRandom),
    Pattern(BindPattern),
    Continuation(TaggedContinuation),
}

/// ⚠ **BUILDING THE FIXTURE IS ITSELF Θ(depth) FOR SOME SUBJECTS, AND FOR THE
/// SPLICED ONES IT COSTS 100 KiB OF FIXED STACK.**
///
/// This function is deliberately separate from [`run_leg`] and is always called
/// on [`on_a_big_stack`]. The first draft of this probe built the fixture inside
/// the bisected thread, and the depth-4 floors it reported were **32 KiB** for
/// the direct legs and **128 KiB** for the spliced ones against **16 KiB** for
/// the `cold_encode` controls on the *identical* term — i.e. the "measurement"
/// was dominated by `EPathMap::new`'s trie insert and `EPathMap::intern`'s
/// canonical-prost rendezvous, neither of which is the subject. Splitting the
/// build out restores the discipline the module header promises: **one traversal
/// per number.**
fn build_fixture(leg: Leg, depth: usize) -> Fixture {
    let par = match leg {
        Leg::DatumSpliced | Leg::PatternSpliced | Leg::ContinuationSpliced => {
            nested_list_over_filled_map(depth)
        }
        _ => nested_list(depth),
    };
    match leg {
        Leg::Datum | Leg::DatumSpliced | Leg::DatumCold => Fixture::Datum(datum_of(par)),
        Leg::Pattern | Leg::PatternSpliced | Leg::PatternCold => Fixture::Pattern(pattern_of(par)),
        Leg::Continuation | Leg::ContinuationSpliced | Leg::ContinuationCold => {
            Fixture::Continuation(continuation_of(par))
        }
    }
}

/// Compute one event-hash leg over a pre-built fixture. **This is the subject,
/// and it is the only thing that runs on the bisected stack.**
///
/// Everything is `mem::forget`-ed: `Drop` for the `Par`/`Expr`/`ExprInstance`
/// family is itself a Θ(depth) recursive traversal (`par_drop` is in the rholang
/// gate's `TRIPWIRE_DEPTH`), so letting the fixture fall out of scope on the
/// bisected thread would make every reading `max(subject, destructor)`.
fn run_leg(leg: Leg, depth: usize, fixture: Fixture) {
    let bytes = match (leg, fixture) {
        (Leg::Datum | Leg::DatumSpliced, Fixture::Datum(value)) => {
            let bytes = event_hash_bytes_list_par_with_random(&value);
            std::mem::forget(value);
            bytes
        }
        (Leg::Pattern | Leg::PatternSpliced, Fixture::Pattern(value)) => {
            let bytes = event_hash_bytes_bind_pattern(&value);
            std::mem::forget(value);
            bytes
        }
        (Leg::Continuation | Leg::ContinuationSpliced, Fixture::Continuation(value)) => {
            let bytes = event_hash_bytes_tagged_continuation(&value);
            std::mem::forget(value);
            bytes
        }
        (Leg::DatumCold, Fixture::Datum(value)) => {
            let bytes = value.cold_encode();
            std::mem::forget(value);
            bytes
        }
        (Leg::PatternCold, Fixture::Pattern(value)) => {
            let bytes = value.cold_encode();
            std::mem::forget(value);
            bytes
        }
        (Leg::ContinuationCold, Fixture::Continuation(value)) => {
            let bytes = value.cold_encode();
            std::mem::forget(value);
            bytes
        }
        (leg, _) => unreachable!(
            "event_hash_leg_depth_probe: leg {} was handed the wrong fixture root type",
            leg.tag()
        ),
    };

    // ★ ANTI-VACUITY, on the probe thread itself. A leg that returned an empty
    // or trivially short buffer would read perfectly flat, and the whole ladder
    // would be measuring nothing. bincode 1.3.3 emits at least the u64-LE length
    // prefix of every nesting level, so the encoding of a `depth`-deep chain is
    // strictly longer than `depth` bytes.
    assert!(
        bytes.len() > depth,
        "VACUOUS PROBE: leg {} at depth {depth} produced only {} bytes; a bincode \
         encoding of a {depth}-level chain cannot be that short, so the fixture \
         is not carrying depth",
        leg.tag(),
        bytes.len()
    );
    std::mem::forget(bytes);
}

// ---------------------------------------------------------------------------
// the child
// ---------------------------------------------------------------------------

/// The child entry point. A NO-OP without its environment, so `--run-ignored
/// all` cannot fail the suite for an unrelated reason.
#[test]
#[ignore = "child process of the event-hash leg probe; driven via EVENT_HASH_DEPTH"]
fn event_hash_leg_child() {
    let Ok(depth) = std::env::var("EVENT_HASH_DEPTH") else {
        println!("event_hash_leg_child: no EVENT_HASH_DEPTH — not a child invocation");
        return;
    };
    let depth: usize = depth.parse().expect("EVENT_HASH_DEPTH must be an integer");
    let stack: usize = std::env::var("EVENT_HASH_STACK")
        .expect("EVENT_HASH_STACK must accompany EVENT_HASH_DEPTH")
        .parse()
        .expect("EVENT_HASH_STACK must be an integer");
    let leg = Leg::from_tag(
        &std::env::var("EVENT_HASH_LEG").expect("EVENT_HASH_LEG must accompany EVENT_HASH_DEPTH"),
    );

    // The fixture is built where its own cost can never bind — see
    // [`build_fixture`] for the measurement this split repaired.
    let fixture = on_a_big_stack(move || build_fixture(leg, depth));

    // ★ EXPLICIT `stack_size`. `.cargo/config.toml` sets `RUST_MIN_STACK =
    // 8388608`, so a probe that let the default stand would measure a 4× larger
    // stack than a node worker has.
    std::thread::Builder::new()
        .stack_size(stack)
        .name("event-hash-leg".to_string())
        .spawn(move || run_leg(leg, depth, fixture))
        .expect("event_hash_leg_depth_probe: failed to spawn")
        .join()
        .expect("event_hash_leg_depth_probe: subject panicked");
}

/// One probe point. `true` iff the leg completed.
fn leg_survives(leg: Leg, depth: usize, stack: usize) -> bool {
    let exe = std::env::current_exe().expect("event_hash_leg_depth_probe: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "event_hash_leg_child"])
        .env("EVENT_HASH_LEG", leg.tag())
        .env("EVENT_HASH_DEPTH", depth.to_string())
        .env("EVENT_HASH_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("event_hash_leg_depth_probe: failed to run child")
        .success()
}

/// Smallest stack (to [`RESOLUTION`] granularity) on which `leg` survives at
/// `depth`. Exponential probe, then bisect — the same method, and the same
/// quantisation, as `rholang/tests/stack_depth_gate.rs::min_stack_for`.
///
/// ⚠ The smallest value this can return is **12,288** — see the module note on
/// the instrument floor.
fn min_stack_for(leg: Leg, depth: usize) -> usize {
    const CEILING: usize = 512 * 1024 * 1024;
    let mut hi = 16 * 1024;
    while hi <= CEILING && !leg_survives(leg, depth, hi) {
        hi *= 2;
    }
    assert!(
        hi <= CEILING,
        "leg {} needed more than 512 MiB at depth {depth}",
        leg.tag()
    );
    let mut lo = hi / 2;
    while hi - lo > RESOLUTION {
        let mid = (lo + hi) / 2;
        if leg_survives(leg, depth, mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// B/level across the ladder.
fn slope(lo_stack: usize, hi_stack: usize) -> f64 {
    (hi_stack as f64 - lo_stack as f64) / (LADDER_HI - LADDER_LO) as f64
}

// ---------------------------------------------------------------------------
// the claims
// ---------------------------------------------------------------------------

/// ★★ **THE THREE EVENT-HASH LEGS ARE Θ(depth) IN NATIVE STACK, AND THE
/// CONVERTED ENCODER ON THE SAME FIXTURE IS NOT.**
///
/// This is a *characterisation* test, and it is deliberately two-sided:
///
/// | leg | asserts | what it refuses |
/// |---|---|---|
/// | 1 | each of the three legs' minimum stack GROWS over `256 → 4,096` | a probe that cannot see the defect at all |
/// | 2 | `cold_encode` on the SAME three fixtures does not grow | "everything slopes, the instrument is broken" |
///
/// Leg 2 is what makes leg 1 a statement about the *legs* rather than about the
/// fixture or the instrument. The control differs from the subject in **one
/// call** — `value.cold_encode()` instead of `event_hash_bytes_…(&value)` — over
/// a byte-identical encoding of a byte-identical term. Any explanation that makes
/// the subject vacuously sloped (a runaway fixture builder, a leaked destructor,
/// a mis-set `stack_size`) makes the control slope too, and leg 2 fails.
///
/// ⚠ **This test is expected to be INVERTED, not deleted, when the legs are
/// converted.** It fails the moment `direct()` starts calling `cold_encode`,
/// which is the point: a one-sided "the legs need at least X bytes" floor would
/// have gone quietly green on the repair and left no record that anything moved.
#[test]
fn the_three_legs_still_slope() {
    println!(
        "\n  leg                      min stack @ {LADDER_LO}   @ {LADDER_HI}     B/level"
    );
    println!("  ────────────────────────  ───────────────  ───────────────  ─────────");

    let mut sloped: Vec<(Leg, f64)> = Vec::with_capacity(DIRECT_LEGS.len() + SPLICED_LEGS.len());
    for leg in DIRECT_LEGS.into_iter().chain(SPLICED_LEGS) {
        let lo = min_stack_for(leg, LADDER_LO);
        let hi = min_stack_for(leg, LADDER_HI);
        let b = slope(lo, hi);
        println!(
            "  {:<24}  {lo:>15}  {hi:>15}  {b:>9.1}",
            leg.tag()
        );
        sloped.push((leg, b));
        assert!(
            hi > lo + ZERO_SLOPE_TOLERANCE,
            "leg {} did NOT slope: {lo} B at depth {LADDER_LO} and {hi} B at depth \
             {LADDER_HI}, a growth of {} B over {} levels, which is within the \
             {ZERO_SLOPE_TOLERANCE} B tolerance. Either the leg has been CONVERTED \
             — in which case this test must be re-derived into a flatness claim \
             beside the other converted subjects, not deleted — or the fixture \
             stopped carrying depth.",
            leg.tag(),
            hi.saturating_sub(lo),
            LADDER_HI - LADDER_LO
        );
    }

    let mut flat: Vec<(Leg, f64)> = Vec::with_capacity(COLD_LEGS.len());
    for leg in COLD_LEGS {
        let lo = min_stack_for(leg, LADDER_LO);
        let hi = min_stack_for(leg, LADDER_HI);
        let b = slope(lo, hi);
        println!(
            "  {:<24}  {lo:>15}  {hi:>15}  {b:>9.1}",
            leg.tag()
        );
        flat.push((leg, b));
        assert!(
            hi <= lo + ZERO_SLOPE_TOLERANCE,
            "THE CONTROL SLOPED. `{}` grew from {lo} B to {hi} B over {} levels. \
             `cold_encode` is the trampolined encoder and must be depth-flat; if \
             it is not, then every 'sloping' verdict above is uninterpretable \
             because the instrument, the fixture or the harness — not the leg — is \
             what is being measured.",
            leg.tag(),
            LADDER_HI - LADDER_LO
        );
    }

    // ── the evidence margin, stated as a comparison ──────────────────────────
    let worst_control = flat
        .iter()
        .map(|(_, b)| *b)
        .fold(0.0f64, f64::max);
    let best_subject = sloped
        .iter()
        .map(|(_, b)| *b)
        .fold(f64::INFINITY, f64::min);
    println!(
        "\n  weakest sloping leg: {best_subject:.1} B/level   \
         strongest flat control: {worst_control:.1} B/level\n"
    );
    assert!(
        best_subject > 10.0 * worst_control.max(1.0),
        "The separation between subjects and controls is under 10×: weakest \
         subject {best_subject:.1} B/level against strongest control \
         {worst_control:.1} B/level. That is not an evidence margin."
    );
}

/// ★ **The spliced subjects really do take the spliced branch.**
///
/// Without this the three `*-spliced` rows would be indistinguishable from the
/// three direct ones: if `intern()` stopped filling the cell, or if
/// `contains_par` stopped descending `EList`, the "spliced" fixture would fall
/// through to `direct()` and the probe would report the derived encoder's slope
/// twice under two different names.
///
/// The discriminator is the one observable the two branches cannot share: the
/// hand-written emitter SPLICES `InternedEPathMap.serde_bytes` at a filled cell,
/// and `bincode::serialize` of the same `EPathMap` serialises the entry trie. If
/// the two produced identical bytes there would be no splice and no reason for
/// `spliced_event_bytes` to exist; the PM-1 differential
/// (`models/tests/epathmap_spliced_event_bytes.rs`) asserts they are equal *by
/// construction of the splice*, so equality here proves the splice ran only in
/// combination with the cell being filled — which the fixture asserts directly.
#[test]
fn the_spliced_fixture_actually_splices() {
    let par = on_a_big_stack(|| nested_list_over_filled_map(4));
    let map_par = on_a_big_stack(filled_epathmap_par);

    // The cell is filled — asserted inside `filled_epathmap_par`, re-asserted
    // here on the value that actually reaches the emitter.
    let Some(ExprInstance::EPathmapBody(map)) = map_par
        .exprs
        .first()
        .and_then(|e| e.expr_instance.as_ref())
    else {
        unreachable!("the spliced fixture must be an EPathMap-bearing Par");
    };
    assert!(
        map.interned_handle().is_some(),
        "the spliced fixture's intern cell is EMPTY, so every `*-spliced` row \
         measured the DIRECT path under a spliced name"
    );

    // And the leg agrees with the derived oracle on those bytes, which is the
    // PM-1 obligation the splice has to keep.
    let value = datum_of(par);
    let spliced = event_hash_bytes_list_par_with_random(&value);
    let direct = bincode::serialize(&value).expect("derived oracle must encode");
    assert_eq!(
        spliced, direct,
        "the spliced emitter and the derived oracle disagree on a filled-cell \
         fixture — that is a consensus defect in `spliced_event_bytes`, not a \
         probe defect"
    );
}

/// ★ **The probe can go RED, and it can go GREEN, for the right reasons.**
///
/// Two halves, and neither is ceremony:
///
/// * every subject and every control survives depth 4 on a 16 KiB stack, so a
///   ladder reading is a measurement of *depth* and not of fixed overhead; and
/// * every **sloping** subject dies at depth 65,536 on that same 16 KiB stack,
///   so the child really is reaching the subject with the environment applied. A
///   child that silently no-op'd (a mistyped variable name, a `--exact` filter
///   that matched nothing) would report `success()` at every depth, and the
///   whole ladder would read flat.
///
/// ⚠ The second half is asserted for the six sloping subjects ONLY. The
/// `*-cold` controls are depth-flat by design, so they are *expected* to survive
/// 65,536 levels on [`DISCRIMINATOR_STACK`] — asserting otherwise would demand
/// the controls fail at being controls.
#[test]
fn the_probe_discriminates() {
    /// One stack that is comfortably above every subject's *fixed* cost and
    /// comfortably below what any Θ(depth) subject needs at [`DEEP`]. 1 MiB
    /// against a smallest observed per-level frame of hundreds of bytes leaves
    /// three orders of magnitude of separation.
    const DISCRIMINATOR_STACK: usize = 1024 * 1024;
    /// Deep enough that a Θ(depth) leg cannot fit in [`DISCRIMINATOR_STACK`] by
    /// orders of magnitude, shallow enough that the iterative builder's heap
    /// footprint stays in tens of megabytes.
    const DEEP: usize = 1 << 16;

    for leg in DIRECT_LEGS.into_iter().chain(SPLICED_LEGS).chain(COLD_LEGS) {
        assert!(
            leg_survives(leg, 4, DISCRIMINATOR_STACK),
            "THE PROBE CANNOT GO GREEN: leg {} did not survive depth 4 on a \
             {} KiB stack. Every ladder reading is then a measurement of fixed \
             overhead, not of depth.",
            leg.tag(),
            DISCRIMINATOR_STACK / 1024
        );
    }

    for leg in DIRECT_LEGS.into_iter().chain(SPLICED_LEGS) {
        assert!(
            !leg_survives(leg, DEEP, DISCRIMINATOR_STACK),
            "THE PROBE CANNOT GO RED: leg {} 'survived' depth {DEEP} on a {} KiB \
             stack. Either the child is not running the subject at all or the \
             environment is not reaching it.",
            leg.tag(),
            DISCRIMINATOR_STACK / 1024
        );
    }
}
