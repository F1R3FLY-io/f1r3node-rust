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

fn queued_deploy(
    timestamp: i64,
    shard: &str,
) -> crypto::rust::signatures::signed::Signed<
    models::rust::casper::protocol::casper_message::DeployData,
> {
    construct_deploy::source_deploy(
        format!("@{}!({})", timestamp, timestamp),
        timestamp,
        None,
        None,
        None,
        None,
        Some(shard.to_string()),
    )
    .unwrap()
}

#[tokio::test]
async fn admission_refuses_position_twenty_five_without_a_proposal_and_recovers_after_drain() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut admitted = Vec::new();
    for timestamp in 0..24 {
        let deploy = queued_deploy(timestamp, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(deploy.clone()).unwrap(),
            Either::Right(_)
        ));
        admitted.push(deploy);
    }
    let refused = queued_deploy(100, &genesis.genesis_block.shard_id);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let trigger: std::sync::Arc<ProposeFunction> = std::sync::Arc::new(move |_, _| {
        observed_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok(ProposerResult::Empty) })
    });
    let error = BlockAPI::deploy(
        &node.engine_cell,
        refused.clone(),
        &Some(trigger),
        false,
        &genesis.genesis_block.shard_id,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<DeployError>(),
        Some(DeployError::PoolBusy {
            pending_ahead: 24,
            capacity: 24
        })
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!node
        .deploy_storage
        .as_ref()
        .contains_sig(&refused.sig)
        .unwrap());
    node.deploy_storage
        .as_ref()
        .remove(vec![admitted.remove(0)])
        .unwrap();
    assert!(matches!(
        node.casper.deploy(refused).unwrap(),
        Either::Right(_)
    ));
}

#[tokio::test]
async fn parallel_reservations_preserve_the_last_position_and_duplicate_priority() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    for timestamp in 0..23 {
        let deploy = queued_deploy(timestamp, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(deploy).unwrap(),
            Either::Right(_)
        ));
    }
    let deploy = queued_deploy(100, &genesis.genesis_block.shard_id);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));
    let handles: Vec<_> = (0..32)
        .map(|_| {
            let casper = node.casper.clone();
            let deploy = deploy.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                casper.deploy(deploy).unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Either::Right(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Either::Left(DeployError::DuplicateDeploy(_))))
            .count(),
        31
    );
    let next = queued_deploy(101, &genesis.genesis_block.shard_id);
    assert!(matches!(
        node.casper.deploy(next).unwrap(),
        Either::Left(DeployError::PoolBusy { .. })
    ));
}

#[tokio::test]
async fn included_pool_copies_do_not_block_fresh_admission() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
    let mut included = Vec::new();
    for timestamp in 0..8 {
        let deploy = queued_deploy(timestamp, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(deploy.clone()).unwrap(),
            Either::Right(_)
        ));
        included.push(deploy);
    }
    node.add_block_from_deploys(&included).await.unwrap();
    for timestamp in 100..124 {
        let deploy = queued_deploy(timestamp, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(deploy).unwrap(),
            Either::Right(_)
        ));
    }
    let deploy = queued_deploy(124, &genesis.genesis_block.shard_id);
    assert!(matches!(
        node.casper.deploy(deploy).unwrap(),
        Either::Left(DeployError::PoolBusy {
            pending_ahead: 24,
            capacity: 24
        })
    ));
}

#[tokio::test]
async fn expired_terminal_and_buffered_pool_copies_do_not_consume_admission_capacity() {
    use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Signed;

    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    let dag = node.block_dag_storage.get_representation().unwrap();
    let mut stored = Vec::new();
    let mut buffered = Vec::new();
    for index in 0..24 {
        let mut expired = queued_deploy(index, &genesis.genesis_block.shard_id).data;
        expired.expiration_timestamp = Some(1);
        stored.push(
            Signed::create(
                expired,
                Box::new(Secp256k1),
                construct_deploy::DEFAULT_SEC.clone(),
            )
            .unwrap(),
        );
        let terminal = queued_deploy(index + 100, &genesis.genesis_block.shard_id);
        dag.put_deploy_terminal_if_absent(&terminal.sig, TerminalRecord {
            state: TerminalState::Expired,
            rejection_count: 0,
            latest_height: 0,
            latest_block_hash: Vec::new(),
        })
        .unwrap();
        stored.push(terminal);
        let retry = queued_deploy(index + 200, &genesis.genesis_block.shard_id);
        buffered.push(retry.clone());
        stored.push(retry);
    }
    node.deploy_storage.as_ref().add(stored).unwrap();
    node.casper
        .rejected_deploy_buffer
        .lock()
        .unwrap()
        .add(buffered)
        .unwrap();
    let fresh = queued_deploy(1000, &genesis.genesis_block.shard_id);
    let result = node.casper.deploy(fresh).unwrap();
    assert!(matches!(result, Either::Right(_)), "{result:?}");
}

#[tokio::test]
async fn future_entries_reserve_capacity_before_ready_work() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let node = TestNode::standalone(genesis.clone()).await.unwrap();
    for timestamp in 0..25 {
        let deploy = construct_deploy::source_deploy(
            "Nil".to_string(),
            timestamp,
            None,
            None,
            None,
            Some(1000),
            Some(genesis.genesis_block.shard_id.clone()),
        )
        .unwrap();
        let result = node.casper.deploy(deploy).unwrap();
        if timestamp < 24 {
            assert!(matches!(result, Either::Right(_)));
        } else {
            assert!(matches!(
                result,
                Either::Left(DeployError::PoolBusy {
                    pending_ahead: 24,
                    capacity: 24
                })
            ));
        }
    }
    assert!(matches!(
        node.casper
            .deploy(queued_deploy(100, &genesis.genesis_block.shard_id))
            .unwrap(),
        Either::Left(DeployError::PoolBusy {
            pending_ahead: 0,
            capacity: 24
        })
    ));
}

#[tokio::test]
async fn peer_blocks_do_not_reduce_local_admission_capacity() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .unwrap();
    let mut nodes = TestNode::create_network(genesis.clone(), 2, None, None, None, None)
        .await
        .unwrap();
    let peer_deploy = queued_deploy(0, &genesis.genesis_block.shard_id);
    let (peer, local) = nodes.split_at_mut(1);
    peer[0]
        .propagate_block(&[peer_deploy], &mut [&mut local[0]])
        .await
        .unwrap();
    for timestamp in 100..124 {
        assert!(matches!(
            local[0]
                .casper
                .deploy(queued_deploy(timestamp, &genesis.genesis_block.shard_id))
                .unwrap(),
            Either::Right(_)
        ));
    }
    assert!(matches!(
        local[0]
            .casper
            .deploy(queued_deploy(124, &genesis.genesis_block.shard_id))
            .unwrap(),
        Either::Left(DeployError::PoolBusy { capacity: 24, .. })
    ));
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

mod admission_regressions {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Condvar, Mutex};
    use std::time::Duration;

    use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};
    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use casper::rust::casper::{Casper, DeployError};
    use casper::rust::util::construct_deploy;
    use crypto::rust::signatures::signed::Signed;
    use models::rust::casper::protocol::casper_message::DeployData;
    use rspace_plus_plus::rspace::history::Either;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
    use shared::rust::ByteBuffer;

    use crate::helper::test_node::TestNode;
    use crate::util::genesis_builder::GenesisBuilder;

    const CAPACITY: usize = 24;
    const CANDIDATES: usize = 32;

    fn signed_deploy(timestamp: i64, shard: &str) -> Signed<DeployData> {
        construct_deploy::source_deploy(
            format!("@{}!({})", timestamp, timestamp),
            timestamp,
            None,
            None,
            None,
            Some(0),
            Some(shard.to_owned()),
        )
        .unwrap()
    }

    fn height(node: &TestNode) -> i64 {
        node.block_dag_storage
            .get_representation()
            .unwrap()
            .latest_block_number()
    }

    #[tokio::test]
    async fn distinct_arrivals_respect_capacity_in_every_selection_order() {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let deploys: Vec<_> = (1..=100)
            .map(|timestamp| signed_deploy(timestamp, &genesis.genesis_block.shard_id))
            .collect();
        assert_eq!(
            deploys
                .iter()
                .map(|d| d.sig.clone())
                .collect::<HashSet<_>>()
                .len(),
            100
        );
        let ascending: Vec<usize> = (0..100).collect();
        let descending: Vec<usize> = (0..100).rev().collect();
        let mut random = ascending.clone();
        let mut state = 710_u64;
        for index in (1..random.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            random.swap(index, (state % (index + 1) as u64) as usize);
        }
        for order in [ascending, descending, random] {
            let node = TestNode::standalone(genesis.clone()).await.unwrap();
            let initial_height = height(&node);
            for (arrival, index) in order.iter().enumerate() {
                let deploy = &deploys[*index];
                let result = node.casper.deploy(deploy.clone()).unwrap();
                if arrival < CAPACITY {
                    assert!(matches!(result, Either::Right(_)), "{result:?}");
                } else {
                    assert!(
                        matches!(
                            result,
                            Either::Left(DeployError::PoolBusy {
                                capacity: CAPACITY,
                                ..
                            })
                        ),
                        "{result:?}"
                    );
                }
            }
            let pool = node.deploy_storage.as_ref();
            assert_eq!(pool.read_all().unwrap().len(), CAPACITY);
            for (arrival, index) in order.iter().enumerate() {
                assert_eq!(
                    pool.contains_sig(&deploys[*index].sig).unwrap(),
                    arrival < CAPACITY
                );
            }
            assert_eq!(height(&node), initial_height);
        }
    }

    #[tokio::test]
    async fn distinct_concurrent_arrivals_share_only_one_remaining_slot() {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let node = TestNode::standalone(genesis.clone()).await.unwrap();
        let initial_height = height(&node);
        let seed: Vec<_> = (0..23)
            .map(|timestamp| signed_deploy(timestamp, &genesis.genesis_block.shard_id))
            .collect();
        for deploy in &seed {
            assert!(matches!(
                node.casper.deploy(deploy.clone()).unwrap(),
                Either::Right(_)
            ));
        }
        let candidates: Vec<_> = (100..100 + CANDIDATES as i64)
            .map(|timestamp| signed_deploy(timestamp, &genesis.genesis_block.shard_id))
            .collect();
        assert_eq!(
            candidates
                .iter()
                .map(|d| d.sig.clone())
                .collect::<HashSet<_>>()
                .len(),
            CANDIDATES
        );

        let barrier = Arc::new(Barrier::new(CANDIDATES));
        let first_finished = Arc::new((Mutex::new(false), Condvar::new()));
        let handles: Vec<_> = candidates
            .into_iter()
            .enumerate()
            .map(|(index, deploy)| {
                let casper = node.casper.clone();
                let barrier = barrier.clone();
                let first_finished = first_finished.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let first = index == CANDIDATES - 1;
                    if !first {
                        let (lock, ready) = &*first_finished;
                        let (finished, _) = ready
                            .wait_timeout_while(
                                lock.lock().unwrap(),
                                Duration::from_secs(30),
                                |v| !*v,
                            )
                            .unwrap();
                        assert!(*finished, "The first admission did not complete");
                    }
                    let signature = deploy.sig.clone();
                    let result = casper.deploy(deploy);
                    if first {
                        let (lock, ready) = &*first_finished;
                        *lock.lock().unwrap() = true;
                        ready.notify_all();
                    }
                    (signature, result)
                })
            })
            .collect();
        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        for handle in handles {
            let (signature, result) = handle.join().unwrap();
            match result.unwrap() {
                Either::Right(id) => {
                    assert_eq!(id, signature.to_vec());
                    accepted.push(signature);
                }
                Either::Left(DeployError::PoolBusy { capacity, .. }) => {
                    assert_eq!(capacity, CAPACITY);
                    refused.push(signature);
                }
                other => panic!("Unexpected admission result: {other:?}"),
            }
        }
        assert_eq!(accepted.len() + refused.len(), CANDIDATES);
        assert_eq!(accepted.len(), 1);
        assert_eq!(refused.len(), CANDIDATES - 1);
        let storage = node.deploy_storage.as_ref();
        assert_eq!(
            storage.read_all().unwrap().len(),
            seed.len() + accepted.len()
        );
        for signature in &accepted {
            assert!(storage.contains_sig(signature).unwrap());
        }
        for signature in &refused {
            assert!(!storage.contains_sig(signature).unwrap());
        }
        for deploy in &seed {
            assert!(storage.contains_sig(&deploy.sig).unwrap());
        }
        assert_eq!(height(&node), initial_height);
        let duplicate = seed[0].clone();
        assert!(matches!(
            node.casper.deploy(duplicate).unwrap(),
            Either::Left(DeployError::DuplicateDeploy(_))
        ));
    }

    #[tokio::test]
    async fn future_deploys_reserve_capacity_even_for_an_earlier_ready_candidate() {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let node = TestNode::standalone(genesis.clone()).await.unwrap();
        for timestamp in 0..CAPACITY as i64 {
            let deploy = construct_deploy::source_deploy(
                "Nil".to_owned(),
                timestamp,
                None,
                None,
                None,
                Some(1000),
                Some(genesis.genesis_block.shard_id.clone()),
            )
            .unwrap();
            assert!(matches!(
                node.casper.deploy(deploy).unwrap(),
                Either::Right(_)
            ));
        }
        let candidate = signed_deploy(100, &genesis.genesis_block.shard_id);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let trigger: Arc<casper::rust::ProposeFunction> = Arc::new(move |_, _| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async { Ok(casper::rust::blocks::proposer::proposer::ProposerResult::Empty) })
        });
        let error = casper::rust::api::block_api::BlockAPI::deploy(
            &node.engine_cell,
            candidate.clone(),
            &Some(trigger),
            false,
            &genesis.genesis_block.shard_id,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<DeployError>(),
                Some(DeployError::PoolBusy {
                    pending_ahead: 0,
                    capacity: CAPACITY,
                })
            ),
            "{error:?}"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let message = error.to_string();
        assert!(message.contains("fresh queue"));
        assert!(!message.contains("queue position"));
        let pool = node.deploy_storage.as_ref();
        assert_eq!(pool.read_all().unwrap().len(), CAPACITY);
        assert!(!pool.contains_sig(&candidate.sig).unwrap());
    }

    #[tokio::test]
    async fn lower_service_preserves_reservations_and_admission_resumes_after_drain() {
        use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;

        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
        let included = signed_deploy(0, &genesis.genesis_block.shard_id);
        let block = match node.create_block(&[included]).await.unwrap() {
            BlockCreatorResult::Created(block, ..) => block,
            other => panic!("Expected a block: {other:?}"),
        };
        let seed: Vec<_> = (100..123)
            .map(|timestamp| signed_deploy(timestamp, &genesis.genesis_block.shard_id))
            .collect();
        for deploy in &seed {
            assert!(matches!(
                node.casper.deploy(deploy.clone()).unwrap(),
                Either::Right(_)
            ));
        }
        assert!(matches!(
            node.process_block(block).await.unwrap(),
            Either::Right(_)
        ));
        let initial_height = height(&node);
        let candidate = signed_deploy(1, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(candidate.clone()).unwrap(),
            Either::Left(DeployError::PoolBusy {
                pending_ahead: 0,
                capacity: 3
            })
        ));
        {
            let storage = node.deploy_storage.as_ref();
            for deploy in &seed {
                assert!(storage.contains_sig(&deploy.sig).unwrap());
            }
        }
        assert!(matches!(
            node.casper.deploy(seed[0].clone()).unwrap(),
            Either::Left(DeployError::DuplicateDeploy(_))
        ));
        node.deploy_storage
            .as_ref()
            .remove(seed[..21].to_vec())
            .unwrap();
        assert!(matches!(
            node.casper.deploy(candidate).unwrap(),
            Either::Right(_)
        ));
        assert!(matches!(
            node.casper
                .deploy(signed_deploy(2, &genesis.genesis_block.shard_id))
                .unwrap(),
            Either::Left(DeployError::PoolBusy { capacity: 3, .. })
        ));
        let storage = node.deploy_storage.as_ref();
        for deploy in &seed[21..] {
            assert!(storage.contains_sig(&deploy.sig).unwrap());
        }
        assert_eq!(height(&node), initial_height);
    }

    #[tokio::test]
    async fn zero_service_allows_one_probe_only_after_the_fresh_queue_drains() {
        use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;

        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
        node.allow_empty_blocks = true;
        let empty = match node.create_block(&[]).await.unwrap() {
            BlockCreatorResult::Created(block, ..) => block,
            other => panic!("Expected an empty block: {other:?}"),
        };
        let waiting = signed_deploy(100, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(waiting.clone()).unwrap(),
            Either::Right(_)
        ));
        assert!(matches!(
            node.process_block(empty).await.unwrap(),
            Either::Right(_)
        ));
        let candidate = signed_deploy(1, &genesis.genesis_block.shard_id);
        assert!(matches!(
            node.casper.deploy(candidate.clone()).unwrap(),
            Either::Left(DeployError::PoolBusy { capacity: 0, .. })
        ));
        assert!(node
            .deploy_storage
            .as_ref()
            .contains_sig(&waiting.sig)
            .unwrap());
        node.deploy_storage.as_ref().remove(vec![waiting]).unwrap();
        assert!(matches!(
            node.casper.deploy(candidate).unwrap(),
            Either::Right(_)
        ));
        assert!(matches!(
            node.casper
                .deploy(signed_deploy(2, &genesis.genesis_block.shard_id))
                .unwrap(),
            Either::Left(DeployError::PoolBusy { capacity: 0, .. })
        ));
    }

    #[tokio::test]
    async fn warm_admission_index_tracks_external_pool_and_lifecycle_changes() {
        use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};

        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let node = TestNode::standalone(genesis.clone()).await.unwrap();
        let shard = &genesis.genesis_block.shard_id;
        let seeded: Vec<_> = (0..24).map(|i| signed_deploy(i, shard)).collect();
        for deploy in &seeded {
            assert!(matches!(
                node.casper.deploy(deploy.clone()).unwrap(),
                Either::Right(_)
            ));
        }
        let candidate = signed_deploy(100, shard);
        assert!(matches!(
            node.casper.deploy(candidate.clone()).unwrap(),
            Either::Left(DeployError::PoolBusy { .. })
        ));
        let proposer = node.deploy_storage.as_ref().clone();
        proposer.remove_by_sig(&seeded[0].sig).unwrap();
        assert!(matches!(
            node.casper.deploy(candidate.clone()).unwrap(),
            Either::Right(_)
        ));
        proposer
            .store
            .put_one(seeded[0].sig.to_vec(), seeded[0].clone())
            .unwrap();
        assert!(matches!(
            node.casper.deploy(signed_deploy(101, shard)).unwrap(),
            Either::Left(DeployError::PoolBusy { .. })
        ));
        let generation = node.block_dag_storage.current_generation();
        let dag = node.block_dag_storage.get_representation().unwrap();
        for deploy in &seeded[0..2] {
            dag.put_deploy_terminal_if_absent(&deploy.sig, TerminalRecord {
                state: TerminalState::Expired,
                rejection_count: 0,
                latest_height: 0,
                latest_block_hash: Vec::new(),
            })
            .unwrap();
        }
        assert_eq!(node.block_dag_storage.current_generation(), generation);
        assert!(matches!(
            node.casper.deploy(signed_deploy(102, shard)).unwrap(),
            Either::Right(_)
        ));
        assert!(matches!(
            node.casper.deploy(signed_deploy(103, shard)).unwrap(),
            Either::Left(DeployError::PoolBusy { .. })
        ));
        node.casper
            .rejected_deploy_buffer
            .lock()
            .unwrap()
            .add(vec![seeded[2].clone()])
            .unwrap();
        assert!(matches!(
            node.casper.deploy(signed_deploy(104, shard)).unwrap(),
            Either::Right(_)
        ));
        node.casper
            .rejected_deploy_buffer
            .lock()
            .unwrap()
            .remove(vec![seeded[2].clone()])
            .unwrap();
        assert!(matches!(
            node.casper.deploy(signed_deploy(105, shard)).unwrap(),
            Either::Left(DeployError::PoolBusy { .. })
        ));
        assert!(proposer.contains_sig(&seeded[2].sig).unwrap());
    }

    #[tokio::test]
    async fn failed_persistence_releases_the_reserved_last_slot() {
        use casper::rust::engine::engine::noop;
        use shared::rust::store::key_value_store::KvStoreError;
        use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
        let shard = &genesis.genesis_block.shard_id;
        for timestamp in 0..23 {
            assert!(matches!(
                node.casper.deploy(signed_deploy(timestamp, shard)).unwrap(),
                Either::Right(_)
            ));
        }
        node.engine_cell.set(Arc::new(noop())).await;
        let original = node.deploy_storage.clone();
        let mut instrumented = original.as_ref().clone();
        instrumented.store = KeyValueTypedStoreImpl::with_value_codec(
            original.store.raw_store().clone(),
            |_| {
                Err(KvStoreError::IoError(
                    "Injected deploy persistence failure".into(),
                ))
            },
            |_| panic!("Warm admission must not decode the pool"),
        );
        node.deploy_storage = Arc::new(instrumented);
        Arc::get_mut(&mut node.casper)
            .expect("Admission fixture must own its Casper instance")
            .deploy_storage = node.deploy_storage.clone();

        let failed = signed_deploy(100, shard);
        let result = node.casper.deploy(failed.clone());
        node.deploy_storage = original.clone();
        Arc::get_mut(&mut node.casper).unwrap().deploy_storage = original;
        assert!(result.is_err());
        {
            let pool = node.deploy_storage.as_ref();
            assert!(!pool.contains_sig(&failed.sig).unwrap());
            assert_eq!(pool.read_all().unwrap().len(), 23);
        }
        assert!(matches!(
            node.casper.deploy(signed_deploy(101, shard)).unwrap(),
            Either::Right(_)
        ));
        assert!(matches!(
            node.casper.deploy(signed_deploy(102, shard)).unwrap(),
            Either::Left(DeployError::PoolBusy { .. })
        ));
    }
    #[derive(Clone)]
    struct StoreWithScanHook {
        inner: Arc<dyn KeyValueStore>,
        after_scan: Arc<dyn Fn() -> Result<(), KvStoreError> + Send + Sync>,
    }

    impl KeyValueStore for StoreWithScanHook {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
            self.inner.get(keys)
        }

        fn put(&self, values: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
            self.inner.put(values)
        }

        fn put_one_if_absent(
            &self,
            key: ByteBuffer,
            value: ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
            self.inner.delete(keys)
        }

        fn delete_if_value(
            &self,
            key: &ByteBuffer,
            expected: &ByteBuffer,
        ) -> Result<bool, KvStoreError> {
            self.inner.delete_if_value(key, expected)
        }

        fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
            self.inner.iterate(f)
        }

        fn iterate_while(
            &self,
            f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            self.inner.iterate_while(f)
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
            let values = self.inner.to_map()?;
            (self.after_scan)()?;
            Ok(values)
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    fn deploy(timestamp: i64, shard: &str) -> Signed<DeployData> {
        construct_deploy::source_deploy(
            "Nil".to_owned(),
            timestamp,
            None,
            None,
            None,
            Some(0),
            Some(shard.to_owned()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn repeated_lifecycle_changes_exhaust_refresh_without_persisting_the_candidate() {
        assert_admission_during_lifecycle_changes(3, true).await;
    }

    #[tokio::test]
    async fn one_lifecycle_change_retries_and_accepts_the_candidate_once() {
        assert_admission_during_lifecycle_changes(1, false).await;
    }

    #[tokio::test]
    async fn unrelated_dag_writes_change_lifecycle_revision_without_changing_deploy_rows() {
        use block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables;
        use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;
        use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
        use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

        let dir = tempfile::tempdir().unwrap();
        let mut stores = new_key_value_store_manager(dir.path().to_path_buf(), None);
        let events = stores
            .store("deploy-lifecycle-events".into())
            .await
            .unwrap();
        let terminal = stores
            .store("deploy-lifecycle-terminal".into())
            .await
            .unwrap();
        let unrelated = stores
            .store("mergeable-channel-cache".into())
            .await
            .unwrap();
        let environment = |store: &Arc<dyn KeyValueStore>| {
            store
                .as_any()
                .downcast_ref::<LmdbKeyValueStore>()
                .unwrap()
                .env
                .clone()
        };
        assert!(Arc::ptr_eq(&environment(&events), &environment(&terminal)));
        assert!(Arc::ptr_eq(&environment(&events), &environment(&unrelated)));

        let lifecycle = DeployLifecycleTables::new(events.clone(), terminal.clone());
        let signature = vec![7; 64];
        assert!(lifecycle.open_dropped(&signature, 0).unwrap());
        let events_before = events.to_map().unwrap();
        let terminal_before = terminal.to_map().unwrap();
        let revision_before = lifecycle.revision().unwrap();
        assert!(lifecycle.get_events(&signature).unwrap().is_some());
        assert!(terminal_before.is_empty());

        unrelated
            .put(vec![(b"revision-probe".to_vec(), b"cache-value".to_vec())])
            .unwrap();

        let revision_after = lifecycle.revision().unwrap();
        assert!(revision_after.0 > revision_before.0);
        assert!(revision_after.1 > revision_before.1);
        assert_eq!(events.to_map().unwrap(), events_before);
        assert_eq!(terminal.to_map().unwrap(), terminal_before);
    }

    async fn assert_admission_during_lifecycle_changes(changes: usize, expect_busy: bool) {
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(None)
            .await
            .unwrap();
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };
        use casper::rust::engine::engine::noop;
        use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;

        let dir = tempfile::tempdir().unwrap();
        let mut stores = new_key_value_store_manager(dir.path().to_path_buf(), None);
        let dag = BlockDagKeyValueStorage::new(&mut stores).await.unwrap();
        dag.insert(&genesis.genesis_block, InsertMode::Approved)
            .unwrap();
        let mut node = TestNode::standalone(genesis.clone()).await.unwrap();
        node.engine_cell.set(Arc::new(noop())).await;
        Arc::get_mut(&mut node.casper)
            .expect("Admission fixture must own its Casper instance")
            .block_dag_storage = dag.clone();
        node.block_dag_storage = dag;
        let shard = &genesis.genesis_block.shard_id;
        let seeded: Vec<_> = (0..23).map(|timestamp| deploy(timestamp, shard)).collect();
        for candidate in &seeded {
            assert!(matches!(
                node.casper.deploy(candidate.clone()).unwrap(),
                Either::Right(_)
            ));
        }

        let initial_generation = node.block_dag_storage.current_generation();
        let initial_revision = node.block_dag_storage.deploy_status_revision();
        assert!(initial_revision.is_some());
        let original = node.deploy_storage.clone();
        let scans = Arc::new(AtomicUsize::new(0));
        let markers: Vec<_> = (0..changes)
            .map(|index| deploy(200 + index as i64, shard).sig)
            .collect();
        let hook = {
            let scans = scans.clone();
            let dag = node.block_dag_storage.get_representation().unwrap();
            Arc::new(move || {
                let scan = scans.fetch_add(1, Ordering::SeqCst);
                if let Some(signature) = markers.get(scan) {
                    dag.put_deploy_terminal_if_absent(signature, TerminalRecord {
                        state: TerminalState::Finalized,
                        rejection_count: 0,
                        latest_height: 0,
                        latest_block_hash: Vec::new(),
                    })?;
                }
                Ok(())
            })
        };
        let wrapped: Arc<dyn KeyValueStore> = Arc::new(StoreWithScanHook {
            inner: original.store.raw_store().clone(),
            after_scan: hook,
        });
        let instrumented =
            KeyValueDeployStorage::from_stores(wrapped, Arc::new(InMemoryKeyValueStore::new()))
                .unwrap();
        let instrumented = Arc::new(instrumented);
        node.deploy_storage = instrumented.clone();
        Arc::get_mut(&mut node.casper).unwrap().deploy_storage = instrumented;

        let candidate = deploy(100, shard);
        let result = node.casper.deploy(candidate.clone());
        node.deploy_storage = original.clone();
        Arc::get_mut(&mut node.casper).unwrap().deploy_storage = original;

        if expect_busy {
            assert!(matches!(
                result,
                Ok(Either::Left(DeployError::AdmissionBusy))
            ));
            assert_eq!(scans.load(Ordering::SeqCst), 3);
        } else {
            assert!(matches!(result, Ok(Either::Right(id)) if id == candidate.sig.to_vec()));
            assert!(scans.load(Ordering::SeqCst) > 1);
        }
        assert_ne!(
            node.block_dag_storage.deploy_status_revision(),
            initial_revision
        );
        assert_eq!(
            node.block_dag_storage.current_generation(),
            initial_generation
        );
        {
            let pool = node.deploy_storage.as_ref();
            assert_eq!(pool.contains_sig(&candidate.sig).unwrap(), !expect_busy);
            assert_eq!(
                pool.read_all().unwrap().len(),
                if expect_busy { 23 } else { 24 }
            );
            for accepted in &seeded {
                assert!(pool.contains_sig(&accepted.sig).unwrap());
            }
        }

        if expect_busy {
            assert!(matches!(
                node.casper.deploy(candidate.clone()).unwrap(),
                Either::Right(_)
            ));
        }
        assert!(matches!(
            node.casper.deploy(candidate).unwrap(),
            Either::Left(DeployError::DuplicateDeploy(_))
        ));
        assert_eq!(node.deploy_storage.as_ref().read_all().unwrap().len(), 24);
        assert!(matches!(
            node.casper.deploy(deploy(101, shard)).unwrap(),
            Either::Left(DeployError::PoolBusy { capacity: 24, .. })
        ));
    }
}
