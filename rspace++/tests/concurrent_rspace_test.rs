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

impl Match<WildcardPattern, String> for AlwaysMatch {
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
// close to linear throughput scaling. Each branch produces on its own channel so
// per-channel phase locks never contend. The only shared resource is the HotStore.
//
// With a global std::sync::RwLock on HotStore all branches serialise on write()
// regardless of channel — N tasks each doing OPS produces takes N*OPS / solo_rate
// wall-clock. With DashMap per-key sharding each branch proceeds independently:
// total wall-clock stays close to OPS / solo_rate (the fastest branch wins).
//
// Threshold: N=4 branches must finish in less than 1.5x the time of 1 branch doing
// the same number of ops. A global RwLock gives ~1.0x (full serialisation);
// DashMap gives 2.5-3.5x when isolated. 1.5x is a stable lower bound that holds
// even under concurrent test suite load.
// Run explicitly with: cargo test -p rspace_plus_plus hot_store_concurrent -- --ignored --nocapture
#[ignore = "timing-sensitive: run in isolation, not as part of the full suite"]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn hot_store_concurrent_branches_scale_linearly() {
    const BRANCHES: usize = 4;
    const OPS_PER_BRANCH: usize = 500;

    // Baseline: single branch, OPS_PER_BRANCH produces on unique channels.
    let solo_space = make_rspace().await;
    let t_solo = Instant::now();
    for i in 0..OPS_PER_BRANCH {
        solo_space.produce(format!("solo_{}", i), "datum".to_string(), false).await.unwrap();
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
    for h in handles { h.await.unwrap(); }
    let concurrent_ms = t_concurrent.elapsed().as_secs_f64() * 1000.0;

    // speedup = how many times faster concurrent is relative to doing BRANCHES*OPS solo
    let expected_solo_equivalent_ms = solo_ms * BRANCHES as f64;
    let speedup = expected_solo_equivalent_ms / concurrent_ms;

    eprintln!(
        "hot_store_parallelism: solo={solo_ms:.1}ms  concurrent({BRANCHES} branches)={concurrent_ms:.1}ms  \
         equiv_solo={expected_solo_equivalent_ms:.1}ms  speedup={speedup:.2}x  (want >2.0x)"
    );

    assert!(
        speedup > 1.5,
        "HotStore concurrent branches achieved only {speedup:.2}x speedup over solo \
         (expected >1.5x). Root cause: global write lock on HotStore serialises all \
         branches even on separate channels. Fix: replace RwLock<HotStoreState> with \
         DashMap per collection.",
    );
}

// Runs SMALL and LARGE op counts on separate fresh spaces and checks that the
// time ratio stays below 25x. O(n) growth gives ~10x ratio; O(n^2) gives ~100x.
// Distinct channels per op so per-channel locks do not affect the measurement.
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
