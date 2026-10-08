use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::CostSignature;
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
async fn native_signed_term_respects_host_rejection_and_preserves_nil_execution() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let term = CostSignedTerm {
        body: Some(Par::default()),
        signature: Some(CostSignature {
            value: Some(CostSignatureValue::Ground(vec![1, 2, 3])),
        }),
    };
    for (limit, succeeds) in [(0, false), (1_000_000, true)] {
        let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)));
        let result = deterministic_reduction::root_with_host_work(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            Some(host.clone()),
            reducer.eval_cost_signed_term(
                &term,
                &Env::new(),
                Blake2b512Random::create_from_length(128),
                &CostAuthority::default(),
            ),
        )
        .await;
        if succeeds {
            assert!(result.is_ok());
            assert!(host.report().rejection.is_none());
        } else {
            assert!(matches!(result, Err(InterpreterError::HostWorkRejected)));
            assert!(host.report().rejection.is_some());
        }
    }
}

#[tokio::test]
async fn economic_observation_preserves_fault_hidden_by_legacy_abort() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let (legacy, observed, _) = deterministic_reduction::root_with_observation(
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
    let (legacy, observed, _) = deterministic_reduction::root_with_observation(
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
        let (legacy, observed, _) = deterministic_reduction::root_with_observation(
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
        let (legacy, next, _) = deterministic_reduction::root_with_observation(
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
    let (legacy, observed, detached_errors) = deterministic_reduction::root_with_observation(
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
    assert!(matches!(
        reducer.finish_detached_errors(legacy.map(|_| ()), detached_errors),
        Err(InterpreterError::UserAbortError)
    ));
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
    let (_, next, _) = tokio::time::timeout(
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

/// D-E3 (DR-110): the walker usage that `walk` reserves.
fn walk_usage(walk: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>) -> [usize; 3] {
    let used = std::cell::Cell::new([0usize; 3]);
    let meter = |operations: usize, scanned: usize, backing: usize| {
        let [o, s, b] = used.get();
        used.set([o + operations, s + scanned, b + backing]);
        Ok::<(), BackingError>(())
    };
    walk(&meter).expect("an unlimited walk");
    used.get()
}

/// D-E3 (DR-110): a reducer copy charges a block inspection of the value for
/// the release of the copy, and a block copy whose owned meter doubles the
/// operations.
#[test]
fn reducer_copy_charges_a_block_inspection_and_an_owned_block_copy() {
    let value = CostSignature {
        value: Some(CostSignatureValue::Quote(
            models::rust::utils::new_gstring_par("r".repeat(4_096), Vec::new(), false),
        )),
    };
    let inspected = std::cell::Cell::new([0usize; 3]);
    let copied = std::cell::Cell::new([0usize; 3]);
    let backing = |operations: usize, scanned: usize, bytes: usize| {
        let [o, s, b] = inspected.get();
        inspected.set([o + operations, s + scanned, b + bytes]);
        Ok::<(), BackingError>(())
    };
    let owned_backing = |operations: usize, scanned: usize, bytes: usize| {
        let [o, s, b] = copied.get();
        copied.set([o + operations, s + scanned, b + bytes]);
        Ok::<(), BackingError>(())
    };
    reserve_reducer_copy(&value, &backing, &owned_backing).expect("an unlimited copy");
    assert_eq!(
        inspected.get(),
        walk_usage(|meter| clone_backing::inspect_blocks(&value, meter))
    );
    assert_eq!(
        copied.get(),
        walk_usage(|meter| clone_backing::reserve_blocks(&value, meter))
    );
}

/// D-E3 (DR-110): the source of a stack produce charges one block inspection
/// of the channel and of the datum for the two bincode passes, then the
/// charge of the metered source itself, and it returns the unmetered source.
#[test]
fn stack_produce_source_charges_both_bincode_passes() {
    use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;

    let channel = models::rust::utils::new_gstring_par("q".repeat(4_096), Vec::new(), false);
    let datum = ListParWithRandom {
        pars: Vec::new(),
        random_state: vec![3; 128],
        cost_authority: None,
        cost_stack: Some(CostStack {
            cells: vec![CostSignature {
                value: Some(CostSignatureValue::Ground(vec![4; 32])),
            }],
        }),
    };
    let walked = std::cell::Cell::new([0usize; 3]);
    let backing = |operations: usize, scanned: usize, bytes: usize| {
        let [o, s, b] = walked.get();
        walked.set([o + operations, s + scanned, b + bytes]);
        Ok::<(), BackingError>(())
    };
    let sourced = std::cell::Cell::new([0usize; 3]);
    let source_meter = |operations: usize, scanned: usize, bytes: usize| {
        let [o, s, b] = sourced.get();
        sourced.set([o + operations, s + scanned, b + bytes]);
        Ok::<(), rspace_plus_plus::rspace::errors::RSpaceError>(())
    };
    let source = produce_source_metered(&channel, &datum, &backing, &source_meter)
        .expect("an unlimited source");
    assert_eq!(source, Produce::create(&channel, &datum, false));
    let channel_pass = walk_usage(|meter| clone_backing::inspect_blocks(&channel, meter));
    let datum_pass = walk_usage(|meter| clone_backing::inspect_blocks(&datum, meter));
    assert_eq!(
        walked.get(),
        [0, 1, 2].map(|index| channel_pass[index] + datum_pass[index])
    );
    let expected_source = std::cell::Cell::new([0usize; 3]);
    let source_only = |operations: usize, scanned: usize, bytes: usize| {
        let [o, s, b] = expected_source.get();
        expected_source.set([o + operations, s + scanned, b + bytes]);
        Ok::<(), rspace_plus_plus::rspace::errors::RSpaceError>(())
    };
    Produce::create_metered(&channel, &datum, false, &source_only as &dyn SourceMeter)
        .expect("an unlimited metered source");
    assert_eq!(sourced.get(), expected_source.get());
}

/// D-E3 (DR-110): the signed term and the stack paths reserve their exact
/// host work before the work. Each accepts exactly the credit that it uses and
/// rejects one unit less in any used dimension.
#[tokio::test]
async fn cost_signed_term_and_stack_accept_exact_credit_and_reject_each_shorter_dimension() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let ground = CostSignature {
        value: Some(CostSignatureValue::Ground(vec![5; 32])),
    };
    let term = CostSignedTerm {
        body: Some(Par::default()),
        signature: Some(ground.clone()),
    };
    let stack = CostStack {
        cells: vec![ground.clone()],
    };
    for signed in [true, false] {
        let run = |limits: HostWorkLimits| {
            let reducer = reducer.clone();
            let term = term.clone();
            let stack = stack.clone();
            async move {
                let host = HostWorkBudget::new(limits);
                let result = deterministic_reduction::root_with_host_work(
                    reducer.space.clone(),
                    reducer.metering.budget(),
                    reducer.reduction_coordinator.clone(),
                    Some(host.clone()),
                    async {
                        if signed {
                            reducer
                                .eval_cost_signed_term(
                                    &term,
                                    &Env::new(),
                                    Blake2b512Random::create_from_length(128),
                                    &CostAuthority::default(),
                                )
                                .await
                        } else {
                            reducer
                                .eval_cost_stack(
                                    &stack,
                                    &Env::new(),
                                    Blake2b512Random::create_from_length(128),
                                    &CostAuthority::default(),
                                )
                                .await
                        }
                    },
                )
                .await;
                (result, host)
            }
        };
        let (result, measured) = run(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX))).await;
        assert!(result.is_ok(), "signed {signed}: {result:?}");
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(0));
        for dimension in HostWorkDimension::ALL {
            exact.set(
                dimension,
                HostWorkLimit::new(measured.usage(dimension).get()),
            );
        }
        let (result, _) = run(exact.clone()).await;
        assert!(result.is_ok(), "signed {signed}: exact credit");
        for dimension in HostWorkDimension::ALL {
            let required = measured.usage(dimension).get();
            if required == 0 {
                continue;
            }
            let mut short = exact.clone();
            short.set(dimension, HostWorkLimit::new(required - 1));
            let (result, host) = run(short).await;
            assert!(
                matches!(result, Err(InterpreterError::HostWorkRejected)),
                "signed {signed}: {dimension:?} one short gave {result:?}"
            );
            assert!(host.report().rejection.is_some());
        }
    }
}
