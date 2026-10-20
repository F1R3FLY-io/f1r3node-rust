use proptest::prelude::*;

use super::super::{
    check_funding_domain_counterexample, classify_fixed_funding_domain, FundingDomainClassification,
};
use super::*;

fn feasible(
    capacity: &[u64],
    demand: &[u64],
    eligible: &[Vec<bool>],
) -> Result<bool, FundingSearchError> {
    let limits = FundingSearchLimits {
        source_cap: NonZeroUsize::new(capacity.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(demand.len().max(1)).unwrap(),
    };
    Ok(matches!(
        solve_funding_feasibility(capacity, demand, eligible, limits)?,
        FundingFeasibility::Feasible { .. }
    ))
}

fn transposed_classification(
    capacity: &[u64],
    demand: &[u64],
    eligible: &[Vec<bool>],
) -> Result<Option<bool>, FundingSearchError> {
    let limits = FundingSearchLimits {
        source_cap: NonZeroUsize::new(capacity.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(demand.len().max(1)).unwrap(),
    };
    match classify_fixed_funding_domain(capacity, demand, eligible, limits)? {
        FundingDomainClassification::Infeasible {
            selected_obligations,
        } => {
            assert!(check_funding_deficit(
                capacity,
                demand,
                eligible,
                &selected_obligations,
                limits.source_cap,
                limits.obligation_cap
            )
            .unwrap());
            Ok(None)
        }
        FundingDomainClassification::Unrestricted { assignment, totals } => {
            assert_eq!(
                check_funding_assignment(
                    capacity,
                    demand,
                    eligible,
                    &assignment,
                    limits.source_cap,
                    limits.obligation_cap
                ),
                Ok(totals)
            );
            Ok(Some(true))
        }
        FundingDomainClassification::Restricted {
            assignment,
            totals,
            counterexample,
        } => {
            assert_eq!(
                check_funding_assignment(
                    capacity,
                    demand,
                    eligible,
                    &assignment,
                    limits.source_cap,
                    limits.obligation_cap
                ),
                Ok(totals)
            );
            assert!(check_funding_domain_counterexample(
                capacity,
                demand,
                eligible,
                counterexample.view(),
                limits
            )?);
            assert!(!feasible(
                counterexample.view().contributions,
                demand,
                eligible
            )?);
            Ok(Some(false))
        }
    }
}

fn unit_assignment_oracle(
    remaining: &mut [u64],
    demand: &mut [u64],
    eligible: &[Vec<bool>],
) -> bool {
    let Some(j) = demand.iter().position(|amount| *amount > 0) else {
        return true;
    };
    demand[j] -= 1;
    for i in 0..remaining.len() {
        if remaining[i] > 0 && eligible[i][j] {
            remaining[i] -= 1;
            let accepted = unit_assignment_oracle(remaining, demand, eligible);
            remaining[i] += 1;
            if accepted {
                demand[j] += 1;
                return true;
            }
        }
    }
    demand[j] += 1;
    false
}

fn all_contribution_vectors(
    capacity: &[u64],
    demand: &[u64],
    eligible: &[Vec<bool>],
) -> Option<bool> {
    if !unit_assignment_oracle(&mut capacity.to_vec(), &mut demand.to_vec(), eligible) {
        return None;
    }
    let total: u64 = demand.iter().sum();
    let mut vector = vec![0_u64; capacity.len()];
    loop {
        if vector.iter().sum::<u64>() == total
            && !unit_assignment_oracle(&mut vector.clone(), &mut demand.to_vec(), eligible)
        {
            return Some(false);
        }
        let mut position = 0;
        while position < vector.len() && vector[position] == capacity[position] {
            vector[position] = 0;
            position += 1;
        }
        if position == vector.len() {
            return Some(true);
        }
        vector[position] += 1;
    }
}

#[test]
fn transposed_reduction_catches_combined_restrictions_missed_by_scalar_checks() {
    let capacity = [3, 6];
    let demand = [2, 2, 2];
    let eligible = [vec![false, false, true], vec![true, true, true]];
    for j in 0..demand.len() {
        let excluded: u64 = capacity
            .iter()
            .zip(&eligible)
            .filter(|(_, row)| !row[j])
            .map(|(cap, _)| *cap)
            .sum();
        assert!(excluded <= 6 - demand[j]);
    }
    assert_eq!(
        transposed_classification(&capacity, &demand, &eligible),
        Ok(Some(false))
    );
    assert_eq!(
        all_contribution_vectors(&capacity, &demand, &eligible),
        Some(false)
    );
    assert!(!unit_assignment_oracle(
        &mut [3, 3],
        &mut demand.clone(),
        &eligible
    ));
}

#[test]
fn redundant_missing_edges_do_not_select_the_restricted_policy() {
    let capacity = [1, 1];
    let demand = [1, 1];
    let eligible = [vec![true, false], vec![true, true]];
    assert_eq!(
        all_contribution_vectors(&capacity, &demand, &eligible),
        Some(true)
    );
    assert_eq!(
        transposed_classification(&capacity, &demand, &eligible),
        Ok(Some(true))
    );
}

#[test]
fn exhaustive_two_by_two_domains_match_all_integer_contribution_vectors() {
    let mut cases = 0;
    for a in 0..=2 {
        for b in 0..=2 {
            for x in 0..=2 {
                for y in 0..=2 {
                    for bits in 0_u8..16 {
                        let eligible = [vec![bits & 1 != 0, bits & 2 != 0], vec![
                            bits & 4 != 0,
                            bits & 8 != 0,
                        ]];
                        let expected = all_contribution_vectors(&[a, b], &[x, y], &eligible);
                        assert_eq!(
                            transposed_classification(&[a, b], &[x, y], &eligible),
                            Ok(expected),
                            "capacity={:?}, demand={:?}, edges={eligible:?}",
                            [a, b],
                            [x, y]
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1296);
}

#[test]
fn transposed_reduction_handles_wide_capacities_zero_demands_and_large_cohorts() {
    for count in [1, 2, 3, 64, 65, 129] {
        let capacity = vec![u64::MAX; count];
        assert_eq!(
            transposed_classification(&capacity, &[], &vec![vec![]; count]),
            Ok(Some(true))
        );
        assert_eq!(
            transposed_classification(&capacity, &[u64::MAX], &vec![vec![true]; count]),
            Ok(Some(true))
        );
        if count > 1 {
            let mut eligible = vec![vec![true, true]; count];
            eligible[0][0] = false;
            assert_eq!(
                transposed_classification(&capacity, &[1, u64::MAX - 1], &eligible),
                Ok(Some(false))
            );
        }
    }
}

#[test]
fn classifier_preserves_dimension_overflow_and_source_limit_errors() {
    let limits = FundingSearchLimits {
        source_cap: NonZeroUsize::new(1).unwrap(),
        obligation_cap: NonZeroUsize::new(2).unwrap(),
    };
    assert_eq!(
        classify_fixed_funding_domain(&[], &[], &[], limits),
        Err(FundingAssignmentError::EmptySources.into())
    );
    assert_eq!(
        classify_fixed_funding_domain(&[1, 1], &[1], &[vec![true], vec![true]], limits),
        Err(FundingAssignmentError::TooManySources.into())
    );
    assert_eq!(
        classify_fixed_funding_domain(&[1], &[1, 1, 1], &[vec![true; 3]], limits),
        Err(FundingAssignmentError::TooManyObligations.into())
    );
    assert_eq!(
        classify_fixed_funding_domain(&[1], &[1], &[vec![]], limits),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        classify_fixed_funding_domain(&[u64::MAX], &[u64::MAX, 1], &[vec![true; 2]], limits),
        Err(FundingAssignmentError::Overflow.into())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_domains_match_complete_assignment_oracle_and_permutations(
        capacity in prop::collection::vec(0_u64..3, 1..6),
        demand in prop::collection::vec(0_u64..3, 0..4),
        bits in prop::collection::vec(any::<bool>(), 15),
    ) {
        let eligible: Vec<Vec<bool>> = (0..capacity.len())
            .map(|i| (0..demand.len()).map(|j| bits[i * 3 + j]).collect()).collect();
        let expected = all_contribution_vectors(&capacity, &demand, &eligible);
        prop_assert_eq!(transposed_classification(&capacity, &demand, &eligible), Ok(expected));
        let reversed_capacity: Vec<_> = capacity.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev()
            .map(|row| row.iter().rev().copied().collect()).collect();
        prop_assert_eq!(transposed_classification(&reversed_capacity, &reversed_demand, &reversed_edges), Ok(expected));
    }
}
