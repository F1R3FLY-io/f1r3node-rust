//! Preregistered experiment 258: prepared exact Par-component membership.
//!
//! Control: dynamic `ParBag::contains`, including canonical component-key
//! encoding. Treatment: `ParBag::contains_key` over typed keys compiled once
//! with the RSpace-style matcher plan outside the timed region. Every retained
//! repetition times both arms in alternating order over the same deterministic
//! 50/50 present/absent query batch.
//!
//! Experiment 257's legacy-Vec comparison remains reproducible at its pinned
//! `8f68bdb0c` git ref and in the committed raw measurement ledger.

use std::hint::black_box;
use std::time::Instant;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::par_bag::{CanonicalParBagKey, ParBag};

const CARDINALITIES: [usize; 4] = [1, 16, 256, 4_096];
const QUERY_PAIRS: usize = 32;
const QUERIES_PER_BATCH: usize = QUERY_PAIRS * 2;
const WARMUP: usize = 10;
// Experiment 258 requires 30; retain 82 for cell-wise paired 99% intervals and
// direct comparability with experiment 257.
const REPS: usize = 82;
const Z_001: f64 = 2.576;

#[derive(Clone, Copy)]
enum Topology {
    Shared,
    Diverse,
}

impl Topology {
    fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Diverse => "diverse",
        }
    }

    fn value(self, ordinal: usize) -> Expr {
        let text = match self {
            // Constant-width ordinal exposes a long shared score/protobuf
            // payload prefix to PathMap's prefix compression.
            Self::Shared => format!("rho:parbag:shared:component:{ordinal:016x}"),
            // Multiplicative scrambling makes the first payload bytes vary;
            // this is the low-prefix-reuse counter-topology.
            Self::Diverse => {
                let mixed = (ordinal as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
                format!("{mixed:016x}:diverse:{ordinal:016x}")
            }
        };
        Expr {
            expr_instance: Some(ExprInstance::GString(text)),
        }
    }
}

struct Fixture {
    label: String,
    legacy: Vec<Expr>,
    pathmap: ParBag,
    queries: Vec<Expr>,
    prepared_queries: Vec<CanonicalParBagKey<Expr>>,
}

impl Fixture {
    fn new(topology: Topology, cardinality: usize) -> Self {
        let legacy = (0..cardinality)
            .map(|ordinal| topology.value(ordinal))
            .collect::<Vec<_>>();
        let mut par = Par::default();
        par.exprs.clone_from(&legacy);
        let pathmap = ParBag::import_legacy(&par).expect("fixture multiplicities fit u64");

        let mut queries = Vec::with_capacity(QUERIES_PER_BATCH);
        for query in 0..QUERY_PAIRS {
            // Present queries land at the end of the vector whenever the
            // cardinality permits, making the linear control pay its actual
            // worst-case search rather than accidentally benchmarking index 0.
            let present = cardinality - 1 - query.min(cardinality - 1);
            queries.push(topology.value(present));
            queries.push(topology.value(cardinality + query));
        }
        let prepared_queries = queries.iter().map(CanonicalParBagKey::compile).collect();

        let fixture = Self {
            label: format!("{}/{}", topology.label(), cardinality),
            legacy,
            pathmap,
            queries,
            prepared_queries,
        };
        let legacy = fixture.legacy();
        let dynamic = fixture.dynamic();
        let prepared = fixture.prepared();
        assert_eq!(legacy, QUERY_PAIRS, "legacy fixture checksum");
        assert_eq!(dynamic, legacy, "dynamic semantic preflight");
        assert_eq!(prepared, legacy, "prepared semantic preflight");
        fixture
    }

    fn legacy(&self) -> usize {
        self.queries
            .iter()
            .filter(|query| {
                self.legacy
                    .iter()
                    .any(|candidate| candidate == black_box(*query))
            })
            .count()
    }

    fn dynamic(&self) -> usize {
        self.queries
            .iter()
            .filter(|query| self.pathmap.contains(black_box(*query)))
            .count()
    }

    fn prepared(&self) -> usize {
        self.prepared_queries
            .iter()
            .filter(|query| self.pathmap.contains_key(black_box(*query)))
            .count()
    }
}

fn mean(samples: &[f64]) -> f64 { samples.iter().sum::<f64>() / samples.len() as f64 }

fn paired_interval(control: &[f64], treatment: &[f64]) -> (f64, f64) {
    let differences = control
        .iter()
        .zip(treatment)
        .map(|(control, treatment)| treatment - control)
        .collect::<Vec<_>>();
    let average = mean(&differences);
    let variance = differences
        .iter()
        .map(|difference| (difference - average).powi(2))
        .sum::<f64>()
        / (differences.len() as f64 - 1.0);
    let half_width = Z_001 * (variance / differences.len() as f64).sqrt();
    (average, half_width)
}

fn median_ratio(control: &[f64], treatment: &[f64]) -> f64 {
    let mut ratios = control
        .iter()
        .zip(treatment)
        .map(|(control, treatment)| treatment / control)
        .collect::<Vec<_>>();
    ratios.sort_by(|left, right| left.total_cmp(right));
    (ratios[ratios.len() / 2 - 1] + ratios[ratios.len() / 2]) / 2.0
}

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| "unavailable".to_string())
}

fn run(fixture: &Fixture) {
    let mut samples = [Vec::with_capacity(REPS), Vec::with_capacity(REPS)];
    for repetition in 0..(WARMUP + REPS) {
        let mut elapsed = [0f64; 2];
        for slot in 0..2 {
            let arm = (repetition + slot) % 2;
            let started = Instant::now();
            let checksum = if arm == 0 {
                fixture.dynamic()
            } else {
                fixture.prepared()
            };
            elapsed[arm] = started.elapsed().as_nanos() as f64;
            assert_eq!(black_box(checksum), QUERY_PAIRS);
        }
        if repetition >= WARMUP {
            let retained = repetition - WARMUP;
            for arm in 0..2 {
                samples[arm].push(elapsed[arm]);
                println!(
                    "OBS\tcell={}\tarm={}\trep={}\tns={:.0}\tchecksum={}",
                    fixture.label,
                    if arm == 0 { "control" } else { "treatment" },
                    retained,
                    elapsed[arm],
                    QUERY_PAIRS
                );
            }
        }
    }

    let control_mean = mean(&samples[0]);
    let treatment_mean = mean(&samples[1]);
    let (difference, half_width) = paired_interval(&samples[0], &samples[1]);
    println!(
        "SUMMARY\tcell={}\tcontrol_mean_ns={control_mean:.3}\ttreatment_mean_ns={treatment_mean:.3}\ttreatment_over_control={:.6}\tmedian_paired_ratio={:.6}\tpaired_difference_ns={difference:.3}\tpaired_99_half_width_ns={half_width:.3}",
        fixture.label,
        treatment_mean / control_mean,
        median_ratio(&samples[0], &samples[1]),
    );
}

fn main() {
    println!(
        "PARBAG_EXPERIMENT\tid=258\treps={REPS}\twarmup={WARMUP}\tqueries_per_batch={QUERIES_PER_BATCH}\tpresent={QUERY_PAIRS}\tabsent={QUERY_PAIRS}"
    );
    println!("LOADAVG\t{}", loadavg());
    for topology in [Topology::Shared, Topology::Diverse] {
        for cardinality in CARDINALITIES {
            run(&Fixture::new(topology, cardinality));
        }
    }
}
