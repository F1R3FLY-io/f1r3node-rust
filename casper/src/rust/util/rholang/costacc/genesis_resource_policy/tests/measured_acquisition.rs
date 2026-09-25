use std::sync::Arc;

use models::rhoapi::{CostAuthority, CostRegion};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use rholang::rust::interpreter::accounting::authority::{
    sig_to_cost_signature, AuthorityByteEventKind,
};
use rholang::rust::interpreter::accounting::byte_accounting::ByteCharge;
use rholang::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, prepare_counted_phlo_discharge, PhloDischargeLimits,
    PhloExecutionLimits, PhloOutcome,
};
use rholang::rust::interpreter::accounting::Sig;
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::*;

#[test]
fn measured_acquisition_binds_adopted_policy_selected_price_and_resource_bound() {
    let adopted = acquisition_context();
    let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)));
    let input = ByteObservationSnapshot {
        rows: vec![Arc::new(ByteObservation {
            event_id: [7; 32],
            kind: AuthorityByteEventKind::Comm,
            authority: CostAuthority {
                regions: vec![CostRegion {
                    instance_id: vec![3; 32],
                    signature: Some(
                        sig_to_cost_signature(&Sig::And(
                            Box::new(Sig::Ground(vec![1])),
                            Box::new(Sig::Ground(vec![2])),
                        ))
                        .unwrap(),
                    ),
                }],
            },
            measurement: Some(ByteCharge {
                introduction_bytes: 0,
                transfer_bytes: 3,
                trace_bytes: 0,
            }),
            legacy_amount: None,
        })],
        metered_context: true,
        history_lost: false,
    };
    let measured = adopted.native_rules().measure(&input, 1).unwrap();
    let regions = measured
        .region_demands(
            NativePhloRegionLimits {
                regions: 10,
                encoded_authority_bytes: 1_000_000,
            },
            &budget,
        )
        .unwrap();
    let located = regions
        .locate_purses(
            NativePhloPurseLimits {
                bindings: 10,
                encoded_binding_bytes: 1_000_000,
            },
            &budget,
        )
        .unwrap();
    let mut descriptor = adopted.genesis().record().schedule().unwrap();
    descriptor.actual_price = 23;
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let acquisition = located
        .prepare_acquisition_demand(
            &binding,
            &terms,
            NativePhloAcquisitionLimits {
                schedule: PhloGenesisPolicy::LIMITS,
                entries: 10,
            },
            &budget,
        )
        .unwrap();
    let schedules = [binding.schedule()];
    let controls = check_phlo_controls(
        schedules[0].environment,
        10,
        u64::MAX,
        SignedPhloControls {
            limit: 44,
            price_ceiling: 23,
            required_owner_ceilings: &[23, 30],
            permitted_schedules: &schedules,
        },
        schedules[0],
        44,
    )
    .unwrap();
    adopted
        .check_measured_acquisition(controls, &acquisition)
        .unwrap();
    let execution_limits = PhloExecutionLimits {
        resource_entries: 100,
        authority_nodes: 1000,
        key_bytes: 1_000_000,
    };
    let discharge = prepare_counted_phlo_discharge(
        &[],
        acquisition.resources(),
        PhloDischargeLimits {
            execution: execution_limits,
            key: models::rust::phlo_obligation::PhloObligationKeyLimits {
                wire: models::rust::phlo_wire::PhloWireLimits {
                    total_bytes: 100_000,
                    field_bytes: 50_000,
                },
                authority_nodes: 1000,
            },
            aggregate_key_bytes: 1_000_000,
        },
        &budget,
    )
    .unwrap();
    let checked =
        check_counted_phlo_execution(controls, discharge.witness(), execution_limits).unwrap();
    assert_eq!(checked.usage(), 44);
    assert_eq!(checked.retained_charge(PhloOutcome::Accepted(&[])), 1013);
    let contract = adopted.bind_execution_contract(controls, &binding).unwrap();
    let prepared = contract
        .prepare(
            Arc::clone(&input.rows[0]),
            NativePhloRegionLimits {
                regions: 10,
                encoded_authority_bytes: 1_000_000,
            },
            &budget,
        )
        .unwrap();
    assert_eq!(prepared.usage(), checked.usage());
    let mut reservation = contract.reservation();
    reservation.reserve(&prepared).unwrap();
    assert_eq!(reservation.used(), checked.usage());
    assert!(reservation.reserve(&prepared).is_err());
    assert_eq!(reservation.used(), checked.usage());
    let wrong_minimum = check_phlo_controls(
        schedules[0].environment,
        0,
        u64::MAX,
        controls.terms(),
        schedules[0],
        44,
    )
    .unwrap();
    assert!(adopted
        .check_measured_acquisition(wrong_minimum, &acquisition)
        .is_err());
    assert!(adopted
        .bind_execution_contract(wrong_minimum, &binding)
        .is_err());
    let mut wrong = descriptor.clone();
    wrong.actual_price = 24;
    let wrong_binding = PhloScheduleBinding::new(&wrong, PhloGenesisPolicy::LIMITS).unwrap();
    let wrong_schedules = [wrong_binding.schedule()];
    let wrong_controls = check_phlo_controls(
        wrong_schedules[0].environment,
        10,
        u64::MAX,
        SignedPhloControls {
            price_ceiling: 24,
            required_owner_ceilings: &[24, 30],
            permitted_schedules: &wrong_schedules,
            ..controls.terms()
        },
        wrong_schedules[0],
        44,
    )
    .unwrap();
    assert!(adopted
        .check_measured_acquisition(wrong_controls, &acquisition)
        .is_err());
    assert!(adopted
        .bind_execution_contract(wrong_controls, &binding)
        .is_err());
    assert!(adopted
        .bind_execution_contract(wrong_controls, &wrong_binding)
        .is_ok());
    for field in 0..3 {
        let mut wrong = descriptor.clone();
        match field {
            0 => wrong.network = b"other",
            1 => wrong.classes[0].weight = 2,
            _ => wrong.classes[0].identity = b"other-compute",
        }
        let bytes = wrong.encode(PhloGenesisPolicy::LIMITS).unwrap();
        let wrong_binding = PhloScheduleBinding::new(&wrong, PhloGenesisPolicy::LIMITS).unwrap();
        let wrong_acquisition = located
            .prepare_acquisition_demand(
                &wrong_binding,
                &bytes,
                NativePhloAcquisitionLimits {
                    schedule: PhloGenesisPolicy::LIMITS,
                    entries: 10,
                },
                &budget,
            )
            .unwrap();
        let wrong_schedules = [wrong_binding.schedule()];
        let wrong_controls = check_phlo_controls(
            wrong_schedules[0].environment,
            10,
            u64::MAX,
            SignedPhloControls {
                permitted_schedules: &wrong_schedules,
                ..controls.terms()
            },
            wrong_schedules[0],
            44,
        )
        .unwrap();
        assert!(adopted
            .check_measured_acquisition(wrong_controls, &wrong_acquisition)
            .is_err());
        assert!(adopted
            .bind_execution_contract(wrong_controls, &wrong_binding)
            .is_err());
    }
}
