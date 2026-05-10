// Integration test — Tier 1 production-path verification of the
// `InvalidFollows` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-29 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order: block_summary's `justification_follows`
// (validate.rs:820) checks that the set of validators in the
// justifications equals the set of bonded validators in the main
// parent. Clearing all justifications gives an empty justified set
// — bonded set has 3 entries (v0, v1, v2) — line 860 returns
// `InvalidFollows`. parents_hash_list stays intact so the earlier
// "missing main parent" arm at line 833 does NOT fire.

use casper::rust::block_status::{BlockError, InvalidBlock};
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
async fn integration_t_invalid_follows() {
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
        // Clear all justifications. Bonded set ≠ justified set.
        b.justifications.clear();
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
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidFollows))
        ),
        "expected InvalidFollows, got: {:?}",
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
         on InvalidFollows"
    );
}
