use super::*;

pub fn monetary_funding_signatures_with_host_work(
    funding: &AuthorityEvent<[u8; 32]>,
    presentations: &[CostSignature],
    host_work: Option<&HostWorkBudget>,
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    let discovered = authority_funding_signatures_for_events_with_host_work(
        std::slice::from_ref(funding),
        &[],
        presentations,
        host_work,
    )?;
    let mut authorized = ResourceMultiset::default();
    let mut atom_signatures = BTreeMap::new();
    for atom in event_atoms(funding)? {
        add_signature_atoms(&mut authorized, &mut atom_signatures, &atom, 1)?;
    }
    let mut eligible = BTreeMap::new();
    for (key, signature) in discovered {
        let mut required = ResourceMultiset::default();
        add_signature_atoms(&mut required, &mut atom_signatures, &signature, 1)?;
        if !required.0.is_empty()
            && required
                .0
                .iter()
                .all(|(atom, count)| *count <= authorized.get(atom))
        {
            eligible.insert(key, signature);
        }
    }
    Ok(eligible)
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use proptest::prelude::*;

    use super::*;

    fn atom(index: u8) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Ground(vec![index])),
        }
    }

    fn compound(indices: &[u8]) -> CostSignature {
        signature_from_atoms(&indices.iter().map(|index| atom(*index)).collect::<Vec<_>>()).unwrap()
    }

    fn funding(indices: &[u8]) -> AuthorityEvent<[u8; 32]> {
        let signature = compound(indices);
        let authority = canonical_authority(&CostAuthority {
            regions: vec![cost_region(&signature, b"monetary eligibility", 0).unwrap()],
        })
        .unwrap();
        AuthorityEvent {
            event_id: [7; 32],
            debit: authority_demand(&authority).unwrap(),
            authority,
        }
    }

    #[test]
    fn fee_eligibility_excludes_unrelated_and_repeated_authority() {
        let presentations = vec![
            compound(&[1, 2]),
            compound(&[1, 1]),
            compound(&[1, 4]),
            atom(4),
            compound(&[]),
        ];
        let eligible =
            monetary_funding_signatures_with_host_work(&funding(&[1, 2, 3]), &presentations, None)
                .unwrap();
        for indices in [vec![1], vec![2], vec![3], vec![1, 2], vec![1, 2, 3]] {
            assert!(eligible.contains_key(
                &cost_signature_to_sig(&compound(&indices))
                    .unwrap()
                    .lane_hash()
            ));
        }
        for indices in [vec![1, 1], vec![1, 4], vec![4], vec![]] {
            assert!(!eligible.contains_key(
                &cost_signature_to_sig(&compound(&indices))
                    .unwrap()
                    .lane_hash()
            ));
        }
    }

    #[test]
    fn fee_eligibility_preserves_multiplicity_when_it_is_authorized() {
        let eligible = monetary_funding_signatures_with_host_work(
            &funding(&[1, 1, 2]),
            &[compound(&[1, 1])],
            None,
        )
        .unwrap();
        assert!(eligible.contains_key(
            &cost_signature_to_sig(&compound(&[1, 1]))
                .unwrap()
                .lane_hash()
        ));
    }

    #[test]
    fn fee_eligibility_rejects_exhausted_host_budget_before_discovery() {
        let limits = HostWorkLimits::uniform(HostWorkLimit::new(0));
        let budget = HostWorkBudget::new(limits);
        assert!(
            monetary_funding_signatures_with_host_work(&funding(&[1, 2]), &[], Some(&budget))
                .is_err()
        );
    }

    #[test]
    fn fee_eligibility_sixty_four_signers_include_the_joint_purse() {
        use std::num::NonZeroUsize;

        use crate::rust::interpreter::accounting::monetary_allocation::{
            MonetaryAllocationError, MonetaryCohort, MonetaryCohortError,
        };

        let selected: Vec<_> = (1..=64).collect();
        let eligible =
            monetary_funding_signatures_with_host_work(&funding(&selected), &[], None).unwrap();
        assert_eq!(eligible.len(), 65);
        let mut inventory = AuthorityPhysicalInventory::default();
        for lane in eligible.keys() {
            inventory.insert_balance_lane(*lane, *lane, 1).unwrap();
        }
        assert_eq!(
            MonetaryCohort::from_inventory(&eligible, &inventory, NonZeroUsize::new(64).unwrap()),
            Err(MonetaryCohortError::Allocation(
                MonetaryAllocationError::TooManyPayers
            ))
        );
        let cohort =
            MonetaryCohort::from_inventory(&eligible, &inventory, NonZeroUsize::new(65).unwrap())
                .unwrap();
        let plan = cohort.allocate(&inventory.balances, 65, 0).unwrap();
        assert_eq!(plan.settlement.custody_debit, inventory.balances);
        assert_eq!(plan.settlement.logical_debit.0.len(), 65);
        assert_eq!(plan.next_cursor, 0);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn fee_eligibility_matches_atom_multiset_containment(
            selected in prop::collection::vec(1_u8..8, 1..17),
            candidate in prop::collection::vec(1_u8..10, 0..17),
        ) {
            let presentation = compound(&candidate);
            let mut presentations = vec![presentation.clone(), atom(10)];
            let event = funding(&selected);
            let eligible = monetary_funding_signatures_with_host_work(&event, &presentations, None).unwrap();
            let expected = !candidate.is_empty() && (1_u8..10).all(|key| {
                candidate.iter().filter(|value| **value == key).count() <= selected.iter().filter(|value| **value == key).count()
            });
            let key = cost_signature_to_sig(&presentation).unwrap().lane_hash();
            prop_assert_eq!(eligible.contains_key(&key), expected);
            presentations.reverse();
            presentations.push(presentation);
            prop_assert_eq!(eligible, monetary_funding_signatures_with_host_work(&event, &presentations, None).unwrap());
        }
    }
}
