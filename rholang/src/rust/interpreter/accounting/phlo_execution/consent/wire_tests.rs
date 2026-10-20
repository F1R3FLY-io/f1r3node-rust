use std::collections::BTreeSet;

use models::rust::phlo_wire::PhloWireLimits;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::Sig;

const WORK: PhloExecutionLimits = PhloExecutionLimits {
    resource_entries: 1024,
    authority_nodes: 8192,
    key_bytes: 1_048_576,
};
const WIRE: PhloSourceLimits = PhloSourceLimits {
    wire: PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    },
    resource_permissions: 1024,
    authority_nodes: 8192,
};

fn resource(authority: &Sig, class: usize) -> PhloResource<'_> {
    PhloResource {
        authority,
        class,
        location: b"slot",
        acquisition_terms: b"terms",
    }
}

fn consent<'a>(resources: &'a [PhloResource<'a>]) -> PhloSourceConsent<'a> {
    PhloSourceConsent {
        custody: b"A",
        hold_cap: 100,
        debit_cap: 20,
        fee_permitted: false,
        resources,
    }
}

#[test]
fn phlo_source_projection_preserves_native_limits_fee_and_resource_set() {
    let authority = Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::Quote(vec![2])),
    );
    let resources = [
        resource(&authority, 2),
        resource(&authority, 1),
        resource(&authority, 2),
    ];
    let native = consent(&resources);
    let policy = native.wire_policy(WORK, WIRE).unwrap();
    assert_eq!(policy.custody(), native.custody);
    assert_eq!(policy.hold_cap(), native.hold_cap);
    assert_eq!(policy.debit_cap(), native.debit_cap);
    assert_eq!(policy.fee_permitted(), native.fee_permitted);
    assert_eq!(policy.resources().len(), 2);
    assert_eq!(policy.resources()[0], resources[1].wire_key(WORK).unwrap());
    let bytes = policy.encode(WIRE).unwrap();
    assert_eq!(PhloSourcePolicyV1::decode(&bytes, WIRE).unwrap(), policy);
}

#[test]
fn phlo_source_projection_shares_one_work_budget_across_all_permissions() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority, 0), resource(&authority, 1)];
    let native = consent(&resources);
    let exact = PhloExecutionLimits {
        resource_entries: 2,
        authority_nodes: 2,
        key_bytes: 21,
    };
    assert!(native.wire_policy(exact, WIRE).is_ok());
    assert_eq!(
        native.wire_policy(
            PhloExecutionLimits {
                resource_entries: 1,
                ..exact
            },
            WIRE
        ),
        Err(PhloConsentError::TooManyPermissions)
    );
    assert_eq!(
        native.wire_policy(
            PhloExecutionLimits {
                authority_nodes: 1,
                ..exact
            },
            WIRE
        ),
        Err(PhloConsentError::Execution(
            PhloExecutionError::TooManyAuthorityNodes
        ))
    );
    assert_eq!(
        native.wire_policy(
            PhloExecutionLimits {
                key_bytes: 20,
                ..exact
            },
            WIRE
        ),
        Err(PhloConsentError::Execution(
            PhloExecutionError::TooManyKeyBytes
        ))
    );
    assert_eq!(
        native.wire_policy(exact, PhloSourceLimits {
            resource_permissions: 1,
            ..WIRE
        }),
        Err(PhloConsentError::TooManyPermissions)
    );
    assert!(native
        .wire_policy(exact, PhloSourceLimits {
            authority_nodes: 1,
            ..WIRE
        })
        .is_err());
    let repeated = [resources[0]; 3];
    assert_eq!(
        consent(&repeated).wire_policy(exact, WIRE),
        Err(PhloConsentError::TooManyPermissions)
    );
}

#[test]
fn phlo_source_projection_rejects_unsupported_authority_in_any_permission() {
    let valid = Sig::Ground(vec![1]);
    let unsupported = Sig::Plus(Box::new(Sig::Unit), Box::new(Sig::Unit));
    for resources in [[resource(&valid, 0), resource(&unsupported, 1)], [
        resource(&unsupported, 0),
        resource(&valid, 1),
    ]] {
        assert_eq!(
            consent(&resources).wire_policy(WORK, WIRE),
            Err(PhloConsentError::Execution(
                PhloExecutionError::UnsupportedFundingAuthority
            ))
        );
    }
    let empty = PhloSourceConsent {
        custody: b"",
        ..consent(&[])
    };
    assert_eq!(
        empty.wire_policy(WORK, WIRE),
        Err(PhloConsentError::SourceWire(PhloSourceError::EmptyCustody))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn phlo_source_projection_preserves_permissions_under_reordering_and_repetition(
        classes in prop::collection::vec(any::<u32>(), 0..129), hold in any::<u64>(),
        debit in any::<u64>(), fee in any::<bool>(),
    ) {
        let authority = Sig::And(Box::new(Sig::Ground(vec![1])), Box::new(Sig::Ground(vec![1])));
        let resources: Vec<_> = classes.iter().map(|class| resource(&authority, *class as usize)).collect();
        let native = PhloSourceConsent {hold_cap: hold, debit_cap: debit, fee_permitted: fee, ..consent(&resources)};
        let policy = native.wire_policy(WORK, WIRE).unwrap();
        let expected: BTreeSet<_> = resources.iter().map(|resource| resource.wire_key(WORK).unwrap().encode(
            models::rust::phlo_resource::PhloResourceLimits {wire: WIRE.wire, authority_nodes: WIRE.authority_nodes}).unwrap()).collect();
        let actual: BTreeSet<_> = policy.resources().iter().map(|key| key.encode(
            models::rust::phlo_resource::PhloResourceLimits {wire: WIRE.wire, authority_nodes: WIRE.authority_nodes}).unwrap()).collect();
        prop_assert_eq!(actual, expected);
        let reordered: Vec<_> = resources.iter().rev().chain(resources.iter()).copied().collect();
        let reordered = PhloSourceConsent { resources: &reordered, ..native }.wire_policy(WORK, WIRE).unwrap();
        prop_assert_eq!(&policy, &reordered);
        prop_assert_eq!(policy.hold_cap(), hold);
        prop_assert_eq!(policy.debit_cap(), debit);
        prop_assert_eq!(policy.fee_permitted(), fee);
        let bytes = policy.encode(WIRE).unwrap();
        prop_assert_eq!(PhloSourcePolicyV1::decode(&bytes, WIRE).unwrap(), policy);
    }
}
