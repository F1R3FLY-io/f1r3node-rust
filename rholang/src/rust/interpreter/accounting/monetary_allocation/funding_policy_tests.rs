use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::super::{
    check_fixed_funding_witness, check_funding_deficit, check_funding_domain_counterexample,
};
use super::*;

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
        if columns.iter().all(|x| *x == 0) {
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

fn contributions(matrix: &[Vec<u64>]) -> Vec<u64> {
    matrix.iter().map(|row| row.iter().sum()).collect()
}

fn rank(matrix: &[Vec<u64>]) -> Vec<u64> {
    let mut values = contributions(matrix);
    values.sort_unstable_by(|a, b| b.cmp(a));
    values
}

fn cyclic(matrix: &[Vec<u64>], cursor: usize) -> Vec<u64> {
    let mut values = contributions(matrix);
    values.rotate_left(cursor);
    values
}

fn assert_oracle(problem: FundingMinimaxProblem<'_>) {
    let all = oracle(problem);
    let full_edges = vec![vec![true; problem.obligations.len()]; problem.capacities.len()];
    let full = oracle(FundingMinimaxProblem {
        eligible: &full_edges,
        ..problem
    });
    let actual_domain: BTreeSet<_> = all.iter().map(|x| contributions(x)).collect();
    let full_domain: BTreeSet<_> = full.iter().map(|x| contributions(x)).collect();
    for cursor in 0..problem.capacities.len() {
        let result =
            select_fixed_funding_policy(problem, cursor, limits(problem), &work()).unwrap();
        let expected = all.iter().min_by(|left, right| {
            rank(left)
                .cmp(&rank(right))
                .then_with(|| cyclic(right, cursor).cmp(&cyclic(left, cursor)))
                .then_with(|| left.cmp(right))
        });
        match result {
            FundingPolicyResult::Infeasible {
                selected_obligations,
            } => {
                assert!(expected.is_none(), "{problem:?}");
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
            FundingPolicyResult::Selected(selection) => {
                assert_eq!(
                    Some(selection.assignment()),
                    expected.map(Vec::as_slice),
                    "{problem:?}, cursor {cursor}"
                );
                assert_eq!(
                    problem.check_assignment(selection.assignment(), limits(problem), &work()),
                    Ok(selection.totals().clone())
                );
                assert!(check_funding_minimax_certificate(
                    problem,
                    selection.assignment(),
                    selection.rank_certificate().cuts(),
                    limits(problem),
                    &work()
                )
                .unwrap());
                assert!(check_fixed_funding_witness(
                    FundingMinimaxProblem {
                        capacities: selection.totals().source_debits(),
                        ..problem
                    },
                    selection.assignment(),
                    selection.witness_certificate().entries(),
                    limits(problem),
                    &work()
                )
                .unwrap());
                let total: u64 = problem.obligations.iter().sum();
                match selection.fragment() {
                    FundingPolicyFragment::Unrestricted => {
                        assert_eq!(actual_domain, full_domain);
                        let legacy = allocate_capped_max_min(
                            problem.capacities,
                            total,
                            cursor,
                            limits(problem).source_cap,
                        )
                        .unwrap();
                        assert_eq!(selection.totals().source_debits(), legacy.debits);
                        assert_eq!(
                            selection.next_cursor(),
                            (total != 0).then_some(legacy.next_cursor)
                        );
                    }
                    FundingPolicyFragment::Restricted {
                        counterexample,
                        contribution_certificate,
                    } => {
                        assert_ne!(actual_domain, full_domain);
                        assert!(check_funding_domain_counterexample(
                            problem.capacities,
                            problem.obligations,
                            problem.eligible,
                            counterexample.view(),
                            limits(problem),
                            &work()
                        )
                        .unwrap());
                        let domain = derive_funding_optimal_domain(
                            problem,
                            selection.assignment(),
                            selection.rank_certificate().cuts(),
                            limits(problem),
                            &work(),
                        )
                        .unwrap();
                        assert!(check_funding_cyclic_tie_certificate(
                            &domain,
                            selection.assignment(),
                            cursor,
                            contribution_certificate.exclusions(),
                            limits(problem),
                            &work()
                        )
                        .unwrap());
                        assert_eq!(
                            selection.next_cursor(),
                            (total != 0).then_some((cursor + 1) % problem.capacities.len())
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn exhaustive_graphs_match_complete_three_objective_and_dispatch_oracle() {
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
                        assert_oracle(FundingMinimaxProblem {
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
    assert_eq!(cases, 1296);
}

#[test]
fn redundant_missing_edges_do_not_select_restricted_cursor_policy() {
    let problem = FundingMinimaxProblem {
        capacities: &[1, 1],
        obligations: &[1, 1],
        eligible: &[vec![true, false], vec![false, true]],
    };
    assert_oracle(problem);
    let FundingPolicyResult::Selected(selection) =
        select_fixed_funding_policy(problem, 0, limits(problem), &work()).unwrap()
    else {
        panic!("the exact diagonal assignment is feasible");
    };
    assert_eq!(selection.fragment(), &FundingPolicyFragment::Unrestricted);
    assert_eq!(selection.next_cursor(), Some(0));
}

#[test]
fn uniquely_feasible_restricted_contributions_still_advance_priority() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 10],
        obligations: &[2, 1],
        eligible: &[vec![true, true], vec![false, true]],
    };
    assert_oracle(problem);
    for cursor in 0..2 {
        let FundingPolicyResult::Selected(selection) =
            select_fixed_funding_policy(problem, cursor, limits(problem), &work()).unwrap()
        else {
            panic!("restricted and flexible obligations fit jointly");
        };
        assert_eq!(selection.totals().source_debits(), &[2, 1]);
        assert!(matches!(
            selection.fragment(),
            FundingPolicyFragment::Restricted { .. }
        ));
        assert_eq!(selection.next_cursor(), Some(1 - cursor));
    }
}

#[test]
fn zero_funding_has_no_update_and_full_width_preserves_all_source_positions() {
    for count in [1, 2, 3, 64, 65, 129] {
        let mut capacity = vec![0; count];
        let edges = vec![vec![true]; count];
        capacity[count - 1] = u64::MAX;
        for total in [0, u64::MAX] {
            let problem = FundingMinimaxProblem {
                capacities: &capacity,
                obligations: &[total],
                eligible: &edges,
            };
            let FundingPolicyResult::Selected(selection) =
                select_fixed_funding_policy(problem, count - 1, limits(problem), &work()).unwrap()
            else {
                panic!("the final source has sufficient capacity");
            };
            assert_eq!(selection.assignment().len(), count);
            assert_eq!(selection.totals().total(), total);
            assert_eq!(selection.totals().source_debits()[count - 1], total);
            assert_eq!(selection.next_cursor(), (total != 0).then_some(count - 1));
        }
    }
    assert_oracle(FundingMinimaxProblem {
        capacities: &[1, 2, 3],
        obligations: &[],
        eligible: &[vec![], vec![], vec![]],
    });
}

#[test]
fn malformed_input_and_invalid_cursor_are_not_economic_infeasibility() {
    let problem = FundingMinimaxProblem {
        capacities: &[1],
        obligations: &[2],
        eligible: &[vec![true]],
    };
    assert_eq!(
        select_fixed_funding_policy(problem, 1, limits(problem), &work()),
        Err(FundingSearchError::InvalidPriorityCursor)
    );
    assert_eq!(
        select_fixed_funding_policy(
            FundingMinimaxProblem {
                eligible: &[vec![]],
                ..problem
            },
            0,
            limits(problem),
            &work()
        ),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    let overflow = FundingMinimaxProblem {
        capacities: &[u64::MAX],
        obligations: &[u64::MAX, 1],
        eligible: &[vec![true; 2]],
    };
    assert!(matches!(
        select_fixed_funding_policy(overflow, 0, limits(overflow), &work()),
        Err(_)
    ));
}

#[test]
fn every_budget_prefix_rejects_partial_policy_results() {
    let problem = FundingMinimaxProblem {
        capacities: &[1],
        obligations: &[1],
        eligible: &[vec![true]],
    };
    let measured = work();
    let expected = select_fixed_funding_policy(problem, 0, limits(problem), &measured).unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let full = measured.usage(dimension).get();
        for prefix in 0..=full {
            let mut bound = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            bound.set(dimension, HostWorkLimit::new(prefix));
            let actual = select_fixed_funding_policy(
                problem,
                0,
                limits(problem),
                &HostWorkBudget::new(bound),
            );
            if prefix == full {
                assert_eq!(actual, Ok(expected.clone()));
            } else {
                assert!(
                    matches!(actual, Err(FundingSearchError::HostWork(_))),
                    "{dimension:?} {prefix}/{full}: {actual:?}"
                );
            }
        }
    }
}

#[test]
fn concurrent_policy_calls_preserve_independent_cursors_and_inputs() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 2, 2],
        obligations: &[1, 2],
        eligible: &[vec![true, false], vec![true, true], vec![false, true]],
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|cursor| {
                scope.spawn(move || {
                    select_fixed_funding_policy(problem, cursor, limits(problem), &work()).unwrap()
                })
            })
            .collect();
        for (cursor, handle) in handles.into_iter().enumerate() {
            assert_eq!(
                handle.join().unwrap(),
                select_fixed_funding_policy(problem, cursor, limits(problem), &work()).unwrap()
            );
        }
    });
    assert_oracle(problem);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn generated_funding_policies_match_complete_oracles(
        sources in 1_usize..4, obligations in 1_usize..3,
        capacities in prop::collection::vec(0_u64..3, 3), amounts in prop::collection::vec(0_u64..3, 2),
        edges in prop::collection::vec(any::<bool>(), 6),
    ) {
        let eligible: Vec<Vec<_>> = (0..sources).map(|i| (0..obligations).map(|j| edges[i * 2 + j]).collect()).collect();
        assert_oracle(FundingMinimaxProblem { capacities: &capacities[..sources], obligations: &amounts[..obligations], eligible: &eligible });
        let reversed_capacity: Vec<_> = capacities[..sources].iter().rev().copied().collect();
        let reversed_demand: Vec<_> = amounts[..obligations].iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        assert_oracle(FundingMinimaxProblem { capacities: &reversed_capacity, obligations: &reversed_demand, eligible: &reversed_edges });
    }
}
