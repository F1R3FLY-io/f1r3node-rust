use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::check_funding_deficit;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits(problem: FundingMinimaxProblem<'_>) -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(problem.capacities.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(problem.obligations.len().max(1)).unwrap(),
    }
}

fn enumerate(
    problem: FundingMinimaxProblem<'_>,
    cell: usize,
    rows: &mut [u64],
    columns: &mut [u64],
    matrix: &mut [Vec<u64>],
    result: &mut Vec<Vec<Vec<u64>>>,
) {
    let width = columns.len();
    if cell == rows.len() * width {
        if rows.iter().chain(columns.iter()).all(|x| *x == 0) {
            result.push(matrix.to_vec());
        }
        return;
    }
    let i = cell / width;
    let j = cell % width;
    let maximum = if problem.eligible[i][j] {
        rows[i].min(columns[j])
    } else {
        0
    };
    for amount in 0..=maximum {
        matrix[i][j] = amount;
        rows[i] -= amount;
        columns[j] -= amount;
        enumerate(problem, cell + 1, rows, columns, matrix, result);
        rows[i] += amount;
        columns[j] += amount;
    }
    matrix[i][j] = 0;
}

fn oracle(problem: FundingMinimaxProblem<'_>) -> Vec<Vec<Vec<u64>>> {
    let mut result = Vec::new();
    enumerate(
        problem,
        0,
        &mut problem.capacities.to_vec(),
        &mut problem.obligations.to_vec(),
        &mut vec![vec![0; problem.obligations.len()]; problem.capacities.len()],
        &mut result,
    );
    result
}

fn assert_matches_oracle(problem: FundingMinimaxProblem<'_>) {
    let assignments = oracle(problem);
    let result = select_fixed_funding_witness(problem, limits(problem), &work()).unwrap();
    match result {
        FundingWitnessResult::Canonical {
            assignment,
            totals,
            certificate,
        } => {
            assert_eq!(Some(&assignment), assignments.iter().min(), "{problem:?}");
            assert_eq!(totals.source_debits(), problem.capacities);
            assert_eq!(
                problem.check_assignment(&assignment, limits(problem), &work()),
                Ok(totals)
            );
            for alternative in &assignments {
                assert_eq!(
                    check_fixed_funding_witness(
                        problem,
                        alternative,
                        certificate.entries(),
                        limits(problem),
                        &work()
                    )
                    .unwrap(),
                    alternative == &assignment
                );
            }
            let mut residual = WitnessResidual::new(problem, &work()).unwrap();
            let mut remainder = assignment.clone();
            for i in 0..assignment.len() {
                for j in 0..assignment[i].len() {
                    residual.freeze(i, j, assignment[i][j]).unwrap();
                    remainder[i][j] = 0;
                    let totals = residual
                        .problem()
                        .check_assignment(&remainder, limits(problem), &work())
                        .unwrap();
                    assert_eq!(totals.source_debits(), residual.contributions);
                    assert_eq!(totals.total(), residual.obligations.iter().sum::<u64>());
                    assert!(!residual.eligible[i][j]);
                }
            }
        }
        FundingWitnessResult::Infeasible {
            selected_obligations,
        } => {
            assert!(assignments.is_empty());
            assert!(check_funding_deficit(
                problem.capacities,
                problem.obligations,
                problem.eligible,
                &selected_obligations,
                limits(problem).source_cap,
                limits(problem).obligation_cap
            )
            .unwrap());
        }
    }
}

#[test]
fn entry_selection_changes_the_witness_without_changing_wallet_totals() {
    let problem = FundingMinimaxProblem {
        capacities: &[1, 1],
        obligations: &[1, 1],
        eligible: &[vec![true; 2], vec![true; 2]],
    };
    let FundingWitnessResult::Canonical { assignment, .. } =
        select_fixed_funding_witness(problem, limits(problem), &work()).unwrap()
    else {
        panic!("the complete graph is feasible");
    };
    assert_eq!(assignment, vec![vec![0, 1], vec![1, 0]]);
    assert_matches_oracle(problem);
}

#[test]
fn certificate_cannot_freeze_future_entries_to_justify_a_nonminimum_prefix() {
    let problem = FundingMinimaxProblem {
        capacities: &[1, 1],
        obligations: &[1, 1],
        eligible: &[vec![true; 2], vec![true; 2]],
    };
    let false_future = FundingMinimaxProblem {
        eligible: &[vec![true, false], vec![false, true]],
        ..problem
    };
    let query = FundingCellQuery {
        exact: false_future,
        source: 0,
        obligation: 0,
        maximum: 0,
    };
    let FundingFeasibility::Infeasible {
        selected_obligations,
    } = solve_funding_cell_bound(query, limits(problem), &work()).unwrap()
    else {
        panic!("freezing future entries can fabricate this deficit");
    };
    let forged = [
        FundingEntryMinimum::Deficit {
            selected_obligations,
        },
        FundingEntryMinimum::Zero,
        FundingEntryMinimum::Zero,
        FundingEntryMinimum::Deficit {
            selected_obligations: vec![false, true],
        },
    ];
    assert!(!check_fixed_funding_witness(
        problem,
        &[vec![1, 0], vec![0, 1]],
        &forged,
        limits(problem),
        &work()
    )
    .unwrap());
}

#[test]
fn missing_extra_and_false_entry_certificates_are_rejected() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 1],
        obligations: &[1, 2],
        eligible: &[vec![true; 2], vec![true; 2]],
    };
    let FundingWitnessResult::Canonical {
        assignment,
        certificate,
        ..
    } = select_fixed_funding_witness(problem, limits(problem), &work()).unwrap()
    else {
        panic!("the complete graph is feasible");
    };
    for i in 0..certificate.entries().len() {
        let mut altered = certificate.entries().to_vec();
        altered.remove(i);
        assert!(!check_fixed_funding_witness(
            problem,
            &assignment,
            &altered,
            limits(problem),
            &work()
        )
        .unwrap());
        let mut altered = certificate.entries().to_vec();
        altered[i] = match &altered[i] {
            FundingEntryMinimum::Zero => FundingEntryMinimum::Deficit {
                selected_obligations: vec![true; 2],
            },
            FundingEntryMinimum::Deficit { .. } => FundingEntryMinimum::Zero,
        };
        assert!(!check_fixed_funding_witness(
            problem,
            &assignment,
            &altered,
            limits(problem),
            &work()
        )
        .unwrap());
    }
    let mut extra = certificate.entries().to_vec();
    extra.push(FundingEntryMinimum::Zero);
    assert!(
        !check_fixed_funding_witness(problem, &assignment, &extra, limits(problem), &work())
            .unwrap()
    );
}

#[test]
fn exhaustive_balanced_graphs_select_the_global_row_major_minimum() {
    let mut cases = 0;
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
                        assert_matches_oracle(FundingMinimaxProblem {
                            capacities: &[c0, c1],
                            obligations: &[q0, q1],
                            eligible: &eligible,
                        });
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 304);
}

#[test]
fn empty_obligations_and_large_cohorts_retain_physical_source_positions() {
    for count in [1, 2, 3, 64, 65, 129] {
        let capacity = vec![0; count];
        let eligible = vec![vec![]; count];
        assert_matches_oracle(FundingMinimaxProblem {
            capacities: &capacity,
            obligations: &[],
            eligible: &eligible,
        });
        let mut capacity = capacity;
        capacity[count - 1] = u64::MAX;
        let eligible = vec![vec![true; 2]; count];
        let problem = FundingMinimaxProblem {
            capacities: &capacity,
            obligations: &[u64::MAX - 1, 1],
            eligible: &eligible,
        };
        let FundingWitnessResult::Canonical {
            assignment,
            totals,
            certificate,
        } = select_fixed_funding_witness(problem, limits(problem), &work()).unwrap()
        else {
            panic!("the final physical source covers both obligations");
        };
        assert_eq!(assignment.len(), count);
        assert_eq!(assignment[count - 1], vec![u64::MAX - 1, 1]);
        assert_eq!(totals.source_debits(), capacity);
        assert!(assignment[..count - 1]
            .iter()
            .flatten()
            .all(|value| *value == 0));
        assert!(check_fixed_funding_witness(
            problem,
            &assignment,
            certificate.entries(),
            limits(problem),
            &work()
        )
        .unwrap());
    }
}

#[test]
fn exact_contribution_validation_precedes_economic_results() {
    let problem = FundingMinimaxProblem {
        capacities: &[2],
        obligations: &[1],
        eligible: &[vec![false]],
    };
    assert_eq!(
        select_fixed_funding_witness(problem, limits(problem), &work()),
        Err(FundingSearchError::UnequalFundingTotals)
    );
    let malformed = FundingMinimaxProblem {
        eligible: &[vec![]],
        ..problem
    };
    assert_eq!(
        select_fixed_funding_witness(malformed, limits(malformed), &work()),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    let empty = FundingMinimaxProblem {
        capacities: &[],
        obligations: &[],
        eligible: &[],
    };
    assert_eq!(
        select_fixed_funding_witness(empty, limits(empty), &work()),
        Err(FundingAssignmentError::EmptySources.into())
    );
}

#[test]
fn every_budget_prefix_preserves_the_original_inputs_and_rejects_partial_results() {
    let problem = FundingMinimaxProblem {
        capacities: &[1],
        obligations: &[1],
        eligible: &[vec![true]],
    };
    let measured = work();
    let expected = select_fixed_funding_witness(problem, limits(problem), &measured).unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let full = measured.usage(dimension).get();
        for prefix in 0..=full {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            bounds.set(dimension, HostWorkLimit::new(prefix));
            let actual = select_fixed_funding_witness(
                problem,
                limits(problem),
                &HostWorkBudget::new(bounds),
            );
            if prefix == full {
                assert_eq!(actual, Ok(expected.clone()));
            } else {
                assert!(
                    matches!(actual, Err(FundingSearchError::HostWork(_))),
                    "{dimension:?} {prefix}/{full}: {actual:?}"
                );
            }
            assert_eq!(problem.capacities, &[1]);
            assert_eq!(problem.obligations, &[1]);
            assert_eq!(problem.eligible, &[vec![true]]);
        }
    }
}

#[test]
fn parallel_witness_selection_has_no_shared_assignment_state() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 1],
        obligations: &[1, 2],
        eligible: &[vec![true; 2], vec![true; 2]],
    };
    let expected = select_fixed_funding_witness(problem, limits(problem), &work()).unwrap();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|_| {
                scope.spawn(|| {
                    select_fixed_funding_witness(problem, limits(problem), &work()).unwrap()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), expected);
        }
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_graphs_match_complete_oracles_in_each_source_and_obligation_order(
        sources in 1_usize..4, obligations in 1_usize..3,
        entries in prop::collection::vec(0_u64..2, 6), edges in prop::collection::vec(any::<bool>(), 6),
    ) {
        let eligible: Vec<Vec<_>> = (0..sources).map(|i| (0..obligations).map(|j| edges[i * 2 + j]).collect()).collect();
        let assignment: Vec<Vec<_>> = (0..sources).map(|i| (0..obligations).map(|j| if eligible[i][j] { entries[i * 2 + j] } else { 0 }).collect()).collect();
        let capacities: Vec<u64> = assignment.iter().map(|row| row.iter().sum()).collect();
        let demand: Vec<u64> = (0..obligations).map(|j| assignment.iter().map(|row| row[j]).sum()).collect();
        assert_matches_oracle(FundingMinimaxProblem { capacities: &capacities, obligations: &demand, eligible: &eligible });
        let reversed_capacity: Vec<_> = capacities.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        assert_matches_oracle(FundingMinimaxProblem { capacities: &reversed_capacity, obligations: &reversed_demand, eligible: &reversed_edges });
    }
}
