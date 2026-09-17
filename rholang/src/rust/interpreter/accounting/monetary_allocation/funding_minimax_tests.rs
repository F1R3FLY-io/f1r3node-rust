use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::{
    check_funding_deficit, check_funding_minimax_certificate, FundingBurdenRank, FundingExcessCut,
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

fn oracle(
    problem: FundingMinimaxProblem<'_>,
    remaining: &mut [u64],
    draws: &mut [u64],
    best: &mut Option<Vec<u64>>,
) {
    let Some(j) = remaining.iter().position(|value| *value > 0) else {
        let mut rank = draws.to_vec();
        rank.sort_unstable_by(|a, b| b.cmp(a));
        if best.as_ref().is_none_or(|prior| rank < *prior) {
            *best = Some(rank);
        }
        return;
    };
    remaining[j] -= 1;
    for i in 0..draws.len() {
        if problem.eligible[i][j] && draws[i] < problem.capacities[i] {
            draws[i] += 1;
            oracle(problem, remaining, draws, best);
            draws[i] -= 1;
        }
    }
    remaining[j] += 1;
}

fn expected(problem: FundingMinimaxProblem<'_>) -> Option<Vec<u64>> {
    let mut best = None;
    oracle(
        problem,
        &mut problem.obligations.to_vec(),
        &mut vec![0; problem.capacities.len()],
        &mut best,
    );
    best
}

fn actual(problem: FundingMinimaxProblem<'_>) -> Option<Vec<u64>> {
    match solve_fixed_funding_minimax_rank(problem, limits(problem), &work()).unwrap() {
        FundingMinimaxRankResult::Optimal {
            assignment,
            totals,
            certificate,
        } => {
            assert_eq!(
                problem.check_assignment(&assignment, limits(problem), &work()),
                Ok(totals.clone())
            );
            assert!(check_funding_minimax_certificate(
                problem,
                &assignment,
                certificate.cuts(),
                limits(problem),
                &work()
            )
            .unwrap());
            Some(
                FundingBurdenRank::from_assignment(&totals)
                    .descending_contributions()
                    .to_vec(),
            )
        }
        FundingMinimaxRankResult::Infeasible {
            selected_obligations,
        } => {
            assert!(check_funding_deficit(
                problem.capacities,
                problem.obligations,
                problem.eligible,
                &selected_obligations,
                limits(problem).source_cap,
                limits(problem).obligation_cap
            )
            .unwrap());
            None
        }
    }
}

#[test]
fn lower_rank_levels_are_optimized_after_the_forced_maximum() {
    let problem = FundingMinimaxProblem {
        capacities: &[5, 5, 5],
        obligations: &[5, 5],
        eligible: &[vec![true, false], vec![false, true], vec![false, true]],
    };
    assert_eq!(actual(problem), Some(vec![5, 3, 2]));
    let wrong = [vec![5, 0], vec![0, 4], vec![0, 1]];
    assert_eq!(
        certify_funding_minimax(problem, &wrong, limits(problem), &work()),
        Ok(None)
    );
    let maximum_only = [
        FundingExcessCut {
            threshold: 0,
            selected_obligations: vec![true, true],
        },
        FundingExcessCut {
            threshold: 3,
            selected_obligations: vec![true, false],
        },
        FundingExcessCut {
            threshold: 4,
            selected_obligations: vec![true, false],
        },
    ];
    assert!(!check_funding_minimax_certificate(
        problem,
        &wrong,
        &maximum_only,
        limits(problem),
        &work()
    )
    .unwrap());
}

#[test]
fn certificates_cannot_omit_or_reorder_levels_or_forge_capacity() {
    let problem = FundingMinimaxProblem {
        capacities: &[5, 5, 5],
        obligations: &[5, 5],
        eligible: &[vec![true, false], vec![false, true], vec![false, true]],
    };
    let assignment = [vec![5, 0], vec![0, 3], vec![0, 2]];
    let certificate = certify_funding_minimax(problem, &assignment, limits(problem), &work())
        .unwrap()
        .unwrap();
    for index in 0..certificate.cuts().len() {
        let mut shortened = certificate.cuts().to_vec();
        shortened.remove(index);
        assert!(!check_funding_minimax_certificate(
            problem,
            &assignment,
            &shortened,
            limits(problem),
            &work()
        )
        .unwrap());
        let mut altered = certificate.cuts().to_vec();
        altered[index].threshold += 1;
        assert!(!check_funding_minimax_certificate(
            problem,
            &assignment,
            &altered,
            limits(problem),
            &work()
        )
        .unwrap());
        altered = certificate.cuts().to_vec();
        if altered[index]
            .selected_obligations
            .iter()
            .any(|selected| *selected)
        {
            altered[index].selected_obligations.fill(false);
            assert!(!check_funding_minimax_certificate(
                problem,
                &assignment,
                &altered,
                limits(problem),
                &work()
            )
            .unwrap());
        }
    }
    let mut reversed = certificate.cuts().to_vec();
    reversed.reverse();
    assert!(!check_funding_minimax_certificate(
        problem,
        &assignment,
        &reversed,
        limits(problem),
        &work()
    )
    .unwrap());
    let unrestricted = FundingMinimaxProblem {
        eligible: &[vec![true; 2], vec![true; 2], vec![true; 2]],
        ..problem
    };
    assert!(!check_funding_minimax_certificate(
        unrestricted,
        &assignment,
        certificate.cuts(),
        limits(unrestricted),
        &work()
    )
    .unwrap());
}

#[test]
fn exhaustive_two_source_graphs_match_global_rank_oracle() {
    let mut cases = 0;
    for mask in 0..16 {
        let edges = [vec![mask & 1 != 0, mask & 2 != 0], vec![
            mask & 4 != 0,
            mask & 8 != 0,
        ]];
        for c0 in 0..=2 {
            for c1 in 0..=2 {
                for q0 in 0..=2 {
                    for q1 in 0..=2 {
                        let problem = FundingMinimaxProblem {
                            capacities: &[c0, c1],
                            obligations: &[q0, q1],
                            eligible: &edges,
                        };
                        assert_eq!(actual(problem), expected(problem), "{problem:?}");
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 1296);
}

#[test]
fn exhaustive_three_source_graphs_match_global_rank_oracle() {
    let mut cases = 0;
    for mask in 0..64 {
        let edges: Vec<Vec<_>> = (0..3)
            .map(|i| (0..2).map(|j| mask & (1 << (i * 2 + j)) != 0).collect())
            .collect();
        for code in 0..27 {
            let capacity = [code % 3, (code / 3) % 3, code / 9];
            for q0 in 0..=2 {
                for q1 in 0..=2 {
                    let problem = FundingMinimaxProblem {
                        capacities: &capacity,
                        obligations: &[q0, q1],
                        eligible: &edges,
                    };
                    assert_eq!(actual(problem), expected(problem), "{problem:?}");
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 15_552);
}

#[test]
fn successive_tight_groups_preserve_three_distinct_rank_levels() {
    let problem = FundingMinimaxProblem {
        capacities: &[15; 6],
        obligations: &[9, 5, 1],
        eligible: &[
            vec![true, false, false],
            vec![true, false, false],
            vec![false, true, false],
            vec![false, true, false],
            vec![false, false, true],
            vec![false, false, true],
        ],
    };
    assert_eq!(actual(problem), Some(vec![5, 4, 3, 2, 1, 0]));
}

#[test]
fn full_width_amounts_and_large_cohorts_have_certified_rank() {
    for count in [1, 2, 3, 64, 65, 129] {
        let capacity = vec![u64::MAX; count];
        let eligible = vec![vec![true]; count];
        let problem = FundingMinimaxProblem {
            capacities: &capacity,
            obligations: &[u64::MAX],
            eligible: &eligible,
        };
        let rank = actual(problem).unwrap();
        assert_eq!(
            rank.iter().map(|x| u128::from(*x)).sum::<u128>(),
            u128::from(u64::MAX)
        );
        assert!(rank[0] - rank[count - 1] <= 1);
    }
    let amount = u64::MAX / 2;
    let problem = FundingMinimaxProblem {
        capacities: &[u64::MAX; 3],
        obligations: &[amount, u64::MAX - amount],
        eligible: &[vec![true, false], vec![false, true], vec![false, true]],
    };
    let rank = actual(problem).unwrap();
    assert_eq!(rank, vec![
        amount,
        (u64::MAX - amount) / 2,
        (u64::MAX - amount) / 2
    ]);
}

#[test]
fn zero_demand_has_no_positive_level_certificates() {
    let problem = FundingMinimaxProblem {
        capacities: &[0, u64::MAX, 2],
        obligations: &[],
        eligible: &[vec![], vec![], vec![]],
    };
    let result = solve_fixed_funding_minimax_rank(problem, limits(problem), &work()).unwrap();
    let FundingMinimaxRankResult::Optimal {
        totals,
        certificate,
        ..
    } = result
    else {
        panic!("zero demand is feasible");
    };
    assert_eq!(totals.source_debits(), &[0, 0, 0]);
    assert!(certificate.cuts().is_empty());
}

#[test]
fn insufficient_budget_never_selects_an_uncertified_result() {
    let problem = FundingMinimaxProblem {
        capacities: &[3, 3, 3],
        obligations: &[3, 2],
        eligible: &[vec![true, false], vec![false, true], vec![false, true]],
    };
    let measured = work();
    let result = solve_fixed_funding_minimax_rank(problem, limits(problem), &measured).unwrap();
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        let full = measured.usage(dimension).get();
        for prefix in 0..=full {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            bounds.set(dimension, HostWorkLimit::new(prefix));
            let actual = solve_fixed_funding_minimax_rank(
                problem,
                limits(problem),
                &HostWorkBudget::new(bounds),
            );
            if prefix == full {
                assert_eq!(actual, Ok(result.clone()));
            } else {
                assert!(
                    matches!(actual, Err(FundingSearchError::HostWork(_))),
                    "{dimension:?} {prefix}/{full}: {actual:?}"
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_restricted_ranks_and_permutations_match_full_oracle(
        capacity in prop::collection::vec(0_u64..5, 1..6),
        demand in prop::collection::vec(0_u64..3, 0..4),
        bits in prop::collection::vec(any::<bool>(), 15),
    ) {
        let eligible: Vec<Vec<_>> = (0..capacity.len()).map(|i| (0..demand.len()).map(|j| bits[i * 3 + j]).collect()).collect();
        let problem = FundingMinimaxProblem { capacities: &capacity, obligations: &demand, eligible: &eligible };
        let oracle = expected(problem);
        prop_assert_eq!(actual(problem), oracle.clone());
        let reversed_capacity: Vec<_> = capacity.iter().rev().copied().collect();
        let reversed_demand: Vec<_> = demand.iter().rev().copied().collect();
        let reversed_edges: Vec<Vec<_>> = eligible.iter().rev().map(|row| row.iter().rev().copied().collect()).collect();
        let reversed = FundingMinimaxProblem { capacities: &reversed_capacity, obligations: &reversed_demand, eligible: &reversed_edges };
        prop_assert_eq!(actual(reversed), oracle);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn wide_generated_feasible_domains_return_independently_certified_ranks(
        count in 1_usize..10,
        width in 1_usize..7,
        values in prop::collection::vec(any::<u64>(), 54),
        additional in prop::collection::vec(any::<bool>(), 54),
        spare in prop::collection::vec(any::<u64>(), 9),
    ) {
        let mut remaining = u64::MAX;
        let mut capacities = vec![0_u64; count];
        let mut obligations = vec![0_u64; width];
        let mut eligible = vec![vec![false; width]; count];
        for i in 0..count {
            for (j, obligation) in obligations.iter_mut().enumerate() {
                eligible[i][j] = additional[i * 6 + j];
                let amount = if eligible[i][j] { (values[i * 6 + j] >> 6).min(remaining) } else { 0 };
                remaining -= amount;
                capacities[i] += amount;
                *obligation += amount;
            }
            capacities[i] = capacities[i].saturating_add(spare[i]);
        }
        let problem = FundingMinimaxProblem { capacities: &capacities, obligations: &obligations, eligible: &eligible };
        prop_assert!(actual(problem).is_some());
    }
}
