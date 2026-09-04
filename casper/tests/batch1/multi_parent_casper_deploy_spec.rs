// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperDeploySpec.scala

use casper::rust::api::block_api::BlockAPI;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::ProposerResult;
use casper::rust::casper::{Casper, DeployError};
use casper::rust::util::construct_deploy;
use casper::rust::ProposeFunction;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn multi_parent_casper_should_reject_legacy_ingress_without_poisoning_the_v6_pool() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let node = TestNode::standalone(genesis.clone()).await.unwrap();

    let deploy =
        construct_deploy::basic_deploy_data(0, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();

    let legacy_result = node.casper.deploy(deploy.clone()).unwrap();
    assert!(matches!(
        legacy_result,
        Either::Left(DeployError::ParsingError(message))
            if message.contains("protocol-v6 admission requires")
    ));
    assert!(node.deploy_storage.lock().read_all().unwrap().is_empty());
    assert!(node
        .deploy_storage
        .lock()
        .read_all_envelopes()
        .unwrap()
        .is_empty());

    let envelope = node.envelope_for_deploy(&deploy).unwrap();
    let expected_id = envelope.envelope_commitment().unwrap().to_vec();
    let deploy_id = match node.casper.deploy_cosigned(envelope).unwrap() {
        Either::Right(id) => id,
        Either::Left(err) => {
            panic!("Deploy returned error: {:?}", err)
        }
    };
    assert_eq!(deploy_id, expected_id);
    assert!(node.deploy_storage.lock().read_all().unwrap().is_empty());
    assert_eq!(
        node.deploy_storage
            .lock()
            .read_all_envelopes()
            .unwrap()
            .len(),
        1
    );
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
    let envelope = node.envelope_for_deploy(&deploy).unwrap();
    let expected_id = envelope.envelope_commitment().unwrap().to_vec();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));

    let handles = (0..32)
        .map(|_| {
            let casper = node.casper.clone();
            let envelope = envelope.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                casper.deploy_cosigned(envelope).unwrap()
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
        .contains_envelope(&expected_id)
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
    let envelope = node.envelope_for_deploy(&deploy).unwrap();
    let first = node.casper.deploy_cosigned(envelope.clone()).unwrap();
    assert!(matches!(first, Either::Right(_)));

    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_trigger = calls.clone();
    let trigger: std::sync::Arc<ProposeFunction> = std::sync::Arc::new(move |_| {
        let calls = calls_for_trigger.clone();
        Box::pin(async move {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ProposerResult::Empty)
        })
    });

    let expected_id = envelope.envelope_commitment().unwrap().to_vec();
    let result = BlockAPI::deploy_cosigned(
        &node.engine_cell,
        envelope,
        &Some(trigger),
        0,
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
    let envelope = node.envelope_for_deploy(&deploy).unwrap();
    let expected_id = envelope.envelope_commitment().unwrap().to_vec();

    node.add_block_from_deploys(std::slice::from_ref(&deploy))
        .await
        .unwrap();
    node.deploy_storage
        .lock()
        .remove_envelope_by_id(&expected_id)
        .unwrap();

    assert!(node
        .block_dag_storage
        .deploy_canonical_appearance(
            &models::rust::deploy_id::DeployLookupId::from_protocol_bytes(6, &expected_id).unwrap(),
        )
        .unwrap()
        .is_some());
    assert!(matches!(
        node.submit_deploy(deploy).unwrap(),
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
    let envelope = node.envelope_for_deploy(&deploy).unwrap();
    let expected_id = envelope.envelope_commitment().unwrap().to_vec();

    node.rejected_deploy_buffer
        .lock()
        .unwrap()
        .add(vec![crate::pending_envelope(envelope)])
        .unwrap();

    assert!(matches!(
        node.submit_deploy(deploy).unwrap(),
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

// D3 (DR-9, refined by DR-31): the client-selected phlo limit no longer controls
// admission or execution. Production derives a finite capacity from authenticated
// authority, and exhaustion rejects before a deployment can certify.

#[tokio::test]
async fn multi_parent_casper_should_succeed_with_authority_funded_deploy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();

    let deploy_data = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        None,
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
        "Authority-funded deploy should succeed"
    );
}

// D3 (DR-9, D.5): `multi_parent_casper_should_reject_deploy_with_phlo_price_lower_than_min_phlo_price`
// is REMOVED — a deploy carries no `phlo_price`, and the per-deploy
// `validate_phlo` min-price SUBMISSION check is deleted. `min_phlo_price` is
// RETAINED as the block-assembly acceptance gate's safety MARGIN (not an API
// admission check); the margin boundary is covered by
// `funded_unfunded_boundary_at_margin` (acceptance.rs).
