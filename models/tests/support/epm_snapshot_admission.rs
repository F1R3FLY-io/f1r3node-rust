use pathmap::PathMap;
use proptest::prelude::*;

use super::*;
use crate::rhoapi::{EPathMap, Expr, Par, Var};
use crate::rust::canonical_path::encode_trie_path;
use crate::rust::epathmap_trie_codec::{self, EPathMapMode, EPathMapRepr};

fn mode(index: u8) -> EPathMapMode {
    [EPathMapMode::Empty, EPathMapMode::Set, EPathMapMode::Map][usize::from(index)]
}

fn representation<V: Clone + Send + Sync + Unpin + 'static>(
    mode: EPathMapMode,
    keys: &[Vec<u8>],
    value: V,
) -> EPathMapRepr<V> {
    match mode {
        EPathMapMode::Empty => EPathMapRepr::Empty,
        EPathMapMode::Set => {
            let mut map = PathMap::new();
            for key in keys {
                map.insert(key, ());
            }
            EPathMapRepr::Set(map)
        }
        EPathMapMode::Map => {
            let mut map = PathMap::new();
            for key in keys {
                map.insert(key, value.clone());
            }
            EPathMapRepr::Map(map)
        }
    }
}

fn integer_key(n: i64) -> Vec<u8> {
    encode_trie_path(&Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(n)),
    }]))
}

fn compare(
    current: EPathMapMode,
    incoming: EPathMapMode,
    keys: &[Vec<u8>],
    validate: bool,
) -> Option<String> {
    let mut arena = ParConstruction::new();
    let value = arena.empty_par().unwrap();
    let old_keys = [integer_key(-10)];
    let mut planned = owned_slots::EPathMap {
        repr: arena.own_decoded_epm_repr(representation(current, &old_keys, value)),
        locally_free: vec![8],
        connective_used: true,
        remainder: Some(Var::default()),
    };
    let mut resident = EPathMap::default();
    resident.locally_free = vec![8];
    resident.connective_used = true;
    resident.remainder = Some(Var::default());
    resident
        .replace_decoded_representation(representation(current, &old_keys, Par::default()))
        .unwrap();
    let sibling = resident.clone();
    let before = resident.trie_snapshot().to_vec();
    let candidate = arena.own_decoded_epm_repr(representation(incoming, keys, value));
    let actual = planned.replace_decoded_representation(candidate, validate);
    let expected = if validate {
        resident.replace_decoded_representation(representation(incoming, keys, Par::default()))
    } else {
        resident.replace_decoded_representation_deferred_key_validation(representation(
            incoming,
            keys,
            Par::default(),
        ))
    };
    assert_eq!(actual.as_ref().err(), expected.as_ref().err());
    let actual_mode = planned.repr.as_repr().mode();
    let mapped = arena.finish_epm_repr(planned.repr).unwrap();
    assert_eq!(
        epathmap_trie_codec::encode(mapped.as_repr()),
        resident.trie_snapshot()
    );
    assert_eq!(actual_mode, resident.mode());
    if actual.is_err() {
        assert_eq!(resident.trie_snapshot(), before);
        assert_eq!(actual_mode, current);
    } else {
        assert_eq!(actual_mode, incoming);
    }
    assert_eq!(sibling.trie_snapshot(), before);
    assert_eq!(planned.locally_free, vec![8]);
    assert!(planned.connective_used);
    assert_eq!(planned.remainder, Some(Var::default()));
    assert_eq!(resident.locally_free, vec![8]);
    assert!(resident.connective_used);
    assert_eq!(resident.remainder, Some(Var::default()));
    actual.err().map(|error| error.to_string())
}

proptest! {
    #[test]
    fn planned_and_resident_admission_match_generated_keys(
        current in 0u8..3,
        incoming in 0u8..3,
        validate in any::<bool>(),
        entries in prop::collection::vec((any::<i64>(), prop::collection::vec(any::<u8>(), 0..16), any::<bool>()), 0..24),
    ) {
        let keys: Vec<_> = entries.into_iter().map(|(n, bytes, canonical)| {
            if canonical { integer_key(n) } else { bytes }
        }).collect();
        compare(mode(current), mode(incoming), &keys, validate);
    }
}

#[test]
fn all_modes_preserve_typed_empties_and_replace_instead_of_union() {
    use rspace_plus_plus::rspace::pathmap_mutation::BarrierProvider;

    let mut fresh = ParConstruction::new();
    let empty_map = fresh.own_decoded_epm_repr(EPathMapRepr::Map(PathMap::new()));
    assert_eq!(empty_map.as_repr().mode(), EPathMapMode::Map);
    assert!(fresh
        .finish(empty_map.barrier.make_barrier())
        .unwrap()
        .is_empty());
    drop(empty_map);
    for current in 0..3 {
        for incoming in 0..3 {
            let conflict = matches!((current, incoming), (1, 2) | (2, 1));
            for keys in [vec![], vec![integer_key(23), integer_key(41)]] {
                assert_eq!(
                    compare(mode(current), mode(incoming), &keys, true).is_some(),
                    conflict
                );
            }
        }
    }
}

#[test]
fn root_and_first_key_errors_preserve_mode_precedence_and_deferred_policy() {
    for incoming in [EPathMapMode::Set, EPathMapMode::Map] {
        let kind = if incoming == EPathMapMode::Set {
            "set"
        } else {
            "map"
        };
        let root = compare(EPathMapMode::Empty, incoming, &[vec![], vec![0xff]], true).unwrap();
        assert_eq!(
            root,
            format!("failed to decode Protobuf message: EPathMap {kind} key: EmptyInput")
        );
        let first = compare(EPathMapMode::Empty, incoming, &[vec![0xff], vec![3]], true).unwrap();
        assert_eq!(
            first,
            format!("failed to decode Protobuf message: EPathMap {kind} key: Truncated")
        );
        let opposite = if incoming == EPathMapMode::Set {
            EPathMapMode::Map
        } else {
            EPathMapMode::Set
        };
        let conflict = compare(opposite, incoming, &[vec![], vec![0xff]], true).unwrap();
        assert_eq!(conflict, format!("failed to decode Protobuf message: EPathMap trie_snapshot cannot change {opposite:?} storage to {incoming:?} storage"));
        assert!(compare(EPathMapMode::Empty, incoming, &[vec![0x0f, 0]], true).is_none());
        assert!(compare(EPathMapMode::Empty, incoming, &[vec![0x0f, 0]], false).is_none());
        assert!(compare(EPathMapMode::Empty, incoming, &[vec![4, 1, 0xff]], true).is_some());
        assert!(compare(EPathMapMode::Empty, incoming, &[vec![4, 1, 0xff]], false).is_none());
    }
}

#[test]
fn dangling_topology_and_rejected_deep_snapshots_retire_on_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for mode in [EPathMapMode::Set, EPathMapMode::Map] {
                let mut arena = ParConstruction::new();
                let value = arena.empty_par().unwrap();
                let mut incoming = arena.own_decoded_epm_repr(representation(mode, &[], value));
                let mut path = Vec::new();
                for _ in 0..8192 {
                    path.push(1);
                    match incoming.as_repr_mut() {
                        EPathMapRepr::Set(map) => {
                            map.create_path(&path);
                        }
                        EPathMapRepr::Map(map) => {
                            map.create_path(&path);
                        }
                        EPathMapRepr::Empty => unreachable!(),
                    }
                    *path.last_mut().unwrap() = 0;
                }
                let expected = epathmap_trie_codec::layout(incoming.as_repr())
                    .prefix()
                    .to_vec();
                let mut parent = owned_slots::EPathMap::default();
                parent
                    .replace_decoded_representation(incoming, true)
                    .unwrap();
                assert_eq!(parent.repr.as_repr().mode(), mode);
                assert_eq!(
                    epathmap_trie_codec::layout(parent.repr.as_repr()).prefix(),
                    expected
                );
                let mut rejected = arena.own_decoded_epm_repr(parent.repr.as_repr().clone());
                match rejected.as_repr_mut() {
                    EPathMapRepr::Set(map) => {
                        map.set_val_at(&[], ());
                    }
                    EPathMapRepr::Map(map) => {
                        map.set_val_at(&[], value);
                    }
                    EPathMapRepr::Empty => unreachable!(),
                }
                let error = parent
                    .replace_decoded_representation(rejected, true)
                    .unwrap_err();
                assert!(error.to_string().contains("EmptyInput"));
                assert_eq!(
                    epathmap_trie_codec::layout(parent.repr.as_repr()).prefix(),
                    expected
                );
                drop(parent);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
