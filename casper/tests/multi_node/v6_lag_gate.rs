use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::helper::v6_network::{v6_genesis, engine, propose_and_broadcast, tick_until_idle};

const THRESHOLD: u64 = 6;

fn lfb_number(node: &TestNode) -> i64 {
    let dag = node.block_dag_storage.get_representation().unwrap();
    dag.block_number(&dag.last_finalized_block()).unwrap()
}

fn lag(node: &TestNode) -> i64 {
    let dag = node.block_dag_storage.get_representation().unwrap();
    let executed = dag
        .block_number(&engine(node).attestation().block_hash)
        .unwrap();
    dag.block_number(&dag.last_finalized_block()).unwrap() - executed
}

async fn round(nodes: &mut [TestNode], proposer: usize, executing: &[usize]) -> BlockMessage {
    for index in executing {
        tick_until_idle(&nodes[*index], 400).await;
    }
    propose_and_broadcast(nodes, proposer, &[]).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lagging_validator_proposes_without_envelopes_until_it_catches_up() {
    let genesis = v6_genesis().await;
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network_driven(genesis, 3, Some(THRESHOLD))
        .await
        .unwrap();

    let mut turn = 0usize;
    while lag(&nodes[0]) <= THRESHOLD as i64 {
        assert!(turn < 60, "node 0 never fell behind");
        round(&mut nodes, turn % 3, &[1, 2]).await;
        turn += 1;
    }
    while !turn.is_multiple_of(3) {
        round(&mut nodes, turn % 3, &[1, 2]).await;
        turn += 1;
    }

    let offer = construct_deploy::source_deploy(
        r#"@"gated"!(1)"#.to_string(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        Some(500_000),
        None,
        None,
        None,
        Some(shard_id),
    )
    .unwrap();
    assert!(matches!(
        nodes[0].casper.deploy(offer.clone()).unwrap(),
        Either::Right(_)
    ));
    let finalized_before = lfb_number(&nodes[0]);
    for _ in 0..2 {
        let gated = round(&mut nodes, 0, &[1, 2]).await;
        assert!(engine(&nodes[0]).lag_gate_engaged());
        assert!(
            gated.body.deploys.is_empty(),
            "a gated proposal carried envelopes"
        );
        assert!(nodes[0].deploy_storage.contains_sig(&offer.sig).unwrap());
        round(&mut nodes, 1, &[1, 2]).await;
        round(&mut nodes, 2, &[1, 2]).await;
    }
    assert!(
        lfb_number(&nodes[0]) > finalized_before,
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
    assert_eq!(carried.body.deploys[0].deploy.sig, offer.sig);
    for node in &nodes {
        let stored = node.block_store.get(&carried.block_hash).unwrap().unwrap();
        assert_eq!(stored.body.deploys, carried.body.deploys);
    }
}
