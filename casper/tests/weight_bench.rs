use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use casper::rust::estimator::Estimator;
use casper::rust::safety::clique_oracle::{CliqueOracle, FtThreshold};
use casper::rust::util::clique::Clique;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

fn block(
    height: i64,
    sender: &Validator,
    parents: Vec<BlockHash>,
    justifications: Vec<Justification>,
    bonds: &[Bond],
) -> BlockMessage {
    let mut hash = [0u8; 32];
    hash[..8].copy_from_slice(&height.to_be_bytes());
    hash[8..].copy_from_slice(&sender[..24]);
    let hash = Bytes::copy_from_slice(&hash);
    get_random_block(
        Some(height),
        Some(height as i32),
        Some(Bytes::from(vec![0; 32])),
        Some(Bytes::from(vec![0; 32])),
        Some(sender.clone()),
        Some(1),
        Some(height),
        Some(parents),
        Some(justifications),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(bonds.to_vec()),
        Some("root".to_string()),
        Some(Box::new(move |_| hash.clone())),
    )
}

fn measurement(operation: &str, validators: usize, wide: bool, mut samples: Vec<u128>) {
    samples.sort_unstable();
    println!(
        "WEIGHT_BENCH operation={operation} validators={validators} wide={wide} samples={} median_ns={} p95_ns={}",
        samples.len(),
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1],
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "Release measurement; run explicitly with --ignored --nocapture"]
async fn consensus_weight_benchmark() {
    let validators: usize = std::env::var("WEIGHT_BENCH_VALIDATORS")
        .unwrap_or_else(|_| "100".to_string())
        .parse()
        .expect("validator count");
    assert!([4, 32, 100].contains(&validators));
    let wide = std::env::var("WEIGHT_BENCH_WIDE").is_ok_and(|value| value == "1");
    let keys: Vec<Validator> = (0..validators)
        .map(|index| {
            Secp256k1
                .to_public(&PrivateKey::from_bytes(&[index as u8 + 1; 32]))
                .bytes
        })
        .collect();
    let bonds: Vec<Bond> = keys
        .iter()
        .enumerate()
        .map(|(index, validator)| {
            let multiplier = index as i64 % 7 + 1;
            let stake = if wide {
                let value = (num_bigint::BigInt::from(1) << 252usize) * multiplier;
                value.to_string().parse().expect("wide stake support")
            } else {
                multiplier.into()
            };
            Bond {
                validator: validator.clone(),
                stake,
            }
        })
        .collect();
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new(&mut manager)
        .await
        .expect("DAG storage");
    let genesis = block(0, &keys[0], Vec::new(), Vec::new(), &bonds);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let floor = BlockMetadata::from_block(&genesis, false, None, None);
    let mut latest = vec![genesis.block_hash.clone(); validators];
    let mut target = genesis.block_hash.clone();
    for height in 1..=8 {
        let justifications = keys
            .iter()
            .zip(&latest)
            .take(if height == 1 { 1 } else { validators })
            .map(|(validator, hash)| Justification {
                validator: validator.clone(),
                latest_block_hash: hash.clone(),
            })
            .collect::<Vec<_>>();
        let mut parents = latest.clone();
        parents.dedup();
        let round = keys
            .iter()
            .map(|validator| {
                block(
                    height,
                    validator,
                    parents.clone(),
                    justifications.clone(),
                    &bonds,
                )
            })
            .collect::<Vec<_>>();
        for value in &round {
            storage.insert(value, InsertMode::Normal).unwrap();
        }
        latest = round.iter().map(|value| value.block_hash.clone()).collect();
        if height == 1 {
            target = latest[0].clone();
        }
    }
    let mut dag = storage.get_representation().unwrap();
    let snapshot: BTreeMap<_, _> = keys.iter().cloned().zip(latest.iter().cloned()).collect();
    let latest_messages: HashMap<_, _> = snapshot.clone().into_iter().collect();
    let weights: HashMap<_, _> = bonds
        .iter()
        .enumerate()
        .map(|(index, bond)| (index, bond.stake.clone()))
        .collect();
    let mut clique_edges = Vec::new();
    for a in 0..validators {
        for b in a + 1..validators {
            if a % 3 == b % 3 {
                clique_edges.push((a, b));
            }
        }
    }
    let estimator = Estimator::apply();
    let mut fork_samples = Vec::new();
    let mut oracle_samples = Vec::new();
    let mut clique_samples = Vec::new();
    let mut expected_tips = None;
    let mut expected_clique = None;
    for iteration in 0..26 {
        let messages = latest_messages.clone();
        let start = Instant::now();
        let choice = estimator
            .tips_with_latest_messages(&mut dag, &floor, messages, i32::MAX, None)
            .await
            .unwrap();
        let elapsed = start.elapsed().as_nanos();
        if let Some(expected) = &expected_tips {
            assert_eq!(&choice.tips, expected);
        } else {
            println!("WEIGHT_BENCH head={}", hex::encode(&choice.tips[0]));
            expected_tips = Some(choice.tips.clone());
        }
        black_box(choice);
        if iteration >= 5 {
            fork_samples.push(elapsed);
        }

        let start = Instant::now();
        let verdict = CliqueOracle::ft_witnessed_exact(
            &target,
            &dag,
            &snapshot,
            FtThreshold::from_ppm(100_000),
            false,
        )
        .await
        .unwrap();
        let elapsed = start.elapsed().as_nanos();
        assert!(black_box(verdict));
        if iteration >= 5 {
            oracle_samples.push(elapsed);
        }

        let start = Instant::now();
        let weight = Clique::find_maximum_clique_by_weight(&clique_edges, &weights);
        let elapsed = start.elapsed().as_nanos();
        if let Some(expected) = &expected_clique {
            assert_eq!(&weight, expected);
        } else {
            println!("WEIGHT_BENCH clique_weight={weight}");
            expected_clique = Some(weight.clone());
        }
        black_box(weight);
        if iteration >= 5 {
            clique_samples.push(elapsed);
        }
    }
    measurement("fork_choice", validators, wide, fork_samples);
    measurement("clique_oracle", validators, wide, oracle_samples);
    measurement("clique_search", validators, wide, clique_samples);
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(peak) = status.lines().find(|line| line.starts_with("VmHWM:")) {
            println!("WEIGHT_BENCH peak_memory={peak}");
        }
    }
}
