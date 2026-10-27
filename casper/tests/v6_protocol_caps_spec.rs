use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::{CasperShardConf, PROTOCOL_V6_VERSION};
use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use casper::rust::genesis::contracts::validator::Validator;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::costacc::genesis_resource_policy::{
    resolve_policy_genesis, shard_resource_schedule, AdoptedResourcePolicy,
};
use casper::rust::util::rholang::costacc::owner_offer::owner_direct_offer;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::gate::{BlockGate, GateInput, RhoBlockGate};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::{
    offered_funded_v6_limits, set_protocol_caps, ProtocolCaps, OFFERED_FUNDED_V6_CAPS,
};
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::record_encoding::{set_record_encoding, RecordEncoding};
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const SHARD: &str = "root";
const LIMIT: u64 = 100_000_000;
const RICH: u128 = 1_000_000_000_000;

fn genesis(
    validator: crypto::rust::public_key::PublicKey,
    payer: &PrivateKey,
    protocol_caps: ProtocolCaps,
) -> Genesis {
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
        vaults: vec![Vault {
            vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(payer))
                .expect("a vault address"),
            initial_balance: RICH,
        }],
        supply: i64::MAX,
        version: PROTOCOL_V6_VERSION,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
        protocol_caps,
    }
}

fn offer(key: &PrivateKey) -> DeployEnvelope {
    let body = DeployData {
        term: "@\"capped\"!(1)".to_string(),
        language: "rholang".to_string(),
        time_stamp: 1,
        valid_after_block_number: 0,
        shard_id: SHARD.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(key, body, LIMIT, 1, &shard_resource_schedule(SHARD, 8))
        .expect("an owner-direct offer");
    DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope)
        .expect("an offered envelope")
}

#[tokio::test]
async fn a_node_adopts_the_genesis_caps_and_the_gate_enforces_them() {
    set_record_encoding(RecordEncoding::Cost);
    let tight = ProtocolCaps {
        funding_search_scans: 1,
        ..OFFERED_FUNDED_V6_CAPS
    };
    let payer = Secp256k1.new_key_pair().0;
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
    let block = Genesis::create_genesis_block(&runtime_manager, &genesis(validator, &payer, tight))
        .await
        .expect("the genesis block is created");
    block_store
        .put_block_message(&block)
        .expect("genesis stored");
    dag.insert(&block, InsertMode::Approved)
        .expect("genesis in dag");
    let policy_genesis = resolve_policy_genesis(&block, &dag, &block_store).unwrap();
    let adopted =
        AdoptedResourcePolicy::load(&runtime_manager, &policy_genesis, &CasperShardConf {
            casper_version: PROTOCOL_V6_VERSION,
            shard_name: SHARD.to_string(),
            min_phlo_price: 1,
            ..CasperShardConf::new()
        })
        .await
        .expect("the shard adopts its policy");
    assert_eq!(adopted.protocol_caps(), Some(tight));
    let root: Bytes = block.body.state.post_state_hash.clone();
    let gate = RhoBlockGate::new(Arc::new(runtime_manager), Arc::new(adopted));
    let envelope = offer(&payer);
    let inputs = [GateInput::Offered(&envelope)];

    set_protocol_caps(tight).expect("positive caps");
    let capped = gate.gate(&root, 10, &inputs).await.unwrap();
    set_protocol_caps(OFFERED_FUNDED_V6_CAPS).expect("positive caps");
    let admitted = gate.gate(&root, 10, &inputs).await.unwrap();

    assert!(!capped[0].is_admitted(), "{:?}", capped[0]);
    assert!(admitted[0].is_admitted(), "{:?}", admitted[0]);
}
