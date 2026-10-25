// See casper/src/test/scala/coop/rchain/casper/api/BondedStatusAPITest.scala

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::engine::multi_parent_casper::{FinalizedBondCache, MultiParentCasperImpl};
use casper::rust::util::construct_deploy;
use casper::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_SEC};
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block_implicits::get_random_block;
use prost::bytes::Bytes;
use shared::rust::store::key_value_store::KvStoreError;

use crate::helper::bonding_util;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext, DEFAULT_VALIDATOR_KEY_PAIRS};

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        // Scala: buildGenesisParameters(
        //   defaultValidatorKeyPairs.take(3) :+ ConstructDeploy.defaultKeyPair,
        //   createBonds(defaultValidatorPks.take(3))
        // )
        // This means:
        // - First 3 validators: random keys from defaultValidatorKeyPairs (bonded)
        // - 4th validator (n4): ConstructDeploy.defaultKeyPair = (DEFAULT_SEC, DEFAULT_PUB)
        //   This matches genesisVaults[0] which has 9,000,000 REV, allowing n4 to pay for bonding

        let validator_key_pairs = vec![
            DEFAULT_VALIDATOR_KEY_PAIRS[0].clone(),
            DEFAULT_VALIDATOR_KEY_PAIRS[1].clone(),
            DEFAULT_VALIDATOR_KEY_PAIRS[2].clone(),
            (DEFAULT_SEC.clone(), DEFAULT_PUB.clone()), // n4 uses DEFAULT keypair to match genesisVaults[0]
        ];

        // Extract public keys for bonds
        let validator_pks: Vec<PublicKey> = validator_key_pairs
            .iter()
            .map(|(_, pk)| pk.clone())
            .collect();

        // Create bonds for first 3 validators only (n4 is unbonded)
        let bonds: HashMap<PublicKey, i64> = validator_pks
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, pk)| (pk.clone(), 2 * i as i64 + 1))
            .collect();

        let mut parameters = GenesisBuilder::build_genesis_parameters(validator_key_pairs, &bonds);
        parameters.2.proof_of_stake.epoch_length = 4;
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("Failed to build genesis");

        Self { genesis }
    }
}

/// Creates an EngineCell with EngineWithCasper from a TestNode's casper instance
/// Scala equivalent:
///   val engine = new EngineWithCasper[Task](node.casperEff)
///   Cell.mvarCell[Task, Engine[Task]](engine).flatMap { implicit engineCell => ... }
async fn bonded_status(public_key: &PublicKey, node: &TestNode) -> bool {
    // Create engine and engine_cell (Scala lines 40-41)
    let casper_for_engine = Arc::new(MultiParentCasperImpl {
        observer: std::sync::OnceLock::new(),
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
        finalized_bond_cache: node.casper.finalized_bond_cache.clone(),
        casper_shard_conf: node.casper.casper_shard_conf.clone(),
        approved_block: node.casper.approved_block.clone(),
        finalization_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalizer_task_in_progress: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        finalizer_task_queued: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        deploys_in_scope_cache: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        active_validators_cache: std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
    });
    let engine = EngineWithCasper::new(casper_for_engine);
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let public_key_bytes = public_key.bytes.to_vec();
    BlockAPI::bond_status(&engine_cell, &public_key_bytes)
        .await
        .expect("bondStatus should not fail")
}

#[tokio::test]
async fn bond_status_should_return_true_for_bonded_validator() {
    let ctx = TestContext::new().await;

    let nodes = TestNode::create_network(ctx.genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();

    let n1_pk = nodes[0]
        .validator_id_opt
        .as_ref()
        .unwrap()
        .public_key
        .clone();
    assert!(
        bonded_status(&n1_pk, &nodes[0]).await,
        "n1 should be bonded"
    );

    let n2_pk = nodes[1]
        .validator_id_opt
        .as_ref()
        .unwrap()
        .public_key
        .clone();
    assert!(
        bonded_status(&n2_pk, &nodes[0]).await,
        "n2 should be bonded"
    );

    let n3_pk = nodes[2]
        .validator_id_opt
        .as_ref()
        .unwrap()
        .public_key
        .clone();
    assert!(
        bonded_status(&n3_pk, &nodes[0]).await,
        "n3 should be bonded"
    );
}

#[tokio::test]
async fn bond_status_should_return_false_for_not_bonded_validators() {
    let ctx = TestContext::new().await;

    let node = TestNode::standalone(ctx.genesis.clone()).await.unwrap();

    let secp256k1 = Secp256k1;
    let (_, public_key) = secp256k1.new_key_pair();

    assert!(
        !bonded_status(&public_key, &node).await,
        "Unbonded validator should return false"
    );
}

#[tokio::test]
async fn cached_bond_status_rejects_a_stale_bonded_entry() {
    let ctx = TestContext::new().await;
    let node = TestNode::standalone(ctx.genesis.clone()).await.unwrap();
    let dag = node.casper.block_dag().await.unwrap();
    assert!(node.casper.finalized_bond_cache.is_bonded(&dag).unwrap());

    let mut stale_dag = dag.clone();
    let mut metadata = dag.lookup_unsafe(&dag.last_finalized_block()).unwrap();
    metadata.block_hash = Bytes::from(vec![42; 32]);
    metadata.weight_map.clear();
    stale_dag
        .block_metadata_index
        .write()
        .add(metadata.clone())
        .unwrap();
    stale_dag.dag_set.insert(metadata.block_hash.clone());
    stale_dag.last_finalized_block_hash = metadata.block_hash;

    assert!(!node
        .casper
        .finalized_bond_cache
        .is_bonded(&stale_dag)
        .unwrap());
}

#[tokio::test]
async fn failed_finalization_after_lfb_advance_rejects_an_unbonded_deploy() {
    let ctx = TestContext::new().await;
    let node = TestNode::standalone(ctx.genesis.clone()).await.unwrap();
    let storage = &node.casper.block_dag_storage;
    let genesis_hash = ctx.genesis.genesis_block.block_hash.clone();
    let shard_id = ctx.genesis.genesis_block.shard_id.clone();
    let make_block = |number, parents| {
        get_random_block(
            Some(number),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(parents),
            Some(vec![]),
            Some(vec![]),
            None,
            Some(vec![]),
            Some(shard_id.clone()),
            None,
        )
    };
    let late_parent = make_block(1, vec![genesis_hash.clone()]);
    let new_lfb = make_block(2, vec![genesis_hash, late_parent.block_hash.clone()]);
    storage.insert(&new_lfb, InsertMode::Normal).unwrap();

    let old_dag = storage.get_representation().unwrap();
    assert!(node
        .casper
        .finalized_bond_cache
        .is_bonded(&old_dag)
        .unwrap());

    let mut effect_calls = 0;
    let result = storage
        .record_directly_finalized(new_lfb.block_hash.clone(), 1.0, |_| {
            effect_calls += 1;
            let outcome = if effect_calls == 1 {
                storage.insert(&late_parent, InsertMode::Normal).map(|_| ())
            } else {
                Err(KvStoreError::IoError("injected effect failure".to_string()))
            };
            async move { outcome }
        })
        .await;
    assert!(matches!(result, Err(KvStoreError::IoError(_))));
    assert_eq!(effect_calls, 2);

    let dag = storage.get_representation().unwrap();
    assert_eq!(dag.last_finalized_block(), new_lfb.block_hash);
    assert!(!node.casper.finalized_bond_cache.is_bonded(&dag).unwrap());

    let deploy = construct_deploy::basic_deploy_data(4001, None, Some(shard_id.clone())).unwrap();
    let rejected = BlockAPI::deploy(&node.engine_cell, deploy.clone(), &None, false, &shard_id)
        .await
        .unwrap_err();
    assert!(rejected.to_string().contains("validator is not bonded"));
    assert!(!node.deploy_storage.contains_sig(&deploy.sig).unwrap());
}

// Bonding through consensus IS implemented in the Rust port: a bond deploy propagated
// through the network makes the validator bonded (the "Scala ignore" from PR #288 is stale —
// this test passes against the current PoS + multi-parent merging).
#[tokio::test]
async fn bond_status_should_return_true_for_newly_bonded_validator() {
    let ctx = TestContext::new().await;

    let mut nodes = TestNode::create_network(ctx.genesis.clone(), 4, None, None, None, None)
        .await
        .unwrap();

    let mut produce_deploys = Vec::new();
    for i in 0..3 {
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        let deploy = construct_deploy::basic_deploy_data(
            i,
            None,
            Some(ctx.genesis.genesis_block.shard_id.clone()),
        )
        .unwrap();
        produce_deploys.push(deploy);
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    let n4_private_key = nodes[3]
        .validator_id_opt
        .as_ref()
        .unwrap()
        .private_key
        .clone();
    let bond_deploy = bonding_util::bonding_deploy(
        1000,
        &n4_private_key,
        Some(ctx.genesis.genesis_block.shard_id.clone()),
    )
    .unwrap();

    let n4_pk = nodes[3]
        .validator_id_opt
        .as_ref()
        .unwrap()
        .public_key
        .clone();

    // Scala line 81: n4 is not bonded initially
    assert!(
        !bonded_status(&n4_pk, &nodes[0]).await,
        "n4 should not be bonded initially"
    );

    let _b1 = TestNode::propagate_block_at_index(&mut nodes, 0, &[bond_deploy])
        .await
        .unwrap();

    let _b2 = TestNode::propagate_block_at_index(&mut nodes, 1, &[produce_deploys[0].clone()])
        .await
        .unwrap();

    assert!(
        !bonded_status(&n4_pk, &nodes[0]).await,
        "n4 should not be bonded yet (b1 not finalized)"
    );

    let _b3 = TestNode::propagate_block_at_index(&mut nodes, 2, &[produce_deploys[1].clone()])
        .await
        .unwrap();

    let _b4 = TestNode::propagate_block_at_index(&mut nodes, 0, &[produce_deploys[2].clone()])
        .await
        .unwrap();

    assert!(
        bonded_status(&n4_pk, &nodes[0]).await,
        "n4 should be bonded now (b1 finalized)"
    );

    assert!(bonded_status(&n4_pk, &nodes[3]).await);
    let finalized = nodes[3].casper.last_finalized_block().await.unwrap();
    assert!(!finalized
        .body
        .state
        .bonds
        .iter()
        .any(|bond| bond.validator == n4_pk.bytes));

    let stale_cache = FinalizedBondCache::from_storage(
        &nodes[3].casper.block_dag_storage,
        nodes[3].casper.validator_id.as_ref(),
    )
    .unwrap();
    let shard_id = &ctx.genesis.genesis_block.shard_id;
    let deploy = construct_deploy::basic_deploy_data(1001, None, Some(shard_id.clone())).unwrap();
    let rejected = BlockAPI::deploy(
        &nodes[3].engine_cell,
        deploy.clone(),
        &None,
        false,
        shard_id,
    )
    .await
    .unwrap_err();
    assert!(rejected.to_string().contains("validator is not bonded"));
    assert!(!nodes[3].deploy_storage.contains_sig(&deploy.sig).unwrap());

    for round in 0..8 {
        let filler =
            construct_deploy::basic_deploy_data(2000 + round, None, Some(shard_id.clone()))
                .unwrap();
        TestNode::propagate_block_at_index(&mut nodes, round as usize % 3, &[filler])
            .await
            .unwrap();
        bonded_status(&n4_pk, &nodes[3]).await;
        let finalized = nodes[3].casper.last_finalized_block().await.unwrap();
        if finalized
            .body
            .state
            .bonds
            .iter()
            .any(|bond| bond.validator == n4_pk.bytes)
        {
            break;
        }
    }

    let finalized = nodes[3].casper.last_finalized_block().await.unwrap();
    assert!(finalized
        .body
        .state
        .bonds
        .iter()
        .any(|bond| bond.validator == n4_pk.bytes));
    let dag = nodes[3].casper.block_dag().await.unwrap();
    assert!(stale_cache.is_bonded(&dag).unwrap());
    let mut missing = dag.clone();
    missing.last_finalized_block_hash = prost::bytes::Bytes::from(vec![42; 32]);
    assert!(stale_cache.is_bonded(&missing).is_err());

    let accepted = BlockAPI::deploy(
        &nodes[3].engine_cell,
        deploy.clone(),
        &None,
        false,
        shard_id,
    )
    .await;
    assert!(accepted.is_ok(), "{accepted:?}");
    assert!(nodes[3].deploy_storage.contains_sig(&deploy.sig).unwrap());
}
