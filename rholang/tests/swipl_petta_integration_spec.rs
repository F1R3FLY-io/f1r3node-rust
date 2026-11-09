use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoList, RhoNumber, RhoString};
use rholang::rust::interpreter::swi_prolog_service::{
    petta_execute_framed, PETTA_BASE_COST, PETTA_MAX_INFERENCES, PETTA_PHLO_PER_INFERENCE,
};
use rholang::rust::interpreter::test_utils::resources::create_runtimes_with_services;
use rholang::rust::interpreter::test_utils::utils::{
    should_skip_ecan_test, should_skip_petta_test,
};
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
    evaluate_petta_term_with_phlo(term, i64::MAX).await
}

async fn evaluate_petta_term_with_phlo(term: &str, phlo: i64) -> (EvaluateResult, RhoRuntimeImpl) {
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
    let initial_phlo = Cost::create(phlo, "test".to_string());

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

    let (frames, result_par) = petta_execute_framed("!(println! hello)").await.0.unwrap();

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
        .0
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

/// Under consensus lockdown no escape hatch reaches a clock or an entropy source.
/// Each case must give the same outcome on two runs. A clock or random value would
/// differ between the runs.
#[tokio::test]
async fn test_petta_consensus_escape_cases_yield_no_clock_or_random() {
    if should_skip_petta_test() {
        return;
    }

    for program in [
        "!(translatePredicate (get_time $t))",
        "!(call (get_time))",
        "(= (get_time $x) 0) !(get_time)",
        "!(import_prolog_function get_time) !(get_time)",
        "!(maplist get_time (1))",
        "!(foldl random_between (1) 100)",
        "!(assert (get_time $t))",
        "!(eval (translatePredicate (get_time $t)))",
        "!(random-int 0 100)",
        "!(current-time)",
    ] {
        let first = format!("{:?}", petta_execute_framed(program).await);
        let second = format!("{:?}", petta_execute_framed(program).await);
        assert_eq!(
            first, second,
            "{program} must give the same outcome on every run"
        );
    }
}

/// The PLN library imports and runs through `rho:petta:execute` under lockdown.
#[tokio::test]
async fn test_petta_pln_import_under_lockdown() {
    if should_skip_ecan_test() {
        return;
    }

    let term = r#"
        new executePetta(`rho:petta:execute`), retCh in {
            executePetta!("!(import! &self (library PLN lib_pln)) !(let (stv $s $c) (Truth_Deduction (stv 0.5 0.9) (stv 0.5 0.9) (stv 0.5 0.9) (stv 0.5 0.9) (stv 0.5 0.9)) (> $c 0))", *retCh) |
            for(@result <- retCh) {
                @"output"!(result)
            }
        }
    "#;

    let (result, runtime) = evaluate_petta_term(term).await;

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let expected = RhoList::create_par(vec![
        RhoBoolean::create_par(true),
        RhoBoolean::create_par(true),
    ]);
    let data = read_channel_data(&runtime, output_channel()).await;
    assert!(
        data.contains(&expected),
        "Expected {expected:?}, got: {data:?}"
    );
}

fn petta_term(metta: &str) -> String {
    format!(
        r#"new executePetta(`rho:petta:execute`), retCh in {{ executePetta!("{}", *retCh) }}"#,
        metta
    )
}

/// The phlo cost of a call grows with the inference count of the program.
#[tokio::test]
async fn test_petta_cost_grows_with_inferences() {
    if should_skip_petta_test() {
        return;
    }

    let nfib = "(= (nfib $n) (if (< $n 2) $n (+ (nfib (- $n 1)) (nfib (- $n 2)))))";
    let (small, _) = evaluate_petta_term(&petta_term(&format!("{nfib} !(nfib 5)"))).await;
    let (large, _) = evaluate_petta_term(&petta_term(&format!("{nfib} !(nfib 15)"))).await;

    assert!(small.errors.is_empty(), "{:?}", small.errors);
    assert!(large.errors.is_empty(), "{:?}", large.errors);
    assert!(
        large.cost.value > small.cost.value + 10_000,
        "nfib 15 must cost more phlo than nfib 5: {} vs {}",
        large.cost.value,
        small.cost.value
    );
}

/// A deploy that cannot pay the base cost fails with OutOfPhlogistonsError before the
/// interpreter starts: the cost log holds the base charge and no inference charge.
#[tokio::test]
async fn test_petta_underfunded_deploy_does_not_spawn() {
    if should_skip_petta_test() {
        return;
    }

    let (result, runtime) = evaluate_petta_term_with_phlo(&petta_term("!(+ 1 2)"), 50_000).await;

    assert!(
        result
            .errors
            .iter()
            .any(|e| matches!(e, InterpreterError::OutOfPhlogistonsError)),
        "Expected OutOfPhlogistonsError, got: {:?}",
        result.errors
    );
    let ops: Vec<String> = runtime
        .get_cost_log()
        .iter()
        .map(|c| c.operation.to_string())
        .collect();
    assert!(ops.iter().any(|op| op == "petta base cost"), "{ops:?}");
    assert!(!ops.iter().any(|op| op == "petta inferences"), "{ops:?}");
}

/// A run whose frame gives no inference count pays the full budget. The memory-limit
/// frame that `petta.sh` emits after an abort has no count.
#[tokio::test]
async fn test_petta_run_without_inference_count_pays_full_budget() {
    if should_skip_petta_test() {
        return;
    }

    let (result, _) = evaluate_petta_term(&petta_term(
        "(= (grow $a $n) (if (== $n 0) $a (grow (atom_concat $a $a) (- $n 1)))) !(size-atom (grow a 33))",
    )).await;

    assert!(
        format!("{:?}", result.errors).contains("memory_limit_exceeded"),
        "Expected memory_limit_exceeded, got: {:?}",
        result.errors
    );
    assert!(
        result.cost.value
            >= PETTA_BASE_COST + PETTA_MAX_INFERENCES as i64 * PETTA_PHLO_PER_INFERENCE,
        "A run without an inference count must pay the full budget, cost: {}",
        result.cost.value
    );
}
