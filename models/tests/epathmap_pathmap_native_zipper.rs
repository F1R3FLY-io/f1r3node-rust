use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::epathmap_trie_codec::{EPathMapMode, EPathMapRepr};
use models::rust::pathmap_integration::{par_to_path, segments_to_key, CursorKind};
use models::rust::rhoapi_ext::{EPathMapAlgebraError, EPathMapEmptyModeError, EntryTrie};
use pathmap::zipper::{Zipper, ZipperMoving, ZipperValues};
use pathmap::PathMap;
use proptest::prelude::*;
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

#[derive(Debug, PartialEq, Eq)]
struct NativeTrieImage {
    mode: EPathMapMode,
    len: usize,
    prefixes: BTreeSet<Vec<u8>>,
    values: BTreeMap<Vec<u8>, Vec<u8>>,
    epm1: Vec<u8>,
}

fn native_mask_observation<V: Clone + Send + Sync + Unpin + 'static>(
    map: &PathMap<V>,
    encode: impl Fn(&V) -> Vec<u8>,
) -> (BTreeSet<Vec<u8>>, BTreeMap<Vec<u8>, Vec<u8>>) {
    let mut prefixes = BTreeSet::new();
    let mut values = BTreeMap::new();
    let mut cursor = map.read_zipper();
    loop {
        prefixes.insert(cursor.path().to_vec());
        if let Some(value) = cursor.val() {
            values.insert(cursor.path().to_vec(), encode(value));
        }
        if let Some(byte) = cursor.child_mask().iter().next() {
            cursor.descend_to_byte(byte);
            continue;
        }
        loop {
            if cursor.at_root() {
                return (prefixes, values);
            }
            let previous = *cursor.path().last().unwrap();
            assert!(cursor.ascend_byte());
            if let Some(byte) = cursor.child_mask().iter().find(|byte| *byte > previous) {
                cursor.descend_to_byte(byte);
                break;
            }
        }
    }
}

fn native_image(trie: &EntryTrie) -> NativeTrieImage {
    let (prefixes, values) = match trie.representation() {
        EPathMapRepr::Empty => (BTreeSet::new(), BTreeMap::new()),
        EPathMapRepr::Set(map) => native_mask_observation(map, |()| Vec::new()),
        EPathMapRepr::Map(map) => native_mask_observation(map, Message::encode_to_vec),
    };
    assert_eq!(values.len(), trie.len());
    NativeTrieImage {
        mode: trie.mode(),
        len: trie.len(),
        prefixes,
        values,
        epm1: trie.trie_snapshot().to_vec(),
    }
}

fn dense_dangling_with_anchor(mode: EPathMapMode) -> EPathMap {
    let mut result = match mode {
        EPathMapMode::Set => set([gint(-1)]),
        EPathMapMode::Map => map([(gint(-1), gint(0))]),
        EPathMapMode::Empty => unreachable!(),
    };
    for path in [vec![0], vec![178], vec![1]] {
        assert!(result.create_path(&path).unwrap());
    }
    assert_eq!(result.len(), 1);
    assert!(!result.path_prefix_exists(&[178, 0]));
    result
}

fn insert_selected(destination: &mut EPathMap, key: Par, value: Par) {
    match destination.mode() {
        EPathMapMode::Set => destination.insert_entry(key),
        EPathMapMode::Map => destination.insert_map_entry(key, value).unwrap(),
        EPathMapMode::Empty => unreachable!(),
    }
}

fn assert_dense_absent_mutation(operation: u8) {
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let mut destination = dense_dangling_with_anchor(mode);
        let retained = destination.clone();
        let before = native_image(destination.entry_trie());
        let changed = match operation {
            0 => destination.remove_encoded_entry(&[178, 0]),
            1 => destination.remove_branches_at(&[178, 0]),
            _ => destination.remove_subtrie_at(&[178, 0]),
        };
        assert!(!changed);
        assert!(!destination.path_prefix_exists(&[178, 0]));
        assert!(destination.contains_entry(&gint(-1)));
        assert_eq!(native_image(destination.entry_trie()), before);
        assert_eq!(native_image(retained.entry_trie()), before);
        drop(destination);
        assert_eq!(native_image(retained.entry_trie()), before);
    }
}

#[test]
fn native_dense_dangling_absent_value_removal_preserves_exact_image() {
    assert_dense_absent_mutation(0);
}

#[test]
fn native_dense_dangling_absent_branch_removal_preserves_exact_image() {
    assert_dense_absent_mutation(1);
}

#[test]
fn native_dense_dangling_absent_subtrie_removal_preserves_exact_image() {
    assert_dense_absent_mutation(2);
}

#[test]
fn native_branch_pruning_then_parent_removal_preserves_dense_dangling_siblings() {
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        for remove_subtrie in [false, true] {
            let mut destination = dense_dangling_with_anchor(mode);
            let expected_final = native_image(destination.entry_trie());
            for (key, value) in [
                (gint(7), gint(70)),
                (list([7, 8]), gint(78)),
                (list([7, 9]), gint(79)),
            ] {
                insert_selected(&mut destination, key, value);
            }
            let retained = destination.clone();
            let before = native_image(retained.entry_trie());
            let prefix = encode_trie_path(&gint(7));
            assert!(encode_trie_path(&list([7, 8])).starts_with(&prefix));
            assert!(destination.remove_branches_at(&prefix));
            assert_eq!(destination.len(), 2);
            assert!(destination.contains_entry(&gint(7)));
            assert!(!destination.contains_entry(&list([7, 8])));
            assert!(!destination.contains_entry(&list([7, 9])));
            let mut expected_parent = dense_dangling_with_anchor(mode);
            insert_selected(&mut expected_parent, gint(7), gint(70));
            assert_eq!(
                native_image(destination.entry_trie()),
                native_image(expected_parent.entry_trie())
            );
            assert!(if remove_subtrie {
                destination.remove_subtrie_at(&prefix)
            } else {
                destination.remove_encoded_entry(&prefix)
            });
            assert_eq!(native_image(destination.entry_trie()), expected_final);
            assert!(!destination.remove_encoded_entry(&prefix));
            assert_eq!(native_image(destination.entry_trie()), expected_final);
            assert_eq!(native_image(retained.entry_trie()), before);
        }
    }
}

#[test]
fn native_map_subtraction_of_canonical_missing_descendant_is_exact_noop() {
    let mut left = dense_dangling_with_anchor(EPathMapMode::Map);
    let missing = list([178, 0]);
    let key = encode_trie_path(&missing);
    assert!(left.create_path(&key[..key.len() - 1]).unwrap());
    assert!(!left.path_prefix_exists(&key));
    let right = map([(missing, gint(900))]);
    let retained_left = left.clone();
    let retained_right = right.clone();
    let before_left = native_image(left.entry_trie());
    let before_right = native_image(right.entry_trie());
    let difference = left.entry_trie().try_subtract(right.entry_trie()).unwrap();
    assert_eq!(native_image(&difference), before_left);
    assert_eq!(native_image(left.entry_trie()), before_left);
    assert_eq!(native_image(right.entry_trie()), before_right);
    drop(left);
    drop(right);
    assert_eq!(native_image(&difference), before_left);
    assert_eq!(native_image(retained_left.entry_trie()), before_left);
    assert_eq!(native_image(retained_right.entry_trie()), before_right);
}

#[test]
fn native_valueless_prefix_subtrie_removal_preserves_dense_anchor_and_clone() {
    let prefix = segments_to_key(&par_to_path(&list([7])), false);
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let mut destination = dense_dangling_with_anchor(mode);
        let expected = native_image(destination.entry_trie());
        for (key, value) in [(list([7, 8]), gint(78)), (list([7, 9]), gint(79))] {
            insert_selected(&mut destination, key, value);
        }
        assert!(destination.path_prefix_exists(&prefix));
        assert!(!destination.contains_encoded_key(&prefix));
        let retained = destination.clone();
        let before = native_image(retained.entry_trie());
        assert!(destination.remove_subtrie_at(&prefix));
        assert!(!destination.path_prefix_exists(&prefix));
        assert_eq!(native_image(destination.entry_trie()), expected);
        assert_eq!(native_image(retained.entry_trie()), before);
        assert!(!destination.remove_subtrie_at(&prefix));
        assert_eq!(native_image(destination.entry_trie()), expected);
        drop(destination);
        assert_eq!(native_image(retained.entry_trie()), before);
    }
}

#[test]
fn native_composed_graft_extends_missing_descendant_of_dangling_prefix() {
    let cursor = par_to_path(&list([91, 92]));
    let prefix = segments_to_key(&cursor, false);
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        for bare_source in [false, true] {
            let mut destination = dense_dangling_with_anchor(mode);
            assert!(destination
                .create_path(&prefix[..prefix.len() - 1])
                .unwrap());
            assert!(!destination.path_prefix_exists(&prefix));
            let retained = destination.clone();
            let before = native_image(retained.entry_trie());
            let source_key = if bare_source { gint(7) } else { list([7]) };
            let source = match mode {
                EPathMapMode::Set => set([source_key]),
                EPathMapMode::Map => map([(source_key, gint(700))]),
                EPathMapMode::Empty => unreachable!(),
            };
            let retained_source = source.clone();
            let source_before = native_image(source.entry_trie());
            destination
                .replace_subtrie(&cursor, CursorKind::Split, &source)
                .unwrap();
            let mut expected = retained.clone();
            insert_selected(&mut expected, list([91, 92, 7]), gint(700));
            assert_eq!(
                native_image(destination.entry_trie()),
                native_image(expected.entry_trie())
            );
            assert!(destination.contains_entry(&list([91, 92, 7])));
            assert_eq!(native_image(retained.entry_trie()), before);
            assert_eq!(native_image(source.entry_trie()), source_before);
            drop(source);
            assert_eq!(native_image(retained_source.entry_trie()), source_before);
        }
    }
}

#[test]
fn native_empty_source_replacement_at_missing_dangling_descendant_is_exact() {
    let cursor = par_to_path(&list([91, 92]));
    let prefix = segments_to_key(&cursor, false);
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        let mut destination = dense_dangling_with_anchor(mode);
        assert!(destination
            .create_path(&prefix[..prefix.len() - 1])
            .unwrap());
        assert!(!destination.path_prefix_exists(&prefix));
        let retained = destination.clone();
        let before = native_image(retained.entry_trie());
        destination
            .replace_subtrie(&cursor, CursorKind::Split, &EPathMap::default())
            .unwrap();
        let mut expected = retained.clone();
        match mode {
            EPathMapMode::Set => expected.insert_entry(list([91, 92])),
            EPathMapMode::Map => {
                assert!(expected
                    .create_path(&encode_trie_path(&list([91, 92])))
                    .unwrap());
            }
            EPathMapMode::Empty => unreachable!(),
        }
        assert_eq!(
            native_image(destination.entry_trie()),
            native_image(expected.entry_trie())
        );
        assert_eq!(native_image(retained.entry_trie()), before);
    }
}

#[test]
fn native_composed_graft_specializes_neutral_empty_at_root_and_missing_prefix() {
    for mode in [EPathMapMode::Set, EPathMapMode::Map] {
        for bare_source in [false, true] {
            let source_key = if bare_source { gint(7) } else { list([7]) };
            let source = match mode {
                EPathMapMode::Set => set([source_key]),
                EPathMapMode::Map => map([(source_key, gint(700))]),
                EPathMapMode::Empty => unreachable!(),
            };
            let source_before = native_image(source.entry_trie());
            for root in [false, true] {
                let mut destination = EPathMap::default();
                let retained_empty = destination.clone();
                let empty_before = native_image(retained_empty.entry_trie());
                let cursor = if root {
                    Vec::new()
                } else {
                    par_to_path(&list([91, 92]))
                };
                destination
                    .replace_subtrie(&cursor, CursorKind::Split, &source)
                    .unwrap();
                let composed = if root { list([7]) } else { list([91, 92, 7]) };
                let expected = match mode {
                    EPathMapMode::Set => set([composed]),
                    EPathMapMode::Map => map([(composed, gint(700))]),
                    EPathMapMode::Empty => unreachable!(),
                };
                assert_eq!(
                    native_image(destination.entry_trie()),
                    native_image(expected.entry_trie())
                );
                assert_eq!(native_image(retained_empty.entry_trie()), empty_before);
                assert_eq!(native_image(source.entry_trie()), source_before);
            }
        }
    }
    let mut empty = EPathMap::default();
    let before = native_image(empty.entry_trie());
    empty
        .replace_subtrie(&[], CursorKind::Split, &EPathMap::default())
        .unwrap();
    assert_eq!(native_image(empty.entry_trie()), before);
}

#[test]
fn native_set_join_extends_dense_dangling_leaf_in_both_operand_orders() {
    assert_native_join_extends_dense_dangling_leaf(EPathMapMode::Set);
}

#[test]
fn native_map_join_extends_dense_dangling_leaf_in_both_operand_orders() {
    assert_native_join_extends_dense_dangling_leaf(EPathMapMode::Map);
}

fn assert_native_join_extends_dense_dangling_leaf(mode: EPathMapMode) {
    let left = dense_dangling_with_anchor(mode);
    let mut right = match mode {
        EPathMapMode::Set => set([gint(-1)]),
        EPathMapMode::Map => map([(gint(-1), gint(0))]),
        EPathMapMode::Empty => unreachable!(),
    };
    assert!(right.create_path(&[178, 0]).unwrap());
    let retained_left = left.clone();
    let retained_right = right.clone();
    let before_left = native_image(left.entry_trie());
    let before_right = native_image(right.entry_trie());
    let mut expected = left.clone();
    assert!(expected.create_path(&[178, 0]).unwrap());
    let expected = native_image(expected.entry_trie());
    for (first, second) in [(&left, &right), (&right, &left)] {
        let joined = first.entry_trie().try_join(second.entry_trie()).unwrap();
        assert_eq!(native_image(&joined), expected);
        assert!(joined.path_prefix_exists(&[178, 0]));
        assert_eq!(native_image(left.entry_trie()), before_left);
        assert_eq!(native_image(right.entry_trie()), before_right);
    }
    assert_eq!(native_image(left.entry_trie()), before_left);
    assert_eq!(native_image(right.entry_trie()), before_right);
    drop(left);
    drop(right);
    assert_eq!(native_image(retained_left.entry_trie()), before_left);
    assert_eq!(native_image(retained_right.entry_trie()), before_right);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn native_dispatch_original_order_dangling_history_and_absent_removals_property(
        map_mode in any::<bool>(),
        operations in prop::collection::vec(
            (
                prop::collection::vec(any::<u8>(), 0..10),
                prop::collection::vec(any::<u8>(), 0..10),
            ),
            0..25,
        ),
        reverse_drop in any::<bool>(),
    ) {
        let mode = if map_mode { EPathMapMode::Map } else { EPathMapMode::Set };
        let mut destination = dense_dangling_with_anchor(mode);
        let initial = native_image(destination.entry_trie());
        let mut prefixes = initial.prefixes;
        let values = initial.values;
        let mut snapshots = Vec::new();
        for (path, mut absent) in operations {
            snapshots.push((destination.clone(), native_image(destination.entry_trie())));
            let expected_created = !prefixes.contains(&path);
            for length in 0..=path.len() {
                prefixes.insert(path[..length].to_vec());
            }
            prop_assert_eq!(destination.create_path(&path).unwrap(), expected_created);
            let after_create = native_image(destination.entry_trie());
            prop_assert_eq!(&after_create.prefixes, &prefixes);
            prop_assert_eq!(&after_create.values, &values);
            prop_assert_eq!(after_create.mode, mode);
            prop_assert_eq!(after_create.len, values.len());
            for (snapshot, expected) in &snapshots {
                prop_assert_eq!(&native_image(snapshot.entry_trie()), expected);
            }
            while prefixes.contains(&absent) {
                absent.push(0);
            }
            prop_assert!(!destination.path_prefix_exists(&absent));
            for operation in 0..3 {
                let pointer = destination.trie_snapshot().as_ptr();
                let changed = match operation {
                    0 => destination.remove_encoded_entry(&absent),
                    1 => destination.remove_branches_at(&absent),
                    _ => destination.remove_subtrie_at(&absent),
                };
                prop_assert!(!changed);
                prop_assert_eq!(destination.trie_snapshot().as_ptr(), pointer);
                prop_assert_eq!(&native_image(destination.entry_trie()), &after_create);
                prop_assert!(!destination.path_prefix_exists(&absent));
                for (snapshot, expected) in &snapshots {
                    prop_assert_eq!(&native_image(snapshot.entry_trie()), expected);
                }
            }
            if !values.contains_key(&path) {
                let pointer = destination.trie_snapshot().as_ptr();
                prop_assert!(!destination.remove_encoded_entry(&path));
                prop_assert_eq!(destination.trie_snapshot().as_ptr(), pointer);
                prop_assert!(destination.path_prefix_exists(&path));
                prop_assert_eq!(&native_image(destination.entry_trie()), &after_create);
                for (snapshot, expected) in &snapshots {
                    prop_assert_eq!(&native_image(snapshot.entry_trie()), expected);
                }
            }
            if snapshots.len() > 4 {
                let retired = if reverse_drop { snapshots.pop().unwrap() } else { snapshots.remove(0) };
                drop(retired);
                prop_assert_eq!(&native_image(destination.entry_trie()), &after_create);
            }
        }
        drop(destination);
        if reverse_drop {
            snapshots.reverse();
        }
        while let Some((snapshot, expected)) = snapshots.pop() {
            prop_assert_eq!(native_image(snapshot.entry_trie()), expected);
            drop(snapshot);
            for (remaining, expected) in &snapshots {
                prop_assert_eq!(&native_image(remaining.entry_trie()), expected);
            }
        }
    }

    #[test]
    fn native_dispatch_join_original_order_histories_equal_exact_prefix_union_property(
        map_mode in any::<bool>(),
        left_paths in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..10), 0..25),
        right_paths in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..10), 0..25),
        reverse_drop in any::<bool>(),
    ) {
        let mode = if map_mode { EPathMapMode::Map } else { EPathMapMode::Set };
        let mut left = dense_dangling_with_anchor(mode);
        let mut right = dense_dangling_with_anchor(mode);
        prop_assert!(right.create_path(&[178, 0]).unwrap());
        let mut left_prefixes = native_image(left.entry_trie()).prefixes;
        let mut right_prefixes = native_image(right.entry_trie()).prefixes;
        for (destination, paths, prefixes) in [
            (&mut left, left_paths, &mut left_prefixes),
            (&mut right, right_paths, &mut right_prefixes),
        ] {
            for path in paths {
                let retained = destination.clone();
                let before = native_image(retained.entry_trie());
                let expected_created = !prefixes.contains(&path);
                for length in 0..=path.len() {
                    prefixes.insert(path[..length].to_vec());
                }
                prop_assert_eq!(destination.create_path(&path).unwrap(), expected_created);
                let after = native_image(destination.entry_trie());
                prop_assert_eq!(&after.prefixes, &*prefixes);
                prop_assert_eq!(&after.values, &before.values);
                prop_assert_eq!(after.mode, mode);
                prop_assert_eq!(after.len, 1);
                prop_assert_eq!(native_image(retained.entry_trie()), before);
            }
        }
        let retained_left = left.clone();
        let retained_right = right.clone();
        let before_left = native_image(left.entry_trie());
        let before_right = native_image(right.entry_trie());
        prop_assert_eq!(&before_left.values, &before_right.values);
        let expected_prefixes = left_prefixes.union(&right_prefixes).cloned().collect::<BTreeSet<_>>();
        let forward = left.entry_trie().try_join(right.entry_trie()).unwrap();
        let reverse = right.entry_trie().try_join(left.entry_trie()).unwrap();
        let joined = native_image(&forward);
        prop_assert_eq!(&joined.prefixes, &expected_prefixes);
        prop_assert_eq!(&joined.values, &before_left.values);
        prop_assert_eq!(joined.mode, mode);
        prop_assert_eq!(joined.len, before_left.values.len());
        prop_assert_eq!(&native_image(&reverse), &joined);
        prop_assert_eq!(&native_image(left.entry_trie()), &before_left);
        prop_assert_eq!(&native_image(right.entry_trie()), &before_right);
        if reverse_drop {
            drop(left);
            drop(right);
        } else {
            drop(right);
            drop(left);
        }
        prop_assert_eq!(&native_image(&forward), &joined);
        prop_assert_eq!(&native_image(&reverse), &joined);
        prop_assert_eq!(native_image(retained_left.entry_trie()), before_left);
        prop_assert_eq!(native_image(retained_right.entry_trie()), before_right);
        drop(forward);
        prop_assert_eq!(native_image(&reverse), joined);
    }
}
