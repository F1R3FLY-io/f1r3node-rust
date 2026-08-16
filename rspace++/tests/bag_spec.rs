use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use pathmap::ring::{AlgebraicResult, DistributiveLattice, Lattice, SELF_IDENT};
use proptest::collection::vec;
use proptest::prelude::*;
use rspace_plus_plus::rspace::bag::{Bag, BagDelta, BagError, Multiplicity};

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
fn transaction_delta_application_is_atomic_on_underflow_and_overflow() {
    let mut underflow_target = bag(&[(b"alpha", 2)]);
    let underflow_before = underflow_target.clone();
    let mut underflow = BagDelta::new();
    underflow
        .record_removal(b"alpha", Multiplicity::new(3).unwrap())
        .unwrap();
    assert_eq!(
        underflow.apply_to(&mut underflow_target),
        Err(BagError::InsufficientMultiplicity {
            available: 2,
            requested: 3,
        })
    );
    assert_eq!(underflow_target, underflow_before);

    let mut overflow_target = bag(&[(b"omega", u64::MAX)]);
    let overflow_before = overflow_target.clone();
    let mut overflow = BagDelta::new();
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
fn clone_is_copy_on_write_and_merkleization_preserves_semantics() {
    let original = bag(&[(b"shared/left", 2), (b"shared/right", 3)]);
    let mut fork = original.clone();
    fork.insert(b"shared/new").unwrap();
    assert_eq!(original.count(b"shared/new"), 0);
    let before = fork.clone();
    fork.merkleize();
    assert_eq!(fork, before);
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

proptest! {
    #[test]
    fn bag_algebra_matches_a_btreemap_multiplicity_oracle(
        left_entries in entry_strategy(),
        right_entries in entry_strategy(),
    ) {
        let left_oracle = oracle(&left_entries);
        let right_oracle = oracle(&right_entries);
        let left = from_oracle(&left_oracle);
        let right = from_oracle(&right_oracle);

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
    }
}
