use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::pathmap_integration::{par_to_path, segments_to_key, CursorKind};
use models::rust::rhoapi_ext::{EPathMapAlgebraError, EPathMapEmptyModeError};
use prost::Message;

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn list(values: impl IntoIterator<Item = i64>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: values.into_iter().map(gint).collect(),
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn set(values: impl IntoIterator<Item = Par>) -> EPathMap {
    EPathMap::new(values.into_iter().collect::<Vec<_>>(), vec![], false, None)
}

fn map(values: impl IntoIterator<Item = (Par, Par)>) -> EPathMap {
    EPathMap::new_map(values, vec![], false, None)
}

fn hash(value: &EPathMap) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

fn topology_only(mode: EPathMapMode, path: &Par) -> EPathMap {
    let anchor = gint(-1);
    let anchor_key = encode_trie_path(&anchor);
    let path_key = encode_trie_path(path);
    let mut result = match mode {
        EPathMapMode::Set => set([anchor]),
        EPathMapMode::Map => map([(anchor, gint(0))]),
        EPathMapMode::Empty => panic!("topology-only storage requires a selected specialization"),
    };
    assert!(result.create_path(&path_key).unwrap());
    assert!(result.remove_encoded_entry(&anchor_key));
    assert_eq!(result.len(), 0);
    assert_eq!(result.mode(), mode);
    assert!(
        !result.is_empty(),
        "value-free PathMap topology is observable"
    );
    assert!(result.path_prefix_exists(&path_key));
    result
}

#[test]
fn leaf_and_subtrie_queries_preserve_set_and_map_semantics() {
    let key = list([1, 2]);
    let encoded = encode_trie_path(&key);
    let set = set([key.clone()]);
    let map = map([(key.clone(), gint(12))]);

    assert_eq!(set.leaf_at_encoded_key(&encoded), Some(key.clone()));
    assert_eq!(map.leaf_at_encoded_key(&encoded), Some(gint(12)));

    let prefix = segments_to_key(&par_to_path(&list([1])), false);
    let set_subtrie = set.subtrie(&prefix);
    let map_subtrie = map.subtrie(&prefix);
    assert_eq!(set_subtrie.mode(), EPathMapMode::Set);
    assert_eq!(map_subtrie.mode(), EPathMapMode::Map);
    assert_eq!(set_subtrie.leaf_at_encoded_key(&encoded), Some(key.clone()));
    assert_eq!(map_subtrie.leaf_at_encoded_key(&encoded), Some(gint(12)));
}

#[test]
fn map_subtrie_replacement_and_drop_head_keep_values_in_pathmap_slots() {
    let mut destination = map([(list([0, 9]), gint(9)), (list([8]), gint(8))]);
    let source = map([(list([1]), gint(11)), (list([2]), gint(22))]);
    let cursor = par_to_path(&list([0]));
    destination
        .replace_subtrie(&cursor, CursorKind::Split, &source)
        .unwrap();

    assert_eq!(destination.mode(), EPathMapMode::Map);
    assert_eq!(destination.get_map_value(&list([0, 9])).unwrap(), None);
    assert_eq!(
        destination.get_map_value(&list([0, 1])).unwrap(),
        Some(&gint(11))
    );
    assert_eq!(
        destination.get_map_value(&list([0, 2])).unwrap(),
        Some(&gint(22))
    );
    assert_eq!(
        destination.get_map_value(&list([8])).unwrap(),
        Some(&gint(8))
    );

    let dropped = destination.drop_head(1);
    assert_eq!(dropped.mode(), EPathMapMode::Map);
    assert_eq!(dropped.get_map_value(&list([1])).unwrap(), Some(&gint(11)));
    assert_eq!(dropped.get_map_value(&list([2])).unwrap(), Some(&gint(22)));
    assert_eq!(
        dropped.get_map_value(&list([])).unwrap(),
        None,
        "an entry whose path is exhausted by dropHead is removed"
    );
}

#[test]
fn neutral_empty_topology_is_rejected_but_selected_modes_retain_it() {
    let key = encode_trie_path(&list([1, 2]));
    let mut empty = EPathMap::default();
    assert_eq!(empty.create_path(&key), Err(EPathMapEmptyModeError));

    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let topology = topology_only(mode, &list([1, 2]));
        assert_eq!(topology.mode(), mode);
        assert_eq!(topology.len(), 0);
        assert!(!topology.is_empty());
    }

    let mut empty_destination = EPathMap::default();
    assert!(matches!(
        empty_destination.replace_subtrie(
            &par_to_path(&list([1])),
            CursorKind::Split,
            &EPathMap::default(),
        ),
        Err(EPathMapAlgebraError::AmbiguousEmpty {
            operation: "setSubtrie"
        })
    ));
}

#[test]
fn epm1_and_both_serialization_surfaces_preserve_value_free_topology() {
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let original = topology_only(mode, &list([1, 2, 3]));
        let snapshot = original.trie_snapshot().to_vec();
        let repr = models::rust::epathmap_trie_codec::decode(&snapshot).unwrap();
        assert_eq!(repr.mode(), mode);
        assert_eq!(models::rust::epathmap_trie_codec::encode(&repr), snapshot);

        let protobuf = original.encode_to_vec();
        let protobuf_round = EPathMap::decode(protobuf.as_slice()).unwrap();
        assert_eq!(protobuf_round.mode(), mode);
        assert_eq!(protobuf_round.trie_snapshot(), snapshot);

        let bincode = bincode::serialize(&original).unwrap();
        let bincode_round: EPathMap = bincode::deserialize(&bincode).unwrap();
        assert_eq!(bincode_round.mode(), mode);
        assert_eq!(bincode_round.trie_snapshot(), snapshot);
    }
}

#[test]
fn eq_hash_and_ord_include_value_free_pathmap_topology() {
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let left = topology_only(mode, &list([1, 2]));
        let equal = topology_only(mode, &list([1, 2]));
        let right = topology_only(mode, &list([1, 3]));

        assert_eq!(left, equal);
        assert_eq!(left.cmp(&equal), std::cmp::Ordering::Equal);
        assert_eq!(hash(&left), hash(&equal));
        assert_ne!(left, right);
        assert_ne!(left.cmp(&right), std::cmp::Ordering::Equal);
    }
}

#[test]
fn native_branch_removal_preserves_unrelated_map_associations() {
    let mut map = map([
        (gint(1), gint(1)),
        (list([1, 2]), gint(12)),
        (list([1, 3]), gint(13)),
        (list([4]), gint(4)),
    ]);
    let prefix = segments_to_key(&par_to_path(&list([1])), false);
    assert!(map.remove_branches_at(&prefix));
    assert_eq!(map.get_map_value(&gint(1)).unwrap(), Some(&gint(1)));
    assert_eq!(map.get_map_value(&list([1, 2])).unwrap(), None);
    assert_eq!(map.get_map_value(&list([1, 3])).unwrap(), None);
    assert_eq!(map.get_map_value(&list([4])).unwrap(), Some(&gint(4)));

    assert!(map.remove_subtrie_at(&prefix));
    assert_eq!(map.get_map_value(&gint(1)).unwrap(), None);
    assert_eq!(map.get_map_value(&list([4])).unwrap(), Some(&gint(4)));
}

#[test]
fn map_algebra_includes_value_free_topology_without_a_par_lattice() {
    let left = topology_only(EPathMapMode::Map, &list([1, 2]));
    let equal = topology_only(EPathMapMode::Map, &list([1, 2]));
    let right = topology_only(EPathMapMode::Map, &list([1, 3]));

    let joined = left.entry_trie().try_join(right.entry_trie()).unwrap();
    assert!(joined.path_prefix_exists(&encode_trie_path(&list([1, 2]))));
    assert!(joined.path_prefix_exists(&encode_trie_path(&list([1, 3]))));
    assert_eq!(joined.mode(), EPathMapMode::Map);

    let met = left.entry_trie().try_meet(equal.entry_trie()).unwrap();
    assert_eq!(met.mode(), EPathMapMode::Map);
    assert!(met.path_prefix_exists(&encode_trie_path(&list([1, 2]))));

    let disjoint = left.entry_trie().try_meet(right.entry_trie()).unwrap();
    assert_eq!(disjoint.len(), 0);
    assert_ne!(disjoint, *left.entry_trie());
}
