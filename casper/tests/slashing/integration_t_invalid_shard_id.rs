// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Integration test — Tier 1 production-path verification of
// `InvalidShardId` rejection persistence without economic evidence.
//
// UC-31 from docs/casper/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3
// (`certified_non_slashable_rejection_preserves_evidence`).
//
// Validation changes one authenticated deploy's shard identifier while the
// top-level shard and certified floor remain valid. The deploy-shard check
// then returns InvalidShardId before protocol-v6 envelope validation.

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
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    let d1 = construct_deploy::basic_deploy_data(0, None, Some("wrong-shard-uc-32".to_string()))
        .expect("wrong-shard authenticated deploy");
    let mutated = propose_with_block_mutation(&mut nodes[0], vec![d1], |_| {})
        .await
        .expect("propose_with_block_mutation");

    // Bypass `check_if_of_interest` — its upstream shard filter
    // would reject the block as NotOfInterest before reaching the
    // shard_identifier validator inside block_summary. The deeper-
    // layer `InvalidShardId` is defence-in-depth; the dispatcher's
    // certified-rejection path is what this test verifies.
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
        !has_v0,
        "demoted: InvalidShardId is judged against local state and \
         mints no slash evidence"
    );
}
