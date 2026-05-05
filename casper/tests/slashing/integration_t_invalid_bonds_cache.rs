// Integration test — Tier 1 production-path verification of the
// `InvalidBondsCache` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-35 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order: block_summary passes → validate_block_checkpoint
// passes → bonds_cache reads bonds from runtime via post_state_hash
// and compares to `block.body.state.bonds`. If they differ,
// `Validate::bonds_cache` (validate.rs:1030) returns InvalidBondsCache.
//
// To drive: mutate body.state.bonds to wrong values AFTER checkpoint.
// Replay still succeeds (it doesn't read body.state.bonds — it
// computes from post-state). The bonds_cache validator detects the
// mismatch.

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::Bond;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_bonds_cache() {
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
        // Replace bonds with a bogus single-validator entry. Replay's
        // computed bonds (from post-state) will not match.
        b.body.state.bonds = vec![Bond {
            validator: Bytes::from(vec![0u8; 32]),
            stake: 999_999_999,
        }];
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
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        ),
        "expected InvalidBondsCache, got: {:?}",
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
         on InvalidBondsCache"
    );
}
