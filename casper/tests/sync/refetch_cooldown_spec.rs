//! A recovery re-request for a buffered block gets a duplicate copy as its
//! reply. Disposing of that copy must not reset the re-fetch rate limit of
//! the block, or every later arrival re-requests it at once and the retry
//! budget never runs out.

use casper::rust::blocks::block_processor::{IntegrityChecked, OfInterestVerdict};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn a_duplicate_copy_of_a_buffered_block_keeps_its_refetch_cooldown() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    let deploy_a = construct_deploy::basic_deploy_data(0, None, Some(shard.clone())).unwrap();
    nodes[0].add_block_from_deploys(&[deploy_a]).await.unwrap();
    nodes[1].shutoff().unwrap();

    let deploy_b = construct_deploy::basic_deploy_data(1, None, Some(shard.clone())).unwrap();
    let block_b = nodes[0].add_block_from_deploys(&[deploy_b]).await.unwrap();

    let first = nodes[1].add_block(block_b.clone()).await.unwrap();
    assert!(
        matches!(first, Either::Left(_)),
        "B without A must not validate, got {first:?}"
    );
    assert!(nodes[1].casper.buffer_contains(&block_b.block_hash));

    let peer0 = nodes[0].local.clone();
    let sent_to_peer0 = |nodes: &[TestNode]| {
        nodes[1]
            .tle
            .test_network()
            .peer_queue(&peer0)
            .unwrap()
            .len()
    };

    nodes[1]
        .block_retriever
        .recover_dependency(block_b.block_hash.clone())
        .await
        .unwrap();
    let after_recovery = sent_to_peer0(&nodes);

    let verdict = nodes[1]
        .block_processor
        .check_if_of_interest(
            nodes[1].casper.clone(),
            IntegrityChecked::check(&block_b).unwrap(),
        )
        .unwrap();
    assert_eq!(verdict, OfInterestVerdict::AlreadyProcessed);
    nodes[1]
        .block_processor
        .dispose_not_of_interest(verdict, IntegrityChecked::check(&block_b).unwrap())
        .await
        .unwrap();

    nodes[1]
        .block_retriever
        .recover_dependency(block_b.block_hash.clone())
        .await
        .unwrap();

    assert_eq!(
        sent_to_peer0(&nodes),
        after_recovery,
        "a re-request inside the cooldown must send nothing after a duplicate copy"
    );
    assert!(
        nodes[1]
            .requested_blocks
            .lock()
            .unwrap()
            .contains_key(&block_b.block_hash),
        "the retriever must keep tracking a block that still waits in the buffer"
    );
}
