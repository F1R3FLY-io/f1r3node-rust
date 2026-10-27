// See casper/src/test/scala/coop/rchain/casper/api/BondedStatusAPITest.scala

use std::collections::HashMap;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::util::construct_deploy::{DEFAULT_PUB, DEFAULT_SEC};
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block_implicits::get_random_block;
use shared::rust::store::key_value_store::KvStoreError;

use crate::helper::bonding_util;
// Changed by DR-134 (DR-66 item 1): offered deploys.
// use casper::rust::util::construct_deploy;
use crate::helper::offered_deploy as construct_deploy;
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
    let engine = EngineWithCasper::new(node.casper.clone());
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

/// An advance whose finalization effect failed leaves the bond cache stale;
/// the next finalizer cycle refreshes it even though that cycle does not
/// advance, and admission then rejects the deploy.
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
    for block in [&late_parent, &new_lfb] {
        node.casper.block_store.put_block_message(block).unwrap();
    }
    storage.insert(&new_lfb, InsertMode::Normal).unwrap();
    assert!(node.casper.finalized_bond_cache.is_bonded());

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
    assert!(
        node.casper.finalized_bond_cache.is_bonded(),
        "fixture precondition: no cycle has run since the advance"
    );

    node.casper.finalize().await.unwrap();
    assert!(
        !node.casper.finalized_bond_cache.is_bonded(),
        "a cycle that does not advance must still refresh the bond cache"
    );

    let deploy = construct_deploy::basic_deploy_data(4001, None, Some(shard_id.clone())).unwrap();
    let rejected = BlockAPI::deploy_offered(
        &node.engine_cell,
        deploy.envelope.to_proto().unwrap(),
        &None,
        false,
        &shard_id,
    )
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
    let finalized = nodes[3].casper.finalize().await.unwrap();
    assert!(!finalized
        .body
        .state
        .bonds
        .iter()
        .any(|bond| bond.validator == n4_pk.bytes));

    let shard_id = &ctx.genesis.genesis_block.shard_id;
    let deploy = construct_deploy::basic_deploy_data(1001, None, Some(shard_id.clone())).unwrap();
    let rejected = BlockAPI::deploy_offered(
        &nodes[3].engine_cell,
        deploy.envelope.to_proto().unwrap(),
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
        let finalized = nodes[3].casper.finalize().await.unwrap();
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

    let finalized = nodes[3].casper.finalize().await.unwrap();
    assert!(finalized
        .body
        .state
        .bonds
        .iter()
        .any(|bond| bond.validator == n4_pk.bytes));
    assert!(nodes[3].casper.finalized_bond_cache.is_bonded());

    let accepted = BlockAPI::deploy_offered(
        &nodes[3].engine_cell,
        deploy.envelope.to_proto().unwrap(),
        &None,
        false,
        shard_id,
    )
    .await;
    assert!(accepted.is_ok(), "{accepted:?}");
    assert!(nodes[3].deploy_storage.contains_sig(&deploy.sig).unwrap());
}
