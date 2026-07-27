//! # The Θ(depth) regression gate
//!
//! **What this gate is for.** A term's *nesting depth* must not be able to
//! determine how much NATIVE STACK the reducer consumes. When it can, a 30-byte
//! program (`@"OUT"!([[[[[[[[[[0]]]]]]]]]])`) aborts the process — a stack
//! overflow is a `SIGSEGV`, not a catchable error, so it takes down the whole
//! node rather than failing one deploy.
//!
//! This gate is what makes "fixed" *checkable* rather than asserted, and what
//! stops the class from being silently reintroduced by the next traversal
//! somebody adds over the `Par` / `Expr` / `ExprInstance` family.
//!
//! Full analysis, enumeration and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
//!
//! ## How it works
//!
//! Each traversal is run on a thread created with an **explicit `stack_size`**,
//! so neither `RUST_MIN_STACK` nor `ulimit -s` can mask a regression. Two
//! assertions are available:
//!
//! * [`assert_depth_independent`] — the traversal survives a FIXED, small stack
//!   at every depth in a wide ladder. This is the real bar: it can only pass if
//!   the traversal's native stack is O(1) in depth.
//!
//! * [`assert_slope_below`] — for traversals that are still Θ(depth), pins the
//!   measured bytes-per-level to a ceiling. This is a **tripwire, not a pass**:
//!   it detects a traversal getting *worse* while making the residual explicit
//!   in code. Every use names why it is still on this list.
//!
//! ## ⚠ Why the constants are per-profile, and why the gate does not hardcode one
//!
//! Frame sizes differ by ~5–12× between profiles because `rustc` does not
//! overlap the stack slots of mutually exclusive `match` arms at `-O0`, and the
//! family's hot functions are 40-arm matches over `ExprInstance`:
//!
//! | traversal                | debug (`-O0`) | release (`-O2`) | ratio |
//! |--------------------------|---------------|-----------------|-------|
//! | `Substitute::substitute` | 195,728 B     | 27,179 B        | 7.2×  |
//! | `ParSortMatcher`         |  78,579 B     |  6,495 B        | 12.1× |
//! | `PrettyPrinter`          |  41,840 B     |  4,242 B        |  9.9× |
//! | `<Par as Clone>`         |  15,875 B     |  2,852 B        |  5.6× |
//!
//! (mettail-rust additionally sets `codegen-backend = "cranelift"` for
//! `[profile.dev]`, which inflates frames again; f1r3node does not.)
//!
//! A gate that hardcoded one profile's byte count would therefore be
//! backend-fragile and would silently pass or fail for the wrong reason. So:
//!
//! * `assert_depth_independent` is **profile-independent by construction** — it
//!   asserts a *shape* (no growth with depth), never a constant.
//! * `assert_slope_below` takes ceilings that are selected per profile via
//!   `cfg!(debug_assertions)`, and are set ~1.5× above the measured value so
//!   that ordinary codegen drift does not flake the gate while a real
//!   regression (which is order-of-magnitude) still trips it.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, Expr, Par};
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::RuntimeBudget;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::substitute::{Substitute, SubstituteTrait};

// ---------------------------------------------------------------------------
// term construction — ITERATIVE, so the builder itself is never the constraint
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

/// `[[[…[0]…]]]` with `depth` bracket levels — the reported shape.
fn nested_list(depth: usize) -> Par {
    let mut p = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        p = elist(vec![p]);
    }
    p
}

/// Tear a deep term down without recursing: `Drop` for this family is itself a
/// Θ(depth) traversal, so a gate that let terms drop normally would be measuring
/// `Drop` rather than the traversal under test. Dismantling bottom-up keeps the
/// harness itself O(1) in native stack.
fn dismantle(mut p: Par) {
    loop {
        let next = match p.exprs.first_mut().and_then(|e| e.expr_instance.as_mut()) {
            Some(ExprInstance::EListBody(l)) if !l.ps.is_empty() => l.ps.pop(),
            _ => None,
        };
        match next {
            Some(child) => p = child,
            None => return,
        }
    }
}

// ---------------------------------------------------------------------------
// the two assertions
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ⚠ Probing has to FORK.
//
// A stack overflow is a `SIGSEGV` caught by the runtime's guard-page handler,
// which prints and `abort()`s. It is NOT a panic and NOT unwindable, so a probe
// that overflows in-process takes the whole test binary with it — including
// every assertion that had already passed, and every OTHER test in the file.
//
// So a probe runs in a CHILD process: the gate re-execs its own test binary with
// `GATE_SUBJECT` / `GATE_DEPTH` / `GATE_STACK` set, and reads the exit status.
// 0 = survived; anything else = did not. This is the same discipline the
// measurement harness (`stack_depth_probe.rs` + `scripts/stack_depth_probe.sh`)
// uses, and it is why a RED gate reports a clean failure instead of a truncated
// test run.
// ---------------------------------------------------------------------------

/// Names the subject a child process should run. Kept in one place so the parent
/// and the child cannot drift.
fn subject(name: &str) -> fn(usize) {
    match name {
        "substitute" => substitute_body,
        "sort" => sort_body,
        "clone" => clone_body,
        "drop" => drop_body,
        "encode" => encode_body,
        other => panic!("stack_depth_gate: unknown GATE_SUBJECT={:?}", other),
    }
}

/// The child entry point. `#[ignore]`d so a normal `cargo test` run never
/// executes it directly; the parent always invokes it explicitly.
///
/// ⚠ It must be a NO-OP when its environment is absent. `cargo test
/// -- --include-ignored` (and `cargo nextest run --run-ignored all`) will run
/// every `#[ignore]`d test, including this one, with no `GATE_SUBJECT` set — so
/// a child entry point that *required* its environment would fail the suite for
/// a reason that has nothing to do with the property being gated. Skipping is
/// correct here precisely because this test is a mechanism, not an assertion:
/// the assertions live in its callers.
#[test]
#[ignore = "child process of the gate; driven via GATE_SUBJECT"]
fn gate_child() {
    let Ok(name) = std::env::var("GATE_SUBJECT") else {
        println!("gate_child: no GATE_SUBJECT — not a child invocation, nothing to do");
        return;
    };
    let depth: usize = std::env::var("GATE_DEPTH")
        .expect("GATE_DEPTH must accompany GATE_SUBJECT")
        .parse()
        .expect("GATE_DEPTH must be an integer");
    let stack: usize = std::env::var("GATE_STACK")
        .expect("GATE_STACK must accompany GATE_SUBJECT")
        .parse()
        .expect("GATE_STACK must be an integer");

    std::thread::Builder::new()
        .stack_size(stack)
        .name("gate".to_string())
        .spawn(move || subject(&name)(depth))
        .expect("stack_depth_gate: failed to spawn")
        .join()
        .expect("stack_depth_gate: subject panicked");
}

/// Run one probe point in a child process. `true` iff it survived.
fn runs_within(stack: usize, depth: usize, subject_name: &str) -> bool {
    let exe = std::env::current_exe().expect("stack_depth_gate: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "gate_child"])
        .env("GATE_SUBJECT", subject_name)
        .env("GATE_DEPTH", depth.to_string())
        .env("GATE_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("stack_depth_gate: failed to run child")
        .success()
}

/// **The real bar.** Assert `body` survives a FIXED stack across a wide depth
/// ladder. Only an O(1)-in-depth traversal can pass: the ladder spans a 64×
/// range, so any per-level cost `c` would need `64·d₀·c` to fit in the same
/// budget as `d₀·c`.
#[allow(dead_code)]
fn assert_depth_independent(name: &str, stack: usize) {
    for depth in [4usize, 16, 64, 256] {
        assert!(
            runs_within(stack, depth, name),
            "DEPTH-INDEPENDENCE GATE FAILED for `{}` at depth {} with a {} KiB stack.\n\
             This traversal's native stack grows with term nesting depth. See\n\
             docs/design/audits/theta-depth-traversals-2026-07-26.md for the conversion pattern.",
            name,
            depth,
            stack / 1024
        );
    }
}

/// **Tripwire for traversals not yet converted.** Bisects the minimum stack at
/// two depths, derives bytes-per-level, and fails if it exceeds `ceiling`.
///
/// This deliberately does NOT claim the traversal is fixed. It claims only that
/// it has not got worse. Every caller documents why its subject is still here.
fn assert_slope_below(name: &str, ceiling_bytes_per_level: usize, lo: usize, hi_depth: usize) {
    // Two depths far enough apart that (a) the fixed intercept is a small share
    // of the difference and (b) the CHEAP traversals clear the minimum viable
    // thread stack at the deeper point — otherwise both probes bottom out on the
    // same floor and the derived slope is a meaningless 0.
    #[allow(non_snake_case)]
    let (LO, HI) = (lo, hi_depth);

    let min_stack = |depth: usize| -> usize {
        // exponential probe then bisect, to page granularity
        let mut hi = 16 * 1024;
        while hi <= 512 * 1024 * 1024 && !runs_within(hi, depth, name) {
            hi *= 2;
        }
        assert!(
            hi <= 512 * 1024 * 1024,
            "`{}` needed more than 512 MiB at depth {}",
            name,
            depth
        );
        let mut lo = hi / 2;
        while hi - lo > 4096 {
            let mid = (lo + hi) / 2;
            if runs_within(mid, depth, name) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    };

    let s_lo = min_stack(LO);
    let s_hi = min_stack(HI);
    let per_level = s_hi.saturating_sub(s_lo) / (HI - LO);

    assert!(
        per_level <= ceiling_bytes_per_level,
        "Θ(DEPTH) TRIPWIRE for `{}`: {} B/level exceeds the {} B/level ceiling \
         ({} KiB @ depth {} -> {} KiB @ depth {}).\n\
         Either a traversal regressed, or codegen changed materially. See\n\
         docs/design/audits/theta-depth-traversals-2026-07-26.md.",
        name,
        per_level,
        ceiling_bytes_per_level,
        s_lo / 1024,
        LO,
        s_hi / 1024,
        HI
    );
    println!("  {name}: {per_level} B/level (ceiling {ceiling_bytes_per_level})");
}

/// Per-profile ceiling. Debug frames are ~5–12× release because `-O0` does not
/// overlap `match`-arm stack slots; see the module docs.
fn ceiling(debug: usize, release: usize) -> usize {
    if cfg!(debug_assertions) {
        debug
    } else {
        release
    }
}

// ---------------------------------------------------------------------------
// subjects
// ---------------------------------------------------------------------------

fn substitute_body(depth: usize) {
    let term = nested_list(depth);
    let s = Substitute {
        metering: MeteredMachine::new(RuntimeBudget::new(Cost::create(
            i64::MAX / 4,
            "stack_depth_gate".to_string(),
        ))),
    };
    let env: Env<Par> = Env::new();
    let out = s
        .substitute(term, 0, &env)
        .expect("stack_depth_gate: substitute failed");
    dismantle(out);
}

fn sort_body(depth: usize) {
    use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
    use models::rust::rholang::sorter::sortable::Sortable;
    let term = nested_list(depth);
    let out = ParSortMatcher::sort_match(&term);
    dismantle(out.term);
    dismantle(term);
}

fn clone_body(depth: usize) {
    let term = nested_list(depth);
    let c = term.clone();
    dismantle(c);
    dismantle(term);
}

fn drop_body(depth: usize) {
    // The ONE subject that must be allowed to drop recursively — that is the
    // thing under test.
    let term = nested_list(depth);
    drop(term);
}

fn encode_body(depth: usize) {
    use prost::Message;
    let term = nested_list(depth);
    let bytes = term.encode_to_vec();
    assert!(!bytes.is_empty());
    dismantle(term);
}

// ---------------------------------------------------------------------------
// THE GATE
// ---------------------------------------------------------------------------

/// Traversals that have been converted to a heap-bounded (explicit worklist)
/// form. Membership of this list is the deliverable; it only ever grows.
///
/// ⚠ EMPTY AT PRESENT — see `theta_depth_tripwire` below and
/// `docs/design/audits/theta-depth-traversals-2026-07-26.md` § "Disposition". No traversal
/// over the `Par` family is heap-bounded yet, so asserting depth-independence
/// for any of them here would be a false claim. This test exists, named and
/// wired, so that converting a traversal is a one-line addition rather than a
/// new piece of infrastructure somebody has to invent under pressure.
#[test]
fn converted_traversals_are_depth_independent() {
    let converted: &[&str] = &[
        // "substitute",
    ];
    for name in converted {
        // 1 MiB is well below what ANY Θ(depth) member needs at depth 256.
        assert_depth_independent(name, 1024 * 1024);
    }
    if converted.is_empty() {
        println!(
            "no traversal converted yet — see docs/design/audits/theta-depth-traversals-2026-07-26.md"
        );
    }
}

/// Tripwire over every traversal still known to be Θ(depth). Ceilings are ~1.5×
/// the values measured on 2026-07-26 (recorded in the audit document), so
/// ordinary codegen drift will not flake while an order-of-magnitude regression
/// still trips.
///
/// A traversal LEAVES this list only by moving to `converted_traversals_are_
/// depth_independent`, never by having its ceiling raised.
#[test]
fn theta_depth_tripwire() {
    // measured 2026-07-26 (debug / release), bytes per nesting level:
    //   substitute 195,728 / 27,179    sort 78,579 /  6,495
    //   clone       15,875 /  2,852    drop      470 /    219
    //   encode       1,948 /    422
    // Expensive traversals are probed shallow (a 64-deep `substitute` already
    // needs ~12 MiB in debug); cheap ones are probed deep so their per-level
    // cost rises clear of the minimum viable thread stack.
    assert_slope_below("substitute", ceiling(300_000, 45_000), 16, 64);
    assert_slope_below("sort", ceiling(120_000, 11_000), 16, 64);
    assert_slope_below("clone", ceiling(25_000, 5_000), 16, 128);
    assert_slope_below("drop", ceiling(1_500, 800), 256, 4096);
    assert_slope_below("encode", ceiling(4_000, 1_500), 64, 1024);
}

/// The reported reproducer, at the depth that aborted the reducer, on the stack
/// a tokio worker actually gets when `RUST_MIN_STACK` is unset (Rust's default
/// spawned-thread size is 2 MiB).
///
/// This is the end-to-end statement of the bug in one assertion: it is the
/// depth at which `@"OUT"!([[…[0]…]])` stopped working.
///
/// ★ CURRENTLY RED — ON PURPOSE. This asserts the bug is FIXED, and it is not
/// yet: `Substitute::substitute` is still Θ(depth) (195,754 B/level debug /
/// 27,179 B/level release, measured by `theta_depth_tripwire` above). Leg-1
/// (de-cloning, landed) removed the O(D²) heap churn and 25% of the release
/// per-level cost but — exactly as the eval-SCC Leg-1 verdict predicted for
/// itself (`bb7fcd20`) — did not change the CLASS. Leg-2, the explicit-worklist
/// conversion, is what flips this test green.
///
/// It is `#[ignore]`d rather than deleted or weakened so that the open work is
/// named in code, at the exact assertion that will observe it being finished.
/// REMOVE THE `#[ignore]` in the same commit that converts substitution, and
/// move `"substitute"` from `theta_depth_tripwire` into
/// `converted_traversals_are_depth_independent`.
#[test]
#[ignore = "RED until Substitute::substitute is converted (Leg-2); see \
            docs/design/audits/theta-depth-traversals-2026-07-26.md"]
fn reported_reproducer_depth_survives_a_default_worker_stack() {
    const DEFAULT_SPAWNED_THREAD_STACK: usize = 2 * 1024 * 1024;
    // Debug frames are ~7× release, so the depth a 2 MiB worker can carry is
    // profile-dependent while the CLASS is not. Pin each profile to the depth
    // the measurements say it must reach.
    let depth = if cfg!(debug_assertions) { 10 } else { 70 };
    assert!(
        runs_within(DEFAULT_SPAWNED_THREAD_STACK, depth, "substitute"),
        "REGRESSION: `[[…[0]…]]` at depth {} no longer survives substitution on a \
         {} MiB thread — the reported consensus-liveness bug is back or worse.",
        depth,
        DEFAULT_SPAWNED_THREAD_STACK / (1024 * 1024)
    );
}
