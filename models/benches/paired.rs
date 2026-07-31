//! # `paired` — the one paired-measurement harness this workspace's benchmarks share
//!
//! ## ⚠⚠ Why this module exists: an instrument defect that shipped TWICE
//!
//! `models/benches/term_ops_bench.rs` and `models/benches/bincode_encoder_bench.rs`
//! each carried a private `measure(label, workload, arm) -> Sample` that ran
//! **every** repetition of one arm and was then called again for the next. Both
//! files' module headers said the opposite, in nearly the same words:
//!
//! > *"**Interleaved A/B.** One repetition measures A then B, and the loop is
//! > repeated `REPS` times. Any drift in clock, thermals or cache state moves
//! > both arms together."*
//!
//! **Neither implementation did that.** The arms were measured in *different time
//! windows* on a workstation that routinely runs six concurrent build jobs, and
//! the consequence is not theoretical. Same binary, consecutive runs, `taskset`
//! pinned:
//!
//! ```text
//!   term_ops_bench   weighted mix   1.0748×  then  0.9461×      (PASS then FAIL)
//!   bincode_encoder_bench weighted mix  1.261×  1.471×  1.154×      (27% peak-to-peak)
//! ```
//!
//! ★ The second row is the reason this is a *shared* module and not two repairs.
//! `term_ops_bench` was fixed first; `bincode_encoder_bench` had the identical defect
//! and the identical false claim, and a fix applied twice is a fix that drifts
//! apart. There is now **one** implementation, and a benchmark that wants an
//! A/B verdict has to come here to get one.
//!
//! ## The method, and the three things the old one lacked
//!
//! 1. **Genuine interleaving.** One repetition times *every* arm, back to back,
//!    so a load excursion or a clock/thermal drift lands inside one repetition
//!    and cancels in the difference instead of landing on whichever arm was
//!    running.
//! 2. **Order rotation.** Within a repetition the *first* arm pays the cold walk
//!    over the workload and the later ones find it warm. Rotating the starting
//!    arm by `rep % N` spreads that bias evenly over all `N` arms instead of
//!    accumulating it on one. ★ For `N = 2` this is order alternation; the
//!    generalization is what lets `bincode_encoder_bench`'s **three** arms use the
//!    same code.
//! 3. **Paired statistics.** The t-test is on the per-repetition *difference* —
//!    one series, `n − 1` degrees of freedom — which is both the correct test for
//!    a paired design and far more powerful here: the between-repetition variance
//!    that dominates Welch's denominator on a loaded host is common-mode and
//!    differences out. The **median per-repetition ratio** is reported alongside
//!    as the load-robust point estimate, and a disagreement between it and the
//!    ratio-of-means is reported as *load contamination* rather than averaged
//!    away.
//!
//! ## ⚠ What this module deliberately does NOT claim to fix
//!
//! Pairing removes the *between-arm* drift. It does not make a wall-clock ratio
//! on this host precise enough to resolve a few per cent: the residual
//! within-repetition contention is still there, and three independent agents have
//! now demonstrated it (this module's own spreads; an sd of 10–14% against a 2%
//! criterion; and an **invariant control** — a benchmark touching no production
//! code — moving **+55%** with criterion reporting `p < 0.05`).
//!
//! ⇒ ★★ **For a verdict on a small delta, use a deterministic instrument**:
//! `valgrind --tool=cachegrind --cache-sim=yes` (`Ir` / `Dw`, no sampling, no
//! skid, byte-reproducible) or `perf stat -e instructions,ls_dispatch.store_dispatch`.
//! Wall clock is corroboration. `term_ops_bench`'s `TERM_OPS_ARM` mode exists to
//! be that instrument's subject, and the two agreed to four significant figures
//! (`Ir` ratio 1.1740 cachegrind vs 1.1742 hardware) on a question wall clock
//! could not call in either direction.

#![allow(dead_code)] // Each bench uses a subset; the module is shared.

use std::time::Instant;

/// Repetitions per arm, retained.
pub const REPS: usize = 60;

/// Discarded leading repetitions: thread-local pools, caches and reused buffers
/// reach their steady state within a handful of passes, and including the cold
/// ones would measure warm-up rather than throughput.
pub const WARMUP: usize = 10;

/// The α = 0.01 two-sided normal quantile. `REPS` is large enough that `t` and
/// `z` agree to three decimals.
const Z_001: f64 = 2.576;

/// One arm's retained timings, in nanoseconds per pass.
pub struct Sample {
    pub times_ns: Vec<f64>,
}

impl Sample {
    pub fn mean(&self) -> f64 {
        self.times_ns.iter().sum::<f64>() / self.times_ns.len() as f64
    }
    pub fn variance(&self) -> f64 {
        let m = self.mean();
        self.times_ns.iter().map(|t| (t - m).powi(2)).sum::<f64>()
            / (self.times_ns.len() as f64 - 1.0)
    }
    pub fn sd(&self) -> f64 {
        self.variance().sqrt()
    }
    /// Half-width of the α = 0.01 two-sided interval around the mean.
    pub fn half_width(&self) -> f64 {
        Z_001 * self.sd() / (self.times_ns.len() as f64).sqrt()
    }
}

/// Welch's `t`, its Welch–Satterthwaite degrees of freedom, and significance at
/// α = 0.01.
///
/// ⚠ Retained only for continuity with the figures the old instrument produced,
/// so a reader can see how much weaker the unpaired statistic is on the same
/// data. [`Pair::paired_t`] is the statistic to use.
pub fn welch(a: &Sample, b: &Sample) -> (f64, f64, bool) {
    let (na, nb) = (a.times_ns.len() as f64, b.times_ns.len() as f64);
    let (va, vb) = (a.variance(), b.variance());
    let se = (va / na + vb / nb).sqrt();
    if se == 0.0 {
        return (0.0, na + nb - 2.0, false);
    }
    let t = (a.mean() - b.mean()) / se;
    let df = (va / na + vb / nb).powi(2)
        / ((va / na).powi(2) / (na - 1.0) + (vb / nb).powi(2) / (nb - 1.0));
    (t, df, t.abs() > Z_001)
}

/// Two arms measured in the SAME repetitions, and the per-repetition difference
/// between them.
///
/// ★ Constructed by [`Arms::pair`]. Because every arm is timed inside every
/// repetition, *any* two arms of an [`Arms`] are genuinely paired — the pairing
/// is a property of how the data was collected, not of which two are compared.
pub struct Pair<'s> {
    pub a: &'s Sample,
    pub b: &'s Sample,
    /// `b_i − a_i`, one per retained repetition.
    pub diffs: Vec<f64>,
    /// `b_i / a_i`, one per retained repetition.
    pub ratios: Vec<f64>,
}

impl Pair<'_> {
    pub fn diff_mean(&self) -> f64 {
        self.diffs.iter().sum::<f64>() / self.diffs.len() as f64
    }
    pub fn diff_sd(&self) -> f64 {
        let m = self.diff_mean();
        (self.diffs.iter().map(|d| (d - m).powi(2)).sum::<f64>()
            / (self.diffs.len() as f64 - 1.0))
            .sqrt()
    }
    /// Half-width of the paired α = 0.01 interval on the mean difference.
    pub fn diff_half_width(&self) -> f64 {
        Z_001 * self.diff_sd() / (self.diffs.len() as f64).sqrt()
    }
    /// Paired `t`, and whether the α = 0.01 interval on the mean difference
    /// **excludes zero**.
    pub fn paired_t(&self) -> (f64, bool) {
        let sd = self.diff_sd();
        if sd == 0.0 {
            return (0.0, false);
        }
        let t = self.diff_mean() / (sd / (self.diffs.len() as f64).sqrt());
        (t, t.abs() > Z_001)
    }
    /// The median of the per-repetition ratios `b_i / a_i` — the load-robust
    /// point estimate.
    pub fn median_ratio(&self) -> f64 {
        let mut r = self.ratios.clone();
        r.sort_by(|x, y| x.partial_cmp(y).expect("paired: NaN ratio"));
        let n = r.len();
        if n % 2 == 1 {
            r[n / 2]
        } else {
            0.5 * (r[n / 2 - 1] + r[n / 2])
        }
    }
    /// `a / b`, i.e. how many times `b`'s throughput exceeds `a`'s, from the
    /// median per-repetition ratio.
    pub fn median_speedup(&self) -> f64 {
        1.0 / self.median_ratio()
    }
}

/// `N` arms measured in the same repetitions.
pub struct Arms<const N: usize> {
    pub labels: [&'static str; N],
    pub samples: [Sample; N],
}

impl<const N: usize> Arms<N> {
    /// The paired comparison of arms `i` and `j`.
    pub fn pair(&self, i: usize, j: usize) -> Pair<'_> {
        let (a, b) = (&self.samples[i], &self.samples[j]);
        let diffs = a
            .times_ns
            .iter()
            .zip(&b.times_ns)
            .map(|(x, y)| y - x)
            .collect();
        let ratios = a
            .times_ns
            .iter()
            .zip(&b.times_ns)
            .map(|(x, y)| y / x)
            .collect();
        Pair { a, b, diffs, ratios }
    }

    /// Print each arm's mean, sd and interval.
    pub fn print(&self) {
        for (label, s) in self.labels.iter().zip(&self.samples) {
            println!(
                "  {label:16} mean {:>12.1} ns   sd {:>10.1} ns   ({:.2}%)   ±{:.1} ns @ α=0.01",
                s.mean(),
                s.sd(),
                100.0 * s.sd() / s.mean(),
                s.half_width()
            );
        }
    }

    /// ★ Report one paired comparison, both ways, and say so when they disagree.
    ///
    /// Returns the **median-of-repetition** speedup, which is the figure to
    /// quote: when it and the ratio-of-means disagree the run is
    /// load-contaminated, and that disagreement is printed rather than hidden.
    pub fn report(&self, name: &str, i: usize, j: usize) -> f64 {
        let p = self.pair(i, j);
        let (wt, df, w_sig) = welch(p.a, p.b);
        let (pt, p_sig) = p.paired_t();
        let of_means = p.a.mean() / p.b.mean();
        let median = p.median_speedup();
        let delta = 100.0 * (p.b.mean() - p.a.mean()) / p.a.mean();
        println!(
            "  ── {name}: {of_means:.3}× of-means ({delta:+.2}%), {median:.3}× median-of-rep \
             | PAIRED Δ {:+.1} ±{:.1} ns, t = {pt:.2}, excludes 0: {p_sig} \
             | Welch t = {wt:.2}, df = {df:.1}, sig: {w_sig}",
            p.diff_mean(),
            p.diff_half_width()
        );
        if !p_sig {
            println!(
                "     (the paired difference does not exclude 0 — the arms are indistinguishable)"
            );
        }
        if (of_means - median).abs() > 0.02 {
            println!(
                "     ⚠ of-means and median-of-rep disagree by {:.3}× — the run is \
                 load-contaminated; the MEDIAN is the estimate to use",
                (of_means - median).abs()
            );
        }
        median
    }
}

/// ★★★ **The measurement.** Times all `N` arms inside every repetition,
/// rotating which one goes first, and releases each pass's products OUTSIDE the
/// timer.
///
/// * `workload` is walked once per pass by each arm.
/// * `arms[k]` maps one workload item to one product. The product is pushed to a
///   preallocated sink so the arm cannot be optimized away, and so an arm that
///   returns an owned term has somewhere to put it.
/// * `release` drains the sink **after the timer stops**. ⚠ This is load-bearing
///   for `term_ops_bench`: `drop_in_place::<Par>` is itself Θ(depth) (gate
///   subject `par_drop`), so timing the release would add the same large term to
///   every arm and dilute the difference the experiment exists to measure.
pub fn measure_arms<T, R, const N: usize>(
    labels: [&'static str; N],
    workload: &[T],
    arms: &mut [&mut dyn FnMut(&T) -> R; N],
    release: &mut dyn FnMut(std::vec::Drain<'_, R>),
) -> Arms<N> {
    let mut times: [Vec<f64>; N] = std::array::from_fn(|_| Vec::with_capacity(REPS));
    // Preallocated once and drained (not dropped) each pass, so no pass after the
    // first pays a reallocation.
    let mut sink: Vec<R> = Vec::with_capacity(workload.len());
    for rep in 0..(REPS + WARMUP) {
        // ★ ROTATION: arm `(rep + s) % N` runs in slot `s`, so over `N`
        // consecutive repetitions every arm occupies every slot exactly once and
        // the cold-first-walk bias is shared instead of accumulated.
        let mut pass = [0.0f64; N];
        for slot in 0..N {
            let k = (rep + slot) % N;
            let start = Instant::now();
            for item in workload {
                sink.push(std::hint::black_box(arms[k](std::hint::black_box(item))));
            }
            let elapsed = start.elapsed();
            // ⚠ UNTIMED.
            release(sink.drain(..));
            pass[k] = elapsed.as_nanos() as f64;
        }
        if rep >= WARMUP {
            for k in 0..N {
                times[k].push(pass[k]);
            }
        }
    }
    let mut it = times.into_iter();
    Arms {
        labels,
        samples: std::array::from_fn(|_| Sample {
            times_ns: it.next().expect("paired: N samples for N arms"),
        }),
    }
}

/// ⚠ Read `/proc/loadavg` — the one number without which no wall-time figure
/// from this harness means anything on a shared workstation.
pub fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unavailable".into())
}
