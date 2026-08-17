//! Causal differential for incremental AC term reservations.
//!
//! Both arms execute the same production spatial-matcher PDA. The
//! compile-time-only differential switch changes exactly one mechanism:
//! `control_final_assignment_scan` reconstructs the term-consumed Bag by
//! scanning all final assignments, while `treatment_committed_bag_delta`
//! maintains normalized term occupancy only when an augmenting root commits.
//! Fixture cloning and output validation remain outside the timed interval.

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use models::rhoapi::{Par, Send};
use models::rust::utils::{new_freevar_expr, new_gint_par};
use rholang::rust::interpreter::matcher::spatial_matcher::{
    set_term_reservation_delta_enabled, SpatialMatcherContext,
};

fn send(index: usize) -> Send {
    let index = i64::try_from(index).expect("fixture index fits i64");
    Send {
        chan: Some(new_gint_par(index, Vec::new(), false)),
        data: vec![new_gint_par(
            index.wrapping_mul(17).wrapping_add(3),
            Vec::new(),
            false,
        )],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    }
}

struct Fixture {
    target: Par,
    pattern: Par,
    expected_remainder: Par,
}

impl Fixture {
    fn new(cardinality: usize) -> Self {
        let sends = (0..cardinality).map(send).collect::<Vec<_>>();
        let selected = sends
            .last()
            .expect("every benchmark cardinality is positive")
            .clone();
        let target = models::par_from_default! {
            sends: sends.clone(),
            ..Default::default()
        };
        let pattern = models::par_from_default! {
            sends: vec![selected],
            exprs: vec![new_freevar_expr(0)],
            connective_used: true,
            ..Default::default()
        };
        let expected_remainder = models::par_from_default! {
            sends: sends[..cardinality - 1].to_vec(),
            ..Default::default()
        };
        Self {
            target,
            pattern,
            expected_remainder,
        }
    }

    fn verify(&self, delta_enabled: bool) {
        set_term_reservation_delta_enabled(delta_enabled);
        let mut matcher = SpatialMatcherContext::new();
        assert!(
            matcher
                .spatial_match_result(self.target.clone(), self.pattern.clone())
                .is_some(),
            "the exact selected Send and free remainder must match"
        );
        assert_eq!(matcher.free_map.get(&0), Some(&self.expected_remainder));
    }

    fn run(target: Par, pattern: Par) {
        let mut matcher = SpatialMatcherContext::new();
        let matched = matcher.spatial_match_result(target, pattern).is_some();
        assert!(matched, "the benchmark fixture must match");
        black_box(matcher.free_map);
    }
}

fn bench_remainder(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("spatial_matcher_ac_remainder");
    for cardinality in [16usize, 256, 4_096] {
        let fixture = Fixture::new(cardinality);
        fixture.verify(false);
        fixture.verify(true);
        group.throughput(Throughput::Elements(cardinality as u64));

        for (arm, delta_enabled) in [
            ("control_final_assignment_scan", false),
            ("treatment_committed_bag_delta", true),
        ] {
            set_term_reservation_delta_enabled(delta_enabled);
            group.bench_function(BenchmarkId::new(arm, cardinality), |bencher| {
                bencher.iter_batched(
                    || (fixture.target.clone(), fixture.pattern.clone()),
                    |(target, pattern)| Fixture::run(target, pattern),
                    BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(82)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5));
    targets = bench_remainder
}
criterion_main!(benches);
