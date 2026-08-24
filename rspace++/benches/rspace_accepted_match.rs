//! Criterion latency experiment for atomic accepted-match publication.
//!
//! Fixture construction and post-state auditing occur outside the measured
//! routine. The frozen control is the pre-transaction call sequence compiled
//! only by the experiment feature; the treatment invokes production code.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use rspace_plus_plus::rspace::hot_store::accepted_match_transition_experiment::{
    AcceptedMatchTransitionExperiment, ExperimentArm, ExperimentCase,
};

fn accepted_match(c: &mut Criterion) {
    for case in ExperimentCase::ALL {
        black_box(AcceptedMatchTransitionExperiment::audit(case));
        let mut group = c.benchmark_group(format!("accepted_match/{}", case.label()));
        for arm in [ExperimentArm::FrozenSplit, ExperimentArm::Atomic] {
            if !case.supports(arm) {
                continue;
            }
            group.bench_with_input(BenchmarkId::from_parameter(arm.label()), &arm, |b, &arm| {
                b.iter_batched(
                    || AcceptedMatchTransitionExperiment::new(case),
                    |mut fixture| black_box(fixture.execute(black_box(arm))),
                    BatchSize::SmallInput,
                );
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(51)
        .confidence_level(0.99)
        .significance_level(0.01)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10));
    targets = accepted_match
}
criterion_main!(benches);
