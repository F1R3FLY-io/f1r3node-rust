use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

struct LiveHeapCounter;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for LiveHeapCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: LiveHeapCounter = LiveHeapCounter;

const ITERATIONS: usize = 10_000;
const TRANSIENT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

struct HeapProfile {
    result: EvaluateResult,
    transient_bytes: usize,
}

async fn evaluate_with_heap_profile<R: RhoRuntime>(runtime: &R, term: &str) -> HeapProfile {
    let before = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_LIVE_BYTES.store(before, Ordering::Relaxed);
    let result = runtime
        .evaluate(
            term,
            Cost::create(i64::MAX, "deferred dispatch memory".to_string()),
            HashMap::new(),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .expect("evaluation failed");
    let peak = PEAK_LIVE_BYTES.load(Ordering::Relaxed);
    let after = LIVE_BYTES.load(Ordering::Relaxed);
    HeapProfile {
        result,
        transient_bytes: peak.saturating_sub(after.max(before)),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tail_recursive_contract_play_and_replay_do_not_hold_per_iteration_heap() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("Failed to create in-memory rspace store");
    let mut additional_system_processes = Vec::new();
    let (mut runtime, replay_runtime, _) =
        create_runtimes(store, false, &mut additional_system_processes).await;

    let term = format!(
        "new loop in {{ contract loop(@n) = {{ if (n <= 0) {{ Nil }} else {{ loop!(n - 1) }} }} | loop!({ITERATIONS}) }}"
    );

    let play = evaluate_with_heap_profile(&runtime, &term).await;
    assert!(
        play.result.errors.is_empty(),
        "play errors: {:?}",
        play.result.errors
    );

    let log = runtime.take_event_log().await;
    replay_runtime.rig(log).await.expect("replay rig failed");

    let replay = evaluate_with_heap_profile(&replay_runtime, &term).await;
    assert!(
        replay.result.errors.is_empty(),
        "replay errors: {:?}",
        replay.result.errors
    );
    replay_runtime
        .check_replay_data()
        .await
        .expect("replay data check failed");
    assert_eq!(play.result.cost, replay.result.cost);

    assert!(
        play.transient_bytes < TRANSIENT_LIMIT_BYTES,
        "play held {} transient bytes for {ITERATIONS} iterations",
        play.transient_bytes
    );
    assert!(
        replay.transient_bytes < TRANSIENT_LIMIT_BYTES,
        "replay held {} transient bytes for {ITERATIONS} iterations",
        replay.transient_bytes
    );

    runtime.create_soft_checkpoint().await;
}
