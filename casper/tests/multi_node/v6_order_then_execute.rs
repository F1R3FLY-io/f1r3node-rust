use std::collections::HashMap;
use std::time::Duration;

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::v6::body::V6Body;
use casper::rust::v6::V6Engine;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use std::sync::Arc;

use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn engine(node: &TestNode) -> Arc<V6Engine> {
    node.casper.v6.clone().expect("v6 engine")
}

async fn propose(node: &mut TestNode, deploys: &[Signed<DeployData>]) -> BlockMessage {
    for deploy in deploys {
        node.casper.deploy(deploy.clone()).unwrap();
    }
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

async fn wait_for(nodes: &[TestNode], target: usize) -> bool {
    for _ in 0..600 {
        if nodes.iter().all(|n| engine(n).canonical_len() >= target) {
            return true;
        }
        for node in nodes {
            engine(node).wake();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn executed_roots(
    nodes: &[TestNode],
    blocks: &[BlockHash],
) -> Vec<HashMap<BlockHash, prost::bytes::Bytes>> {
    nodes
        .iter()
        .map(|n| {
            blocks
                .iter()
                .filter_map(|b| engine(n).executed(b).map(|r| (b.clone(), r)))
                .collect()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_agree_on_racing_blocks_without_merging_or_replaying_on_the_consensus_path() {
    crate::init_logger();
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_v6_network(genesis.clone(), 3)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let keys = [
        construct_deploy::DEFAULT_SEC.clone(),
        construct_deploy::DEFAULT_SEC2.clone(),
    ];
    let mut offset = 0i64;
    let mut deploy = |source: &str, key: usize| {
        offset += 1;
        construct_deploy::source_deploy(
            source.to_string(),
            now + offset,
            Some(500_000),
            None,
            Some(keys[key].clone()),
            None,
            Some(shard_id.clone()),
        )
        .unwrap()
    };

    let racing = vec![
        deploy(r#"@"race"!(1)"#, 0),
        deploy(r#"@"race"!(2)"#, 1),
        deploy(r#"for (@v <- @"race") { @"won"!(v) }"#, 0),
        deploy(r#"for (@v <- @"race") { @"second"!(v) }"#, 1),
    ];
    let mut blocks = Vec::new();
    let first = propose(&mut nodes[0], &racing).await;
    assert_eq!(first.body.deploys.len(), 4);
    assert!(first.body.deploys.iter().all(|d| d.deploy_log.is_empty()));
    assert!(first.body.rejected_deploys.is_empty());
    broadcast(&mut nodes, &first).await;
    blocks.push(first.block_hash.clone());

    for round in 1..24 {
        let proposer = round % nodes.len();
        let extra = if round % 4 == 0 {
            vec![deploy(&format!(r#"@"tick"!({})"#, round), round % 2)]
        } else {
            vec![]
        };
        let block = propose(&mut nodes[proposer], &extra).await;
        broadcast(&mut nodes, &block).await;
        blocks.push(block.block_hash.clone());
    }

    assert!(
        wait_for(&nodes, 4).await,
        "the executors did not fix canonical records"
    );

    let roots = executed_roots(&nodes, &blocks);
    assert!(
        roots[0].contains_key(&first.block_hash),
        "the racing block has no canonical root"
    );
    for other in &roots[1..] {
        for (block, root) in other {
            if let Some(mine) = roots[0].get(block) {
                assert_eq!(mine, root, "validators disagree on an executed root");
            }
        }
    }
    for node in &nodes {
        assert!(
            engine(node).evidence().is_empty(),
            "honest validators produced evidence"
        );
    }

    let cited = blocks.iter().any(|hash| {
        let block = nodes[0].block_store.get(hash).unwrap().unwrap();
        V6Body::decode(&block.body.extra_bytes)
            .unwrap()
            .map(|b| b.records.iter().any(|r| r.target == first.block_hash))
            .unwrap_or(false)
    });
    assert!(cited, "no block carried a record of the racing block");
}
