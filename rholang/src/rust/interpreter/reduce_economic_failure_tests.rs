use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::CostSignature;
use rspace_plus_plus::rspace::rspace::RSpace;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::economic_failure::EvaluationFailureSummary;
use crate::rust::interpreter::accounting::phlo_execution::PhloFailure;
use crate::rust::interpreter::rho_runtime::RhoRuntime;
use crate::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use crate::rust::interpreter::test_utils::resources::with_runtime;

/// Changed by DR-131: the host-work rejection half went with host work. The
/// test keeps its check that a signed term with a Nil body executes.
#[tokio::test]
async fn native_signed_term_preserves_nil_execution() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let term = CostSignedTerm {
        body: Some(Par::default()),
        signature: Some(CostSignature {
            value: Some(CostSignatureValue::Ground(vec![1, 2, 3])),
        }),
    };
    // Changed by DR-132 (T6): the term runs in a per-call scope, as `inj` runs
    // it. The scheduler root is gone (D3-A).
    // let result = deterministic_reduction::root(
    //     reducer.space.clone(),
    //     reducer.metering.budget(),
    //     reducer.reduction_coordinator.clone(),
    //     reducer.eval_cost_signed_term(
    //         &term,
    //         &Env::new(),
    //         Blake2b512Random::create_from_length(128),
    //         &CostAuthority::default(),
    //     ),
    // )
    // .await;
    let result = with_dispatch_scope(reducer.eval_cost_signed_term(
        &term,
        &Env::new(),
        Blake2b512Random::create_from_length(128),
        &CostAuthority::default(),
    ))
    .await
    .0;
    assert!(result.is_ok());
}

#[tokio::test]
async fn economic_observation_preserves_fault_hidden_by_legacy_abort() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Changed by DR-132 (T6): a per-call scope replaces the scheduler root.
    // let (legacy, observed, _) = deterministic_reduction::root_with_observation(
    //     reducer.space.clone(),
    //     reducer.metering.budget(),
    //     reducer.reduction_coordinator.clone(),
    //     async {
    //         reducer.aggregate_evaluator_errors(vec![
    //             InterpreterError::UserAbortError,
    //             InterpreterError::BugFoundError("injected platform failure".into()),
    //         ])
    //     },
    // )
    // .await;
    let (legacy, observed, _) = with_dispatch_scope(async {
        reducer.aggregate_evaluator_errors(vec![
            InterpreterError::UserAbortError,
            InterpreterError::BugFoundError("injected platform failure".into()),
        ])
    })
    .await;
    assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
    assert_eq!(
        observed,
        EvaluationFailureSummary::single(PhloFailure::User)
            .union(EvaluationFailureSummary::single(PhloFailure::Platform))
    );
    assert!(!observed.permits_retained_charge());
}

/// Changed by DR-131 (F-4): the classification has no quota, so a user abort
/// is a user failure. It was a platform failure while a zero quota rejected
/// the classification. The execution budget that the test checked went with
/// host work.
#[tokio::test]
async fn economic_observation_classifies_by_errors_and_keeps_the_legacy_error() {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    // Changed by DR-132 (T6): a per-call scope replaces the scheduler root.
    // let (legacy, observed, _) = deterministic_reduction::root_with_observation(
    //     reducer.space.clone(),
    //     reducer.metering.budget(),
    //     reducer.reduction_coordinator.clone(),
    //     async { reducer.aggregate_evaluator_errors(vec![InterpreterError::UserAbortError]) },
    // )
    // .await;
    let (legacy, observed, _) = with_dispatch_scope(async {
        reducer.aggregate_evaluator_errors(vec![InterpreterError::UserAbortError])
    })
    .await;
    assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
    assert_eq!(
        observed,
        EvaluationFailureSummary::single(PhloFailure::User)
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
        // Changed by DR-132 (T6): a per-call scope replaces the scheduler root.
        // let (legacy, observed, _) = deterministic_reduction::root_with_observation(
        //     reducer.space.clone(),
        //     reducer.metering.budget(),
        //     reducer.reduction_coordinator.clone(),
        //     async { reducer.aggregate_evaluator_errors(errors) },
        // )
        // .await;
        let (legacy, observed, _) =
            with_dispatch_scope(async { reducer.aggregate_evaluator_errors(errors) }).await;
        assert!(matches!(legacy, Err(InterpreterError::UserAbortError)));
        assert_eq!(
            observed,
            EvaluationFailureSummary::single(PhloFailure::User)
                .union(EvaluationFailureSummary::single(PhloFailure::Certificate))
                .union(EvaluationFailureSummary::single(PhloFailure::Unclassified))
        );
        // Changed by DR-132 (T6): a per-call scope replaces the scheduler root.
        // let (legacy, next, _) = deterministic_reduction::root_with_observation(
        //     reducer.space.clone(),
        //     reducer.metering.budget(),
        //     reducer.reduction_coordinator.clone(),
        //     async { reducer.aggregate_evaluator_errors(Vec::new()) },
        // )
        // .await;
        let (legacy, next, _) =
            with_dispatch_scope(async { reducer.aggregate_evaluator_errors(Vec::new()) }).await;
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
    // Changed by DR-132 (T6): a per-call scope replaces the scheduler root.
    // let (legacy, observed, detached_errors) = deterministic_reduction::root_with_observation(
    //     reducer.space.clone(),
    //     reducer.metering.budget(),
    //     reducer.reduction_coordinator.clone(),
    //     async {
    //         reducer
    //             .run_parallel_dispatches(vec![
    //                 Box::pin(async move {
    //                     tokio::task::yield_now().await;
    //                     child_a.aggregate_evaluator_errors(vec![
    //                         InterpreterError::UserAbortError,
    //                         InterpreterError::BugFoundError("parallel platform fault".into()),
    //                     ])
    //                 }),
    //                 Box::pin(async move {
    //                     child_b.aggregate_evaluator_errors(vec![
    //                         InterpreterError::UserAbortError,
    //                         InterpreterError::OutOfPhlogistonsError,
    //                     ])
    //                 }),
    //             ])
    //             .await
    //     },
    // )
    // .await;
    let (legacy, observed, detached_errors) = with_dispatch_scope(async {
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
    })
    .await;
    // Changed by DR-132 (T6, P7): inside the scope the parent returns at once,
    // and the drain returns the task errors. The final aggregate of `inj`
    // collapses them.
    // assert!(matches!(
    //     reducer.finish_detached_errors(legacy.map(|_| ()), detached_errors),
    //     Err(InterpreterError::UserAbortError)
    // ));
    assert!(matches!(legacy, Ok(DispatchType::Skip)));
    assert!(matches!(
        reducer.aggregate_evaluator_errors(detached_errors),
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
    // Changed by DR-132 (T6): a per-call scope replaces the scheduler root,
    // and the scope needs no reducer.
    // let (_, reducer) =
    //     create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
    //         .await;
    let (reported, observed) = tokio::sync::oneshot::channel();
    {
        // let canceled = deterministic_reduction::root_with_observation(
        //     reducer.space.clone(),
        //     reducer.metering.budget(),
        //     reducer.reduction_coordinator.clone(),
        //     async {
        //         deterministic_reduction::record_evaluator_failures(&[
        //             InterpreterError::BugFoundError("canceled root failure".into()),
        //         ]);
        //         reported.send(()).unwrap();
        //         std::future::pending::<()>().await;
        //     },
        // );
        let canceled = with_dispatch_scope(async {
            record_evaluator_failures(&[InterpreterError::BugFoundError(
                "canceled root failure".into(),
            )]);
            reported
                .send(())
                .expect("the test waits for the failure report");
            std::future::pending::<()>().await;
        });
        tokio::pin!(canceled);
        tokio::select! {
            _ = &mut canceled => panic!("root must remain pending until cancellation"),
            result = observed => result.unwrap(),
        }
    }
    // Changed by DR-132 (T6): the scope of each call is new, and no evaluation
    // guard exists.
    // let (_, next, _) = tokio::time::timeout(
    //     std::time::Duration::from_secs(2),
    //     deterministic_reduction::root_with_observation(
    //         reducer.space.clone(),
    //         reducer.metering.budget(),
    //         reducer.reduction_coordinator.clone(),
    //         async {},
    //     ),
    // )
    // .await
    // .expect("canceled root must release its evaluation guard");
    let (_, next, _) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        with_dispatch_scope(async {}),
    )
    .await
    .expect("a canceled call leaves nothing that blocks the next call");
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
        // Changed by DR-128 (bug 11414): a division by zero is a user failure,
        // so this run holds the user class alone. The run below keeps the
        // unclassified veto of a mixed run with the partly signed join, which
        // DR-128 leaves an unclassified reduce error.
        // assert_eq!(
        //     mixed.economic_failures,
        //     EvaluationFailureSummary::single(PhloFailure::User)
        //         .union(EvaluationFailureSummary::single(PhloFailure::Unclassified))
        // );
        assert_eq!(
            mixed.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
        );
        let unclassified = runtime
            .evaluate_with_term(
                "new abort(`rho:execution:abort`), x, w in { abort!(Nil) | \
                 for( {% y <- x %}[ s ] & @z <- w ){ Nil } }",
            )
            .await
            .expect("the evaluation completes");
        assert_eq!(unclassified.errors, vec![InterpreterError::UserAbortError]);
        assert_eq!(
            unclassified.economic_failures,
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

// ── Unknown methods (DR-125, bug 11008) ────────────────────────────────────

/// Added by DR-125: the error of a call of `method`, which the method table
/// lacks. The reducer does not inspect the receiver, so the type is generic.
fn unknown_method(method: &str) -> InterpreterError {
    InterpreterError::MethodNotDefined {
        method: method.to_string(),
        other_type: "any type".to_string(),
    }
}

/// Added by DR-125: a call of a method that the method table lacks, in
/// process position (the first site, `eval_expr`), is the user failure
/// `MethodNotDefined`. The summary holds the user class alone, so it permits
/// the retained charge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_method_in_process_position_is_a_user_failure() {
    with_runtime("unknown-method-process-", |mut runtime| async move {
        let result = runtime
            .evaluate_with_term("{}.xxx()")
            .await
            .expect("the evaluation completes");
        assert_eq!(result.errors, vec![unknown_method("xxx")]);
        assert_eq!(
            result.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
        );
        assert!(result.economic_failures.permits_retained_charge());
    })
    .await;
}

/// Added by DR-125: a call of a method that the method table lacks, as an
/// operand (the second site, `eval_expr_to_expr`), is the same user failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_method_in_an_operand_is_a_user_failure() {
    with_runtime("unknown-method-operand-", |mut runtime| async move {
        let result = runtime
            .evaluate_with_term("@\"r\"!(1 + {}.xxx())")
            .await
            .expect("the evaluation completes");
        assert_eq!(result.errors, vec![unknown_method("xxx")]);
        assert_eq!(
            result.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
        );
        assert!(result.economic_failures.permits_retained_charge());
    })
    .await;
}

/// Added by DR-125: a property test with a fixed seed. For method names that
/// the method table lacks, both sites raise `MethodNotDefined` for the name,
/// and the summary is the user class alone. The names have a prefix that no
/// keyword and no table method has, and the test checks that the table lacks
/// each name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_method_names_are_user_failures_at_both_sites() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let table = reducer.method_table();
    let strategy = proptest::string::string_regex("unknown[A-Z][a-zA-Z0-9]{0,8}")
        .expect("the name pattern compiles");
    let mut runner = TestRunner::new_with_rng(
        Config::with_cases(16),
        TestRng::from_seed(RngAlgorithm::ChaCha, &[11u8; 32]),
    );
    let mut names = Vec::with_capacity(16);
    for _ in 0..16 {
        let name = strategy
            .new_tree(&mut runner)
            .expect("the strategy produces a name")
            .current();
        assert!(!table.contains_key(&name), "the table lacks {name}");
        names.push(name);
    }
    with_runtime("unknown-method-names-", |mut runtime| async move {
        for name in names {
            for term in [
                format!("{{}}.{name}()"),
                format!("@\"r\"!(1 + {{}}.{name}())"),
            ] {
                let result = runtime
                    .evaluate_with_term(&term)
                    .await
                    .expect("the evaluation completes");
                assert_eq!(result.errors, vec![unknown_method(&name)], "{term}");
                assert_eq!(
                    result.economic_failures,
                    EvaluationFailureSummary::single(PhloFailure::User),
                    "{term}"
                );
            }
        }
    })
    .await;
}

// ── Failure classes under racing evaluation (DR-132, P-4) ──────────────────

/// Added by DR-132 (P-4): the error of a generated branch. The kinds cover
/// each failure class and each error that collapses an aggregate.
fn generated_branch_error(kind: u8) -> InterpreterError {
    match kind % 6 {
        0 => InterpreterError::UserAbortError,
        1 => InterpreterError::OutOfPhlogistonsError,
        2 => InterpreterError::BugFoundError("generated platform failure".into()),
        3 => InterpreterError::ReduceError("generated unclassified failure".into()),
        4 => InterpreterError::SignedLimitExhausted,
        _ => InterpreterError::IfConditionTypeError {
            actual_type: "generated".into(),
        },
    }
}

/// Added by DR-132 (P-4): a failing branch that completes after `delay`
/// milliseconds, so the branches complete in an order that differs from the
/// order in which they start.
fn delayed_failure(
    error: InterpreterError,
    delay: u64,
) -> Pin<Box<dyn Future<Output = Result<DispatchType, InterpreterError>> + std::marker::Send>> {
    Box::pin(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        Err(error)
    })
}

/// Added by DR-132 (P-4): the debug forms of `errors` in a fixed order, to
/// compare two error lists as multisets.
fn sorted_debug(errors: &[InterpreterError]) -> Vec<String> {
    let mut forms: Vec<String> = errors.iter().map(|error| format!("{error:?}")).collect();
    forms.sort();
    forms
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    /// Added by DR-132 (P-4): failing branches with random delays run through
    /// `run_parallel_dispatches`, and failing continuation bodies run through
    /// `eval_continuation`, inside one per-call scope. Racing evaluation
    /// completes them in any order. The drain returns every error, and the
    /// failure classes of the scope equal `classify_errors` over all of them,
    /// whatever the order.
    #[test]
    fn failure_summary_ignores_completion_order(
        branches in proptest::collection::vec((0_u8..6, 0_u64..3), 1..10),
        bodies in 0_usize..4,
    ) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("a test runtime");
        let (expected, observed, drained) = runtime.block_on(async {
            let (_, reducer) =
                create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                    .await;
            let body = crate::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
                "if (1) { Nil } else { Nil }",
            )
            .expect("the body parses");
            let body_error = reducer
                .eval(body.clone(), &Env::new(), Blake2b512Random::create_from_length(128))
                .await
                .expect_err("the body fails");
            let mut expected: Vec<InterpreterError> = branches
                .iter()
                .map(|(kind, _)| generated_branch_error(*kind))
                .collect();
            expected.extend(std::iter::repeat_n(body_error, bodies));
            let futures = branches
                .iter()
                .map(|(kind, delay)| delayed_failure(generated_branch_error(*kind), *delay))
                .collect();
            let core = reducer.core.clone();
            let (dispatched, observed, drained) = with_dispatch_scope(async {
                for _ in 0..bodies {
                    core.eval_continuation(
                        body.clone(),
                        Env::new(),
                        Blake2b512Random::create_from_length(128),
                        false,
                    )
                    .await
                    .expect("a body in a scope returns at once");
                }
                reducer.run_parallel_dispatches(futures).await
            })
            .await;
            assert!(matches!(dispatched, Ok(DispatchType::Skip)));
            (expected, observed, drained)
        });
        proptest::prop_assert_eq!(sorted_debug(&drained), sorted_debug(&expected));
        proptest::prop_assert_eq!(
            observed,
            classify_errors(&expected).expect("the classification allocates")
        );
    }
}

/// Added by DR-132 (F5): a user failure in a continuation body is recorded
/// when phlo exhaustion collapses the error list. The body sends on `go` and
/// fails on its condition. The receive on `go` starts a loop that runs until
/// the phlo is spent. The list of the evaluation holds the user failure and
/// the exhaustion, and the aggregate collapses it to the exhaustion. `inj`
/// records the whole list before the aggregate, so the summary holds both
/// classes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_body_failure_is_recorded_when_exhaustion_collapses_the_list() {
    with_runtime("collapsed-body-failure-", |runtime| async move {
        let result = runtime
            .evaluate(
                "new c, go in { contract c(@x) = { go!(Nil) | if (x) { Nil } else { Nil } } | c!(1) | \
                 for (_ <- go) { new loop in { contract loop(@n) = { loop!(n + 1) } | loop!(0) } } }",
                Cost::create(100_000, "collapsed body failure".to_string()),
                HashMap::new(),
                Blake2b512Random::create_from_length(128),
            )
            .await
            .expect("the evaluation completes");
        assert_eq!(result.errors, vec![InterpreterError::OutOfPhlogistonsError]);
        assert_eq!(
            result.economic_failures,
            EvaluationFailureSummary::single(PhloFailure::User)
                .union(EvaluationFailureSummary::single(PhloFailure::Certificate))
        );
    })
    .await;
}

/// Added by DR-132 (A1): a direct `eval` without a per-call scope awaits every
/// task that it starts, as on dev. A persistent send and a peek fire their
/// bodies through `run_parallel_dispatches`, so the space holds the results of
/// the bodies when `eval` returns, and the failure of a body returns from
/// `eval`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_direct_eval_without_a_scope_awaits_persistent_sends_and_peeks() {
    use rspace_plus_plus::rspace::rspace_interface::ISpace;

    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let term = crate::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
        r#"@"x"!!(1) | for (@a <<- @"x") { @"out"!(a) } | @"y"!(2) | for (@b <<- @"y") { @"peeked"!(b) }"#,
    )
    .expect("the term parses");
    reducer
        .eval(term, &Env::new(), Blake2b512Random::create_from_length(128))
        .await
        .expect("the direct evaluation succeeds");
    for (channel, value) in [("out", 1), ("x", 1), ("peeked", 2), ("y", 2)] {
        let data = space
            .get_data(&new_gstring_par(channel.to_string(), Vec::new(), false))
            .await;
        assert_eq!(data.len(), 1, "{channel} holds one datum");
        assert_eq!(
            data[0].a.pars,
            vec![new_gint_par(value, Vec::new(), false)],
            "{channel}"
        );
    }

    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    let failing = crate::rust::interpreter::compiler::compiler::Compiler::source_to_adt(
        r#"@"x"!!(1) | for (@a <<- @"x") { if (a) { Nil } else { Nil } }"#,
    )
    .expect("the failing term parses");
    assert!(matches!(
        reducer
            .eval(
                failing,
                &Env::new(),
                Blake2b512Random::create_from_length(128)
            )
            .await,
        Err(InterpreterError::IfConditionTypeError { .. })
    ));
}

// ── Deterministic user reducer errors (DR-128, bug 11414) ──────────────────

/// Added by DR-128 (bug 11414): the reducer errors that depend only on the
/// user's term are classified user failures. Each test evaluates terms that
/// reach the sites of one error kind. It reports every site that does not end
/// in the expected error, classified as a user failure alone, so one run shows
/// every site.
mod user_reducer_errors {
    use super::*;
    use crate::rust::interpreter::accounting::economic_failure::classify_errors;
    use crate::rust::interpreter::rho_runtime::RhoRuntimeImpl;

    /// One site of a deterministic user reducer error: a term that reaches
    /// the site and the message that the site reports.
    struct Site {
        name: &'static str,
        term: String,
        message: String,
    }

    fn site(name: &'static str, term: &str, message: &str) -> Site {
        Site {
            name,
            term: term.to_string(),
            message: message.to_string(),
        }
    }

    /// Evaluates each site and returns every site whose evaluation does not
    /// end with exactly the error `variant(message)`, which reports
    /// `Reduce error: <message>` and is classified as a user failure alone.
    async fn mismatches(
        runtime: &mut RhoRuntimeImpl,
        variant: fn(String) -> InterpreterError,
        sites: &[Site],
    ) -> Vec<String> {
        let mut found = Vec::with_capacity(sites.len());
        for site in sites {
            let result = runtime
                .evaluate_with_term(&site.term)
                .await
                .expect("the evaluation completes");
            let text = format!("Reduce error: {}", site.message);
            let error = variant(site.message.clone());
            let texts = result
                .errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            if result.economic_failures != EvaluationFailureSummary::single(PhloFailure::User)
                || !result.economic_failures.permits_retained_charge()
                || texts.as_slice() != std::slice::from_ref(&text)
                || result.errors.as_slice() != std::slice::from_ref(&error)
                || error.to_string() != text
            {
                found.push(format!(
                    "{}: `{}` gave {:?} with {:?}",
                    site.name, site.term, result.economic_failures, result.errors
                ));
            }
        }
        found
    }

    /// Asserts that no site mismatched. The message shows the count and the
    /// first twenty sites.
    fn assert_no_mismatches(found: &[String], total: usize) {
        assert!(
            found.is_empty(),
            "{} of {} sites do not end in the expected user failure: {:#?}",
            found.len(),
            total,
            &found[..found.len().min(20)]
        );
    }

    /// The class of one error alone.
    fn class_of(error: &InterpreterError) -> EvaluationFailureSummary {
        classify_errors(std::slice::from_ref(error)).expect("an unmetered classification")
    }

    /// A process that holds one term of each of the six process kinds.
    fn every_process_kind() -> Par {
        Par::default()
            .with_sends(vec![Send::default()])
            .with_receives(vec![Receive::default()])
            .with_news(vec![New::default()])
            .with_matches(vec![Match::default()])
            .with_unforgeables(vec![GUnforgeable::default()])
            .with_bundles(vec![Bundle::default()])
    }

    /// A process that receives Nil and then evaluates `failing` with the
    /// received value bound to `v`.
    fn after_receiving_nil(failing: &str) -> String {
        format!("new x in {{ x!(Nil) | for (@v <- x) {{ {failing} }} }}")
    }

    /// Added by DR-128: a division or a modulo by zero is a user failure at
    /// each of its ten sites.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_division_by_zero_is_a_user_failure_at_every_site() {
        let sites = [
            site("Int /", "@\"r\"!(1 / 0)", "Division by zero"),
            site("Int %", "@\"r\"!(1 % 0)", "Modulo by zero"),
            site("BigInt /", "@\"r\"!(1n / 0n)", "Division by zero"),
            site("BigInt %", "@\"r\"!(1n % 0n)", "Modulo by zero"),
            site("BigRat /", "@\"r\"!(1r / 0r)", "Division by zero"),
            site("BigRat %", "@\"r\"!(1r % 0r)", "Modulo by zero"),
            site("FixedPoint /", "@\"r\"!(1.5p1 / 0.0p1)", "Division by zero"),
            site("FixedPoint %", "@\"r\"!(1.5p1 % 0.0p1)", "Modulo by zero"),
            site("sized /", "@\"r\"!(1u8 / 0u8)", "Division by zero"),
            site("sized %", "@\"r\"!(1u16 % 0u16)", "Modulo by zero"),
        ];
        with_runtime("division-by-zero-", |mut runtime| async move {
            let found = mismatches(&mut runtime, InterpreterError::DivisionByZero, &sites).await;
            assert_no_mismatches(&found, sites.len());
        })
        .await;
    }

    /// Added by DR-128: an integer result that does not fit its type is a
    /// user failure at each of its six sites.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_arithmetic_overflow_is_a_user_failure_at_every_site() {
        let sites = [
            site(
                "Int negation",
                "@\"r\"!(-(-9223372036854775807 - 1))",
                "Arithmetic overflow in negation",
            ),
            site(
                "Int32 negation",
                "@\"r\"!(-(-2147483647i32 - 1i32))",
                "Arithmetic overflow in negation",
            ),
            site(
                "Int *",
                "@\"r\"!(9223372036854775807 * 2)",
                "Arithmetic overflow in multiplication: 9223372036854775807 * 2",
            ),
            site(
                "Int /",
                "@\"r\"!((-9223372036854775807 - 1) / -1)",
                "Arithmetic overflow in division",
            ),
            site(
                "Int %",
                "@\"r\"!((-9223372036854775807 - 1) % -1)",
                "Arithmetic overflow in modulo",
            ),
            site(
                "sized *",
                "@\"r\"!(255u8 * 2u8)",
                "Arithmetic overflow in multiplication: 255u8 * 2u8",
            ),
        ];
        with_runtime("arithmetic-overflow-", |mut runtime| async move {
            let found =
                mismatches(&mut runtime, InterpreterError::ArithmeticOverflow, &sites).await;
            assert_no_mismatches(&found, sites.len());
        })
        .await;
    }

    /// Added by DR-128: an integer method argument outside the method's
    /// domain is a user failure at each of its three sites.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_argument_out_of_range_is_a_user_failure_at_every_site() {
        let sites = [
            site(
                "tuple nth",
                "@\"r\"!((4, 5).nth(9))",
                "Error: index out of bound: 9",
            ),
            site(
                "list nth",
                "@\"r\"!([4, 5].nth(2))",
                "Error: index out of bound: 2",
            ),
            site(
                "byte array nth",
                "@\"r\"!(\"ff\".hexToBytes().nth(9))",
                "Error: index out of bound: 9",
            ),
            site(
                "dropHead",
                "@\"r\"!({| [\"a\"] |}.dropHead(-1))",
                "dropHead argument must be non-negative, got: -1",
            ),
        ];
        with_runtime("argument-out-of-range-", |mut runtime| async move {
            let found =
                mismatches(&mut runtime, InterpreterError::ArgumentOutOfRange, &sites).await;
            assert_no_mismatches(&found, sites.len());
        })
        .await;
    }

    /// Added by DR-128: a send or a receive on a bundle without that
    /// permission is a user failure at both sites.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_bundle_permission_error_is_a_user_failure_at_both_sites() {
        let sites = [
            site(
                "send",
                "new x in { @{bundle0{*x}}!(1) }",
                "Trying to send on non-writeable channel.",
            ),
            site(
                "receive",
                "new x in { for (_ <- @{bundle0{*x}}) { Nil } }",
                "Trying to read from non-readable channel.",
            ),
        ];
        with_runtime("bundle-permission-", |mut runtime| async move {
            let found = mismatches(
                &mut runtime,
                InterpreterError::BundlePermissionError,
                &sites,
            )
            .await;
            assert_no_mismatches(&found, sites.len());
        })
        .await;
    }

    /// Added by DR-128: an operand, receiver or argument of a type or shape
    /// that the operation rejects is a user failure at each of its sixteen
    /// sites. Fourteen sites take a source term. The two guards that need
    /// every process kind at once take a hand-built process.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_value_type_error_is_a_user_failure_at_every_site() {
        let compare = format!(
            "Unexpected compare: {:?} vs. {:?}",
            Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("a".to_string())),
            }
        );
        let singular = "Error: Method can only be called on singular expressions.";
        let sites = [
            site("relational operator", "@\"r\"!(1 < \"a\")", &compare),
            site(
                "interpolation value",
                "@\"r\"!(\"${x}\" %% {\"x\": [1]})",
                "Error: interpolation doesn't support \"list\"",
            ),
            site(
                "interpolation key",
                "@\"r\"!(\"${x}\" %% {1: 2})",
                "Error: interpolation Map should only contain String keys",
            ),
            site(
                "hexToBytes receiver",
                &after_receiving_nil("@\"r\"!(v.hexToBytes())"),
                singular,
            ),
            site(
                "bytesToHex receiver",
                &after_receiving_nil("@\"r\"!(v.bytesToHex())"),
                singular,
            ),
            site(
                "toUtf8Bytes receiver",
                &after_receiving_nil("@\"r\"!(v.toUtf8Bytes())"),
                singular,
            ),
            site(
                "operand kind",
                "new y in { @\"r\"!(*y + 1) }",
                "Error: parallel or non expression found where expression expected.",
            ),
            site(
                "operand count",
                &after_receiving_nil("@\"r\"!(v + 1)"),
                "Error: Multiple expressions given.",
            ),
            site(
                "toString receiver kind",
                "@\"r\"!(42.toString())",
                "Error: non unforgeable found where unforgeable expected.",
            ),
            site(
                "toString receiver count",
                &after_receiving_nil("@\"r\"!(v.toString())"),
                "Error: Multiple unforgeables given.",
            ),
            site(
                "nth argument type",
                "@\"r\"!([1].nth(\"a\"))",
                "Error: expression didn't evaluate to integer.",
            ),
            site(
                "nth argument count",
                &after_receiving_nil("@\"r\"!([1].nth(v))"),
                "Error: Integer expected, or unimplemented expression.",
            ),
            site(
                "not operand type",
                "@\"r\"!(not 1)",
                "Error: expression didn't evaluate to boolean.",
            ),
            site(
                "not operand count",
                &after_receiving_nil("@\"r\"!(not v)"),
                "Error: Multiple expressions given.",
            ),
        ];
        let total = sites.len() + 2;
        let mut found = with_runtime("value-type-", |mut runtime| async move {
            mismatches(&mut runtime, InterpreterError::ValueTypeError, &sites).await
        })
        .await;
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let kinds = every_process_kind();
        let integer = reducer
            .eval_to_i64(&kinds, &Env::new())
            .expect_err("an integer argument with every process kind fails");
        let boolean = reducer
            .eval_to_bool(&kinds, &Env::new())
            .expect_err("a boolean operand with every process kind fails");
        let kind_message = "Error: parallel or non expression found where expression expected.";
        for (name, error) in [
            ("integer argument with every kind", integer),
            ("boolean operand with every kind", boolean),
        ] {
            if error != InterpreterError::ValueTypeError(kind_message.to_string())
                || error.to_string() != format!("Reduce error: {kind_message}")
                || class_of(&error) != EvaluationFailureSummary::single(PhloFailure::User)
            {
                found.push(format!(
                    "{name}: gave {:?} with {error:?}",
                    class_of(&error)
                ));
            }
        }
        assert_no_mismatches(&found, total);
    }

    /// Added by DR-128: a process with more than 32767 terms is a user
    /// failure. A hand-built process of 32768 terms reaches the site, as in
    /// `reduce_spec`. A contract that doubles a one-send process fifteen
    /// times builds one from a short source term.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_process_with_too_many_terms_is_a_user_failure() {
        let text = "The number of terms in the Par is 32768, which exceeds the limit of 32767";
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let error = reducer
            .eval(
                Par::default().with_news(vec![New::default(); i16::MAX as usize + 1]),
                &Env::new(),
                Blake2b512Random::create_from_length(128),
            )
            .await
            .expect_err("a process with 32768 terms fails");
        let sites = [site(
            "doubling contract",
            "new double, ret in { \
             contract double(@p, @n, r) = { \
               if (n == 0) { r!(p) } else { double!(p | p, n - 1, *r) } \
             } | \
             double!(@\"t\"!(0), 15, *ret) | \
             for (@big <- ret) { big } }",
            text,
        )];
        let mut found = with_runtime("term-structure-", |mut runtime| async move {
            mismatches(&mut runtime, InterpreterError::TermStructureError, &sites).await
        })
        .await;
        if error != InterpreterError::TermStructureError(text.to_string())
            || error.to_string() != format!("Reduce error: {text}")
            || class_of(&error) != EvaluationFailureSummary::single(PhloFailure::User)
        {
            found.push(format!(
                "hand-built process of 32768 terms: gave {:?} with {error:?}",
                class_of(&error)
            ));
        }
        assert_no_mismatches(&found, 2);
    }

    /// Added by DR-128: two sites reuse existing user variants. A modulo of
    /// two floats is `OperatorNotDefined`, and `nth` on a value that is not a
    /// list, a tuple or a byte array is `MethodNotDefined`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_reused_variants_are_user_failures() {
        let cases = [
            (
                "@\"r\"!(1.0f64 % 0.0f64)",
                InterpreterError::OperatorNotDefined {
                    op: "%".to_string(),
                    other_type: "float".to_string(),
                },
            ),
            ("@\"r\"!(42.nth(0))", InterpreterError::MethodNotDefined {
                method: "nth".to_string(),
                other_type: "int".to_string(),
            }),
        ];
        with_runtime("reused-variants-", |mut runtime| async move {
            for (term, expected) in cases {
                let result = runtime
                    .evaluate_with_term(term)
                    .await
                    .expect("the evaluation completes");
                assert_eq!(result.errors, vec![expected], "{term}");
                assert_eq!(
                    result.economic_failures,
                    EvaluationFailureSummary::single(PhloFailure::User),
                    "{term}"
                );
            }
        })
        .await;
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let float32 = |value: f32| {
            Some(Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GFloat32(value.to_bits())),
            }]))
        };
        let modulo = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EModBody(EMod {
                p1: float32(1.0),
                p2: float32(0.0),
            })),
        }]);
        let error = reducer
            .eval_expr(&modulo, &Env::new())
            .expect_err("a modulo of two float32 values fails");
        assert_eq!(error, InterpreterError::OperatorNotDefined {
            op: "%".to_string(),
            other_type: "float32".to_string(),
        });
        assert_eq!(
            class_of(&error),
            EvaluationFailureSummary::single(PhloFailure::User)
        );
    }

    /// Added by DR-128: a lookup of an unknown URN is an illegal argument of
    /// the registry lookup process. Legacy execution keeps the illegal
    /// argument unclassified. Native execution makes it the user failure
    /// `SystemProcessShapeError` (DR-114, dispatch.rs), which the offered
    /// probe checks.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unknown_urn_lookup_is_an_illegal_argument() {
        with_runtime("unknown-urn-", |mut runtime| async move {
            let result = runtime
                .evaluate_with_term(
                    "new lookup(`rho:internal:registry_lookup`), ret in { \
                     lookup!(\"no-such-urn\", *ret) }",
                )
                .await
                .expect("the evaluation completes");
            assert_eq!(result.errors, vec![InterpreterError::IllegalArgumentError(
                "registry_lookup: unknown URN: no-such-urn".to_string()
            )]);
            assert_eq!(
                result.economic_failures,
                EvaluationFailureSummary::single(PhloFailure::Unclassified)
            );
        })
        .await;
    }

    /// Added by DR-128, negative controls. A sized operand out of the range of
    /// its type needs a hand-built process, so it stays an unclassified
    /// `ReduceError`. A join that signs some clauses but not all stays an
    /// unclassified `ReduceError` too: DR-128 leaves that site for its own
    /// design.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn internal_sites_and_the_partly_signed_join_stay_unclassified() {
        let (_, reducer) =
            create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
                .await;
        let uint8 = |value: u32| {
            Some(Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GUint8(value)),
            }]))
        };
        let sum = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: uint8(256),
                p2: uint8(1),
            })),
        }]);
        let error = reducer
            .eval_expr(&sum, &Env::new())
            .expect_err("an out-of-range uint8 fails");
        assert_eq!(
            error,
            InterpreterError::ReduceError("Value 256 is out of range for u8".to_string())
        );
        assert_eq!(
            class_of(&error),
            EvaluationFailureSummary::single(PhloFailure::Unclassified)
        );
        with_runtime("partly-signed-join-", |mut runtime| async move {
            let result = runtime
                .evaluate_with_term("new x, w in { for( {% y <- x %}[ s ] & @z <- w ){ Nil } }")
                .await
                .expect("the evaluation completes");
            assert_eq!(result.errors, vec![InterpreterError::ReduceError(
                "cost-accounting: a join must sign either every receive clause or none".to_string()
            )]);
            assert_eq!(
                result.economic_failures,
                EvaluationFailureSummary::single(PhloFailure::Unclassified)
            );
        })
        .await;
    }

    /// Draws one value of `strategy`.
    fn draw<S: proptest::strategy::Strategy>(
        runner: &mut proptest::test_runner::TestRunner,
        strategy: S,
    ) -> S::Value {
        use proptest::strategy::ValueTree;
        strategy
            .new_tree(runner)
            .expect("the strategy draws a value")
            .current()
    }

    /// Added by DR-128: a property test with a fixed seed, like DR-125's. For
    /// 32 generated numerators of each numeric kind, a division and a modulo
    /// by zero are user failures. For 32 generated lengths, an index at or
    /// past the end of a list, a tuple or a byte array is a user failure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn generated_zero_divisors_and_out_of_range_indices_are_user_failures() {
        use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

        let mut runner = TestRunner::new_with_rng(
            Config::with_cases(32),
            TestRng::from_seed(RngAlgorithm::ChaCha, &[14u8; 32]),
        );
        let mut divisors = Vec::with_capacity(32 * 18);
        let mut indices = Vec::with_capacity(32 * 3);
        for _ in 0..32 {
            let int = draw(&mut runner, 0_i64..=i64::MAX);
            let big = draw(&mut runner, "[1-9][0-9]{0,29}");
            let (whole, tenth) = draw(&mut runner, (0_u32..100_000, 0_u8..10));
            let byte = draw(&mut runner, 0_u8..=u8::MAX);
            let short = draw(&mut runner, 0_u16..=u16::MAX);
            let word = draw(&mut runner, 0_u32..=u32::MAX);
            let long = draw(&mut runner, 0_u64..=u64::MAX);
            let signed = draw(&mut runner, 0_i32..=i32::MAX);
            for (numerator, zero) in [
                (int.to_string(), "0"),
                (format!("{big}n"), "0n"),
                (format!("{big}r"), "0r"),
                (format!("{whole}.{tenth}p1"), "0.0p1"),
                (format!("{byte}u8"), "0u8"),
                (format!("{short}u16"), "0u16"),
                (format!("{word}u32"), "0u32"),
                (format!("{long}u64"), "0u64"),
                (format!("{signed}i32"), "0i32"),
            ] {
                for (op, text) in [("/", "Division by zero"), ("%", "Modulo by zero")] {
                    divisors.push(site(
                        "generated divisor",
                        &format!("@\"r\"!({numerator} {op} {zero})"),
                        text,
                    ));
                }
            }
        }
        for _ in 0..32 {
            let length = draw(&mut runner, 0_usize..6);
            let index = length + draw(&mut runner, 0_usize..10);
            let elements = (0..length)
                .map(|element| element.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let text = format!("Error: index out of bound: {index}");
            indices.push(site(
                "generated list index",
                &format!("@\"r\"!([{elements}].nth({index}))"),
                &text,
            ));
            if length >= 2 {
                indices.push(site(
                    "generated tuple index",
                    &format!("@\"r\"!(({elements}).nth({index}))"),
                    &text,
                ));
            }
            indices.push(site(
                "generated byte index",
                &format!(
                    "@\"r\"!(\"{}\".hexToBytes().nth({index}))",
                    "ab".repeat(length)
                ),
                &text,
            ));
        }
        with_runtime("generated-user-errors-", |mut runtime| async move {
            let mut found =
                mismatches(&mut runtime, InterpreterError::DivisionByZero, &divisors).await;
            found.extend(
                mismatches(&mut runtime, InterpreterError::ArgumentOutOfRange, &indices).await,
            );
            assert_no_mismatches(&found, divisors.len() + indices.len());
        })
        .await;
    }
}
