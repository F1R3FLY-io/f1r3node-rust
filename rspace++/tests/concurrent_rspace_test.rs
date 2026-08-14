// Regression guard: event_log append in RSpace must be O(n), not O(n^2).
//
// log_produce/log_consume/log_comm used Vec::insert(0, event) which shifts
// all existing entries on every call. With M total ops the cost is O(M^2).
// Fixed by replacing with push(). If reverted, 10x more ops will take ~100x
// longer instead of ~10x.

use std::sync::Arc;
use std::time::Instant;

use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct WildcardPattern;

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct StringCont;

struct AlwaysMatch;

impl Match<WildcardPattern, String, StringCont> for AlwaysMatch {
    fn get(&self, _p: &WildcardPattern, a: &String) -> Option<String> { Some(a.clone()) }
}

type TestSpace = RSpace<String, WildcardPattern, String, StringCont>;

async fn make_rspace() -> TestSpace {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    RSpace::create(store, Arc::new(Box::new(AlwaysMatch))).unwrap()
}

async fn timed_produces(space: &TestSpace, ops: usize) -> std::time::Duration {
    let t = Instant::now();
    for i in 0..ops {
        space
            .produce(format!("ch_{}", i), "datum".to_string(), false)
            .await
            .unwrap();
    }
    t.elapsed()
}

// Verifies that N concurrent par-branches on separate private channels achieve
// close to linear throughput scaling. Each branch produces on its own channel
// so per-channel phase locks never contend. The only shared resource is the
// HotStore.
//
// With a global std::sync::RwLock on HotStore all branches serialise on write()
// regardless of channel — N tasks each doing OPS produces takes N*OPS /
// solo_rate wall-clock. With DashMap per-key sharding each branch proceeds
// independently: total wall-clock stays close to OPS / solo_rate (the fastest
// branch wins).
//
// Threshold: N=4 branches must finish in less than 1.5x the time of 1 branch
// doing the same number of ops. A global RwLock gives ~1.0x (full
// serialisation); DashMap gives 2.5-3.5x when isolated. 1.5x is a stable lower
// bound that holds even under concurrent test suite load.
// Run explicitly with: cargo test -p rspace_plus_plus hot_store_concurrent --
// --ignored --nocapture
#[ignore = "timing-sensitive: run in isolation, not as part of the full suite"]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hot_store_concurrent_branches_scale_linearly() {
    const BRANCHES: usize = 4;
    const OPS_PER_BRANCH: usize = 500;

    // Baseline: single branch, OPS_PER_BRANCH produces on unique channels.
    let solo_space = make_rspace().await;
    let t_solo = Instant::now();
    for i in 0..OPS_PER_BRANCH {
        solo_space
            .produce(format!("solo_{}", i), "datum".to_string(), false)
            .await
            .unwrap();
    }
    let solo_ms = t_solo.elapsed().as_secs_f64() * 1000.0;

    // Concurrent: BRANCHES tasks, each on its own private channel set.
    // Total work = BRANCHES * OPS_PER_BRANCH, same per-branch work as solo.
    let concurrent_space = Arc::new(make_rspace().await);
    let t_concurrent = Instant::now();
    let handles: Vec<_> = (0..BRANCHES)
        .map(|b| {
            let s = concurrent_space.clone();
            tokio::spawn(async move {
                for i in 0..OPS_PER_BRANCH {
                    s.produce(format!("branch_{}_{}", b, i), "datum".to_string(), false)
                        .await
                        .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    let concurrent_ms = t_concurrent.elapsed().as_secs_f64() * 1000.0;

    // speedup = how many times faster concurrent is relative to doing BRANCHES*OPS
    // solo
    let expected_solo_equivalent_ms = solo_ms * BRANCHES as f64;
    let speedup = expected_solo_equivalent_ms / concurrent_ms;

    eprintln!(
        "hot_store_parallelism: solo={solo_ms:.1}ms  concurrent({BRANCHES} \
         branches)={concurrent_ms:.1}ms  equiv_solo={expected_solo_equivalent_ms:.1}ms  \
         speedup={speedup:.2}x  (want >2.0x)"
    );

    assert!(
        speedup > 1.5,
        "HotStore concurrent branches achieved only {speedup:.2}x speedup over solo (expected \
         >1.5x). Root cause: global write lock on HotStore serialises all branches even on \
         separate channels. Fix: replace RwLock<HotStoreState> with DashMap per collection.",
    );
}

// Runs SMALL and LARGE op counts on separate fresh spaces and checks that the
// time ratio stays below 25x. O(n) growth gives ~10x ratio; O(n^2) gives ~100x.
// Distinct channels per op so per-channel locks do not affect the measurement.
// Run explicitly with: cargo test -p rspace_plus_plus event_log_insert --
// --ignored --nocapture
#[ignore = "timing-sensitive: run in isolation, not as part of the full suite"]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn event_log_insert_complexity_is_not_quadratic() {
    const SMALL: usize = 200;
    const LARGE: usize = 2000;

    let t_small = timed_produces(&make_rspace().await, SMALL).await;
    let t_large = timed_produces(&make_rspace().await, LARGE).await;

    let ratio = t_large.as_secs_f64() / t_small.as_secs_f64().max(0.000_001);

    eprintln!(
        "event_log_complexity: ops={SMALL} -> {:.3}ms  ops={LARGE} -> {:.3}ms  ratio={ratio:.1}x",
        t_small.as_millis(),
        t_large.as_millis(),
    );

    assert!(
        ratio < 25.0,
        "event_log O(n^2) regression: {LARGE} ops took {ratio:.1}x longer than {SMALL} ops \
         (expected <25x for O(n) growth). Fix: use push() instead of Vec::insert(0,..) in \
         log_produce, log_consume, log_comm.",
    );
}

// Regression guard for f1r3node-rust#43 / issue-43.
//
// Originally found the bug: real replay data (mntd EVAL ms / PAR x columns)
// showed a single deploy's evaluate() cost growing with the number of
// *other* deploys already replayed in the same block (57ms at cap=1 ->
// 372ms at cap=100 per deploy, identical per-deploy work). PR #72's own
// fixes and the COMM_CON/COMM_PRO histograms (sub-ms even at cap=100) ruled
// out rspace++ commit/lock work as the cause. Root cause:
// `create_soft_checkpoint()`, which `replay_runtime.rs::run_user_deploy` calls
// unconditionally at the start of *every* user deploy (for failure rollback)
// and which measured EVAL time includes, called `HotStore::snapshot()`, which
// used to full-clone all five DashMaps into plain HashMaps -- O(total store
// size), not O(1) or O(per-deploy work). See git history for pre-fix
// baseline measurements.
//
// Fix: HotStore's five state maps are now backed by NUM_SHARDS (256)
// independent `imbl::HashMap` shards (`hot_store.rs`, `ShardedMap`).
// `snapshot()` clones each shard's current persistent-map value directly
// (an O(1) refcount bump per shard) instead of rebuilding a flat map by
// visiting every entry -- O(NUM_SHARDS), not O(store size). This test now
// guards the fix: time growth from a 100x larger store should stay small
// and bounded, not track the store-size growth.
//
// Methodology note: the first `create_soft_checkpoint()` call against a
// freshly built store pays one-off heap first-touch cost unrelated to
// store size (measured: ~50us vs. a steady-state ~18us, regardless of
// prefill size). Each `avg_checkpoint_us` run below discards that first
// sample before averaging so the measured ratio reflects snapshot cost,
// not allocator warm-up -- without the discard this test's ratio isn't a
// reliable signal and can fail on correct code at block-realistic sizes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn soft_checkpoint_cost_does_not_scale_with_accumulated_store_size() {
    const SMALL_PREFILL: usize = 100;
    const LARGE_PREFILL: usize = 10_000;
    const SAMPLES: usize = 50;

    async fn avg_checkpoint_us(prefill: usize, samples: usize) -> f64 {
        let space = make_rspace().await;
        // Simulate `prefill` earlier deploys' worth of accumulated HotStore
        // state -- distinct channels, one produce each, matching how
        // TransferTerm deploys each touch their own vault/registry channels.
        for i in 0..prefill {
            space
                .produce(format!("prefill_ch_{i}"), "datum".to_string(), false)
                .await
                .unwrap();
        }

        // Discard the first call -- pays one-off heap first-touch cost, not
        // snapshot cost (see methodology note above).
        let _ = space.create_soft_checkpoint().await;

        // Time create_soft_checkpoint() in isolation -- this is exactly what
        // run_user_deploy() pays once per deploy, before evaluate() even
        // starts, purely for failure-rollback support.
        let t0 = Instant::now();
        for _ in 0..samples {
            let _ = space.create_soft_checkpoint().await;
        }
        t0.elapsed().as_micros() as f64 / samples as f64
    }

    let small_us = avg_checkpoint_us(SMALL_PREFILL, SAMPLES).await;
    let large_us = avg_checkpoint_us(LARGE_PREFILL, SAMPLES).await;
    let ratio = large_us / small_us.max(1e-9);
    let store_size_ratio = LARGE_PREFILL as f64 / SMALL_PREFILL as f64;

    eprintln!(
        "soft_checkpoint_cost: store_size={SMALL_PREFILL} -> {small_us:.1}us/call   \
         store_size={LARGE_PREFILL} -> {large_us:.1}us/call   time_ratio={ratio:.1}x   \
         store_size_ratio={store_size_ratio:.1}x"
    );

    // Post-fix (sharded persistent maps), with warm-up discarded: driven by
    // shard-lock/allocation overhead, not store size. O(store-size) (the
    // pre-fix behavior) would put this near store_size_ratio (100x); a
    // regression back to that pattern should fail this bound well before
    // reaching it. Some margin above the steady-state ratio is kept for
    // CI timing variance.
    const MAX_RATIO: f64 = 8.0;
    assert!(
        ratio < MAX_RATIO,
        "create_soft_checkpoint() scaled with accumulated store size ({ratio:.1}x for a \
         {store_size_ratio:.1}x larger store, expected <{MAX_RATIO:.0}x) -- regression back to an \
         O(store-size) HotStore::snapshot(), the issue-43 root cause. See ShardedMap in \
         hot_store.rs.",
    );
}

// Diagnostic for the issue-43 CPU-inversion residual: node/src/main.rs builds
// its main runtime with `Builder::new_multi_thread().enable_all()` and no
// explicit `worker_threads()`, so the worker count defaults to
// `available_parallelism()` -- which on a cgroup-limited container reads the
// CPU *quota*, not the amount of work actually available. Issue
// f1r3node-rust#43 documents block-replay getting *slower* as CPU quota (and
// therefore Tokio worker count) grows, for a block of only a handful of trivial
// deploys -- the opposite of what more CPU should buy.
//
// This reproduces the same shape of workload locally: a fixed, small number of
// independent produce/consume chains standing in for deploys, run under
// runtimes built with different worker_threads counts. If wall time grows with
// thread count here, that's a local, cluster-free confirmation that Tokio
// scheduling overhead (not rspace++ locking, already ruled out in #43 by sub-ms
// lock-acquire metrics) is a plausible driver of the residual inversion.
// Run explicitly with: cargo test -p rspace_plus_plus worker_threads_scaling --
// --ignored --nocapture
#[ignore = "timing-sensitive: run in isolation, not as part of the full suite"]
#[test]
fn small_workload_does_not_slow_down_with_more_worker_threads() {
    const DEPLOYS: usize = 10;
    const OPS_PER_DEPLOY: usize = 4;
    const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];

    let mut results = Vec::new();
    for &threads in &THREAD_COUNTS {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .enable_all()
            .build()
            .unwrap();

        let ms = rt.block_on(async {
            let space = Arc::new(make_rspace().await);
            let t = Instant::now();
            let handles: Vec<_> = (0..DEPLOYS)
                .map(|d| {
                    let s = space.clone();
                    tokio::spawn(async move {
                        for i in 0..OPS_PER_DEPLOY {
                            s.produce(format!("deploy_{d}_ch_{i}"), "datum".to_string(), false)
                                .await
                                .unwrap();
                        }
                    })
                })
                .collect();
            for h in handles {
                h.await.unwrap();
            }
            t.elapsed().as_secs_f64() * 1000.0
        });

        eprintln!(
            "worker_threads={threads:>2}  {DEPLOYS} deploys x {OPS_PER_DEPLOY} ops  =  {ms:.3}ms"
        );
        results.push((threads, ms));
    }

    let (baseline_threads, baseline_ms) = results[0];
    let (max_threads, max_threads_ms) = *results.last().unwrap();
    let slowdown = max_threads_ms / baseline_ms.max(0.001);

    eprintln!(
        "worker_threads_scaling: {baseline_threads} thread(s)={baseline_ms:.3}ms  {max_threads} \
         thread(s)={max_threads_ms:.3}ms  slowdown={slowdown:.2}x"
    );

    assert!(
        slowdown < 3.0,
        "small fixed workload ({DEPLOYS} deploys) got {slowdown:.2}x slower going from \
         {baseline_threads} to {max_threads} tokio worker_threads (expected <3x). If this reproduces \
         on a CI/production-like host, node/src/main.rs's \
         Builder::new_multi_thread().enable_all() (no explicit worker_threads(), so it defaults \
         to available_parallelism()/cgroup CPU quota) is a live suspect for the issue-43 \
         CPU-inversion residual -- more worker threads contending over the same small set of \
         locks/channels can cost more in scheduling overhead than it buys in parallelism for \
         trivial per-block workloads.",
    );
}
