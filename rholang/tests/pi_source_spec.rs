// CoC Phase 2a: Pi types parsed from source text, through grammar →
// AST → normalizer → reducer. Pi is a type-level value in Phase 2a:
// it flows through the pipeline and is a WHNF value (reduces to itself).
// The typechecker arrives later; for now these tests pin the parse
// shape and reduction semantics.
//
// Surface forms:
//   `(x: A, y: B) -> C`  -- dependent Pi
//   `A -> B`              -- non-dependent sugar (anonymous binder `_`)

use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPi, Par};
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

fn expect_pi(par: &Par) -> &EPi {
    match first_expr(par) {
        ExprInstance::EPiBody(pi) => pi,
        other => panic!("expected EPiBody, got {other:?}"),
    }
}

// Dependent Pi with one binder: `(x: Int) -> Int`. bind_count=1, one
// param type, codomain = Int (an Int SimpleType constant).
#[tokio::test]
async fn source_pi_single_binder() {
    let par = compile("(x: Int) -> Int");
    let pi = expect_pi(&par);
    assert_eq!(pi.bind_count, 1);
    assert_eq!(pi.param_types.len(), 1);
}

// Multi-binder dependent Pi.
#[tokio::test]
async fn source_pi_multi_binder() {
    let par = compile("(x: Int, y: Int) -> Bool");
    let pi = expect_pi(&par);
    assert_eq!(pi.bind_count, 2);
    assert_eq!(pi.param_types.len(), 2);
}

// Non-dependent sugar: `Int -> Bool`. Stored as a Pi with a single
// anonymous `_` binder; from the type's perspective the binder is
// unused in the codomain.
#[tokio::test]
async fn source_pi_non_dependent_sugar() {
    let par = compile("Int -> Bool");
    let pi = expect_pi(&par);
    assert_eq!(pi.bind_count, 1, "sugar introduces one anonymous binder");
    assert_eq!(pi.param_types.len(), 1);
}

// Pi is a WHNF value: reducing it preserves the structure.
#[tokio::test]
async fn source_pi_is_whnf_value() {
    let par = compile("(x: Int) -> Int");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    // Result must still be a Pi.
    expect_pi(&out);
}

// Pi codomain can reference its binder — exercises the normalizer
// extending bound_map_chain with the Pi's param name. Compilation
// succeeding (no "free variables not allowed" error) is the pass
// condition; the parse shape is checked structurally.
#[tokio::test]
async fn source_pi_binder_resolves_in_codomain() {
    // `x + 1` is a nonsense codomain type-wise, but Phase 2a doesn't
    // typecheck, so any proc flows as the codomain.
    let par = compile("(x: Int) -> x + 1");
    let pi = expect_pi(&par);
    assert_eq!(pi.bind_count, 1);
    // Codomain body must reference the binder as BoundVar(0), not a
    // free var — if it leaked as free, the Compiler would have
    // rejected the source.
    let _body = pi.codomain.as_ref().expect("codomain");
}

// Smoke test: these shapes all compile without panic.
#[tokio::test]
async fn source_pi_shapes() {
    let samples = [
        "(x: Int) -> Int",
        "(x: Int, y: Int) -> Int",
        "Int -> Int",
        "Int -> Bool",
        "(f: Int -> Int) -> Int",
        "((x: Int) -> Int) -> Bool",
    ];
    for src in &samples {
        let _par = compile(src);
    }
}
