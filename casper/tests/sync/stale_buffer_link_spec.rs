use casper::rust::blocks::block_processor::OfInterestVerdict;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::block_hash::BlockHashSerde;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn a_released_block_with_a_stale_parent_link_is_processed() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let shard = genesis.genesis_block.shard_id.clone();

    let deploy_a = construct_deploy::basic_deploy_data(0, None, Some(shard.clone())).unwrap();
    let block_a = nodes[0].add_block_from_deploys(&[deploy_a]).await.unwrap();
    nodes[1].shutoff().unwrap();
    let a_result = nodes[1].add_block(block_a.clone()).await.unwrap();
    assert!(
        matches!(a_result, Either::Right(_)),
        "A must validate, got {a_result:?}"
    );

    let deploy_b = construct_deploy::basic_deploy_data(1, None, Some(shard)).unwrap();
    let block_b = nodes[0].add_block_from_deploys(&[deploy_b]).await.unwrap();

    nodes[1]
        .block_store
        .put(block_b.block_hash.clone(), &block_b)
        .unwrap();
    nodes[1]
        .casper
        .casper_buffer_storage
        .add_relation(
            BlockHashSerde(block_a.block_hash.clone()),
            BlockHashSerde(block_b.block_hash.clone()),
        )
        .unwrap();

    let released = nodes[1].casper.get_dependency_free_from_buffer().unwrap();
    assert!(
        released.iter().any(|b| b.block_hash == block_b.block_hash),
        "B depends only on the validated A, so the release scan offers it"
    );

    let verdict = nodes[1]
        .block_processor
        .check_if_of_interest(nodes[1].casper.clone(), &block_b)
        .unwrap();
    assert_eq!(
        verdict,
        OfInterestVerdict::Fresh,
        "a released block whose only buffered parent is validated must be processed, not dropped as already processed"
    );
}
