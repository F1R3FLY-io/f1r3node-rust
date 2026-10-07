use super::*;

#[test]
fn zero_contribution_does_not_advance_the_cursor_revision() {
    let (eligible, inventory) = fixture(&[0, 1, 2], &[0, 0, 0]);
    let cohort = cohort(&eligible, &inventory, 3);
    let cursor = MonetaryCursor::new(7, 2, NonZeroUsize::new(3).unwrap()).unwrap();
    let plan = cohort
        .plan_all_to_all_contribution(&inventory.balances, 0, &[1; 32], cursor)
        .unwrap();
    assert_eq!(plan.cursor_transition(), None);
    assert_eq!(plan.settlement(), &AuthorityBalanceSettlement::default());
    assert_eq!(cursor.revision(), 7);
    assert_eq!(cursor.position(), 2);
}

#[test]
fn zero_contribution_accepts_exhausted_revision_without_bypassing_cursor_validation() {
    let (eligible, inventory) = fixture(&[0], &[0]);
    let cohort = cohort(&eligible, &inventory, 1);
    let last = MonetaryCursor::new(i64::MAX, 0, NonZeroUsize::new(1).unwrap()).unwrap();
    let plan = cohort
        .plan_all_to_all_contribution(&inventory.balances, 0, &[1; 32], last)
        .unwrap();
    assert_eq!(
        plan.into_parts(),
        (AuthorityBalanceSettlement::default(), None)
    );
    assert_eq!(
        cohort.plan_all_to_all_contribution(&inventory.balances, 1, &[1; 32], last),
        Err(MonetaryCohortError::Cursor(
            MonetaryCursorError::RevisionExhausted
        ))
    );
    let foreign_position = MonetaryCursor::new(0, 1, NonZeroUsize::new(2).unwrap()).unwrap();
    assert_eq!(
        cohort.plan_all_to_all_contribution(&inventory.balances, 0, &[1; 32], foreign_position),
        Err(MonetaryCohortError::Cursor(
            MonetaryCursorError::InvalidPosition
        ))
    );
}

#[test]
fn positive_contribution_preserves_fee_plan_and_stale_scope_guards() {
    let (eligible, inventory) = fixture(&[0, 1, 2], &[9, 9, 9]);
    let cohort = cohort(&eligible, &inventory, 3);
    let count = NonZeroUsize::new(3).unwrap();
    for amount in [1, 8, 9, 27, 28] {
        for position in 0..3 {
            let cursor = MonetaryCursor::new(7, position, count).unwrap();
            let old = cohort
                .plan(&inventory.balances, amount, &[1; 32], cursor)
                .map(|plan| {
                    let (settlement, transition) = plan.into_parts();
                    (settlement, Some(transition))
                });
            let new = cohort
                .plan_all_to_all_contribution(&inventory.balances, amount, &[1; 32], cursor)
                .map(AllToAllContributionPlan::into_parts);
            assert_eq!(new, old);
            if let Ok((_, Some(transition))) = new {
                assert_eq!(
                    transition.checked_successor(&cohort.scope_id(&[2; 32]), cursor, count),
                    Err(MonetaryCursorError::ScopeMismatch)
                );
                assert_eq!(
                    transition.checked_successor(
                        &cohort.scope_id(&[1; 32]),
                        transition.next(),
                        count
                    ),
                    Err(MonetaryCursorError::StaleTransition)
                );
            }
        }
    }
    let cursor = MonetaryCursor::INITIAL;
    let zero = cohort
        .plan_all_to_all_contribution(&inventory.balances, 0, &[1; 32], cursor)
        .unwrap();
    let positive = cohort
        .plan_all_to_all_contribution(&inventory.balances, 1, &[1; 32], cursor)
        .unwrap();
    let next = positive.cursor_transition().unwrap().next();
    assert_eq!(zero.cursor_transition(), None);
    let later_zero = cohort
        .plan_all_to_all_contribution(&inventory.balances, 0, &[1; 32], next)
        .unwrap();
    assert_eq!(later_zero.cursor_transition(), None);
    assert_eq!(later_zero.settlement(), zero.settlement());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn generated_contribution_plans_preserve_aliases_capacity_and_positive_refinement(
        capacities in prop::collection::vec(0u64..100, 1..130),
        amount in 0u64..13_000, seed in any::<usize>(), revision in 0i64..i64::MAX,
    ) {
        let ids: Vec<_> = (0..capacities.len()).map(|index| index as u8).collect();
        let aliases: Vec<_> = ids.iter().chain(ids.iter()).copied().collect();
        let (eligible, inventory) = fixture(&aliases, &capacities);
        let cohort = cohort(&eligible, &inventory, capacities.len());
        let count = NonZeroUsize::new(capacities.len()).unwrap();
        let cursor = MonetaryCursor::new(revision, (seed % capacities.len()) as i64, count).unwrap();
        let zero = cohort.plan_all_to_all_contribution(&inventory.balances, 0, &[3; 32], cursor).unwrap();
        prop_assert_eq!(zero.cursor_transition(), None);
        prop_assert_eq!(zero.settlement(), &AuthorityBalanceSettlement::default());
        if amount > 0 {
            let expected = cohort.plan(&inventory.balances, amount, &[3; 32], cursor)
                .map(|plan| { let (settlement, transition) = plan.into_parts(); (settlement, Some(transition)) });
            let actual = cohort.plan_all_to_all_contribution(&inventory.balances, amount, &[3; 32], cursor)
                .map(AllToAllContributionPlan::into_parts);
            prop_assert_eq!(&actual, &expected);
            if let Ok((settlement, transition)) = actual {
                prop_assert_eq!(settlement.custody_debit.0.values().sum::<u64>(), amount);
                prop_assert_eq!(physicalize_balance_debit(&settlement.logical_debit, &inventory.balance_custody).unwrap(), settlement.custody_debit);
                prop_assert_eq!(transition.unwrap().next().revision(), revision + 1);
            }
        }
    }

    #[test]
    fn generated_mixed_histories_count_only_successful_positive_contributions(
        count in 1usize..130, start in 0i64..20, actions in prop::collection::vec(any::<bool>(), 0..257),
    ) {
        let ids: Vec<_> = (0..count).map(|index| index as u8).collect();
        let (eligible, inventory) = fixture(&ids, &vec![1000; count]);
        let cohort = cohort(&eligible, &inventory, count);
        let size = NonZeroUsize::new(count).unwrap();
        let initial = MonetaryCursor::new(i64::MAX - start, 0, size).unwrap();
        let mut cursor = initial;
        let mut successes = 0;
        for positive in actions {
            let before = cursor;
            match cohort.plan_all_to_all_contribution(&inventory.balances, u64::from(positive), &[3; 32], cursor) {
                Ok(plan) => {
                    if let Some(transition) = plan.cursor_transition() {
                        prop_assert!(positive);
                        cursor = transition.checked_successor(&cohort.scope_id(&[3; 32]), cursor, size).unwrap();
                        successes += 1;
                    } else {
                        prop_assert!(!positive);
                        prop_assert_eq!(plan.settlement(), &AuthorityBalanceSettlement::default());
                        prop_assert_eq!(cursor, before);
                    }
                }
                Err(error) => {
                    prop_assert!(positive);
                    prop_assert_eq!(cursor.revision(), i64::MAX);
                    prop_assert_eq!(error, MonetaryCohortError::Cursor(MonetaryCursorError::RevisionExhausted));
                }
            }
            prop_assert_eq!(cursor.revision() as i128, initial.revision() as i128 + successes);
        }
    }
}
