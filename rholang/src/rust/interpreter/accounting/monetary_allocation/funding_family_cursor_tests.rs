use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

#[derive(Clone, Debug)]
struct Candidate {
    draws: Vec<u64>,
    fee: Vec<u64>,
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

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn outcome_candidates(problem: FundingOutcomeProblem<'_>, common: &[u64]) -> Vec<Candidate> {
    fn assign(
        problem: FundingOutcomeProblem<'_>,
        cell: usize,
        remaining: &mut [u64],
        available: &mut [u64],
        draws: &mut [u64],
        out: &mut Vec<Candidate>,
    ) {
        if cell == remaining.len() * available.len() {
            if remaining.iter().any(|x| *x != 0) {
                return;
            }
            if let Some(eligible) = problem.unit_fee_eligible {
                for payer in 0..available.len() {
                    if eligible[payer] && available[payer] > 0 {
                        let mut fee = vec![0; available.len()];
                        fee[payer] = 1;
                        out.push(Candidate {
                            draws: draws.to_vec(),
                            fee,
                        });
                    }
                }
            } else {
                out.push(Candidate {
                    draws: draws.to_vec(),
                    fee: vec![0; available.len()],
                });
            }
            return;
        }
        let source = cell / remaining.len();
        let obligation = cell % remaining.len();
        let maximum = if problem.resources.eligible[source][obligation] {
            available[source].min(remaining[obligation])
        } else {
            0
        };
        for amount in 0..=maximum {
            available[source] -= amount;
            remaining[obligation] -= amount;
            draws[source] += amount;
            assign(problem, cell + 1, remaining, available, draws, out);
            available[source] += amount;
            remaining[obligation] += amount;
            draws[source] -= amount;
        }
    }
    let mut out = Vec::new();
    let mut available: Vec<_> = common
        .iter()
        .zip(problem.resources.capacities)
        .map(|(a, b)| (*a).min(*b))
        .collect();
    assign(
        problem,
        0,
        &mut problem.resources.obligations.to_vec(),
        &mut available,
        &mut vec![0; common.len()],
        &mut out,
    );
    out
}

fn feasible_families(problem: FundingFamilyOptimizationProblem<'_>) -> Vec<Vec<Candidate>> {
    fn extend(
        alternatives: &[Vec<Candidate>],
        exposure: u128,
        prefix: &mut Vec<Candidate>,
        out: &mut Vec<Vec<Candidate>>,
    ) {
        if prefix.len() == alternatives.len() {
            let n = prefix[0].draws.len();
            let total: u128 = (0..n)
                .map(|i| {
                    u128::from(
                        prefix
                            .iter()
                            .map(|case| case.draws[i] + case.fee[i])
                            .max()
                            .unwrap(),
                    )
                })
                .sum();
            if total <= exposure {
                out.push(prefix.clone());
            }
            return;
        }
        for candidate in &alternatives[prefix.len()] {
            prefix.push(candidate.clone());
            extend(alternatives, exposure, prefix, out);
            prefix.pop();
        }
    }
    let alternatives: Vec<_> = problem
        .outcomes
        .iter()
        .map(|case| outcome_candidates(*case, problem.capacities))
        .collect();
    let mut out = Vec::new();
    extend(
        &alternatives,
        problem.exposure_limit,
        &mut Vec::new(),
        &mut out,
    );
    out
}

fn rank(draws: &[u64]) -> Vec<u64> {
    let mut sorted = draws.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    sorted
}

fn simplex(capacities: &[u64], total: u64) -> BTreeSet<Vec<u64>> {
    fn extend(capacities: &[u64], left: u64, prefix: &mut Vec<u64>, out: &mut BTreeSet<Vec<u64>>) {
        if prefix.len() == capacities.len() {
            if left == 0 {
                out.insert(prefix.clone());
            }
            return;
        }
        for amount in 0..=capacities[prefix.len()].min(left) {
            prefix.push(amount);
            extend(capacities, left - amount, prefix, out);
            prefix.pop();
        }
    }
    let mut out = BTreeSet::new();
    extend(capacities, total, &mut Vec::new(), &mut out);
    out
}

fn capped_cursor(capacities: &[u64], total: u64, cursor: usize) -> usize {
    let mut level = 0;
    while capacities
        .iter()
        .map(|cap| (*cap).min(level + 1))
        .sum::<u64>()
        <= total
        && level < total
    {
        level += 1;
    }
    let mut residual = total - capacities.iter().map(|cap| (*cap).min(level)).sum::<u64>();
    let mut next = cursor;
    for offset in 0..capacities.len() {
        let payer = (cursor + offset) % capacities.len();
        if residual > 0 && capacities[payer] > level {
            residual -= 1;
            next = (payer + 1) % capacities.len();
        }
    }
    assert_eq!(residual, 0);
    next
}

fn assert_projection_oracle(problem: FundingFamilyOptimizationProblem<'_>) {
    let groups: Vec<_> = (0..problem.outcomes.len()).collect();
    let families = feasible_families(problem);
    let actual = select_funding_family_policy(problem, &groups, limits(), &work()).unwrap();
    assert_eq!(actual.is_some(), !families.is_empty());
    let Some(actual) = actual else {
        return;
    };
    let chosen = actual.allocation().outcomes();
    assert!(families.iter().any(|family| family
        .iter()
        .zip(chosen)
        .all(|(candidate, selected)| candidate.draws
            == selected.resource_totals().source_debits()
            && candidate.fee == selected.fee_debits())));
    assert_eq!(actual.transitions().len(), problem.outcomes.len());
    for target in 0..problem.outcomes.len() {
        let total = problem.outcomes[target]
            .resources
            .obligations
            .iter()
            .sum::<u64>();
        let projection: BTreeSet<_> = families
            .iter()
            .filter(|family| {
                family.iter().enumerate().all(|(i, candidate)| {
                    (i == target
                        || rank(&candidate.draws)
                            == rank(chosen[i].resource_totals().source_debits()))
                        && (i >= target
                            || candidate.draws == chosen[i].resource_totals().source_debits())
                })
            })
            .map(|family| family[target].draws.clone())
            .collect();
        let capacities: Vec<_> = problem
            .capacities
            .iter()
            .zip(problem.outcomes[target].resources.capacities)
            .map(|(common, local)| (*common).min(*local))
            .collect();
        let full_simplex = simplex(&capacities, total);
        let unrestricted = projection == full_simplex;
        assert_eq!(
            actual.transitions()[target].resource_unrestricted(),
            unrestricted,
            "target={target}, capacities={capacities:?}, total={total}, projection={projection:?}"
        );
        match actual.transitions()[target].resource_restriction_witness() {
            Some(witness) => {
                assert!(!unrestricted);
                assert!(full_simplex.contains(witness));
                assert!(!projection.contains(witness));
            }
            None => assert!(unrestricted),
        }
        let resource_next = (total > 0).then(|| {
            if unrestricted {
                capped_cursor(&capacities, total, problem.resource_cursor)
            } else {
                (problem.resource_cursor + 1) % capacities.len()
            }
        });
        assert_eq!(
            actual.transitions()[target].resource_next_cursor(),
            resource_next
        );
        let mut possible_fee_payers = vec![false; capacities.len()];
        for family in &families {
            if family.iter().enumerate().all(|(i, candidate)| {
                candidate.draws == chosen[i].resource_totals().source_debits()
                    && (i >= target || candidate.fee == chosen[i].fee_debits())
            }) {
                for (possible, debit) in possible_fee_payers.iter_mut().zip(&family[target].fee) {
                    *possible |= *debit == 1;
                }
            }
        }
        assert_eq!(
            actual.transitions()[target].possible_fee_payers(),
            possible_fee_payers
        );
        let expected_fee_next = possible_fee_payers.iter().any(|payer| *payer).then(|| {
            let fee_capacities: Vec<_> = possible_fee_payers
                .iter()
                .map(|payer| u64::from(*payer))
                .collect();
            capped_cursor(&fee_capacities, 1, problem.fee_cursor)
        });
        assert_eq!(
            actual.transitions()[target].fee_next_cursor(),
            expected_fee_next
        );
    }
}

#[test]
fn single_feasible_fee_payer_preserves_the_captured_cursor() {
    let capacities = [0, 1, 0];
    let edges = vec![vec![false]; 3];
    let fees = [true, true, true];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &[0],
            eligible: &edges,
        },
        unit_fee_eligible: Some(&fees),
    }];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 1,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    assert_projection_oracle(problem);
    let selected = select_funding_family_policy(problem, &[0], limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(selected.transitions()[0].possible_fee_payers(), &[
        false, true, false
    ]);
    assert_eq!(selected.transitions()[0].fee_next_cursor(), Some(0));
}

#[test]
fn family_cursor_uses_complete_marginals_instead_of_temporary_optimizer_caps() {
    let capacities = [3, 3, 3];
    let edges = vec![vec![true]; 3];
    let first_fee = [true, false, false];
    let second_fee = [false, true, false];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[2],
                eligible: &edges,
            },
            unit_fee_eligible: Some(&first_fee),
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[3],
                eligible: &edges,
            },
            unit_fee_eligible: Some(&second_fee),
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 4,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    assert_projection_oracle(problem);
    let selected = select_funding_family_policy(problem, &[0, 1], limits(), &work())
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.allocation().outcomes()[0]
            .resource_totals()
            .source_debits(),
        &[0, 1, 1]
    );
    assert!(!selected.transitions()[0].resource_unrestricted());
    assert_eq!(selected.transitions()[0].resource_next_cursor(), Some(1));
    let standalone = select_funding_with_unit_fee(
        outcomes[0].resources,
        &first_fee,
        0,
        0,
        limits().search,
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(standalone.resource_next_cursor(), Some(2));
}

#[test]
fn duplicate_semantic_outcomes_reuse_one_projection_and_cursor_transition() {
    let capacities = [2, 2, 2];
    let edges = vec![vec![true]; 3];
    let fee = [true, true, true];
    let single = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &[2],
            eligible: &edges,
        },
        unit_fee_eligible: Some(&fee),
    }];
    let duplicates = [single[0], single[0], single[0]];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &single,
        exposure_limit: 3,
        resource_cursor: 1,
        fee_cursor: 2,
    };
    let once = select_funding_family_policy(problem, &[0], limits(), &work())
        .unwrap()
        .unwrap();
    let repeated = select_funding_family_policy(
        FundingFamilyOptimizationProblem {
            outcomes: &duplicates,
            ..problem
        },
        &[0, 0, 0],
        limits(),
        &work(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(repeated.allocation().holds(), once.allocation().holds());
    assert_eq!(repeated.transitions().len(), 3);
    for transition in repeated.transitions() {
        assert_eq!(
            transition.resource_unrestricted(),
            once.transitions()[0].resource_unrestricted()
        );
        assert_eq!(
            transition.resource_next_cursor(),
            once.transitions()[0].resource_next_cursor()
        );
        assert_eq!(
            transition.fee_next_cursor(),
            once.transitions()[0].fee_next_cursor()
        );
        assert_eq!(
            transition.possible_fee_payers(),
            once.transitions()[0].possible_fee_payers()
        );
    }
}

#[test]
fn malformed_semantic_groups_and_exhausted_host_work_are_errors() {
    let capacities = [2, 2];
    let edges = vec![vec![true]; 2];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &edges,
            },
            unit_fee_eligible: None,
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[2],
                eligible: &edges,
            },
            unit_fee_eligible: None,
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 4,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    for groups in [&[0][..], &[0, 0][..], &[1, 2][..], &[0, 2][..], &[1, 0][..]] {
        assert!(select_funding_family_policy(problem, groups, limits(), &work()).is_err());
    }
    let zero = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(select_funding_family_policy(problem, &[0, 1], limits(), &zero).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn generated_complete_family_marginals_match_independent_enumeration(
        capacities in prop::collection::vec(0_u64..=3, 2..=3),
        case_capacities in prop::array::uniform2(prop::array::uniform3(0_u64..=3)),
        demands in prop::array::uniform2(0_u64..=3),
        resource_bits in prop::array::uniform2(any::<u8>()),
        fee_bits in prop::array::uniform2(any::<u8>()),
        fee_present in prop::array::uniform2(any::<bool>()),
        exposure in 0_u128..=7,
        resource_cursor in any::<usize>(),
        fee_cursor in any::<usize>(),
    ) {
        let edges: Vec<Vec<Vec<bool>>> = resource_bits.iter().map(|bits| (0..capacities.len()).map(|i| vec![(bits & (1 << i)) != 0]).collect()).collect();
        let fee: Vec<Vec<bool>> = fee_bits.iter().map(|bits| (0..capacities.len()).map(|i| (bits & (1 << i)) != 0).collect()).collect();
        let outcomes: Vec<_> = (0..2).map(|i| FundingOutcomeProblem {
            resources: FundingMinimaxProblem { capacities: &case_capacities[i][..capacities.len()], obligations: &demands[i..i+1], eligible: &edges[i] },
            unit_fee_eligible: fee_present[i].then_some(fee[i].as_slice()),
        }).collect();
        assert_projection_oracle(FundingFamilyOptimizationProblem {
            capacities: &capacities, outcomes: &outcomes, exposure_limit: exposure,
            resource_cursor: resource_cursor % capacities.len(), fee_cursor: fee_cursor % capacities.len(),
        });
    }

    #[test]
    fn generated_single_outcome_cursors_match_existing_policy(
        capacities in prop::collection::vec(0_u64..=3, 1..=4),
        demand in 0_u64..=4,
        resource_bits in any::<u8>(), fee_bits in any::<u8>(),
        resource_seed in any::<usize>(), fee_seed in any::<usize>(),
    ) {
        let edges: Vec<_> = (0..capacities.len()).map(|i| vec![(resource_bits & (1 << i)) != 0]).collect();
        let fees: Vec<_> = (0..capacities.len()).map(|i| (fee_bits & (1 << i)) != 0).collect();
        let resources = FundingMinimaxProblem { capacities: &capacities, obligations: &[demand], eligible: &edges };
        let outcomes = [FundingOutcomeProblem { resources, unit_fee_eligible: Some(&fees) }];
        let resource_cursor = resource_seed % capacities.len();
        let fee_cursor = fee_seed % capacities.len();
        let family = select_funding_family_policy(FundingFamilyOptimizationProblem {
            capacities: &capacities, outcomes: &outcomes, exposure_limit: capacities.iter().map(|v| u128::from(*v)).sum(), resource_cursor, fee_cursor,
        }, &[0], limits(), &work()).unwrap();
        let standalone = select_funding_with_unit_fee(resources, &fees, resource_cursor, fee_cursor, limits().search, &work()).unwrap();
        prop_assert_eq!(family.is_some(), standalone.is_some());
        if let (Some(family), Some(standalone)) = (family, standalone) {
            prop_assert_eq!(family.transitions()[0].resource_next_cursor(), standalone.resource_next_cursor());
            prop_assert_eq!(family.transitions()[0].fee_next_cursor(), Some(standalone.fee().next_cursor));
            prop_assert_eq!(family.allocation().outcomes()[0].resources(), standalone.resource_assignment());
        }
    }
}
