use std::sync::Arc;
use std::time::Duration;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, DeployError, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::genesis_resource_policy::shard_resource_schedule;
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::v6::block_record::BlockRecord;
use casper::rust::v6::receipt::OfferedStatus;
use casper::rust::v6::validity::{check_structure, Limits};
use casper::rust::v6::V6Engine;
use crypto::rust::private_key::PrivateKey;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

const LIMIT: u64 = 1_000_000;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn v6_genesis() -> GenesisContext {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.version = casper::rust::casper::PROTOCOL_V6_VERSION;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap()
}

fn body(shard_id: &str, time_stamp: i64, expiration_timestamp: Option<i64>) -> DeployData {
    DeployData {
        term: "@\"offered\"!(1)".to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: shard_id.to_string(),
        expiration_timestamp,
        authority_presentations: Vec::new(),
    }
}

fn offer(key: &PrivateKey, body: DeployData, price: u64) -> DeployDataProto {
    let shard = body.shard_id.clone();
    let schedule = shard_resource_schedule(&shard, 8);
    owner_direct_offer(key, body, LIMIT, price, &schedule).expect("an owner-direct offer")
}

fn identity(proto: &DeployDataProto) -> Vec<u8> {
    DeployEnvelope::from_proto(proto.clone(), offered_funded_v6_limits().envelope)
        .expect("an offered envelope")
        .identity()
        .as_bytes()
        .to_vec()
}

async fn submit(node: &TestNode, proto: DeployDataProto, shard_id: &str) -> eyre::Result<String> {
    BlockAPI::deploy_offered(&node.engine_cell, proto, &None, false, shard_id).await
}

fn engine(node: &TestNode) -> Arc<V6Engine> { node.casper.v6.clone().expect("v6 engine") }

async fn propose(node: &TestNode) -> BlockMessage {
    let snapshot = node.casper.get_snapshot().await.unwrap();
    let validator = node.casper.get_validator().unwrap();
    match engine(node)
        .create_block(&snapshot, &validator, DeploySelection::StandardAllowEmpty)
        .await
        .unwrap()
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("expected a block, got {:?}", other),
    }
}

fn message(error: eyre::Report) -> String { format!("{:#}", error) }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_offered_deploy_is_admitted_and_lands_as_ordered_on_every_validator() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();
    assert!(nodes[0].casper.adopted_resource_policy().is_some());

    let proto = offer(
        &construct_deploy::DEFAULT_SEC,
        body(&shard_id, now(), None),
        1,
    );
    let id = identity(&proto);
    submit(&nodes[0], proto, &shard_id)
        .await
        .expect("the offer is admitted");

    let block = propose(&nodes[0]).await;
    let ordered: Vec<_> = block
        .body
        .deploys
        .iter()
        .filter_map(ProcessedUserDeploy::as_ordered)
        .collect();
    assert_eq!(ordered.len(), 1);
    assert_eq!(ordered[0].identity().as_bytes(), id.as_slice());
    let snapshot = nodes[0].casper.get_snapshot().await.unwrap();
    let block_number_of = |hash: &_| snapshot.dag.block_number(hash);
    assert!(check_structure::<BlockRecord>(&block, &Limits::default(), &block_number_of).is_ok());
    let ordered_only = Limits {
        ordered_only: true,
        ..Limits::default()
    };
    assert!(check_structure::<BlockRecord>(&block, &ordered_only, &block_number_of).is_ok());

    for node in nodes.iter_mut() {
        let status = node.process_block(block.clone()).await.unwrap();
        assert!(
            matches!(status, Either::Right(_)),
            "block rejected: {:?}",
            status
        );
    }
    let mut landed = false;
    for _ in 0..600 {
        if nodes.iter().all(|node| engine(node).canonical_len() >= 1) {
            landed = true;
            break;
        }
        for node in &nodes {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        landed,
        "the block with the ordered offer did not become canonical"
    );
    for node in &nodes {
        let stored = node.block_store.get(&block.block_hash).unwrap().unwrap();
        assert_eq!(stored.body.deploys, block.body.deploys);
    }
    let mut roots: Vec<_> = Vec::new();
    for round in 0..240 {
        roots = nodes
            .iter()
            .map(|node| engine(node).executed(&block.block_hash))
            .collect();
        if roots.iter().all(Option::is_some) {
            break;
        }
        for node in &nodes {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let proposer = (round + 1) % nodes.len();
        let heartbeat = propose(&nodes[proposer]).await;
        assert!(heartbeat.body.deploys.is_empty());
        for node in nodes.iter_mut() {
            let status = node.process_block(heartbeat.clone()).await.unwrap();
            assert!(
                matches!(status, Either::Right(_)),
                "heartbeat rejected: {:?}",
                status
            );
        }
    }
    assert!(
        roots[0].is_some(),
        "the block with the offer was not executed"
    );
    assert!(roots.iter().all(|root| *root == roots[0]));
    assert!(nodes[0]
        .block_store
        .deploy_sigs(&block.block_hash)
        .unwrap()
        .unwrap()
        .contains(&id));
    let again = propose(&nodes[0]).await;
    assert!(
        again.body.deploys.is_empty(),
        "an included offer was proposed again"
    );
}

#[tokio::test]
async fn a_duplicate_offer_is_refused() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::create_v6_network(genesis, 1)
        .await
        .unwrap()
        .remove(0);
    let proto = offer(
        &construct_deploy::DEFAULT_SEC,
        body(&shard_id, now(), None),
        1,
    );
    let id = identity(&proto);
    submit(&node, proto.clone(), &shard_id)
        .await
        .expect("the first offer is admitted");
    let error = submit(&node, proto, &shard_id).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<DeployError>(),
        Some(DeployError::DuplicateDeploy(duplicate)) if duplicate == &id
    ));
}

#[tokio::test]
async fn offers_outside_the_submission_rules_are_refused() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::create_v6_network(genesis, 1)
        .await
        .unwrap()
        .remove(0);
    let key = &construct_deploy::DEFAULT_SEC;

    let wrong_shard = offer(key, body("another-shard", now(), None), 1);
    assert!(
        message(submit(&node, wrong_shard, &shard_id).await.unwrap_err())
            .contains("is not as expected network shard")
    );

    let expired = offer(key, body(&shard_id, now() - 10_000, Some(now() - 5_000)), 1);
    assert!(message(submit(&node, expired, &shard_id).await.unwrap_err()).contains("has expired"));

    let mut non_canonical = offer(key, body(&shard_id, now(), None), 1);
    assert!(non_canonical.deployer.is_empty());
    non_canonical.deployer = vec![4; 65].into();
    assert!(submit(&node, non_canonical, &shard_id).await.is_err());

    let legacy = models::rust::casper::protocol::casper_message::DeployData::to_proto(
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).unwrap(),
    );
    assert!(message(submit(&node, legacy, &shard_id).await.unwrap_err())
        .contains("requires the offered-funded authorization format"));

    let read_only = BlockAPI::deploy_offered(
        &node.engine_cell,
        offer(key, body(&shard_id, now(), None), 1),
        &None,
        true,
        &shard_id,
    )
    .await
    .unwrap_err();
    assert!(message(read_only).contains("read-only"));
    assert!(!node.deploy_storage.lock().non_empty().unwrap());
}

#[tokio::test]
async fn an_offer_priced_below_the_shard_minimum_is_refused() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::create_v6_network(genesis, 1)
        .await
        .unwrap()
        .remove(0);
    let error = submit(
        &node,
        offer(
            &construct_deploy::DEFAULT_SEC,
            body(&shard_id, now(), None),
            0,
        ),
        &shard_id,
    )
    .await
    .unwrap_err();
    assert!(!node.deploy_storage.lock().non_empty().unwrap());
    let text = message(error);
    assert!(
        text.contains("less than minimum price") || text.contains("schedule"),
        "{}",
        text
    );
}

#[tokio::test]
async fn a_legacy_shard_refuses_offered_deploys() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::standalone(genesis).await.unwrap();
    assert!(node.casper.adopted_resource_policy().is_none());
    let error = submit(
        &node,
        offer(
            &construct_deploy::DEFAULT_SEC,
            body(&shard_id, now(), None),
            1,
        ),
        &shard_id,
    )
    .await
    .unwrap_err();
    assert!(message(error).contains("no genesis resource policy"));
    let envelope = DeployEnvelope::from_proto(
        offer(
            &construct_deploy::DEFAULT_SEC,
            body(&shard_id, now(), None),
            1,
        ),
        offered_funded_v6_limits().envelope,
    )
    .unwrap();
    assert!(node.casper.deploy_envelope(envelope).is_err());
}

#[tokio::test]
async fn a_protocol_6_shard_refuses_legacy_deploys() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::create_v6_network(genesis, 1)
        .await
        .unwrap()
        .remove(0);
    let legacy = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).unwrap();
    assert!(node.casper.deploy(legacy.clone()).is_err());
    let error = BlockAPI::deploy(&node.engine_cell, legacy, &None, false, &shard_id)
        .await
        .unwrap_err();
    assert!(message(error).contains("accepts only offered-funded deploys"));
    assert!(!node.deploy_storage.lock().non_empty().unwrap());
}

async fn status(node: &TestNode, id: &[u8]) -> OfferedStatus {
    BlockAPI::offered_status(&node.engine_cell, id)
        .await
        .expect("an offered status")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_offer_reports_pending_then_ordered_then_its_receipt() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();
    let proto = offer(
        &construct_deploy::DEFAULT_SEC,
        body(&shard_id, now(), None),
        1,
    );
    let id = identity(&proto);
    assert_eq!(status(&nodes[0], &id).await, OfferedStatus::Unknown);
    submit(&nodes[0], proto, &shard_id).await.unwrap();
    assert_eq!(status(&nodes[0], &id).await, OfferedStatus::Pending);

    let block = propose(&nodes[0]).await;
    for node in nodes.iter_mut() {
        let processed = node.process_block(block.clone()).await.unwrap();
        assert!(matches!(processed, Either::Right(_)), "{:?}", processed);
    }
    match status(&nodes[1], &id).await {
        OfferedStatus::Ordered { block_hash } => assert_eq!(block_hash, block.block_hash),
        OfferedStatus::Executed(receipt) => assert_eq!(receipt.block_hash, block.block_hash),
        other => panic!("expected the offer to be ordered, got {:?}", other),
    }

    for round in 0..240 {
        if nodes
            .iter()
            .all(|node| engine(node).record(&block.block_hash).is_some())
        {
            break;
        }
        for node in &nodes {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let heartbeat = propose(&nodes[(round + 1) % nodes.len()]).await;
        for node in nodes.iter_mut() {
            node.process_block(heartbeat.clone()).await.unwrap();
        }
    }
    for node in &nodes {
        let record = engine(node)
            .record(&block.block_hash)
            .expect("the executed record is retained");
        let entry = &record.entries()[0];
        let OfferedStatus::Executed(receipt) = status(node, &id).await else {
            panic!("expected a receipt");
        };
        assert_eq!(receipt.block_hash, block.block_hash);
        assert_eq!(receipt.outcome, entry.executed.outcome);
        assert_eq!(receipt.phlo_used, entry.executed.phlo_used);
        assert_eq!(receipt.debits, entry.settled.debits);
        assert_eq!(receipt.rev_spent, entry.settled.rev_spent);
        assert!(receipt.verdict.is_admitted());
        assert!(!receipt.debits.is_empty());
        let charges = receipt.charges.expect("decoded cost evidence");
        assert_eq!(charges.phlo_limit, LIMIT);
        assert_eq!(charges.phlo_price, 1);
    }
}

#[tokio::test]
async fn an_unfunded_offer_is_removed_at_proposal_and_reports_why() {
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let node = TestNode::create_v6_network(genesis, 1)
        .await
        .unwrap()
        .remove(0);
    let (unfunded, _) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
    let proto = offer(&unfunded, body(&shard_id, now(), None), 1);
    let id = identity(&proto);
    submit(&node, proto.clone(), &shard_id).await.unwrap();
    assert_eq!(status(&node, &id).await, OfferedStatus::Pending);

    let block = propose(&node).await;
    assert!(block.body.deploys.is_empty());
    assert!(!node.deploy_storage.lock().non_empty().unwrap());
    let OfferedStatus::Rejected(rejection) = status(&node, &id).await else {
        panic!("expected the offer to be rejected");
    };
    assert_eq!(rejection.deploy_id.as_ref(), id.as_slice());
    assert_eq!(rejection.block_number, block.body.state.block_number);
    assert_eq!(
        rejection.pre_state_root.as_deref(),
        Some(block.body.state.pre_state_hash.as_ref())
    );
    assert!(!rejection.reason.is_empty());

    submit(&node, proto, &shard_id).await.unwrap();
    assert_eq!(status(&node, &id).await, OfferedStatus::Pending);
}
