// Standalone benchmark: measures wall-clock time for N concurrent par-branches
// each performing OPS produce() calls on private channels.
//
// Run with CPU flame graph:
//   cargo flamegraph --bin bench_par_branches
//
// Run with tokio-console (async task states):
//   RUSTFLAGS="--cfg tokio_unstable" cargo run --bin bench_par_branches
//   # then: tokio-console in another terminal
//
// Run with off-CPU profiling on Linux:
//   cargo build --release --bin bench_par_branches
//   /usr/share/bcc/tools/offcputime -p $(./target/release/bench_par_branches &; echo $!) 10 \
//     | flamegraph.pl > offcpu.svg

use std::sync::Arc;
use std::time::Instant;

use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct Wildcard;

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct Cont;

struct AlwaysMatch;

impl Match<Wildcard, String> for AlwaysMatch {
    fn get(&self, _: Wildcard, a: String) -> Option<String> { Some(a) }
}

type Space = RSpace<String, Wildcard, String, Cont>;

async fn make_rspace() -> Space {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    RSpace::create(store, Arc::new(Box::new(AlwaysMatch))).unwrap()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let branches: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(32);
    let ops_per_branch: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    println!("bench_par_branches: branches={branches}  ops_per_branch={ops_per_branch}  total_ops={}", branches * ops_per_branch);

    // Baseline: sequential, one branch at a time
    let seq_space = make_rspace().await;
    let t_seq = Instant::now();
    for b in 0..branches {
        for i in 0..ops_per_branch {
            seq_space.produce(format!("seq_{}_{}", b, i), "datum".to_string(), false).await.unwrap();
        }
    }
    let seq_ms = t_seq.elapsed().as_millis();

    // Concurrent: all branches in parallel tokio tasks
    let par_space = Arc::new(make_rspace().await);
    let t_par = Instant::now();
    let handles: Vec<_> = (0..branches)
        .map(|b| {
            let s = par_space.clone();
            tokio::spawn(async move {
                for i in 0..ops_per_branch {
                    s.produce(format!("par_{}_{}", b, i), "datum".to_string(), false).await.unwrap();
                }
            })
        })
        .collect();
    for h in handles { h.await.unwrap(); }
    let par_ms = t_par.elapsed().as_millis();

    let speedup = seq_ms as f64 / par_ms as f64;

    println!("sequential:  {seq_ms} ms");
    println!("concurrent:  {par_ms} ms");
    println!("speedup:     {speedup:.2}x  (ideal: {branches}x)");
    println!("efficiency:  {:.1}%  (speedup / branches * 100)", speedup / branches as f64 * 100.0);
}
