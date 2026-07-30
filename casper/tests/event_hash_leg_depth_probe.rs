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
//! ## ★★ STATUS (work item #124): the DIRECT legs are converted; the SPLICED
//! ## spine is not
//!
//! | path | native stack, debug ▸ release | state |
//! |---|---|---|
//! | `contains_par` guard + `direct()` | 3,040 ▸ 160 B/level | ⟶ **converted** |
//! | `cold_encode()` only, guard untouched | 960 ▸ 128 B/level | an intermediate that was NOT the fix |
//! | both converted (**today**) | **0 ▸ 0** | ✓ `the_direct_legs_are_flat_…` |
//! | the hand-written `emit_*` spine | 1,008 ▸ **208** B/level | ⚠ **still Θ(depth)** |
//!
//! ⚠★ **A DISCLOSED REGRESSION ON THE SPLICED ROW: 176 ▸ 208 B/level in RELEASE.**
//! The spliced spine's source was not touched, but its release per-level frame
//! grew by 32 B (+18 %) when `contains_par` became a worklist. This is
//! **measured**, not inferred, by an A/B in which the *instrument was held
//! invariant* — the same probe binary source, the same machine, the same session,
//! swapping only `models/src/rust/spliced_event_bytes.rs`:
//!
//! | arm | subject | spliced, release |
//! |---|---|---|
//! | C | the pre-conversion file at `42d5082a` | 176.0 B/level |
//! | D | the converted file | 208.0 B/level |
//!
//! Debug is **unaffected** (1,008 B/level in every run, before and after), which
//! is what identifies this as an optimiser-layout effect rather than an
//! algorithmic one — `emit_vec` invokes the guard at every level of the spliced
//! walk, so the guard's locals can land in the recursive frame. ★ Two mitigations
//! were tried and **both failed to move it**: `#[inline(never)]` on `contains_par`
//! (208.0) and `#[inline(never)]` on all thirteen `contains_*` wrappers handed to
//! `emit_vec` as function pointers (208.0). The mechanism is therefore *not* the
//! guard being inlined into the emit chain, and is not yet identified.
//!
//! ★ The trade is stated rather than buried: the SPLICED path was already
//! Θ(depth) and is already the known-unconverted residual, while the DIRECT path
//! — **95.43 % of production datums** — went from Θ(depth) to flat. Paying 18 %
//! more per level on a path that is scheduled for conversion anyway, to remove
//! the exposure entirely from the common path, is the right direction; but it is
//! a real cost on a real tripwire subject and is recorded here so that the next
//! reader of the 208 does not attribute it to the spliced emitter's own code.
//!
//! ⚠ Do not read `72ae7101`'s recorded release figure of 208 as agreeing with the
//! 208 above. That commit recorded 208 for the **unconverted** file; this file's
//! unconverted arm measures **176**, three times (two pre-conversion ladder runs
//! and arm C). The agreement is a coincidence of two different configurations,
//! and the recorded 208 does not reproduce here.
//!
//! ★ **The direct legs took TWO conversions, and the middle row is why this file
//! exists.** The obvious repair — route `direct()` through `cold_encode` — moved
//! the debug slope from 3,040 to 960 and the release slope from 160 to only 128,
//! and the pre-conversion form of this file's ladder test stayed GREEN through it,
//! because the leg was *still sloping*. The residual was recursion 1 below: the
//! `contains_par` guard, which runs before either emitter and, to answer `false`,
//! must descend the entire term. In release it was 128 of the original 160
//! B/level — **80 % of the defect lived in the guard, not the encoder.**
//!
//! ## The structure — a dispatch scan followed by one of two emitters
//!
//! `spliced_event_bytes` exists to splice cached `InternedEPathMap.serde_bytes`
//! into the bincode stream at filled-cell `EPathMap` nodes:
//!
//! ```text
//!   event_hash_bytes_<leg>(value)
//!        │
//!        ├─ contains_par(value)                    ← was RECURSION 1, ALWAYS RUN
//!        │     now an EXPLICIT WORKLIST over &Par    (a LIFO `Vec<&Par>`; every
//!        │     ✓ CONVERTED — 0 native frames/level    container reaches its
//!        │                                            children as Par, so one
//!        │                                            stack covers the term)
//!        │
//!        ├─ false ⇒ value.cold_encode()            ← was RECURSION 2a (the
//!        │     the trampolined single-walk encoder     95.43 % case: no filled
//!        │     ✓ CONVERTED — 0 native frames/level     cell)
//!        │
//!        └─ true  ⇒ emit_<leg>(value, out)         ← RECURSION 2b
//!              emit_vec ▸ emit_par ▸ emit_expr ▸ …    ⚠ STILL Θ(depth), and
//!              hand-written, mutually recursive       still measured below
//! ```
//!
//! `ColdStoreEncode::cold_encode` (`models::rust::rholang::wire_encode`) is a
//! single-walk trampolined encoder, byte-identical to `bincode::serialize` with
//! O(1) native stack, implemented for exactly `Par`, `BindPattern`,
//! `ListParWithRandom` and `TaggedContinuation` — i.e. for **all three legs' root
//! types**. The **channel** leg was routed through it in `00ff9187`; these three
//! followed, and the byte-identity obligation that carries (event hashes reach
//! the block hash) is discharged by
//! `models/tests/event_hash_leg_cold_encode_identity.rs`.
//!
//! ⚠ The guard's conversion carries a *different* obligation, which no byte
//! differential can discharge: the predicate produces no bytes, it only selects
//! between two byte-identical emitters, so a broken predicate leaves every byte
//! gate green. It is gated instead by the module-private
//! `contains_par_equivalence` tests in `spliced_event_bytes.rs`, which compare the
//! worklist against the pre-conversion recursion held verbatim as a frozen oracle.
//!
//! ## What this file measures, and what it deliberately does not assert
//!
//! This is a **measurement** harness in the sense of
//! `rholang/tests/stack_depth_probe.rs`: one traversal per number. It reports the
//! minimum stack on which each leg survives at two depths and derives B/level.
//! It asserts only what is *currently true* — the three direct legs are flat, the
//! three spliced ones still slope, and the `cold_encode` control is flat — so
//! that:
//!
//! * the numbers in the report are reproducible by anyone, on any machine, and
//! * the day someone converts the spliced spine, this file goes RED and has to be
//!   re-derived again rather than deleted. A one-sided floor would have gone
//!   quietly green instead.
//!
//! ★ That discipline is not decorative: it is what produced the finding above.
//! The pre-conversion test was written as "the three legs still slope" with the
//! instruction to invert it on repair, and when the encoder was converted it
//! **stayed green** — which is precisely how the 960 B/level residual in the
//! guard was caught instead of being shipped as a completed conversion.
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

/// ★★ **THE THREE DIRECT EVENT-HASH LEGS ARE NOW FLAT; THE THREE SPLICED ONES
/// ARE STILL Θ(depth).**
///
/// ⚠ **THIS TEST HAS BEEN INVERTED, AS ITS PREVIOUS FORM SAID IT WOULD BE.** It
/// was written as "the three legs still slope", with the standing instruction
/// that the conversion must *re-derive* it into a flatness claim rather than
/// delete it. That has now happened, and the three-way split below is the record
/// of what moved and what did not.
///
/// # What changed, and what it took
///
/// | subject | before | after | in both profiles |
/// |---|---|---|---|
/// | `datum` / `pattern` / `continuation` | 3,040 ▸ 160 B/level | **0 ▸ 0** | converted |
/// | `*-spliced` | 1,008 ▸ 176 B/level | **unchanged** | NOT converted |
/// | `*-cold` (control) | 0 ▸ 0 | **0 ▸ 0** | must not move |
///
/// ★ **The direct legs needed TWO conversions, not one, and the previous form of
/// this test is what proved it.** Routing `direct()` through
/// `ColdStoreEncode::cold_encode` moved the debug slope only from 3,040 to 960
/// B/level and the release slope only from 160 to 128 — this test stayed GREEN,
/// because the leg was still sloping. The residual was `contains_par`, the guard
/// scan that runs *before* either emitter and must descend the whole term to
/// prove the ABSENCE of a filled intern cell. Converting that scan to an explicit
/// worklist is what took both profiles to zero. A test that had asserted only
/// "the encoder is now `cold_encode`" would have reported success at the 960
/// B/level state.
///
/// # The three-way structure
///
/// | leg | asserts | what it refuses |
/// |---|---|---|
/// | 1 | the three DIRECT legs do not grow over `256 → 4,096` | "the conversion is done" without measuring it |
/// | 2 | the three SPLICED legs still DO grow | a residual quietly reclassified as fixed |
/// | 3 | `cold_encode` on the same fixtures does not grow | "everything is flat, the instrument is broken" |
///
/// Leg 3 is what makes leg 1 mean anything: a harness that had stopped reaching
/// the subject would read flat for the converted legs *and* for the controls, and
/// leg 2 — which still demands real slope from real subjects on the same
/// instrument, in the same run — is what refuses that reading. ★ The three legs
/// are therefore mutually load-bearing: **no single failure mode can make all
/// three pass**, which is a property the two-sided form did not have once its
/// subjects started going flat.
///
/// ⚠ Leg 2 is a TRIPWIRE, not an aspiration. The spliced spine is deliberately
/// not converted here — see `models/src/rust/spliced_event_bytes.rs` and the
/// ruling recorded at `00ff9187`. When it *is* converted, this test must be
/// re-derived again rather than deleted, on the same rule that produced this
/// revision.
#[test]
fn the_direct_legs_are_flat_and_the_spliced_legs_still_slope() {
    println!(
        "\n  leg                      min stack @ {LADDER_LO}   @ {LADDER_HI}     B/level"
    );
    println!("  ────────────────────────  ───────────────  ───────────────  ─────────");

    /// One ladder reading, printed as it is taken.
    fn measure(leg: Leg) -> f64 {
        let lo = min_stack_for(leg, LADDER_LO);
        let hi = min_stack_for(leg, LADDER_HI);
        let b = slope(lo, hi);
        println!("  {:<24}  {lo:>15}  {hi:>15}  {b:>9.1}", leg.tag());
        b
    }

    /// A subject that must NOT grow, with its own reason for existing.
    fn assert_flat(leg: Leg, why: &str) -> f64 {
        let lo = min_stack_for(leg, LADDER_LO);
        let hi = min_stack_for(leg, LADDER_HI);
        let b = slope(lo, hi);
        println!("  {:<24}  {lo:>15}  {hi:>15}  {b:>9.1}", leg.tag());
        assert!(
            hi <= lo + ZERO_SLOPE_TOLERANCE,
            "`{}` SLOPED: {lo} B at depth {LADDER_LO} and {hi} B at depth \
             {LADDER_HI} — {} B of growth over {} levels, past the \
             {ZERO_SLOPE_TOLERANCE} B tolerance. {why}",
            leg.tag(),
            hi.saturating_sub(lo),
            LADDER_HI - LADDER_LO,
        );
        b
    }

    // ── leg 1 — THE DELIVERABLE: the three direct legs are depth-flat ─────────
    let mut converted: Vec<f64> = Vec::with_capacity(DIRECT_LEGS.len());
    for leg in DIRECT_LEGS {
        converted.push(assert_flat(
            leg,
            "This leg was CONVERTED — `event_hash_bytes_*` returns \
             `cold_encode()` on the map-free branch AND `contains_par` is an \
             explicit worklist. A regression here means one of those two was \
             undone, or a THIRD recursion has been introduced on the path; \
             `models/src/rust/spliced_event_bytes.rs` is the subject. \
             ⚠ Do not 'fix' this by widening the tolerance.",
        ));
    }

    // ── leg 2 — THE TRIPWIRE: the spliced spine is still Θ(depth) ─────────────
    let mut sloped: Vec<f64> = Vec::with_capacity(SPLICED_LEGS.len());
    for leg in SPLICED_LEGS {
        let b = measure(leg);
        sloped.push(b);
        assert!(
            b > 0.0,
            "leg {} did NOT slope. Either the hand-written `emit_*` spine has \
             been CONVERTED — in which case this test must be re-derived AGAIN \
             (move the leg up to the flat group; do not delete the assertion) — \
             or the spliced fixture stopped carrying depth, or stopped filling \
             its intern cell and silently fell through to the now-flat direct \
             path, which `the_spliced_fixture_actually_splices` is what catches.",
            leg.tag()
        );
    }

    // ── leg 3 — THE CONTROL: unchanged, and it must stay unchanged ────────────
    let mut controls: Vec<f64> = Vec::with_capacity(COLD_LEGS.len());
    for leg in COLD_LEGS {
        controls.push(assert_flat(
            leg,
            "THE CONTROL SLOPED. `cold_encode` is the trampolined encoder and \
             must be depth-flat; if it is not, then every verdict above is \
             uninterpretable, because the instrument, the fixture or the harness \
             — not the leg — is what is being measured.",
        ));
    }

    // ── the evidence margin ──────────────────────────────────────────────────
    //
    // ★ The margin is now taken between the SPLICED tripwire and the CONVERTED
    // legs, which is a strictly stronger statement than the pre-conversion form
    // could make: the two groups are the SAME three root types, on the SAME
    // instrument, in the SAME run, differing only in which emitter the fixture
    // routes to. A harness-wide artefact would move both groups together and
    // collapse this ratio.
    let worst_flat = converted
        .iter()
        .chain(controls.iter())
        .copied()
        .fold(0.0f64, f64::max);
    let weakest_sloped = sloped.iter().copied().fold(f64::INFINITY, f64::min);
    println!(
        "\n  weakest sloping leg: {weakest_sloped:.1} B/level   \
         strongest flat subject/control: {worst_flat:.1} B/level\n"
    );
    assert!(
        weakest_sloped > 10.0 * worst_flat.max(1.0),
        "The separation between the sloping spliced legs and the flat ones is \
         under 10×: weakest sloping {weakest_sloped:.1} B/level against \
         strongest flat {worst_flat:.1} B/level. That is not an evidence margin."
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
/// ⚠ The second half is asserted for the **three spliced** subjects only — the
/// ones still Θ(depth). The `*-cold` controls are depth-flat by design, and the
/// three DIRECT legs are now depth-flat by *repair*; both are expected to survive
/// 65,536 levels on [`DISCRIMINATOR_STACK`], and asserting otherwise would demand
/// that the deliverable fail.
///
/// ★ The direct legs are not merely dropped from the second half — they are moved
/// into it with the opposite polarity. "Survives 65,536 levels on 1 MiB" is a far
/// stronger statement than the ladder's "flat from 256 to 4,096": at 3,040 B/level
/// the unconverted leg would have needed ~190 MiB there. That is the conversion
/// stated as a fact about a term no node will ever legitimately see, rather than
/// as a slope.
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

    for leg in SPLICED_LEGS {
        assert!(
            !leg_survives(leg, DEEP, DISCRIMINATOR_STACK),
            "THE PROBE CANNOT GO RED: leg {} 'survived' depth {DEEP} on a {} KiB \
             stack. Either the child is not running the subject at all or the \
             environment is not reaching it.",
            leg.tag(),
            DISCRIMINATOR_STACK / 1024
        );
    }

    // ★ THE CONVERSION, stated at a depth no ladder reaches. At the measured
    // pre-conversion slope of 3,040 B/level (debug) a depth-65,536 datum needed
    // ~190 MiB of native stack; it now runs in one.
    for leg in DIRECT_LEGS {
        assert!(
            leg_survives(leg, DEEP, DISCRIMINATOR_STACK),
            "leg {} did NOT survive depth {DEEP} on a {} KiB stack. It is a \
             CONVERTED leg — `cold_encode()` on the map-free branch and an \
             explicit-worklist `contains_par` guard — so it must be independent \
             of term depth. A failure here is the conversion coming undone, not \
             a probe defect.",
            leg.tag(),
            DISCRIMINATOR_STACK / 1024
        );
    }
}

// ---------------------------------------------------------------------------
// ★★ THE SECOND FINDING — THE SPLICED PATH IS Θ(depth²) IN TIME
// ---------------------------------------------------------------------------

/// ★★ **A SEPARATE DEFECT FROM THE STACK ONE, IN THE SAME FUNCTION, AND THE
/// STACK CONVERSION DOES NOT TOUCH IT.**
///
/// # The mechanism, read from source
///
/// `emit_vec` and `emit_option` decide per element which emitter to use:
///
/// ```ignore
/// fn emit_vec<T>(items: &[T], contains: fn(&T) -> bool, emit: fn(&T, &mut Vec<u8>), out: &mut Vec<u8>) {
///     emit_u64(items.len() as u64, out);
///     for item in items {
///         if contains(item) { emit(item, out) } else { emit_black_box(item, out) }
///     }
/// }
/// ```
///
/// `contains` is the **whole-subtree** scan. On a chain of `d` levels that is
/// map-bearing all the way down, the walk calls it at level 1 on a subtree of
/// size `d`, at level 2 on a subtree of size `d − 1`, and so on:
///
/// ```math
/// T(d) \;=\; \sum_{k=1}^{d} \Theta(k) \;=\; \Theta(d^{2})
/// ```
///
/// ⚠ The stack conversion did **not** change this and could not have: making
/// `contains_par` iterative removes its native *frames*, not its *work*. The
/// asymptotics are unchanged, which is the honest reading and is why this is
/// filed as its own finding rather than absorbed into the conversion's report.
///
/// ★ The DIRECT path is **not** quadratic: it runs the scan exactly once, so it
/// is Θ(term size) in time. This test measures both, so "quadratic" is a
/// statement about the spliced spine specifically and not about event hashing.
///
/// # Why this is `#[ignore]`d
///
/// It is a **timing** measurement, and a wall-clock assertion in CI is a flake
/// generator on shared runners. It is kept executable and reproducible on demand
/// rather than deleted, because the alternative — a paragraph of prose asserting
/// a complexity class — is exactly the kind of claim that rots. Run it with:
///
/// ```text
/// cargo test --release -p casper --test event_hash_leg_depth_probe -- \
///     --ignored --nocapture the_spliced_path_is_quadratic_in_time
/// ```
///
/// # Reading the output
///
/// The `ns/level` column is the discriminator, not `ns`. Linear time would hold
/// it constant as depth grows; quadratic time makes it grow in proportion to
/// depth. The `ratio` column doubles the depth each row: **≈2 means linear, ≈4
/// means quadratic.**
#[test]
#[ignore = "wall-clock measurement; run explicitly (see the doc comment)"]
fn the_spliced_path_is_quadratic_in_time() {
    use std::time::Instant;

    /// Repeats per point, so a single scheduling hiccup cannot own a row.
    const REPS: u32 = 5;

    fn time_leg(build: fn(usize) -> Par, depth: usize) -> f64 {
        let mut best = f64::INFINITY;
        for _ in 0..REPS {
            // ★ The fixture is built OUTSIDE the timed region. Building it is
            // itself Θ(depth) and would otherwise be counted as the subject.
            let value = ListParWithRandom {
                pars: vec![build(depth)],
                random_state: vec![0xAA; 32],
            };
            let t0 = Instant::now();
            let bytes = event_hash_bytes_list_par_with_random(&value);
            let dt = t0.elapsed().as_secs_f64() * 1e9;
            // ★ ANTI-VACUITY, inside the loop: an emitter that returned early
            // would be timed as instantaneous and read as beautifully linear.
            assert!(
                bytes.len() > depth,
                "VACUOUS: depth {depth} produced only {} B",
                bytes.len()
            );
            std::mem::forget(value);
            std::mem::forget(bytes);
            best = best.min(dt);
        }
        best
    }

    println!("\n  ── SPLICED (filled cell at the bottom of the chain) ──");
    println!("  {:>7}  {:>14}  {:>12}  {:>7}", "depth", "ns", "ns/level", "ratio");
    let mut rows: Vec<(usize, f64)> = Vec::with_capacity(6);
    let mut prev_per_level = f64::NAN;
    for depth in [64usize, 128, 256, 512, 1_024, 2_048] {
        let ns = time_leg(nested_list_over_filled_map, depth);
        let per_level = ns / depth as f64;
        let ratio = per_level / prev_per_level;
        println!("  {depth:>7}  {ns:>14.0}  {per_level:>12.1}  {ratio:>7.2}");
        prev_per_level = per_level;
        rows.push((depth, ns));
    }

    println!("\n  ── DIRECT (map-free — the converted path, for contrast) ──");
    println!("  {:>7}  {:>14}  {:>12}  {:>7}", "depth", "ns", "ns/level", "ratio");
    let mut direct_rows: Vec<(usize, f64)> = Vec::with_capacity(6);
    let mut prev = f64::NAN;
    for depth in [64usize, 128, 256, 512, 1_024, 2_048] {
        let ns = time_leg(nested_list, depth);
        let per_level = ns / depth as f64;
        println!("  {depth:>7}  {ns:>14.0}  {per_level:>12.1}  {:>7.2}", per_level / prev);
        prev = per_level;
        direct_rows.push((depth, ns));
    }

    // The empirical exponent, by least squares on log-log. `t = c·dᵖ` ⇒
    // `log t = log c + p·log d`, so `p` is the slope.
    fn exponent(rows: &[(usize, f64)]) -> f64 {
        let n = rows.len() as f64;
        let (sx, sy) = rows.iter().fold((0.0, 0.0), |(sx, sy), (d, t)| {
            (sx + (*d as f64).ln(), sy + t.ln())
        });
        let (mx, my) = (sx / n, sy / n);
        let (num, den) = rows.iter().fold((0.0, 0.0), |(num, den), (d, t)| {
            let dx = (*d as f64).ln() - mx;
            (num + dx * (t.ln() - my), den + dx * dx)
        });
        num / den
    }

    let spliced_p = exponent(&rows);
    let direct_p = exponent(&direct_rows);
    println!(
        "\n  fitted exponent  spliced: {spliced_p:.2}   direct: {direct_p:.2}   \
         (1.0 = linear, 2.0 = quadratic)\n"
    );

    // ⚠ A loose bar, deliberately. The claim is a COMPLEXITY CLASS, not a
    // constant, and the point of separation is that the two paths land on
    // different sides of 1.5 — not that either hits its ideal exponent.
    assert!(
        spliced_p > 1.5,
        "the spliced path fitted an exponent of {spliced_p:.2}, i.e. it no longer \
         looks quadratic. If `contains_par` has been memoised or hoisted out of \
         `emit_vec`, this finding is RESOLVED and this test should be re-derived \
         into a linearity claim rather than deleted."
    );
    assert!(
        direct_p < 1.5,
        "the DIRECT path fitted an exponent of {direct_p:.2}. It runs the \
         contains-scan exactly once and must be linear; a quadratic direct path \
         would mean the scan is being re-entered somewhere it should not be."
    );
}
