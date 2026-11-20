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
use casper::rust::v6::executor::{BlockExecutor, BlockTimings};
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
    let n = u16::try_from(index + 1).expect("a payer index below 65535");
    bytes[30] = 7u8.wrapping_add((n >> 8) as u8);
    bytes[31] = n as u8;
    PrivateKey::from_bytes(&bytes)
}

fn validator() -> PublicKey { Secp256k1.to_public(&PrivateKey::from_bytes(&[3u8; 32])) }

async fn launch(payers: usize) -> Host { launch_with(payers, 2).await }

async fn launch_with(payers: usize, workers: usize) -> Host {
    let rm = mk_runtime_manager("v6-exit", Some(Genesis::default_mergeable_tags_arc())).await;
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = PROTOCOL_V6;
    genesis.vaults = (0..payers)
        .map(|index| Vault {
            vault_address: VaultAddress::from_public_key(&Secp256k1.to_public(&payer(index)))
                .expect("a vault address"),
            initial_balance: shared::rust::token_amount::TokenAmount::new((BALANCE).into())
                .unwrap(),
        })
        .collect();
    let block = Genesis::create_genesis_block_with_policy(
        &rm,
        &genesis,
        Some(&ShardResourcePolicy::default()),
    )
    .await
    .expect("the genesis block is created");
    Host {
        executor: BlockExecutor::new(Arc::new(rm), workers),
        genesis: block,
    }
}

async fn replica(genesis: &BlockMessage) -> Host {
    let rm = mk_runtime_manager(
        "v6-exit-replica",
        Some(Genesis::default_mergeable_tags_arc()),
    )
    .await;
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
            Some(1.into()),
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
    record
        .entries()
        .iter()
        .map(|e| e.executed.outcome)
        .collect()
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
            .map(|index| {
                offer(
                    &payer(index),
                    &format!("@\"timed-{index}\"!({index})"),
                    index as i64 + 1,
                )
            })
            .collect();
        let block = block(&host, 1, deploys);
        let mut best: Option<(Duration, BlockTimings, usize)> = None;
        for _ in 0..3 {
            let started = Instant::now();
            let (record, timings) = host
                .executor
                .execute_timed(&pre_root(&host), &block)
                .await
                .expect("the block executes");
            let total = started.elapsed();
            assert!(outcomes(&record)
                .iter()
                .all(|outcome| *outcome == EnvelopeOutcome::Completed));
            if best.as_ref().is_none_or(|(b, _, _)| total < *b) {
                best = Some((total, timings, record.encode().len()));
            }
        }
        let (total, t, record_bytes) = best.expect("three runs");
        let e = t.envelopes;
        let execution = e.gate + e.runs + e.merge + e.settlement;
        println!(
            "envelopes={size} block={total:?} gate={:?} runs={:?} (spawn={:?} setup={:?} eval={:?} checkpoint={:?}, summed over runs) merge={:?} settlement={:?} close_block={:?} audit={:?} record={:?} record_bytes={record_bytes} settlement_share={:.2}% execution_per_envelope={:?} envelopes_per_second={:.2} execution_envelopes_per_second={:.2}",
            e.gate,
            e.runs,
            e.run_spawn,
            e.run_setup,
            e.run_eval,
            e.run_checkpoint,
            e.merge,
            e.settlement,
            t.close_block,
            t.audit,
            t.record,
            100.0 * e.settlement.as_secs_f64() / total.as_secs_f64(),
            execution / size as u32,
            size as f64 / total.as_secs_f64(),
            size as f64 / execution.as_secs_f64(),
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
            .map(|index| {
                offer(
                    &payer(index),
                    &format!("@\"sized-{index}\"!({index})"),
                    index as i64 + 1,
                )
            })
            .collect();
        let block = block(&host, 1, deploys);
        let record = host
            .executor
            .execute(&pre_root(&host), &block)
            .await
            .expect("the block executes");
        let bytes = record.encode().len();
        println!(
            "envelopes={size} record_bytes={bytes} per_envelope={}",
            bytes / size
        );
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement: where one envelope's run time goes"]
async fn one_envelope_profile() {
    use rholang::rust::interpreter::compiler::compiler::Compiler;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    let rm = mk_runtime_manager("v6-profile", Some(Genesis::default_mergeable_tags_arc())).await;
    let host = launch(1).await;
    let root = Blake2b256Hash::from_bytes_prost(&pre_root(&host));
    let n = 32u32;
    let mut spawn = Duration::ZERO;
    let mut reset = Duration::ZERO;
    let mut checkpoint = Duration::ZERO;
    let mut compile = Duration::ZERO;
    for i in 0..n {
        let started = Instant::now();
        let mut runtime = rm.spawn_runtime().await.unwrap();
        spawn += started.elapsed();
        let started = Instant::now();
        let _ = runtime.reset(&root).await;
        reset += started.elapsed();
        let started = Instant::now();
        let _ = runtime.create_checkpoint().await;
        checkpoint += started.elapsed();
        let started = Instant::now();
        Compiler::source_to_adt(&format!("@\"one-{i}\"!({i})")).unwrap();
        compile += started.elapsed();
    }
    println!(
        "per envelope: spawn_runtime={:?} reset={:?} empty_checkpoint={:?} compile_one_send={:?}",
        spawn / n,
        reset / n,
        checkpoint / n,
        compile / n
    );

    let empty = block(&host, 1, vec![]);
    let mut close = Duration::MAX;
    for _ in 0..3 {
        let (_, t) = host
            .executor
            .execute_timed(&pre_root(&host), &empty)
            .await
            .unwrap();
        close = close.min(t.close_block);
    }
    println!("empty block: close_block={close:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement: the compute charge of representative terms"]
async fn compute_charge_corpus() {
    let host = launch(1).await;
    let other = VaultAddress::from_public_key(&Secp256k1.to_public(&payer(5)))
        .unwrap()
        .to_base58();
    let corpus: Vec<(&str, String)> = vec![
        ("nil", "Nil".to_string()),
        ("one send", "@\"c\"!(1)".to_string()),
        ("one receive", "for (_ <- @\"r\") { Nil }".to_string()),
        ("one COMM", "new x in { x!(1) | for (_ <- x) { Nil } }".to_string()),
        ("ten COMMs", "new x in { contract x(@n) = { Nil } | x!(0) | x!(1) | x!(2) | x!(3) | x!(4) | x!(5) | x!(6) | x!(7) | x!(8) | x!(9) }".to_string()),
        ("join", "new x, y in { x!(1) | y!(2) | for (_ <- x & _ <- y) { Nil } }".to_string()),
        ("peek", "new x in { x!(1) | for (_ <<- x) { Nil } }".to_string()),
        ("arithmetic", "new x in { x!(1 + 2 * 3 - 4) | for (_ <- x) { Nil } }".to_string()),
        ("list of 100", format!("@\"l\"!({:?})", (0..100).collect::<Vec<_>>())),
        ("string of 1000", format!("@\"s\"!(\"{}\")", "a".repeat(1000))),
        (
            "native transfer",
            format!("new w(`rho:rev:wallet`), d(`rho:system:deployerId`) in {{ w!(\"transfer\", (*d, \"{other}\", 10), \"out\") }}"),
        ),
        ("stack literal", "new d(`rho:system:deployerId`), slot in { slot :: slot :: d :: () }".to_string()),
    ];
    println!("| term | outcome | COMM phlo | compute phlo | log events |");
    for (i, (name, term)) in corpus.iter().enumerate() {
        let block = block(&host, 1, vec![offer(&payer(0), term, i as i64 + 1)]);
        let record = host
            .executor
            .execute(&pre_root(&host), &block)
            .await
            .expect("the corpus block executes");
        let e = &record.entries()[0].executed;
        println!(
            "| {name} | {:?} | {} | {} | {} |",
            e.outcome,
            e.comm_phlo,
            e.compute_phlo,
            e.user_log.len()
        );
    }
}

#[test]
#[ignore = "prints the keys and addresses of the CLI end-to-end run"]
fn cli_end_to_end_keys() {
    for (name, byte) in [("validator", 0x11u8), ("payer", 0x22), ("payee", 0x33)] {
        let sk = PrivateKey::from_bytes(&[byte; 32]);
        let pk = Secp256k1.to_public(&sk);
        println!(
            "{name} sk={} pk={} address={}",
            hex::encode(sk.bytes.as_ref()),
            hex::encode(pk.bytes.as_ref()),
            VaultAddress::from_public_key(&pk).unwrap().to_base58()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pooled_runtimes_give_the_records_of_fresh_ones() {
    let host = launch(4).await;
    let first = block(&host, 1, vec![
        offer(
            &payer(0),
            "new x in { x!(1) | for (_ <- x) { @\"a\"!(1) } }",
            1,
        ),
        offer(&payer(1), "@\"race\"!(1)", 2),
        offer(&payer(2), "for (@v <- @\"race\") { @\"won\"!(v) }", 3),
    ]);
    let second = block(&host, 2, vec![
        offer(
            &payer(3),
            "new x in { contract x(@n) = { Nil } | x!(1) | x!(2) }",
            4,
        ),
        offer(&payer(0), "@\"b\"!(\"x\")", 5),
    ]);
    let pooled = &host.executor;
    let a1 = pooled.execute(&pre_root(&host), &first).await.unwrap();
    let b1 = pooled.execute(&pre_root(&host), &second).await.unwrap();
    let a2 = pooled.execute(&pre_root(&host), &first).await.unwrap();
    for (x, y) in a1.entries().iter().zip(a2.entries()) {
        assert_eq!(x.executed.outcome, y.executed.outcome);
        assert_eq!(x.executed.comm_phlo, y.executed.comm_phlo, "comm");
        assert_eq!(x.executed.compute_phlo, y.executed.compute_phlo, "compute");
        let sorted = |log: &[models::rust::casper::protocol::casper_message::Event]| {
            let mut events: Vec<String> = log.iter().map(|e| format!("{e:?}")).collect();
            events.sort();
            events
        };
        assert_eq!(
            sorted(&x.executed.user_log),
            sorted(&y.executed.user_log),
            "log"
        );
        assert_eq!(x.executed.footprint, y.executed.footprint, "footprint");
    }
    assert_eq!(a1.gate(), a2.gate(), "gate");
    assert_eq!(a1.settlement(), a2.settlement(), "settlement");
    assert_eq!(a1.mid_root(), a2.mid_root());
    assert_eq!(a1.post_root(), a2.post_root());
    for record in [&a1, &b1] {
        let target = if record == &a1 { &first } else { &second };
        assert_eq!(
            pooled.replay(record, target).await.unwrap(),
            *record.post_root()
        );
    }
    let fresh = replica(&host.genesis).await;
    let b2 = fresh
        .executor
        .execute(&pre_root(&host), &second)
        .await
        .unwrap();
    assert_eq!(b2.post_root(), b1.post_root());
    assert_eq!(b2.settlement(), b1.settlement());
}

async fn outcomes_of(host: &Host, number: i64, terms: &[&str]) -> (BlockMessage, BlockRecord) {
    let deploys = terms
        .iter()
        .enumerate()
        .map(|(i, term)| offer(&payer(i), term, number * 100 + i as i64))
        .collect();
    let target = block(host, number, deploys);
    let record = host
        .executor
        .execute(&pre_root(host), &target)
        .await
        .unwrap();
    assert_eq!(
        host.executor.replay(&record, &target).await.unwrap(),
        *record.post_root()
    );
    (target, record)
}

#[tokio::test(flavor = "multi_thread")]
async fn footprints_let_commuting_runs_merge() {
    let host = launch(4).await;
    let (_, sends) = outcomes_of(&host, 1, &[
        "@\"queue\"!(\"a\")",
        "@\"queue\"!(\"b\")",
        "@\"queue\"!(\"c\")",
    ])
    .await;
    assert_eq!(outcomes(&sends), vec![EnvelopeOutcome::Completed; 3]);
    let (_, numbers) = outcomes_of(&host, 4, &["@\"cell\"!(1)", "@\"cell\"!(2)"]).await;
    assert_eq!(outcomes(&numbers), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted
    ]);
    let lookup = "new rl(`rho:registry:lookup`), ret in { rl!(`rho:vault:system`, *ret) }";
    let (_, reads) = outcomes_of(&host, 2, &[lookup, lookup]).await;
    assert_eq!(outcomes(&reads), vec![EnvelopeOutcome::Completed; 2]);
    let (_, apart) = outcomes_of(&host, 3, &[
        "for (_ <- @\"p\") { Nil }",
        "for (_ <- @\"q\") { Nil }",
    ])
    .await;
    assert_eq!(outcomes(&apart), vec![EnvelopeOutcome::Completed; 2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn footprints_reject_runs_that_touch_the_same_cell() {
    let host = launch(4).await;
    let (_, receive_then_send) = outcomes_of(&host, 1, &[
        "for (_ <- @\"r\") { Nil }",
        "@\"r\"!(1)",
        "@\"s\"!(1)",
    ])
    .await;
    assert_eq!(outcomes(&receive_then_send), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted,
        EnvelopeOutcome::Completed
    ]);
    let (_, two_receivers) = outcomes_of(&host, 2, &[
        "for (_ <- @\"t\") { Nil }",
        "for (_ <- @\"t\") { Nil }",
    ])
    .await;
    assert_eq!(outcomes(&two_receivers), vec![
        EnvelopeOutcome::Completed,
        EnvelopeOutcome::Conflicted
    ]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_record_carries_each_run_footprint_and_replay_checks_it() {
    let host = launch(4).await;
    let (target, record) =
        outcomes_of(&host, 1, &["@\"u\"!(1)", "for (_ <- @\"v\") { Nil }"]).await;
    let footprints: Vec<_> = record
        .entries()
        .iter()
        .map(|e| e.executed.footprint.clone())
        .collect();
    assert!(footprints.iter().all(|f| !f.is_empty()));
    assert!(!footprints[0].conflicts_with(&footprints[1]));
    let decoded = BlockRecord::decode(&record.encode()).unwrap();
    assert_eq!(decoded, record);
    let mut forged = record.entries().to_vec();
    forged[0].executed.footprint = footprints[1].clone();
    let forged = BlockRecord::new(
        record.target().clone(),
        record.pre_root().clone(),
        record.mid_root().clone(),
        record.post_root().clone(),
        forged,
        record.system_deploys().to_vec(),
    )
    .and_then(|r| r.with_gate(record.gate().to_vec()))
    .unwrap()
    .with_settlement(record.settlement().unwrap().clone());
    assert!(host.executor.replay(&forged, &target).await.is_err());
}

fn env_list(name: &str, default: &str) -> Vec<usize> {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(|n| n.trim().parse().expect("a list of numbers"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "scaling run: V6_SCALING_WORKERS, V6_SCALING_ENVELOPES"]
async fn scaling_by_worker_count() {
    let workers = env_list("V6_SCALING_WORKERS", "1,2,4,8,16");
    let size = env_list("V6_SCALING_ENVELOPES", "256")[0];
    let mut base: Option<f64> = None;
    for w in workers {
        let host = launch_with(size, w).await;
        let deploys = (0..size)
            .map(|index| {
                offer(
                    &payer(index),
                    &format!("@\"scaled-{index}\"!(\"{index}\")"),
                    index as i64 + 1,
                )
            })
            .collect();
        let block = block(&host, 1, deploys);
        let mut best: Option<(Duration, BlockTimings)> = None;
        for _ in 0..3 {
            let (record, t) = host
                .executor
                .execute_timed(&pre_root(&host), &block)
                .await
                .expect("the block executes");
            assert!(outcomes(&record)
                .iter()
                .all(|o| *o == EnvelopeOutcome::Completed));
            let e = t.envelopes;
            let execution = e.gate + e.runs + e.merge + e.settlement;
            if best.as_ref().is_none_or(|(b, _)| execution < *b) {
                best = Some((execution, t));
            }
        }
        let (execution, t) = best.expect("three runs");
        let e = t.envelopes;
        let rate = size as f64 / execution.as_secs_f64();
        let one = *base.get_or_insert(rate / w as f64);
        println!(
            "workers={w} envelopes={size} execution={execution:?} envelopes_per_second={rate:.2} speedup={:.2} efficiency={:.2} gate={:?} runs={:?} setup={:?} eval={:?} changes={:?} merge={:?} settlement={:?}",
            rate / one,
            rate / (one * w as f64),
            e.gate,
            e.runs,
            e.run_setup,
            e.run_eval,
            e.run_checkpoint,
            e.merge,
            e.settlement,
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_charges_do_not_depend_on_the_interleaving() {
    let host = launch(4).await;
    let racing = block(&host, 2, vec![
        offer(
            &payer(3),
            "new x in { contract x(@n) = { Nil } | x!(1) | x!(2) }",
            4,
        ),
        offer(
            &payer(0),
            "new x in { x!(1) | x!(2) | for (_ <- x) { Nil } | for (_ <- x) { Nil } }",
            5,
        ),
        offer(
            &payer(1),
            "new x in { for (_ <= x) { Nil } | x!(1) | x!(2) | x!(3) }",
            6,
        ),
    ]);
    let mut charged: Vec<std::collections::BTreeSet<u64>> = vec![Default::default(); 3];
    for _ in 0..30 {
        let record = host
            .executor
            .execute(&pre_root(&host), &racing)
            .await
            .unwrap();
        for (seen, entry) in charged.iter_mut().zip(record.entries()) {
            seen.insert(entry.executed.compute_phlo);
        }
        assert_eq!(
            host.executor.replay(&record, &racing).await.unwrap(),
            *record.post_root()
        );
    }
    assert!(charged.iter().all(|seen| seen.len() == 1), "{charged:?}");
}

async fn chain(host: &Host, blocks: &[&[&str]]) -> Vec<BlockRecord> {
    let mut root = pre_root(host);
    let mut records = Vec::new();
    for (n, terms) in blocks.iter().enumerate() {
        let number = n as i64 + 1;
        let deploys = terms
            .iter()
            .enumerate()
            .map(|(i, term)| offer(&payer(i), term, number * 100 + i as i64))
            .collect();
        let target = block(host, number, deploys);
        let record = host.executor.execute(&root, &target).await.unwrap();
        assert_eq!(
            host.executor.replay(&record, &target).await.unwrap(),
            *record.post_root()
        );
        root = record.post_root().clone();
        records.push(record);
    }
    records
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_contract_that_sends_the_same_datum_on_each_firing_keeps_merging() {
    let host = launch(4).await;
    let records = chain(&host, &[
        &["contract @\"c\"(_) = { @\"ran\"!(1) }"],
        &["@\"c\"!(0)"],
        &["@\"c\"!(0)"],
        &["@\"c\"!(0)"],
        &["@\"c\"!(0)", "@\"c\"!(0)"],
    ])
    .await;
    for record in &records[..4] {
        assert_eq!(outcomes(record), vec![EnvelopeOutcome::Completed]);
    }
    assert_eq!(outcomes(&records[4]), vec![EnvelopeOutcome::Completed; 2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn replay_refuses_a_record_whose_surface_attribution_is_forged() {
    let host = launch(4).await;
    let (store_block, stored) = outcomes_of(&host, 1, &["contract @\"svc\"(_) = { Nil }"]).await;
    drop(store_block);
    let call = offer(&payer(1), "@\"svc\"!(0)", 201);
    let target = block(&host, 2, vec![call]);
    let record = host
        .executor
        .execute(stored.post_root(), &target)
        .await
        .unwrap();
    assert_eq!(
        host.executor.replay(&record, &target).await.unwrap(),
        *record.post_root()
    );
    assert_eq!(record.entries()[0].executed.surfaces.len(), 2);
    let mut forged = record.entries().to_vec();
    forged[0].executed.surfaces.retain(|(s, _)| {
        s.as_ref()
            .is_some_and(|s| s.envelope.as_slice() == forged_sig(&target))
    });
    let forged = BlockRecord::new(
        record.target().clone(),
        record.pre_root().clone(),
        record.mid_root().clone(),
        record.post_root().clone(),
        forged,
        record.system_deploys().to_vec(),
    )
    .and_then(|r| r.with_gate(record.gate().to_vec()))
    .unwrap()
    .with_settlement(record.settlement().unwrap().clone());
    assert!(host.executor.replay(&forged, &target).await.is_err());
}

fn forged_sig(target: &BlockMessage) -> Vec<u8> { target.body.deploys[0].deploy.sig.to_vec() }
