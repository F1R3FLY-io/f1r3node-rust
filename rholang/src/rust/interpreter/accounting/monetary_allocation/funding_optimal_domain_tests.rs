use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::{
    certify_funding_minimax, check_funding_cyclic_tie_certificate, select_funding_cyclic_tie,
    FundingPrefixExclusion,
};

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits(problem: FundingMinimaxProblem<'_>) -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(problem.capacities.len().max(1)).unwrap(),
        obligation_cap: NonZeroUsize::new(problem.obligations.len().max(1)).unwrap(),
    }
}

fn domain(problem: FundingMinimaxProblem<'_>, reference: &[Vec<u64>]) -> FundingOptimalDomain {
    let certificate = certify_funding_minimax(problem, reference, limits(problem), &work())
        .unwrap()
        .unwrap();
    derive_funding_optimal_domain(
        problem,
        reference,
        certificate.cuts(),
        limits(problem),
        &work(),
    )
    .unwrap()
}

fn contributions(matrix: &[Vec<u64>]) -> Vec<u64> {
    matrix.iter().map(|row| row.iter().sum()).collect()
}

fn rank(matrix: &[Vec<u64>]) -> Vec<u64> {
    let mut result = contributions(matrix);
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

fn enumerate(
    problem: FundingMinimaxProblem<'_>,
    position: usize,
    matrix: &mut [Vec<u64>],
    row_left: &mut [u64],
    column_left: &mut [u64],
    results: &mut Vec<Vec<Vec<u64>>>,
) {
    let width = column_left.len();
    if position == row_left.len() * width {
        if column_left.iter().all(|x| *x == 0) {
            results.push(matrix.to_vec());
        }
        return;
    }
    let i = position / width;
    let j = position % width;
    let maximum = if problem.eligible[i][j] {
        row_left[i].min(column_left[j])
    } else {
        0
    };
    for value in 0..=maximum {
        matrix[i][j] = value;
        row_left[i] -= value;
        column_left[j] -= value;
        enumerate(
            problem,
            position + 1,
            matrix,
            row_left,
            column_left,
            results,
        );
        row_left[i] += value;
        column_left[j] += value;
    }
    matrix[i][j] = 0;
}

fn oracle(problem: FundingMinimaxProblem<'_>) -> Vec<Vec<Vec<u64>>> {
    let mut result = Vec::new();
    enumerate(
        problem,
        0,
        &mut vec![vec![0; problem.obligations.len()]; problem.capacities.len()],
        &mut problem.capacities.to_vec(),
        &mut problem.obligations.to_vec(),
        &mut result,
    );
    result
}

fn cyclic_key(matrix: &[Vec<u64>], cursor: usize) -> Vec<u64> {
    let draws = contributions(matrix);
    draws[cursor..]
        .iter()
        .chain(&draws[..cursor])
        .copied()
        .collect()
}

fn assert_exact_domain_and_ties(problem: FundingMinimaxProblem<'_>) {
    let assignments = oracle(problem);
    let Some(best) = assignments.iter().map(|matrix| rank(matrix)).min() else {
        return;
    };
    let optima: Vec<_> = assignments
        .iter()
        .filter(|matrix| rank(matrix) == best)
        .collect();
    for reference in [optima[0], *optima.last().unwrap()] {
        let captured = domain(problem, reference);
        for matrix in &assignments {
            assert_eq!(
                captured
                    .check_assignment(matrix, limits(problem), &work())
                    .is_ok(),
                rank(matrix) == best,
                "problem={problem:?} reference={reference:?} matrix={matrix:?} domain={captured:?}"
            );
        }
        for cursor in 0..problem.capacities.len() {
            let selected =
                select_funding_cyclic_tie(&captured, cursor, limits(problem), &work()).unwrap();
            let expected = optima
                .iter()
                .map(|matrix| cyclic_key(matrix, cursor))
                .max()
                .unwrap();
            assert_eq!(
                cyclic_key(&selected.assignment, cursor),
                expected,
                "problem={problem:?} cursor={cursor}"
            );
            for matrix in &optima {
                let accepted = check_funding_cyclic_tie_certificate(
                    &captured,
                    matrix,
                    cursor,
                    selected.certificate.exclusions(),
                    limits(problem),
                    &work(),
                )
                .unwrap();
                if accepted {
                    assert_eq!(cyclic_key(matrix, cursor), expected);
                }
            }
        }
    }
}

#[test]
fn paired_breakpoints_exclude_a_worse_rank_with_equal_predecessor_excess() {
    let problem = FundingMinimaxProblem {
        capacities: &[6, 6],
        obligations: &[6],
        eligible: &[vec![true], vec![true]],
    };
    let reference = [vec![3], vec![3]];
    let captured = domain(problem, &reference);
    assert_eq!(captured.problem().upper, &[3, 3]);
    assert!(problem
        .check_assignment(&[vec![4], vec![2]], limits(problem), &work())
        .is_ok());
    assert!(captured
        .check_assignment(&[vec![4], vec![2]], limits(problem), &work())
        .is_err());
    assert!(captured
        .check_assignment(&reference, limits(problem), &work())
        .is_ok());
    assert_exact_domain_and_ties(problem);
}

#[test]
fn tight_cut_removes_cross_obligation_draws_not_excluded_by_source_bounds() {
    let problem = FundingMinimaxProblem {
        capacities: &[10, 10, 10],
        obligations: &[7, 2],
        eligible: &[vec![true, true], vec![true, false], vec![false, true]],
    };
    let reference = [vec![4, 0], vec![3, 0], vec![0, 2]];
    let cuts = [
        FundingExcessCut {
            threshold: 1,
            selected_obligations: vec![true, true],
        },
        FundingExcessCut {
            threshold: 2,
            selected_obligations: vec![true, false],
        },
        FundingExcessCut {
            threshold: 3,
            selected_obligations: vec![true, false],
        },
        FundingExcessCut {
            threshold: 4,
            selected_obligations: vec![false, false],
        },
    ];
    let captured =
        derive_funding_optimal_domain(problem, &reference, &cuts, limits(problem), &work())
            .unwrap();
    assert!(!captured.problem().eligible[0][1]);
    let wrong = [vec![3, 1], vec![4, 0], vec![0, 1]];
    let draws = contributions(&wrong);
    assert!(draws
        .iter()
        .zip(captured.problem().lower)
        .all(|(x, low)| x >= low));
    assert!(draws
        .iter()
        .zip(captured.problem().upper)
        .all(|(x, high)| x <= high));
    assert!(captured
        .check_assignment(&wrong, limits(problem), &work())
        .is_err());
    let generated = domain(problem, &reference);
    for matrix in oracle(problem) {
        assert_eq!(
            captured
                .check_assignment(&matrix, limits(problem), &work())
                .is_ok(),
            generated
                .check_assignment(&matrix, limits(problem), &work())
                .is_ok()
        );
    }
    assert_exact_domain_and_ties(problem);
}

#[test]
fn zero_excess_requires_a_tight_cut_not_truncated_oversupply() {
    let problem = FundingMinimaxProblem {
        capacities: &[10, 10],
        obligations: &[3],
        eligible: &[vec![true], vec![true]],
    };
    let reference = [vec![2], vec![1]];
    let certificate = certify_funding_minimax(problem, &reference, limits(problem), &work())
        .unwrap()
        .unwrap();
    let mut cuts = certificate.cuts().to_vec();
    let last = cuts.last_mut().unwrap();
    assert_eq!(last.threshold, 2);
    assert_eq!(last.selected_obligations, vec![false]);
    last.selected_obligations.fill(true);
    assert!(!check_funding_minimax_certificate(
        problem,
        &reference,
        &cuts,
        limits(problem),
        &work()
    )
    .unwrap());
    assert_eq!(
        derive_funding_optimal_domain(problem, &reference, &cuts, limits(problem), &work()),
        Err(FundingSearchError::InvalidResult)
    );
}

#[test]
fn cyclic_certificate_cannot_freeze_future_wallets_to_fake_a_maximum() {
    let problem = FundingMinimaxProblem {
        capacities: &[10, 10],
        obligations: &[3],
        eligible: &[vec![true], vec![true]],
    };
    let captured = domain(problem, &[vec![2], vec![1]]);
    for (cursor, expected) in [(0, vec![2, 1]), (1, vec![1, 2])] {
        let selected =
            select_funding_cyclic_tie(&captured, cursor, limits(problem), &work()).unwrap();
        assert_eq!(selected.totals.source_debits(), expected);
    }
    let wrong = [vec![1], vec![2]];
    let forged = [
        FundingPrefixExclusion::LowerCut {
            selected_sources: vec![true, true],
        },
        FundingPrefixExclusion::Capacity,
    ];
    assert!(!check_funding_cyclic_tie_certificate(
        &captured,
        &wrong,
        0,
        &forged,
        limits(problem),
        &work()
    )
    .unwrap());
    assert!(!check_funding_cyclic_tie_certificate(
        &captured,
        &wrong,
        0,
        &[
            FundingPrefixExclusion::Capacity,
            FundingPrefixExclusion::Capacity
        ],
        limits(problem),
        &work()
    )
    .unwrap());
}

#[test]
fn cyclic_certificates_reject_missing_entries_and_invalid_cursors() {
    let problem = FundingMinimaxProblem {
        capacities: &[3, 3],
        obligations: &[3],
        eligible: &[vec![true], vec![true]],
    };
    let captured = domain(problem, &[vec![2], vec![1]]);
    let selected = select_funding_cyclic_tie(&captured, 0, limits(problem), &work()).unwrap();
    let proof = selected.certificate.exclusions();
    for index in 0..proof.len() {
        let mut short = proof.to_vec();
        short.remove(index);
        assert!(!check_funding_cyclic_tie_certificate(
            &captured,
            &selected.assignment,
            0,
            &short,
            limits(problem),
            &work()
        )
        .unwrap());
    }
    for cursor in [2, usize::MAX] {
        assert_eq!(
            select_funding_cyclic_tie(&captured, cursor, limits(problem), &work()),
            Err(FundingSearchError::InvalidPriorityCursor)
        );
        assert_eq!(
            check_funding_cyclic_tie_certificate(
                &captured,
                &selected.assignment,
                cursor,
                proof,
                limits(problem),
                &work()
            ),
            Err(FundingSearchError::InvalidPriorityCursor)
        );
    }
}

#[test]
fn exhaustive_small_graphs_preserve_exactly_every_optimal_assignment_and_cyclic_maximum() {
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
                        assert_exact_domain_and_ties(FundingMinimaxProblem {
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
fn parallel_selectors_share_only_the_work_budget_not_funding_state() {
    let problem = FundingMinimaxProblem {
        capacities: &[4, 4, 4],
        obligations: &[5],
        eligible: &[vec![true], vec![true], vec![true]],
    };
    let captured = domain(problem, &[vec![2], vec![2], vec![1]]);
    let before = captured.clone();
    let measured = work();
    let expected: Vec<_> = (0..3)
        .map(|cursor| {
            select_funding_cyclic_tie(&captured, cursor, limits(problem), &measured).unwrap()
        })
        .collect();
    for sufficient in [true, false] {
        let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
        if !sufficient {
            bounds.set(
                HostWorkDimension::SearchCandidates,
                HostWorkLimit::new(measured.usage(HostWorkDimension::SearchCandidates).get() - 1),
            );
        }
        let shared = HostWorkBudget::new(bounds);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..3)
                .map(|cursor| {
                    let captured = &captured;
                    let shared = &shared;
                    scope.spawn(move || {
                        select_funding_cyclic_tie(captured, cursor, limits(problem), shared)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        let mut rejected = false;
        for (actual, expected) in results.into_iter().zip(&expected) {
            match actual {
                Ok(actual) => assert_eq!(&actual, expected),
                Err(FundingSearchError::HostWork(_)) => rejected = true,
                Err(other) => panic!("unexpected concurrent failure: {other:?}"),
            }
        }
        assert_eq!(rejected, !sufficient);
        if sufficient {
            assert_eq!(shared.usages(), measured.usages());
        }
        assert_eq!(captured, before);
    }
}

#[test]
fn arbitrary_cohorts_and_maximum_total_keep_cyclic_residual_order() {
    for count in [1, 2, 3, 64, 65, 129] {
        for total in [0, 1, u64::MAX] {
            let capacity = vec![u64::MAX; count];
            let eligible = vec![vec![true]; count];
            let problem = FundingMinimaxProblem {
                capacities: &capacity,
                obligations: &[total],
                eligible: &eligible,
            };
            let base = total / count as u64;
            let residual = (total % count as u64) as usize;
            let reference: Vec<_> = (0..count)
                .map(|i| vec![base + u64::from(i < residual)])
                .collect();
            let captured = domain(problem, &reference);
            for cursor in [0, count / 2, count - 1] {
                let selected =
                    select_funding_cyclic_tie(&captured, cursor, limits(problem), &work()).unwrap();
                let expected: Vec<_> = (0..count)
                    .map(|i| base + u64::from((i + count - cursor) % count < residual))
                    .collect();
                assert_eq!(selected.totals.source_debits(), expected);
                assert_eq!(selected.totals.total(), total);
            }
        }
    }
}

#[test]
fn every_budget_prefix_fails_closed_without_changing_the_captured_domain() {
    let problem = FundingMinimaxProblem {
        capacities: &[2, 2],
        obligations: &[1],
        eligible: &[vec![true], vec![true]],
    };
    let reference = [vec![1], vec![0]];
    let certificate = certify_funding_minimax(problem, &reference, limits(problem), &work())
        .unwrap()
        .unwrap();
    let captured = domain(problem, &reference);
    let before = captured.clone();
    let selected = select_funding_cyclic_tie(&captured, 0, limits(problem), &work()).unwrap();
    for operation in 0..3 {
        let run = |budget: &HostWorkBudget| -> Result<(), FundingSearchError> {
            match operation {
                0 => {
                    assert_eq!(
                        derive_funding_optimal_domain(
                            problem,
                            &reference,
                            certificate.cuts(),
                            limits(problem),
                            budget
                        )?,
                        captured
                    );
                }
                1 => {
                    assert_eq!(
                        select_funding_cyclic_tie(&captured, 0, limits(problem), budget)?,
                        selected
                    );
                }
                _ => {
                    assert!(check_funding_cyclic_tie_certificate(
                        &captured,
                        &selected.assignment,
                        0,
                        selected.certificate.exclusions(),
                        limits(problem),
                        budget
                    )?);
                }
            }
            Ok(())
        };
        let measured = work();
        run(&measured).unwrap();
        for dimension in [
            HostWorkDimension::SearchCandidates,
            HostWorkDimension::SearchStateBytes,
            HostWorkDimension::VerificationOperations,
        ] {
            let full = measured.usage(dimension).get();
            for prefix in 0..=full {
                let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
                bounds.set(dimension, HostWorkLimit::new(prefix));
                let result = run(&HostWorkBudget::new(bounds));
                if prefix == full {
                    assert_eq!(result, Ok(()));
                } else {
                    assert!(
                        matches!(result, Err(FundingSearchError::HostWork(_))),
                        "operation={operation} {dimension:?} {prefix}/{full}: {result:?}"
                    );
                }
                assert_eq!(captured, before);
            }
        }
    }
}

#[test]
fn zero_total_search_work_does_not_grow_with_unused_capacity() {
    for count in [1, 3, 65] {
        let mut baseline = None;
        for ceiling in [1, u64::MAX] {
            let capacity = vec![ceiling; count];
            let eligible = vec![vec![true]; count];
            let problem = FundingMinimaxProblem {
                capacities: &capacity,
                obligations: &[0],
                eligible: &eligible,
            };
            let captured = domain(problem, &vec![vec![0]; count]);
            let measured = work();
            let selected =
                select_funding_cyclic_tie(&captured, count - 1, limits(problem), &measured)
                    .unwrap();
            assert_eq!(selected.totals.source_debits(), vec![0; count]);
            match baseline {
                None => baseline = Some(measured.usages()),
                Some(expected) => assert_eq!(measured.usages(), expected),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_domains_and_all_cursors_match_complete_assignment_oracle(
        capacity in prop::collection::vec(0_u64..4, 1..5),
        demand in prop::collection::vec(0_u64..3, 0..3),
        bits in prop::collection::vec(any::<bool>(), 8),
    ) {
        let eligible: Vec<Vec<_>> = (0..capacity.len()).map(|i| (0..demand.len()).map(|j| bits[i * 2 + j]).collect()).collect();
        assert_exact_domain_and_ties(FundingMinimaxProblem { capacities: &capacity, obligations: &demand, eligible: &eligible });
        let reversed_capacity: Vec<_> = capacity.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_eligible: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        assert_exact_domain_and_ties(FundingMinimaxProblem {
            capacities: &reversed_capacity, obligations: &reversed_demand, eligible: &reversed_eligible,
        });
    }
}
