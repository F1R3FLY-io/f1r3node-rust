//! Headless profiler driver for the candidate-mask evaluator differential.

use std::hint::black_box;
use std::time::Instant;

use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::new_gint_par;
use rholang::rust::interpreter::matcher::CandidateMaskBenchmarkFixture;

fn integer(value: usize) -> Par {
    new_gint_par(
        i64::try_from(value).expect("probe value fits i64"),
        Vec::new(),
        false,
    )
}

fn connective(instance: models::rhoapi::connective::ConnectiveInstance) -> Par {
    vector_par(Vec::new(), true).with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

fn opposing_width_connective(depth: usize, conjunction: bool) -> Par {
    let mut construction = ParConstruction::new();
    let terminal = construction
        .import_par(&integer(1))
        .expect("import probe terminal");
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
        let shallow_component = construction
            .connective_component(&shallow_shell, &vec![terminal; shallow_arity])
            .expect("plan probe shallow sibling");
        let shallow = construction
            .singleton_component(shallow_component, Vec::new(), true)
            .expect("plan probe shallow carrier");
        let outer_component = construction
            .connective_component(&outer_shell, &[shallow, nested])
            .expect("plan probe outer connective");
        nested = construction
            .singleton_component(outer_component, Vec::new(), true)
            .expect("plan probe outer carrier");
        nested_work = 1 + nested_work + (1 + shallow_arity);
    }
    construction
        .finish(nested)
        .expect("finish probe connective")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let arm = args.next().expect("arm: control|treatment");
    let fixture_name = args.next().expect("fixture: duplicates4096|opposing256");
    let iterations = args
        .next()
        .expect("iteration count")
        .parse::<usize>()
        .expect("iteration count is usize");
    assert!(args.next().is_none(), "unexpected probe argument");
    let optimized = match arm.as_str() {
        "control" => false,
        "treatment" => true,
        _ => panic!("arm must be control or treatment"),
    };
    let (targets, pattern, expected) = match fixture_name.as_str() {
        "duplicates4096" => (
            vec![integer(1); 4_096],
            connective(ConnOrBody(ConnectiveBody {
                ps: std::iter::repeat_with(|| integer(1)).take(32).collect(),
            })),
            4_096,
        ),
        "opposing256" => (
            vec![integer(1); 256],
            opposing_width_connective(8, false),
            256,
        ),
        _ => panic!("fixture must be duplicates4096 or opposing256"),
    };
    let prepared =
        CandidateMaskBenchmarkFixture::from_targets_and_pattern(targets, pattern, optimized)
            .expect("probe target index is representable");
    for _ in 0..3 {
        assert_eq!(prepared.positions().len(), expected);
    }

    let started = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..iterations {
        let positions = black_box(prepared.positions());
        assert_eq!(positions.len(), expected);
        checksum = checksum.wrapping_add(positions.first().copied().unwrap_or(0));
        checksum = checksum.wrapping_add(positions.last().copied().unwrap_or(0));
        black_box(&positions);
    }
    println!(
        "candidate-mask-probe arm={arm} fixture={fixture_name} warmup=3 iterations={iterations} \
         elapsed_ns={} checksum={checksum}",
        started.elapsed().as_nanos()
    );
}
