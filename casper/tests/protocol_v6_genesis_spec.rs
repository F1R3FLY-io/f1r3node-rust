use std::collections::HashMap;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, CASPER_PROTOCOL_VERSION, PROTOCOL_V6_VERSION};
use casper::rust::engine::block_approver_protocol::BlockApproverProtocol;
use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::validator::Validator;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::costacc::genesis_resource_policy::{
    resolve_policy_genesis, shard_resource_policy, AdoptedResourcePolicy, GenesisResourcePolicy,
};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use comm::rust::test_instances::TransportLayerStub;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::{ApprovedBlockCandidate, BlockMessage};
use models::rust::record_encoding::{set_record_encoding, RecordEncoding};
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const SHARD: &str = "root";

struct Shard {
    runtime_manager: RuntimeManager,
    block_store: KeyValueBlockStore,
    dag: BlockDagKeyValueStorage,
    genesis: Genesis,
    block: BlockMessage,
}

fn genesis(version: i64, validator: crypto::rust::public_key::PublicKey) -> Genesis {
    Genesis {
        shard_id: SHARD.to_string(),
        timestamp: 0,
        block_number: 0,
        proof_of_stake: ProofOfStake {
            minimum_bond: 1,
            maximum_bond: i64::MAX,
            validators: vec![Validator {
                pk: validator,
                stake: 100,
            }],
            epoch_length: 1000,
            quarantine_length: 50000,
            number_of_active_validators: 1,
            fault_tolerance_threshold_ppm: 0,
            max_parent_depth: 15,
            deploy_lifespan: 50,
            min_phlo_price: 1,
            pos_multi_sig_public_keys: vec![
                "04db91a53a2b72fcdcb201031772da86edad1e4979eb6742928d27731b1771e0bc40c9e9c9fa6554bdec041a87cee423d6f2e09e9dfb408b78e85a4aa611aad20c".to_string(),
                "042a736b30fffcc7d5a58bb9416f7e46180818c82b15542d0a7819d1a437aa7f4b6940c50db73a67bfc5f5ec5b5fa555d24ef8339b03edaa09c096de4ded6eae14".to_string(),
                "047f0f0f5bbe1d6d1a8dac4d88a3957851940f39a57cd89d55fe25b536ab67e6d76fd3f365c83e5bfe11fe7117e549b1ae3dd39bfc867d1c725a4177692c4e7754".to_string(),
            ],
            pos_multi_sig_quorum: 2,
        },
        vaults: Vec::new(),
        supply: i64::MAX,
        version,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
    }
}

async fn launch(version: i64) -> Shard {
    set_record_encoding(match version {
        PROTOCOL_V6_VERSION => RecordEncoding::Cost,
        _ => RecordEncoding::Legacy,
    });
    let (_, validator) = Secp256k1.new_key_pair();
    let mut kvm = InMemoryStoreManager::new();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
        .await
        .expect("block store");
    let dag = BlockDagKeyValueStorage::new(&mut kvm).await.expect("dag");
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = RuntimeManager::mergeable_store(&mut kvm)
        .await
        .expect("mergeable store");
    let (runtime_manager, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        Genesis::default_mergeable_tags_arc(),
        ExternalServices::noop(),
    );
    let genesis = genesis(version, validator);
    let block = Genesis::create_genesis_block(&runtime_manager, &genesis)
        .await
        .expect("the genesis block is created");
    block_store
        .put_block_message(&block)
        .expect("genesis stored");
    dag.insert(&block, InsertMode::Approved)
        .expect("genesis in dag");
    drop(runtime_manager);
    let runtime_manager = reopen(&mut kvm).await;
    Shard {
        runtime_manager,
        block_store,
        dag,
        genesis,
        block,
    }
}

async fn reopen(kvm: &mut InMemoryStoreManager) -> RuntimeManager {
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = RuntimeManager::mergeable_store(kvm)
        .await
        .expect("mergeable store");
    RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        Genesis::default_mergeable_tags_arc(),
        ExternalServices::noop(),
    )
    .0
}

fn shard_conf(version: i64) -> CasperShardConf {
    CasperShardConf {
        casper_version: version,
        shard_name: SHARD.to_string(),
        min_phlo_price: 1,
        ..CasperShardConf::new()
    }
}

async fn validate(shard: &Shard, version: i64) -> Result<(), String> {
    let bonds: HashMap<Bytes, i64> = shard
        .block
        .body
        .state
        .bonds
        .iter()
        .map(|bond| (bond.validator.clone(), bond.stake))
        .collect();
    let pos = &shard.genesis.proof_of_stake;
    BlockApproverProtocol::<TransportLayerStub>::validate_candidate(
        &shard.runtime_manager,
        &ApprovedBlockCandidate {
            block: shard.block.clone(),
            required_sigs: 0,
        },
        0,
        0,
        &shard.genesis.vaults,
        &bonds,
        pos.minimum_bond,
        pos.maximum_bond,
        pos.epoch_length,
        pos.quarantine_length,
        pos.number_of_active_validators,
        pos.fault_tolerance_threshold_ppm,
        pos.max_parent_depth,
        pos.deploy_lifespan,
        pos.min_phlo_price,
        SHARD,
        &pos.pos_multi_sig_public_keys,
        pos.pos_multi_sig_quorum,
        &shard.genesis.native_token_name,
        &shard.genesis.native_token_symbol,
        shard.genesis.native_token_decimals,
        version,
    )
    .await
}

#[tokio::test]
async fn a_protocol_6_launch_seals_and_adopts_the_shard_policy() {
    let shard = launch(PROTOCOL_V6_VERSION).await;
    assert_eq!(shard.block.header.version, PROTOCOL_V6_VERSION);
    assert_eq!(
        shard.block.body.state.pre_state_hash,
        RuntimeManager::empty_state_hash_fixed()
    );

    let expected = shard_resource_policy(SHARD, 8).unwrap();
    let post = &shard.block.body.state.post_state_hash;
    assert_eq!(
        shard
            .runtime_manager
            .find_genesis_resource_policy(post)
            .await
            .unwrap(),
        Some(expected.clone())
    );

    let genesis = resolve_policy_genesis(&shard.block, &shard.dag, &shard.block_store).unwrap();
    let adopted = AdoptedResourcePolicy::load(
        &shard.runtime_manager,
        &genesis,
        &shard_conf(PROTOCOL_V6_VERSION),
    )
    .await
    .unwrap();
    assert!(adopted.offered_funded_v6_active());
    assert_eq!(adopted.genesis().record(), &expected);
    assert_eq!(adopted.genesis().minimum_price(), 1);

    let loaded = GenesisResourcePolicy::load(&shard.runtime_manager, &genesis)
        .await
        .unwrap();
    assert!(loaded.adopt(&shard_conf(CASPER_PROTOCOL_VERSION)).is_err());
}

#[tokio::test]
async fn genesis_validators_accept_only_a_candidate_of_their_own_mode() {
    let shard = launch(PROTOCOL_V6_VERSION).await;
    validate(&shard, PROTOCOL_V6_VERSION)
        .await
        .expect("a protocol-6 validator accepts the protocol-6 candidate");
    let refusal = validate(&shard, CASPER_PROTOCOL_VERSION)
        .await
        .expect_err("a legacy validator refuses the protocol-6 candidate");
    assert!(refusal.contains("protocol version 6"), "{refusal}");

    let mut relabelled = shard;
    relabelled.block.header.version = CASPER_PROTOCOL_VERSION;
    let refusal = validate(&relabelled, CASPER_PROTOCOL_VERSION)
        .await
        .expect_err("a protocol-6 body under a legacy header is refused");
    assert!(
        refusal.contains("do not match expected blessed contracts"),
        "{refusal}"
    );
}

#[tokio::test]
async fn a_node_joining_from_a_recent_block_adopts_the_genesis_policy() {
    let shard = launch(PROTOCOL_V6_VERSION).await;
    let mut anchor = shard.block.clone();
    anchor.header.parents_hash_list = vec![shard.block.block_hash.clone()];
    anchor.body.state.block_number = 1;
    anchor.block_hash = casper::rust::util::proto_util::hash_block(&anchor);
    casper::rust::casper::check_protocol_mode(&anchor, &shard_conf(PROTOCOL_V6_VERSION)).unwrap();
    assert!(casper::rust::casper::check_protocol_mode(
        &anchor,
        &shard_conf(CASPER_PROTOCOL_VERSION)
    )
    .is_err());

    let genesis = resolve_policy_genesis(&anchor, &shard.dag, &shard.block_store).unwrap();
    assert_eq!(genesis.block_hash, shard.block.block_hash);
    let adopted = AdoptedResourcePolicy::load_at(
        &shard.runtime_manager,
        &genesis,
        &anchor.body.state.post_state_hash,
        &shard_conf(PROTOCOL_V6_VERSION),
    )
    .await
    .unwrap();
    assert!(adopted.offered_funded_v6_active());
}
