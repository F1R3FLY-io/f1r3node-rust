use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoList, RhoNumber, RhoString};
use rholang::rust::interpreter::swi_prolog_service::petta_execute_framed;
use rholang::rust::interpreter::test_utils::resources::create_runtimes_with_services;
use rholang::rust::interpreter::test_utils::utils::should_skip_petta_test;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

fn output_channel() -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("output".to_string())),
        }],
        ..Default::default()
    }
}

async fn read_channel_data(runtime: &RhoRuntimeImpl, ch: Par) -> HashSet<Par> {
    runtime
        .get_hot_changes()
        .await
        .get(&vec![ch])
        .map(|row| row.data.iter().flat_map(|d| d.a.pars.clone()).collect())
        .unwrap_or_default()
}

async fn evaluate_petta_term(term: &str) -> (EvaluateResult, RhoRuntimeImpl) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();

    #[allow(clippy::type_complexity)]
    let (runtime, _, _): (
        RhoRuntimeImpl,
        RhoRuntimeImpl,
        Arc<
            Box<
                dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    ) = create_runtimes_with_services(store, false, &mut Vec::new(), ExternalServices::noop())
        .await;

    let rand = Blake2b512Random::create_from_bytes(&[]);
    let initial_phlo = Cost::create(i64::MAX, "test".to_string());

    let res = runtime
        .evaluate(term, initial_phlo, HashMap::new(), rand)
        .await
        .expect("Evaluation failed");

    (res, runtime)
}

#[tokio::test]
async fn test_petta_rholang_integration_swap() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, retCh in {
            executePetta!("(= (swap (Pair $x $y)) (Pair $y $x)) !(swap (Pair 1 3))", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "PeTTa swap should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![RhoList::create_par(vec![
        RhoString::create_par("Pair".into()),
        RhoNumber::create_par(3),
        RhoNumber::create_par(1),
    ])]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:#?} on channel 'output', got: {data:#?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_integration_fibonacci() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, retCh in {
            executePetta!("(= (fib-tr $n $a $b) (if (== $n 0) $a (fib-tr (- $n 1) $b (+ $a $b)))) (= (fib $n) (fib-tr $n 0 1)) !(fib 10)", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "PeTTa fibonacci should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![RhoNumber::create_par(55)]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:#?} on channel 'output', got: {data:#?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_integration_arithmetic() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, retCh in {
            executePetta!("!(+ 1 2)", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "PeTTa arithmetic should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![RhoNumber::create_par(3)]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:?} on channel 'output', got: {data:?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_multiple_calls() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, ret1, ret2 in {
            executePetta!("!(+ 1 2)", *ret1) |
            executePetta!("!(* 3 4)", *ret2) |
            for(@r1 <- ret1; @r2 <- ret2) {
                @"output"!([r1, r2])
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "Multiple PeTTa calls should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![
        RhoList::create_par(vec![RhoNumber::create_par(3)]),
        RhoList::create_par(vec![RhoNumber::create_par(12)]),
    ]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:?} on channel 'output', got: {data:?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_integration_println() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, retCh in {
            executePetta!("!(println! hello)", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "PeTTa println! should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![RhoBoolean::create_par(true)]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:?} on channel 'output', got: {data:?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_println_frames() {
    if should_skip_petta_test() {
        return;
    }

    let (frames, result_par) = petta_execute_framed("!(println! hello)").await.unwrap();

    assert_eq!(frames.len(), 1, "println! should emit exactly one frame");
    assert_eq!(
        frames[0].channel, "rho:io:stdout",
        "println! frame channel should be rho:io:stdout"
    );
    assert_eq!(
        frames[0].arguments,
        vec![RhoString::create_par("hello".into())],
        "println! frame argument should be \"hello\""
    );

    let expected_result = RhoList::create_par(vec![RhoBoolean::create_par(true)]);
    assert_eq!(
        result_par, expected_result,
        "println! return value should be [true]"
    );
}

#[tokio::test]
async fn test_petta_rholang_integration_trace() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), output, retCh in {
            executePetta!("!(trace! hello goodbye)", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(
        result.errors.is_empty(),
        "PeTTa trace! should execute without errors: {:?}",
        result.errors
    );

    let expected = RhoList::create_par(vec![RhoString::create_par("goodbye".into())]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:?} on channel 'output', got: {data:?}"
    );
}

#[tokio::test]
async fn test_petta_rholang_trace_frames() {
    if should_skip_petta_test() {
        return;
    }

    let (frames, result_par) = petta_execute_framed("!(trace! hello goodbye)")
        .await
        .unwrap();

    assert_eq!(
        frames.len(),
        1,
        "trace! should emit exactly one frame (from the println! half)"
    );
    assert_eq!(
        frames[0].channel, "rho:io:stdout",
        "trace! frame channel should be rho:io:stdout"
    );
    assert_eq!(
        frames[0].arguments,
        vec![RhoString::create_par("hello".into())],
        "trace! frame argument should be the first argument (\"hello\")"
    );

    let expected_result = RhoList::create_par(vec![RhoString::create_par("goodbye".into())]);
    assert_eq!(
        result_par, expected_result,
        "trace! return value should be the second argument [\"goodbye\"]"
    );
}

#[tokio::test]
async fn test_petta_rholang_error_handling() {
    if should_skip_petta_test() {
        return;
    }

    // Test with invalid MeTTa syntax
    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("(= incomplete", *retCh)
        }
    "#;

    let (result, _runtime) = evaluate_petta_term(term).await;

    // Should produce an error due to invalid syntax
    assert!(
        !result.errors.is_empty(),
        "Invalid MeTTa syntax should produce errors"
    );
}

/// The deterministic inference budget (not wall-clock) bounds a runaway program
/// through the full Rholang runtime. A naive exponential `nfib` far exceeds
/// `PETTA_MAX_INFERENCES`, so the handler fails with the deterministic
/// `inference_limit_exceeded` error — identical on every node — rather than a
/// machine-dependent timeout.
#[tokio::test]
async fn test_petta_rholang_inference_budget_enforced() {
    if should_skip_petta_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("(= (nfib $n) (if (< $n 2) $n (+ (nfib (- $n 1)) (nfib (- $n 2))))) !(nfib 1000)", *retCh)
        }
    "#;

    let (result, _runtime) = evaluate_petta_term(term).await;

    assert!(
        !result.errors.is_empty(),
        "A runaway computation must produce errors: {:?}",
        result.errors
    );

    let has_budget_error = result
        .errors
        .iter()
        .any(|err| format!("{:?}", err).contains("inference_limit_exceeded"));
    assert!(
        has_budget_error,
        "Expected a deterministic inference_limit_exceeded error, got: {:?}",
        result.errors
    );
}

/// The entropy/clock builtins are blocked on the consensus handler path, so a
/// program that calls one yields the SAME output on every run instead of a
/// fresh value per run. Two independent executions of `random-int` over a large
/// range would differ (p ≈ 1e-9 of coinciding) if the builtin were live; equal
/// output demonstrates it is neutralized and the deploy is replay-safe.
#[tokio::test]
async fn test_petta_blocked_entropy_builtins_are_deterministic() {
    if should_skip_petta_test() {
        return;
    }

    for program in ["!(random-int 0 1000000000)", "!(current-time)"] {
        let first = format!("{:?}", petta_execute_framed(program).await);
        let second = format!("{:?}", petta_execute_framed(program).await);
        assert_eq!(
            first, second,
            "blocked builtin must be deterministic across runs for {program}"
        );
    }
}
