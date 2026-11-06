// CoC Phase 1a: β-WHNF reduction over directly-constructed ELambda / EApp
// Expr trees. These tests exercise reduce.rs without the parser, so they
// can land before the parser PR that adds surface syntax for lambdas.
//
// Binding convention under test: for a lambda of `bind_count` N, the surface
// order `(x_0, x_1, ..., x_{N-1}) => body` binds x_i to BoundVar(i) in the
// body. EApp pushes args into env in reverse so args[0] = BoundVar(0).

use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EApp, ELambda, Expr, Par};
use models::rust::utils::{new_boundvar_par, new_eplus_par, new_gint_par};
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
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

fn lambda_par(bind_count: i32, body: Par) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
            bind_count,
            body: Some(body),
            locally_free: Vec::new(),
            connective_used: false,
            param_types: Vec::new(),
        })),
    }])
}

fn app_par(fun: Par, args: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EAppBody(EApp {
            fun: Some(fun),
            args,
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
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

// ((x) => x)(42) → 42
#[tokio::test]
async fn identity_lambda_applied() {
    let r = reducer().await;
    let id = lambda_par(1, new_boundvar_par(0, Vec::new(), false));
    let term = app_par(id, vec![new_gint_par(42, Vec::new(), false)]);

    let out = r.eval_expr(&term, &Env::new()).unwrap();
    assert_gint(&out, 42);
}

// ((x, y) => x)(1, 2) → 1
//   In our convention args[0] = BoundVar(0) = x, so pick x returns 1.
#[tokio::test]
async fn const_first_of_two() {
    let r = reducer().await;
    let const_fst = lambda_par(2, new_boundvar_par(0, Vec::new(), false));
    let term = app_par(const_fst, vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
    ]);

    let out = r.eval_expr(&term, &Env::new()).unwrap();
    assert_gint(&out, 1);
}

// ((x, y) => y)(1, 2) → 2
#[tokio::test]
async fn const_second_of_two() {
    let r = reducer().await;
    let snd = lambda_par(2, new_boundvar_par(1, Vec::new(), false));
    let term = app_par(snd, vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
    ]);

    let out = r.eval_expr(&term, &Env::new()).unwrap();
    assert_gint(&out, 2);
}

// ((x) => x + 1)(41) → 42
#[tokio::test]
async fn add_one_body() {
    let r = reducer().await;
    let body = new_eplus_par(
        new_boundvar_par(0, Vec::new(), false),
        new_gint_par(1, Vec::new(), false),
    );
    let add_one = lambda_par(1, body);
    let term = app_par(add_one, vec![new_gint_par(41, Vec::new(), false)]);

    let out = r.eval_expr(&term, &Env::new()).unwrap();
    assert_gint(&out, 42);
}

// (((x) => (y) => x + y)(3))(4) → 7
//   Nested curry: outer lambda returns an inner lambda; inner refers
//   to the outer's binder via BoundVar(1). After the first application
//   substitutes 3 for BoundVar(0) inside the inner lambda body, the inner
//   body becomes `3 + BoundVar(0)`. Applying 4 yields 7.
#[tokio::test]
async fn curried_add() {
    let r = reducer().await;
    // body of inner lambda: BoundVar(0) (= y) + BoundVar(1) (= x)
    let inner_body = new_eplus_par(
        new_boundvar_par(1, Vec::new(), false),
        new_boundvar_par(0, Vec::new(), false),
    );
    let inner = lambda_par(1, inner_body);
    let outer = lambda_par(1, inner);

    let first_app = app_par(outer, vec![new_gint_par(3, Vec::new(), false)]);
    let second_app = app_par(first_app, vec![new_gint_par(4, Vec::new(), false)]);

    let out = r.eval_expr(&second_app, &Env::new()).unwrap();
    assert_gint(&out, 7);
}

// ((f) => f(10))((x) => x + 5) → 15
//   Function argument. The outer lambda is a "function application combinator".
#[tokio::test]
async fn higher_order_apply() {
    let r = reducer().await;
    let caller_body = app_par(new_boundvar_par(0, Vec::new(), false), vec![new_gint_par(
        10,
        Vec::new(),
        false,
    )]);
    let caller = lambda_par(1, caller_body);

    let inc5_body = new_eplus_par(
        new_boundvar_par(0, Vec::new(), false),
        new_gint_par(5, Vec::new(), false),
    );
    let inc5 = lambda_par(1, inc5_body);

    let term = app_par(caller, vec![inc5]);
    let out = r.eval_expr(&term, &Env::new()).unwrap();
    assert_gint(&out, 15);
}

// Arity mismatch: ((x) => x)(1, 2) → ReduceError.
#[tokio::test]
async fn arity_mismatch_errors() {
    let r = reducer().await;
    let id = lambda_par(1, new_boundvar_par(0, Vec::new(), false));
    let term = app_par(id, vec![
        new_gint_par(1, Vec::new(), false),
        new_gint_par(2, Vec::new(), false),
    ]);

    let err = r.eval_expr(&term, &Env::new()).unwrap_err();
    match err {
        InterpreterError::ReduceError(msg) => {
            assert!(
                msg.contains("arity mismatch"),
                "expected arity mismatch error, got: {msg}"
            );
        }
        other => panic!("expected ReduceError, got {other:?}"),
    }
}

// Non-lambda head: (42)(1) → ReduceError.
//   An application whose head reduces to a non-lambda expr must error.
#[tokio::test]
async fn non_lambda_head_errors() {
    let r = reducer().await;
    let term = app_par(new_gint_par(42, Vec::new(), false), vec![new_gint_par(
        1,
        Vec::new(),
        false,
    )]);

    let err = r.eval_expr(&term, &Env::new()).unwrap_err();
    match err {
        InterpreterError::ReduceError(msg) => {
            assert!(
                msg.contains("not a lambda"),
                "expected non-lambda error, got: {msg}"
            );
        }
        other => panic!("expected ReduceError, got {other:?}"),
    }
}

// WHNF: a bare lambda (no app) is a value and evaluates to itself.
//   `(x) => x + 1` should NOT reduce the body.
#[tokio::test]
async fn bare_lambda_is_whnf_value() {
    let r = reducer().await;
    let body = new_eplus_par(
        new_boundvar_par(0, Vec::new(), false),
        new_gint_par(1, Vec::new(), false),
    );
    let lam = lambda_par(1, body.clone());

    let out = r.eval_expr(&lam, &Env::new()).unwrap();
    // Result must still be an ELambda whose body is the unreduced EPlus.
    match first_expr(&out) {
        ExprInstance::ELambdaBody(l) => {
            assert_eq!(l.bind_count, 1);
            let b = l.body.as_ref().expect("lambda body");
            match b.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                Some(ExprInstance::EPlusBody(_)) => {}
                other => panic!("expected body to still be EPlus (unreduced), got {other:?}"),
            }
        }
        other => panic!("expected ELambdaBody, got {other:?}"),
    }
}

// Env-aware substitution: (x in outer env = 10); ((y) => x + y)(5) → 15.
//   The outer binder reference inside the lambda body must resolve to the
//   outer env's binding after β-reduction. Exercises the env.shift logic
//   in substitute: inside the lambda body `x` is BoundVar(1) (because the
//   lambda's own binder sits at BoundVar(0)); after beta the body sees
//   BoundVar(0) = 5 (local) and BoundVar(1) = 10 (outer-via-shift).
#[tokio::test]
async fn outer_env_binding_resolves_through_lambda() {
    let r = reducer().await;
    // body: BoundVar(1) + BoundVar(0)  — outer x + inner y
    let body = new_eplus_par(
        new_boundvar_par(1, Vec::new(), false),
        new_boundvar_par(0, Vec::new(), false),
    );
    let lam = lambda_par(1, body);
    let term = app_par(lam, vec![new_gint_par(5, Vec::new(), false)]);

    // Outer env binds BoundVar(0) → 10.
    let env = Env::new().put(new_gint_par(10, Vec::new(), false));
    let out = r.eval_expr(&term, &env).unwrap();
    assert_gint(&out, 15);
}
