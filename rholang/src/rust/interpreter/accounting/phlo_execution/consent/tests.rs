use std::num::NonZeroUsize;

use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloExecutionLimits,
    PhloExecutionWitness, PhloFailure, PhloFundingCase, PhloFundingLimits, PhloFundingSource,
    PhloOutcome,
};
use crate::rust::interpreter::accounting::Sig;

mod wire_family_tests;
mod wire_controls_family;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"network",
    shard: b"shard",
    asset: b"asset",
    unit: b"unit",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENV,
    weights: &[1, 2],
    actual_price: 1,
}];

fn controls() -> SignedPhloControls<'static> {
    SignedPhloControls {
        limit: 1024,
        price_ceiling: 1,
        required_owner_ceilings: &[1],
        permitted_schedules: &SCHEDULES,
    }
}

fn limits() -> PhloConsentLimits {
    PhloConsentLimits {
        sources: 256,
        permission_entries: 4096,
        case_cells: 65_536,
        authority_nodes: 65_536,
        key_bytes: 1_048_576,
    }
}

fn source(custody: &[u8], amount: u64) -> PhloFundingSource<'_> {
    PhloFundingSource {
        custody,
        capacity: amount,
        exposure_limit: amount,
        debit_limit: amount,
    }
}

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        authority,
        location: b"slot",
        class: 0,
        acquisition_terms: b"terms",
    }
}

fn policy<'a>(
    custody: &'a [u8],
    amount: u64,
    fee_permitted: bool,
    resources: &'a [PhloResource<'a>],
) -> PhloSourceConsent<'a> {
    PhloSourceConsent {
        custody,
        hold_cap: amount,
        debit_cap: amount,
        fee_permitted,
        resources,
    }
}

fn intent<'a>(
    sources: &'a [PhloSourceConsent<'a>],
    total_exposure: u128,
) -> DecodedPhloFamilyIntent<'a> {
    DecodedPhloFamilyIntent {
        controls: controls(),
        total_exposure,
        sources,
    }
}

struct Family<'a> {
    sources: &'a [PhloFundingSource<'a>],
    resources: &'a [PhloResource<'a>],
    outcome: PhloOutcome<'a>,
    eligible: &'a [Vec<bool>],
    assignment: &'a [Vec<u64>],
    exposure: u128,
}

impl Family<'_> {
    fn with<T>(&self, test: impl FnOnce(&CheckedPhloFundingFamily<'_>) -> T) -> T {
        let checked =
            check_phlo_controls(ENV, 0, u64::MAX, controls(), SCHEDULES[0], 1024).unwrap();
        let execution = check_phlo_execution(
            checked,
            PhloExecutionWitness {
                available: &[],
                required: self.resources,
                used: &[],
                unused: &[],
                fresh: self.resources,
            },
            PhloExecutionLimits {
                resource_entries: 1024,
                authority_nodes: 65_536,
                key_bytes: 1_048_576,
            },
        )
        .unwrap();
        let obligations =
            project_phlo_obligations(execution, self.outcome, NonZeroUsize::new(1024).unwrap())
                .unwrap();
        let cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: self.eligible,
            assignment: self.assignment,
        }];
        let funding_limits = PhloFundingLimits {
            sources: NonZeroUsize::new(256).unwrap(),
            cases: NonZeroUsize::new(64).unwrap(),
            obligations: NonZeroUsize::new(1024).unwrap(),
            assignment_cells: 65_536,
            custody_bytes: 65_536,
        };
        let family =
            check_phlo_funding_family(self.sources, &cases, self.exposure, funding_limits).unwrap();
        test(&family)
    }
}

#[test]
fn consent_binds_three_purses_and_preserves_original_custody() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority), resource(&authority)];
    let sources = [source(b"a", 1), source(b"b", 1), source(b"c", 1)];
    let eligible = [vec![true, false], vec![false, true], vec![false, true]];
    let assignment = [vec![1, 0], vec![0, 1], vec![0, 1]];
    let policies = [
        policy(b"c", 1, false, &resources[..1]),
        policy(b"a", 1, true, &[]),
        policy(b"b", 1, false, &resources[..1]),
    ];
    Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 3,
    }
    .with(|family| {
        let checked = check_phlo_family_consent(family, intent(&policies, 3), limits()).unwrap();
        assert!(std::ptr::eq(checked.family(), family));
        assert_eq!(checked.intent().sources, &policies);
        let amounts = checked.family().native_amounts(0).unwrap();
        assert_eq!(
            amounts
                .iter()
                .map(|amount| amount.custody())
                .collect::<Vec<_>>(),
            vec![b"a", b"b", b"c"]
        );
        assert_eq!(
            amounts
                .iter()
                .map(|amount| amount.acquisition())
                .collect::<Vec<_>>(),
            vec![0, 1, 1]
        );
    });
}

#[test]
fn missing_duplicate_and_overstated_source_consent_fail_without_mutation() {
    let sources = [source(b"a", 7), source(b"b", 7)];
    let eligible = [vec![true], vec![false]];
    let assignment = [vec![1], vec![0]];
    let policies = [policy(b"a", 7, true, &[]), policy(b"b", 7, false, &[])];
    Family {
        sources: &sources,
        resources: &[],
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 14,
    }
    .with(|family| {
        let before = family.clone();
        assert!(check_phlo_family_consent(family, intent(&policies, 14), limits()).is_ok());
        assert_eq!(
            check_phlo_family_consent(family, intent(&policies[..1], 14), limits()),
            Err(PhloConsentError::MissingSource)
        );
        let duplicate = [policies[0], policies[0], policies[1]];
        assert_eq!(
            check_phlo_family_consent(family, intent(&duplicate, 14), limits()),
            Err(PhloConsentError::DuplicateCustody)
        );
        for index in 0..2 {
            let mut changed = policies;
            changed[index].hold_cap = 6;
            assert_eq!(
                check_phlo_family_consent(family, intent(&changed, 14), limits()),
                Err(PhloConsentError::HoldCapExceeded)
            );
            changed = policies;
            changed[index].debit_cap = 6;
            assert_eq!(
                check_phlo_family_consent(family, intent(&changed, 14), limits()),
                Err(PhloConsentError::DebitCapExceeded)
            );
        }
        assert_eq!(
            check_phlo_family_consent(family, intent(&policies, 13), limits()),
            Err(PhloConsentError::TotalExposureExceeded)
        );
        assert_eq!(*family, before);
    });
}

#[test]
fn changed_controls_cannot_authorize_an_existing_family() {
    let sources = [source(b"a", 1)];
    let eligible = [vec![true]];
    let assignment = [vec![1]];
    let policies = [policy(b"a", 1, true, &[])];
    let alternate_schedules = [PhloSchedule {
        actual_price: 0,
        ..SCHEDULES[0]
    }];
    let original = controls();
    let altered = [
        SignedPhloControls {
            limit: 1023,
            ..original
        },
        SignedPhloControls {
            price_ceiling: 2,
            ..original
        },
        SignedPhloControls {
            required_owner_ceilings: &[2],
            ..original
        },
        SignedPhloControls {
            permitted_schedules: &alternate_schedules,
            ..original
        },
    ];
    Family {
        sources: &sources,
        resources: &[],
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 1,
    }
    .with(|family| {
        for controls in altered {
            let changed = DecodedPhloFamilyIntent {
                controls,
                ..intent(&policies, 1)
            };
            assert_eq!(
                check_phlo_family_consent(family, changed, limits()),
                Err(PhloConsentError::DifferentControls)
            );
        }
    });
}

#[test]
fn equal_prices_do_not_replace_exact_resource_permissions() {
    let authority = Sig::Ground(vec![1]);
    let other = Sig::Ground(vec![2]);
    let resources = [resource(&authority)];
    let sources = [source(b"a", 2)];
    let eligible = [vec![true, true]];
    let assignment = [vec![1, 1]];
    let wrong = [
        PhloResource {
            location: b"elsewhere",
            ..resources[0]
        },
        PhloResource {
            acquisition_terms: b"other terms",
            ..resources[0]
        },
        PhloResource {
            class: 1,
            ..resources[0]
        },
        PhloResource {
            authority: &other,
            ..resources[0]
        },
    ];
    Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 2,
    }
    .with(|family| {
        for changed in wrong {
            let permitted = [changed];
            let policies = [policy(b"a", 2, true, &permitted)];
            assert_eq!(
                check_phlo_family_consent(family, intent(&policies, 2), limits()),
                Err(PhloConsentError::PermissionExceeded)
            );
        }
        let no_fee = [policy(b"a", 2, false, &resources)];
        assert_eq!(
            check_phlo_family_consent(family, intent(&no_fee, 2), limits()),
            Err(PhloConsentError::PermissionExceeded)
        );
    });
}

#[test]
fn zero_debits_and_failed_outcomes_do_not_expand_permissions() {
    let authority = Sig::Unit;
    let resources = [resource(&authority)];
    let sources = [source(b"a", 1), source(b"b", 1)];
    let eligible = [vec![true, true], vec![false, true]];
    let policies = [
        policy(b"a", 1, true, &resources),
        policy(b"b", 1, false, &[]),
    ];
    for outcome in [
        PhloOutcome::Accepted(&[]),
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
    ] {
        let fee = u64::from(outcome == PhloOutcome::Accepted(&[]));
        let assignment = [vec![fee, 0], vec![0, 0]];
        Family {
            sources: &sources,
            resources: &resources,
            outcome,
            eligible: &eligible,
            assignment: &assignment,
            exposure: 2,
        }
        .with(|family| {
            assert_eq!(
                check_phlo_family_consent(family, intent(&policies, 2), limits()),
                Err(PhloConsentError::PermissionExceeded)
            );
            let permitted = [policies[0], policy(b"b", 1, false, &resources)];
            assert!(check_phlo_family_consent(family, intent(&permitted, 2), limits()).is_ok());
        });
    }
}

#[test]
fn consent_limits_accept_exact_boundaries_and_reject_each_shortfall() {
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
        assert!(check_phlo_family_consent(family, intent(&policies, 3), exact).is_ok());
        for (limited, error) in [
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
                check_phlo_family_consent(family, intent(&policies, 3), limited),
                Err(error)
            );
        }
    });
}

#[test]
fn every_case_requires_consent_even_when_a_later_case_has_no_charge() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    let later_resources = [PhloResource {
        location: b"later slot",
        ..resources[0]
    }];
    let sources = [source(b"a", 2)];
    let eligible = [vec![true, true]];
    let first_assignment = [vec![1, 1]];
    let first = Family {
        sources: &sources,
        resources: &resources,
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &first_assignment,
        exposure: 2,
    };
    for outcome in [
        PhloOutcome::Accepted(&[]),
        PhloOutcome::AdmissionRejected,
        PhloOutcome::Accepted(&[PhloFailure::Platform]),
    ] {
        let amount = u64::from(outcome == PhloOutcome::Accepted(&[]));
        let later_assignment = [vec![amount, amount]];
        let later = Family {
            sources: &sources,
            resources: &later_resources,
            outcome,
            eligible: &eligible,
            assignment: &later_assignment,
            exposure: 2,
        };
        first.with(|first_family| {
            later.with(|later_family| {
                let mut cases = [first_family.cases()[0], later_family.cases()[0]];
                let funding_limits = PhloFundingLimits {
                    sources: NonZeroUsize::new(1).unwrap(),
                    cases: NonZeroUsize::new(2).unwrap(),
                    obligations: NonZeroUsize::new(2).unwrap(),
                    assignment_cells: 4,
                    custody_bytes: 1,
                };
                for _ in 0..2 {
                    let family =
                        check_phlo_funding_family(&sources, &cases, 2, funding_limits).unwrap();
                    let first_only = [policy(b"a", 2, true, &resources)];
                    assert_eq!(
                        check_phlo_family_consent(&family, intent(&first_only, 2), limits()),
                        Err(PhloConsentError::PermissionExceeded)
                    );
                    let all_resources = [resources[0], later_resources[0]];
                    let all = [policy(b"a", 2, true, &all_resources)];
                    let exact = PhloConsentLimits {
                        case_cells: 4,
                        ..limits()
                    };
                    assert!(check_phlo_family_consent(&family, intent(&all, 2), exact).is_ok());
                    let short = PhloConsentLimits {
                        case_cells: 3,
                        ..exact
                    };
                    assert_eq!(
                        check_phlo_family_consent(&family, intent(&all, 2), short),
                        Err(PhloConsentError::TooManyCaseCells)
                    );
                    cases.reverse();
                }
            })
        });
    }
}

#[test]
fn unused_policy_entries_still_require_bounded_valid_resource_keys() {
    let sources = [source(b"a", 1)];
    let eligible = [vec![true]];
    let assignment = [vec![1]];
    let authority = Sig::And(
        Box::new(Sig::Unit),
        Box::new(Sig::Bang(Box::new(Sig::Unit))),
    );
    let invalid = [resource(&authority)];
    let policies = [
        policy(b"a", 1, true, &[]),
        policy(b"unused", 0, false, &invalid),
    ];
    Family {
        sources: &sources,
        resources: &[],
        outcome: PhloOutcome::Accepted(&[]),
        eligible: &eligible,
        assignment: &assignment,
        exposure: 1,
    }
    .with(|family| {
        assert_eq!(
            check_phlo_family_consent(family, intent(&policies, 1), limits()),
            Err(PhloExecutionError::UnsupportedFundingAuthority.into()),
        );
    });
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_consent_matches_every_cap_and_permission(
        entries in prop::collection::vec((0u64..4, 0u64..4, any::<bool>(), any::<bool>()), 1..130),
        fee_seed in any::<usize>(), resource_seed in any::<usize>(),
        exposure_seed in any::<u16>(),
        location in prop::collection::vec(any::<u8>(), 0..16),
    ) {
        let count = entries.len();
        let identities: Vec<_> = (0..count).map(|index| index.to_be_bytes()).collect();
        let sources: Vec<_> = identities.iter().map(|identity| source(identity, 2)).collect();
        let authority = Sig::Ground(vec![1]);
        let resources = [PhloResource { location: &location, ..resource(&authority) }];
        let eligible = vec![vec![true, true]; count];
        let assignment: Vec<_> = (0..count).map(|index| vec![u64::from(index == fee_seed % count), u64::from(index == resource_seed % count)]).collect();
        let policies: Vec<_> = entries.iter().zip(&identities).map(|((hold_cap, debit_cap, fee, resources_allowed), identity)| PhloSourceConsent {
            custody: identity, hold_cap: *hold_cap, debit_cap: *debit_cap, fee_permitted: *fee,
            resources: if *resources_allowed { &resources } else { &[] },
        }).collect();
        let required_exposure = (2 * count) as u128;
        let consent_exposure = u128::from(exposure_seed) % (required_exposure + 3);
        let expected = consent_exposure >= required_exposure && entries.iter().all(|(hold, debit, fee, resource)| *hold >= 2 && *debit >= 2 && *fee && *resource);
        Family { sources: &sources, resources: &resources, outcome: PhloOutcome::Accepted(&[]), eligible: &eligible, assignment: &assignment, exposure: required_exposure }.with(|family| {
            prop_assert_eq!(check_phlo_family_consent(family, intent(&policies, consent_exposure), limits()).is_ok(), expected);
            Ok(())
        })?;
    }

    #[test]
    fn generated_valid_cohorts_preserve_consent_under_policy_permutation(
        count in 1usize..130, offset in any::<usize>(), extra in 0u64..100,
    ) {
        let identities: Vec<_> = (0..count).map(|index| index.to_be_bytes()).collect();
        let sources: Vec<_> = identities.iter().map(|identity| source(identity, 2)).collect();
        let authority = Sig::Ground(vec![1]);
        let resources = [resource(&authority), resource(&authority)];
        let eligible = vec![vec![true, true]; count];
        let assignment: Vec<_> = (0..count).map(|index| vec![u64::from(index == 0), u64::from(index == count - 1)]).collect();
        let mut policies: Vec<_> = identities.iter().map(|identity| policy(identity, 2 + extra, true, &resources)).collect();
        let fixture = Family { sources: &sources, resources: &resources[..1], outcome: PhloOutcome::Accepted(&[]), eligible: &eligible, assignment: &assignment, exposure: (2 * count) as u128 };
        fixture.with(|family| {
            prop_assert!(check_phlo_family_consent(family, intent(&policies, (2 * count) as u128), limits()).is_ok());
            Ok(())
        })?;
        policies.rotate_left(offset % count);
        policies.reverse();
        fixture.with(|family| {
            let checked = check_phlo_family_consent(family, intent(&policies, (2 * count) as u128), limits()).unwrap();
            prop_assert_eq!(checked.family().source_holds(), family.source_holds());
            prop_assert_eq!(checked.family().total_held(), 2);
            Ok(())
        })?;
    }
}
