use proptest::prelude::*;

#[path = "counted_tests.rs"]
mod counted;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};

const ENVIRONMENT: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"test network",
    shard: b"test shard",
    asset: b"native asset",
    unit: b"native phlo",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENVIRONMENT,
    weights: &[1, 2, 3, 4],
    actual_price: 2,
}];
const LIMITS: PhloExecutionLimits = PhloExecutionLimits {
    resource_entries: 8192,
    authority_nodes: 131_072,
    key_bytes: 1_048_576,
};

fn controls(limit: u64) -> CheckedPhloControls<'static> { configured(&SCHEDULES, limit, limit) }

fn configured<'a>(
    schedules: &'a [PhloSchedule<'a>],
    limit: u64,
    bound: u64,
) -> CheckedPhloControls<'a> {
    check_phlo_controls(
        ENVIRONMENT,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: schedules[0].actual_price,
            required_owner_ceilings: &[u64::MAX],
            permitted_schedules: schedules,
        },
        schedules[0],
        bound,
    )
    .unwrap()
}

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        location: b"slot",
        class: 0,
        acquisition_terms: b"acquired",
        authority,
    }
}

fn witness<'a>(
    available: &'a [PhloResource<'a>],
    required: &'a [PhloResource<'a>],
    used: &'a [PhloResource<'a>],
    unused: &'a [PhloResource<'a>],
    fresh: &'a [PhloResource<'a>],
) -> PhloExecutionWitness<'a> {
    PhloExecutionWitness {
        available,
        required,
        used,
        unused,
        fresh,
    }
}

#[test]
fn partial_prepaid_funding_preserves_usage_and_charges_only_new_acquisition() {
    let authority = Sig::Ground(vec![1]);
    let available = vec![resource(&authority); 2];
    let required = vec![resource(&authority); 5];
    let fresh = vec![resource(&authority); 3];
    let evidence = witness(&available, &required, &available, &[], &fresh);
    let checked = check_phlo_execution(controls(10), evidence, LIMITS).unwrap();
    assert_eq!(checked.witness().occurrences(), Some(evidence));
    assert_eq!(checked.usage(), 5);
    assert_eq!(checked.prepaid_usage(), 2);
    assert_eq!(checked.fresh_usage(), 3);
    assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&[])), 7);
    assert_eq!(checked.controls().resource_bound(), 10);
}

#[test]
fn fully_prepaid_use_still_obeys_the_execution_limit() {
    let authority = Sig::Ground(vec![1]);
    let required = vec![resource(&authority); 11];
    let evidence = witness(&required, &required, &required, &[], &[]);
    assert_eq!(
        check_phlo_execution(controls(10), evidence, LIMITS),
        Err(PhloExecutionError::UsageExceeded)
    );
    let accepted = check_phlo_execution(controls(11), evidence, LIMITS).unwrap();
    assert_eq!(accepted.prepaid_usage(), 11);
    assert_eq!(accepted.fresh_usage(), 0);
    assert_eq!(accepted.retained_charge(PhloOutcome::Accepted(&[])), 1);
}

#[test]
fn each_resource_identity_field_prevents_incompatible_credit() {
    let ground = Sig::Ground(vec![1]);
    let quote = Sig::Quote(vec![1]);
    let other = Sig::Ground(vec![2]);
    let original = resource(&ground);
    let available = [original];
    for changed in [
        PhloResource {
            location: b"other",
            ..original
        },
        PhloResource {
            class: 1,
            ..original
        },
        PhloResource {
            acquisition_terms: b"new terms",
            ..original
        },
        PhloResource {
            authority: &quote,
            ..original
        },
        PhloResource {
            authority: &other,
            ..original
        },
    ] {
        let required = [changed];
        assert_eq!(
            check_phlo_execution(
                controls(10),
                witness(&available, &required, &available, &[], &[]),
                LIMITS
            ),
            Err(PhloExecutionError::DemandPartitionMismatch),
        );
        let proper = check_phlo_execution(
            controls(10),
            witness(&available, &required, &[], &available, &required),
            LIMITS,
        )
        .unwrap();
        assert_eq!(proper.prepaid_usage(), 0);
    }
}

#[test]
fn duplicate_occurrences_need_separate_backing_and_compatible_credit_cannot_be_skipped() {
    let authority = Sig::Ground(vec![1]);
    let one = [resource(&authority)];
    let two = [resource(&authority); 2];
    assert_eq!(
        check_phlo_execution(controls(10), witness(&one, &two, &two, &[], &[]), LIMITS),
        Err(PhloExecutionError::SupplyPartitionMismatch),
    );
    let valid =
        check_phlo_execution(controls(10), witness(&one, &two, &one, &[], &one), LIMITS).unwrap();
    assert_eq!(valid.prepaid_usage(), 1);
    assert_eq!(valid.fresh_usage(), 1);
    assert_eq!(
        check_phlo_execution(controls(10), witness(&one, &one, &[], &one, &one), LIMITS),
        Err(PhloExecutionError::UnusedCompatiblePrepaid),
    );
}

#[test]
fn compound_value_is_additive_but_regrouping_cannot_be_silent_prepaid_conversion() {
    let a = Sig::Ground(vec![1]);
    let b = Sig::Quote(vec![2]);
    let ab = Sig::And(Box::new(a.clone()), Box::new(b.clone()));
    let compound = [resource(&ab)];
    let separate = [resource(&a), resource(&b)];
    let combined = check_phlo_execution(
        controls(10),
        witness(&[], &compound, &[], &[], &compound),
        LIMITS,
    )
    .unwrap();
    let split = check_phlo_execution(
        controls(10),
        witness(&[], &separate, &[], &[], &separate),
        LIMITS,
    )
    .unwrap();
    assert_eq!(combined.usage(), 2);
    assert_eq!(
        combined.retained_charge(PhloOutcome::Accepted(&[])),
        split.retained_charge(PhloOutcome::Accepted(&[]))
    );
    assert_eq!(
        check_phlo_execution(
            controls(10),
            witness(&compound, &separate, &compound, &[], &[]),
            LIMITS
        ),
        Err(PhloExecutionError::DemandPartitionMismatch),
    );
}

#[test]
fn unknown_classes_are_not_free_even_with_unit_authority() {
    let authority = Sig::Unit;
    let unknown = [PhloResource {
        class: 4,
        ..resource(&authority)
    }];
    assert_eq!(
        check_phlo_execution(
            controls(0),
            witness(&[], &unknown, &[], &[], &unknown),
            LIMITS
        ),
        Err(PhloExecutionError::UnknownResourceClass),
    );
    let unused = check_phlo_execution(
        controls(0),
        witness(&unknown, &[], &[], &unknown, &[]),
        LIMITS,
    )
    .unwrap();
    assert_eq!(unused.usage(), 0);
}

#[test]
fn zero_weights_do_not_bypass_typed_discharge() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let schedules = [PhloSchedule {
        weights: &[0],
        ..SCHEDULES[0]
    }];
    let terms = configured(&schedules, 0, 0);
    assert_eq!(
        check_phlo_execution(terms, witness(&[], &required, &[], &[], &[]), LIMITS),
        Err(PhloExecutionError::DemandPartitionMismatch),
    );
    let valid =
        check_phlo_execution(terms, witness(&[], &required, &[], &[], &required), LIMITS).unwrap();
    assert_eq!(valid.usage(), 0);
    assert_eq!(valid.retained_charge(PhloOutcome::Accepted(&[])), 1);
}

#[test]
fn valuation_checks_products_totals_and_resource_bounds_before_settlement() {
    let a = Sig::Ground(vec![1]);
    let ab = Sig::And(Box::new(a.clone()), Box::new(a.clone()));
    let compound = [resource(&ab)];
    let repeated = [resource(&a); 2];
    let schedules = [PhloSchedule {
        weights: &[u64::MAX],
        actual_price: 0,
        ..SCHEDULES[0]
    }];
    let terms = configured(&schedules, u64::MAX, u64::MAX);
    for required in [&compound[..], &repeated[..]] {
        assert_eq!(
            check_phlo_execution(terms, witness(&[], required, &[], &[], required), LIMITS),
            Err(PhloExecutionError::ArithmeticOverflow),
        );
    }
    let one = [resource(&a)];
    let maximum = check_phlo_execution(terms, witness(&[], &one, &[], &[], &one), LIMITS).unwrap();
    assert_eq!(maximum.usage(), u64::MAX);
    assert_eq!(maximum.retained_charge(PhloOutcome::Accepted(&[])), 1);
    let tighter = configured(&SCHEDULES, 10, 0);
    assert_eq!(
        check_phlo_execution(tighter, witness(&[], &one, &[], &[], &one), LIMITS),
        Err(PhloExecutionError::UsageExceeded)
    );
}

#[test]
fn witness_work_limits_count_all_roles_and_accept_exact_boundaries() {
    let authority = Sig::Ground(vec![1]);
    let one = [resource(&authority)];
    let evidence = witness(&one, &one, &one, &[], &[]);
    let exact = PhloExecutionLimits {
        resource_entries: 3,
        authority_nodes: 3,
        key_bytes: 39,
    };
    assert!(check_phlo_execution(controls(10), evidence, exact).is_ok());
    for (limits, expected) in [
        (
            PhloExecutionLimits {
                resource_entries: 2,
                ..exact
            },
            PhloExecutionError::TooManyResourceEntries,
        ),
        (
            PhloExecutionLimits {
                authority_nodes: 2,
                ..exact
            },
            PhloExecutionError::TooManyAuthorityNodes,
        ),
        (
            PhloExecutionLimits {
                key_bytes: 38,
                ..exact
            },
            PhloExecutionError::TooManyKeyBytes,
        ),
    ] {
        assert_eq!(
            check_phlo_execution(controls(10), evidence, limits),
            Err(expected)
        );
    }
    assert!(check_phlo_execution(
        controls(0),
        witness(&[], &[], &[], &[], &[]),
        PhloExecutionLimits {
            resource_entries: 0,
            authority_nodes: 0,
            key_bytes: 0
        }
    )
    .is_ok());
}

#[test]
fn capability_formulas_never_supply_payable_resources() {
    let a = Sig::Ground(vec![1]);
    for unsupported in [
        Sig::Bang(Box::new(a.clone())),
        Sig::WhyNot(Box::new(a.clone())),
        Sig::Plus(Box::new(a.clone()), Box::new(a.clone())),
        Sig::With(Box::new(a.clone()), Box::new(a.clone())),
        Sig::Lolly(Box::new(a.clone()), Box::new(a.clone())),
        Sig::Threshold {
            threshold: 1,
            members: vec![a.clone()],
        },
    ] {
        let nested = Sig::And(Box::new(a.clone()), Box::new(unsupported));
        let resources = [resource(&nested)];
        assert_eq!(
            check_phlo_execution(
                controls(10),
                witness(&resources, &[], &[], &resources, &[]),
                LIMITS
            ),
            Err(PhloExecutionError::UnsupportedFundingAuthority),
        );
    }
}

#[test]
fn mixed_failure_lists_cancel_all_candidate_charges() {
    let authority = Sig::Ground(vec![1]);
    let required = [resource(&authority)];
    let checked = check_phlo_execution(
        controls(10),
        witness(&[], &required, &[], &[], &required),
        LIMITS,
    )
    .unwrap();
    assert_eq!(checked.retained_charge(PhloOutcome::AdmissionRejected), 0);
    assert_eq!(
        checked.retained_charge(PhloOutcome::Accepted(&[PhloFailure::User])),
        3
    );
    for bad in [
        PhloFailure::Platform,
        PhloFailure::Certificate,
        PhloFailure::Unclassified,
    ] {
        for position in 0..8 {
            let mut failures = vec![PhloFailure::User; 8];
            failures[position] = bad;
            assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&failures)), 0);
        }
    }
}

#[test]
fn exhaustive_occurrence_partitions_match_the_formal_equations() {
    let authority = Sig::Ground(vec![1]);
    for available in 0..5 {
        for required in 0..5 {
            for used in 0..5 {
                for unused in 0..5 {
                    for fresh in 0..5 {
                        let parts = [available, required, used, unused, fresh]
                            .map(|count| vec![resource(&authority); count]);
                        let expected = available == used + unused
                            && required == used + fresh
                            && (unused == 0 || fresh == 0);
                        let result = check_phlo_execution(
                            controls(10),
                            witness(&parts[0], &parts[1], &parts[2], &parts[3], &parts[4]),
                            LIMITS,
                        );
                        assert_eq!(result.is_ok(), expected);
                    }
                }
            }
        }
    }
}

fn funding_authority() -> impl Strategy<Value = Sig> {
    prop_oneof![
        Just(Sig::Unit),
        prop::collection::vec(any::<u8>(), 0..16).prop_map(Sig::Ground),
        prop::collection::vec(any::<u8>(), 0..16).prop_map(Sig::Quote),
    ]
    .prop_recursive(5, 64, 2, |inner| {
        (inner.clone(), inner).prop_map(|(left, right)| Sig::And(Box::new(left), Box::new(right)))
    })
}

fn reference_authority_size_and_units(authority: &Sig) -> (usize, u64) {
    match authority {
        Sig::Unit => (1, 0),
        Sig::Ground(_) | Sig::Quote(_) => (1, 1),
        Sig::And(left, right) => {
            let (left_nodes, left_units) = reference_authority_size_and_units(left);
            let (right_nodes, right_units) = reference_authority_size_and_units(right);
            (1 + left_nodes + right_nodes, left_units + right_units)
        }
        _ => unreachable!(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn flattened_authorities_preserve_structural_identity_size_and_valuation(
        left in funding_authority(),
        right in funding_authority(),
    ) {
        let mut budget = WorkBudget { remaining_nodes: LIMITS.authority_nodes, remaining_bytes: LIMITS.key_bytes };
        let left_key = resource_key(resource(&left), &mut budget).unwrap();
        let right_key = resource_key(resource(&right), &mut budget).unwrap();
        prop_assert_eq!(left_key == right_key, left == right);
        let cloned_left = left.clone();
        let same_key = resource_key(resource(&cloned_left), &mut budget).unwrap();
        prop_assert_eq!(&left_key, &same_key);
        let (expected_nodes, expected_units) = reference_authority_size_and_units(&left);
        prop_assert_eq!(left_key.authority.len(), expected_nodes);
        let counts = ResourceCounts::from([(left_key, 1)]);
        prop_assert_eq!(weighted_usage(&counts, &[7]).unwrap(), expected_units * 7);
        let first = Sig::And(Box::new(Sig::And(Box::new(left.clone()), Box::new(right.clone()))), Box::new(Sig::Unit));
        let second = Sig::And(Box::new(left), Box::new(Sig::And(Box::new(right), Box::new(Sig::Unit))));
        prop_assert_ne!(resource_key(resource(&first), &mut budget).unwrap(), resource_key(resource(&second), &mut budget).unwrap());
    }

    #[test]
    fn generated_typed_partitions_match_independent_count_and_charge_oracles(
        cases in prop::collection::vec((0_usize..10, 0_usize..10), 1..65),
    ) {
        let authorities: Vec<_> = (0..cases.len()).map(|index| Sig::Ground(vec![index as u8])).collect();
        let mut available = Vec::new();
        let mut required = Vec::new();
        let mut used = Vec::new();
        let mut unused = Vec::new();
        let mut fresh = Vec::new();
        let mut expected_usage = 0_u64;
        let mut expected_prepaid = 0_u64;
        let mut expected_fresh = 0_u64;
        for (index, ((supply, demand), authority)) in cases.iter().zip(&authorities).enumerate() {
            let item = PhloResource { class: index % 4, ..resource(authority) };
            let matched = (*supply).min(*demand);
            available.extend(std::iter::repeat_n(item, *supply));
            required.extend(std::iter::repeat_n(item, *demand));
            used.extend(std::iter::repeat_n(item, matched));
            unused.extend(std::iter::repeat_n(item, *supply - matched));
            fresh.extend(std::iter::repeat_n(item, *demand - matched));
            let weight = (index % 4 + 1) as u64;
            expected_usage += *demand as u64 * weight;
            expected_prepaid += matched as u64 * weight;
            expected_fresh += (*demand - matched) as u64 * weight;
        }
        let evidence = witness(&available, &required, &used, &unused, &fresh);
        let checked = check_phlo_execution(controls(10_000), evidence, LIMITS).unwrap();
        prop_assert_eq!(checked.usage(), expected_usage);
        prop_assert_eq!(checked.prepaid_usage(), expected_prepaid);
        prop_assert_eq!(checked.fresh_usage(), expected_fresh);
        prop_assert_eq!(checked.usage(), checked.prepaid_usage() + checked.fresh_usage());
        prop_assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&[])), 2 * expected_fresh + 1);
        let expected_charge = checked.retained_charge(PhloOutcome::Accepted(&[]));
        available.reverse(); required.reverse(); used.reverse(); unused.reverse(); fresh.reverse();
        let reordered = check_phlo_execution(controls(10_000), witness(&available, &required, &used, &unused, &fresh), LIMITS).unwrap();
        prop_assert_eq!(reordered.retained_charge(PhloOutcome::Accepted(&[])), expected_charge);
        prop_assert_eq!(reordered.usage(), expected_usage);
    }

    #[test]
    fn generated_malformed_partitions_match_independent_list_occurrences(
        parts in prop::collection::vec(prop::collection::vec(0_u8..4, 0..20), 5),
    ) {
        let authorities = [Sig::Ground(vec![0]), Sig::Ground(vec![1]), Sig::Quote(vec![0]), Sig::Quote(vec![1])];
        let resources: Vec<Vec<_>> = parts.iter().map(|part| part.iter().map(|key| resource(&authorities[usize::from(*key)])).collect()).collect();
        let expected = (0_u8..4).all(|key| {
            let counts: Vec<_> = parts.iter().map(|part| part.iter().filter(|value| **value == key).count()).collect();
            counts[0] == counts[2] + counts[3]
                && counts[1] == counts[2] + counts[4]
                && (counts[3] == 0 || counts[4] == 0)
        });
        prop_assert_eq!(check_phlo_execution(controls(100), witness(&resources[0], &resources[1], &resources[2], &resources[3], &resources[4]), LIMITS).is_ok(), expected);
    }
}
