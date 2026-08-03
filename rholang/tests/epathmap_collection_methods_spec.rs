//! Native generic-collection method coverage for homogeneous EPathMap storage.
//!
//! These tests keep the receiver as `EPathMap` throughout.  They therefore
//! guard against the integration regressing to an `EMap`, `ESet`, or decoded
//! entry-vector compatibility path.

use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, EMethod, EPathMap, Expr, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::single_expr;
use models::rust::utils::{new_elist_par, new_gint_par, new_gstring_par};
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

fn int(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn string(value: &str) -> Par { new_gstring_par(value.to_owned(), Vec::new(), false) }

fn list(values: Vec<Par>) -> Par {
    new_elist_par(values, Vec::new(), false, None, Vec::new(), false)
}

fn pathmap(value: EPathMap) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(value)),
    }])
}

fn method(target: Par, name: &str, arguments: Vec<Par>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMethodBody(EMethod {
            method_name: name.to_owned(),
            target: Some(target),
            arguments,
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
}

fn native_pathmap(value: &Par) -> &EPathMap {
    let [
        Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
        },
    ] = value.exprs.as_slice()
    else {
        panic!("generic EPathMap method changed the carrier: {value:?}");
    };
    pathmap
}

async fn reducer() -> Arc<DebruijnInterpreter> {
    let (_, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;
    reducer
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn map_mode_generic_methods_stay_trie_native() {
    let reducer = reducer().await;
    let env = Env::new();
    let list_key = list(vec![int(5)]);
    let base = EPathMap::new_map(
        vec![(int(1), string("one")), (list_key.clone(), string("list"))],
        Vec::new(),
        false,
        None,
    );
    assert_eq!(base.mode(), EPathMapMode::Map);

    let get = reducer
        .eval_expr(
            &method(pathmap(base.clone()), "get", vec![list_key.clone()]),
            &env,
        )
        .expect("get on map-mode EPathMap");
    assert_eq!(get, string("list"));

    let fallback = reducer
        .eval_expr(
            &method(pathmap(base.clone()), "getOrElse", vec![
                int(99),
                string("fallback"),
            ]),
            &env,
        )
        .expect("getOrElse on map-mode EPathMap");
    assert_eq!(fallback, string("fallback"));

    let contains = reducer
        .eval_expr(
            &method(pathmap(base.clone()), "contains", vec![list_key.clone()]),
            &env,
        )
        .expect("contains on map-mode EPathMap");
    assert_eq!(
        single_expr(&contains).unwrap().expr_instance,
        Some(ExprInstance::GBool(true))
    );

    let size = reducer
        .eval_expr(&method(pathmap(base.clone()), "size", Vec::new()), &env)
        .expect("size on map-mode EPathMap");
    assert_eq!(size, int(2));

    let keys = reducer
        .eval_expr(&method(pathmap(base.clone()), "keys", Vec::new()), &env)
        .expect("keys on map-mode EPathMap");
    let Some(ExprInstance::ESetBody(keys)) =
        single_expr(&keys).and_then(|expr| expr.expr_instance.clone())
    else {
        panic!("keys did not return ESet: {keys:?}");
    };
    let keys = ParSetTypeMapper::eset_to_par_set(keys).ps;
    assert!(keys.contains(int(1)));
    assert!(keys.contains(list_key.clone()));

    let set = reducer
        .eval_expr(
            &method(pathmap(base.clone()), "set", vec![int(2), string("two")]),
            &env,
        )
        .expect("set on map-mode EPathMap");
    let set = native_pathmap(&set);
    assert_eq!(set.mode(), EPathMapMode::Map);
    assert_eq!(set.len(), 3);
    assert_eq!(set.get_map_value(&int(2)).unwrap(), Some(&string("two")));

    let deleted = reducer
        .eval_expr(
            &method(pathmap(base), "delete", vec![list_key.clone()]),
            &env,
        )
        .expect("delete on map-mode EPathMap");
    let deleted = native_pathmap(&deleted);
    assert_eq!(deleted.mode(), EPathMapMode::Map);
    assert_eq!(deleted.len(), 1);
    assert!(!deleted.contains_entry(&list_key));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_mode_and_neutral_empty_specialize_without_mixing() {
    let reducer = reducer().await;
    let env = Env::new();
    let member = list(vec![int(5)]);
    let set = EPathMap::new(vec![member.clone()], Vec::new(), false, None);
    assert_eq!(set.mode(), EPathMapMode::Set);

    let contains = reducer
        .eval_expr(
            &method(pathmap(set.clone()), "contains", vec![member.clone()]),
            &env,
        )
        .expect("contains on set-mode EPathMap");
    assert_eq!(
        single_expr(&contains).unwrap().expr_instance,
        Some(ExprInstance::GBool(true))
    );

    let deleted = reducer
        .eval_expr(&method(pathmap(set.clone()), "delete", vec![member]), &env)
        .expect("delete on set-mode EPathMap");
    let deleted = native_pathmap(&deleted);
    assert_eq!(deleted.mode(), EPathMapMode::Empty);
    assert!(deleted.is_empty());

    let mixed = reducer.eval_expr(
        &method(pathmap(set), "set", vec![int(1), string("one")]),
        &env,
    );
    assert!(
        mixed.is_err(),
        "set membership must not be mixed with map values"
    );

    let specialized = reducer
        .eval_expr(
            &method(pathmap(EPathMap::default()), "set", vec![
                int(1),
                string("one"),
            ]),
            &env,
        )
        .expect("the first map insertion specializes neutral empty storage");
    let specialized = native_pathmap(&specialized);
    assert_eq!(specialized.mode(), EPathMapMode::Map);
    assert_eq!(
        specialized.get_map_value(&int(1)).unwrap(),
        Some(&string("one"))
    );
}
