use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use rspace_plus_plus::rspace::rspace::RSpace;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::phlo_execution::PhloFailure;
use crate::rust::interpreter::rho_runtime::RhoRuntime;
use crate::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use crate::rust::interpreter::test_utils::resources::with_runtime;

#[tokio::test]
async fn economic_observation_preserves_fault_hidden_by_legacy_abort() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let (legacy, observed) = deterministic_reduction::root_with_observation(
        reducer.space.clone(),
        reducer.metering.budget(),
        reducer.reduction_coordinator.clone(),
        None,
        async {
            reducer.aggregate_evaluator_errors(vec![
                InterpreterError::UserAbortError,
                InterpreterError::BugFoundError("injected platform failure".into()),
            ])
        },
    )
    .await;
    assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
    assert_eq!(
        observed,
        EvaluationFailureSummary::single(PhloFailure::User)
            .union(EvaluationFailureSummary::single(PhloFailure::Platform))
    );
    assert!(!observed.permits_retained_charge());
}

#[tokio::test]
async fn economic_observation_quota_does_not_change_execution_budget_or_legacy_error() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let execution = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    let before = execution.report();
    let (legacy, observed) = deterministic_reduction::root_with_observation(
        reducer.space.clone(),
        reducer.metering.budget(),
        reducer.reduction_coordinator.clone(),
        Some(execution.clone()),
        async { reducer.aggregate_evaluator_errors(vec![InterpreterError::UserAbortError]) },
    )
    .await;
    assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
    assert_eq!(execution.report(), before);
    assert_eq!(
        observed,
        EvaluationFailureSummary::single(PhloFailure::Platform)
    );
}

#[tokio::test]
async fn economic_observation_joins_nested_faults_without_reusing_the_previous_root() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    for reverse in [false, true] {
        let mut errors = vec![
            InterpreterError::UserAbortError,
            InterpreterError::AggregateError {
                interpreter_errors: vec![
                    InterpreterError::OutOfPhlogistonsError,
                    InterpreterError::ReduceError("ambiguous failure".into()),
                ],
            },
        ];
        if reverse {
            errors.reverse();
        }
        let (legacy, observed) = deterministic_reduction::root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async { reducer.aggregate_evaluator_errors(errors) },
        )
        .await;
        assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
        assert_eq!(
            observed,
            EvaluationFailureSummary::single(PhloFailure::User)
                .union(EvaluationFailureSummary::single(PhloFailure::Certificate))
                .union(EvaluationFailureSummary::single(PhloFailure::Unclassified))
        );
        let (legacy, next) = deterministic_reduction::root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async { reducer.aggregate_evaluator_errors(Vec::new()) },
        )
        .await;
        assert!(legacy.is_ok());
        assert_eq!(next, EvaluationFailureSummary::default());
        assert!(!observed.permits_retained_charge());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn economic_observation_keeps_faults_collapsed_inside_parallel_children() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let child_a = reducer.clone();
    let child_b = reducer.clone();
    let (legacy, observed) = deterministic_reduction::root_with_observation(
        reducer.space.clone(),
        reducer.metering.budget(),
        reducer.reduction_coordinator.clone(),
        None,
        async {
            reducer
                .run_parallel_dispatches(vec![
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        child_a.aggregate_evaluator_errors(vec![
                            InterpreterError::UserAbortError,
                            InterpreterError::BugFoundError("parallel platform fault".into()),
                        ])
                    }),
                    Box::pin(async move {
                        child_b.aggregate_evaluator_errors(vec![
                            InterpreterError::UserAbortError,
                            InterpreterError::OutOfPhlogistonsError,
                        ])
                    }),
                ])
                .await
        },
    )
    .await;
    assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
    assert_eq!(
        observed,
        EvaluationFailureSummary::single(PhloFailure::User)
            .union(EvaluationFailureSummary::single(PhloFailure::Platform))
            .union(EvaluationFailureSummary::single(PhloFailure::Certificate))
    );
}

#[tokio::test]
async fn economic_observation_cancellation_does_not_leak_into_the_next_root() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let (reported, observed) = tokio::sync::oneshot::channel();
    {
        let canceled = deterministic_reduction::root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {
                deterministic_reduction::record_evaluator_failures(&[
                    InterpreterError::BugFoundError("canceled root failure".into()),
                ]);
                reported.send(()).unwrap();
                std::future::pending::<()>().await;
            },
        );
        tokio::pin!(canceled);
        tokio::select! {
            _ = &mut canceled => panic!("root must remain pending until cancellation"),
            result = observed => result.unwrap(),
        }
    }
    let (_, next) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        deterministic_reduction::root_with_observation(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            None,
            async {},
        ),
    )
    .await
    .expect("canceled root must release its evaluation guard");
    assert_eq!(next, EvaluationFailureSummary::default());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn economic_observation_reaches_results_and_isolates_parse_and_reduction_failures() {
    with_runtime("economic-observation-", |mut runtime| async move {
        let abort = runtime
            .evaluate_with_term("new abort(`rho:execution:abort`) in { abort!(Nil) }")
            .await
            .unwrap();
        assert_eq!(abort.errors, vec![InterpreterError::UserAbortError]);
        assert_eq!(
            abort.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
        );

        let mixed = runtime
            .evaluate_with_term(
                "new abort(`rho:execution:abort`) in { abort!(Nil) | @\"result\"!(1 / 0) }",
            )
            .await
            .unwrap();
        assert_eq!(mixed.errors, vec![InterpreterError::UserAbortError]);
        assert_eq!(
            mixed.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
                .union(EvaluationFailureSummary::single(PhloFailure::Unclassified))
        );

        let malformed = runtime.evaluate_with_term("new").await.unwrap();
        assert!(!malformed.errors.is_empty());
        assert_eq!(
            malformed.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::Unclassified)
        );

        let negative = runtime
            .evaluate_with_phlo("Nil", Cost::create(-1, "invalid initial phlo"))
            .await
            .unwrap();
        assert!(!negative.errors.is_empty());
        assert_eq!(
            negative.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::Unclassified)
        );

        let (host_failure, report) = runtime
            .evaluate_with_host_work(
                "Nil",
                Cost::unsafe_max(),
                HashMap::new(),
                Blake2b512Random::create_from_length(128),
                HostWorkLimits::uniform(HostWorkLimit::new(0)),
            )
            .await
            .unwrap();
        assert!(report.rejection.is_some());
        assert_eq!(host_failure.errors, vec![
            InterpreterError::HostWorkRejected
        ]);
        assert_eq!(
            host_failure.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::Platform)
        );

        let success = runtime.evaluate_with_term("Nil").await.unwrap();
        assert!(success.errors.is_empty());
        assert_eq!(
            success.economic_failures,
            EvaluationFailureSummary::default()
        );
        assert_eq!(
            abort.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
        );
    })
    .await;
}
