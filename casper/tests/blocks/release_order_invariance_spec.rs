use std::collections::BTreeMap;

use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

type Outcome = (
    BTreeMap<Vec<u8>, String>,
    Vec<bool>,
    BTreeMap<Vec<u8>, Vec<u8>>,
);

async fn process_in_order(genesis: &GenesisContext, blocks: &[BlockMessage]) -> Outcome {
    let mut nodes = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .unwrap();
    let receiver = &mut nodes[3];
    receiver.shutoff().unwrap();

    let mut verdicts = BTreeMap::new();
    for block in blocks {
        let status = receiver.process_block(block.clone()).await.unwrap();
        verdicts.insert(block.block_hash.to_vec(), format!("{status:?}"));
    }

    let dag = receiver.casper.block_dag().await.unwrap();
    let in_dag = blocks.iter().map(|b| dag.contains(&b.block_hash)).collect();
    let latest = dag
        .latest_message_hashes()
        .into_iter()
        .map(|(validator, hash)| (validator.to_vec(), hash.to_vec()))
        .collect();
    (verdicts, in_dag, latest)
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut all = Vec::new();
    for rest in permutations(n - 1) {
        for at in 0..=rest.len() {
            let mut order = rest.clone();
            order.insert(at, n - 1);
            all.push(order);
        }
    }
    all
}

#[tokio::test]
async fn ready_blocks_reach_the_same_dag_and_verdicts_in_every_processing_order() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut producers = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    let mut ready = Vec::new();
    for (index, producer) in producers.iter_mut().take(3).enumerate() {
        producer.shutoff().unwrap();
        let deploy =
            construct_deploy::basic_deploy_data(index as i32, None, Some(shard.clone())).unwrap();
        ready.push(producer.add_block_from_deploys(&[deploy]).await.unwrap());
    }
    let genesis_hash = genesis.genesis_block.block_hash.clone();
    for block in &ready {
        assert_eq!(
            block.header.parents_hash_list,
            vec![genesis_hash.clone()],
            "each block depends only on genesis, so all of them are ready at once"
        );
    }

    let fifo = process_in_order(&genesis, &ready).await;
    assert!(
        fifo.1.iter().all(|held| *held),
        "first-in-first-out order puts every ready block in the DAG"
    );

    for order in permutations(ready.len()) {
        let blocks: Vec<BlockMessage> = order.iter().map(|i| ready[*i].clone()).collect();
        let outcome = process_in_order(&genesis, &blocks).await;
        let mut in_dag_by_original = vec![false; ready.len()];
        for (position, original) in order.iter().enumerate() {
            in_dag_by_original[*original] = outcome.1[position];
        }
        assert_eq!(
            (&outcome.0, &in_dag_by_original, &outcome.2),
            (&fifo.0, &fifo.1, &fifo.2),
            "processing order {order:?} changed a verdict, the DAG, or the latest messages"
        );
    }
}
