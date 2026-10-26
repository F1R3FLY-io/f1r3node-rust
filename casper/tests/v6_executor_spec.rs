use std::path::PathBuf;
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
use casper::rust::v6::gate::{BlockGate, GateInput, GateVerdict, RhoBlockGate};
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::settlement::EnvelopeOutcome;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::casper::BlockMessageProto;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedUserDeploy};
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::DeployEnvelope;
use models::rust::record_encoding::{set_record_encoding, RecordEncoding};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const SHARD: &str = "root";
const LIMIT: u64 = 100_000_000;
const BALANCE: u128 = 1_000_000_000_000;

struct Host {
    executor: BlockExecutor,
    gate: RhoBlockGate,
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

async fn launch(payers: usize) -> Host { start(payers, None).await }

async fn replica(host: &Host) -> Host { start(0, Some(&host.genesis)).await }

async fn start(payers: usize, replayed: Option<&BlockMessage>) -> Host {
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
    let block = match replayed {
        None => {
            Genesis::create_genesis_block(&runtime_manager, &genesis(validator.clone(), payers))
                .await
                .expect("the genesis block is created")
        }
        Some(genesis) => {
            let deploys = genesis
                .body
                .deploys
                .iter()
                .map(|deploy| deploy.as_legacy().expect("a legacy genesis deploy").clone())
                .collect();
            let root = runtime_manager
                .replay_compute_state(
                    &RuntimeManager::empty_state_hash_fixed(),
                    deploys,
                    genesis.body.system_deploys.clone(),
                    &BlockData::from_block(genesis),
                    None,
                    true,
                )
                .await
                .expect("the replica replays the genesis block");
            assert_eq!(root, genesis.body.state.post_state_hash);
            genesis.clone()
        }
    };
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
    let runtime_manager = Arc::new(runtime_manager);
    let adopted = Arc::new(adopted);
    Host {
        executor: BlockExecutor::new(Arc::clone(&runtime_manager), Arc::clone(&adopted)),
        gate: RhoBlockGate::new(runtime_manager, adopted),
        genesis: block,
        validator,
    }
}

async fn hosts(payers: usize) -> (Host, Host) {
    let host = launch(payers).await;
    let other = replica(&host).await;
    assert_eq!(pre_root(&host), pre_root(&other));
    (host, other)
}

fn offer(key: &PrivateKey, term: &str, time_stamp: i64) -> ProcessedUserDeploy {
    priced_offer(key, term, time_stamp, 1)
}

fn priced_offer(key: &PrivateKey, term: &str, time_stamp: i64, price: u64) -> ProcessedUserDeploy {
    let body = DeployData {
        term: term.to_string(),
        language: "rholang".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: SHARD.to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let proto = owner_direct_offer(key, body, LIMIT, price, &shard_resource_schedule(SHARD, 8))
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
        EnvelopeOutcome::Conflicted
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
        EnvelopeOutcome::Conflicted
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
async fn two_numbers_sent_on_one_fresh_channel_conflict_and_two_strings_do_not() {
    let (host, other) = hosts(2).await;
    let numbers = block(&host, 1, vec![
        offer(&payer(0), "@\"n\"!(1)", 1),
        offer(&payer(1), "@\"n\"!(2)", 2),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &numbers)
        .await
        .expect("the numeric block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted
    ]);
    assert_eq!(
        other
            .executor
            .replay(&record, &numbers)
            .await
            .expect("another host replays the numeric block"),
        *record.post_root()
    );
    let strings = block(&host, 1, vec![
        offer(&payer(0), "@\"s\"!(\"one\")", 1),
        offer(&payer(1), "@\"s\"!(\"two\")", 2),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &strings)
        .await
        .expect("the string block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Completed
    ]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_racing_block_run_in_parallel_replays_on_another_host() {
    let (host, other) = hosts(2).await;
    let block = block(&host, 1, vec![
        offer(&payer(0), "@\"race\"!(1)", 1),
        offer(&payer(1), "@\"race\"!(2)", 2),
        offer(&payer(0), "for (@v <- @\"race\") { @\"won\"!(v) }", 3),
        offer(&payer(1), "for (@v <- @\"race\") { @\"second\"!(v) }", 4),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the racing block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Conflicted
    ]);
    for _ in 0..3 {
        assert_eq!(
            other
                .executor
                .replay(&record, &block)
                .await
                .expect("another host replays the racing block"),
            *record.post_root()
        );
    }
    let again = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the racing block executes again");
    assert_eq!(outcomes(&again), outcomes(&record));
    assert_eq!(
        other
            .executor
            .replay(&again, &block)
            .await
            .expect("another host replays the second execution"),
        *again.post_root()
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
    let host = launch(64).await;
    for size in [1usize, 4, 16, 64] {
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
        let busy = best.runs + best.merge + best.settlement;
        println!(
            "envelopes={size} runs={:?} merge={:?} settlement={:?} passes={} settlement_per_envelope={:?} mean_total={:?} envelopes_per_second={:.2}",
            best.runs,
            best.merge,
            best.settlement,
            best.settlement_passes,
            best.settlement / n,
            total / 3,
            size as f64 / busy.as_secs_f64(),
        );
    }
}

const UNFUNDED: usize = 9;
const PRICES: [u64; 3] = [1, 6_000, 100_000];

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
}

fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|cases| cases.parse().ok())
            .unwrap_or(8),
        failure_persistence: None,
        ..Config::default()
    })
}

#[test]
fn every_executed_envelope_was_admitted_by_the_gate_at_the_pre_state() {
    let runtime = runtime();
    let (host, other) = runtime.block_on(hosts(2));
    let offers = proptest::collection::vec((0..3usize, 0..PRICES.len()), 1..6);
    runner()
        .run(&offers, |offers| {
            runtime.block_on(async {
                let deploys: Vec<_> = offers
                    .iter()
                    .enumerate()
                    .map(|(index, (who, price))| {
                        let key = payer(if *who == 2 { UNFUNDED } else { *who });
                        priced_offer(
                            &key,
                            &format!("@\"gate-{index}\"!(\"x\")"),
                            i64::try_from(index + 1).expect("a small index"),
                            PRICES[*price],
                        )
                    })
                    .collect();
                let block = block(&host, 1, deploys);
                let inputs: Vec<GateInput<'_>> =
                    block.body.deploys.iter().map(GateInput::from).collect();
                let verdicts = host
                    .gate
                    .gate(&pre_root(&host), block.header.timestamp, &inputs)
                    .await
                    .expect("the gate runs");
                let record = host
                    .executor
                    .execute(&pre_root(&host), &block)
                    .await
                    .expect("the block executes");
                prop_assert_eq!(record.entries().len(), verdicts.len());
                for (entry, verdict) in record.entries().iter().zip(&verdicts) {
                    prop_assert_eq!(&entry.executed.verdict, verdict);
                    match verdict {
                        GateVerdict::Rejected { .. } => {
                            prop_assert_eq!(entry.executed.outcome, EnvelopeOutcome::NotRun);
                            prop_assert_eq!(entry.executed.phlo_used, 0);
                            prop_assert!(entry.executed.user_log.is_empty());
                            prop_assert!(entry.settled.debits.is_empty());
                            prop_assert_eq!(entry.settled.rev_spent, 0);
                        }
                        GateVerdict::Admitted { .. } => {
                            prop_assert_ne!(entry.executed.outcome, EnvelopeOutcome::NotRun);
                        }
                    }
                }
                if verdicts.iter().all(|verdict| !verdict.is_admitted()) {
                    prop_assert_eq!(record.mid_root(), &pre_root(&host));
                }
                prop_assert_eq!(
                    other
                        .executor
                        .replay(&record, &block)
                        .await
                        .expect("another host replays the record"),
                    record.post_root().clone()
                );
                Ok(())
            })
        })
        .expect("GateBeforeExecute holds");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Contender {
    Consumer(usize),
    Number(usize, i64),
    Independent,
}

fn contender_term(contender: Contender, index: usize) -> String {
    match contender {
        Contender::Consumer(pot) => {
            format!("for (@v <- @\"pot-{pot}\") {{ @\"won-{pot}-{index}\"!(\"x\") }}")
        }
        Contender::Number(channel, value) => format!("@\"num-{channel}\"!({value})"),
        Contender::Independent => format!("@\"free-{index}\"!(\"x\")"),
    }
}

fn expected_outcomes(contenders: &[Contender]) -> Vec<EnvelopeOutcome> {
    contenders
        .iter()
        .enumerate()
        .map(|(index, contender)| {
            let first =
                |same: &dyn Fn(&Contender) -> bool| contenders.iter().position(same) == Some(index);
            let wins = match *contender {
                Contender::Consumer(pot) => first(&|other| *other == Contender::Consumer(pot)),
                Contender::Number(channel, _) => {
                    first(&|other| matches!(other, Contender::Number(c, _) if *c == channel))
                }
                Contender::Independent => true,
            };
            if wins {
                EnvelopeOutcome::Completed
            } else {
                EnvelopeOutcome::Conflicted
            }
        })
        .collect()
}

#[test]
fn at_most_one_of_competing_envelopes_survives_and_it_is_the_first() {
    let runtime = runtime();
    let (host, other, seed, seeded) = runtime.block_on(async {
        let (host, other) = hosts(2).await;
        let seed = block(&host, 1, vec![offer(
            &payer(0),
            "@\"pot-0\"!(0) | @\"pot-1\"!(1)",
            1,
        )]);
        let seeded = host
            .executor
            .execute(&pre_root(&host), &seed)
            .await
            .expect("the seeding block executes");
        assert_eq!(outcomes(&seeded), vec![EnvelopeOutcome::Completed]);
        assert_eq!(
            other
                .executor
                .replay(&seeded, &seed)
                .await
                .expect("another host replays the seeding block"),
            *seeded.post_root()
        );
        (host, other, seed, seeded)
    });
    let contender = prop_oneof![
        (0..2usize).prop_map(Contender::Consumer),
        (0..2usize, 1..100i64).prop_map(|(channel, value)| Contender::Number(channel, value)),
        Just(Contender::Independent),
    ];
    runner()
        .run(&proptest::collection::vec(contender, 2..8), |contenders| {
            runtime.block_on(async {
                let deploys = contenders
                    .iter()
                    .enumerate()
                    .map(|(index, contender)| {
                        offer(
                            &payer(index % 2),
                            &contender_term(*contender, index),
                            i64::try_from(index + 2).expect("a small index"),
                        )
                    })
                    .collect();
                let mut competing = block(&host, 2, deploys);
                competing.header.parents_hash_list = vec![seed.block_hash.clone()];
                let record = host
                    .executor
                    .execute(seeded.post_root(), &competing)
                    .await
                    .expect("the competing block executes");
                prop_assert_eq!(outcomes(&record), expected_outcomes(&contenders));
                prop_assert!(record
                    .entries()
                    .iter()
                    .all(|entry| entry.settled.rev_spent > 0));
                prop_assert_eq!(
                    other
                        .executor
                        .replay(&record, &competing)
                        .await
                        .expect("another host replays the competing block"),
                    record.post_root().clone()
                );
                Ok(())
            })
        })
        .expect("AtMostOneOfCompeting holds");
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v6/cross_host")
}

fn decode_block(bytes: &[u8]) -> BlockMessage {
    let limits = offered_funded_v6_limits();
    BlockMessage::from_proto_with_limits(
        BlockMessageProto::decode(bytes).expect("a block message"),
        limits.envelope,
        limits.evidence,
    )
    .expect("a valid block")
}

#[tokio::test]
#[ignore = "writes the cross-host replay fixture"]
async fn write_cross_host_fixture() {
    let host = launch(2).await;
    let block = block(&host, 1, vec![
        offer(&payer(0), "@\"race\"!(1)", 1),
        offer(&payer(1), "@\"race\"!(2)", 2),
        offer(&payer(0), "for (@v <- @\"race\") { @\"won\"!(v) }", 3),
        offer(&payer(1), "@\"apart\"!(\"x\")", 4),
        offer(&payer(UNFUNDED), "@\"refused\"!(\"x\")", 5),
    ]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the fixture block executes");
    assert_eq!(outcomes(&record), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::NotRun,
    ]);
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).expect("the fixture directory");
    std::fs::write(
        dir.join("genesis.bin"),
        host.genesis.to_proto().encode_to_vec(),
    )
    .expect("the genesis fixture");
    std::fs::write(dir.join("block.bin"), block.to_proto().encode_to_vec())
        .expect("the block fixture");
    std::fs::write(dir.join("record.bin"), record.encode()).expect("the record fixture");
}

#[tokio::test]
async fn a_block_recorded_on_another_host_replays_here() {
    let dir = fixture_dir();
    let read = |name: &str| std::fs::read(dir.join(name)).expect("a fixture file");
    let genesis = decode_block(&read("genesis.bin"));
    let block = decode_block(&read("block.bin"));
    let record = BlockRecord::decode(&read("record.bin")).expect("a recorded block");
    assert_eq!(record.target(), &block.block_hash);
    let host = start(0, Some(&genesis)).await;
    assert_eq!(record.pre_root(), &pre_root(&host));
    assert_eq!(
        host.executor
            .replay(&record, &block)
            .await
            .expect("this host replays the recorded block"),
        *record.post_root()
    );
}
