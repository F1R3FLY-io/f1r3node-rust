use proptest::prelude::*;

use super::*;
use crate::rhoapi as rho;
use crate::rust::rholang::protobuf_decoder::{self as decoder, owned_slots as owned};

fn leaf(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn slot(arena: &mut ParConstruction, value: &Par) -> ProtobufParBuilder {
    let handle = arena.import_par(value).unwrap();
    let node = &arena.pars[handle.0];
    ProtobufParBuilder {
        occurrence_root: node.occurrence_root,
        locally_free: node.locally_free.clone(),
        connective_used: node.connective_used,
    }
}

fn integer(arena: &mut ParConstruction, value: i64) -> ProtobufParBuilder {
    slot(arena, &leaf(value))
}

struct Materialize<'a>(&'a mut ParConstruction);

impl owned::Projection for Materialize<'_> {
    type Error = ParConstructionError;

    fn par(&mut self, value: ProtobufParBuilder) -> Result<Par, Self::Error> {
        let plan = self.0.freeze_protobuf_par(value)?;
        self.0.finish(plan)
    }

    fn epathmap(&mut self, _: owned::EPathMap) -> Result<rho::EPathMap, Self::Error> {
        unreachable!("EPM fixtures use an independent resident reference")
    }
}

fn check<T: ParBagComponent>(
    arena: &mut ParConstruction,
    component: PlannedComponent,
    expected: &T,
) {
    let root = arena
        .par([(component, Multiplicity::ONE)], Vec::new(), false)
        .unwrap();
    let actual = arena.finish(root).unwrap();
    let body = protobuf_encoder::encode_to_vec(expected);
    let mut input = Vec::new();
    prost::encoding::encode_key(
        T::TAG.byte() as u32,
        prost::encoding::WireType::LengthDelimited,
        &mut input,
    );
    prost::encoding::encode_varint(body.len() as u64, &mut input);
    input.extend_from_slice(&body);
    let reference =
        crate::protobuf_decoder_oracle::legacy(|| decoder::decode_par(input.as_slice())).unwrap();
    assert_eq!(actual.compact_snapshot(), reference.compact_snapshot());
    assert_eq!(
        actual.protobuf_bytes().unwrap(),
        reference.protobuf_bytes().unwrap()
    );
    assert_eq!(
        arena.protobuf_bytes(root),
        reference.protobuf_bytes().unwrap()
    );
}

macro_rules! case {
    ($make:expr) => {{
        let make = $make;
        let mut arena = ParConstruction::new();
        let expected = make(&mut arena)
            .project(&mut Materialize(&mut arena))
            .unwrap();
        let component = make(&mut arena).plan(&mut arena).unwrap();
        check(&mut arena, component, &expected);
    }};
}

#[test]
fn all_component_families_match_frozen_attachment_for_complete_and_incomplete_shells() {
    case!(|_: &mut ParConstruction| owned::Send::default());
    case!(|_: &mut ParConstruction| owned::Receive::default());
    case!(|_: &mut ParConstruction| owned::New::default());
    case!(|_: &mut ParConstruction| owned::Expr::default());
    case!(|_: &mut ParConstruction| owned::Match::default());
    case!(|_: &mut ParConstruction| owned::GUnforgeable::default());
    case!(|_: &mut ParConstruction| owned::Bundle::default());
    case!(|_: &mut ParConstruction| owned::Connective::default());
    case!(|_: &mut ParConstruction| owned::If::default());
    case!(|a: &mut ParConstruction| owned::Send {
        chan: Some(integer(a, 11)),
        data: vec![integer(a, 22), integer(a, 33)],
        persistent: true,
        connective_used: true,
        locally_free: vec![5],
        cost_region: None,
    });
    case!(|a: &mut ParConstruction| owned::New {
        p: Some(integer(a, 11)),
        uri: vec!["z".into(), "a".into()],
        injections: [("z".into(), integer(a, 22)), ("a".into(), integer(a, 33))].into(),
        bind_count: 2,
        locally_free: vec![5],
    });
    case!(|a: &mut ParConstruction| owned::Bundle {
        body: Some(integer(a, 11)),
        read_flag: true,
        write_flag: false,
    });
    case!(|a: &mut ParConstruction| owned::If {
        condition: Some(integer(a, 11)),
        if_true: Some(integer(a, 22)),
        if_false: Some(integer(a, 33)),
        connective_used: true,
        locally_free: vec![5],
    });
    case!(|a: &mut ParConstruction| owned::Connective {
        connective_instance: Some(owned::ConnectiveConnectiveInstance::ConnAndBody(
            owned::ConnectiveBody {
                ps: vec![integer(a, 33), integer(a, 11), integer(a, 22)],
            }
        )),
    });
    case!(|_: &mut ParConstruction| owned::GUnforgeable {
        unf_instance: Some(owned::GUnforgeableUnfInstance::GPrivateBody(
            owned::GPrivate { id: vec![1, 2, 3] }
        )),
    });
}

proptest! {
    #[test]
    fn method_order_and_incomplete_lane_match_frozen_keys(
        target in any::<i64>(), arguments in prop::collection::vec(any::<i64>(), 0..16), present in any::<bool>(),
    ) {
        case!(|a: &mut ParConstruction| owned::Expr {
            expr_instance: Some(owned::ExprExprInstance::EMethodBody(owned::EMethod {
                target: present.then(|| integer(a, target)),
                arguments: arguments.iter().map(|&v| integer(a, v)).collect(),
                method_name: "method".into(), locally_free: vec![2], connective_used: true,
            })),
        });
    }

    #[test]
    fn ordered_and_commutative_collections_match_frozen_attachment(
        values in prop::collection::vec(-4i64..5, 0..12), complete in any::<bool>(),
    ) {
        case!(|a: &mut ParConstruction| owned::Expr {
            expr_instance: Some(owned::ExprExprInstance::EListBody(owned::EList {
                ps: values.iter().map(|&v| integer(a, v)).collect(), ..Default::default()
            })),
        });
        case!(|a: &mut ParConstruction| owned::Expr {
            expr_instance: Some(owned::ExprExprInstance::ETupleBody(owned::ETuple {
                ps: values.iter().map(|&v| integer(a, v)).collect(), ..Default::default()
            })),
        });
        case!(|a: &mut ParConstruction| owned::Expr {
            expr_instance: Some(owned::ExprExprInstance::ESetBody(owned::ESet {
                ps: values.iter().map(|&v| integer(a, v)).collect(), ..Default::default()
            })),
        });
        case!(|a: &mut ParConstruction| owned::Expr {
            expr_instance: Some(owned::ExprExprInstance::EMapBody(owned::EMap {
                kvs: values.iter().enumerate().map(|(index, &v)| owned::KeyValuePair {
                    key: complete.then(|| integer(a, v)), value: Some(integer(a, index as i64)),
                }).collect(), ..Default::default()
            })),
        });
        case!(|a: &mut ParConstruction| owned::Send {
            chan: complete.then(|| integer(a, 11)),
            data: values.iter().map(|&v| integer(a, v)).collect(), ..Default::default()
        });
        case!(|a: &mut ParConstruction| owned::New {
            p: complete.then(|| integer(a, 11)),
            injections: values.iter().enumerate().map(|(index, &v)| (index.to_string(), integer(a, v))).collect(),
            uri: vec!["z".into(), "a".into()], ..Default::default()
        });
    }

    #[test]
    fn guard_and_condition_completion_preserve_frontier_semantics(
        choices in prop::collection::vec(0u8..5, 0..12), body_present in any::<bool>(),
    ) {
        let guard = |a: &mut ParConstruction, choice| match choice {
            0 => None,
            1 => Some(ProtobufParBuilder::default()),
            2 => Some(ProtobufParBuilder { locally_free: vec![4], ..Default::default() }),
            3 => Some(ProtobufParBuilder { connective_used: true, ..Default::default() }),
            _ => Some(integer(a, 77)),
        };
        for choice in 0..5 {
            case!(|a: &mut ParConstruction| owned::Receive {
                binds: vec![owned::ReceiveBind { patterns: vec![integer(a, 11), integer(a, 22)],
                    source: Some(integer(a, 33)), ..Default::default() }],
                body: body_present.then(|| integer(a, 44)), condition: guard(a, choice),
                persistent: true, bind_count: 2, ..Default::default()
            });
        }
        case!(|a: &mut ParConstruction| owned::Match {
            target: body_present.then(|| integer(a, 11)),
            cases: choices.iter().map(|&choice| owned::MatchCase {
                pattern: Some(integer(a, 22)), source: Some(integer(a, 33)),
                guard: guard(a, choice), free_count: 1,
            }).collect(), ..Default::default()
        });
    }
}

#[test]
fn captured_epm_bounds_and_complete_shape_are_certified() {
    assert_eq!(owned::COMPONENT_EPM_CUT_BOUNDS.len(), 9);
    for &(name, bound) in owned::COMPONENT_EPM_CUT_BOUNDS {
        assert_eq!(bound, usize::from(name == "Expr"));
    }
    for instance in [
        ExprInstance::EPathmapBody(rho::EPathMap::default()),
        ExprInstance::EZipperBody(rho::EZipper {
            pathmap: Some(rho::EPathMap::default()),
            ..Default::default()
        }),
    ] {
        assert!(<Expr as ParBagComponent>::semantic_shape_is_complete(
            &Expr {
                expr_instance: Some(instance)
            }
        ));
    }
    case!(|_: &mut ParConstruction| owned::Expr {
        expr_instance: Some(owned::ExprExprInstance::EZipperBody(owned::EZipper {
            current_path: vec![vec![3, 2]],
            is_write_zipper: true,
            cursor_kind: 2,
            ..Default::default()
        })),
    });
}

#[test]
fn epm_and_ezipper_attachment_preserve_topology_payloads_and_distinct_metadata() {
    for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
        for valued in [false, true] {
            for zipper in [false, true] {
                let mut arena = ParConstruction::new();
                let key = crate::rust::canonical_path::encode_trie_path(&leaf(42));
                let payload = leaf(91);
                let handle = arena.import_par(&payload).unwrap();
                let (source, reference) = match mode {
                    EPathMapMode::Empty => (EPathMapRepr::Empty, EPathMapRepr::Empty),
                    EPathMapMode::Set => {
                        let mut map = PathMap::new();
                        map.create_path([255, 0, 1]);
                        map.create_path([255, 0, 2]);
                        if valued {
                            map.set_val_at(&key, ());
                        }
                        (EPathMapRepr::Set(map.clone()), EPathMapRepr::Set(map))
                    }
                    EPathMapMode::Map => {
                        let mut source = PathMap::new();
                        let mut reference = PathMap::new();
                        for path in [[255, 0, 1], [255, 0, 2]] {
                            source.create_path(path);
                            reference.create_path(path);
                        }
                        if valued {
                            source.set_val_at(&key, handle);
                            reference.set_val_at(&key, payload);
                        }
                        (EPathMapRepr::Map(source), EPathMapRepr::Map(reference))
                    }
                };
                let prefix = epathmap_trie_codec::layout(&source).prefix().to_vec();
                let remainder = Some(rho::Var {
                    var_instance: Some(rho::var::VarInstance::FreeVar(3)),
                });
                let pathmap = owned::EPathMap {
                    repr: arena.own_epm_repr(source),
                    locally_free: vec![4],
                    connective_used: true,
                    remainder: remainder.clone(),
                };
                let mut expected = rho::EPathMap::default();
                expected.replace_decoded_representation(reference).unwrap();
                expected.locally_free = vec![4];
                expected.connective_used = true;
                expected.remainder = remainder;
                let (source, expected) = if zipper {
                    (
                        owned::ExprExprInstance::EZipperBody(owned::EZipper {
                            pathmap: Some(pathmap),
                            current_path: vec![vec![3, 84], vec![0]],
                            is_write_zipper: true,
                            locally_free: vec![8],
                            connective_used: false,
                            cursor_kind: 2,
                        }),
                        ExprInstance::EZipperBody(rho::EZipper {
                            pathmap: Some(expected),
                            current_path: vec![vec![3, 84], vec![0]],
                            is_write_zipper: true,
                            locally_free: vec![8],
                            connective_used: false,
                            cursor_kind: 2,
                        }),
                    )
                } else {
                    (
                        owned::ExprExprInstance::EPathmapBody(pathmap),
                        ExprInstance::EPathmapBody(expected),
                    )
                };
                let component = owned::Expr {
                    expr_instance: Some(source),
                }
                .plan(&mut arena)
                .unwrap();
                assert_eq!(&*arena.epathmaps.last().unwrap().prefix, prefix);
                check(&mut arena, component, &Expr {
                    expr_instance: Some(expected),
                });
            }
        }
    }
}

#[test]
fn deep_mixed_attachment_uses_flat_plans_without_resident_children() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let build = || {
                let mut arena = ParConstruction::new();
                let mut value = ProtobufParBuilder::default();
                for level in 0..8192 {
                    let component = if level % 3 == 0 {
                        owned::Expr {
                            expr_instance: Some(owned::ExprExprInstance::ENotBody(owned::ENot {
                                p: Some(value),
                            })),
                        }
                        .plan(&mut arena)
                        .unwrap()
                    } else if level % 3 == 1 {
                        owned::Send {
                            chan: Some(value),
                            ..Default::default()
                        }
                        .plan(&mut arena)
                        .unwrap()
                    } else {
                        let child = arena.freeze_protobuf_par(value).unwrap();
                        let mut map = PathMap::new();
                        map.set_val_at([3, 2], child);
                        let repr = arena.own_epm_repr(EPathMapRepr::Map(map));
                        owned::Expr {
                            expr_instance: Some(owned::ExprExprInstance::EPathmapBody(
                                owned::EPathMap {
                                    repr,
                                    ..Default::default()
                                },
                            )),
                        }
                        .plan(&mut arena)
                        .unwrap()
                    };
                    value = ProtobufParBuilder::default();
                    arena
                        .insert_protobuf_component(&mut value, component)
                        .unwrap();
                }
                assert_eq!(arena.components.len(), 8192);
                assert_eq!(arena.pars.len(), 8192);
                let root = arena.freeze_protobuf_par(value).unwrap();
                (arena, root)
            };
            #[cfg(feature = "protobuf-projection-experiment")]
            let (arena, root) = {
                let (built, totals) = substitution_cost::capture(build);
                assert_eq!(totals.get(substitution_cost::Metric::FinishAttempts), 0);
                assert_eq!(totals.get(substitution_cost::Metric::ResidentBytes), 0);
                built
            };
            #[cfg(not(feature = "protobuf-projection-experiment"))]
            let (arena, root) = build();
            let resident = arena.finish(root).unwrap();
            assert_eq!(
                resident.protobuf_bytes().unwrap(),
                arena.protobuf_bytes(root)
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
