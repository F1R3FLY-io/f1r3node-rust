use std::collections::BTreeMap;

use super::{Bag, BagDelta, Multiplicity};

#[derive(Clone, Copy, Debug)]
enum Construction {
    PrefixLowerThenExactHigher,
    PrefixHigherThenExactLower,
    PrefixBulkLowerThenHigher,
    PrefixBulkHigherThenLower,
    ExactLowerThenHigher,
    ExactHigherThenLower,
}

#[derive(Clone, Copy, Debug)]
enum Ownership {
    Unique,
    SharedDropSourceFirst,
    SharedDropSuccessorFirst,
}

fn long_same_node_keys() -> ([Vec<u8>; 2], Vec<u8>) {
    let mut lower = vec![0xa5; 939];
    let mut higher = lower.clone();
    lower[203] = 0x11;
    higher[203] = 0x22;
    let prefix = lower[..203].to_vec();
    assert_eq!(
        lower
            .iter()
            .zip(&higher)
            .take_while(|(left, right)| left == right)
            .count(),
        prefix.len()
    );
    ([lower, higher], prefix)
}

fn insert_count(bag: &mut Bag, path: &[u8], count: u64) {
    bag.insert_many(path, Multiplicity::new(count).unwrap())
        .unwrap();
}

fn construct(
    keys: &[Vec<u8>; 2],
    prefix: &[u8],
    counts: [u64; 2],
    construction: Construction,
) -> Bag {
    let mut result = Bag::new();
    match construction {
        Construction::PrefixLowerThenExactHigher => {
            let mut prepared = Bag::new();
            insert_count(&mut prepared, &keys[0][prefix.len()..], counts[0]);
            result.replace_prefix_with_relative(prefix, &prepared, false);
            insert_count(&mut result, &keys[1], counts[1]);
        }
        Construction::PrefixHigherThenExactLower => {
            let mut prepared = Bag::new();
            insert_count(&mut prepared, &keys[1][prefix.len()..], counts[1]);
            result.replace_prefix_with_relative(prefix, &prepared, false);
            insert_count(&mut result, &keys[0], counts[0]);
        }
        Construction::PrefixBulkLowerThenHigher | Construction::PrefixBulkHigherThenLower => {
            let mut prepared = Bag::new();
            let order = match construction {
                Construction::PrefixBulkLowerThenHigher => [0, 1],
                Construction::PrefixBulkHigherThenLower => [1, 0],
                _ => unreachable!(),
            };
            for index in order {
                insert_count(&mut prepared, &keys[index][prefix.len()..], counts[index]);
            }
            result.replace_prefix_with_relative(prefix, &prepared, false);
        }
        Construction::ExactLowerThenHigher | Construction::ExactHigherThenLower => {
            let order = match construction {
                Construction::ExactLowerThenHigher => [0, 1],
                Construction::ExactHigherThenLower => [1, 0],
                _ => unreachable!(),
            };
            for index in order {
                insert_count(&mut result, &keys[index], counts[index]);
            }
        }
    }
    result
}

fn expected_bag(entries: &BTreeMap<Vec<u8>, u64>) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        insert_count(&mut result, path, *count);
    }
    result
}

fn observed_entries(bag: &Bag) -> Vec<(Vec<u8>, u64)> {
    bag.iter()
        .map(|(path, multiplicity)| (path, multiplicity.get()))
        .collect()
}

fn assert_all_observations(
    actual: &Bag,
    expected: &BTreeMap<Vec<u8>, u64>,
    explicit_absent: &[&[u8]],
    prefixes: &[Vec<u8>],
) {
    for (path, count) in expected {
        assert_eq!(actual.count(path), *count, "point lookup for {path:?}");
    }
    for path in explicit_absent {
        assert_eq!(actual.count(path), 0, "absent point lookup for {path:?}");
    }

    let expected_entries = expected
        .iter()
        .map(|(path, count)| (path.clone(), *count))
        .collect::<Vec<_>>();
    assert_eq!(observed_entries(actual), expected_entries, "full iterator");
    assert_eq!(actual.distinct_len(), expected.len(), "distinct metadata");
    assert_eq!(
        actual.exact_cardinality(),
        expected.values().map(|count| u128::from(*count)).sum(),
        "cardinality metadata"
    );
    assert_eq!(actual.prefix_value_count(b""), expected.len(), "root zipper value count");

    for prefix in prefixes {
        let expected_prefix = expected_entries
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .cloned()
            .collect::<Vec<_>>();
        let actual_prefix = actual
            .entries_with_prefix(prefix)
            .into_iter()
            .map(|(path, multiplicity)| (path, multiplicity.get()))
            .collect::<Vec<_>>();
        assert_eq!(actual_prefix, expected_prefix, "prefix traversal at {prefix:?}");
        assert_eq!(
            actual.prefix_value_count(prefix),
            expected_prefix.len(),
            "prefix zipper value count at {prefix:?}"
        );
    }

    let rebuilt = expected_bag(expected);
    assert_eq!(actual.compact_bytes(), rebuilt.compact_bytes(), "canonical ACT bytes");
    let decoded = Bag::from_compact_bytes(&actual.compact_bytes()).unwrap();
    assert_eq!(observed_entries(&decoded), expected_entries, "ACT decode endpoints");
    assert_eq!(decoded.compact_bytes(), actual.compact_bytes(), "ACT byte round trip");
}

#[test]
fn destructive_publication_is_coherent_for_every_mixed_construction_and_cow_order() {
    let (keys, prefix) = long_same_node_keys();
    let constructions = [
        Construction::PrefixLowerThenExactHigher,
        Construction::PrefixHigherThenExactLower,
        Construction::PrefixBulkLowerThenHigher,
        Construction::PrefixBulkHigherThenLower,
        Construction::ExactLowerThenHigher,
        Construction::ExactHigherThenLower,
    ];
    let ownerships =
        [Ownership::Unique, Ownership::SharedDropSourceFirst, Ownership::SharedDropSuccessorFirst];
    let prefixes =
        vec![Vec::new(), prefix.clone(), keys[0][..204].to_vec(), keys[1][..204].to_vec(), vec![
            0xff,
        ]];

    for construction in constructions {
        for selected in [0usize, 1] {
            for (counts, removal) in [([1, 1], 1), ([3, 5], 1), ([3, 5], 3)] {
                for ownership in ownerships {
                    let mut actual = construct(&keys, &prefix, counts, construction);
                    let before = BTreeMap::from([
                        (keys[0].clone(), counts[0]),
                        (keys[1].clone(), counts[1]),
                    ]);
                    assert_all_observations(&actual, &before, &[], &prefixes);
                    let source_identity = actual.anchor_identity();
                    let source = (!matches!(ownership, Ownership::Unique)).then(|| actual.clone());
                    let mut expected = before.clone();
                    let selected_count = expected[&keys[selected]];
                    let removes_endpoint = selected_count == removal;
                    if removes_endpoint {
                        expected.remove(&keys[selected]);
                    } else {
                        expected.insert(keys[selected].clone(), selected_count - removal);
                    }
                    let absent = removes_endpoint
                        .then_some(keys[selected].as_slice())
                        .into_iter()
                        .collect::<Vec<_>>();

                    let mut delta = BagDelta::new();
                    delta
                        .record_removal(&keys[selected], Multiplicity::new(removal).unwrap())
                        .unwrap();
                    delta.apply_to(&mut actual).unwrap();
                    if matches!(ownership, Ownership::Unique) && !removes_endpoint {
                        assert_eq!(
                            actual.anchor_identity(),
                            source_identity,
                            "a unique value-only decrement retains its write-ready anchor: \
                             {construction:?}, selected={selected}"
                        );
                    } else {
                        assert_ne!(
                            actual.anchor_identity(),
                            source_identity,
                            "shared value updates and every structural deletion publish a private \
                             candidate: {construction:?}, selected={selected}, \
                             ownership={ownership:?}"
                        );
                    }
                    assert_all_observations(&actual, &expected, &absent, &prefixes);

                    if let Some(source) = source {
                        assert_eq!(source.anchor_identity(), source_identity);
                        assert_all_observations(&source, &before, &[], &prefixes);
                        match ownership {
                            Ownership::SharedDropSourceFirst => {
                                drop(source);
                                assert_all_observations(&actual, &expected, &absent, &prefixes);
                            }
                            Ownership::SharedDropSuccessorFirst => {
                                drop(actual);
                                assert_all_observations(&source, &before, &[], &prefixes);
                            }
                            Ownership::Unique => unreachable!(),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unique_value_only_updates_retain_the_anchor_across_bag_apis() {
    fn fixture() -> Bag {
        let mut bag = Bag::new();
        insert_count(&mut bag, b"shared/prefix/selected", 7);
        insert_count(&mut bag, b"shared/prefix/sibling", 11);
        bag
    }

    let selected = b"shared/prefix/selected";
    let decrement = Multiplicity::new(2).unwrap();

    let mut via_remove_many = fixture();
    let anchor = via_remove_many.anchor_identity();
    assert_eq!(
        via_remove_many
            .remove_many(selected, decrement)
            .unwrap()
            .get(),
        5
    );
    assert_eq!(via_remove_many.anchor_identity(), anchor);

    let mut via_try_remove_many = fixture();
    let anchor = via_try_remove_many.anchor_identity();
    assert_eq!(
        via_try_remove_many
            .try_remove_many(selected, decrement)
            .unwrap()
            .unwrap()
            .get(),
        5
    );
    assert_eq!(via_try_remove_many.anchor_identity(), anchor);

    let mut via_set_count = fixture();
    let anchor = via_set_count.anchor_identity();
    via_set_count.set_count(selected, Some(Multiplicity::new(5).unwrap()));
    assert_eq!(via_set_count.anchor_identity(), anchor);

    let mut via_delta = fixture();
    let anchor = via_delta.anchor_identity();
    let mut delta = BagDelta::new();
    delta.record_removal(selected, decrement).unwrap();
    delta.apply_to(&mut via_delta).unwrap();
    assert_eq!(via_delta.anchor_identity(), anchor);
}

#[test]
fn shared_value_only_updates_detach_without_mutating_the_alias() {
    let mut source = Bag::new();
    insert_count(&mut source, b"shared/prefix/selected", 7);
    insert_count(&mut source, b"shared/prefix/sibling", 11);
    let source_anchor = source.anchor_identity();
    let source_bytes = source.compact_bytes();

    let mut successor = source.clone();
    assert_eq!(successor.anchor_identity(), source_anchor);
    assert_eq!(
        successor
            .remove_many(b"shared/prefix/selected", Multiplicity::new(2).unwrap())
            .unwrap()
            .get(),
        5
    );

    assert_ne!(successor.anchor_identity(), source_anchor);
    assert_eq!(source.anchor_identity(), source_anchor);
    assert_eq!(source.compact_bytes(), source_bytes);
    assert_eq!(source.count(b"shared/prefix/selected"), 7);
    assert_eq!(source.count(b"shared/prefix/sibling"), 11);
    assert_eq!(successor.count(b"shared/prefix/selected"), 5);
    assert_eq!(successor.count(b"shared/prefix/sibling"), 11);
}
