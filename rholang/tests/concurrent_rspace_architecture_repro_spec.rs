use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Barrier};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
};
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::{
    create_runtimes, create_runtimes_for_epoch,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::Log;

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
        "reduce.rs still uses join_all; the design requires non-blocking concurrent branch completion"
    );
    // The eval-parallel + continuation joins now DETACH each branch onto the atomic-counter completion
    // driver (`spawn_detached`) instead of the parent awaiting a `FuturesUnordered` join. This is an even
    // stronger form of completion-order, non-parent-pinning draining: it removes the O(N) parked-parent
    // chain entirely (the parent returns immediately; the driver awaits live -> 0 at the inj/eval drain).
    // Supersedes the earlier `FuturesUnordered` requirement.
    assert!(
        reducer.contains("spawn_detached"),
        "reduce.rs no longer detaches eval branches onto the atomic-counter completion driver (spawn_detached)"
    );
    assert!(
        !reducer.contains("FuturesUnordered"),
        "reduce.rs still awaits a FuturesUnordered join; the detached-spawn driver removes the parent await"
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
    // The two weighted reservations total 110 against a fixed 100-token
    // ceiling. Exactly one may pass the live CAS gate, and canonical
    // reconciliation must clamp the exhausted trace to the ceiling regardless
    // of which reservation thread wins.
    let budget = RuntimeBudget::new(Cost::create(100, "atomic reserve repro"));
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
    assert_eq!(budget.total_cost().value, 100);
    assert_eq!(budget.remaining().value, 0);
    assert_eq!(budget.cost_trace_event_count(), 2);
}

async fn evaluate(term: &str) -> EvaluateResult {
    let mut store_manager = InMemoryStoreManager::new();
    let stores = store_manager
        .r_space_stores()
        .await
        .expect("failed to create in-memory stores");
    // Changed by the merge of feature/cost-accounted-rho (finding R1-39): the
    // callers compare protocol-6 COMM charges, which F1R3Lang applies in the
    // CostV1 epoch. On a Legacy runtime both charges are 0, so the comparison
    // proves nothing.
    // let (mut runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    let (mut runtime, _, _) =
        create_runtimes_for_epoch(EventCodecEpoch::CostV1, stores, false, &mut Vec::new()).await;

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
    // Added by the merge of feature/cost-accounted-rho (finding R1-39): the
    // comparison is evidence only when the COMM is charged.
    assert!(produce_first.cost.value > 0, "the COMM must be charged");
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
    // Changed by the merge of feature/cost-accounted-rho (finding R1-39): play
    // and replay charges are compared below, so both run in the CostV1 epoch.
    // let (mut runtime, mut replay_runtime, _) =
    //     create_runtimes(stores, false, &mut Vec::new()).await;
    let (mut runtime, mut replay_runtime, _) =
        create_runtimes_for_epoch(EventCodecEpoch::CostV1, stores, false, &mut Vec::new()).await;
    let initial_phlo = Cost::create(100_000, "concurrent rspace repro");
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let pre_state_root = runtime.get_root().await;

    let play = runtime
        .evaluate(term, initial_phlo.clone(), Default::default(), rand.clone())
        .await
        .expect("play evaluation failed");
    let checkpoint = runtime.create_checkpoint().await;
    replay_runtime
        .reset(&pre_state_root)
        .await
        .expect("replay reset failed");
    replay_runtime
        .rig(checkpoint.log.clone())
        .await
        .expect("replay rig failed");
    let replay = replay_runtime
        .evaluate(term, initial_phlo, Default::default(), rand)
        .await
        .expect("replay evaluation failed");
    replay_runtime
        .check_replay_data()
        .await
        .expect("replay left unconsumed COMM events");
    let replay_root = replay_runtime.get_root().await;

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
    // Added by the merge of feature/cost-accounted-rho (finding R1-39): the
    // comparison is evidence only when play charges something.
    assert!(play.cost.value > 0, "play must charge the COMMs");
    assert_eq!(
        play.cost.value, replay.cost.value,
        "play/replay body interleavings must not change charged phlo"
    );
    assert_eq!(
        replay_root, checkpoint.root,
        "play/replay body interleavings must produce the same tuplespace root"
    );
}

async fn deterministic_checkpoint(term: &str) -> (Blake2b256Hash, Log) {
    let mut store_manager = InMemoryStoreManager::new();
    let stores = store_manager
        .r_space_stores()
        .await
        .expect("failed to create in-memory stores");
    let (mut runtime, _, _) = create_runtimes(stores, false, &mut Vec::new()).await;
    let result = runtime
        .evaluate(
            term,
            Cost::create(100_000, "deterministic reduction frontier"),
            Default::default(),
            Blake2b512Random::create_from_bytes(&[]),
        )
        .await
        .expect("evaluation failed");
    assert!(
        result.errors.is_empty(),
        "evaluation errors: {:?}",
        result.errors
    );
    let checkpoint = runtime.create_checkpoint().await;
    (checkpoint.root, checkpoint.log)
}

// Superseded by DR-129: restored dev's randomized candidate order (user
// decision 2026-10-12). Play takes either datum, so the term ends in one of two
// states. The test below replaces this one.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn competing_parallel_produces_have_one_canonical_comm_and_residual_state() {
//     let term = r#"@"x"!(1) | @"x"!(2) | for (@y <- @"x") { @"out"!(y) }"#;
//     let expected = deterministic_checkpoint(term).await;
//     for _ in 0..16 {
//         let observed = deterministic_checkpoint(term).await;
//         assert_eq!(observed.0, expected.0);
//         assert_eq!(observed.1, expected.1);
//     }
// }

/// Added by DR-129: the number of runs of each term of an outcome test. A term
/// with two equally likely outcomes misses one in every run with probability
/// `2 * 2^-OUTCOME_RUNS`.
const OUTCOME_RUNS: usize = 64;

/// Added by DR-129: one run of `term`. Play evaluates the term, and the legacy
/// replay replays the recorded log. Replay must charge the same phlo, use
/// every recorded event and reach the same root. Returns the root and the
/// outcome: the data that the run left on the `observed` channels.
async fn replayed_outcome(term: &str, observed: &[&str]) -> (Blake2b256Hash, String) {
    let mut store_manager = InMemoryStoreManager::new();
    let stores = store_manager
        .r_space_stores()
        .await
        .expect("failed to create in-memory stores");
    // Changed by the merge of feature/cost-accounted-rho (finding R1-39): play
    // and replay charges are compared below, so both run in the CostV1 epoch.
    // let (mut runtime, mut replay_runtime, _) =
    //     create_runtimes(stores, false, &mut Vec::new()).await;
    let (mut runtime, mut replay_runtime, _) =
        create_runtimes_for_epoch(EventCodecEpoch::CostV1, stores, false, &mut Vec::new()).await;
    let initial_phlo = Cost::create(100_000, "random candidate order outcome");
    let rand = Blake2b512Random::create_from_bytes(&[]);
    let pre_state_root = runtime.get_root().await;
    let play = runtime
        .evaluate(term, initial_phlo.clone(), Default::default(), rand.clone())
        .await
        .expect("play evaluation failed");
    assert!(play.errors.is_empty(), "play errors: {:?}", play.errors);
    let checkpoint = runtime.create_checkpoint().await;
    replay_runtime
        .reset(&pre_state_root)
        .await
        .expect("replay reset failed");
    replay_runtime
        .rig(checkpoint.log.clone())
        .await
        .expect("replay rig failed");
    let replay = replay_runtime
        .evaluate(term, initial_phlo, Default::default(), rand)
        .await
        .expect("replay evaluation failed");
    assert!(
        replay.errors.is_empty(),
        "replay errors: {:?}",
        replay.errors
    );
    // Added by the merge of feature/cost-accounted-rho (finding R1-39): the
    // comparison is evidence only when play charges something.
    assert!(play.cost.value > 0, "play must charge the COMMs");
    assert_eq!(
        replay.cost.value, play.cost.value,
        "replay charged other phlo for {term}"
    );
    replay_runtime
        .check_replay_data()
        .await
        .expect("replay left recorded events unused");
    assert_eq!(
        replay_runtime.create_checkpoint().await.root,
        checkpoint.root,
        "replay reached another root for {term}"
    );
    let mut outcome = Vec::with_capacity(observed.len());
    for name in observed {
        let channel = models::rust::utils::new_gstring_par(name.to_string(), Vec::new(), false);
        // Changed by the merge of feature/cost-accounted-rho (finding R1-26):
        // F1R3Lang's `get_data` is fallible, and its `Datum` shares the
        // payload by `Arc`.
        //     .await
        //     .into_iter()
        //     .flat_map(|datum| datum.a.pars)
        let mut values: Vec<String> = runtime
            .get_data(&channel)
            .await
            .expect("the runtime reads the observed channel")
            .into_iter()
            .flat_map(|datum| datum.a.pars.clone())
            .map(|par| PrettyPrinter::new().build_string_from_message(&par))
            .collect();
        values.sort();
        outcome.push(format!("{name}={values:?}"));
    }
    (checkpoint.root, outcome.join(" "))
}

/// Added by DR-129: plays every term `OUTCOME_RUNS` times and replays each
/// record. The outcomes seen must be exactly `expected`, and each outcome
/// must end in one root.
async fn assert_outcomes(terms: &[&str], observed: &[&str], expected: &[&str]) {
    let mut roots: BTreeMap<String, BTreeSet<Blake2b256Hash>> = BTreeMap::new();
    for _ in 0..OUTCOME_RUNS {
        for term in terms {
            let (root, outcome) = replayed_outcome(term, observed).await;
            roots.entry(outcome).or_default().insert(root);
        }
    }
    let seen: BTreeSet<&str> = roots.keys().map(String::as_str).collect();
    let known: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(seen, known, "the outcomes of {terms:?}");
    for (outcome, outcome_roots) in &roots {
        assert_eq!(
            outcome_roots.len(),
            1,
            "outcome {outcome} ended in several roots"
        );
    }
}

/// Added by DR-129: both data match the receive, and play takes either one.
/// Each run replays its own record, and the runs show both outcomes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_parallel_produces_end_in_either_outcome_and_replay() {
    assert_outcomes(
        &[r#"@"x"!(1) | @"x"!(2) | for (@y <- @"x") { @"out"!(y) }"#],
        &["out", "x"],
        &[r#"out=["1"] x=["2"]"#, r#"out=["2"] x=["1"]"#],
    )
    .await;
}

async fn assert_canonical_permutations(terms: &[&str]) {
    let expected = deterministic_checkpoint(terms[0]).await;
    for _ in 0..4 {
        for term in terms {
            let observed = deterministic_checkpoint(term).await;
            assert_eq!(observed.0, expected.0, "root diverged for {term}");
            assert_eq!(observed.1, expected.1, "event log diverged for {term}");
        }
    }
}

// DR-129: this test stays canonical under dev's random play order. The
// scheduler runs the sends before the receives, so the first receive in path
// order takes the datum and play meets no choice (spec runs 2026-10-13).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_consumers_are_canonical_across_parallel_permutations() {
    assert_canonical_permutations(&[
        r#"for (@a <- @"x") { @"left"!(a) } | for (@b <- @"x") { @"right"!(b) } | @"x"!(1)"#,
        r#"@"x"!(1) | for (@b <- @"x") { @"right"!(b) } | for (@a <- @"x") { @"left"!(a) }"#,
        r#"for (@b <- @"x") { @"right"!(b) } | @"x"!(1) | for (@a <- @"x") { @"left"!(a) }"#,
    ])
    .await;
}

// DR-129: this test stays canonical under dev's random play order. The
// scheduler runs the sends before the receives, so the first receive in path
// order takes the datum and play meets no choice (spec runs 2026-10-13).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlapping_join_and_single_channel_consume_are_one_canonical_component() {
    assert_canonical_permutations(&[
        r#"@"x"!(1) | @"y"!(2) | for (@a <- @"x" & @b <- @"y") { @"join"!((a, b)) } | for (@c <- @"x") { @"single"!(c) }"#,
        r#"for (@c <- @"x") { @"single"!(c) } | @"y"!(2) | for (@a <- @"x" & @b <- @"y") { @"join"!((a, b)) } | @"x"!(1)"#,
        r#"for (@a <- @"x" & @b <- @"y") { @"join"!((a, b)) } | @"x"!(1) | for (@c <- @"x") { @"single"!(c) } | @"y"!(2)"#,
    ])
    .await;
}

// Superseded by DR-129: restored dev's randomized candidate order (user
// decision 2026-10-12). The guarded receive meets both data, and play tries
// either one first, so the guarded term ends in one of two states. The test
// below replaces this one.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn persistent_peek_and_guarded_matches_are_canonical() {
//     assert_canonical_permutations(&[
//         r#"@"x"!!(1) | for (@a <<- @"x") { @"peek"!(a) } | for (@b <- @"x") { @"take"!(b) }"#,
//         r#"for (@b <- @"x") { @"take"!(b) } | @"x"!!(1) | for (@a <<- @"x") { @"peek"!(a) }"#,
//     ])
//     .await;
//     assert_canonical_permutations(&[
//         r#"@"x"!(1) | @"x"!(2) | for (@a <- @"x" where a > 1) { @"out"!(a) }"#,
//         r#"for (@a <- @"x" where a > 1) { @"out"!(a) } | @"x"!(2) | @"x"!(1)"#,
//     ])
//     .await;
// }

/// Added by DR-129: the persistent datum still gives one canonical outcome.
/// The guarded receive meets both data: when play tries 1 first, the guard
/// vetoes it and play stores the receive without trying 2, as on dev. Each run
/// replays its own record, and the runs show both outcomes.
///
/// Changed by the merge of feature/cost-accounted-rho (decision 4, finding
/// R1-22): F1R3Lang's complete guard-aware search tries 2 after the guard
/// vetoes 1, so play never stores the receive while 2 rests. Every arrival
/// order ends in `out=["2"] x=["1"]`, and each run still replays its own
/// record. The test keeps its DR-129 name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persistent_peek_is_canonical_and_guarded_match_ends_in_either_outcome() {
    assert_canonical_permutations(&[
        r#"@"x"!!(1) | for (@a <<- @"x") { @"peek"!(a) } | for (@b <- @"x") { @"take"!(b) }"#,
        r#"for (@b <- @"x") { @"take"!(b) } | @"x"!!(1) | for (@a <<- @"x") { @"peek"!(a) }"#,
    ])
    .await;
    assert_outcomes(
        &[
            r#"@"x"!(1) | @"x"!(2) | for (@a <- @"x" where a > 1) { @"out"!(a) }"#,
            r#"for (@a <- @"x" where a > 1) { @"out"!(a) } | @"x"!(2) | @"x"!(1)"#,
        ],
        &["out", "x"],
        // Changed by the merge of feature/cost-accounted-rho (finding R1-22):
        // the stranded outcome `out=[] x=["1", "2"]` no longer occurs.
        // &[r#"out=["2"] x=["1"]"#, r#"out=[] x=["1", "2"]"#],
        &[r#"out=["2"] x=["1"]"#],
    )
    .await;
}

// Superseded by DR-129: restored dev's randomized candidate order (user
// decision 2026-10-12). The persistent receive takes the two data in either
// order, so the event log varies while the state does not. The test below
// replaces this one.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn persistent_receive_matches_multiple_parallel_produces_canonically() {
//     assert_canonical_permutations(&[
//         r#"for (@a <= @"x") { @"out"!(a) } | @"x"!(1) | @"x"!(2)"#,
//         r#"@"x"!(2) | for (@a <= @"x") { @"out"!(a) } | @"x"!(1)"#,
//         r#"@"x"!(1) | @"x"!(2) | for (@a <= @"x") { @"out"!(a) }"#,
//     ])
//     .await;
// }

/// Added by DR-129: the persistent receive takes both data in either order.
/// Each run replays its own record, and every run ends in the one state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persistent_receive_replays_every_order_of_parallel_produces() {
    assert_outcomes(
        &[
            r#"for (@a <= @"x") { @"out"!(a) } | @"x"!(1) | @"x"!(2)"#,
            r#"@"x"!(2) | for (@a <= @"x") { @"out"!(a) } | @"x"!(1)"#,
            r#"@"x"!(1) | @"x"!(2) | for (@a <= @"x") { @"out"!(a) }"#,
        ],
        &["out", "x"],
        &[r#"out=["1", "2"] x=[]"#],
    )
    .await;
}
