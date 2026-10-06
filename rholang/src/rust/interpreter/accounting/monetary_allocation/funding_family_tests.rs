use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> FundingFamilyLimits {
    FundingFamilyLimits {
        search: FundingSearchLimits {
            source_cap: NonZeroUsize::new(129).unwrap(),
            obligation_cap: NonZeroUsize::new(16).unwrap(),
        },
        case_cap: NonZeroUsize::new(16).unwrap(),
    }
}

fn assert_witness(problem: FundingFamilyProblem<'_>, witness: &FundingFamilyWitness) {
    assert_eq!(problem.cases.len(), witness.assignments().len());
    assert_eq!(problem.cases.len(), witness.totals().len());
    let mut holds = vec![0; problem.capacities.len()];
    for ((case, matrix), totals) in problem
        .cases
        .iter()
        .zip(witness.assignments())
        .zip(witness.totals())
    {
        assert_eq!(matrix.len(), problem.capacities.len());
        for (i, row) in matrix.iter().enumerate() {
            assert_eq!(row.len(), case.obligations.len());
            let draw: u64 = row.iter().sum();
            assert!(draw >= case.lower[i]);
            assert!(draw <= case.upper[i]);
            assert!(draw <= problem.capacities[i]);
            assert_eq!(draw, totals.source_debits()[i]);
            holds[i] = holds[i].max(draw);
            for (j, amount) in row.iter().enumerate() {
                assert!(*amount == 0 || case.eligible[i][j]);
            }
        }
        for (j, required) in case.obligations.iter().enumerate() {
            assert_eq!(matrix.iter().map(|row| row[j]).sum::<u64>(), *required);
        }
    }
    assert_eq!(witness.holds(), holds);
    assert_eq!(
        witness.total_held(),
        holds.iter().map(|x| u128::from(*x)).sum::<u128>()
    );
    assert!(witness.total_held() <= problem.exposure_limit);
}

fn case_oracle(case: FundingBoxProblem<'_>, holds: &[u64]) -> bool {
    fn visit(
        case: FundingBoxProblem<'_>,
        cell: usize,
        draws: &mut [u64],
        remaining: &mut [u64],
        holds: &[u64],
    ) -> bool {
        if cell == holds.len() * remaining.len() {
            return remaining.iter().all(|x| *x == 0)
                && draws
                    .iter()
                    .zip(case.lower)
                    .all(|(draw, lower)| draw >= lower);
        }
        let i = cell / remaining.len();
        let j = cell % remaining.len();
        let cap = holds[i].min(case.upper[i]);
        let upper = if case.eligible[i][j] {
            (cap - draws[i]).min(remaining[j])
        } else {
            0
        };
        for amount in 0..=upper {
            draws[i] += amount;
            remaining[j] -= amount;
            let found = visit(case, cell + 1, draws, remaining, holds);
            draws[i] -= amount;
            remaining[j] += amount;
            if found {
                return true;
            }
        }
        false
    }
    visit(
        case,
        0,
        &mut vec![0; holds.len()],
        &mut case.obligations.to_vec(),
        holds,
    )
}

fn family_oracle(problem: FundingFamilyProblem<'_>) -> bool {
    fn visit(problem: FundingFamilyProblem<'_>, i: usize, holds: &mut [u64]) -> bool {
        if i == holds.len() {
            return holds.iter().map(|x| u128::from(*x)).sum::<u128>() <= problem.exposure_limit
                && problem.cases.iter().all(|case| case_oracle(*case, holds));
        }
        for amount in 0..=problem.capacities[i] {
            holds[i] = amount;
            if visit(problem, i + 1, holds) {
                return true;
            }
        }
        false
    }
    visit(problem, 0, &mut vec![0; problem.capacities.len()])
}

#[test]
fn family_search_recovers_the_incompatible_independent_ties() {
    let capacities = [2, 2, 2, 2, 1];
    let lower = [0; 5];
    let alpha = vec![
        vec![false, false, false],
        vec![false, true, false],
        vec![false, true, true],
        vec![false, true, false],
        vec![true, false, false],
    ];
    let beta = vec![
        vec![false, true, false],
        vec![false, true, false],
        vec![false, false, true],
        vec![false, false, true],
        vec![true, false, false],
    ];
    let cases = [
        FundingBoxProblem {
            lower: &lower,
            upper: &capacities,
            obligations: &[1, 1, 1],
            eligible: &alpha,
        },
        FundingBoxProblem {
            lower: &lower,
            upper: &capacities,
            obligations: &[1, 1, 2],
            eligible: &beta,
        },
    ];
    for limit in 0..=5 {
        let problem = FundingFamilyProblem {
            capacities: &capacities,
            cases: &cases,
            exposure_limit: limit,
        };
        let selected = solve_funding_family_feasibility(problem, limits(), &work()).unwrap();
        assert_eq!(selected.is_some(), limit >= 4);
        if let Some(witness) = selected {
            assert_witness(problem, &witness);
        }
    }
}

#[test]
fn family_search_does_not_reject_a_feasible_family_after_one_bad_witness() {
    let capacities = [1, 1];
    let flexible = vec![vec![true], vec![true]];
    let restricted = vec![vec![false], vec![true]];
    let cases = [
        FundingBoxProblem {
            lower: &[0, 0],
            upper: &capacities,
            obligations: &[1],
            eligible: &flexible,
        },
        FundingBoxProblem {
            lower: &[0, 0],
            upper: &capacities,
            obligations: &[1],
            eligible: &restricted,
        },
    ];
    let problem = FundingFamilyProblem {
        capacities: &capacities,
        cases: &cases,
        exposure_limit: 1,
    };
    let selected = solve_funding_family_feasibility(problem, limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.holds(), [0, 1]);
    assert_witness(problem, &selected);
}

#[test]
fn family_search_supports_full_width_amounts_and_large_cohorts() {
    for n in [1, 3, 65, 129] {
        let capacities = vec![u64::MAX; n];
        let lower = vec![0; n];
        let eligible = vec![vec![true]; n];
        let cases = [FundingBoxProblem {
            lower: &lower,
            upper: &capacities,
            obligations: &[u64::MAX],
            eligible: &eligible,
        }];
        let problem = FundingFamilyProblem {
            capacities: &capacities,
            cases: &cases,
            exposure_limit: u128::from(u64::MAX),
        };
        let selected = solve_funding_family_feasibility(problem, limits(), &work())
            .unwrap()
            .unwrap();
        assert_witness(problem, &selected);
    }
}

#[test]
fn family_full_width_conflict_requires_range_search_not_token_iteration() {
    let capacities = [u64::MAX, u64::MAX];
    let flexible = vec![vec![true], vec![true]];
    let only_b = vec![vec![false], vec![true]];
    let only_a = vec![vec![true], vec![false]];
    for first in [&flexible, &only_a] {
        let cases = [
            FundingBoxProblem {
                lower: &[0, 0],
                upper: &capacities,
                obligations: &[u64::MAX],
                eligible: first,
            },
            FundingBoxProblem {
                lower: &[0, 0],
                upper: &capacities,
                obligations: &[u64::MAX],
                eligible: &only_b,
            },
        ];
        let problem = FundingFamilyProblem {
            capacities: &capacities,
            cases: &cases,
            exposure_limit: u128::from(u64::MAX),
        };
        let selected = solve_funding_family_feasibility(problem, limits(), &work()).unwrap();
        assert_eq!(selected.is_some(), first == &flexible);
        if let Some(witness) = selected {
            assert_eq!(witness.holds(), [0, u64::MAX]);
            assert_witness(problem, &witness);
        }
    }
}

#[test]
fn family_feasibility_and_canonical_priority_agree_on_the_approved_alternative() {
    use crate::rust::interpreter::accounting::monetary_allocation::FundingFamilyResourcePriority;

    let capacities = [2, 2, 2, 2, 1];
    let alpha = vec![
        vec![false, false, false],
        vec![false, true, false],
        vec![false, true, true],
        vec![false, true, false],
        vec![true, false, false],
    ];
    let beta = vec![
        vec![false, true, false],
        vec![false, true, false],
        vec![false, false, true],
        vec![false, false, true],
        vec![true, false, false],
    ];
    let alternatives = [[[0, 1, 1, 0, 1], [0, 1, 1, 1, 1]], [[0, 0, 1, 1, 1], [
        1, 0, 1, 1, 1,
    ]]];
    let mut keys = Vec::new();
    for draws in alternatives {
        let cases = [
            FundingBoxProblem {
                lower: &draws[0],
                upper: &draws[0],
                obligations: &[1, 1, 1],
                eligible: &alpha,
            },
            FundingBoxProblem {
                lower: &draws[1],
                upper: &draws[1],
                obligations: &[1, 1, 2],
                eligible: &beta,
            },
        ];
        let problem = FundingFamilyProblem {
            capacities: &capacities,
            cases: &cases,
            exposure_limit: 4,
        };
        let witness = solve_funding_family_feasibility(problem, limits(), &work())
            .unwrap()
            .unwrap();
        assert_witness(problem, &witness);
        let mut resources = draws;
        resources[0][4] -= 1;
        resources[1][4] -= 1;
        keys.push(
            FundingFamilyResourcePriority::from_canonical_resource_draws(
                &[&resources[0], &resources[1]],
                0,
                limits(),
                &work(),
            )
            .unwrap(),
        );
    }
    assert_eq!(
        keys[0].compare(&keys[1], &work()).unwrap(),
        std::cmp::Ordering::Less
    );
}

#[test]
fn family_validation_precedes_infeasibility_and_exhaustion_is_not_rejection() {
    let eligible = vec![vec![true]];
    let valid = FundingBoxProblem {
        lower: &[0],
        upper: &[1],
        obligations: &[1],
        eligible: &eligible,
    };
    let malformed = FundingBoxProblem {
        lower: &[],
        ..valid
    };
    let cases = [valid, malformed];
    let problem = FundingFamilyProblem {
        capacities: &[0],
        cases: &cases,
        exposure_limit: 0,
    };
    assert!(matches!(
        solve_funding_family_feasibility(problem, limits(), &work()),
        Err(FundingFamilyError::Search(
            FundingSearchError::InvalidProblem(FundingAssignmentError::InvalidDimensions)
        ))
    ));
    let cases = [valid];
    let problem = FundingFamilyProblem {
        capacities: &[1],
        cases: &cases,
        exposure_limit: 1,
    };
    let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        solve_funding_family_feasibility(problem, limits(), &budget),
        Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
    ));
    assert_eq!(
        solve_funding_family_feasibility(
            FundingFamilyProblem {
                cases: &[],
                ..problem
            },
            limits(),
            &work()
        ),
        Err(FundingFamilyError::EmptyCases)
    );
    assert_eq!(
        solve_funding_family_feasibility(
            FundingFamilyProblem {
                cases: &[valid, valid],
                ..problem
            },
            FundingFamilyLimits {
                case_cap: NonZeroUsize::new(1).unwrap(),
                ..limits()
            },
            &work()
        ),
        Err(FundingFamilyError::TooManyCases)
    );
}

#[test]
fn parallel_family_searches_have_no_shared_mutable_funding_state() {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(family_search_does_not_reject_a_feasible_family_after_one_bad_witness)
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
    });
}

#[test]
fn family_exhaustive_two_source_two_outcome_domains() {
    for a in 0..=2 {
        for b in 0..=2 {
            for first in 0..=3 {
                for second in 0..=3 {
                    for mask in 0_u8..16 {
                        let alpha = vec![vec![mask & 1 != 0], vec![mask & 2 != 0]];
                        let beta = vec![vec![mask & 4 != 0], vec![mask & 8 != 0]];
                        let capacities = [a, b];
                        let first_demand = [first];
                        let second_demand = [second];
                        let cases = [
                            FundingBoxProblem {
                                lower: &[0, 0],
                                upper: &capacities,
                                obligations: &first_demand,
                                eligible: &alpha,
                            },
                            FundingBoxProblem {
                                lower: &[0, 0],
                                upper: &capacities,
                                obligations: &second_demand,
                                eligible: &beta,
                            },
                        ];
                        for exposure_limit in 0..=4 {
                            let problem = FundingFamilyProblem {
                                capacities: &capacities,
                                cases: &cases,
                                exposure_limit,
                            };
                            let result =
                                solve_funding_family_feasibility(problem, limits(), &work())
                                    .unwrap();
                            assert_eq!(result.is_some(), family_oracle(problem), "capacity={capacities:?}, demands={first},{second}, mask={mask}, exposure={exposure_limit}");
                            if let Some(witness) = result {
                                assert_witness(problem, &witness);
                            }
                        }
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn family_feasibility_matches_complete_small_integer_oracle(
        data in (1_usize..4, 1_usize..4, 0_usize..3).prop_flat_map(|(n, k, m)| (
            Just((n, k, m)),
            prop::collection::vec(0_u64..3, n),
            prop::collection::vec(0_u64..3, k * m),
            prop::collection::vec(any::<bool>(), k * n * m),
            prop::collection::vec((0_u64..3, 0_u64..3), k * n),
            0_u128..10,
        ))
    ) {
        let ((n, k, m), capacities, demands, edges, bounds, exposure_limit) = data;
        let mut lower = Vec::new();
        let mut upper = Vec::new();
        let mut eligible = Vec::new();
        for o in 0..k {
            lower.push((0..n).map(|i| bounds[o*n+i].0.min(bounds[o*n+i].1)).collect::<Vec<_>>());
            upper.push((0..n).map(|i| bounds[o*n+i].0.max(bounds[o*n+i].1)).collect::<Vec<_>>());
            eligible.push((0..n).map(|i| edges[(o*n+i)*m..(o*n+i+1)*m].to_vec()).collect::<Vec<_>>());
        }
        let cases: Vec<_> = (0..k).map(|o| FundingBoxProblem { lower: &lower[o], upper: &upper[o], obligations: &demands[o*m..(o+1)*m], eligible: &eligible[o] }).collect();
        let problem = FundingFamilyProblem { capacities: &capacities, cases: &cases, exposure_limit };
        let result = solve_funding_family_feasibility(problem, limits(), &work()).unwrap();
        prop_assert_eq!(result.is_some(), family_oracle(problem));
        if let Some(witness) = &result { assert_witness(problem, witness); }
        let reversed: Vec<_> = cases.iter().rev().copied().collect();
        let reordered = solve_funding_family_feasibility(FundingFamilyProblem { cases: &reversed, ..problem }, limits(), &work()).unwrap();
        prop_assert_eq!(result.is_some(), reordered.is_some());
        let reversed_capacities: Vec<_> = capacities.iter().rev().copied().collect();
        let reversed_lower: Vec<Vec<_>> = lower.iter().map(|row| row.iter().rev().copied().collect()).collect();
        let reversed_upper: Vec<Vec<_>> = upper.iter().map(|row| row.iter().rev().copied().collect()).collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().map(|rows| rows.iter().rev().cloned().collect()).collect();
        let source_permuted: Vec<_> = (0..k).map(|o| FundingBoxProblem {
            lower: &reversed_lower[o], upper: &reversed_upper[o], obligations: &demands[o*m..(o+1)*m], eligible: &reversed_edges[o],
        }).collect();
        let permuted_problem = FundingFamilyProblem { capacities: &reversed_capacities, cases: &source_permuted, exposure_limit };
        let permuted = solve_funding_family_feasibility(permuted_problem, limits(), &work()).unwrap();
        prop_assert_eq!(result.is_some(), permuted.is_some());
        if let Some(witness) = &permuted { assert_witness(permuted_problem, witness); }
    }
}
