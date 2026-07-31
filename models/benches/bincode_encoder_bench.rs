//! # The encoder benchmark — weighted by a MEASURED distribution
//!
//! ## ★ Why the weighting is the whole experiment
//!
//! *"2× faster at depth 6,000 and 20% slower at depth 3 is a NET LOSS."* A
//! benchmark that samples depth uniformly, or that leans on the tail because
//! the tail is where the old encoder failed, would report a win the node never
//! collects.
//!
//! So the distribution is **measured, not assumed**. `ListParWithRandom::
//! stable_hash_bytes` — the datum leg of `hash_produce`, which runs once per
//! produce — was instrumented and five interpreter suites were run
//! (`reduce_spec`, `interpreter_spec`, `demo_verification`,
//! `where_examples_compile`, `rholang_numeric_eval_spec`), giving **1,773
//! datums**:
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
//! **Nothing deeper than 6 was observed.** The production workload is
//! overwhelmingly shallow and small, which is precisely the regime where a
//! per-node dispatch cost would show up and where a "removes a whole
//! traversal" claim has to earn its keep on a handful of nodes.
//!
//! ## Method
//!
//! * **★ GENUINELY interleaved, PAIRED, and rotated — via the SHARED
//!   [`paired`] harness.** ⚠ This bullet used to claim *"one repetition measures
//!   A then B, and the loop is repeated `REPS` times. Any drift in clock,
//!   thermals or cache state moves both arms together"* — and the private
//!   `measure()` it described ran **every** repetition of one arm before
//!   starting the next, so the three arms were measured in three different time
//!   windows. On this host, three consecutive runs of the identical binary at
//!   loadavg 16.7 reported the weighted owned-`Vec` ratio as **1.261×, 1.471×
//!   and 1.154×** — 27% peak-to-peak. The `1.194 ± 0.005×` that
//!   `docs/design/stack-safety/stack-safety-report-2026-07-29.md` §5.4.1 records
//!   from `n = 3` runs of this instrument was three draws that happened to land
//!   close together, not a precision. ★ `term_ops_bench` had the identical
//!   defect and the identical false claim; both now share ONE implementation, in
//!   `models/benches/paired.rs`, because a fix applied twice is a fix that
//!   drifts apart.
//! * **Welch's t-test.** Two numbers are not a throughput claim. Reported with
//!   the Welch–Satterthwaite degrees of freedom and the effect size, so the
//!   verdict is legible.
//! * **CPU affinity and frequency** are the caller's job:
//!   `taskset -c <cpu> cargo bench …` with the governor at `performance`. The
//!   harness prints what it sees so a run at a scaling frequency is visible in
//!   the output rather than silently averaged in.
//!
//! ## What is compared
//!
//! | arm | produces | comparable to |
//! |---|---|---|
//! | `derived` | an owned `Vec<u8>` | today's `bincode::serialize` |
//! | `machine` | an owned `Vec<u8>` | the like-for-like replacement |
//! | `machine_reused` | a borrowed `&[u8]` | the additional win where the caller does not need ownership |
//!
//! The like-for-like verdict is `derived` vs `machine`. `machine_reused` is
//! reported separately because it changes the *contract*, and folding a
//! contract change into a throughput number would overstate the result.

use std::time::Duration;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, ETuple, Expr, KeyValuePair, ListParWithRandom, Par, Send};
// ★ The SHARED paired-measurement harness. One implementation for this bench and
// `term_ops_bench`, because both carried the same defect and a fix applied twice
// is a fix that drifts apart. See `paired.rs`'s module docs.
#[path = "paired.rs"]
mod paired;
use paired::{loadavg, measure_arms};

use models::rust::rholang::bincode_encoder::{encode, with_encoded};

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

/// How many copies of each depth appear in one workload pass, scaled so the
/// mix reproduces the measured shares and one pass is long enough to dominate
/// timer granularity.
const WORKLOAD_SCALE: f64 = 2000.0;

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
/// averages (3.20 nodes, 661 B per datum) rather than being a bare spine — a
/// spine would understate the per-node cost the shallow case is meant to expose.
fn datum(depth: usize) -> ListParWithRandom {
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
    ListParWithRandom {
        pars: vec![body],
        // The per-produce random state, which every real datum carries.
        random_state: vec![0xa5; 32],
    }
}

/// A map-heavy datum: the `EMap`/`KeyValuePair` shape, which exercises the
/// counted-repeat path over pairs rather than over `Par`s.
fn map_datum(entries: usize) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(models::rhoapi::EMap {
                    kvs: (0..entries)
                        .map(|i| KeyValuePair {
                            key: Some(gstr(&format!("k{i}"))),
                            value: Some(gint(i as i64)),
                        })
                        .collect(),
                    locally_free: vec![],
                    connective_used: false,
                    remainder: None,
                })),
            }],
            ..Default::default()
        }],
        random_state: vec![0x5a; 32],
    }
}

/// The production-weighted workload: one pass over the measured mix.
fn weighted_workload() -> Vec<ListParWithRandom> {
    let mut out = Vec::new();
    for (depth, share) in MEASURED {
        let copies = (share * WORKLOAD_SCALE).round().max(1.0) as usize;
        for _ in 0..copies {
            out.push(datum(*depth));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------



/// The three arms, measured in the same repetitions and rotated: the derived
/// `bincode::serialize` oracle, the machine returning an owned `Vec`, and the
/// machine writing into a reused thread-local buffer.
///
/// ⚠ All three produce a `usize` (the byte length), so the sink is trivial and
/// `release` only has to consume it — unlike `term_ops_bench`, where the products
/// are owned terms and the untimed release is load-bearing.
fn three_arms(workload: &[ListParWithRandom]) -> paired::Arms<3> {
    let mut a = |v: &ListParWithRandom| bincode::serialize(v).expect("oracle").len();
    let mut b = |v: &ListParWithRandom| encode(v).len();
    let mut c = |v: &ListParWithRandom| with_encoded(v, <[u8]>::len);
    measure_arms(
        ["derived", "machine", "machine_reused"],
        workload,
        &mut [&mut a, &mut b, &mut c],
        &mut |drain| {
            for n in drain {
                std::hint::black_box(n);
            }
        },
    )
}

/// The two arms the shapes-and-tail cells use: the derived oracle and the machine
/// returning an owned `Vec`.
fn two_arms(workload: &[ListParWithRandom]) -> paired::Arms<2> {
    let mut a = |v: &ListParWithRandom| bincode::serialize(v).expect("oracle").len();
    let mut b = |v: &ListParWithRandom| encode(v).len();
    measure_arms(
        ["derived", "machine"],
        workload,
        &mut [&mut a, &mut b],
        &mut |drain| {
            for n in drain {
                std::hint::black_box(n);
            }
        },
    )
}

fn environment() {
    println!("ENVIRONMENT");
    println!(
        "  loadavg    {}   ◀── every ns below is conditional on this",
        loadavg()
    );
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

fn main() {
    environment();

    // -------------------------------------------------------------------
    // THE VERDICT: the production-weighted mix
    // -------------------------------------------------------------------
    let workload = weighted_workload();
    let bytes: usize = workload.iter().map(|v| encode(v).len()).sum();
    println!(
        "PRODUCTION-WEIGHTED MIX — {} datums/pass, {} B/pass, {:.0} B/datum \
         (measured mean 661 B)",
        workload.len(),
        bytes,
        bytes as f64 / workload.len() as f64
    );
    let arms = three_arms(&workload);
    arms.print();
    arms.report("weighted, owned Vec", 0, 1);
    arms.report("weighted, reused buffer", 0, 2);
    println!();

    // -------------------------------------------------------------------
    // Per-depth, so the shape of the win is visible and a regression at the
    // dominant depth cannot hide behind a win at the tail.
    // -------------------------------------------------------------------
    println!("PER DEPTH (unweighted; depth 2 carries 95.43% of production)");
    for depth in [1usize, 2, 3, 4, 6, 16, 64] {
        let one = vec![datum(depth); 512];
        println!("  depth {depth}:");
        let arms = three_arms(&one);
        arms.print();
        arms.report(&format!("depth {depth}, owned"), 0, 1);
        arms.report(&format!("depth {depth}, reused"), 0, 2);
    }
    println!();

    // -------------------------------------------------------------------
    // Shapes the mix does not reach.
    // -------------------------------------------------------------------
    println!("OTHER SHAPES");
    for (label, one) in [
        ("map(64 entries)", vec![map_datum(64); 64]),
        ("map(1024 entries)", vec![map_datum(1024); 8]),
        (
            "wide(4096 siblings)",
            vec![
                ListParWithRandom {
                    pars: (0..4096).map(|i| gint(i)).collect(),
                    random_state: vec![1; 32],
                };
                4
            ],
        ),
    ] {
        println!("  {label}:");
        let arms = two_arms(&one);
        arms.print();
        arms.report(label, 0, 1);
    }

    // -------------------------------------------------------------------
    // The tail the old encoder could not reach at all. Reported, never
    // weighted: the measured distribution tops out at depth 6.
    // -------------------------------------------------------------------
    println!();
    println!("DEEP TAIL (never weighted — the measured distribution stops at depth 6)");
    for depth in [256usize, 1024] {
        let one = vec![datum(depth); 4];
        println!("  depth {depth}:");
        let arms = two_arms(&one);
        arms.print();
        arms.report(&format!("depth {depth}"), 0, 1);
        // Release without a Θ(depth) recursive `Drop` on this thread.
        std::mem::forget(one);
    }
    // The weighted workload's terms are shallow; dropping them is cheap.
    drop(workload);
    let _ = Duration::from_secs(0);
}
