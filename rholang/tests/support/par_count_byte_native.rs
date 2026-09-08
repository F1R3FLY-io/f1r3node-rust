use std::cell::{Cell, RefCell};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Connective, ConnectiveBody, EVar, Expr, Par, Var};
use models::rust::par_bag::Multiplicity;
use models::rust::rholang::protobuf_cursor::ProtobufRegion;

use super::{CountBound, CountNode, DecodedSource, ParCount};

#[path = "../../../models/tests/par_corpus/mod.rs"]
mod corpus;

#[derive(Default, Debug)]
struct WorkStats {
    decoded: usize,
    copies: Vec<(usize, u64)>,
    visits: Vec<(bool, std::ops::Range<usize>)>,
    max_work: usize,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static STATS: RefCell<WorkStats> = RefCell::new(WorkStats::default());
}

struct Measurement;

impl Drop for Measurement {
    fn drop(&mut self) { ACTIVE.with(|active| active.set(false)); }
}

pub(super) fn decoded_component() { record(|stats| stats.decoded += 1); }

pub(super) fn copied_endpoint(bytes: usize, multiplicity: u64) {
    record(|stats| stats.copies.push((bytes, multiplicity)));
}

pub(super) fn visited_region(connective: bool, region: &ProtobufRegion) {
    record(|stats| stats.visits.push((connective, region.absolute_range())));
}

pub(super) fn live_work(count: usize) {
    record(|stats| stats.max_work = stats.max_work.max(count));
}

fn record(work: impl FnOnce(&mut WorkStats)) {
    if ACTIVE.with(Cell::get) {
        STATS.with(|stats| work(&mut stats.borrow_mut()));
    }
}

fn measure_stats<T>(work: impl FnOnce() -> T) -> (T, WorkStats) {
    ACTIVE.with(|active| assert!(!active.replace(true), "measurement scopes do not nest"));
    let guard = Measurement;
    STATS.with(|stats| *stats.borrow_mut() = WorkStats::default());
    let result = work();
    let stats = STATS.with(|stats| std::mem::take(&mut *stats.borrow_mut()));
    drop(guard);
    (result, stats)
}

fn measure<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let (result, stats) = measure_stats(work);
    (result, stats.decoded)
}

fn scalar_par() -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }])
}

fn one_expression() -> ParCount {
    let mut result = ParCount::zero();
    result.exprs = CountBound::Finite(1);
    result
}

#[test]
fn resident_connective_counting_does_not_decode_suffixes() {
    let mut subject = scalar_par();
    for _ in 0..8 {
        subject = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![subject],
            })),
        }]);
    }
    let retained = subject.clone();
    let ((minimum, maximum), decoded) = measure(|| ParCount::zero().min_max_par_ref(&subject));
    assert_eq!(minimum, one_expression());
    assert_eq!(maximum, minimum);
    assert!(subject
        .components()
        .shares_storage_with(retained.components()));
    assert_eq!(
        decoded, 0,
        "admitted byte-native counting must not decode recursive components"
    );
}

#[test]
fn admitted_missing_and_signed_expression_forms_do_not_decode() {
    let mut instances = vec![None, Some(ExprInstance::EVarBody(EVar { v: None }))];
    instances.push(Some(ExprInstance::EVarBody(EVar {
        v: Some(Var { var_instance: None }),
    })));
    for level in [i32::MIN, -1, 0, 1, i32::MAX] {
        for variable in [VarInstance::BoundVar(level), VarInstance::FreeVar(level)] {
            instances.push(Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(variable),
                }),
            })));
        }
    }
    for instance in instances {
        let hole = matches!(
            &instance,
            Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(_))
                }),
            }))
        );
        let subject = Par::default().with_exprs(vec![Expr {
            expr_instance: instance,
        }]);
        let ((minimum, maximum), decoded) = measure(|| ParCount::zero().min_max_par_ref(&subject));
        let expected = if hole {
            (ParCount::zero(), ParCount::unbounded())
        } else {
            (one_expression(), one_expression())
        };
        assert_eq!((minimum, maximum), expected);
        assert_eq!(
            decoded, 0,
            "every admitted missing/signed Expr form must be byte-classified"
        );
    }
}

#[test]
fn compressed_connective_multiplicity_does_not_require_decoding() {
    for amount in [1, 1_001, u64::MAX] {
        let mut subject = Par::default();
        subject
            .insert_component_many(
                &Connective {
                    connective_instance: Some(ConnectiveInstance::ConnInt(false)),
                },
                Multiplicity::new(amount).expect("positive fixture multiplicity"),
                Vec::new(),
                false,
            )
            .expect("canonical scalar connective");
        let ((minimum, maximum), decoded) = measure(|| ParCount::zero().min_max_par_ref(&subject));
        let mut expected = ParCount::zero();
        expected.exprs = CountBound::Finite(u128::from(amount));
        assert_eq!(minimum, expected);
        assert_eq!(maximum, expected);
        assert_eq!(
            decoded, 0,
            "distinct scalar Connective endpoints require no decoder"
        );
    }
}

fn delimited(field: u8, payload: &[u8]) -> Vec<u8> {
    let mut result = vec![(field << 3) | 2];
    let mut len = payload.len();
    while len >= 128 {
        result.push((len as u8 & 127) | 128);
        len >>= 7;
    }
    result.push(len as u8);
    result.extend_from_slice(payload);
    result
}

fn encoded_count(bytes: &[u8]) -> ((ParCount, ParCount), WorkStats) {
    let region = ProtobufRegion::copy_from_slice(bytes);
    measure_stats(|| ParCount::zero().min_max_drive::<DecodedSource>(CountNode::EncodedPar(region)))
}

#[test]
fn every_generated_expression_uses_the_byte_route_in_encoded_par_regions() {
    use models::rust::rholang::protobuf_encoder::encode_to_vec;
    use models::rust::rholang::protobuf_schema_tables::EXPR_INSTANCE_PROTOBUF_VARIANTS;

    let arms = corpus::every_expr_instance();
    assert_eq!(EXPR_INSTANCE_PROTOBUF_VARIANTS.len(), 36);
    assert_eq!(arms.len(), EXPR_INSTANCE_PROTOBUF_VARIANTS.len());
    for ((name, instance), descriptor) in arms.into_iter().zip(EXPR_INSTANCE_PROTOBUF_VARIANTS) {
        assert_eq!(name, descriptor.name);
        let expression = Expr {
            expr_instance: Some(instance),
        };
        let expected = if matches!(
            expression.expr_instance.as_ref(),
            Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(_) | VarInstance::Wildcard(_)),
                })
            }))
        ) {
            (ParCount::zero(), ParCount::unbounded())
        } else {
            (one_expression(), one_expression())
        };
        let bytes = delimited(5, &encode_to_vec(&expression));
        let (actual, stats) = encoded_count(&bytes);
        assert_eq!(actual, expected, "{name}");
        assert_eq!(stats.decoded, 0, "{name}");
        assert!(stats.copies.is_empty(), "{name}");
    }
}

#[test]
fn encoded_equal_empty_bodies_preserve_family_and_repeated_occurrences() {
    let mut bytes = Vec::new();
    for field in [1, 2, 4, 5, 6, 7, 8, 11, 12] {
        for _ in 0..2 {
            bytes.extend(delimited(field, &[]));
        }
    }
    let (actual, stats) = encoded_count(&bytes);
    let two = CountBound::Finite(2);
    let expected = ParCount {
        sends: two,
        receives: two,
        news: two,
        exprs: two,
        matches: two,
        unforgeables: two,
        bundles: two,
        conditionals: two,
    };
    assert_eq!(actual, (expected.clone(), expected));
    assert_eq!(stats.decoded, 0);
    assert_eq!(stats.visits.len(), 3);
    assert!(stats.copies.is_empty());
}

#[test]
fn byte_cursor_visits_children_once_in_source_order_without_nested_copies() {
    for width in [0usize, 1, 16, 256] {
        let mut body = Vec::new();
        let mut relative = Vec::new();
        for index in 0..width {
            let child = delimited(5, &[]).repeat(index % 5 + 1);
            let field = delimited(1, &child);
            let start = body.len() + field.len() - child.len();
            relative.push(start..start + child.len());
            body.extend(field);
        }
        for tag in [1, 2] {
            let bytes = delimited(tag, &body);
            let offset = bytes.len() - body.len();
            let region = ProtobufRegion::copy_from_slice(&bytes);
            let (actual, stats) = measure_stats(|| {
                ParCount::zero()
                    .min_max_drive::<DecodedSource>(CountNode::EncodedConnective(region))
            });
            let mut expected_trace = vec![(true, 0..bytes.len())];
            expected_trace.extend(
                relative
                    .iter()
                    .map(|r| (false, r.start + offset..r.end + offset)),
            );
            assert_eq!(stats.visits, expected_trace);
            assert_eq!(stats.decoded, 0);
            assert!(stats.copies.is_empty());
            assert!(
                stats.max_work <= 2,
                "width {width}: {} frames",
                stats.max_work
            );
            if width > 1 {
                let mut reversed = expected_trace.clone();
                reversed[1..].reverse();
                assert_ne!(stats.visits, reversed);
                let mut duplicated = expected_trace.clone();
                duplicated.push(expected_trace[1].clone());
                assert_ne!(stats.visits, duplicated);
                let mut skipped = expected_trace.clone();
                skipped.remove(1);
                assert_ne!(stats.visits, skipped);
            }
            if width == 0 {
                assert_eq!(
                    actual,
                    if tag == 1 {
                        (ParCount::zero(), ParCount::unbounded())
                    } else {
                        (ParCount::unbounded(), ParCount::zero())
                    }
                );
            }
        }
    }
}

#[test]
fn nested_count_regions_share_one_endpoint_copy_with_depth_bounded_work() {
    for depth in [1usize, 8, 32, 128] {
        let mut subject = scalar_par();
        for level in 0..depth {
            let body = ConnectiveBody { ps: vec![subject] };
            subject = Par::default().with_connectives(vec![Connective {
                connective_instance: Some(if level % 2 == 0 {
                    ConnectiveInstance::ConnAndBody(body)
                } else {
                    ConnectiveInstance::ConnOrBody(body)
                }),
            }]);
        }
        let mut payload_len = 0;
        subject.visit_encoded::<Connective>(|entry| payload_len = entry.payload().len());
        let (actual, stats) = measure_stats(|| ParCount::zero().min_max_par_ref(&subject));
        assert_eq!(actual, (one_expression(), one_expression()));
        assert_eq!(stats.copies, vec![(payload_len, 1)]);
        assert_eq!(stats.decoded, 0);
        assert_eq!(stats.visits.len(), depth * 2);
        assert!(stats.max_work <= depth * 2 + 1);
        for pair in stats.visits.windows(2) {
            assert!(pair[0].1.start <= pair[1].1.start);
            assert!(pair[1].1.end <= pair[0].1.end);
        }
    }
}

#[test]
fn compressed_endpoint_work_is_independent_of_multiplicity() {
    let connective = Connective {
        connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: vec![scalar_par()],
        })),
    };
    let mut previous = None;
    for amount in [1, 1_001, u64::MAX] {
        let mut subject = Par::default();
        subject
            .insert_component_many(
                &connective,
                Multiplicity::new(amount).unwrap(),
                vec![],
                false,
            )
            .unwrap();
        let (actual, stats) = measure_stats(|| ParCount::zero().min_max_par_ref(&subject));
        let mut expected = one_expression();
        expected.exprs = CountBound::Finite(u128::from(amount));
        assert_eq!(actual, (expected.clone(), expected));
        assert_eq!(stats.decoded, 0);
        assert_eq!(stats.copies.len(), 1);
        assert_eq!(stats.copies[0].1, amount);
        let work = (stats.copies[0].0, stats.visits, stats.max_work);
        if let Some(previous) = &previous {
            assert_eq!(&work, previous);
        }
        previous = Some(work);
    }
}

#[test]
fn unproved_par_framing_delegates_to_generated_decoder() {
    use models::rust::rholang::protobuf_decoder::decode_par;
    for bytes in [vec![0x50, 0], vec![0x2a, 0, 0x2a, 0], vec![0x80], vec![
        0x2a, 5,
    ]] {
        let expected = decode_par(bytes.as_slice());
        let observed = std::panic::catch_unwind(|| encoded_count(&bytes));
        match expected {
            Ok(par) => {
                let (actual, stats) = observed.expect("generated decoder accepts the frame");
                assert_eq!(actual, ParCount::zero().min_max_par_ref(&par));
                if bytes == [0x50, 0] {
                    assert!(stats.decoded > 0);
                }
            }
            Err(_) => assert!(
                observed.is_err(),
                "invalid framing must not invent an interval"
            ),
        }
    }
}

#[test]
fn root_and_child_metadata_do_not_change_counting_or_copy_work() {
    use models::rust::rholang::protobuf_encoder::encode_to_vec;

    for active in [false, true] {
        for free in [vec![], vec![1, 128]] {
            let mut child = scalar_par();
            child.replace_connective_used(active);
            child.replace_locally_free(free.clone());
            let body = ConnectiveBody { ps: vec![child] };
            let mut subject = Par::default().with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(body)),
            }]);
            subject.replace_connective_used(active);
            subject.replace_locally_free(free);
            let bytes = encode_to_vec(&subject);
            let (encoded, encoded_stats) = encoded_count(&bytes);
            let (resident, resident_stats) =
                measure_stats(|| ParCount::zero().min_max_par_ref(&subject));
            assert_eq!(encoded, (one_expression(), one_expression()));
            assert_eq!(resident, encoded);
            assert_eq!(encoded_stats.decoded, 0);
            assert_eq!(resident_stats.decoded, 0);
            assert!(encoded_stats.copies.is_empty());
            assert_eq!(resident_stats.copies.len(), 1);
        }
    }
}

#[test]
fn terminal_connectives_and_signed_var_refs_preserve_equations_without_descent() {
    use models::rhoapi::VarRef;
    use models::rust::rholang::protobuf_encoder::encode_to_vec;

    let mut cases = vec![
        (Connective::default(), (ParCount::zero(), ParCount::zero())),
        (
            Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(scalar_par())),
            },
            (ParCount::zero(), ParCount::unbounded()),
        ),
    ];
    for value in [false, true] {
        for instance in [
            ConnectiveInstance::ConnBool(value),
            ConnectiveInstance::ConnInt(value),
            ConnectiveInstance::ConnString(value),
            ConnectiveInstance::ConnUri(value),
            ConnectiveInstance::ConnByteArray(value),
        ] {
            cases.push((
                Connective {
                    connective_instance: Some(instance),
                },
                (one_expression(), one_expression()),
            ));
        }
    }
    for index in [i32::MIN, -1, 0, 1, i32::MAX] {
        for depth in [i32::MIN, -1, 0, 1, i32::MAX] {
            cases.push((
                Connective {
                    connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef {
                        index,
                        depth,
                    })),
                },
                (ParCount::zero(), ParCount::zero()),
            ));
        }
    }
    for (subject, expected) in cases {
        let bytes = encode_to_vec(&subject);
        let region = ProtobufRegion::copy_from_slice(&bytes);
        let (encoded, stats) = measure_stats(|| {
            ParCount::zero().min_max_drive::<DecodedSource>(CountNode::EncodedConnective(region))
        });
        assert_eq!(encoded, expected);
        assert_eq!(stats.visits, vec![(true, 0..bytes.len())]);
        assert_eq!(stats.decoded, 0);
        assert!(stats.copies.is_empty());
        assert_eq!(ParCount::zero().min_max_con_ref(&subject), expected);
        assert_eq!(ParCount::zero().min_max_con(subject), expected);
    }
}
