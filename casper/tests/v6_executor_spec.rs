use std::sync::Arc;
use std::time::Duration;

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
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, RecordEntry};
use casper::rust::v6::executor::{BlockExecutor, PhaseTimings};
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::settlement::EnvelopeOutcome;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
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
const BALANCE: u128 = 1_000_000_000_000;

struct Host {
    executor: BlockExecutor,
    genesis: BlockMessage,
    validator: PublicKey,
}

fn payer(index: usize) -> PrivateKey {
    let mut bytes = [7u8; 32];
    bytes[31] = u8::try_from(index + 1).expect("a small payer index");
    PrivateKey::from_bytes(&bytes)
}

fn genesis(validator: PublicKey, payers: usize) -> Genesis {
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
        vaults: (0..payers)
            .map(|index| Vault {
                vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(&payer(index)))
                    .expect("a vault address"),
                initial_balance: BALANCE,
            })
            .collect(),
        supply: i64::MAX,
        version: PROTOCOL_V6_VERSION,
        native_token_name: "F1R3CAP".to_string(),
        native_token_symbol: "F1R3".to_string(),
        native_token_decimals: 8,
        fs_bundle: Vec::new(),
        consensus_fs_snapshot_cadence: None,
    }
}

async fn launch(payers: usize) -> Host {
    set_record_encoding(RecordEncoding::Cost);
    let validator = Secp256k1.to_public(&PrivateKey::from_bytes(&[3u8; 32]));
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
    let block =
        Genesis::create_genesis_block(&runtime_manager, &genesis(validator.clone(), payers))
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
    Host {
        executor: BlockExecutor::new(Arc::new(runtime_manager), Arc::new(adopted)),
        genesis: block,
        validator,
    }
}

async fn hosts(payers: usize) -> (Host, Host) {
    let host = launch(payers).await;
    let other = launch(payers).await;
    assert_eq!(pre_root(&host), pre_root(&other));
    (host, other)
}

fn offer(key: &PrivateKey, term: &str, time_stamp: i64) -> ProcessedUserDeploy {
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
    let envelope = DeployEnvelope::from_proto(proto, offered_funded_v6_limits().envelope)
        .expect("an offered envelope");
    ProcessedUserDeploy::ordered(envelope).expect("an ordered envelope")
}

fn block(host: &Host, number: i64, deploys: Vec<ProcessedUserDeploy>) -> BlockMessage {
    let mut block = host.genesis.clone();
    block.block_hash = Bytes::from(format!("executed-block-{number}").into_bytes());
    block.header.parents_hash_list = vec![host.genesis.block_hash.clone()];
    block.header.timestamp = 10 * number;
    block.body.state.block_number = number;
    block.body.deploys = deploys;
    block.sender = host.validator.bytes.clone();
    block.seq_num = i32::try_from(number).expect("a small block number");
    block
}

fn outcomes(record: &BlockRecord) -> Vec<EnvelopeOutcome> {
    record
        .entries()
        .iter()
        .map(|entry| entry.executed.outcome)
        .collect()
}

fn pre_root(host: &Host) -> Bytes { host.genesis.body.state.post_state_hash.clone() }

fn rebuilt(record: &BlockRecord, mid_root: Bytes, entries: Vec<RecordEntry>) -> BlockRecord {
    BlockRecord::new(
        record.target().clone(),
        record.pre_root().clone(),
        mid_root,
        record.post_root().clone(),
        entries,
        record.system_deploys().to_vec(),
    )
    .expect("a well-formed forged record")
}

#[tokio::test]
async fn one_offer_executes_and_replays_on_another_host() {
    let (host, other) = hosts(2).await;
    let block = block(&host, 1, vec![offer(&payer(0), "@\"a\"!(1)", 1)]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    assert_eq!(outcomes(&record), vec![EnvelopeOutcome::Completed]);
    assert_ne!(record.mid_root(), &pre_root(&host));
    assert!(record.entries()[0].settled.rev_spent > 0);
    let decoded = BlockRecord::decode(&record.encode()).expect("the record decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        other
            .executor
            .replay(&decoded, &block)
            .await
            .expect("another host replays the record"),
        *record.post_root()
    );
    assert_eq!(
        host.executor
            .replay(&record, &block)
            .await
            .expect("the runner replays its record"),
        *record.post_root()
    );
}

#[tokio::test]
async fn independent_offers_both_merge_and_replay() {
    let (host, other) = hosts(2).await;
    let block = block(&host, 1, vec![
        offer(&payer(0), "@\"b\"!(1)", 1),
        offer(&payer(1), "@\"c\"!(2)", 2),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Completed
    ]);
    assert!(record
        .entries()
        .iter()
        .all(|entry| entry.settled.rev_spent > 0));
    assert_eq!(
        other
            .executor
            .replay(&record, &block)
            .await
            .expect("another host replays the record"),
        *record.post_root()
    );
}

#[tokio::test]
async fn a_send_and_a_receive_in_one_block_conflict_and_the_later_pays() {
    let (host, other) = hosts(2).await;
    let block = block(&host, 1, vec![
        offer(&payer(0), "@\"x\"!(1)", 1),
        offer(&payer(1), "for (_ <- @\"x\") { Nil }", 2),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::RolledBack
    ]);
    assert!(record.entries()[1].settled.rev_spent > 0);
    assert_eq!(
        other
            .executor
            .replay(&record, &block)
            .await
            .expect("another host replays the record"),
        *record.post_root()
    );
}

#[tokio::test]
async fn of_two_consumers_of_one_datum_at_most_one_wins() {
    let (host, other) = hosts(2).await;
    let first = block(&host, 1, vec![offer(&payer(0), "@\"pot\"!(1)", 1)]);
    let seeded = host
        .executor
        .execute(&pre_root(&host), &first)
        .await
        .expect("the seeding block executes");
    assert_eq!(outcomes(&seeded), vec![EnvelopeOutcome::Completed]);
    let mut second = block(&host, 2, vec![
        offer(&payer(0), "for (@v <- @\"pot\") { @\"won\"!(v) }", 2),
        offer(&payer(1), "for (@v <- @\"pot\") { @\"won\"!(v) }", 3),
    ]);
    second.header.parents_hash_list = vec![first.block_hash.clone()];
    let record = host
        .executor
        .execute(seeded.post_root(), &second)
        .await
        .expect("the competing block executes");
    let won = outcomes(&record)
        .iter()
        .filter(|outcome| **outcome == EnvelopeOutcome::Completed)
        .count();
    assert_eq!(won, 1);
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::RolledBack
    ]);
    assert!(record
        .entries()
        .iter()
        .all(|entry| entry.settled.rev_spent > 0));
    assert_eq!(
        other
            .executor
            .replay(&seeded, &first)
            .await
            .expect("another host replays the seeding block"),
        *seeded.post_root()
    );
    assert_eq!(
        other
            .executor
            .replay(&record, &second)
            .await
            .expect("another host replays the competing block"),
        *record.post_root()
    );
}

#[tokio::test]
async fn forged_outcomes_and_merge_roots_are_refused_on_replay() {
    let (host, other) = hosts(2).await;
    let block = block(&host, 1, vec![
        offer(&payer(0), "@\"y\"!(1)", 1),
        offer(&payer(1), "for (_ <- @\"y\") { Nil }", 2),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    let mut kept = record.entries().to_vec();
    kept[1].executed.outcome = EnvelopeOutcome::Completed;
    let unrolled = rebuilt(&record, record.mid_root().clone(), kept);
    assert!(other.executor.replay(&unrolled, &block).await.is_err());
    let mut dropped = record.entries().to_vec();
    dropped[0].executed.outcome = EnvelopeOutcome::RolledBack;
    let censored = rebuilt(&record, record.mid_root().clone(), dropped);
    assert!(other.executor.replay(&censored, &block).await.is_err());
    let unmerged = rebuilt(&record, pre_root(&host), record.entries().to_vec());
    assert!(other.executor.replay(&unmerged, &block).await.is_err());
    let mut other_block = block.clone();
    other_block.block_hash = Bytes::from_static(b"another-block");
    assert!(other.executor.replay(&record, &other_block).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement: run with --ignored on a multi-core host"]
async fn phase_timings_by_block_size() {
    let host = launch(16).await;
    for size in [1usize, 4, 16] {
        let deploys = (0..size)
            .map(|index| {
                offer(
                    &payer(index),
                    &format!("@\"timed-{index}\"!({index})"),
                    i64::try_from(index + 1).expect("a small index"),
                )
            })
            .collect();
        let block = block(&host, 1, deploys);
        let mut best: Option<PhaseTimings> = None;
        let mut total = Duration::ZERO;
        for _ in 0..3 {
            let started = std::time::Instant::now();
            let (record, timings) = host
                .executor
                .execute_timed(&pre_root(&host), &block)
                .await
                .expect("the block executes");
            total += started.elapsed();
            assert!(outcomes(&record)
                .iter()
                .all(|outcome| *outcome == EnvelopeOutcome::Completed));
            best = Some(match best {
                Some(previous)
                    if previous.runs + previous.settlement < timings.runs + timings.settlement =>
                {
                    previous
                }
                _ => timings,
            });
        }
        let best = best.expect("three timed runs");
        let n = u32::try_from(size).expect("a small size");
        println!(
            "envelopes={size} runs={:?} merge={:?} settlement={:?} passes={} settlement_per_envelope={:?} mean_total={:?}",
            best.runs,
            best.merge,
            best.settlement,
            best.settlement_passes,
            best.settlement / n,
            total / 3,
        );
    }
}
