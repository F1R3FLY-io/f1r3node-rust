use models::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::protobuf_encoder::encode_to_vec;

use super::{counting_alloc, integer, CountBound, ParCount};

fn leaf(value: i64) -> Par { Par::default().with_exprs(vec![integer(value)]) }

fn branch(child: Par, and: bool) -> Par {
    let body = ConnectiveBody { ps: vec![child] };
    Par::default()
        .with_connectives(vec![Connective {
            connective_instance: Some(if and {
                ConnAndBody(body)
            } else {
                ConnOrBody(body)
            }),
        }])
        .with_connective_used(true)
}

fn planned_spine(depth: usize) -> Par {
    let mut arena = ParConstruction::new();
    let mut root = arena.import_par(&leaf(0)).unwrap();
    for level in 0..depth {
        let body = ConnectiveBody {
            ps: vec![Par::default()],
        };
        let shell = Connective {
            connective_instance: Some(if level % 2 == 0 {
                ConnAndBody(body)
            } else {
                ConnOrBody(body)
            }),
        };
        let component = arena.connective_component(&shell, &[root]).unwrap();
        root = arena.singleton_component(component, vec![], true).unwrap();
    }
    arena.finish(root).unwrap()
}

fn wide(width: usize, separate: bool) -> Par {
    let children: Vec<_> = (0..width).map(|i| leaf(i as i64)).collect();
    let connectives = if separate {
        children
            .into_iter()
            .map(|child| Connective {
                connective_instance: Some(ConnAndBody(ConnectiveBody { ps: vec![child] })),
            })
            .collect()
    } else {
        vec![Connective {
            connective_instance: Some(ConnAndBody(ConnectiveBody { ps: children })),
        }]
    };
    Par::default()
        .with_connectives(connectives)
        .with_connective_used(true)
}

fn expected(expressions: usize) -> (ParCount, ParCount) {
    let mut count = ParCount::zero();
    count.exprs = CountBound::Finite(expressions as u128);
    (count.clone(), count)
}

fn sample(subject: &Par, expressions: usize) -> (usize, usize) {
    let retained = subject.clone();
    let (result, allocations, bytes) =
        counting_alloc::measure(|| ParCount::zero().min_max_par_ref(subject));
    assert_eq!(result, expected(expressions));
    assert!(subject
        .components()
        .shares_storage_with(retained.components()));
    (allocations, bytes)
}

#[test]
fn planned_spines_equal_original_small_fixtures() {
    for depth in 0..16 {
        let mut original = leaf(0);
        for level in 0..depth {
            original = branch(original, level % 2 == 0);
        }
        let planned = planned_spine(depth);
        assert_eq!(planned, original);
        assert_eq!(encode_to_vec(&planned), encode_to_vec(&original));
        assert_eq!(ParCount::zero().min_max_par_ref(&planned), expected(1));
    }
}

#[test]
fn count_only_allocations_follow_depth_not_repeated_suffix_size() {
    let mut previous = None;
    for depth in [128usize, 512, 2048] {
        let subject = planned_spine(depth);
        let input = encode_to_vec(&subject).len();
        let measured = sample(&subject, 1);
        eprintln!(
            "PCBN allocation depth={depth} input_bytes={input} calls={} requested_bytes={}",
            measured.0, measured.1
        );
        if let Some((old_depth, old)) = previous {
            let old: (usize, usize) = old;
            counting_alloc::assert_subquadratic("count calls", old_depth, old.0, depth, measured.0)
                .unwrap();
            counting_alloc::assert_subquadratic(
                "count requested bytes",
                old_depth,
                old.1,
                depth,
                measured.1,
            )
            .unwrap();
        }
        previous = Some((depth, measured));
    }
}

#[test]
fn count_only_allocations_follow_width_for_encoded_and_resident_siblings() {
    for separate in [false, true] {
        let mut previous = None;
        for width in [32usize, 128, 512] {
            let subject = wide(width, separate);
            let input = encode_to_vec(&subject).len();
            let measured = sample(&subject, if separate { width } else { 1 });
            eprintln!("PCBN allocation separate={separate} width={width} input_bytes={input} calls={} requested_bytes={}", measured.0, measured.1);
            if let Some((old_width, old)) = previous {
                let old: (usize, usize) = old;
                counting_alloc::assert_subquadratic(
                    "wide count calls",
                    old_width,
                    old.0,
                    width,
                    measured.0,
                )
                .unwrap();
                counting_alloc::assert_subquadratic(
                    "wide count requested bytes",
                    old_width,
                    old.1,
                    width,
                    measured.1,
                )
                .unwrap();
            }
            previous = Some((width, measured));
        }
    }
}

fn repeated_suffix_control(subject: &Par) -> ((ParCount, ParCount), usize) {
    let mut cursor = subject.clone();
    let mut decoded = 0;
    while let Some((connective, multiplicity)) = cursor.first::<Connective>().unwrap() {
        assert_eq!(multiplicity.get(), 1);
        decoded += 1;
        match connective.connective_instance {
            Some(ConnAndBody(mut body) | ConnOrBody(mut body)) => {
                assert_eq!(body.ps.len(), 1);
                cursor = body.ps.pop().unwrap();
            }
            _ => panic!("the control requires a unary AND/OR spine"),
        }
    }
    (ParCount::zero().min_max_par_ref(&cursor), decoded)
}

#[test]
fn actual_repeated_suffix_decoding_is_rejected_by_the_allocation_bound() {
    let mut observations = Vec::new();
    for depth in [16usize, 64] {
        let subject = planned_spine(depth);
        let ((interval, decoded), calls, bytes) =
            counting_alloc::measure(|| repeated_suffix_control(&subject));
        assert_eq!(interval, expected(1));
        assert_eq!(decoded, depth);
        let native = sample(&subject, 1);
        eprintln!("PCBN actual suffix control depth={depth} decoded={decoded} calls={calls} requested_bytes={bytes}; native_calls={} native_requested_bytes={}", native.0, native.1);
        observations.push((calls, bytes, native));
    }
    let low = observations[0];
    let high = observations[1];
    assert!(
        counting_alloc::assert_subquadratic("suffix control calls", 16, low.0, 64, high.0).is_err()
    );
    assert!(
        counting_alloc::assert_subquadratic("suffix control bytes", 16, low.1, 64, high.1).is_err()
    );
    counting_alloc::assert_subquadratic("native calls", 16, low.2 .0, 64, high.2 .0).unwrap();
    counting_alloc::assert_subquadratic("native bytes", 16, low.2 .1, 64, high.2 .1).unwrap();
}

#[test]
fn deep_and_wide_counts_complete_on_a_128_kib_stack() {
    std::thread::Builder::new()
        .name("par-count-resource-stack".into())
        .stack_size(128 * 1024)
        .spawn(|| {
            for (subject, expressions) in [
                (planned_spine(4096), 1),
                (wide(4096, false), 1),
                (wide(4096, true), 4096),
            ] {
                let _ = sample(&subject, expressions);
            }
        })
        .unwrap()
        .join()
        .expect("count resource fixtures must complete on a fixed stack");
}
