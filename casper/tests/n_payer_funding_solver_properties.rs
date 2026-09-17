use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    allocate_capped_max_min, check_fixed_funding_batch, check_funding_assignment,
    FixedFundingBatchEntry, FundingAssignmentError, FundingBatchError, FundingSearchError,
    FundingSearchLimits, MonetaryAllocationError,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

fn cap(value: usize) -> NonZeroUsize { NonZeroUsize::new(value).unwrap() }

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000)))
}

fn limits() -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: cap(64),
        obligation_cap: cap(64),
    }
}

fn batch_error(error: FundingAssignmentError) -> FundingBatchError {
    FundingBatchError::Check(FundingSearchError::InvalidProblem(error))
}

#[test]
fn n_payer_funding_solver_accepts_every_configured_arity() {
    assert_eq!(
        allocate_capped_max_min(&[], 0, 0, cap(64)),
        Err(MonetaryAllocationError::EmptyPayers)
    );
    for count in 1..=64 {
        let capacities = vec![1_u64; count];
        let plan = allocate_capped_max_min(&capacities, count as u64, 0, cap(64)).unwrap();
        assert_eq!(plan.debits, capacities);
        assert!(allocate_capped_max_min(&capacities, count as u64 + 1, 0, cap(64)).is_err());
    }
    assert_eq!(
        allocate_capped_max_min(&vec![1_u64; 65], 1, 0, cap(64)),
        Err(MonetaryAllocationError::TooManyPayers)
    );
}

fn validate_batch(
    capacities: &[u64],
    entries: &[Vec<Vec<u64>>],
    eligibility: &[Vec<bool>],
) -> Result<Vec<u64>, FundingBatchError> {
    let demands: Vec<Vec<_>> = entries
        .iter()
        .map(|entry| {
            (0..eligibility[0].len())
                .map(|column| entry.iter().map(|row| row[column]).sum())
                .collect()
        })
        .collect();
    let batch: Vec<_> = entries
        .iter()
        .zip(&demands)
        .map(|(entry, demand)| FixedFundingBatchEntry {
            obligations: demand,
            eligible: eligibility,
            assignment: entry,
        })
        .collect();
    let checked = check_fixed_funding_batch(capacities, &batch, limits(), cap(64), &budget())?;
    assert_eq!(
        checked.total(),
        checked
            .source_debits()
            .iter()
            .copied()
            .map(u128::from)
            .sum::<u128>()
    );
    for ((remaining, debit), original) in checked
        .remaining()
        .iter()
        .zip(checked.source_debits())
        .zip(capacities)
    {
        assert_eq!(
            u128::from(*remaining) + u128::from(*debit),
            u128::from(*original)
        );
    }
    Ok(checked.remaining().to_vec())
}

#[test]
fn batch_revalidation_rejects_two_individually_valid_shared_purse_draws() {
    let first = vec![vec![1], vec![0], vec![0]];
    let second = first.clone();
    let capacity = [1, 1, 1];
    let eligible = vec![vec![true]; 3];
    for entry in [&first, &second] {
        assert!(
            check_funding_assignment(&capacity, &[1], &eligible, entry, cap(64), cap(64)).is_ok()
        );
    }
    assert_eq!(
        validate_batch(&capacity, &[first, second], &eligible),
        Err(batch_error(
            FundingAssignmentError::InsufficientSourceCapacity
        ))
    );
    assert_eq!(capacity, [1, 1, 1]);
}

#[test]
fn fixed_batch_all_six_orders_preserve_eligibility_and_balances() {
    let entries = [
        vec![vec![1, 0], vec![0, 1], vec![0, 0]],
        vec![vec![0, 0], vec![0, 1], vec![1, 1]],
        vec![vec![1, 0], vec![0, 0], vec![0, 1]],
    ];
    let eligible = vec![vec![true, false], vec![false, true], vec![true, true]];
    for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
        2, 1, 0,
    ]] {
        let batch: Vec<_> = order.iter().map(|index| entries[*index].clone()).collect();
        assert_eq!(
            validate_batch(&[3, 4, 5], &batch, &eligible),
            Ok(vec![1, 2, 2])
        );
    }
    let mut forbidden = entries[0].clone();
    forbidden[0][1] = 1;
    assert_eq!(
        validate_batch(&[3, 4, 5], &[forbidden], &eligible),
        Err(batch_error(FundingAssignmentError::IneligibleDraw))
    );
}

#[test]
fn batch_bounds_and_malformed_entries_return_no_partial_result() {
    assert_eq!(
        check_fixed_funding_batch(&[], &[], limits(), cap(1), &budget()),
        Err(batch_error(FundingAssignmentError::EmptySources))
    );
    assert_eq!(
        check_fixed_funding_batch(&[1; 65], &[], limits(), cap(1), &budget()),
        Err(batch_error(FundingAssignmentError::TooManySources))
    );
    for count in 1..=64 {
        let original = vec![u64::MAX; count];
        let checked =
            check_fixed_funding_batch(&original, &[], limits(), cap(1), &budget()).unwrap();
        assert_eq!(checked.remaining(), &original);
        assert_eq!(checked.source_debits(), &vec![0; count]);
        assert_eq!(checked.total(), 0);
    }
    let capacity = [3];
    let good = FixedFundingBatchEntry {
        obligations: &[1],
        eligible: &[vec![true]],
        assignment: &[vec![1]],
    };
    let invalid = FixedFundingBatchEntry {
        assignment: &[],
        ..good
    };
    assert_eq!(
        check_fixed_funding_batch(&capacity, &[good, invalid], limits(), cap(1), &budget()),
        Err(FundingBatchError::TooManyEntries)
    );
    assert_eq!(
        check_fixed_funding_batch(&capacity, &[good, invalid], limits(), cap(2), &budget()),
        Err(batch_error(FundingAssignmentError::InvalidDimensions))
    );
    let wrong = FixedFundingBatchEntry {
        obligations: &[2],
        ..good
    };
    assert_eq!(
        check_fixed_funding_batch(&capacity, &[good, wrong], limits(), cap(2), &budget()),
        Err(batch_error(FundingAssignmentError::ObligationMismatch))
    );
    let oversized = FixedFundingBatchEntry {
        obligations: &[0; 65],
        ..good
    };
    assert_eq!(
        check_fixed_funding_batch(&capacity, &[oversized], limits(), cap(1), &budget()),
        Err(batch_error(FundingAssignmentError::TooManyObligations))
    );
    assert_eq!(capacity, [3]);
}

#[test]
fn batch_total_can_exceed_one_source_integer_range_without_overdraw() {
    let capacity = [u64::MAX, u64::MAX];
    let first = FixedFundingBatchEntry {
        obligations: &[u64::MAX],
        eligible: &[vec![true], vec![false]],
        assignment: &[vec![u64::MAX], vec![0]],
    };
    let second = FixedFundingBatchEntry {
        obligations: &[u64::MAX],
        eligible: &[vec![false], vec![true]],
        assignment: &[vec![0], vec![u64::MAX]],
    };
    for entries in [[first, second], [second, first]] {
        let checked =
            check_fixed_funding_batch(&capacity, &entries, limits(), cap(2), &budget()).unwrap();
        assert_eq!(checked.remaining(), &[0, 0]);
        assert_eq!(checked.source_debits(), &capacity);
        assert_eq!(checked.total(), 2 * u128::from(u64::MAX));
    }
    let overflow = FixedFundingBatchEntry {
        obligations: &[u64::MAX, 1],
        eligible: &[vec![true, true]],
        assignment: &[vec![u64::MAX, 1]],
    };
    assert_eq!(
        check_fixed_funding_batch(&[u64::MAX], &[overflow], limits(), cap(1), &budget()),
        Err(batch_error(FundingAssignmentError::Overflow))
    );
}

#[test]
fn batch_host_work_limits_cover_success_and_late_failure() {
    let entry = FixedFundingBatchEntry {
        obligations: &[1],
        eligible: &[vec![true]],
        assignment: &[vec![1]],
    };
    let capacity = [3];
    let measured = budget();
    let expected =
        check_fixed_funding_batch(&capacity, &[entry, entry], limits(), cap(2), &measured).unwrap();
    for dimension in [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::SearchStateBytes,
    ] {
        let required = measured.usage(dimension).get();
        assert!(required > 0);
        let mut exact = measured.limits();
        exact.set(dimension, HostWorkLimit::new(required));
        assert_eq!(
            check_fixed_funding_batch(
                &capacity,
                &[entry, entry],
                limits(),
                cap(2),
                &HostWorkBudget::new(exact)
            )
            .unwrap(),
            expected
        );
        exact.set(dimension, HostWorkLimit::new(required - 1));
        assert!(matches!(
            check_fixed_funding_batch(
                &capacity,
                &[entry, entry],
                limits(),
                cap(2),
                &HostWorkBudget::new(exact)
            ),
            Err(FundingBatchError::Check(FundingSearchError::HostWork(_)))
        ));
        assert_eq!(capacity, [3]);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn n_payer_funding_solver_is_deterministic_and_conserves(
        capacities in prop::collection::vec(0_u64..32, 1..=64),
        requested in any::<u64>(),
        cursor_seed in any::<usize>(),
    ) {
        let available = capacities.iter().map(|value| u128::from(*value)).sum::<u128>();
        let amount = u128::from(requested).min(available) as u64;
        let cursor = cursor_seed % capacities.len();
        let first = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
        let second = allocate_capped_max_min(&capacities, amount, cursor, cap(64)).unwrap();
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(first.debits.iter().sum::<u64>(), amount);
        prop_assert!(first.debits.iter().zip(&capacities).all(|(debit, capacity)| debit <= capacity));
        prop_assert!(first.next_cursor < capacities.len());
    }

    #[test]
    fn fixed_batch_permutations_match_per_source_conservation(
        source_count in 1usize..=64,
        obligation_count in 1usize..=5,
        batch_count in 0usize..=12,
        amounts in prop::collection::vec(0_u64..1000, 1..100),
        permissions in prop::collection::vec(any::<bool>(), 1..100),
        rotation in any::<usize>(),
    ) {
        let eligible: Vec<Vec<_>> = (0..source_count).map(|source| {
            (0..obligation_count).map(|obligation| {
                permissions[(source * obligation_count + obligation) % permissions.len()]
            }).collect()
        }).collect();
        let entries: Vec<Vec<Vec<u64>>> = (0..batch_count).map(|batch| {
            (0..source_count).map(|source| {
                (0..obligation_count).map(|obligation| {
                    if eligible[source][obligation] {
                        amounts[(batch * source_count * obligation_count + source * obligation_count + obligation) % amounts.len()]
                    } else { 0 }
                }).collect()
            }).collect()
        }).collect();
        let capacities: Vec<_> = (0..source_count).map(|source| {
            7 + entries.iter().flat_map(|entry| &entry[source]).sum::<u64>()
        }).collect();
        let expected = vec![7; source_count];
        prop_assert_eq!(validate_batch(&capacities, &entries, &eligible).unwrap(), &expected[..]);
        let mut reordered = entries.clone();
        if batch_count != 0 { reordered.rotate_left(rotation % batch_count); }
        reordered.reverse();
        prop_assert_eq!(validate_batch(&capacities, &reordered, &eligible).unwrap(), expected);
        for source in 0..source_count {
            let total = capacities[source] - 7;
            if total > 0 {
                let mut insufficient = capacities.clone();
                insufficient[source] = total - 1;
                prop_assert_eq!(validate_batch(&insufficient, &entries, &eligible), Err(batch_error(FundingAssignmentError::InsufficientSourceCapacity)));
                prop_assert_eq!(validate_batch(&insufficient, &reordered, &eligible), Err(batch_error(FundingAssignmentError::InsufficientSourceCapacity)));
            }
        }
    }

    #[test]
    fn batch_acceptance_matches_independent_aggregate_oracle(
        source_count in 1usize..=16,
        raw_entries in prop::collection::vec(
            (0usize..=5, prop::collection::vec((0_u64..8, any::<bool>()), 1..81)),
            0..=8,
        ),
        exact_demands in any::<bool>(),
        sufficient_capacities in any::<bool>(),
        permit_draws in any::<bool>(),
    ) {
        let assignments: Vec<Vec<Vec<u64>>> = raw_entries.iter().map(|(count, cells)| {
            (0..source_count).map(|source| {
                (0..*count).map(|column| cells[(source * count + column) % cells.len()].0).collect()
            }).collect()
        }).collect();
        let eligibility: Vec<Vec<Vec<bool>>> = raw_entries.iter().map(|(count, cells)| {
            (0..source_count).map(|source| {
                (0..*count).map(|column| permit_draws || cells[(source * count + column) % cells.len()].1).collect()
            }).collect()
        }).collect();
        let demands: Vec<Vec<u64>> = raw_entries.iter().zip(&assignments).map(|((count, _), assignment)| {
            (0..*count).map(|column| {
                assignment.iter().map(|row| row[column]).sum::<u64>() + u64::from(!exact_demands)
            }).collect()
        }).collect();
        let aggregate: Vec<u128> = (0..source_count).map(|source| {
            assignments.iter().flat_map(|entry| &entry[source]).map(|draw| u128::from(*draw)).sum()
        }).collect();
        let capacities: Vec<u64> = aggregate.iter().map(|draw| {
            if sufficient_capacities { *draw as u64 } else { (*draw as u64).saturating_sub(1) }
        }).collect();
        let entries: Vec<_> = assignments.iter().zip(&eligibility).zip(&demands).map(|((assignment, eligible), obligations)| {
            FixedFundingBatchEntry { assignment, eligible, obligations }
        }).collect();
        let expected_valid = aggregate.iter().zip(&capacities).all(|(draw, capacity)| *draw <= u128::from(*capacity))
            && entries.iter().all(|entry| {
                entry.assignment.iter().zip(entry.eligible).all(|(row, allowed)| {
                    row.iter().zip(allowed).all(|(draw, permitted)| *draw == 0 || *permitted)
                }) && entry.obligations.iter().enumerate().all(|(column, demand)| {
                    entry.assignment.iter().map(|row| u128::from(row[column])).sum::<u128>() == u128::from(*demand)
                })
            });
        for batch in [entries.clone(), entries.into_iter().rev().collect()] {
            let actual = check_fixed_funding_batch(&capacities, &batch, limits(), cap(8), &budget());
            prop_assert_eq!(actual.is_ok(), expected_valid);
            if let Ok(checked) = actual {
                prop_assert_eq!(checked.total(), aggregate.iter().sum::<u128>());
                for (source, draw) in aggregate.iter().enumerate() {
                    prop_assert_eq!(u128::from(checked.source_debits()[source]), *draw);
                    prop_assert_eq!(u128::from(checked.remaining()[source]) + draw, u128::from(capacities[source]));
                }
            }
        }
    }
}
