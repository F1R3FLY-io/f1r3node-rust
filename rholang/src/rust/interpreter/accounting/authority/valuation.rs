use super::{
    cost_signature_to_sig, event_atoms, AuthorityError, AuthorityEvent, CostSignature,
    ResourceMultiset,
};

pub(super) fn demand_from_atoms(
    atoms: &[CostSignature],
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    atoms
        .iter()
        .try_fold(ResourceMultiset::default(), |allocation, atom| {
            allocation.checked_add(&ResourceMultiset::singleton(
                cost_signature_to_sig(atom)?.lane_hash(),
                1,
            ))
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityResourceDemand {
    event: AuthorityEvent<[u8; 32]>,
    atoms: ResourceMultiset<[u8; 32]>,
}

impl AuthorityResourceDemand {
    pub fn from_event(event: &AuthorityEvent<[u8; 32]>) -> Result<Self, AuthorityError> {
        let atoms = demand_from_atoms(&event_atoms(event)?)?;
        Ok(Self {
            event: event.clone(),
            atoms,
        })
    }

    pub fn event_id(&self) -> &[u8; 32] { &self.event.event_id }

    pub fn event(&self) -> &AuthorityEvent<[u8; 32]> { &self.event }

    pub fn atoms(&self) -> &ResourceMultiset<[u8; 32]> { &self.atoms }

    pub fn value(&self, unit_price: u64) -> Result<u64, AuthorityError> {
        self.atoms.0.values().try_fold(0_u64, |value, amount| {
            let component = amount
                .checked_mul(unit_price)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
            value
                .checked_add(component)
                .ok_or(AuthorityError::ArithmeticOverflow)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::rhoapi::{CostAuthority, CostSignature};
    use proptest::prelude::*;

    use super::super::{
        authority_demand, authority_funding_options, canonical_authority, compound_cost_signatures,
        cost_region, sig_to_cost_signature,
    };
    use super::*;
    use crate::rust::interpreter::accounting::Sig;

    fn event(signatures: &[CostSignature]) -> AuthorityEvent<[u8; 32]> {
        let authority = canonical_authority(&CostAuthority {
            regions: signatures
                .iter()
                .enumerate()
                .map(|(index, signature)| {
                    cost_region(signature, b"valuation test", u32::try_from(index).unwrap())
                        .unwrap()
                })
                .collect(),
        })
        .unwrap();
        AuthorityEvent {
            event_id: [91; 32],
            debit: authority_demand(&authority).unwrap(),
            authority,
        }
    }

    fn atom(id: u8) -> CostSignature { sig_to_cost_signature(&Sig::Ground(vec![id])).unwrap() }

    #[test]
    fn regrouping_preserves_value_and_authority_occurrences() {
        let a = atom(1);
        let b = atom(2);
        let split = AuthorityResourceDemand::from_event(&event(&[a.clone(), b.clone()])).unwrap();
        let combined =
            AuthorityResourceDemand::from_event(&event(&[
                compound_cost_signatures(&a, &b).unwrap()
            ]))
            .unwrap();
        assert_eq!(split.atoms(), combined.atoms());
        assert_eq!(split.value(7), combined.value(7));
        assert_ne!(split.event(), combined.event());
        assert_eq!(split.value(7).unwrap(), 14);
        assert_eq!(split.event_id(), &[91; 32]);
        let repeated = AuthorityResourceDemand::from_event(&event(&[a.clone(), a])).unwrap();
        assert_eq!(repeated.atoms().0.len(), 1);
        assert_eq!(repeated.atoms().0.values().copied().sum::<u64>(), 2);
        assert_eq!(repeated.value(7).unwrap(), 14);
    }

    #[test]
    fn altered_event_debit_cannot_supply_a_pricing_demand() {
        let mut forged = event(&[atom(1)]);
        forged.debit = ResourceMultiset::default();
        assert_eq!(
            AuthorityResourceDemand::from_event(&forged),
            Err(AuthorityError::EventDebitMismatch)
        );
    }

    #[test]
    fn units_and_values_do_not_replace_authority_identity() {
        let a = AuthorityResourceDemand::from_event(&event(&[atom(1)])).unwrap();
        let b = AuthorityResourceDemand::from_event(&event(&[atom(2)])).unwrap();
        assert_eq!(a.value(3), b.value(3));
        assert_ne!(a.atoms(), b.atoms());
        let unit = sig_to_cost_signature(&Sig::Unit).unwrap();
        let empty = AuthorityResourceDemand::from_event(&event(&[unit])).unwrap();
        assert!(empty.atoms().0.is_empty());
        assert_eq!(empty.value(u64::MAX).unwrap(), 0);
    }

    #[test]
    fn equal_value_and_atoms_do_not_replace_region_identity() {
        let signature = atom(1);
        let original = event(&[signature.clone()]);
        let mut relocated = original.clone();
        relocated.authority = canonical_authority(&CostAuthority {
            regions: vec![cost_region(&signature, b"different location", 0).unwrap()],
        })
        .unwrap();
        relocated.debit = authority_demand(&relocated.authority).unwrap();
        let original_demand = AuthorityResourceDemand::from_event(&original).unwrap();
        let relocated_demand = AuthorityResourceDemand::from_event(&relocated).unwrap();
        assert_eq!(original_demand.atoms(), relocated_demand.atoms());
        assert_eq!(original_demand.value(7), relocated_demand.value(7));
        assert_ne!(original_demand, relocated_demand);
    }

    #[test]
    fn demand_retains_an_immutable_validated_event_snapshot() {
        let mut original = event(&[atom(1), atom(1), atom(2)]);
        let captured = original.clone();
        let demand = AuthorityResourceDemand::from_event(&original).unwrap();
        original.event_id[0] ^= 1;
        original.authority.regions.clear();
        original.debit = ResourceMultiset::default();
        assert_eq!(demand.event(), &captured);
        assert_eq!(demand.event_id(), &captured.event_id);
        assert_eq!(demand.event().authority.regions.len(), 3);
        assert_eq!(demand.atoms().0.values().sum::<u64>(), 3);
        assert!(demand.event().verify_authority().is_ok());
        assert_ne!(demand.event(), &original);
    }

    #[test]
    fn malformed_or_noncanonical_regions_cannot_enter_a_demand() {
        let mut invalid = event(&[atom(1)]);
        invalid.authority.regions[0].instance_id.pop();
        assert_eq!(
            AuthorityResourceDemand::from_event(&invalid),
            Err(AuthorityError::InvalidRegionIdentity)
        );
        let mut duplicated = event(&[atom(1)]);
        duplicated
            .authority
            .regions
            .push(duplicated.authority.regions[0].clone());
        assert_eq!(
            AuthorityResourceDemand::from_event(&duplicated),
            Err(AuthorityError::NonCanonicalAuthority)
        );
    }

    #[test]
    fn pricing_overflow_rejects_both_component_and_aggregate_overflow() {
        let a = atom(1);
        let b = atom(2);
        assert_eq!(
            AuthorityResourceDemand::from_event(&event(&[a.clone()]))
                .unwrap()
                .value(u64::MAX)
                .unwrap(),
            u64::MAX
        );
        assert_eq!(
            AuthorityResourceDemand::from_event(&event(&[a.clone(), a.clone()]))
                .unwrap()
                .value(u64::MAX),
            Err(AuthorityError::ArithmeticOverflow)
        );
        assert_eq!(
            AuthorityResourceDemand::from_event(&event(&[a, b]))
                .unwrap()
                .value(u64::MAX),
            Err(AuthorityError::ArithmeticOverflow)
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn native_demand_matches_occurrence_oracle(
            ids in proptest::collection::vec(0_u8..8, 1..65), price in 0_u64..1_000_000
        ) {
            let signatures: Vec<_> = ids.iter().copied().map(atom).collect();
            let flat = event(&signatures);
            let mut compound = signatures[0].clone();
            for next in &signatures[1..] { compound = compound_cost_signatures(&compound, next).unwrap(); }
            let grouped = event(&[compound]);
            let split_demand = AuthorityResourceDemand::from_event(&flat).unwrap();
            let grouped_demand = AuthorityResourceDemand::from_event(&grouped).unwrap();
            let mut expected = BTreeMap::<[u8; 32], u64>::new();
            for id in &ids {
                let key = cost_signature_to_sig(&atom(*id)).unwrap().lane_hash();
                *expected.entry(key).or_default() += 1;
            }
            prop_assert_eq!(&split_demand.atoms().0, &expected);
            prop_assert_eq!(split_demand.atoms(), grouped_demand.atoms());
            prop_assert_eq!(split_demand.event(), &flat);
            prop_assert_eq!(grouped_demand.event(), &grouped);
            if ids.len() > 1 {
                prop_assert_ne!(&split_demand, &grouped_demand);
            }
            prop_assert_eq!(split_demand.value(price).unwrap(), price * ids.len() as u64);
            prop_assert!(authority_funding_options(&flat).unwrap().contains(split_demand.atoms()));
            prop_assert!(authority_funding_options(&grouped).unwrap().contains(grouped_demand.atoms()));
        }

        #[test]
        fn region_mutation_preserves_value_but_not_the_witness(id in any::<u8>(), byte in 0_usize..32) {
            let original = event(&[atom(id)]);
            let mut changed = original.clone();
            changed.authority.regions[0].instance_id[byte] ^= 1;
            let before = AuthorityResourceDemand::from_event(&original).unwrap();
            let after = AuthorityResourceDemand::from_event(&changed).unwrap();
            prop_assert_eq!(before.atoms(), after.atoms());
            prop_assert_eq!(before.value(17), after.value(17));
            prop_assert_eq!(before.event(), &original);
            prop_assert_eq!(after.event(), &changed);
            prop_assert_ne!(before, after);
        }
    }
}
