//! Scaling benchmark for the canonical-protobuf `If` matcher cursor.
//!
//! Fixture construction and CoW root cloning are outside each timed sample.
//! The timed body is the production spatial matcher, including its one-time
//! extraction of each authoritative top-level ParBag endpoint.

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use models::rhoapi::{If, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::utils::{new_freevar_par, new_gint_par};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn fixture(depth: usize) -> (Par, Par) {
    let mut construction = ParConstruction::new();
    let mut target = construction
        .import_par(&integer(7))
        .expect("import target terminal");
    let mut pattern = construction
        .import_par(&new_freevar_par(0, Vec::new()))
        .expect("import pattern terminal");
    let if_true = construction
        .import_par(&integer(12))
        .expect("import true branch");
    let if_false = construction
        .import_par(&integer(13))
        .expect("import false branch");
    let target_shell = If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        locally_free: Vec::new(),
        connective_used: false,
    };
    let pattern_shell = If {
        connective_used: true,
        ..target_shell.clone()
    };

    for _ in 0..depth {
        let component = construction
            .if_component(&target_shell, &[target, if_true, if_false])
            .expect("plan target If");
        target = construction
            .singleton_component(component, Vec::new(), false)
            .expect("plan target carrier");

        let component = construction
            .if_component(&pattern_shell, &[pattern, if_true, if_false])
            .expect("plan pattern If");
        pattern = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan pattern carrier");
    }

    (
        construction.finish(target).expect("finish target"),
        construction.finish(pattern).expect("finish pattern"),
    )
}

fn benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("spatial_matcher_if_cursor");
    for depth in [128usize, 512, 2_048, 4_096] {
        let (target, pattern) = fixture(depth);
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |bencher, _| {
            bencher.iter_batched(
                || (target.clone(), pattern.clone()),
                |(target, pattern)| {
                    let mut context = SpatialMatcherContext::new();
                    let matched = context.spatial_match(target, pattern).is_some();
                    assert!(matched);
                    black_box(matched)
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .noise_threshold(0.02)
        .with_plots()
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = benchmark
}
criterion_main!(benches);
