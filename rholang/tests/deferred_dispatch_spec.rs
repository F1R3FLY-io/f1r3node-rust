use std::collections::HashMap;
use std::time::Duration;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::{create_runtimes, with_runtime};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn unlimited_phlo() -> Cost { Cost::create(i64::MAX, "deferred dispatch spec".to_string()) }

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[]) }

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_evaluation_aborts_deferred_dispatches() {
    with_runtime("deferred-dispatch-cancel-", |runtime| async move {
        let term = "new loop in { contract loop(@n) = { if (n <= 0) { Nil } else { loop!(n - 1) } } | loop!(1000000000) }";
        runtime.reducer.reset_eval_work_stats();

        let evaluation = runtime.evaluate(term, unlimited_phlo(), HashMap::new(), rand());
        assert!(
            tokio::time::timeout(Duration::from_millis(500), evaluation)
                .await
                .is_err(),
            "the loop must still run when the evaluation is dropped"
        );

        tokio::time::sleep(Duration::from_millis(200)).await;
        let stopped_at = runtime.reducer.eval_work_stats().single_term_evaluations;
        assert!(stopped_at > 0, "the loop did not start");

        tokio::time::sleep(Duration::from_millis(500)).await;
        let later = runtime.reducer.eval_work_stats().single_term_evaluations;
        assert_eq!(
            later, stopped_at,
            "deferred dispatches kept running after the evaluation was dropped"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fan_out_play_and_replay_charge_the_same_cost_in_every_round() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm
        .r_space_stores()
        .await
        .expect("Failed to create in-memory rspace store");
    let mut additional_system_processes = Vec::new();
    let (mut runtime, mut replay_runtime, _) =
        create_runtimes(store, false, &mut additional_system_processes).await;

    let sends = (0..16)
        .map(|i| format!("loop!({})", 200 + i))
        .collect::<Vec<_>>()
        .join(" | ");
    let term = format!(
        "new loop, done in {{ contract loop(@n) = {{ if (n <= 0) {{ done!(n) }} else {{ loop!(n - 1) }} }} | for (_ <= done) {{ Nil }} | {sends} }}"
    );

    let mut costs = Vec::new();
    for _ in 0..3 {
        let play_checkpoint = runtime.create_soft_checkpoint().await;
        let replay_checkpoint = replay_runtime.create_soft_checkpoint().await;

        let play = runtime
            .evaluate(&term, unlimited_phlo(), HashMap::new(), rand())
            .await
            .expect("play failed");
        assert!(play.errors.is_empty(), "play errors: {:?}", play.errors);

        let log = runtime.take_event_log().await;
        replay_runtime.rig(log).await.expect("replay rig failed");
        let replay = replay_runtime
            .evaluate(&term, unlimited_phlo(), HashMap::new(), rand())
            .await
            .expect("replay failed");
        assert!(
            replay.errors.is_empty(),
            "replay errors: {:?}",
            replay.errors
        );
        replay_runtime
            .check_replay_data()
            .await
            .expect("replay data check failed");
        assert_eq!(play.cost, replay.cost);
        costs.push(play.cost.value);

        runtime.revert_to_soft_checkpoint(play_checkpoint).await;
        replay_runtime
            .revert_to_soft_checkpoint(replay_checkpoint)
            .await;
    }

    assert!(
        costs.windows(2).all(|pair| pair[0] == pair[1]),
        "fan-out cost differs between rounds: {costs:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn error_in_deferred_body_is_reported_by_the_evaluation() {
    with_runtime("deferred-dispatch-error-", |runtime| async move {
        let term = "new c in { contract c(@x) = { if (x) { Nil } else { Nil } } | c!(1) }";

        let result = runtime
            .evaluate(term, unlimited_phlo(), HashMap::new(), rand())
            .await
            .expect("evaluation failed");

        assert!(
            matches!(result.errors.as_slice(), [
                InterpreterError::IfConditionTypeError { .. }
            ]),
            "unexpected errors: {:?}",
            result.errors
        );
    })
    .await;
}
