// f1r3lang-with-monads Stage B.2: reducer-level end-to-end tests for
// native `foreach` evaluation. Confirms the paper's §4.2 one-step
// rule: for each element of the source, substitute into the body,
// combine via the aggregator (default ParMonoid = parallel
// composition), publish once on yield_name.

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

// `foreach ( x <- [1, 2, 3] ) yield ( out ) { @"collect"!(x) }` under
// ParMonoid aggregates the three send-contributions into a single
// publication on `out` of the parallel composition
// `@"collect"!(1) | @"collect"!(2) | @"collect"!(3)`. A listener on
// `out` that walks through those sends sees all three.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_sends_aggregated_contributions() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        // Receiver forwards whatever lands on `out` by firing its
        // aggregate (which contains the three inner sends) into the
        // tuple space, so `data_at("collect")` reveals them.
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [1, 2, 3] ) yield ( out ) { @"collect"!(x) }
              | for (@body <= out) { body }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "collect").await;
        assert_eq!(data.len(), 3, "three parallel sends in the aggregate");
        let mut values: Vec<i64> = data
            .iter()
            .filter_map(
                |p| match p.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                    Some(ExprInstance::GInt(i)) => Some(*i),
                    _ => None,
                },
            )
            .collect();
        values.sort();
        assert_eq!(values, vec![1, 2, 3]);
    })
    .await
}

// Empty source publishes the aggregator's unit (Nil / empty Par) so
// consumers see exactly one publication regardless of source size.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_empty_source_publishes_unit() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [] ) yield ( out ) { @"collect"!(x) }
              | for (@_ <= out) { @"done"!(1) }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "done").await;
        assert_eq!(
            data.len(),
            1,
            "empty-source foreach still publishes its unit, firing the listener once"
        );
        assert!(
            data_at(&runtime, "collect").await.is_empty(),
            "no element → body never fires → no collect sends"
        );
    })
    .await
}

// Single-element source: the aggregator degenerates to the body's own
// contribution.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_singleton_source_publishes_single_contribution() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [42] ) yield ( out ) { @"collect"!(x) }
              | for (@body <= out) { body }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "collect").await;
        assert_eq!(data.len(), 1);
        assert_eq!(
            data[0].exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GInt(42)),
        );
    })
    .await
}

// §4.4 `&` — independent multi-bind: cartesian product.
// `x <- [1, 2] & y <- [10, 20]` produces 4 candidates; body is
// `@"out"!(x + y)`. Expected values: 11, 12, 21, 22.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_amp_independent_binds_cartesian_product() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [1, 2] & y <- [10, 20] )
              yield ( out ) { @"out"!(x + y) }
              | for (@body <= out) { body }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "out").await;
        assert_eq!(data.len(), 4, "cartesian product has 2*2 = 4 candidates");
        let mut vals: Vec<i64> = data
            .iter()
            .filter_map(
                |p| match p.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                    Some(ExprInstance::GInt(i)) => Some(*i),
                    _ => None,
                },
            )
            .collect();
        vals.sort();
        assert_eq!(vals, vec![11, 12, 21, 22]);
    })
    .await
}

// §4.5 `where` — filters out non-passing candidates by contributing
// the aggregator's unit. `foreach ( x <- [1, 2, 3] where x > 1 )`
// should emit contributions only for 2 and 3.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_where_filters_candidates() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [1, 2, 3] where x > 1 )
              yield ( out ) { @"kept"!(x) }
              | for (@body <= out) { body }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "kept").await;
        let mut vals: Vec<i64> = data
            .iter()
            .filter_map(
                |p| match p.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                    Some(ExprInstance::GInt(i)) => Some(*i),
                    _ => None,
                },
            )
            .collect();
        vals.sort();
        assert_eq!(vals, vec![2, 3]);
    })
    .await
}

// §4.4 + §4.5 combined: cartesian product with a guard that filters
// pairs. `x <- [1, 2] & y <- [10, 20] where x + y > 15` keeps only
// (1, 20), (2, 20) — expected sums 21, 22.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreach_amp_with_where_filters_cartesian_candidates() {
    with_runtime("foreach-reduce-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            r#"
            new out in {
              foreach ( x <- [1, 2] & y <- [10, 20] where x + y > 15 )
              yield ( out ) { @"kept"!(x + y) }
              | for (@body <= out) { body }
            }
            "#,
        )
        .await;
        let data = data_at(&runtime, "kept").await;
        let mut vals: Vec<i64> = data
            .iter()
            .filter_map(
                |p| match p.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
                    Some(ExprInstance::GInt(i)) => Some(*i),
                    _ => None,
                },
            )
            .collect();
        vals.sort();
        assert_eq!(vals, vec![21, 22]);
    })
    .await
}
