//! Measurement, not behaviour: what the merge pays to fold chain event logs and
//! to clone chains, on chains indexed from real fee-paying blocks.
//!
//! - Fold: a pairwise left fold through `EventLogIndex::combine` builds a new
//!   index holding the whole accumulator for every input, as the merge folded
//!   before; `EventLogIndex::combine_all` extends one accumulator in place. Both
//!   paths do the same per-element work, so the difference is the rebuilding.
//! - Clone: a deep copy duplicates a chain's three event-log indices and its
//!   state changes, as every chain clone did before they were shared; a chain
//!   clone now shares them.
//!
//! Run: cargo test -p casper --release --test merge_fold_bench -- --include-ignored --nocapture

use std::hint::black_box;
use std::time::Instant;

use casper::rust::merging::block_index;
use casper::rust::merging::deploy_chain_index::DeployChainIndex;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use tracing::Level;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, Registry};

#[allow(dead_code)]
mod util {
    pub mod genesis_builder;
    pub mod rholang {
        pub mod resources;
    }
}

use util::genesis_builder::GenesisContext;
use util::rholang::resources::with_runtime_manager;

pub fn init_logger() {}

const REPORT_TARGET: &str = "merge_fold_bench";
const BLOCKS: usize = 40;
const DEPLOYS_PER_BLOCK: usize = 4;
const REPEATS: usize = 21;
const CHAIN_COUNTS: [usize; 4] = [5, 11, 22, 44];

fn install_subscriber() {
    let report = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(
            Targets::new()
                .with_default(Level::WARN)
                .with_target(REPORT_TARGET, Level::INFO),
        );
    tracing::subscriber::set_global_default(Registry::default().with(report))
        .expect("global subscriber already set");
}

async fn index_real_blocks(
    runtime_manager: &RuntimeManager,
    genesis_context: &GenesisContext,
    genesis_block: &BlockMessage,
) -> Vec<DeployChainIndex> {
    let keys = [
        construct_deploy::DEFAULT_SEC.clone(),
        construct_deploy::DEFAULT_SEC2.clone(),
    ];
    let validator = genesis_context.validator_pks()[0].clone();
    let mut state = genesis_block.body.state.post_state_hash.clone();
    let mut chains = Vec::new();

    for block in 0..BLOCKS {
        let deploys: Vec<_> = (0..DEPLOYS_PER_BLOCK)
            .map(|d| {
                let i = block * DEPLOYS_PER_BLOCK + d;
                construct_deploy::source_deploy_now_full(
                    format!("@{i}!({i})"),
                    Some(100_000),
                    None,
                    Some(keys[d % keys.len()].clone()),
                    Some(i as i64),
                    None,
                )
                .expect("deploy")
            })
            .collect();
        let number = block as i64 + 1;
        let seq_num = block as i32 + 1;
        let block_data = BlockData {
            time_stamp: deploys[0].data.time_stamp,
            block_number: number,
            sender: validator.clone(),
            seq_num,
        };
        let (post_state, processed, system) = runtime_manager
            .compute_state(
                &state,
                deploys,
                Vec::<SystemDeployEnum>::new(),
                block_data,
                None,
            )
            .await
            .expect("compute_state");
        assert!(
            processed.iter().all(|pd| !pd.is_failed),
            "every deploy in block {block} must succeed"
        );
        let mergeable = runtime_manager
            .load_mergeable_channels(&post_state, validator.bytes.clone(), seq_num)
            .expect("mergeable channels saved by compute_state");
        let index = block_index::new(
            &Bytes::from(vec![block as u8; 32]),
            number,
            &processed,
            &system,
            &Blake2b256Hash::from_bytes_prost(&state),
            &Blake2b256Hash::from_bytes_prost(&post_state),
            &runtime_manager.history_repo,
            &mergeable,
        )
        .expect("block index");
        chains.extend(index.deploy_chains);
        state = post_state;
    }
    chains
}

fn index_size(index: &EventLogIndex) -> usize {
    index.produces_linear.0.len()
        + index.produces_persistent.0.len()
        + index.produces_consumed.0.len()
        + index.produces_peeked.0.len()
        + index.produces_copied_by_peek.0.len()
        + index.produces_touching_base_joins.0.len()
        + index.consumes_linear_and_peeks.0.len()
        + index.consumes_persistent.0.len()
        + index.consumes_produced.0.len()
        + index.produces_mergeable.0.len()
        + index.consumes_mergeable.0.len()
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in timings"));
    values[values.len() / 2]
}

/// Median wall time of `REPEATS` calls, including dropping the result, since
/// the merge pays for both.
fn median_ms<T>(mut f: impl FnMut() -> T) -> f64 {
    let times = (0..REPEATS)
        .map(|_| {
            let started = Instant::now();
            let result = f();
            black_box(&result);
            drop(result);
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    median(times)
}

fn pairwise_fold(chains: &[DeployChainIndex]) -> EventLogIndex {
    chains.iter().fold(EventLogIndex::empty(), |acc, chain| {
        EventLogIndex::combine(&acc, &chain.event_log_index).expect("combine")
    })
}

fn in_place_fold(chains: &[DeployChainIndex]) -> EventLogIndex {
    EventLogIndex::combine_all(chains.iter().map(|chain| chain.event_log_index.as_ref()))
        .expect("combine_all")
}

fn deep_clone(chains: &[DeployChainIndex]) -> Vec<impl Sized> {
    chains
        .iter()
        .map(|chain| {
            (
                chain.clone(),
                (*chain.user_event_log_index).clone(),
                (*chain.system_event_log_index).clone(),
                (*chain.event_log_index).clone(),
                (*chain.state_changes).clone(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn measure_merge_fold_and_chain_clone() {
    install_subscriber();

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let chains =
                index_real_blocks(&runtime_manager, &genesis_context, &genesis_block).await;
            let mut sizes: Vec<f64> = chains
                .iter()
                .map(|c| index_size(&c.event_log_index) as f64)
                .collect();
            let mut datums: Vec<f64> = chains
                .iter()
                .map(|c| c.state_changes.datums_changes.len() as f64)
                .collect();
            let mut deploys: Vec<f64> = chains
                .iter()
                .map(|c| c.deploys_with_cost.0.len() as f64)
                .collect();
            tracing::info!(
                target: REPORT_TARGET,
                "{} chains from {BLOCKS} blocks of {DEPLOYS_PER_BLOCK} deploys | per chain p50: \
                 {:.0} deploys, {:.0} event-log index entries, {:.0} datum changes",
                chains.len(),
                median(std::mem::take(&mut deploys)),
                median(std::mem::take(&mut sizes)),
                median(std::mem::take(&mut datums)),
            );

            for n in CHAIN_COUNTS.into_iter().filter(|n| *n <= chains.len()) {
                let sample = &chains[..n];
                assert_eq!(
                    pairwise_fold(sample),
                    in_place_fold(sample),
                    "both folds must produce the same index"
                );
                let pairwise = median_ms(|| pairwise_fold(sample));
                let in_place = median_ms(|| in_place_fold(sample));
                let deep = median_ms(|| deep_clone(sample));
                let shared = median_ms(|| sample.to_vec());
                tracing::info!(
                    target: REPORT_TARGET,
                    "N={n:>3} | fold: pairwise {pairwise:8.3} ms, in place {in_place:8.3} ms \
                     ({:5.1}x) | clone: deep {deep:8.3} ms, shared {shared:8.3} ms ({:6.1}x)",
                    pairwise / in_place,
                    deep / shared,
                );
            }
        },
    )
    .await
    .expect("runtime manager");
}
