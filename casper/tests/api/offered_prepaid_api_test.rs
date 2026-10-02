use casper::rust::api::block_api::BlockAPI;
use casper::rust::block_status::BlockError;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use casper::rust::util::rholang::costacc::vault_payer::{balance_query_source, vault_payer};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::casper::protocol::casper_message::ProcessedUserDeploy;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::native_cost_evidence::NativePrepaidDeltaV1;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn schedule() -> PhloScheduleV1<'static> {
    let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

fn signed_offer(
    term: &str,
    timestamp: i64,
    valid_after: i64,
    retained_birth: bool,
    owner_secret: crypto::rust::private_key::PrivateKey,
    owner_public: &crypto::rust::public_key::PublicKey,
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let owner = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner_public.bytes))),
    };
    let payer = vault_payer(&owner).unwrap();
    let a_stack = Compiler::source_to_adt("a :: ()").unwrap();
    let a = a_stack.cost_stacks[0].cells[0].clone();
    let owner_channel = SignatureChannel::from_sig(&cost_signature_to_sig(&owner).unwrap())
        .par
        .encode_to_vec();
    let a_channel = SignatureChannel::from_sig(&cost_signature_to_sig(&a).unwrap())
        .par
        .encode_to_vec();
    let selected = schedule();
    let acquisition_terms = selected.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let owner_authority = cost_signature_to_sig(&owner).unwrap();
    let a_authority = cost_signature_to_sig(&a).unwrap();
    let resource_limits = PhloExecutionLimits {
        resource_entries: 1,
        authority_nodes: limits.funding.authority_nodes,
        key_bytes: limits.funding.wire.field_bytes,
    };
    let mut permissions = Vec::new();
    for class in 0..selected.classes.len() {
        permissions.push(
            PhloResource {
                location: &owner_channel,
                class,
                acquisition_terms: &acquisition_terms,
                authority: &owner_authority,
            }
            .wire_key(resource_limits)
            .unwrap(),
        );
    }
    let retained_class = selected
        .classes
        .iter()
        .position(|class| {
            class.measurement_rule == NativePhloDimension::Introduction.measurement_rule()
        })
        .unwrap();
    for class in 0..selected.classes.len() {
        if retained_birth && class != retained_class {
            continue;
        }
        permissions.push(
            PhloResource {
                location: &a_channel,
                class,
                acquisition_terms: &acquisition_terms,
                authority: &a_authority,
            }
            .wire_key(resource_limits)
            .unwrap(),
        );
    }
    let permission_count = permissions.len();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        5_000_000,
        5_000_000,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: permission_count,
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .unwrap();
    let intent = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 2_000_000,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![selected.clone()],
            },
            schedule_commitment: selected.digest(PhloGenesisPolicy::LIMITS).unwrap(),
            total_exposure: 5_000_000,
            sources: vec![source],
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    })
    .unwrap();
    let body = DeployData {
        term: term.to_owned(),
        language: "rholang".to_owned(),
        time_stamp: timestamp,
        valid_after_block_number: valid_after,
        shard_id: "root".to_owned(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, intent, 2_000_000, 2, limits).unwrap();
    let signed =
        Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner_secret).unwrap();
    OfferedFundedDeploy::to_proto(&signed).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_retained_birth_funds_later_prepaid_draw_across_validators() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let policy = PhloGenesisPolicy::from_schedule(&schedule())
        .unwrap()
        .with_offered_funded_v6_active();
    let genesis = GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let adopted = AdoptedResourcePolicy::load(
        &nodes[0].runtime_manager,
        &genesis.genesis_block,
        nodes[0].casper.casper_shard_conf(),
    )
    .await
    .unwrap();
    assert!(adopted.offered_funded_v6_active());
    let owner_secret = genesis.genesis_vaults[0].0.clone();
    let owner_public = &genesis.genesis_vaults[0].1;
    let owner_address = VaultAddress::from_public_key(owner_public).unwrap();
    let first = signed_offer("a :: ()", 1, 0, true, owner_secret.clone(), owner_public);
    let first_id: block_storage::rust::dag::block_dag_key_value_storage::DeployId =
        first.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, first, &None, false, "root")
        .await
        .unwrap();
    nodes[0].allow_empty_blocks = true;
    let first_legacy_turn = TestNode::propagate_block_at_index(&mut nodes, 0, &[])
        .await
        .unwrap();
    assert_eq!(first_legacy_turn.body.state.block_number, 1);
    let first_block = nodes[0].create_block_unsafe(&[]).await.unwrap();
    assert_eq!(first_block.body.state.block_number, 2);
    assert_eq!(first_block.body.deploys.len(), 1);
    assert_eq!(
        first_block.body.deploys[0].identity_bytes(),
        first_id.as_slice()
    );
    let protocol = offered_funded_v6_limits();
    let first_evidence = first_block.body.deploys[0]
        .as_offered()
        .unwrap()
        .evidence(protocol.evidence)
        .unwrap();
    let first_delta =
        NativePrepaidDeltaV1::decode(first_evidence.prepaid_delta, protocol.prepaid_delta).unwrap();
    assert!(!first_delta.births.is_empty());
    assert!(first_delta
        .replacements
        .iter()
        .any(|change| change.expected_hash.is_none() && change.replacement_hash.is_some()));
    for node in &mut nodes {
        assert!(matches!(
            node.process_block(first_block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    let first_receipt = BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &first_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_receipt.purses.len(), 1);
    assert_eq!(
        first_receipt.purses[0].address,
        owner_address.to_base58().as_bytes()
    );
    assert!(first_receipt.retained_phlo > 0);
    assert_eq!(
        first_receipt.rev_spent,
        u128::from(first_receipt.fresh_phlo + first_receipt.retained_phlo)
            * u128::from(first_evidence.phlo_price)
            + first_receipt.fee_rev
    );
    assert!(first_receipt.phlo_used + first_receipt.retained_phlo <= first_receipt.phlo_limit);

    let second = signed_offer(
        "{% @\"prepaid-draw\"!(true) %}[a]",
        2,
        2,
        false,
        owner_secret,
        owner_public,
    );
    let second_id: block_storage::rust::dag::block_dag_key_value_storage::DeployId =
        second.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, second, &None, false, "root")
        .await
        .unwrap();
    let second_legacy_turn = TestNode::propagate_block_at_index(&mut nodes, 0, &[])
        .await
        .unwrap();
    assert_eq!(second_legacy_turn.body.state.block_number, 3);
    let second_block = nodes[0].create_block_unsafe(&[]).await.unwrap();
    assert_eq!(second_block.body.state.block_number, 4);
    assert_eq!(second_block.body.deploys.len(), 1);
    assert_eq!(
        second_block.body.deploys[0].identity_bytes(),
        second_id.as_slice()
    );
    let second_offered = second_block.body.deploys[0].as_offered().unwrap();
    let second_evidence = second_offered.evidence(protocol.evidence).unwrap();
    let second_delta =
        NativePrepaidDeltaV1::decode(second_evidence.prepaid_delta, protocol.prepaid_delta)
            .unwrap();
    assert!(second_delta.draws.iter().any(|draw| {
        first_delta
            .births
            .iter()
            .any(|birth| birth.stack_id == draw.stack_id)
    }));
    assert!(!second_delta.replacements.is_empty());

    let mut altered_delta = second_delta;
    altered_delta.draws[0].receipt_index += 1;
    let altered_delta = altered_delta.encode(protocol.prepaid_delta).unwrap();
    let mut altered_evidence = second_evidence.clone();
    altered_evidence.prepaid_delta = &altered_delta;
    let mut altered_proto = second_offered.to_proto(protocol.evidence).unwrap();
    altered_proto.native_cost_evidence =
        Some(altered_evidence.encode(protocol.evidence).unwrap().into());
    let mut altered_block = second_block.clone();
    altered_block.body.deploys[0] = ProcessedUserDeploy::from_proto(altered_proto).unwrap();
    let altered_block = resign_block(
        &altered_block,
        &nodes[0].validator_id_opt.as_ref().unwrap().private_key,
    );
    let prior_heads = nodes[2]
        .casper
        .block_dag()
        .await
        .unwrap()
        .latest_message_hashes();
    assert!(matches!(
        nodes[2].process_block(altered_block.clone()).await.unwrap(),
        Either::Left(BlockError::BlockException(error))
            if error.to_string().contains("independent prepaid delta differs from committed evidence")
    ));
    assert!(!nodes[2].casper.dag_contains(&altered_block.block_hash));
    assert_eq!(
        nodes[2]
            .casper
            .block_dag()
            .await
            .unwrap()
            .latest_message_hashes(),
        prior_heads
    );

    for node in &mut nodes {
        assert!(matches!(
            node.process_block(second_block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    let producer_receipt =
        BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &second_id)
            .await
            .unwrap()
            .unwrap();
    let validator_receipt =
        BlockAPI::find_offered_settlement_receipt(&nodes[1].engine_cell, &second_id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(producer_receipt, validator_receipt);
    assert_eq!(producer_receipt.purses.len(), 1);
    assert_eq!(
        producer_receipt.purses[0].address,
        owner_address.to_base58().as_bytes()
    );
    assert_eq!(
        producer_receipt.rev_spent,
        u128::from(producer_receipt.fresh_phlo + producer_receipt.retained_phlo)
            * u128::from(second_evidence.phlo_price)
            + producer_receipt.fee_rev
    );
    assert!(
        producer_receipt.phlo_used + producer_receipt.retained_phlo <= producer_receipt.phlo_limit
    );
    assert!(producer_receipt.rev_spent <= producer_receipt.rev_ceiling);
    assert!(producer_receipt.purses[0].post_balance <= first_receipt.purses[0].post_balance);
    let (balance, _) = nodes[1]
        .runtime_manager
        .play_exploratory_deploy(
            balance_query_source(&owner_address),
            &second_block.body.state.post_state_hash,
            None,
        )
        .await
        .unwrap();
    assert_eq!(balance.len(), 1);
    assert_eq!(
        u64::try_from(RhoNumber::unapply(&balance[0]).unwrap()).unwrap(),
        producer_receipt.purses[0].post_balance
    );
}
