use proptest::prelude::*;

use super::*;

fn cap(count: usize) -> NonZeroUsize { NonZeroUsize::new(count).unwrap() }

fn unit_oracle(capacities: &[u64], amount: u64, cursor: usize) -> Vec<u64> {
    let mut debits = vec![0; capacities.len()];
    for _ in 0..amount {
        let minimum = debits
            .iter()
            .enumerate()
            .filter(|(index, value)| **value < capacities[*index])
            .map(|(_, value)| *value)
            .min()
            .unwrap();
        let payer = (cursor..capacities.len())
            .chain(0..cursor)
            .find(|index| debits[*index] == minimum && debits[*index] < capacities[*index])
            .unwrap();
        debits[payer] += 1;
    }
    debits
}

fn round_oracle(capacities: &[u64], mut amount: u64, cursor: usize) -> MonetaryAllocation {
    let mut debits = vec![0; capacities.len()];
    let mut next_cursor = cursor;
    while amount > 0 {
        let eligible: Vec<_> = (cursor..capacities.len())
            .chain(0..cursor)
            .filter(|index| debits[*index] < capacities[*index])
            .collect();
        assert!(!eligible.is_empty());
        if amount >= eligible.len() as u64 {
            for payer in &eligible {
                debits[*payer] += 1;
            }
            amount -= eligible.len() as u64;
        } else {
            for payer in eligible.into_iter().take(amount as usize) {
                debits[payer] += 1;
                next_cursor = (payer + 1) % capacities.len();
            }
            amount = 0;
        }
    }
    MonetaryAllocation {
        debits,
        next_cursor,
    }
}

fn assert_invariants(capacities: &[u64], amount: u64, cursor: usize, plan: &MonetaryAllocation) {
    assert_eq!(plan.debits.len(), capacities.len());
    assert_eq!(
        plan.debits
            .iter()
            .map(|value| u128::from(*value))
            .sum::<u128>(),
        u128::from(amount)
    );
    assert!(plan.next_cursor < capacities.len());
    for (payer, debit) in plan.debits.iter().enumerate() {
        assert!(*debit <= capacities[payer]);
        if *debit < capacities[payer] {
            for other in &plan.debits {
                assert!(u128::from(*other) <= u128::from(*debit) + 1);
            }
        }
    }
    if amount == 0 {
        assert_eq!(plan.next_cursor, cursor);
    }
}

#[test]
fn every_configured_arity_rejects_empty_excess_and_invalid_cursor() {
    for limit in 1..=64 {
        assert_eq!(
            allocate_capped_max_min(&[], 0, 0, cap(limit)),
            Err(MonetaryAllocationError::EmptyPayers)
        );
        assert_eq!(
            allocate_capped_max_min(&vec![1; limit + 1], 1, 0, cap(limit)),
            Err(MonetaryAllocationError::TooManyPayers)
        );
        for count in 1..=limit {
            assert_eq!(
                allocate_capped_max_min(&vec![1; count], 1, count, cap(limit)),
                Err(MonetaryAllocationError::InvalidCursor)
            );
            let plan =
                allocate_capped_max_min(&vec![1; count], count as u64, 0, cap(limit)).unwrap();
            assert_eq!(plan.debits, vec![1; count]);
        }
    }
}

#[test]
fn exhaustive_small_allocations_match_unit_oracle() {
    for encoded in 0..256_u64 {
        let capacities: Vec<_> = (0..4).map(|index| (encoded >> (index * 2)) & 3).collect();
        let total = capacities.iter().sum::<u64>();
        for cursor in 0..4 {
            for amount in 0..=total {
                let plan = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
                assert_invariants(&capacities, amount, cursor, &plan);
                assert_eq!(plan.debits, unit_oracle(&capacities, amount, cursor));
                assert_eq!(plan, round_oracle(&capacities, amount, cursor));
            }
            assert_eq!(
                allocate_capped_max_min(&capacities, total + 1, cursor, cap(64)),
                Err(MonetaryAllocationError::InsufficientCapacity)
            );
        }
    }
}

#[test]
fn one_total_fee_rotates_through_every_payer() {
    for count in 1..=64 {
        for initial in 0..count {
            let mut cursor = initial;
            let mut paid = vec![0_u64; count];
            let mut capacities = vec![4_u64; count];
            for _ in 0..(3 * count) {
                let plan = allocate_capped_max_min(&capacities, 1, cursor, cap(64)).unwrap();
                assert_invariants(&capacities, 1, cursor, &plan);
                for payer in 0..count {
                    capacities[payer] -= plan.debits[payer];
                    paid[payer] += plan.debits[payer];
                }
                cursor = plan.next_cursor;
            }
            assert_eq!(paid, vec![3; count]);
            assert_eq!(cursor, initial);
        }
    }
}

#[test]
fn saturated_payers_are_skipped_without_removing_their_positions() {
    let plan = allocate_capped_max_min(&[0, 9, 0, 9], 3, 2, cap(64)).unwrap();
    assert_eq!(plan.debits, vec![0, 1, 0, 2]);
    assert_eq!(plan.next_cursor, 0);
}

#[test]
fn no_residual_preserves_cursor_even_when_the_charge_is_positive() {
    for capacities in [vec![0, 5, 0, 0], vec![2, 2, 2, 2], vec![0, 3, 0, 3]] {
        for cursor in 0..capacities.len() {
            let amount = capacities.iter().filter(|capacity| **capacity > 0).count() as u64;
            let plan = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
            assert_eq!(plan.next_cursor, cursor);
            assert_eq!(plan, round_oracle(&capacities, amount, cursor));
        }
    }
}

#[test]
fn multiple_residual_recipients_cross_the_cohort_boundary() {
    let plan = allocate_capped_max_min(&[4, 0, 4, 4, 0], 2, 3, cap(64)).unwrap();
    assert_eq!(plan.debits, vec![1, 0, 0, 1, 0]);
    assert_eq!(plan.next_cursor, 1);
}

#[test]
fn maximum_obligation_does_not_overflow_the_aggregate() {
    for count in 1..=64 {
        let capacities = vec![u64::MAX; count];
        let plan = allocate_capped_max_min(&capacities, u64::MAX, count - 1, cap(64)).unwrap();
        assert_invariants(&capacities, u64::MAX, count - 1, &plan);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_allocations_preserve_formal_invariants(
        capacities in prop::collection::vec(any::<u64>(), 1..=64),
        requested in any::<u64>(), cursor_seed in any::<usize>(),
    ) {
        let available = capacities.iter().map(|value| u128::from(*value)).sum::<u128>();
        let amount = u128::from(requested).min(available) as u64;
        let cursor = cursor_seed % capacities.len();
        let plan = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
        assert_invariants(&capacities, amount, cursor, &plan);
        prop_assert_eq!(&plan, &allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap());
    }

    #[test]
    fn generated_small_allocations_match_independent_oracle(
        capacities in prop::collection::vec(0_u64..32, 1..=64),
        requested in 0_u64..1024, cursor_seed in any::<usize>(),
    ) {
        let amount = requested.min(capacities.iter().sum());
        let cursor = cursor_seed % capacities.len();
        let plan = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
        prop_assert_eq!(plan.debits, unit_oracle(&capacities, amount, cursor));
    }

    #[test]
    fn reservation_bounded_settlement_preserves_caps_and_refunds(
        capacities in prop::collection::vec(any::<u64>(), 1..=64),
        requested in any::<u64>(), realized in any::<u64>(),
        cursor_seed in any::<usize>(),
    ) {
        let available = capacities.iter().map(|value| u128::from(*value)).sum::<u128>();
        let maximum = u128::from(requested).min(available) as u64;
        let actual = realized.min(maximum);
        let cursor = cursor_seed % capacities.len();
        let reserved = allocate_capped_max_min(&capacities, maximum, cursor, cap(64)).unwrap();
        let settled = allocate_capped_max_min(&reserved.debits, actual, cursor, cap(64)).unwrap();
        assert_invariants(&reserved.debits, actual, cursor, &settled);
        let mut refund_total = 0_u128;
        for ((capacity, held), debit) in capacities.iter().zip(&reserved.debits).zip(&settled.debits) {
            prop_assert!(*debit <= *held && *held <= *capacity);
            let refund = held.checked_sub(*debit).unwrap();
            prop_assert_eq!(u128::from(refund) + u128::from(*debit), u128::from(*held));
            refund_total += u128::from(refund);
        }
        prop_assert_eq!(refund_total + u128::from(actual), u128::from(maximum));
    }

    #[test]
    fn reservation_bounded_settlement_matches_independent_oracle(
        capacities in prop::collection::vec(0_u64..12, 1..=64),
        requested in 0_u64..768, realized in 0_u64..768,
        cursor_seed in any::<usize>(),
    ) {
        let maximum = requested.min(capacities.iter().sum());
        let actual = realized.min(maximum);
        let cursor = cursor_seed % capacities.len();
        let reserved = allocate_capped_max_min(&capacities, maximum, cursor, cap(64)).unwrap();
        prop_assert_eq!(&reserved.debits, &unit_oracle(&capacities, maximum, cursor));
        let settled = allocate_capped_max_min(&reserved.debits, actual, cursor, cap(64)).unwrap();
        prop_assert_eq!(settled.debits, unit_oracle(&reserved.debits, actual, cursor));
    }

    #[test]
    fn cyclic_reindexing_preserves_allocation_and_cursor(
        capacities in prop::collection::vec(any::<u64>(), 1..=64),
        requested in any::<u64>(), cursor_seed in any::<usize>(), shift_seed in any::<usize>(),
    ) {
        let count = capacities.len();
        let cursor = cursor_seed % count;
        let shift = shift_seed % count;
        let available = capacities.iter().map(|value| u128::from(*value)).sum::<u128>();
        let amount = u128::from(requested).min(available) as u64;
        let mut expected = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
        let mut rotated = capacities;
        rotated.rotate_left(shift);
        expected.debits.rotate_left(shift);
        expected.next_cursor = (expected.next_cursor + count - shift) % count;
        let actual = allocate_capped_max_min(&rotated, amount, (cursor + count - shift) % count, cap(64)).unwrap();
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn changed_capacities_follow_the_full_round_oracle(
        changes in prop::collection::vec((0_usize..8, 0_u64..32, 0_u64..32), 0..40),
        initial_cursor in 0_usize..8,
    ) {
        let mut capacities = vec![5_u64; 8];
        let mut cursor = initial_cursor;
        for (payer, capacity, requested) in changes {
            capacities[payer] = capacity;
            let amount = requested.min(capacities.iter().sum());
            let plan = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
            prop_assert_eq!(&plan, &round_oracle(&capacities, amount, cursor));
            for (balance, debit) in capacities.iter_mut().zip(&plan.debits) {
                *balance -= debit;
            }
            cursor = plan.next_cursor;
        }
    }
}
