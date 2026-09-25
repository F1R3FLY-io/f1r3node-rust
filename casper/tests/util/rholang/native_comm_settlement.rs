use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    NativeAttemptSettlementInput, NativeAttemptSettlementLimits, NativeFundedExecutionContext,
    NativeFundedReplayInput, NativeFundedReplayLimits,
};
use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use casper::rust::util::rholang::costacc::prepaid_receipts::{
    NativeMeasuredSettlementLimits, NativePrepaidCellLimits, NativePrepaidConsumption,
    NativePrepaidDemandLimits, NativePrepaidInventoryLimits, NativeRetainedBirthLimits,
    NativeRetainedRecordLimits, NativeRetainedSettlementLimits, NativeWalletSettlement,
    PrepaidCellLimits, PrepaidReceiptBucketLimits, PrepaidReceiptLimits, PrepaidStackCaptureLimits,
    PrepaidStackPop, PrepaidStackPopLimits,
};
use crypto::rust::signatures::signed::Cosigner;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits};
use models::rust::utils::new_gint_par;
use prost::Message;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    NativePhloAcquisitionLimits, NativePhloPurseLimits, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding;
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_counted_phlo_execution, CountedPhloExecutionWitness, PhloObligationFundingInput,
    PhloObligationFundingLimits, PhloResource, PhloResourceAmount, PhloSourceConsent,
    RetainedBirthFundingLimits, RetainedCellBackingLimits,
};
use rholang::rust::interpreter::accounting::{
    NativeOperationJournalLimits, NativeOperationTraceLimits,
};

use super::*;

#[path = "native_comm_settlement/prepaid.rs"]
mod prepaid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_comm_settles_compute_and_bytes_at_the_adopted_price() { run_cases(false).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_comm_consumes_rooted_prepaid_resources_without_double_charging() {
    run_cases(true).await;
}

async fn run_cases(prepaid: bool) {
    with_runtime_manager(move |manager, genesis, block| async move {
        let (_, _, minimum) =
            casper::rust::util::token_metadata_check::read_on_chain_consensus_parameters(
                &manager,
                &block.body.state.post_state_hash,
            )
            .await
            .unwrap();
        let shard = casper::rust::casper::CasperShardConf {
            min_phlo_price: minimum,
            casper_version: block.header.version,
            shard_name: block.shard_id.clone(),
            ..casper::rust::casper::CasperShardConf::new()
        };
        let (original, limits) =
            funded_snapshot_envelope(PrivateKey::from_bytes(&[0xca; 32]), &shard);
        let treasury = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        for (owners, failed) in [(1, false), (1, true), (3, false), (3, true)] {
            verify(
                &manager,
                &original,
                limits,
                u64::try_from(minimum).unwrap(),
                &treasury,
                &block,
                (owners, failed, prepaid),
            )
            .await;
        }
    })
    .await
    .unwrap();
}

async fn verify(
    manager: &RuntimeManager,
    original: &Cosigned<FundedDeploy>,
    mut limits: DirectWalletFundingLimits,
    minimum: u64,
    treasury: &VaultAddress,
    block: &BlockMessage,
    scenario: (usize, bool, bool),
) {
    let (owners, failed, prepaid) = scenario;
    eprintln!("native funded replay: owners={owners}, failed={failed}, prepaid={prepaid}");
    let cap = |n| NonZeroUsize::new(n).unwrap();
    let budget = || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
    let keys: Vec<_> = (0..owners)
        .map(|index| PrivateKey::from_bytes(&[0xca + u8::try_from(index).unwrap(); 32]))
        .collect();
    let mut members: Vec<_> = keys
        .iter()
        .map(|key| {
            (
                Cosigner {
                    pk: Secp256k1.to_public(key),
                    sig: Vec::new().into(),
                    sig_algorithm: Box::new(Secp256k1),
                },
                key,
            )
        })
        .collect();
    members.sort_by_key(|(signer, _)| signer.principal_bytes_v61().unwrap());
    let principals: Vec<_> = members
        .iter()
        .map(|(signer, _)| signer.principal_bytes_v61().unwrap())
        .collect();
    let authority =
        accounting::funding_sig_compound(&principals.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let wallets: Vec<_> = members
        .iter()
        .map(|(signer, _)| VaultAddress::from_public_key(&signer.pk).unwrap())
        .collect();
    let payers: Vec<_> = principals
        .iter()
        .map(|principal| {
            vault_payer(&CostSignature {
                value: Some(Value::Ground(principal.clone())),
            })
            .unwrap()
        })
        .collect();
    let mut record =
        PhloFundingIntentV1::decode(original.data.funding_intent(), limits.funding).unwrap();
    limits.funding.controls.total_classes = 4;
    limits.funding.resource_permissions = 4 * owners;
    limits.funding.authority_nodes = 1024;
    limits.members = cap(owners);
    limits.funding.controls.owners = owners;
    limits.funding.sources = owners;
    let weights = [2, 3, 5, 7];
    record.controls.permitted_schedules[0].classes = NativePhloDimension::ALL
        .into_iter()
        .zip(weights)
        .zip([
            b"compute".as_slice(),
            b"introduction",
            b"transfer",
            b"trace",
        ])
        .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
        .collect();
    record.controls.permitted_schedules[0].actual_price = 3;
    record.controls.limit = 1_000_000;
    record.controls.price_ceiling = 3;
    record.controls.required_owner_ceilings = vec![3; owners];
    record.total_exposure = 3_000_001;
    record.schedule_commitment = record.controls.permitted_schedules[0]
        .digest(limits.funding.controls.schedule(4))
        .unwrap();
    let mut parameters =
        crate::util::genesis_builder::GenesisBuilder::build_genesis_parameters_with_defaults(
            None, None,
        )
        .2;
    parameters
        .vaults
        .iter_mut()
        .find(|vault| vault.vault_address.to_base58() == treasury.to_base58())
        .unwrap()
        .initial_balance = 4_000_000 * u64::try_from(owners).unwrap() + 1_000_000;
    parameters.resource_policy = Some(
        models::rust::phlo_schedule::PhloGenesisPolicy::from_schedule(
            &record.controls.permitted_schedules[0],
        )
        .unwrap(),
    );
    let genesis =
        casper::rust::genesis::genesis::Genesis::create_genesis_block(manager, &parameters)
            .await
            .unwrap();
    let shard = casper::rust::casper::CasperShardConf {
        min_phlo_price: i64::try_from(minimum).unwrap(),
        casper_version: block.header.version,
        shard_name: block.shard_id.clone(),
        ..casper::rust::casper::CasperShardConf::new()
    };
    let adopted_policy = AdoptedResourcePolicy::load(manager, &genesis, &shard)
        .await
        .unwrap();
    let adopted = &adopted_policy;
    let mut seed = RuntimeOps::new(manager.spawn_runtime().await);
    let mut funded_root = genesis.body.state.post_state_hash.clone();
    for (index, wallet) in wallets.iter().enumerate() {
        funded_root = successful_system_state(
            seed.play_system_deploy(
                &funded_root,
                &mut transfer(
                    treasury,
                    wallet,
                    4_000_000,
                    0xde + u8::try_from(index).unwrap(),
                ),
            )
            .await
            .unwrap(),
        );
    }
    let terms = record.controls.permitted_schedules[0]
        .encode(limits.funding.controls.schedule(4))
        .unwrap();
    let location = supply::supply_channel(&authority).encode_to_vec();
    let permissions: Vec<_> = (0..4)
        .map(|class| PhloResource {
            location: &location,
            class,
            acquisition_terms: &terms,
            authority: &authority,
        })
        .collect();
    let execution_limits = PhloExecutionLimits {
        resource_entries: 128,
        authority_nodes: 1024,
        key_bytes: 1_048_576,
    };
    let prepaid_seed = if prepaid {
        let seeded = prepaid::seed(
            manager,
            adopted,
            &funded_root,
            treasury,
            &payers,
            permissions[1],
            (limits.funding.wire, execution_limits),
        )
        .await;
        funded_root = seeded.root.clone();
        Some(seeded)
    } else {
        None
    };
    let root = &funded_root;
    let initial_balance = if prepaid { 4_000_000 - 18 } else { 4_000_000 };
    record.sources = payers
        .iter()
        .map(|payer| {
            PhloSourceConsent {
                custody: &payer.custody_key,
                hold_cap: 3_000_001,
                debit_cap: 3_000_001,
                fee_permitted: true,
                resources: &permissions,
            }
            .wire_policy(execution_limits, PhloSourceLimits {
                wire: limits.funding.wire,
                resource_permissions: 4,
                authority_nodes: 128,
            })
            .unwrap()
        })
        .collect();
    let payload = FundedDeployLimits {
        deploy_bytes: 2_097_152,
        signing: limits.funding.wire,
        funding: limits.funding,
    };
    let mut body = original.data.body().clone();
    body.term = if failed {
        "@99!(9) | new c in { c!(7) | for (@x <- c) { @0!(x + true) } }"
    } else {
        "@99!(9) | new c in { c!(7) | for (@x <- c) { @0!(x) } }"
    }
    .to_owned();
    let data = OfferedFundedDeploy::new(
        body,
        record.encode(limits.funding).unwrap(),
        1_000_000,
        3,
        payload,
    )
    .unwrap();
    let unsigned: Vec<_> = members.iter().map(|(signer, _)| signer.clone()).collect();
    let mut bitmap = vec![0; owners.div_ceil(8)];
    for index in 0..owners {
        bitmap[index / 8] |= 1 << (index % 8);
    }
    let threshold = u32::try_from(owners).unwrap();
    let hash = Cosigned::envelope_signing_hash_for_presence(
        &data,
        &unsigned,
        threshold,
        &bitmap,
        "secp256k1",
    )
    .unwrap();
    for (signer, key) in &mut members {
        signer.sig = Secp256k1.sign(&hash, &key.bytes).into();
    }
    let signed = Cosigned::from_envelope_signed_data_threshold(
        data,
        members.into_iter().map(|(signer, _)| signer).collect(),
        threshold,
    )
    .unwrap();
    let decoded =
        PhloFundingIntentV1::decode(signed.data.funding_intent(), limits.funding).unwrap();
    let intent = PhloFundingIntentBinding::new(&decoded, limits.funding).unwrap();
    let view = intent.view().unwrap();
    let schedule = view.controls().permitted_schedules[0];
    let schedule_binding = PhloScheduleBinding::new(
        &decoded.controls.permitted_schedules[0],
        limits.funding.controls.schedule(4),
    )
    .unwrap();
    let controls = check_phlo_controls(
        schedule.environment,
        adopted.genesis().minimum_price(),
        u64::MAX,
        view.controls(),
        schedule,
        1_000_000,
    )
    .unwrap();
    let envelope = DeployEnvelope::from_proto(
        OfferedFundedDeploy::to_proto(&signed).unwrap(),
        DeployEnvelopeLimits {
            payload,
            members: limits.members,
        },
    )
    .unwrap();
    let regions = NativePhloRegionLimits {
        regions: 32,
        encoded_authority_bytes: 1_048_576,
    };
    let mut probe = manager.spawn_runtime().await;
    probe
        .reset(&Blake2b256Hash::from_bytes_prost(root))
        .await
        .unwrap();
    probe.set_block_data(BlockData::from_block(block)).await;
    probe
        .set_deploy_data(
            rholang::rust::interpreter::system_processes::DeployData::from_envelope(&envelope),
        )
        .await;
    probe.cost.set_unmetered(false);
    probe.cost.set_deploy_id_funded(
        envelope.identity().as_bytes().try_into().unwrap(),
        accounting::funding_sig(&signed),
    );
    let measured = probe
        .evaluate_with_native_phlo(
            &signed.data.body().term,
            models::rust::normalizer_env::normalizer_env_from_envelope(&envelope),
            casper::rust::util::rholang::tools::Tools::user_envelope_rng(&envelope),
            None,
            accounting::NativeRuntimeConfig::new(
                adopted
                    .bind_execution_contract(controls, &schedule_binding)
                    .unwrap(),
                accounting::native_phlo_rules::NativeBudgetTraceLimits {
                    attempts: 100_000,
                    path_segments: 4096,
                    regions,
                },
                budget(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(!measured.errors.is_empty(), failed, "{:?}", measured.errors);
    if failed {
        assert!(measured.economic_failures.contains(PhloFailure::User));
        assert!(measured.economic_failures.permits_retained_charge());
    }
    let mut quantities = [0_u64; 4];
    let expected_signature = accounting::authority::sig_to_cost_signature(&authority).unwrap();
    for row in &measured.byte_observations.rows {
        assert!(!row.authority.regions.is_empty());
        for region in &row.authority.regions {
            assert_eq!(region.signature.as_ref(), Some(&expected_signature));
        }
        let raw = row.measurement.unwrap();
        let values = [
            u64::from(row.kind == accounting::authority::AuthorityByteEventKind::Comm),
            raw.introduction_bytes,
            raw.transfer_bytes,
            raw.trace_bytes,
        ];
        for (total, value) in quantities.iter_mut().zip(values) {
            *total += value * u64::try_from(row.authority.regions.len()).unwrap();
        }
    }
    assert_eq!(
        measured
            .byte_observations
            .rows
            .iter()
            .filter(|row| row.kind == accounting::authority::AuthorityByteEventKind::Comm)
            .count(),
        1
    );
    assert!(quantities.iter().all(|quantity| *quantity > 0));
    let expected_usage: u64 = quantities
        .iter()
        .zip(weights)
        .map(|(quantity, weight)| quantity * weight * u64::try_from(owners).unwrap())
        .sum();
    assert_eq!(measured.native_phlo_usage, Some(expected_usage));
    let required: Vec<_> = permissions
        .iter()
        .zip(quantities)
        .map(|(resource, quantity)| PhloResourceAmount {
            resource: *resource,
            quantity,
        })
        .collect();
    let prepaid_used: Vec<_> = prepaid_seed
        .as_ref()
        .map(|_| PhloResourceAmount {
            resource: permissions[1],
            quantity: 1,
        })
        .into_iter()
        .collect();
    let mut fresh = required.clone();
    if prepaid {
        assert!(fresh[1].quantity > 1);
        fresh[1].quantity -= 1;
    }
    let execution = check_counted_phlo_execution(
        controls,
        CountedPhloExecutionWitness {
            available: &prepaid_used,
            required: &required,
            used: &prepaid_used,
            unused: &[],
            fresh: &fresh,
        },
        execution_limits,
    )
    .unwrap();
    let failures = if failed {
        &[PhloFailure::User][..]
    } else {
        &[]
    };
    let obligations =
        project_phlo_obligations(execution, PhloOutcome::Accepted(failures), cap(5)).unwrap();
    let expected_acquisition = expected_usage * 3 - if prepaid { 9 * owners as u64 } else { 0 };
    assert_eq!(obligations.total(), 1 + expected_acquisition);
    let snapshot = authorize_offered_direct_wallet_funding(&signed, limits)
        .unwrap()
        .read_policy_snapshot(manager, root.clone(), cap(owners + 1), &budget())
        .await
        .unwrap();
    let sources: Vec<_> = snapshot
        .wallets()
        .sources()
        .iter()
        .map(|s| s.source())
        .collect();
    let eligible = vec![vec![true; 5]; owners];
    let capacities: Vec<_> = sources.iter().map(|source| source.capacity).collect();
    let source_keys: Vec<_> = sources.iter().map(|source| source.custody).collect();
    let keys = PhloObligationKeyLimits {
        wire: limits.funding.wire,
        authority_nodes: 128,
    };
    let selection = obligations
        .select_funding(
            PhloObligationFundingInput {
                source_keys: &source_keys,
                capacities: &capacities,
                eligible: &eligible,
                canonical_resource_cursor: snapshot
                    .resource_cursor()
                    .map_or(0, |cursor| cursor.position_index(cap(owners)).unwrap()),
                canonical_fee_cursor: snapshot
                    .fee_cursor()
                    .map_or(0, |cursor| cursor.position_index(cap(owners)).unwrap()),
            },
            PhloObligationFundingLimits {
                search: FundingSearchLimits {
                    source_cap: cap(owners),
                    obligation_cap: cap(5),
                },
                keys,
                aggregate_key_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap()
        .unwrap();
    let cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &eligible,
        assignment: selection.assignment(),
    }];
    let family_limits = PhloFundingLimits {
        sources: cap(owners),
        cases: cap(1),
        obligations: cap(5),
        assignment_cells: 5 * owners,
        custody_bytes: 32 * owners,
    };
    let family =
        check_phlo_funding_family(&sources, &cases, decoded.total_exposure, family_limits).unwrap();
    let checked = snapshot
        .bind_native_family_in_context(
            &view,
            &family,
            PhloFundingTerms {
                required_owner_ceilings: view.controls().required_owner_ceilings,
                asset: schedule.environment.asset,
                schedule_commitment: schedule.commitment,
            },
            SignedPhloConsentLimits {
                members: cap(owners),
                intent: limits.funding,
                consent: PhloConsentLimits {
                    sources: owners,
                    permission_entries: 4 * owners,
                    case_cells: 5 * owners,
                    authority_nodes: 128,
                    key_bytes: 1_048_576,
                },
            },
            PhloFamilyFundingLimits {
                funding: family_limits,
                keys,
                aggregate_key_bytes: 1_048_576,
            },
            adopted,
            &budget(),
        )
        .unwrap()
        .unwrap();
    let cell = NativePrepaidCellLimits {
        wire: limits.funding.wire,
        authority_nodes: 128,
        sources: owners,
    };
    let cells = PrepaidCellLimits {
        cells: 2,
        wire: limits.funding.wire,
    };
    let stacks = prepaid_seed
        .as_ref()
        .map_or(&[][..], |seed| seed.stacks.as_slice());
    let draws: Vec<_> = stacks
        .iter()
        .map(|stack| PrepaidStackPop {
            stack_id: stack.instance_id,
            receipt_index: 0,
            count: 1,
        })
        .collect();
    let measured_demand = adopted
        .native_rules()
        .measure(&measured.byte_observations, 32)
        .unwrap();
    let measured_regions = measured_demand.region_demands(regions, &budget()).unwrap();
    let measured_locations = measured_regions
        .locate_purses(
            NativePhloPurseLimits {
                bindings: 32,
                encoded_binding_bytes: 1_048_576,
            },
            &budget(),
        )
        .unwrap();
    let acquisition = measured_locations
        .prepare_acquisition_demand(
            &schedule_binding,
            &terms,
            NativePhloAcquisitionLimits {
                schedule: limits.funding.controls.schedule(4),
                entries: 32,
            },
            &budget(),
        )
        .unwrap();
    let demand_positions: Vec<_> = prepaid_seed
        .as_ref()
        .map(|_| {
            acquisition
                .resources()
                .iter()
                .position(|entry| entry.resource.class == 1 && entry.quantity > 0)
                .unwrap()
        })
        .into_iter()
        .collect();
    let captured = manager
        .capture_prepaid_stacks(
            root.as_ref().try_into().unwrap(),
            stacks,
            adopted,
            PrepaidStackCaptureLimits {
                stacks: 1,
                physical_cells: 2,
                physical_bytes: 1_048_576,
                bucket: PrepaidReceiptBucketLimits {
                    occurrences: 1,
                    wire: limits.funding.wire,
                },
                cells,
                native_cell: cell,
                receipts: PrepaidReceiptLimits {
                    entries: 1,
                    value_bytes: 1_048_576,
                    batch_bytes: 1_048_576,
                },
            },
            &budget(),
        )
        .unwrap();
    let inventory = captured
        .resource_inventory(
            NativePrepaidInventoryLimits {
                cells,
                cell,
                execution: execution_limits,
            },
            &budget(),
        )
        .unwrap();
    let settlement_limits = NativeAttemptSettlementLimits {
        observations: 32,
        regions,
        purses: NativePhloPurseLimits {
            bindings: 32,
            encoded_binding_bytes: 1_048_576,
        },
        acquisition: NativePhloAcquisitionLimits {
            schedule: limits.funding.controls.schedule(4),
            entries: 32,
        },
        demand: NativePrepaidDemandLimits {
            draws: 1,
            authority_bytes: 1_048_576,
            execution: execution_limits,
        },
        settlement: NativeMeasuredSettlementLimits {
            matching: PhloOutcomeMatchLimits {
                execution: execution_limits,
                key: keys,
                aggregate_key_bytes: 1_048_576,
                cases: cap(1),
            },
            capture: PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: cap(owners),
                    obligation_cap: cap(5),
                },
                key: keys,
                aggregate_key_bytes: 1_048_576,
            },
        },
    };
    let context = || NativeFundedExecutionContext {
        block_data: BlockData::from_block(block),
        invalid_blocks: HashMap::new(),
        trace: accounting::native_phlo_rules::NativeBudgetTraceLimits {
            attempts: 100_000,
            path_segments: 4096,
            regions,
        },
        host_work: budget(),
    };
    let (first, second) = tokio::join!(
        manager.evaluate_native_funded(&checked, &envelope, adopted, &schedule_binding, context()),
        manager.evaluate_native_funded(&checked, &envelope, adopted, &schedule_binding, context()),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let replay_limits = NativeFundedReplayLimits {
        journal: NativeOperationJournalLimits {
            budget: context().trace,
            operations: 1000,
            total_path_segments: 100_000,
            source_entries: 100_000,
            footprint_entries: 100_000,
            footprint_bytes: 10_000_000,
            predecessor_edges: 100_000,
        },
        trace: NativeOperationTraceLimits {
            events: 1000,
            source_entries: 100_000,
            source_bytes: 10_000_000,
            telemetry_items: 10_000,
            telemetry_bytes: 1_000_000,
        },
    };
    let original_replay = first.replay_input().unwrap();
    let mut wrong_session = original_replay.recording.clone();
    wrong_session.session[0] ^= 1;
    let mut wrong_usage = original_replay.recording.clone();
    wrong_usage.used += 1;
    let empty_events = std::sync::Arc::from([]);
    for changed in [
        NativeFundedReplayInput {
            recording: &wrong_session,
            ..original_replay
        },
        NativeFundedReplayInput {
            recording: &wrong_usage,
            ..original_replay
        },
        NativeFundedReplayInput {
            events: &empty_events,
            ..original_replay
        },
    ] {
        assert!(manager
            .replay_native_funded(
                &checked,
                &envelope,
                adopted,
                &schedule_binding,
                context(),
                changed,
                replay_limits,
            )
            .await
            .is_err());
    }
    let mut rejected_context = context();
    rejected_context.host_work =
        HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(manager
        .replay_native_funded(
            &checked,
            &envelope,
            adopted,
            &schedule_binding,
            rejected_context,
            original_replay,
            replay_limits,
        )
        .await
        .is_err());
    let (replayed_first, replayed_second) = tokio::join!(
        manager.replay_native_funded(
            &checked,
            &envelope,
            adopted,
            &schedule_binding,
            context(),
            first.replay_input().unwrap(),
            replay_limits
        ),
        manager.replay_native_funded(
            &checked,
            &envelope,
            adopted,
            &schedule_binding,
            context(),
            second.replay_input().unwrap(),
            replay_limits
        ),
    );
    let mut post_roots = Vec::new();
    for mut attempt in [
        first,
        second,
        replayed_first.unwrap(),
        replayed_second.unwrap(),
    ] {
        assert_eq!(
            !attempt.evaluation().errors.is_empty(),
            failed,
            "{:?}",
            attempt.evaluation().errors
        );
        assert_eq!(attempt.evaluation().native_phlo_usage, Some(expected_usage));
        assert!(attempt
            .evaluation()
            .byte_observations
            .has_complete_measurements());
        let observed = &attempt.evaluation().byte_observations;
        assert_eq!(
            observed
                .rows
                .iter()
                .filter(|row| row.kind == accounting::authority::AuthorityByteEventKind::Comm)
                .count(),
            1
        );
        assert!(observed.rows.iter().any(|row| {
            row.measurement
                .as_ref()
                .is_some_and(|raw| raw.transfer_bytes > 0)
        }));
        for changed in [
            NativeAttemptSettlementInput {
                inventory: &inventory,
                schedule: &schedule_binding,
                terms: b"different acquisition terms",
                draws: &[],
                demand_positions: &[],
                retained: &[],
            },
            NativeAttemptSettlementInput {
                inventory: &inventory,
                schedule: &schedule_binding,
                terms: &terms,
                draws: &[],
                demand_positions: &[0],
                retained: &[],
            },
        ] {
            assert!(attempt
                .capture_measured_settlement(changed, settlement_limits, &budget())
                .is_err());
        }
        let settlement = attempt
            .capture_measured_settlement(
                NativeAttemptSettlementInput {
                    inventory: &inventory,
                    schedule: &schedule_binding,
                    terms: &terms,
                    draws: &draws,
                    demand_positions: &demand_positions,
                    retained: &[],
                },
                settlement_limits,
                &budget(),
            )
            .unwrap();
        assert_eq!(
            settlement
                .capture()
                .amounts()
                .iter()
                .map(|amount| amount.acquisition())
                .sum::<i64>(),
            i64::try_from(expected_acquisition).unwrap()
        );
        assert_eq!(
            settlement
                .capture()
                .amounts()
                .iter()
                .map(|amount| amount.fee())
                .sum::<i64>(),
            1
        );
        let receipt_limits = PrepaidReceiptLimits {
            entries: 2,
            value_bytes: 1_048_576,
            batch_bytes: 1_048_576,
        };
        let bucket_limits = PrepaidReceiptBucketLimits {
            occurrences: 1,
            wire: limits.funding.wire,
        };
        let birth_limits = NativeRetainedBirthLimits {
            funding: RetainedBirthFundingLimits {
                births: 0,
                cells: 0,
                obligations: 5,
                authority_bytes: 1_048_576,
            },
            physical_cells: 0,
            physical_bytes: 1_048_576,
        };
        let absence = manager
            .capture_prepaid_receipts(
                snapshot.wallets().pre_state_root(),
                &[],
                receipt_limits,
                &budget(),
            )
            .unwrap();
        let births = attempt
            .runtime()
            .capture_retained_births(&settlement, adopted, &[], birth_limits, &budget())
            .await
            .unwrap();
        let records = births
            .prepare_cell_records(
                NativeRetainedRecordLimits {
                    backing: RetainedCellBackingLimits {
                        cells: 0,
                        contributions: 0,
                    },
                    cell,
                    stack: cells,
                    aggregate_bytes: 1_048_576,
                },
                &budget(),
            )
            .unwrap();
        let prepared = || {
            records
                .prepare_insertions(&absence, bucket_limits, receipt_limits, &budget())
                .unwrap()
        };
        let consumption = || NativePrepaidConsumption {
            captured: &captured,
            draws: &draws,
            limits: PrepaidStackPopLimits {
                draws: 1,
                bucket: bucket_limits,
                cells,
                receipts: receipt_limits,
            },
            execution: execution_limits,
            cell,
            physical_cells: 2,
            physical_bytes: 1_048_576,
        };
        let wallet = || NativeWalletSettlement {
            reservation_id: [0xdd; 32],
            fee_address: treasury,
            initial_rand: Blake2b512Random::create_from_bytes(&[0xdd]),
        };
        let publication_limits = NativeRetainedSettlementLimits {
            births: birth_limits,
            receipts: receipt_limits,
        };
        let mut foreign = RuntimeOps::new(manager.spawn_runtime().await);
        let foreign_root = Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash);
        foreign.runtime.reset(&foreign_root).await.unwrap();
        let rejected = foreign
            .apply_prepaid_retained_wallet_settlement(
                prepared(),
                consumption(),
                wallet(),
                publication_limits,
                &budget(),
            )
            .await
            .unwrap_err();
        assert!(
            rejected.to_string().contains("authorized state root"),
            "{rejected}"
        );
        assert_eq!(foreign.runtime.create_checkpoint().await.root, foreign_root);
        if prepaid {
            let before = attempt.runtime().runtime.create_soft_checkpoint().await;
            let mut missing = consumption();
            missing.draws = &[];
            let rejected = attempt
                .runtime()
                .apply_prepaid_retained_wallet_settlement(
                    prepared(),
                    missing,
                    wallet(),
                    publication_limits,
                    &budget(),
                )
                .await
                .unwrap_err();
            assert!(
                rejected
                    .to_string()
                    .contains("omits a checked prepaid resource"),
                "{rejected}"
            );
            let after = attempt.runtime().runtime.create_soft_checkpoint().await;
            assert_eq!(before.cache_snapshot.data, after.cache_snapshot.data);
            assert_eq!(
                before.cache_snapshot.continuations,
                after.cache_snapshot.continuations
            );
            assert_eq!(
                before.cache_snapshot.installed_continuations,
                after.cache_snapshot.installed_continuations
            );
            assert_eq!(before.cache_snapshot.joins, after.cache_snapshot.joins);
            assert_eq!(
                before.cache_snapshot.installed_joins,
                after.cache_snapshot.installed_joins
            );
            assert_eq!(before.log, after.log);
            assert_eq!(before.produce_counter, after.produce_counter);
        }
        let settled = attempt
            .runtime()
            .apply_prepaid_retained_wallet_settlement(
                prepared(),
                consumption(),
                wallet(),
                publication_limits,
                &budget(),
            )
            .await
            .unwrap();
        assert!(!settled.log.is_empty());
        let post = attempt
            .runtime()
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost();
        for (wallet, payer) in wallets.iter().zip(&payers) {
            let index = snapshot
                .canonical_custodies()
                .iter()
                .position(|custody| **custody == payer.custody_key)
                .unwrap();
            let amounts = settlement.capture().amounts()[index];
            assert_eq!(
                system_vault_balance(manager, &post, wallet).await,
                initial_balance - amounts.acquisition() - amounts.fee()
            );
            assert_eq!(
                system_vault_balance(manager, root, wallet).await,
                initial_balance
            );
        }
        if let Some(seed) = &prepaid_seed {
            prepaid::verify_remaining(seed, attempt.runtime(), limits.funding.wire).await;
        }
        let data = attempt
            .runtime()
            .get_data_par(&new_gint_par(0, Vec::new(), false))
            .await;
        assert_eq!(
            data,
            if failed {
                Vec::new()
            } else {
                vec![new_gint_par(7, Vec::new(), false)]
            }
        );
        let marker = attempt
            .runtime()
            .get_data_par(&new_gint_par(99, Vec::new(), false))
            .await;
        assert_eq!(
            marker,
            if failed {
                Vec::new()
            } else {
                vec![new_gint_par(9, Vec::new(), false)]
            }
        );
        let mut duplicate = settlement
            .prepare_request(
                [0xdd; 32],
                treasury,
                Blake2b512Random::create_from_bytes(&[0xdd]),
                &budget(),
            )
            .unwrap();
        let duplicate_result = attempt
            .runtime()
            .play_system_deploy(&post, &mut duplicate.request().unwrap())
            .await
            .unwrap();
        assert!(matches!(
            duplicate_result,
            SystemDeployResult::PlayFailed { .. }
        ));
        let duplicate_post = attempt
            .runtime()
            .runtime
            .create_checkpoint()
            .await
            .root
            .to_bytes_prost();
        assert_eq!(post, duplicate_post);
        post_roots.push(post);
    }
    assert!(post_roots.iter().all(|root| root == &post_roots[0]));
}
