use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
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
    }
}

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
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
    let actual = optimize_funding_family(problem, limits(), &work()).unwrap();
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
    let selected = optimize_funding_family(problem, limits(), &work())
        .unwrap()
        .unwrap();
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
        let selected = optimize_funding_family(problem, limits(), &work())
            .unwrap()
            .unwrap();
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
        optimize_funding_family(problem, limits(), &work())
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
        optimize_funding_family(problem, limits(), &work()),
        Err(FundingFamilyError::Search(
            FundingSearchError::InvalidProblem(FundingAssignmentError::InvalidDimensions)
        ))
    ));
    let empty_budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        optimize_funding_family(
            FundingFamilyOptimizationProblem {
                outcomes: &[valid],
                ..problem
            },
            limits(),
            &empty_budget
        ),
        Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
    ));
}

#[test]
fn family_optimizer_does_not_publish_an_incumbent_on_late_exhaustion() {
    let capacities = [1, 1];
    let only_b = vec![vec![false], vec![true]];
    let either = vec![vec![true], vec![true]];
    let outcomes = [
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &only_b,
            },
            unit_fee_eligible: None,
        },
        FundingOutcomeProblem {
            resources: FundingMinimaxProblem {
                capacities: &capacities,
                obligations: &[1],
                eligible: &either,
            },
            unit_fee_eligible: None,
        },
    ];
    let problem = FundingFamilyOptimizationProblem {
        capacities: &capacities,
        outcomes: &outcomes,
        exposure_limit: 1,
        resource_cursor: 0,
        fee_cursor: 0,
    };
    let complete = work();
    let result = optimize_funding_family(problem, limits(), &complete)
        .unwrap()
        .unwrap();
    assert_eq!(result.holds(), [0, 1]);
    let used = complete.usage(HostWorkDimension::SearchCandidates).get();
    let mut capped = complete.limits();
    capped.set(
        HostWorkDimension::SearchCandidates,
        HostWorkLimit::new(used - 1),
    );
    assert!(matches!(
        optimize_funding_family(problem, limits(), &HostWorkBudget::new(capped)),
        Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
    ));
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
    let selected = optimize_funding_family(problem, limits(), &work())
        .unwrap()
        .unwrap();
    let proposed = [FundingOutcomeProposal {
        resources: selected.outcomes()[0].resources(),
        fee_debits: selected.outcomes()[0].fee_debits(),
    }];
    let proposal = FundingFamilyProposal {
        outcomes: &proposed,
        holds: selected.holds(),
    };
    assert!(verify_funding_family_allocation(problem, proposal, limits(), &work()).unwrap());
    assert!(!verify_funding_family_allocation(
        problem,
        FundingFamilyProposal {
            holds: &[0, 2],
            ..proposal
        },
        limits(),
        &work()
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
        &work()
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
        &work()
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
    let existing = select_funding_with_unit_fee(resources, &fees, 0, 0, limits().search, &work())
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
        &work(),
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
            let baseline = optimize_funding_family(problem, limits(), &work())
                .unwrap()
                .unwrap();
            for order in [vec![0, 0, 1], vec![0, 1, 1], vec![0, 0, 1, 1]] {
                let duplicated: Vec<_> = order.iter().map(|i| outcomes[*i]).collect();
                let duplicated_problem = FundingFamilyOptimizationProblem {
                    outcomes: &duplicated,
                    ..problem
                };
                let selected = optimize_funding_family(duplicated_problem, limits(), &work())
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
    let independent = select_funding_with_unit_fee(
        outcomes[1].resources,
        &second_fee,
        0,
        0,
        limits().search,
        &work(),
    )
    .unwrap()
    .unwrap();
    assert!(independent.resource_unrestricted());
    assert_eq!(independent.resource_totals().source_debits(), [1, 0, 0, 0]);
    let selected = optimize_funding_family(problem, limits(), &work())
        .unwrap()
        .unwrap();
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
                let selected = optimize_funding_family(problem, limits(), &work())
                    .unwrap()
                    .unwrap();
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
                        &work(),
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
                        &work(),
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn family_optimizer_matches_complete_multi_outcome_oracle(
        data in (1_usize..4, 1_usize..4, 0_usize..3).prop_flat_map(|(n,k,m)| (
            Just((n,k,m)), prop::collection::vec(0_u64..3,n), prop::collection::vec(0_u64..4,k*n),
            prop::collection::vec(0_u64..3,k*m), prop::collection::vec(any::<bool>(),k*n*m),
            prop::collection::vec(any::<bool>(),k*n), prop::collection::vec(any::<bool>(),k),
            0_u128..7, 0_usize..n, 0_usize..n,
        ))
    ) {
        let ((n,k,m), capacities, outcome_caps, amounts, edges, fees, charged, exposure_limit, resource_cursor, fee_cursor) = data;
        let eligibility: Vec<Vec<Vec<_>>> = (0..k).map(|o| (0..n).map(|i| edges[(o*n+i)*m..(o*n+i+1)*m].to_vec()).collect()).collect();
        let outcomes: Vec<_> = (0..k).map(|o| FundingOutcomeProblem {
            resources: FundingMinimaxProblem { capacities: &outcome_caps[o*n..(o+1)*n], obligations: &amounts[o*m..(o+1)*m], eligible: &eligibility[o] },
            unit_fee_eligible: charged[o].then_some(&fees[o*n..(o+1)*n]),
        }).collect();
        assert_oracle(FundingFamilyOptimizationProblem { capacities: &capacities, outcomes: &outcomes, exposure_limit, resource_cursor, fee_cursor });
    }
}
