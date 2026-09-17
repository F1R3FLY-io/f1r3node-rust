use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloEnvironment, PhloSchedule, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, check_phlo_execution, CountedPhloExecutionWitness,
    PhloExecutionWitness, PhloFailure, PhloResource, PhloResourceAmount,
};
use crate::rust::interpreter::accounting::Sig;

const ENV: PhloEnvironment<'static> = PhloEnvironment {
    decimal_scale: 8,
    protocol_version: 6,
    network: b"network",
    shard: b"shard",
    asset: b"REV",
    unit: b"phlo",
};
const SCHEDULES: [PhloSchedule<'static>; 1] = [PhloSchedule {
    commitment: [1; 32],
    environment: ENV,
    weights: &[1, 1, 0],
    actual_price: 2,
}];

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

fn limits() -> PhloOutcomeMatchLimits {
    PhloOutcomeMatchLimits {
        execution: PhloExecutionLimits {
            resource_entries: 512,
            authority_nodes: 65_536,
            key_bytes: 1_048_576,
        },
        key: PhloObligationKeyLimits {
            wire: models::rust::phlo_wire::PhloWireLimits {
                total_bytes: 16_384,
                field_bytes: 8192,
            },
            authority_nodes: 65_536,
        },
        aggregate_key_bytes: 1_048_576,
        cases: NonZeroUsize::new(16).unwrap(),
    }
}

fn resource(authority: &Sig) -> PhloResource<'_> {
    PhloResource {
        location: b"slot",
        class: 0,
        acquisition_terms: b"terms",
        authority,
    }
}

fn checked<'a>(witness: PhloExecutionWitness<'a>) -> CheckedPhloExecution<'a> {
    configured(witness, &SCHEDULES, 100)
}

fn configured<'a>(
    witness: PhloExecutionWitness<'a>,
    schedules: &'a [PhloSchedule<'a>],
    limit: u64,
) -> CheckedPhloExecution<'a> {
    let controls = check_phlo_controls(
        schedules[0].environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit,
            price_ceiling: 4,
            required_owner_ceilings: &[4],
            permitted_schedules: schedules,
        },
        schedules[0],
        100,
    )
    .unwrap();
    check_phlo_execution(controls, witness, limits().execution).unwrap()
}

fn fresh<'a>(resources: &'a [PhloResource<'a>]) -> PhloExecutionWitness<'a> {
    PhloExecutionWitness {
        available: &[],
        required: resources,
        used: &[],
        unused: &[],
        fresh: resources,
    }
}

fn equal(left: CheckedPhloExecution<'_>, right: CheckedPhloExecution<'_>) -> bool {
    let left = normalize_execution(left, limits(), &work()).unwrap();
    let right = normalize_execution(right, limits(), &work()).unwrap();
    normalized_executions_equal(&left, &right, &work()).unwrap()
}

#[test]
fn equal_scalar_cost_does_not_match_different_typed_resources() {
    let authority = Sig::Ground(vec![1]);
    let other = Sig::Ground(vec![2]);
    let original = [resource(&authority)];
    let baseline = checked(fresh(&original));
    for changed in [
        PhloResource {
            location: b"other",
            ..original[0]
        },
        PhloResource {
            acquisition_terms: b"other",
            ..original[0]
        },
        PhloResource {
            authority: &other,
            ..original[0]
        },
        PhloResource {
            class: 1,
            ..original[0]
        },
    ] {
        let changed = [changed];
        let observed = checked(fresh(&changed));
        assert_eq!(baseline.usage(), observed.usage());
        assert_eq!(baseline.fresh_usage(), observed.fresh_usage());
        assert_eq!(
            baseline.retained_charge(PhloOutcome::Accepted(&[])),
            observed.retained_charge(PhloOutcome::Accepted(&[]))
        );
        assert!(!equal(baseline, observed));
    }
}

#[test]
fn matching_includes_unused_supply_and_zero_weight_resource_multiplicity() {
    let authority = Sig::Ground(vec![1]);
    let billable = [resource(&authority)];
    let unused = [PhloResource {
        location: b"unused",
        ..resource(&authority)
    }];
    let baseline = checked(fresh(&billable));
    let with_unused = checked(PhloExecutionWitness {
        available: &unused,
        unused: &unused,
        ..fresh(&billable)
    });
    assert_eq!(baseline.usage(), with_unused.usage());
    assert_eq!(baseline.prepaid_usage(), with_unused.prepaid_usage());
    assert_eq!(baseline.fresh_usage(), with_unused.fresh_usage());
    assert!(!equal(baseline, with_unused));
    let free = PhloResource {
        class: 2,
        ..resource(&authority)
    };
    let once = [billable[0], free];
    let twice = [billable[0], free, free];
    let one_free = checked(fresh(&once));
    let two_free = checked(fresh(&twice));
    assert_eq!(one_free.usage(), two_free.usage());
    assert_eq!(
        one_free.retained_charge(PhloOutcome::Accepted(&[])),
        two_free.retained_charge(PhloOutcome::Accepted(&[]))
    );
    assert!(!equal(one_free, two_free));
}

#[test]
fn selected_controls_and_schedule_are_part_of_execution_identity() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    let baseline = checked(fresh(&resources));
    assert!(!equal(
        baseline,
        configured(fresh(&resources), &SCHEDULES, 101)
    ));
    for schedule in [
        PhloSchedule {
            commitment: [2; 32],
            ..SCHEDULES[0]
        },
        PhloSchedule {
            environment: PhloEnvironment {
                shard: b"other",
                ..ENV
            },
            ..SCHEDULES[0]
        },
        PhloSchedule {
            environment: PhloEnvironment {
                network: b"other",
                ..ENV
            },
            ..SCHEDULES[0]
        },
        PhloSchedule {
            environment: PhloEnvironment {
                asset: b"other",
                ..ENV
            },
            ..SCHEDULES[0]
        },
        PhloSchedule {
            actual_price: 3,
            ..SCHEDULES[0]
        },
    ] {
        let schedules = [schedule];
        assert!(!equal(
            baseline,
            configured(fresh(&resources), &schedules, 100)
        ));
    }
}

#[test]
fn exact_outcome_masks_ignore_order_and_duplicates_but_not_failure_kind() {
    let kinds = [
        PhloFailure::User,
        PhloFailure::Platform,
        PhloFailure::Certificate,
        PhloFailure::Unclassified,
    ];
    let rejected = normalize_outcome(PhloOutcome::AdmissionRejected, &work()).unwrap();
    let mut normalized = Vec::new();
    for bits in 0_u8..16 {
        let failures: Vec<_> = kinds
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & (1 << i) != 0)
            .map(|(_, kind)| *kind)
            .collect();
        let original = normalize_outcome(PhloOutcome::Accepted(&failures), &work()).unwrap();
        let reversed: Vec<_> = failures.iter().rev().copied().collect();
        let repeated: Vec<_> = failures.iter().chain(&failures).copied().collect();
        assert_eq!(
            original,
            normalize_outcome(PhloOutcome::Accepted(&reversed), &work()).unwrap()
        );
        assert_eq!(
            original,
            normalize_outcome(PhloOutcome::Accepted(&repeated), &work()).unwrap()
        );
        assert_ne!(original, rejected);
        for earlier in &normalized {
            assert_ne!(&original, earlier);
        }
        normalized.push(original);
    }
}

#[test]
fn normalization_limits_fail_closed_for_resources_keys_and_host_work() {
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority); 3];
    let execution = checked(fresh(&resources));
    let measured = work();
    let normalized = normalize_execution(execution, limits(), &measured).unwrap();
    assert!(normalize_execution(
        execution,
        PhloOutcomeMatchLimits {
            aggregate_key_bytes: 0,
            ..limits()
        },
        &work()
    )
    .is_err());
    assert!(normalize_execution(
        execution,
        PhloOutcomeMatchLimits {
            execution: PhloExecutionLimits {
                resource_entries: 1,
                ..limits().execution
            },
            ..limits()
        },
        &work()
    )
    .is_err());
    assert!(normalize_execution(
        execution,
        PhloOutcomeMatchLimits {
            key: PhloObligationKeyLimits {
                authority_nodes: 0,
                ..limits().key
            },
            ..limits()
        },
        &work()
    )
    .is_err());
    let mut charged = 0;
    for dimension in HostWorkDimension::ALL {
        let amount = measured.usage(dimension).get();
        if amount == 0 {
            continue;
        }
        charged += 1;
        let mut exact = measured.limits();
        exact.set(dimension, HostWorkLimit::new(amount));
        let accepted =
            normalize_execution(execution, limits(), &HostWorkBudget::new(exact)).unwrap();
        assert!(normalized_executions_equal(&normalized, &accepted, &work()).unwrap());
        exact.set(dimension, HostWorkLimit::new(amount - 1));
        assert!(normalize_execution(execution, limits(), &HostWorkBudget::new(exact)).is_err());
    }
    assert!(charged > 0);
    let comparison = work();
    assert!(normalized_executions_equal(&normalized, &normalized, &comparison).unwrap());
    let mut comparison_dimensions = 0;
    for dimension in HostWorkDimension::ALL {
        let amount = comparison.usage(dimension).get();
        if amount == 0 {
            continue;
        }
        comparison_dimensions += 1;
        let mut exact = comparison.limits();
        exact.set(dimension, HostWorkLimit::new(amount));
        assert!(
            normalized_executions_equal(&normalized, &normalized, &HostWorkBudget::new(exact))
                .unwrap()
        );
        exact.set(dimension, HostWorkLimit::new(amount - 1));
        assert!(
            normalized_executions_equal(&normalized, &normalized, &HostWorkBudget::new(exact))
                .is_err()
        );
    }
    assert!(comparison_dimensions > 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_five_multiset_permutations_match_but_zero_cost_mutations_do_not(
        supplied in prop::array::uniform3(0_usize..=4),
        demanded in prop::array::uniform3(0_usize..=4),
    ) {
        let authority = Sig::Ground(vec![1]);
        let pool = [resource(&authority), PhloResource { location: b"other", class: 1, ..resource(&authority) },
            PhloResource { location: b"free", class: 2, ..resource(&authority) }];
        let collect = |counts: [usize; 3]| -> Vec<_> {
            (0..3).flat_map(|i| std::iter::repeat_n(pool[i], counts[i])).collect()
        };
        let available = collect(supplied);
        let required = collect(demanded);
        let used = collect(std::array::from_fn(|i| supplied[i].min(demanded[i])));
        let unused = collect(std::array::from_fn(|i| supplied[i].saturating_sub(demanded[i])));
        let fresh_resources = collect(std::array::from_fn(|i| demanded[i].saturating_sub(supplied[i])));
        let original = checked(PhloExecutionWitness { available: &available, required: &required, used: &used, unused: &unused, fresh: &fresh_resources });
        let compact: Vec<Vec<_>> = [&available, &required, &used, &unused, &fresh_resources]
            .iter().map(|part| pool.iter().filter_map(|resource| {
                let quantity = part.iter().filter(|item| *item == resource).count() as u64;
                (quantity > 0).then_some(PhloResourceAmount { resource: *resource, quantity })
            }).collect()).collect();
        let compact_checked = check_counted_phlo_execution(original.controls(), CountedPhloExecutionWitness {
            available: &compact[0], required: &compact[1], used: &compact[2],
            unused: &compact[3], fresh: &compact[4],
        }, limits().execution).unwrap();
        prop_assert!(equal(original, compact_checked));
        let reversed: Vec<Vec<_>> = [&available, &required, &used, &unused, &fresh_resources].into_iter()
            .map(|resources| resources.iter().rev().copied().collect()).collect();
        let reordered = checked(PhloExecutionWitness { available: &reversed[0], required: &reversed[1], used: &reversed[2], unused: &reversed[3], fresh: &reversed[4] });
        prop_assert!(equal(original, reordered));
        let new_resource = PhloResource { location: b"new-free", class: 2, ..resource(&authority) };
        let mut extra_required = required.clone(); extra_required.push(new_resource);
        let mut extra_fresh = fresh_resources.clone(); extra_fresh.push(new_resource);
        let changed = checked(PhloExecutionWitness { required: &extra_required, fresh: &extra_fresh, ..original.witness().occurrences().unwrap() });
        prop_assert_eq!(original.usage(), changed.usage());
        prop_assert_eq!(original.retained_charge(PhloOutcome::Accepted(&[])), changed.retained_charge(PhloOutcome::Accepted(&[])));
        prop_assert!(!equal(original, changed));
        let mut extra_available = available.clone(); extra_available.push(new_resource);
        let mut extra_unused = unused.clone(); extra_unused.push(new_resource);
        let changed_supply = checked(PhloExecutionWitness { available: &extra_available, unused: &extra_unused, ..original.witness().occurrences().unwrap() });
        prop_assert_eq!(original.usage(), changed_supply.usage());
        prop_assert_eq!(original.retained_charge(PhloOutcome::Accepted(&[])), changed_supply.retained_charge(PhloOutcome::Accepted(&[])));
        prop_assert!(!equal(original, changed_supply));
    }
}
