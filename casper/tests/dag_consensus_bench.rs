#[allow(dead_code)]
#[path = "helper/test_node.rs"]
pub mod fixture_test_node;
#[allow(dead_code)]
#[path = "util/genesis_builder.rs"]
pub mod fixture_genesis_builder;
#[allow(dead_code)]
#[path = "util/rholang/resources.rs"]
pub mod fixture_resources;
#[allow(dead_code)]
#[path = "util/comm/transport_layer_test_impl.rs"]
pub mod fixture_transport;

mod helper {
    pub use crate::fixture_test_node as test_node;
}

mod util {
    pub use crate::fixture_genesis_builder as genesis_builder;
    pub mod rholang {
        pub use crate::fixture_resources as resources;
    }
    pub mod comm {
        pub use crate::fixture_transport as transport_layer_test_impl;
    }
}

#[path = "dag_consensus_bench/backend.rs"]
mod backend;

use std::collections::HashMap;
use std::time::Instant;

use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::util::construct_deploy;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;
use serde::Serialize;
use util::genesis_builder::{GenesisBuilder, GenesisContext};

pub fn init_logger() { shared::rust::tracing_init::init_for_tests(); }

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Frame {
    height: i64,
    block_hash: String,
    parents: Vec<String>,
    post_state: String,
    finalized: String,
    finalized_height: i64,
}

#[derive(Serialize)]
struct Latency {
    count: usize,
    p50_ns: u64,
    p95_ns: u64,
    max_ns: u64,
}

fn latency(samples: &[u64]) -> Latency {
    assert!(!samples.is_empty());
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Latency {
        count: sorted.len(),
        p50_ns: sorted[(sorted.len() * 50).div_ceil(100) - 1],
        p95_ns: sorted[(sorted.len() * 95).div_ceil(100) - 1],
        max_ns: *sorted.last().unwrap(),
    }
}

fn count(name: &str, default: usize) -> usize {
    let value = std::env::var(name)
        .map(|v| v.parse().expect("integer benchmark count"))
        .unwrap_or(default);
    assert!((1..=10_000).contains(&value), "{name} must be 1..=10000");
    value
}

async fn genesis() -> GenesisContext {
    let validators: Vec<_> = (1..=4)
        .map(|seed| {
            let secret = PrivateKey::from_bytes(&[seed; 32]);
            let public = Secp256k1.to_public(&secret);
            (secret, public)
        })
        .collect();
    let bonds: HashMap<_, _> = validators
        .iter()
        .map(|(_, public)| (public.clone(), 10))
        .collect();
    let mut parameters = GenesisBuilder::build_genesis_parameters(validators.clone(), &bonds);
    parameters.1 = vec![
        (
            construct_deploy::DEFAULT_SEC.clone(),
            construct_deploy::DEFAULT_PUB.clone(),
        ),
        (
            construct_deploy::DEFAULT_SEC2.clone(),
            construct_deploy::DEFAULT_PUB2.clone(),
        ),
    ];
    parameters.2.vaults = parameters
        .1
        .iter()
        .map(|(_, public)| Vault {
            vault_address: VaultAddress::from_public_key(public).unwrap(),
            initial_balance: 900_000_000,
        })
        .chain(validators.iter().map(|(_, public)| Vault {
            vault_address: VaultAddress::from_public_key(public).unwrap(),
            initial_balance: 0,
        }))
        .collect();
    parameters
        .2
        .proof_of_stake
        .validators
        .sort_by(|a, b| a.pk.bytes.cmp(&b.pk.bytes));
    GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap()
}

async fn run(
    genesis: &GenesisContext,
    window: i64,
    depth: i32,
    warmup: usize,
    samples: usize,
) -> (serde_json::Value, Vec<Frame>) {
    let run_started = Instant::now();
    eprintln!("DAG_CONSENSUS_STAGE network_start window={window} depth={depth}");
    let mut nodes = backend::network(genesis.clone(), window, depth).await;
    eprintln!(
        "DAG_CONSENSUS_STAGE network_ready elapsed_s={:.3}",
        run_started.elapsed().as_secs_f64()
    );
    for node in &mut nodes {
        node.allow_empty_blocks = true;
    }
    let mut proposal_ns = Vec::with_capacity(samples);
    let mut validation_ns = Vec::with_capacity(samples * 3);
    let mut trace: Vec<Frame> = Vec::with_capacity(warmup + samples);
    let mut maximum_resident = 0;
    let mut previous_resident = backend::resident_blocks(&nodes[0]);
    let mut measured_cold_proposals = 0;
    for step in 0..warmup + samples {
        if step >= warmup && previous_resident < step + 1 {
            measured_cold_proposals += 1;
        }
        let proposer = step % nodes.len();
        let timestamp = 1_700_000_000_000i64 + step as i64 * 1000;
        let height = trace.last().map_or(0, |frame| frame.height);
        let deploy = construct_deploy::source_deploy(
            format!("@\"dag-consensus-bench-{step}\"!({step})"),
            timestamp,
            Some(90_000),
            Some(1),
            Some(construct_deploy::DEFAULT_SEC.clone()),
            Some(height),
            Some(genesis.genesis_block.shard_id.clone()),
        )
        .unwrap();
        let admitted = nodes[proposer].casper.deploy(deploy).unwrap();
        assert!(
            matches!(admitted, Either::Right(_)),
            "deploy admission: {admitted:?}"
        );
        let started = Instant::now();
        let snapshot = nodes[proposer].casper.get_snapshot().await.unwrap();
        let validator = nodes[proposer].casper.get_validator().unwrap();
        let created = block_creator::create_at(
            &snapshot,
            &validator,
            nodes[proposer].deploy_storage.clone(),
            nodes[proposer].rejected_deploy_buffer.clone(),
            &nodes[proposer].runtime_manager,
            &mut nodes[proposer].block_store.clone(),
            DeploySelection::StandardAllowEmpty,
            timestamp,
        )
        .await
        .unwrap();
        let elapsed = started.elapsed().as_nanos() as u64;
        let BlockCreatorResult::Created(block, ..) = created else {
            panic!("creation: {created:?}");
        };
        if step >= warmup {
            proposal_ns.push(elapsed);
        }
        for (index, node) in nodes.iter_mut().enumerate() {
            let started = Instant::now();
            let result = node.process_block(block.clone()).await.unwrap();
            let elapsed = started.elapsed().as_nanos() as u64;
            assert!(
                matches!(result, Either::Right(_)),
                "validation node{index}: {result:?}"
            );
            if step >= warmup && index != proposer {
                validation_ns.push(elapsed);
            }
        }
        let mut finalized = None;
        for node in &nodes {
            let lfb = node.casper.finalize().await.unwrap();
            let identity = (hex::encode(&lfb.block_hash), lfb.body.state.block_number);
            if let Some(expected) = &finalized {
                assert_eq!(expected, &identity, "peer finality diverged");
            }
            finalized = Some(identity);
        }
        let (finalized, finalized_height) = finalized.unwrap();
        previous_resident = backend::resident_blocks(&nodes[0]);
        maximum_resident = maximum_resident.max(previous_resident);
        if step % 32 == 0 || step + 1 == warmup || step + 1 == warmup + samples {
            eprintln!("DAG_CONSENSUS_STAGE progress proposals={} lfb_height={finalized_height} resident={previous_resident} elapsed_s={:.3}", step + 1, run_started.elapsed().as_secs_f64());
        }
        trace.push(Frame {
            height: block.body.state.block_number,
            block_hash: hex::encode(&block.block_hash),
            parents: block
                .header
                .parents_hash_list
                .iter()
                .map(hex::encode)
                .collect(),
            post_state: hex::encode(&block.body.state.post_state_hash),
            finalized,
            finalized_height,
        });
    }
    let resident = backend::resident_blocks(&nodes[0]);
    let held = warmup + samples + 1;
    let cold = held.saturating_sub(resident);
    let lfb_height = trace.last().unwrap().finalized_height;
    if backend::bounded_backend() && lfb_height > window {
        assert!(cold > 0, "finality crossed the hot window without eviction");
    }
    let trace_digest = hex::encode(Blake2b256::hash(serde_json::to_vec(&trace).unwrap()));
    let result = serde_json::json!({
        "window": window, "parent_depth": depth, "warmup_proposals": warmup,
        "proposal": latency(&proposal_ns), "peer_validation": latency(&validation_ns),
        "held_blocks": held, "resident_blocks": resident, "cold_blocks": cold,
        "max_resident_blocks": maximum_resident, "lfb_height": lfb_height,
        "measured_cold_proposals": measured_cold_proposals,
        "cold_window_exercised_in_every_sample": measured_cold_proposals == samples,
        "trace_digest": trace_digest, "genesis_hash": hex::encode(&genesis.genesis_block.block_hash),
    });
    eprintln!("DAG_CONSENSUS_RESULT {result}");
    (result, trace)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit deterministic consensus latency and eviction benchmark"]
async fn deterministic_consensus_latency_matrix() {
    let mode = std::env::var("DAG_CONSENSUS_MODE").unwrap_or_else(|_| "correctness".into());
    assert!(["correctness", "performance", "smoke"].contains(&mode.as_str()));
    let performance = mode == "performance";
    let warmup = count("DAG_CONSENSUS_WARMUP", if performance { 1088 } else { 24 });
    let samples = count("DAG_CONSENSUS_SAMPLES", if performance { 100 } else { 4 });
    let windows: Vec<i64> = std::env::var("DAG_CONSENSUS_WINDOW")
        .map(|v| vec![v.parse().expect("window")])
        .unwrap_or_else(|_| {
            if performance {
                vec![1024]
            } else {
                vec![1024, 1, 2, 16]
            }
        });
    let depths: Vec<i32> = std::env::var("DAG_CONSENSUS_DEPTH")
        .map(|v| vec![v.parse().expect("depth")])
        .unwrap_or_else(|_| {
            if performance {
                vec![15]
            } else {
                vec![15, 64, i32::MAX]
            }
        });
    let genesis_started = Instant::now();
    eprintln!("DAG_CONSENSUS_STAGE genesis_start");
    let genesis = genesis().await;
    eprintln!(
        "DAG_CONSENSUS_STAGE genesis_ready elapsed_s={:.3}",
        genesis_started.elapsed().as_secs_f64()
    );
    let single_configuration = windows.len() == 1 && depths.len() == 1;
    let mut results = Vec::new();
    for depth in depths {
        let mut expected = None;
        for &window in &windows {
            let (result, trace) = run(&genesis, window, depth, warmup, samples).await;
            if let Some(expected) = &expected {
                assert_eq!(
                    expected, &trace,
                    "consensus differs for window{window}, depth{depth}"
                );
            } else {
                expected = Some(trace);
            }
            results.push(result);
        }
    }
    let artifact = serde_json::json!({
        "variant": std::env::var("DAG_CONSENSUS_VARIANT").unwrap_or_else(|_| "windowed".into()),
        "mode": mode,
        "optimized_build": !cfg!(debug_assertions),
        "latency_release_sample_count_met": samples >= 100,
        "single_configuration_process": single_configuration,
        "latency_release_process_shape_met": performance && single_configuration && samples >= 100 && !cfg!(debug_assertions),
        "results": results,
    });
    if let Ok(path) = std::env::var("DAG_CONSENSUS_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    }
}
