use casper::rust::test_utils::helper::test_node::TestNode;
use casper::rust::test_utils::util::genesis_builder::GenesisBuilder;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

#[tokio::test]
async fn standalone_test_node_proposes_and_validates_a_valid_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis should build");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut node = TestNode::standalone(genesis)
        .await
        .expect("standalone test node should start");
    let deploy = construct_deploy::basic_deploy_data(0, None, Some(shard_id))
        .expect("valid deploy should build");

    let block = node
        .create_block_unsafe(&[deploy])
        .await
        .expect("standalone test node should propose a block");
    let validation = node
        .process_block(block.clone())
        .await
        .expect("standalone test node should process a proposed block");

    assert!(matches!(validation, Either::Right(_)));
    assert!(node.contains(&block.block_hash));
}
