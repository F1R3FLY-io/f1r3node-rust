use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::{
    FundingMinimaxProblem, FundingSearchLimits,
};

fn limits() -> FundingFamilyLimits {
    FundingFamilyLimits {
        search: FundingSearchLimits {
            source_cap: NonZeroUsize::new(129).unwrap(),
            obligation_cap: NonZeroUsize::new(16).unwrap(),
        },
        case_cap: NonZeroUsize::new(16).unwrap(),
    }
}

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    draws: Vec<u64>,
    fees: Vec<u64>,
}

fn candidates(
    outcome: FundingOutcomeProblem<'_>,
    capacities: &[u64],
    constraint: FundingResourceConstraint<'_>,
    forced: Option<usize>,
) -> Vec<Candidate> {
    fn visit(
        outcome: FundingOutcomeProblem<'_>,
        cell: usize,
        available: &mut [u64],
        remaining: &mut [u64],
        matrix: &mut [Vec<u64>],
        output: &mut Vec<Candidate>,
    ) {
        if cell == available.len() * remaining.len() {
            if remaining.iter().any(|amount| *amount != 0) {
                return;
            }
            let draws: Vec<u64> = matrix.iter().map(|row| row.iter().sum()).collect();
            if let Some(eligible) = outcome.unit_fee_eligible {
                for payer in 0..available.len() {
                    if eligible[payer] && available[payer] > 0 {
                        let mut fees = vec![0; available.len()];
                        fees[payer] = 1;
                        output.push(Candidate {
                            draws: draws.clone(),
                            fees,
                        });
                    }
                }
            } else {
                output.push(Candidate {
                    draws,
                    fees: vec![0; available.len()],
                });
            }
            return;
        }
        let i = cell / remaining.len();
        let j = cell % remaining.len();
        let maximum = if outcome.resources.eligible[i][j] {
            available[i].min(remaining[j])
        } else {
            0
        };
        for amount in 0..=maximum {
            available[i] -= amount;
            remaining[j] -= amount;
            matrix[i][j] = amount;
            visit(outcome, cell + 1, available, remaining, matrix, output);
            available[i] += amount;
            remaining[j] += amount;
        }
        matrix[i][j] = 0;
    }
    let n = capacities.len();
    let mut available: Vec<_> = capacities
        .iter()
        .zip(outcome.resources.capacities)
        .map(|(a, b)| (*a).min(*b))
        .collect();
    let mut result = Vec::new();
    visit(
        outcome,
        0,
        &mut available,
        &mut outcome.resources.obligations.to_vec(),
        &mut vec![vec![0; outcome.resources.obligations.len()]; n],
        &mut result,
    );
    result.retain(|candidate| {
        let matches = match constraint {
            FundingResourceConstraint::Fixed(draws) => candidate.draws == draws,
            FundingResourceConstraint::Rank(rank) => {
                let mut actual = candidate.draws.clone();
                actual.sort_unstable_by(|a, b| b.cmp(a));
                actual == rank
            }
        };
        matches && forced.is_none_or(|payer| candidate.fees[payer] == 1)
    });
    result
}

fn assert_oracle(
    problem: FundingFamilyOptimizationProblem<'_>,
    constraints: &[FundingResourceConstraint<'_>],
    forced: &[Option<usize>],
) {
    fn exists(
        alternatives: &[Vec<Candidate>],
        depth: usize,
        holds: &[u64],
        exposure: u128,
    ) -> bool {
        if depth == alternatives.len() {
            return true;
        }
        alternatives[depth].iter().any(|candidate| {
            let next: Vec<_> = holds
                .iter()
                .enumerate()
                .map(|(i, held)| (*held).max(candidate.draws[i] + candidate.fees[i]))
                .collect();
            next.iter().map(|held| u128::from(*held)).sum::<u128>() <= exposure
                && exists(alternatives, depth + 1, &next, exposure)
        })
    }
    let alternatives: Vec<_> = problem
        .outcomes
        .iter()
        .enumerate()
        .map(|(i, outcome)| candidates(*outcome, problem.capacities, constraints[i], forced[i]))
        .collect();
    let expected = exists(
        &alternatives,
        0,
        &vec![0; problem.capacities.len()],
        problem.exposure_limit,
    );
    let result =
        solve_funding_family_slice(problem, constraints, forced, limits(), &work()).unwrap();
    assert_eq!(result.is_some(), expected);
    if let Some(result) = result {
        let mut holds = vec![0_u64; problem.capacities.len()];
        for (i, options) in alternatives.iter().enumerate() {
            assert!(options.contains(&Candidate {
                draws: result.draws[i].clone(),
                fees: result.fees[i].clone()
            }));
            for (source, held) in holds.iter_mut().enumerate() {
                *held = (*held).max(result.draws[i][source] + result.fees[i][source]);
            }
        }
        assert_eq!(result.holds, holds);
        assert!(holds.iter().map(|held| u128::from(*held)).sum::<u128>() <= problem.exposure_limit);
    }
}

#[test]
fn every_small_two_source_rank_fee_and_exposure_domain_matches_oracle() {
    let caps = [2, 2];
    for edges in 0..16 {
        let first = [vec![edges & 1 != 0], vec![edges & 2 != 0]];
        let second = [vec![edges & 4 != 0], vec![edges & 8 != 0]];
        for fees in 0..16 {
            let fee_a = [fees & 1 != 0, fees & 2 != 0];
            let fee_b = [fees & 4 != 0, fees & 8 != 0];
            let outcomes = [
                FundingOutcomeProblem {
                    resources: FundingMinimaxProblem {
                        capacities: &caps,
                        obligations: &[1],
                        eligible: &first,
                    },
                    unit_fee_eligible: Some(&fee_a),
                },
                FundingOutcomeProblem {
                    resources: FundingMinimaxProblem {
                        capacities: &caps,
                        obligations: &[1],
                        eligible: &second,
                    },
                    unit_fee_eligible: Some(&fee_b),
                },
            ];
            for exposure_limit in 1..=4 {
                assert_oracle(
                    FundingFamilyOptimizationProblem {
                        capacities: &caps,
                        outcomes: &outcomes,
                        exposure_limit,
                        resource_cursor: 0,
                        fee_cursor: 1,
                    },
                    &[
                        FundingResourceConstraint::Rank(&[1, 0]),
                        FundingResourceConstraint::Rank(&[1, 0]),
                    ],
                    &[None, None],
                );
            }
        }
    }
}

#[test]
fn family_rank_need_not_be_locally_minimax_and_still_requires_resource_flow() {
    let caps = [2, 2];
    let all = [vec![true], vec![true]];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &caps,
            obligations: &[2],
            eligible: &all,
        },
        unit_fee_eligible: None,
    }];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &caps,
        outcomes: &outcomes,
        exposure_limit: 2,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let selected = solve_funding_family_slice(
        problem,
        &[FundingResourceConstraint::Rank(&[2, 0])],
        &[None],
        limits(),
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.draws, [vec![2, 0]]);
    assert_oracle(problem, &[FundingResourceConstraint::Rank(&[2, 0])], &[
        None,
    ]);
    let diagonal = [vec![true, false], vec![false, true]];
    let restricted = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            obligations: &[1, 1],
            eligible: &diagonal,
            ..outcomes[0].resources
        },
        ..outcomes[0]
    }];
    let restricted_problem = FundingFamilyOptimizationProblem {
        outcomes: &restricted,
        ..problem
    };
    assert!(solve_funding_family_slice(
        restricted_problem,
        &[FundingResourceConstraint::Rank(&[2, 0])],
        &[None],
        limits(),
        &work()
    )
    .unwrap()
    .is_none());
    assert_oracle(
        restricted_problem,
        &[FundingResourceConstraint::Rank(&[2, 0])],
        &[None],
    );
    assert_oracle(
        restricted_problem,
        &[FundingResourceConstraint::Rank(&[1, 1])],
        &[None],
    );
}

#[test]
fn fixed_draws_forced_fees_and_cross_outcome_backtracking_preserve_feasibility() {
    let caps = [1, 1, 1];
    let first = [vec![true], vec![true], vec![false]];
    let second = [vec![false], vec![true], vec![false]];
    let fees = [false, false, true];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &caps,
                obligations: &[1],
                eligible: &first,
            },
            unit_fee_eligible: Some(&fees),
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &caps,
                obligations: &[1],
                eligible: &second,
            },
            unit_fee_eligible: Some(&fees),
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &caps,
        outcomes: &outcomes,
        exposure_limit: 2,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let constraints = [
        FundingResourceConstraint::Rank(&[1, 0, 0]),
        FundingResourceConstraint::Fixed(&[0, 1, 0]),
    ];
    let selected =
        solve_funding_family_slice(problem, &constraints, &[Some(2), None], limits(), &work())
            .unwrap()
            .unwrap();
    assert_eq!(selected.draws, [vec![0, 1, 0], vec![0, 1, 0]]);
    assert_eq!(selected.holds, [0, 1, 1]);
    assert_oracle(problem, &constraints, &[Some(2), None]);
    assert_oracle(problem, &constraints, &[Some(1), None]);
}

#[test]
fn duplicate_rank_values_large_cohorts_and_full_width_fees_need_no_token_iteration() {
    for n in [3, 65, 129] {
        let caps = vec![1; n];
        let rank = vec![1; n];
        let edges = vec![vec![true]; n];
        let amounts = [n as u64];
        let outcomes = [FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &caps,
                obligations: &amounts,
                eligible: &edges,
            },
            unit_fee_eligible: None,
        }];
        let selected = solve_funding_family_slice(
            FundingFamilyOptimizationProblem {
                capacities: &caps,
                outcomes: &outcomes,
                exposure_limit: n as u128,
                resource_cursor: n - 1,
                fee_cursor: n - 1,
            },
            &[FundingResourceConstraint::Rank(&rank)],
            &[None],
            limits(),
            &work(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.draws, [rank]);
        assert_eq!(selected.holds, caps);
    }
    let caps = [u64::MAX, 1, 1];
    let edges = [vec![true], vec![true], vec![true]];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &caps,
            obligations: &[u64::MAX],
            eligible: &edges,
        },
        unit_fee_eligible: Some(&[true, false, false]),
    }];
    let selected = solve_funding_family_slice(
        FundingFamilyOptimizationProblem {
            capacities: &caps,
            outcomes: &outcomes,
            exposure_limit: u128::from(u64::MAX) + 1,
            resource_cursor: 0,
            fee_cursor: 0,
        },
        &[FundingResourceConstraint::Rank(&[u64::MAX - 2, 1, 1])],
        &[Some(0)],
        limits(),
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.draws, [vec![u64::MAX - 2, 1, 1]]);
    assert_eq!(selected.fees, [vec![1, 0, 0]]);
    assert_eq!(selected.holds, [u64::MAX - 1, 1, 1]);
}

#[test]
fn late_budget_exhaustion_never_returns_a_partial_family() {
    let capacities = [1, 1];
    let first = [vec![true], vec![true]];
    let second = [vec![false], vec![true]];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &first,
            },
            unit_fee_eligible: None,
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &second,
            },
            unit_fee_eligible: None,
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 1,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let constraints = [
        FundingResourceConstraint::Rank(&[1, 0]),
        FundingResourceConstraint::Rank(&[1, 0]),
    ];
    let complete = work();
    assert!(
        solve_funding_family_slice(problem, &constraints, &[None, None], limits(), &complete)
            .unwrap()
            .is_some()
    );
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let used = complete.usage(dimension).get();
        assert!(used > 0);
        let mut capped = complete.limits();
        capped.set(dimension, HostWorkLimit::new(used - 1));
        assert!(matches!(
            solve_funding_family_slice(
                problem,
                &constraints,
                &[None, None],
                limits(),
                &HostWorkBudget::new(capped)
            ),
            Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
        ));
    }
}

#[test]
fn malformed_later_constraints_and_budget_exhaustion_are_errors() {
    let caps = [0, 0];
    let edges = [vec![true], vec![true]];
    let case = FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &caps,
            obligations: &[1],
            eligible: &edges,
        },
        unit_fee_eligible: None,
    };
    let outcomes = [case, case];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &caps,
        outcomes: &outcomes,
        exposure_limit: 0,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    assert!(matches!(
        solve_funding_family_slice(
            problem,
            &[
                FundingResourceConstraint::Rank(&[1, 0]),
                FundingResourceConstraint::Rank(&[0, 1])
            ],
            &[None, None],
            limits(),
            &work()
        ),
        Err(FundingFamilyError::Search(
            FundingSearchError::InvalidResult
        ))
    ));
    let empty = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        solve_funding_family_slice(
            problem,
            &[
                FundingResourceConstraint::Rank(&[1, 0]),
                FundingResourceConstraint::Rank(&[1, 0])
            ],
            &[None, None],
            limits(),
            &empty
        ),
        Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_fixed_and_rank_slices_match_complete_assignment_oracle(
        data in (1_usize..4,1_usize..4).prop_flat_map(|(n,k)| (
            Just((n,k)),prop::collection::vec(0_u64..4,n),prop::collection::vec(0_u64..4,n*k),
            prop::collection::vec(0_u64..3,n*k),prop::collection::vec(any::<bool>(),n*k),
            prop::collection::vec(any::<bool>(),n*k),prop::collection::vec(any::<bool>(),k),
            prop::collection::vec(any::<bool>(),k),prop::collection::vec(0_usize..=n,k),
            0_u128..10,0_usize..n,0_usize..n,
        ))
    ) {
        let ((n,k),caps,case_caps,draws,edges,fees,charged,fixed,forced,exposure_limit,resource_cursor,fee_cursor)=data;
        let amounts:Vec<_>=(0..k).map(|i| vec![draws[i*n..(i+1)*n].iter().sum()]).collect();
        let eligibility:Vec<Vec<Vec<_>>>=(0..k).map(|i| (0..n).map(|j| vec![edges[i*n+j]]).collect()).collect();
        let ranked:Vec<Vec<_>>=(0..k).map(|i| {let mut values=draws[i*n..(i+1)*n].to_vec();values.sort_unstable_by(|a,b| b.cmp(a));values}).collect();
        let constraints:Vec<_>=(0..k).map(|i| if fixed[i] {FundingResourceConstraint::Fixed(&draws[i*n..(i+1)*n])}else{FundingResourceConstraint::Rank(&ranked[i])}).collect();
        let forced:Vec<_>=(0..k).map(|i| (charged[i]&&forced[i]<n).then_some(forced[i])).collect();
        let outcomes:Vec<_>=(0..k).map(|i| FundingOutcomeProblem {resources:FundingMinimaxProblem {capacities:&case_caps[i*n..(i+1)*n],obligations:&amounts[i],eligible:&eligibility[i]},unit_fee_eligible:charged[i].then_some(&fees[i*n..(i+1)*n])}).collect();
        assert_oracle(FundingFamilyOptimizationProblem {capacities:&caps,outcomes:&outcomes,exposure_limit,resource_cursor,fee_cursor},&constraints,&forced);
    }
}
