// A block whose sequence number was changed after signing is judged
// `InvalidBlockHash` and is dropped: no node holds it as an equivocation.

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{canonical_validator_order, production_snapshot_at};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_block_hash_records() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Round 1: v0 produces a well-formed block.
    let deploy_data =
        construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("deploy_data");
    nodes[0]
        .casper
        .deploy(deploy_data)
        .expect("deploy should succeed");
    let signed_block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .expect("create_block_unsafe");

    // Mutate seq_num without resigning. The block_hash validation
    // fires before sequence_number, so the classifier returns
    // InvalidBlockHash.
    let invalid_block = {
        let mut invalid = signed_block.clone();
        invalid.seq_num = 47;
        invalid
    };

    // Process through node 1 — must classify InvalidBlockHash.
    let status = nodes[1]
        .process_block(invalid_block.clone())
        .await
        .expect("process_block");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
        ),
        "catch-all arm: classifier returns InvalidBlockHash, got: {:?}",
        status
    );

    let snapshot = production_snapshot_at(
        &nodes[1],
        &signed_block, // post-state of the *valid* block (genesis successor)
        &genesis.genesis_block,
        validators,
    )
    .await
    .expect("snapshot");

    let v0_label = "v0";
    let has_any_record =
        (0..=50).any(|base| <_ as SlashingObserver>::has_record(&snapshot, v0_label, base));
    assert!(
        !has_any_record,
        "demoted: InvalidBlockHash is dropped without minting slash evidence"
    );
}
