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
    fn get(&self, _p: WildcardPattern, a: String) -> Option<String> { Some(a) }
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
