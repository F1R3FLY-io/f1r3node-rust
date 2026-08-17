//! Causal benchmark for PathMap algebra over AC connective candidate domains.
//!
//! Both arms execute the same production list PDA. The benchmark-only control
//! feature changes only pure-connective candidate compilation from the native
//! PathMap union/intersection plan back to the predecessor `All` domain.
//! Fixture cloning is excluded from each timed sample.

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkGroup, BenchmarkId, Criterion,
};
use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par, Send};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_gint_par, new_send};
use rholang::rust::interpreter::matcher::{
    benchmark_match_par_list, benchmark_match_send_list_with_remainder,
};

const PRIMARY_WIDTH: usize = 4_096;
const GUARD_WIDTH: usize = 256;

fn integer(value: usize) -> Par {
    new_gint_par(
        i64::try_from(value).expect("benchmark index fits i64"),
        Vec::new(),
        false,
    )
}

fn connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

struct ParFixture {
    targets: Vec<Par>,
    patterns: Vec<Par>,
    expected: bool,
}

impl ParFixture {
    fn rejecting_connective_chain() -> Self {
        let targets = (0..PRIMARY_WIDTH).map(integer).collect();
        let patterns = (0..PRIMARY_WIDTH)
            .map(|pattern| {
                if pattern == 0 {
                    integer(1)
                } else {
                    connective(ConnOrBody(ConnectiveBody {
                        ps: vec![integer(pattern), integer(pattern + 1)],
                    }))
                }
            })
            .collect();
        Self {
            targets,
            patterns,
            expected: false,
        }
    }

    fn exact_diagonal() -> Self {
        Self {
            targets: (0..GUARD_WIDTH).map(integer).collect(),
            patterns: (0..GUARD_WIDTH).map(integer).collect(),
            expected: true,
        }
    }

    fn positive_disjunctions() -> Self {
        Self {
            targets: (0..GUARD_WIDTH).map(integer).collect(),
            patterns: (0..GUARD_WIDTH)
                .map(|pattern| {
                    connective(ConnOrBody(ConnectiveBody {
                        ps: vec![integer(pattern + GUARD_WIDTH), integer(pattern)],
                    }))
                })
                .collect(),
            expected: true,
        }
    }

    fn positive_conjunctions() -> Self {
        Self {
            targets: (0..GUARD_WIDTH).map(integer).collect(),
            patterns: (0..GUARD_WIDTH)
                .map(|pattern| {
                    connective(ConnAndBody(ConnectiveBody {
                        ps: vec![
                            connective(ConnOrBody(ConnectiveBody {
                                ps: vec![integer(pattern + GUARD_WIDTH), integer(pattern)],
                            })),
                            integer(pattern),
                        ],
                    }))
                })
                .collect(),
            expected: true,
        }
    }

    fn duplicate_exact_terms() -> Self {
        Self {
            targets: vec![integer(7); GUARD_WIDTH],
            patterns: vec![integer(7); GUARD_WIDTH],
            expected: true,
        }
    }
}

fn bench_par_fixture(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    fixture: &ParFixture,
) {
    for (arm, optimized) in [("control", false), ("treatment", true)] {
        group.bench_with_input(
            BenchmarkId::new(name, arm),
            &optimized,
            |bencher, optimized| {
                bencher.iter_batched(
                    || (fixture.targets.clone(), fixture.patterns.clone()),
                    |(targets, patterns)| {
                        let result =
                            benchmark_match_par_list(targets, patterns, *optimized).is_some();
                        assert_eq!(result, fixture.expected);
                        black_box(result)
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
}

fn send(index: usize) -> Send {
    new_send(
        integer(index),
        vec![integer(index.wrapping_mul(17))],
        false,
        Vec::new(),
        false,
    )
}

fn bench_connective_domains(criterion: &mut Criterion) {
    let primary = ParFixture::rejecting_connective_chain();
    let diagonal = ParFixture::exact_diagonal();
    let positive_or = ParFixture::positive_disjunctions();
    let positive_and = ParFixture::positive_conjunctions();
    let duplicates = ParFixture::duplicate_exact_terms();
    let remainder_targets: Vec<_> = (0..GUARD_WIDTH).map(send).collect();
    let remainder_patterns = vec![send(GUARD_WIDTH - 1)];

    let mut group = criterion.benchmark_group("spatial_matcher_connective_domains");
    bench_par_fixture(&mut group, "primary_rejecting_or_chain", &primary);
    bench_par_fixture(&mut group, "guard_exact_diagonal", &diagonal);
    bench_par_fixture(&mut group, "guard_positive_or", &positive_or);
    bench_par_fixture(&mut group, "guard_positive_and", &positive_and);
    bench_par_fixture(&mut group, "guard_duplicate_exact", &duplicates);

    for (arm, optimized) in [("control", false), ("treatment", true)] {
        group.bench_with_input(
            BenchmarkId::new("guard_send_remainder", arm),
            &optimized,
            |bencher, optimized| {
                bencher.iter_batched(
                    || (remainder_targets.clone(), remainder_patterns.clone()),
                    |(targets, patterns)| {
                        let result = benchmark_match_send_list_with_remainder(
                            targets, patterns, 0, *optimized,
                        )
                        .is_some();
                        assert!(result);
                        black_box(result)
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5))
        .noise_threshold(0.02)
        .with_plots()
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_connective_domains
}
criterion_main!(benches);
