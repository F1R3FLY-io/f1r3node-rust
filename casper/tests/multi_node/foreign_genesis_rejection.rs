// Validates that a block from a chain with a different genesis is not accepted
// by the main network even when both chains share the same shard_id.
//
// Rejection path: the foreign block's parent hash (its own genesis) is unknown
// to the main network's block store, so check_dependencies_with_effects returns
// false without ever reaching Casper validation. The block is buffered until
// the stale TTL expires, but it never enters the main DAG.

use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use lazy_static::lazy_static;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

lazy_static! {
    // One key pair per test binary execution — keeps GenesisParameters stable so
    // the rogue genesis hits the cache on repeated runs within the same process.
    static ref ROGUE_SK_PK: (
        crypto::rust::private_key::PrivateKey,
        crypto::rust::public_key::PublicKey,
    ) = Secp256k1.new_key_pair();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreign_chain_blocks_are_not_accepted_by_main_network() {
    crate::init_logger();

    // Main genesis uses the default 4-validator set (cached across all tests).
    let main_genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();

    // Rogue genesis: 1 validator with fresh keys -> genuinely different genesis block.
    let (rogue_sk, rogue_pk) = ROGUE_SK_PK.clone();
    let rogue_bonds = GenesisBuilder::create_bonds(vec![rogue_pk.clone()]);
    let rogue_params =
        GenesisBuilder::build_genesis_parameters(vec![(rogue_sk, rogue_pk)], &rogue_bonds);
    let rogue_genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(rogue_params))
        .await
        .unwrap();

    assert_ne!(
        main_genesis.genesis_block.block_hash, rogue_genesis.genesis_block.block_hash,
        "test setup error: genesis hashes must differ"
    );

    let main_shard_id = main_genesis.genesis_block.shard_id.clone();

    // 3-node main network and a standalone rogue node (separate genesis, separate stores).
    let mut main_nodes = TestNode::create_network(main_genesis, 3, None, None, None, None)
        .await
        .unwrap();
    let mut rogue_node = TestNode::standalone(rogue_genesis.clone()).await.unwrap();

    // Rogue node proposes one block on its own chain.
    // DEFAULT_SEC is pre-funded in every genesis created by GenesisBuilder, so
    // the deploy is spendable from the rogue chain's vault without special setup.
    let deploy = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        Some(1_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(rogue_genesis.genesis_block.shard_id.clone()),
    )
    .unwrap();
    let rogue_block = rogue_node.add_block_from_deploys(&[deploy]).await.unwrap();

    // Phase 1: rogue -> main.
    // check_if_of_interest passes (same shard_id), well-formed check passes, then
    // check_dependencies_with_effects finds the parent (rogue genesis hash) missing
    // from main's store -> returns false -> Either::Left(missing_blocks).
    let rogue_to_main_status = main_nodes[0]
        .process_block(rogue_block.clone())
        .await
        .unwrap();

    assert!(
        matches!(rogue_to_main_status, Either::Left(_)),
        "main network must not accept a block whose parent traces to a foreign genesis; got {:?}",
        rogue_to_main_status
    );
    assert!(
        !main_nodes[0].casper.dag_contains(&rogue_block.block_hash),
        "foreign block must not be present in the main network's DAG"
    );

    // Phase 2: main -> rogue (symmetric rejection).
    // The rogue node's store only knows its own genesis. A block from the main
    // network has main_genesis_hash as its parent — unknown to the rogue store.
    // Same missing-parent path fires in the opposite direction.
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    let main_deploy = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        Some(1_000_000),
        None,
        Some(construct_deploy::DEFAULT_SEC.clone()),
        None,
        Some(main_shard_id.clone()),
    )
    .unwrap();
    let main_block = main_nodes[0]
        .add_block_from_deploys(&[main_deploy])
        .await
        .unwrap();

    let main_to_rogue_status = rogue_node.process_block(main_block.clone()).await.unwrap();

    assert!(
        matches!(main_to_rogue_status, Either::Left(_)),
        "rogue node must not accept a block whose parent traces to the main genesis; got {:?}",
        main_to_rogue_status
    );
    assert!(
        !rogue_node.casper.dag_contains(&main_block.block_hash),
        "main-network block must not be present in the rogue node's DAG"
    );
}
