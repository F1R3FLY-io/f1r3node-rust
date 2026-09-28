// Variant of bench_par_branches: each of the N branches reuses ONE channel
// for all of its ops_per_branch produce() calls, instead of a brand-new
// channel per op.
//
// Why this exists (see issue #50 follow-up investigation): bench_par_branches
// gives every single operation a unique channel, which means every
// get_joins() call misses HotStore's in-memory `joins` cache and falls
// through to a real history-store read (confirmed via the
// hot-store.get_joins.history_fill metric: one fill per call, a 100% miss
// rate). The real rholang-par contract does not do this — each branch's
// `loop` channel is created once (`new loop in { contract loop(@n) = ... }`)
// and reused for every recursive `loop!(n-1)` within that branch, so the real
// cache-miss rate is ~N/(N*iters), not 100%. bench_par_branches' "no
// speedup" reading may therefore be measuring a benchmark artifact rather
// than a real rspace-layer bottleneck. This variant isolates the effect of
// that one variable (channel reuse) while keeping everything else identical
// to bench_par_branches, so the two can be compared directly at the same
// branch/op counts.
//
// Produce-only, like bench_par_branches: nothing consumes, so HotStore::
// put_datum's per-channel Vec accumulates unboundedly. At large
// ops_per_branch (e.g. 32x50_000) that Vec::insert(0, ...) O(n) cost starts
// to dominate and speedup drops sharply — that's this benchmark's own
// artifact, not the real contract's behavior (its persistent `contract
// loop(@n)` consumes every produce immediately, so stored data stays near-
// zero). The issue #50 headline comparison uses 32x5_000 (this file's
// default), where it doesn't yet dominate.
//
// Run with CPU flame graph:
//   cargo flamegraph --bin bench_par_branches_channel_reuse

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

impl Match<Wildcard, String, Cont> for AlwaysMatch {
    fn get(&self, _: &Wildcard, a: &String) -> Option<String> { Some(a.clone()) }
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
        .unwrap_or(5000);

    if branches == 0 || ops_per_branch == 0 {
        eprintln!(
            "branches and ops_per_branch must both be positive (got {branches} {ops_per_branch})"
        );
        std::process::exit(1);
    }
    let total_ops = branches.checked_mul(ops_per_branch).unwrap_or_else(|| {
        eprintln!("branches * ops_per_branch overflows usize ({branches} * {ops_per_branch})");
        std::process::exit(1);
    });

    println!(
        "bench_par_branches_channel_reuse: branches={branches}  ops_per_branch={ops_per_branch}  \
         total_ops={total_ops}"
    );

    // Baseline: sequential, one branch at a time. Each branch has its own
    // channel (matching the real contract's per-branch `new loop`), reused
    // for every op in that branch.
    let seq_space = make_rspace().await;
    let t_seq = Instant::now();
    for b in 0..branches {
        let channel = format!("seq_{}", b);
        for _ in 0..ops_per_branch {
            seq_space
                .produce(channel.clone(), "datum".to_string(), false)
                .await
                .unwrap();
        }
    }
    let seq_elapsed = t_seq.elapsed();
    let seq_ms = seq_elapsed.as_millis();

    // Concurrent: all branches in parallel tokio tasks, each on its own
    // reused channel.
    let par_space = Arc::new(make_rspace().await);
    let t_par = Instant::now();
    let handles: Vec<_> = (0..branches)
        .map(|b| {
            let s = par_space.clone();
            tokio::spawn(async move {
                let channel = format!("par_{}", b);
                for _ in 0..ops_per_branch {
                    s.produce(channel.clone(), "datum".to_string(), false)
                        .await
                        .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    let par_elapsed = t_par.elapsed();
    let par_ms = par_elapsed.as_millis();

    let speedup = if par_elapsed.as_secs_f64() > 0.0 {
        seq_elapsed.as_secs_f64() / par_elapsed.as_secs_f64()
    } else {
        f64::INFINITY
    };

    println!("sequential:  {seq_ms} ms");
    println!("concurrent:  {par_ms} ms");
    println!("speedup:     {speedup:.2}x  (ideal: {branches}x)");
    println!("efficiency:  {:.1}%  (speedup / branches * 100)", speedup / branches as f64 * 100.0);
}
