use std::sync::Arc;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostRegion, CostSignature, CostSignatureCompound, Par};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use crate::rust::interpreter::accounting::native_phlo_rules::tests::schedule;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloPurseLimits, NativePhloRegionLimits,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, SignedPhloControls,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, prepare_counted_phlo_discharge, PhloDischargeLimits,
    PhloExecutionError, PhloExecutionLimits, PhloOutcome,
};

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn region_limits() -> NativePhloRegionLimits {
    NativePhloRegionLimits {
        regions: 100_000,
        encoded_authority_bytes: 10_000_000,
    }
}

fn purse_limits() -> NativePhloPurseLimits {
    NativePhloPurseLimits {
        bindings: 100_000,
        encoded_binding_bytes: 10_000_000,
    }
}

fn limits() -> NativePhloAcquisitionLimits {
    NativePhloAcquisitionLimits {
        schedule: PhloGenesisPolicy::LIMITS,
        entries: 100_000,
    }
}

fn execution_limits() -> PhloExecutionLimits {
    PhloExecutionLimits {
        resource_entries: 100_000,
        authority_nodes: 1_000_000,
        key_bytes: 20_000_000,
    }
}

fn snapshot(quantities: [u64; 3], owners: usize, copies: usize) -> ByteObservationSnapshot {
    let signature = sort_signature(&CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: (0..owners)
                .map(|owner| CostSignature {
                    value: Some(Value::Ground((owner as u64).to_be_bytes().to_vec())),
                })
                .collect(),
        })),
    })
    .term;
    let row = Arc::new(ByteObservation {
        event_id: [7; 32],
        kind: AuthorityByteEventKind::Comm,
        authority: CostAuthority {
            regions: (1..=2)
                .map(|region| CostRegion {
                    instance_id: vec![region; 32],
                    signature: Some(signature.clone()),
                })
                .collect(),
        },
        measurement: Some(ByteCharge {
            introduction_bytes: quantities[0],
            transfer_bytes: quantities[1],
            trace_bytes: quantities[2],
        }),
        legacy_amount: None,
    });
    ByteObservationSnapshot {
        rows: vec![row; copies],
        metered_context: true,
        history_lost: false,
    }
}

fn check_case(
    quantities: [u64; 3],
    owners: usize,
    copies: usize,
    price: u64,
    weights: [u64; 4],
    rotation: usize,
) {
    let mut descriptor = schedule();
    for (class, weight) in descriptor.classes.iter_mut().zip(weights) {
        class.weight = weight;
    }
    descriptor.classes.rotate_left(rotation);
    descriptor.actual_price = price;
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let input = snapshot(quantities, owners, copies);
    let original = input.clone();
    let measured = rules.measure(&input, copies).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    let prepared = located
        .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
        .unwrap();
    let positive = 1 + quantities
        .into_iter()
        .filter(|quantity| *quantity > 0)
        .count();
    assert_eq!(prepared.resources().len(), positive * 2 * copies);
    for ((origin, amount), expected) in prepared.occurrences().zip(located.occurrences()) {
        assert_eq!(origin, expected);
        assert_eq!(amount.resource.location, expected.purse().encoded_channel());
        assert_eq!(amount.resource.authority, expected.purse().authority());
        assert_eq!(
            amount.resource.class,
            expected.demand().measurement().class()
        );
        assert_eq!(amount.resource.acquisition_terms, terms);
        assert_eq!(amount.quantity, expected.demand().measurement().quantity());
    }
    let expected = u128::from(weights[0])
        + quantities
            .into_iter()
            .zip(weights[1..].iter())
            .map(|(q, w)| u128::from(q) * u128::from(*w))
            .sum::<u128>();
    let expected = u64::try_from(expected * owners as u128 * 2 * copies as u128).unwrap();
    let schedules = [binding.schedule()];
    let ceilings = [price];
    let controls = check_phlo_controls(
        schedules[0].environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: expected,
            price_ceiling: price,
            required_owner_ceilings: &ceilings,
            permitted_schedules: &schedules,
        },
        schedules[0],
        expected,
    )
    .unwrap();
    prepared.check_controls(controls).unwrap();
    let discharge = prepare_counted_phlo_discharge(
        &[],
        prepared.resources(),
        PhloDischargeLimits {
            execution: execution_limits(),
            key: models::rust::phlo_obligation::PhloObligationKeyLimits {
                wire: models::rust::phlo_wire::PhloWireLimits {
                    total_bytes: 1_000_000,
                    field_bytes: 500_000,
                },
                authority_nodes: 10_000,
            },
            aggregate_key_bytes: 20_000_000,
        },
        &budget(),
    )
    .unwrap();
    let checked =
        check_counted_phlo_execution(controls, discharge.witness(), execution_limits()).unwrap();
    assert_eq!(checked.usage(), expected);
    assert_eq!(checked.prepaid_usage(), 0);
    assert_eq!(checked.fresh_usage(), expected);
    let contract =
        crate::rust::interpreter::accounting::native_phlo_rules::NativePhloExecutionContract::new(
            controls, &binding,
        )
        .unwrap();
    let mut reservation = contract.reservation();
    for row in &input.rows {
        let charge = reservation
            .prepare(Arc::clone(row), region_limits(), &budget())
            .unwrap();
        reservation.reserve(&charge).unwrap();
    }
    assert_eq!(reservation.used(), checked.usage());
    assert_eq!(
        checked.retained_charge(PhloOutcome::Accepted(&[])),
        1 + expected * price
    );
    if expected > 0 {
        let bounded = check_phlo_controls(
            schedules[0].environment,
            0,
            u64::MAX,
            SignedPhloControls {
                limit: expected - 1,
                ..controls.terms()
            },
            schedules[0],
            expected - 1,
        )
        .unwrap();
        assert_eq!(
            check_counted_phlo_execution(bounded, discharge.witness(), execution_limits()),
            Err(PhloExecutionError::UsageExceeded)
        );
    }
    assert_eq!(input, original);
}

#[test]
fn native_acquisition_supports_arbitrary_cohorts_zero_valuation_and_counted_quantities() {
    for owners in [0, 1, 3, 65, 1000] {
        for price in [0, 7] {
            check_case([3, 5, 7], owners, 2, price, [2, 0, 3, 5], 2);
        }
    }
    check_case([u64::MAX / 2, 0, 0], 0, 1, 7, [1; 4], 0);
    check_case([u64::MAX / 2, 0, 0], 3, 1, 0, [0; 4], 3);
    check_case([0; 3], 3, 0, 0, [1; 4], 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn measured_acquisition_matches_independent_weighted_usage(
        quantities in prop::array::uniform3(0_u64..2000), owners in 0_usize..40,
        copies in 0_usize..5, price in 0_u64..200,
        weights in prop::array::uniform4(0_u64..20), rotation in 0_usize..4,
    ) { check_case(quantities, owners, copies, price, weights, rotation); }
}

#[test]
fn native_acquisition_requires_exact_terms_and_measured_class_mapping() {
    let descriptor = schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let input = snapshot([3, 5, 7], 3, 1);
    let measured = rules.measure(&input, 1).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    for field in 0..8 {
        let mut changed = descriptor.clone();
        match field {
            0 => changed.actual_price += 1,
            1 => changed.network = b"other",
            2 => changed.shard = b"other",
            3 => changed.protocol_version += 1,
            4 => changed.settlement_asset = b"other",
            5 => changed.settlement_unit = b"other",
            6 => changed.decimal_scale += 1,
            _ => changed.classes[0].weight += 1,
        }
        let bytes = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
        assert!(matches!(
            located.prepare_acquisition_demand(&binding, &bytes, limits(), &budget()),
            Err(NativePhloAcquisitionError::TermsMismatch)
        ));
    }
    let mut changed = descriptor.clone();
    changed.classes.rotate_left(1);
    let bytes = changed.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let other = PhloScheduleBinding::new(&changed, PhloGenesisPolicy::LIMITS).unwrap();
    assert!(matches!(
        located.prepare_acquisition_demand(&other, &bytes, limits(), &budget()),
        Err(NativePhloAcquisitionError::ClassMismatch)
    ));
}

#[test]
fn native_acquisition_limits_reject_without_changing_observations() {
    let descriptor = schedule();
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let input = snapshot([3, 5, 7], 3, 1);
    let original = input.clone();
    let measured = rules.measure(&input, 1).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    assert!(located
        .prepare_acquisition_demand(
            &binding,
            &terms,
            NativePhloAcquisitionLimits {
                entries: 8,
                ..limits()
            },
            &budget()
        )
        .is_ok());
    assert!(matches!(
        located.prepare_acquisition_demand(
            &binding,
            &terms,
            NativePhloAcquisitionLimits {
                entries: 7,
                ..limits()
            },
            &budget()
        ),
        Err(NativePhloAcquisitionError::EntryLimit)
    ));
    for dimension in [
        HostWorkDimension::VerificationOperations,
        HostWorkDimension::VerificationBytes,
        HostWorkDimension::SearchStateBytes,
    ] {
        let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
        caps.set(dimension, HostWorkLimit::new(0));
        assert!(located
            .prepare_acquisition_demand(&binding, &terms, limits(), &HostWorkBudget::new(caps))
            .is_err());
    }
    assert_eq!(input, original);
}

#[test]
fn native_acquisition_preserves_distinct_authority_despite_channel_aliases() {
    let descriptor = schedule();
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let mut input = snapshot([0; 3], 1, 1);
    let regions = &mut Arc::make_mut(&mut input.rows[0]).authority.regions;
    regions[0].signature = Some(CostSignature {
        value: Some(Value::Quote(Par::default())),
    });
    regions[1].signature = Some(CostSignature {
        value: Some(Value::Ground(Vec::new())),
    });
    let measured = rules.measure(&input, 1).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    let prepared = located
        .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
        .unwrap();
    assert_eq!(prepared.resources().len(), 2);
    assert_eq!(
        prepared.resources()[0].resource.location,
        prepared.resources()[1].resource.location
    );
    assert_ne!(
        prepared.resources()[0].resource.authority,
        prepared.resources()[1].resource.authority
    );
}

#[test]
fn native_acquisition_does_not_expand_maximum_quantity_or_hide_quantity_overflow() {
    let mut descriptor = schedule();
    for class in &mut descriptor.classes {
        class.weight = 0;
    }
    descriptor.actual_price = 0;
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let rules = NativePhloRules::resolve(&descriptor).unwrap();
    let input = snapshot([u64::MAX; 3], 3, 1);
    let measured = rules.measure(&input, 1).unwrap();
    let regions = measured.region_demands(region_limits(), &budget()).unwrap();
    let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
    let prepared = located
        .prepare_acquisition_demand(&binding, &terms, limits(), &budget())
        .unwrap();
    assert_eq!(prepared.resources().len(), 8);
    assert_eq!(
        prepared
            .resources()
            .iter()
            .filter(|entry| entry.quantity == u64::MAX)
            .count(),
        6
    );
    let schedules = [binding.schedule()];
    let controls = check_phlo_controls(
        schedules[0].environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 0,
            price_ceiling: 0,
            required_owner_ceilings: &[0],
            permitted_schedules: &schedules,
        },
        schedules[0],
        0,
    )
    .unwrap();
    let witness =
        crate::rust::interpreter::accounting::phlo_execution::CountedPhloExecutionWitness {
            available: &[],
            used: &[],
            unused: &[],
            required: prepared.resources(),
            fresh: prepared.resources(),
        };
    assert_eq!(
        check_counted_phlo_execution(controls, witness, execution_limits()),
        Err(PhloExecutionError::ArithmeticOverflow)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn acquisition_preserves_ordered_evidence_and_permutation_across_price_changes(
        quantities in prop::array::uniform3(0_u64..2000),
        other_quantities in prop::array::uniform3(0_u64..2000),
        owners in 0_usize..40, other_owners in 0_usize..40,
        old_price in 0_u64..200, new_price in 0_u64..200,
    ) {
        let mut input = snapshot(quantities, owners, 1);
        let mut other = snapshot(other_quantities, other_owners, 1);
        let second = Arc::make_mut(&mut other.rows[0]);
        second.event_id = [8; 32];
        for (index, region) in second.authority.regions.iter_mut().enumerate() {
            region.instance_id = vec![10 + index as u8; 32];
        }
        input.rows.extend(other.rows);
        let capture = |input: &ByteObservationSnapshot, price| {
            let mut descriptor = schedule();
            descriptor.actual_price = price;
            let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
            let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
            let rules = NativePhloRules::resolve(&descriptor).unwrap();
            let measured = rules.measure(input, 2).unwrap();
            let regions = measured.region_demands(region_limits(), &budget()).unwrap();
            let located = regions.locate_purses(purse_limits(), &budget()).unwrap();
            let prepared = located.prepare_acquisition_demand(&binding, &terms, limits(), &budget()).unwrap();
            prepared.occurrences().map(|(origin, amount)| {
                assert_eq!(amount.resource.acquisition_terms, terms);
                (origin.demand().measurement().observation().event_id,
                 origin.demand().region().instance_id.clone(), amount.resource.class,
                 amount.resource.location.to_vec(), amount.resource.authority.clone(), amount.quantity)
            }).collect::<Vec<_>>()
        };
        let first = capture(&input, old_price);
        prop_assert_eq!(&first, &capture(&input, new_price));
        let initial = ByteObservationSnapshot { rows: input.rows[..1].to_vec(), ..input.clone() };
        let tail = ByteObservationSnapshot { rows: input.rows[1..].to_vec(), ..input.clone() };
        let mut appended = capture(&initial, old_price);
        appended.extend(capture(&tail, old_price));
        prop_assert_eq!(&first, &appended);
        input.rows.reverse();
        let mut reverse = capture(&input, old_price);
        let mut first = first;
        first.sort_by_key(|entry| (entry.0, entry.1.clone(), entry.2));
        reverse.sort_by_key(|entry| (entry.0, entry.1.clone(), entry.2));
        prop_assert_eq!(first, reverse);
    }
}
