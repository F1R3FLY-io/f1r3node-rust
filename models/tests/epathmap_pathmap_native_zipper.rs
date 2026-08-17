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
fn native_zipper_graft_totalizes_bare_singleton_composition_collisions() {
    let cursor = par_to_path(&list([9]));

    let mut set_destination = set([list([9, 99]), list([8])]);
    let set_source = set([gint(5), list([5])]);
    set_destination
        .replace_subtrie(&cursor, CursorKind::Split, &set_source)
        .expect("valid set composition must not panic");
    assert_eq!(set_destination.len(), 2);
    assert!(set_destination.contains_entry(&list([9, 5])));
    assert!(set_destination.contains_entry(&list([8])));

    let mut map_destination = map([(list([9, 99]), gint(99)), (list([8]), gint(8))]);
    let map_source = map([(gint(5), gint(50)), (list([5]), gint(500))]);
    map_destination
        .replace_subtrie(&cursor, CursorKind::Split, &map_source)
        .expect("valid map composition must not panic");
    assert_eq!(map_destination.len(), 2);
    assert_eq!(
        map_destination.get_map_value(&list([9, 5])).unwrap(),
        Some(&gint(500)),
        "the later canonical split entry retains predecessor precedence"
    );
    assert_eq!(
        map_destination.get_map_value(&list([8])).unwrap(),
        Some(&gint(8))
    );
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
fn focused_subtrie_preserves_a_value_free_endpoint() {
    let endpoint = list([1, 7]);
    let encoded = encode_trie_path(&endpoint);
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let source = topology_only(mode, &endpoint);
        let selected = source.subtrie(&encoded);
        assert_eq!(selected.mode(), mode);
        assert_eq!(selected.len(), 0);
        assert!(
            !selected.is_empty(),
            "a selected value-free endpoint remains observable in {mode:?} mode"
        );
        assert!(selected.path_prefix_exists(&encoded));
        assert_eq!(selected.trie_snapshot(), source.trie_snapshot());
    }
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

#[test]
fn exact_map_meet_preserves_common_topology_and_first_conflict() {
    fn assert_topology(left: &EPathMap, right: &EPathMap, expected: &EPathMap) {
        let actual = left
            .entry_trie()
            .try_meet(right.entry_trie())
            .expect("compatible map meet succeeds");
        assert_eq!(actual, *expected.entry_trie());
        assert_eq!(
            actual.trie_snapshot(),
            expected.entry_trie().trie_snapshot()
        );
    }

    let common_topology = list([7, 8]);
    let left_topology = topology_only(EPathMapMode::Map, &common_topology);
    let right_topology = topology_only(EPathMapMode::Map, &common_topology);
    assert_topology(&left_topology, &right_topology, &left_topology);

    let first = list([1]);
    let second = list([2]);
    let first_key = encode_trie_path(&first);
    let second_key = encode_trie_path(&second);
    let mut left = map([(first.clone(), gint(10))]);
    let mut right = map([(second.clone(), gint(20))]);
    left.create_path(&second_key).unwrap();
    right.create_path(&first_key).unwrap();
    let mut expected = topology_only(EPathMapMode::Map, &first);
    assert!(expected.create_path(&second_key).unwrap());
    assert_topology(&left, &right, &expected);

    let conflict_left = map([(first.clone(), gint(10)), (second.clone(), gint(20))]);
    let conflict_right = map([(first, gint(11)), (second, gint(21))]);
    assert_eq!(
        conflict_left
            .entry_trie()
            .try_meet(conflict_right.entry_trie())
            .expect_err("the first unequal shared value conflicts"),
        EPathMapAlgebraError::ValueConflict {
            operation: "meet",
            encoded_key: first_key,
        }
    );
}

#[cfg(feature = "epathmap-subtract-into-differential")]
#[test]
fn set_subtract_into_treatment_matches_whole_map_subtraction() {
    fn assert_same(left: &EPathMap, right: &EPathMap) {
        let left_before = left.trie_snapshot().to_vec();
        let right_before = right.trie_snapshot().to_vec();
        let control = left.entry_trie().try_subtract(right.entry_trie()).unwrap();
        let treatment = left
            .entry_trie()
            .try_subtract_into_treatment(right.entry_trie())
            .unwrap();
        assert_eq!(control, treatment);
        assert_eq!(control.trie_snapshot(), treatment.trie_snapshot());
        assert_eq!(left.trie_snapshot(), left_before);
        assert_eq!(right.trie_snapshot(), right_before);
    }

    let disjoint_left = set([gint(1), list([1, 2])]);
    let disjoint_right = set([gint(2), list([2, 1])]);
    assert_same(&disjoint_left, &disjoint_right);

    let overlap_left = set([gint(1), gint(2), gint(3)]);
    let overlap_right = set([gint(2), gint(3), gint(4)]);
    assert_same(&overlap_left, &overlap_right);
    assert_same(&overlap_left, &overlap_left);

    let topology = topology_only(EPathMapMode::Set, &list([7, 8]));
    let equal_topology = topology_only(EPathMapMode::Set, &list([7, 8]));
    let other_topology = topology_only(EPathMapMode::Set, &list([7, 9]));
    assert_same(&topology, &equal_topology);
    assert_same(&topology, &other_topology);
}
