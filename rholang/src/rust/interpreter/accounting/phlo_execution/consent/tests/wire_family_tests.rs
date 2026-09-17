use models::rust::phlo_wire::PhloWireLimits;

use super::*;

const WIRE: PhloSourceLimits = PhloSourceLimits {
    wire: PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    },
    resource_permissions: 4096,
    authority_nodes: 65_536,
};
const WORK: PhloExecutionLimits = PhloExecutionLimits {
    resource_entries: 4096,
    authority_nodes: 65_536,
    key_bytes: 1_048_576,
};

fn compare(
    family: &CheckedPhloFundingFamily<'_>,
    native: DecodedPhloFamilyIntent<'_>,
    limits: PhloConsentLimits,
) -> Result<(), PhloConsentError> {
    let original = check_phlo_family_consent(family, native, limits).map(|_| ());
    let records: Vec<_> = native
        .sources
        .iter()
        .map(|source| {
            source
                .wire_policy(WORK, WIRE)
                .unwrap()
                .encode(WIRE)
                .unwrap()
        })
        .collect();
    let decoded: Vec<_> = records
        .iter()
        .map(|record| PhloSourcePolicyV1::decode(record, WIRE).unwrap())
        .collect();
    let wire = DecodedPhloFamilyWireIntent {
        controls: native.controls,
        total_exposure: native.total_exposure,
        sources: &decoded,
    };
    let checked = check_phlo_family_wire_consent(family, wire, limits);
    if let Ok(accepted) = checked {
        let terms = crate::rust::interpreter::accounting::phlo_controls::PhloFundingTerms {
            required_owner_ceilings: wire.controls.required_owner_ceilings,
            asset: ENV.asset,
            schedule_commitment: SCHEDULES[0].commitment,
        };
        let bound = accepted.bind_funding_terms(terms).unwrap();
        assert_eq!(bound.consent(), accepted);
        assert_eq!(bound.terms().terms(), terms);
        assert_eq!(accepted.bind_funding_terms(
            crate::rust::interpreter::accounting::phlo_controls::PhloFundingTerms {
                schedule_commitment: [7; 32], ..terms
            }), Err(crate::rust::interpreter::accounting::phlo_controls::PhloFundingTermsError::ScheduleMismatch));
        assert!(std::ptr::eq(accepted.family(), family));
        assert_eq!(accepted.intent(), wire);
        for selected in 0..family.cases().len() {
            assert_eq!(
                accepted.family().native_amounts(selected),
                family.native_amounts(selected)
            );
        }
    }
    let result = checked.map(|_| ());
    assert_eq!(result, original);
    result
}

#[test]
fn wire_family_consent_preserves_original_sources_and_rejects_every_cap_violation() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    let sources = [source(b"a", 2), source(b"b", 2), source(b"c", 2)];
    let eligible = vec![vec![true, true]; 3];
    let assignment = [vec![1, 0], vec![0, 1], vec![0, 0]];
    let policies = [
        policy(b"c", 2, true, &resources),
        policy(b"a", 2, true, &resources),
        policy(b"b", 2, true, &resources),
    ];
    Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 6,
    }
    .with(|family| {
        assert_eq!(compare(family, intent(&policies, 6), limits()), Ok(()));
        assert_eq!(
            compare(family, intent(&policies, 5), limits()),
            Err(PhloConsentError::TotalExposureExceeded)
        );
        assert_eq!(
            compare(family, intent(&policies[..2], 6), limits()),
            Err(PhloConsentError::MissingSource)
        );
        let duplicate = [policies[0], policies[0], policies[1], policies[2]];
        assert_eq!(
            compare(family, intent(&duplicate, 6), limits()),
            Err(PhloConsentError::DuplicateCustody)
        );
        for index in 0..3 {
            let mut reduced = policies;
            reduced[index].hold_cap = 1;
            assert_eq!(
                compare(family, intent(&reduced, 6), limits()),
                Err(PhloConsentError::HoldCapExceeded)
            );
            let mut reduced = policies;
            reduced[index].debit_cap = 1;
            assert_eq!(
                compare(family, intent(&reduced, 6), limits()),
                Err(PhloConsentError::DebitCapExceeded)
            );
            let mut reduced = policies;
            reduced[index].fee_permitted = false;
            assert_eq!(
                compare(family, intent(&reduced, 6), limits()),
                Err(PhloConsentError::PermissionExceeded)
            );
            let mut reduced = policies;
            reduced[index].resources = &[];
            assert_eq!(
                compare(family, intent(&reduced, 6), limits()),
                Err(PhloConsentError::PermissionExceeded)
            );
        }
        let changed = DecodedPhloFamilyIntent {
            controls: SignedPhloControls {
                limit: 1023,
                ..controls()
            },
            ..intent(&policies, 6)
        };
        assert_eq!(
            compare(family, changed, limits()),
            Err(PhloConsentError::DifferentControls)
        );
    });
}

#[test]
fn wire_family_consent_uses_exact_resource_keys_and_independent_fee_permission() {
    let authority = Sig::Ground(vec![1]);
    let quoted = Sig::Quote(vec![1]);
    let resources = [resource(&authority)];
    let sources = [source(b"a", 2)];
    let eligible = [vec![true, true]];
    let assignment = [vec![1, 1]];
    Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 2,
    }
    .with(|family| {
        for alternative in [
            PhloResource {
                location: b"other",
                ..resources[0]
            },
            PhloResource {
                class: 1,
                ..resources[0]
            },
            PhloResource {
                acquisition_terms: b"other",
                ..resources[0]
            },
            PhloResource {
                authority: &quoted,
                ..resources[0]
            },
        ] {
            let permissions = [alternative];
            let policies = [policy(b"a", 2, true, &permissions)];
            assert_eq!(
                compare(family, intent(&policies, 2), limits()),
                Err(PhloConsentError::PermissionExceeded)
            );
        }
        let no_fee = [policy(b"a", 2, false, &resources)];
        assert_eq!(
            compare(family, intent(&no_fee, 2), limits()),
            Err(PhloConsentError::PermissionExceeded)
        );
    });
}

#[test]
fn wire_family_consent_enforces_all_work_limits_at_exact_boundaries() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority), resource(&authority)];
    let sources = [source(b"a", 1), source(b"b", 1), source(b"c", 1)];
    let eligible = [vec![true, false], vec![false, true], vec![false, true]];
    let assignment = [vec![1, 0], vec![0, 1], vec![0, 1]];
    let policies = [
        policy(b"a", 1, true, &[]),
        policy(b"b", 1, false, &resources[..1]),
        policy(b"c", 1, false, &resources[..1]),
    ];
    let exact = PhloConsentLimits {
        sources: 3,
        permission_entries: 2,
        case_cells: 6,
        authority_nodes: 3,
        key_bytes: 36,
    };
    Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 3,
    }
    .with(|family| {
        assert_eq!(compare(family, intent(&policies, 3), exact), Ok(()));
        for (reduced, expected) in [
            (
                PhloConsentLimits {
                    sources: 2,
                    ..exact
                },
                PhloConsentError::TooManySources,
            ),
            (
                PhloConsentLimits {
                    permission_entries: 1,
                    ..exact
                },
                PhloConsentError::TooManyPermissions,
            ),
            (
                PhloConsentLimits {
                    case_cells: 5,
                    ..exact
                },
                PhloConsentError::TooManyCaseCells,
            ),
            (
                PhloConsentLimits {
                    authority_nodes: 2,
                    ..exact
                },
                PhloExecutionError::TooManyAuthorityNodes.into(),
            ),
            (
                PhloConsentLimits {
                    key_bytes: 35,
                    ..exact
                },
                PhloExecutionError::TooManyKeyBytes.into(),
            ),
        ] {
            assert_eq!(
                compare(family, intent(&policies, 3), reduced),
                Err(expected)
            );
        }
    });
}

#[test]
fn wire_family_consent_checks_later_failed_outcomes_before_selection() {
    let authority = Sig::Ground(vec![1]);
    let first_resources = [resource(&authority)];
    let later_resources = [PhloResource {
        location: b"later",
        ..first_resources[0]
    }];
    let sources = [source(b"a", 2)];
    let eligible = [vec![true, true]];
    let assignment = [vec![1, 1]];
    let zero = [vec![0, 0]];
    for outcome in [
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
    ] {
        Family {
            sources: &sources,
            resources: &first_resources,
            outcome: PhloOutcome::Accepted(&[]),
            eligible: &eligible,
            assignment: &assignment,
            exposure: 2,
        }
        .with(|first| {
            Family {
                sources: &sources,
                resources: &later_resources,
                outcome,
                eligible: &eligible,
                assignment: &zero,
                exposure: 2,
            }
            .with(|later| {
                let mut cases = [first.cases()[0], later.cases()[0]];
                let family_limits = PhloFundingLimits {
                    sources: NonZeroUsize::new(1).unwrap(),
                    cases: NonZeroUsize::new(2).unwrap(),
                    obligations: NonZeroUsize::new(2).unwrap(),
                    assignment_cells: 4,
                    custody_bytes: 1,
                };
                for _ in 0..2 {
                    let family =
                        check_phlo_funding_family(&sources, &cases, 2, family_limits).unwrap();
                    let first_only = [policy(b"a", 2, true, &first_resources)];
                    assert_eq!(
                        compare(&family, intent(&first_only, 2), limits()),
                        Err(PhloConsentError::PermissionExceeded)
                    );
                    let permissions = [first_resources[0], later_resources[0]];
                    let both = [policy(b"a", 2, true, &permissions)];
                    assert_eq!(compare(&family, intent(&both, 2), limits()), Ok(()));
                    cases.reverse();
                }
            });
        });
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn wire_family_generated_cohorts_match_native_and_independent_consent_oracle(
        entries in prop::collection::vec((0u64..4, 0u64..4, any::<bool>(), any::<bool>()), 1..130),
        valid in any::<bool>(), offset in any::<usize>(),
    ) {
        let count = entries.len();
        let identities: Vec<_> = (0..count).map(|index| index.to_be_bytes()).collect();
        let sources: Vec<_> = identities.iter().map(|identity| source(identity, 2)).collect();
        let authority = Sig::Ground(vec![1]);
        let resources = [resource(&authority), resource(&authority)];
        let eligible = vec![vec![true, true]; count];
        let assignment: Vec<_> = (0..count).map(|index| vec![u64::from(index == 0), u64::from(index == count - 1)]).collect();
        let mut policies: Vec<_> = entries.iter().zip(&identities).map(|((hold, debit, fee, allowed), custody)|
            PhloSourceConsent {custody, hold_cap: if valid {2} else {*hold}, debit_cap: if valid {2} else {*debit},
                fee_permitted: valid || *fee, resources: if valid || *allowed {&resources} else {&[]}}).collect();
        let expected = valid || entries.iter().all(|(hold, debit, fee, allowed)| *hold >= 2 && *debit >= 2 && *fee && *allowed);
        policies.rotate_left(offset % count);
        Family {sources: &sources, resources: &resources[..1], outcome: PhloOutcome::Accepted(&[]), eligible: &eligible,
            assignment: &assignment, exposure: (2 * count) as u128}.with(|family| {
            prop_assert_eq!(compare(family, intent(&policies, (2 * count) as u128), limits()).is_ok(), expected);
            Ok(())
        })?;
    }
}
