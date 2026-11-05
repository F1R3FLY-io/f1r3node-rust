// CoC Phase 1c: end-to-end from Rholang source text through grammar →
// AST → normalizer → β-reduction. Surface syntax:
//   lambda: `([x_0, …, x_{n-1}] =>body)` (outer parens required)
//   app:    `fun[args]` where fun is a lambda or parenthesized expression
//
// `fun[args]` with a bare variable requires wrapping: `(f)[args]`. The outer
// parens on `app`'s fun avoid collision with `name(args)` patterns in the
// existing grammar (contract/for/agent).

use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn reducer() -> Arc<DebruijnInterpreter> {
    let cost = CostAccounting::empty_cost();
    cost.set(Cost::unsafe_max());

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let space = RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
    let rspace: RhoISpace = Arc::new(Box::new(space));

    DebruijnInterpreter::new(
        rspace,
        Arc::new(HashMap::new()),
        Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        Arc::new(HashMap::new()),
        cost,
    )
}

fn compile(src: &str) -> Par {
    Compiler::source_to_adt(src).unwrap_or_else(|e| panic!("compile failed for {src:?}: {e:?}"))
}

fn first_expr(par: &Par) -> &ExprInstance {
    par.exprs
        .first()
        .and_then(|e| e.expr_instance.as_ref())
        .expect("expected at least one expr in par")
}

fn assert_gint(par: &Par, expected: i64) {
    match first_expr(par) {
        ExprInstance::GInt(i) => assert_eq!(*i, expected, "expected GInt({expected})"),
        other => panic!("expected GInt({expected}), got {other:?}"),
    }
}

// Source `((x) => x + 1)(41)` compiles to an EApp of an ELambda, which
// β-reduces to 42.
#[tokio::test]
async fn source_lambda_add_one() {
    let par = compile("((x) => x + 1)(41)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 42);
}

// Multi-arg lambda from source.
#[tokio::test]
async fn source_multi_arg_lambda() {
    let par = compile("((x, y) => x + y)(3, 4)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 7);
}

// Curried lambda from source. Both levels parse as separate lambdas and
// β-reduce left-associatively.
#[tokio::test]
async fn source_curried_lambda() {
    let par = compile("(((x) =>((y) => x + y))(3))(4)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 7);
}

// Identity lambda applied to a nested lambda.
#[tokio::test]
async fn source_lambda_as_argument() {
    let par = compile("((f) =>f(10))(((x) => x + 5))");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 15);
}

// Bare var-fun application: `f(args)` with no whitespace restriction.
#[tokio::test]
async fn source_bare_var_fun_app() {
    let par = compile("((inc) =>inc(7))(((x) => x + 1))");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 8);
}

// Zero-arg lambda (thunk): `(() =>42)()` evaluates to 42.
#[tokio::test]
async fn source_zero_arg_lambda() {
    let par = compile("(() =>42)()");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 42);
}

// Smoke test: these shapes all parse + normalize without panicking.
#[tokio::test]
async fn source_parses_without_panic() {
    let samples = [
        "((x) => x)(1)",
        "((x, y) => x)(1, 2)",
        "((x, y, z) => x + y + z)(1, 2, 3)",
    ];
    for src in &samples {
        let _par = compile(src);
    }
}

// CoC Phase 2b: lambda params may carry type annotations `x: A`.
// Annotations are parsed but currently discarded at normalization
// (the Phase 2c typechecker will wire them through). Reduction
// result must be identical to the untyped form.
#[tokio::test]
async fn source_typed_lambda_add_one() {
    let par = compile("((x: Int) => x + 1)(41)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 42);
}

#[tokio::test]
async fn source_typed_lambda_multi_arg() {
    let par = compile("((x: Int, y: Int) => x + y)(3, 4)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 7);
}

#[tokio::test]
async fn source_mixed_typed_and_untyped_params() {
    // The grammar allows per-param annotation; mixed is legal too.
    let par = compile("((x: Int, y) => x + y)(10, 5)");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 15);
}

// Function-typed parameter: `(f: Int -> Int) => f(7)`.
// Exercises Pi types appearing as param annotations.
#[tokio::test]
async fn source_lambda_with_function_type_param() {
    let par = compile("((f: Int -> Int) => f(7))(((x) => x + 1))");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    assert_gint(&out, 8);
}
