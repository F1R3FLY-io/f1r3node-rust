use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, EPathMap, EPlus, Expr, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::utils::new_gint_par;
use rho_pure_eval::Env;
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;

type TestSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

fn gint(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn plus(left: i64, right: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
            p1: Some(gint(left)),
            p2: Some(gint(right)),
        })),
    }])
}

fn pathmap_expr(pathmap: EPathMap) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(pathmap)),
    }])
}

fn evaluated_pathmap(par: &Par) -> &EPathMap {
    let [expr] = par.exprs.as_slice() else {
        panic!("evaluated EPathMap must remain one expression")
    };
    let Some(ExprInstance::EPathmapBody(pathmap)) = &expr.expr_instance else {
        panic!("evaluated expression must remain an EPathMap")
    };
    pathmap
}

#[tokio::test(flavor = "current_thread")]
async fn evaluator_streams_set_and_map_children_in_canonical_forward_order() {
    let (_space, reducer) = create_test_space::<TestSpace>().await;
    let env = Env::new();

    let empty = reducer
        .eval_expr(&pathmap_expr(EPathMap::default()), &env)
        .expect("neutral empty evaluates");
    assert!(evaluated_pathmap(&empty).is_empty());

    let set = EPathMap::new(vec![plus(4, 2), plus(1, 2)], Vec::new(), false, None);
    let evaluated_set = reducer
        .eval_expr(&pathmap_expr(set), &env)
        .expect("set-mode EPathMap evaluates");
    let evaluated_set = evaluated_pathmap(&evaluated_set);
    assert_eq!(evaluated_set.len(), 2);
    assert!(evaluated_set.contains_entry(&gint(3)));
    assert!(evaluated_set.contains_entry(&gint(6)));

    let map = EPathMap::new_map(
        [(plus(4, 2), plus(20, 2)), (plus(1, 2), plus(10, 1))],
        Vec::new(),
        false,
        None,
    );
    let evaluated_map = reducer
        .eval_expr(&pathmap_expr(map), &env)
        .expect("map-mode EPathMap evaluates");
    let evaluated_map = evaluated_pathmap(&evaluated_map);
    assert_eq!(evaluated_map.len(), 2);
    assert_eq!(
        evaluated_map.get_map_value(&gint(3)).unwrap(),
        Some(&gint(11))
    );
    assert_eq!(
        evaluated_map.get_map_value(&gint(6)).unwrap(),
        Some(&gint(22))
    );
}
