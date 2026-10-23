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
use casper::rust::v6::gate::{BlockGate, GateInput, GateVerdict, RhoBlockGate};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
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
const ONE_HOLD: u128 = 150_000_000;
const POOR: u128 = 10_000_000;

struct Keys {
    rich: PrivateKey,
    pair: PrivateKey,
    poor: PrivateKey,
    unfunded: PrivateKey,
}

fn vault(key: &PrivateKey, balance: u128) -> Vault {
    Vault {
        vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(key))
            .expect("a vault address"),
        initial_balance: balance,
    }
}

fn genesis(validator: crypto::rust::public_key::PublicKey, keys: &Keys) -> Genesis {
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
        vaults: vec![
            vault(&keys.rich, RICH),
            vault(&keys.pair, ONE_HOLD),
            vault(&keys.poor, POOR),
        ],
        supply: i64::MAX,
        version: PROTOCOL_V6_VERSION,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
    }
}

struct Shard {
    gate: RhoBlockGate,
    root: Bytes,
    keys: Keys,
}

async fn launch() -> Shard {
    set_record_encoding(RecordEncoding::Cost);
    let keys = Keys {
        rich: Secp256k1.new_key_pair().0,
        pair: Secp256k1.new_key_pair().0,
        poor: Secp256k1.new_key_pair().0,
        unfunded: Secp256k1.new_key_pair().0,
    };
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
    let block = Genesis::create_genesis_block(&runtime_manager, &genesis(validator, &keys))
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
    Shard {
        gate: RhoBlockGate::new(Arc::new(runtime_manager), Arc::new(adopted)),
        root: block.body.state.post_state_hash.clone(),
        keys,
    }
}

fn offer(key: &PrivateKey, term: &str, time_stamp: i64) -> DeployEnvelope {
    let body = DeployData {
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
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

fn holds(verdict: &GateVerdict) -> u128 {
    match verdict {
        GateVerdict::Admitted { holds } => holds.iter().map(|(_, hold)| hold).sum(),
        GateVerdict::Rejected { cause } => panic!("rejected: {cause}"),
    }
}

fn cause(verdict: &GateVerdict) -> &str {
    match verdict {
        GateVerdict::Rejected { cause } => cause,
        GateVerdict::Admitted { .. } => "",
    }
}

#[tokio::test]
async fn the_gate_admits_funded_offers_and_rejects_the_rest_one_by_one() {
    let shard = launch().await;
    let first = offer(&shard.keys.rich, "@\"a\"!(1)", 1);
    let second = offer(&shard.keys.rich, "@\"b\"!(2)", 2);
    let poor = offer(&shard.keys.poor, "@\"c\"!(3)", 3);
    let unfunded = offer(&shard.keys.unfunded, "@\"d\"!(4)", 4);
    let inputs = [
        GateInput::Offered(&first),
        GateInput::Legacy,
        GateInput::Offered(&poor),
        GateInput::Offered(&first),
        GateInput::Offered(&unfunded),
        GateInput::Offered(&second),
    ];
    let verdicts = shard.gate.gate(&shard.root, 10, &inputs).await.unwrap();
    assert_eq!(verdicts.len(), inputs.len());
    for admitted in [0, 5] {
        let hold = holds(&verdicts[admitted]);
        assert!(
            hold >= u128::from(LIMIT),
            "the hold covers the signed limit"
        );
        assert!(hold <= RICH);
    }
    for rejected in [1, 2, 3, 4] {
        assert!(
            !verdicts[rejected].is_admitted(),
            "{rejected}: {:?}",
            verdicts[rejected]
        );
    }
    for (index, expected) in [
        (1, "only offered-funded"),
        (2, "insufficient phlogiston"),
        (3, "repeated"),
        (4, "insufficient phlogiston"),
    ] {
        assert!(
            cause(&verdicts[index]).contains(expected),
            "{index}: {:?}",
            verdicts[index]
        );
    }
    let again = shard.gate.gate(&shard.root, 10, &inputs).await.unwrap();
    assert_eq!(again, verdicts, "the gate is deterministic at one root");
}

#[tokio::test]
async fn offers_that_share_a_purse_are_admitted_or_rejected_together() {
    let shard = launch().await;
    let one = offer(&shard.keys.pair, "@\"e\"!(5)", 5);
    let two = offer(&shard.keys.pair, "@\"f\"!(6)", 6);
    let rich = offer(&shard.keys.rich, "@\"g\"!(7)", 7);

    let alone = shard
        .gate
        .gate(&shard.root, 10, &[GateInput::Offered(&one)])
        .await
        .unwrap();
    assert!(holds(&alone[0]) <= ONE_HOLD, "one offer fits the purse");

    let verdicts = shard
        .gate
        .gate(&shard.root, 10, &[
            GateInput::Offered(&one),
            GateInput::Offered(&rich),
            GateInput::Offered(&two),
        ])
        .await
        .unwrap();
    assert!(!verdicts[0].is_admitted());
    assert!(!verdicts[2].is_admitted());
    assert!(cause(&verdicts[0]).contains("of the group on a purse"));
    assert_eq!(verdicts[0], verdicts[2]);
    assert!(verdicts[1].is_admitted(), "another purse is not affected");
}

#[tokio::test]
async fn the_gate_needs_a_32_byte_root() {
    let shard = launch().await;
    let first = offer(&shard.keys.rich, "@\"h\"!(8)", 8);
    assert!(shard
        .gate
        .gate(&Bytes::from_static(b"short"), 10, &[GateInput::Offered(
            &first
        )])
        .await
        .is_err());
}
