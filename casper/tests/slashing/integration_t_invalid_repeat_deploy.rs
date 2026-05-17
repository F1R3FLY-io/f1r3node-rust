// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Integration test — Tier 1 production-path verification of the
// `InvalidRepeatDeploy` arm of the dispatcher's `is_slashable()`
// catch-all (Bug #3 fix).
//
// UC-32 from docs/theory/slashing/slashing-specification.md §12.
// Theorem citation: T-9.3 (catch-all dispatcher), Rocq
// formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// Recipe:
//   1. v0 proposes b1 normally containing d1.
//   2. v0 proposes b2 via propose_with_block_mutation with a FRESH
//      deploy d2; then the mutator REPLACES body.deploys with the
//      ProcessedDeploy form of d1.
//   3. Validation order: block_summary's `repeat_deploy`
//      (validate.rs:269) iterates b2.body.deploys, finds d1, walks
//      ancestors, finds d1 in b1 → InvalidRepeatDeploy. Replay
//      (which would also detect this differently) runs LATER, so
//      repeat_deploy fires first.

use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::{
    canonical_validator_order, production_snapshot_at, propose_with_block_mutation,
};
use super::observer::SlashingObserver;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[serial_test::serial]
#[tokio::test]
async fn integration_t_invalid_repeat_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();

    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .expect("Failed to create network");

    let validators = canonical_validator_order(&genesis);

    // Step 1: nodes[0] proposes b1 normally with d1.
    let d1 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).expect("d1");
    nodes[0].casper.deploy(d1.clone()).expect("deploy d1");
    let b1 = nodes[0].create_block_unsafe(&[]).await.expect("create b1");
    // nodes[0] processes b1 back into its own DAG so the snapshot
    // for proposing b2 sees b1 as the parent (not genesis). Without
    // this, b1's tuple-space effects exist but the DAG doesn't know
    // about b1, leading to "Unable to consume results of system
    // deploy" during checkpoint computation for b2.
    let _ = nodes[0]
        .process_block(b1.clone())
        .await
        .expect("nodes[0] process b1");
    // nodes[1] receives b1 so it's in their DAG when checking b2.
    let _ = nodes[1]
        .process_block(b1.clone())
        .await
        .expect("nodes[1] process b1");

    // Step 2: nodes[0] proposes b2 with a fresh deploy d2 for
    // checkpoint-computation; then mutator REPLACES body.deploys
    // with a ProcessedDeploy wrapping the original d1.
    let d2 = construct_deploy::basic_deploy_data(20, None, Some(shard_id.clone())).expect("d2");
    let d1_processed = ProcessedDeploy {
        deploy: d1.clone(),
        cost: PCost { cost: 0 },
        deploy_log: Vec::new(),
        is_failed: false,
        system_deploy_error: None,
        cost_trace_digest: Default::default(),
        cost_trace_event_count: 0,
    };
    let mutated = propose_with_block_mutation(&mut nodes[0], vec![d2], move |b| {
        b.body.deploys = vec![d1_processed];
    })
    .await
    .expect("propose_with_block_mutation");

    // Step 3: nodes[1] processes b2; repeat_deploy detects d1 is
    // already in b1's body (b1 is an ancestor of b2).
    let status = nodes[1]
        .process_block(mutated.clone())
        .await
        .expect("process_block");
    assert!(
        matches!(
            status,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy))
        ),
        "expected InvalidRepeatDeploy, got: {:?}",
        status
    );

    let snapshot = production_snapshot_at(&nodes[1], &b1, &genesis.genesis_block, validators)
        .await
        .expect("snapshot");

    let has_v0 = (0..=10).any(|b| <_ as SlashingObserver>::has_record(&snapshot, "v0", b));
    assert!(
        has_v0,
        "post-fix #3 catch-all: dispatcher mints record for v0 \
         on InvalidRepeatDeploy"
    );
}
