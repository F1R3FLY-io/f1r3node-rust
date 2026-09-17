use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_bounds::PhloBoundsError;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, check_phlo_funding_terms, PhloControlsError, PhloFundingTerms,
};

fn limits() -> PhloControlsLimits {
    PhloControlsLimits {
        wire: PhloWireLimits {
            total_bytes: 1_048_576,
            field_bytes: 524_288,
        },
        owners: 4096,
        schedules: 64,
        total_classes: 256,
    }
}

fn descriptor(owners: Vec<u64>, limit: u64, ceiling: u64, price: u64) -> PhloControlsV1<'static> {
    PhloControlsV1 {
        limit,
        price_ceiling: ceiling,
        required_owner_ceilings: owners,
        permitted_schedules: vec![PhloScheduleV1 {
            protocol_version: 6,
            network: b"test network",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"smallest REV unit",
            decimal_scale: 8,
            classes: vec![PhloResourceClassV1 {
                identity: b"COMM",
                measurement_unit: b"authority demand",
                measurement_rule: [1; 32],
                valuation_rule: [2; 32],
                weight: 3,
            }],
            actual_price: price,
            compatibility_rule: [4; 32],
        }],
    }
}

#[test]
fn phlo_controls_wire_binding_uses_all_terms_and_full_schedule_digest() {
    let original = descriptor(vec![3, 5, 8], 10, 3, 2);
    let wire = original.encode(limits()).unwrap();
    let decoded = PhloControlsV1::decode(&wire, limits()).unwrap();
    let binding = PhloControlsBinding::new(&decoded, limits()).unwrap();
    assert_eq!(binding.descriptor(), &original);
    let view = binding.view().unwrap();
    let terms = view.terms();
    assert_eq!(terms.required_owner_ceilings, &[3, 5, 8]);
    let selected = terms.permitted_schedules[0];
    assert_eq!(selected.weights, &[3]);
    assert_eq!(
        selected.commitment,
        original.permitted_schedules[0]
            .digest(limits().schedule(1))
            .unwrap()
    );
    let checked =
        check_phlo_controls(selected.environment, 0, i64::MAX as u64, terms, selected, 6).unwrap();
    assert_eq!(checked.terms().limit, 10);
    assert_eq!(checked.terms().price_ceiling, 3);
    assert_eq!(checked.resource_bound(), 6);
    let right = PhloFundingTerms {
        required_owner_ceilings: &[3, 5, 8],
        asset: b"REV",
        schedule_commitment: selected.commitment,
    };
    assert_eq!(
        check_phlo_funding_terms(checked, right).unwrap().controls(),
        checked
    );
    let mut changed = selected;
    changed.commitment[0] ^= 1;
    assert_eq!(
        check_phlo_controls(selected.environment, 0, i64::MAX as u64, terms, changed, 6),
        Err(PhloControlsError::ScheduleNotPermitted)
    );
}

#[test]
fn selected_policy_requires_the_selected_descriptor_not_any_consented_descriptor() {
    let mut original = descriptor(vec![3], 10, 3, 2);
    let mut other = original.permitted_schedules[0].clone();
    other.classes[0].measurement_rule[0] ^= 1;
    original.permitted_schedules.push(other);
    let binding = PhloControlsBinding::new(&original, limits()).unwrap();
    let view = binding.view().unwrap();
    let required =
        PhloScheduleBinding::new(&original.permitted_schedules[0], limits().schedule(1)).unwrap();
    let schedules = view.terms().permitted_schedules;
    assert_eq!(
        view.check_schedule_policy(&schedules[0].commitment, required.policy()),
        Ok(())
    );
    assert_eq!(
        view.check_schedule_policy(&schedules[1].commitment, required.policy()),
        Err(PhloSchedulePolicyMismatch)
    );
    assert_eq!(
        view.check_schedule_policy(&[0; 32], required.policy()),
        Err(PhloSchedulePolicyMismatch)
    );
}

#[test]
fn phlo_controls_wire_binding_does_not_turn_empty_or_insufficient_consent_into_authorization() {
    for owners in [vec![], vec![3, 1, 8]] {
        let expected = if owners.is_empty() {
            PhloBoundsError::MissingOwnerConsent
        } else {
            PhloBoundsError::OwnerPriceCeilingExceeded
        };
        let original = descriptor(owners, 10, 3, 2);
        let wire = original.encode(limits()).unwrap();
        let decoded = PhloControlsV1::decode(&wire, limits()).unwrap();
        let binding = PhloControlsBinding::new(&decoded, limits()).unwrap();
        let view = binding.view().unwrap();
        let terms = view.terms();
        let selected = terms.permitted_schedules[0];
        assert_eq!(
            check_phlo_controls(selected.environment, 0, i64::MAX as u64, terms, selected, 6),
            Err(PhloControlsError::Numeric(expected))
        );
    }
    let original = descriptor(vec![u64::MAX], u64::MAX, u64::MAX, 1);
    let binding = PhloControlsBinding::new(&original, limits()).unwrap();
    let view = binding.view().unwrap();
    let terms = view.terms();
    let selected = terms.permitted_schedules[0];
    assert!(matches!(
        check_phlo_controls(selected.environment, 0, u64::MAX, terms, selected, 0),
        Err(PhloControlsError::Numeric(_))
    ));
}

#[test]
fn phlo_controls_wire_binding_enforces_record_limits_before_materializing_schedules() {
    let original = descriptor(vec![3, 5, 8], 10, 3, 2);
    for limited in [
        PhloControlsLimits {
            owners: 2,
            ..limits()
        },
        PhloControlsLimits {
            schedules: 0,
            ..limits()
        },
        PhloControlsLimits {
            total_classes: 0,
            ..limits()
        },
        PhloControlsLimits {
            wire: PhloWireLimits {
                total_bytes: 1,
                ..limits().wire
            },
            ..limits()
        },
    ] {
        assert!(PhloControlsBinding::new(&original, limited).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_controls_wire_generated_binding_matches_original_admission(
        owners in prop::collection::vec(0u64..100, 0..130),
        limit in 0u64..1000, ceiling in 0u64..100, price in 0u64..100, bound in 0u64..1000,
        compatible in any::<bool>(),
    ) {
        let owners = if compatible {
            if owners.is_empty() { vec![price] } else { owners.into_iter().map(|value| value.max(price)).collect() }
        } else { owners };
        let ceiling = if compatible { ceiling.max(price) } else { ceiling };
        let bound = if compatible { bound.min(limit) } else { bound };
        let original = descriptor(owners, limit, ceiling, price);
        let wire = original.encode(limits()).unwrap();
        let decoded = PhloControlsV1::decode(&wire, limits()).unwrap();
        let original_binding = PhloControlsBinding::new(&original, limits()).unwrap();
        let decoded_binding = PhloControlsBinding::new(&decoded, limits()).unwrap();
        let original_view = original_binding.view().unwrap();
        let decoded_view = decoded_binding.view().unwrap();
        prop_assert_eq!(original_view.terms(), decoded_view.terms());
        let terms = decoded_view.terms();
        let selected = terms.permitted_schedules[0];
        let actual = check_phlo_controls(selected.environment, 0, u64::MAX, terms, selected, bound);
        let expected = check_phlo_controls(selected.environment, 0, u64::MAX, original_view.terms(), selected, bound);
        prop_assert_eq!(&actual, &expected);
        let consent = !original.required_owner_ceilings.is_empty()
            && price <= ceiling && original.required_owner_ceilings.iter().all(|owner| price <= *owner)
            && bound <= limit;
        prop_assert_eq!(actual.is_ok(), consent);
        if let Ok(checked) = actual {
            let right = PhloFundingTerms { required_owner_ceilings: &original.required_owner_ceilings,
                asset: b"REV", schedule_commitment: selected.commitment };
            prop_assert!(check_phlo_funding_terms(checked, right).is_ok());
        }
    }
}
