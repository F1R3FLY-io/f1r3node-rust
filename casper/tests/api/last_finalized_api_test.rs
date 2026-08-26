// See casper/src/test/scala/coop/rchain/casper/api/LastFinalizedAPITest.scala

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

/// Test fixture that holds common test data for LastFinalizedAPITest
struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        // Note: validatorsNum not specified, so uses default = 4
        // But zip with List(10, 10, 10) means only first 3 validators get bonds
        fn bonds_function(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
            validators
                .into_iter()
                .zip(vec![10i64, 10i64, 10i64])
                .collect()
        }

        let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(
            Some(bonds_function),
            None, // Use default validatorsNum = 4, matching Scala behavior
        );
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("Failed to build genesis");

        Self { genesis }
    }
}

/// Creates an EngineCell with EngineWithCasper from a TestNode's casper instance
/// Equivalent to Scala: val engine = new EngineWithCasper[Task](n1.casperEff)
///                       engineCell <- Cell.mvarCell[Task, Engine[Task]](engine)
async fn create_engine_cell(node: &TestNode) -> EngineCell {
    let casper_for_engine = Arc::new(MultiParentCasperImpl {
        block_retriever: node.casper.block_retriever.clone(),
        event_publisher: node.casper.event_publisher.clone(),
        runtime_manager: node.casper.runtime_manager.clone(),
        estimator: node.casper.estimator.clone(),
        block_store: node.casper.block_store.clone(),
        block_dag_storage: node.casper.block_dag_storage.clone(),
        deploy_storage: node.casper.deploy_storage.clone(),
        pending_cosigner_metadata: node.casper.pending_cosigner_metadata.clone(),
        rejected_deploy_buffer: node.casper.rejected_deploy_buffer.clone(),
        casper_buffer_storage: node.casper.casper_buffer_storage.clone(),
        validator_id: node.casper.validator_id.clone(),
        casper_shard_conf: node.casper.casper_shard_conf.clone(),
        approved_block: node.casper.approved_block.clone(),
        finalization_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        recovery_sync_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalization_schedule: std::sync::Arc::new(
            casper::rust::finality::finalization_schedule::FinalizationSchedule::new(2),
        ),
        heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        deploys_in_scope_cache: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        active_validators_cache: std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
    });
    let engine = EngineWithCasper::new(casper_for_engine);
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;
    engine_cell
}

async fn is_finalized(block: &BlockMessage, engine_cell: &EngineCell) -> bool {
    // `BlockAPI::is_finalized` expects the hex string of the raw `block_hash` (it pads + hex-decodes
    // back to the DAG key). NOT `proto_util::hash_string`, which returns a *doubly*-encoded
    // `hex(block_hash.encode_to_vec())` — passing that yielded a key that never matched the DAG's
    // finalized set, so this helper used to return `false` for every block unconditionally.
    let block_hash_str = hex::encode(&block.block_hash);
    BlockAPI::is_finalized(engine_cell, &block_hash_str)
        .await
        .expect("isFinalized should not fail")
}

/*
 * DAG Looks like this:
 *
 *           b7
 *           |
 *           b6
 *           |
 *           b5 <- last finalized block
 *         / |
 *        |  b4
 *        |  |
 *       b2  b3
 *         \ |
 *           b1
 *           |
 *         genesis
 */
#[tokio::test]
async fn is_finalized_should_return_true_for_ancestors_of_last_finalized_block() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    // Produce a round-robin DAG deep enough that the finalizer advances several blocks past
    // genesis. Each block is propagated to all nodes before the next is produced, so the DAG is
    // linear: the block at height h is an ancestor of every block at height > h. (The Scala port
    // asserted a specific "b5" frontier over a hand-built DAG; the Rust multi-parent finalizer is
    // more conservative — it finalizes from the base up — so this instead asserts the invariant
    // that actually characterizes correct finalization: `isFinalized` is downward-closed over the
    // ancestors of the last finalized block.)
    let mut blocks = Vec::new();
    for i in 0..15 {
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        let deploy = construct_deploy::basic_deploy_data(
            i,
            None,
            Some(ctx.genesis.genesis_block.shard_id.clone()),
        )
        .unwrap();
        let block = TestNode::propagate_block_at_index(&mut nodes, (i % 3) as usize, &[deploy])
            .await
            .unwrap();
        blocks.push(block);
    }

    let last_finalized_block = nodes[0].casper.last_finalized_block().await.unwrap();
    let lfb_number = last_finalized_block.body.state.block_number;
    let engine_cell = create_engine_cell(&nodes[0]).await;

    // Finalization advanced past genesis, so the LFB has real ancestors to check.
    assert!(
        lfb_number >= 1,
        "finalization should advance past genesis after a deep round-robin DAG (LFB height={lfb_number})"
    );

    // The last finalized block itself reports finalized.
    assert!(
        is_finalized(&last_finalized_block, &engine_cell).await,
        "the last finalized block (height {lfb_number}) must report isFinalized == true"
    );

    // `isFinalized` is exactly the finalized prefix of this linear DAG: every block at or below the
    // LFB height is finalized (downward-closed over the ancestors of the LFB, the LFB included), and
    // every block above it is not yet finalized (the frontier). This is the invariant the Scala
    // "ancestors are finalized" test was reaching for.
    for block in &blocks {
        let height = block.body.state.block_number;
        let finalized = is_finalized(block, &engine_cell).await;
        if height <= lfb_number {
            assert!(
                finalized,
                "ancestor/self at height {height} (<= LFB height {lfb_number}) must be finalized"
            );
        } else {
            assert!(
                !finalized,
                "frontier block at height {height} (> LFB height {lfb_number}) must not be finalized"
            );
        }
    }
}

/*
 * DAG Looks like this:
 *
 *           b5
 *             \
 *              b4
 *             /
 *        b7 b3 <- last finalized block
 *        |    \
 *        b6    b2
 *          \  /
 *           b1
 *       [n3 n1 n2]
 *           |
 *         genesis
 */
// TODO: Multi-parent merging changes finalization semantics.
// Scala ignored this in PR #288.
// Non-ancestors of the last finalized block (its children, uncles, and cousins) must report
// `isFinalized == false`. This holds under the Rust multi-parent finalizer (the "Scala ignore"
// port label was stale — the property is real coverage).
#[tokio::test]
async fn should_return_false_for_children_uncles_and_cousins_of_last_finalized_block() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    // Note: We create deploys one by one with sleep to ensure unique timestamps.
    let mut produce_deploys = Vec::new();
    for i in 0..7 {
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        let deploy = construct_deploy::basic_deploy_data(
            i,
            None,
            Some(ctx.genesis.genesis_block.shard_id.clone()),
        )
        .unwrap();
        produce_deploys.push(deploy);
    }

    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[produce_deploys[0].clone()])
        .await
        .unwrap();

    let _b2 = TestNode::propagate_block_to_one(&mut nodes, 1, 0, &[produce_deploys[1].clone()])
        .await
        .unwrap();

    let b3 = TestNode::propagate_block_to_one(&mut nodes, 0, 1, &[produce_deploys[2].clone()])
        .await
        .unwrap();

    let b4 = TestNode::propagate_block_to_one(&mut nodes, 1, 0, &[produce_deploys[3].clone()])
        .await
        .unwrap();

    let _b5 = TestNode::propagate_block_to_one(&mut nodes, 0, 1, &[produce_deploys[4].clone()])
        .await
        .unwrap();

    nodes[2].tle.test_network().clear(&nodes[2].local).unwrap(); // n3 misses b2, b3, b4, b5

    let b6 = TestNode::propagate_block_at_index(&mut nodes, 2, &[produce_deploys[5].clone()])
        .await
        .unwrap();

    let b7 = TestNode::propagate_block_at_index(&mut nodes, 2, &[produce_deploys[6].clone()])
        .await
        .unwrap();

    let last_finalized_block = nodes[0].casper.last_finalized_block().await.unwrap();

    let b3_block_hash = proto_util::hash_string(&b3);
    assert_eq!(
        proto_util::hash_string(&last_finalized_block),
        b3_block_hash,
        "Expected last finalized block to be b3"
    );

    let engine_cell = create_engine_cell(&nodes[0]).await;

    assert!(
        !is_finalized(&b4, &engine_cell).await,
        "b4 (child of b3) should not be finalized"
    );

    assert!(
        !is_finalized(&b6, &engine_cell).await,
        "b6 (uncle of b3) should not be finalized"
    );

    assert!(
        !is_finalized(&b7, &engine_cell).await,
        "b7 (cousin of b3) should not be finalized"
    );
}
