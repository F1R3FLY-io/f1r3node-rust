// CoC Phase 3.3: reducer-level end-to-end tests for type-directed
// match. A full Rholang program compiles, reduces, and the correct
// case body fires as observed via the rspace tuple-space state.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

fn string_channel(name: &str) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }],
        ..Default::default()
    }
}

async fn eval_ok(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime
        .evaluate_with_term(term)
        .await
        .unwrap_or_else(|e| panic!("evaluate failed for {term:?}: {e:?}"));
    assert!(
        res.errors.is_empty(),
        "Expected success for: {term}\nErrors: {:?}",
        res.errors,
    );
}

async fn data_at(runtime: &RhoRuntimeImpl, name: &str) -> Vec<Par> {
    runtime
        .get_data(&string_channel(name))
        .await
        .into_iter()
        .flat_map(|d| d.a.pars)
        .collect()
}

// Matching a typed lambda against a matching Pi pattern fires the
// first case; the mismatch arm is never taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn match_pi_pattern_fires_matching_case() {
    with_runtime("coc-type-match-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "match (x: Int) => x { Int -> Int => @\"ok\"!(\"yes\") _ => @\"ok\"!(\"no\") }",
        )
        .await;
        let data = data_at(&runtime, "ok").await;
        assert_eq!(data.len(), 1, "exactly one case should fire");
        assert_eq!(
            data[0].exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GString("yes".to_string())),
            "the Pi-matching arm should have fired, not the wildcard"
        );
    })
    .await
}

// A non-matching Pi (different codomain) falls through to the wildcard.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn match_pi_pattern_mismatch_falls_through() {
    with_runtime("coc-type-match-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "match (x: Int) => x { Int -> Bool => @\"m\"!(\"yes\") _ => @\"m\"!(\"no\") }",
        )
        .await;
        let data = data_at(&runtime, "m").await;
        assert_eq!(data.len(), 1);
        assert_eq!(
            data[0].exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GString("no".to_string())),
            "lambda of type Int -> Int must not match Int -> Bool"
        );
    })
    .await
}

// A non-function value never matches a Pi pattern.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn match_pi_pattern_rejects_ground_value() {
    with_runtime("coc-type-match-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "match 42 { Int -> Int => @\"g\"!(\"func\") _ => @\"g\"!(\"ground\") }",
        )
        .await;
        let data = data_at(&runtime, "g").await;
        assert_eq!(data.len(), 1);
        assert_eq!(
            data[0].exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GString("ground".to_string())),
            "42 is not a function; Pi pattern must reject it"
        );
    })
    .await
}

// No matching arm at all: match reduces to a no-op (no side effect).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn match_pi_pattern_with_no_match_reduces_to_noop() {
    with_runtime("coc-type-match-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "match 42 { Int -> Int => @\"n\"!(\"hit\") }",
        )
        .await;
        assert!(
            data_at(&runtime, "n").await.is_empty(),
            "a match with no matching case is a no-op"
        );
    })
    .await
}
