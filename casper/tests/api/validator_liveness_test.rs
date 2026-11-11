use std::sync::Arc;

use casper::rust::api::validator_liveness::validator_liveness;
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;

use crate::economics::support::{admit_everywhere, identity, propose};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// A validator's liveness report shows every committee member's bucket, the
/// member that never proposes draining, this node's own decision, and the
/// state of this node's last finalized block.
#[tokio::test]
async fn a_liveness_report_shows_buckets_decision_and_finalized_state() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let silent = identity(&nodes[3]);
    for _ in 0..4 {
        for index in 0..3 {
            let block = propose(&mut nodes[index], &[], &[]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    for _ in 0..12 {
        nodes[0].casper.finalize().await.expect("finalizer cycle");
    }

    let engine_cell = EngineCell::init();
    engine_cell
        .set(Arc::new(EngineWithCasper::new(nodes[0].casper.clone())))
        .await;
    let report = validator_liveness(&engine_cell).await.expect("report");

    let members: Vec<_> = report
        .local_view
        .buckets
        .iter()
        .map(|bucket| bucket.validator.clone())
        .collect();
    assert_eq!(members.len(), 4, "every committee member has a bucket");
    let silent_bucket = report
        .local_view
        .buckets
        .iter()
        .find(|bucket| bucket.validator == silent)
        .expect("the silent member is tracked");
    let full = &silent_bucket.capacity * 1_000_000u64;
    assert!(
        silent_bucket.level < full,
        "a member that never proposed has drained"
    );
    assert!(
        report.decision.is_some(),
        "a validator node reports its decision"
    );

    let finalized = nodes[0]
        .casper
        .last_finalized_block()
        .await
        .expect("last finalized block");
    assert_eq!(report.finalized_state.block_hash, finalized.block_hash);
    assert!(report.finalized_state.block_number > 0);
    assert_eq!(report.finalized_state.state.bonds.len(), 4);
}
