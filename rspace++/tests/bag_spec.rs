use std::collections::BTreeMap;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};

use multiset::HashMultiSet;
use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::ring::{AlgebraicResult, DistributiveLattice, Lattice, SELF_IDENT};
use proptest::collection::vec;
use proptest::prelude::*;
use rspace_plus_plus::rspace::bag::{
    Bag, BagCodecError, BagDelta, BagError, Multiplicity, SortedUniqueBagError, UniqueBagError,
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
fn unique_stream_preserves_arbitrary_order_prefixes_and_failure_is_private() {
    let entries =
        [(&b"z"[..], 7), (&b"a\0z"[..], 3), (&b""[..], 2), (&b"a"[..], 5), (&b"a\0"[..], 11)];
    let expected = bag(&entries);
    let actual = Bag::try_from_unique(|writer| {
        let mut buffer = Vec::new();
        for (path, count) in entries {
            buffer.clear();
            buffer.extend_from_slice(path);
            writer.push(&buffer, Multiplicity::new(count).unwrap())?;
        }
        Ok::<(), UniqueBagError>(())
    })
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.compact_bytes(), expected.compact_bytes());
    for repeat in [b"".as_slice(), b"z", b"a\0z"] {
        let rejected = Bag::try_from_unique(|writer| {
            for (path, count) in entries {
                writer.push(path, Multiplicity::new(count).unwrap())?;
            }
            writer.push(repeat, Multiplicity::ONE)
        });
        assert_eq!(rejected.unwrap_err(), UniqueBagError::DuplicateKey);
    }
    let refused = Bag::try_from_unique(|writer| {
        writer.push(b"private", Multiplicity::ONE).unwrap();
        Err::<(), _>("producer failure")
    });
    assert_eq!(refused.unwrap_err(), "producer failure");
    assert_eq!(actual, expected);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn unique_stream_matches_point_insertion_in_input_order(
        entries in vec((vec(any::<u8>(), 0..40), 1u64..1000), 0..100),
    ) {
        let mut seen = std::collections::BTreeSet::new();
        let mut expected = Bag::new();
        let unique: Vec<_> = entries.into_iter().filter(|(key, _)| seen.insert(key.clone())).collect();
        let actual = Bag::try_from_unique(|writer| {
            for (key, count) in &unique {
                let count = Multiplicity::new(*count).unwrap();
                writer.push(key, count)?;
                expected.insert_many(key, count).unwrap();
            }
            Ok::<(), UniqueBagError>(())
        }).unwrap();
        prop_assert_eq!(actual.cardinality(), expected.cardinality());
        prop_assert_eq!(actual.distinct_len(), expected.distinct_len());
        prop_assert_eq!(actual.compact_bytes(), expected.compact_bytes());
    }
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
fn singleton_delta_failure_is_byte_identical_and_metric_identical() {
    let mut underflow_target = bag(&[(b"alpha", 1), (b"sibling", 7)]);
    let underflow_bytes = underflow_target.compact_bytes();
    let underflow_metrics = (
        underflow_target.distinct_len(),
        underflow_target.cardinality(),
        underflow_target.cardinality_is_saturated(),
    );
    let mut underflow = BagDelta::new();
    underflow
        .record_removal(b"alpha", Multiplicity::new(2).unwrap())
        .unwrap();
    assert_eq!(
        underflow.apply_to(&mut underflow_target),
        Err(BagError::InsufficientMultiplicity {
            available: 1,
            requested: 2,
        })
    );
    assert_eq!(underflow_target.compact_bytes(), underflow_bytes);
    assert_eq!(
        (
            underflow_target.distinct_len(),
            underflow_target.cardinality(),
            underflow_target.cardinality_is_saturated(),
        ),
        underflow_metrics
    );

    let mut overflow_target = bag(&[(b"alpha", u64::MAX), (b"sibling", 7)]);
    let overflow_bytes = overflow_target.compact_bytes();
    let overflow_metrics = (
        overflow_target.distinct_len(),
        overflow_target.cardinality(),
        overflow_target.cardinality_is_saturated(),
    );
    let mut overflow = BagDelta::new();
    overflow
        .record_addition(b"alpha", Multiplicity::ONE)
        .unwrap();
    assert_eq!(
        overflow.apply_to(&mut overflow_target),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1,
        })
    );
    assert_eq!(overflow_target.compact_bytes(), overflow_bytes);
    assert_eq!(
        (
            overflow_target.distinct_len(),
            overflow_target.cardinality(),
            overflow_target.cardinality_is_saturated(),
        ),
        overflow_metrics
    );
}

#[test]
fn singleton_delta_preserves_realistic_same_prefix_sibling_and_cow_snapshot() {
    for drop_snapshot_first in [false, true] {
        let mut selected = vec![0xA5; 939];
        let mut sibling = selected.clone();
        selected[203] = 0x11;
        sibling[203] = 0x22;
        assert_eq!(
            selected
                .iter()
                .zip(&sibling)
                .take_while(|(a, b)| a == b)
                .count(),
            203
        );

        let mut target = Bag::new();
        target.insert(&selected).unwrap();
        target.insert(&sibling).unwrap();
        let snapshot = target.clone();
        let mut delta = BagDelta::new();
        delta.record_removal(&selected, Multiplicity::ONE).unwrap();

        delta.apply_to(&mut target).unwrap();
        assert_eq!(target.count(&selected), 0);
        assert_eq!(target.count(&sibling), 1);
        assert_eq!(target.distinct_len(), 1);
        assert_eq!(target.cardinality(), 1);
        assert!(!target.cardinality_is_saturated());
        assert_eq!(snapshot.count(&selected), 1);
        assert_eq!(snapshot.count(&sibling), 1);

        if drop_snapshot_first {
            drop(snapshot);
            assert_eq!(target.count(&sibling), 1);
        } else {
            drop(target);
            assert_eq!(snapshot.count(&selected), 1);
            assert_eq!(snapshot.count(&sibling), 1);
        }
    }
}

#[test]
fn singleton_final_removal_returns_a_canonical_write_ready_empty_bag() {
    let mut target = bag(&[(b"only", 1)]);
    let mut delta = BagDelta::new();
    delta.record_removal(b"only", Multiplicity::ONE).unwrap();

    delta.apply_to(&mut target).unwrap();
    assert!(target.is_empty());
    assert_eq!(target.distinct_len(), 0);
    assert_eq!(target.cardinality(), 0);
    assert!(!target.cardinality_is_saturated());
    assert_eq!(target.compact_bytes(), Bag::new().compact_bytes());

    target.insert(b"replacement").unwrap();
    assert_eq!(target.count(b"replacement"), 1);
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
fn sum_refuses_the_first_overflowing_endpoint_in_either_operand_order() {
    let nearly_full = u64::MAX - 1;
    let smaller = bag(&[(b"b", 2), (b"d", 3)]);
    let larger = bag(&[(b"a", 1), (b"b", nearly_full), (b"c", 1), (b"d", nearly_full), (b"e", 1)]);
    assert_eq!(
        smaller.sum(&larger),
        Err(BagError::MultiplicityOverflow {
            current: 2,
            added: nearly_full,
        }),
        "sum must refuse at the first overflowing endpoint and report self's count, then other's"
    );
    assert_eq!(
        larger.sum(&smaller),
        Err(BagError::MultiplicityOverflow {
            current: nearly_full,
            added: 2,
        }),
        "sum must refuse at the first overflowing endpoint and report self's count, then other's"
    );
    let fitting = bag(&[(b"b", 1), (b"d", 1)]);
    let expected = bag(&[(b"a", 1), (b"b", u64::MAX), (b"c", 1), (b"d", u64::MAX), (b"e", 1)]);
    assert_eq!(
        fitting
            .sum(&larger)
            .expect("counts that reach u64::MAX exactly fit"),
        expected
    );
    assert_eq!(
        larger
            .sum(&fitting)
            .expect("counts that reach u64::MAX exactly fit"),
        expected
    );
}

#[test]
fn meet_combines_exact_values_when_an_overlap_is_also_a_prefix() {
    let left = bag(&[(b"\x07", 7), (b"\x08", 1)]);
    let right = bag(&[(b"\x07", 1), (b"\x07\x00", 1), (b"\x08", 1)]);
    assert_eq!(left.meet(&right), bag(&[(b"\x07", 1), (b"\x08", 1)]));
    assert_eq!(right.meet(&left), bag(&[(b"\x07", 1), (b"\x08", 1)]));
}

fn algebra_count_strategy() -> impl Strategy<Value = u64> {
    prop_oneof![Just(0), Just(1), Just(u64::MAX), any::<u64>()]
}

fn borrowed_cursor_entries(value: &Bag) -> Vec<(Vec<u8>, Multiplicity)> {
    let mut entries = Vec::new();
    value.visit_relative_prefix(b"", |path, multiplicity| {
        entries.push((path.to_vec(), multiplicity));
    });
    entries
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn exact_algebra_retains_prefix_related_endpoint_independence(
        prefix in vec(any::<u8>(), 0..32),
        left_counts in prop::array::uniform3(algebra_count_strategy()),
        right_counts in prop::array::uniform3(algebra_count_strategy()),
    ) {
        let mut first_child = prefix.clone();
        first_child.push(0);
        let mut second_child = prefix.clone();
        second_child.push(1);
        let keys = [prefix, first_child, second_child];
        let mut left = Bag::new();
        let mut right = Bag::new();
        for (index, key) in keys.iter().enumerate() {
            if left_counts[index] != 0 {
                left.insert_many(key, Multiplicity::new(left_counts[index]).unwrap()).unwrap();
            }
            if right_counts[index] != 0 {
                right.insert_many(key, Multiplicity::new(right_counts[index]).unwrap()).unwrap();
            }
        }
        let left_snapshot = left.clone();
        let right_snapshot = right.clone();
        let left_bytes = left.compact_bytes();
        let right_bytes = right.compact_bytes();
        let join = left.join(&right);
        let meet = left.meet(&right);
        let subtract = left.subtract(&right);

        for value in [&left, &right, &join, &meet, &subtract] {
            let borrowed = borrowed_cursor_entries(value);
            prop_assert!(borrowed.windows(2).all(|pair| pair[0].0 < pair[1].0));
            prop_assert_eq!(borrowed.len(), value.distinct_len());
            prop_assert_eq!(borrowed, value.iter().collect::<Vec<_>>());
        }

        for (index, key) in keys.iter().enumerate() {
            prop_assert_eq!(join.count(key), left_counts[index].max(right_counts[index]));
            prop_assert_eq!(meet.count(key), left_counts[index].min(right_counts[index]));
            prop_assert_eq!(subtract.count(key), left_counts[index].saturating_sub(right_counts[index]));
        }
        prop_assert_eq!(
            left.is_subbag_of(&right),
            left_counts.iter().zip(right_counts).all(|(left, right)| *left <= right),
        );
        prop_assert_eq!(left.compact_bytes(), left_bytes);
        prop_assert_eq!(right.compact_bytes(), right_bytes);
        prop_assert_eq!(left, left_snapshot);
        prop_assert_eq!(right, right_snapshot);
    }
}

#[test]
fn prefix_coupled_algebra_mutants_disagree_with_the_exact_key_oracle() {
    let ancestor = bag(&[(b"", u64::MAX)]);
    let descendant = bag(&[(b"\0", 1)]);
    assert_eq!(ancestor.meet(&descendant), Bag::new());
    assert_ne!(ancestor.meet(&descendant), bag(&[(b"", 1)]));
    assert_eq!(ancestor.subtract(&descendant), ancestor);
    assert_eq!(descendant.subtract(&ancestor), descendant);
    assert_ne!(descendant.subtract(&ancestor), Bag::new());
    assert!(!ancestor.is_subbag_of(&descendant));
    assert!(!descendant.is_subbag_of(&ancestor));
    assert_eq!(ancestor.join(&descendant).count(b""), u64::MAX);
    assert_eq!(ancestor.join(&descendant).count(b"\0"), 1);
}

#[test]
fn deep_prefix_algebra_is_stack_safe_and_preserves_copy_on_write_roots() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let prefix = vec![0xA5; 200_000];
            let mut child = prefix.clone();
            child.push(0);
            let mut sibling = prefix.clone();
            sibling.push(1);
            let left = bag(&[(&prefix, 7), (&child, 3)]);
            let right = bag(&[(&prefix, 5), (&sibling, 11)]);
            let left_snapshot = left.clone();
            let right_snapshot = right.clone();
            let join = left.join(&right);
            let meet = left.meet(&right);
            let subtract = left.subtract(&right);
            assert_eq!(join.count(&prefix), 7);
            assert_eq!(join.count(&child), 3);
            assert_eq!(join.count(&sibling), 11);
            assert_eq!(meet.count(&prefix), 5);
            assert_eq!(meet.distinct_len(), 1);
            assert_eq!(subtract.count(&prefix), 2);
            assert_eq!(subtract.count(&child), 3);
            assert_eq!(subtract.count(&sibling), 0);
            assert!(!left.is_subbag_of(&right));
            assert_eq!(left, left_snapshot);
            assert_eq!(right, right_snapshot);
        })
        .unwrap()
        .join()
        .unwrap();
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

#[derive(Clone, Copy, Debug)]
enum ComparedEndpoint {
    First,
    Last,
    Any(prop::sample::Index),
}

#[derive(Clone, Copy, Debug)]
enum PairDifference {
    None,
    PathExtended(ComparedEndpoint, u8),
    Removed(ComparedEndpoint),
    Recounted(ComparedEndpoint, u64),
}

fn compared_endpoint_strategy() -> impl Strategy<Value = ComparedEndpoint> {
    prop_oneof![
        Just(ComparedEndpoint::First),
        Just(ComparedEndpoint::Last),
        any::<prop::sample::Index>().prop_map(ComparedEndpoint::Any),
    ]
}

fn pair_difference_strategy() -> impl Strategy<Value = PairDifference> {
    let recount = prop_oneof![Just(1u64), Just(u64::MAX), 1u64..=u64::MAX];
    prop_oneof![
        Just(PairDifference::None),
        (compared_endpoint_strategy(), any::<u8>())
            .prop_map(|(endpoint, byte)| PairDifference::PathExtended(endpoint, byte)),
        compared_endpoint_strategy().prop_map(PairDifference::Removed),
        (compared_endpoint_strategy(), recount)
            .prop_map(|(endpoint, count)| PairDifference::Recounted(endpoint, count)),
    ]
}

fn differ(entries: &BTreeMap<Vec<u8>, u64>, difference: PairDifference) -> BTreeMap<Vec<u8>, u64> {
    let mut related = entries.clone();
    let endpoint = match difference {
        PairDifference::None => return related,
        PairDifference::PathExtended(endpoint, _) |
        PairDifference::Removed(endpoint) |
        PairDifference::Recounted(endpoint, _) => endpoint,
    };
    let position = match (endpoint, entries.len()) {
        (_, 0) => return related,
        (ComparedEndpoint::First, _) => 0,
        (ComparedEndpoint::Last, length) => length - 1,
        (ComparedEndpoint::Any(index), length) => index.index(length),
    };
    let (path, count) = entries
        .iter()
        .nth(position)
        .expect("the chosen position is below the entry count");
    match difference {
        PairDifference::None => {}
        PairDifference::PathExtended(_, byte) => {
            related.remove(path);
            let mut extended = path.clone();
            extended.push(byte);
            *related.entry(extended).or_default() += count;
        }
        PairDifference::Removed(_) => {
            related.remove(path);
        }
        PairDifference::Recounted(_, recount) => {
            related.insert(path.clone(), recount);
        }
    }
    related
}

fn fixed_key_digest(value: &Bag) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

fn owned_iterator_digest(value: &Bag) -> u64 {
    let mut state = DefaultHasher::new();
    value.distinct_len().hash(&mut state);
    value
        .iter()
        .map(|(_, multiplicity)| u128::from(multiplicity.get()))
        .sum::<u128>()
        .hash(&mut state);
    for entry in value.iter() {
        entry.hash(&mut state);
    }
    state.finish()
}

struct OwnedIteratorEntries<'a>(&'a Bag);

impl fmt::Debug for OwnedIteratorEntries<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.0.iter()).finish()
    }
}

struct OwnedIteratorBag<'a>(&'a Bag);

impl fmt::Debug for OwnedIteratorBag<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bag")
            .field("distinct_len", &self.0.distinct_len())
            .field("cardinality", &self.0.cardinality())
            .field("cardinality_is_saturated", &self.0.cardinality_is_saturated())
            .field("entries", &OwnedIteratorEntries(self.0))
            .finish()
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn borrowed_cursor_observers_agree_with_the_owned_iterator_reference(
        entries in vec((vec(0u8..4, 0..6), 1u64..=u64::from(u32::MAX)), 0..24),
        difference in pair_difference_strategy(),
    ) {
        let left_entries = entries.into_iter().collect::<BTreeMap<_, _>>();
        let right_entries = differ(&left_entries, difference);
        let left = from_oracle(&left_entries);
        let mut right = Bag::new();
        for (path, count) in right_entries.iter().rev() {
            right
                .insert_many(path, Multiplicity::new(*count).expect("a generated count is positive"))
                .expect("a generated count fits the Bag");
        }
        for (first, second) in [(&left, &right), (&right, &left)] {
            prop_assert_eq!(
                first == second,
                first.iter().eq(second.iter()),
                "eq differs from the owned-iterator reference"
            );
            prop_assert_eq!(
                first.cmp(second),
                first.iter().cmp(second.iter()),
                "cmp differs from the owned-iterator reference"
            );
        }
        for value in [&left, &right] {
            prop_assert_eq!(
                fixed_key_digest(value),
                owned_iterator_digest(value),
                "the fixed-key digest differs from the owned-iterator reference"
            );
            prop_assert_eq!(
                format!("{value:?}"),
                format!("{:?}", OwnedIteratorBag(value)),
                "Debug differs from the owned-iterator reference"
            );
            prop_assert_eq!(
                format!("{value:#?}"),
                format!("{:#?}", OwnedIteratorBag(value)),
                "alternate Debug differs from the owned-iterator reference"
            );
        }
    }
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
