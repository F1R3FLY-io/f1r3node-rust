// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperDeploySpec.scala

use std::collections::HashMap;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::ProposerResult;
use casper::rust::casper::{Casper, DeployError, MultiParentCasper};
// Changed by DR-134 (DR-66 item 1): offered deploys.
// use casper::rust::util::construct_deploy;
use casper::rust::util::construct_deploy::DEFAULT_PUB;
use casper::rust::ProposeFunction;
use crypto::rust::public_key::PublicKey;
use models::rust::block_metadata::BlockMetadata;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::offered_deploy as construct_deploy;
use crate::helper::offered_deploy::{owner_offer_limited, OfferedDeploy};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// Added by DR-134 (DR-66 item 1): the wall-clock timestamp of an offer that
/// signs its own phlo limit, as `source_deploy_now_full` takes it.
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_millis() as i64
}

#[tokio::test]
async fn validator_bond_check_uses_selected_parent_metadata() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut snapshot = node.casper.get_snapshot().await.unwrap();
    let validator = node.casper.get_validator().unwrap().public_key.bytes;
    let bonded_parent = snapshot.parents[0].clone();
    let mut unbonded_parent = bonded_parent.clone();
    unbonded_parent.block_hash = Bytes::from(vec![42; 32]);
    unbonded_parent.body.state.bonds.clear();

    snapshot
        .dag
        .dag_set
        .insert(unbonded_parent.block_hash.clone());
    snapshot
        .dag
        .block_metadata_index
        .write()
        .add(BlockMetadata::from_block(
            &unbonded_parent,
            false,
            None,
            None,
        ))
        .unwrap();

    snapshot.parents = vec![unbonded_parent.clone()];
    assert!(!snapshot.is_validator_bonded(&validator).unwrap());

    snapshot.last_finalized_block = unbonded_parent.block_hash;
    snapshot.parents = vec![bonded_parent];
    assert!(snapshot.is_validator_bonded(&validator).unwrap());
}

#[tokio::test]
async fn selected_parent_metadata_matches_block_bonds_after_admission() {
    fn assert_bonds_match(snapshot: &casper::rust::casper::CasperSnapshot) {
        let parent = snapshot.parents.first().unwrap();
        let metadata = snapshot.dag.lookup_unsafe(&parent.block_hash).unwrap();
        let bonds = parent
            .body
            .state
            .bonds
            .iter()
            .map(|bond| (bond.validator.clone(), bond.stake))
            .collect();
        assert_eq!(metadata.weight_map, bonds);
    }

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
    assert_bonds_match(&node.casper.get_snapshot().await.unwrap());

    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let block = node.add_block_from_deploys(&[deploy]).await.unwrap();
    let snapshot = node.casper.get_snapshot().await.unwrap();
    assert_eq!(snapshot.parents[0].block_hash, block.block_hash);
    assert_bonds_match(&snapshot);
}

#[tokio::test]
async fn block_api_rejects_deploy_from_unbonded_validator() {
    fn bonds_without_first(validators: Vec<PublicKey>) -> HashMap<PublicKey, i64> {
        GenesisBuilder::create_bonds(validators.into_iter().skip(1).collect())
    }

    let parameters =
        GenesisBuilder::build_genesis_parameters_with_defaults(Some(bonds_without_first), None);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    let result = BlockAPI::deploy_offered(
        &node.engine_cell,
        deploy.envelope.to_proto().unwrap(),
        &None,
        false,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("validator is not bonded"));
    assert!(!node
        .deploy_storage
        .lock()
        .contains_sig(&deploy.sig)
        .unwrap());
}

#[tokio::test]
async fn block_api_rejects_deploy_from_keyless_read_only_node() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let nodes = TestNode::create_network(genesis.clone(), 1, None, None, None, Some(1))
        .await
        .unwrap();
    let observer = &nodes[1];
    assert!(observer.casper.get_validator().is_none());
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    let result = BlockAPI::deploy_offered(
        &observer.engine_cell,
        deploy.envelope.to_proto().unwrap(),
        &None,
        true,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("node is running in read-only mode"));
    assert!(!observer
        .deploy_storage
        .lock()
        .contains_sig(&deploy.sig)
        .unwrap());
}

#[tokio::test]
async fn block_api_accepts_deploy_from_bonded_validator_without_autopropose() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    let result = BlockAPI::deploy_offered(
        &node.engine_cell,
        deploy.envelope.to_proto().unwrap(),
        &None,
        false,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result.is_ok(), "{result:?}");
    assert!(node
        .deploy_storage
        .lock()
        .contains_sig(&deploy.sig)
        .unwrap());
}

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

    // Changed by DR-134 (DR-66 item 1): TestNode::deploy submits an offered
    // envelope.
    // let result = node.casper.deploy(deploy.clone());
    let result = node.deploy(deploy.clone());

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
async fn multi_parent_casper_should_reject_concurrent_identical_deploys() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let expected_id = deploy.sig.to_vec();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));

    let handles = (0..32)
        .map(|_| {
            let casper = node.casper.clone();
            let deploy = deploy.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                // Changed by DR-134 (DR-66 item 1): the offered envelope goes
                // through the policy that the node adopted at start, as
                // TestNode::deploy does.
                // casper.deploy(deploy).unwrap()
                let policy = casper
                    .adopted_resource_policy
                    .as_ref()
                    .expect("an offered genesis adopts a resource policy");
                casper.deploy_envelope(deploy.envelope, policy).unwrap()
            })
        })
        .collect::<Vec<_>>();

    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    let accepted = results
        .iter()
        .filter(|result| matches!(result, Either::Right(id) if id == &expected_id))
        .count();
    let duplicates = results
        .iter()
        .filter(|result| {
            matches!(result, Either::Left(DeployError::DuplicateDeploy(id)) if id == &expected_id)
        })
        .count();

    assert_eq!(accepted, 1);
    assert_eq!(duplicates, 31);
    assert!(node
        .deploy_storage
        .lock()
        .contains_sig(&expected_id)
        .unwrap());
}

#[tokio::test]
async fn block_api_should_not_trigger_propose_for_a_duplicate_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    // Changed by DR-134 (DR-66 item 1): TestNode::deploy submits an offered
    // envelope.
    // let first = node.casper.deploy(deploy.clone()).unwrap();
    let first = node.deploy(deploy.clone()).unwrap();
    assert!(matches!(first, Either::Right(_)));

    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_trigger = calls.clone();
    let trigger: std::sync::Arc<ProposeFunction> = std::sync::Arc::new(move |_, _| {
        let calls = calls_for_trigger.clone();
        Box::pin(async move {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ProposerResult::Empty)
        })
    });

    let expected_id = deploy.sig.to_vec();
    // Changed by DR-134 (DR-66 item 1): the offered API takes the signed
    // offer and checks duplicates as the legacy API did.
    // let result = BlockAPI::deploy_offered(
    //     &node.engine_cell,
    //     deploy,
    //     &Some(trigger),
    //     false,
    //     &genesis.genesis_block.shard_id,
    // )
    // .await;
    let result = BlockAPI::deploy_offered(
        &node.engine_cell,
        deploy
            .envelope
            .to_proto()
            .expect("an offered test deploy encodes"),
        &Some(trigger),
        false,
        &genesis.genesis_block.shard_id,
    )
    .await;

    let error = result.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<DeployError>(),
        Some(DeployError::DuplicateDeploy(id)) if id == &expected_id
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn multi_parent_casper_should_reject_a_deploy_already_in_the_dag() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let expected_id = deploy.sig.to_vec();

    node.add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .unwrap();
    // Changed by DR-134 (DR-66 item 1): the pool removes an offered envelope by
    // its deploy id.
    // node.deploy_storage
    //     .lock()
    //     .remove(vec![deploy.clone()])
    //     .unwrap();
    node.deploy_storage
        .lock()
        .remove_by_sig(&deploy.sig)
        .unwrap();

    assert!(node
        .block_dag_storage
        .deploy_canonical_appearance(&expected_id)
        .unwrap()
        .is_some());
    // Changed by DR-134 (DR-66 item 1): TestNode::deploy submits an offered
    // envelope.
    // assert!(matches!(
    //     node.casper.deploy(deploy).unwrap(),
    //     Either::Left(DeployError::DuplicateDeploy(id)) if id == expected_id
    // ));
    assert!(matches!(
        node.deploy(deploy).unwrap(),
        Either::Left(DeployError::DuplicateDeploy(id)) if id == expected_id
    ));
}

#[tokio::test]
async fn multi_parent_casper_should_reject_a_deploy_in_the_rejected_buffer() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let expected_id = deploy.sig.to_vec();

    // Changed by DR-134 (DR-66 item 1): the rejected buffer holds offered
    // envelopes.
    // node.rejected_deploy_buffer
    //     .lock()
    //     .unwrap()
    //     .add(vec![deploy.clone()])
    //     .unwrap();
    node.rejected_deploy_buffer
        .lock()
        .unwrap()
        .add_envelopes(std::slice::from_ref(&deploy.envelope))
        .unwrap();

    // Changed by DR-134 (DR-66 item 1): TestNode::deploy submits an offered
    // envelope.
    // assert!(matches!(
    //     node.casper.deploy(deploy).unwrap(),
    //     Either::Left(DeployError::DuplicateDeploy(id)) if id == expected_id
    // ));
    assert!(matches!(
        node.deploy(deploy).unwrap(),
        Either::Left(DeployError::DuplicateDeploy(id)) if id == expected_id
    ));
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
            .propagate_block(std::slice::from_ref(&deploy), &mut nodes_for_propagate)
            .await
            .unwrap()
    };

    // Scala: node1.createBlock(deploy)
    // node1 tries to create block with the same deploy
    let create_block_result2 = nodes[1]
        .create_block(std::slice::from_ref(&deploy))
        .await
        .unwrap();

    // Should return NoNewDeploys since deploy was already used
    assert!(
        matches!(create_block_result2, BlockCreatorResult::NoNewDeploys),
        "Expected NoNewDeploys, got: {:?}",
        create_block_result2
    );
}

#[tokio::test]
async fn multi_parent_casper_should_fail_when_deploying_with_insufficient_phlos() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();

    // Changed by DR-134 (DR-66 item 1): an offer signs its own phlo limit. A
    // Nil term costs 0 under the native meter (COMMs and bytes), so no limit
    // is insufficient for it. The term therefore loops until it exhausts its
    // signed limit, which is a charged user failure (DR-113). The limit is
    // 20,000, as in
    // `signed_limit_exhaustion_is_published_as_a_charged_user_failure`
    // (casper/tests/api/offered_funded_api_test.rs). A limit of 1 is below the
    // estimate of the contract and the first send of the loop, and the check
    // refuses an offer whose estimate passes its signed limit before
    // execution, so no block holds it (`decide` in
    // casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/gate.rs,
    // P1:2210-2212).
    // let deploy_data = construct_deploy::source_deploy_now_full(
    //     "Nil".to_string(),
    //     Some(1),
    //     None,
    //     None,
    //     None,
    //     Some(genesis.genesis_block.shard_id.clone()),
    // )
    // .unwrap();
    let phlo_limit = 20_000;
    let deploy_data = OfferedDeploy::from_proto(owner_offer_limited(
        &(construct_deploy::DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
        now_millis(),
        "new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) }".to_string(),
        0,
        genesis.genesis_block.shard_id.clone(),
        phlo_limit,
    ));

    let result = node.create_block(&[deploy_data]).await.unwrap();

    let block = match result {
        BlockCreatorResult::Created(b, ..) => b,
        other => panic!("Expected Created block, got: {:?}", other),
    };

    assert!(
        !block.body.deploys.is_empty(),
        "Block should have at least one deploy"
    );
    assert!(
        block.body.deploys[0].is_failed(),
        "Deploy should be marked as failed due to insufficient phlos"
    );
    // Changed by DR-134 (DR-66 item 1): the native meter denies the first
    // charge that does not fit the signed limit (`reserve_native_usage` in
    // rholang/src/rust/interpreter/accounting/native_phlo_rules/execution.rs),
    // so the failed deploy pays the work granted up to the limit.
    // assert_eq!(block.body.deploys[0].cost().cost, 1);
    let cost = block.body.deploys[0].cost().cost;
    assert!(
        cost > 0 && cost <= phlo_limit,
        "Deploy should pay the work granted up to its signed limit ({phlo_limit}), got {cost}"
    );
}

#[tokio::test]
async fn multi_parent_casper_should_succeed_if_given_enough_phlos_for_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();

    // Changed by DR-134 (DR-66 item 1): an offer signs its own phlo limit.
    // let deploy_data = construct_deploy::source_deploy_now_full(
    //     "Nil".to_string(),
    //     Some(100),
    //     None,
    //     None,
    //     None,
    //     Some(genesis.genesis_block.shard_id.clone()),
    // )
    // .unwrap();
    let deploy_data = OfferedDeploy::from_proto(owner_offer_limited(
        &(construct_deploy::DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
        now_millis(),
        "Nil".to_string(),
        0,
        genesis.genesis_block.shard_id.clone(),
        100,
    ));

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
        !block.body.deploys[0].is_failed(),
        "Deploy should succeed with sufficient phlos"
    );
    // Changed by DR-134 (DR-66 item 1): the native meter measures COMMs and
    // bytes, and a Nil deploy has neither, so its measured phlo is 0.
    // assert_eq!(block.body.deploys[0].cost().cost, 3);
    assert_eq!(block.body.deploys[0].cost().cost, 0);
}

// Disabled by DR-134 (DR-66 item 3, user answer Q6 "Delete"): this test pins
// the cost that the legacy meter charges for one matched COMM (443 phlo). The
// cost-accounting branch added it to keep the historical cost of legacy
// deploys (commit 068d51c65), and dev has no such test. The legacy meter and
// body-only deploys are deleted. An offered deploy of this term pays for its
// COMM and its storage bytes under the native meter, and no publication pins
// that cost.
// #[tokio::test]
// async fn legacy_matched_comm_keeps_exact_historical_cost() {
//     let genesis = GenesisBuilder::new()
//         .build_genesis_with_parameters(None)
//         .await
//         .expect("Failed to build genesis");
//
//     let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
//     let deploy = construct_deploy::source_deploy_now_full(
//         "new x in { x!(1) | for (@n <- x) { Nil } }".to_string(),
//         Some(1_000_000),
//         None,
//         None,
//         None,
//         Some(genesis.genesis_block.shard_id.clone()),
//     )
//     .unwrap();
//
//     let block = node.add_block_from_deploys(&[deploy]).await.unwrap();
//
//     assert_eq!(block.body.deploys.len(), 1);
//     assert!(!block.body.deploys[0].is_failed());
//     assert_eq!(block.body.deploys[0].cost().cost, 443);
// }

#[tokio::test]
async fn multi_parent_casper_should_reject_deploy_with_phlo_price_lower_than_min_phlo_price() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let node = TestNode::standalone(genesis.clone()).await.unwrap();

    // The API reads the floor from the running casper's shard conf (1 in the
    // test fixture), so an underpriced deploy is one below that value.
    let min_phlo_price = 1i64;
    let phlo_price = 0i64;
    let is_node_read_only = false;
    let shard_id = genesis.genesis_block.shard_id.clone();

    // Changed by DR-134 (DR-66 item 1): an offer signed at a price below the
    // adopted minimum goes through the offered deploy API.
    // let deploy_data = construct_deploy::source_deploy_now_full(
    //     "Nil".to_string(),
    //     None,
    //     Some(phlo_price),
    //     None,
    //     None,
    //     Some(shard_id.clone()),
    // )
    // .unwrap();
    //
    // // Scala: BlockAPI.deploy[Effect](deployData, None, minPhloPrice = minPhloPrice, isNodeReadOnly, shardId = SHARD_ID)
    // let result = BlockAPI::deploy_offered(
    //     &node.engine_cell,
    //     deploy_data,
    //     &None,
    //     is_node_read_only,
    //     &shard_id,
    // )
    // .await;
    let offer = crate::helper::offered_deploy::owner_offer_priced(
        &(construct_deploy::DEFAULT_SEC.clone(), DEFAULT_PUB.clone()),
        now_millis(),
        "Nil".to_string(),
        phlo_price as u64,
    );
    let result = BlockAPI::deploy_offered(
        &node.engine_cell,
        offer,
        &None,
        is_node_read_only,
        &shard_id,
    )
    .await;

    // Scala: err.isLeft shouldBe true
    assert!(
        result.is_err(),
        "Deploy with low phloPrice should be rejected"
    );

    // Scala: ex shouldBe a[RuntimeException]
    // Scala: ex.getMessage shouldBe s"Phlo price $phloPrice is less than minimum price $minPhloPrice."
    let error = result.unwrap_err();
    let error_message = format!("{:?}", error);
    // Changed by DR-134 (DR-66 item 1): the offered API's minimum-price message.
    // let expected_message = format!(
    //     "Phlo price {} is less than minimum price {}",
    //     phlo_price, min_phlo_price
    // );
    assert!(
        phlo_price < min_phlo_price,
        "the offer is priced below the minimum"
    );
    let expected_message = "offered phloPrice is below the adopted minimum".to_string();
    assert!(
        error_message.contains(&expected_message),
        "Error message should contain: '{}'\nGot: {}",
        expected_message,
        error_message
    );
}
