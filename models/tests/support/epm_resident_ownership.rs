use std::panic::{catch_unwind, AssertUnwindSafe};

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::Expr;

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn deep_value(depth: usize) -> Par {
    use crate::rhoapi::ENot;
    use crate::rust::par_construction::ParConstruction;
    let mut arena = ParConstruction::new();
    let mut value = arena.empty_par().unwrap();
    let expr = Expr {
        expr_instance: Some(ExprInstance::ENotBody(ENot {
            p: Some(Par::default()),
        })),
    };
    for _ in 0..depth {
        let component = arena.expr_component(&expr, &[value], false).unwrap();
        value = arena
            .singleton_component(component, Vec::new(), false)
            .unwrap();
    }
    arena.finish(value).unwrap()
}

fn fixture(map: bool, value: i64) -> EPathMap {
    if map {
        EPathMap::new_map([(integer(1), integer(value))], vec![3], true, None)
    } else {
        EPathMap::from_set_iter([integer(value)], vec![3], true, None)
    }
}

fn metadata(entries: &EntryTrie) -> (usize, bool, bool, Vec<u8>, bool) {
    (
        entries.len,
        entries.entries_stable,
        entries.entries_reducer_eval_identity,
        entries.union_locally_free.clone(),
        entries.any_connective_used,
    )
}

fn assert_cold(entries: &EntryTrie) {
    assert!(entries.storage.trie_snapshot.get().is_none());
    assert!(entries.storage.epm_layout.get().is_none());
}

fn topology_owner(map: bool, depth: usize) -> OwnedResidentEPathMap {
    let repr = if map {
        EPathMapRepr::Map(pathmap::PathMap::new())
    } else {
        EPathMapRepr::Set(pathmap::PathMap::new())
    };
    let mut owner = OwnedResidentEPathMap::new(repr, ResidentEpmBarrier);
    let mut path = Vec::new();
    for _ in 0..depth {
        path.push(1);
        match owner.as_repr_mut() {
            EPathMapRepr::Set(map) => {
                native_create_path(map, &path);
            }
            EPathMapRepr::Map(map) => {
                native_create_path(map, &path);
            }
            EPathMapRepr::Empty => unreachable!(),
        }
        *path.last_mut().unwrap() = 0;
    }
    owner
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/epm_resident_ownership.regressions")
        ))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn cold_and_warm_aliases_share_storage_until_mutation(
        map in any::<bool>(), initial in -1000i64..1000, replacement in 1001i64..2000,
        warm in any::<bool>(), mutate_first in any::<bool>(),
    ) {
        let mut first = fixture(map, initial);
        assert_cold(&first.ps);
        let mut second = first.clone();
        prop_assert!(Arc::ptr_eq(&first.ps.storage, &second.ps.storage));
        prop_assert_eq!(Arc::strong_count(&first.ps.storage), 2);
        if warm {
            let bytes = first.trie_snapshot().as_ptr();
            prop_assert_eq!(bytes, second.trie_snapshot().as_ptr());
        }
        let (changed, sibling) = if mutate_first { (&mut first, &second) } else { (&mut second, &first) };
        let old_metadata = metadata(&sibling.ps);
        if map {
            changed.ps.insert_map_entry(integer(1), integer(replacement)).unwrap();
        } else {
            changed.ps.insert_entry(integer(replacement));
        }
        prop_assert!(!Arc::ptr_eq(&changed.ps.storage, &sibling.ps.storage));
        assert_cold(&changed.ps);
        prop_assert_eq!(metadata(&sibling.ps), old_metadata);
        let original = fixture(map, initial);
        prop_assert_eq!(sibling.trie_snapshot(), original.trie_snapshot());
        let expected = if map { fixture(true, replacement) } else {
            EPathMap::from_set_iter([integer(initial), integer(replacement)], vec![3], true, None)
        };
        prop_assert_eq!(changed.trie_snapshot(), expected.trie_snapshot());
        prop_assert_ne!(changed.trie_snapshot(), sibling.trie_snapshot());
    }
}

#[test]
fn mutation_clears_both_caches_before_exposing_repr() {
    for shared in [false, true] {
        for map in [false, true] {
            let mut current = fixture(map, 10);
            let bytes = current.trie_snapshot().to_vec();
            let old_metadata = metadata(&current.ps);
            let sibling = shared.then(|| current.clone());
            assert!(catch_unwind(AssertUnwindSafe(|| {
                let _ = current.ps.representation_mut();
                assert_cold(&current.ps);
                panic!("injected mutation-entry interruption");
            }))
            .is_err());
            assert_cold(&current.ps);
            assert_eq!(metadata(&current.ps), old_metadata);
            if let Some(sibling) = sibling {
                assert!(!Arc::ptr_eq(&current.ps.storage, &sibling.ps.storage));
                assert!(sibling.ps.storage.trie_snapshot.get().is_some());
                assert!(sibling.ps.storage.epm_layout.get().is_some());
                assert_eq!(sibling.trie_snapshot(), bytes);
            }
            assert_eq!(current.trie_snapshot(), bytes);
        }
    }
}

#[test]
fn no_op_mutations_preserve_storage_metadata_and_caches() {
    for map in [false, true] {
        let mut current = fixture(map, 10);
        current.create_path(b"dangling").unwrap();
        let bytes = current.trie_snapshot().to_vec();
        let sibling = current.clone();
        let before = metadata(&current.ps);
        assert!(!current.ps.remove_encoded_entry(b"absent"));
        assert!(!current.ps.remove_branches_at(b"absent"));
        assert!(!current.ps.remove_subtrie_at(b"absent"));
        assert!(!current.ps.create_path(b"dangling").unwrap());
        if !map {
            current.ps.extend_entries(&EntryTrie::default());
        }
        assert!(Arc::ptr_eq(&current.ps.storage, &sibling.ps.storage));
        assert_eq!(metadata(&current.ps), before);
        assert_eq!(current.ps.storage.trie_snapshot.get().unwrap(), &bytes);
        assert!(current.ps.storage.epm_layout.get().is_some());

        let mut empty =
            EntryTrie::from_owned_repr_with_key_validation(topology_owner(map, 8), true).unwrap();
        empty.trie_snapshot();
        let sibling = empty.clone();
        if map {
            assert!(empty.remove_greatest_map_entry().unwrap().is_none());
        } else {
            assert!(empty.remove_greatest_entry().is_none());
        }
        assert!(Arc::ptr_eq(&empty.storage, &sibling.storage));
        assert!(empty.storage.trie_snapshot.get().is_some());

        for subtrie in [false, true] {
            let mut native = pathmap::PathMap::<()>::new();
            native_create_path(&mut native, &[7]);
            let expected = native.remove_branches_at([7], true);
            let repr = if map {
                let mut trie = pathmap::PathMap::<Par>::new();
                native_create_path(&mut trie, &[7]);
                EPathMapRepr::Map(trie)
            } else {
                let mut trie = pathmap::PathMap::<()>::new();
                native_create_path(&mut trie, &[7]);
                EPathMapRepr::Set(trie)
            };
            let mut entries = EntryTrie::from_repr(repr).unwrap();
            entries.trie_snapshot();
            let sibling = entries.clone();
            let actual = if subtrie {
                entries.remove_subtrie_at(&[7])
            } else {
                entries.remove_branches_at(&[7])
            };
            assert_eq!(actual, expected);
            let exists = match entries.representation() {
                EPathMapRepr::Empty => false,
                EPathMapRepr::Set(trie) => trie.path_exists_at([7]),
                EPathMapRepr::Map(trie) => trie.path_exists_at([7]),
            };
            assert_eq!(exists, native.path_exists_at([7]));
            assert!(match sibling.representation() {
                EPathMapRepr::Set(trie) => trie.path_exists_at([7]),
                EPathMapRepr::Map(trie) => trie.path_exists_at([7]),
                EPathMapRepr::Empty => false,
            });
        }
    }
}

#[test]
fn resident_owners_retire_unique_shared_and_topology_only_on_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for map in [false, true] {
                for shared in [false, true] {
                    for values in [false, true] {
                        let mut entries = EntryTrie::from_owned_repr_with_key_validation(
                            topology_owner(map, 8192),
                            true,
                        )
                        .unwrap();
                        if values {
                            if map {
                                entries.insert_map_entry(integer(1), integer(2)).unwrap();
                            } else {
                                entries.insert_entry(integer(1));
                            }
                        }
                        let sibling = shared.then(|| entries.clone());
                        drop(entries);
                        if let Some(sibling) = sibling {
                            assert_eq!(sibling.len, usize::from(values));
                            let mut path = vec![0; 8192];
                            *path.last_mut().unwrap() = 1;
                            assert!(match sibling.representation() {
                                EPathMapRepr::Set(map) => map.path_exists_at(&path),
                                EPathMapRepr::Map(map) => map.path_exists_at(&path),
                                EPathMapRepr::Empty => false,
                            });
                            drop(sibling);
                        }
                    }
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn guarded_admission_rejection_and_replacement_preserve_previous_state() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for map in [false, true] {
                let mut current = fixture(map, 7);
                let bytes = current.trie_snapshot().to_vec();
                let sibling = current.clone();
                assert!(current
                    .replace_owned_decoded_representation_with(topology_owner(!map, 8192), true,)
                    .is_err());
                assert!(current.shares_storage_with(&sibling));
                assert_eq!(current.trie_snapshot(), bytes);
                let mut invalid = topology_owner(map, 8192);
                match invalid.as_repr_mut() {
                    EPathMapRepr::Set(map) => {
                        native_insert(map, b"invalid", ());
                    }
                    EPathMapRepr::Map(map) => {
                        native_insert(map, b"invalid", integer(2));
                    }
                    EPathMapRepr::Empty => unreachable!(),
                }
                assert!(current
                    .replace_owned_decoded_representation_with(invalid, true)
                    .is_err());
                assert!(current.shares_storage_with(&sibling));
                assert_eq!(current.trie_snapshot(), bytes);
                let mut raw_owner = topology_owner(true, 8192);
                let EPathMapRepr::Map(incoming) = raw_owner.as_repr_mut() else {
                    unreachable!()
                };
                native_insert(incoming, b"invalid", deep_value(8192));
                let raw = std::mem::take(raw_owner.as_repr_mut());
                assert!(current
                    .replace_decoded_representation_with(raw, true)
                    .is_err());
                assert!(current.shares_storage_with(&sibling));
                assert_eq!(current.trie_snapshot(), bytes);
                current
                    .replace_owned_decoded_representation_with(topology_owner(map, 8192), true)
                    .unwrap();
                assert!(!current.shares_storage_with(&sibling));
                current
                    .replace_owned_decoded_representation_with(
                        OwnedResidentEPathMap::default(),
                        true,
                    )
                    .unwrap();
                assert_eq!(current.mode(), EPathMapMode::Empty);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn algebra_and_consuming_transfers_preserve_values_and_metadata() {
    let mut neutral = EntryTrie::default();
    let typed_empty = EntryTrie::from_repr(EPathMapRepr::Set(pathmap::PathMap::new())).unwrap();
    assert_eq!(typed_empty.mode(), EPathMapMode::Set);
    neutral.extend_entries(&typed_empty);
    assert_eq!(neutral.mode(), EPathMapMode::Set);
    assert_eq!(neutral.trie_snapshot(), typed_empty.trie_snapshot());
    let mut set = fixture(false, 1);
    set.trie_snapshot();
    let sibling = set.clone();
    set.ps.extend_entries(&fixture(false, 2).ps);
    assert!(!set.shares_storage_with(&sibling));
    assert_eq!(sibling.trie_snapshot(), fixture(false, 1).trie_snapshot());
    let expected = EPathMap::from_set_iter([integer(1), integer(2)], vec![3], true, None);
    assert_eq!(set.trie_snapshot(), expected.trie_snapshot());
    let joined = set.ps.try_join(&set.ps).unwrap();
    assert!(Arc::ptr_eq(&joined.storage, &set.ps.storage));
    drop(joined);
    for shared in [false, true] {
        let current = fixture(true, 3);
        let sibling = shared.then(|| current.clone());
        let mut parts = current.into_owned_parts();
        assert_eq!(parts.locally_free, vec![3]);
        assert!(parts.connective_used);
        let Some(OwnedEPathMapEntry::Map { key, value }) = parts.entries.next() else {
            panic!("expected one map entry");
        };
        assert_eq!(key, encode_trie_path(&integer(1)));
        assert_eq!(value, integer(3));
        assert!(parts.entries.next().is_none());
        if let Some(sibling) = sibling {
            assert_eq!(sibling.trie_snapshot(), fixture(true, 3).trie_snapshot());
        }
    }
}
