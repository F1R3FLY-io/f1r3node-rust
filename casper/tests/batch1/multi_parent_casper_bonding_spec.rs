// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperBondingSpec.scala
//
// End-to-end "should allow bonding" through consensus: a validator that is NOT in the genesis
// bond set signs a bond deploy which a bonded validator carries into a block; once that block
// finalizes, the previously-unbonded validator IS bonded (its stake is in consensus state).
// This exercises the bonding path through the real PoS contract + multi-parent merging.

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use casper::rust::util::construct_deploy;
use casper::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_SEC};
use crypto::rust::public_key::PublicKey;

use crate::helper::bonding_util;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, DEFAULT_VALIDATOR_KEY_PAIRS};

/// Queries the PoS `bondStatus` for `public_key` against `node`'s current state (mirrors the
/// helper in `api::bonded_status_api_test`).
async fn bonded_status(public_key: &PublicKey, node: &TestNode) -> bool {
    let casper_for_engine = Arc::new(MultiParentCasperImpl {
        divergence_monitor: node.casper.divergence_monitor.clone(),
        block_retriever: node.casper.block_retriever.clone(),
        event_publisher: node.casper.event_publisher.clone(),
        runtime_manager: node.casper.runtime_manager.clone(),
        estimator: node.casper.estimator.clone(),
        block_store: node.casper.block_store.clone(),
        block_dag_storage: node.casper.block_dag_storage.clone(),
        deploy_storage: node.casper.deploy_storage.clone(),
        rejected_deploy_buffer: node.casper.rejected_deploy_buffer.clone(),
        deploy_lifecycle: node.casper.deploy_lifecycle.clone(),
        casper_buffer_storage: node.casper.casper_buffer_storage.clone(),
        validator_id: node.casper.validator_id.clone(),
        casper_shard_conf: node.casper.casper_shard_conf.clone(),
        approved_block: node.casper.approved_block.clone(),
        finalization_in_progress: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        recovery_sync_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalization_schedule: Arc::new(
            casper::rust::finality::finalization_schedule::FinalizationSchedule::new(2),
        ),
        certificate_verification_schedule: Arc::new(
            casper::rust::finality::certificate::CertificateVerificationSchedule::new(2),
        ),
        finalizer_task_in_progress: node.casper.finalizer_task_in_progress.clone(),
        finalizer_task_queued: node.casper.finalizer_task_queued.clone(),
        heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        deploys_in_scope_cache: Arc::new(parking_lot::Mutex::new(None)),
        active_validators_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    });
    let engine = EngineWithCasper::new(casper_for_engine);
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;
    BlockAPI::bond_status(&engine_cell, &public_key.bytes.to_vec())
        .await
        .expect("bondStatus should not fail")
}

#[tokio::test]
async fn multi_parent_casper_should_allow_bonding() {
    // First 3 validators are bonded at genesis; the 4th (n4) uses the DEFAULT keypair — which
    // owns genesisVaults[0] (9,000,000 REV) so it can pay for its own bond — and is UNBONDED.
    let validator_key_pairs = vec![
        DEFAULT_VALIDATOR_KEY_PAIRS[0].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[1].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[2].clone(),
        (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
    ];
    let validator_pks: Vec<PublicKey> = validator_key_pairs
        .iter()
        .map(|(_, pk)| pk.clone())
        .collect();
    let bonds: HashMap<PublicKey, i64> = validator_pks
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, pk)| (pk.clone(), 2 * i as i64 + 1))
        .collect();

    let parameters = GenesisBuilder::build_genesis_parameters(validator_key_pairs, &bonds);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .expect("create 4-node network");
    let shard_id = genesis.genesis_block.shard_id.clone();

    // n4 is unbonded at genesis.
    assert!(
        !bonded_status(&DEFAULT_PUB, &nodes[0]).await,
        "n4 must be UNBONDED at genesis"
    );

    // n4 signs a bond deploy; a bonded validator (node 0) carries it into a block.
    let bond_deploy = bonding_util::bonding_deploy(1000, &DEFAULT_SEC, Some(shard_id.clone()))
        .expect("bond deploy");
    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[bond_deploy])
        .await
        .expect("propagate the bond block");

    // Advance the DAG with the bonded genesis validators until b1 (the bond) finalizes.
    for round in 0..4 {
        let d = construct_deploy::basic_deploy_data(round, None, Some(shard_id.clone()))
            .expect("filler deploy");
        let _ = TestNode::propagate_block_at_index(&mut nodes, (round as usize + 1) % 3, &[d])
            .await
            .expect("propagate a filler block");
    }

    // The bond took effect through consensus: n4 is now bonded.
    assert!(
        bonded_status(&DEFAULT_PUB, &nodes[0]).await,
        "n4 must be BONDED after its bond block finalizes"
    );
}

/// B2 of the #341 TDD plan: a finalized bond survives epoch-boundary
/// merges.
///
/// The default test genesis sets a huge PoS epoch length precisely to
/// avoid the epoch change in the PoS close-block method, "which causes
/// block merge conflicts" (util/genesis_builder.rs). That avoidance is the
/// #341 blind spot: in production every epoch boundary runs that code, and
/// the preflight loses a freshly-bonded validator's bond around one. This
/// test walks straight into it: a small epoch length, a bond that
/// finalizes before the boundary, and sibling blocks at each boundary so a
/// multi-parent merge must adjudicate two close-block epoch changes.
#[tokio::test]
async fn a_finalized_bond_survives_an_epoch_boundary_merge() {
    let validator_key_pairs = vec![
        DEFAULT_VALIDATOR_KEY_PAIRS[0].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[1].clone(),
        DEFAULT_VALIDATOR_KEY_PAIRS[2].clone(),
        (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
    ];
    let validator_pks: Vec<PublicKey> = validator_key_pairs
        .iter()
        .map(|(_, pk)| pk.clone())
        .collect();
    let bonds: HashMap<PublicKey, i64> = validator_pks
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, pk)| (pk.clone(), 2 * i as i64 + 1))
        .collect();

    let mut parameters = GenesisBuilder::build_genesis_parameters(validator_key_pairs, &bonds);
    // Small epoch: boundaries at block numbers 4, 8, 12 — inside the walk
    // below, not beyond it.
    parameters.2.proof_of_stake.epoch_length = 4;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 4, None, None, None, None)
        .await
        .expect("create 4-node network");
    let shard_id = genesis.genesis_block.shard_id.clone();

    // n4 bonds in block #1.
    let bond_deploy = bonding_util::bonding_deploy(1000, &DEFAULT_SEC, Some(shard_id.clone()))
        .expect("bond deploy");
    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[bond_deploy])
        .await
        .expect("propagate the bond block");

    // Finalize the bond with filler rounds (blocks #2..#5, crossing the
    // first boundary at #4).
    for round in 0..4 {
        let d = construct_deploy::basic_deploy_data(round, None, Some(shard_id.clone()))
            .expect("filler deploy");
        let _ = TestNode::propagate_block_at_index(&mut nodes, (round as usize + 1) % 3, &[d])
            .await
            .expect("propagate a filler block");
    }
    assert!(
        bonded_status(&DEFAULT_PUB, &nodes[0]).await,
        "n4 must be BONDED once its bond block finalizes"
    );

    // Three rounds of {two sibling blocks, sync, merge block}: each round
    // advances the height by two, so the walk covers the #8 and #12
    // boundaries with multi-parent merges of sibling close-block state.
    for round in 0..3i32 {
        let d1 = construct_deploy::basic_deploy_data(100 + round * 3, None, Some(shard_id.clone()))
            .expect("sibling deploy 1");
        let d2 = construct_deploy::basic_deploy_data(101 + round * 3, None, Some(shard_id.clone()))
            .expect("sibling deploy 2");
        let d3 = construct_deploy::basic_deploy_data(102 + round * 3, None, Some(shard_id.clone()))
            .expect("merge deploy");
        // Siblings: node 0 and node 1 each build on their own view.
        let _s1 = nodes[0]
            .add_block_from_deploys(&[d1])
            .await
            .expect("sibling block 1");
        let _s2 = nodes[1]
            .add_block_from_deploys(&[d2])
            .await
            .expect("sibling block 2");
        {
            let mut refs: Vec<&mut TestNode> = nodes.iter_mut().collect();
            TestNode::propagate(&mut refs)
                .await
                .expect("propagate siblings");
        }
        // Node 2 merges both siblings.
        let _m = TestNode::propagate_block_at_index(&mut nodes, 2, &[d3])
            .await
            .expect("propagate the merge block");
    }

    // Finalize the boundary merges.
    for round in 0..4i32 {
        let d = construct_deploy::basic_deploy_data(200 + round, None, Some(shard_id.clone()))
            .expect("tail filler deploy");
        let _ = TestNode::propagate_block_at_index(&mut nodes, (round as usize + 1) % 3, &[d])
            .await
            .expect("propagate a tail filler block");
    }

    assert!(
        bonded_status(&DEFAULT_PUB, &nodes[0]).await,
        "n4's finalized bond must survive epoch-boundary merges (issue #341)"
    );
}
