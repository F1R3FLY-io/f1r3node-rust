// Integration test — Tier 1 production-path verification of the
// `InvalidSequenceNumber` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-31 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order (validate.rs::block_summary): block_hash → ... →
// parents → SEQUENCE_NUMBER. Mutating top-level `block.seq_num`
// (parent_max for v0 is 0; expected next is 1; we set it to 50)
// keeps earlier validators satisfied and trips
// `Validate::sequence_number` (validate.rs:310).

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_sequence_number() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    let mutated = propose_with_block_mutation(&mut nodes[0], vec![d1], |b| {
        // Mutate seq_num to a wrong value (genuine = 1). Setting to 5
        // keeps the dispatcher's record key (v0, seq_num - 1 = 4)
        // within the snapshot scan range 0..=10 below.
        b.seq_num = 5;
    })
    .await
    .expect("propose_with_block_mutation");

    let status = nodes[1]
        .process_block(mutated.clone())
        .await
        .expect("process_block");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidSequenceNumber))
        ),
        "expected InvalidSequenceNumber, got: {:?}",
        status
    );

    let snapshot = production_snapshot_at(
        &nodes[1],
        &genesis.genesis_block,
        &genesis.genesis_block,
        validators,
    )
    .await
    .expect("snapshot");

    let has_v0 = (0..=10).any(|b| <_ as SlashingObserver>::has_record(&snapshot, "v0", b));
    assert!(
        has_v0,
        "post-fix #3 catch-all: dispatcher mints record for v0 \
         on InvalidSequenceNumber"
    );
}
