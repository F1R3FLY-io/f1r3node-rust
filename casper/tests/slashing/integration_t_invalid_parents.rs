// Integration test — Tier 1 production-path verification of the
// `InvalidParents` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-28 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order: block_summary's `justification_follows`
// (validate.rs:820) runs after `block_number` and returns
// `InvalidParents` from line 833 when `parent_hashes.first() ==
// None`. To keep `block_number` happy on the empty-parents path
// the mutator ALSO zeroes `body.state.block_number` (since the
// validator's `max_block_number = -1` fold gives expected = 0
// when the parents list is empty). Result: block_number passes,
// justification_follows returns InvalidParents.

use std::time::{SystemTime, UNIX_EPOCH};

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_parents() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Use a deploy with valid_after_block_number = -1 so the future_
    // transaction validator (line 497: `valid_after >= block_number`)
    // does NOT fire when we mutate body.state.block_number to 0 below.
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let d1 = construct_deploy::source_deploy(
        "@0!(0)".to_string(),
        timestamp,
        Some(90_000),
        Some(1),
        None,
        Some(-1),
        Some(shard_id.clone()),
    )
    .expect("d1");
    let mutated = propose_with_block_mutation(&mut nodes[0], vec![d1], |b| {
        // Empty parents list AND zero block_number: block_number
        // validator computes expected = -1 + 1 = 0 with empty
        // parents; setting body.state.block_number = 0 makes that
        // check pass. justification_follows then trips on
        // parent_hashes.first() == None → InvalidParents.
        b.header.parents_hash_list = vec![];
        b.body.state.block_number = 0;
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
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidParents))
        ),
        "expected InvalidParents, got: {:?}",
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
         on InvalidParents"
    );
}
