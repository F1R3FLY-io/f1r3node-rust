// How the per-block cost of equivocation handling grows with the number of
// equivocations a shard has ever stored, when the equivocators have since
// left the committee and the current committee holds the stake.

use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::CasperSnapshot;
use casper::rust::equivocation_detector::EquivocationDetector;
use casper::rust::slashing_authorization::authorized_slash_candidates;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use num_bigint::BigInt;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

const FORMER_VALIDATORS: usize = 100;
const COMMITTEE: usize = 4;
const SAMPLES: usize = 20;

fn validator(group: u8, index: usize) -> Validator {
    let mut key = vec![group; 65];
    key[1..9].copy_from_slice(&(index as u64).to_be_bytes());
    Bytes::from(key)
}

fn block(
    height: i64,
    seq_num: i32,
    sender: &Validator,
    nonce: u8,
    parents: Vec<BlockHash>,
    justifications: Vec<Justification>,
) -> BlockMessage {
    let mut hash = [0u8; 32];
    hash[..8].copy_from_slice(&height.to_be_bytes());
    hash[8..16].copy_from_slice(&sender[1..9]);
    hash[16] = sender[0];
    hash[17] = nonce;
    let hash = Bytes::copy_from_slice(&hash);
    get_random_block(
        Some(height),
        Some(seq_num),
        Some(Bytes::from(vec![0; 32])),
        Some(Bytes::from(vec![0; 32])),
        Some(sender.clone()),
        Some(1),
        Some(height),
        Some(parents),
        Some(justifications),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some("root".to_string()),
        Some(Box::new(move |_| hash.clone())),
    )
}

fn report(operation: &str, invalid_blocks: usize, mut samples: Vec<u128>) {
    samples.sort_unstable();
    tracing::info!(
        target: "f1r3fly.bench.equivocation_scaling",
        operation,
        invalid_blocks,
        samples = samples.len(),
        median_ns = samples[samples.len() / 2] as u64,
        p95_ns = samples[(samples.len() * 95).div_ceil(100) - 1] as u64,
        "equivocation scaling"
    );
}

async fn measure(invalid_blocks: usize) {
    let mut kvm = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
    let genesis = block(0, 0, &validator(0xEE, 0), 0, Vec::new(), Vec::new());
    block_store.put_block_message(&genesis).unwrap();
    storage.insert(&genesis, InsertMode::Approved).unwrap();

    let mut tip = genesis.block_hash.clone();
    for index in 0..invalid_blocks {
        let sender = validator(0xAA, index % FORMER_VALIDATORS);
        let seq_num = (index / FORMER_VALIDATORS) as i32 + 1;
        let height = index as i64 + 1;
        let first = block(height, seq_num, &sender, 1, vec![tip.clone()], Vec::new());
        let twin = block(height, seq_num, &sender, 2, vec![tip.clone()], Vec::new());
        for b in [&first, &twin] {
            block_store.put_block_message(b).unwrap();
        }
        storage.insert(&first, InsertMode::Normal).unwrap();
        storage
            .insert(&twin, InsertMode::Equivocation {
                sibling: first.block_hash.clone(),
            })
            .unwrap();
        tip = first.block_hash;
    }

    let committee: Vec<Validator> = (0..COMMITTEE).map(|i| validator(0xCC, i)).collect();
    let bonds: Vec<Bond> = committee
        .iter()
        .map(|v| Bond {
            validator: v.clone(),
            stake: 100u64.into(),
        })
        .collect();
    let slashable: HashMap<Validator, BigInt> = committee
        .iter()
        .map(|v| (v.clone(), BigInt::from(100)))
        .collect();
    let judged = block(
        invalid_blocks as i64 + 1,
        1,
        &committee[0],
        0,
        vec![tip.clone()],
        Vec::new(),
    );

    let mut snapshot = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        black_box(storage.get_representation().unwrap());
        snapshot.push(started.elapsed().as_nanos());
    }
    report("snapshot", invalid_blocks, snapshot);

    let dag = storage.get_representation().unwrap();
    let mut neglect = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        black_box(
            EquivocationDetector::check_neglected_equivocations(
                &judged,
                &dag,
                &block_store,
                &genesis,
                &bonds,
            )
            .unwrap(),
        );
        neglect.push(started.elapsed().as_nanos());
    }
    report("neglect_check", invalid_blocks, neglect);

    let casper_snapshot = CasperSnapshot::new(dag, Bytes::new());
    let mut candidates = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        black_box(authorized_slash_candidates(&casper_snapshot, &block_store, &slashable).unwrap());
        candidates.push(started.elapsed().as_nanos());
    }
    report("slash_candidates", invalid_blocks, candidates);

    let mut insert = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let sender = validator(0xAB, sample);
        let height = invalid_blocks as i64 + 2 + sample as i64;
        let first = block(height, 1, &sender, 1, vec![tip.clone()], Vec::new());
        let twin = block(height, 1, &sender, 2, vec![tip.clone()], Vec::new());
        storage.insert(&first, InsertMode::Normal).unwrap();
        let started = Instant::now();
        storage
            .insert(&twin, InsertMode::Equivocation {
                sibling: first.block_hash.clone(),
            })
            .unwrap();
        insert.push(started.elapsed().as_nanos());
    }
    report("equivocation_insert", invalid_blocks, insert);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "Release measurement; run explicitly with --ignored --nocapture"]
async fn equivocation_handling_cost_by_stored_equivocations() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("f1r3fly.bench.equivocation_scaling=info")
        .try_init();
    for invalid_blocks in [0, 1_000, 4_000, 8_000] {
        measure(invalid_blocks).await;
    }
}
