// Integration test — Tier 1 production-path verification of the
// `ContainsFutureDeploy` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-36 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order: block_summary's `future_transaction`
// (validate.rs:486) runs after block_number. It rejects any deploy
// whose `valid_after_block_number >= block.body.state.block_number`.
//
// Construction: include a deploy with `valid_after_block_number =
// 100` while the block is being proposed at block_number = 1. The
// genuine `compute_deploys_checkpoint` does not filter — it
// processes every deploy passed in alt_deploys.

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
async fn integration_t_contains_future_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Construct a deploy with `valid_after_block_number = 100`. The
    // first post-genesis block is at block_number = 1; deploy is
    // "from the future" of every block until block_number 101.
    let future_deploy = construct_deploy::source_deploy_now_full(
        "@0!(0)".to_string(),
        Some(90_000),
        Some(1),
        None,
        Some(100),
        Some(shard_id.clone()),
    )
    .expect("future_deploy");

    let mutated =
        propose_with_block_mutation(&mut nodes[0], vec![future_deploy], |_b| {
            // No mutator needed — the deploy itself is the mutation.
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
            Either::Left(BlockError::Invalid(InvalidBlock::ContainsFutureDeploy))
        ),
        "expected ContainsFutureDeploy, got: {:?}",
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
         on ContainsFutureDeploy"
    );
}
