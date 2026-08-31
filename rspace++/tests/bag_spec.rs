use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use multiset::HashMultiSet;
use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::ring::{AlgebraicResult, DistributiveLattice, Lattice, SELF_IDENT};
use proptest::collection::vec;
use proptest::prelude::*;
use rspace_plus_plus::rspace::bag::{
    Bag, BagCodecError, BagDelta, BagError, Multiplicity, SortedUniqueBagError,
};

fn bag(entries: &[(&[u8], u64)]) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        result
            .insert_many(path, Multiplicity::new(*count).unwrap())
            .unwrap();
    }
    result
}

#[test]
fn multiplicity_rejects_zero_and_detects_overflow() {
    assert_eq!(Multiplicity::new(0), Err(BagError::ZeroMultiplicity));
    let mut value = Bag::new();
    value
        .insert_many(b"x", Multiplicity::new(u64::MAX).unwrap())
        .unwrap();
    assert_eq!(
        value.insert(b"x"),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1,
        })
    );
    assert_eq!(value.count(b"x"), u64::MAX);
    assert_eq!(
        Multiplicity::new(2)
            .unwrap()
            .checked_sub(Multiplicity::new(3).unwrap()),
        Err(BagError::InsufficientMultiplicity {
            available: 2,
            requested: 3,
        })
    );
}

#[test]
fn sorted_unique_stream_builds_one_exact_prefix_sharing_bag() {
    let expected = bag(&[(b"", 1), (b"a", 2), (b"ab", 3), (b"b", 4)]);
    let actual = Bag::try_from_sorted_unique(|writer| {
        for (path, count) in [(&b""[..], 1), (&b"a"[..], 2), (&b"ab"[..], 3), (&b"b"[..], 4)] {
            writer.push(path, Multiplicity::new(count).unwrap())?;
        }
        Ok::<(), SortedUniqueBagError>(())
    })
    .unwrap();

    assert_eq!(actual, expected);
    assert_eq!(actual.distinct_len(), 4);
    assert_eq!(actual.cardinality(), 10);
    assert_eq!(actual.compact_bytes(), expected.compact_bytes());
}

#[test]
fn sorted_unique_stream_rejects_duplicates_and_reversal_without_publication() {
    for paths in [[&b"a"[..], &b"a"[..]], [&b"b"[..], &b"a"[..]], [&b""[..], &b""[..]]] {
        let result = Bag::try_from_sorted_unique(|writer| {
            for path in paths {
                writer.push(path, Multiplicity::ONE)?;
            }
            Ok::<(), SortedUniqueBagError>(())
        });
        assert_eq!(result.unwrap_err(), SortedUniqueBagError::NonAscending);
    }
}

#[test]
fn cardinality_uses_the_multiplicity_domain_without_breaking_bag_closure() {
    let maximum = Multiplicity::new(u64::MAX).unwrap();
    let mut value = bag(&[(b"alpha", u64::MAX)]);
    assert_eq!(value.cardinality(), u64::MAX);
    assert!(!value.cardinality_is_saturated());

    value.insert(b"beta").unwrap();
    assert_eq!(value.cardinality(), u64::MAX);
    assert!(value.cardinality_is_saturated());
    assert_eq!(value.count(b"alpha"), u64::MAX);
    assert_eq!(value.count(b"beta"), 1);

    let round_trip = Bag::from_compact_bytes(&value.compact_bytes()).unwrap();
    assert_eq!(round_trip, value);
    assert_eq!(round_trip.cardinality(), u64::MAX);
    assert!(round_trip.cardinality_is_saturated());

    value.remove(b"beta");
    assert_eq!(value.cardinality(), u64::MAX);
    assert!(!value.cardinality_is_saturated());

    let left = bag(&[(b"alpha", u64::MAX)]);
    let right = bag(&[(b"beta", u64::MAX)]);
    let joined = left.join(&right);
    assert_eq!(joined.count(b"alpha"), maximum.get());
    assert_eq!(joined.count(b"beta"), maximum.get());
    assert_eq!(joined.cardinality(), u64::MAX);
    assert!(joined.cardinality_is_saturated());
}

#[test]
fn zipper_scaling_is_exact_and_rejects_endpoint_overflow_atomically() {
    // Include the empty path so the test covers a value at the zipper root as
    // well as prefix-sharing descendants and a disjoint branch.
    let source = bag(&[(b"", 11), (b"alpha", 2), (b"alpine", 3), (b"beta", 5)]);
    assert!(source.scale(0).unwrap().is_empty());
    assert_eq!(source.scale(1).unwrap(), source);

    let scaled = source.scale(7).unwrap();
    assert_eq!(scaled.count(b""), 77);
    assert_eq!(scaled.count(b"alpha"), 14);
    assert_eq!(scaled.count(b"alpine"), 21);
    assert_eq!(scaled.count(b"beta"), 35);
    assert_eq!(scaled.distinct_len(), source.distinct_len());
    assert_eq!(scaled.cardinality(), 147);
    assert_eq!(source.cardinality(), 21);

    let maximum = bag(&[(b"maximum", u64::MAX)]);
    assert_eq!(
        maximum.scale(2),
        Err(BagError::MultiplicityProductOverflow {
            current: u64::MAX,
            factor: 2,
        })
    );
    assert_eq!(maximum.count(b"maximum"), u64::MAX);
    assert!(!maximum.cardinality_is_saturated());
}

#[test]
fn multiplicity_bincode_rejects_zero() {
    let encoded_zero = bincode::serialize(&0u64).unwrap();
    assert!(bincode::deserialize::<Multiplicity>(&encoded_zero).is_err());
    let original = Multiplicity::new(17).unwrap();
    let encoded = bincode::serialize(&original).unwrap();
    assert_eq!(bincode::deserialize::<Multiplicity>(&encoded).unwrap(), original);
}

#[test]
fn insert_remove_and_metadata_preserve_the_absent_zero_invariant() {
    let mut value = Bag::new();
    value.insert(b"alpha").unwrap();
    value
        .insert_many(b"alpha", Multiplicity::new(2).unwrap())
        .unwrap();
    value.insert(b"beta").unwrap();
    assert_eq!(value.count(b"alpha"), 3);
    assert_eq!(value.distinct_len(), 2);
    assert_eq!(value.cardinality(), 4);
    assert_eq!(value.remove(b"alpha").unwrap().get(), 2);
    assert_eq!(value.remove_many(b"alpha", Multiplicity::new(9).unwrap()), None);
    assert!(!value.contains(b"alpha"));
    assert_eq!(value.distinct_len(), 1);
    assert_eq!(value.cardinality(), 1);
}

#[test]
fn exact_removal_is_atomic_on_insufficient_multiplicity() {
    let mut value = bag(&[(b"alpha", 2), (b"beta", 1)]);
    let before = value.clone();
    assert_eq!(
        value.try_remove_many(b"alpha", Multiplicity::new(3).unwrap()),
        Err(BagError::InsufficientMultiplicity {
            available: 2,
            requested: 3,
        })
    );
    assert_eq!(value, before);
    assert_eq!(
        value.try_remove_many(b"missing", Multiplicity::ONE),
        Err(BagError::InsufficientMultiplicity {
            available: 0,
            requested: 1,
        })
    );
    assert_eq!(value, before);
}

#[test]
fn normalized_transaction_delta_applies_and_reverts_exactly() {
    let original = bag(&[(b"alpha", 5), (b"beta", 1)]);
    let mut delta = BagDelta::new();
    delta
        .record_removal(b"alpha", Multiplicity::new(4).unwrap())
        .unwrap();
    delta.record_addition(b"alpha", Multiplicity::ONE).unwrap();
    delta
        .record_addition(b"beta", Multiplicity::new(2).unwrap())
        .unwrap();
    delta
        .record_addition(b"gamma", Multiplicity::new(3).unwrap())
        .unwrap();
    delta.record_removal(b"gamma", Multiplicity::ONE).unwrap();

    assert_eq!(delta.removals(), &bag(&[(b"alpha", 3)]));
    assert_eq!(delta.additions(), &bag(&[(b"beta", 2), (b"gamma", 2)]));

    let mut changed = original.clone();
    delta.apply_to(&mut changed).unwrap();
    assert_eq!(changed, bag(&[(b"alpha", 2), (b"beta", 3), (b"gamma", 2)]));
    delta.revert_from(&mut changed).unwrap();
    assert_eq!(changed, original);
    assert_eq!(delta.inverse().inverse(), delta);
}

#[test]
fn transaction_cursor_preserves_exact_prefix_overlaps_and_prunes_deleted_leaves() {
    let original = bag(&[
        (b"shared", 2),
        (b"shared/a", 1),
        (b"shared/a/deep", 3),
        (b"shared/b", 4),
        (b"unrelated", 5),
    ]);
    let mut delta = BagDelta::new();
    delta.record_removal(b"shared", Multiplicity::ONE).unwrap();
    delta
        .record_removal(b"shared/a", Multiplicity::ONE)
        .unwrap();
    delta
        .record_removal(b"shared/a/deep", Multiplicity::new(2).unwrap())
        .unwrap();
    delta
        .record_addition(b"shared/aa", Multiplicity::new(7).unwrap())
        .unwrap();
    delta
        .record_addition(b"shared/b", Multiplicity::new(2).unwrap())
        .unwrap();

    let mut changed = original.clone();
    delta.apply_to(&mut changed).unwrap();
    assert_eq!(
        changed,
        bag(&[
            (b"shared", 1),
            (b"shared/a/deep", 1),
            (b"shared/aa", 7),
            (b"shared/b", 6),
            (b"unrelated", 5),
        ])
    );
    assert_eq!(changed.distinct_len(), 5);
    assert_eq!(changed.cardinality(), 20);
    assert_eq!(Bag::from_compact_bytes(&changed.compact_bytes()).unwrap(), changed);

    delta.revert_from(&mut changed).unwrap();
    assert_eq!(changed, original);
}

#[test]
fn transaction_delta_application_is_atomic_on_underflow_and_overflow() {
    // `alpha` is visited before the absent `zeta`. A one-pass mutation would
    // therefore decrement it before discovering the late underflow.
    let mut underflow_target = bag(&[(b"alpha", 2)]);
    let underflow_before = underflow_target.clone();
    let mut underflow = BagDelta::new();
    underflow
        .record_removal(b"alpha", Multiplicity::ONE)
        .unwrap();
    underflow
        .record_removal(b"zeta", Multiplicity::new(3).unwrap())
        .unwrap();
    assert_eq!(
        underflow.apply_to(&mut underflow_target),
        Err(BagError::InsufficientMultiplicity {
            available: 0,
            requested: 3,
        })
    );
    assert_eq!(underflow_target, underflow_before);

    // Likewise, inserting `alpha` must not become visible when the later
    // `omega` addition overflows.
    let mut overflow_target = bag(&[(b"omega", u64::MAX)]);
    let overflow_before = overflow_target.clone();
    let mut overflow = BagDelta::new();
    overflow
        .record_addition(b"alpha", Multiplicity::ONE)
        .unwrap();
    overflow
        .record_addition(b"omega", Multiplicity::ONE)
        .unwrap();
    assert_eq!(
        overflow.apply_to(&mut overflow_target),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1,
        })
    );
    assert_eq!(overflow_target, overflow_before);
}

#[test]
fn sum_is_additive_while_join_and_meet_are_lattice_operations() {
    let left = bag(&[(b"a", 2), (b"b", 5)]);
    let right = bag(&[(b"a", 3), (b"c", 7)]);
    let sum = left.sum(&right).unwrap();
    let join = left.join(&right);
    let meet = left.meet(&right);
    assert_eq!(sum.count(b"a"), 5);
    assert_eq!(join.count(b"a"), 3);
    assert_eq!(join.count(b"b"), 5);
    assert_eq!(join.count(b"c"), 7);
    assert_eq!(meet, bag(&[(b"a", 2)]));
    assert_eq!(left.join(&left), left);
    assert_ne!(left.sum(&left).unwrap(), left);
}

#[test]
fn meet_combines_exact_values_when_an_overlap_is_also_a_prefix() {
    let left = bag(&[(b"\x07", 7), (b"\x08", 1)]);
    let right = bag(&[(b"\x07", 1), (b"\x07\x00", 1), (b"\x08", 1)]);
    assert_eq!(left.meet(&right), bag(&[(b"\x07", 1), (b"\x08", 1)]));
    assert_eq!(right.meet(&left), bag(&[(b"\x07", 1), (b"\x08", 1)]));
}

#[test]
fn bag_lattice_satisfies_commutativity_associativity_and_absorption() {
    let a = bag(&[(b"shared", 2), (b"a", 8)]);
    let b = bag(&[(b"shared", 5), (b"b", 3)]);
    let c = bag(&[(b"shared", 4), (b"c", 9)]);
    assert_eq!(a.join(&b), b.join(&a));
    assert_eq!(a.meet(&b), b.meet(&a));
    assert_eq!(a.join(&b).join(&c), a.join(&b.join(&c)));
    assert_eq!(a.meet(&b).meet(&c), a.meet(&b.meet(&c)));
    assert_eq!(a.join(&a.meet(&b)), a);
    assert_eq!(a.meet(&a.join(&b)), a);
}

#[test]
fn subtraction_is_truncated_and_reports_the_identity_case() {
    let left = bag(&[(b"a", 8), (b"b", 2)]);
    let right = bag(&[(b"a", 3), (b"b", 9)]);
    assert_eq!(left.subtract(&right), bag(&[(b"a", 5)]));
    let disjoint = bag(&[(b"z", 1)]);
    assert!(matches!(left.psubtract(&disjoint), AlgebraicResult::Identity(SELF_IDENT)));
}

#[test]
fn prefix_queries_remain_in_trie_order() {
    let value =
        bag(&[(b"prefix/a", 2), (b"prefix/a/deeper", 3), (b"prefix/b", 4), (b"unrelated", 5)]);
    assert_eq!(value.entries_with_prefix(b"prefix/"), vec![
        (b"prefix/a".to_vec(), Multiplicity::new(2).unwrap()),
        (b"prefix/a/deeper".to_vec(), Multiplicity::new(3).unwrap()),
        (b"prefix/b".to_vec(), Multiplicity::new(4).unwrap()),
    ]);
}

#[test]
fn relative_prefix_queries_borrow_suffixes_and_include_the_prefix_endpoint() {
    let value = bag(&[
        (b"prefix", 1),
        (b"prefix/a", 2),
        (b"prefix/a/deeper", 3),
        (b"prefix/b", 4),
        (b"unrelated", 5),
    ]);
    let mut relative = Vec::new();
    value.visit_relative_prefix(b"prefix", |suffix, multiplicity| {
        relative.push((suffix.to_vec(), multiplicity));
    });
    assert_eq!(relative, vec![
        (Vec::new(), Multiplicity::ONE),
        (b"/a".to_vec(), Multiplicity::new(2).unwrap()),
        (b"/a/deeper".to_vec(), Multiplicity::new(3).unwrap()),
        (b"/b".to_vec(), Multiplicity::new(4).unwrap()),
    ]);

    value.visit_relative_prefix(b"missing", |_suffix, _multiplicity| {
        panic!("a nonexistent prefix must not escape to an adjacent trie branch");
    });

    let adjacent = bag(&[(&[140], 1), (&[141], 1), (&[], 1)]);
    let mut selected = Vec::new();
    adjacent.visit_relative_prefix(&[140], |suffix, multiplicity| {
        selected.push((suffix.to_vec(), multiplicity));
    });
    assert_eq!(selected, vec![(Vec::new(), Multiplicity::ONE)]);
}

#[test]
fn prefix_removal_detaches_only_the_selected_subtrie_and_updates_metadata() {
    let mut value =
        bag(&[(b"prefix/a", 2), (b"prefix/a/deeper", 3), (b"prefix/b", 4), (b"other", 5)]);
    assert_eq!(value.remove_prefix(b"prefix/a"), (2, 5));
    assert_eq!(value.entries_with_prefix(b"prefix/"), vec![(
        b"prefix/b".to_vec(),
        Multiplicity::new(4).unwrap()
    ),]);
    assert_eq!(value.distinct_len(), 2);
    assert_eq!(value.cardinality(), 9);
}

#[test]
fn clone_is_copy_on_write_and_preserves_canonical_bytes() {
    let original = bag(&[(b"shared/left", 2), (b"shared/right", 3)]);
    let mut fork = original.clone();
    fork.insert(b"shared/new").unwrap();
    assert_eq!(original.count(b"shared/new"), 0);
    assert_eq!(Bag::from_compact_bytes(&fork.compact_bytes()).unwrap(), fork);
}

#[test]
fn deep_keys_clone_and_drop_on_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let key = vec![0xA5; 1_000_000];
            let mut value = Bag::new();
            value.insert(&key).unwrap();
            let fork = value.clone();
            drop(value);
            assert_eq!(fork.count(&key), 1);
            drop(fork);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_prefix_removal_is_stack_safe_and_preserves_the_adjacent_branch() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut selected = vec![0xAA];
            selected.extend(std::iter::repeat_n(0x5A, 200_000));
            let adjacent = vec![0xAB, 0x01];
            let mut value = Bag::new();
            value
                .insert_many(&selected, Multiplicity::new(17).unwrap())
                .unwrap();
            value
                .insert_many(&adjacent, Multiplicity::new(3).unwrap())
                .unwrap();

            assert_eq!(value.remove_prefix(&[0xAA]), (1, 17));
            assert_eq!(value.count(&selected), 0);
            assert_eq!(value.count(&adjacent), 3);
            assert_eq!(value.distinct_len(), 1);
            assert_eq!(value.cardinality(), 3);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn trait_algebra_agrees_with_inherent_algebra() {
    let left = bag(&[(b"a", 2), (b"b", 1)]);
    let right = bag(&[(b"a", 3), (b"c", 1)]);
    assert!(
        matches!(left.pjoin(&right), AlgebraicResult::Element(value) if value == left.join(&right))
    );
    assert!(
        matches!(left.pmeet(&right), AlgebraicResult::Element(value) if value == left.meet(&right))
    );
}

#[test]
fn equality_order_hash_and_debug_share_one_canonical_entry_observation() {
    let left = bag(&[(b"z", 1), (b"a", 2), (b"middle", 3)]);
    let right = bag(&[(b"middle", 3), (b"z", 1), (b"a", 2)]);
    assert_eq!(left, right);
    assert_eq!(left.cmp(&right), std::cmp::Ordering::Equal);

    let mut left_hash = DefaultHasher::new();
    left.hash(&mut left_hash);
    let mut right_hash = DefaultHasher::new();
    right.hash(&mut right_hash);
    assert_eq!(left_hash.finish(), right_hash.finish());

    assert!(bag(&[(b"a", 1)]) < bag(&[(b"a", 2)]));
    let rendered = format!("{left:?}");
    assert!(rendered.contains("distinct_len: 3"));
    assert!(rendered.contains("cardinality: 6"));
}

#[test]
fn compact_byte_image_is_the_canonical_pathmap_arena_for_empty_and_populated_bags() {
    for value in [Bag::new(), bag(&[(b"", 7), (b"shared/alpha", 2), (b"shared/beta", 9)])] {
        let image = value.compact_bytes();
        assert!(image.starts_with(b"ACTree03"));
        let decoded = Bag::from_compact_bytes(&image).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(decoded.compact_bytes(), image);

        let bincode_bytes = bincode::serialize(&value).unwrap();
        assert_eq!(&bincode_bytes[8..], image);
        assert_eq!(bincode::deserialize::<Bag>(&bincode_bytes).unwrap(), value);
    }
}

#[test]
fn compact_reader_rejects_malformed_non_bag_and_noncanonical_images() {
    let image = bag(&[(b"alpha", 2)]).compact_bytes();
    assert!(matches!(
        Bag::from_compact_bytes(&image[..image.len() - 1]),
        Err(BagCodecError::CompactTrie(_))
    ));

    let mut alternate = image.clone();
    alternate.push(0);
    assert!(matches!(Bag::from_compact_bytes(&alternate), Err(BagCodecError::CompactTrie(_))));

    let mut zero = PathMap::new();
    zero.insert(b"zero", 0u64);
    let zero_image = ArenaCompactTree::from_zipper(zero.read_zipper(), |value| *value);
    assert_eq!(
        Bag::from_compact_bytes(zero_image.get_data()),
        Err(BagCodecError::ZeroMultiplicity)
    );

    let mut dangling = PathMap::<u64>::new();
    dangling.create_path(b"dangling");
    let dangling_image = ArenaCompactTree::from_zipper(dangling.read_zipper(), |value| *value);
    assert_eq!(
        Bag::from_compact_bytes(dangling_image.get_data()),
        Err(BagCodecError::MissingMultiplicity)
    );
}

#[test]
fn semantic_decode_error_drops_a_deep_partial_bag_on_a_small_stack() {
    let mut source = PathMap::new();
    let deep_valid = vec![0xFE; 200_000];
    source.insert(&deep_valid, 7u64);
    source.insert([0xFF], 0u64);
    let image = ArenaCompactTree::from_zipper(source.read_zipper(), |value| *value)
        .get_data()
        .to_vec();
    drop(source);

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            assert_eq!(Bag::from_compact_bytes(&image), Err(BagCodecError::ZeroMultiplicity));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn compact_byte_image_round_trip_is_stack_safe_for_a_deep_compressed_key() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let path = vec![0x5a; 200_000];
            let mut value = Bag::new();
            value
                .insert_many(&path, Multiplicity::new(17).unwrap())
                .unwrap();
            let image = value.compact_bytes();
            let decoded = Bag::from_compact_bytes(&image).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(decoded.compact_bytes(), image);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn compact_byte_image_round_trip_is_stack_safe_for_a_wide_shared_prefix() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut value = Bag::new();
            for ordinal in 0..20_000u32 {
                let mut path = b"shared/wide/".to_vec();
                path.extend_from_slice(&ordinal.to_be_bytes());
                value
                    .insert_many(&path, Multiplicity::new(u64::from(ordinal % 17 + 1)).unwrap())
                    .unwrap();
            }
            let image = value.compact_bytes();
            let decoded = Bag::from_compact_bytes(&image).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(decoded.distinct_len(), 20_000);
            assert_eq!(decoded.compact_bytes(), image);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn oracle(entries: &[(Vec<u8>, u8)]) -> BTreeMap<Vec<u8>, u64> {
    let mut result = BTreeMap::new();
    for (path, count) in entries {
        *result.entry(path.clone()).or_default() += u64::from(*count);
    }
    result
}

fn from_oracle(entries: &BTreeMap<Vec<u8>, u64>) -> Bag {
    let mut result = Bag::new();
    for (path, count) in entries {
        result
            .insert_many(path, Multiplicity::new(*count).unwrap())
            .unwrap();
    }
    result
}

fn entry_strategy() -> impl Strategy<Value = Vec<(Vec<u8>, u8)>> {
    vec((vec(any::<u8>(), 0..8), 1u8..=16), 0..40)
}

#[derive(Clone, Debug)]
enum BagOperation {
    Insert(Vec<u8>, u8),
    Remove(Vec<u8>, u8),
    RemovePrefix(Vec<u8>),
}

fn operation_strategy() -> impl Strategy<Value = BagOperation> {
    prop_oneof![
        5 => (vec(any::<u8>(), 0..8), 1u8..=16)
            .prop_map(|(path, amount)| BagOperation::Insert(path, amount)),
        4 => (vec(any::<u8>(), 0..8), 1u8..=16)
            .prop_map(|(path, amount)| BagOperation::Remove(path, amount)),
        1 => vec(any::<u8>(), 0..5).prop_map(BagOperation::RemovePrefix),
    ]
}

fn hash_multiset_snapshot(oracle: &HashMultiSet<Vec<u8>>) -> BTreeMap<Vec<u8>, u64> {
    oracle
        .distinct_elements()
        .map(|path| (path.clone(), oracle.count_of(path) as u64))
        .collect()
}

proptest! {
    #[test]
    fn bag_mutations_match_the_legacy_hash_multiset_oracle(
        operations in vec(operation_strategy(), 0..160),
    ) {
        let mut value = Bag::new();
        let mut legacy = HashMultiSet::new();

        for operation in operations {
            match operation {
                BagOperation::Insert(path, amount) => {
                    value
                        .insert_many(&path, Multiplicity::new(u64::from(amount)).unwrap())
                        .unwrap();
                    legacy.insert_times(path, usize::from(amount));
                }
                BagOperation::Remove(path, amount) => {
                    let removed = legacy.remove_times(&path, usize::from(amount));
                    value.remove_many(&path, Multiplicity::new(u64::from(amount)).unwrap());
                    prop_assert_eq!(value.count(&path), legacy.count_of(&path) as u64);
                    prop_assert!(removed <= usize::from(amount));
                }
                BagOperation::RemovePrefix(prefix) => {
                    let keys = legacy
                        .distinct_elements()
                        .filter(|path| path.starts_with(&prefix))
                        .cloned()
                        .collect::<Vec<_>>();
                    let expected_distinct = keys.len();
                    let expected_cardinality = keys
                        .iter()
                        .map(|path| legacy.count_of(path))
                        .sum::<usize>();
                    for path in keys {
                        let count = legacy.count_of(&path);
                        legacy.remove_times(&path, count);
                    }
                    prop_assert_eq!(
                        value.remove_prefix(&prefix),
                        (
                            expected_distinct,
                            u64::try_from(expected_cardinality)
                                .expect("generated cardinality fits u64"),
                        ),
                    );
                }
            }

            let snapshot = hash_multiset_snapshot(&legacy);
            let expected = from_oracle(&snapshot);
            prop_assert_eq!(&value, &expected);
            prop_assert_eq!(value.distinct_len(), snapshot.len());
            prop_assert_eq!(
                value.cardinality(),
                u64::try_from(legacy.len()).expect("generated cardinality fits u64")
            );
        }
    }

    #[test]
    fn bag_algebra_matches_a_btreemap_multiplicity_oracle(
        left_entries in entry_strategy(),
        right_entries in entry_strategy(),
        third_entries in entry_strategy(),
    ) {
        let left_oracle = oracle(&left_entries);
        let right_oracle = oracle(&right_entries);
        let left = from_oracle(&left_oracle);
        let right = from_oracle(&right_oracle);
        let third = from_oracle(&oracle(&third_entries));

        let keys = left_oracle
            .keys()
            .chain(right_oracle.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let expected_sum = keys
            .iter()
            .filter_map(|key| {
                let count = left_oracle.get(key).copied().unwrap_or(0) +
                    right_oracle.get(key).copied().unwrap_or(0);
                (count != 0).then(|| (key.clone(), count))
            })
            .collect::<BTreeMap<_, _>>();
        let expected_join = keys
            .iter()
            .filter_map(|key| {
                let count = left_oracle
                    .get(key)
                    .copied()
                    .unwrap_or(0)
                    .max(right_oracle.get(key).copied().unwrap_or(0));
                (count != 0).then(|| (key.clone(), count))
            })
            .collect::<BTreeMap<_, _>>();
        let expected_meet = keys
            .iter()
            .filter_map(|key| {
                let count = left_oracle
                    .get(key)
                    .copied()
                    .unwrap_or(0)
                    .min(right_oracle.get(key).copied().unwrap_or(0));
                (count != 0).then(|| (key.clone(), count))
            })
            .collect::<BTreeMap<_, _>>();
        let expected_difference = keys
            .iter()
            .filter_map(|key| {
                let count = left_oracle
                    .get(key)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(right_oracle.get(key).copied().unwrap_or(0));
                (count != 0).then(|| (key.clone(), count))
            })
            .collect::<BTreeMap<_, _>>();

        prop_assert_eq!(left.sum(&right).unwrap(), from_oracle(&expected_sum));
        prop_assert_eq!(left.join(&right), from_oracle(&expected_join));
        prop_assert_eq!(left.meet(&right), from_oracle(&expected_meet));
        prop_assert_eq!(left.subtract(&right), from_oracle(&expected_difference));
        prop_assert_eq!(
            left.is_subbag_of(&right),
            left_oracle.iter().all(|(path, count)| {
                right_oracle.get(path).copied().unwrap_or(0) >= *count
            })
        );
        let compact = left.compact_bytes();
        prop_assert_eq!(Bag::from_compact_bytes(&compact).unwrap(), left.clone());
        let bincode_round_trip =
            bincode::deserialize::<Bag>(&bincode::serialize(&left).unwrap()).unwrap();
        prop_assert_eq!(&bincode_round_trip, &left);

        prop_assert_eq!(left.join(&right), right.join(&left));
        prop_assert_eq!(left.meet(&right), right.meet(&left));
        prop_assert_eq!(left.join(&left), left.clone());
        prop_assert_eq!(left.meet(&left), left.clone());
        prop_assert_eq!(left.join(&right).join(&third), left.join(&right.join(&third)));
        prop_assert_eq!(left.meet(&right).meet(&third), left.meet(&right.meet(&third)));
        prop_assert_eq!(left.join(&left.meet(&right)), left.clone());
        prop_assert_eq!(left.meet(&left.join(&right)), left.clone());
        prop_assert_eq!(
            left.meet(&right.join(&third)),
            left.meet(&right).join(&left.meet(&third)),
        );
        prop_assert_eq!(
            left.join(&right.meet(&third)),
            left.join(&right).meet(&left.join(&third)),
        );
        prop_assert_eq!(
            left.subtract(&right).sum(&left.meet(&right)).unwrap(),
            left.clone(),
        );

        let mut delta = BagDelta::new();
        for (path, amount) in left.subtract(&right).iter() {
            delta.record_removal(&path, amount).unwrap();
        }
        for (path, amount) in right.subtract(&left).iter() {
            delta.record_addition(&path, amount).unwrap();
        }
        let mut transformed = left.clone();
        delta.apply_to(&mut transformed).unwrap();
        prop_assert_eq!(&transformed, &right);
        delta.revert_from(&mut transformed).unwrap();
        prop_assert_eq!(transformed, left);
    }
}
