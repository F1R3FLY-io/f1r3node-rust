// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Integration test — Tier 1 production-path verification of the
// `InvalidShardId` arm of the dispatcher's `is_slashable()` catch-
// all (Bug #3 fix).
//
// UC-31 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher records every
// slashable variant), formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order (validate.rs::block_summary): block_hash →
// timestamp → SHARD_IDENTIFIER. Mutating `block.shard_id` after
// checkpoint computation, then re-signing, makes block_hash
// validation pass (signature is over the mutated body) but
// `Validate::shard_identifier` (validate.rs:259) fires next.

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, process_block_bypassing_of_interest_filter, production_snapshot_at,
    propose_with_block_mutation,
};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_shard_id() {
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
        // Mutate the top-level shard_id to a shard the genesis does
        // not match. Validate::shard_identifier (validate.rs:259)
        // compares `block.shard_id` against the configured shard;
        // any mismatch is an InvalidShardId.
        b.shard_id = "wrong-shard-uc-32".to_string();
    })
    .await
    .expect("propose_with_block_mutation");

    // Bypass `check_if_of_interest` — its upstream shard filter
    // would reject the block as NotOfInterest before reaching the
    // shard_identifier validator inside block_summary. The deeper-
    // layer `InvalidShardId` is defence-in-depth; the dispatcher's
    // catch-all is what we're verifying.
    let status = process_block_bypassing_of_interest_filter(&mut nodes[1], mutated.clone())
        .await
        .expect("process_block_bypassing_of_interest_filter");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidShardId))
        ),
        "expected InvalidShardId, got: {:?}",
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
         on InvalidShardId"
    );
}
