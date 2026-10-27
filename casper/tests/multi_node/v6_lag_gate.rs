use std::sync::Arc;

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::genesis_resource_policy::shard_resource_schedule;
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::v6::V6Engine;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const THRESHOLD: u64 = 6;

fn engine(node: &TestNode) -> Arc<V6Engine> { node.casper.v6.clone().expect("v6 engine") }

fn offer(shard_id: &str) -> DeployEnvelope {
    let time_stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let body = DeployData {
        term: r#"@"gated"!(1)"#.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: shard_id.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(
        &construct_deploy::DEFAULT_SEC,
        body,
        1_000_000,
        1,
        &shard_resource_schedule(shard_id, 8),
    )
    .unwrap();
    DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope).unwrap()
}

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

async fn broadcast(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        let status = node.process_block(block.clone()).await.unwrap();
        assert!(
            matches!(status, Either::Right(_)),
            "block rejected: {:?}",
            status
        );
    }
}

async fn execute(node: &TestNode) {
    for _ in 0..400 {
        if !engine(node).tick().await.unwrap() {
            return;
        }
    }
}

async fn lfb_number(node: &TestNode) -> i64 {
    let dag = node.block_dag_storage.get_representation().unwrap();
    dag.block_number(&dag.last_finalized_block()).unwrap()
}

async fn lag(node: &TestNode) -> i64 {
    let dag = node.block_dag_storage.get_representation().unwrap();
    let executed = dag
        .block_number(&engine(node).attestation().block_hash)
        .unwrap();
    dag.block_number(&dag.last_finalized_block()).unwrap() - executed
}

async fn round(nodes: &mut [TestNode], proposer: usize, executing: &[usize]) -> BlockMessage {
    for index in executing {
        execute(&nodes[*index]).await;
    }
    let block = propose(&nodes[proposer]).await;
    broadcast(nodes, &block).await;
    block
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lagging_validator_proposes_without_envelopes_until_it_catches_up() {
    crate::init_logger();
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.version = casper::rust::casper::PROTOCOL_V6_VERSION;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network_driven(genesis, 3, THRESHOLD)
        .await
        .unwrap();

    let mut turn = 0usize;
    while lag(&nodes[0]).await <= THRESHOLD as i64 {
        assert!(turn < 60, "node 0 never fell behind");
        round(&mut nodes, turn % 3, &[1, 2]).await;
        turn += 1;
    }
    while !turn.is_multiple_of(3) {
        round(&mut nodes, turn % 3, &[1, 2]).await;
        turn += 1;
    }

    let envelope = offer(&shard_id);
    assert!(matches!(
        nodes[0].casper.deploy_envelope(envelope.clone()).unwrap(),
        Either::Right(_)
    ));
    let finalized_before = lfb_number(&nodes[0]).await;
    for _ in 0..2 {
        let gated = round(&mut nodes, 0, &[1, 2]).await;
        assert!(engine(&nodes[0]).lag_gate_engaged());
        assert!(
            gated.body.deploys.is_empty(),
            "a gated proposal carried envelopes"
        );
        assert!(nodes[0].deploy_storage.lock().non_empty().unwrap());
        round(&mut nodes, 1, &[1, 2]).await;
        round(&mut nodes, 2, &[1, 2]).await;
    }
    assert!(
        lfb_number(&nodes[0]).await > finalized_before,
        "finality stalled while the gate was engaged"
    );

    let mut carried = None;
    for _ in 0..20 {
        let block = round(&mut nodes, 0, &[0, 1, 2]).await;
        if !block.body.deploys.is_empty() {
            carried = Some(block);
            break;
        }
        round(&mut nodes, 1, &[0, 1, 2]).await;
        round(&mut nodes, 2, &[0, 1, 2]).await;
    }
    let carried = carried.expect("node 0 never proposed the pending offer");
    assert!(!engine(&nodes[0]).lag_gate_engaged());
    assert_eq!(carried.body.deploys.len(), 1);
    assert_eq!(
        carried.body.deploys[0]
            .as_ordered()
            .expect("an ordered envelope")
            .identity(),
        envelope.identity()
    );
    for node in &nodes {
        let stored = node.block_store.get(&carried.block_hash).unwrap().unwrap();
        assert_eq!(stored.body.deploys, carried.body.deploys);
    }
}
