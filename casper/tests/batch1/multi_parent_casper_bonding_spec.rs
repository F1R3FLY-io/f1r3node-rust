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
        finalization_in_progress: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        recovery_sync_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalization_schedule: Arc::new(
            casper::rust::finality::finalization_schedule::FinalizationSchedule::new(2),
        ),
        certificate_verification_schedule: Arc::new(
            casper::rust::finality::certificate::CertificateVerificationSchedule::new(2),
        ),
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
