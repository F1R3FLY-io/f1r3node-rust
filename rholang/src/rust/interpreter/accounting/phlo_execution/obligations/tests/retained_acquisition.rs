use super::*;
use crate::rust::interpreter::accounting::phlo_execution::{
    check_phlo_family_consent, check_phlo_funding_family, DecodedPhloFamilyIntent,
    PhloConsentLimits, PhloFundingCase, PhloFundingLimits, PhloFundingSource, PhloResourceAmount,
    PhloSourceConsent,
};

#[test]
fn retained_acquisition_is_a_separate_funded_obligation_not_a_comm() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let retained = [PhloResourceAmount {
        resource: required[0],
        quantity: 3,
    }];
    let base = execution(&SCHEDULES, &[], &required, &required);
    let checked = base
        .with_retained_acquisitions(&retained, &funding_work())
        .unwrap();
    assert_eq!(checked.usage(), base.usage());
    assert_eq!(checked.fresh_usage(), base.fresh_usage());
    assert_eq!(checked.witness(), base.witness());
    assert_eq!(checked.retained_acquisition_value(), 6);
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
    assert_eq!(obligations.keys(), &[
        PhloObligationKey::Fee,
        PhloObligationKey::Resource(required[0]),
        PhloObligationKey::RetainedResource(required[0])
    ]);
    assert_eq!(obligations.quantities(), &[1, 1, 3]);
    assert_eq!(obligations.amounts(), &[1, 2, 6]);
    assert_eq!(obligations.total(), 9);
    let keys = obligations.encoded_keys(wire_limits(), 65_536).unwrap();
    assert_ne!(keys[1], keys[2]);
    assert!(matches!(
        PhloObligationKeyV1::decode(&keys[2], wire_limits()).unwrap(),
        PhloObligationKeyV1::RetainedResource(_)
    ));
    for outcome in [
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::User]),
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
        PhloOutcome::Accepted(&[PhloFailure::Certificate]),
        PhloOutcome::Accepted(&[PhloFailure::Unclassified]),
        PhloOutcome::Accepted(&[PhloFailure::User, PhloFailure::Platform]),
    ] {
        assert_eq!(
            project_phlo_obligations(checked, outcome, cap(3)),
            Err(PhloObligationError::RetainedResourcesOnFailure)
        );
    }
    assert_eq!(
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(2)),
        Err(PhloObligationError::TooManyObligations)
    );
    let prepaid = execution(&SCHEDULES, &required, &required, &[])
        .with_retained_acquisitions(&retained, &funding_work())
        .unwrap();
    assert_eq!(
        project_phlo_obligations(prepaid, PhloOutcome::Accepted(&[]), cap(2))
            .unwrap()
            .amounts(),
        &[1, 6]
    );
}

#[test]
fn retained_acquisition_requires_distinct_backing_and_signed_resource_permission() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let retained = [PhloResourceAmount {
        resource: required[0],
        quantity: 3,
    }];
    let checked = execution(&SCHEDULES, &[], &required, &required)
        .with_retained_acquisitions(&retained, &funding_work())
        .unwrap();
    let obligations =
        project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
    for n in [1usize, 3, 65] {
        let identities = (0..n).map(|i| (i as u64).to_be_bytes()).collect::<Vec<_>>();
        let keys = identities
            .iter()
            .map(|id| id.as_slice())
            .collect::<Vec<_>>();
        let capacity = vec![9u64; n];
        let eligible = vec![vec![true; 3]; n];
        let input = PhloObligationFundingInput {
            source_keys: &keys,
            capacities: &capacity,
            eligible: &eligible,
            canonical_resource_cursor: 0,
            canonical_fee_cursor: 0,
        };
        let selection = obligations
            .select_funding(input, selection_limits(), &funding_work())
            .unwrap()
            .unwrap();
        for (column, expected) in [1u64, 2, 6].into_iter().enumerate() {
            assert_eq!(
                selection
                    .assignment()
                    .iter()
                    .map(|row| row[column])
                    .sum::<u64>(),
                expected
            );
        }
        let sources = keys
            .iter()
            .map(|custody| PhloFundingSource {
                custody,
                capacity: 9,
                exposure_limit: 9,
                debit_limit: 9,
            })
            .collect::<Vec<_>>();
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: &eligible,
            assignment: selection.assignment(),
        }];
        let family = check_phlo_funding_family(&sources, &cases, 9, PhloFundingLimits {
            sources: cap(65),
            cases: cap(1),
            obligations: cap(3),
            assignment_cells: 195,
            custody_bytes: 1024,
        })
        .unwrap();
        let policies = keys
            .iter()
            .map(|custody| PhloSourceConsent {
                custody,
                hold_cap: 9,
                debit_cap: 9,
                fee_permitted: true,
                resources: &required,
            })
            .collect::<Vec<_>>();
        let intent = DecodedPhloFamilyIntent {
            controls: checked.controls().terms(),
            total_exposure: 9,
            sources: &policies,
        };
        let consent_limits = PhloConsentLimits {
            sources: 65,
            permission_entries: 65,
            case_cells: 195,
            authority_nodes: 1024,
            key_bytes: 1_048_576,
        };
        assert!(check_phlo_family_consent(&family, intent, consent_limits).is_ok());
        let denied = keys
            .iter()
            .map(|custody| PhloSourceConsent {
                custody,
                hold_cap: 9,
                debit_cap: 9,
                fee_permitted: true,
                resources: &[],
            })
            .collect::<Vec<_>>();
        assert!(check_phlo_family_consent(
            &family,
            DecodedPhloFamilyIntent {
                sources: &denied,
                ..intent
            },
            consent_limits
        )
        .is_err());
        assert!(
            check_phlo_funding_family(&sources, &cases, 8, PhloFundingLimits {
                sources: cap(65),
                cases: cap(1),
                obligations: cap(3),
                assignment_cells: 195,
                custody_bytes: 1024,
            })
            .is_err()
        );
    }
}

#[test]
fn retained_acquisition_checks_zero_quantities_overflow_and_work() {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    let authority = Sig::Ground(vec![1]);
    let base = execution(&SCHEDULES, &[], &[], &[]);
    for quantity in [0, u64::MAX] {
        let output = [PhloResourceAmount {
            resource: resource(&authority),
            quantity,
        }];
        assert!(base
            .with_retained_acquisitions(&output, &funding_work())
            .is_err());
    }
    let output = [PhloResourceAmount {
        resource: resource(&authority),
        quantity: 1,
    }];
    let zero = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(base.with_retained_acquisitions(&output, &zero).is_err());
    let unknown = [PhloResourceAmount {
        resource: PhloResource {
            class: 3,
            ..resource(&authority)
        },
        quantity: 1,
    }];
    assert!(base
        .with_retained_acquisitions(&unknown, &funding_work())
        .is_err());
    let free = PhloResource {
        class: 2,
        ..resource(&authority)
    };
    let overflowing = [
        PhloResourceAmount {
            resource: free,
            quantity: u64::MAX,
        },
        PhloResourceAmount {
            resource: free,
            quantity: 1,
        },
    ];
    assert!(base
        .with_retained_acquisitions(&overflowing, &funding_work())
        .is_err());
    let retained = base
        .with_retained_acquisitions(&output, &funding_work())
        .unwrap();
    let cleared = retained
        .with_retained_acquisitions(&[], &funding_work())
        .unwrap();
    assert_eq!(cleared, base);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn retained_acquisition_projection_refines_separate_conservation(
        used in 0usize..10, fresh in 0usize..10, retained in 1u64..1000,
        split in any::<bool>(), price in 0u64..1000,
    ) {
        let authority = Sig::And(Box::new(Sig::Ground(vec![1])), Box::new(Sig::Ground(vec![2])));
        let resources = vec![resource(&authority); used + fresh];
        let schedules = [PhloSchedule { actual_price: price, ..SCHEDULES[0] }];
        let base = execution(&schedules, &resources[..used], &resources, &resources[used..]);
        let outputs = if split && retained > 1 {
            vec![PhloResourceAmount { resource: resource(&authority), quantity: 1 },
                PhloResourceAmount { resource: resource(&authority), quantity: retained - 1 }]
        } else { vec![PhloResourceAmount { resource: resource(&authority), quantity: retained }] };
        let checked = base.with_retained_acquisitions(&outputs, &funding_work()).unwrap();
        let obligations = project_phlo_obligations(checked, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
        prop_assert_eq!(checked.usage(), 2 * (used + fresh) as u64);
        prop_assert_eq!(obligations.total(), 1 + 2 * (fresh as u64 + retained) * price);
        prop_assert_eq!(obligations.amounts().last().copied(), Some(2 * retained * price));
        prop_assert_eq!(obligations.quantities().last().copied(), Some(retained));
        let mut reversed = outputs.clone(); reversed.reverse();
        let other = base.with_retained_acquisitions(&reversed, &funding_work()).unwrap();
        let compared = project_phlo_obligations(other, PhloOutcome::Accepted(&[]), cap(3)).unwrap();
        prop_assert_eq!(obligations.amounts(), compared.amounts());
        prop_assert_eq!(obligations.encoded_keys(wire_limits(), 65_536).unwrap(), compared.encoded_keys(wire_limits(), 65_536).unwrap());
    }
}
