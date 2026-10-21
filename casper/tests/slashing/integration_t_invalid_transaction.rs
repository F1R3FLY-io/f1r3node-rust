// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Integration test — Tier 1 production-path verification of the
// `InvalidTransaction` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-34 from docs/casper/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Validation order: block_summary passes → validate_block_checkpoint
// runs replay → if computed_post_state_hash != block.body.state.
// post_state_hash, returns Right(None), which the dispatcher
// converts to `InvalidTransaction`
// (engine/multi_parent_casper/mod.rs:570-573).
//
// Mutating body.state.post_state_hash to a different value (after
// checkpoint computation but before signing) is the canonical way
// to drive replay-mismatch.

// Changed by DR-134 (DR-66 item 1): the offered verdict is a BlockException,
// and the DAG query needs the Casper trait.
// use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::block_status::BlockError;
use casper::rust::casper::MultiParentCasper;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;
// Changed by DR-134 (DR-66 item 1): offered deploys.
// use casper::rust::util::construct_deploy;
use crate::helper::offered_deploy as construct_deploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_transaction() {
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
        // Mutate post_state_hash to 32 bytes of 0xFF — replay will
        // compute the genuine hash and disagree, triggering Right(None)
        // → InvalidTransaction.
        b.body.state.post_state_hash = Bytes::from(vec![0xFF; 32]);
    })
    .await
    .expect("propose_with_block_mutation");

    let status = nodes[1]
        .process_block(mutated.clone())
        .await
        .expect("process_block");
    // Changed by DR-134 (DR-66 item 1): the offered replay checks the
    // committed post-state itself (`offered_block.rs:563-567`), so validation
    // returns a BlockException, and the block processor records the block as
    // InvalidTransaction (`block_processor.rs:629-648`), whose demoted drop arm
    // keeps the block out of the DAG. The drop and the absence of slash
    // evidence are the same as for a body-only block.
    // assert!(
    //     matches!(
    //         status,
    //         Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction))
    //     ),
    //     "expected InvalidTransaction, got: {:?}",
    //     status
    // );
    assert!(
        matches!(
            &status,
            Either::Left(BlockError::BlockException(error))
                if error.to_string().contains("offered block replay post-state differs from commitment")
        ),
        "expected the offered replay mismatch, got: {:?}",
        status
    );
    let dag = nodes[1]
        .casper
        .block_dag()
        .await
        .expect("dag representation");
    assert!(
        !dag.contains(&mutated.block_hash),
        "a demoted verdict's block must not enter the DAG"
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
        "demoted: InvalidTransaction is judged against local replay state \
         and mints no slash evidence"
    );
}
