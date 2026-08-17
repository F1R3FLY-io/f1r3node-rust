//! Causal benchmark for consuming zipper algebra over owned candidate masks.
//!
//! Both arms use the already-selected native PathMap connective domains and
//! the complete production list PDA. The benchmark-only treatment changes only
//! reduction of owned finite position masks from immutable `join`/`meet` to
//! consuming `join_map_into`/`meet_into`. Fixture cloning is excluded from each
//! timed sample.

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkGroup, BenchmarkId, Criterion,
};
use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::matcher::benchmark_match_par_list_candidate_mask_algebra;

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

struct Fixture {
    targets: Vec<Par>,
    patterns: Vec<Par>,
    expected: bool,
}

impl Fixture {
    fn rejecting_or_chain() -> Self {
        Self {
            targets: (0..PRIMARY_WIDTH).map(integer).collect(),
            patterns: (0..PRIMARY_WIDTH)
                .map(|pattern| {
                    if pattern == 0 {
                        integer(1)
                    } else {
                        connective(ConnOrBody(ConnectiveBody {
                            ps: vec![integer(pattern), integer(pattern + 1)],
                        }))
                    }
                })
                .collect(),
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

    fn finite_unions() -> Self {
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

    fn finite_intersections() -> Self {
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

    fn duplicate_positions() -> Self {
        Self {
            targets: vec![integer(7); GUARD_WIDTH],
            patterns: (0..GUARD_WIDTH)
                .map(|_| {
                    connective(ConnOrBody(ConnectiveBody {
                        ps: vec![integer(8), integer(7)],
                    }))
                })
                .collect(),
            expected: true,
        }
    }
}

fn bench_fixture(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    fixture: &Fixture,
) {
    for (arm, consuming) in [("control", false), ("treatment", true)] {
        group.bench_with_input(
            BenchmarkId::new(name, arm),
            &consuming,
            |bencher, consuming| {
                bencher.iter_batched(
                    || (fixture.targets.clone(), fixture.patterns.clone()),
                    |(targets, patterns)| {
                        let (result, free_map) = benchmark_match_par_list_candidate_mask_algebra(
                            targets, patterns, *consuming,
                        );
                        assert_eq!(result.is_some(), fixture.expected);
                        black_box((result, free_map))
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
}

fn bench_candidate_mask_algebra(criterion: &mut Criterion) {
    let primary = Fixture::rejecting_or_chain();
    let exact = Fixture::exact_diagonal();
    let unions = Fixture::finite_unions();
    let intersections = Fixture::finite_intersections();
    let duplicates = Fixture::duplicate_positions();

    let mut group = criterion.benchmark_group("spatial_matcher_candidate_mask_algebra");
    bench_fixture(&mut group, "primary_rejecting_or_chain", &primary);
    bench_fixture(&mut group, "guard_exact_diagonal", &exact);
    bench_fixture(&mut group, "guard_finite_unions", &unions);
    bench_fixture(&mut group, "guard_finite_intersections", &intersections);
    bench_fixture(&mut group, "guard_duplicate_positions", &duplicates);
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
    targets = bench_candidate_mask_algebra
}
criterion_main!(benches);
