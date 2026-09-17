use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::check_funding_deficit;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits(problem: FundingBoxProblem<'_>) -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(problem.upper.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(problem.obligations.len().max(1)).unwrap(),
    }
}

fn row_sums(matrix: &[Vec<u64>]) -> Vec<u64> { matrix.iter().map(|row| row.iter().sum()).collect() }

pub(super) fn assert_transition(
    problem: FundingBoxProblem<'_>,
    before: &[Vec<u64>],
    after: &[Vec<u64>],
    receiver: usize,
    donor: usize,
    amount: u64,
    path: &[(usize, usize, usize)],
) {
    assert_ne!(receiver, donor);
    assert!(amount > 0);
    let old = row_sums(before);
    let new = row_sums(after);
    let mut sources = std::collections::BTreeSet::from([receiver]);
    let mut obligations = std::collections::BTreeSet::new();
    let mut expected: Vec<Vec<i128>> = before
        .iter()
        .map(|row| row.iter().map(|x| i128::from(*x)).collect())
        .collect();
    let mut node = receiver;
    for &(prior, j, next) in path.iter().rev() {
        assert_eq!(prior, node);
        assert!(sources.insert(next));
        assert!(obligations.insert(j));
        assert!(problem.eligible[prior][j]);
        assert!(before[next][j] >= amount);
        expected[prior][j] += i128::from(amount);
        expected[next][j] -= i128::from(amount);
        node = next;
    }
    assert_eq!(node, donor);
    for i in 0..old.len() {
        for j in 0..problem.obligations.len() {
            assert_eq!(i128::from(after[i][j]), expected[i][j]);
            assert!(problem.eligible[i][j] || after[i][j] == 0);
        }
        assert!(new[i] <= problem.upper[i]);
        assert!(new[i] >= old[i].min(problem.lower[i]));
        assert_eq!(
            i128::from(new[i]) - i128::from(old[i]),
            if i == receiver {
                i128::from(amount)
            } else if i == donor {
                -i128::from(amount)
            } else {
                0
            }
        );
    }
    for (j, required) in problem.obligations.iter().enumerate() {
        assert_eq!(after.iter().map(|row| row[j]).sum::<u64>(), *required);
    }
    let deficit = |rows: &[u64]| -> u128 {
        rows.iter()
            .zip(problem.lower)
            .map(|(value, lower)| u128::from(lower.saturating_sub(*value)))
            .sum()
    };
    assert_eq!(deficit(&old) - deficit(&new), u128::from(amount));
}

fn validate_result(problem: FundingBoxProblem<'_>, result: FundingBoxFeasibility) -> bool {
    match result {
        FundingBoxFeasibility::Feasible { assignment, totals } => {
            assert_eq!(
                check_funding_assignment(
                    problem.upper,
                    problem.obligations,
                    problem.eligible,
                    &assignment,
                    limits(problem).source_cap,
                    limits(problem).obligation_cap,
                ),
                Ok(totals.clone())
            );
            assert!(totals
                .source_debits()
                .iter()
                .zip(problem.lower)
                .all(|(x, lo)| x >= lo));
            true
        }
        FundingBoxFeasibility::UpperDeficit {
            selected_obligations,
        } => {
            assert!(check_funding_deficit(
                problem.upper,
                problem.obligations,
                problem.eligible,
                &selected_obligations,
                limits(problem).source_cap,
                limits(problem).obligation_cap,
            )
            .unwrap());
            false
        }
        FundingBoxFeasibility::LowerDeficit { selected_sources } => {
            assert!(
                check_lower_funding_cut(problem, &selected_sources, limits(problem), &work())
                    .unwrap()
            );
            let requested: u128 = problem
                .lower
                .iter()
                .zip(&selected_sources)
                .filter(|(_, selected)| **selected)
                .map(|(value, _)| u128::from(*value))
                .sum();
            let available: u128 = problem
                .obligations
                .iter()
                .enumerate()
                .filter(|(j, _)| {
                    selected_sources
                        .iter()
                        .enumerate()
                        .any(|(i, selected)| *selected && problem.eligible[i][*j])
                })
                .map(|(_, value)| u128::from(*value))
                .sum();
            assert!(requested > available);
            false
        }
    }
}

fn oracle(problem: FundingBoxProblem<'_>, draws: &mut [u64], remaining: &mut [u64]) -> bool {
    let Some(j) = remaining.iter().position(|x| *x > 0) else {
        return draws.iter().zip(problem.lower).all(|(x, lo)| x >= lo);
    };
    remaining[j] -= 1;
    for i in 0..draws.len() {
        if problem.eligible[i][j] && draws[i] < problem.upper[i] {
            draws[i] += 1;
            let result = oracle(problem, draws, remaining);
            draws[i] -= 1;
            if result {
                remaining[j] += 1;
                return true;
            }
        }
    }
    remaining[j] += 1;
    false
}

#[test]
fn exact_lower_bounds_require_alternating_reassignment_through_a_full_source() {
    let problem = FundingBoxProblem {
        lower: &[1, 1, 0],
        upper: &[1, 1, 1],
        obligations: &[1, 1],
        eligible: &[vec![true, false], vec![true, true], vec![false, true]],
    };
    let initial = vec![vec![0, 0], vec![1, 0], vec![0, 1]];
    let totals = problem
        .verify_assignment(&initial, limits(problem), &work())
        .unwrap();
    let result = rebalance(problem, initial, totals, limits(problem), &work()).unwrap();
    let FundingBoxFeasibility::Feasible { assignment, totals } = result else {
        panic!("the two-edge alternating path must satisfy both lower bounds");
    };
    assert_eq!(assignment, [vec![1, 0], vec![0, 1], vec![0, 0]]);
    assert_eq!(totals.source_debits(), &[1, 1, 0]);
}

#[test]
fn unrelated_obligations_cannot_satisfy_a_source_lower_bound() {
    let problem = FundingBoxProblem {
        lower: &[2, 0],
        upper: &[2, 2],
        obligations: &[1, 1],
        eligible: &[vec![true, false], vec![false, true]],
    };
    assert_eq!(
        solve_box_funding_feasibility(problem, limits(problem), &work()),
        Ok(FundingBoxFeasibility::LowerDeficit {
            selected_sources: vec![true, false]
        })
    );
    assert!(!check_lower_funding_cut(problem, &[true, true], limits(problem), &work()).unwrap());
    let sufficient = FundingBoxProblem {
        lower: &[1, 0],
        ..problem
    };
    assert!(
        !check_lower_funding_cut(sufficient, &[true, false], limits(problem), &work()).unwrap()
    );
}

#[test]
fn exhaustive_two_source_boxes_match_independent_unit_oracle() {
    let mut cases = 0;
    for mask in 0..16 {
        let edges = [vec![mask & 1 != 0, mask & 2 != 0], vec![
            mask & 4 != 0,
            mask & 8 != 0,
        ]];
        for u0 in 0..=2 {
            for u1 in 0..=2 {
                for l0 in 0..=u0 {
                    for l1 in 0..=u1 {
                        for q0 in 0..=2 {
                            for q1 in 0..=2 {
                                let problem = FundingBoxProblem {
                                    lower: &[l0, l1],
                                    upper: &[u0, u1],
                                    obligations: &[q0, q1],
                                    eligible: &edges,
                                };
                                let expected = oracle(problem, &mut [0, 0], &mut [q0, q1]);
                                let result = solve_box_funding_feasibility(
                                    problem,
                                    limits(problem),
                                    &work(),
                                )
                                .unwrap();
                                assert_eq!(
                                    validate_result(problem, result),
                                    expected,
                                    "{problem:?}"
                                );
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 5184);
}

#[test]
fn full_width_amounts_do_not_expand_to_per_token_work() {
    let mut usages = None;
    for amount in [1, 1_000_000, u64::MAX / 2] {
        let problem = FundingBoxProblem {
            lower: &[amount, amount, 0],
            upper: &[amount; 3],
            obligations: &[amount; 2],
            eligible: &[vec![true, false], vec![true, true], vec![false, true]],
        };
        let initial = vec![vec![0, 0], vec![amount, 0], vec![0, amount]];
        let totals = problem
            .verify_assignment(&initial, limits(problem), &work())
            .unwrap();
        let budget = work();
        let result = rebalance(problem, initial, totals, limits(problem), &budget).unwrap();
        assert!(validate_result(problem, result));
        let counts: Vec<_> = [
            HostWorkDimension::SearchCandidates,
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationOperations,
        ]
        .map(|dimension| budget.usage(dimension).get())
        .into();
        if let Some(previous) = &usages {
            assert_eq!(&counts, previous);
        }
        usages = Some(counts);
    }
}

#[test]
fn wide_lower_sums_empty_demands_and_arbitrary_cohorts_are_supported() {
    for count in [1, 2, 3, 64, 65, 129] {
        let upper = vec![u64::MAX; count];
        let lower = vec![u64::MAX / count as u64; count];
        let eligible = vec![vec![true]; count];
        let problem = FundingBoxProblem {
            lower: &lower,
            upper: &upper,
            obligations: &[u64::MAX],
            eligible: &eligible,
        };
        let result = solve_box_funding_feasibility(problem, limits(problem), &work()).unwrap();
        assert!(validate_result(problem, result));
        let impossible = FundingBoxProblem {
            lower: &upper,
            ..problem
        };
        let result =
            solve_box_funding_feasibility(impossible, limits(impossible), &work()).unwrap();
        assert_eq!(validate_result(impossible, result), count == 1);
        let zeros = vec![0; count];
        let empty_edges = vec![vec![]; count];
        let empty = FundingBoxProblem {
            lower: &zeros,
            upper: &upper,
            obligations: &[],
            eligible: &empty_edges,
        };
        let result = solve_box_funding_feasibility(empty, limits(empty), &work()).unwrap();
        assert!(validate_result(empty, result));
        let impossible = FundingBoxProblem {
            lower: &upper,
            ..empty
        };
        let result =
            solve_box_funding_feasibility(impossible, limits(impossible), &work()).unwrap();
        assert!(!validate_result(impossible, result));
    }
}

#[test]
fn every_budget_prefix_returns_an_error_not_a_different_funding_result() {
    for lower in [[1, 0], [2, 0]] {
        let problem = FundingBoxProblem {
            lower: &lower,
            upper: &[2, 2],
            obligations: &[1, 1],
            eligible: &[vec![true, false], vec![true, true]],
        };
        let measured = work();
        let expected = solve_box_funding_feasibility(problem, limits(problem), &measured).unwrap();
        for dimension in [
            HostWorkDimension::SearchCandidates,
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationOperations,
        ] {
            let full = measured.usage(dimension).get();
            for prefix in 0..=full {
                let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
                bounds.set(dimension, HostWorkLimit::new(prefix));
                let actual = solve_box_funding_feasibility(
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
            }
        }
    }
}

#[test]
fn malformed_bounds_and_amount_overflow_are_input_errors() {
    let problem = FundingBoxProblem {
        lower: &[0],
        upper: &[1],
        obligations: &[1],
        eligible: &[vec![true]],
    };
    assert_eq!(
        solve_box_funding_feasibility(
            FundingBoxProblem {
                lower: &[2],
                ..problem
            },
            limits(problem),
            &work()
        ),
        Err(FundingSearchError::InvalidSourceBounds)
    );
    assert_eq!(
        solve_box_funding_feasibility(
            FundingBoxProblem {
                lower: &[],
                ..problem
            },
            limits(problem),
            &work()
        ),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        check_lower_funding_cut(problem, &[], limits(problem), &work()),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    let overflow = FundingBoxProblem {
        lower: &[0],
        upper: &[u64::MAX],
        obligations: &[u64::MAX, 1],
        eligible: &[vec![true; 2]],
    };
    assert_eq!(
        solve_box_funding_feasibility(overflow, limits(overflow), &work()),
        Err(FundingAssignmentError::Overflow.into())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_initial_assignments_match_every_lower_cut(
        count in 1_usize..9,
        width in 0_usize..5,
        cells in prop::collection::vec(0_u64..4, 32),
        allowed in prop::collection::vec(any::<bool>(), 32),
        extra in prop::collection::vec(0_u64..4, 8),
        raw_lower in prop::collection::vec(any::<u64>(), 8),
    ) {
        let eligible: Vec<Vec<_>> = (0..count).map(|i| (0..width).map(|j| allowed[i * 4 + j]).collect()).collect();
        let initial: Vec<Vec<_>> = (0..count).map(|i| (0..width).map(|j| if eligible[i][j] { cells[i * 4 + j] } else { 0 }).collect()).collect();
        let rows = row_sums(&initial);
        let upper: Vec<_> = rows.iter().zip(extra).map(|(row, extra)| row + extra).collect();
        let lower: Vec<_> = upper.iter().zip(raw_lower).map(|(upper, lower)| lower % (upper + 1)).collect();
        let demand: Vec<u64> = (0..width).map(|j| initial.iter().map(|row| row[j]).sum()).collect();
        let problem = FundingBoxProblem { lower: &lower, upper: &upper, obligations: &demand, eligible: &eligible };
        let totals = problem.verify_assignment(&initial, limits(problem), &work()).unwrap();
        let result = rebalance(problem, initial, totals, limits(problem), &work()).unwrap();
        let actual = validate_result(problem, result);
        let mut every_cut = true;
        for mask in 0..(1_usize << count) {
            let selected: Vec<_> = (0..count).map(|i| mask & (1 << i) != 0).collect();
            let requested: u64 = lower.iter().zip(&selected).filter(|(_, chosen)| **chosen).map(|(value, _)| *value).sum();
            let supplied: u64 = demand.iter().enumerate().filter(|(j, _)| (0..count).any(|i| selected[i] && eligible[i][*j])).map(|(_, value)| *value).sum();
            let failed = requested > supplied;
            prop_assert_eq!(check_lower_funding_cut(problem, &selected, limits(problem), &work()).unwrap(), failed);
            every_cut &= !failed;
        }
        prop_assert_eq!(actual, every_cut);
    }

    #[test]
    fn generated_boxes_agree_with_the_complete_oracle_and_permutations(
        upper in prop::collection::vec(0_u64..4, 1..6),
        demand in prop::collection::vec(0_u64..3, 0..4),
        raw_lower in prop::collection::vec(0_u64..4, 5),
        bits in prop::collection::vec(any::<bool>(), 15),
    ) {
        let lower: Vec<_> = upper.iter().zip(raw_lower).map(|(hi, lo)| lo.min(*hi)).collect();
        let eligible: Vec<Vec<_>> = (0..upper.len()).map(|i| (0..demand.len()).map(|j| bits[i * 3 + j]).collect()).collect();
        let problem = FundingBoxProblem { lower: &lower, upper: &upper, obligations: &demand, eligible: &eligible };
        let expected = oracle(problem, &mut vec![0; upper.len()], &mut demand.clone());
        let result = solve_box_funding_feasibility(problem, limits(problem), &work()).unwrap();
        prop_assert_eq!(validate_result(problem, result), expected);
        let reversed_upper: Vec<_> = upper.iter().rev().copied().collect();
        let reversed_lower: Vec<_> = lower.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        let reversed = FundingBoxProblem { lower: &reversed_lower, upper: &reversed_upper, obligations: &reversed_demand, eligible: &reversed_edges };
        let result = solve_box_funding_feasibility(reversed, limits(reversed), &work()).unwrap();
        prop_assert_eq!(validate_result(reversed, result), expected);
    }
}
