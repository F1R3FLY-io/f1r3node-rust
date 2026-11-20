// See casper/src/test/scala/coop/rchain/casper/api/BlocksResponseAPITest.scala
// See [[/docs/casper/images/no_finalizable_block_mistake_with_no_disagreement_check.png]]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::api::block_api::BlockAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::estimator::Estimator;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use models::rust::validator::Validator;

use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;
use crate::helper::{block_generator, block_util};
use crate::util::rholang::resources::{
    generate_scope_id, mk_runtime_manager_at, mk_test_rnode_store_manager_shared,
};
use crate::util::test_mocks::MockKeyValueStore;

fn create_validators_and_bonds() -> (Validator, Validator, Validator, Bond, Bond, Bond, Vec<Bond>) {
    let v1 = block_util::generate_validator(Some("Validator One"));
    let v2 = block_util::generate_validator(Some("Validator Two"));
    let v3 = block_util::generate_validator(Some("Validator Three"));

    let v1_bond = Bond {
        validator: v1.clone(),
        stake: 25.into(),
    };
    let v2_bond = Bond {
        validator: v2.clone(),
        stake: 20.into(),
    };
    let v3_bond = Bond {
        validator: v3.clone(),
        stake: 15.into(),
    };

    let bonds = vec![v1_bond.clone(), v2_bond.clone(), v3_bond.clone()];

    (v1, v2, v3, v1_bond, v2_bond, v3_bond, bonds)
}

// Helper function to create storage components (similar to Scala's BlockDagStorageFixture)
async fn create_storage(_prefix: &str) -> IndexedBlockDagStorage {
    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let dag = crate::util::rholang::resources::block_dag_storage_from_dyn(&mut *kvm)
        .await
        .unwrap();
    IndexedBlockDagStorage::new(dag)
}

const MAX_BLOCK_LIMIT: i32 = 50;

fn create_dag_with_8_blocks(
    block_store: &mut KeyValueBlockStore,
    dag_storage: &mut IndexedBlockDagStorage,
) -> BlockMessage {
    let (v1, v2, v3, _v1_bond, _v2_bond, _v3_bond, bonds) = create_validators_and_bonds();

    let genesis = block_generator::create_genesis_block(
        block_store,
        dag_storage,
        None,
        Some(bonds.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let genesis_hash = genesis.block_hash.clone();

    let justifications_b2: HashMap<Validator, BlockHash> = [
        (v1.clone(), genesis_hash.clone()),
        (v2.clone(), genesis_hash.clone()),
        (v3.clone(), genesis_hash.clone()),
    ]
    .into_iter()
    .collect();

    let b2 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![genesis_hash.clone()],
        &genesis,
        Some(v2.clone()),
        Some(bonds.clone()),
        Some(justifications_b2),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let justifications_b3: HashMap<Validator, BlockHash> = [
        (v1.clone(), genesis_hash.clone()),
        (v2.clone(), genesis_hash.clone()),
        (v3.clone(), genesis_hash.clone()),
    ]
    .into_iter()
    .collect();

    let b3 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![genesis_hash.clone()],
        &genesis,
        Some(v1.clone()),
        Some(bonds.clone()),
        Some(justifications_b3),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let b2_hash = b2.block_hash.clone();
    let b3_hash = b3.block_hash.clone();

    let justifications_b4: HashMap<Validator, BlockHash> = [
        (v1.clone(), genesis_hash.clone()),
        (v2.clone(), b2_hash.clone()),
        (v3.clone(), b2_hash.clone()),
    ]
    .into_iter()
    .collect();

    let b4 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![b2_hash.clone()],
        &genesis,
        Some(v3.clone()),
        Some(bonds.clone()),
        Some(justifications_b4),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let justifications_b5: HashMap<Validator, BlockHash> = [
        (v1.clone(), b3_hash.clone()),
        (v2.clone(), b2_hash.clone()),
        (v3.clone(), genesis_hash.clone()),
    ]
    .into_iter()
    .collect();

    let b5 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![b3_hash.clone()],
        &genesis,
        Some(v2.clone()),
        Some(bonds.clone()),
        Some(justifications_b5),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let b4_hash = b4.block_hash.clone();
    let b5_hash = b5.block_hash.clone();

    let justifications_b6: HashMap<Validator, BlockHash> = [
        (v1.clone(), b3_hash.clone()),
        (v2.clone(), b2_hash.clone()),
        (v3.clone(), b4_hash.clone()),
    ]
    .into_iter()
    .collect();

    let b6 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![b4_hash.clone()],
        &genesis,
        Some(v1.clone()),
        Some(bonds.clone()),
        Some(justifications_b6),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let justifications_b7: HashMap<Validator, BlockHash> = [
        (v1.clone(), b3_hash.clone()),
        (v2.clone(), b5_hash.clone()),
        (v3.clone(), b4_hash.clone()),
    ]
    .into_iter()
    .collect();

    let _b7 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![b5_hash.clone()],
        &genesis,
        Some(v3.clone()),
        Some(bonds.clone()),
        Some(justifications_b7),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let b6_hash = b6.block_hash.clone();

    let justifications_b8: HashMap<Validator, BlockHash> = [
        (v1.clone(), b6_hash.clone()),
        (v2.clone(), b5_hash.clone()),
        (v3.clone(), b4_hash.clone()),
    ]
    .into_iter()
    .collect();

    let _b8 = block_generator::create_block(
        block_store,
        dag_storage,
        vec![b6_hash],
        &genesis,
        Some(v2.clone()),
        Some(bonds.clone()),
        Some(justifications_b8),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );

    genesis
}

#[tokio::test]
async fn show_main_chain_should_return_only_blocks_in_the_main_chain() {
    let shared_kvm_data = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<u8>>::new()));

    // Create block_store with shared kvm
    let mut block_store = KeyValueBlockStore::new(
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
    );

    let mut block_dag_storage = create_storage("show-main-chain-test-").await;

    let genesis = create_dag_with_8_blocks(&mut block_store, &mut block_dag_storage);

    let mut dag = block_dag_storage
        .get_representation()
        .expect("dag representation");

    let latest_messages = dag.latest_message_hashes().into_iter().collect();
    let tips = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &BlockMetadata::from_block(&genesis, None, None, None),
            latest_messages,
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap();

    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let runtime_manager = mk_runtime_manager_at(&mut *kvm, None).await;

    let casper_effect = NoOpsCasperEffect::new_with_shared_kvm(
        Some(tips.tips),
        Arc::new(runtime_manager),
        block_store.clone(),
        dag.clone(),
        shared_kvm_data.clone(), // shared kvm!
    );

    let engine = EngineWithCasper::new(Arc::new(casper_effect));

    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let blocks_response = BlockAPI::show_main_chain(&engine_cell, 10, MAX_BLOCK_LIMIT).await;

    assert_eq!(
        blocks_response.len(),
        5,
        "Main chain should contain 5 blocks"
    );
}

#[tokio::test]
async fn get_blocks_should_return_all_blocks() {
    let shared_kvm_data = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<u8>>::new()));

    // Create block_store with shared kvm
    let mut block_store = KeyValueBlockStore::new(
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
    );

    let mut dag_storage = create_storage("get-blocks-test-").await;

    let genesis = create_dag_with_8_blocks(&mut block_store, &mut dag_storage);

    let mut dag = dag_storage
        .get_representation()
        .expect("dag representation");

    let latest_messages = dag.latest_message_hashes().into_iter().collect();
    let tips = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &BlockMetadata::from_block(&genesis, None, None, None),
            latest_messages,
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap();

    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let runtime_manager = mk_runtime_manager_at(&mut *kvm, None).await;

    let casper_effect = NoOpsCasperEffect::new_with_shared_kvm(
        Some(tips.tips),
        Arc::new(runtime_manager),
        block_store.clone(),
        dag.clone(),
        shared_kvm_data.clone(),
    );

    let engine = EngineWithCasper::new(Arc::new(casper_effect));

    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let blocks_response = BlockAPI::get_blocks(&engine_cell, 10, MAX_BLOCK_LIMIT)
        .await
        .unwrap();
    // TODO: from Scala -> Switch to 4 when we implement block height correctly
    assert_eq!(blocks_response.len(), 8, "Should return all 8 blocks");
}

#[tokio::test]
async fn get_blocks_should_return_until_depth() {
    let shared_kvm_data = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<u8>>::new()));

    // Create block_store with shared kvm
    let mut block_store = KeyValueBlockStore::new(
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
    );

    let mut dag_storage = create_storage("get-blocks-depth-test-").await;

    let genesis = create_dag_with_8_blocks(&mut block_store, &mut dag_storage);

    let mut dag = dag_storage
        .get_representation()
        .expect("dag representation");

    let latest_messages = dag.latest_message_hashes().into_iter().collect();
    let tips = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &BlockMetadata::from_block(&genesis, None, None, None),
            latest_messages,
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap();

    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let runtime_manager = mk_runtime_manager_at(&mut *kvm, None).await;

    let casper_effect = NoOpsCasperEffect::new_with_shared_kvm(
        Some(tips.tips),
        Arc::new(runtime_manager),
        block_store.clone(),
        dag.clone(),
        shared_kvm_data.clone(),
    );

    let engine = EngineWithCasper::new(Arc::new(casper_effect));

    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let blocks_response = BlockAPI::get_blocks(&engine_cell, 2, MAX_BLOCK_LIMIT)
        .await
        .unwrap();

    // TODO: from Scala -> Switch to 3 when we implement block height correctly
    assert_eq!(
        blocks_response.len(),
        2,
        "Should return 2 blocks until depth"
    );
}

#[tokio::test]
async fn get_blocks_by_heights_should_return_blocks_between_start_and_end() {
    let shared_kvm_data = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<u8>>::new()));

    // Create block_store with shared kvm
    let mut block_store = KeyValueBlockStore::new(
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
    );

    let mut dag_storage = create_storage("get-blocks-by-heights-test-").await;

    let genesis = create_dag_with_8_blocks(&mut block_store, &mut dag_storage);

    let mut dag = dag_storage
        .get_representation()
        .expect("dag representation");

    let latest_messages = dag.latest_message_hashes().into_iter().collect();
    let tips = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &BlockMetadata::from_block(&genesis, None, None, None),
            latest_messages,
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap();

    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let runtime_manager = mk_runtime_manager_at(&mut *kvm, None).await;

    let casper_effect = NoOpsCasperEffect::new_with_shared_kvm(
        Some(tips.tips),
        Arc::new(runtime_manager),
        block_store.clone(),
        dag.clone(),
        shared_kvm_data.clone(),
    );

    let engine = EngineWithCasper::new(Arc::new(casper_effect));

    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    let response = BlockAPI::get_blocks_by_heights(&engine_cell, 2, 5, MAX_BLOCK_LIMIT)
        .await
        .unwrap();

    assert_eq!(response.served.len(), 4, "Should return 4 served blocks");
    assert!(
        response.pruned.is_empty(),
        "Happy path should report no pruned siblings"
    );
    assert!(
        response.unavailable.is_empty(),
        "Happy path should report no unavailable siblings"
    );
    assert_eq!(
        response.prune_floor, 0,
        "Prune floor should reflect anchor default when no override was set"
    );
    assert_eq!(
        response.served.first().unwrap().block_number,
        2,
        "First block should be at height 2"
    );
    assert_eq!(
        response.served.last().unwrap().block_number,
        5,
        "Last block should be at height 5"
    );
}

/// Build the engine around a DAG of 8 blocks, delete one block's body
/// so the DAG metadata survives but the body is gone, and set the
/// retention-sweep floor to the given value. Returns `(engine_cell,
/// pruned_hash, pruned_height)` so tests can assert against the exact
/// missing hash. The DAG metadata-index survives
/// [`KeyValueBlockStore::discard_stored`] because retention does not
/// sweep metadata (#1010 / #1045).
async fn setup_dag_with_discarded_body(
    scope_label: &str,
    prune_floor: i64,
    target_block_height: i64,
) -> (
    EngineCell,
    BlockHash,
    i64,
    std::sync::Arc<std::sync::atomic::AtomicI64>,
) {
    let shared_kvm_data = Arc::new(Mutex::new(HashMap::<Vec<u8>, Vec<u8>>::new()));

    let mut block_store = KeyValueBlockStore::new(
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
        Arc::new(MockKeyValueStore::with_shared_data(shared_kvm_data.clone())),
    );

    let mut dag_storage = create_storage(scope_label).await;

    let genesis = create_dag_with_8_blocks(&mut block_store, &mut dag_storage);

    let mut dag = dag_storage
        .get_representation()
        .expect("dag representation");

    let latest_messages = dag.latest_message_hashes().into_iter().collect();
    let tips = Estimator::apply()
        .tips_with_latest_messages(
            &mut dag,
            &BlockMetadata::from_block(&genesis, None, None, None),
            latest_messages,
            Estimator::UNLIMITED_PARENTS,
            None,
        )
        .await
        .unwrap();

    // Pick a block at the requested height from the topo_sort rows.
    let rows = dag.topo_sort(0, Some(10)).expect("topo_sort");
    let mut target_hash: Option<BlockHash> = None;
    for row in &rows {
        for hash in row {
            let meta = dag.lookup(hash).unwrap().expect("metadata present");
            if meta.block_number == target_block_height {
                target_hash = Some(hash.clone());
                break;
            }
        }
        if target_hash.is_some() {
            break;
        }
    }
    let target_hash = target_hash.expect("no block at target height");
    let target_height = dag
        .lookup(&target_hash)
        .unwrap()
        .expect("metadata")
        .block_number;

    // Delete only the body. DAG metadata survives — the sweep does
    // not touch BlockMetadata (#1010 / #1045), so `dag.lookup` keeps
    // returning the height for the deleted hash.
    block_store
        .discard_stored(&target_hash)
        .expect("discard body");

    let scope_id = generate_scope_id();
    let mut kvm = mk_test_rnode_store_manager_shared(scope_id);
    let runtime_manager = mk_runtime_manager_at(&mut *kvm, None).await;

    let casper_effect = NoOpsCasperEffect::new_with_shared_kvm(
        Some(tips.tips),
        Arc::new(runtime_manager),
        block_store.clone(),
        dag.clone(),
        shared_kvm_data.clone(),
    );
    let anchor_handle = casper_effect.anchor_handle();
    anchor_handle.store(prune_floor, std::sync::atomic::Ordering::Release);

    let engine = EngineWithCasper::new(Arc::new(casper_effect));
    let engine_cell = EngineCell::init();
    engine_cell.set(Arc::new(engine)).await;

    (engine_cell, target_hash, target_height, anchor_handle)
}

#[tokio::test]
async fn get_blocks_by_heights_reports_pruned_siblings_below_floor() {
    // Put the floor above the target height so the missing body is
    // classified as pruned-permanent.
    let (engine_cell, pruned_hash, pruned_height, _anchor) =
        setup_dag_with_discarded_body("gbh-pruned-", 100, 2).await;

    let response = BlockAPI::get_blocks_by_heights(&engine_cell, 0, 10, MAX_BLOCK_LIMIT)
        .await
        .unwrap();

    assert_eq!(
        response.prune_floor, 100,
        "envelope prune_floor mirrors the anchor snapshot"
    );
    assert!(
        response
            .served
            .iter()
            .all(|b| b.block_hash != hex::encode(&pruned_hash)),
        "the pruned hash must not appear in served"
    );
    assert_eq!(
        response.pruned.len(),
        1,
        "exactly the one deleted hash should be reported as pruned"
    );
    assert_eq!(response.pruned[0].hash, pruned_hash);
    assert_eq!(response.pruned[0].block_height, pruned_height);
    assert!(
        response.unavailable.is_empty(),
        "height is below the floor, so no entries should be unavailable"
    );
}

#[tokio::test]
async fn get_blocks_by_heights_reports_unavailable_siblings_above_floor() {
    // Put the floor below the target height so the missing body is
    // classified as transiently unavailable.
    let (engine_cell, missing_hash, missing_height, _anchor) =
        setup_dag_with_discarded_body("gbh-unavail-", 1, 2).await;

    let response = BlockAPI::get_blocks_by_heights(&engine_cell, 0, 10, MAX_BLOCK_LIMIT)
        .await
        .unwrap();

    assert_eq!(response.prune_floor, 1);
    assert_eq!(
        response.unavailable.len(),
        1,
        "the missing hash is above the floor and belongs to `unavailable`"
    );
    assert_eq!(response.unavailable[0].hash, missing_hash);
    assert_eq!(response.unavailable[0].block_height, missing_height);
    assert!(
        response.pruned.is_empty(),
        "height is above the floor, so no entries should be pruned"
    );
    assert!(response
        .served
        .iter()
        .all(|b| b.block_hash != hex::encode(&missing_hash)),);
}

#[tokio::test]
async fn get_block_single_hash_splits_pruned_and_unavailable_by_floor() {
    use casper::rust::api::block_api::{BlockPrunedError, BlockUnavailableError};

    // Same setup; the single-hash path should classify by the same
    // threshold the by-heights path does.
    let (engine_cell_pruned, pruned_hash, _pruned_height, _) =
        setup_dag_with_discarded_body("gb-single-pruned-", 100, 2).await;
    let err = BlockAPI::get_block(&engine_cell_pruned, &hex::encode(&pruned_hash))
        .await
        .expect_err("body deleted below floor should error");
    assert!(
        err.downcast_ref::<BlockPrunedError>().is_some(),
        "expected BlockPrunedError, got {err:?}"
    );

    let (engine_cell_unavail, missing_hash, _missing_height, _) =
        setup_dag_with_discarded_body("gb-single-unavail-", 1, 2).await;
    let err = BlockAPI::get_block(&engine_cell_unavail, &hex::encode(&missing_hash))
        .await
        .expect_err("body deleted above floor should error");
    assert!(
        err.downcast_ref::<BlockUnavailableError>().is_some(),
        "expected BlockUnavailableError, got {err:?}"
    );
}

#[tokio::test]
async fn get_blocks_by_heights_classifies_floor_boundary_as_pruned() {
    // block_height == prune_floor must go into `pruned`. GC advances the
    // effective anchor with fetch_max to the target height *before* the
    // Delete phase (mergeable_channels_gc/job.rs), so a body visible as
    // missing at exactly the floor was swept.
    let (engine_cell, boundary_hash, boundary_height, _anchor) =
        setup_dag_with_discarded_body("gbh-boundary-", 2, 2).await;
    assert_eq!(boundary_height, 2);

    let response = BlockAPI::get_blocks_by_heights(&engine_cell, 0, 10, MAX_BLOCK_LIMIT)
        .await
        .unwrap();

    assert_eq!(response.prune_floor, 2);
    assert_eq!(response.pruned.len(), 1);
    assert_eq!(response.pruned[0].hash, boundary_hash);
    assert_eq!(response.pruned[0].block_height, boundary_height);
    assert!(
        response.unavailable.is_empty(),
        "boundary height equals the floor; nothing should be unavailable"
    );
}

#[tokio::test]
async fn every_api_entry_point_reports_a_missing_casper_instance() {
    use models::rhoapi::Par;

    let engine_cell = EngineCell::init();

    assert!(BlockAPI::get_blocks(&engine_cell, 10, MAX_BLOCK_LIMIT)
        .await
        .is_err());
    assert!(BlockAPI::get_blocks_full(&engine_cell, 10, MAX_BLOCK_LIMIT)
        .await
        .is_err());
    assert!(
        BlockAPI::get_blocks_by_heights(&engine_cell, 0, 5, MAX_BLOCK_LIMIT)
            .await
            .is_err()
    );
    assert!(
        BlockAPI::get_blocks_by_heights_full(&engine_cell, 0, 5, MAX_BLOCK_LIMIT)
            .await
            .is_err()
    );
    assert!(
        BlockAPI::machine_verifiable_dag(&engine_cell, 10, MAX_BLOCK_LIMIT)
            .await
            .is_err()
    );
    assert!(BlockAPI::find_deploy(&engine_cell, &vec![1u8; 10])
        .await
        .is_err());
    assert!(BlockAPI::get_block(&engine_cell, "abcdef01").await.is_err());
    assert!(BlockAPI::last_finalized_block(&engine_cell).await.is_err());
    assert!(BlockAPI::is_finalized(&engine_cell, "abcdef01")
        .await
        .is_err());
    assert!(
        BlockAPI::deploy_finalization_status(&engine_cell, &[1u8; 8])
            .await
            .is_err()
    );
    assert!(BlockAPI::list_pending_deploys(&engine_cell, None)
        .await
        .is_err());
    assert!(BlockAPI::bond_status(&engine_cell, &vec![1u8; 33])
        .await
        .is_err());
    assert!(BlockAPI::get_latest_message(&engine_cell).await.is_err());
    assert!(BlockAPI::get_listening_name_data_response(
        &engine_cell,
        1,
        Par::default(),
        MAX_BLOCK_LIMIT
    )
    .await
    .is_err());
    assert!(BlockAPI::get_listening_name_continuation_response(
        &engine_cell,
        1,
        &[Par::default()],
        MAX_BLOCK_LIMIT
    )
    .await
    .is_err());
    assert!(
        BlockAPI::get_data_at_par(&engine_cell, &Par::default(), "aabbcc".to_string(), false)
            .await
            .is_err()
    );

    let main_chain = BlockAPI::show_main_chain(&engine_cell, 10, MAX_BLOCK_LIMIT).await;
    assert!(
        main_chain.is_empty(),
        "show_main_chain degrades to an empty listing without casper"
    );
}

#[test]
fn preview_private_names_is_deterministic_and_clamped() {
    let deployer: Vec<u8> = vec![7u8; 33];

    let first = BlockAPI::preview_private_names(&deployer, 42, 10, 3).unwrap();
    let second = BlockAPI::preview_private_names(&deployer, 42, 10, 3).unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(
        first, second,
        "the same deployer, timestamp and valid-after block must yield the same names"
    );

    let different_time = BlockAPI::preview_private_names(&deployer, 43, 10, 3).unwrap();
    assert_ne!(first, different_time);

    let different_valid_after = BlockAPI::preview_private_names(&deployer, 42, 11, 3).unwrap();
    assert_ne!(first, different_valid_after);

    assert!(BlockAPI::preview_private_names(&deployer, 42, 10, -5)
        .unwrap()
        .is_empty());
}
