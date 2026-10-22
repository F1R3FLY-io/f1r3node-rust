use std::num::NonZeroUsize;

use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    resources: Vec<Vec<u64>>,
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
        search_scans: usize::MAX,
    }
}

fn candidates(problem: FundingOutcomeProblem<'_>, capacities: &[u64]) -> Vec<Candidate> {
    fn visit(
        problem: FundingOutcomeProblem<'_>,
        cell: usize,
        available: &mut [u64],
        remaining: &mut [u64],
        matrix: &mut [Vec<u64>],
        output: &mut Vec<Candidate>,
    ) {
        if cell == available.len() * remaining.len() {
            if remaining.iter().any(|x| *x != 0) {
                return;
            }
            let draws: Vec<_> = matrix.iter().map(|row| row.iter().sum()).collect();
            if let Some(allowed) = problem.unit_fee_eligible {
                for payer in 0..available.len() {
                    if allowed[payer] && available[payer] > 0 {
                        let mut fee = vec![0; available.len()];
                        fee[payer] = 1;
                        output.push(Candidate {
                            resources: matrix.to_vec(),
                            draws: draws.clone(),
                            fee,
                        });
                    }
                }
            } else {
                output.push(Candidate {
                    resources: matrix.to_vec(),
                    draws,
                    fee: vec![0; available.len()],
                });
            }
            return;
        }
        let i = cell / remaining.len();
        let j = cell % remaining.len();
        let cap = if problem.resources.eligible[i][j] {
            available[i].min(remaining[j])
        } else {
            0
        };
        for amount in 0..=cap {
            available[i] -= amount;
            remaining[j] -= amount;
            matrix[i][j] = amount;
            visit(problem, cell + 1, available, remaining, matrix, output);
            available[i] += amount;
            remaining[j] += amount;
        }
        matrix[i][j] = 0;
    }
    let mut output = Vec::new();
    let mut available: Vec<_> = capacities
        .iter()
        .zip(problem.resources.capacities)
        .map(|(a, b)| (*a).min(*b))
        .collect();
    visit(
        problem,
        0,
        &mut available,
        &mut problem.resources.obligations.to_vec(),
        &mut vec![vec![0; problem.resources.obligations.len()]; capacities.len()],
        &mut output,
    );
    output
}

fn ordered(
    left: &[Candidate],
    right: &[Candidate],
    resource_cursor: usize,
    fee_cursor: usize,
) -> Ordering {
    for (lhs, rhs) in left.iter().zip(right) {
        let mut a = lhs.draws.clone();
        let mut b = rhs.draws.clone();
        a.sort_unstable_by(|a, b| b.cmp(a));
        b.sort_unstable_by(|a, b| b.cmp(a));
        if a != b {
            return a.cmp(&b);
        }
    }
    for (lhs, rhs) in left.iter().zip(right) {
        for offset in 0..lhs.draws.len() {
            let i = (resource_cursor + offset) % lhs.draws.len();
            if lhs.draws[i] != rhs.draws[i] {
                return rhs.draws[i].cmp(&lhs.draws[i]);
            }
        }
    }
    for (lhs, rhs) in left.iter().zip(right) {
        for offset in 0..lhs.fee.len() {
            let i = (fee_cursor + offset) % lhs.fee.len();
            if lhs.fee[i] != rhs.fee[i] {
                return rhs.fee[i].cmp(&lhs.fee[i]);
            }
        }
    }
    for (lhs, rhs) in left.iter().zip(right) {
        if lhs.resources != rhs.resources {
            return lhs.resources.cmp(&rhs.resources);
        }
    }
    Ordering::Equal
}

fn oracle(problem: FundingFamilyOptimizationProblem<'_>) -> Option<Vec<Candidate>> {
    fn visit(
        problem: FundingFamilyOptimizationProblem<'_>,
        alternatives: &[Vec<Candidate>],
        selected: &mut Vec<Candidate>,
        best: &mut Option<Vec<Candidate>>,
    ) {
        if selected.len() == alternatives.len() {
            let mut holds = vec![0_u64; problem.capacities.len()];
            for outcome in selected.iter() {
                for (i, hold) in holds.iter_mut().enumerate() {
                    *hold = (*hold).max(outcome.draws[i] + outcome.fee[i]);
                }
            }
            if holds.iter().map(|x| u128::from(*x)).sum::<u128>() > problem.exposure_limit {
                return;
            }
            if best.as_ref().is_none_or(|current| {
                ordered(
                    selected,
                    current,
                    problem.resource_cursor,
                    problem.fee_cursor,
                ) == Ordering::Less
            }) {
                *best = Some(selected.clone());
            }
            return;
        }
        for candidate in &alternatives[selected.len()] {
            selected.push(candidate.clone());
            visit(problem, alternatives, selected, best);
            selected.pop();
        }
    }
    let alternatives: Vec<_> = problem
        .outcomes
        .iter()
        .map(|outcome| candidates(*outcome, problem.capacities))
        .collect();
    let mut best = None;
    visit(problem, &alternatives, &mut Vec::new(), &mut best);
    best
}

fn assert_oracle(problem: FundingFamilyOptimizationProblem<'_>) {
    let expected = oracle(problem);
    let actual = optimize_funding_family(problem, limits()).unwrap();
    assert_eq!(actual.is_some(), expected.is_some());
    if let (Some(actual), Some(expected)) = (actual, expected) {
        let mut holds = vec![0; problem.capacities.len()];
        for (selected, expected) in actual.outcomes().iter().zip(expected) {
            assert_eq!(selected.resources(), expected.resources);
            assert_eq!(selected.resource_totals().source_debits(), expected.draws);
            assert_eq!(selected.fee_debits(), expected.fee);
            for (i, hold) in holds.iter_mut().enumerate() {
                *hold = (*hold).max(expected.draws[i] + expected.fee[i]);
            }
        }
        assert_eq!(actual.holds(), holds);
        assert_eq!(
            actual.total_held(),
            holds.iter().map(|x| u128::from(*x)).sum::<u128>()
        );
        assert!(actual.total_held() <= problem.exposure_limit);
    }
}

#[test]
fn family_optimizer_selects_the_approved_five_source_family() {
    let capacities = [2, 2, 2, 2, 1];
    let alpha = vec![
        vec![false, false],
        vec![true, false],
        vec![true, true],
        vec![true, false],
        vec![false, false],
    ];
    let beta = vec![
        vec![true, false],
        vec![true, false],
        vec![false, true],
        vec![false, true],
        vec![false, false],
    ];
    let fee = [false, false, false, false, true];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1, 1],
                eligible: &alpha,
            },
            unit_fee_eligible: Some(&fee),
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1, 2],
                eligible: &beta,
            },
            unit_fee_eligible: Some(&fee),
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 4,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let selected = optimize_funding_family(problem, limits()).unwrap().unwrap();
    assert_eq!(selected.holds(), [0, 1, 1, 1, 1]);
    assert_eq!(selected.outcomes()[0].resource_totals().source_debits(), [
        0, 1, 1, 0, 0
    ]);
    assert_eq!(selected.outcomes()[1].resource_totals().source_debits(), [
        0, 1, 1, 1, 0
    ]);
    assert_oracle(problem);
    assert_oracle(FundingFamilyOptimizationProblem {
        outcomes: &[outcomes[1], outcomes[0]],
        ..problem
    });
}

#[test]
fn family_optimizer_checks_every_small_two_source_fee_pattern() {
    let capacities = [1, 1];
    for mask in 0_u8..16 {
        let a = vec![vec![mask & 1 != 0], vec![mask & 2 != 0]];
        let b = vec![vec![mask & 4 != 0], vec![mask & 8 != 0]];
        for fees in 0_u8..16 {
            let fa = [fees & 1 != 0, fees & 2 != 0];
            let fb = [fees & 4 != 0, fees & 8 != 0];
            let outcomes = [
                FundingOutcomeProblem {
                    resources: FundingMinimaxProblem {
                        capacities: &capacities,
                        obligations: &[1],
                        eligible: &a,
                    },
                    unit_fee_eligible: Some(&fa),
                },
                FundingOutcomeProblem {
                    resources: FundingMinimaxProblem {
                        capacities: &capacities,
                        obligations: &[1],
                        eligible: &b,
                    },
                    unit_fee_eligible: Some(&fb),
                },
            ];
            for resource_cursor in 0..2 {
                for fee_cursor in 0..2 {
                    for exposure_limit in 1..=2 {
                        assert_oracle(FundingFamilyOptimizationProblem {
                            capacities: &capacities,
                            outcomes: &outcomes,
                            exposure_limit,
                            resource_cursor,
                            fee_cursor,
                        });
                    }
                }
            }
        }
    }
}

#[test]
fn family_optimizer_handles_zero_charges_wide_amounts_and_many_sources() {
    for n in [1, 3, 65, 129] {
        let capacities = vec![u64::MAX; n];
        let eligible = vec![vec![true]; n];
        let fee = vec![true; n];
        let outcomes = [FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &eligible,
            },
            unit_fee_eligible: Some(&fee),
        }];
        let problem = FundingFamilyOptimizationProblem {
            capacities: &capacities,
            outcomes: &outcomes,
            exposure_limit: 2,
            resource_cursor: n - 1,
            fee_cursor: 0,
        };
        let selected = optimize_funding_family(problem, limits()).unwrap().unwrap();
        assert_eq!(selected.total_held(), 2);
        assert_eq!(
            selected.outcomes()[0].resource_totals().source_debits()[n - 1],
            1
        );
    }
    let eligible = vec![vec![true]];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &[u64::MAX],
            obligations: &[u64::MAX],
            eligible: &eligible,
        },
        unit_fee_eligible: None,
    }];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &[u64::MAX],
        outcomes: &outcomes,
        exposure_limit: u128::from(u64::MAX),
        resource_cursor: 0,
        fee_cursor: 0,
    };
    assert_eq!(
        optimize_funding_family(problem, limits())
            .unwrap()
            .unwrap()
            .total_held(),
        u128::from(u64::MAX)
    );
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            obligations: &[0],
            ..outcomes[0].resources
        },
        unit_fee_eligible: None,
    }];
    assert_oracle(FundingFamilyOptimizationProblem {
        capacities: &[1],
        outcomes: &outcomes,
        exposure_limit: 0,
        ..problem
    });
}

#[test]
fn family_optimizer_validates_all_inputs_before_reporting_infeasibility() {
    let eligible = vec![vec![true]];
    let valid = FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &[1],
            obligations: &[1],
            eligible: &eligible,
        },
        unit_fee_eligible: None,
    };
    let malformed = FundingOutcomeProblem {
        unit_fee_eligible: Some(&[]),
        ..valid
    };
    let problem = FundingFamilyOptimizationProblem {
        capacities: &[0],
        outcomes: &[valid, malformed],
        exposure_limit: 0,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    assert!(matches!(
        optimize_funding_family(problem, limits()),
        Err(FundingFamilyError::Search(
            FundingSearchError::InvalidProblem(FundingAssignmentError::InvalidDimensions)
        ))
    ));
    assert!(matches!(
        optimize_funding_family(
            FundingFamilyOptimizationProblem {
                outcomes: &[valid],
                ..problem
            },
            limits(),
        ),
        Ok(None)
    ));
}

#[test]
fn family_optimizer_reports_no_allocation_when_a_charge_exceeds_the_exposure() {
    let capacities = [1_u64 << 40, 1_u64 << 40];
    let eligible = [vec![true], vec![true]];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &[1_u64 << 40],
            eligible: &eligible,
        },
        unit_fee_eligible: None,
    }];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: (1_u128 << 40) - 1,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let result = optimize_funding_family(problem, limits());
    assert!(matches!(result, Ok(None)), "{result:?}");
}

#[test]
fn family_verifier_rejects_altered_and_feasible_noncanonical_proposals() {
    let capacities = [2, 2];
    let eligible = vec![vec![true], vec![true]];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &[1],
            eligible: &eligible,
        },
        unit_fee_eligible: Some(&[false, true]),
    }];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 2,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let selected = optimize_funding_family(problem, limits()).unwrap().unwrap();
    let proposed = [FundingOutcomeProposal {
        resources: selected.outcomes()[0].resources(),
        fee_debits: selected.outcomes()[0].fee_debits(),
    }];
    let proposal = FundingFamilyProposal {
        outcomes: &proposed,
        holds: selected.holds(),
    };
    assert!(verify_funding_family_allocation(problem, proposal, limits()).unwrap());
    assert!(!verify_funding_family_allocation(
        problem,
        FundingFamilyProposal {
            holds: &[0, 2],
            ..proposal
        },
        limits(),
    )
    .unwrap());
    let alternative = [FundingOutcomeProposal {
        resources: &[vec![0], vec![1]],
        fee_debits: &[0, 1],
    }];
    assert!(!verify_funding_family_allocation(
        problem,
        FundingFamilyProposal {
            outcomes: &alternative,
            holds: &[0, 2]
        },
        limits(),
    )
    .unwrap());
    let altered = [FundingOutcomeProposal {
        fee_debits: &[1, 0],
        ..proposed[0]
    }];
    assert!(!verify_funding_family_allocation(
        problem,
        FundingFamilyProposal {
            outcomes: &altered,
            ..proposal
        },
        limits(),
    )
    .unwrap());
}

#[test]
fn single_outcome_family_preserves_full_width_resources_plus_unit_fee() {
    let capacities = [u64::MAX, 1];
    let eligible = [vec![true], vec![true]];
    let fees = [true, false];
    let resources = FundingMinimaxProblem {
        capacities: &capacities,
        obligations: &[u64::MAX],
        eligible: &eligible,
    };
    let existing = select_funding_with_unit_fee(resources, &fees, 0, 0, limits().search)
        .unwrap()
        .unwrap();
    let outcomes = [FundingOutcomeProblem {
        resources,
        unit_fee_eligible: Some(&fees),
    }];
    let selected = optimize_funding_family(
        FundingFamilyOptimizationProblem {
            capacities: &capacities,
            outcomes: &outcomes,
            exposure_limit: u128::from(u64::MAX) + 1,
            resource_cursor: 0,
            fee_cursor: 0,
        },
        limits(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        selected.outcomes()[0].resources(),
        existing.resource_assignment()
    );
    assert_eq!(selected.outcomes()[0].resource_totals().source_debits(), [
        u64::MAX - 1,
        1
    ]);
    assert_eq!(selected.outcomes()[0].fee_debits(), [1, 0]);
    assert_eq!(selected.holds(), capacities);
    assert_eq!(selected.total_held(), u128::from(u64::MAX) + 1);
}

#[test]
fn duplicate_outcomes_preserve_allocations_and_shared_holds() {
    let capacities = [1, 1, 1];
    let alpha = [vec![false], vec![true], vec![false]];
    let beta = [vec![true], vec![true], vec![false]];
    let fees = [false, false, true];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &alpha,
            },
            unit_fee_eligible: Some(&fees),
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &beta,
            },
            unit_fee_eligible: Some(&fees),
        },
    ];
    for resource_cursor in 0..3 {
        for fee_cursor in 0..3 {
            let problem = FundingFamilyOptimizationProblem {
                capacities: &capacities,
                outcomes: &outcomes,
                exposure_limit: 2,
                resource_cursor,
                fee_cursor,
            };
            let baseline = optimize_funding_family(problem, limits()).unwrap().unwrap();
            for order in [vec![0, 0, 1], vec![0, 1, 1], vec![0, 0, 1, 1]] {
                let duplicated: Vec<_> = order.iter().map(|i| outcomes[*i]).collect();
                let duplicated_problem = FundingFamilyOptimizationProblem {
                    outcomes: &duplicated,
                    ..problem
                };
                let selected = optimize_funding_family(duplicated_problem, limits())
                    .unwrap()
                    .unwrap();
                for (outcome, original) in selected.outcomes().iter().zip(&order) {
                    assert_eq!(outcome, &baseline.outcomes()[*original]);
                }
                assert_eq!(selected.holds(), baseline.holds());
                assert_eq!(selected.total_held(), baseline.total_held());
                assert_oracle(duplicated_problem);
            }
        }
    }
}

#[test]
fn complete_family_priority_differs_from_an_unrestricted_outcome_projection() {
    let capacities = [2, 2, 2, 1];
    let first_edges = [vec![false], vec![true], vec![true], vec![false]];
    let second_edges = [vec![true], vec![true], vec![true], vec![true]];
    let first_fee = [false, false, false, true];
    let second_fee = [false, false, true, false];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &first_edges,
            },
            unit_fee_eligible: Some(&first_fee),
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &second_edges,
            },
            unit_fee_eligible: Some(&second_fee),
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 3,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let first_candidates = candidates(outcomes[0], &capacities);
    let second_candidates = candidates(outcomes[1], &capacities);
    let first_witness = Candidate {
        resources: vec![vec![0], vec![0], vec![1], vec![0]],
        draws: vec![0, 0, 1, 0],
        fee: vec![0, 0, 0, 1],
    };
    assert!(first_candidates.contains(&first_witness));
    for payer in 0..capacities.len() {
        let mut draws = vec![0; capacities.len()];
        draws[payer] = 1;
        let second_witness = Candidate {
            resources: draws.iter().map(|draw| vec![*draw]).collect(),
            draws,
            fee: vec![0, 0, 1, 0],
        };
        assert!(second_candidates.contains(&second_witness));
        let holds: Vec<_> = (0..capacities.len())
            .map(|source| {
                (first_witness.draws[source] + first_witness.fee[source])
                    .max(second_witness.draws[source] + second_witness.fee[source])
            })
            .collect();
        assert!(holds.iter().zip(capacities).all(|(hold, cap)| *hold <= cap));
        assert!(holds.iter().map(|hold| u128::from(*hold)).sum::<u128>() <= 3);
    }
    let independent =
        select_funding_with_unit_fee(outcomes[1].resources, &second_fee, 0, 0, limits().search)
            .unwrap()
            .unwrap();
    assert!(independent.resource_unrestricted());
    assert_eq!(independent.resource_totals().source_debits(), [1, 0, 0, 0]);
    let selected = optimize_funding_family(problem, limits()).unwrap().unwrap();
    assert_eq!(selected.outcomes()[0].resource_totals().source_debits(), [
        0, 1, 0, 0
    ]);
    assert_eq!(selected.outcomes()[1].resource_totals().source_debits(), [
        0, 1, 0, 0
    ]);
    assert_eq!(selected.outcomes()[0].fee_debits(), [0, 0, 0, 1]);
    assert_eq!(selected.outcomes()[1].fee_debits(), [0, 0, 1, 0]);
    assert_eq!(selected.holds(), [0, 1, 1, 1]);
    assert_eq!(selected.total_held(), 3);
    assert_ne!(
        selected.outcomes()[1].resource_totals().source_debits(),
        independent.resource_totals().source_debits()
    );
    assert_oracle(problem);
}

#[test]
fn single_outcome_family_preserves_existing_resource_and_fee_allocations() {
    let capacities = [3, 2, 1];
    let debit_limits = [2, 3, 1];
    let effective = [2, 2, 1];
    let eligible = vec![vec![true, true], vec![true, true], vec![true, true]];
    let fees = [true, false, true];
    for charged in [false, true] {
        for resource_cursor in 0..3 {
            for fee_cursor in 0..3 {
                let resources = FundingMinimaxProblem {
                    capacities: &debit_limits,
                    obligations: &[2, 1],
                    eligible: &eligible,
                };
                let outcomes = [FundingOutcomeProblem {
                    resources,
                    unit_fee_eligible: charged.then_some(fees.as_slice()),
                }];
                let problem = FundingFamilyOptimizationProblem {
                    capacities: &capacities,
                    outcomes: &outcomes,
                    exposure_limit: 4,
                    resource_cursor,
                    fee_cursor,
                };
                let selected = optimize_funding_family(problem, limits()).unwrap().unwrap();
                let effective_problem = FundingMinimaxProblem {
                    capacities: &effective,
                    ..resources
                };
                if charged {
                    let existing = select_funding_with_unit_fee(
                        effective_problem,
                        &fees,
                        resource_cursor,
                        fee_cursor,
                        limits().search,
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(
                        selected.outcomes()[0].resources(),
                        existing.resource_assignment()
                    );
                    assert_eq!(selected.outcomes()[0].fee_debits(), existing.fee().debits);
                } else {
                    let FundingPolicyResult::Selected(existing) = select_fixed_funding_policy(
                        effective_problem,
                        resource_cursor,
                        limits().search,
                    )
                    .unwrap() else {
                        panic!("fundable outcome");
                    };
                    assert_eq!(selected.outcomes()[0].resources(), existing.assignment());
                    assert_eq!(selected.outcomes()[0].fee_debits(), [0, 0, 0]);
                }
            }
        }
    }
}

type SmallFamily = (
    (usize, usize, usize),
    Vec<u64>,
    Vec<u64>,
    Vec<u64>,
    Vec<bool>,
    Vec<bool>,
    Vec<bool>,
    u128,
    usize,
    usize,
);

fn small_family() -> impl Strategy<Value = SmallFamily> {
    (1_usize..4, 1_usize..4, 0_usize..3).prop_flat_map(|(n, k, m)| {
        (
            Just((n, k, m)),
            prop::collection::vec(0_u64..3, n),
            prop::collection::vec(0_u64..4, k * n),
            prop::collection::vec(0_u64..3, k * m),
            prop::collection::vec(any::<bool>(), k * n * m),
            prop::collection::vec(any::<bool>(), k * n),
            prop::collection::vec(any::<bool>(), k),
            0_u128..7,
            0_usize..n,
            0_usize..n,
        )
    })
}

fn with_small_family(data: SmallFamily, check: impl FnOnce(FundingFamilyOptimizationProblem<'_>)) {
    let (
        (n, k, m),
        capacities,
        outcome_caps,
        amounts,
        edges,
        fees,
        charged,
        exposure_limit,
        resource_cursor,
        fee_cursor,
    ) = data;
    let eligibility: Vec<Vec<Vec<_>>> = (0..k)
        .map(|o| {
            (0..n)
                .map(|i| edges[(o * n + i) * m..(o * n + i + 1) * m].to_vec())
                .collect()
        })
        .collect();
    let outcomes: Vec<_> = (0..k)
        .map(|o| FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &outcome_caps[o * n..(o + 1) * n],
                obligations: &amounts[o * m..(o + 1) * m],
                eligible: &eligibility[o],
            },
            unit_fee_eligible: charged[o].then_some(&fees[o * n..(o + 1) * n]),
        })
        .collect();
    check(FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit,
        resource_cursor,
        fee_cursor,
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn family_optimizer_matches_complete_multi_outcome_oracle(data in small_family()) {
        with_small_family(data, assert_oracle);
    }
}

// Added by DR-131 (decision D-J): the early exit returns the result of the
// search loop alone, on every small family, with or without a charge above the
// exposure limit.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn family_optimizer_early_exit_matches_the_search_loop(data in small_family()) {
        with_small_family(data, |problem| {
            let (root, _) = validate(problem, limits()).expect("generated families are valid");
            let searched = search_funding_family(problem, root, limits())
                .expect("the search loop completes on a valid small family");
            let optimized = optimize_funding_family(problem, limits())
                .expect("the optimizer completes on a valid small family");
            assert_eq!(optimized, searched);
        });
    }
}

/// Added by DR-131 (decision D-K): times one funding search at the protocol
/// size limits. Each of the protocol's sources is eligible for every
/// obligation and for the unit fee. The obligation count grows by a factor of
/// 4 from 16 to the protocol's obligation limit. The policy rejects a case with
/// more than `funding_case.cells` cells before the search, so the points above
/// that product are beyond production. A point that takes more than 600
/// seconds ends the sweep. The test is a measurement, so it is ignored. Run it
/// alone with `--ignored --nocapture`, pinned to one core.
#[test]
#[ignore = "DR-131 decision D-K: a measurement that can take hours"]
fn funding_search_time_at_the_protocol_size_limits() {
    let protocol = models::rust::cost_protocol_limits::offered_funded_v6_limits();
    let n = protocol.funding_case.sources;
    let obligation_limit = protocol.funding_case.obligations;
    let limits = FundingFamilyLimits {
        search: FundingSearchLimits {
            source_cap: NonZeroUsize::new(n).expect("a positive source limit"),
            obligation_cap: NonZeroUsize::new(obligation_limit)
                .expect("a positive obligation limit"),
        },
        case_cap: NonZeroUsize::new(1).expect("one case"),
        search_scans: usize::MAX,
    };
    let capacities = vec![u64::MAX; n];
    let fee = vec![true; n];
    let mut m = 16;
    while m <= obligation_limit {
        let count = u64::try_from(m).expect("an obligation count fits in u64");
        let amount = u64::MAX / count;
        let obligations = vec![amount; m];
        let eligible = vec![vec![true; m]; n];
        let outcomes = [FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &obligations,
                eligible: &eligible,
            },
            unit_fee_eligible: Some(&fee),
        }];
        let charge = u128::from(amount) * u128::from(count) + 1;
        let problem = FundingFamilyOptimizationProblem {
            capacities: &capacities,
            outcomes: &outcomes,
            exposure_limit: charge,
            resource_cursor: 0,
            fee_cursor: 0,
        };
        let started = std::time::Instant::now();
        let selected = optimize_funding_family(problem, limits)
            .expect("the search completes")
            .expect("the search returns an allocation");
        let elapsed = started.elapsed();
        assert_eq!(selected.total_held(), charge);
        println!(
            "D-K sources={n} obligations={m} cells={} cell_limit={} charge={charge} seconds={:.3}",
            n * m,
            protocol.funding_case.cells,
            elapsed.as_secs_f64()
        );
        if elapsed > std::time::Duration::from_secs(600) {
            println!("D-K stopped after obligations={m}: the point took more than 600 seconds");
            break;
        }
        m *= 4;
    }
}

fn capped(search_scans: usize) -> FundingFamilyLimits {
    FundingFamilyLimits {
        search_scans,
        ..limits()
    }
}

fn counted(
    problem: FundingFamilyOptimizationProblem<'_>,
    limits: FundingFamilyLimits,
) -> (
    Result<Option<FundingFamilyAllocation>, FundingFamilyError>,
    usize,
) {
    let scope = funding_scan_scope(limits.search_scans);
    let result = optimize_within_budget(problem, limits);
    let remaining = scope.remaining().expect("the scope holds a budget");
    (result, limits.search_scans - remaining)
}

fn dense_point<T>(
    n: usize,
    m: usize,
    check: impl FnOnce(FundingFamilyOptimizationProblem<'_>) -> T,
) -> T {
    let capacities = vec![u64::MAX; n];
    let fee = vec![true; n];
    let count = u64::try_from(m).expect("an obligation count fits in u64");
    let amount = u64::MAX / count;
    let obligations = vec![amount; m];
    let eligible = vec![vec![true; m]; n];
    let outcomes = [FundingOutcomeProblem {
        resources: FundingMinimaxProblem {
            capacities: &capacities,
            obligations: &obligations,
            eligible: &eligible,
        },
        unit_fee_eligible: Some(&fee),
    }];
    check(FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: u128::from(amount) * u128::from(count) + 1,
        resource_cursor: 0,
        fee_cursor: 0,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn the_scan_cap_admits_exactly_the_searches_within_it(data in small_family()) {
        with_small_family(data, |problem| {
            let (expected, scans) = counted(problem, limits());
            let expected = expected.expect("the uncapped search completes");
            assert_eq!(
                optimize_funding_family(problem, capped(scans))
                    .expect("a cap equal to the scan count admits the search"),
                expected
            );
            if scans > 0 {
                assert!(matches!(
                    optimize_funding_family(problem, capped(scans - 1)),
                    Err(FundingFamilyError::Search(FundingSearchError::ScanCapExceeded))
                ));
            }
        });
    }
}

#[test]
fn the_scan_count_is_the_same_on_every_thread() {
    let count = || dense_point(6, 3, |problem| counted(problem, limits()).1);
    let expected = count();
    assert!(expected > 1, "the fixture must scan more than one edge");
    let counts: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8).map(|_| scope.spawn(count)).collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a counting thread joins"))
            .collect()
    });
    assert!(counts.iter().all(|scans| *scans == expected), "{counts:?}");
    dense_point(6, 3, |problem| {
        assert!(matches!(
            optimize_funding_family(problem, capped(expected - 1)),
            Err(FundingFamilyError::Search(
                FundingSearchError::ScanCapExceeded
            ))
        ));
        assert!(optimize_funding_family(problem, capped(expected))
            .expect("the cap admits the search")
            .is_some());
    });
}

#[test]
fn a_scan_scope_restores_the_enclosing_budget() {
    let outer = funding_scan_scope(7);
    dense_point(4, 2, |problem| {
        optimize_funding_family(problem, limits()).expect("the search completes")
    });
    assert_eq!(outer.remaining(), Some(7));
    drop(outer);
    let scope = funding_scan_scope(usize::MAX);
    drop(scope);
    dense_point(4, 2, |problem| {
        let (result, scans) = counted(problem, limits());
        assert!(result.expect("the search completes").is_some());
        assert!(scans > 0);
    });
}

#[test]
#[ignore = "a measurement for choosing the funding search scan cap"]
fn funding_search_scans_and_time_by_size() {
    for n in [2_usize, 8, 16, 32, 64] {
        for m in [1_usize, 4, 16, 64] {
            let limits = FundingFamilyLimits {
                search: FundingSearchLimits {
                    source_cap: NonZeroUsize::new(n).expect("a positive source count"),
                    obligation_cap: NonZeroUsize::new(m).expect("a positive obligation count"),
                },
                case_cap: NonZeroUsize::new(1).expect("one case"),
                search_scans: usize::MAX,
            };
            let started = std::time::Instant::now();
            let scans = dense_point(n, m, |problem| {
                let (result, scans) = counted(problem, limits);
                assert!(result.expect("the search completes").is_some());
                scans
            });
            let seconds = started.elapsed().as_secs_f64();
            println!(
                "CAP sources={n} obligations={m} scans={scans} seconds={seconds:.4} \
                 nanoseconds_per_scan={:.2}",
                seconds * 1e9 / scans as f64
            );
        }
    }
}
