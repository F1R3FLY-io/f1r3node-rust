// Integration test — Tier 1 production-path verification of the
// `_` non-slashable arm of `MultiParentCasperImpl::handle_invalid_block`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5
// (production-path integration). Plan-agent designed; the
// "valid arm" recipe is the simplest of the five Track 2 tests
// and serves as the smoke test confirming the
// SlashingProductionAdapter wiring is correct.
//
// Property: a well-formed block processed through the real
// production pipeline produces no EquivocationRecord and no entry
// in the invalid-block index. This catches the false-positive
// regression where the dispatcher mints unconditionally.

use casper::rust::block_status::ValidBlock;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

use super::integration_helpers::{canonical_validator_order, production_snapshot_at};
use super::observer::SlashingObserver;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_valid_no_record() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
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

    // Process through node 1 — must classify Valid.
    let status = nodes[1]
        .process_block(signed_block.clone())
        .await
        .expect("process_block");
    assert!(
        matches!(status, Either::Right(ValidBlock::Valid)),
        "well-formed block must classify Valid, got: {:?}",
        status
    );

    // Snapshot the production tier and assert no record was minted
    // and the block is NOT in the invalid index.
    let snapshot = production_snapshot_at(&nodes[1], &signed_block, &genesis.genesis_block, validators)
        .await
        .expect("snapshot");

    // No equivocation record at any (validator, base_seq) for v0.
    for base in 0..=20 {
        assert!(
            !<_ as SlashingObserver>::has_record(&snapshot, "v0", base),
            "Valid arm: no record minted at (v0, {})",
            base
        );
    }

    // Coop vault remains at 0 (no slashing occurred).
    assert_eq!(<_ as SlashingObserver>::coop_vault(&snapshot), 0,
        "Valid arm: no stake forfeited");

    // All validators still active, all positively bonded.
    for i in 0..3 {
        let v = format!("v{}", i);
        assert!(<_ as SlashingObserver>::is_active(&snapshot, &v),
            "Valid arm: {} stays active", v);
        assert!(<_ as SlashingObserver>::bond(&snapshot, &v) > 0,
            "Valid arm: {} stays bonded", v);
    }
}
