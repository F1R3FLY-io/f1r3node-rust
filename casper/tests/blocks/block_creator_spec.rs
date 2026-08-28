// See casper/src/test/scala/coop/rchain/casper/blocks/proposer/BlockCreatorSpec.scala
//
// Unit tests for BlockCreator.
// Tests the deploy preparation and cleanup logic.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::proposer::block_creator;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validator_identity::ValidatorIdentity;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashSet;
use models::rust::casper::protocol::casper_message::DeployData;
use models::ByteString;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use crate::util::genesis_builder::DEFAULT_VALIDATOR_SKS;
use crate::util::rholang::resources;

const DEPLOY_LIFESPAN: i64 = 50;

/// Creates a signed deploy with the given parameters
fn create_deploy(
    valid_after_block_number: i64,
    expiration_timestamp: Option<i64>,
    validator_sk: &PrivateKey,
) -> Signed<DeployData> {
    let deploy_data = DeployData {
        term: format!("new x in {{ x!({}) }}", valid_after_block_number),
        time_stamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        phlo_price: 1,
        phlo_limit: 1000,
        valid_after_block_number,
        shard_id: "test-shard".to_string(),
        expiration_timestamp,
    };

    Signed::create(deploy_data, Box::new(Secp256k1), validator_sk.clone())
        .expect("Failed to create signed deploy")
}

/// Gives the snapshot a parent whose main-parent chain roots at a
/// PARENTLESS block at `floor_height`: the cold frontier walk treats a
/// parentless block as genesis (finalized by definition), so the derived
/// floor lands exactly there — letting a test close or open the
/// floor-clock validity window by choosing the height.
async fn stage_floor_at(
    snapshot: &mut CasperSnapshot,
    block_store: &KeyValueBlockStore,
    kvm: &mut InMemoryStoreManager,
    floor_height: i64,
    parent_height: i64,
) {
    stage_floor_at_with_records(
        snapshot,
        block_store,
        kvm,
        floor_height,
        parent_height,
        Vec::new(),
    )
    .await
}

/// `stage_floor_at`, with kept rejection records riding in the FLOOR block —
/// settled adjudications, so the retry gate is open for their sigs.
async fn stage_floor_at_with_records(
    snapshot: &mut CasperSnapshot,
    block_store: &KeyValueBlockStore,
    kvm: &mut InMemoryStoreManager,
    floor_height: i64,
    parent_height: i64,
    floor_records: Vec<models::rust::casper::protocol::casper_message::RejectedDeploy>,
) {
    stage_floor_with_block_records(
        snapshot,
        block_store,
        kvm,
        floor_height,
        parent_height,
        floor_records,
        Vec::new(),
    )
    .await
}

/// Full-control staging: records may ride the floor block (settled — the
/// retry gate is open) or the live parent (unsettled — the gate is closed).
async fn stage_floor_with_block_records(
    snapshot: &mut CasperSnapshot,
    block_store: &KeyValueBlockStore,
    kvm: &mut InMemoryStoreManager,
    floor_height: i64,
    parent_height: i64,
    floor_records: Vec<models::rust::casper::protocol::casper_message::RejectedDeploy>,
    parent_records: Vec<models::rust::casper::protocol::casper_message::RejectedDeploy>,
) {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits;

    let mut root = block_implicits::get_random_block(
        Some(floor_height),
        Some(0),
        None,
        None,
        None,
        None,
        Some(0),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some("test-shard".to_string()),
        None,
    );
    root.body.rejected_deploys = floor_records;
    let mut parent = block_implicits::get_random_block(
        Some(parent_height),
        Some(1),
        None,
        None,
        None,
        None,
        Some(1),
        Some(vec![root.block_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some("test-shard".to_string()),
        None,
    );
    parent.body.rejected_deploys = parent_records;
    let dag_storage = BlockDagKeyValueStorage::new(kvm)
        .await
        .expect("dag storage");
    block_store.put_block_message(&root).expect("store root");
    block_store
        .put_block_message(&parent)
        .expect("store parent");
    dag_storage
        .insert(&root, InsertMode::Approved)
        .expect("insert root");
    dag_storage
        .insert(&parent, InsertMode::Normal)
        .expect("insert parent");
    snapshot.dag = dag_storage.get_representation().expect("dag");
    snapshot.parents = vec![parent];
}

/// Creates a CasperSnapshot for testing with the given parameters.
/// Uses an in-memory DAG representation (matching Scala's TestBlockDagRepresentation).
fn create_snapshot(max_block_num: i64, validator_id: Bytes) -> CasperSnapshot {
    let shard_conf = CasperShardConf {
        fault_tolerance_threshold: 0.0,
        shard_name: "test-shard".to_string(),
        parent_shard_id: "".to_string(),
        finalization_rate: 0,
        max_number_of_parents: 10,
        max_parent_depth: 0,
        synchrony_constraint_threshold: 0.0,
        height_constraint_threshold: 0,
        deploy_lifespan: DEPLOY_LIFESPAN,
        casper_version: 1,
        config_version: 1,
        bond_minimum: 0,
        bond_maximum: i64::MAX,
        epoch_length: 0,
        quarantine_length: 0,
        min_phlo_price: 0,
        enable_mergeable_channel_gc: false,
        mergeable_channels_gc_depth_buffer: 10,
        disable_validator_progress_check: false,
        ..CasperShardConf::new()
    };

    let mut bonds_map: HashMap<ByteString, i64> = HashMap::new();
    bonds_map.insert(validator_id.clone(), 100);

    // Set maxSeqNums like Scala does: Map(validatorId -> 0)
    let mut max_seq_nums: HashMap<ByteString, u64> = HashMap::new();
    max_seq_nums.insert(validator_id.clone(), 0);

    let on_chain_state = OnChainCasperState {
        shard_conf,
        bonds_map,
        active_validators: vec![validator_id],
    };

    // Use in-memory DAG representation (like Scala's TestBlockDagRepresentation)
    let dag = resources::new_key_value_dag_representation();

    CasperSnapshot {
        dag,
        last_finalized_block: Bytes::new(),
        lca: Bytes::new(),
        tips: vec![],
        parents: vec![],
        justifications: HashSet::new(),
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums,
        on_chain_state,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seen_deploys_wait_for_finalized_recovery_buffer() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploy = create_deploy(1, None, &validator_sk);

    deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("add deploy");
    snapshot.deploys_in_scope.insert(deploy.sig.clone());

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare without rejected buffer");
    assert!(
        prepared.deploys.is_empty(),
        "ordinary deploy storage must not re-admit an already-seen deploy"
    );

    snapshot.rejected_in_scope.insert(deploy.sig.clone());
    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare after ordinary merge rejection");
    assert!(
        prepared.deploys.is_empty(),
        "ordinary deploy storage must not re-admit merge-rejected deploys that are still in scope"
    );

    rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .add(vec![deploy.clone()])
        .expect("add rejected deploy");
    // The retry gate needs the rejection SETTLED: stage a floor whose block
    // carries the deploy's kept rejection record.
    stage_floor_at_with_records(&mut snapshot, &block_store, &mut kvm, 10, 20, vec![
        models::rust::casper::protocol::casper_message::RejectedDeploy {
            sig: deploy.sig.clone(),
            duplicate: false,
            carrier: Bytes::new(),
        },
    ])
    .await;
    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare with rejected buffer");
    assert!(
        prepared.deploys.contains(&deploy),
        "rejected-buffer deploys with a settled rejection must be retryable while the rejected source remains in scope"
    );
    assert!(rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .contains_sig(&deploy.sig)
        .expect("contains rejected deploy"));

    snapshot.rejected_in_scope.insert(deploy.sig.clone());
    snapshot.deploys_in_scope.remove(&deploy.sig);
    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare after merge rejection");
    assert!(
        prepared.deploys.contains(&deploy),
        "rejected-buffer deploys must remain recoverable once the earlier clean inclusion is no longer in unresolved scope"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_deploys_are_ignored_when_user_deploy_leadership_is_disabled() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploy = create_deploy(1, None, &validator_sk);

    deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("add deploy");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        false,
    )
    .await
    .expect("prepare without user deploy leadership");

    assert!(
        prepared.deploys.is_empty(),
        "non-leaders must not propose ordinary deploys"
    );
    assert!(deploy_storage
        .lock()
        .read_all()
        .expect("read ordinary deploy storage")
        .iter()
        .any(|stored| stored.sig == deploy.sig));

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare with user deploy leadership");

    assert!(
        prepared.deploys.contains(&deploy),
        "leaders must be able to propose ordinary deploys"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unrelated_ordinary_deploys_remain_selectable_while_scope_has_user_work() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let in_scope = create_deploy(1, None, &validator_sk);
    let pending = create_deploy(2, None, &validator_sk);

    snapshot.deploys_in_scope.insert(in_scope.sig.clone());
    deploy_storage
        .lock()
        .add(vec![in_scope.clone(), pending.clone()])
        .expect("seed deploy storage");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare deploys");

    assert!(!prepared.deploys.contains(&in_scope));
    assert!(prepared.deploys.contains(&pending));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_deploy_selection_uses_config_cap() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 40;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploys: Vec<Signed<DeployData>> = (1..=60)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();
    deploy_storage
        .lock()
        .add(deploys)
        .expect("seed ordinary deploys");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        70,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare ordinary deploys");

    assert_eq!(
        prepared.deploys.len(),
        40,
        "ordinary deploy proposals must use the configured throughput cap"
    );
    assert!(prepared.cap_hit);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordinary_deploy_selection_is_bounded_when_config_is_huge() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploys: Vec<Signed<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();

    deploy_storage
        .lock()
        .add(deploys)
        .expect("seed ordinary deploys");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare ordinary deploys");

    assert_eq!(
        prepared.deploys.len(),
        128,
        "ordinary deploy proposals must remain bounded when config is huge"
    );
    assert!(prepared.cap_hit);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovered_deploy_selection_uses_normal_retry_window() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let recovered: Vec<Signed<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();

    rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .add(recovered.clone())
        .expect("seed rejected deploys");

    // Every buffered retry's kept rejection rides in the staged floor block:
    // settled adjudications, so the retry gate is open and the WINDOW is the
    // only thing bounding selection.
    let floor_records = recovered
        .iter()
        .map(
            |d| models::rust::casper::protocol::casper_message::RejectedDeploy {
                sig: d.sig.clone(),
                duplicate: false,
                carrier: Bytes::new(),
            },
        )
        .collect();
    stage_floor_at_with_records(
        &mut snapshot,
        &block_store,
        &mut kvm,
        190,
        199,
        floor_records,
    )
    .await;

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare recovered deploys");

    assert_eq!(
        prepared.deploys.len(),
        32,
        "recovered-buffer proposals must remain bounded by the retry proposal window"
    );
    let selected_valid_after: HashSet<i64> = prepared
        .deploys
        .iter()
        .map(|d| d.data.valid_after_block_number)
        .collect();
    let expected: HashSet<i64> = (1..=32).collect();
    assert_eq!(
        selected_valid_after, expected,
        "recovered deploy selection should converge on the same deterministic retry slice"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_in_scope_ordinary_deploy_waits_for_recovery_buffer() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let rejected: Vec<Signed<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();

    for deploy in &rejected {
        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        snapshot.rejected_in_scope.insert(deploy.sig.clone());
    }
    deploy_storage
        .lock()
        .add(rejected)
        .expect("seed ordinary rejected deploys");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare rejected ordinary deploys");

    assert_eq!(
        prepared.deploys.len(),
        0,
        "ordinary deploys that are already clean in scope must wait for finalized recovery buffering"
    );
}

/// The gate covers EVERY retry route, not just the buffered one. Deep floor
/// lag makes the geometry: the rejection record is inside the walk window
/// (so the sig is rejected-in-scope) while its carrier is below it (so the
/// sig is NOT in deploys_in_scope and nothing suppresses the pool copy).
/// The record has not settled into the floor closure, so the gate is
/// closed — a proposal carrying the retry would be `PrematureDeployRetry`
/// on every validator. The proposer must defer it, never mint it: a
/// proposal the gate admits is a proposal every validator accepts.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pool_retry_of_an_unsettled_rejection_is_deferred_by_the_gate() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;

    // The pool holds the retry; the buffer does NOT (the deep-lag geometry
    // reaches the pool route, which bypasses the buffered gate).
    let deploy = create_deploy(6, None, &validator_sk);
    snapshot.rejected_in_scope.insert(deploy.sig.clone());
    deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("seed pool retry");

    // Floor at 5, tip parent at 199 (block #200): the walk window is
    // [150, 200], the record rides the LIVE parent (in window, unsettled),
    // and the carrier is below the window — floor-clock admission keeps
    // the retry alive while the tip window has long closed over it.
    stage_floor_with_block_records(
        &mut snapshot,
        &block_store,
        &mut kvm,
        5,
        199,
        Vec::new(),
        vec![
            models::rust::casper::protocol::casper_message::RejectedDeploy {
                sig: deploy.sig.clone(),
                duplicate: false,
                carrier: Bytes::new(),
            },
        ],
    )
    .await;

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        200,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare with a live (unsettled) rejection in scope");

    assert!(
        !prepared.deploys.iter().any(|d| d.sig == deploy.sig),
        "a pool retry whose rejection has not settled into the floor \
         closure must be deferred by the gate, not proposed — every \
         validator would reject the block as PrematureDeployRetry \
         (selected sigs: {:?})",
        prepared
            .deploys
            .iter()
            .map(|d| hex::encode(&d.sig[..8.min(d.sig.len())]))
            .collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_expired_deploy_in_unresolved_scope_is_removed_from_storage() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let snapshot = create_snapshot(100, validator_id);
    let deploy = create_deploy(0, None, &validator_sk);
    snapshot.deploys_in_scope.insert(deploy.sig.clone());
    deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("seed deploy storage");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        101,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer,
        &block_store,
        false,
        true,
    )
    .await
    .expect("prepare deploys");

    assert!(prepared.deploys.is_empty());
    assert!(!deploy_storage
        .lock()
        .read_all()
        .expect("read deploy storage")
        .contains(&deploy));
}

// Inverts the former block_expired_rejected_deploy_retries_after_source_leaves_scope
// contract. The retry carve-out could never succeed: Validate::transaction_expiration
// has no recovery exemption, so a block-expired retry only produced a self-created
// block that failed its own validation, and — with the deploy never purged — the
// proposer rebuilt the same invalid block on every heartbeat (issue #197's permanent
// finalization wedge). Expiry is terminal for rejected-buffer work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_expired_rejected_deploy_is_purged_not_retried() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(100, validator_id);
    // Floor at #60, lifespan 50: the valid_after-0 window closed at floor
    // #50 — expiry is judged on the FLOOR clock, so the purge needs a
    // derivable floor past the window (a tip past it is not enough).
    stage_floor_at(&mut snapshot, &block_store, &mut kvm, 60, 100).await;
    let deploy = create_deploy(0, None, &validator_sk);
    snapshot.rejected_in_scope.insert(deploy.sig.clone());
    deploy_storage
        .lock()
        .add(vec![deploy.clone()])
        .expect("seed deploy storage");

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        101,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer,
        &block_store,
        false,
        true,
    )
    .await
    .expect("prepare deploys");

    assert!(prepared.deploys.is_empty());
    assert!(!deploy_storage
        .lock()
        .read_all()
        .expect("read deploy storage")
        .contains(&deploy));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn buffered_retry_admission_follows_the_recovered_deploys_policy_switch() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();
    let mut kvm = InMemoryStoreManager::new();
    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("rejected deploy buffer"),
    ));
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let mut snapshot = create_snapshot(20, validator_id);
    let deploy = create_deploy(1, None, &validator_sk);

    rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .add(vec![deploy.clone()])
        .expect("seed rejected deploys");

    // A settled rejection in the staged floor opens the retry gate, so the
    // policy switch is the only thing deciding admission below.
    stage_floor_at_with_records(&mut snapshot, &block_store, &mut kvm, 10, 20, vec![
        models::rust::casper::protocol::casper_message::RejectedDeploy {
            sig: deploy.sig.clone(),
            duplicate: false,
            carrier: Bytes::new(),
        },
    ])
    .await;

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        false,
        true,
    )
    .await
    .expect("prepare with recovered admission disabled");

    assert!(
        prepared.deploys.is_empty(),
        "with allow_recovered_deploys off, buffered retries must not be proposed"
    );
    assert!(rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .contains_sig(&deploy.sig)
        .expect("contains rejected deploy"));

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        false,
    )
    .await
    .expect("prepare with recovered admission on, ordinary off");

    assert!(
        prepared.deploys.contains(&deploy),
        "with allow_recovered_deploys on, a gate-open retry is proposed even while ordinary deploys are deferred"
    );

    let prepared = block_creator::prepare_user_deploys(
        &snapshot,
        21,
        i64::MAX,
        deploy_storage,
        rejected_deploy_buffer,
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare with both admission switches on");

    assert!(
        prepared.deploys.contains(&deploy),
        "with both switches on, the gate-open retry is proposed"
    );
}

/// Test: "remove block-expired deploys while keeping valid ones in storage"
///
/// With deployLifespan = 50 and currentBlock = 101 (maxBlockNum = 100),
/// earliestBlockNumber = 101 - 50 = 51
///
/// Expired deploy: validAfterBlockNumber = 0 (<= 51, expired)
/// Valid deploy: validAfterBlockNumber = 60 (> 51 and < 101, valid)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_remove_block_expired_deploys_while_keeping_valid_ones() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();

    // Create all stores from a single InMemoryStoreManager (like Scala's kvm pattern)
    let mut kvm = InMemoryStoreManager::new();

    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("Failed to create deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("Failed to create rejected deploy buffer"),
    ));

    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("Failed to create block store");

    let rspace_store = kvm
        .r_space_stores()
        .await
        .expect("Failed to get rspace store");
    let mergeable_store = resources::mergeable_store_from_dyn(&mut kvm)
        .await
        .expect("Failed to create mergeable store");

    let (runtime_manager, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(casper::rust::genesis::genesis::Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );

    // Create deploys:
    // - Expired deploy: validAfterBlockNumber = 0 (<= 51, expired)
    // - Valid deploy: validAfterBlockNumber = 60 (> 51 and < 101, valid)
    let expired_deploy = create_deploy(0, None, &validator_sk);
    let valid_deploy = create_deploy(60, None, &validator_sk);

    // Add both deploys to storage
    {
        let mut ds = deploy_storage.lock();
        ds.add(vec![expired_deploy.clone(), valid_deploy.clone()])
            .expect("Failed to add deploys");

        // Verify both deploys are in storage
        let deploys_before = ds.read_all().expect("Failed to read deploys");
        assert_eq!(deploys_before.len(), 2, "Expected 2 deploys before create");
    }

    // Create snapshot with maxBlockNum = 100
    let snapshot = create_snapshot(100, validator_id);

    // Call BlockCreator.create
    // The cleanup happens in prepareUserDeploys before block creation
    // Block creation may fail due to empty parents, but that's after cleanup
    let _ = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &runtime_manager,
        &mut block_store.clone(),
        false,
    )
    .await;

    // Verify: expired deploy removed, valid deploy kept
    {
        let ds = deploy_storage.lock();
        let deploys_after = ds.read_all().expect("Failed to read deploys");
        assert_eq!(
            deploys_after.len(),
            1,
            "Expected 1 deploy after create (expired should be removed)"
        );

        let remaining_deploy = deploys_after.iter().next().unwrap();
        assert_eq!(
            remaining_deploy.sig, valid_deploy.sig,
            "Expected valid deploy to remain"
        );
    }
}

/// Test: "remove both block-expired and time-expired deploys while keeping valid ones"
///
/// - Block-expired deploy (validAfterBlockNumber = 0 is expired)
/// - Time-expired deploy (validAfterBlockNumber = 60 is valid, but expirationTimestamp is past)
/// - Valid deploy (validAfterBlockNumber = 60 is valid, no expiration timestamp)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_remove_both_block_expired_and_time_expired_deploys() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();

    // Create all stores from a single InMemoryStoreManager (like Scala's kvm pattern)
    let mut kvm = InMemoryStoreManager::new();

    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("Failed to create deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("Failed to create rejected deploy buffer"),
    ));

    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("Failed to create block store");

    let rspace_store = kvm
        .r_space_stores()
        .await
        .expect("Failed to get rspace store");
    let mergeable_store = resources::mergeable_store_from_dyn(&mut kvm)
        .await
        .expect("Failed to create mergeable store");

    let (runtime_manager, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(casper::rust::genesis::genesis::Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );

    // 1 minute ago (past timestamp for time-expired deploy)
    let past_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        - 60000;

    // Create deploys:
    // - Block-expired deploy (validAfterBlockNumber = 0 is expired)
    // - Time-expired deploy (validAfterBlockNumber = 60 is valid, but expirationTimestamp is past)
    // - Valid deploy (validAfterBlockNumber = 60 is valid, no expiration timestamp)
    let block_expired_deploy = create_deploy(0, None, &validator_sk);
    let time_expired_deploy = create_deploy(60, Some(past_timestamp), &validator_sk);
    let valid_deploy = create_deploy(60, None, &validator_sk);

    // Add all deploys to storage
    {
        let mut ds = deploy_storage.lock();
        ds.add(vec![
            block_expired_deploy.clone(),
            time_expired_deploy.clone(),
            valid_deploy.clone(),
        ])
        .expect("Failed to add deploys");

        // Verify all deploys are in storage
        let deploys_before = ds.read_all().expect("Failed to read deploys");
        assert_eq!(deploys_before.len(), 3, "Expected 3 deploys before create");
    }

    // Create snapshot with maxBlockNum = 100
    let snapshot = create_snapshot(100, validator_id);

    // Call BlockCreator.create
    let _ = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &runtime_manager,
        &mut block_store.clone(),
        false,
    )
    .await;

    // Verify: both expired deploys removed, valid deploy kept
    {
        let ds = deploy_storage.lock();
        let deploys_after = ds.read_all().expect("Failed to read deploys");
        assert_eq!(
            deploys_after.len(),
            1,
            "Expected 1 deploy after create (both expired should be removed)"
        );

        let remaining_deploy = deploys_after.iter().next().unwrap();
        assert_eq!(
            remaining_deploy.sig, valid_deploy.sig,
            "Expected valid deploy to remain"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_remove_expired_deploys_from_rejected_deploy_buffer() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_identity = ValidatorIdentity::new(&validator_sk);
    let validator_id: Bytes = validator_identity.public_key.bytes.clone();

    let mut kvm = InMemoryStoreManager::new();

    let deploy_storage = Arc::new(parking_lot::Mutex::new(
        KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("Failed to create deploy storage"),
    ));
    let rejected_deploy_buffer = Arc::new(Mutex::new(
        block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("Failed to create rejected deploy buffer"),
    ));

    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("Failed to create block store");

    let rspace_store = kvm
        .r_space_stores()
        .await
        .expect("Failed to get rspace store");
    let mergeable_store = resources::mergeable_store_from_dyn(&mut kvm)
        .await
        .expect("Failed to create mergeable store");

    let (runtime_manager, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(casper::rust::genesis::genesis::Genesis::default_mergeable_tags()),
        rholang::rust::interpreter::external_services::ExternalServices::noop(),
    );

    let past_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
        - 60_000;
    let block_expired_deploy = create_deploy(0, None, &validator_sk);
    let time_expired_deploy = create_deploy(60, Some(past_timestamp), &validator_sk);
    let valid_deploy = create_deploy(60, None, &validator_sk);

    {
        let mut buf = rejected_deploy_buffer.lock().unwrap();
        buf.add(vec![
            block_expired_deploy.clone(),
            time_expired_deploy.clone(),
            valid_deploy.clone(),
        ])
        .expect("Failed to add deploys to buffer");

        let deploys_before = buf.read_all().expect("Failed to read buffer");
        assert_eq!(
            deploys_before.len(),
            3,
            "Expected 3 deploys in buffer before create"
        );
    }

    let mut snapshot = create_snapshot(100, validator_id);
    // Floor at #60, lifespan 50: valid_after 0 is window-closed at the
    // floor clock (removal fires); valid_after 60 stays window-open.
    stage_floor_at(&mut snapshot, &block_store, &mut kvm, 60, 100).await;

    let _ = block_creator::create(
        &snapshot,
        &validator_identity,
        None,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &runtime_manager,
        &mut block_store.clone(),
        false,
    )
    .await;

    {
        let buf = rejected_deploy_buffer.lock().unwrap();
        // Inverted from "must remain" (issue #197): a block-expired buffered deploy can
        // never pass Validate::transaction_expiration again, so retaining it only
        // re-offers unproposable work — the fuel of the permanent propose wedge.
        assert!(
            !buf.contains_sig(&block_expired_deploy.sig)
                .expect("Failed to query buffer for block-expired sig"),
            "Block-expired sig must NOT remain in the rejected-deploy buffer after create"
        );
        assert!(
            !buf.contains_sig(&time_expired_deploy.sig)
                .expect("Failed to query buffer for time-expired sig"),
            "Time-expired sig must NOT remain in the rejected-deploy buffer after create"
        );
        assert!(
            buf.contains_sig(&valid_deploy.sig)
                .expect("Failed to query buffer for valid sig"),
            "Valid (unexpired) sig must remain in the rejected-deploy buffer"
        );
    }
}
