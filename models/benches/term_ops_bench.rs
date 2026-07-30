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
//! * **★ GENUINELY interleaved, PAIRED A/B.** One repetition times A then B —
//!   and alternates that order by repetition parity — so a load excursion or a
//!   clock/thermal drift lands inside one repetition and **cancels in the
//!   difference** rather than landing on whichever arm happened to be running.
//!   ⚠ This module claimed interleaving for a long time while
//!   [`measure`]-as-written ran *every* repetition of one arm and was then called
//!   again for the other. The two arms were measured in **different time
//!   windows**, and on this host at load average 15.7 two back-to-back runs of
//!   the identical binary reported **1.0748× (PASS)** and **0.9461× (FAIL)** on
//!   the weighted mix. Every ratio quoted from the old instrument — 0.678×,
//!   0.665×, and the −2.8% that "refuted" the walk elimination — carries that
//!   uncertainty. See the shared [`paired`] module.
//! * **The paired t-test** on the per-repetition difference at α = 0.01, plus the
//!   **median per-repetition ratio** as the load-robust point estimate. Welch's
//!   unpaired t (with Welch–Satterthwaite df) is still printed for continuity
//!   with the prior reports, so a reader can see how much weaker it is on the
//!   same data.
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
//! `driven` ≥ **0.98×** `derived` on the production-weighted mix, judged on the
//! **median per-repetition ratio**, with the paired α = 0.01 interval on the
//! difference reported alongside — an interval containing zero means the arms are
//! indistinguishable, which is the same verdict for this purpose. Reported
//! explicitly by [`verdict`], PASS or FAIL, rather than left for a reader to
//! compute.
//!
//! ⚠ **Every wall-time figure here is only as good as the machine it was taken
//! on.** [`environment`] prints `/proc/loadavg` before and after, because this
//! workspace routinely has several concurrent builds running and a ratio quoted
//! without its load is not a measurement.
//!
//! ```text
//!   cargo bench -p models --bench term_ops_bench 2>&1 | tee /tmp/term_ops.log
//! ```

use std::hint::black_box;
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ETuple, Expr, Par, Send};
// ★ The SHARED paired-measurement harness — ONE implementation for this bench
// and `wire_encode_bench`, both of which carried the same all-A-then-all-B defect
// behind the same false "interleaved A/B" claim. See `paired.rs`.
#[path = "paired.rs"]
mod paired;
use paired::{loadavg, measure_arms, welch, Arms, Pair};

use models::rust::rholang::drive::{Outcome, Step};
use models::rust::rholang::par_children::dismantle_all;
use models::rust::rholang::term_ops::{oracle_clone_par, CloneKont, CloneNode, CloneTraversal, CloneVal};

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

/// ★★ **The SINGLE-NODE leg — `n = 1`. Stage 0b.**
///
/// `datum(d)` contains `n(d) = 2d + 2` `Par` nodes (one wrapper plus one sibling
/// per level over a four-node base), so the per-depth legs sample
/// `n ∈ {4, 6, 8, 10, 14, 34, 130}` and **never reach the intercept.** Fitting
/// `T(n) = A + B·n` on that range extrapolates `A` from six points none of which
/// is near `n = 1`, and `b228545f`'s own two-parameter fit over-predicts the
/// mid-range (0.738 vs 0.665 at `d = 2`) — evidence that a third component
/// exists.
///
/// A workload of bare `gint(0)`s is **exactly one `Par` node per call**: one
/// `exprs` entry holding a `GInt`, no nested `Par` anywhere. So
/// `T(1) = A + B`, and with `B` estimated from the slope of the per-depth legs
/// the fixed per-call cost `A` is **measured within one node-equivalent rather
/// than fitted from a range that excludes it.**
///
/// ⚠ Never `vec![gint(0); n]` — that would call the function under test while
/// building the fixture.
fn single_node_workload(n: usize) -> Vec<Par> {
    (0..n).map(|_| gint(0)).collect()
}

/// ★ **The node count of one `datum(depth)`, computed rather than asserted from
/// a formula.**
///
/// The regression in Stage 0a is denominated in `Par` nodes per call, so the
/// benchmark must *report* the count it is regressing on instead of leaving the
/// reader to trust `n(d) = 2d + 2`. Walks the same family the clone traversal
/// does: `Par → exprs/sends/…`, counting every `Par` reachable from the root
/// including the root. Recursive, and that is fine — it runs on fixtures of
/// depth ≤ 64, outside every timed region.
fn count_par_nodes(p: &Par) -> usize {
    fn walk_expr(e: &Expr, acc: &mut usize) {
        match e.expr_instance.as_ref() {
            Some(ExprInstance::EListBody(l)) => {
                for q in &l.ps {
                    walk(q, acc);
                }
            }
            Some(ExprInstance::ETupleBody(t)) => {
                for q in &t.ps {
                    walk(q, acc);
                }
            }
            _ => {}
        }
    }
    fn walk(p: &Par, acc: &mut usize) {
        *acc += 1;
        for e in &p.exprs {
            walk_expr(e, acc);
        }
        for s in &p.sends {
            if let Some(c) = s.chan.as_ref() {
                walk(c, acc);
            }
            for d in &s.data {
                walk(d, acc);
            }
        }
    }
    let mut acc = 0;
    walk(p, &mut acc);
    acc
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// The two clone arms, measured in the same repetitions and order-alternated:
/// the derived oracle and the driven `<Par as Clone>::clone`.
///
/// ⚠ The release closure is load-bearing and UNTIMED: both arms produce owned
/// `Par`s, and `drop_in_place::<Par>` is itself Θ(depth) (gate subject
/// `par_drop`), so timing the teardown would add the same large term to both arms
/// and dilute the difference the experiment exists to measure.
fn clone_arms(workload: &[Par]) -> Arms<2> {
    let mut derived = |p: &Par| oracle_clone_par(p);
    let mut driven = <Par as Clone>::clone;
    measure_arms(
        ["derived", "driven"],
        workload,
        &mut [&mut derived, &mut driven],
        &mut |drain| dismantle_all(drain),
    )
}

fn report(name: &str, p: &Pair<'_>) -> f64 {
    let (derived, driven) = (&p.a, &p.b);
    let (wt, df, w_significant) = welch(derived, driven);
    let (pt, p_significant) = p.paired_t();
    let speedup = derived.mean() / driven.mean();
    let median_speedup = p.median_speedup();
    let delta = 100.0 * (driven.mean() - derived.mean()) / derived.mean();
    println!(
        "  ── {name}: {speedup:.3}× of-means ({delta:+.2}%), {median_speedup:.3}× median-of-rep \
         | paired t = {pt:.2} excludes 0: {p_significant} | Welch t = {wt:.2}, df = {df:.1}, \
         significant: {w_significant}"
    );
    if !p_significant {
        println!("     (the paired difference does not exclude 0 — the arms are indistinguishable)");
    }
    if (speedup - median_speedup).abs() > 0.02 {
        println!(
            "     ⚠ of-means and median-of-rep disagree by {:.3}× — the run is load-contaminated; \
             the MEDIAN is the estimate to use",
            (speedup - median_speedup).abs()
        );
    }
    median_speedup
}

/// ★★ **THE VERDICT**, stated rather than left to the reader.
///
/// PASS iff `driven` is at least [`THRESHOLD`] × `derived`'s throughput, **judged
/// on the median per-repetition ratio** — the load-robust estimator. The paired
/// α = 0.01 interval on the difference is reported alongside: an interval that
/// contains zero means the arms are *indistinguishable*, which satisfies the
/// criterion as surely as a measured win does — the criterion is "not slower",
/// not "faster".
fn verdict(name: &str, p: &Pair<'_>) {
    let (derived, driven) = (&p.a, &p.b);
    let speedup = p.median_speedup();
    let of_means = derived.mean() / driven.mean();
    let (_, resolved) = p.paired_t();
    let pass = speedup >= THRESHOLD;
    println!();
    println!(
        "  ╔══ ACCEPTANCE: {name} ══",
    );
    println!(
        "  ║ driven / derived throughput = {speedup:.4}× (median-of-rep; {of_means:.4}× \
         of-means)   threshold = {THRESHOLD:.2}×   => {}",
        if pass { "PASS" } else { "FAIL" }
    );
    println!(
        "  ║ paired α=0.01 interval on Δ = {:+.1} ± {:.1} ns — {}",
        p.diff_mean(),
        p.diff_half_width(),
        if resolved {
            "EXCLUDES 0, the difference is resolved"
        } else {
            "CONTAINS 0 — the two arms are indistinguishable, which satisfies \"not slower\""
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
    println!("  loadavg    {}   ◀── every ns below is conditional on this", loadavg());
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

/// ★★ **Stage 0c — the single-arm, single-purpose mode `cachegrind` measures.**
///
/// Selected by `TERM_OPS_ARM`; when it is set the process runs **one arm over
/// `TERM_OPS_PASSES` passes of the production-weighted mix and exits**, so a
/// deterministic instruction-and-data-reference count describes ONE mechanism
/// instead of superimposing every leg of the benchmark.
///
/// ```text
///   TERM_OPS_ARM=fixture valgrind --tool=cachegrind --cache-sim=yes …   the control
///   TERM_OPS_ARM=derived valgrind --tool=cachegrind --cache-sim=yes …   the oracle
///   TERM_OPS_ARM=driven  valgrind --tool=cachegrind --cache-sim=yes …   drive_with
/// ```
///
/// ★ Why a `fixture` arm exists: the process's own start-up, the workload
/// construction and the teardown all write memory, and none of that is the
/// subject. `fixture` performs **everything except the clone** — build the mix,
/// touch it, dismantle it — so
/// `D wr(arm) − D wr(fixture)` isolates the clone (plus the release of the
/// clones, which is byte-identical between the two arms because the two arms
/// produce byte-identical trees, proven by
/// `models/tests/clone_equivalence_corpus.rs`). The **difference of the two
/// differences** is therefore exactly the mechanism delta, with the shared terms
/// cancelling twice.
///
/// ⚠ **Not PEBS.** `mem-stores` is an Intel PEBS event and does not exist on this
/// host's Zen 3 ISA (Threadripper PRO 5975WX); `cachegrind`'s `D wr` is
/// deterministic, unsampled, and unskewed, and it is exactly the currency the
/// "three extra 248-byte moves per node" hypothesis is denominated in.
fn cachegrind_arm(arm: &str) {
    let passes: usize = std::env::var("TERM_OPS_PASSES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let workload = weighted_workload();
    let nodes: usize = workload.iter().map(count_par_nodes).sum();
    let mut sink: Vec<Par> = Vec::with_capacity(workload.len());
    let mut acc = 0usize;
    for _ in 0..passes {
        match arm {
            // The control: no clone at all, but the same walk over the same
            // fixture and the same drain of the same sink.
            "fixture" => {
                for value in &workload {
                    acc += black_box(value).exprs.len();
                }
            }
            "derived" => {
                for value in &workload {
                    sink.push(black_box(oracle_clone_par(black_box(value))));
                }
            }
            "driven" => {
                for value in &workload {
                    sink.push(black_box(black_box(value).clone()));
                }
            }
            other => panic!(
                "term_ops_bench: unknown TERM_OPS_ARM `{other}` — expected \
                 `fixture`, `derived` or `driven`"
            ),
        }
        acc += sink.len();
        dismantle_all(sink.drain(..));
    }
    println!(
        "TERM_OPS_ARM={arm} passes={passes} nodes/pass={nodes} \
         nodes total={} sink acc={acc}",
        nodes * passes
    );
    dismantle_all(workload);
}

/// ★ **Stage 1f / the width pins.** Printed rather than argued: every byte
/// claim in the two prior reports rests on `size_of::<Par>()`, and it was
/// asserted nowhere.
fn widths() {
    println!("WIDTHS (bytes)");
    println!("  size_of::<Par>()                    {}", std::mem::size_of::<Par>());
    println!("  size_of::<Expr>()                   {}", std::mem::size_of::<Expr>());
    println!("  size_of::<Send>()                   {}", std::mem::size_of::<Send>());
    println!(
        "  size_of::<CloneNode>()              {}",
        std::mem::size_of::<CloneNode<'_>>()
    );
    println!(
        "  size_of::<CloneKont>()              {}",
        std::mem::size_of::<CloneKont<'_>>()
    );
    println!(
        "  size_of::<CloneVal>()               {}   ◀── ONE value-stack slot; every \
         push/pop MOVES this",
        std::mem::size_of::<CloneVal>()
    );
    println!(
        "  size_of::<Step<CloneTraversal>>()   {}   ◀── ONE work-stack slot; \
         multiplied by DEPTH",
        std::mem::size_of::<Step<'_, CloneTraversal>>()
    );
    println!(
        "  size_of::<Outcome<CloneVal, CloneNode>>() {}  ◀── a RETURN VALUE, \
         NOT on the Θ(depth) stack (Stage 1f)",
        std::mem::size_of::<Outcome<CloneVal, CloneNode<'_>>>()
    );
    println!();
}

/// Ordinary least squares of `t = a + b·n`, with the residual of each point.
///
/// Returned as `(a, b, residuals, rmse)`. Written out rather than pulled from a
/// crate because the whole model is two parameters and the benchmark must not
/// grow a dependency to print them.
fn ols(points: &[(f64, f64)]) -> (f64, f64, Vec<f64>, f64) {
    let m = points.len() as f64;
    let sn: f64 = points.iter().map(|(n, _)| *n).sum();
    let st: f64 = points.iter().map(|(_, t)| *t).sum();
    let snn: f64 = points.iter().map(|(n, _)| n * n).sum();
    let snt: f64 = points.iter().map(|(n, t)| n * t).sum();
    let denom = m * snn - sn * sn;
    let b = (m * snt - sn * st) / denom;
    let a = (st - b * sn) / m;
    let residuals: Vec<f64> = points.iter().map(|(n, t)| t - (a + b * n)).collect();
    let rmse = (residuals.iter().map(|r| r * r).sum::<f64>() / m).sqrt();
    (a, b, residuals, rmse)
}

/// ★★★ **Stage 0a, done so the fit is admissible: the CONSTANT-FOOTPRINT SWEEP.**
///
/// The per-depth legs sweep `n` while holding the *call count* fixed at 512, so
/// the workload's byte footprint grows with `n` — `512 × n × 248 B`, from 0.5 MB
/// at `n = 4` to 16.5 MB at `n = 130`. That crosses this CPU's L2 (1 MiB/core) and
/// eats into a 32 MiB L3 slice, and the effect is not subtle: **the DERIVED arm's
/// own per-node cost triples over that range**, 117 ns → 361 ns, with no
/// mechanism change whatsoever.
///
/// ⇒ A two-parameter fit `T(n) = A + B·n` over the per-depth legs is therefore
/// **not** measuring a fixed cost and a per-node cost; it is measuring those plus
/// a monotone cache-residency term, which is exactly the shape of a systematic
/// mid-range residual. ★ **That is the third component the plan asked to have
/// measured rather than fitted, and it is a property of the FIXTURE, not of
/// either mechanism.**
///
/// This leg removes it two ways at once:
///
/// 1. **Constant footprint.** The number of calls per pass is `TOTAL_NODES / n`,
///    so every point touches the same ≈3,072 `Par` nodes ≈ 762 kB of source (plus
///    an equal sink) — deliberately the depth-2 footprint, i.e. production's.
/// 2. **One uniform node shape.** [`spine`] gives exactly `n = k + 1` `Par`
///    nodes, every one of them a `Par` → one `Expr` → `EList` → one `ps`, so `B`
///    and `C` are per-node costs of *one* kind of node rather than a blend that
///    changes composition with `n`. (`datum(d)` mixes `Send`s, `GString`s and two
///    `Expr`s into the base and adds a different sibling at every level.)
///
/// The output is `A` (fixed per call), `C` (derived per node), `B` (driven per
/// node) **with the residual at every point**, so a reader can see whether the
/// two-parameter model actually fits before believing either coefficient.
fn constant_footprint_sweep() {
    /// Total `Par` nodes touched per pass, held constant across the sweep. Equal
    /// to the depth-2 leg's 512 × 6, so the footprint is production's.
    const TOTAL_NODES: usize = 3072;
    println!(
        "CONSTANT-FOOTPRINT SWEEP — {TOTAL_NODES} Par nodes/pass at every point \
         ({} kB source), uniform spine shape",
        TOTAL_NODES * std::mem::size_of::<Par>() / 1024
    );
    let mut derived_points: Vec<(f64, f64)> = Vec::with_capacity(10);
    let mut driven_points: Vec<(f64, f64)> = Vec::with_capacity(10);
    let mut gap_points: Vec<(f64, f64)> = Vec::with_capacity(10);
    for levels in [0usize, 1, 2, 3, 5, 7, 9, 13, 19, 29] {
        let n = levels + 1;
        let calls = TOTAL_NODES / n;
        let one: Vec<Par> = (0..calls).map(|_| spine(levels)).collect();
        let measured = count_par_nodes(&one[0]);
        assert_eq!(
            measured, n,
            "VACUOUS: spine({levels}) carries {measured} Par nodes, not {n} — the sweep's \
             independent variable would not be the node count"
        );
        println!("  n = {n:>3} node(s)/call, {calls:>4} calls/pass:");
        let arms = clone_arms(&one);
        arms.print();
        let paired = arms.pair(0, 1);
        let (dc, mc) = (
            paired.a.mean() / calls as f64,
            paired.b.mean() / calls as f64,
        );
        println!(
            "     per-call: derived {dc:>9.2} ns, driven {mc:>9.2} ns, gap {:+.2} ns ±{:.2}   \
             per-node: derived {:.3} ns, driven {:.3} ns",
            paired.diff_mean() / calls as f64,
            paired.diff_half_width() / calls as f64,
            dc / n as f64,
            mc / n as f64
        );
        derived_points.push((n as f64, dc));
        driven_points.push((n as f64, mc));
        gap_points.push((n as f64, paired.diff_mean() / calls as f64));
        dismantle_all(one);
    }
    let (a_d, c, res_d, rmse_d) = ols(&derived_points);
    let (a_m, b, res_m, rmse_m) = ols(&driven_points);
    let (a_g, slope_g, res_g, rmse_g) = ols(&gap_points);
    println!();
    println!("  ╔══ THE DECOMPOSITION — OLS on t = a + b·n at CONSTANT footprint");
    println!(
        "  ║ derived:  A_derived = {a_d:>8.2} ns/call   C = {c:>7.3} ns/node   RMSE {rmse_d:.2} ns"
    );
    println!(
        "  ║ driven:   A_driven  = {a_m:>8.2} ns/call   B = {b:>7.3} ns/node   RMSE {rmse_m:.2} ns"
    );
    println!(
        "  ║ gap:      A         = {a_g:>8.2} ns/call   B-C = {slope_g:>7.3} ns/node   \
         RMSE {rmse_g:.2} ns"
    );
    println!("  ║");
    println!("  ║   n   derived resid   driven resid   gap resid");
    for (i, (n, _)) in derived_points.iter().enumerate() {
        println!(
            "  ║ {n:>3.0}   {:>12.2}   {:>12.2}   {:>9.2}",
            res_d[i], res_m[i], res_g[i]
        );
    }
    println!("  ║");
    // ★★ A, MEASURED AT THE INTERCEPT rather than extrapolated from the whole
    // range. The OLS intercept over n ∈ [1, 30] absorbs any curvature in the
    // series, and the DERIVED arm genuinely curves (its per-node cost rises with
    // n, because its native recursion deepens and its node composition shifts
    // from all-leaf to all-wrapper). Two adjacent low points give A without that
    // contamination: with `t(n) = A + B·n` locally, `A = 2·t(1) − t(2)`.
    let a_direct_d = 2.0 * derived_points[0].1 - derived_points[1].1;
    let a_direct_m = 2.0 * driven_points[0].1 - driven_points[1].1;
    println!(
        "  ║ ★ A AT THE INTERCEPT (A = 2·t(1) − t(2), free of the fit's curvature):"
    );
    println!(
        "  ║     derived {a_direct_d:>8.2} ns/call     driven {a_direct_m:>8.2} ns/call     \
         gap {:+.2} ns/call",
        a_direct_m - a_direct_d
    );
    println!(
        "  ║   ⚠ Compare the OLS intercepts above ({a_d:.2} / {a_m:.2}): a disagreement \
         IS the curvature,"
    );
    println!(
        "  ║     and it is the DERIVED arm that curves — so an `A` read off the GAP fit is \
         an artifact of"
    );
    println!("  ║     straight-lining a curve, not a fixed cost of the driven mechanism.");
    println!("  ║");
    println!(
        "  ║ ★ A as a share of the depth-2 gap: the depth-2 datum has n = 6, so the \
         two-parameter model puts"
    );
    println!(
        "  ║   the fixed per-call term at {:.1}% of the predicted gap there ({:.1} of {:.1} ns).",
        100.0 * a_g / (a_g + slope_g * 6.0),
        a_g,
        a_g + slope_g * 6.0
    );
    println!("  ╚══");
    println!();
}

fn main() {
    if let Ok(arm) = std::env::var("TERM_OPS_ARM") {
        cachegrind_arm(&arm);
        return;
    }

    environment();
    widths();

    // -------------------------------------------------------------------
    // ★★ THE VERDICT: the production-weighted mix
    // -------------------------------------------------------------------
    let workload = weighted_workload();
    let mix_nodes: usize = workload.iter().map(count_par_nodes).sum();
    println!(
        "PRODUCTION-WEIGHTED MIX — {} datums/pass, {mix_nodes} Par nodes/pass \
         (95.43% at depth 2, nothing deeper than 6)",
        workload.len()
    );
    let arms = clone_arms(&workload);
    arms.print();
    let weighted = arms.pair(0, 1);
    report("weighted", &weighted);
    verdict("the production-weighted mix", &weighted);

    // -------------------------------------------------------------------
    // Per depth, so a regression at the dominant depth cannot hide behind a
    // win at the tail.
    // -------------------------------------------------------------------
    // -------------------------------------------------------------------
    // ★★ Stage 0b — the SINGLE-NODE leg. `n = 1`, so the fixed per-call cost
    // `A` is MEASURED at the intercept instead of extrapolated from a range
    // (`n ∈ [4, 130]`) that excludes it. This is the control neither prior
    // attempt at this gap ran.
    // -------------------------------------------------------------------
    println!("SINGLE-NODE LEG (n = 1 Par node per call — the intercept, MEASURED)");
    {
        let one = single_node_workload(512);
        let nodes = count_par_nodes(&one[0]);
        assert_eq!(
            nodes, 1,
            "VACUOUS: the single-node leg's datum carries {nodes} Par nodes, not 1 — the \
             intercept it is supposed to measure would be A + {nodes}·B instead of A + B"
        );
        println!("  n = 1 node/call, 512 calls/pass:");
        let arms = clone_arms(&one);
        arms.print();
        let paired = arms.pair(0, 1);
        report("n=1", &paired);
        println!(
            "     per-call: derived {:.2} ns, driven {:.2} ns, gap {:+.2} ns ±{:.2} \
             ◀── A + B, both arms",
            paired.a.mean() / 512.0,
            paired.b.mean() / 512.0,
            paired.diff_mean() / 512.0,
            paired.diff_half_width() / 512.0
        );
        dismantle_all(one);
    }
    println!();

    constant_footprint_sweep();

    println!("PER DEPTH (unweighted; depth 2 carries 95.43% of production)");
    for depth in [1usize, 2, 3, 4, 6, 16, 64] {
        let one = uniform_workload(depth, 512);
        let nodes = count_par_nodes(&one[0]);
        println!("  depth {depth} (n = {nodes} Par nodes/call):");
        let arms = clone_arms(&one);
        arms.print();
        let paired = arms.pair(0, 1);
        report(&format!("depth {depth}"), &paired);
        println!(
            "     per-call: derived {:.2} ns, driven {:.2} ns, gap {:+.2} ns ±{:.2}   \
             per-node: derived {:.3} ns, driven {:.3} ns",
            paired.a.mean() / 512.0,
            paired.b.mean() / 512.0,
            paired.diff_mean() / 512.0,
            paired.diff_half_width() / 512.0,
            paired.a.mean() / 512.0 / nodes as f64,
            paired.b.mean() / 512.0 / nodes as f64
        );
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
                let arms = clone_arms(&one);
                arms.print();
                let paired = arms.pair(0, 1);
                let speedup = report(&format!("depth {depth}"), &paired);
                dismantle_all(one);
                speedup
            })
            .expect("term_ops_bench: failed to spawn the tail thread");
        let speedup = handle.join().expect("term_ops_bench: the tail leg panicked");
        println!("     (reported only; {speedup:.3}× does not enter the verdict)");
    }
    println!();

    deep_leg();

    println!("  loadavg at exit  {}", loadavg());

    dismantle_all(workload);
}
