// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperDeploySpec.scala

use std::collections::HashMap;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::ProposerResult;
use casper::rust::casper::{Casper, DeployError, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::ProposeFunction;
use crypto::rust::public_key::PublicKey;
use models::rust::block_metadata::BlockMetadata;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn direct_deploy_refuses_invalid_signatures_without_persisting_them() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut deploy =
        construct_deploy::basic_deploy_data(41, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let signature = deploy.sig.clone();
    deploy.data.phlo_limit += 1;

    assert!(matches!(
        node.casper.deploy(deploy).unwrap(),
        Either::Left(DeployError::SignatureVerificationFailed)
    ));
    assert!(!node.deploy_storage.contains_sig(&signature).unwrap());
    assert!(!node
        .casper
        .rejected_deploy_buffer
        .lock()
        .unwrap()
        .contains_sig(&signature)
        .unwrap());
}

#[tokio::test]
async fn direct_deploy_accepts_after_quarantining_a_damaged_pool_copy() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let deploy =
        construct_deploy::basic_deploy_data(44, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let signature = deploy.sig.to_vec();
    let key = node.deploy_storage.store.encode_key(&signature).unwrap();
    node.deploy_storage
        .store
        .raw_store()
        .put_one(key, vec![0xff, 0x01])
        .unwrap();

    assert!(matches!(node.casper.deploy(deploy.clone()).unwrap(),
        Either::Right(id) if id == signature));
    assert!(node.deploy_storage.contains_sig(&signature).unwrap());
    assert!(node.deploy_storage.read_all().unwrap().contains(&deploy));
}

#[tokio::test]
async fn direct_deploy_preserves_duplicate_priority_for_an_already_stored_signature() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut deploy =
        construct_deploy::basic_deploy_data(42, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let signature = deploy.sig.to_vec();
    node.casper
        .rejected_deploy_buffer
        .lock()
        .unwrap()
        .add(vec![deploy.clone()])
        .unwrap();
    deploy.data.phlo_limit += 1;

    assert!(matches!(node.casper.deploy(deploy).unwrap(),
        Either::Left(DeployError::DuplicateDeploy(id)) if id == signature));
}

#[tokio::test]
async fn direct_deploy_checks_a_cold_buffer_record_without_holding_its_mutex() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock, Weak};

    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueRejectedDeployBuffer;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use prost::Message;
    use shared::rust::store::key_value_store::KvStoreError;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    static BUFFER: OnceLock<Weak<Mutex<KeyValueRejectedDeployBuffer>>> = OnceLock::new();
    static DECODED: AtomicUsize = AtomicUsize::new(0);

    fn decode(bytes: &[u8]) -> Result<Signed<DeployData>, KvStoreError> {
        let buffer = BUFFER.get().unwrap().upgrade().unwrap();
        assert!(buffer.try_lock().is_ok());
        DECODED.fetch_add(1, Ordering::SeqCst);
        let proto = models::casper::DeployDataProto::decode(&bytes[b"F1R3DEP\x01".len()..])
            .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
        DeployData::from_proto_unverified(proto).map_err(KvStoreError::SerializationError)
    }

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut candidate =
        construct_deploy::basic_deploy_data(45, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap();
    let signature = candidate.sig.to_vec();
    let buffer = node.casper.rejected_deploy_buffer.clone();
    assert!(BUFFER.set(Arc::downgrade(&buffer)).is_ok());
    {
        let mut guard = buffer.lock().unwrap();
        guard.store = KeyValueTypedStoreImpl::with_value_codec(
            guard.store.raw_store().clone(),
            |deploy| {
                let mut bytes = b"F1R3DEP\x01".to_vec();
                DeployData::to_proto_ref(deploy)
                    .encode(&mut bytes)
                    .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
                Ok(bytes)
            },
            |bytes| decode(bytes),
        );
        guard
            .store
            .put_one(signature.clone(), candidate.clone())
            .unwrap();
    }
    candidate.data.phlo_limit += 1;

    assert!(matches!(node.casper.deploy(candidate).unwrap(),
        Either::Left(DeployError::DuplicateDeploy(id)) if id == signature));
    assert_eq!(DECODED.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn direct_deploy_refuses_an_algorithm_that_the_store_cannot_decode() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let data =
        construct_deploy::basic_deploy_data(43, None, Some(genesis.genesis_block.shard_id.clone()))
            .unwrap()
            .data;
    let deploy = crypto::rust::signatures::signed::Signed::create(
        data,
        Box::new(crypto::rust::signatures::ed25519::Ed25519),
        crypto::rust::private_key::PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    assert!(deploy.verify());
    let signature = deploy.sig.clone();

    assert!(matches!(node.casper.deploy(deploy).unwrap(),
        Either::Left(DeployError::UnknownSignatureAlgorithm(algorithm)) if algorithm == "ed25519"));
    assert!(!node.deploy_storage.contains_sig(&signature).unwrap());
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

    let result = BlockAPI::deploy(
        &node.engine_cell,
        deploy.clone(),
        &None,
        false,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("validator is not bonded"));
    assert!(!node.deploy_storage.contains_sig(&deploy.sig).unwrap());
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

    let result = BlockAPI::deploy(
        &observer.engine_cell,
        deploy.clone(),
        &None,
        true,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("node is running in read-only mode"));
    assert!(!observer.deploy_storage.contains_sig(&deploy.sig).unwrap());
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

    let result = BlockAPI::deploy(
        &node.engine_cell,
        deploy.clone(),
        &None,
        false,
        &genesis.genesis_block.shard_id,
    )
    .await;

    assert!(result.is_ok(), "{result:?}");
    assert!(node.deploy_storage.contains_sig(&deploy.sig).unwrap());
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
                casper.deploy(deploy).unwrap()
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
    assert!(node.deploy_storage.contains_sig(&expected_id).unwrap());
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
    let first = node.casper.deploy(deploy.clone()).unwrap();
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
    let result = BlockAPI::deploy(
        &node.engine_cell,
        deploy,
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
    node.deploy_storage.remove(vec![deploy.clone()]).unwrap();

    assert!(node
        .block_dag_storage
        .deploy_canonical_appearance(&expected_id)
        .unwrap()
        .is_some());
    assert!(matches!(
        node.casper.deploy(deploy).unwrap(),
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

    node.rejected_deploy_buffer
        .lock()
        .unwrap()
        .add(vec![deploy.clone()])
        .unwrap();

    assert!(matches!(
        node.casper.deploy(deploy).unwrap(),
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

    let deploy_data = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        Some(1),
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

    assert!(
        !block.body.deploys.is_empty(),
        "Block should have at least one deploy"
    );
    assert!(
        block.body.deploys[0].is_failed,
        "Deploy should be marked as failed due to insufficient phlos"
    );
}

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

    let deploy_data = construct_deploy::source_deploy_now_full(
        "Nil".to_string(),
        None,
        Some(phlo_price),
        None,
        None,
        Some(shard_id.clone()),
    )
    .unwrap();

    // Scala: BlockAPI.deploy[Effect](deployData, None, minPhloPrice = minPhloPrice, isNodeReadOnly, shardId = SHARD_ID)
    let result = BlockAPI::deploy(
        &node.engine_cell,
        deploy_data,
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
    let expected_message = format!(
        "Phlo price {} is less than minimum price {}",
        phlo_price, min_phlo_price
    );
    assert!(
        error_message.contains(&expected_message),
        "Error message should contain: '{}'\nGot: {}",
        expected_message,
        error_message
    );
}
