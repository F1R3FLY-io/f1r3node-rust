use std::collections::BTreeMap;

use casper::rust::block_status::ValidBlock;
use casper::rust::blocks::ingest::dependency_tracker::{
    BodyAt, DependencyTracker, HeldMeta, Work, READY_AGE_LIMIT_DISPATCHES,
};
use casper::rust::blocks::ingest::ingest_view::IngestView;
use casper::rust::casper::MultiParentCasper;
use casper::rust::util::{construct_deploy, proto_util};
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;

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
    observe(receiver, blocks, verdicts).await
}

async fn observe(
    receiver: &TestNode,
    blocks: &[BlockMessage],
    verdicts: BTreeMap<Vec<u8>, String>,
) -> Outcome {
    let dag = receiver.casper.block_dag().await.unwrap();
    let in_dag = blocks
        .iter()
        .map(|b| dag.contains(&b.block_hash).unwrap())
        .collect();
    let latest = dag
        .latest_message_hashes()
        .into_iter()
        .map(|(validator, hash)| (validator.to_vec(), hash.to_vec()))
        .collect();
    (verdicts, in_dag, latest)
}

fn held_meta(block: &BlockMessage) -> HeldMeta {
    HeldMeta {
        hash: block.block_hash.clone(),
        sender: block.sender.clone(),
        height: proto_util::block_number(block),
        deps: proto_util::dependencies_hashes_of(block),
        parents: block.header.parents_hash_list.clone(),
        justification_validators: block
            .justifications
            .iter()
            .map(|justification| justification.validator.clone())
            .collect(),
        deferred: false,
        body: BodyAt::Disk,
        size: 0,
        trusted: true,
        bonded_at_anchor: true,
        solicited: false,
        pushed_by: None,
    }
}

async fn dispatch_released_blocks(
    genesis: &GenesisContext,
    parents: &[BlockMessage; 2],
    children: &[BlockMessage; 2],
    first: usize,
    wait_dispatches: u64,
) -> Outcome {
    let mut nodes = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .unwrap();
    let receiver = &mut nodes[3];
    receiver.shutoff().unwrap();

    assert!(first < 2, "first selects one of the two released blocks");
    let second = 1 - first;
    let mut tracker = DependencyTracker::new(IngestView::new());
    for (child, parent) in children.iter().zip(parents) {
        tracker.hold(held_meta(child), Work::Validate, vec![parent
            .block_hash
            .clone()]);
    }
    assert_eq!(
        tracker.next_ready(),
        None,
        "both blocks wait for their parents"
    );
    for parent in parents {
        receiver.process_block(parent.clone()).await.unwrap();
        assert!(receiver
            .block_dag_storage
            .contains(&parent.block_hash)
            .unwrap());
    }
    assert_eq!(tracker.inserted(&parents[first].block_hash).released, vec![
        children[first].block_hash.clone()
    ]);

    let mut background = held_meta(&parents[first]);
    background.hash = b"background-work".to_vec().into();
    for _ in 0..wait_dispatches {
        tracker.hold(background.clone(), Work::Validate, vec![]);
        assert_eq!(
            tracker.next_ready(),
            Some((background.hash.clone(), Work::Validate))
        );
        tracker.inserted(&background.hash);
    }
    assert_eq!(
        tracker.inserted(&parents[second].block_hash).released,
        vec![children[second].block_hash.clone()]
    );
    if wait_dispatches > 0 {
        assert!(background.height < proto_util::block_number(&children[first]));
        tracker.hold(background.clone(), Work::Validate, vec![]);
    }

    let mut next = tracker.next_ready();
    assert_eq!(
        next,
        Some((children[first].block_hash.clone(), Work::Validate)),
        "the first released block must take the next slot in this schedule"
    );
    let mut order = Vec::new();
    let mut verdicts = BTreeMap::new();
    while let Some((hash, work)) = next {
        assert_eq!(work, Work::Validate);
        if hash != background.hash {
            let block = children
                .iter()
                .find(|child| child.block_hash == hash)
                .unwrap();
            let verdict = receiver.process_block(block.clone()).await.unwrap();
            assert_eq!(
                verdict,
                Either::Right(ValidBlock::Valid),
                "the released block {hash:?} must validate"
            );
            verdicts.insert(hash.to_vec(), format!("{verdict:?}"));
            order.push(hash.clone());
        }
        tracker.inserted(&hash);
        next = tracker.next_ready();
    }
    assert_eq!(order, [
        children[first].block_hash.clone(),
        children[second].block_hash.clone()
    ]);
    observe(receiver, children, verdicts).await
}

#[tokio::test]
async fn released_blocks_keep_their_verdicts_and_dag_in_both_dispatch_orders() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let mut producers = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .unwrap();
    let shard = &genesis.genesis_block.shard_id;
    let mut parents = Vec::new();
    let mut children = Vec::new();
    for (index, producer) in producers.iter_mut().take(2).enumerate() {
        producer.shutoff().unwrap();
        let parent_deploy =
            construct_deploy::basic_deploy_data((2 * index) as i32, None, Some(shard.clone()))
                .unwrap();
        let child_deploy =
            construct_deploy::basic_deploy_data((2 * index + 1) as i32, None, Some(shard.clone()))
                .unwrap();
        let parent = producer
            .add_block_from_deploys(&[parent_deploy])
            .await
            .unwrap();
        let child = producer
            .add_block_from_deploys(&[child_deploy])
            .await
            .unwrap();
        assert_eq!(
            child.header.parents_hash_list.as_slice(),
            std::slice::from_ref(&parent.block_hash)
        );
        parents.push(parent);
        children.push(child);
    }
    let parents = parents.try_into().unwrap();
    let children = children.try_into().unwrap();
    let ordinary = dispatch_released_blocks(&genesis, &parents, &children, 0, 0).await;
    assert!(ordinary.1.iter().all(|present| *present));
    let aged =
        dispatch_released_blocks(&genesis, &parents, &children, 1, READY_AGE_LIMIT_DISPATCHES)
            .await;
    assert_eq!(aged, ordinary);
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
