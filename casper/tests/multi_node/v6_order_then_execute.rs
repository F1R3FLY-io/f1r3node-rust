use std::collections::HashMap;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::util::construct_deploy;
use casper::rust::v6::receipt::OfferedStatus;
use casper::rust::v6::record::ExecutionRecord;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::DeployData;

use crate::helper::test_node::TestNode;
use crate::helper::v6_network::{
    broadcast, cited_records, engine, propose, propose_and_broadcast, tick_until_idle,
    wait_for_finality, wake_until,
};
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

async fn genesis() -> GenesisContext {
    GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap()
}

struct Deploys {
    shard_id: String,
    now: i64,
    offset: i64,
}

impl Deploys {
    fn new(genesis: &GenesisContext) -> Self {
        Deploys {
            shard_id: genesis.genesis_block.shard_id.clone(),
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            offset: 0,
        }
    }

    fn deploy(&mut self, source: &str, key: usize) -> Signed<DeployData> {
        let keys = [
            construct_deploy::DEFAULT_SEC.clone(),
            construct_deploy::DEFAULT_SEC2.clone(),
        ];
        self.offset += 1;
        construct_deploy::source_deploy(
            source.to_string(),
            self.now + self.offset,
            Some(500_000),
            None,
            Some(keys[key].clone()),
            None,
            Some(self.shard_id.clone()),
        )
        .unwrap()
    }
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

fn assert_roots_agree(nodes: &[TestNode], blocks: &[BlockHash]) {
    let roots = executed_roots(nodes, blocks);
    for other in &roots[1..] {
        for (block, root) in other {
            if let Some(mine) = roots[0].get(block) {
                assert_eq!(mine, root, "validators disagree on an executed root");
            }
        }
    }
    for node in nodes {
        assert!(
            engine(node).evidence().is_empty(),
            "honest validators produced evidence"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validators_agree_on_racing_blocks_without_merging_or_replaying_on_the_consensus_path() {
    let genesis = genesis().await;
    let mut deploys = Deploys::new(&genesis);
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();

    let racing = vec![
        deploys.deploy(r#"@"race"!(1)"#, 0),
        deploys.deploy(r#"@"race"!(2)"#, 1),
        deploys.deploy(r#"for (@v <- @"race") { @"won"!(v) }"#, 0),
        deploys.deploy(r#"for (@v <- @"race") { @"second"!(v) }"#, 1),
    ];
    let mut blocks = Vec::new();
    let first = propose(&nodes[0], &racing).await;
    assert_eq!(first.body.deploys.len(), 4);
    assert!(first.body.deploys.iter().all(|d| d.deploy_log.is_empty()));
    assert!(first.body.rejected_deploys.is_empty());
    broadcast(&mut nodes, &first).await;
    blocks.push(first.block_hash.clone());

    for round in 1..24 {
        let proposer = round % nodes.len();
        let extra = if round % 4 == 0 {
            vec![deploys.deploy(&format!(r#"@"tick"!({})"#, round), round % 2)]
        } else {
            vec![]
        };
        let block = propose_and_broadcast(&mut nodes, proposer, &extra).await;
        blocks.push(block.block_hash.clone());
    }

    assert!(
        wait_canonical(&nodes, 4).await,
        "the executors did not fix canonical records"
    );
    assert!(
        engine(&nodes[0]).executed(&first.block_hash).is_some(),
        "the racing block has no canonical root"
    );
    assert_roots_agree(&nodes, &blocks);

    let cited = blocks.iter().any(|hash| {
        cited_records(&nodes[0], hash)
            .iter()
            .any(|record| record.target() == &first.block_hash)
    });
    assert!(cited, "no block carried a record of the racing block");
}

async fn wait_canonical(nodes: &[TestNode], target: usize) -> bool {
    wait_until(nodes, |node| engine(node).canonical_len() >= target).await
}

async fn wait_until<F: Fn(&TestNode) -> bool>(nodes: &[TestNode], done: F) -> bool {
    wake_until(nodes, 600, done).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn driven_engines_advance_only_when_the_test_ticks_them() {
    let genesis = genesis().await;
    let mut deploys = Deploys::new(&genesis);
    let mut nodes = TestNode::create_v6_network_driven(genesis, 3)
        .await
        .unwrap();

    let mut blocks = Vec::new();
    for round in 0..9 {
        let extra = vec![deploys.deploy(&format!(r#"@"driven"!({})"#, round), round % 2)];
        let block = propose_and_broadcast(&mut nodes, round % 3, &extra).await;
        blocks.push(block.block_hash.clone());
    }
    assert!(
        wait_for_finality(&nodes, 600).await,
        "the shard did not finalize a block"
    );
    for node in &nodes {
        assert_eq!(
            engine(node).canonical_len(),
            1,
            "a driven engine ran on its own"
        );
        assert!(
            tick_until_idle(node, 1_000).await > 0,
            "the first tick did no work"
        );
        assert_eq!(
            engine(node).canonical_len(),
            1,
            "nothing is fixed before a citation"
        );
    }

    let mut round = 0;
    while !nodes
        .iter()
        .all(|node| engine(node).executed(&blocks[0]).is_some())
    {
        assert!(round < 60, "the driven engines never fixed the first block");
        let block = propose_and_broadcast(&mut nodes, round % 3, &[]).await;
        blocks.push(block.block_hash.clone());
        for node in &nodes {
            tick_until_idle(node, 1_000).await;
        }
        round += 1;
    }
    assert_roots_agree(&nodes, &blocks);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_executed_offer_reports_its_receipt_on_every_validator() {
    let genesis = genesis().await;
    let mut deploys = Deploys::new(&genesis);
    let mut nodes = TestNode::create_v6_network(genesis, 3).await.unwrap();
    let offer = deploys.deploy(r#"new x in { x!(1) | for (_ <- x) { Nil } }"#, 0);
    let id = offer.sig.to_vec();

    let status = |node: &TestNode| {
        let engine_cell = node.engine_cell.clone();
        let id = id.clone();
        async move {
            BlockAPI::offered_status(&engine_cell, &id)
                .await
                .expect("an offered status")
        }
    };
    assert_eq!(status(&nodes[0]).await, OfferedStatus::Unknown);

    let block = propose_and_broadcast(&mut nodes, 0, &[offer]).await;
    assert!(matches!(
        status(&nodes[1]).await,
        OfferedStatus::Ordered { .. } | OfferedStatus::Executed(_)
    ));

    let retained = |node: &TestNode| engine(node).record(&block.block_hash).is_some();
    let mut round = 0;
    while !wake_until(&nodes, 5, retained).await {
        assert!(
            round < 120,
            "the offer's block was never fixed with a record"
        );
        round += 1;
        propose_and_broadcast(&mut nodes, round % 3, &[]).await;
    }

    for node in &nodes {
        let record = engine(node)
            .record(&block.block_hash)
            .expect("the record is retained");
        let entry = &record.entries()[0].executed;
        let OfferedStatus::Executed(receipt) = status(node).await else {
            panic!("expected a receipt");
        };
        assert_eq!(receipt.block_hash, block.block_hash);
        assert_eq!(receipt.outcome, entry.outcome);
        assert_eq!(receipt.comm_phlo, entry.comm_phlo);
        assert_eq!(receipt.compute_phlo, entry.compute_phlo);
        assert_eq!(receipt.event_count, entry.user_log.len());
        assert_eq!(receipt.comm_phlo, 1, "the offer fires one COMM");
        assert!(receipt.compute_phlo > 0);
    }
}

#[tokio::test]
async fn offers_are_refused_once_the_pool_is_full() {
    use casper::rust::casper::{Casper, DeployError, DEPLOY_POOL_BLOCKS_OF_BACKLOG_DEFAULT};
    use rspace_plus_plus::rspace::history::Either;

    let genesis = genesis().await;
    let mut deploys = Deploys::new(&genesis);
    let node = TestNode::create_v6_network_with_block_deploys(genesis, 1, 1)
        .await
        .unwrap()
        .remove(0);
    for index in 0..DEPLOY_POOL_BLOCKS_OF_BACKLOG_DEFAULT {
        let offer = deploys.deploy(&format!(r#"@"pool"!({})"#, index), index % 2);
        assert!(matches!(
            node.casper.deploy(offer).unwrap(),
            Either::Right(_)
        ));
    }
    let overflow = deploys.deploy(r#"@"pool"!(-1)"#, 0);
    match node.casper.deploy(overflow).unwrap() {
        Either::Left(DeployError::PoolFull { pending, cap }) => {
            assert_eq!(cap, DEPLOY_POOL_BLOCKS_OF_BACKLOG_DEFAULT);
            assert_eq!(pending, cap);
        }
        other => panic!("expected the pool to be full, got {:?}", other),
    }
}
