// See casper/src/test/scala/coop/rchain/casper/blocks/proposer/BlockCreatorSpec.scala
//
// Unit tests for BlockCreator.
// Tests the deploy preparation and cleanup logic.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::pending_deploy::PendingDeploy;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::proposer::block_creator;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::validator_identity::ValidatorIdentity;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use dashmap::DashSet;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Justification, ProcessedDeploy, RejectedDeploy, RejectedDeployReason,
};
use models::rust::deploy_id::DeployLookupId;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::helper::block_generator::build_block_at_height;
use crate::util::genesis_builder::DEFAULT_VALIDATOR_SKS;

const DEPLOY_LIFESPAN: i64 = 50;

fn create_deploy(
    valid_after_block_number: i64,
    expiration_timestamp: Option<i64>,
    validator_sk: &PrivateKey,
) -> Cosigned<DeployData> {
    let deploy_data = DeployData {
        language: "rholang".to_string(),
        term: format!("new x in {{ x!({}) }}", valid_after_block_number),
        time_stamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        valid_after_block_number,
        shard_id: "test-shard".to_string(),
        expiration_timestamp,
        authority_presentations: Vec::new(),
    };

    Cosigned::create_single_envelope(deploy_data, Box::new(Secp256k1), validator_sk.clone())
        .expect("Failed to create deploy envelope")
}

fn pending_entry(deploy: &Cosigned<DeployData>) -> PendingDeploy {
    crate::pending_envelope(deploy.clone())
}

fn deploy_id(deploy: &Cosigned<DeployData>) -> DeployLookupId {
    pending_entry(deploy).typed_deploy_id().clone()
}

fn add_deploys(storage: &mut KeyValueDeployStorage, deploys: Vec<Cosigned<DeployData>>) {
    for deploy in deploys {
        assert!(storage
            .add_envelope_if_absent(deploy)
            .expect("add deploy envelope"));
    }
}

/// Creates a CasperSnapshot for testing with the given parameters.
/// Uses an in-memory DAG representation (matching Scala's TestBlockDagRepresentation).
async fn create_snapshot(
    max_block_num: i64,
    validator_id: Bytes,
    block_store: &KeyValueBlockStore,
    kvm: &mut InMemoryStoreManager,
    rejected: &[Cosigned<DeployData>],
) -> CasperSnapshot {
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
        casper_version: casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        config_version: 1,
        bond_minimum: 0,
        bond_maximum: i64::MAX,
        epoch_length: 0,
        quarantine_length: 0,
        min_phlo_price: 0,
        enable_mergeable_channel_gc: false,
        mergeable_channels_gc_depth_buffer: 10,
        disable_late_block_filtering: false,
        disable_validator_progress_check: false,
        ..CasperShardConf::new()
    };

    let recovery_validator_id = ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[1])
        .public_key
        .bytes
        .clone();
    let validators = if rejected.is_empty() {
        vec![validator_id.clone()]
    } else {
        vec![validator_id.clone(), recovery_validator_id.clone()]
    };
    let bonds = validators
        .iter()
        .map(|validator| Bond {
            validator: validator.clone(),
            stake: 100,
        })
        .collect::<Vec<_>>();
    let bonds_map = validators
        .iter()
        .map(|validator| (validator.clone(), 100))
        .collect();
    let max_seq_nums = validators
        .iter()
        .map(|validator| (validator.clone(), 3))
        .collect();
    let bond_generations = validators
        .iter()
        .map(|validator| (validator.clone(), BondGeneration::GENESIS))
        .collect();

    let on_chain_state = OnChainCasperState {
        shard_conf,
        bonds_map,
        bond_generations,
        active_validators: validators,
    };

    let dag_storage = BlockDagKeyValueStorage::new(kvm)
        .await
        .expect("DAG storage");
    let genesis = build_block_at_height(
        0,
        Vec::new(),
        Some(validator_id.clone()),
        0,
        Some(bonds.clone()),
        Some(Vec::new()),
        None,
        None,
        Some("test-shard".to_string()),
        None,
        Some(0),
    );
    block_store
        .put_block_message(&genesis)
        .expect("store finalized genesis");
    let mut dag = dag_storage
        .insert(&genesis, InsertMode::ApprovedGenesis)
        .expect("insert finalized genesis");
    dag.put_cached_floor(genesis.block_hash.clone(), genesis.block_hash.clone())
        .expect("cache genesis floor");
    dag.put_cached_frontier(genesis.block_hash.clone(), genesis.block_hash.clone())
        .expect("cache genesis frontier");

    let (parent, last_finalized_block, justifications) = if rejected.is_empty() {
        let source = build_block_at_height(
            max_block_num.saturating_sub(1).max(1),
            vec![genesis.block_hash.clone()],
            Some(validator_id.clone()),
            1,
            Some(bonds.clone()),
            Some(vec![Justification {
                validator: validator_id.clone(),
                latest_block_hash: genesis.block_hash.clone(),
            }]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(1),
        );
        block_store
            .put_block_message(&source)
            .expect("store floor source");
        dag_storage
            .insert(&source, InsertMode::Normal)
            .expect("insert floor source");
        let boundary = build_block_at_height(
            max_block_num.max(2),
            vec![source.block_hash.clone()],
            Some(validator_id.clone()),
            2,
            Some(bonds.clone()),
            Some(vec![Justification {
                validator: validator_id.clone(),
                latest_block_hash: source.block_hash.clone(),
            }]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(2),
        );
        block_store
            .put_block_message(&boundary)
            .expect("store finalized boundary");
        dag = dag_storage
            .insert(&boundary, InsertMode::Normal)
            .expect("insert finalized boundary");
        let justifications = vec![Justification {
            validator: validator_id.clone(),
            latest_block_hash: boundary.block_hash.clone(),
        }];
        (boundary.clone(), boundary.block_hash, justifications)
    } else {
        let genesis_view = vec![
            Justification {
                validator: validator_id.clone(),
                latest_block_hash: genesis.block_hash.clone(),
            },
            Justification {
                validator: recovery_validator_id.clone(),
                latest_block_hash: genesis.block_hash.clone(),
            },
        ];
        let base = build_block_at_height(
            max_block_num.saturating_sub(4).max(1),
            vec![genesis.block_hash.clone()],
            Some(validator_id.clone()),
            1,
            Some(bonds.clone()),
            Some(genesis_view.clone()),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(1),
        );
        let source = build_block_at_height(
            max_block_num.saturating_sub(4).max(1),
            vec![genesis.block_hash.clone()],
            Some(recovery_validator_id.clone()),
            1,
            Some(bonds.clone()),
            Some(genesis_view),
            Some(
                rejected
                    .iter()
                    .map(ProcessedDeploy::empty_from_cosigned)
                    .collect(),
            ),
            None,
            Some("test-shard".to_string()),
            None,
            Some(1),
        );
        for block in [&base, &source] {
            block_store
                .put_block_message(block)
                .expect("store merge parent");
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert merge parent");
        }
        let mut boundary = build_block_at_height(
            max_block_num.saturating_sub(3).max(2),
            vec![base.block_hash.clone(), source.block_hash.clone()],
            Some(validator_id.clone()),
            2,
            Some(bonds.clone()),
            Some(vec![
                Justification {
                    validator: validator_id.clone(),
                    latest_block_hash: base.block_hash.clone(),
                },
                Justification {
                    validator: recovery_validator_id.clone(),
                    latest_block_hash: source.block_hash.clone(),
                },
            ]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(2),
        );
        boundary.body.merge_base = base.block_hash.clone();
        boundary.body.rejected_deploys = rejected
            .iter()
            .map(|deploy| {
                let DeployLookupId::V6(deploy_id) = deploy_id(deploy) else {
                    unreachable!()
                };
                RejectedDeploy::occurrence_v6(
                    deploy_id,
                    source.block_hash.clone(),
                    RejectedDeployReason::MergeConflict,
                )
            })
            .collect();
        block_store
            .put_block_message(&boundary)
            .expect("store rejection boundary");
        dag_storage
            .insert(&boundary, InsertMode::Normal)
            .expect("insert rejection boundary");
        let first_witness = build_block_at_height(
            max_block_num.saturating_sub(2).max(3),
            vec![boundary.block_hash.clone()],
            Some(recovery_validator_id.clone()),
            3,
            Some(bonds.clone()),
            Some(vec![
                Justification {
                    validator: validator_id.clone(),
                    latest_block_hash: boundary.block_hash.clone(),
                },
                Justification {
                    validator: recovery_validator_id.clone(),
                    latest_block_hash: source.block_hash.clone(),
                },
            ]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(2),
        );
        block_store
            .put_block_message(&first_witness)
            .expect("store floor witness");
        dag_storage
            .insert(&first_witness, InsertMode::Normal)
            .expect("insert floor witness");
        let confirmation = build_block_at_height(
            max_block_num.saturating_sub(1).max(4),
            vec![first_witness.block_hash.clone()],
            Some(validator_id.clone()),
            4,
            Some(bonds.clone()),
            Some(vec![
                Justification {
                    validator: validator_id.clone(),
                    latest_block_hash: boundary.block_hash.clone(),
                },
                Justification {
                    validator: recovery_validator_id.clone(),
                    latest_block_hash: first_witness.block_hash.clone(),
                },
            ]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(3),
        );
        block_store
            .put_block_message(&confirmation)
            .expect("store floor confirmation");
        dag_storage
            .insert(&confirmation, InsertMode::Normal)
            .expect("insert floor confirmation");
        let witness = build_block_at_height(
            max_block_num.max(5),
            vec![confirmation.block_hash.clone()],
            Some(recovery_validator_id.clone()),
            5,
            Some(bonds.clone()),
            Some(vec![
                Justification {
                    validator: validator_id.clone(),
                    latest_block_hash: confirmation.block_hash.clone(),
                },
                Justification {
                    validator: recovery_validator_id.clone(),
                    latest_block_hash: first_witness.block_hash.clone(),
                },
            ]),
            None,
            None,
            Some("test-shard".to_string()),
            None,
            Some(3),
        );
        block_store
            .put_block_message(&witness)
            .expect("store settled floor witness");
        dag = dag_storage
            .insert(&witness, InsertMode::Normal)
            .expect("insert settled floor witness");
        let justifications = vec![
            Justification {
                validator: validator_id.clone(),
                latest_block_hash: confirmation.block_hash.clone(),
            },
            Justification {
                validator: recovery_validator_id,
                latest_block_hash: witness.block_hash.clone(),
            },
        ];
        (witness, boundary.block_hash, justifications)
    };

    CasperSnapshot {
        dag,
        last_finalized_block,
        lca: Bytes::new(),
        tips: vec![],
        parents: vec![parent],
        justifications,
        invalid_blocks: HashMap::new(),
        deploys_in_scope: Arc::new(DashSet::new()),
        rejected_in_scope: Arc::new(DashSet::new()),
        max_block_num,
        max_seq_nums,
        finalized_floor_bonds: bonds,
        on_chain_state,
        consensus_context:
            casper::rust::causal_equivocation::CertifiedConsensusContext::pre_genesis(),
        finalized_floor_certificate: None,
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
    let deploy = create_deploy(1, None, &validator_sk);
    let mut snapshot = create_snapshot(
        20,
        validator_id,
        &block_store,
        &mut kvm,
        std::slice::from_ref(&deploy),
    )
    .await;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;

    add_deploys(&mut deploy_storage.lock(), vec![deploy.clone()]);
    snapshot.deploys_in_scope.insert(deploy_id(&deploy));

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

    snapshot.rejected_in_scope.insert(deploy_id(&deploy));
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
        .add(vec![pending_entry(&deploy)])
        .expect("add rejected deploy");
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
        prepared.deploys.contains(&pending_entry(&deploy)),
        "rejected-buffer deploys with a visible rejection must be retryable while the rejected source remains in scope"
    );
    assert!(rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .contains_id(&deploy_id(&deploy))
        .expect("contains rejected deploy"));

    snapshot.rejected_in_scope.insert(deploy_id(&deploy));
    snapshot.deploys_in_scope.remove(&deploy_id(&deploy));
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
        prepared.deploys.contains(&pending_entry(&deploy)),
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
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &[]).await;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploy = create_deploy(1, None, &validator_sk);

    add_deploys(&mut deploy_storage.lock(), vec![deploy.clone()]);

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
        .read_all_envelopes()
        .expect("read ordinary deploy storage")
        .contains(&deploy));

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
        prepared.deploys.contains(&pending_entry(&deploy)),
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
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &[]).await;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let in_scope = create_deploy(1, None, &validator_sk);
    let pending = create_deploy(2, None, &validator_sk);

    snapshot.deploys_in_scope.insert(deploy_id(&in_scope));
    add_deploys(&mut deploy_storage.lock(), vec![
        in_scope.clone(),
        pending.clone(),
    ]);

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

    assert!(!prepared.deploys.contains(&pending_entry(&in_scope)));
    assert!(prepared.deploys.contains(&pending_entry(&pending)));
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
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &[]).await;
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 40;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploys: Vec<Cosigned<DeployData>> = (1..=60)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();
    add_deploys(&mut deploy_storage.lock(), deploys);

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
    let validator_id: Bytes = ValidatorIdentity::new(&validator_sk)
        .public_key
        .bytes
        .clone();
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
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &[]).await;
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let deploys: Vec<Cosigned<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();

    add_deploys(&mut deploy_storage.lock(), deploys);

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
    let recovered: Vec<Cosigned<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &recovered).await;
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;

    rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .add(recovered.iter().map(pending_entry).collect())
        .expect("seed rejected deploys");

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
        .map(|d| d.data().valid_after_block_number)
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
    let mut snapshot = create_snapshot(20, validator_id, &block_store, &mut kvm, &[]).await;
    snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block = 777_777;
    snapshot.on_chain_state.shard_conf.deploy_lifespan = 10_000;
    let rejected: Vec<Cosigned<DeployData>> = (1..=160)
        .map(|n| create_deploy(n, None, &validator_sk))
        .collect();

    for deploy in &rejected {
        snapshot.deploys_in_scope.insert(deploy_id(deploy));
        snapshot.rejected_in_scope.insert(deploy_id(deploy));
    }
    add_deploys(&mut deploy_storage.lock(), rejected);

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
    let snapshot = create_snapshot(100, validator_id, &block_store, &mut kvm, &[]).await;
    let deploy = create_deploy(0, None, &validator_sk);
    snapshot.deploys_in_scope.insert(deploy_id(&deploy));
    add_deploys(&mut deploy_storage.lock(), vec![deploy.clone()]);

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
        .read_all_envelopes()
        .expect("read deploy storage")
        .contains(&deploy));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_expired_rejected_deploy_is_terminal_even_with_visible_rejection() {
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
    let snapshot = create_snapshot(100, validator_id, &block_store, &mut kvm, &[]).await;
    let deploy = create_deploy(0, None, &validator_sk);
    snapshot.rejected_in_scope.insert(deploy_id(&deploy));
    add_deploys(&mut deploy_storage.lock(), vec![deploy.clone()]);

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
        .read_all_envelopes()
        .expect("read deploy storage")
        .contains(&deploy));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovered_deploys_are_ignored_when_recovery_leadership_is_disabled() {
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
    let deploy = create_deploy(1, None, &validator_sk);
    let snapshot = create_snapshot(
        20,
        validator_id,
        &block_store,
        &mut kvm,
        std::slice::from_ref(&deploy),
    )
    .await;

    rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .add(vec![pending_entry(&deploy)])
        .expect("seed rejected deploys");

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
    .expect("prepare without recovery leadership");

    assert!(
        prepared.deploys.is_empty(),
        "non-leaders must not propose recovered deploys"
    );
    assert!(rejected_deploy_buffer
        .lock()
        .expect("rejected buffer lock")
        .contains_id(&deploy_id(&deploy))
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
    .expect("prepare recovered-only leadership");

    assert!(
        prepared.deploys.contains(&pending_entry(&deploy)),
        "leaders must be able to propose recovered deploys while ordinary deploys are deferred"
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
    .expect("prepare with recovery leadership");

    assert!(
        prepared.deploys.contains(&pending_entry(&deploy)),
        "leaders must be able to propose recovered deploys"
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

    // Create deploys:
    // - Expired deploy: validAfterBlockNumber = 0 (<= 51, expired)
    // - Valid deploy: validAfterBlockNumber = 60 (> 51 and < 101, valid)
    let expired_deploy = create_deploy(0, None, &validator_sk);
    let valid_deploy = create_deploy(60, None, &validator_sk);

    // Add both deploys to storage
    {
        let mut ds = deploy_storage.lock();
        add_deploys(&mut ds, vec![expired_deploy.clone(), valid_deploy.clone()]);

        // Verify both deploys are in storage
        let deploys_before = ds.read_all_envelopes().expect("Failed to read deploys");
        assert_eq!(deploys_before.len(), 2, "Expected 2 deploys before create");
    }

    // Create snapshot with maxBlockNum = 100
    let snapshot = create_snapshot(100, validator_id, &block_store, &mut kvm, &[]).await;

    block_creator::prepare_user_deploys(
        &snapshot,
        101,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        false,
        true,
    )
    .await
    .expect("prepare deploys");

    // Verify: expired deploy removed, valid deploy kept
    {
        let ds = deploy_storage.lock();
        let deploys_after = ds.read_all_envelopes().expect("Failed to read deploys");
        assert_eq!(
            deploys_after.len(),
            1,
            "Expected 1 deploy after create (expired should be removed)"
        );

        let remaining_deploy = deploys_after.iter().next().unwrap();
        assert_eq!(
            remaining_deploy, &valid_deploy,
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
    let validator_id: Bytes = ValidatorIdentity::new(&validator_sk)
        .public_key
        .bytes
        .clone();

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
        add_deploys(&mut ds, vec![
            block_expired_deploy.clone(),
            time_expired_deploy.clone(),
            valid_deploy.clone(),
        ]);

        // Verify all deploys are in storage
        let deploys_before = ds.read_all_envelopes().expect("Failed to read deploys");
        assert_eq!(deploys_before.len(), 3, "Expected 3 deploys before create");
    }

    // Create snapshot with maxBlockNum = 100
    let snapshot = create_snapshot(100, validator_id, &block_store, &mut kvm, &[]).await;

    block_creator::prepare_user_deploys(
        &snapshot,
        101,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        false,
        true,
    )
    .await
    .expect("prepare deploys");

    // Verify: both expired deploys removed, valid deploy kept
    {
        let ds = deploy_storage.lock();
        let deploys_after = ds.read_all_envelopes().expect("Failed to read deploys");
        assert_eq!(
            deploys_after.len(),
            1,
            "Expected 1 deploy after create (both expired should be removed)"
        );

        let remaining_deploy = deploys_after.iter().next().unwrap();
        assert_eq!(
            remaining_deploy, &valid_deploy,
            "Expected valid deploy to remain"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_remove_expired_deploys_from_rejected_deploy_buffer() {
    crate::init_logger();

    let validator_sk = DEFAULT_VALIDATOR_SKS[0].clone();
    let validator_id: Bytes = ValidatorIdentity::new(&validator_sk)
        .public_key
        .bytes
        .clone();

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
            pending_entry(&block_expired_deploy),
            pending_entry(&time_expired_deploy),
            pending_entry(&valid_deploy),
        ])
        .expect("Failed to add deploys to buffer");

        let deploys_before = buf.read_all().expect("Failed to read buffer");
        assert_eq!(
            deploys_before.len(),
            3,
            "Expected 3 deploys in buffer before create"
        );
    }

    let snapshot = create_snapshot(100, validator_id, &block_store, &mut kvm, &[]).await;

    block_creator::prepare_user_deploys(
        &snapshot,
        101,
        i64::MAX,
        deploy_storage.clone(),
        rejected_deploy_buffer.clone(),
        &block_store,
        true,
        true,
    )
    .await
    .expect("prepare rejected deploys");

    {
        let buf = rejected_deploy_buffer.lock().unwrap();
        assert!(
            !buf.contains_id(&deploy_id(&block_expired_deploy))
                .expect("Failed to query buffer for block-expired sig"),
            "Block-expired recovered sig must NOT remain in the rejected-deploy buffer"
        );
        assert!(
            !buf.contains_id(&deploy_id(&time_expired_deploy))
                .expect("Failed to query buffer for time-expired sig"),
            "Time-expired sig must NOT remain in the rejected-deploy buffer after create"
        );
        assert!(
            buf.contains_id(&deploy_id(&valid_deploy))
                .expect("Failed to query buffer for valid sig"),
            "Valid (unexpired) sig must remain in the rejected-deploy buffer"
        );
    }
}
