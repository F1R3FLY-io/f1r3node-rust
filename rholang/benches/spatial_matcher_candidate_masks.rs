//! Causal benchmark for candidate-domain representation and evaluation.
//!
//! Both arms build the same authoritative target `Bag` and candidate plan.
//! The benchmark-only control reconstructs the pre-fix occurrence-expanded,
//! postorder `PathMap<()>` evaluator; the treatment keeps duplicate identities
//! as group endpoints, schedules by Ershov pressure, streams lattice folds and
//! expands the final domain adaptively. Fixture cloning is excluded.

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion, Throughput,
};
use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_gint_par, new_wildcard_par};
use rholang::rust::interpreter::matcher::CandidateMaskBenchmarkFixture;

fn integer(value: usize) -> Par {
    new_gint_par(
        i64::try_from(value).expect("benchmark value fits i64"),
        Vec::new(),
        false,
    )
}

fn connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

fn flat_connective(values: impl IntoIterator<Item = Par>, conjunction: bool) -> Par {
    let ps = values.into_iter().collect();
    connective(if conjunction {
        ConnAndBody(ConnectiveBody { ps })
    } else {
        ConnOrBody(ConnectiveBody { ps })
    })
}

fn opposing_width_connective(depth: usize, conjunction: bool) -> Par {
    let mut construction = ParConstruction::new();
    let terminal = construction
        .import_par(&integer(1))
        .expect("import benchmark terminal");
    let mut nested = terminal;
    let mut nested_work = 1usize;
    let outer_shell = Connective {
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
        let shallow_arity = nested_work + 1;
        let shallow_shell = Connective {
            connective_instance: Some(if conjunction {
                ConnAndBody(ConnectiveBody {
                    ps: vec![Par::default(); shallow_arity],
                })
            } else {
                ConnOrBody(ConnectiveBody {
                    ps: vec![Par::default(); shallow_arity],
                })
            }),
        };
        let shallow = construction
            .connective_component(&shallow_shell, &vec![terminal; shallow_arity])
            .and_then(|component| construction.singleton_component(component, Vec::new(), true))
            .expect("plan benchmark shallow sibling");
        let outer = construction
            .connective_component(&outer_shell, &[shallow, nested])
            .and_then(|component| construction.singleton_component(component, Vec::new(), true))
            .expect("plan benchmark opposing-width carrier");
        nested = outer;
        nested_work = 1 + nested_work + (1 + shallow_arity);
    }
    construction
        .finish(nested)
        .expect("finish benchmark opposing-width connective")
}

struct Fixture {
    targets: Vec<Par>,
    pattern: Par,
    expected_positions: Vec<u64>,
}

fn bench_fixture(
    group: &mut BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    fixture: &Fixture,
) {
    group.throughput(Throughput::Elements(fixture.targets.len() as u64));
    let control =
        CandidateMaskBenchmarkFixture::new(fixture.targets.clone(), fixture.pattern.clone(), false)
            .expect("control target index is representable");
    let treatment =
        CandidateMaskBenchmarkFixture::new(fixture.targets.clone(), fixture.pattern.clone(), true)
            .expect("treatment target index is representable");
    let control_preflight = control.positions();
    let treatment_preflight = treatment.positions();
    assert_eq!(
        control_preflight, treatment_preflight,
        "arm mismatch: {name}"
    );
    assert_eq!(
        control_preflight.as_ref(),
        fixture.expected_positions.as_slice(),
        "oracle mismatch: {name}"
    );

    for (arm, prepared) in [
        ("control_occurrence", &control),
        ("treatment_group", &treatment),
    ] {
        group.bench_function(BenchmarkId::new(name, arm), |bencher| {
            bencher.iter(|| black_box(prepared.positions()));
        });
    }
}

fn bench_candidate_masks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("spatial_matcher_candidate_masks");

    for width in [1usize, 16, 256, 4_096] {
        for conjunction in [false, true] {
            let fixture = Fixture {
                targets: vec![integer(1); width],
                pattern: flat_connective(
                    std::iter::repeat_with(|| integer(1)).take(32),
                    conjunction,
                ),
                expected_positions: (0..width).map(|position| position as u64).collect(),
            };
            bench_fixture(
                &mut group,
                &format!(
                    "duplicates_width_{width}_{}",
                    if conjunction { "and" } else { "or" }
                ),
                &fixture,
            );
        }
    }

    let distinct_sparse = Fixture {
        targets: (0..4_096).map(integer).collect(),
        pattern: flat_connective([integer(7), integer(4_000)], false),
        expected_positions: vec![7, 4_000],
    };
    bench_fixture(&mut group, "distinct_sparse_width_4096", &distinct_sparse);

    let interleaved_dense = Fixture {
        targets: (0..8).flat_map(|_| (0..128).map(integer)).collect(),
        pattern: flat_connective((0..96).map(integer), false),
        expected_positions: (0..1_024)
            .filter(|position| position % 128 < 96)
            .map(|position| position as u64)
            .collect(),
    };
    bench_fixture(&mut group, "interleaved_dense_1024", &interleaved_dense);

    for conjunction in [false, true] {
        let opposing = Fixture {
            targets: vec![integer(1); 256],
            pattern: opposing_width_connective(8, conjunction),
            expected_positions: (0..256).map(|position| position as u64).collect(),
        };
        bench_fixture(
            &mut group,
            if conjunction {
                "opposing_depth_8_width_256_and"
            } else {
                "opposing_depth_8_width_256_or"
            },
            &opposing,
        );
    }

    // Pre-fix candidate compilation retained a universal Query node here, so
    // postorder still evaluated the expensive sibling before union discovered
    // `All`.  Treatment canonicalizes the universal leaf and collapses the
    // plan before evaluation.
    let universal_before_expensive = Fixture {
        targets: (0..4_096).map(integer).collect(),
        pattern: flat_connective(
            [
                new_wildcard_par(Vec::new(), true),
                opposing_width_connective(8, false),
            ],
            false,
        ),
        expected_positions: (0..4_096).map(|position| position as u64).collect(),
    };
    bench_fixture(
        &mut group,
        "universal_before_opposing_depth_8_width_4096",
        &universal_before_expensive,
    );

    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(51)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4))
        .noise_threshold(0.02)
        .with_plots()
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_candidate_masks
}
criterion_main!(benches);
