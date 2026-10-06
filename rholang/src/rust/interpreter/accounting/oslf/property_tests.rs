use proptest::prelude::*;

use super::*;

fn amount() -> impl Strategy<Value = u64> {
    prop_oneof![Just(0), Just(1), Just(2), Just(u64::MAX), any::<u64>()]
}

fn state(supply: u64, demand: u64, exact: bool) -> ResourceObservation<u8> {
    let supply = ResourceMultiset::singleton(0, supply);
    let demand = ResourceMultiset::singleton(0, demand);
    if exact {
        ResourceObservation::exact(supply, demand)
    } else {
        ResourceObservation::upper_bound(supply, demand)
    }
}

fn spend(grade: ResourceMultiset<u8>) -> Formula<u8> {
    Formula::Spend {
        grade,
        continuation: Box::new(Formula::True),
    }
}

#[test]
fn zero_only_stored_grade_cannot_witness_an_interaction() {
    for exact in [false, true] {
        let observation = state(0, 0, exact);
        for count in [1_u8, 2, 3, 32, 255] {
            let grade = ResourceMultiset((0..count).map(|key| (key, 0)).collect());
            assert_eq!(evaluate(&observation, &spend(grade)), Verdict::Unsatisfied);
        }
    }
}

#[test]
fn zero_only_exact_grade_cannot_witness_an_interaction() {
    let grade = ResourceMultiset([(0, 0)].into_iter().collect());
    assert_eq!(
        evaluate(&state(0, 0, true), &spend(grade)),
        Verdict::Unsatisfied
    );
}

#[test]
fn zero_padding_does_not_claim_an_unrelated_location() {
    let resources = ResourceMultiset([(0, 1), (1, 1)].into_iter().collect());
    let observation = ResourceObservation::exact(resources.clone(), resources);
    let grade = ResourceMultiset([(0, 1), (1, 0)].into_iter().collect());
    let formula = Formula::Spatial(Box::new(spend(grade)), Box::new(Formula::relevant(1)));
    assert_eq!(evaluate(&observation, &formula), Verdict::Satisfied);
}

proptest! {
    #[test]
    fn same_authority_at_another_location_cannot_fund_a_spend(
        authority in any::<u8>(),
        requested in 1_u64..=u32::MAX as u64,
        excess in 0_u64..=u32::MAX as u64,
    ) {
        let here = (0_u8, authority);
        let there = (1_u8, authority);
        let funded = ResourceObservation::exact(
            ResourceMultiset::singleton(here, requested + excess),
            ResourceMultiset::singleton(here, requested),
        );
        let formula = Formula::Located {
            surface: here,
            body: Box::new(Formula::Spend {
                grade: ResourceMultiset::singleton(here, requested),
                continuation: Box::new(Formula::True),
            }),
        };
        prop_assert_eq!(evaluate(&funded, &formula), Verdict::Satisfied);
        let misplaced = ResourceObservation::exact(
            ResourceMultiset::singleton(there, requested + excess),
            ResourceMultiset::singleton(here, requested),
        );
        prop_assert_eq!(evaluate(&misplaced, &formula), Verdict::Unsatisfied);
        let foreign_body = Formula::Located {
            surface: here,
            body: Box::new(Formula::Available { surface: there, amount: requested }),
        };
        prop_assert_eq!(evaluate(&misplaced, &foreign_body), Verdict::Unsatisfied);
        prop_assert_eq!(
            evaluate(&misplaced, &Formula::Available { surface: there, amount: requested }),
            Verdict::Satisfied,
        );
    }

    #[test]
    fn located_positive_spends_compose_with_exact_residuals(
        entries in prop::collection::vec(
            (any::<u8>(), 1_u64..=u32::MAX as u64,
             0_u64..=u32::MAX as u64, 0_u64..=u32::MAX as u64), 1..33),
    ) {
        let mut available = ResourceMultiset::default();
        let mut demand = ResourceMultiset::default();
        let mut grade = ResourceMultiset::default();
        let mut formulas = Vec::new();
        for (location, &(authority, charge, supply_left, demand_left)) in entries.iter().enumerate() {
            let surface = (location, authority);
            available.0.insert(surface, charge + supply_left);
            demand.0.insert(surface, charge + demand_left);
            grade.0.insert(surface, charge);
            formulas.push(Formula::Located {
                surface,
                body: Box::new(Formula::Spend {
                    grade: ResourceMultiset::singleton(surface, charge),
                    continuation: Box::new(Formula::All(vec![
                        Formula::Available { surface, amount: supply_left },
                        Formula::Not(Box::new(Formula::Available { surface, amount: supply_left + 1 })),
                        Formula::Required { surface, amount: demand_left },
                        Formula::Not(Box::new(Formula::Required { surface, amount: demand_left + 1 })),
                    ])),
                }),
            });
        }
        let observation = ResourceObservation::exact(available.clone(), demand.clone());
        let forward = formulas.iter().cloned().fold(Formula::True, |left, right| {
            Formula::Spatial(Box::new(left), Box::new(right))
        });
        let reverse = formulas.iter().rev().cloned().fold(Formula::True, |left, right| {
            Formula::Spatial(Box::new(left), Box::new(right))
        });
        prop_assert_eq!(evaluate(&observation, &forward), Verdict::Satisfied);
        prop_assert_eq!(evaluate(&observation, &reverse), Verdict::Satisfied);
        let overlapping = Formula::Spatial(Box::new(formulas[0].clone()), Box::new(formulas[0].clone()));
        prop_assert_eq!(evaluate(&observation, &overlapping), Verdict::Unsatisfied);
        let mut sequential = observation.clone();
        for (location, &(authority, charge, _, _)) in entries.iter().enumerate() {
            sequential = sequential.after_spend(&ResourceMultiset::singleton((location, authority), charge)).unwrap();
        }
        let mut reversed = observation.clone();
        for (location, &(authority, charge, _, _)) in entries.iter().enumerate().rev() {
            reversed = reversed.after_spend(&ResourceMultiset::singleton((location, authority), charge)).unwrap();
        }
        prop_assert_eq!(&sequential, &reversed);
        prop_assert_eq!(observation.after_spend(&grade), Some(sequential.clone()));
        for (location, &(authority, _, supply_left, demand_left)) in entries.iter().enumerate() {
            let surface = (location, authority);
            prop_assert_eq!(sequential.available.get(&surface), supply_left);
            let DemandKnowledge::Exact(remaining) = &sequential.demand else {
                prop_assert!(false, "exact demand became an upper bound");
                return Ok(());
            };
            prop_assert_eq!(remaining.get(&surface), demand_left);
        }
        prop_assert_eq!(
            evaluate(&ResourceObservation::upper_bound(available, demand), &forward),
            Verdict::Indeterminate,
        );
    }

    #[test]
    fn zero_padding_preserves_spend_and_spatial_verdicts(
        entries in prop::collection::btree_map(any::<u8>(), amount(), 0..65),
        other in any::<u8>(), exact in any::<bool>(),
    ) {
        let mut padded = entries.clone();
        padded.entry(other).or_insert(0);
        let canonical = ResourceMultiset(padded.iter().filter(|(_, amount)| **amount > 0).map(|(&key, &amount)| (key, amount)).collect());
        let resources = ResourceMultiset(padded.keys().map(|&key| (key, u64::MAX)).collect());
        let observation = if exact {
            ResourceObservation::exact(resources.clone(), resources)
        } else {
            ResourceObservation::upper_bound(resources.clone(), resources)
        };
        let padded = spend(ResourceMultiset(padded));
        let canonical = spend(canonical);
        prop_assert_eq!(evaluate(&observation, &padded), evaluate(&observation, &canonical));
        let pad_pair = Formula::Spatial(Box::new(padded), Box::new(Formula::relevant(other)));
        let canonical_pair = Formula::Spatial(Box::new(canonical), Box::new(Formula::relevant(other)));
        prop_assert_eq!(evaluate(&observation, &pad_pair), evaluate(&observation, &canonical_pair));
    }

    #[test]
    fn exact_atoms_match_the_proven_inequalities(
        supply in amount(), demand in amount(), requested in amount(),
    ) {
        let observation = state(supply, demand, true);
        prop_assert_eq!(
            satisfies(&observation, &Formula::Available { surface: 0, amount: requested }),
            requested <= supply,
        );
        prop_assert_eq!(
            satisfies(&observation, &Formula::Required { surface: 0, amount: requested }),
            requested <= demand,
        );
        prop_assert_eq!(
            satisfies(&observation, &spend(ResourceMultiset::singleton(0, requested))),
            requested > 0 && requested <= supply && requested <= demand,
        );
        prop_assert_eq!(satisfies(&observation, &Formula::Sufficient { surface: 0 }), demand <= supply);
    }

    #[test]
    fn conservative_bounds_do_not_become_event_evidence(
        supply in amount(), upper in amount(), requested in amount(),
    ) {
        let observation = state(supply, upper, false);
        let expected_required = if requested == 0 {
            Verdict::Satisfied
        } else if requested > upper {
            Verdict::Unsatisfied
        } else {
            Verdict::Indeterminate
        };
        prop_assert_eq!(evaluate(&observation, &Formula::Required { surface: 0, amount: requested }), expected_required);
        let expected_spend = if requested == 0 || requested > supply || requested > upper {
            Verdict::Unsatisfied
        } else {
            Verdict::Indeterminate
        };
        prop_assert_eq!(evaluate(&observation, &spend(ResourceMultiset::singleton(0, requested))), expected_spend);
    }

    #[test]
    fn usage_disciplines_match_the_model(supply in amount(), demand in amount()) {
        let observation = state(supply, demand, true);
        prop_assert_eq!(satisfies(&observation, &Formula::linear(0)), supply >= 1 && demand == 1);
        prop_assert_eq!(satisfies(&observation, &Formula::relevant(0)), supply >= 1 && demand >= 1);
        prop_assert!(satisfies(&observation, &Formula::copyable(0)));
        prop_assert!(satisfies(&state(supply, demand, false), &Formula::copyable(0)));
    }

    #[test]
    fn sufficiency_composes_for_many_locations(
        entries in prop::collection::vec((amount(), amount()), 0..65),
    ) {
        let available = ResourceMultiset(entries.iter().enumerate().map(|(key, &(supply, _))| (key, supply)).collect());
        let demand = ResourceMultiset(entries.iter().enumerate().map(|(key, &(_, demand))| (key, demand)).collect());
        let observation = ResourceObservation::upper_bound(available, demand);
        let formulas = Formula::All((0..entries.len()).map(|surface| Formula::Sufficient { surface }).collect());
        let locally_funded = entries.iter().all(|(supply, demand)| demand <= supply);
        prop_assert_eq!(satisfies(&observation, &formulas), locally_funded);
        if locally_funded {
            let total_supply: u128 = entries.iter().map(|&(supply, _)| u128::from(supply)).sum();
            let total_demand: u128 = entries.iter().map(|&(_, demand)| u128::from(demand)).sum();
            prop_assert!(total_demand <= total_supply);
        }
    }

    #[test]
    fn spending_preserves_other_locations_and_exact_residuals(
        supply in amount(), demand in amount(),
        other_supply in amount(), other_demand in amount(),
        requests in prop::collection::vec(amount(), 0..65),
    ) {
        let mut current = state(supply, demand, true);
        current.available.0.insert(1, other_supply);
        if let DemandKnowledge::Exact(resources) = &mut current.demand {
            resources.0.insert(1, other_demand);
        }
        let mut residual_supply = supply;
        let mut residual_demand = demand;
        for requested in requests {
            let grade = ResourceMultiset::singleton(0, requested);
            let permitted = requested > 0 && requested <= residual_supply && requested <= residual_demand;
            prop_assert_eq!(satisfies(&current, &spend(grade.clone())), permitted);
            if permitted {
                current = current.after_spend(&grade).expect("a proved spend has an exact residual");
                residual_supply -= requested;
                residual_demand -= requested;
            }
            prop_assert_eq!(current.available.get(&0), residual_supply);
            prop_assert_eq!(current.available.get(&1), other_supply);
            let DemandKnowledge::Exact(resources) = &current.demand else {
                unreachable!("exact observations remain exact");
            };
            prop_assert_eq!(resources.get(&0), residual_demand);
            prop_assert_eq!(resources.get(&1), other_demand);
        }
    }
}
