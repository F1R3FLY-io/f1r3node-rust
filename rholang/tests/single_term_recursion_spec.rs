use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recursive_single_term_evaluation_has_bounded_task_and_yield_work() {
    const ITERATIONS: u64 = 2_000;

    with_runtime("single-term-recursion-", |runtime| async move {
        let term = format!(
            "new loop in {{ contract loop(@n) = {{ match n {{ 0 => Nil _ => loop!(n - 1) }} }} | loop!({ITERATIONS}) }}"
        );
        runtime.reducer.reset_eval_work_stats();

        let result = runtime
            .evaluate(
                &term,
                Cost::create(i64::MAX, "single-term recursion work bound".to_string()),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(&[]),
            )
            .await
            .expect("recursive evaluation failed");

        assert!(result.errors.is_empty(), "recursive evaluation returned errors: {:?}", result.errors);
        let stats = runtime.reducer.eval_work_stats();
        assert!(stats.single_term_evaluations >= ITERATIONS);
        assert!(stats.spawned_eval_tasks <= 4, "unexpected recursive task growth: {stats:?}");
        assert!(stats.yielded_single_term_evaluations > 0);
        assert!(stats.yielded_single_term_evaluations * 256 <= stats.single_term_evaluations);
        assert!(stats.single_term_evaluations < (stats.yielded_single_term_evaluations + 1) * 256);
    })
    .await;
}
