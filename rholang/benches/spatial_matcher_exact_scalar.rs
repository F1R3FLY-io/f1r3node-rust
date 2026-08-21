//! Causal benchmark for validated byte-native scalar equality inside encoded
//! branching Connectives.
//!
//! Both arms execute the production spatial-matcher PDA. The benchmark-only
//! control changes only exact scalar children from direct canonical-byte
//! equality back to the complete protobuf decoder and resident matcher.
//! Fixture construction and CoW root cloning are excluded from timed samples.

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::matcher::benchmark_match_par_exact_scalar;

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn skewed_fixture(depth: usize, conjunction: bool) -> (Par, Par) {
    let mut construction = ParConstruction::new();
    let target = integer(1);
    let mut nested = construction
        .import_par(&target)
        .expect("import success terminal");
    let success = nested;
    let failure = construction
        .import_par(&integer(0))
        .expect("import exact scalar refusal");
    let shell = Connective {
        connective_instance: Some(if conjunction {
            ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        } else {
            ConnOrBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })
        }),
    };

    for _ in 0..depth {
        let children = if conjunction {
            [nested, success]
        } else {
            [failure, nested]
        };
        let component = construction
            .connective_component(&shell, &children)
            .expect("plan skewed branching Connective");
        nested = construction
            .singleton_component(component, Vec::new(), true)
            .expect("plan Connective carrier");
    }

    (target, construction.finish(nested).expect("finish fixture"))
}

fn benchmark(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("spatial_matcher_exact_scalar");
    for depth in [128usize, 512, 2_048, 4_096] {
        for (shape, conjunction) in [("and_equal", true), ("or_late_success", false)] {
            let (target, pattern) = skewed_fixture(depth, conjunction);
            for (arm, optimized) in [("control_decode", false), ("treatment_bytes", true)] {
                group.bench_with_input(
                    BenchmarkId::new(format!("{shape}/{arm}"), depth),
                    &optimized,
                    |bencher, optimized| {
                        bencher.iter_batched(
                            || (target.clone(), pattern.clone()),
                            |(target, pattern)| {
                                let matched =
                                    benchmark_match_par_exact_scalar(target, pattern, *optimized)
                                        .is_some();
                                assert!(matched);
                                black_box(matched)
                            },
                            BatchSize::PerIteration,
                        );
                    },
                );
            }
        }
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
