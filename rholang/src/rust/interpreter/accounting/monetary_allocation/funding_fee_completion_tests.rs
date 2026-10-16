use proptest::prelude::*;

use super::super::{
    select_fixed_funding_policy, FundingMinimaxProblem, FundingPolicyResult, FundingSearchLimits,
};
use super::*;

fn cap(count: usize) -> NonZeroUsize { NonZeroUsize::new(count).unwrap() }

fn selected(capacities: &[u64], obligations: &[u64], eligible: &[Vec<bool>]) -> Vec<Vec<u64>> {
    let result = select_fixed_funding_policy(
        FundingMinimaxProblem {
            capacities,
            obligations,
            eligible,
        },
        0,
        FundingSearchLimits {
            source_cap: cap(capacities.len()),
            obligation_cap: cap(obligations.len()),
        },
    )
    .unwrap();
    match result {
        FundingPolicyResult::Selected(plan) => plan.assignment().to_vec(),
        FundingPolicyResult::Infeasible { .. } => panic!("feasible fixture"),
    }
}

#[test]
fn folding_the_fee_into_resource_fairness_changes_the_selected_resource_draws() {
    let resources = selected(&[10, 10], &[2], &[vec![true], vec![true]]);
    assert_eq!(resources, vec![vec![1], vec![1]]);
    let combined = selected(&[10, 10], &[1, 2], &[vec![true; 2], vec![true; 2]]);
    assert_eq!(combined, vec![vec![0, 2], vec![1, 0]]);
    assert_ne!(combined.iter().map(|row| row[1]).collect::<Vec<_>>(), vec![
        1, 1
    ]);
}

#[test]
fn resource_only_selection_can_hide_a_jointly_feasible_plan() {
    let chosen = selected(&[1, 1], &[1], &[vec![true], vec![true]]);
    assert_eq!(chosen, vec![vec![1], vec![0]]);
    assert!(!can_complete_unit_fee(&[1, 1], &[1, 0], &[true, false], cap(2)).unwrap());
    assert!(can_complete_unit_fee(&[1, 1], &[0, 1], &[true, false], cap(2)).unwrap());
}

#[test]
fn fee_completion_checks_every_source() {
    assert!(matches!(
        can_complete_unit_fee(&[2, 0], &[0, 1], &[true; 2], cap(2)),
        Err(FundingSearchError::InvalidProblem(
            FundingAssignmentError::InsufficientSourceCapacity
        ))
    ));
    for count in [1, 2, 3, 64, 65, 129] {
        let capacities = vec![u64::MAX; count];
        for allowed in [false, true] {
            assert_eq!(
                can_complete_unit_fee(
                    &capacities,
                    &vec![0; count],
                    &vec![allowed; count],
                    cap(count)
                )
                .unwrap(),
                allowed
            );
            assert!(!can_complete_unit_fee(
                &capacities,
                &capacities,
                &vec![allowed; count],
                cap(count)
            )
            .unwrap());
        }
    }
    assert!(can_complete_unit_fee(&[], &[], &[], cap(1)).is_err());
    assert!(can_complete_unit_fee(&[1, 1], &[0, 0], &[true; 2], cap(1)).is_err());
    assert!(can_complete_unit_fee(&[1, 1], &[0], &[true; 2], cap(2)).is_err());
    assert!(can_complete_unit_fee(&[1, 1], &[0, 0], &[true], cap(2)).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn fee_completion_matches_the_independent_remaining_sum(
        rows in prop::collection::vec((any::<u64>(), any::<u64>(), any::<bool>()), 1..=129),
    ) {
        let capacities: Vec<_> = rows.iter().map(|row| row.0).collect();
        let draws: Vec<_> = rows.iter().map(|row| row.1.min(row.0)).collect();
        let eligible: Vec<_> = rows.iter().map(|row| row.2).collect();
        let remaining: u128 = rows.iter().map(|(capacity, draw, eligible)| if *eligible {
            u128::from(*capacity) - u128::from((*draw).min(*capacity))
        } else { 0 }).sum();
        prop_assert_eq!(can_complete_unit_fee(&capacities, &draws, &eligible, cap(rows.len())).unwrap(), remaining > 0);
    }
}
