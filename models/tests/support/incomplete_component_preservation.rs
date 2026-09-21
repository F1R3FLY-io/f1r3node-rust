use super::*;
use crate::rhoapi as rho;
use crate::rust::rholang::protobuf_cursor::ProtobufRegion;

fn leaf(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn arena_lengths(arena: &ParConstruction) -> [usize; 7] {
    [
        arena.byte_fragments.len(),
        arena.scores.len(),
        arena.protobuf_nodes.len(),
        arena.epathmaps.len(),
        arena.components.len(),
        arena.occurrence_nodes.len(),
        arena.pars.len(),
    ]
}

fn check_shell<T: ParBagComponent>(make: impl Fn(&[Par]) -> T, children: &[Par]) -> Option<Par> {
    use std::hash::{Hash, Hasher};
    let resident = make(children);
    let shell = make(&vec![Par::default(); children.len()]);
    let mut arena = ParConstruction::new();
    let prior = arena.import_par(&leaf(73)).unwrap();
    let prior_bytes = arena.protobuf_bytes(prior);
    let handles: Vec<_> = children
        .iter()
        .map(|value| arena.import_par(value).unwrap())
        .collect();
    let before = arena_lengths(&arena);
    let component = arena.incomplete_component(&shell, &handles).unwrap();
    if T::semantic_shape_is_complete(&resident) {
        assert!(component.is_none());
        assert_eq!(arena_lengths(&arena), before);
        return None;
    }
    let component = component.expect("incomplete shell was declined");
    assert!(!arena.components[component.0].canonical_path_stable);
    let mut body = Vec::new();
    arena.emit_proto(arena.components[component.0].protobuf, &mut body);
    assert_eq!(body, protobuf_encoder::encode_to_vec(&resident));
    let count = Multiplicity::new(3).unwrap();
    let planned = arena
        .par([(component, count)], vec![5, 0, 128], true)
        .unwrap();
    let actual = arena.finish(planned).unwrap();
    let mut expected = Par::default();
    expected
        .insert_component_many(&resident, count, vec![5, 0, 128], true)
        .unwrap();
    assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    assert_eq!(
        actual.protobuf_bytes().unwrap(),
        expected.protobuf_bytes().unwrap()
    );
    assert_eq!(
        crate::rust::rholang::bincode_encoder::encode(&actual),
        crate::rust::rholang::bincode_encoder::encode(&expected)
    );
    assert_eq!(actual.locally_free(), expected.locally_free());
    assert_eq!(actual.connective_used(), expected.connective_used());
    assert_eq!(actual.cardinality(), 3);
    assert_eq!(actual.family_cardinality::<T>(), 3);
    assert_eq!(actual.cmp(&expected), Ordering::Equal);
    let mut actual_hash = std::collections::hash_map::DefaultHasher::new();
    let mut expected_hash = std::collections::hash_map::DefaultHasher::new();
    actual.hash(&mut actual_hash);
    expected.hash(&mut expected_hash);
    assert_eq!(actual_hash.finish(), expected_hash.finish());
    let committed = arena.components.len();
    if !handles.is_empty() {
        assert!(matches!(
            arena.incomplete_component(&shell, &handles[..handles.len() - 1]),
            Err(ParConstructionError::MissingParSlot { .. })
        ));
        assert_eq!(arena.components.len(), committed);
    }
    let mut extra = handles.clone();
    extra.push(prior);
    assert!(matches!(
        arena.incomplete_component(&shell, &extra),
        Err(ParConstructionError::ExtraParSlots { .. })
    ));
    assert_eq!(arena.components.len(), committed);
    assert_eq!(arena.protobuf_bytes(prior), prior_bytes);
    assert_eq!(
        arena.finish(planned).unwrap().compact_snapshot(),
        actual.compact_snapshot()
    );
    Some(actual)
}

fn selected_children(mask: u8, count: usize, values: &[Par]) -> Vec<Option<Par>> {
    let mut values = values.iter();
    (0..count)
        .map(|bit| (mask & (1 << bit) != 0).then(|| values.next().unwrap().clone()))
        .collect()
}

fn mask_children(mask: u8) -> Vec<Par> {
    (0..mask.count_ones())
        .map(|index| leaf(9 - i64::from(index)).with_locally_free(vec![index as u8, 128]))
        .collect()
}

#[test]
fn planned_incomplete_top_level_families_match_resident_endpoints() {
    check_shell(
        |v| Send {
            data: v.to_vec(),
            locally_free: vec![7],
            persistent: true,
            connective_used: true,
            ..Send::default()
        },
        &[leaf(9), leaf(2), leaf(9)],
    )
    .unwrap();
    check_shell(
        |v| New {
            uri: vec!["z".into(), "a".into()],
            injections: [("a".into(), v[0].clone()), ("z".into(), v[1].clone())].into(),
            locally_free: vec![1],
            ..New::default()
        },
        &[Par::default(), leaf(8)],
    )
    .unwrap();
    check_shell(
        |_| Bundle {
            write_flag: true,
            read_flag: true,
            ..Bundle::default()
        },
        &[],
    )
    .unwrap();
    for mask in 0u8..8 {
        let children = mask_children(mask);
        check_shell(
            |v| {
                let o = selected_children(mask, 3, v);
                If {
                    condition: o[0].clone(),
                    if_true: o[1].clone(),
                    if_false: o[2].clone(),
                    locally_free: vec![4],
                    connective_used: true,
                }
            },
            &children,
        );
        check_shell(
            |v| {
                let o = selected_children(mask, 3, v);
                Match {
                    target: o[0].clone(),
                    cases: vec![rho::MatchCase {
                        pattern: o[1].clone(),
                        source: o[2].clone(),
                        free_count: 2,
                        guard: None,
                    }],
                    locally_free: vec![3],
                    connective_used: true,
                }
            },
            &children,
        );
    }
    for mask in 0u8..4 {
        check_shell(
            |v| {
                let o = selected_children(mask, 2, v);
                Receive {
                    binds: vec![ReceiveBind {
                        source: o[0].clone(),
                        ..ReceiveBind::default()
                    }],
                    body: o[1].clone(),
                    locally_free: vec![2],
                    connective_used: true,
                    ..Receive::default()
                }
            },
            &mask_children(mask),
        );
    }
}

#[test]
fn planned_incomplete_expr_presence_matrix_matches_resident_endpoints() {
    macro_rules! binary {
        ($variant:ident, $ty:ident) => {
            for mask in 0u8..4 {
                check_shell(
                    |v| {
                        let o = selected_children(mask, 2, v);
                        Expr {
                            expr_instance: Some(ExprInstance::$variant(rho::$ty {
                                p1: o[0].clone(),
                                p2: o[1].clone(),
                            })),
                        }
                    },
                    &mask_children(mask),
                );
            }
        };
    }
    binary!(EMultBody, EMult);
    binary!(EDivBody, EDiv);
    binary!(EModBody, EMod);
    binary!(EPlusBody, EPlus);
    binary!(EMinusBody, EMinus);
    binary!(ELtBody, ELt);
    binary!(ELteBody, ELte);
    binary!(EGtBody, EGt);
    binary!(EGteBody, EGte);
    binary!(EEqBody, EEq);
    binary!(ENeqBody, ENeq);
    binary!(EAndBody, EAnd);
    binary!(EOrBody, EOr);
    binary!(EPercentPercentBody, EPercentPercent);
    binary!(EPlusPlusBody, EPlusPlus);
    binary!(EMinusMinusBody, EMinusMinus);
    for variant in [
        ExprInstance::ENotBody(rho::ENot { p: None }),
        ExprInstance::ENegBody(rho::ENeg { p: None }),
        ExprInstance::EVarBody(rho::EVar { v: None }),
        ExprInstance::EZipperBody(rho::EZipper::default()),
    ] {
        check_shell(
            |_| Expr {
                expr_instance: Some(variant.clone()),
            },
            &[],
        )
        .unwrap();
    }
    for mask in 0u8..4 {
        check_shell(
            |v| {
                let o = selected_children(mask, 2, v);
                Expr {
                    expr_instance: Some(ExprInstance::EMatchesBody(rho::EMatches {
                        target: o[0].clone(),
                        pattern: o[1].clone(),
                    })),
                }
            },
            &mask_children(mask),
        );
    }
    check_shell(
        |v| Expr {
            expr_instance: Some(ExprInstance::EMethodBody(rho::EMethod {
                method_name: "method".into(),
                arguments: v.to_vec(),
                locally_free: vec![7],
                connective_used: true,
                ..rho::EMethod::default()
            })),
        },
        &[leaf(3), Par::default(), leaf(2)],
    )
    .unwrap();
}

#[test]
fn incomplete_collection_keeps_duplicate_pairs_and_order() {
    check_shell(
        |v| Expr {
            expr_instance: Some(ExprInstance::EMapBody(rho::EMap {
                kvs: vec![
                    rho::KeyValuePair {
                        key: Some(v[0].clone()),
                        value: Some(v[1].clone()),
                    },
                    rho::KeyValuePair {
                        key: None,
                        value: Some(v[2].clone()),
                    },
                    rho::KeyValuePair {
                        key: Some(v[3].clone()),
                        value: Some(v[4].clone()),
                    },
                ],
                locally_free: vec![9],
                connective_used: true,
                remainder: None,
            })),
        },
        &[leaf(9), leaf(1), leaf(4), leaf(9), leaf(2)],
    )
    .unwrap();
}

#[test]
fn incomplete_nested_shells_preserve_guards_patterns_and_injections() {
    check_shell(
        |v| Receive {
            binds: vec![ReceiveBind {
                patterns: vec![v[0].clone(), v[1].clone()],
                source: Some(v[2].clone()),
                ..ReceiveBind::default()
            }],
            condition: Some(v[3].clone()),
            ..Receive::default()
        },
        &[leaf(8), leaf(2), leaf(7), Par::default()],
    )
    .unwrap();
    check_shell(
        |v| Match {
            cases: vec![rho::MatchCase {
                pattern: Some(v[0].clone()),
                source: Some(v[1].clone()),
                guard: Some(v[2].clone()),
                free_count: 3,
            }],
            ..Match::default()
        },
        &[leaf(3), leaf(2), leaf(1)],
    )
    .unwrap();
    check_shell(
        |v| New {
            injections: [("metadata".into(), v[0].clone())].into(),
            ..New::default()
        },
        &[Par::default().with_locally_free(vec![7, 128])],
    )
    .unwrap();
}

#[test]
fn incomplete_slot_order_and_presence_mutants_change_bytes() {
    let send = |v: &[Par]| Send {
        data: v.to_vec(),
        ..Send::default()
    };
    let actual = check_shell(send, &[leaf(7), leaf(9)]).unwrap();
    for values in [vec![leaf(9), leaf(7)], vec![leaf(7), leaf(7)], vec![leaf(
        7,
    )]] {
        let mutant = check_shell(send, &values).unwrap();
        assert_ne!(actual.compact_snapshot(), mutant.compact_snapshot());
    }
    let absent = check_shell(|_| If::default(), &[]).unwrap();
    let present = check_shell(
        |v| If {
            condition: Some(v[0].clone()),
            ..If::default()
        },
        &[Par::default()],
    )
    .unwrap();
    assert_ne!(absent.compact_snapshot(), present.compact_snapshot());
}

proptest::proptest! {
    #[test]
    fn incomplete_plans_preserve_arbitrary_ordered_children(
        values in proptest::collection::vec(proptest::prelude::any::<i64>(), 0..12),
        metadata in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..260),
    ) {
        let children: Vec<_> = values.into_iter().map(|value| leaf(value).with_locally_free(metadata.clone())).collect();
        check_shell(|v| Send { data: v.to_vec(), locally_free: metadata.clone(), ..Send::default() }, &children).unwrap();
    }
}

#[test]
fn incomplete_mixed_family_construction_keeps_handles_on_a_fixed_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for depth in [256usize, 1024, 4096, 8192] {
                let mut arena = ParConstruction::new();
                let mut child = arena.import_par(&leaf(7)).unwrap();
                for level in 0..depth {
                    let component = match level % 4 {
                        0 => arena.incomplete_component(
                            &Send {
                                data: vec![Par::default()],
                                ..Send::default()
                            },
                            &[child],
                        ),
                        1 => arena.incomplete_component(
                            &If {
                                condition: Some(Par::default()),
                                ..If::default()
                            },
                            &[child],
                        ),
                        2 => arena.incomplete_component(
                            &Expr {
                                expr_instance: Some(ExprInstance::EPlusBody(rho::EPlus {
                                    p1: Some(Par::default()),
                                    p2: None,
                                })),
                            },
                            &[child],
                        ),
                        _ => arena.incomplete_component(
                            &Match {
                                cases: vec![rho::MatchCase {
                                    pattern: Some(Par::default()),
                                    ..rho::MatchCase::default()
                                }],
                                ..Match::default()
                            },
                            &[child],
                        ),
                    }
                    .unwrap()
                    .unwrap();
                    child = arena
                        .singleton_component(component, Vec::new(), false)
                        .unwrap();
                }
                let refs = arena
                    .protobuf_nodes
                    .iter()
                    .flat_map(|node| &node.pieces)
                    .filter(|piece| matches!(piece, ProtoPiece::Par { .. }))
                    .count();
                assert_eq!(refs, depth);
                let bytes: usize = arena
                    .protobuf_nodes
                    .iter()
                    .flat_map(|node| &node.pieces)
                    .map(|piece| match piece {
                        ProtoPiece::Bytes(bytes) => bytes.len(),
                        ProtoPiece::SharedBytes(bytes) => bytes.len(),
                        _ => 0,
                    })
                    .sum();
                assert!(
                    bytes <= depth * 32 + 64,
                    "descendant bodies were copied: {bytes} at {depth}"
                );
                assert!(arena.protobuf_nodes.len() <= depth * 4 + 4);
                assert_eq!(arena.components.len(), depth + 1);
                let expected = arena.protobuf_bytes(child);
                let actual = arena.finish(child).unwrap();
                assert_eq!(actual.protobuf_bytes().unwrap(), expected);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn incomplete_import_shares_body_and_preserves_canonical_image() {
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }]);
    let input = Send {
        data: vec![Par::default(), child],
        locally_free: vec![5],
        connective_used: true,
        ..Send::default()
    };
    let shallow = Send {
        data: vec![Par::default(), Par::default()],
        ..input.clone()
    };
    let source = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&input));
    let pointer = source.as_bytes().as_ptr();
    let mut arena = ParConstruction::new();
    let prior = arena
        .import_par(&Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(42)),
        }]))
        .unwrap();
    let prior_bytes = arena.finish(prior).unwrap();
    let component = arena
        .import_admitted_incomplete_component_region(&shallow, source.clone())
        .unwrap()
        .unwrap();
    let node = &arena.protobuf_nodes[arena.components[component.0].protobuf];
    let [ProtoPiece::SharedBytes(bytes)] = node.pieces.as_slice() else {
        panic!("one shared body leaf")
    };
    assert_eq!(bytes.as_ptr(), pointer);
    assert_eq!(bytes.as_ref().as_ref(), source.as_bytes());
    assert!(std::mem::size_of::<ProtoPiece>() <= std::mem::size_of::<(usize, usize, usize)>());
    drop(source);
    let count = Multiplicity::new(3).unwrap();
    let planned = arena.par([(component, count)], vec![5], true).unwrap();
    let actual = arena.finish(planned).unwrap();
    let mut expected = Par::default();
    expected
        .insert_component_many(&input, count, vec![5], true)
        .unwrap();
    assert_eq!(
        actual.components().compact_bytes(),
        expected.components().compact_bytes()
    );
    assert_eq!(
        protobuf_encoder::encode_to_vec(&actual),
        protobuf_encoder::encode_to_vec(&expected)
    );
    assert_eq!(arena.finish(prior).unwrap(), prior_bytes);
}

#[cfg(feature = "protobuf-projection-experiment")]
#[test]
fn incomplete_planning_has_no_resident_finish_events() {
    use super::substitution_cost::{capture, Metric};
    let mut arena = ParConstruction::new();
    let mut child = arena.import_par(&leaf(7)).unwrap();
    let (_, planning) = capture(|| {
        for _ in 0..4096 {
            let component = arena
                .incomplete_component(
                    &Send {
                        data: vec![Par::default()],
                        ..Send::default()
                    },
                    &[child],
                )
                .unwrap()
                .unwrap();
            child = arena
                .singleton_component(component, Vec::new(), false)
                .unwrap();
        }
    });
    for metric in [
        Metric::FinishAttempts,
        Metric::FinishSuccesses,
        Metric::ResidentBytes,
        Metric::RootKeys,
        Metric::RootKeyBytes,
    ] {
        assert_eq!(planning.get(metric), 0);
    }
    let (actual, emitted) = capture(|| arena.finish(child).unwrap());
    assert_eq!(emitted.get(Metric::FinishAttempts), 1);
    assert_eq!(emitted.get(Metric::FinishSuccesses), 1);
    assert_eq!(emitted.get(Metric::RootKeys), 1);
    assert_eq!(emitted.get(Metric::ResidentBytes), 0);
    assert_eq!(
        actual.protobuf_bytes().unwrap(),
        arena.protobuf_bytes(child)
    );
}

#[test]
fn complete_shell_declines_opaque_import_without_arena_mutation() {
    let input = Send {
        chan: Some(Par::default()),
        ..Send::default()
    };
    let region = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&input));
    let mut arena = ParConstruction::new();
    let before = (
        arena.components.len(),
        arena.protobuf_nodes.len(),
        arena.scores.len(),
    );
    assert!(arena
        .import_admitted_incomplete_component_region(&input, region)
        .unwrap()
        .is_none());
    assert_eq!(
        (
            arena.components.len(),
            arena.protobuf_nodes.len(),
            arena.scores.len()
        ),
        before
    );
}
