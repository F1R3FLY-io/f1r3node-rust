use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

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

fn enumerate(problem: FundingMinimaxProblem<'_>) -> Vec<Vec<Vec<u64>>> {
    fn visit(
        problem: FundingMinimaxProblem<'_>,
        cell: usize,
        available: &mut [u64],
        demand: &mut [u64],
        assignment: &mut [Vec<u64>],
        result: &mut Vec<Vec<Vec<u64>>>,
    ) {
        if cell == available.len() * demand.len() {
            if demand.iter().all(|x| *x == 0) {
                result.push(assignment.to_vec());
            }
            return;
        }
        let i = cell / demand.len();
        let j = cell % demand.len();
        let bound = if problem.eligible[i][j] {
            available[i].min(demand[j])
        } else {
            0
        };
        for amount in 0..=bound {
            available[i] -= amount;
            demand[j] -= amount;
            assignment[i][j] = amount;
            visit(problem, cell + 1, available, demand, assignment, result);
            available[i] += amount;
            demand[j] += amount;
        }
        assignment[i][j] = 0;
    }
    let mut result = Vec::new();
    visit(
        problem,
        0,
        &mut problem.capacities.to_vec(),
        &mut problem.obligations.to_vec(),
        &mut vec![vec![0; problem.obligations.len()]; problem.capacities.len()],
        &mut result,
    );
    result
}

fn draws(matrix: &[Vec<u64>]) -> Vec<u64> { matrix.iter().map(|row| row.iter().sum()).collect() }

fn order(left: &[Vec<u64>], right: &[Vec<u64>], cursor: usize) -> Ordering {
    let mut lhs = draws(left);
    let mut rhs = draws(right);
    lhs.sort_unstable_by(|a, b| b.cmp(a));
    rhs.sort_unstable_by(|a, b| b.cmp(a));
    let mut left_cycle = draws(left);
    let mut right_cycle = draws(right);
    left_cycle.rotate_left(cursor);
    right_cycle.rotate_left(cursor);
    lhs.cmp(&rhs)
        .then_with(|| right_cycle.cmp(&left_cycle))
        .then_with(|| left.cmp(right))
}

fn assert_oracle(problem: FundingMinimaxProblem<'_>, fee_eligible: &[bool]) {
    let count = problem.capacities.len();
    let mut complete = enumerate(problem);
    complete.retain(|matrix| {
        draws(matrix)
            .iter()
            .enumerate()
            .any(|(i, draw)| fee_eligible[i] && *draw < problem.capacities[i])
    });
    let all_edges = vec![vec![true; problem.obligations.len()]; count];
    let full = enumerate(FundingMinimaxProblem {
        eligible: &all_edges,
        ..problem
    });
    let full_domain: BTreeSet<_> = full.iter().map(|matrix| draws(matrix)).collect();
    let actual_domain: BTreeSet<_> = complete.iter().map(|matrix| draws(matrix)).collect();
    for resource_cursor in 0..count {
        for fee_cursor in 0..count {
            let selected = select_funding_with_unit_fee(
                problem,
                fee_eligible,
                resource_cursor,
                fee_cursor,
                limits(problem),
                &work(),
            )
            .unwrap();
            let expected = complete.iter().min_by(|a, b| order(a, b, resource_cursor));
            let Some(expected) = expected else {
                assert!(selected.is_none());
                continue;
            };
            let selected = selected.unwrap();
            assert_eq!(
                selected.resource_assignment(),
                expected,
                "{problem:?}, fee={fee_eligible:?}"
            );
            let contributions = draws(expected);
            assert_eq!(selected.resource_totals().source_debits(), contributions);
            let unrestricted = actual_domain == full_domain;
            assert_eq!(
                selected.resource_unrestricted(),
                unrestricted,
                "{problem:?}, fee={fee_eligible:?}"
            );
            let fee_payer = (0..count)
                .map(|offset| (fee_cursor + offset) % count)
                .find(|i| fee_eligible[*i] && contributions[*i] < problem.capacities[*i])
                .unwrap();
            assert_eq!(
                selected.fee().debits,
                (0..count)
                    .map(|i| u64::from(i == fee_payer))
                    .collect::<Vec<_>>()
            );
            let fee_options = (0..count)
                .filter(|i| fee_eligible[*i] && contributions[*i] < problem.capacities[*i])
                .count();
            assert_eq!(
                selected.fee().next_cursor,
                if fee_options == 1 {
                    fee_cursor
                } else {
                    (fee_payer + 1) % count
                }
            );
            let total: u64 = problem.obligations.iter().sum();
            let expected_next = if total == 0 {
                None
            } else if !unrestricted {
                Some((resource_cursor + 1) % count)
            } else {
                let level = (0..=total)
                    .filter(|level| {
                        problem
                            .capacities
                            .iter()
                            .map(|cap| (*cap).min(*level))
                            .sum::<u64>()
                            <= total
                    })
                    .max()
                    .unwrap();
                let last_extra = (0..count)
                    .map(|offset| (resource_cursor + offset) % count)
                    .rfind(|i| contributions[*i] > problem.capacities[*i].min(level));
                Some(last_extra.map_or(resource_cursor, |i| (i + 1) % count))
            };
            assert_eq!(selected.resource_next_cursor(), expected_next);
        }
    }
}

#[test]
fn joint_selection_repairs_both_sequential_failures_and_keeps_resource_fairness() {
    assert_oracle(
        FundingMinimaxProblem {
            capacities: &[1, 1],
            obligations: &[1],
            eligible: &[vec![true], vec![true]],
        },
        &[true, false],
    );
    assert_oracle(
        FundingMinimaxProblem {
            capacities: &[1, 1],
            obligations: &[1],
            eligible: &[vec![true], vec![false]],
        },
        &[true, true],
    );
    assert_oracle(
        FundingMinimaxProblem {
            capacities: &[3, 3],
            obligations: &[2],
            eligible: &[vec![true], vec![true]],
        },
        &[true, true],
    );
    assert_oracle(
        FundingMinimaxProblem {
            capacities: &[1, 2, 1],
            obligations: &[1, 1],
            eligible: &[vec![true, false], vec![true, true], vec![false, true]],
        },
        &[true, false, true],
    );
}

#[test]
fn independently_optimal_case_ties_do_not_define_a_joint_family_policy() {
    let capacities = [2, 2, 2, 2, 1];
    let fee = [false, false, false, false, true];
    let first_edges = [
        vec![false, false],
        vec![true, false],
        vec![true, true],
        vec![true, false],
        vec![false, false],
    ];
    let second_edges = [
        vec![true, false],
        vec![true, false],
        vec![false, true],
        vec![false, true],
        vec![false, false],
    ];
    let first = FundingMinimaxProblem {
        capacities: &capacities,
        obligations: &[1, 1],
        eligible: &first_edges,
    };
    let second = FundingMinimaxProblem {
        capacities: &capacities,
        obligations: &[1, 2],
        eligible: &second_edges,
    };
    let selected_first = select_funding_with_unit_fee(first, &fee, 0, 0, limits(first), &work())
        .unwrap()
        .unwrap();
    let selected_second = select_funding_with_unit_fee(second, &fee, 0, 0, limits(second), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected_first.resource_totals().source_debits(), &[
        0, 1, 1, 0, 0
    ]);
    assert_eq!(selected_second.resource_totals().source_debits(), &[
        1, 0, 1, 1, 0
    ]);
    let exposure = |left: &[Vec<u64>], right: &[Vec<u64>]| -> u64 {
        draws(left)
            .iter()
            .zip(draws(right))
            .map(|(a, b)| (*a).max(b))
            .sum::<u64>()
            + 1
    };
    assert_eq!(
        exposure(
            selected_first.resource_assignment(),
            selected_second.resource_assignment()
        ),
        5
    );
    let left_domain = enumerate(first);
    let right_domain = enumerate(second);
    let mut complete = Vec::new();
    for left in &left_domain {
        for right in &right_domain {
            if exposure(left, right) <= 4 {
                complete.push((left, right));
            }
        }
    }
    assert_eq!(complete.len(), 6);
    let projected_first = complete
        .iter()
        .map(|(left, _)| *left)
        .min_by(|a, b| order(a, b, 0))
        .unwrap();
    let projected_second = complete
        .iter()
        .map(|(_, right)| *right)
        .min_by(|a, b| order(a, b, 0))
        .unwrap();
    assert_eq!(projected_first, selected_first.resource_assignment());
    assert_eq!(projected_second, selected_second.resource_assignment());
    assert_eq!(exposure(projected_first, projected_second), 5);
    assert!(complete
        .iter()
        .any(|(left, right)| *left == projected_first && draws(right) == [0, 1, 1, 1, 0]));
    assert!(complete
        .iter()
        .any(|(left, right)| *right == projected_second && draws(left) == [0, 0, 1, 1, 0]));
}

#[test]
fn exhaustive_two_source_domains_match_joint_oracle() {
    for left in 0..=2 {
        for right in 0..=2 {
            for amount in 0..=4 {
                for edges in 0..4 {
                    for fee in 0..4 {
                        assert_oracle(
                            FundingMinimaxProblem {
                                capacities: &[left, right],
                                obligations: &[amount],
                                eligible: &[vec![edges & 1 != 0], vec![edges & 2 != 0]],
                            },
                            &[fee & 1 != 0, fee & 2 != 0],
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn arbitrary_cohorts_full_width_and_zero_resource_cursors() {
    for count in [1, 2, 3, 64, 65, 129] {
        let capacities = vec![u64::MAX; count];
        let edges = vec![vec![]; count];
        let problem = FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &[],
            eligible: &edges,
        };
        let result = select_funding_with_unit_fee(
            problem,
            &vec![true; count],
            count - 1,
            count - 1,
            limits(problem),
            &work(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.resource_next_cursor(), None);
        assert_eq!(result.fee().next_cursor, 0);
        assert_eq!(result.fee().debits[count - 1], 1);
        assert!(result.resource_unrestricted());
    }
    let edges = [vec![true], vec![true]];
    let problem = FundingMinimaxProblem {
        capacities: &[u64::MAX, 1],
        obligations: &[u64::MAX],
        eligible: &edges,
    };
    let result =
        select_funding_with_unit_fee(problem, &[true, false], 0, 0, limits(problem), &work())
            .unwrap()
            .unwrap();
    assert_eq!(result.resource_totals().source_debits(), &[u64::MAX - 1, 1]);
    assert_eq!(result.fee().debits, [1, 0]);
    assert!(!result.resource_unrestricted());
}

#[test]
fn positive_resource_funding_has_no_two_wallet_or_machine_word_boundary() {
    for count in [3, 65] {
        let capacities = vec![1; count];
        let edges = vec![vec![true]; count];
        let amounts = [count as u64 - 1];
        let problem = FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &amounts,
            eligible: &edges,
        };
        let mut fee = vec![false; count];
        fee[0] = true;
        let selected =
            select_funding_with_unit_fee(problem, &fee, 0, count - 1, limits(problem), &work())
                .unwrap()
                .unwrap();
        assert_eq!(selected.resource_totals().source_debits()[0], 0);
        assert!(selected.resource_totals().source_debits()[1..]
            .iter()
            .all(|draw| *draw == 1));
        assert_eq!(selected.fee().debits[0], 1);
        assert_eq!(selected.resource_next_cursor(), Some(1));
        assert_eq!(selected.fee().next_cursor, count - 1);
    }
}

#[test]
fn errors_and_work_exhaustion_never_report_insolvency() {
    let edges = [vec![true], vec![true]];
    let problem = FundingMinimaxProblem {
        capacities: &[1, 1],
        obligations: &[1],
        eligible: &edges,
    };
    let baseline = work();
    select_funding_with_unit_fee(problem, &[true, false], 0, 0, limits(problem), &baseline)
        .unwrap()
        .unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let required = baseline.usage(dimension).get();
        for limit in [0, required / 2, required - 1, required] {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            bounds.set(dimension, HostWorkLimit::new(limit));
            let result = select_funding_with_unit_fee(
                problem,
                &[true, false],
                0,
                0,
                limits(problem),
                &HostWorkBudget::new(bounds),
            );
            if limit == required {
                assert!(result.unwrap().is_some());
            } else {
                assert!(matches!(result, Err(FundingSearchError::HostWork(_))));
            }
        }
    }
    assert!(
        select_funding_with_unit_fee(problem, &[true], 0, 0, limits(problem), &work()).is_err()
    );
    assert!(
        select_funding_with_unit_fee(problem, &[true; 2], 2, 0, limits(problem), &work()).is_err()
    );
    assert!(
        select_funding_with_unit_fee(problem, &[true; 2], 0, 2, limits(problem), &work()).is_err()
    );
    let bad = FundingMinimaxProblem {
        eligible: &[vec![], vec![]],
        ..problem
    };
    assert!(select_funding_with_unit_fee(bad, &[false; 2], 0, 0, limits(bad), &work()).is_err());
}

#[test]
fn parallel_fee_planners_preserve_private_assignments_and_cursor_scopes() {
    let edges = [vec![true, false], vec![true, true], vec![false, true]];
    let problem = FundingMinimaxProblem {
        capacities: &[1, 2, 1],
        obligations: &[1, 1],
        eligible: &edges,
    };
    let fee = [true, false, true];
    let expected: Vec<_> = (0..3)
        .map(|cursor| {
            select_funding_with_unit_fee(
                problem,
                &fee,
                cursor,
                (cursor + 1) % 3,
                limits(problem),
                &work(),
            )
            .unwrap()
        })
        .collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..3)
            .map(|cursor| {
                scope.spawn(move || {
                    select_funding_with_unit_fee(
                        problem,
                        &fee,
                        cursor,
                        (cursor + 1) % 3,
                        limits(problem),
                        &work(),
                    )
                    .unwrap()
                })
            })
            .collect();
        for (handle, expected) in handles.into_iter().zip(expected) {
            assert_eq!(handle.join().unwrap(), expected);
        }
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn joint_policy_matches_complete_multi_obligation_domains(
        rows in prop::collection::vec((0_u64..=3, any::<bool>(), prop::array::uniform2(any::<bool>())), 1..=4),
        demand in prop::array::uniform2(0_u64..=3),
    ) {
        let capacities: Vec<_> = rows.iter().map(|row| row.0).collect();
        let fee: Vec<_> = rows.iter().map(|row| row.1).collect();
        let edges: Vec<_> = rows.iter().map(|row| row.2.to_vec()).collect();
        assert_oracle(FundingMinimaxProblem { capacities: &capacities, obligations: &demand, eligible: &edges }, &fee);
    }
}
