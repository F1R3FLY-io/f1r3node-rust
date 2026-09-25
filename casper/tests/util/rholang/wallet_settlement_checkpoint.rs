use casper::rust::util::rholang::costacc::direct_wallet_funding::CheckedDirectWalletSettlement;
use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use casper::rust::util::rholang::costacc::prepaid_receipts::{
    NativeMeasuredSettlementLimits, NativePrepaidCell, NativePrepaidCellLimits,
    NativePrepaidConsumption, NativePrepaidContribution, NativePrepaidDemandInput,
    NativePrepaidDemandLimits, NativePrepaidInventoryLimits, NativePrepaidOrigin,
    NativeRetainedBirthLimits, NativeRetainedRecordLimits, NativeRetainedSettlementLimits,
    NativeWalletSettlement, OrderedPrepaidCells, PrepaidCellLimits, PrepaidReceiptBucket,
    PrepaidReceiptBucketLimits, PrepaidReceiptChange, PrepaidReceiptLimits,
    PrepaidStackCaptureLimits, PrepaidStackPop, PrepaidStackPopLimits,
};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::AuthorityBornStack;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::SignedPhloControls;
use rholang::rust::interpreter::accounting::phlo_execution::{
    PhloObligationKey, PhloResource, PhloResourceAmount, PhloSourceConsent, RetainedBirthFunding,
    RetainedBirthFundingLimits, RetainedCellBackingLimits,
};

use super::*;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

pub(super) async fn verify_retained(
    manager: &RuntimeManager,
    original: &Cosigned<OfferedFundedDeploy>,
    funding_limits: DirectWalletFundingLimits,
    policy: &AdoptedResourcePolicy,
    treasury: &VaultAddress,
    pre_state: &StateHash,
) {
    let wallet_pre_state = pre_state.clone();
    let key = PrivateKey::from_bytes(&[0xca; 32]);
    let signature = CostSignature {
        value: Some(Value::Ground(accounting::principal_ground_v61(
            &Secp256k1.to_public(&key).bytes,
        ))),
    };
    let authority = Sig::Ground(accounting::principal_ground_v61(
        &Secp256k1.to_public(&key).bytes,
    ));
    let payer = vault_payer(&signature).unwrap();
    let mut record =
        PhloFundingIntentV1::decode(original.data.funding_intent(), funding_limits.funding)
            .unwrap();
    let terms = record.controls.permitted_schedules[0]
        .encode(funding_limits.funding.controls.schedule(1))
        .unwrap();
    let resource_location = supply::supply_channel(&authority).encode_to_vec();
    let resource = PhloResource {
        location: &resource_location,
        class: 0,
        acquisition_terms: &terms,
        authority: &authority,
    };
    let execution_limits = PhloExecutionLimits {
        resource_entries: 8,
        authority_nodes: 128,
        key_bytes: 1_048_576,
    };
    let cell_limit = NativePrepaidCellLimits {
        wire: funding_limits.funding.wire,
        authority_nodes: 128,
        sources: 1,
    };
    let stack_limit = PrepaidCellLimits {
        cells: 2,
        wire: funding_limits.funding.wire,
    };
    let receipts = PrepaidReceiptLimits {
        entries: 3,
        value_bytes: 524_288,
        batch_bytes: 1_048_576,
    };
    let bucket_limit = PrepaidReceiptBucketLimits {
        occurrences: 2,
        wire: funding_limits.funding.wire,
    };
    let mut seed = RuntimeOps::new(manager.spawn_runtime().await);
    seed.runtime
        .reset(&Blake2b256Hash::from_bytes_prost(pre_state))
        .await
        .unwrap();
    let channel = supply::supply_channel(&authority);
    seed.runtime
        .reducer
        .space
        .produce(
            channel.clone(),
            ListParWithRandom {
                pars: Vec::new(),
                random_state: vec![42],
                cost_authority: None,
                cost_stack: Some(CostStack {
                    cells: vec![signature.clone(); 2],
                }),
            },
            false,
        )
        .await
        .unwrap();
    let old_stacks =
        supply::decode_purse_inventory(&seed.get_data_datums(&channel).await, &signature)
            .unwrap()
            .stacks;
    let old_source = old_stacks[0].source_hash;
    let resource_key = resource
        .wire_key(execution_limits)
        .unwrap()
        .encode(models::rust::phlo_resource::PhloResourceLimits {
            wire: funding_limits.funding.wire,
            authority_nodes: 128,
        })
        .unwrap();
    let old_cells: Vec<_> = (0..2)
        .map(|index| {
            NativePrepaidCell::encode(
                policy,
                NativePrepaidOrigin {
                    genesis_root: policy.genesis().genesis_root().as_ref().try_into().unwrap(),
                    pre_state_root: pre_state.as_ref().try_into().unwrap(),
                    deploy_id: [0xcc; 32],
                    birth_source: old_source,
                    cell_index: index,
                },
                &resource_key,
                &[NativePrepaidContribution {
                    custody: payer.custody_key,
                    amount: 1,
                }],
                cell_limit,
            )
            .unwrap()
        })
        .collect();
    let old_record = OrderedPrepaidCells::encode(
        &old_cells.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        stack_limit,
    )
    .unwrap();
    let old_bucket = PrepaidReceiptBucket::new(old_source, &[&old_record], bucket_limit)
        .unwrap()
        .encode(bucket_limit)
        .unwrap();
    seed.replace_prepaid_receipts(
        &[PrepaidReceiptChange {
            receipt_id: PrepaidReceiptBucket::key_for_source(&old_source),
            expected: None,
            replacement: Some(&old_bucket),
        }],
        receipts,
    )
    .await
    .unwrap();
    let seeded_state = seed.runtime.create_checkpoint().await.root.to_bytes_prost();
    let pre_state = &seeded_state;
    let old_capture = manager
        .capture_prepaid_stacks(
            pre_state.as_ref().try_into().unwrap(),
            &old_stacks,
            policy,
            PrepaidStackCaptureLimits {
                stacks: 4,
                physical_cells: 4,
                physical_bytes: 1_048_576,
                bucket: bucket_limit,
                cells: stack_limit,
                native_cell: cell_limit,
                receipts,
            },
            &budget(),
        )
        .unwrap();
    let draws = [PrepaidStackPop {
        stack_id: old_stacks[0].instance_id,
        receipt_index: 0,
        count: 1,
    }];
    let pop_limits = PrepaidStackPopLimits {
        draws: 1,
        bucket: bucket_limit,
        cells: stack_limit,
        receipts,
    };
    let consumption = || NativePrepaidConsumption {
        captured: &old_capture,
        draws: &draws,
        limits: pop_limits,
        execution: execution_limits,
        cell: cell_limit,
        physical_cells: 4,
        physical_bytes: 1_048_576,
    };
    let wallet = || NativeWalletSettlement {
        reservation_id: [0xd1; 32],
        fee_address: treasury,
        initial_rand: Blake2b512Random::create_from_bytes(&[0xd1]),
    };
    let permissions = [resource];
    record.sources = vec![PhloSourceConsent {
        custody: &payer.custody_key,
        hold_cap: 10,
        debit_cap: 10,
        fee_permitted: true,
        resources: &permissions,
    }
    .wire_policy(execution_limits, PhloSourceLimits {
        wire: funding_limits.funding.wire,
        resource_permissions: 1,
        authority_nodes: 128,
    })
    .unwrap()];
    let envelope = Cosigned::create_single_envelope(
        OfferedFundedDeploy::new(
            original.data.body().clone(),
            record.encode(funding_limits.funding).unwrap(),
            8,
            1,
            FundedDeployLimits {
                deploy_bytes: 2_097_152,
                signing: funding_limits.funding.wire,
                funding: funding_limits.funding,
            },
        )
        .unwrap(),
        Box::new(Secp256k1),
        key,
    )
    .unwrap();
    let decoded =
        PhloFundingIntentV1::decode(envelope.data.funding_intent(), funding_limits.funding)
            .unwrap();
    let binding = PhloFundingIntentBinding::new(&decoded, funding_limits.funding).unwrap();
    let view = binding.view().unwrap();
    let schedule = view.controls().permitted_schedules[0];
    let controls = check_phlo_controls(
        schedule.environment,
        policy.genesis().minimum_price(),
        u64::MAX,
        view.controls(),
        schedule,
        1,
    )
    .unwrap();
    let retained = [PhloResourceAmount {
        resource,
        quantity: 1,
    }];
    let available = [resource, resource];
    let used = [resource];
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: &available,
            required: &used,
            used: &used,
            unused: &used,
            fresh: &[],
        },
        execution_limits,
    )
    .unwrap()
    .with_retained_acquisitions(&retained, &budget())
    .unwrap();
    let cap = |n| NonZeroUsize::new(n).unwrap();
    let obligations =
        project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), cap(2)).unwrap();
    assert_eq!(obligations.amounts(), &[1, 1]);
    let observations =
        rholang::rust::interpreter::accounting::byte_receipts::ByteObservationSnapshot {
            rows: vec![std::sync::Arc::new(
                rholang::rust::interpreter::accounting::byte_receipts::ByteObservation {
                    event_id: [0xda; 32],
                    kind: accounting::authority::AuthorityByteEventKind::Comm,
                    authority: models::rhoapi::CostAuthority {
                        regions: vec![models::rhoapi::CostRegion {
                            instance_id: vec![0xdb; 32],
                            signature: Some(signature.clone()),
                        }],
                    },
                    measurement: Some(accounting::byte_accounting::ByteCharge {
                        introduction_bytes: 0,
                        transfer_bytes: 0,
                        trace_bytes: 0,
                    }),
                    legacy_amount: None,
                },
            )],
            metered_context: true,
            history_lost: false,
        };
    let measured = policy.native_rules().measure(&observations, 1).unwrap();
    let regions = measured
        .region_demands(
            NativePhloRegionLimits {
                regions: 4,
                encoded_authority_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let located = regions
        .locate_purses(
            NativePhloPurseLimits {
                bindings: 4,
                encoded_binding_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let schedule_binding = accounting::phlo_controls::PhloScheduleBinding::new(
        &decoded.controls.permitted_schedules[0],
        models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS,
    )
    .unwrap();
    let acquisition = located
        .prepare_acquisition_demand(
            &schedule_binding,
            &terms,
            NativePhloAcquisitionLimits {
                schedule: models::rust::phlo_schedule::PhloGenesisPolicy::LIMITS,
                entries: 4,
            },
            &budget(),
        )
        .unwrap();
    let inventory = old_capture
        .resource_inventory(
            NativePrepaidInventoryLimits {
                cells: stack_limit,
                cell: cell_limit,
                execution: execution_limits,
            },
            &budget(),
        )
        .unwrap();
    let measured_limits = NativePrepaidDemandLimits {
        draws: 1,
        authority_bytes: 1_048_576,
        execution: execution_limits,
    };
    let measured_binding = inventory
        .bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: pre_state.as_ref().try_into().unwrap(),
                controls,
                demand: &acquisition,
                draws: &draws,
                demand_positions: &[0],
            },
            measured_limits,
            &budget(),
        )
        .unwrap();
    assert_eq!(measured_binding.controls(), controls);
    let measured_execution = accounting::phlo_execution::check_counted_phlo_execution(
        controls,
        measured_binding.witness(),
        execution_limits,
    )
    .unwrap()
    .with_retained_acquisitions(&retained, &budget())
    .unwrap();
    let measured_obligations =
        project_phlo_obligations(measured_execution, PhloOutcome::Accepted(&[]), cap(2)).unwrap();
    assert_eq!(measured_obligations.amounts(), obligations.amounts());
    let snapshot = authorize_offered_direct_wallet_funding(&envelope, funding_limits)
        .unwrap()
        .read_policy_snapshot(manager, pre_state.clone(), cap(2), &budget())
        .await
        .unwrap();
    let sources: Vec<_> = snapshot
        .wallets()
        .sources()
        .iter()
        .map(|source| source.source())
        .collect();
    let eligible = [vec![true, true]];
    let assignment = [vec![1, 1]];
    let cases = [
        PhloFundingCase {
            obligations: &obligations,
            eligible: &eligible,
            assignment: &assignment,
        },
        PhloFundingCase {
            obligations: &measured_obligations,
            eligible: &eligible,
            assignment: &assignment,
        },
    ];
    let family_limits = PhloFundingLimits {
        sources: cap(1),
        cases: cap(2),
        obligations: cap(2),
        assignment_cells: 4,
        custody_bytes: 32,
    };
    let family =
        check_phlo_funding_family(&sources, &cases, decoded.total_exposure, family_limits).unwrap();
    let keys = PhloObligationKeyLimits {
        wire: funding_limits.funding.wire,
        authority_nodes: 128,
    };
    let signed_limits = SignedPhloConsentLimits {
        members: cap(1),
        intent: funding_limits.funding,
        consent: PhloConsentLimits {
            sources: 1,
            permission_entries: 1,
            case_cells: 4,
            authority_nodes: 128,
            key_bytes: 1_048_576,
        },
    };
    let search_limits = PhloFamilyFundingLimits {
        funding: family_limits,
        keys,
        aggregate_key_bytes: 1_048_576,
    };
    let checked_policy = snapshot
        .bind_native_family_in_context(
            &view,
            &family,
            PhloFundingTerms {
                required_owner_ceilings: view.controls().required_owner_ceilings,
                asset: schedule.environment.asset,
                schedule_commitment: schedule.commitment,
            },
            signed_limits,
            search_limits,
            policy,
            &budget(),
        )
        .unwrap()
        .unwrap();
    let checked = checked_policy
        .capture_settlement(
            execution,
            PhloOutcome::Accepted(&[]),
            PhloOutcomeMatchLimits {
                execution: execution_limits,
                key: keys,
                aggregate_key_bytes: 1_048_576,
                cases: cap(2),
            },
            PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: cap(1),
                    obligation_cap: cap(2),
                },
                key: keys,
                aggregate_key_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let measured_settlement_limits = NativeMeasuredSettlementLimits {
        matching: PhloOutcomeMatchLimits {
            execution: execution_limits,
            key: keys,
            aggregate_key_bytes: 1_048_576,
            cases: cap(2),
        },
        capture: PhloCaptureLimits {
            funding: FundingSearchLimits {
                source_cap: cap(1),
                obligation_cap: cap(2),
            },
            key: keys,
            aggregate_key_bytes: 1_048_576,
        },
    };
    let measured_checked = measured_binding
        .capture_settlement(
            &checked_policy,
            &retained,
            PhloOutcome::Accepted(&[]),
            measured_settlement_limits,
            &budget(),
        )
        .unwrap();
    let stale_snapshot = authorize_offered_direct_wallet_funding(&envelope, funding_limits)
        .unwrap()
        .read_policy_snapshot(manager, wallet_pre_state, cap(2), &budget())
        .await
        .unwrap();
    let stale_policy = stale_snapshot
        .bind_native_family_in_context(
            &view,
            &family,
            PhloFundingTerms {
                required_owner_ceilings: view.controls().required_owner_ceilings,
                asset: schedule.environment.asset,
                schedule_commitment: schedule.commitment,
            },
            signed_limits,
            search_limits,
            policy,
            &budget(),
        )
        .unwrap()
        .unwrap();
    let stale_error = measured_binding
        .capture_settlement(
            &stale_policy,
            &retained,
            PhloOutcome::Accepted(&[]),
            measured_settlement_limits,
            &budget(),
        )
        .unwrap_err();
    assert!(stale_error.to_string().contains("different state roots"));
    assert_eq!(measured_checked.capture().scoped().capture().branch(), 1);
    assert_eq!(
        measured_checked.capture().amounts(),
        checked.capture().amounts()
    );
    assert!(measured_binding
        .capture_settlement(
            &checked_policy,
            &[],
            PhloOutcome::Accepted(&[]),
            measured_settlement_limits,
            &budget(),
        )
        .is_err());
    assert!(measured_binding
        .capture_settlement(
            &checked_policy,
            &retained,
            PhloOutcome::AdmissionRejected,
            measured_settlement_limits,
            &budget(),
        )
        .is_err());
    let other_controls = check_phlo_controls(
        schedule.environment,
        policy.genesis().minimum_price(),
        u64::MAX,
        SignedPhloControls {
            limit: 7,
            ..view.controls()
        },
        schedule,
        1,
    )
    .unwrap();
    let other_binding = inventory
        .bind_measured_demand(
            NativePrepaidDemandInput {
                expected_root: pre_state.as_ref().try_into().unwrap(),
                controls: other_controls,
                demand: &acquisition,
                draws: &draws,
                demand_positions: &[0],
            },
            measured_limits,
            &budget(),
        )
        .unwrap();
    assert!(other_binding
        .capture_settlement(
            &checked_policy,
            &retained,
            PhloOutcome::Accepted(&[]),
            measured_settlement_limits,
            &budget(),
        )
        .is_err());
    let checked = measured_checked;
    let canonical = checked.capture().scoped().capture();
    let column = canonical
        .obligations()
        .position(|column| matches!(column.key(), PhloObligationKey::RetainedResource(_)))
        .unwrap();
    let root = Blake2b256Hash::from_bytes_prost(pre_state);
    let channel = supply::supply_channel(&authority);
    let datum = ListParWithRandom {
        pars: Vec::new(),
        random_state: vec![43],
        cost_authority: None,
        cost_stack: Some(CostStack {
            cells: vec![signature.clone()],
        }),
    };
    let mut play = RuntimeOps::new(manager.spawn_runtime().await);
    play.runtime.reset(&root).await.unwrap();
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum.clone(), false)
        .await
        .unwrap();
    let stack = supply::decode_purse_inventory(&play.get_data_datums(&channel).await, &signature)
        .unwrap()
        .stacks
        .into_iter()
        .find(|stack| stack.random_state == vec![43])
        .unwrap();
    let born = AuthorityBornStack {
        stack_id: stack.instance_id,
        produce_hash: stack.source_hash,
        cells: stack.stack.cells.clone(),
    };
    let births_limit = NativeRetainedBirthLimits {
        funding: RetainedBirthFundingLimits {
            births: 1,
            cells: 1,
            obligations: 2,
            authority_bytes: 524_288,
        },
        physical_cells: 3,
        physical_bytes: 524_288,
    };
    let positions = [column];
    let births = play
        .capture_retained_births(
            &checked,
            policy,
            &[RetainedBirthFunding {
                birth: &born,
                obligation_positions: &positions,
            }],
            births_limit,
            &budget(),
        )
        .await
        .unwrap();
    let records = births
        .prepare_cell_records(
            NativeRetainedRecordLimits {
                backing: RetainedCellBackingLimits {
                    cells: 1,
                    contributions: 1,
                },
                cell: cell_limit,
                stack: stack_limit,
                aggregate_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let receipt_key = PrepaidReceiptBucket::key_for_source(&born.produce_hash);
    let absence = manager
        .capture_prepaid_receipts(
            snapshot.wallets().pre_state_root(),
            &[receipt_key],
            receipts,
            &budget(),
        )
        .unwrap();
    let prepare = || {
        records
            .prepare_insertions(&absence, bucket_limit, receipts, &budget())
            .unwrap()
    };
    let birth_root = play.runtime.create_checkpoint().await.root;
    play.runtime.reset(&root).await.unwrap();
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum.clone(), false)
        .await
        .unwrap();
    let limits = NativeRetainedSettlementLimits {
        births: births_limit,
        receipts,
    };
    let error = play
        .apply_retained_wallet_settlement(
            prepare(),
            [0xd1; 32],
            treasury,
            Blake2b512Random::create_from_bytes(&[0xd1]),
            limits,
            &budget(),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires checked physical consumption"),
        "{error}"
    );
    for invalid_draws in [
        Vec::new(),
        vec![PrepaidStackPop {
            count: 0,
            ..draws[0]
        }],
        vec![PrepaidStackPop {
            count: 2,
            ..draws[0]
        }],
        vec![PrepaidStackPop {
            receipt_index: 1,
            ..draws[0]
        }],
        vec![PrepaidStackPop {
            stack_id: [0; 32],
            ..draws[0]
        }],
        vec![draws[0], draws[0]],
    ] {
        play.apply_prepaid_retained_wallet_settlement(
            prepare(),
            NativePrepaidConsumption {
                draws: &invalid_draws,
                limits: PrepaidStackPopLimits {
                    draws: 2,
                    ..pop_limits
                },
                ..consumption()
            },
            wallet(),
            limits,
            &budget(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            play.read_prepaid_receipt(
                &PrepaidReceiptBucket::key_for_source(&old_source),
                receipts.value_bytes
            )
            .await
            .unwrap(),
            Some(old_bucket.clone())
        );
        assert!(play
            .read_prepaid_receipt(&receipt_key, receipts.value_bytes)
            .await
            .unwrap()
            .is_none());
    }
    for funding in [
        RetainedBirthFundingLimits {
            births: 0,
            ..births_limit.funding
        },
        RetainedBirthFundingLimits {
            cells: 0,
            ..births_limit.funding
        },
        RetainedBirthFundingLimits {
            obligations: 1,
            ..births_limit.funding
        },
        RetainedBirthFundingLimits {
            authority_bytes: 0,
            ..births_limit.funding
        },
    ] {
        let error = play
            .apply_prepaid_retained_wallet_settlement(
                prepare(),
                consumption(),
                wallet(),
                NativeRetainedSettlementLimits {
                    births: NativeRetainedBirthLimits {
                        funding,
                        ..births_limit
                    },
                    ..limits
                },
                &budget(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("structural limit"), "{error}");
    }
    let error = play
        .apply_prepaid_retained_wallet_settlement(
            prepare(),
            consumption(),
            wallet(),
            NativeRetainedSettlementLimits {
                receipts: PrepaidReceiptLimits {
                    value_bytes: 1,
                    ..receipts
                },
                ..limits
            },
            &budget(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("oversized value"), "{error}");
    assert_eq!(play.runtime.create_checkpoint().await.root, birth_root);
    assert_eq!(
        system_vault_balance(manager, &birth_root.to_bytes_prost(), &payer.address).await,
        6
    );
    assert!(play
        .read_prepaid_receipt(&receipt_key, receipts.value_bytes)
        .await
        .unwrap()
        .is_none());

    play.runtime.reset(&root).await.unwrap();
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum.clone(), false)
        .await
        .unwrap();
    play.apply_prepaid_retained_wallet_settlement(
        prepare(),
        NativePrepaidConsumption {
            limits: PrepaidStackPopLimits {
                bucket: PrepaidReceiptBucketLimits {
                    occurrences: 0,
                    ..bucket_limit
                },
                ..pop_limits
            },
            ..consumption()
        },
        wallet(),
        limits,
        &budget(),
    )
    .await
    .unwrap_err();
    assert_eq!(play.runtime.create_checkpoint().await.root, birth_root);
    assert_eq!(
        system_vault_balance(manager, &birth_root.to_bytes_prost(), &payer.address).await,
        6
    );
    play.runtime.reset(&root).await.unwrap();
    let outer = play.runtime.create_soft_checkpoint().await;
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum.clone(), false)
        .await
        .unwrap();
    let settled = play
        .apply_prepaid_retained_wallet_settlement(
            prepare(),
            consumption(),
            wallet(),
            limits,
            &budget(),
        )
        .await
        .unwrap();
    let stored = play
        .read_prepaid_receipt(&receipt_key, receipts.value_bytes)
        .await
        .unwrap()
        .unwrap();
    let bucket = PrepaidReceiptBucket::decode(&stored, bucket_limit).unwrap();
    let cells = OrderedPrepaidCells::decode(bucket.receipts()[0], stack_limit).unwrap();
    let cell = NativePrepaidCell::decode(policy, cells.cells()[0], cell_limit).unwrap();
    assert_eq!(cell.acquisition_value(), 1);
    assert_eq!(cell.contributions()[0].custody, payer.custody_key);
    assert_eq!(cell.contributions()[0].amount, 1);
    assert!(play
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&old_source),
            receipts.value_bytes
        )
        .await
        .unwrap()
        .is_none());
    let tail = supply::decode_purse_inventory(&play.get_data_datums(&channel).await, &signature)
        .unwrap()
        .stacks
        .into_iter()
        .find(|stack| stack.random_state == vec![42])
        .unwrap();
    assert_eq!(tail.stack.cells, vec![signature.clone()]);
    let tail_record = play
        .read_prepaid_receipt(
            &PrepaidReceiptBucket::key_for_source(&tail.source_hash),
            receipts.value_bytes,
        )
        .await
        .unwrap()
        .unwrap();
    let tail_bucket = PrepaidReceiptBucket::decode(&tail_record, bucket_limit).unwrap();
    let tail_cells = OrderedPrepaidCells::decode(tail_bucket.receipts()[0], stack_limit).unwrap();
    assert_eq!(tail_cells.cells(), &[old_cells[1].as_slice()]);
    play.runtime.revert_to_soft_checkpoint(outer).await;
    assert_eq!(play.runtime.create_checkpoint().await.root, root);
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum.clone(), false)
        .await
        .unwrap();
    play.apply_prepaid_retained_wallet_settlement(
        prepare(),
        consumption(),
        wallet(),
        limits,
        &budget(),
    )
    .await
    .unwrap();
    let post = play.runtime.create_checkpoint().await.root;
    assert_eq!(
        system_vault_balance(manager, &post.to_bytes_prost(), &payer.address).await,
        4
    );
    let mut replay = RuntimeOps::new(manager.spawn_replay_runtime().await);
    replay.runtime.reset(&root).await.unwrap();
    replay
        .runtime
        .rig(
            settled
                .log
                .iter()
                .map(casper::rust::util::event_converter::to_rspace_event)
                .collect(),
        )
        .await
        .unwrap();
    replay
        .runtime
        .reducer
        .space
        .produce(channel, datum, false)
        .await
        .unwrap();
    replay
        .apply_prepaid_retained_wallet_settlement(
            prepare(),
            consumption(),
            wallet(),
            limits,
            &budget(),
        )
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(replay.runtime.create_checkpoint().await.root, post);
}

pub(super) async fn verify(
    manager: &RuntimeManager,
    checked: &CheckedDirectWalletSettlement<'_, OfferedFundedDeploy>,
    policy: &AdoptedResourcePolicy,
    treasury: &VaultAddress,
    expected_post: &StateHash,
    charged: bool,
) {
    let root = checked.snapshot().wallets().pre_state_root();
    let root_hash = Blake2b256Hash::from_bytes(root.to_vec());
    let wire = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    let limits = NativeRetainedSettlementLimits {
        births: NativeRetainedBirthLimits {
            funding: RetainedBirthFundingLimits {
                births: 16,
                cells: 32,
                obligations: 16,
                authority_bytes: 524_288,
            },
            physical_cells: 32,
            physical_bytes: 524_288,
        },
        receipts: PrepaidReceiptLimits {
            entries: 16,
            value_bytes: 524_288,
            batch_bytes: 1_048_576,
        },
    };
    let records_limits = NativeRetainedRecordLimits {
        backing: RetainedCellBackingLimits {
            cells: 32,
            contributions: 128,
        },
        cell: NativePrepaidCellLimits {
            wire,
            authority_nodes: 128,
            sources: 64,
        },
        stack: PrepaidCellLimits { cells: 32, wire },
        aggregate_bytes: 1_048_576,
    };
    let bucket = PrepaidReceiptBucketLimits {
        occurrences: 16,
        wire,
    };
    let absence = manager
        .capture_prepaid_receipts(root, &[], limits.receipts, &budget())
        .unwrap();
    let mut play = RuntimeOps::new(manager.spawn_runtime().await);
    play.runtime.reset(&root_hash).await.unwrap();
    let births = play
        .capture_retained_births(checked, policy, &[], limits.births, &budget())
        .await
        .unwrap();
    let records = births
        .prepare_cell_records(records_limits, &budget())
        .unwrap();
    let prepare = || {
        records
            .prepare_insertions(&absence, bucket, limits.receipts, &budget())
            .unwrap()
    };
    let settled = play
        .apply_retained_wallet_settlement(
            prepare(),
            [0xcd; 32],
            treasury,
            Blake2b512Random::create_from_bytes(&[0xcd]),
            limits,
            &budget(),
        )
        .await
        .unwrap();
    assert!(!settled.log.is_empty());
    assert_eq!(
        play.runtime.create_checkpoint().await.root.to_bytes_prost(),
        *expected_post
    );
    let mut replay = RuntimeOps::new(manager.spawn_replay_runtime().await);
    replay.runtime.reset(&root_hash).await.unwrap();
    replay
        .runtime
        .rig(
            settled
                .log
                .iter()
                .map(casper::rust::util::event_converter::to_rspace_event)
                .collect(),
        )
        .await
        .unwrap();
    replay
        .apply_retained_wallet_settlement(
            prepare(),
            [0xcd; 32],
            treasury,
            Blake2b512Random::create_from_bytes(&[0xcd]),
            limits,
            &budget(),
        )
        .await
        .unwrap();
    replay.runtime.check_replay_data().await.unwrap();
    assert_eq!(
        replay
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost(),
        *expected_post
    );

    play.runtime.reset(&root_hash).await.unwrap();
    let outer = play.runtime.create_soft_checkpoint().await;
    let channel = new_gstring_par("funded-checkpoint-application".into(), Vec::new(), false);
    let datum = ListParWithRandom {
        pars: vec![RhoNumber::create_par(41)],
        random_state: vec![41],
        ..Default::default()
    };
    play.runtime
        .reducer
        .space
        .produce(channel.clone(), datum, false)
        .await
        .unwrap();
    let prefix = play.runtime.take_event_log().await;
    assert_eq!(prefix.len(), 1);
    let applied = play
        .apply_retained_wallet_settlement(
            prepare(),
            [0xcd; 32],
            treasury,
            Blake2b512Random::create_from_bytes(&[0xcd]),
            limits,
            &budget(),
        )
        .await
        .unwrap();
    assert_eq!(applied.log, settled.log);
    assert_eq!(play.get_data_datums(&channel).await.len(), 1);
    play.runtime.revert_to_soft_checkpoint(outer).await;
    assert!(play.get_data_datums(&channel).await.is_empty());
    assert_eq!(play.runtime.create_checkpoint().await.root, root_hash);

    if charged {
        play.runtime.reset(&root_hash).await.unwrap();
        play.apply_retained_wallet_settlement(
            prepare(),
            [0xcd; 32],
            treasury,
            Blake2b512Random::create_from_bytes(&[0xcd]),
            limits,
            &budget(),
        )
        .await
        .unwrap();
        assert!(play
            .apply_retained_wallet_settlement(
                prepare(),
                [0xcd; 32],
                treasury,
                Blake2b512Random::create_from_bytes(&[0xcd]),
                limits,
                &budget(),
            )
            .await
            .is_err());
        assert_eq!(
            play.runtime.create_checkpoint().await.root.to_bytes_prost(),
            *expected_post
        );
        play.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(expected_post))
            .await
            .unwrap();
        assert!(play
            .apply_retained_wallet_settlement(
                prepare(),
                [0xcd; 32],
                treasury,
                Blake2b512Random::create_from_bytes(&[0xcd]),
                limits,
                &budget(),
            )
            .await
            .is_err());
        assert_eq!(
            play.runtime.create_checkpoint().await.root.to_bytes_prost(),
            *expected_post
        );
    }
}
