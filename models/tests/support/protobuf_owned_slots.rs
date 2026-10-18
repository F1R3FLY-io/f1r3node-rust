use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use prost::Message;

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{self, Expr, Par};
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::par_construction::{ParConstruction, ProtobufParBuilder};
use crate::rust::rholang::protobuf_decoder::{self as decoder, owned_slots as owned};
use crate::rust::rholang::schema_meta_tables::SCHEMA_CHILDREN;
use crate::rust::rholang::schema_test_adapters;

#[derive(Default)]
struct Materialize {
    arena: ParConstruction,
    visited: usize,
    fail_at: Option<usize>,
    bodies: Vec<Vec<u8>>,
}

impl owned::Projection for Materialize {
    type Error = &'static str;

    fn par(&mut self, value: ProtobufParBuilder) -> Result<Par, Self::Error> {
        self.visited += 1;
        if self.fail_at == Some(self.visited) {
            return Err("injected projection failure");
        }
        let plan = self.arena.freeze_protobuf_par(value).unwrap();
        let par = self.arena.finish(plan).unwrap();
        self.bodies.push(par.encode_to_vec());
        Ok(par)
    }

    fn epathmap(&mut self, value: owned::EPathMap) -> Result<rhoapi::EPathMap, Self::Error> {
        assert!(matches!(value.repr.as_repr(), EPathMapRepr::Empty));
        assert!(value.locally_free.is_empty());
        assert!(!value.connective_used);
        assert!(value.remainder.is_none());
        Ok(rhoapi::EPathMap::default())
    }
}

fn add(context: &mut Materialize, slot: &mut ProtobufParBuilder, value: i64) {
    let component = context
        .arena
        .import_component(&Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        })
        .unwrap();
    context
        .arena
        .insert_protobuf_component(slot, component)
        .unwrap();
}

fn integer(context: &mut Materialize, value: i64) -> ProtobufParBuilder {
    let mut slot = ProtobufParBuilder::default();
    add(context, &mut slot, value);
    slot
}

fn resident(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

#[test]
fn every_descriptor_field_and_oneof_arm_projects_without_canonicalizing_its_owner() {
    let count =
        schema_test_adapters::owned_slot_projection_cases(&mut Materialize::default()).unwrap();
    assert!(count > 200, "only {count} descriptor witnesses");
}

#[test]
fn owned_rank_witness_covers_every_schema_edge_and_the_epm_native_remainder() {
    let rows: BTreeMap<_, _> = owned::OWNERSHIP_RANKS
        .iter()
        .map(|(name, rank, children)| (name.rsplit('.').next().unwrap(), (*rank, *children)))
        .collect();
    assert_eq!(rows.len(), SCHEMA_CHILDREN.len());
    assert_eq!(rows.len(), owned::OWNERSHIP_RANKS.len());
    for (name, children) in SCHEMA_CHILDREN {
        let (rank, owned_children) = rows[name];
        let owned_names: Vec<_> = owned_children
            .iter()
            .map(|child| child.rsplit('.').next().unwrap())
            .collect();
        assert_eq!(&owned_names, children);
        assert_eq!(
            rank,
            children
                .iter()
                .map(|child| rows[child].0 + 1)
                .max()
                .unwrap_or(0)
        );
        for child in children.iter() {
            assert!(rows[child].0 < rank);
        }
    }
    assert_eq!(owned::EPM_REMAINDER_OWNERSHIP_RANK, rows["Var"].0);
    let mut pending = vec!["Var"];
    let mut seen = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name) {
            continue;
        }
        assert!(!["Par", "EPathMap"].contains(&name));
        pending.extend(
            rows[name]
                .1
                .iter()
                .map(|child| child.rsplit('.').next().unwrap()),
        );
    }
}

#[test]
fn optional_take_and_reopen_preserve_presence_and_accumulated_children() {
    let mut context = Materialize::default();
    let absent = owned::Send::default().project(&mut context).unwrap();
    assert!(absent.chan.is_none());
    let explicit = owned::Send {
        chan: Some(Default::default()),
        ..Default::default()
    }
    .project(&mut context)
    .unwrap();
    assert_eq!(explicit.chan, Some(Par::default()));
    let mut send = owned::Send {
        chan: Some(integer(&mut context, 11)),
        ..Default::default()
    };
    let mut reopened = send.chan.take().unwrap_or_default();
    assert!(send.chan.is_none());
    add(&mut context, &mut reopened, 22);
    send.chan = Some(reopened);
    let result = send.project(&mut context).unwrap().chan.unwrap();
    assert_eq!(result.family_cardinality::<Expr>(), 2);
    for value in [11, 22] {
        assert!(result.contains(&Expr {
            expr_instance: Some(ExprInstance::GInt(value))
        }));
    }
}

#[test]
fn projection_failure_stops_before_later_children_and_does_not_publish_a_result() {
    let mut context = Materialize {
        fail_at: Some(2),
        ..Default::default()
    };
    let owned = owned::Send {
        chan: Some(integer(&mut context, 1)),
        data: vec![integer(&mut context, 2), integer(&mut context, 3)],
        ..Default::default()
    };
    let mut destination = rhoapi::Send {
        chan: Some(resident(99)),
        ..Default::default()
    };
    let before = destination.encode_to_vec();
    let result = owned.project(&mut context).map(|value| destination = value);
    assert_eq!(result, Err("injected projection failure"));
    assert_eq!(context.visited, 2);
    assert_eq!(destination.encode_to_vec(), before);
}

#[test]
fn message_oneof_reopens_same_arm_but_discards_a_different_arm() {
    for same_arm in [false, true] {
        let mut bytes = Expr {
            expr_instance: Some(if same_arm {
                ExprInstance::EPlusBody(rhoapi::EPlus {
                    p1: Some(resident(11)),
                    p2: None,
                })
            } else {
                ExprInstance::EMinusBody(rhoapi::EMinus {
                    p1: Some(resident(11)),
                    p2: None,
                })
            }),
        }
        .encode_to_vec();
        bytes.extend(
            Expr {
                expr_instance: Some(ExprInstance::EPlusBody(rhoapi::EPlus {
                    p1: None,
                    p2: Some(resident(22)),
                })),
            }
            .encode_to_vec(),
        );
        let actual = decoder::decode_expr(bytes.as_slice()).unwrap();
        let reference =
            crate::protobuf_decoder_oracle::legacy(|| decoder::decode_expr(bytes.as_slice()))
                .unwrap();
        assert_eq!(actual.encode_to_vec(), reference.encode_to_vec());
        let Some(ExprInstance::EPlusBody(sum)) = actual.expr_instance else {
            panic!("wrong oneof arm");
        };
        assert_eq!(sum.p1, same_arm.then(|| resident(11)));
        assert_eq!(sum.p2, Some(resident(22)));
        let mut context = Materialize::default();
        let mut state = owned::Expr {
            expr_instance: Some(if same_arm {
                owned::ExprExprInstance::EPlusBody(owned::EPlus {
                    p1: Some(integer(&mut context, 11)),
                    p2: None,
                })
            } else {
                owned::ExprExprInstance::EMinusBody(owned::EMinus {
                    p1: Some(integer(&mut context, 11)),
                    p2: None,
                })
            }),
        };
        let mut child = match state.expr_instance.take() {
            Some(owned::ExprExprInstance::EPlusBody(child)) => child,
            _ => owned::EPlus::default(),
        };
        assert!(state.expr_instance.is_none());
        child.p2 = Some(integer(&mut context, 22));
        state.expr_instance = Some(owned::ExprExprInstance::EPlusBody(child));
        assert_eq!(
            state.project(&mut context).unwrap().encode_to_vec(),
            reference.encode_to_vec()
        );
    }
}

#[test]
fn tagged_continuation_projects_body_before_guard_in_protobuf_order() {
    let mut context = Materialize::default();
    let value = owned::TaggedContinuation {
        guard: Some(integer(&mut context, 22)),
        tagged_cont: Some(owned::TaggedContinuationTaggedCont::ParBody(
            owned::ParWithRandom {
                body: Some(integer(&mut context, 11)),
                random_state: vec![4, 3, 2, 1],
            },
        )),
        ..Default::default()
    };
    let result = value.project(&mut context).unwrap();
    assert_eq!(context.bodies, [
        resident(11).encode_to_vec(),
        resident(22).encode_to_vec()
    ]);
    assert_eq!(result.guard, Some(resident(22)));
    let Some(rhoapi::tagged_continuation::TaggedCont::ParBody(body)) = result.tagged_cont else {
        panic!("wrong continuation arm");
    };
    assert_eq!(body.body, Some(resident(11)));
    assert_eq!(body.random_state, [4, 3, 2, 1]);
}

#[test]
fn keyed_entry_defaults_and_failure_keep_the_existing_parent_binding() {
    let field = |tag, bytes: &[u8]| {
        let mut out = Vec::new();
        prost::encoding::bytes::encode(tag, &bytes.to_vec(), &mut out);
        out
    };
    let mut entry = field(2, &resident(11).encode_to_vec());
    entry.extend(field(2, &resident(22).encode_to_vec()));
    let bytes = field(4, &entry);
    let actual = decoder::decode_new(bytes.as_slice()).unwrap();
    let reference =
        crate::protobuf_decoder_oracle::legacy(|| decoder::decode_new(bytes.as_slice())).unwrap();
    assert_eq!(actual.encode_to_vec(), reference.encode_to_vec());
    assert_eq!(actual.injections[""].family_cardinality::<Expr>(), 2);
    let mut replaced = bytes;
    replaced.extend(field(4, &[]));
    replaced.extend(field(4, &field(1, b"key-only")));
    let result = decoder::decode_new(replaced.as_slice()).unwrap();
    assert_eq!(result.injections.len(), 2);
    assert_eq!(result.injections[""], Par::default());
    assert_eq!(result.injections["key-only"], Par::default());
    let mut entry = field(1, b"existing");
    entry.extend(field(2, &resident(33).encode_to_vec()));
    entry.extend([0x12, 0xff]);
    let failed = field(4, &entry);
    let observe = || {
        let mut parent = rhoapi::New {
            injections: [("existing".into(), resident(99))].into(),
            ..Default::default()
        };
        let before = parent.encode_to_vec();
        let mut input = failed.as_slice();
        let result = parent.merge(&mut input);
        assert!(result.is_err());
        assert_eq!(parent.encode_to_vec(), before);
        (result, input.len(), before)
    };
    assert_eq!(crate::protobuf_decoder_oracle::legacy(observe), observe());
}

proptest! {
    #[test]
    fn repeated_children_and_keyed_replacements_preserve_values_and_order(
        values in prop::collection::vec(any::<i64>(), 0..40),
        entries in prop::collection::vec((0u8..8, any::<i64>()), 0..40),
    ) {
        let mut context = Materialize::default();
        let owned = owned::ListParWithRandom {
            pars: values.iter().map(|value| integer(&mut context, *value)).collect(),
            random_state: vec![4, 3, 2, 1],
            ..Default::default()
        };
        let result = owned.project(&mut context).unwrap();
        prop_assert_eq!(result.pars, values.iter().map(|value| resident(*value)).collect::<Vec<_>>());
        prop_assert_eq!(result.random_state, vec![4, 3, 2, 1]);
        let mut owned = owned::New::default();
        let mut expected = BTreeMap::new();
        for (key, value) in entries {
            owned.injections.insert(key.to_string(), integer(&mut context, value));
            expected.insert(key.to_string(), resident(value));
        }
        prop_assert_eq!(owned.project(&mut context).unwrap().injections, expected);
    }
}
