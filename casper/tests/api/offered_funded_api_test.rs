use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::block_report_api::BlockReportAPI;
use casper::rust::block_status::BlockError;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::report_store::ReportStore;
use casper::rust::safety_oracle::CliqueOracleImpl;
use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    authorize_offered_direct_wallet_funding, DirectWalletFundingLimits,
};
use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use casper::rust::util::rholang::costacc::vault_payer::{balance_query_source, vault_payer};
use comm::rust::discovery::node_discovery::NodeDiscovery;
use comm::rust::errors::CommError;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::rp_conf::RPConfCell;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::casper::v1::deploy_response::Message as DeployResponseMessage;
use models::casper::v1::deploy_service_server::DeployService;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::casper::protocol::casper_message::ProcessedUserDeploy;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::native_wallet_receipt::{NativeWalletReceiptLimits, NativeWalletReceiptV1};
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use node::rust::api::deploy_grpc_service_v1::DeployGrpcServiceV1Impl;
use node::rust::api::web_api::{WebApi, WebApiImpl};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

struct EmptyDiscovery;

#[async_trait::async_trait]
impl NodeDiscovery for EmptyDiscovery {
    async fn discover(&self) -> Result<(), CommError> { Ok(()) }

    fn peers(&self) -> Result<Vec<PeerNode>, CommError> { Ok(Vec::new()) }

    fn remove_peer(&self, _peer: &PeerNode) -> Result<(), CommError> { Ok(()) }
}

fn report_api(node: &TestNode) -> BlockReportAPI {
    let block_store = || {
        KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
    };
    BlockReportAPI::new(
        casper::rust::reporting_casper::noop(),
        ReportStore::new(Arc::new(InMemoryKeyValueStore::new())),
        node.engine_cell.clone(),
        block_store(),
        CliqueOracleImpl,
        false,
    )
}

fn offered_grpc_service(node: &TestNode) -> DeployGrpcServiceV1Impl {
    DeployGrpcServiceV1Impl::new(
        10,
        None,
        false,
        "test".to_string(),
        "root".to_string(),
        1,
        "F1R3".to_string(),
        "F1R3".to_string(),
        8,
        false,
        node.engine_cell.clone(),
        report_api(node),
        models::rhoapi::Par::default(),
        KeyValueBlockStore::new(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        ),
        RPConfCell::new(node.rp_conf.clone()),
        node.connections_cell.clone(),
        Arc::new(EmptyDiscovery),
        100,
        Arc::new(AtomicBool::new(true)),
    )
}

fn offered_web_api(node: &TestNode) -> WebApiImpl {
    WebApiImpl::new(
        10,
        false,
        "test".to_string(),
        "root".to_string(),
        1,
        "F1R3".to_string(),
        "F1R3".to_string(),
        8,
        false,
        report_api(node),
        models::rhoapi::Par::default(),
        Arc::new(node.engine_cell.clone()),
        RPConfCell::new(node.rp_conf.clone()),
        node.connections_cell.clone(),
        Arc::new(EmptyDiscovery),
        None,
        100,
        0,
        Arc::new(AtomicBool::new(true)),
    )
}

fn selected_schedule() -> PhloScheduleV1<'static> {
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
    owner_secret: crypto::rust::private_key::PrivateKey,
    owner_public: &crypto::rust::public_key::PublicKey,
) -> models::casper::DeployDataProto {
    signed_offer_at(owner_secret, owner_public, 1)
}

fn signed_offer_at(
    owner_secret: crypto::rust::private_key::PrivateKey,
    owner_public: &crypto::rust::public_key::PublicKey,
    time_stamp: i64,
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner_public.bytes))),
    };
    let payer = vault_payer(&signature).unwrap();
    let schedule = selected_schedule();
    let acquisition_terms = schedule.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let authority = cost_signature_to_sig(&signature).unwrap();
    let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
    let permissions = (0..schedule.classes.len())
        .map(|class| {
            PhloResource {
                location: &location,
                class,
                acquisition_terms: &acquisition_terms,
                authority: &authority,
            }
            .wire_key(PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: limits.funding.authority_nodes,
                key_bytes: limits.funding.wire.field_bytes,
            })
            .unwrap()
        })
        .collect();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        5_000_000,
        5_000_000,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: schedule.classes.len(),
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .unwrap();
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 2_000_000,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![schedule.clone()],
            },
            schedule_commitment: schedule.digest(PhloGenesisPolicy::LIMITS).unwrap(),
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
        term: "new x in { x!(0) }".to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, funding, 2_000_000, 2, limits).unwrap();
    let signed =
        Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner_secret).unwrap();
    OfferedFundedDeploy::to_proto(&signed).unwrap()
}

async fn offered_v6_genesis(validators: usize) -> crate::util::genesis_builder::GenesisContext {
    let mut parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let policy = PhloGenesisPolicy::from_schedule(&selected_schedule())
        .expect("selected schedule forms a genesis policy")
        .with_offered_funded_v6_active();
    GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("offered v6 genesis builds")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unfundable_offer_at_queue_head_is_quarantined_and_next_offer_is_included() {
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    let genesis = offered_v6_genesis(1).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("single-node network starts");
    let unfunded_secret = crypto::rust::private_key::PrivateKey::from_bytes(&[0x5a; 32]);
    let unfunded_public = Secp256k1.to_public(&unfunded_secret);
    assert!(genesis
        .genesis_vaults
        .iter()
        .all(|(_, public)| public.bytes != unfunded_public.bytes));
    let unfundable = signed_offer_at(unfunded_secret, &unfunded_public, 1);
    let fundable = signed_offer_at(
        genesis.genesis_vaults[0].0.clone(),
        &genesis.genesis_vaults[0].1,
        2,
    );
    for offer in [&unfundable, &fundable] {
        BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
            .await
            .expect("well-formed offer is admitted");
    }
    let unfundable_id =
        models::rust::deploy_id::DeployIdV6::try_from(unfundable.deploy_id.to_vec())
            .expect("offered deploy id has 32 bytes");

    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the proposer skips the unfundable head offer");
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(
        block.body.deploys[0].identity_bytes(),
        fundable.deploy_id.as_ref()
    );
    let storage = nodes[0].deploy_storage.lock();
    assert!(!storage
        .contains_envelope_id(&unfundable_id)
        .expect("pending envelope lookup succeeds"));
    let rejection = storage
        .envelope_rejection(&unfundable_id)
        .expect("the rejected offer has a status entry");
    assert_eq!(rejection.block_number, block.body.state.block_number);
    assert_eq!(
        rejection.pre_state_root.as_deref(),
        Some(block.body.state.pre_state_hash.as_ref())
    );
    assert!(!rejection.reason.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_validator_rejects_a_block_with_a_body_only_deploy() {
    let genesis = offered_v6_genesis(2).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .expect("two-node network starts");
    let offer = signed_offer(
        genesis.genesis_vaults[0].0.clone(),
        &genesis.genesis_vaults[0].1,
    );
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer, &None, false, "root")
        .await
        .expect("well-formed offer is admitted");
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("the offered block is created");
    let mut forged = block.clone();
    forged.body.deploys.push(ProcessedUserDeploy::Legacy(
        casper::rust::util::construct_deploy::basic_processed_deploy(0, Some("root".to_string()))
            .expect("body-only deploy builds"),
    ));
    let forged = resign_block(
        &forged,
        &nodes[0]
            .validator_id_opt
            .as_ref()
            .expect("node 0 is a validator")
            .private_key,
    );
    let status = nodes[1]
        .process_block(forged.clone())
        .await
        .expect("validation completes");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(
                casper::rust::block_status::InvalidBlock::InvalidTransaction
            ))
        ),
        "unexpected status {status:?}"
    );
    assert!(!nodes[1].casper.dag_contains(&forged.block_hash));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_direct_rev_api_proposal_validator_replay_and_receipt() {
    let genesis = offered_v6_genesis(3).await;
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
    let selected = selected_schedule()
        .encode(PhloGenesisPolicy::LIMITS)
        .unwrap();
    adopted.check_acquisition_terms(&selected).unwrap();
    let offer = signed_offer(
        genesis.genesis_vaults[0].0.clone(),
        &genesis.genesis_vaults[0].1,
    );
    let limits = offered_funded_v6_limits().envelope;
    let checked = OfferedFundedDeploy::from_proto(offer.clone(), limits.payload).unwrap();
    assert_eq!(checked.data.phlo_limit(), 2_000_000);
    assert_eq!(checked.data.phlo_price(), 2);
    let body_only = Signed::create(
        models::rust::casper::protocol::casper_message::DeployData {
            term: checked.data.body().term.clone(),
            time_stamp: checked.data.body().time_stamp,
            phlo_price: 2,
            phlo_limit: 2_000_000,
            valid_after_block_number: checked.data.body().valid_after_block_number,
            shard_id: checked.data.body().shard_id.clone(),
            expiration_timestamp: checked.data.body().expiration_timestamp,
        },
        Box::new(Secp256k1),
        genesis.genesis_vaults[0].0.clone(),
    )
    .unwrap();
    assert!(BlockAPI::deploy(
        &nodes[0].engine_cell,
        body_only.clone(),
        &None,
        false,
        "root",
    )
    .await
    .is_err());
    assert!(nodes[0].casper.deploy(body_only).is_err());
    let wallet = authorize_offered_direct_wallet_funding(&checked, DirectWalletFundingLimits {
        members: limits.members,
        funding: limits.payload.funding,
    })
    .unwrap();
    assert_eq!(wallet.payers().len(), 1);
    let service = offered_grpc_service(&nodes[0]);
    let mut changed_price = offer.clone();
    changed_price.phlo_price = 3;
    assert!(OfferedFundedDeploy::from_proto(changed_price.clone(), limits.payload).is_err());
    let changed_price_result = service
        .do_deploy(tonic::Request::new(changed_price))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        changed_price_result.message,
        Some(DeployResponseMessage::Error(_))
    ));
    let mut changed_limit = offer.clone();
    changed_limit.phlo_limit = 2_000_001;
    assert!(OfferedFundedDeploy::from_proto(changed_limit.clone(), limits.payload).is_err());
    let changed_limit_result = service
        .do_deploy(tonic::Request::new(changed_limit))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        changed_limit_result.message,
        Some(DeployResponseMessage::Error(_))
    ));
    let deploy_id: block_storage::rust::dag::block_dag_key_value_storage::DeployId =
        offer.deploy_id.to_vec();
    let wire_offer =
        models::casper::DeployDataProto::decode(offer.encode_to_vec().as_slice()).unwrap();
    let accepted = service
        .do_deploy(tonic::Request::new(wire_offer))
        .await
        .unwrap()
        .into_inner();
    assert!(
        matches!(accepted.message, Some(DeployResponseMessage::Result(result)) if result.contains("DeployId is:"))
    );
    let v1_funding =
        PhloFundingIntentV2::decode(checked.data.funding_intent(), PhloFundingIntentV2Limits {
            wire: limits.payload.funding.wire,
            base: limits.payload.funding,
            grant_uses: limits.payload.funding.wire.total_bytes / 8,
            grant_id_bytes: limits.payload.funding.wire.field_bytes,
            quote_evidence_bytes: limits.payload.funding.wire.field_bytes,
        })
        .unwrap()
        .base
        .encode(limits.payload.funding)
        .unwrap();
    let v1_offer = OfferedFundedDeploy::new(
        checked.data.body().clone(),
        v1_funding,
        2_000_000,
        2,
        limits.payload,
    )
    .unwrap();
    let v1_offer = Cosigned::create_single_envelope(
        v1_offer,
        Box::new(Secp256k1),
        genesis.genesis_vaults[0].0.clone(),
    )
    .unwrap();
    assert!(BlockAPI::deploy_offered(
        &nodes[0].engine_cell,
        OfferedFundedDeploy::to_proto(&v1_offer).unwrap(),
        &None,
        false,
        "root",
    )
    .await
    .is_err());
    // Disabled: legacy accounting forbidden under v6 (epic 8946, D3). Offered deploys no
    // longer alternate block turns with legacy deploys, so block 1 carries the offer.
    // nodes[0].allow_empty_blocks = true;
    // let legacy_turn = TestNode::propagate_block_at_index(&mut nodes, 0, &[])
    //     .await
    //     .unwrap();
    // assert_eq!(legacy_turn.body.state.block_number, 1);
    // assert!(legacy_turn.body.deploys.is_empty());
    let block = nodes[0].create_block_unsafe(&[]).await.unwrap();
    // Disabled: see the legacy-turn note above (epic 8946, D3).
    // assert_eq!(block.body.state.block_number, 2);
    assert_eq!(block.body.state.block_number, 1);
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(block.body.deploys[0].identity_bytes(), deploy_id.as_slice());
    let protocol = offered_funded_v6_limits();
    let offered = block.body.deploys[0].as_offered().unwrap();
    let evidence = offered.evidence(protocol.evidence).unwrap();
    let receipt_limits = NativeWalletReceiptLimits {
        wire: PhloWireLimits {
            total_bytes: protocol.evidence.field_bytes,
            field_bytes: protocol.evidence.field_bytes,
        },
        payers: protocol.envelope.members.get(),
    };
    let mut altered_receipt =
        NativeWalletReceiptV1::decode(evidence.wallet_settlement, receipt_limits).unwrap();
    altered_receipt.rows[0].post_balance += 1;
    let altered_receipt = altered_receipt.encode(receipt_limits).unwrap();
    let mut altered_evidence = evidence.clone();
    altered_evidence.wallet_settlement = &altered_receipt;
    let mut altered_proto = offered.to_proto(protocol.evidence).unwrap();
    altered_proto.native_cost_evidence =
        Some(altered_evidence.encode(protocol.evidence).unwrap().into());
    let mut altered_block = block.clone();
    altered_block.body.deploys[0] = ProcessedUserDeploy::from_proto(altered_proto).unwrap();
    let altered_block = resign_block(
        &altered_block,
        &nodes[0].validator_id_opt.as_ref().unwrap().private_key,
    );
    assert_ne!(altered_block.block_hash, block.block_hash);
    let original_heads = nodes[2]
        .casper
        .block_dag()
        .await
        .unwrap()
        .latest_message_hashes();
    let altered_status = nodes[2].process_block(altered_block.clone()).await.unwrap();
    assert!(matches!(
        altered_status,
        Either::Left(BlockError::BlockException(error))
            if error.to_string().contains("independent wallet receipt differs from committed cost evidence")
    ));
    assert!(!nodes[2].casper.dag_contains(&altered_block.block_hash));
    assert_eq!(
        nodes[2]
            .casper
            .block_dag()
            .await
            .unwrap()
            .latest_message_hashes(),
        original_heads
    );
    assert!(
        BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &deploy_id)
            .await
            .is_err()
    );
    assert!(matches!(
        nodes[0].process_block(block.clone()).await.unwrap(),
        Either::Right(_)
    ));
    assert!(matches!(
        nodes[1].process_block(block.clone()).await.unwrap(),
        Either::Right(_)
    ));
    let producer_receipt =
        BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &deploy_id)
            .await
            .unwrap()
            .unwrap();
    let validator_receipt =
        BlockAPI::find_offered_settlement_receipt(&nodes[1].engine_cell, &deploy_id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(producer_receipt, validator_receipt);
    let public_receipt = offered_web_api(&nodes[1])
        .offered_settlement_receipt(hex::encode(&deploy_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        public_receipt.rev_spent,
        validator_receipt.rev_spent.to_string()
    );
    assert_eq!(
        public_receipt.rev_ceiling,
        validator_receipt.rev_ceiling.to_string()
    );
    assert_eq!(public_receipt.phlo_limit, validator_receipt.phlo_limit);
    assert_eq!(public_receipt.phlo_price, validator_receipt.phlo_price);
    assert_eq!(
        public_receipt.purses[0].post_balance,
        validator_receipt.purses[0].post_balance.to_string()
    );
    let public_json = serde_json::to_value(&public_receipt).unwrap();
    assert_eq!(
        public_json["revSpent"],
        validator_receipt.rev_spent.to_string()
    );
    assert_eq!(public_json["phloLimit"], validator_receipt.phlo_limit);
    assert_eq!(
        public_json["purses"][0]["postBalance"],
        validator_receipt.purses[0].post_balance.to_string()
    );
    assert_eq!(
        producer_receipt.block_hash.as_ref(),
        block.block_hash.as_ref()
    );
    assert_eq!(producer_receipt.phlo_limit, 2_000_000);
    assert_eq!(producer_receipt.phlo_price, 2);
    assert!(producer_receipt.phlo_used <= producer_receipt.phlo_limit);
    assert_eq!(
        producer_receipt.rev_spent,
        u128::from(producer_receipt.phlo_used) * 2 + producer_receipt.fee_rev
    );
    assert!(producer_receipt.rev_spent <= producer_receipt.rev_ceiling);
    let payer_address = VaultAddress::from_public_key(&genesis.genesis_vaults[0].1).unwrap();
    assert_eq!(producer_receipt.purses.len(), 1);
    assert_eq!(
        producer_receipt.purses[0].address,
        payer_address.to_base58().as_bytes()
    );
    let (observed, _) = nodes[1]
        .runtime_manager
        .play_exploratory_deploy(
            balance_query_source(&payer_address),
            &block.body.state.post_state_hash,
            None,
        )
        .await
        .unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(
        u64::try_from(RhoNumber::unapply(&observed[0]).unwrap()).unwrap(),
        producer_receipt.purses[0].post_balance
    );
}
