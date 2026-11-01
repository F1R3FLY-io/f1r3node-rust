use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::backend::ExecutionBackend;
use casper::rust::v6::block_record::{BlockRecord, EnvelopeOutcome};
use casper::rust::v6::body::{Attestation, StateHash, V6Body, V6Records};
use casper::rust::v6::executor::BlockExecutor;
use casper::rust::v6::record::ExecutionRecord;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};
use casper::rust::v6::validity::check_structure;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::casper::BlockMessageProto;
use models::rust::casper::protocol::casper_message::{BlockMessage, ProcessedDeploy};
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

const BALANCE: u128 = 1_000_000_000_000;
const UNFUNDED: usize = 9;

struct Host {
    executor: BlockExecutor,
    genesis: BlockMessage,
}

fn payer(index: usize) -> PrivateKey {
    let mut bytes = [7u8; 32];
    bytes[31] = u8::try_from(index + 1).expect("a small payer index");
    PrivateKey::from_bytes(&bytes)
}

fn validator() -> PublicKey { Secp256k1.to_public(&PrivateKey::from_bytes(&[3u8; 32])) }

async fn launch(payers: usize) -> Host {
    let rm = mk_runtime_manager("v6-exit", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = (0..payers)
        .map(|index| Vault {
            vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(&payer(index)))
                .expect("a vault address"),
            initial_balance: BALANCE,
        })
        .collect();
    let block =
        Genesis::create_genesis_block_with_policy(&rm, &genesis, Some(&ShardResourcePolicy::default()))
            .await
            .expect("the genesis block is created");
    Host {
        executor: BlockExecutor::new(Arc::new(rm), 2),
        genesis: block,
    }
}

async fn replica(genesis: &BlockMessage) -> Host {
    let rm = mk_runtime_manager("v6-exit-replica", Some(Genesis::default_mergeable_tags_arc())).await;
    let root = rm
        .replay_compute_state(
            &RuntimeManager::empty_state_hash_fixed(),
            genesis.body.deploys.clone(),
            genesis.body.system_deploys.clone(),
            &BlockData::from_block(genesis),
            None,
            true,
        )
        .await
        .expect("the replica replays the genesis block");
    assert_eq!(root, genesis.body.state.post_state_hash);
    Host {
        executor: BlockExecutor::new(Arc::new(rm), 2),
        genesis: genesis.clone(),
    }
}

fn pre_root(host: &Host) -> StateHash { host.genesis.body.state.post_state_hash.clone() }

fn offer(key: &PrivateKey, term: &str, time_stamp: i64) -> ProcessedDeploy {
    ProcessedDeploy::empty(
        construct_deploy::source_deploy(
            term.to_string(),
            time_stamp,
            Some(100_000),
            Some(1),
            Some(key.clone()),
            None,
            Some(host_shard()),
        )
        .expect("an offer"),
    )
}

fn host_shard() -> String { "root".to_string() }

fn block(host: &Host, number: i64, deploys: Vec<ProcessedDeploy>) -> BlockMessage {
    let mut block = host.genesis.clone();
    block.block_hash = Bytes::from(format!("executed-block-{number}").into_bytes());
    block.header.parents_hash_list = vec![host.genesis.block_hash.clone()];
    block.header.timestamp = 10 * number;
    block.body.state.block_number = number;
    block.body.deploys = deploys;
    block.body.system_deploys.clear();
    block.sender = validator().bytes.clone();
    block.seq_num = i32::try_from(number).expect("a small block number");
    block
}

fn outcomes(record: &BlockRecord) -> Vec<EnvelopeOutcome> {
    record.entries().iter().map(|e| e.executed.outcome).collect()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v6/cross_host")
}

fn decode_block(bytes: &[u8]) -> BlockMessage {
    BlockMessage::from_proto(BlockMessageProto::decode(bytes).expect("a block message"))
        .expect("a valid block")
}

fn fixture_block(host: &Host) -> BlockMessage {
    block(host, 1, vec![
        offer(&payer(0), "@\"race\"!(1)", 1),
        offer(&payer(1), "for (@v <- @\"race\") { @\"won\"!(v) }", 2),
        offer(&payer(0), "for (@v <- @\"race\") { @\"lost\"!(v) }", 3),
        offer(&payer(1), "@\"apart\"!(\"x\")", 4),
        offer(&payer(UNFUNDED), "@\"refused\"!(\"x\")", 5),
    ])
}

#[tokio::test]
#[ignore = "writes the cross-host replay fixture"]
async fn write_cross_host_fixture() {
    let host = launch(2).await;
    let block = fixture_block(&host);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the fixture block executes");
    let seen = outcomes(&record);
    assert_eq!(seen[4], EnvelopeOutcome::NotRun);
    assert!(seen.contains(&EnvelopeOutcome::Conflicted), "{seen:?}");
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).expect("the fixture directory");
    std::fs::write(dir.join("genesis.bin"), host.genesis.to_proto().encode_to_vec())
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
    let host = replica(&genesis).await;
    assert_eq!(record.pre_root(), &pre_root(&host));
    assert_eq!(
        host.executor
            .replay(&record, &block)
            .await
            .expect("this host replays the recorded block"),
        *record.post_root()
    );
}

#[tokio::test]
async fn a_block_executed_on_one_host_replays_on_another() {
    let host = launch(2).await;
    let other = replica(&host.genesis).await;
    let block = fixture_block(&host);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    assert_eq!(
        other
            .executor
            .replay(&record, &block)
            .await
            .expect("the other host replays it"),
        *record.post_root()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cost_breakdown_of_one_envelope() {
    let host = launch(1).await;
    let block = block(&host, 1, vec![offer(
        &payer(0),
        "new x, y in { x!(1) | for (_ <- x) { y!(2) } }",
        1,
    )]);
    let record = host
        .executor
        .execute(&pre_root(&host), &block)
        .await
        .expect("the block executes");
    let entry = &record.entries()[0].executed;
    let settlement = record.settlement().expect("a gated block is settled");
    let charge = &settlement.charges[0];
    println!(
        "comm_phlo={} compute_phlo={} user_log_events={} charges={:?} proposer_credit={} compute_credit={} record_bytes={}",
        entry.comm_phlo,
        entry.compute_phlo,
        entry.user_log.len(),
        charge,
        settlement.proposer_credit,
        settlement.compute_credit,
        record.encode().len(),
    );
    assert_eq!(entry.outcome, EnvelopeOutcome::Completed);
    assert_eq!(entry.comm_phlo, 1);
    assert!(entry.compute_phlo > 0);
    assert_eq!(settlement.proposer_credit, entry.comm_phlo);
    assert_eq!(settlement.compute_credit, entry.compute_phlo);
    assert!(settlement.is_balanced());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement: run with --ignored on a multi-core host"]
async fn phase_timings_by_block_size() {
    let host = launch(64).await;
    for size in [1usize, 4, 16, 64] {
        let deploys = (0..size)
            .map(|index| offer(&payer(index), &format!("@\"timed-{index}\"!({index})"), index as i64 + 1))
            .collect();
        let block = block(&host, 1, deploys);
        let mut best = Duration::MAX;
        for _ in 0..3 {
            let started = Instant::now();
            let record = host
                .executor
                .execute(&pre_root(&host), &block)
                .await
                .expect("the block executes");
            best = best.min(started.elapsed());
            assert!(outcomes(&record)
                .iter()
                .all(|outcome| *outcome == EnvelopeOutcome::Completed));
        }
        println!(
            "envelopes={size} block={best:?} per_envelope={:?} envelopes_per_second={:.2}",
            best / size as u32,
            size as f64 / best.as_secs_f64(),
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn record_bytes_per_envelope_and_validation_at_the_byte_cap() {
    let host = launch(16).await;
    let limits = ShardResourcePolicy::default().limits();
    let mut largest = None;
    for size in [1usize, 4, 16] {
        let deploys = (0..size)
            .map(|index| offer(&payer(index), &format!("@\"sized-{index}\"!({index})"), index as i64 + 1))
            .collect();
        let block = block(&host, 1, deploys);
        let record = host
            .executor
            .execute(&pre_root(&host), &block)
            .await
            .expect("the block executes");
        let bytes = record.encode().len();
        println!("envelopes={size} record_bytes={bytes} per_envelope={}", bytes / size);
        assert!(bytes <= limits.max_record_bytes);
        largest = Some(record);
    }

    let record = largest.expect("a measured record");
    let attestation = Attestation {
        block_hash: host.genesis.block_hash.clone(),
        root: pre_root(&host),
    };
    let mut records = Vec::new();
    let mut total = 0;
    while records.len() < limits.max_records {
        let copy = BlockRecord::decode(&record.encode()).expect("a copy");
        let copy = BlockRecord::new(
            Bytes::from(format!("cited-{}", records.len()).into_bytes()),
            copy.pre_root().clone(),
            copy.mid_root().clone(),
            copy.post_root().clone(),
            copy.entries().to_vec(),
            copy.system_deploys().to_vec(),
        )
        .expect("a copied record");
        let size = copy.encode().len();
        if total + size > limits.max_record_bytes {
            break;
        }
        total += size;
        records.push(copy);
    }
    let mut at_cap = block(&host, 2, vec![]);
    at_cap.body.rejected_deploys.clear();
    at_cap.body.applied_from_scope.clear();
    at_cap.body.extra_bytes = Bytes::new();
    at_cap.body.state.pre_state_hash = attestation.root.clone();
    at_cap.body.state.post_state_hash = attestation.root.clone();
    at_cap.body.merge_base = host.genesis.block_hash.clone();
    at_cap.body.v6 = Some(V6Body::with_records(attestation, &records));
    let started = Instant::now();
    let checked = check_structure::<BlockRecord>(&at_cap, &limits, &|_| Some(0));
    println!(
        "records={} bytes={total} validation={:?}",
        records.len(),
        started.elapsed()
    );
    assert!(checked.is_ok(), "{:?}", checked.err());
}
