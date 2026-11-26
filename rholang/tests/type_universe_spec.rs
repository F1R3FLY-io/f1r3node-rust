// CoC Phase 2c (prelude): the `Type` universe keyword is parseable
// and reduces to itself (same as other SimpleTypes). It maps to a
// `ConnType(true)` connective, mirroring Int/Bool/String/Uri/ByteArray.
// The actual typechecker and refined match semantics arrive in Phase 2d.

use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::connective::ConnectiveInstance;
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

fn first_connective(par: &Par) -> Option<&ConnectiveInstance> {
    par.connectives.first()?.connective_instance.as_ref()
}

// `Type` alone parses to a Par with a single ConnType connective.
#[tokio::test]
async fn source_type_universe_parses() {
    let par = compile("Type");
    match first_connective(&par) {
        Some(ConnectiveInstance::ConnType(_)) => {}
        other => panic!("expected ConnType, got {other:?}"),
    }
}

// `Type` is a value under reduction — it stays itself.
#[tokio::test]
async fn source_type_universe_is_whnf_value() {
    let par = compile("Type");
    let r = reducer().await;
    let out = r.eval_expr(&par, &Env::new()).unwrap();
    match first_connective(&out) {
        Some(ConnectiveInstance::ConnType(_)) => {}
        other => panic!("expected ConnType after reduction, got {other:?}"),
    }
}

// Pi codomain can be `Type`: `Int -> Type` is the type of type-families
// over Int (a kind-level function). Phase 2c just checks it parses.
#[tokio::test]
async fn source_type_as_pi_codomain() {
    let par = compile("Int -> Type");
    // The whole thing is an EPi; its codomain Par has a ConnType.
    use models::rhoapi::expr::ExprInstance;
    let expr = par
        .exprs
        .first()
        .and_then(|e| e.expr_instance.as_ref())
        .expect("expected expr");
    match expr {
        ExprInstance::EPiBody(pi) => {
            let codomain = pi.codomain.as_ref().expect("codomain");
            match first_connective(codomain) {
                Some(ConnectiveInstance::ConnType(_)) => {}
                other => panic!("expected ConnType codomain, got {other:?}"),
            }
        }
        other => panic!("expected EPi, got {other:?}"),
    }
}

// `Type` as a lambda parameter type annotation: `(T: Type) => T` —
// a polymorphic identity function (not yet typechecked).
#[tokio::test]
async fn source_type_as_param_annotation() {
    // Parses + normalizes. Phase 2c discards the type annotation.
    let _par = compile("((T: Type) => T)(Int)");
}

// Smoke test: combinations of Type with Pi and lambda parse without panic.
#[tokio::test]
async fn source_type_shapes() {
    let samples = [
        "Type",
        "Int -> Type",
        "Type -> Type",
        "(T: Type) -> T",
        "((T: Type) => T)",
    ];
    for src in &samples {
        let _par = compile(src);
    }
}
