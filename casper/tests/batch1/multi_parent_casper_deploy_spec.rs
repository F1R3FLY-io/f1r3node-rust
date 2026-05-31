// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperDeploySpec.scala

use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::casper::Casper;
use casper::rust::util::construct_deploy;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn multi_parent_casper_should_accept_a_deploy_and_return_its_id() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let node = TestNode::standalone(genesis.clone()).await.unwrap();

    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    let result = node.casper.deploy(deploy.clone());

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    // Scala: deployId = res.right.get
    let deploy_id_either = result.unwrap();
    let deploy_id = match deploy_id_either {
        Either::Right(id) => id,
        Either::Left(err) => {
            panic!("Deploy returned error: {:?}", err)
        }
    };

    assert_eq!(deploy_id, deploy.sig.to_vec());
}

#[tokio::test]
async fn multi_parent_casper_should_not_create_a_block_with_a_repeated_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();

    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    // Scala: node0.propagateBlock(deploy)(node1)
    // node0 propagates block with deploy to node1 only
    let _block = {
        let (node0_slice, rest) = nodes.split_at_mut(1);
        let node1_slice = &mut rest[0..1];
        let mut nodes_for_propagate: Vec<&mut TestNode> = node1_slice.iter_mut().collect();
        node0_slice[0]
            .propagate_block(&[deploy.clone()], &mut nodes_for_propagate)
            .await
            .unwrap()
    };

    // Scala: node1.createBlock(deploy)
    // node1 tries to create block with the same deploy
    let create_block_result2 = nodes[1].create_block(&[deploy.clone()]).await.unwrap();

    // Should return NoNewDeploys since deploy was already used
    assert!(
        matches!(create_block_result2, BlockCreatorResult::NoNewDeploys),
        "Expected NoNewDeploys, got: {:?}",
        create_block_result2
    );
}

// D3 (DR-9, OD-1): `multi_parent_casper_should_fail_when_deploying_with_insufficient_phlos`
// is REMOVED — accepted deploys run UNMETERED-FOR-LIVENESS (no per-deploy
// phlo_limit cap), so a low budget no longer aborts an accepted deploy with
// out-of-phlogistons. Fundedness is proven by the per-signature acceptance gate
// (covered by `funded_unfunded_boundary_at_margin` / `drained_present_pool_rejects`).

#[tokio::test]
async fn multi_parent_casper_should_succeed_if_given_enough_phlos_for_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();

    let deploy_data = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        Some(100),
        None,
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .unwrap();

    let result = node.create_block(&[deploy_data]).await.unwrap();

    let block = match result {
        BlockCreatorResult::Created(b, ..) => b,
        other => panic!("Expected Created block, got: {:?}", other),
    };

    // Scala: assert(!block.body.deploys.head.isFailed)
    assert!(
        !block.body.deploys.is_empty(),
        "Block should have at least one deploy"
    );
    assert!(
        !block.body.deploys[0].is_failed,
        "Deploy should succeed with sufficient phlos"
    );
}

// D3 (DR-9, D.5): `multi_parent_casper_should_reject_deploy_with_phlo_price_lower_than_min_phlo_price`
// is REMOVED — a deploy carries no `phlo_price`, and the per-deploy
// `validate_phlo` min-price SUBMISSION check is deleted. `min_phlo_price` is
// RETAINED as the block-assembly acceptance gate's safety MARGIN (not an API
// admission check); the margin boundary is covered by
// `funded_unfunded_boundary_at_margin` (acceptance.rs).
