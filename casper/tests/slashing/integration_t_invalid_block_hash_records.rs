use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn invalid_block_hash_is_unattributable_and_cannot_frame_the_signer() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let deploy_data =
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("deploy_data");
    assert!(matches!(
        nodes[0]
            .submit_deploy(deploy_data)
            .expect("deploy should succeed"),
        Either::Right(_)
    ));
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("create_block_unsafe");

    let invalid_block = {
        let mut invalid = signed_block.clone();
        invalid.seq_num = 47;
        invalid
    };

    let status = nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("process_block");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
        ),
        "mutated signed frame must be classified InvalidBlockHash, got: {:?}",
        status
    );

    assert!(!nodes[1].contains(&invalid_block.block_hash));
    let records = nodes[1]
        .block_dag_storage
        .access_equivocations_tracker(|tracker| tracker.data())
        .expect("equivocations tracker");
    assert!(
        records
            .iter()
            .all(|record| record.equivocator != signed_block.sender),
        "a relay-mutated body must not create slash evidence against the authenticated signer"
    );

    let original_status = nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("process original block");
    assert!(matches!(original_status, Either::Right(_)));
    assert!(nodes[1].contains(&signed_block.block_hash));
}
