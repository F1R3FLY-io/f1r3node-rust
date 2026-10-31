use std::sync::Arc;
use std::time::Duration;

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::v6::block_record::BlockRecord;
use casper::rust::v6::body::V6Body;
use casper::rust::v6::V6Engine;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;

pub fn engine(node: &TestNode) -> Arc<V6Engine> {
    node.casper
        .v6
        .clone()
        .expect("a v6 test node has an engine")
}

pub async fn propose(node: &TestNode, deploys: &[Signed<DeployData>]) -> BlockMessage {
    for deploy in deploys {
        let admitted = node
            .casper
            .deploy(deploy.clone())
            .expect("deploy admission");
        assert!(
            matches!(admitted, Either::Right(_)),
            "deploy refused: {:?}",
            admitted
        );
    }
    let snapshot = node.casper.get_snapshot().await.expect("a snapshot");
    let validator = node.casper.get_validator().expect("a validator");
    match engine(node)
        .create_block(&snapshot, &validator, DeploySelection::StandardAllowEmpty)
        .await
        .expect("v6 block creation")
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("expected a block, got {:?}", other),
    }
}

pub async fn broadcast(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        let status = node
            .process_block(block.clone())
            .await
            .expect("block processing");
        assert!(
            matches!(status, Either::Right(_)),
            "block rejected: {:?}",
            status
        );
    }
}

pub async fn propose_and_broadcast(
    nodes: &mut [TestNode],
    proposer: usize,
    deploys: &[Signed<DeployData>],
) -> BlockMessage {
    let block = propose(&nodes[proposer], deploys).await;
    broadcast(nodes, &block).await;
    block
}

pub async fn wake_until<F>(nodes: &[TestNode], rounds: usize, done: F) -> bool
where F: Fn(&TestNode) -> bool {
    for _ in 0..rounds {
        if nodes.iter().all(&done) {
            return true;
        }
        for node in nodes {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    nodes.iter().all(done)
}

pub async fn tick_until_idle(node: &TestNode, max_steps: usize) -> usize {
    let engine = engine(node);
    for step in 0..max_steps {
        if !engine.tick().await.expect("an engine step") {
            return step;
        }
    }
    max_steps
}

pub fn cited_records(node: &TestNode, block: &BlockHash) -> Vec<BlockRecord> {
    let message = node
        .block_store
        .get(block)
        .expect("block store read")
        .expect("a stored block");
    V6Body::decode(&message.body.extra_bytes)
        .expect("a readable v6 body")
        .map(|body| {
            body.decode_records::<BlockRecord>()
                .expect("readable records")
        })
        .unwrap_or_default()
}

pub async fn wait_for_finality(nodes: &[TestNode], rounds: usize) -> bool {
    for _ in 0..rounds {
        let finalized = nodes.iter().all(|node| {
            node.block_dag_storage
                .get_representation()
                .map(|dag| dag.last_finalized_block() != node.genesis.block_hash)
                .unwrap_or(false)
        });
        if finalized {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}
