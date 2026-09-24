use std::panic::{catch_unwind, AssertUnwindSafe};

use proptest::prelude::*;

use super::*;
use crate::rust::rholang::protobuf_decoder::owned_slots as owned;

fn small_stack(f: impl FnOnce() + std::marker::Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn public_materialization_preserves_topology_values_and_metadata() {
    small_stack(|| {
        let mut arena = ParConstruction::new();
        let mut child = arena.empty_par().unwrap();
        let shell = Expr {
            expr_instance: Some(ExprInstance::ENotBody(crate::rhoapi::ENot {
                p: Some(Par::default()),
            })),
        };
        for _ in 0..8192 {
            let component = arena.expr_component(&shell, &[child], false).unwrap();
            child = arena
                .singleton_component(component, Vec::new(), false)
                .unwrap();
        }
        let key = arena.empty_par().unwrap();
        let encoded_key = arena.canonical_path_bytes(key).unwrap();
        let expected_value = arena.finish(child).unwrap();
        for mode in [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map] {
            let mut source = if mode == EPathMapMode::Empty {
                OwnedPlannedEPathMap::default()
            } else {
                comb(&mut arena, mode, false)
            };
            match source.as_repr_mut() {
                EPathMapRepr::Empty => {}
                EPathMapRepr::Set(map) => {
                    map.insert(&encoded_key, ());
                }
                EPathMapRepr::Map(map) => {
                    map.insert(&encoded_key, child);
                }
            }
            let expected_topology = epathmap_trie_codec::layout(source.as_repr())
                .prefix()
                .to_vec();
            #[cfg(feature = "protobuf-projection-experiment")]
            let mapped = {
                let (mapped, totals) =
                    substitution_cost::capture(|| arena.finish_epm_repr(source).unwrap());
                assert_eq!(
                    totals.get(substitution_cost::Metric::FinishAttempts),
                    usize::from(mode == EPathMapMode::Map)
                );
                mapped
            };
            #[cfg(not(feature = "protobuf-projection-experiment"))]
            let mapped = arena.finish_epm_repr(source).unwrap();
            assert_eq!(
                epathmap_trie_codec::layout(mapped.as_repr()).prefix(),
                expected_topology
            );
            let mut public = EPathMap::default();
            public.locally_free = vec![2];
            public.connective_used = true;
            public.remainder = Some(crate::rhoapi::Var::default());
            public
                .replace_owned_decoded_representation_with(mapped, true)
                .unwrap();
            assert_eq!(public.mode(), mode);
            assert_eq!(public.locally_free, vec![2]);
            assert!(public.connective_used);
            assert!(public.remainder.is_some());
            if mode == EPathMapMode::Map {
                assert_eq!(
                    public
                        .entry_trie()
                        .get_map_value_by_encoded_key(&encoded_key)
                        .unwrap(),
                    Some(&expected_value)
                );
            }
            let mut expected_snapshot = expected_topology;
            if mode == EPathMapMode::Map {
                let body = expected_value.protobuf_bytes().unwrap();
                prost::encoding::encode_varint(body.len() as u64, &mut expected_snapshot);
                expected_snapshot.extend_from_slice(&body);
            }
            assert_eq!(public.trie_snapshot(), expected_snapshot);
            let bytes = protobuf_encoder::encode_to_vec(&public);
            assert!(!bytes.is_empty());
            drop(public);
        }
    });
}

fn comb(arena: &mut ParConstruction, mode: EPathMapMode, valued: bool) -> OwnedPlannedEPathMap {
    let barrier = arena.epm_barrier();
    let repr = match mode {
        EPathMapMode::Set => EPathMapRepr::Set(PathMap::new()),
        EPathMapMode::Map => EPathMapRepr::Map(PathMap::new()),
        EPathMapMode::Empty => EPathMapRepr::Empty,
    };
    let mut owner = EpmReprOwner::new(repr, PlannedEpmBarrier(Some(barrier)));
    let mut key = Vec::new();
    for _ in 0..8192 {
        key.push(1);
        match owner.as_repr_mut() {
            EPathMapRepr::Set(map) => {
                if valued {
                    map.set_val_at(&key, ());
                } else {
                    map.create_path(&key);
                }
            }
            EPathMapRepr::Map(map) => {
                if valued {
                    map.set_val_at(&key, barrier);
                } else {
                    map.create_path(&key);
                }
            }
            EPathMapRepr::Empty => unreachable!(),
        }
        *key.last_mut().unwrap() = 0;
    }
    owner
}

proptest! {
    #[test]
    fn arena_barriers_preserve_validity_and_reuse(count in 0usize..64, growth in 0usize..64) {
        let mut arena = ParConstruction::new();
        for _ in 0..count { arena.empty_par().unwrap(); }
        let barrier = ArenaEpmBarrier(RefCell::new(&mut arena));
        let first = barrier.make_barrier();
        prop_assert_eq!(first.0, 0);
        prop_assert_eq!(barrier.0.borrow().pars.len(), count.max(1));
        prop_assert_eq!(barrier.make_barrier(), first);
        drop(barrier);
        for _ in 0..growth { arena.empty_par().unwrap(); }
        prop_assert_eq!(arena.epm_barrier(), first);
        prop_assert_eq!(arena.pars.len(), count.max(1) + growth);
        prop_assert!(arena.finish(first).unwrap().is_empty());
        for index in 0..count { prop_assert!(arena.finish(PlannedPar(index)).unwrap().is_empty()); }
    }
}

#[test]
fn planned_owners_retire_branching_sets_maps_and_topology() {
    small_stack(|| {
        for mode in [EPathMapMode::Set, EPathMapMode::Map] {
            for valued in [false, true] {
                let mut arena = ParConstruction::new();
                let owner = comb(&mut arena, mode, valued);
                let count = arena.pars.len();
                assert_eq!(owner.as_repr().mode(), mode);
                assert!(!owner.as_repr().is_empty());
                match owner.as_repr() {
                    EPathMapRepr::Set(map) => assert_eq!(map.read_zipper().to_next_val(), valued),
                    EPathMapRepr::Map(map) => assert_eq!(map.read_zipper().to_next_val(), valued),
                    EPathMapRepr::Empty => unreachable!(),
                }
                drop(owner);
                assert_eq!(arena.pars.len(), count);
            }
        }
    });
}

#[test]
fn raw_topology_ingress_preserves_topology_and_bootstraps_valid_barrier() {
    small_stack(|| {
        let mut source_arena = ParConstruction::new();
        let mut source = comb(&mut source_arena, EPathMapMode::Map, false);
        let raw = std::mem::take(source.as_repr_mut());
        let mut arena = ParConstruction::new();
        let owner = arena.own_epm_repr(raw);
        assert!(matches!(owner.as_repr(), EPathMapRepr::Map(_)));
        assert!(!owner.as_repr().is_empty());
        assert_eq!(arena.pars.len(), 1);
        drop(owner);
        let count = arena.pars.len();
        let owner = arena.own_epm_repr(EPathMapRepr::Map(PathMap::new()));
        assert_eq!(arena.pars.len(), count);
        drop(owner);
        let mut empty = ParConstruction::new();
        drop(empty.own_epm_repr(EPathMapRepr::Map(PathMap::new())));
        assert!(empty.pars.is_empty());
        let mut source = comb(&mut source_arena, EPathMapMode::Map, false);
        let expected_prefix = epathmap_trie_codec::layout(source.as_repr())
            .prefix()
            .to_vec();
        let raw = std::mem::take(source.as_repr_mut());
        let actual = empty
            .epathmap_repr_expr_component(raw, Vec::new(), false, None)
            .unwrap();
        let actual = empty
            .singleton_component(actual, Vec::new(), false)
            .unwrap();
        assert!(!empty.protobuf_bytes(actual).is_empty());
        assert_eq!(&*empty.epathmaps[0].prefix, expected_prefix);
        assert!(matches!(
            empty.epathmaps[0].repr.as_repr(),
            EPathMapRepr::Map(_)
        ));
    });
}

#[test]
fn builder_unwind_retires_partial_branching_keys() {
    small_stack(|| {
        for mode in [EPathMapMode::Set, EPathMapMode::Map] {
            let mut arena = ParConstruction::new();
            let value = arena.empty_par().unwrap();
            let mut children = Vec::new();
            let mut path = Vec::new();
            for _ in 0..2048 {
                path.push(1);
                let key = arena.empty_par().unwrap();
                arena.pars[key.0].canonical_path_override = Some(path.clone().into_boxed_slice());
                children.push(key);
                if mode == EPathMapMode::Map {
                    children.push(value);
                }
                *path.last_mut().unwrap() = 0;
            }
            children.push(PlannedPar(usize::MAX));
            if mode == EPathMapMode::Map {
                children.push(value);
            }
            assert!(
                catch_unwind(AssertUnwindSafe(|| arena.build_epm_entries(mode, &children)))
                    .is_err()
            );
            assert!(arena.finish(value).unwrap().is_empty());
        }
    });
}

#[test]
fn planned_length_refusal_preserves_error_and_arena_reuse() {
    let mut arena = ParConstruction::new();
    let key = arena.empty_par().unwrap();
    let value = arena.empty_par().unwrap();
    let before = arena.pars.len();
    let owner = arena
        .build_epm_entries(EPathMapMode::Map, &[key, value])
        .unwrap();
    assert_eq!(arena.pars.len(), before);
    arena.pars[value.0].protobuf_len = usize::MAX;
    let error = arena
        .planned_epathmap_repr(owner, &[], false, None)
        .unwrap_err();
    assert!(matches!(
        error,
        ParConstructionError::LengthOverflow("planned EPathMap snapshot")
    ));
    assert!(arena.epathmaps.is_empty());
    arena.pars[value.0].protobuf_len = 0;
    let component = arena
        .epathmap_expr_component(EPathMapMode::Map, &[key, value], Vec::new(), false, None)
        .unwrap();
    let root = arena
        .singleton_component(component, Vec::new(), false)
        .unwrap();
    assert_eq!(
        arena.finish(root).unwrap().protobuf_bytes().unwrap(),
        arena.protobuf_bytes(root)
    );
}

#[test]
fn generated_mirror_replacement_and_projection_failure_retire_owned_maps() {
    struct Reject;
    impl owned::Projection for Reject {
        type Error = &'static str;
        fn par(&mut self, _: ProtobufParBuilder) -> Result<Par, Self::Error> {
            Err("selected failure")
        }
        fn epathmap(&mut self, _: owned::EPathMap) -> Result<EPathMap, Self::Error> {
            Err("selected failure")
        }
    }
    small_stack(|| {
        let mut arena = ParConstruction::new();
        for mode in [EPathMapMode::Set, EPathMapMode::Map] {
            let mut frame = owned::EPathMap {
                repr: comb(&mut arena, EPathMapMode::Map, true),
                ..Default::default()
            };
            frame.repr = comb(&mut arena, mode, mode == EPathMapMode::Map);
            let result = frame.project(&mut Reject);
            assert!(matches!(result, Err("selected failure")));
        }
        assert_eq!(arena.pars.len(), 1);
    });
}

#[test]
fn arena_disposal_retires_planned_trie_nodes_on_small_stack() {
    small_stack(|| {
        let mut arena = ParConstruction::new();
        let owner = comb(&mut arena, EPathMapMode::Map, true);
        arena.epathmaps.push(PlannedEPathMapNode {
            repr: owner,
            prefix: Box::new([]),
            value_order: Box::new([]),
            snapshot_len: 0,
            snapshot_zero_count: 0,
            entries_stable: false,
        });
        drop(arena);
    });
}
