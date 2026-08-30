use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::*;
use crate::rspace::r#match::Match;
use crate::rspace::operation_context::{self, OperationOrder};
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::trace::event::{Event, IOEvent};

// ── minimal types ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct Wildcard;

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
struct Cont;

struct AlwaysMatch;

impl Match<Wildcard, String, Cont> for AlwaysMatch {
    fn get(&self, _: &Wildcard, a: &String) -> Option<String> { Some(a.clone()) }
}

async fn make_rspace() -> RSpace<String, Wildcard, String, Cont> {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    RSpace::create(store, Arc::new(Box::new(AlwaysMatch))).unwrap()
}

fn order(step: u64) -> OperationOrder {
    OperationOrder {
        session: [9; 32],
        path: vec![(step, 0)],
    }
}

#[tokio::test]
async fn ordered_event_log_uses_causal_order_instead_of_arrival_order() {
    let rspace = make_rspace().await;
    operation_context::scope(
        order(2),
        rspace.produce("later".to_string(), "two".to_string(), false),
    )
    .await
    .unwrap();
    operation_context::scope(
        order(1),
        rspace.produce("earlier".to_string(), "one".to_string(), false),
    )
    .await
    .unwrap();

    assert_eq!(rspace.take_event_log().await, vec![
        Event::IoEvent(IOEvent::Produce(Produce::create(
            &"earlier".to_string(),
            &"one".to_string(),
            false,
        ))),
        Event::IoEvent(IOEvent::Produce(Produce::create(
            &"later".to_string(),
            &"two".to_string(),
            false,
        ))),
    ]);
}

#[tokio::test]
async fn soft_checkpoint_preserves_and_reverts_canonical_event_order() {
    let rspace = make_rspace().await;
    operation_context::scope(
        order(3),
        rspace.produce("third".to_string(), "three".to_string(), false),
    )
    .await
    .unwrap();
    operation_context::scope(
        order(1),
        rspace.produce("first".to_string(), "one".to_string(), false),
    )
    .await
    .unwrap();

    let checkpoint = rspace.create_soft_checkpoint().await;
    let expected = checkpoint.log.clone();
    assert_eq!(expected, vec![
        Event::IoEvent(IOEvent::Produce(Produce::create(
            &"first".to_string(),
            &"one".to_string(),
            false,
        ))),
        Event::IoEvent(IOEvent::Produce(Produce::create(
            &"third".to_string(),
            &"three".to_string(),
            false,
        ))),
    ]);
    rspace.revert_to_soft_checkpoint(checkpoint).await.unwrap();
    assert_eq!(rspace.take_event_log().await, expected);
}

#[tokio::test]
async fn produce_metadata_updates_reach_causally_ordered_events() {
    let rspace = make_rspace().await;
    operation_context::scope(
        order(1),
        rspace.produce("service".to_string(), "request".to_string(), false),
    )
    .await
    .unwrap();
    let updated = Produce::create(&"service".to_string(), &"request".to_string(), false)
        .mark_as_non_deterministic(vec![b"response".to_vec()]);
    rspace.update_produce(updated).await;

    let log = rspace.take_event_log().await;
    let Event::IoEvent(IOEvent::Produce(produce)) = &log[0] else {
        panic!("expected produce event")
    };
    assert!(!produce.is_deterministic);
    assert_eq!(produce.output_value, vec![b"response".to_vec()]);
}

// Measures contention on the event_log mutex while N concurrent tasks call
// produce() on separate channels. The log is pre-filled to PRE_FILL entries
// before the concurrent phase so every insert starts with a large existing
// log, making the mutex hold time long enough to observe.
//
// Observer runs on a dedicated OS thread (not a tokio task) because
// std::sync::Mutex::lock() blocks the worker thread without yielding, so a
// tokio observer would never be scheduled while producers hold the mutex.
//
// Passes when event_log uses O(1) append: hold time drops to ~10 ns and
// the observer almost never catches the mutex held.

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn event_log_mutex_does_not_contend_under_concurrent_produces() {
    const TASKS: usize = 4;
    const OPS_PER_TASK: usize = 200;
    // Simulates mid-deploy state: the event_log grows to PRE_FILL entries
    // before the concurrent test starts. Each subsequent Vec::insert(0,..)
    // must shift all existing entries — O(PRE_FILL × sizeof(Event)) bytes.
    // At PRE_FILL=8000 and sizeof(Event)≈150 bytes on M1:
    //   shift ≈ 1.2 MB / 50 GB/s ≈ 24 μs per insert → detectable.
    const PRE_FILL: usize = 8_000;
    // Fraction of observer probes that find the mutex already held.
    const MAX_CONTENTION_RATE: f64 = 0.20;

    let rspace = make_rspace().await;

    // Observer on a dedicated OS thread: probes try_lock() in a spin loop.
    // Must be an OS thread, not a tokio task — std::sync::Mutex::lock()
    // blocks the worker thread without yielding, so a tokio observer would
    // never run while any producer holds the mutex.
    let event_log = rspace.event_log.clone();
    let running = Arc::new(AtomicBool::new(true));
    let total_probes = Arc::new(AtomicU64::new(0));
    let contended_probes = Arc::new(AtomicU64::new(0));

    {
        let event_log = event_log.clone();
        let running = running.clone();
        let total = total_probes.clone();
        let contended = contended_probes.clone();
        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                total.fetch_add(1, Ordering::Relaxed);
                if event_log.try_lock().is_err() {
                    contended.fetch_add(1, Ordering::Relaxed);
                }
                std::hint::spin_loop();
            }
        });
    }

    // Pre-fill: grow the log to PRE_FILL entries before the concurrent phase.
    // Counters are reset after so we measure only concurrent contention.
    for i in 0..PRE_FILL {
        rspace
            .produce(format!("prefill_{}", i), "datum".to_string(), false)
            .await
            .unwrap();
    }

    total_probes.store(0, Ordering::Relaxed);
    contended_probes.store(0, Ordering::Relaxed);

    // N concurrent producers, each on its own channel set.
    let handles: Vec<_> = (0..TASKS)
        .map(|i| {
            let s = rspace.clone();
            tokio::spawn(async move {
                for j in 0..OPS_PER_TASK {
                    s.produce(format!("ch_{}_{}", i, j), "datum".to_string(), false)
                        .await
                        .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    running.store(false, Ordering::Relaxed);

    let total = total_probes.load(Ordering::Relaxed);
    let contended = contended_probes.load(Ordering::Relaxed);
    let rate = if total > 0 {
        contended as f64 / total as f64
    } else {
        0.0
    };

    eprintln!(
        "event_log_contention: probes={total}  contended={contended}  rate={:.1}%  (threshold \
         <{:.0}%)",
        rate * 100.0,
        MAX_CONTENTION_RATE * 100.0,
    );

    assert!(
        rate < MAX_CONTENTION_RATE,
        "event_log mutex contention too high: {:.1}% of probes found the mutex held (threshold \
         {:.0}%). Root cause: all {} concurrent tasks share one std::sync::Mutex<event_log> — \
         every produce() call acquires it, blocking other worker threads. Fix: per-task event \
         logs merged at checkpoint, or a lock-free append structure.",
        rate * 100.0,
        MAX_CONTENTION_RATE * 100.0,
        TASKS,
    );
}

// Mirrors the rholang-par benchmark: PAR_BRANCHES concurrent tokio tasks
// each call produce() OPS_PER_BRANCH times on their own private channels,
// all sharing one RSpace and therefore one event_log.
//
// The log grows naturally from zero to PAR_BRANCHES * OPS_PER_BRANCH entries.
// Passes when event_log uses O(1) append.

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn par_branch_event_log_does_not_contend_at_rholang_par_scale() {
    const PAR_BRANCHES: usize = 32;
    const OPS_PER_BRANCH: usize = 500;
    const MAX_CONTENTION_RATE: f64 = 0.20;

    let rspace = make_rspace().await;

    let event_log = rspace.event_log.clone();
    let running = Arc::new(AtomicBool::new(true));
    let total_probes = Arc::new(AtomicU64::new(0));
    let contended_probes = Arc::new(AtomicU64::new(0));

    {
        let event_log = event_log.clone();
        let running = running.clone();
        let total = total_probes.clone();
        let contended = contended_probes.clone();
        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                total.fetch_add(1, Ordering::Relaxed);
                if event_log.try_lock().is_err() {
                    contended.fetch_add(1, Ordering::Relaxed);
                }
                std::hint::spin_loop();
            }
        });
    }

    // 32 par-branches, each producing on its own unique channels.
    // No matching happens, so contention comes purely from log growth.
    let handles: Vec<_> = (0..PAR_BRANCHES)
        .map(|i| {
            let s = rspace.clone();
            tokio::spawn(async move {
                for j in 0..OPS_PER_BRANCH {
                    s.produce(format!("branch_{}_{}", i, j), "datum".to_string(), false)
                        .await
                        .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    running.store(false, Ordering::Relaxed);

    let total = total_probes.load(Ordering::Relaxed);
    let contended = contended_probes.load(Ordering::Relaxed);
    let rate = if total > 0 {
        contended as f64 / total as f64
    } else {
        0.0
    };

    eprintln!(
        "par_branch_contention: branches={PAR_BRANCHES}  ops_per_branch={OPS_PER_BRANCH}  \
         total_ops={}  probes={total}  contended={contended}  rate={:.1}%  (threshold <{:.0}%)",
        PAR_BRANCHES * OPS_PER_BRANCH,
        rate * 100.0,
        MAX_CONTENTION_RATE * 100.0,
    );

    assert!(
        rate < MAX_CONTENTION_RATE,
        "event_log mutex contention too high at {PAR_BRANCHES} par-branches: {:.1}% of probes \
         found the mutex held (threshold {:.0}%). Root cause: all {PAR_BRANCHES} par-branch tasks \
         share one std::sync::Mutex<event_log> and each produce() calls Vec::insert(0,..) — O(n) \
         shift where n grows to {} entries. Fix: per-branch event logs merged at \
         create_checkpoint, or replace insert(0,..) with push().",
        rate * 100.0,
        MAX_CONTENTION_RATE * 100.0,
        PAR_BRANCHES * OPS_PER_BRANCH,
    );
}

// Disambiguates two candidate explanations for the near-zero wall-clock
// speedup measured end-to-end at rholang-par scale (bench_par_branches:
// 0.88-0.96x at 32 branches against a worker-thread-bounded ideal — see
// issue #50 follow-up):
// (a) event_log/produce_counter themselves cost that much, or
// (b) something else in produce()'s path (HotStore, the striped
//     per-channel lock, tokio scheduling) is the real cost and these
//     two locks are not the story despite being std::sync::Mutex.
//
// Calls log_produce() directly — the exact call log_produce() itself
// takes (event_log.push then, for non-persist, produce_counter.insert)
// with no produce_lock(), no HotStore, no get_store() RwLock read, no
// matcher involved. Isolates these two locks from every other cost
// produce() incurs, at the same branch/op counts as bench_par_branches.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn event_log_and_produce_counter_isolated_cost_at_rholang_par_scale() {
    const PAR_BRANCHES: usize = 32;
    const OPS_PER_BRANCH: usize = 5000;

    // Sequential baseline: identical total ops, one after another.
    let seq_rspace = make_rspace().await;
    let t_seq = Instant::now();
    for b in 0..PAR_BRANCHES {
        for i in 0..OPS_PER_BRANCH {
            let channel = format!("seq_{}_{}", b, i);
            let data = "datum".to_string();
            let produce_ref = Produce::create(&channel, &data, false);
            seq_rspace.log_produce(&produce_ref, false);
        }
    }
    let seq_ms = t_seq.elapsed().as_millis().max(1);

    // Concurrent: PAR_BRANCHES tokio tasks, each on its own channel set,
    // all sharing one RSpace and therefore one event_log/produce_counter.
    let par_rspace = Arc::new(make_rspace().await);
    let t_par = Instant::now();
    let handles: Vec<_> = (0..PAR_BRANCHES)
        .map(|b| {
            let s = par_rspace.clone();
            tokio::spawn(async move {
                for i in 0..OPS_PER_BRANCH {
                    let channel = format!("par_{}_{}", b, i);
                    let data = "datum".to_string();
                    let produce_ref = Produce::create(&channel, &data, false);
                    s.log_produce(&produce_ref, false);
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    let par_ms = t_par.elapsed().as_millis().max(1);

    // log_produce() is synchronous and the branch loops never await, so
    // each task holds its worker for the whole loop: at most num_workers
    // (further capped by the host's cores) branches run at once. That, not
    // PAR_BRANCHES, is the achievable ideal for the efficiency metric.
    let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
    let ideal = PAR_BRANCHES.min(num_workers).min(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(usize::MAX),
    );
    let speedup = seq_ms as f64 / par_ms as f64;
    let efficiency = speedup / ideal as f64 * 100.0;

    eprintln!(
        "event_log+produce_counter isolated cost: branches={PAR_BRANCHES} \
         ops_per_branch={OPS_PER_BRANCH} total_ops={} sequential={seq_ms}ms concurrent={par_ms}ms \
         speedup={:.2}x (ideal {ideal}x: {PAR_BRANCHES} branches on {num_workers} workers) \
         efficiency={:.1}%",
        PAR_BRANCHES * OPS_PER_BRANCH,
        speedup,
        efficiency,
    );

    // No assertion: this test is diagnostic, not a regression gate. It
    // reports the isolated cost of these two locks so it can be compared
    // against bench_par_branches' end-to-end number for the same
    // branch/op counts (see issue #50 investigation).
}

// With produce_counter now sharded (issue #50, part 1), this isolates
// what's left: event_log alone, pushing directly to the field (bypassing
// log_produce()/produce_counter entirely) at the same branch/op scale.
// Confirms whether event_log on its own still accounts for the residual
// near-zero speedup, and gives a before/after number for its own fix.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn event_log_isolated_cost_at_rholang_par_scale() {
    const PAR_BRANCHES: usize = 32;
    const OPS_PER_BRANCH: usize = 5000;

    // Sequential baseline: identical total ops, one after another.
    let seq_rspace = make_rspace().await;
    let t_seq = Instant::now();
    for b in 0..PAR_BRANCHES {
        for i in 0..OPS_PER_BRANCH {
            let channel = format!("seq_{}_{}", b, i);
            let data = "datum".to_string();
            let produce_ref = Produce::create(&channel, &data, false);
            seq_rspace
                .event_log
                .lock()
                .expect("event log lock")
                .push(Event::IoEvent(IOEvent::Produce(produce_ref)));
        }
    }
    let seq_ms = t_seq.elapsed().as_millis().max(1);

    // Concurrent: PAR_BRANCHES tokio tasks, all sharing one event_log.
    let par_rspace = Arc::new(make_rspace().await);
    let t_par = Instant::now();
    let handles: Vec<_> = (0..PAR_BRANCHES)
        .map(|b| {
            let s = par_rspace.clone();
            tokio::spawn(async move {
                for i in 0..OPS_PER_BRANCH {
                    let channel = format!("par_{}_{}", b, i);
                    let data = "datum".to_string();
                    let produce_ref = Produce::create(&channel, &data, false);
                    s.event_log
                        .lock()
                        .expect("event log lock")
                        .push(Event::IoEvent(IOEvent::Produce(produce_ref)));
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    let par_ms = t_par.elapsed().as_millis().max(1);

    let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
    let ideal = PAR_BRANCHES.min(num_workers).min(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(usize::MAX),
    );
    let speedup = seq_ms as f64 / par_ms as f64;
    let efficiency = speedup / ideal as f64 * 100.0;

    eprintln!(
        "event_log isolated cost (produce_counter excluded): branches={PAR_BRANCHES} \
         ops_per_branch={OPS_PER_BRANCH} total_ops={} sequential={seq_ms}ms concurrent={par_ms}ms \
         speedup={:.2}x (ideal {ideal}x: {PAR_BRANCHES} branches on {num_workers} workers) \
         efficiency={:.1}%",
        PAR_BRANCHES * OPS_PER_BRANCH,
        speedup,
        efficiency,
    );

    // Diagnostic, not a tight bound: absolute timings swing widely with
    // build profile (debug vs --release) and op count (see issue #50
    // follow-up), so this only catches catastrophic regressions back to
    // real lock-based contention, not general slowdowns.
    assert!(
        par_ms <= seq_ms.saturating_mul(5),
        "event_log regressed: concurrent ({par_ms}ms) more than 5x slower than sequential \
         ({seq_ms}ms) at {PAR_BRANCHES} branches x {OPS_PER_BRANCH} ops — see issue #50"
    );
}

// get_store() is called at least 3x per produce()/consume() (produce_lock,
// locked_produce, store_data) with no writer ever contending it on the
// hot path. Isolates just that call, at 2x the op count, as a regression
// sentinel against reintroducing lock-based contention on this path.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn get_store_read_lock_isolated_cost_at_rholang_par_scale() {
    const PAR_BRANCHES: usize = 32;
    // 10x rholang-par's own scale (not 5000): get_store() is now a fast
    // ArcSwap load, so at 5000 ops/branch tokio::spawn's one-time per-task
    // setup cost dominates the measured concurrent time and swamps the
    // per-call cost this test means to isolate, making the ratio flaky
    // (regardless of real ArcSwap performance). See the build-profile/op-
    // count lesson in issues/02-intra-deploy-par-mutex-rspace.md.
    const OPS_PER_BRANCH: usize = 50000;

    let seq_rspace = make_rspace().await;
    let t_seq = Instant::now();
    for _ in 0..(PAR_BRANCHES * OPS_PER_BRANCH) {
        let _ = seq_rspace.get_store();
        let _ = seq_rspace.get_store();
    }
    let seq_ms = t_seq.elapsed().as_millis().max(1);

    let par_rspace = Arc::new(make_rspace().await);
    let t_par = Instant::now();
    let handles: Vec<_> = (0..PAR_BRANCHES)
        .map(|_| {
            let s = par_rspace.clone();
            tokio::spawn(async move {
                for _ in 0..OPS_PER_BRANCH {
                    let _ = s.get_store();
                    let _ = s.get_store();
                }
            })
        })
        .collect();
    for h in handles {
        h.await.unwrap();
    }
    let par_ms = t_par.elapsed().as_millis().max(1);

    let num_workers = tokio::runtime::Handle::current().metrics().num_workers();
    let ideal = PAR_BRANCHES.min(num_workers).min(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(usize::MAX),
    );
    let speedup = seq_ms as f64 / par_ms as f64;
    let efficiency = speedup / ideal as f64 * 100.0;

    eprintln!(
        "get_store() ArcSwap load isolated cost (2 calls/op): branches={PAR_BRANCHES} \
         ops_per_branch={OPS_PER_BRANCH} total_calls={} sequential={seq_ms}ms \
         concurrent={par_ms}ms speedup={:.2}x (ideal {ideal}x) efficiency={:.1}%",
        PAR_BRANCHES * OPS_PER_BRANCH * 2,
        speedup,
        efficiency,
    );

    // Diagnostic, not a tight bound: see the comment on
    // event_log_isolated_cost_at_rholang_par_scale above for why. This loop
    // has no .await point inside it (a tight synchronous hot loop), the
    // worst case for any scheduler/atomic primitive and not representative
    // of real produce()/consume() usage (confirmed fine on the actual
    // cluster — see issues/02-intra-deploy-par-mutex-rspace.md). On a loaded
    // shared machine this ratio has been observed up to ~20x with no
    // functional regression; 50x stays a safe floor for catching a real
    // reintroduction of RwLock-class contention (which measured >10x even
    // in isolation) without flaking on ordinary system noise.
    assert!(
        par_ms <= seq_ms.saturating_mul(50),
        "get_store() regressed: concurrent ({par_ms}ms) more than 50x slower than sequential \
         ({seq_ms}ms) at {PAR_BRANCHES} branches x {OPS_PER_BRANCH} ops x2 calls/op — see issue \
         #50"
    );
}
