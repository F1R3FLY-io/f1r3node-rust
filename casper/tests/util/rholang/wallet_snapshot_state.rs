use std::num::NonZeroUsize;

use acceptance::{RuntimeManagerSupplyReader, SupplyReader};
use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    authorize_direct_wallet_funding, authorize_offered_direct_wallet_funding,
    DirectWalletBindingError, DirectWalletFundingLimits, DirectWalletPolicySnapshotError,
};
use models::rhoapi::cost_signature::Value;
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::{FundedDeploy, FundedDeployLimits, OfferedFundedDeploy};
use rholang::rust::interpreter::accounting::monetary_allocation::{
    FundingSearchLimits, MonetaryCursor, MonetaryCursorTransition,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloFundingTerms,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    check_phlo_execution, check_phlo_funding_family, project_phlo_obligations, PhloCaptureLimits,
    PhloConsentLimits, PhloExecutionLimits, PhloExecutionWitness, PhloFailure,
    PhloFamilyFundingLimits, PhloFundingCase, PhloFundingIntentBinding, PhloFundingLimits,
    PhloFundingSource, PhloOutcome, PhloOutcomeMatchLimits, SignedPhloConsentLimits,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::*;

#[path = "wallet_snapshot_context.rs"]
mod context;

async fn balances_at(
    manager: &RuntimeManager,
    state: &StateHash,
    signatures: &[CostSignature; 2],
) -> [i64; 2] {
    let reader = RuntimeManagerSupplyReader {
        runtime_manager: manager,
        pre_state_hash: state.clone(),
    };
    assert_eq!(reader.pre_state_root().as_slice(), state.as_ref());
    let (left, right) = tokio::join!(
        reader.read_purse(&signatures[0]),
        reader.read_purse(&signatures[1]),
    );
    [
        left.unwrap().balance.unwrap(),
        right.unwrap().balance.unwrap(),
    ]
}

fn transfer(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    amount: i64,
    id: u8,
) -> ApplyCostDeploy {
    let count = NonZeroUsize::new(1).unwrap();
    ApplyCostDeploy::new(
        [id; 32],
        vec![VaultAllocation::new(payer.to_base58(), amount).unwrap()],
        vec![VaultSettlement::new(payer.to_base58(), 0, amount).unwrap()],
        recipient.to_base58(),
        Blake2b512Random::create_from_bytes(&[id]),
    )
    .unwrap()
    .with_fee_cursor(
        MonetaryCursorTransition::new(
            [id; 32],
            MonetaryCursor::new(0, 0, count).unwrap(),
            0,
            count,
        )
        .unwrap(),
        count,
    )
    .unwrap()
}

fn funded_snapshot_envelope(
    private_key: PrivateKey,
    adopted_shard: &casper::rust::casper::CasperShardConf,
) -> (Cosigned<FundedDeploy>, DirectWalletFundingLimits) {
    let payer = vault_payer(&CostSignature {
        value: Some(Value::Ground(accounting::principal_ground_v61(
            &Secp256k1.to_public(&private_key).bytes,
        ))),
    })
    .unwrap();
    let wire = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    let controls = PhloControlsLimits {
        wire,
        owners: 1,
        schedules: 1,
        total_classes: 1,
    };
    let funding = PhloFundingIntentLimits {
        wire,
        controls,
        sources: 1,
        resource_permissions: 1,
        authority_nodes: 16,
    };
    let schedule = PhloScheduleV1 {
        protocol_version: u64::try_from(adopted_shard.casper_version).unwrap(),
        network: b"snapshot-test",
        shard: adopted_shard.shard_name.as_bytes(),
        settlement_asset: b"REV",
        settlement_unit: b"phlo",
        decimal_scale: 8,
        classes: vec![PhloResourceClassV1 {
            identity: b"COMM",
            measurement_unit: b"authority demand",
            measurement_rule: [2; 32],
            valuation_rule: [3; 32],
            weight: 1,
        }],
        actual_price: 1,
        compatibility_rule: [4; 32],
    };
    let record = PhloFundingIntentV1 {
        schedule_commitment: schedule.digest(controls.schedule(1)).unwrap(),
        controls: PhloControlsV1 {
            limit: 8,
            price_ceiling: 1,
            required_owner_ceilings: vec![1],
            permitted_schedules: vec![schedule],
        },
        total_exposure: 10,
        sources: vec![PhloSourcePolicyV1::new(
            &payer.custody_key,
            10,
            10,
            true,
            vec![],
            PhloSourceLimits {
                wire,
                resource_permissions: 1,
                authority_nodes: 16,
            },
        )
        .unwrap()],
    };
    let body = DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp: 1,
        valid_after_block_number: 0,
        shard_id: adopted_shard.shard_name.clone(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let envelope = Cosigned::create_single_envelope(
        FundedDeploy::new(body, record.encode(funding).unwrap(), FundedDeployLimits {
            deploy_bytes: 2_097_152,
            signing: wire,
            funding,
        })
        .unwrap(),
        Box::new(Secp256k1),
        private_key,
    )
    .unwrap();
    (envelope, DirectWalletFundingLimits {
        members: NonZeroUsize::new(1).unwrap(),
        funding,
    })
}

fn transfer_with_snapshot_cursor(
    payer: &VaultAddress,
    recipient: &VaultAddress,
    amount: i64,
    id: u8,
    scope: [u8; 32],
) -> ApplyCostDeploy {
    let count = NonZeroUsize::new(1).unwrap();
    ApplyCostDeploy::new(
        [id; 32],
        vec![VaultAllocation::new(payer.to_base58(), amount).unwrap()],
        vec![VaultSettlement::new(payer.to_base58(), 0, amount).unwrap()],
        recipient.to_base58(),
        Blake2b512Random::create_from_bytes(&[id]),
    )
    .unwrap()
    .with_fee_cursor(
        MonetaryCursorTransition::new(scope, MonetaryCursor::INITIAL, 0, count).unwrap(),
        count,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn genesis_resource_policy_is_immutable_root_bound_and_required() {
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::rholang::costacc::genesis_resource_policy::GenesisResourcePolicy;
    use models::rust::phlo_schedule::PhloGenesisPolicy;

    use crate::util::genesis_builder::GenesisBuilder;

    with_runtime_manager(|manager, _, old_genesis| async move {
        assert!(GenesisResourcePolicy::load(&manager, &old_genesis)
            .await
            .is_err());
        let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None).2;
        let shard = casper::rust::casper::CasperShardConf {
            min_phlo_price: parameters.proof_of_stake.min_phlo_price,
            casper_version: parameters.version,
            shard_name: parameters.shard_id.clone(),
            ..casper::rust::casper::CasperShardConf::new()
        };
        let (envelope, limits) =
            funded_snapshot_envelope(PrivateKey::from_bytes(&[0xcb; 32]), &shard);
        let intent =
            PhloFundingIntentV1::decode(envelope.data.funding_intent(), limits.funding).unwrap();
        let policy =
            PhloGenesisPolicy::from_schedule(&intent.controls.permitted_schedules[0]).unwrap();
        parameters.resource_policy = Some(policy.clone());
        let genesis = Genesis::create_genesis_block(&manager, &parameters)
            .await
            .unwrap();
        validate_policy_genesis(&manager, &genesis, &parameters, Some(&policy))
            .await
            .unwrap();
        let omitted = validate_policy_genesis(&manager, &genesis, &parameters, None)
            .await
            .unwrap_err();
        assert!(omitted.contains("do not match expected blessed contracts"));
        assert_ne!(
            genesis.body.state.post_state_hash,
            old_genesis.body.state.post_state_hash
        );
        let (first, second) = tokio::join!(
            GenesisResourcePolicy::load(&manager, &genesis),
            GenesisResourcePolicy::load(&manager, &genesis),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.record(), &policy);
        assert_eq!(first.record(), second.record());
        assert_eq!(first.genesis_root(), &genesis.body.state.post_state_hash);
        let binding = PhloFundingIntentBinding::new(&intent, limits.funding).unwrap();
        first
            .check_funding_intent(&binding.view().unwrap())
            .unwrap();
        let mut changed_intent = intent.clone();
        changed_intent.controls.permitted_schedules[0].classes[0].weight += 1;
        changed_intent.schedule_commitment = changed_intent.controls.permitted_schedules[0]
            .digest(limits.funding.controls.schedule(1))
            .unwrap();
        let changed_binding =
            PhloFundingIntentBinding::new(&changed_intent, limits.funding).unwrap();
        assert!(first
            .check_funding_intent(&changed_binding.view().unwrap())
            .is_err());
        parameters.resource_policy = Some(
            PhloGenesisPolicy::from_schedule(&changed_intent.controls.permitted_schedules[0])
                .unwrap(),
        );
        let altered = validate_policy_genesis(
            &manager,
            &genesis,
            &parameters,
            parameters.resource_policy.as_ref(),
        )
        .await
        .unwrap_err();
        assert!(altered.contains("do not match expected blessed contracts"));
        let other_genesis = Genesis::create_genesis_block(&manager, &parameters)
            .await
            .unwrap();
        assert_ne!(
            other_genesis.body.state.post_state_hash,
            genesis.body.state.post_state_hash
        );
        let other = GenesisResourcePolicy::load(&manager, &other_genesis)
            .await
            .unwrap();
        assert_ne!(first.record(), other.record());
        let restored = GenesisResourcePolicy::load(&manager, &genesis)
            .await
            .unwrap();
        assert_eq!(restored.record(), first.record());
        assert!(GenesisResourcePolicy::load(&manager, &old_genesis)
            .await
            .is_err());
        let mut child = genesis.clone();
        child
            .header
            .parents_hash_list
            .push(genesis.block_hash.clone());
        assert!(GenesisResourcePolicy::load(&manager, &child).await.is_err());
        let mut wrong_shard = genesis.clone();
        wrong_shard.shard_id.push_str("-wrong");
        assert!(GenesisResourcePolicy::load(&manager, &wrong_shard)
            .await
            .is_err());
    })
    .await
    .unwrap();
}

async fn validate_policy_genesis(
    manager: &RuntimeManager,
    genesis: &BlockMessage,
    parameters: &casper::rust::genesis::genesis::Genesis,
    policy: Option<&models::rust::phlo_schedule::PhloGenesisPolicy>,
) -> Result<(), String> {
    use casper::rust::engine::block_approver_protocol::BlockApproverProtocol;

    use crate::util::comm::transport_layer_test_impl::TransportLayerTestImpl;
    let pos = &parameters.proof_of_stake;
    let bonds = pos
        .validators
        .iter()
        .map(|validator| (validator.pk.bytes.clone(), validator.stake))
        .collect();
    let candidate = models::rust::casper::protocol::casper_message::ApprovedBlockCandidate {
        block: genesis.clone(),
        required_sigs: 0,
    };
    BlockApproverProtocol::<TransportLayerTestImpl>::validate_candidate(
        manager,
        &candidate,
        0,
        parameters.timestamp,
        &parameters.vaults,
        &bonds,
        pos.minimum_bond,
        pos.maximum_bond,
        pos.epoch_length,
        pos.quarantine_length,
        pos.number_of_active_validators,
        pos.fault_tolerance_threshold_ppm,
        pos.max_parent_depth,
        pos.deploy_lifespan,
        pos.min_phlo_price,
        &parameters.shard_id,
        &pos.pos_multi_sig_public_keys,
        pos.pos_multi_sig_quorum,
        pos.max_cosigners_per_deploy,
        pos.initial_phlogiston,
        pos.epoch_phlogiston,
        parameters.version,
        &parameters.client_fuel_allocations,
        &parameters.native_token_name,
        &parameters.native_token_symbol,
        parameters.native_token_decimals,
        policy,
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_snapshot_pins_wallet_and_both_cursor_scopes_to_each_requested_root() {
    with_runtime_manager(|mut manager, genesis, block| async move {
        let (_, _, minimum) =
            casper::rust::util::token_metadata_check::read_on_chain_consensus_parameters(
                &manager,
                &block.body.state.post_state_hash,
            )
            .await
            .unwrap();
        let adopted_shard = casper::rust::casper::CasperShardConf {
            min_phlo_price: minimum,
            casper_version: block.header.version,
            shard_name: block.shard_id.clone(),
            ..casper::rust::casper::CasperShardConf::new()
        };
        let treasury = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
        let private_key = PrivateKey::from_bytes(&[0xca; 32]);
        let wallet = VaultAddress::from_public_key(&Secp256k1.to_public(&private_key)).unwrap();
        let (envelope, limits) = funded_snapshot_envelope(private_key.clone(), &adopted_shard);
        let mut parameters = crate::util::genesis_builder::GenesisBuilder::build_genesis_parameters_with_defaults(None, None).2;
        let intent = PhloFundingIntentV1::decode(envelope.data.funding_intent(), limits.funding).unwrap();
        parameters.resource_policy = Some(models::rust::phlo_schedule::PhloGenesisPolicy::from_schedule(
            &intent.controls.permitted_schedules[0],
        ).unwrap());
        let policy_genesis = casper::rust::genesis::genesis::Genesis::create_genesis_block(&manager, &parameters).await.unwrap();
        let genesis_policy = casper::rust::util::rholang::costacc::genesis_resource_policy::GenesisResourcePolicy::load(
            &manager, &policy_genesis,
        ).await.unwrap();
        let initial = policy_genesis.body.state.post_state_hash;
        let offered = Cosigned::create_single_envelope(
            OfferedFundedDeploy::new(
                envelope.data.body().clone(),
                envelope.data.funding_intent().to_vec(),
                8,
                1,
                FundedDeployLimits {
                    deploy_bytes: 2_097_152,
                    signing: limits.funding.wire,
                    funding: limits.funding,
                },
            )
            .unwrap(),
            Box::new(Secp256k1),
            private_key,
        )
        .unwrap();
        let parallel = NonZeroUsize::new(2).unwrap();
        let budget =
            || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let base = successful_system_state(
            ops.play_system_deploy(&initial, &mut transfer(&treasury, &wallet, 7, 0xca))
                .await
                .unwrap(),
        );
        let baseline = authorize_direct_wallet_funding(&envelope, limits)
            .unwrap()
            .read_policy_snapshot(&manager, base.clone(), parallel, &budget())
            .await
            .unwrap();
        let resource_scope = baseline.resource_scope();
        let fee_scope = baseline.fee_scope();
        assert_ne!(resource_scope, fee_scope);
        assert_eq!(
            baseline.wallets().pre_state_root().as_slice(),
            base.as_ref()
        );
        assert_eq!(baseline.wallets().sources()[0].source().capacity, 7);
        assert_eq!(baseline.resource_cursor(), None);
        assert_eq!(baseline.fee_cursor(), None);
        assert_eq!(baseline.canonical_custodies(), &[baseline
            .wallets()
            .sources()[0]
            .source()
            .custody]);
        let middle = successful_system_state(
            ops.play_system_deploy(
                &base,
                &mut transfer_with_snapshot_cursor(&wallet, &treasury, 2, 0xcb, resource_scope),
            )
            .await
            .unwrap(),
        );
        let newest = successful_system_state(
            ops.play_system_deploy(
                &middle,
                &mut transfer_with_snapshot_cursor(&treasury, &wallet, 1, 0xcc, fee_scope),
            )
            .await
            .unwrap(),
        );
        assert_ne!(base, middle);
        assert_ne!(middle, newest);
        let old_authorized = authorize_direct_wallet_funding(&envelope, limits).unwrap();
        let middle_authorized = authorize_direct_wallet_funding(&envelope, limits).unwrap();
        let new_authorized = authorize_direct_wallet_funding(&envelope, limits).unwrap();
        let budgets = [budget(), budget(), budget()];
        let (old_snapshot, middle_snapshot, new_snapshot) = tokio::join!(
            old_authorized.read_policy_snapshot(&manager, base.clone(), parallel, &budgets[0]),
            middle_authorized.read_policy_snapshot(&manager, middle.clone(), parallel, &budgets[1]),
            new_authorized.read_policy_snapshot(&manager, newest.clone(), parallel, &budgets[2]),
        );
        let old_snapshot = old_snapshot.unwrap();
        let middle_snapshot = middle_snapshot.unwrap();
        let new_snapshot = new_snapshot.unwrap();
        let offered_new = authorize_offered_direct_wallet_funding(&offered, limits)
            .unwrap()
            .read_policy_snapshot(&manager, newest.clone(), parallel, &budget())
            .await
            .unwrap();
        for (original, root) in [
            (&old_snapshot, &base),
            (&middle_snapshot, &middle),
            (&new_snapshot, &newest),
        ] {
            let checked = authorize_offered_direct_wallet_funding(&offered, limits)
                .unwrap()
                .read_policy_snapshot(&manager, root.clone(), parallel, &budget())
                .await
                .unwrap();
            assert_eq!(checked.wallets().sources(), original.wallets().sources());
            assert_eq!(
                checked.wallets().pre_state_root(),
                original.wallets().pre_state_root()
            );
            assert_eq!(
                checked.canonical_custodies(),
                original.canonical_custodies()
            );
            assert_eq!(checked.resource_scope(), original.resource_scope());
            assert_eq!(checked.fee_scope(), original.fee_scope());
            assert_eq!(checked.resource_cursor(), original.resource_cursor());
            assert_eq!(checked.fee_cursor(), original.fee_cursor());
            assert!(std::ptr::eq(
                checked.wallets().authorization().envelope(),
                &offered
            ));
        }
        let advanced = Some(MonetaryCursor::new(1, 0, NonZeroUsize::new(1).unwrap()).unwrap());
        for (snapshot, root, balance, resource_cursor, fee_cursor) in [
            (&old_snapshot, &base, 7, None, None),
            (&middle_snapshot, &middle, 5, advanced, None),
            (&new_snapshot, &newest, 6, advanced, advanced),
        ] {
            assert_eq!(
                snapshot.wallets().pre_state_root().as_slice(),
                root.as_ref()
            );
            assert_eq!(snapshot.wallets().sources()[0].source().capacity, balance);
            assert_eq!(
                snapshot.wallets().sources()[0].inventory().balance,
                Some(balance as i64)
            );
            assert_eq!(snapshot.resource_scope(), resource_scope);
            assert_eq!(snapshot.fee_scope(), fee_scope);
            assert_eq!(snapshot.resource_cursor(), resource_cursor);
            assert_eq!(snapshot.fee_cursor(), fee_cursor);
            assert_eq!(
                snapshot.canonical_custodies(),
                baseline.canonical_custodies()
            );
        }
        assert_eq!(
            old_snapshot.wallets().sources(),
            baseline.wallets().sources()
        );
        let record = new_snapshot.wallets().authorization().record();
        let binding = PhloFundingIntentBinding::new(record, limits.funding).unwrap();
        let view = binding.view().unwrap();
        let schedule = view.controls().permitted_schedules[0];
        let policy_record = &record.controls.permitted_schedules[0];
        let policy_binding =
            rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding::new(
                policy_record,
                limits
                    .funding
                    .controls
                    .schedule(policy_record.classes.len()),
            )
            .unwrap();
        let controls = check_phlo_controls(
            schedule.environment,
            u64::try_from(minimum).unwrap(),
            u64::MAX,
            view.controls(),
            schedule,
            0,
        )
        .unwrap();
        let execution = check_phlo_execution(
            controls,
            PhloExecutionWitness {
                available: &[],
                required: &[],
                used: &[],
                unused: &[],
                fresh: &[],
            },
            PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: 16,
                key_bytes: 1_048_576,
            },
        )
        .unwrap();
        let one = NonZeroUsize::new(1).unwrap();
        let charged = project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), one).unwrap();
        let zero = project_phlo_obligations(
            execution,
            PhloOutcome::Accepted(&[PhloFailure::Platform]),
            one,
        )
        .unwrap();
        let sources: Vec<_> = new_snapshot
            .wallets()
            .sources()
            .iter()
            .map(|source| source.source())
            .collect();
        let eligible = [vec![true]];
        let charged_assignment = [vec![1]];
        let zero_assignment = [vec![0]];
        let cases = [
            PhloFundingCase {
                obligations: &charged,
                eligible: &eligible,
                assignment: &charged_assignment,
            },
            PhloFundingCase {
                obligations: &zero,
                eligible: &eligible,
                assignment: &zero_assignment,
            },
        ];
        let family_limits = PhloFundingLimits {
            sources: one,
            cases: NonZeroUsize::new(2).unwrap(),
            obligations: one,
            assignment_cells: 2,
            custody_bytes: 32,
        };
        let family =
            check_phlo_funding_family(&sources, &cases, record.total_exposure, family_limits)
                .unwrap();
        let terms = PhloFundingTerms {
            required_owner_ceilings: view.controls().required_owner_ceilings,
            asset: schedule.environment.asset,
            schedule_commitment: schedule.commitment,
        };
        let signed_limits = SignedPhloConsentLimits {
            members: one,
            intent: limits.funding,
            consent: PhloConsentLimits {
                sources: 1,
                permission_entries: 1,
                case_cells: 2,
                authority_nodes: 16,
                key_bytes: 1_048_576,
            },
        };
        let policy_limits = PhloFamilyFundingLimits {
            funding: family_limits,
            keys: PhloObligationKeyLimits {
                wire: limits.funding.wire,
                authority_nodes: 16,
            },
            aggregate_key_bytes: 1_048_576,
        };
        let checked_policy = new_snapshot
            .bind_native_family(
                &view,
                &family,
                terms,
                signed_limits,
                policy_limits,
                &budget(),
            )
            .unwrap()
            .unwrap();
        assert!(std::ptr::eq(checked_policy.snapshot(), &new_snapshot));
        for (wrong_context, expected) in [
            (
                casper::rust::casper::CasperShardConf {
                    min_phlo_price: minimum + 1,
                    ..adopted_shard.clone()
                },
                DirectWalletPolicySnapshotError::ChainMinimumMismatch,
            ),
            (
                casper::rust::casper::CasperShardConf {
                    casper_version: adopted_shard.casper_version + 1,
                    ..adopted_shard.clone()
                },
                DirectWalletPolicySnapshotError::ChainVersionMismatch,
            ),
            (
                casper::rust::casper::CasperShardConf {
                    shard_name: "another-shard".into(),
                    ..adopted_shard.clone()
                },
                DirectWalletPolicySnapshotError::ChainShardMismatch,
            ),
        ] {
            let rejected = offered_new
                .bind_native_family(
                    &view,
                    &family,
                    terms,
                    signed_limits,
                    policy_limits,
                    &wrong_context,
                    policy_binding.policy(),
                    &budget(),
                )
                .unwrap_err();
            assert_eq!(
                std::mem::discriminant(&rejected),
                std::mem::discriminant(&expected)
            );
        }
        for field in 0..3 {
            let mut other_policy = policy_record.clone();
            match field {
                0 => other_policy.classes[0].weight ^= 1,
                1 => other_policy.classes[0].measurement_rule[0] ^= 1,
                _ => other_policy.compatibility_rule[0] ^= 1,
            }
            let other_binding =
                rholang::rust::interpreter::accounting::phlo_controls::PhloScheduleBinding::new(
                    &other_policy,
                    limits.funding.controls.schedule(other_policy.classes.len()),
                )
                .unwrap();
            assert!(matches!(
                offered_new.bind_native_family(
                    &view,
                    &family,
                    terms,
                    signed_limits,
                    policy_limits,
                    &adopted_shard,
                    other_binding.policy(),
                    &budget(),
                ),
                Err(DirectWalletPolicySnapshotError::SchedulePolicy(_))
            ));
        }
        parameters.proof_of_stake.min_phlo_price = minimum + 1;
        let stricter_genesis = casper::rust::genesis::genesis::Genesis::create_genesis_block(
            &manager, &parameters,
        ).await.unwrap();
        let stricter_policy = casper::rust::util::rholang::costacc::genesis_resource_policy::GenesisResourcePolicy::load(
            &manager, &stricter_genesis,
        ).await.unwrap();
        assert_eq!(stricter_policy.record(), genesis_policy.record());
        assert_eq!(stricter_policy.minimum_price(), u64::try_from(minimum + 1).unwrap());
        assert_eq!(genesis_policy.minimum_price(), u64::try_from(minimum).unwrap());
        assert!(matches!(offered_new.bind_native_family_from_genesis(
            &view, &family, terms, signed_limits, policy_limits, &adopted_shard,
            &stricter_policy, &budget(),
        ), Err(DirectWalletPolicySnapshotError::ChainMinimumMismatch)));
        let offered_policy = offered_new
            .bind_native_family_from_genesis(
                &view,
                &family,
                terms,
                signed_limits,
                policy_limits,
                &adopted_shard,
                &genesis_policy,
                &budget(),
            )
            .unwrap()
            .unwrap();
        assert!(std::ptr::eq(offered_policy.snapshot(), &offered_new));
        assert_eq!(
            checked_policy
                .snapshot()
                .wallets()
                .pre_state_root()
                .as_slice(),
            newest.as_ref()
        );
        let captured_snapshots = checked_policy.policy().policy().snapshots();
        assert_eq!(captured_snapshots.resource.scope, resource_scope);
        assert_eq!(captured_snapshots.fee.scope, fee_scope);
        assert_eq!(Some(captured_snapshots.resource.cursor), advanced);
        assert_eq!(Some(captured_snapshots.fee.cursor), advanced);
        let capture_limits = PhloCaptureLimits {
            funding: FundingSearchLimits {
                source_cap: one,
                obligation_cap: one,
            },
            key: policy_limits.keys,
            aggregate_key_bytes: policy_limits.aggregate_key_bytes,
        };
        let charged_capture = checked_policy
            .policy()
            .capture_case(0, capture_limits, &budget())
            .unwrap();
        for branch in 0..2 {
            let original = checked_policy
                .policy()
                .capture_case(branch, capture_limits, &budget())
                .unwrap();
            let captured = offered_policy
                .policy()
                .capture_case(branch, capture_limits, &budget())
                .unwrap();
            assert!(std::ptr::eq(
                captured.scoped().signed_intent().envelope(),
                &offered
            ));
            assert_eq!(captured.amounts(), original.amounts());
            assert_eq!(
                captured.scoped().resource_transition(),
                original.scoped().resource_transition()
            );
            assert_eq!(
                captured.scoped().fee_transition(),
                original.scoped().fee_transition()
            );
        }
        assert_eq!(charged_capture.scoped().resource_transition(), None);
        let fee_transition = charged_capture.scoped().fee_transition().unwrap();
        assert_eq!(fee_transition.scope(), &fee_scope);
        assert_eq!(Some(fee_transition.expected()), advanced);
        assert_eq!(fee_transition.next().revision(), 2);
        assert_eq!(fee_transition.next().position(), 0);
        assert_eq!(
            (
                charged_capture.amounts()[0].hold(),
                charged_capture.amounts()[0].acquisition(),
                charged_capture.amounts()[0].fee(),
                charged_capture.amounts()[0].refund()
            ),
            (1, 0, 1, 0)
        );
        let zero_capture = checked_policy
            .policy()
            .capture_case(1, capture_limits, &budget())
            .unwrap();
        assert_eq!(zero_capture.scoped().resource_transition(), None);
        assert_eq!(zero_capture.scoped().fee_transition(), None);
        assert_eq!(
            (
                zero_capture.amounts()[0].hold(),
                zero_capture.amounts()[0].acquisition(),
                zero_capture.amounts()[0].fee(),
                zero_capture.amounts()[0].refund()
            ),
            (1, 0, 0, 1)
        );
        let wrong_sources = [PhloFundingSource {
            capacity: sources[0].capacity + 1,
            ..sources[0]
        }];
        let wrong_family =
            check_phlo_funding_family(&wrong_sources, &cases, record.total_exposure, family_limits)
                .unwrap();
        let no_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
        assert!(matches!(
            new_snapshot.bind_native_family(
                &view,
                &wrong_family,
                terms,
                signed_limits,
                policy_limits,
                &no_work
            ),
            Err(DirectWalletPolicySnapshotError::HostWork(_))
        ));
        assert!(matches!(
            new_snapshot.bind_native_family(
                &view,
                &wrong_family,
                terms,
                signed_limits,
                policy_limits,
                &budget()
            ),
            Err(DirectWalletPolicySnapshotError::Binding(
                DirectWalletBindingError::SourcesMismatch
            ))
        ));
        assert_eq!(
            ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
            newest
        );
        assert_eq!(system_vault_balance(&manager, &base, &wallet).await, 7);
        assert_eq!(system_vault_balance(&manager, &middle, &wallet).await, 5);
        assert_eq!(system_vault_balance(&manager, &newest, &wallet).await, 6);
        let match_limits = PhloOutcomeMatchLimits {
            execution: PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: 16,
                key_bytes: 1_048_576,
            },
            key: policy_limits.keys,
            aggregate_key_bytes: policy_limits.aggregate_key_bytes,
            cases: family_limits.cases,
        };
        for failures in [&[][..], &[PhloFailure::Platform][..]] {
            let outcome = PhloOutcome::Accepted(failures);
            let checked = offered_policy
                .capture_settlement(execution, outcome, match_limits, capture_limits, &budget())
                .unwrap();
            assert!(std::ptr::eq(checked.snapshot(), &offered_new));
            assert!(std::ptr::eq(
                checked.capture().scoped().signed_intent().envelope(),
                &offered
            ));
            assert!(checked
                .prepare_request(
                    [0xcd; 32],
                    &treasury,
                    Blake2b512Random::create_from_bytes(&[0xcd]),
                    &no_work
                )
                .is_err());
            let mut prepared = checked
                .prepare_request(
                    [0xcd; 32],
                    &treasury,
                    Blake2b512Random::create_from_bytes(&[0xcd]),
                    &budget(),
                )
                .unwrap();
            assert!(std::ptr::eq(prepared.checked(), &checked));
            let legacy_checked = checked_policy
                .capture_settlement(execution, outcome, match_limits, capture_limits, &budget())
                .unwrap();
            let mut legacy_prepared = legacy_checked
                .prepare_request(
                    [0xcd; 32],
                    &treasury,
                    Blake2b512Random::create_from_bytes(&[0xcd]),
                    &budget(),
                )
                .unwrap();
            assert_eq!(
                prepared.request().unwrap().env(),
                legacy_prepared.request().unwrap().env()
            );
            let other = offered_policy
                .capture_settlement(
                    execution,
                    PhloOutcome::Accepted(if failures.is_empty() {
                        &[PhloFailure::Platform]
                    } else {
                        &[]
                    }),
                    match_limits,
                    capture_limits,
                    &budget(),
                )
                .unwrap();
            let mut other_prepared = other
                .prepare_request(
                    [0xcf; 32],
                    &treasury,
                    Blake2b512Random::create_from_bytes(&[0xcf]),
                    &budget(),
                )
                .unwrap();
            {
                let mut first_view = prepared.request().unwrap();
                let mut second_view = other_prepared.request().unwrap();
                let first_env = first_view.env();
                let second_env = second_view.env();
                std::mem::swap(&mut first_view, &mut second_view);
                assert!(std::ptr::eq(first_view.checked(), &other));
                assert!(std::ptr::eq(second_view.checked(), &checked));
                assert_eq!(first_view.env(), second_env);
                assert_eq!(second_view.env(), first_env);
            }
            assert!(std::ptr::eq(
                prepared.request().unwrap().checked(),
                &checked
            ));
            let mut replay_prepared = checked
                .prepare_request(
                    [0xcd; 32],
                    &treasury,
                    Blake2b512Random::create_from_bytes(&[0xcd]),
                    &budget(),
                )
                .unwrap();
            let post = compare_successful_system_deploys(
                &mut manager,
                &genesis,
                &newest,
                &mut prepared.request().unwrap(),
                &mut replay_prepared.request().unwrap(),
                |_| true,
            )
            .await
            .unwrap();
            let expected_fee = i64::from(failures.is_empty());
            assert_eq!(
                system_vault_balance(&manager, &post, &wallet).await,
                6 - expected_fee
            );
            let after = authorize_offered_direct_wallet_funding(&offered, limits)
                .unwrap()
                .read_policy_snapshot(&manager, post, parallel, &budget())
                .await
                .unwrap();
            assert_eq!(after.resource_cursor(), advanced);
            assert_eq!(after.fee_cursor().unwrap().revision(), 1 + expected_fee);
            assert_eq!(
                checked.snapshot().wallets().pre_state_root().as_slice(),
                newest.as_ref()
            );
        }
        assert!(offered_policy
            .capture_settlement(
                execution,
                PhloOutcome::AdmissionRejected,
                match_limits,
                capture_limits,
                &budget()
            )
            .is_err());
        let zero_cases = [cases[1]];
        let zero_family =
            check_phlo_funding_family(&sources, &zero_cases, record.total_exposure, family_limits)
                .unwrap();
        let zero_policy = offered_new
            .bind_native_family_from_genesis(
                &view,
                &zero_family,
                terms,
                signed_limits,
                policy_limits,
                &adopted_shard,
                &genesis_policy,
                &budget(),
            )
            .unwrap()
            .unwrap();
        let zero_checked = zero_policy
            .capture_settlement(
                execution,
                PhloOutcome::Accepted(&[PhloFailure::Platform]),
                match_limits,
                capture_limits,
                &budget(),
            )
            .unwrap();
        let mut zero_prepared = zero_checked
            .prepare_request(
                [0xce; 32],
                &treasury,
                Blake2b512Random::create_from_bytes(&[0xce]),
                &budget(),
            )
            .unwrap();
        assert!(zero_prepared.request().is_none());
        assert!(std::ptr::eq(
            zero_prepared.checked().snapshot(),
            &offered_new
        ));
        assert!(std::ptr::eq(
            zero_prepared
                .checked()
                .capture()
                .scoped()
                .signed_intent()
                .envelope(),
            &offered
        ));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_reads_keep_the_requested_root_across_transfers_and_topups() {
    with_runtime_manager(|manager, genesis, block| async move {
        let initial = block.body.state.post_state_hash;
        let keys = [
            genesis.genesis_vaults[0].1.clone(),
            Secp256k1.to_public(&PrivateKey::from_bytes(&[0xc7; 32])),
        ];
        let signatures = keys.clone().map(|key| CostSignature {
            value: Some(Value::Ground(key.bytes.to_vec())),
        });
        let wallets = keys.map(|key| VaultAddress::from_public_key(&key).unwrap());
        let original = balances_at(&manager, &initial, &signatures).await;
        assert!(original[0] >= 7);
        assert_eq!(original[1], 0);
        let mut ops = RuntimeOps::new(manager.spawn_runtime().await);
        let debited = successful_system_state(
            ops.play_system_deploy(&initial, &mut transfer(&wallets[0], &wallets[1], 7, 0xc7))
                .await
                .unwrap(),
        );
        let topped_up = successful_system_state(
            ops.play_system_deploy(&debited, &mut transfer(&wallets[1], &wallets[0], 3, 0xc8))
                .await
                .unwrap(),
        );
        let (before, after) = tokio::join!(
            balances_at(&manager, &initial, &signatures),
            balances_at(&manager, &topped_up, &signatures),
        );
        assert_eq!(before, original);
        assert_eq!(after, [original[0] - 4, 4]);
        assert_eq!(balances_at(&manager, &debited, &signatures).await, [
            original[0] - 7,
            7
        ],);
        assert_eq!(
            ops.runtime.create_checkpoint().await.root.to_bytes_prost(),
            topped_up
        );
    })
    .await
    .unwrap();
}
