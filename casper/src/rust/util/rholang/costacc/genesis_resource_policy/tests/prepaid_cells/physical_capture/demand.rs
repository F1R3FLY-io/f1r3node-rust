use proptest::test_runner::{Config, TestRunner};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use rholang::rust::interpreter::accounting::byte_accounting::ByteCharge;
use rholang::rust::interpreter::accounting::byte_receipts::{
    ByteObservation, ByteObservationSnapshot,
};
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, PhloExecutionLimits, PhloOutcome,
};

use super::*;
use crate::rust::util::rholang::costacc::prepaid_receipts::{
    NativePrepaidConsumption, NativePrepaidDemandInput, NativePrepaidDemandLimits,
    NativePrepaidInventoryLimits, PrepaidStackPop, PrepaidStackPopLimits,
};

#[tokio::test]
async fn measured_prepaid_binding_preserves_original_prices_and_exact_physical_consumption() {
    let context = acquisition_context();
    let quoted = models::rhoapi::Par::default();
    let owner_bytes = quoted.encode_to_vec();
    let (manager, mut runtime, stacks) = fixture_with_authority(3, 2, CostSignature {
        value: Some(Value::Ground(owner_bytes.clone())),
    })
    .await;
    let source = stacks[0].source_hash;
    let location = supply_channel(&Sig::Ground(owner_bytes.clone())).encode_to_vec();
    let values: Vec<_> = (0..3)
        .map(|occurrence| {
            let cells: Vec<_> = (0..2)
                .map(|index| {
                    let price = 10_000 + index as u64;
                    let mut schedule = context.genesis().record().schedule().unwrap();
                    schedule.actual_price = price;
                    let terms = schedule.encode(PhloGenesisPolicy::LIMITS).unwrap();
                    let key = PhloResourceKeyV1 {
                        location: &location,
                        class: 1,
                        acquisition_terms: &terms,
                        authority: vec![PhloAuthorityNode::Ground(&owner_bytes)],
                    }
                    .encode(PhloResourceLimits {
                        wire: LIMITS.wire,
                        authority_nodes: LIMITS.authority_nodes,
                    })
                    .unwrap();
                    NativePrepaidCell::encode(
                        &context,
                        NativePrepaidOrigin {
                            birth_source: source,
                            cell_index: index as u64,
                            deploy_id: [occurrence; 32],
                            ..origin()
                        },
                        &key,
                        &equal_shares(7 * price, 65),
                        LIMITS,
                    )
                    .unwrap()
                })
                .collect();
            OrderedPrepaidCells::encode(
                &cells.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                limits().cells,
            )
            .unwrap()
        })
        .collect();
    let root = store(&mut runtime, source, source, &values).await;
    let captured = manager
        .capture_prepaid_stacks(root, &stacks, &context, limits(), &budget())
        .unwrap();
    let execution = PhloExecutionLimits {
        resource_entries: 1000,
        authority_nodes: 100_000,
        key_bytes: 1_000_000,
    };
    let inventory = captured
        .resource_inventory(
            NativePrepaidInventoryLimits {
                cells: limits().cells,
                cell: LIMITS,
                execution,
            },
            &budget(),
        )
        .unwrap();
    let bind_limits = NativePrepaidDemandLimits {
        draws: 4,
        authority_bytes: 100_000,
        execution,
    };
    let mut descriptor = context.genesis().record().schedule().unwrap();
    descriptor.actual_price = 23;
    let terms = descriptor.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS).unwrap();
    let schedules = [binding.schedule()];
    let controls = check_phlo_controls(
        schedules[0].environment,
        10,
        u64::MAX,
        SignedPhloControls {
            limit: 57,
            price_ceiling: 23,
            required_owner_ceilings: &[23],
            permitted_schedules: &schedules,
        },
        schedules[0],
        57,
    )
    .unwrap();
    for alias in [false, true] {
        let input = ByteObservationSnapshot {
            rows: vec![Arc::new(ByteObservation {
                event_id: [7; 32],
                kind: AuthorityByteEventKind::Comm,
                authority: models::rhoapi::CostAuthority {
                    regions: vec![models::rhoapi::CostRegion {
                        instance_id: vec![3; 32],
                        signature: Some(CostSignature {
                            value: Some(if alias {
                                Value::Quote(quoted.clone())
                            } else {
                                Value::Ground(owner_bytes.clone())
                            }),
                        }),
                    }],
                },
                measurement: Some(ByteCharge {
                    introduction_bytes: 0,
                    transfer_bytes: 8,
                    trace_bytes: 0,
                }),
                legacy_amount: None,
            })],
            metered_context: true,
            history_lost: false,
        };
        let measured = context.native_rules().measure(&input, 1).unwrap();
        let regions = measured
            .region_demands(
                NativePhloRegionLimits {
                    regions: 10,
                    encoded_authority_bytes: 100_000,
                },
                &budget(),
            )
            .unwrap();
        let located = regions
            .locate_purses(
                NativePhloPurseLimits {
                    bindings: 10,
                    encoded_binding_bytes: 100_000,
                },
                &budget(),
            )
            .unwrap();
        let demand = located
            .prepare_acquisition_demand(
                &binding,
                &terms,
                NativePhloAcquisitionLimits {
                    schedule: PhloGenesisPolicy::LIMITS,
                    entries: 10,
                },
                &budget(),
            )
            .unwrap();
        let transfer = demand
            .resources()
            .iter()
            .position(|amount| amount.resource.class == 1)
            .unwrap();
        assert_eq!(demand.resources()[transfer].resource.location, location);
        let first = PrepaidStackPop {
            stack_id: stacks[0].instance_id,
            receipt_index: 0,
            count: 1,
        };
        if alias {
            let error = inventory
                .bind_measured_demand(
                    NativePrepaidDemandInput {
                        expected_root: root,
                        controls,
                        demand: &demand,
                        draws: &[first],
                        demand_positions: &[transfer],
                    },
                    bind_limits,
                    &budget(),
                )
                .unwrap_err();
            assert!(error.to_string().contains("stack head differs"));
            continue;
        }
        let mut runner = TestRunner::new(Config {
            cases: 128,
            source_file: Some(file!()),
            ..Config::default()
        });
        runner
            .run(
                &(proptest::collection::vec(0_u64..3, 3), any::<bool>()),
                |(counts, reverse)| {
                    let mut draws: Vec<_> = stacks
                        .iter()
                        .zip(&counts)
                        .enumerate()
                        .filter_map(|(receipt_index, (stack, &count))| {
                            (count > 0).then_some(PrepaidStackPop {
                                stack_id: stack.instance_id,
                                receipt_index,
                                count,
                            })
                        })
                        .collect();
                    if reverse {
                        draws.reverse();
                    }
                    let count = counts.iter().sum::<u64>();
                    let positions = vec![transfer; count as usize];
                    let bound = inventory
                        .bind_measured_demand(
                            NativePrepaidDemandInput {
                                expected_root: root,
                                controls,
                                demand: &demand,
                                draws: &draws,
                                demand_positions: &positions,
                            },
                            bind_limits,
                            &budget(),
                        )
                        .unwrap();
                    prop_assert_eq!(bound.root(), root);
                    prop_assert_eq!(bound.draws(), draws.as_slice());
                    let checked =
                        check_counted_phlo_execution(controls, bound.witness(), execution).unwrap();
                    prop_assert_eq!(checked.usage(), 57);
                    prop_assert_eq!(checked.prepaid_usage(), 7 * count);
                    prop_assert_eq!(checked.fresh_usage(), 57 - 7 * count);
                    prop_assert_eq!(
                        checked.retained_charge(PhloOutcome::Accepted(&[])),
                        1 + (57 - 7 * count) * 23
                    );
                    for used in bound.witness().used {
                        let original = PhloScheduleV1::decode(
                            used.resource.acquisition_terms,
                            PhloGenesisPolicy::LIMITS,
                        )
                        .unwrap();
                        prop_assert!((10_000..=10_001).contains(&original.actual_price));
                    }
                    let consumption = NativePrepaidConsumption {
                        captured: &captured,
                        draws: &draws,
                        limits: PrepaidStackPopLimits {
                            draws: 4,
                            bucket: limits().bucket,
                            cells: limits().cells,
                            receipts: limits().receipts,
                        },
                        execution,
                        cell: LIMITS,
                        physical_cells: limits().physical_cells,
                        physical_bytes: limits().physical_bytes,
                    };
                    prop_assert!(consumption
                        .check_resources(bound.witness().used.iter().copied(), &budget())
                        .is_ok());
                    Ok(())
                },
            )
            .unwrap();
        for (expected_root, draws, positions) in [
            ([99; 32], vec![first], vec![transfer]),
            (root, vec![first], vec![]),
            (root, vec![first], vec![transfer, transfer]),
            (root, vec![first], vec![99]),
            (root, vec![first, first], vec![transfer; 2]),
            (root, vec![first], vec![1 - transfer]),
        ] {
            assert!(inventory
                .bind_measured_demand(
                    NativePrepaidDemandInput {
                        expected_root,
                        controls,
                        demand: &demand,
                        draws: &draws,
                        demand_positions: &positions,
                    },
                    bind_limits,
                    &budget()
                )
                .is_err());
        }
        for dimension in [
            models::rust::host_work::HostWorkDimension::VerificationOperations,
            models::rust::host_work::HostWorkDimension::VerificationBytes,
            models::rust::host_work::HostWorkDimension::SearchStateBytes,
            models::rust::host_work::HostWorkDimension::AuthorityNodes,
        ] {
            let mut caps = HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
            caps.set(dimension, HostWorkLimit::new(0));
            let work = HostWorkBudget::new(caps);
            assert!(inventory
                .bind_measured_demand(
                    NativePrepaidDemandInput {
                        expected_root: root,
                        controls,
                        demand: &demand,
                        draws: &[first],
                        demand_positions: &[transfer],
                    },
                    bind_limits,
                    &work
                )
                .is_err());
            assert!(work.is_rejected());
        }
        for limited in [
            NativePrepaidDemandLimits {
                draws: 0,
                ..bind_limits
            },
            NativePrepaidDemandLimits {
                authority_bytes: 0,
                ..bind_limits
            },
            NativePrepaidDemandLimits {
                execution: PhloExecutionLimits {
                    resource_entries: 0,
                    ..execution
                },
                ..bind_limits
            },
        ] {
            assert!(inventory
                .bind_measured_demand(
                    NativePrepaidDemandInput {
                        expected_root: root,
                        controls,
                        demand: &demand,
                        draws: &[first],
                        demand_positions: &[transfer],
                    },
                    limited,
                    &budget()
                )
                .is_err());
        }
    }
    assert_eq!(runtime.runtime.create_checkpoint().await.root.bytes(), root);
}
