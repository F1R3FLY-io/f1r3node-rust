use super::super::sparse_ledger;
use super::*;

fn balances() -> impl Strategy<Value = BTreeMap<u8, u64>> {
    prop::collection::btree_map(
        any::<u8>(),
        prop_oneof![Just(0), Just(u64::MAX), any::<u64>(), 0..32_u64],
        0..65,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn sparse_updates_match_copying_algebra_and_preserve_failed_input(
        initial in balances(),
        debit in balances(),
        capacity in prop::option::of(balances()),
    ) {
        let original = ResourceMultiset(initial.clone());
        let delta = ResourceMultiset(debit.clone());
        let expected_add = original.checked_add(&delta);
        let expected_admission = expected_add.as_ref().is_ok_and(|next| {
            capacity.as_ref().is_none_or(|limit| ResourceMultiset(limit.clone()).dominates(next))
        });
        prop_assert_eq!(
            sparse_ledger::validate_add(&initial, &debit, capacity.as_ref()).is_ok(),
            expected_admission,
        );
        let mut added = initial.clone();
        let actual_add = sparse_ledger::add_assign(&mut added, &debit);
        prop_assert_eq!(actual_add.is_ok(), expected_add.is_ok());
        match expected_add {
            Ok(expected) => prop_assert_eq!(added, expected.0),
            Err(_) => prop_assert_eq!(added, initial.clone()),
        }
        let expected_sub = original.checked_sub(&delta);
        let mut subtracted = initial.clone();
        let actual_sub = sparse_ledger::sub_assign(&mut subtracted, &debit);
        prop_assert_eq!(actual_sub.is_ok(), expected_sub.is_ok());
        match expected_sub {
            Ok(expected) => prop_assert_eq!(subtracted, expected.0),
            Err(_) => prop_assert_eq!(subtracted, initial),
        }
    }

    #[test]
    fn sparse_operation_sequences_match_algebra(
        initial in balances(),
        operations in prop::collection::vec((any::<bool>(), balances()), 0..65),
    ) {
        let mut expected = ResourceMultiset(initial.clone());
        let mut actual = initial;
        for (add, debit) in operations {
            let result = if add {
                expected.checked_add(&ResourceMultiset(debit.clone()))
            } else {
                expected.checked_sub(&ResourceMultiset(debit.clone()))
            };
            let updated = if add {
                sparse_ledger::add_assign(&mut actual, &debit)
            } else {
                sparse_ledger::sub_assign(&mut actual, &debit)
            };
            prop_assert_eq!(updated.is_ok(), result.is_ok());
            if let Ok(next) = result { expected = next; }
            prop_assert_eq!(&actual, &expected.0);
        }
    }
}

#[test]
fn later_key_failure_cannot_partially_change_earlier_balances() {
    let initial = BTreeMap::from([(1, 4), (2, u64::MAX)]);
    let mut ledger = initial.clone();
    let debit = BTreeMap::from([(1, 2), (2, 1)]);
    assert_eq!(
        sparse_ledger::add_assign(&mut ledger, &debit),
        Err(sparse_ledger::LedgerError::Overflow),
    );
    assert_eq!(ledger, initial);
    let debit = BTreeMap::from([(1, 2), (3, 1)]);
    assert_eq!(
        sparse_ledger::sub_assign(&mut ledger, &debit),
        Err(sparse_ledger::LedgerError::Insufficient),
    );
    assert_eq!(ledger, initial);
}

#[test]
fn sparse_updates_preserve_zero_entry_and_unrelated_key_semantics() {
    let mut ledger = BTreeMap::from([(1, 0), (2, 0), (3, 7)]);
    let debit = BTreeMap::from([(1, 0), (4, 0)]);
    sparse_ledger::add_assign(&mut ledger, &debit).unwrap();
    assert_eq!(ledger, BTreeMap::from([(2, 0), (3, 7)]));
    sparse_ledger::sub_assign(&mut ledger, &BTreeMap::from([(3, 7)])).unwrap();
    assert_eq!(ledger, BTreeMap::from([(2, 0)]));
}
