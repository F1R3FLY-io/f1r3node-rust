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
// Changed by DR-116 (gap G6): unused after the port to offered envelopes.
// use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use node::rust::api::deploy_grpc_service_v1::DeployGrpcServiceV1Impl;
use node::rust::api::web_api::{WebApi, WebApiImpl};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
// Changed by DR-116 (gap G6): unused after the port to offered envelopes.
// use rholang::rust::interpreter::accounting::native_phlo_rules::{
//     native_resource_compatibility_rule, NativePhloDimension,
// };
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

use crate::helper::block_util::resign_block;
// Added by DR-116 (gap G6): the offer builder and the schedule moved to the
// shared offered test module.
use crate::helper::offered_deploy::{owner_direct_offer, selected_schedule};
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
    // Changed by DR-116 (gap G6): the shared offered genesis builder.
    // let mut parameters =
    //     GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators));
    // parameters.2.version = 6;
    // parameters.2.proof_of_stake.min_phlo_price = 1;
    // let policy = PhloGenesisPolicy::from_schedule(&selected_schedule())
    //     .expect("selected schedule forms a genesis policy")
    //     .with_offered_funded_v6_active();
    // GenesisBuilder::new()
    //     .with_resource_policy(policy)
    //     .build_genesis_with_parameters(Some(parameters))
    //     .await
    //     .expect("offered v6 genesis builds")
    GenesisBuilder::offered_v6()
        .build_genesis_with_parameters(Some(
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators)),
        ))
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

/// DR-99 (bug 9954): a node that joins a policy chain after genesis restores
/// from a post-genesis anchor. Its approved block is the anchor, it holds the
/// anchor state and a copy of genesis, and it must adopt the genesis resource
/// policy. Before DR-99 the node did not start, because the policy load took
/// the approved block as the genesis block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_restored_from_a_post_genesis_anchor_adopts_the_genesis_resource_policy() {
    use casper::rust::casper::hash_set_casper;
    use casper::rust::util::rholang::costacc::genesis_resource_policy::GenesisResourcePolicy;
    use models::rust::deploy_envelope::DeployEnvelope;

    let genesis = offered_v6_genesis(1).await;
    let genesis_block = genesis.genesis_block.clone();
    let mut nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, None)
        .await
        .expect("single-node network starts");
    // The anchor carries an offer, so its post-state differs from the
    // genesis post-state (an empty block keeps the genesis post-state).
    BlockAPI::deploy_offered(
        &nodes[0].engine_cell,
        signed_offer(
            genesis.genesis_vaults[0].0.clone(),
            &genesis.genesis_vaults[0].1,
        ),
        &None,
        false,
        "root",
    )
    .await
    .expect("well-formed offer is admitted");
    let anchor = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("an offered block extends the policy chain");
    assert_eq!(anchor.body.deploys.len(), 1);
    assert_eq!(anchor.header.parents_hash_list, vec![genesis_block
        .block_hash
        .clone()]);
    assert_ne!(
        anchor.body.state.post_state_hash,
        genesis_block.body.state.post_state_hash
    );
    let runtime_manager = nodes[0].runtime_manager.clone();
    let shard_conf = nodes[0].casper.casper_shard_conf.clone();

    // Regression witness: the load before DR-99 takes the approved block as
    // the genesis block, so it refuses the anchor.
    let refused = AdoptedResourcePolicy::load(&runtime_manager, &anchor, &shard_conf)
        .await
        .expect_err("the anchor is not a genesis block");
    assert!(refused
        .to_string()
        .contains("requires the approved genesis block"));

    // The sealed constants read at the anchor equal the genesis values, and
    // the reads use the given state: an unknown state fails.
    let at_genesis = GenesisResourcePolicy::load(&runtime_manager, &genesis_block)
        .await
        .expect("the genesis policy loads at the genesis state");
    let at_anchor = GenesisResourcePolicy::load_at(
        &runtime_manager,
        &genesis_block,
        &anchor.body.state.post_state_hash,
    )
    .await
    .expect("the genesis policy loads at the anchor state");
    assert_eq!(
        at_anchor.genesis_root(),
        &genesis_block.body.state.post_state_hash
    );
    assert_eq!(at_anchor.genesis_root(), at_genesis.genesis_root());
    assert_eq!(
        at_anchor.record().encode().expect("the record encodes"),
        at_genesis.record().encode().expect("the record encodes")
    );
    assert_eq!(at_anchor.minimum_price(), at_genesis.minimum_price());
    assert!(GenesisResourcePolicy::load_at(
        &runtime_manager,
        &genesis_block,
        &vec![0x5c; 32].into(),
    )
    .await
    .is_err());

    let deploy_storage = nodes[0].deploy_storage.lock().clone();
    let joined = hash_set_casper(
        nodes[0].casper.block_retriever.clone(),
        nodes[0].casper.event_publisher.clone(),
        Arc::new(runtime_manager.clone()),
        nodes[0].casper.estimator.clone(),
        nodes[0].block_store.clone(),
        nodes[0].block_dag_storage.clone(),
        deploy_storage,
        nodes[0].rejected_deploy_buffer.clone(),
        nodes[0].casper.casper_buffer_storage.clone(),
        None,
        shard_conf.clone(),
        anchor.clone(),
        casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
    )
    .await
    .expect("a node restored from a post-genesis anchor starts");
    assert_eq!(
        joined
            .get_approved_block()
            .expect("the joined node has an approved block")
            .block_hash,
        anchor.block_hash
    );
    let adopted = joined
        .adopted_resource_policy()
        .expect("the joined node adopts the genesis policy");
    assert_eq!(
        adopted.genesis().genesis_root(),
        &genesis_block.body.state.post_state_hash
    );
    assert!(adopted.offered_funded_v6_active());
    assert!(joined.offered_funded_active());
    assert_eq!(joined.get_version(), 6);

    // Admission: the joined node admits an offer under its adopted policy and
    // refuses a policy with another genesis identity.
    let limits = offered_funded_v6_limits().envelope;
    let offer = DeployEnvelope::from_proto(
        signed_offer_at(
            genesis.genesis_vaults[0].0.clone(),
            &genesis.genesis_vaults[0].1,
            11,
        ),
        limits,
    )
    .expect("the offer is a canonical envelope");
    assert!(matches!(
        joined.deploy_envelope(offer, adopted),
        Ok(Either::Right(_))
    ));
    let mut foreign_genesis = genesis_block.clone();
    foreign_genesis.body.state.post_state_hash = anchor.body.state.post_state_hash.clone();
    let foreign = AdoptedResourcePolicy::load_at(
        &runtime_manager,
        &foreign_genesis,
        &anchor.body.state.post_state_hash,
        &shard_conf,
    )
    .await
    .expect("a policy with another identity loads");
    assert_ne!(
        foreign.genesis().genesis_root(),
        adopted.genesis().genesis_root()
    );
    let second_offer = DeployEnvelope::from_proto(
        signed_offer_at(
            genesis.genesis_vaults[0].0.clone(),
            &genesis.genesis_vaults[0].1,
            12,
        ),
        limits,
    )
    .expect("the offer is a canonical envelope");
    assert!(joined.deploy_envelope(second_offer, &foreign).is_err());
}

// DR-101 (bug 10056): system residue in shared registry state is not charged
// to the deployment that last wrote it.

/// The Embers testnet log initializer: registry lookups, a TreeHashMap,
/// contracts on a compound channel and an insertSigned of its environment.
const REGISTRY_ENVIRONMENT_INITIALIZER: &str = r#"new rl(`rho:registry:lookup`),
    rs(`rho:registry:insertSigned:secp256k1`),
    abort(`rho:execution:abort`),
    prevEnvCh,
    initEnv,
    contractDeployer(`rho:rchain:deployerId`),
    log,
    uriCh
in {
    rl!(ENV_URI, *prevEnvCh) |
    for(@Nil <- prevEnvCh) { initEnv!() } |
    for(@(version, _) <- prevEnvCh) {
        if (version < VERSION) { initEnv!() }
    } |
    for(<- initEnv) {
        new rl(`rho:registry:lookup`),
            devNull(`rho:io:devNull`),
            treeHashMapCh,
            treeHashMapLookupCh,
            treeHashMapInitCh,
            stackCh,
            stackLookupCh,
            private
        in {
            rl!(`rho:lang:treeHashMap`, *treeHashMapLookupCh) |
            for(treeHashMap <- treeHashMapLookupCh) {
                treeHashMap!("init", 3, *treeHashMapInitCh) |
                for(@map <- treeHashMapInitCh) { treeHashMapCh!(*treeHashMap, map) }
            } |
            rl!(`rho:lang:stack`, *stackLookupCh) |
            for(@(_, stack) <- stackLookupCh) { stackCh!(stack) } |
            contract @(*log, *private)(@level, @message) = {
                new deployData(`rho:deploy:data`), deployDataCh, valueCh, nilCh, logsCh in {
                    deployData!(*deployDataCh) |
                    for(_, _, @deployId <- deployDataCh; treeHashMap, @map <<- treeHashMapCh; stack <<- stackCh) {
                        treeHashMap!("getOrElse", map, deployId.toString(), *valueCh, *nilCh) |
                        for(@logs <- valueCh) {
                            stack!("push", logs, {"level": level, "message": message}, *devNull)
                        } |
                        for(<- nilCh) {
                            stack!("init", *logsCh) |
                            for(@logs <- logsCh) {
                                stack!("push", logs, {"level": level, "message": message}, *devNull) |
                                treeHashMap!("set", map, deployId.toString(), logs, *devNull)
                            }
                        }
                    }
                }
            } |
            contract log(@"info", @message) = { @(*log, *private)!("info", message) } |
            contract log(@"get", @deployId, ret) = {
                new valueCh, nilCh in {
                    for(treeHashMap, @map <<- treeHashMapCh) {
                        treeHashMap!("getOrElse", map, deployId, *valueCh, *nilCh)
                    } |
                    for(@logs <- valueCh; stack <<- stackCh) { stack!("toList", logs, *ret) } |
                    for(<- nilCh) { ret!(Nil) }
                }
            }
        } |
        rs!(PUBLIC_KEY, (VERSION, bundle+{*log}), SIG, *uriCh) |
        for(@Nil <- uriCh) { abort!("failed to insert env") }
    }
}"#;

const REGISTRY_LOOKUP: &str =
    "new rl(`rho:registry:lookup`), ret in { rl!(`rho:lang:treeHashMap`, *ret) }";

/// The insertSigned signature over `(timestamp, deployer, version)` that the
/// registry checks against the inserting deployment.
fn registry_insert_signature(
    environment_secret: &[u8; 32],
    deployer_public: &[u8],
    time_stamp: i64,
) -> (Vec<u8>, Vec<u8>) {
    use crypto::rust::hash::blake2b256::Blake2b256;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{ETuple, Expr, Par};

    let ground = |instance: ExprInstance| Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    };
    let signed = ground(ExprInstance::ETupleBody(ETuple {
        ps: vec![
            ground(ExprInstance::GInt(time_stamp)),
            ground(ExprInstance::GByteArray(deployer_public.to_vec())),
            ground(ExprInstance::GInt(0)),
        ],
        ..Default::default()
    }));
    let environment_public = Secp256k1.to_public(
        &crypto::rust::private_key::PrivateKey::from_bytes(environment_secret),
    );
    (
        environment_public.bytes.to_vec(),
        Secp256k1.sign(
            &Blake2b256::hash(signed.encode_to_vec()),
            environment_secret,
        ),
    )
}

fn registry_environment_initializer(deployer_public: &[u8], time_stamp: i64) -> String {
    use crypto::rust::hash::blake2b256::Blake2b256;

    let (environment_public, signature) =
        registry_insert_signature(&[0x42; 32], deployer_public, time_stamp);
    let uri = rholang::rust::interpreter::registry::registry::Registry::build_uri(
        &Blake2b256::hash(environment_public.clone()),
    );
    REGISTRY_ENVIRONMENT_INITIALIZER
        .replace("ENV_URI", &format!("`{uri}`"))
        .replace("VERSION", "0")
        .replace(
            "PUBLIC_KEY",
            &format!("\"{}\".hexToBytes()", hex::encode(&environment_public)),
        )
        .replace(
            "SIG",
            &format!("\"{}\".hexToBytes()", hex::encode(&signature)),
        )
}

fn registry_insert(
    deployer_public: &[u8],
    time_stamp: i64,
    environment_secret: [u8; 32],
) -> String {
    let (environment_public, signature) =
        registry_insert_signature(&environment_secret, deployer_public, time_stamp);
    format!(
        "new rs(`rho:registry:insertSigned:secp256k1`), uriCh, value in {{ \
         rs!(\"{}\".hexToBytes(), (0, bundle+{{*value}}), \"{}\".hexToBytes(), *uriCh) }}",
        hex::encode(&environment_public),
        hex::encode(&signature)
    )
}

/// Proposes one owner-direct offer on node 0. Every other node replays the
/// block, and all settlement receipts agree.
async fn propose_offer(
    nodes: &mut [TestNode],
    offer: models::casper::DeployDataProto,
) -> Result<
    (
        models::rust::casper::protocol::casper_message::BlockMessage,
        casper::rust::api::block_api::OfferedSettlementReceipt,
    ),
    String,
> {
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer.clone(), &None, false, "root")
        .await
        .expect("a well-formed offer is admitted");
    let id = models::rust::deploy_id::DeployIdV6::try_from(offer.deploy_id.to_vec())
        .expect("an offered deploy id has 32 bytes");
    let Ok(block) = nodes[0].create_block_unsafe(&[]).await else {
        let storage = nodes[0].deploy_storage.lock();
        return Err(storage
            .envelope_rejection(&id)
            .map(|rejection| rejection.reason)
            .unwrap_or_else(|| "no block and no recorded rejection".to_string()));
    };
    assert_eq!(block.body.deploys.len(), 1);
    assert!(!block.body.deploys[0].is_failed());
    for node in nodes.iter_mut() {
        assert!(matches!(
            node.process_block(block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    let deploy_id = offer.deploy_id.to_vec();
    let receipts = futures::future::join_all(
        nodes
            .iter()
            .map(|node| BlockAPI::find_offered_settlement_receipt(&node.engine_cell, &deploy_id)),
    )
    .await;
    let receipts = receipts
        .into_iter()
        .map(|receipt| receipt.unwrap().expect("every node holds the receipt"))
        .collect::<Vec<_>>();
    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(receipts[0].purses.len(), 1);
    Ok((block, receipts[0].clone()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registry_initializer_is_funded_on_a_fresh_genesis() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let signer = genesis.genesis_vaults[0].clone();
    let term = registry_environment_initializer(&signer.1.bytes, 1);
    propose_offer(&mut nodes, owner_direct_offer(&signer, 1, term))
        .await
        .expect("the initializer is funded by its own signer");
}

/// The registry work of signer B after signer A wrote the registry.
async fn after_another_signers_registry_insert(
    second_term: impl Fn(&[u8], i64) -> String,
) -> Result<casper::rust::api::block_api::OfferedSettlementReceipt, String> {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let writer = genesis.genesis_vaults[2].clone();
    let reader = genesis.genesis_vaults[0].clone();
    assert_ne!(writer.1.bytes, reader.1.bytes);
    propose_offer(
        &mut nodes,
        owner_direct_offer(&writer, 1, registry_insert(&writer.1.bytes, 1, [0x30; 32])),
    )
    .await
    .expect("the writer's insert is funded");
    propose_offer(
        &mut nodes,
        owner_direct_offer(&reader, 2, second_term(&reader.1.bytes, 2)),
    )
    .await
    .map(|(_, receipt)| receipt)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_signer_initializes_a_registry_environment_after_a_registry_insert() {
    after_another_signers_registry_insert(registry_environment_initializer)
        .await
        .expect("registry residue of another signer costs this signer nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_signer_looks_up_the_registry_after_a_registry_insert() {
    after_another_signers_registry_insert(|_, _| REGISTRY_LOOKUP.to_string())
        .await
        .expect("a registry lookup is funded by its own signer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_signer_inserts_into_the_registry_after_a_registry_insert() {
    after_another_signers_registry_insert(|public, time_stamp| {
        registry_insert(public, time_stamp, [0x31; 32])
    })
    .await
    .expect("a registry insert is funded by its own signer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_registry_lookup_costs_the_same_whoever_wrote_the_registry_last() {
    let genesis = offered_v6_genesis(3).await;
    let writer = genesis.genesis_vaults[2].clone();
    let reader = genesis.genesis_vaults[0].clone();
    let mut spent = Vec::new();
    for looker in [&writer, &reader] {
        let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
            .await
            .unwrap();
        propose_offer(
            &mut nodes,
            owner_direct_offer(&writer, 1, registry_insert(&writer.1.bytes, 1, [0x30; 32])),
        )
        .await
        .expect("the writer's insert is funded");
        let (_, receipt) = propose_offer(
            &mut nodes,
            owner_direct_offer(looker, 2, REGISTRY_LOOKUP.to_string()),
        )
        .await
        .expect("the lookup is funded");
        spent.push(receipt.rev_spent);
    }
    assert_eq!(spent[0], spent[1]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn user_written_data_still_needs_its_writers_token() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let writer = genesis.genesis_vaults[2].clone();
    let reader = genesis.genesis_vaults[0].clone();
    propose_offer(
        &mut nodes,
        owner_direct_offer(&writer, 1, "@\"user-written-channel\"!(1)".to_string()),
    )
    .await
    .expect("the writer's datum is funded");
    let rejection = propose_offer(
        &mut nodes,
        owner_direct_offer(
            &reader,
            2,
            "for(_ <- @\"user-written-channel\") { Nil }".to_string(),
        ),
    )
    .await
    .expect_err("consuming a user-signed datum needs the writer's token (DR-68/D5)");
    assert!(
        rejection.contains("no feasible signed assignment"),
        "{rejection}"
    );
}

/// DR-101 (P1 rem:signed-subst): `Either.map2` runs the caller's function
/// inside a genesis body. The function keeps its caller's seal, so what it
/// leaves behind is user-written data that still needs the caller's token.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_that_a_system_contract_runs_keeps_its_callers_seal() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let caller = genesis.genesis_vaults[2].clone();
    let other = genesis.genesis_vaults[0].clone();
    let map2 = r#"new rl(`rho:registry:lookup`), eitherCh, ret, f in {
        rl!(`rho:lang:either`, *eitherCh) |
        for(@(_, either) <- eitherCh) {
            @either!("map2", (true, 1), (true, 2),
                for (@x, @y, r <- f) { r!(x + y) | for(_ <- @"either-leftover") { Nil } },
                *ret)
        }
    }"#;
    propose_offer(&mut nodes, owner_direct_offer(&caller, 1, map2.to_string()))
        .await
        .expect("the caller's map2 is funded");
    let rejection = propose_offer(
        &mut nodes,
        owner_direct_offer(&other, 2, "@\"either-leftover\"!(1)".to_string()),
    )
    .await
    .expect_err("the leftover receive keeps the caller's seal");
    assert!(
        rejection.contains("no feasible signed assignment"),
        "{rejection}"
    );
}

/// A term that moves `amount` from the signer's vault at `from` to the vault at
/// `to`, and publishes the transfer result.
fn vault_transfer(from: &VaultAddress, to: &VaultAddress, amount: u64) -> String {
    format!(
        r#"new rl(`rho:registry:lookup`), systemVaultCh, vaultCh, authKeyCh, resultCh,
          deployerId(`rho:system:deployerId`) in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{}", *vaultCh) |
            @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
            for (@(true, vault) <- vaultCh & key <- authKeyCh) {{
              @vault!("transfer", "{}", {amount}, *key, *resultCh) |
              for (@result <- resultCh) {{ @"vault-transfer-result"!(result) }}
            }}
          }}
        }}"#,
        from.to_base58(),
        to.to_base58(),
    )
}

async fn vault_balance(
    nodes: &[TestNode],
    root: &models::rust::block::state_hash::StateHash,
    address: &VaultAddress,
) -> u64 {
    let (observed, _) = nodes[0]
        .runtime_manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(observed.len(), 1);
    u64::try_from(RhoNumber::unapply(&observed[0]).unwrap()).unwrap()
}

/// DR-101: a deposit into a vault is system residue of the deployment that
/// made it, so the owner of the vault spends it with its own token alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_signer_spends_a_deposit_that_another_signer_made() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let owner = genesis.genesis_vaults[2].clone();
    let depositor = genesis.genesis_vaults[0].clone();
    let owner_vault = VaultAddress::from_public_key(&owner.1).unwrap();
    let depositor_vault = VaultAddress::from_public_key(&depositor.1).unwrap();
    let initial = vault_balance(
        &nodes,
        &genesis.genesis_block.body.state.post_state_hash,
        &owner_vault,
    )
    .await;
    let (deposit, _) = propose_offer(
        &mut nodes,
        owner_direct_offer(
            &depositor,
            1,
            vault_transfer(&depositor_vault, &owner_vault, 1_000),
        ),
    )
    .await
    .expect("the depositor's transfer is funded");
    let deposited = vault_balance(&nodes, &deposit.body.state.post_state_hash, &owner_vault).await;
    assert_eq!(deposited, initial + 1_000);
    let (spend, receipt) = propose_offer(
        &mut nodes,
        owner_direct_offer(
            &owner,
            2,
            vault_transfer(&owner_vault, &depositor_vault, 500),
        ),
    )
    .await
    .expect("the owner spends the deposit with its own token");
    let remaining = vault_balance(&nodes, &spend.body.state.post_state_hash, &owner_vault).await;
    assert_eq!(
        u128::from(remaining) + 500 + receipt.rev_spent,
        u128::from(deposited)
    );
}

/// A term that inserts version 1.0.0 of `project` through the public
/// `rho:registry:1.0.0` entry point, and publishes the result.
fn version_insert(project: &str) -> String {
    format!(
        r#"new getReg(`rho:registry:1.0.0`), notify, regCh, ret in {{
          getReg!(*regCh, *notify) |
          for (@reg <- regCh) {{
            @reg!("insertVersion", "serve", "{project}", "1.0.0", "code", *ret) |
            for (@inserted <- ret) {{ @"version-inserted-{project}"!(inserted) }}
          }}
        }}"#
    )
}

/// DR-101: the versioned registry keeps every entry in one store datum. After
/// one signer inserts a version, another signer inserts a version with its own
/// token alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_signer_inserts_a_version_after_a_version_insert() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let writer = genesis.genesis_vaults[2].clone();
    let reader = genesis.genesis_vaults[0].clone();
    propose_offer(
        &mut nodes,
        owner_direct_offer(&writer, 1, version_insert("first")),
    )
    .await
    .expect("the first insert is funded");
    let (block, _) = propose_offer(
        &mut nodes,
        owner_direct_offer(&reader, 2, version_insert("second")),
    )
    .await
    .expect("the second insert is funded by its own signer");
    let published = nodes[0]
        .runtime_manager
        .get_data(
            block.body.state.post_state_hash.clone(),
            &models::rust::utils::new_gstring_par(
                "version-inserted-second".to_owned(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        rholang::rust::interpreter::rho_type::RhoBoolean::unapply(&published[0]),
        Some(true)
    );
}

/// DR-101: one signer writes the versioned registry in a block, and another
/// signer writes a user datum in a sibling block. A third signer inserts a
/// version in the block that merges both branches, with its own token alone.
///
/// Disabled until gap G6 of the Phase D plan lands: on this branch, the
/// proposer of a block with two offered parents fails with "offered-funded
/// recovery requires an envelope buffer", before funding runs. The test
/// fails for that reason with and without DR-101.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// Re-enabled by DR-116: gap G6 lands offered recovery for a multi-parent merge.
// #[ignore = "needs gap G6 (offered recovery for a multi-parent merge); see DR-101"]
async fn a_third_signer_inserts_a_version_after_a_merge_with_a_writers_branch() {
    let genesis = offered_v6_genesis(4).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let first = genesis.genesis_vaults[2].clone();
    let second = genesis.genesis_vaults[3].clone();
    let third = genesis.genesis_vaults[0].clone();
    let mut siblings = Vec::with_capacity(2);
    for (index, (writer, term)) in [
        (&first, version_insert("first")),
        (&second, "@\"sibling-datum\"!(1)".to_string()),
    ]
    .into_iter()
    .enumerate()
    {
        let offer = owner_direct_offer(writer, index as i64 + 1, term);
        BlockAPI::deploy_offered(&nodes[index].engine_cell, offer, &None, false, "root")
            .await
            .expect("a well-formed offer is admitted");
        let block = nodes[index]
            .create_block_unsafe(&[])
            .await
            .expect("each sibling deploy is funded");
        assert_eq!(block.header.parents_hash_list, vec![genesis
            .genesis_block
            .block_hash
            .clone()]);
        siblings.push(block);
    }
    for node in nodes.iter_mut() {
        for block in &siblings {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }
    let (merged, _) = propose_offer(
        &mut nodes,
        owner_direct_offer(&third, 3, version_insert("third")),
    )
    .await
    .expect("the third insert is funded by its own signer");
    assert_eq!(merged.header.parents_hash_list.len(), 2);
    let published = nodes[0]
        .runtime_manager
        .get_data(
            merged.body.state.post_state_hash.clone(),
            &models::rust::utils::new_gstring_par(
                "version-inserted-third".to_owned(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(
        rholang::rust::interpreter::rho_type::RhoBoolean::unapply(&published[0]),
        Some(true)
    );
}

/// DR-102: on a small offered block, the producer's self-replay and every
/// validator replay charge the same usage, and each role charges its own
/// acceptance work to a separate budget. The block fits the committed caps.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_replay_usage_is_identical_across_roles_on_a_small_block() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let signer = genesis.genesis_vaults[0].clone();
    crate::helper::offered_replay_usage::propose_offer_with_identical_replay_usage(
        &mut nodes,
        owner_direct_offer(&signer, 1, "new x in { x!(0) }".to_string()),
        "small block",
    )
    .await;
}

/// A term that completes one COMM and then fails with a classified user
/// error (OperatorExpectedError), so the native run rolls back its user
/// events and publishes the deploy as a charged user failure.
const FAILING_AFTER_A_COMM: &str = "new ack in { ack!(0) | for (_ <- ack) { ack!(1 + \"a\") } }";

/// Admits `offer` on `nodes[creator]` and creates its block. No node has
/// processed the block yet, so a sibling can still be created first.
async fn offered_block(
    nodes: &mut [TestNode],
    creator: usize,
    offer: models::casper::DeployDataProto,
) -> (
    models::rust::casper::protocol::casper_message::BlockMessage,
    Vec<u8>,
) {
    BlockAPI::deploy_offered(
        &nodes[creator].engine_cell,
        offer.clone(),
        &None,
        false,
        "root",
    )
    .await
    .expect("a well-formed offer is admitted");
    let id = models::rust::deploy_id::DeployIdV6::try_from(offer.deploy_id.to_vec())
        .expect("an offered deploy id has 32 bytes");
    let block = match nodes[creator].create_block_unsafe(&[]).await {
        Ok(block) => block,
        Err(error) => {
            let storage = nodes[creator].deploy_storage.lock();
            panic!(
                "the offer's block is not created ({error:?}); recorded rejection: {:?}",
                storage
                    .envelope_rejection(&id)
                    .map(|rejection| rejection.reason)
            );
        }
    };
    assert_eq!(block.body.deploys.len(), 1);
    assert!(block.body.deploys[0].as_offered().is_some());
    (block, offer.deploy_id.to_vec())
}

/// The user event index of `deploy_id`'s merge chain in `block`, and the index
/// that `events` alone give with the deploy's stored mergeable map.
fn merge_chain_index(
    node: &TestNode,
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    deploy_id: &[u8],
    events: &[models::rust::casper::protocol::casper_message::Event],
) -> (
    rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
    rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
) {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
    let runtime_manager = &node.runtime_manager;
    let pre_state = Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash);
    let post_state = Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash);
    let mergeable = runtime_manager
        .load_mergeable_channels(
            &block.body.state.post_state_hash,
            block.sender.clone(),
            block.seq_num,
        )
        .expect("the block's mergeable channels are stored");
    let index = casper::rust::merging::block_index::new(
        &block.block_hash,
        block.body.state.block_number,
        &block.body.deploys,
        &block.body.system_deploys,
        &pre_state,
        &post_state,
        &runtime_manager.history_repo,
        &mergeable,
    )
    .expect("the block indexes");
    let chain = index
        .deploy_chains
        .iter()
        .find(|chain| {
            chain
                .deploys_with_cost
                .0
                .iter()
                .any(|deploy| deploy.deploy_id.as_ref() == deploy_id)
        })
        .expect("the deploy has a merge chain");
    let expected = casper::rust::merging::block_index::create_event_log_index(
        events,
        runtime_manager.history_repo.clone(),
        &pre_state,
        mergeable[0].clone(),
    );
    (
        chain.user_event_log_index.clone(),
        EventLogIndex::combine(&EventLogIndex::empty(), &expected)
            .expect("an index combines with the empty index"),
    )
}

/// The channels that `events` produce on, directly or inside a COMM.
fn produced_channels(
    events: &[models::rust::casper::protocol::casper_message::Event],
) -> Vec<rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash> {
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
    let mut channels = Vec::with_capacity(events.len());
    for event in events {
        match casper::rust::util::event_converter::to_rspace_event(event) {
            Event::IoEvent(IOEvent::Produce(produce)) => channels.push(produce.channel_hash),
            Event::IoEvent(IOEvent::Consume(_)) => {}
            Event::Comm(comm) => channels.extend(comm.produces.into_iter().map(|p| p.channel_hash)),
        }
    }
    channels.sort();
    channels.dedup();
    channels
}

/// DR-115 (bug 10986): block A holds a failed offered deploy, and block B is
/// an empty sibling with more stake. The merge block M takes B as its base
/// and merges A from scope. M's pre-state must keep A's payer debit and fee.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_offered_deploy_keeps_its_charge_when_its_block_is_a_merged_branch() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    nodes[1].allow_empty_blocks = true;
    nodes[2].allow_empty_blocks = true;
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let fee_vault = VaultAddress::from_public_key(&genesis.validator_key_pairs[0].1).unwrap();
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;
    assert_eq!(vault_balance(&nodes, &genesis_root, &fee_vault).await, 0);

    let (failed, id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&payer, 1, FAILING_AFTER_A_COMM.to_string()),
    )
    .await;
    assert!(failed.body.deploys[0].is_failed());
    assert_eq!(failed.sender, genesis.validator_key_pairs[0].1.bytes);
    let sibling = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("node 1 creates an empty sibling");
    assert!(sibling.body.deploys.is_empty());
    for node in nodes.iter_mut() {
        for block in [&failed, &sibling] {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }

    // The merge index holds exactly the committed suffix: everything after the
    // three user events of the term (one stored and one matched operation),
    // with the wallet map.
    let offered = failed.body.deploys[0]
        .as_offered()
        .expect("block A holds an offered deploy");
    let evidence = offered
        .evidence(offered_funded_v6_limits().evidence)
        .expect("the committed evidence decodes");
    let user_events =
        casper::rust::util::rholang::costacc::offered_evidence::committed_user_event_count(
            &evidence,
            &rholang::rust::interpreter::host_work::HostWorkBudget::new(
                models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits(),
            ),
        )
        .expect("the committed journal decodes");
    assert_eq!(user_events, 3);
    let suffix =
        casper::rust::util::rholang::costacc::offered_evidence::failed_offered_committed_suffix(
            offered,
        )
        .expect("the committed suffix decodes");
    assert_eq!(suffix, &offered.deploy_log()[user_events..]);
    assert!(!suffix.is_empty());
    // A journal that does not decode fails closed: the index neither skips the
    // deploy nor takes its whole log.
    let corrupted_evidence = models::rust::native_cost_evidence::NativeCostEvidenceV1 {
        operation_journal: &[0xff, 0xff],
        ..evidence.clone()
    };
    let corrupted =
        models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy::new(
            offered.envelope().clone(),
            *offered.cost(),
            offered.deploy_log().to_vec(),
            true,
            casper::rust::util::rholang::costacc::offered_evidence::encode_committed_native_evidence(
                &corrupted_evidence,
                offered_funded_v6_limits().evidence,
                &rholang::rust::interpreter::host_work::HostWorkBudget::new(
                    models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits(),
                ),
            )
            .expect("the corrupted evidence encodes"),
            offered_funded_v6_limits().evidence,
        )
        .expect("the envelope checks do not decode the journal");
    assert!(
        casper::rust::util::rholang::costacc::offered_evidence::failed_offered_committed_suffix(
            &corrupted
        )
        .is_err()
    );
    let (indexed, expected) = merge_chain_index(&nodes[2], &failed, &id, suffix);
    assert_eq!(
        indexed, expected,
        "the merge index holds exactly the committed suffix"
    );

    let merge = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates the merge block");
    assert_eq!(merge.header.parents_hash_list, vec![
        sibling.block_hash.clone(),
        failed.block_hash.clone()
    ]);
    for node in nodes.iter_mut() {
        assert!(matches!(
            node.process_block(merge.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    assert!(merge.body.rejected_deploys.is_empty());

    let receipt = BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &id)
        .await
        .unwrap()
        .expect("the failed deploy carries a settlement receipt");
    assert!(receipt.rev_spent > 0);
    let merged_root = merge.body.state.pre_state_hash.clone();
    let charged = vault_balance(&nodes, &merged_root, &payer_vault).await;
    assert_eq!(
        u128::from(charged) + receipt.rev_spent,
        u128::from(initial),
        "the merged state keeps the payer debit"
    );
    assert_eq!(charged, receipt.purses[0].post_balance);
    let fee = vault_balance(&nodes, &merged_root, &fee_vault).await;
    assert_eq!(
        u128::from(fee),
        receipt.fee_rev,
        "the merged state keeps the fee"
    );
    assert_eq!(
        fee,
        vault_balance(&nodes, &failed.body.state.post_state_hash, &fee_vault).await
    );
    assert!(merge
        .body
        .applied_from_scope
        .iter()
        .any(|sig| sig.as_ref() == id.as_slice()));

    // M's pre-state agrees with A's post-state on every channel that A's
    // committed suffix produces on: the balances, the fee vault, the payer's
    // settlement cursor and the receipts.
    let channels = produced_channels(suffix);
    assert!(!channels.is_empty());
    let history = &nodes[2].runtime_manager.history_repo;
    let after_failed = history
        .get_history_reader(&Blake2b256Hash::from_bytes_prost(
            &failed.body.state.post_state_hash,
        ))
        .expect("A's post-state is readable");
    let merged = history
        .get_history_reader(&Blake2b256Hash::from_bytes_prost(
            &merge.body.state.pre_state_hash,
        ))
        .expect("M's pre-state is readable");
    for channel in &channels {
        let committed = after_failed
            .get_data(channel)
            .expect("A's post-state data reads");
        let carried = merged.get_data(channel).expect("M's pre-state data reads");
        assert_eq!(committed.len(), carried.len(), "channel {channel:?}");
        assert!(
            committed.iter().all(|datum| carried.contains(datum)),
            "channel {channel:?}"
        );
    }
}

/// DR-115 (bug 10986): the base sibling settles an offered deploy of another
/// payer. A failed offered deploy of the merged branch is then charged in the
/// merged state or rejected with a record. It is never dropped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_offered_deploy_in_a_conflicting_merged_branch_is_rejected_not_dropped() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;

    let (failed, id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&payer, 1, FAILING_AFTER_A_COMM.to_string()),
    )
    .await;
    assert!(failed.body.deploys[0].is_failed());
    let (sibling, sibling_id) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(
            &genesis.genesis_vaults[0],
            2,
            "new x in { x!(0) }".to_string(),
        ),
    )
    .await;
    assert!(!sibling.body.deploys[0].is_failed());
    let (sibling_indexed, sibling_expected) = merge_chain_index(
        &nodes[1],
        &sibling,
        &sibling_id,
        sibling.body.deploys[0].deploy_log(),
    );
    assert_eq!(
        sibling_indexed, sibling_expected,
        "a successful offered deploy keeps its whole log in the index"
    );
    for node in nodes.iter_mut() {
        for block in [&failed, &sibling] {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }

    let snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("node 2 takes a snapshot");
    assert_eq!(snapshot.parents.len(), 2);
    assert_eq!(snapshot.parents[0].block_hash, sibling.block_hash);
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect();
    let runtime_manager = nodes[2].runtime_manager.clone();
    let merged = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[2].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &runtime_manager,
        &latest_messages,
        None,
        None,
        None,
    )
    .await
    .expect("the merge over both siblings computes");

    let receipt = BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &id)
        .await
        .unwrap()
        .expect("the failed deploy carries a settlement receipt");
    let balance = vault_balance(&nodes[2..], &merged.state, &payer_vault).await;
    let charged = u128::from(balance) + receipt.rev_spent == u128::from(initial);
    let untouched = balance == initial;
    let rejected = merged
        .rejected_user
        .iter()
        .any(|record| record.sig.as_ref() == id.as_slice() && record.carrier == failed.block_hash);
    assert!(
        charged || untouched,
        "the payer is charged once or not at all"
    );
    assert!(
        charged ^ rejected,
        "the failed deploy is charged or rejected with a record \
         (charged = {charged}, rejected = {rejected})"
    );
}

/// DR-115 (bug 10986): two validators include the same failed offer in sibling
/// blocks. Both copies settle on the payer's cursor, so their settlements
/// conflict and the merged state charges the payer exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_offered_deploy_in_two_sibling_blocks_is_charged_once() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;

    let offer = owner_direct_offer(&payer, 1, FAILING_AFTER_A_COMM.to_string());
    let (first, id) = offered_block(&mut nodes, 0, offer.clone()).await;
    let (second, second_id) = offered_block(&mut nodes, 1, offer).await;
    assert_eq!(id, second_id);
    assert!(first.body.deploys[0].is_failed());
    assert!(second.body.deploys[0].is_failed());
    for node in nodes.iter_mut() {
        for block in [&first, &second] {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }

    let snapshot = nodes[2]
        .casper
        .get_snapshot()
        .await
        .expect("node 2 takes a snapshot");
    assert_eq!(snapshot.parents.len(), 2);
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect();
    let runtime_manager = nodes[2].runtime_manager.clone();
    let merged = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &nodes[2].block_store,
        snapshot.parents.clone(),
        &snapshot,
        &runtime_manager,
        &latest_messages,
        None,
        None,
        None,
    )
    .await
    .expect("the merge over both copies computes");

    let receipt = BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &id)
        .await
        .unwrap()
        .expect("the failed deploy carries a settlement receipt");
    let balance = vault_balance(&nodes[2..], &merged.state, &payer_vault).await;
    assert_eq!(
        u128::from(balance) + receipt.rev_spent,
        u128::from(initial),
        "the merged state charges the payer exactly once"
    );
    assert_eq!(
        merged
            .rejected_user
            .iter()
            .filter(|record| record.sig.as_ref() == id.as_slice())
            .count(),
        1,
        "the merge rejects the second copy with one record"
    );
}

/// DR-116 (gap G6): the identities in a node's rejected-envelope buffer.
fn buffered_envelope_ids(node: &TestNode) -> Vec<Vec<u8>> {
    node.rejected_deploy_buffer
        .lock()
        .expect("the buffer lock is not poisoned")
        .read_all_envelopes(offered_funded_v6_limits().envelope)
        .expect("the envelope table decodes")
        .iter()
        .map(|envelope| envelope.identity().as_bytes().to_vec())
        .collect()
}

/// DR-116 (gap G6): two offered siblings conflict through the global cost
/// cursor lock (bug 11004), so the merge rejects node 0's offer. Node 0 owns the
/// rejected carrier. It accepts the merge block and buffers the envelope. Before
/// DR-116 it marked the honest block invalid.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_accepts_a_merge_that_rejects_its_own_offer() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    nodes[2].allow_empty_blocks = true;
    let (first, first_id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(
            &genesis.genesis_vaults[2],
            1,
            "new x in { x!(0) }".to_string(),
        ),
    )
    .await;
    let (second, _) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(
            &genesis.genesis_vaults[0],
            2,
            "new y in { y!(0) }".to_string(),
        ),
    )
    .await;
    assert!(!first.body.deploys[0].is_failed());
    for node in nodes.iter_mut() {
        for block in [&first, &second] {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }
    let merge = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates the merge block");
    assert_eq!(merge.header.parents_hash_list, vec![
        second.block_hash.clone(),
        first.block_hash.clone()
    ]);
    assert!(
        merge.body.rejected_deploys.iter().any(|record| {
            record.sig.as_ref() == first_id.as_slice()
                && record.carrier == first.block_hash
                && !record.duplicate
        }),
        "staging precondition: the merge rejects node 0's offer"
    );
    for node in nodes.iter_mut() {
        assert!(
            matches!(
                node.process_block(merge.clone()).await.unwrap(),
                Either::Right(_)
            ),
            "every node accepts the merge block"
        );
    }
    assert_eq!(buffered_envelope_ids(&nodes[0]), vec![first_id]);
    assert!(buffered_envelope_ids(&nodes[1]).is_empty());
    assert!(buffered_envelope_ids(&nodes[2]).is_empty());
}

/// Stages a merge that rejects a failed offered deploy of node 0: block A holds
/// the failed deploy, and the base sibling B settles another payer's offer.
/// Returns the nodes, A, the failed deploy's identity, the merge block and the
/// payer's genesis balance. No node has processed the merge block yet.
async fn rejected_failed_offer_merge() -> (
    Vec<TestNode>,
    models::rust::casper::protocol::casper_message::BlockMessage,
    Vec<u8>,
    models::rust::casper::protocol::casper_message::BlockMessage,
    u64,
) {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    nodes[2].allow_empty_blocks = true;
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let initial = vault_balance(
        &nodes,
        &genesis.genesis_block.body.state.post_state_hash,
        &payer_vault,
    )
    .await;
    let (failed, id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&payer, 1, FAILING_AFTER_A_COMM.to_string()),
    )
    .await;
    assert!(failed.body.deploys[0].is_failed());
    let (sibling, _) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(
            &genesis.genesis_vaults[0],
            2,
            "new x in { x!(0) }".to_string(),
        ),
    )
    .await;
    for node in nodes.iter_mut() {
        for block in [&failed, &sibling] {
            assert!(matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ));
        }
    }
    let merge = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates the merge block");
    assert!(
        merge.body.rejected_deploys.iter().any(|record| {
            record.sig.as_ref() == id.as_slice()
                && record.carrier == failed.block_hash
                && !record.duplicate
        }),
        "staging precondition: the merge rejects the failed offered deploy"
    );
    (nodes, failed, id, merge, initial)
}

/// DR-116 (gap G6): a failed offered deploy that a merge rejects is final, as a
/// failed deploy is in Casper. Its owner accepts the merge and buffers nothing,
/// and the merged state does not charge the payer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rejected_failed_offered_deploy_is_not_recovered() {
    let (mut nodes, _failed, id, merge, initial) = rejected_failed_offer_merge().await;
    for node in nodes.iter_mut() {
        assert!(
            matches!(
                node.process_block(merge.clone()).await.unwrap(),
                Either::Right(_)
            ),
            "every node accepts the merge block"
        );
    }
    for node in &nodes {
        assert!(buffered_envelope_ids(node).is_empty());
    }
    let payer =
        VaultAddress::from_public_key(&offered_v6_genesis(3).await.genesis_vaults[2].1).unwrap();
    assert_eq!(
        vault_balance(&nodes, &merge.body.state.pre_state_hash, &payer).await,
        initial,
        "the merged state does not charge the rejected failed deploy"
    );
    assert!(!merge
        .body
        .applied_from_scope
        .iter()
        .any(|sig| sig.as_ref() == id.as_slice()));
}

/// DR-116 (gap G6): the rejection of a failed offered copy ends the recovery
/// custody of its identity, so an entry that the owner already buffered for the
/// same identity is removed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rejected_failed_offered_copy_removes_the_buffered_entry() {
    let (mut nodes, failed, id, merge, _initial) = rejected_failed_offer_merge().await;
    let envelope = failed.body.deploys[0]
        .as_offered()
        .expect("block A holds an offered deploy")
        .envelope()
        .clone();
    nodes[0]
        .rejected_deploy_buffer
        .lock()
        .expect("the buffer lock is not poisoned")
        .add_envelopes(std::slice::from_ref(&envelope))
        .expect("the envelope is buffered");
    assert_eq!(buffered_envelope_ids(&nodes[0]), vec![id]);
    assert!(matches!(
        nodes[0].process_block(merge.clone()).await.unwrap(),
        Either::Right(_)
    ));
    assert!(buffered_envelope_ids(&nodes[0]).is_empty());
}

/// DR-113: a deploy that exhausts its signed phlo limit is a classified user
/// failure. The block publishes it as failed, the payer pays the granted work
/// and the fee, and every validator accepts the block with the same receipt.
/// Before DR-113 the exhaustion was a certificate failure, and the proposer
/// rejected the offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signed_limit_exhaustion_is_published_as_a_charged_user_failure() {
    const ENDLESS_LOOP: &str = "new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) }";
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;
    let limit = 20_000;
    let offer = crate::helper::offered_deploy::owner_offer_limited(
        &payer,
        1,
        ENDLESS_LOOP.to_string(),
        0,
        "root".to_string(),
        limit,
    );
    let (block, id) = offered_block(&mut nodes, 0, offer).await;
    assert!(
        block.body.deploys[0].is_failed(),
        "the exhausted deploy is published as failed"
    );
    for node in nodes.iter_mut() {
        assert!(
            matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ),
            "every validator accepts the block"
        );
    }
    let receipts = futures::future::join_all(
        nodes
            .iter()
            .map(|node| BlockAPI::find_offered_settlement_receipt(&node.engine_cell, &id)),
    )
    .await
    .into_iter()
    .map(|receipt| receipt.unwrap().expect("every node holds the receipt"))
    .collect::<Vec<_>>();
    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));
    let receipt = &receipts[0];
    assert_eq!(receipt.phlo_limit, limit);
    assert!(receipt.phlo_used > 0 && receipt.phlo_used <= receipt.phlo_limit);
    assert_eq!(receipt.retained_phlo, 0);
    assert!(receipt.rev_spent > 0 && receipt.rev_spent <= receipt.rev_ceiling);
    let balance = vault_balance(&nodes, &block.body.state.post_state_hash, &payer_vault).await;
    assert_eq!(
        u128::from(balance) + receipt.rev_spent,
        u128::from(initial),
        "the payer pays the granted work and the fee"
    );
}

/// DR-114: every node accepts `block`, and every node holds the same
/// settlement receipt for `id`. Returns that receipt.
async fn accepted_everywhere_with_one_receipt(
    nodes: &mut [TestNode],
    block: &models::rust::casper::protocol::casper_message::BlockMessage,
    id: &[u8],
) -> casper::rust::api::block_api::OfferedSettlementReceipt {
    for node in nodes.iter_mut() {
        assert!(
            matches!(
                node.process_block(block.clone()).await.unwrap(),
                Either::Right(_)
            ),
            "every validator accepts the block"
        );
    }
    let id = id.to_vec();
    let receipts = futures::future::join_all(
        nodes
            .iter()
            .map(|node| BlockAPI::find_offered_settlement_receipt(&node.engine_cell, &id)),
    )
    .await
    .into_iter()
    .map(|receipt| receipt.unwrap().expect("every node holds the receipt"))
    .collect::<Vec<_>>();
    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));
    receipts
        .into_iter()
        .next()
        .expect("a node holds the receipt")
}

/// DR-114: an offered deploy asks an external service. The proposer records
/// the reply in the block, and every validator replays the block from the
/// record. A validator's own service answers differently, so a replay that
/// asked it would reject the block.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_deploy_records_an_external_reply_and_validators_replay_it() {
    use rholang::rust::interpreter::openai_service::{OpenAIMockConfig, OpenAIService};
    use rholang::rust::interpreter::rho_type::RhoString;
    const ASK: &str = r#"new gpt4(`rho:ai:gpt4`), ack in {
        gpt4!("hello", *ack) | for (@answer <- ack) { @"answer"!(answer) }
    }"#;
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    *nodes[0]
        .runtime_manager
        .external_services
        .openai
        .lock()
        .await = OpenAIService::Mock(OpenAIMockConfig::single_completion("recorded answer"));
    let payer = genesis.genesis_vaults[2].clone();
    let (block, id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&payer, 1, ASK.to_string()),
    )
    .await;
    let deploy = block.body.deploys[0]
        .as_offered()
        .expect("the block holds an offered deploy");
    assert!(!deploy.is_failed(), "the call succeeds");
    let records = deploy
        .deploy_log()
        .iter()
        .filter_map(|event| match event {
            models::rust::casper::protocol::casper_message::Event::Produce(produce)
                if !produce.output_value.is_empty() =>
            {
                Some(produce.output_value.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [record] = records.as_slice() else {
        panic!("the block records one reply, found {}", records.len());
    };
    let reply = record
        .iter()
        .map(|bytes| models::rhoapi::Par::decode(bytes.as_ref()).expect("a record holds pars"))
        .collect::<Vec<_>>();
    assert_eq!(reply, vec![RhoString::create_par(
        "recorded answer".to_owned()
    )]);
    let receipt = accepted_everywhere_with_one_receipt(&mut nodes, &block, &id).await;
    assert!(receipt.phlo_used > 0 && receipt.phlo_used <= receipt.phlo_limit);
    assert!(receipt.rev_spent > 0 && receipt.rev_spent <= receipt.rev_ceiling);
    let answer = models::rust::utils::new_gstring_par("answer".to_string(), Vec::new(), false);
    for node in nodes.iter().skip(1) {
        assert_eq!(
            node.runtime_manager
                .get_data(block.body.state.post_state_hash.clone(), &answer)
                .await
                .unwrap(),
            vec![RhoString::create_par("recorded answer".to_owned())],
            "the post-state of each validator holds the recorded reply"
        );
    }
}

/// DR-114 (§4.1 option A): a malformed call of a system process depends only
/// on the deploy's term. The block publishes it as a failed deploy, the payer
/// pays the granted work and the fee, and every validator accepts the block.
/// The datum that the deploy produced before the call is rolled back. Before
/// DR-114 the error was not a user failure, and the proposer rejected the
/// offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_malformed_system_call_is_published_as_a_charged_user_failure() {
    const MALFORMED: &str = r#"new hash(`rho:crypto:sha256Hash`), ack, done in {
        @"rolled back"!(1) | done!(0) | for (_ <- done) { hash!(42, *ack) }
    }"#;
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault = VaultAddress::from_public_key(&payer.1).unwrap();
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;
    let (block, id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&payer, 1, MALFORMED.to_string()),
    )
    .await;
    assert!(
        block.body.deploys[0].is_failed(),
        "the malformed call is published as failed"
    );
    let receipt = accepted_everywhere_with_one_receipt(&mut nodes, &block, &id).await;
    assert!(receipt.phlo_used > 0 && receipt.phlo_used <= receipt.phlo_limit);
    assert_eq!(receipt.retained_phlo, 0);
    assert!(receipt.rev_spent > 0 && receipt.rev_spent <= receipt.rev_ceiling);
    let balance = vault_balance(&nodes, &block.body.state.post_state_hash, &payer_vault).await;
    assert_eq!(
        u128::from(balance) + receipt.rev_spent,
        u128::from(initial),
        "the payer pays the granted work and the fee"
    );
    let rolled_back =
        models::rust::utils::new_gstring_par("rolled back".to_string(), Vec::new(), false);
    for node in nodes.iter() {
        assert!(
            node.runtime_manager
                .get_data(block.body.state.post_state_hash.clone(), &rolled_back)
                .await
                .unwrap()
                .is_empty(),
            "the failed deploy leaves no datum"
        );
    }
}

/// DR-120 (gap G9): a merge of two contender siblings above a neutral base.
/// Node 2 creates an empty sibling before it sees the contenders, every node
/// receives the three blocks, and node 2 merges them with the empty sibling
/// as the main parent. Both contenders are then in the merge scope, so the
/// v6 rule, not the base, decides between them.
async fn neutral_base_merge(
    nodes: &mut [TestNode],
    contenders: [&models::rust::casper::protocol::casper_message::BlockMessage; 2],
) -> models::rust::casper::protocol::casper_message::BlockMessage {
    let neutral = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates an empty sibling");
    assert!(neutral.body.deploys.is_empty());
    for block in [contenders[0], contenders[1], &neutral] {
        for node in nodes.iter_mut() {
            assert!(matches!(
                node.process_block(block.clone())
                    .await
                    .expect("the node processes the block"),
                Either::Right(_)
            ));
        }
    }
    let merge = crate::batch2::staging::mint_on_parents(
        &mut nodes[2],
        vec![
            neutral.clone(),
            contenders[0].clone(),
            contenders[1].clone(),
        ],
        "neutral-base merge",
    )
    .await;
    assert_eq!(
        merge.header.parents_hash_list.first(),
        Some(&neutral.block_hash)
    );
    for node in nodes.iter_mut().take(2) {
        assert!(
            matches!(
                node.process_block(merge.clone())
                    .await
                    .expect("the node processes the merge block"),
                Either::Right(_)
            ),
            "every node accepts the merge block"
        );
    }
    merge
}

/// The rejection records of `merge` for the deploy ids in `ids`.
fn records_for<'a>(
    merge: &'a models::rust::casper::protocol::casper_message::BlockMessage,
    ids: &[&[u8]],
) -> Vec<&'a models::rust::casper::protocol::casper_message::RejectedDeploy> {
    merge
        .body
        .rejected_deploys
        .iter()
        .filter(|record| ids.contains(&record.sig.as_ref()))
        .collect()
}

/// DR-120 (gap G9): two signers' version inserts conflict on the one store
/// datum of the versioned registry. Above a neutral base both are in the
/// merge scope, so the order K decides. The candidates tie on the pinned
/// flag, the prior losses and the height, so dev's branch order decides, and
/// it puts the chain that costs more first (deploy_chain_index.rs:179-190).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_signers_on_one_registry_key_keep_one_by_k() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the test network starts");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let padding: Vec<String> = (0..8).map(|i| format!("@\"g9-pad\"!({i})")).collect();
    let costly = format!("{} | {}", version_insert("costly"), padding.join(" | "));
    let (cheap_block, cheap_id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&genesis.genesis_vaults[2], 1, version_insert("cheap")),
    )
    .await;
    let (costly_block, costly_id) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(&genesis.genesis_vaults[0], 2, costly),
    )
    .await;
    let cheap_cost = cheap_block.body.deploys[0].cost().cost;
    let costly_cost = costly_block.body.deploys[0].cost().cost;
    assert!(
        costly_cost > cheap_cost,
        "staging precondition: the padded insert costs more ({costly_cost} > {cheap_cost})"
    );
    let merge = neutral_base_merge(&mut nodes, [&cheap_block, &costly_block]).await;
    let records = records_for(&merge, &[cheap_id.as_slice(), costly_id.as_slice()]);
    assert_eq!(records.len(), 1, "the merge keeps one contender");
    assert_eq!(
        records[0].sig.as_ref(),
        cheap_id.as_slice(),
        "K keeps the chain that costs more"
    );
    assert_eq!(records[0].carrier, cheap_block.block_hash);
    assert!(!records[0].duplicate);
}

/// DR-120 (gap G9), same-signer siblings: two offers of one signer conflict
/// on the signer's cursor cells. Above a neutral base the order K keeps one
/// and defers the other. The owner of the rejected carrier buffers the
/// offer; it re-proposes it once the rejection settles in the floor
/// (`retry_gate_spec::settled_rejection_opens_the_gate_and_the_owner_retries`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_signer_siblings_keep_one_and_the_owner_buffers_the_other() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the test network starts");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let signer = genesis.genesis_vaults[2].clone();
    let (first, first_id) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&signer, 1, "new x in { x!(0) }".to_string()),
    )
    .await;
    let (second, second_id) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(&signer, 2, "new y in { y!(1) }".to_string()),
    )
    .await;
    let merge = neutral_base_merge(&mut nodes, [&first, &second]).await;
    let records = records_for(&merge, &[first_id.as_slice(), second_id.as_slice()]);
    assert_eq!(records.len(), 1, "the merge keeps one sibling");
    let (owner, loser_id) = match records[0].sig.as_ref() == first_id.as_slice() {
        true => (0usize, first_id.clone()),
        false => (1usize, second_id.clone()),
    };
    assert_eq!(
        records[0].carrier,
        [&first, &second][owner].block_hash,
        "the record names the deferred sibling's carrier"
    );
    for (index, node) in nodes.iter().enumerate() {
        match index == owner {
            true => assert_eq!(buffered_envelope_ids(node), vec![loser_id.clone()]),
            false => assert!(buffered_envelope_ids(node).is_empty()),
        }
    }
}

/// DR-120 (gap G9): the v6 merge result does not depend on the order of the
/// secondary parents (DR-15 item 4). Two nodes merge one neutral base with
/// the same two contenders in opposite secondary orders. Each node uses its
/// own parents-post-state cache, and both compute one state and one
/// rejection set.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn permuted_secondary_parents_give_the_same_v6_root() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the test network starts");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let (left, _) = offered_block(
        &mut nodes,
        0,
        owner_direct_offer(&genesis.genesis_vaults[2], 1, version_insert("left")),
    )
    .await;
    let (right, _) = offered_block(
        &mut nodes,
        1,
        owner_direct_offer(&genesis.genesis_vaults[0], 2, version_insert("right")),
    )
    .await;
    let neutral = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates an empty sibling");
    for block in [&left, &right, &neutral] {
        for node in nodes.iter_mut() {
            assert!(matches!(
                node.process_block(block.clone())
                    .await
                    .expect("the node processes the block"),
                Either::Right(_)
            ));
        }
    }
    let mut results = Vec::with_capacity(2);
    for (index, parents) in [vec![neutral.clone(), left.clone(), right.clone()], vec![
        neutral.clone(),
        right.clone(),
        left.clone(),
    ]]
    .into_iter()
    .enumerate()
    {
        let snapshot = nodes[index]
            .casper
            .get_snapshot()
            .await
            .expect("the node takes a snapshot");
        let latest_messages: std::collections::BTreeMap<_, _> = snapshot
            .justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect();
        let runtime_manager = nodes[index].runtime_manager.clone();
        let merged = casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
            &nodes[index].block_store,
            parents,
            &snapshot,
            &runtime_manager,
            &latest_messages,
            None,
            None,
            None,
        )
        .await
        .expect("the merge computes");
        let mut rejected: Vec<Vec<u8>> = merged
            .rejected_user
            .iter()
            .map(|record| record.sig.to_vec())
            .collect();
        rejected.sort();
        results.push((merged.state.clone(), rejected));
    }
    assert_eq!(
        results[0], results[1],
        "both secondary orders give one state and one rejection set"
    );
    assert_eq!(results[0].1.len(), 1, "the contenders conflict");
}

/// DR-120 (gap G9): `node` merges `parents` with its own snapshot and the
/// latest messages of that snapshot.
async fn merge_on(
    node: &TestNode,
    parents: Vec<models::rust::casper::protocol::casper_message::BlockMessage>,
) -> casper::rust::util::rholang::runtime_manager::MergedPreState {
    let snapshot = node
        .casper
        .get_snapshot()
        .await
        .expect("the node takes a snapshot");
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot
        .justifications
        .iter()
        .map(|justification| {
            (
                justification.validator.clone(),
                justification.latest_block_hash.clone(),
            )
        })
        .collect();
    let runtime_manager = node.runtime_manager.clone();
    casper::rust::util::rholang::interpreter_util::compute_parents_post_state(
        &node.block_store,
        parents,
        &snapshot,
        &runtime_manager,
        &latest_messages,
        None,
        None,
        None,
    )
    .await
    .expect("the merge computes")
}

/// DR-120 (gap G9), C2: one offer in two sibling blocks above a neutral base.
/// Both copies are in the merge scope. The fast path declines on the
/// repeated deploy id (F5). The two copies are equal chains, so dev's chain
/// set keeps one of them (deploy_chain_index.rs:163-171), and the merge
/// applies the offer once. The merged state charges the payer exactly once,
/// and no record defers the offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_offer_in_two_siblings_is_charged_once() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the test network starts");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault =
        VaultAddress::from_public_key(&payer.1).expect("the payer key forms a vault address");
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;
    let offer = owner_direct_offer(&payer, 1, "new x in { x!(0) }".to_string());
    let (first, id) = offered_block(&mut nodes, 0, offer.clone()).await;
    let (second, second_id) = offered_block(&mut nodes, 1, offer).await;
    assert_eq!(id, second_id);
    let neutral = nodes[2]
        .create_block_unsafe(&[])
        .await
        .expect("node 2 creates an empty sibling");
    for block in [&first, &second, &neutral] {
        for node in nodes.iter_mut() {
            assert!(matches!(
                node.process_block(block.clone())
                    .await
                    .expect("the node processes the block"),
                Either::Right(_)
            ));
        }
    }
    let merged = merge_on(&nodes[2], vec![neutral, first, second]).await;
    assert!(
        merged
            .applied_from_scope
            .contains(&prost::bytes::Bytes::from(id.clone())),
        "the merge applies the offer from its scope"
    );
    assert!(
        merged
            .rejected_user
            .iter()
            .all(|record| record.sig.as_ref() != id.as_slice() || record.duplicate),
        "no record defers the offer"
    );
    let receipt = BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &id)
        .await
        .expect("the receipt query succeeds")
        .expect("the offer carries a settlement receipt");
    let balance = vault_balance(&nodes[2..], &merged.state, &payer_vault).await;
    assert_eq!(
        u128::from(balance) + receipt.rev_spent,
        u128::from(initial),
        "the merged state charges the payer exactly once"
    );
}

/// DR-120 (gap G9), law L7: one offer in two sibling blocks, and the merge's
/// main parent is one of them. The other copy is a scope copy of an offer
/// that the base settled. Its settlement consumes a fee-cursor datum that the
/// base no longer holds, so the fast path declines
/// (`CursorLinearity.scope_copy_of_base_deploy_fails_fast_path`). The fast
/// path would apply every scope chain. The slow path drops the copy, so the
/// merge does not apply the offer from its scope, and the merged state
/// charges the payer exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scope_copy_of_a_base_offer_takes_the_slow_path() {
    let genesis = offered_v6_genesis(3).await;
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("the test network starts");
    let payer = genesis.genesis_vaults[2].clone();
    let payer_vault =
        VaultAddress::from_public_key(&payer.1).expect("the payer key forms a vault address");
    let genesis_root = genesis.genesis_block.body.state.post_state_hash.clone();
    let initial = vault_balance(&nodes, &genesis_root, &payer_vault).await;
    let offer = owner_direct_offer(&payer, 1, "new x in { x!(0) }".to_string());
    let (first, id) = offered_block(&mut nodes, 0, offer.clone()).await;
    let (second, _) = offered_block(&mut nodes, 1, offer).await;
    for node in nodes.iter_mut() {
        for block in [&first, &second] {
            assert!(matches!(
                node.process_block(block.clone())
                    .await
                    .expect("the node processes the block"),
                Either::Right(_)
            ));
        }
    }
    let merged = merge_on(&nodes[2], vec![first, second]).await;
    assert!(
        !merged
            .applied_from_scope
            .contains(&prost::bytes::Bytes::from(id.clone())),
        "the merge does not apply the scope copy"
    );
    assert!(
        merged
            .rejected_user
            .iter()
            .all(|record| record.sig.as_ref() != id.as_slice() || record.duplicate),
        "no record defers the offer"
    );
    let receipt = BlockAPI::find_offered_settlement_receipt(&nodes[2].engine_cell, &id)
        .await
        .expect("the receipt query succeeds")
        .expect("the offer carries a settlement receipt");
    let balance = vault_balance(&nodes[2..], &merged.state, &payer_vault).await;
    assert_eq!(
        u128::from(balance) + receipt.rev_spent,
        u128::from(initial),
        "the merged state charges the payer exactly once"
    );
}
