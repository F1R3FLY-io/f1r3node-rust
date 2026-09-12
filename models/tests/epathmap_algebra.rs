use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::pathmap_integration::CursorKind;
use models::rust::rhoapi_ext::{EPathMapAlgebraError, EntryTrie};
use models::rust::utils::new_freevar_par;
use pathmap::PathMap;
use proptest::prelude::*;

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

fn set(values: impl IntoIterator<Item = i64>) -> EPathMap {
    EPathMap::new(
        values.into_iter().map(gint).collect::<Vec<_>>(),
        vec![],
        false,
        None,
    )
}

fn map(values: impl IntoIterator<Item = (i64, i64)>) -> EPathMap {
    EPathMap::new_map(
        values
            .into_iter()
            .map(|(key, value)| (gint(key), gint(value))),
        vec![],
        false,
        None,
    )
}

fn set_values(entries: &models::rust::rhoapi_ext::EntryTrie) -> Vec<i64> {
    entries
        .entries_owned()
        .into_iter()
        .map(
            |par| match par.first::<Expr>().unwrap().unwrap().0.expr_instance {
                Some(ExprInstance::GInt(value)) => value,
                other => panic!("expected integer set member, got {other:?}"),
            },
        )
        .collect()
}

fn assert_map_value(entries: &models::rust::rhoapi_ext::EntryTrie, key: i64, value: Option<i64>) {
    let actual = entries
        .get_map_value(&gint(key))
        .expect("result remains map-mode")
        .map(
            |par| match par.first::<Expr>().unwrap().unwrap().0.expr_instance {
                Some(ExprInstance::GInt(value)) => value,
                other => panic!("expected integer map value, got {other:?}"),
            },
        );
    assert_eq!(actual, value);
}

fn assert_map_join_snapshot(label: &str, left: &EPathMap, right: &EPathMap, expected: &EPathMap) {
    let actual = left.entry_trie().try_join(right.entry_trie()).unwrap();
    assert_eq!(
        actual,
        *expected.entry_trie(),
        "EntryTrie mismatch: {label}"
    );
    assert_eq!(
        actual.trie_snapshot(),
        expected.entry_trie().trie_snapshot(),
        "EPM1 mismatch: {label}"
    );
}

#[test]
fn clone_family_map_join_shortcut_is_invalidated_by_every_mutation() {
    let base = map([(0, 0), (1, 10), (2, 20)]);
    let shared = base.clone();
    assert_map_join_snapshot("unchanged clone family", &base, &shared, &base);

    let independent = map([(0, 0), (1, 10), (2, 20)]);
    assert_map_join_snapshot("independently equal", &base, &independent, &base);

    let mut extended = base.clone();
    extended.insert_map_entry(gint(3), gint(30)).unwrap();
    assert_map_join_snapshot("extended clone", &extended, &base, &extended);

    let mut replaced = base.clone();
    replaced.insert_map_entry(gint(2), gint(999)).unwrap();
    assert!(matches!(
        replaced.entry_trie().try_join(base.entry_trie()),
        Err(EPathMapAlgebraError::ValueConflict { .. })
    ));

    let key_two = encode_trie_path(&gint(2));
    let mut removed = base.clone();
    assert!(removed.remove_encoded_entry(&key_two));
    assert_map_join_snapshot("removed entry clone", &removed, &base, &base);

    let mut topology = base.clone();
    let mut descendant = key_two.clone();
    descendant.extend_from_slice(&[0xAA, 0xBB]);
    assert!(topology.create_path(&descendant).unwrap());
    assert_map_join_snapshot("value-free topology clone", &base, &topology, &topology);

    let mut pruned = topology.clone();
    assert!(pruned.remove_branches_at(&key_two));
    assert_map_join_snapshot("pruned topology clone", &pruned, &topology, &topology);

    let mut subtrie_removed = base.clone();
    assert!(subtrie_removed.remove_subtrie_at(&key_two));
    assert_map_join_snapshot("removed subtrie clone", &subtrie_removed, &base, &base);

    let mut greatest_removed = base.clone();
    assert!(greatest_removed
        .remove_greatest_map_entry()
        .unwrap()
        .is_some());
    assert_map_join_snapshot("removed greatest clone", &greatest_removed, &base, &base);

    let mut replaced_subtrie = base.clone();
    replaced_subtrie
        .replace_subtrie(&[], CursorKind::Prefix, &map([(8, 80), (9, 90)]))
        .unwrap();
    let replaced_snapshot = replaced_subtrie.entry_trie().trie_snapshot().to_vec();
    let combined = replaced_subtrie
        .entry_trie()
        .try_join(base.entry_trie())
        .unwrap();
    assert_eq!(combined.len(), 5);
    assert_ne!(combined.trie_snapshot(), replaced_snapshot);
}

fn assert_clone_join_storage(base: &EPathMap, force_snapshot: bool) {
    if force_snapshot {
        assert!(!base.trie_snapshot().is_empty());
    }
    let shared = base.clone();
    let joined = EPathMap::new(
        base.entry_trie().try_join(shared.entry_trie()).unwrap(),
        vec![],
        false,
        None,
    );
    assert!(joined.shares_storage_with(base));
    assert!(joined.shares_storage_with(&shared));
    assert_eq!(joined.entry_trie(), base.entry_trie());
    assert_eq!(
        joined.trie_snapshot().as_ptr(),
        base.trie_snapshot().as_ptr()
    );
}

#[test]
fn clone_family_join_retains_storage_for_both_modes_and_cache_states() {
    for map_mode in [false, true] {
        for force_snapshot in [false, true] {
            for dangling in [false, true] {
                let mut base = if map_mode {
                    map([(0, 0), (1, 10), (2, 20)])
                } else {
                    set([0, 1, 2])
                };
                if dangling {
                    for path in [&[0][..], &[178], &[1], &[178, 0]] {
                        base.create_path(path).unwrap();
                    }
                }
                assert_clone_join_storage(&base, force_snapshot);
            }
        }
    }
}

#[test]
fn clone_family_set_join_shortcut_is_invalidated_by_mutation() {
    for force_snapshot in [false, true] {
        let base = set([0, 1, 2]);
        if force_snapshot {
            assert!(!base.trie_snapshot().is_empty());
        }
        let mut extended = base.clone();
        extended.insert_entry(gint(3));
        assert!(!extended.shares_storage_with(&base));
        assert_map_join_snapshot("extended set", &extended, &base, &extended);

        let key = encode_trie_path(&gint(2));
        let mut removed = base.clone();
        assert!(removed.remove_encoded_entry(&key));
        assert!(!removed.shares_storage_with(&base));
        assert_map_join_snapshot("removed set entry", &removed, &base, &base);

        let mut topology = base.clone();
        let mut descendant = key.clone();
        descendant.extend_from_slice(&[0xAA, 0xBB]);
        assert!(topology.create_path(&descendant).unwrap());
        assert!(!topology.shares_storage_with(&base));
        assert_map_join_snapshot("set topology", &base, &topology, &topology);

        let mut pruned = topology.clone();
        assert!(pruned.remove_branches_at(&key));
        assert!(!pruned.shares_storage_with(&topology));
        assert_map_join_snapshot("pruned set topology", &pruned, &topology, &topology);

        let mut subtrie_removed = base.clone();
        assert!(subtrie_removed.remove_subtrie_at(&key));
        assert!(!subtrie_removed.shares_storage_with(&base));
        assert_map_join_snapshot("removed set subtrie", &subtrie_removed, &base, &base);

        let mut greatest_removed = base.clone();
        assert!(greatest_removed.remove_greatest_entry().is_some());
        assert!(!greatest_removed.shares_storage_with(&base));
        assert_map_join_snapshot(
            "removed greatest set entry",
            &greatest_removed,
            &base,
            &base,
        );

        let mut replaced = base.clone();
        replaced
            .replace_subtrie(&[], CursorKind::Prefix, &set([8, 9]))
            .unwrap();
        assert!(!replaced.shares_storage_with(&base));
        assert_map_join_snapshot(
            "replaced set subtrie",
            &replaced,
            &base,
            &EPathMap::new(
                vec![gint(0), gint(1), gint(2), list([8]), list([9])],
                vec![],
                false,
                None,
            ),
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn clone_family_join_preserves_storage_after_generated_topology_histories(
        map_mode in any::<bool>(),
        force_snapshot in any::<bool>(),
        entries in prop::collection::vec(any::<i16>(), 1..32),
        paths in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..16), 0..32),
    ) {
        let mut base = if map_mode {
            map(entries.iter().map(|&v| (i64::from(v), i64::from(v))))
        } else {
            set(entries.iter().map(|&v| i64::from(v)))
        };
        for path in paths {
            base.create_path(&path).unwrap();
        }
        assert_clone_join_storage(&base, force_snapshot);
    }
}

#[test]
fn empty_is_mode_neutral_for_the_partial_algebra() {
    let empty = EPathMap::default();
    let set = set([1, 2]);
    let map = map([(1, 10), (2, 20)]);

    assert_eq!(empty.mode(), EPathMapMode::Empty);
    assert_eq!(
        empty
            .entry_trie()
            .try_join(set.entry_trie())
            .unwrap()
            .mode(),
        EPathMapMode::Set
    );
    assert_eq!(
        empty
            .entry_trie()
            .try_join(map.entry_trie())
            .unwrap()
            .mode(),
        EPathMapMode::Map
    );
    assert_eq!(
        set.entry_trie()
            .try_meet(empty.entry_trie())
            .unwrap()
            .mode(),
        EPathMapMode::Empty
    );
    assert_eq!(
        map.entry_trie()
            .try_subtract(empty.entry_trie())
            .unwrap()
            .mode(),
        EPathMapMode::Map
    );
}

#[test]
fn set_algebra_uses_pathmap_lattice_operations() {
    let left = set([1, 2, 3]);
    let right = set([3, 4]);

    assert_eq!(
        set_values(&left.entry_trie().try_join(right.entry_trie()).unwrap()),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        set_values(&left.entry_trie().try_meet(right.entry_trie()).unwrap()),
        vec![3]
    );
    assert_eq!(
        set_values(&left.entry_trie().try_subtract(right.entry_trie()).unwrap()),
        vec![1, 2]
    );

    for a in 0u8..8 {
        for b in 0u8..8 {
            let left = set((0..3).filter(|bit| a & (1 << bit) != 0).map(i64::from));
            let right = set((0..3).filter(|bit| b & (1 << bit) != 0).map(i64::from));
            let ab = left.entry_trie().try_join(right.entry_trie()).unwrap();
            let ba = right.entry_trie().try_join(left.entry_trie()).unwrap();
            assert_eq!(set_values(&ab), set_values(&ba));
            let idem = left.entry_trie().try_join(left.entry_trie()).unwrap();
            assert_eq!(set_values(&idem), set_values(left.entry_trie()));
        }
    }
}

#[test]
fn map_algebra_preserves_associations_and_rejects_unequal_overlaps() {
    let left = map([(1, 10), (2, 20)]);
    let right = map([(2, 20), (3, 30)]);

    let joined = left.entry_trie().try_join(right.entry_trie()).unwrap();
    assert_map_value(&joined, 1, Some(10));
    assert_map_value(&joined, 2, Some(20));
    assert_map_value(&joined, 3, Some(30));

    let met = left.entry_trie().try_meet(right.entry_trie()).unwrap();
    assert_map_value(&met, 1, None);
    assert_map_value(&met, 2, Some(20));
    assert_map_value(&met, 3, None);

    let subtracted = left.entry_trie().try_subtract(right.entry_trie()).unwrap();
    assert_map_value(&subtracted, 1, Some(10));
    assert_map_value(&subtracted, 2, None);

    let conflict = map([(2, 99)]);
    for result in [
        left.entry_trie().try_join(conflict.entry_trie()),
        left.entry_trie().try_meet(conflict.entry_trie()),
    ] {
        assert!(matches!(
            result,
            Err(EPathMapAlgebraError::ValueConflict { .. })
        ));
    }

    // Difference treats the right map as a key mask; its associated values do
    // not participate in the operation.
    let masked = left
        .entry_trie()
        .try_subtract(conflict.entry_trie())
        .unwrap();
    assert_map_value(&masked, 1, Some(10));
    assert_map_value(&masked, 2, None);
}

#[test]
fn encoded_key_masks_specialize_subtraction_and_selection_without_mode_conversion() {
    let mut mask = PathMap::new();
    mask.insert(encode_trie_path(&gint(2)), ());

    let set = set([1, 2, 3]);
    let set_difference = set.entry_trie().subtract_encoded_keys(&mask);
    let set_selection = set.entry_trie().select_encoded_keys(&mask);
    assert_eq!(set_difference.mode(), EPathMapMode::Set);
    assert_eq!(set_values(&set_difference), vec![1, 3]);
    assert_eq!(set_values(&set_selection), vec![2]);

    let map = map([(1, 10), (2, 20), (3, 30)]);
    let map_difference = map.entry_trie().subtract_encoded_keys(&mask);
    let map_selection = map.entry_trie().select_encoded_keys(&mask);
    assert_eq!(map_difference.mode(), EPathMapMode::Map);
    assert_map_value(&map_difference, 1, Some(10));
    assert_map_value(&map_difference, 2, None);
    assert_map_value(&map_difference, 3, Some(30));
    assert_eq!(map_selection.mode(), EPathMapMode::Map);
    assert_map_value(&map_selection, 1, None);
    assert_map_value(&map_selection, 2, Some(20));
    assert_map_value(&map_selection, 3, None);
}

#[test]
fn canonical_key_flags_are_read_without_decoding_a_par_projection() {
    let exact = encode_trie_path(&gint(7));
    let exact_metadata = EntryTrie::encoded_key_metadata(&exact).expect("exact canonical key");
    assert!(!exact_metadata.connective_used);
    assert!(exact_metadata.locally_free_is_empty);

    let dynamic = encode_trie_path(&new_freevar_par(3, vec![0b0000_1000]));
    let dynamic_metadata =
        EntryTrie::encoded_key_metadata(&dynamic).expect("dynamic canonical key");
    assert!(dynamic_metadata.connective_used);
    assert!(!dynamic_metadata.locally_free_is_empty);
}

#[test]
fn nonempty_set_and_map_modes_cannot_mix() {
    let set = set([1]);
    let map = map([(1, 10)]);
    for result in [
        set.entry_trie().try_join(map.entry_trie()),
        set.entry_trie().try_meet(map.entry_trie()),
        set.entry_trie().try_subtract(map.entry_trie()),
        set.entry_trie().try_restrict(map.entry_trie()),
    ] {
        assert!(matches!(
            result,
            Err(EPathMapAlgebraError::ModeMismatch { .. })
        ));
    }
}

#[test]
fn member_prefix_restriction_is_specialized_for_both_modes() {
    let set_base = EPathMap::new(
        vec![list([1, 2]), list([1, 3]), list([4])],
        vec![],
        false,
        None,
    );
    let set_selector = EPathMap::new(vec![list([1])], vec![], false, None);
    let set_result = set_base
        .entry_trie()
        .try_restrict_member_prefixes(set_selector.entry_trie())
        .unwrap();
    assert_eq!(set_result.len(), 2);

    let map_base = EPathMap::new_map(
        [
            (list([1, 2]), gint(12)),
            (list([1, 3]), gint(13)),
            (list([4]), gint(4)),
        ],
        vec![],
        false,
        None,
    );
    let map_selector = EPathMap::new_map([(list([1]), gint(0))], vec![], false, None);
    let map_result = map_base
        .entry_trie()
        .try_restrict_member_prefixes(map_selector.entry_trie())
        .unwrap();
    assert_eq!(map_result.len(), 2);
    assert_eq!(map_result.mode(), EPathMapMode::Map);
}

#[test]
fn map_algebra_is_stack_safe_for_deep_values_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut deep = gint(0);
            for _ in 0..4096 {
                deep = Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![deep],
                        locally_free: Vec::new(),
                        connective_used: false,
                        remainder: None,
                    })),
                }]);
            }
            let left = EPathMap::new_map([(gint(1), deep.clone())], vec![], false, None);
            let right = EPathMap::new_map([(gint(1), deep)], vec![], false, None);
            let joined = left.entry_trie().try_join(right.entry_trie()).unwrap();
            assert_eq!(joined.len(), 1);
            let met = left.entry_trie().try_meet(right.entry_trie()).unwrap();
            assert_eq!(met.len(), 1);
        })
        .unwrap()
        .join()
        .unwrap();
}
