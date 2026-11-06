// CoC Phase 1b: end-to-end integration from AST (built via ast_builder,
// no grammar yet) → normalizer → β-reduction. Validates the entire
// pipeline downstream of parsing.
//
// Grammar support for surface `(x) => body` / `f(x)` lands in Phase 1c.

use std::collections::HashMap;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EApp, ELambda, Par};
use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use rholang::rust::interpreter::compiler::free_map::FreeMap;
use rholang::rust::interpreter::compiler::normalize::{normalize_ann_proc, ProcVisitInputs};
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
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

fn empty_inputs() -> ProcVisitInputs {
    ProcVisitInputs {
        par: Par::default(),
        bound_map_chain: BoundMapChain::new(),
        free_map: FreeMap::new(),
    }
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

// Normalizing `(x) => x` produces an ELambda with bind_count=1 whose body
// references BoundVar(0). The lambda is a WHNF value; eval returns it
// unchanged.
#[tokio::test]
async fn normalize_identity_lambda_structure() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let lam = ParBuilderUtil::create_ast_lambda(&["x"], x_ref, &parser);

    let result = normalize_ann_proc(&lam, empty_inputs(), &HashMap::new(), &parser).unwrap();

    match first_expr(&result.par) {
        ExprInstance::ELambdaBody(ELambda {
            bind_count, body, ..
        }) => {
            assert_eq!(*bind_count, 1, "identity lambda has one binder");
            let body_par = body.as_ref().expect("lambda body");
            // Body must reference its binder as BoundVar(0), not a free var.
            match body_par
                .exprs
                .first()
                .and_then(|e| e.expr_instance.as_ref())
            {
                Some(ExprInstance::EVarBody(evar)) => {
                    let v = evar.v.as_ref().unwrap();
                    match &v.var_instance {
                        Some(models::rhoapi::var::VarInstance::BoundVar(i)) => {
                            assert_eq!(*i, 0, "identity binder resolved to BoundVar(0)");
                        }
                        other => panic!("expected BoundVar(0), got {other:?}"),
                    }
                }
                other => panic!("expected EVar body, got {other:?}"),
            }
        }
        other => panic!("expected ELambdaBody, got {other:?}"),
    }
}

// `((x) => x + 1)(41)` normalizes to EApp(ELambda, [41]); reducer β-reduces
// it to 42.
#[tokio::test]
async fn normalize_then_reduce_add_one() {
    let parser = rholang_parser::RholangParser::new();

    // body: x + 1
    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let one = ParBuilderUtil::create_ast_int(1, &parser);
    let body = ParBuilderUtil::create_ast_add(x_ref, one, &parser);

    let lam = ParBuilderUtil::create_ast_lambda(&["x"], body, &parser);
    let arg = ParBuilderUtil::create_ast_int(41, &parser);
    let app = ParBuilderUtil::create_ast_app(lam, vec![arg], &parser);

    let normalized = normalize_ann_proc(&app, empty_inputs(), &HashMap::new(), &parser).unwrap();

    // Structure check: top-level is an EApp whose fun is an ELambda.
    match first_expr(&normalized.par) {
        ExprInstance::EAppBody(EApp { fun, args, .. }) => {
            assert_eq!(args.len(), 1);
            let fun_par = fun.as_ref().unwrap();
            match fun_par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                Some(ExprInstance::ELambdaBody(_)) => {}
                other => panic!("expected ELambda fun, got {other:?}"),
            }
        }
        other => panic!("expected EAppBody, got {other:?}"),
    }

    // Reduction check.
    let r = reducer().await;
    let reduced = r.eval_expr(&normalized.par, &Env::new()).unwrap();
    assert_gint(&reduced, 42);
}

// Disambiguating multi-arg convention test: `((x, y) => x)(1, 2)` returns
// the FIRST arg. The `multi_arg_lambda_add` test below uses `+` which is
// commutative, so it can't tell which arg maps to which binder. This one
// can: it fires if (and only if) surface order `(x_0, …, x_{N-1})` binds
// `x_i` to BoundVar(i) in the body.
#[tokio::test]
async fn multi_arg_lambda_returns_first_arg() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let lam = ParBuilderUtil::create_ast_lambda(&["x", "y"], x_ref, &parser);
    let a1 = ParBuilderUtil::create_ast_int(1, &parser);
    let a2 = ParBuilderUtil::create_ast_int(2, &parser);
    let app = ParBuilderUtil::create_ast_app(lam, vec![a1, a2], &parser);

    let normalized = normalize_ann_proc(&app, empty_inputs(), &HashMap::new(), &parser).unwrap();

    let r = reducer().await;
    let reduced = r.eval_expr(&normalized.par, &Env::new()).unwrap();
    assert_gint(&reduced, 1);
}

// Multi-arg lambda `((x, y) => x + y)(3, 4)` reduces to 7.
// Convention: surface order `(x, y)` makes x = BoundVar(0), y = BoundVar(1).
#[tokio::test]
async fn multi_arg_lambda_add() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let y_ref = ParBuilderUtil::create_ast_proc_var("y", &parser);
    let body = ParBuilderUtil::create_ast_add(x_ref, y_ref, &parser);

    let lam = ParBuilderUtil::create_ast_lambda(&["x", "y"], body, &parser);
    let a3 = ParBuilderUtil::create_ast_int(3, &parser);
    let a4 = ParBuilderUtil::create_ast_int(4, &parser);
    let app = ParBuilderUtil::create_ast_app(lam, vec![a3, a4], &parser);

    let normalized = normalize_ann_proc(&app, empty_inputs(), &HashMap::new(), &parser).unwrap();

    let r = reducer().await;
    let reduced = r.eval_expr(&normalized.par, &Env::new()).unwrap();
    assert_gint(&reduced, 7);
}

// Nested lambda: `(((x) => (y) => x + y)(3))(4)` reduces to 7. This proves
// the normalizer tracks nested scopes correctly and De Bruijn indexing in
// the inner body resolves `x` as BoundVar(1) (outer binder) and `y` as
// BoundVar(0) (inner binder).
#[tokio::test]
async fn curried_add_normalized() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let y_ref = ParBuilderUtil::create_ast_proc_var("y", &parser);
    let add_body = ParBuilderUtil::create_ast_add(x_ref, y_ref, &parser);

    let inner_lam = ParBuilderUtil::create_ast_lambda(&["y"], add_body, &parser);
    let outer_lam = ParBuilderUtil::create_ast_lambda(&["x"], inner_lam, &parser);

    let three = ParBuilderUtil::create_ast_int(3, &parser);
    let app1 = ParBuilderUtil::create_ast_app(outer_lam, vec![three], &parser);
    let four = ParBuilderUtil::create_ast_int(4, &parser);
    let app2 = ParBuilderUtil::create_ast_app(app1, vec![four], &parser);

    let normalized = normalize_ann_proc(&app2, empty_inputs(), &HashMap::new(), &parser).unwrap();

    let r = reducer().await;
    let reduced = r.eval_expr(&normalized.par, &Env::new()).unwrap();
    assert_gint(&reduced, 7);
}

// Free variables outside the lambda are preserved as free vars in the body,
// not swallowed by the lambda. Here `z` is free in the lambda body; after
// normalization the body carries a free_map entry for z.
#[tokio::test]
async fn lambda_leaves_outer_free_vars_free() {
    let parser = rholang_parser::RholangParser::new();

    // body: x + z  (x is bound, z is free)
    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let z_ref = ParBuilderUtil::create_ast_proc_var("z", &parser);
    let body = ParBuilderUtil::create_ast_add(x_ref, z_ref, &parser);

    let lam = ParBuilderUtil::create_ast_lambda(&["x"], body, &parser);

    let result = normalize_ann_proc(&lam, empty_inputs(), &HashMap::new(), &parser).unwrap();

    // z shows up in free_map; x does not.
    assert!(
        result.free_map.get("z").is_some(),
        "z must remain free: {:?}",
        result.free_map
    );
    assert!(
        result.free_map.get("x").is_none(),
        "x was bound by lambda; must not leak as free"
    );
}

// Typed lambda `(x: Int) => x` normalizes with `param_types` populated
// (length 1, slot 0 = Int connective). Regression test for the
// annotation-plumbing path through p_lambda_normalizer.
#[tokio::test]
async fn typed_lambda_carries_param_type_annotation() {
    use rholang_parser::ast::SimpleType;
    let parser = rholang_parser::RholangParser::new();

    let int_ann = ParBuilderUtil::create_ast_simple_type(SimpleType::Int, &parser);
    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let lam = ParBuilderUtil::create_ast_lambda_typed(
        vec![("x", Some(int_ann))],
        x_ref,
        &parser,
    );

    let normalized = normalize_ann_proc(&lam, empty_inputs(), &HashMap::new(), &parser).unwrap();

    match first_expr(&normalized.par) {
        ExprInstance::ELambdaBody(ELambda {
            bind_count,
            param_types,
            ..
        }) => {
            assert_eq!(*bind_count, 1);
            assert_eq!(param_types.len(), 1, "annotation populates param_types");
            let ann = &param_types[0];
            assert_eq!(ann.connectives.len(), 1);
            use models::rhoapi::connective::ConnectiveInstance;
            match ann.connectives[0].connective_instance.as_ref() {
                Some(ConnectiveInstance::ConnInt(_)) => {}
                other => panic!("expected ConnInt annotation, got {other:?}"),
            }
        }
        other => panic!("expected ELambda, got {other:?}"),
    }
}

// Unannotated lambda stays `param_types = []` (legacy shape) so the
// reducer and existing proto consumers see no change.
#[tokio::test]
async fn untyped_lambda_leaves_param_types_empty() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let lam = ParBuilderUtil::create_ast_lambda(&["x"], x_ref, &parser);

    let normalized = normalize_ann_proc(&lam, empty_inputs(), &HashMap::new(), &parser).unwrap();

    match first_expr(&normalized.par) {
        ExprInstance::ELambdaBody(ELambda { param_types, .. }) => {
            assert!(
                param_types.is_empty(),
                "unannotated lambda leaves param_types empty: {param_types:?}"
            );
        }
        other => panic!("expected ELambda, got {other:?}"),
    }
}

// Shadowing: inner `(x) => (x) => x` binds the inner x; the inner reference
// resolves to BoundVar(0), not BoundVar(1). Verified by reducing
// `(((x) => (x) => x)(1))(2)` to 2 (inner x wins).
#[tokio::test]
async fn inner_lambda_shadows_outer_binder() {
    let parser = rholang_parser::RholangParser::new();

    let x_ref = ParBuilderUtil::create_ast_proc_var("x", &parser);
    let inner = ParBuilderUtil::create_ast_lambda(&["x"], x_ref, &parser);
    let outer = ParBuilderUtil::create_ast_lambda(&["x"], inner, &parser);

    let one = ParBuilderUtil::create_ast_int(1, &parser);
    let app1 = ParBuilderUtil::create_ast_app(outer, vec![one], &parser);
    let two = ParBuilderUtil::create_ast_int(2, &parser);
    let app2 = ParBuilderUtil::create_ast_app(app1, vec![two], &parser);

    let normalized = normalize_ann_proc(&app2, empty_inputs(), &HashMap::new(), &parser).unwrap();

    let r = reducer().await;
    let reduced = r.eval_expr(&normalized.par, &Env::new()).unwrap();
    // Inner x wins → output is 2.
    assert_gint(&reduced, 2);
}
