//! # The term-op benchmark — `Clone`, weighted by a MEASURED distribution
//!
//! Stage F-4 replaced `<Par as Clone>::clone` (a `#[derive(Clone)]` expansion)
//! with an explicit-worklist traversal over `drive::drive_with`. That removes a
//! Θ(depth) native recursion, and removing a Θ(depth) recursion is worth nothing
//! if it costs throughput on the shape production actually clones.
//!
//! ## ★ Why the weighting is the whole experiment
//!
//! *"2× faster at depth 6,000 and 20% slower at depth 3 is a NET LOSS."* The
//! distribution is **measured, not assumed**, and it is the same one
//! `models/benches/wire_encode_bench.rs` established:
//! `ListParWithRandom::stable_hash_bytes` — the datum leg of `hash_produce`, which
//! runs once per produce — was instrumented and five interpreter suites were run,
//! giving **1,773 datums**:
//!
//! ```text
//!    depth   datums    share
//!    ─────   ──────   ──────
//!      1         12    0.68%
//!      2      1,692   95.43%     ◀── the case that decides the verdict
//!      3         36    2.03%
//!      4          3    0.17%
//!      5         26    1.47%
//!      6          4    0.23%
//!    ─────   ──────   ──────
//!    total    1,773            3.20 nodes/datum, 661 B/datum
//! ```
//!
//! **Nothing deeper than 6 was observed.** So the acceptance criterion is the
//! SHALLOW case, and the deep tail is reported but never weighted.
//!
//! ## The two arms
//!
//! | arm | what it is |
//! |---|---|
//! | `derived` | `term_ops::oracle_clone_par` — the `#[derive(Clone)]` body, re-emitted by the same generator from the same resolved fields, recursing into its own family rather than through `<Par as Clone>::clone`. Θ(depth) by construction. |
//! | `driven` | `<Par as Clone>::clone` — the generated `impl` over `drive_with` on thread-local pooled stacks. |
//!
//! ⚠ `derived` is the oracle `models/tests/clone_equivalence_corpus.rs` proves
//! byte-identical to `driven` on eight axes over 67 enumerated shapes, so this is
//! a like-for-like comparison of two implementations of one function, not of two
//! functions.
//!
//! ## Method
//!
//! * **Interleaved A/B.** One repetition measures A then B, and the loop repeats
//!   `REPS` times, so drift in clock, thermals or cache state moves both arms
//!   together.
//! * **Welch's t-test** with Welch–Satterthwaite degrees of freedom, at α = 0.01.
//! * **The teardown is OUTSIDE the timed region.** Both arms produce owned `Par`s
//!   that have to be released, and `drop_in_place::<Par>` is itself Θ(depth) (gate
//!   subject `par_drop`) — timing it would add the same large term to both arms and
//!   dilute the difference the experiment exists to measure. Each repetition clones
//!   into a preallocated sink (timed), then releases the sink through
//!   `par_children::dismantle_all` (untimed).
//! * ⚠ **The workload is built WITHOUT `Clone`.** `vec![datum(d); n]` would call the
//!   function under test `n − 1` times while constructing the fixture; every
//!   workload here is built by calling its constructor `n` times.
//! * **CPU affinity and frequency** are the caller's job:
//!   `taskset -c <cpu> cargo bench --bench term_ops_bench` with the governor at
//!   `performance`. The harness prints what it sees.
//!
//! ## The acceptance criterion
//!
//! `driven` ≥ **0.98×** `derived` on the production-weighted mix, with the two
//! arms' ranges non-overlapping at α = 0.01 — or indistinguishable, which is the
//! same verdict for this purpose. Reported explicitly by [`verdict`], PASS or
//! FAIL, rather than left for a reader to compute.
//!
//! ```text
//!   cargo bench -p models --bench term_ops_bench 2>&1 | tee /tmp/term_ops.log
//! ```

use std::hint::black_box;
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ETuple, Expr, Par, Send};
use models::rust::rholang::par_children::dismantle_all;
use models::rust::rholang::term_ops::oracle_clone_par;

// ---------------------------------------------------------------------------
// The measured distribution
// ---------------------------------------------------------------------------

/// `(depth, share)` — the measured produce-depth distribution. Shares are the
/// observed counts over 1,773 datums, not a smoothed model.
const MEASURED: &[(usize, f64)] = &[
    (1, 12.0 / 1773.0),
    (2, 1692.0 / 1773.0),
    (3, 36.0 / 1773.0),
    (4, 3.0 / 1773.0),
    (5, 26.0 / 1773.0),
    (6, 4.0 / 1773.0),
];

/// How many copies of each depth appear in one workload pass, scaled so the mix
/// reproduces the measured shares and one pass dominates timer granularity.
const WORKLOAD_SCALE: f64 = 2000.0;

/// The acceptance threshold: `driven` must be at least this fraction of
/// `derived`'s throughput on the weighted mix.
const THRESHOLD: f64 = 0.98;

fn gint(n: i64) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    }
}

fn gstr(s: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }],
        ..Default::default()
    }
}

/// A datum of nesting `depth` whose node count and byte size track the measured
/// averages (3.20 nodes, 661 B per datum) rather than being a bare spine — a spine
/// would understate the per-node cost the shallow case is meant to expose.
///
/// ★ Identical in shape to `wire_encode_bench.rs`'s `datum`, so the two
/// benchmarks' shallow readings are about the same terms. Returns the `Par`
/// directly rather than a `ListParWithRandom`, because `Clone` is the subject.
fn datum(depth: usize) -> Par {
    let mut body = Par {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(42)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("channel-payload".into())),
            },
        ],
        sends: vec![Send {
            chan: Some(gstr("ack")),
            data: vec![gint(1), gint(2)],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        }],
        locally_free: vec![],
        connective_used: false,
        ..Default::default()
    };
    for level in 1..depth {
        body = Par {
            exprs: vec![Expr {
                expr_instance: Some(if level % 2 == 0 {
                    ExprInstance::ETupleBody(ETuple {
                        ps: vec![body, gint(level as i64)],
                        locally_free: vec![],
                        connective_used: false,
                    })
                } else {
                    ExprInstance::EListBody(EList {
                        ps: vec![body, gstr("sibling")],
                        locally_free: vec![],
                        connective_used: false,
                        remainder: None,
                    })
                }),
            }],
            ..Default::default()
        };
    }
    body
}

/// A bare `EList` spine of `depth` levels — the shape the depth gate's ladders use,
/// and the cheapest way to reach a depth the derived form cannot survive.
fn spine(depth: usize) -> Par {
    let mut p = gint(0);
    for _ in 0..depth {
        p = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![p],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        };
    }
    p
}

/// The production-weighted workload: one pass over the measured mix.
///
/// ⚠ Built by calling `datum` once per copy. `vec![datum(d); n]` would clone.
fn weighted_workload() -> Vec<Par> {
    let mut out = Vec::new();
    for (depth, share) in MEASURED {
        let copies = (share * WORKLOAD_SCALE).round().max(1.0) as usize;
        for _ in 0..copies {
            out.push(datum(*depth));
        }
    }
    out
}

/// `n` independently-built datums of one depth. ⚠ Never `vec![_; n]`.
fn uniform_workload(depth: usize, n: usize) -> Vec<Par> {
    (0..n).map(|_| datum(depth)).collect()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

struct Sample {
    times_ns: Vec<f64>,
}

impl Sample {
    fn mean(&self) -> f64 {
        self.times_ns.iter().sum::<f64>() / self.times_ns.len() as f64
    }
    fn variance(&self) -> f64 {
        let m = self.mean();
        self.times_ns.iter().map(|t| (t - m).powi(2)).sum::<f64>()
            / (self.times_ns.len() as f64 - 1.0)
    }
    fn sd(&self) -> f64 {
        self.variance().sqrt()
    }
    /// The half-width of the α = 0.01 two-sided interval around the mean, by the
    /// normal approximation. Used for the "ranges non-overlapping" clause.
    fn half_width(&self) -> f64 {
        2.576 * self.sd() / (self.times_ns.len() as f64).sqrt()
    }
}

/// Welch's t, its Welch–Satterthwaite degrees of freedom, and a two-sided
/// significance verdict at α = 0.01 by the normal approximation (`REPS` is large
/// enough that `t` and `z` agree to three decimals).
fn welch(a: &Sample, b: &Sample) -> (f64, f64, bool) {
    let (na, nb) = (a.times_ns.len() as f64, b.times_ns.len() as f64);
    let (va, vb) = (a.variance(), b.variance());
    let se = (va / na + vb / nb).sqrt();
    if se == 0.0 {
        return (0.0, na + nb - 2.0, false);
    }
    let t = (a.mean() - b.mean()) / se;
    let df = (va / na + vb / nb).powi(2)
        / ((va / na).powi(2) / (na - 1.0) + (vb / nb).powi(2) / (nb - 1.0));
    (t, df, t.abs() > 2.576)
}

/// Repetitions per arm.
const REPS: usize = 60;
/// Discarded leading repetitions: the thread-local stack pool reaches its steady
/// state within a handful of passes, and including the cold ones would measure
/// warm-up rather than throughput.
const WARMUP: usize = 10;

/// Time one arm over `workload`, releasing each pass's clones OUTSIDE the timer.
fn measure(label: &str, workload: &[Par], mut clone_one: impl FnMut(&Par) -> Par) -> Sample {
    let mut times = Vec::with_capacity(REPS);
    // Preallocated once and drained (not dropped) each pass, so no pass after the
    // first pays a reallocation.
    let mut sink: Vec<Par> = Vec::with_capacity(workload.len());
    for rep in 0..(REPS + WARMUP) {
        let start = Instant::now();
        for value in workload {
            sink.push(black_box(clone_one(black_box(value))));
        }
        let elapsed = start.elapsed();
        // ⚠ UNTIMED. `drop_in_place::<Par>` is Θ(depth) (gate subject `par_drop`),
        // so timing the release would add the same large term to both arms.
        dismantle_all(sink.drain(..));
        if rep >= WARMUP {
            times.push(elapsed.as_nanos() as f64);
        }
    }
    let s = Sample { times_ns: times };
    println!(
        "  {label:10} mean {:>12.1} ns   sd {:>10.1} ns   ({:.2}%)   ±{:.1} ns @ α=0.01",
        s.mean(),
        s.sd(),
        100.0 * s.sd() / s.mean(),
        s.half_width()
    );
    s
}

fn report(name: &str, derived: &Sample, driven: &Sample) -> f64 {
    let (t, df, significant) = welch(derived, driven);
    let speedup = derived.mean() / driven.mean();
    let delta = 100.0 * (driven.mean() - derived.mean()) / derived.mean();
    println!(
        "  ── {name}: {speedup:.3}× ({delta:+.2}%)  Welch t = {t:.2}, df = {df:.1}, \
         significant at α=0.01: {significant}"
    );
    if !significant {
        println!("     (no significant difference — the arms are indistinguishable)");
    }
    speedup
}

/// ★★ **THE VERDICT**, stated rather than left to the reader.
///
/// PASS iff `driven` is at least [`THRESHOLD`] × `derived`'s throughput. The
/// "ranges non-overlapping at α = 0.01" clause is reported alongside: two arms
/// whose intervals overlap are *indistinguishable*, which satisfies the criterion
/// as surely as a measured win does — the criterion is "not slower", not "faster".
fn verdict(name: &str, derived: &Sample, driven: &Sample) {
    let speedup = derived.mean() / driven.mean();
    let overlap = (derived.mean() - derived.half_width()) < (driven.mean() + driven.half_width())
        && (driven.mean() - driven.half_width()) < (derived.mean() + derived.half_width());
    let pass = speedup >= THRESHOLD;
    println!();
    println!(
        "  ╔══ ACCEPTANCE: {name} ══",
    );
    println!(
        "  ║ driven / derived throughput = {speedup:.4}×   threshold = {THRESHOLD:.2}×   \
         => {}",
        if pass { "PASS" } else { "FAIL" }
    );
    println!(
        "  ║ α=0.01 intervals {}",
        if overlap {
            "OVERLAP — the two arms are indistinguishable, which satisfies \"not slower\""
        } else {
            "are DISJOINT — the difference is resolved"
        }
    );
    if !pass {
        println!(
            "  ║ ⚠ FAIL. The shallow case is the verdict: 95.43% of production terms are at \
             depth 2, so a deep-tail win does not pay for a shallow-case loss. The fallback in \
             the plan is a depth-k hybrid, which keeps a Θ(depth) prefix and must still be flat \
             overall — bring it to the reviewer rather than shipping it."
        );
    }
    println!("  ╚══");
    println!();
}

fn environment() {
    println!("ENVIRONMENT");
    for (label, path) in [
        ("governor", "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        ("cur_freq", "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"),
        ("max_freq", "/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq"),
    ] {
        let value = std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unavailable".into());
        println!("  {label:10} {value}");
    }
    println!(
        "  profile    {}",
        if cfg!(debug_assertions) {
            "DEBUG — ⚠ a throughput claim from a debug build is not a throughput claim"
        } else {
            "release"
        }
    );
    println!();
}

/// ★ The DEEP leg: the driven clone at depth 4,096, on a stack the derived form
/// provably cannot survive.
///
/// ⚠ The derived arm is **not attempted** here, and the reason is a standing rule
/// rather than caution: a stack overflow is `SIGSEGV` on the guard page, not a
/// catchable `Err` or an unwinding panic, so a probe that overflows takes the whole
/// process with it. The executed version of "the derived form does not survive"
/// lives in `rholang/tests/stack_depth_gate.rs`, which runs each subject in a
/// CHILD PROCESS and reads its exit status
/// (`the_clone_conversion_is_visible_against_its_own_derived_control`, leg 3).
///
/// What this leg establishes is the positive half: the driven clone COMPLETES at a
/// depth where 3,254 B/level (the derived form's bisected release cost) would need
/// **13.0 MiB** — 6.5× a tokio worker's default 2 MiB.
fn deep_leg() {
    const DEPTH: usize = 4096;
    // 1 MiB: measured sufficient for the driven clone (12 KiB release, 48 KiB
    // debug at depth 128, and FLAT), and 13× short of what the derived form needs.
    const STACK: usize = 1024 * 1024;

    println!("DEEP LEG — the driven clone at depth {DEPTH} on a {} KiB stack", STACK / 1024);
    let handle = std::thread::Builder::new()
        .stack_size(STACK)
        .name("term-ops-deep".to_string())
        .spawn(|| {
            // Built ITERATIVELY, so the fixture does not consume the stack the
            // clone is being measured on.
            let term = spine(DEPTH);
            let start = Instant::now();
            let clone = black_box(term.clone());
            let elapsed = start.elapsed();
            // ⚠ Both released ITERATIVELY: `drop_in_place::<Par>` at depth 4,096
            // would overflow this very stack.
            let mut depth = 0usize;
            let mut cur = &clone;
            while let Some(ExprInstance::EListBody(l)) =
                cur.exprs.first().and_then(|e| e.expr_instance.as_ref())
            {
                if l.ps.is_empty() {
                    break;
                }
                depth += 1;
                cur = &l.ps[0];
            }
            dismantle_all([clone, term]);
            (elapsed, depth)
        })
        .expect("term_ops_bench: failed to spawn the deep-leg thread");
    let (elapsed, measured_depth) = handle
        .join()
        .expect("term_ops_bench: the driven clone did not survive the deep leg");
    assert_eq!(
        measured_depth, DEPTH,
        "VACUOUS: the deep clone carries depth {measured_depth}, not {DEPTH} — a collapsed \
         fixture runs in O(1) stack and proves nothing"
    );
    println!(
        "  driven: COMPLETED in {:.1} µs, clone carries depth {measured_depth}",
        elapsed.as_nanos() as f64 / 1000.0
    );
    println!(
        "  derived: NOT ATTEMPTED in-process — at its bisected 3,254 B/level (release) depth \
         {DEPTH} needs {:.1} MiB, 13× this thread's stack. A stack overflow is SIGSEGV on the \
         guard page and is uncatchable, so the executed refusal is \
         `stack_depth_gate::the_clone_conversion_is_visible_against_its_own_derived_control` \
         leg 3, which reads a CHILD PROCESS's exit status.",
        3254.0 * DEPTH as f64 / (1024.0 * 1024.0)
    );
    println!();
}

fn main() {
    environment();

    // -------------------------------------------------------------------
    // ★★ THE VERDICT: the production-weighted mix
    // -------------------------------------------------------------------
    let workload = weighted_workload();
    println!(
        "PRODUCTION-WEIGHTED MIX — {} datums/pass (95.43% at depth 2, nothing deeper than 6)",
        workload.len()
    );
    let derived = measure("derived", &workload, |p| oracle_clone_par(p));
    let driven = measure("driven", &workload, |p| p.clone());
    report("weighted", &derived, &driven);
    verdict("the production-weighted mix", &derived, &driven);

    // -------------------------------------------------------------------
    // Per depth, so a regression at the dominant depth cannot hide behind a
    // win at the tail.
    // -------------------------------------------------------------------
    println!("PER DEPTH (unweighted; depth 2 carries 95.43% of production)");
    for depth in [1usize, 2, 3, 4, 6, 16, 64] {
        let one = uniform_workload(depth, 512);
        println!("  depth {depth}:");
        let d = measure("derived", &one, |p| oracle_clone_par(p));
        let m = measure("driven", &one, |p| p.clone());
        report(&format!("depth {depth}"), &d, &m);
        dismantle_all(one);
    }
    println!();

    // -------------------------------------------------------------------
    // The deep tail. Reported, never weighted — the measured distribution
    // stops at depth 6.
    // -------------------------------------------------------------------
    println!("DEEP TAIL (never weighted — the measured distribution stops at depth 6)");
    for depth in [256usize, 1024] {
        // ⚠ On a BIG stack: the `derived` arm is Θ(depth) and at 1,024 levels needs
        // more than the 8 MiB a main thread gets. `driven` needs 12 KiB.
        let handle = std::thread::Builder::new()
            .stack_size(512 * 1024 * 1024)
            .name("term-ops-tail".to_string())
            .spawn(move || {
                let one = uniform_workload(depth, 4);
                println!("  depth {depth}:");
                let d = measure("derived", &one, |p| oracle_clone_par(p));
                let m = measure("driven", &one, |p| p.clone());
                let speedup = report(&format!("depth {depth}"), &d, &m);
                dismantle_all(one);
                speedup
            })
            .expect("term_ops_bench: failed to spawn the tail thread");
        let speedup = handle.join().expect("term_ops_bench: the tail leg panicked");
        println!("     (reported only; {speedup:.3}× does not enter the verdict)");
    }
    println!();

    deep_leg();

    dismantle_all(workload);
}
