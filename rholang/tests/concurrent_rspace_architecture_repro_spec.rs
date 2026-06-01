use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Barrier};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
};
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn repo_path(relative: impl AsRef<Path>) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn source(relative: impl AsRef<Path>) -> String {
    let path = repo_path(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn concurrent_rspace_architecture_repro_interpreter_must_not_use_global_ispace_mutex() {
    let runtime = source("src/rust/interpreter/rho_runtime.rs");

    assert!(
        !runtime.contains("pub type RhoISpace = Arc<\n    tokio::sync::Mutex"),
        "RhoISpace is still wrapped in a global tokio mutex; this reproduces the global RSpace serialization defect"
    );
    assert!(
        !runtime.contains(".space.try_lock()") && !runtime.contains(".space\n            .try_lock()"),
        "runtime access still try-locks the shared RSpace, so independent channel operations cannot make fully parallel progress"
    );
}

#[test]
fn concurrent_rspace_architecture_repro_eval_loop_must_not_use_join_all() {
    let reducer = source("src/rust/interpreter/reduce.rs");

    assert!(
        !reducer.contains("futures::future::join_all"),
        "reduce.rs still uses join_all; the documented design requires completion-order branch draining"
    );
    assert!(
        reducer.contains("FuturesUnordered"),
        "reduce.rs does not use FuturesUnordered for completion-order branch draining"
    );
}

#[test]
fn concurrent_rspace_architecture_repro_cost_balance_must_not_be_mutex_protected() {
    let accounting = source("src/rust/interpreter/accounting/mod.rs");

    assert!(
        !accounting.contains("state: Arc<Mutex<Cost>>"),
        "CostManager still keeps the spendable balance behind Arc<Mutex<Cost>> instead of an atomic reserve/CAS surface"
    );
}

fn concurrent_event(path: u32, weight: u64) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [7; 32],
        // D0: per-signature lane key, constant within this single deploy.
        sig_hash: [0; 32],
        source_path: SourcePath(vec![path]),
        redex_id: RedexId(path as u64),
        local_index: path as u64,
        kind: BillableKind::Comm,
        weight,
    }
}

#[test]
fn concurrent_rspace_architecture_repro_atomic_runtime_budget_must_not_overspend() {
    // D3 (DR-9): a COMM costs exactly ONE token; the per-op `weight` (60/50
    // below) is diagnostic and does NOT gate liveness. A one-token budget
    // therefore admits exactly one of the two concurrent COMMs: the canonical
    // reconciliation commits the first event in canonical order and marks the
    // other out-of-phlogiston, so the budget is never overspent regardless of
    // how the two reservation threads race.
    let budget = RuntimeBudget::new(Cost::create(1, "atomic reserve repro"));
    let accepted = Arc::new(AtomicI64::new(0));
    let rejected = Arc::new(AtomicI64::new(0));
    let both_ready = Arc::new(Barrier::new(2));

    let run_reserve = |event: BillableTokenEvent| {
        let budget = budget.clone();
        let accepted = Arc::clone(&accepted);
        let rejected = Arc::clone(&rejected);
        let both_ready = Arc::clone(&both_ready);

        std::thread::spawn(move || {
            both_ready.wait();
            match budget.reserve_canonical(event) {
                Ok(()) => accepted.fetch_add(1, Ordering::SeqCst),
                Err(_) => rejected.fetch_add(1, Ordering::SeqCst),
            };
        })
    };

    let reserve_a = run_reserve(concurrent_event(0, 60));
    let reserve_b = run_reserve(concurrent_event(1, 50));
    reserve_a.join().expect("reserve A panicked");
    reserve_b.join().expect("reserve B panicked");

    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert_eq!(rejected.load(Ordering::SeqCst), 1);
    // One COMM committed (1 token) against a 1-token budget: consumed == 1,
    // remaining == 0. Both attempts are recorded in the canonical cost trace
    // (1 committed + 1 out-of-phlogiston).
    assert_eq!(budget.total_cost().value, 1);
    assert_eq!(budget.remaining().value, 0);
    assert_eq!(budget.cost_trace_event_count(), 2);
}

async fn evaluate(term: &str) -> EvaluateResult {
    let mut store_manager = InMemoryStoreManager::new();
    let stores = store_manager
        .r_space_stores()
        .await
        .expect("failed to create in-memory stores");
    let (mut runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;

    runtime
        .evaluate_with_phlo(term, Cost::create(10_000, "concurrent rspace repro"))
        .await
        .expect("evaluation failed")
}

#[tokio::test]
async fn concurrent_rspace_architecture_repro_comm_cost_must_be_trigger_order_independent() {
    let produce_first = evaluate("@0!(0) | for (_ <- @0) { 0 }").await;
    let consume_first = evaluate("for (_ <- @0) { 0 } | @0!(0)").await;

    assert!(
        produce_first.errors.is_empty(),
        "produce-first evaluation failed: {:?}",
        produce_first.errors
    );
    assert!(
        consume_first.errors.is_empty(),
        "consume-first evaluation failed: {:?}",
        consume_first.errors
    );
    assert_eq!(
        produce_first.cost.value, consume_first.cost.value,
        "equivalent COMM scenarios must charge the same cost regardless of whether produce or consume triggers the match"
    );
    assert_eq!(
        produce_first.cost.value, consume_first.cost.value,
        "equivalent COMM scenarios must preserve the same billable cost evidence"
    );
}

#[tokio::test]
async fn concurrent_rspace_architecture_repro_play_replay_costs_must_match_for_interacting_bodies()
{
    let term = "@0!(0) | for (_ <- @0) { @2!(0) } | @1!(0) | for (_ <- @1) { for (_ <- @2) { 0 } }";
    let mut store_manager = InMemoryStoreManager::new();
    let stores = store_manager
        .r_space_stores()
        .await
        .expect("failed to create in-memory stores");
    let (mut runtime, mut replay_runtime, _) =
        create_runtimes(stores, false, &mut Vec::new()).await;
    let initial_phlo = Cost::create(100_000, "concurrent rspace repro");
    let rand = Blake2b512Random::create_from_bytes(&[]);

    let play = runtime
        .evaluate(term, initial_phlo.clone(), Default::default(), rand.clone())
        .await
        .expect("play evaluation failed");
    let checkpoint = runtime.create_checkpoint().await;
    replay_runtime
        .reset(&checkpoint.root)
        .await
        .expect("replay reset failed");
    replay_runtime
        .rig(checkpoint.log)
        .await
        .expect("replay rig failed");
    let replay = replay_runtime
        .evaluate(term, initial_phlo, Default::default(), rand)
        .await
        .expect("replay evaluation failed");

    assert!(
        play.errors.is_empty(),
        "play evaluation should succeed: {:?}",
        play.errors
    );
    assert!(
        replay.errors.is_empty(),
        "replay evaluation should succeed: {:?}",
        replay.errors
    );
    assert_eq!(
        play.cost.value, replay.cost.value,
        "play/replay body interleavings must not change charged phlo"
    );
}
