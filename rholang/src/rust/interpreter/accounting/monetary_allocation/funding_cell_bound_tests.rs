use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits(query: FundingCellQuery<'_>) -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(query.exact.capacities.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(query.exact.obligations.len().max(1)).unwrap(),
    }
}

fn oracle(query: FundingCellQuery<'_>, rows: &mut [u64], columns: &mut [u64], cell: u64) -> bool {
    let Some(j) = columns.iter().position(|amount| *amount > 0) else {
        return rows.iter().all(|amount| *amount == 0);
    };
    columns[j] -= 1;
    for i in 0..rows.len() {
        let bounded = i == query.source && j == query.obligation;
        if rows[i] == 0 || !query.exact.eligible[i][j] || (bounded && cell == query.maximum) {
            continue;
        }
        rows[i] -= 1;
        let found = oracle(query, rows, columns, cell + u64::from(bounded));
        rows[i] += 1;
        if found {
            columns[j] += 1;
            return true;
        }
    }
    columns[j] += 1;
    false
}

fn validate(query: FundingCellQuery<'_>, result: FundingFeasibility) -> bool {
    match result {
        FundingFeasibility::Feasible { assignment, totals } => {
            assert_eq!(assignment.len(), query.exact.capacities.len());
            assert_eq!(totals.source_debits(), query.exact.capacities);
            assert_eq!(
                query
                    .exact
                    .check_assignment(&assignment, limits(query), &work()),
                Ok(totals)
            );
            assert!(assignment[query.source][query.obligation] <= query.maximum);
            true
        }
        FundingFeasibility::Infeasible {
            selected_obligations,
        } => {
            assert!(check_funding_cell_deficit(
                query,
                &selected_obligations,
                limits(query),
                &work()
            )
            .unwrap());
            false
        }
    }
}

#[test]
fn entry_bounds_preserve_wallet_totals_without_forcing_exact_entry_amounts() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 2],
        obligations: &[2, 2],
        eligible: &[vec![true, true], vec![true, true]],
    };
    for source in 0..2 {
        for obligation in 0..2 {
            for maximum in [0, 1, 2, u64::MAX] {
                let query = FundingCellQuery {
                    exact: problem,
                    source,
                    obligation,
                    maximum,
                };
                assert!(validate(
                    query,
                    solve_funding_cell_bound(query, limits(query), &work()).unwrap()
                ));
            }
        }
    }
}

#[test]
fn a_deficit_is_bound_to_the_exact_entry_and_does_not_relax_eligibility() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 2],
        obligations: &[2, 2],
        eligible: &[vec![true, false], vec![false, true]],
    };
    let query = FundingCellQuery {
        exact: problem,
        source: 0,
        obligation: 0,
        maximum: 1,
    };
    let FundingFeasibility::Infeasible {
        selected_obligations,
    } = solve_funding_cell_bound(query, limits(query), &work()).unwrap()
    else {
        panic!("the diagonal entry requires two units");
    };
    assert!(
        check_funding_cell_deficit(query, &selected_obligations, limits(query), &work()).unwrap()
    );
    for alternative in [FundingCellQuery { source: 1, ..query }, FundingCellQuery {
        maximum: 2,
        ..query
    }] {
        assert!(!check_funding_cell_deficit(
            alternative,
            &selected_obligations,
            limits(alternative),
            &work()
        )
        .unwrap());
        assert!(validate(
            alternative,
            solve_funding_cell_bound(alternative, limits(alternative), &work()).unwrap()
        ));
    }
    assert_eq!(
        check_funding_cell_deficit(query, &[], limits(query), &work()),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
}

#[test]
fn exhaustive_balanced_graphs_match_the_complete_entry_bound_oracle() {
    let mut queries = 0;
    for mask in 0..16 {
        let eligible = [vec![mask & 1 != 0, mask & 2 != 0], vec![
            mask & 4 != 0,
            mask & 8 != 0,
        ]];
        for c0 in 0..=2 {
            for c1 in 0..=2 {
                for q0 in 0..=2 {
                    for q1 in 0..=2 {
                        if c0 + c1 != q0 + q1 {
                            continue;
                        }
                        let problem = FundingMinimaxProblem {
                            capacities: &[c0, c1],
                            obligations: &[q0, q1],
                            eligible: &eligible,
                        };
                        for source in 0..2 {
                            for obligation in 0..2 {
                                for maximum in 0..=2 {
                                    let query = FundingCellQuery {
                                        exact: problem,
                                        source,
                                        obligation,
                                        maximum,
                                    };
                                    let expected = oracle(
                                        query,
                                        &mut problem.capacities.to_vec(),
                                        &mut problem.obligations.to_vec(),
                                        0,
                                    );
                                    let actual =
                                        solve_funding_cell_bound(query, limits(query), &work())
                                            .unwrap();
                                    assert_eq!(validate(query, actual), expected, "{query:?}");
                                    queries += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(queries, 3648);
}

#[test]
fn wide_totals_and_arbitrary_cohorts_do_not_create_an_extra_wallet() {
    for count in [1, 2, 3, 64, 65, 129] {
        let mut capacity = vec![0; count];
        capacity[count - 1] = u64::MAX;
        let eligible = vec![vec![true; 2]; count];
        let problem = FundingMinimaxProblem {
            capacities: &capacity,
            obligations: &[u64::MAX - 1, 1],
            eligible: &eligible,
        };
        for maximum in [u64::MAX - 2, u64::MAX - 1, u64::MAX] {
            let query = FundingCellQuery {
                exact: problem,
                source: count - 1,
                obligation: 0,
                maximum,
            };
            let result = solve_funding_cell_bound(query, limits(query), &work()).unwrap();
            assert_eq!(validate(query, result), maximum >= u64::MAX - 1);
        }
    }
}

#[test]
fn malformed_inputs_and_nonexact_contributions_are_errors_not_deficits() {
    let problem = FundingMinimaxProblem {
        capacities: &[1],
        obligations: &[1],
        eligible: &[vec![true]],
    };
    let query = FundingCellQuery {
        exact: problem,
        source: 0,
        obligation: 0,
        maximum: 0,
    };
    for altered in [FundingCellQuery { source: 1, ..query }, FundingCellQuery {
        obligation: usize::MAX,
        ..query
    }] {
        assert_eq!(
            solve_funding_cell_bound(altered, limits(altered), &work()),
            Err(FundingSearchError::InvalidAssignmentCell)
        );
    }
    let wrong = FundingCellQuery {
        exact: FundingMinimaxProblem {
            capacities: &[2],
            ..problem
        },
        ..query
    };
    assert_eq!(
        solve_funding_cell_bound(wrong, limits(wrong), &work()),
        Err(FundingSearchError::UnequalFundingTotals)
    );
    let wrong = FundingCellQuery {
        exact: FundingMinimaxProblem {
            eligible: &[vec![]],
            ..problem
        },
        ..query
    };
    assert_eq!(
        solve_funding_cell_bound(wrong, limits(wrong), &work()),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    let wrong = FundingCellQuery {
        exact: FundingMinimaxProblem {
            capacities: &[u64::MAX, 1],
            obligations: &[u64::MAX],
            eligible: &[vec![true], vec![true]],
        },
        ..query
    };
    assert_eq!(
        solve_funding_cell_bound(wrong, limits(wrong), &work()),
        Err(FundingAssignmentError::Overflow.into())
    );
}

#[test]
fn interrupted_queries_never_return_partial_assignments_or_false_deficits() {
    let problem = FundingMinimaxProblem {
        capacities: &[1],
        obligations: &[1],
        eligible: &[vec![true]],
    };
    for maximum in [0, 1] {
        let query = FundingCellQuery {
            exact: problem,
            source: 0,
            obligation: 0,
            maximum,
        };
        let measured = work();
        let expected = solve_funding_cell_bound(query, limits(query), &measured).unwrap();
        for dimension in [
            HostWorkDimension::SearchCandidates,
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationOperations,
        ] {
            let full = measured.usage(dimension).get();
            for prefix in 0..=full {
                let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
                bounds.set(dimension, HostWorkLimit::new(prefix));
                let actual =
                    solve_funding_cell_bound(query, limits(query), &HostWorkBudget::new(bounds));
                if prefix == full {
                    assert_eq!(actual, Ok(expected.clone()));
                } else {
                    assert!(
                        matches!(actual, Err(FundingSearchError::HostWork(_))),
                        "{maximum} {dimension:?} {prefix}/{full}: {actual:?}"
                    );
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_exact_graphs_and_relabelings_match_the_assignment_oracle(
        sources in 1_usize..4, obligations in 1_usize..3,
        entries in prop::collection::vec(0_u64..2, 6),
        edges in prop::collection::vec(any::<bool>(), 6),
        source_index in any::<usize>(), obligation_index in any::<usize>(), maximum in 0_u64..5,
    ) {
        let eligible: Vec<Vec<_>> = (0..sources).map(|i| (0..obligations).map(|j| edges[i * 2 + j]).collect()).collect();
        let assignment: Vec<Vec<_>> = (0..sources).map(|i| (0..obligations).map(|j| if eligible[i][j] { entries[i * 2 + j] } else { 0 }).collect()).collect();
        let capacities: Vec<u64> = assignment.iter().map(|row| row.iter().sum()).collect();
        let demand: Vec<u64> = (0..obligations).map(|j| assignment.iter().map(|row| row[j]).sum()).collect();
        let problem = FundingMinimaxProblem { capacities: &capacities, obligations: &demand, eligible: &eligible };
        let query = FundingCellQuery { exact: problem, source: source_index % sources, obligation: obligation_index % obligations, maximum };
        let expected = oracle(query, &mut capacities.clone(), &mut demand.clone(), 0);
        let result = solve_funding_cell_bound(query, limits(query), &work()).unwrap();
        prop_assert_eq!(validate(query, result), expected);
        let reversed_capacity: Vec<_> = capacities.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        let reversed = FundingCellQuery {
            exact: FundingMinimaxProblem { capacities: &reversed_capacity, obligations: &reversed_demand, eligible: &reversed_edges },
            source: sources - 1 - query.source, obligation: obligations - 1 - query.obligation, maximum,
        };
        let result = solve_funding_cell_bound(reversed, limits(reversed), &work()).unwrap();
        prop_assert_eq!(validate(reversed, result), expected);
    }
}
