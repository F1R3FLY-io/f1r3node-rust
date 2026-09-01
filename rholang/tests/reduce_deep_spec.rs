// Deep-coverage evaluation tests for reduce.rs: method arms, operator error
// branches, and evaluation flow paths not exercised by reduce_spec.rs.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, If, Par};
use rholang::rust::interpreter::env::Env;
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
    let res = runtime.evaluate_with_term(term).await.unwrap();
    assert!(
        res.errors.is_empty(),
        "Expected success for: {}\nErrors: {:?}",
        term,
        res.errors
    );
}

async fn eval_err(runtime: &mut RhoRuntimeImpl, term: &str) {
    let res = runtime.evaluate_with_term(term).await.unwrap();
    assert!(
        !res.errors.is_empty(),
        "Expected error for: {}\nGot success",
        term
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

/// Evaluates `@"lhs<id>"!(<lhs>)` and `@"rhs<id>"!(<rhs>)` and asserts the
/// stored values are identical Pars. Both sides go through the same
/// normalize/eval path, so this compares evaluated results robustly.
async fn assert_evals_same(runtime: &mut RhoRuntimeImpl, id: &str, lhs: &str, rhs: &str) {
    let lhs_chan = format!("lhs-{}", id);
    let rhs_chan = format!("rhs-{}", id);
    eval_ok(runtime, &format!("@\"{}\"!({})", lhs_chan, lhs)).await;
    eval_ok(runtime, &format!("@\"{}\"!({})", rhs_chan, rhs)).await;
    let lhs_data = data_at(runtime, &lhs_chan).await;
    let rhs_data = data_at(runtime, &rhs_chan).await;
    assert!(
        !lhs_data.is_empty(),
        "no data for {} from {}",
        lhs_chan,
        lhs
    );
    assert_eq!(
        lhs_data, rhs_data,
        "{} evaluated differently from {}",
        lhs, rhs
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tuple_and_bytearray_nth() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(&mut runtime, "t1", "(4, 5, 6).nth(1)", "5").await;
        assert_evals_same(&mut runtime, "t2", "\"ff02\".hexToBytes().nth(1)", "2").await;
        eval_err(&mut runtime, "@\"e1\"!((4, 5).nth(9))").await;
        eval_err(&mut runtime, "@\"e2\"!(\"ff\".hexToBytes().nth(9))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.nth(0))").await;
        eval_err(&mut runtime, "@\"e4\"!([1, 2].nth(0, 1))").await;
        eval_err(&mut runtime, "@\"e5\"!([1, 2].nth(true))").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn take_get_contains_add_delete_methods() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(&mut runtime, "g1", "{\"a\": 1, \"b\": 2}.get(\"a\")", "1").await;
        assert_evals_same(&mut runtime, "g2", "{\"a\": 1}.get(\"missing\")", "Nil").await;
        assert_evals_same(&mut runtime, "c1", "Set(1, 2).contains(1)", "true").await;
        assert_evals_same(&mut runtime, "c2", "Set(1, 2).contains(3)", "false").await;
        assert_evals_same(&mut runtime, "c3", "{\"a\": 1}.contains(\"a\")", "true").await;
        assert_evals_same(&mut runtime, "c4", "{\"a\": 1}.contains(\"z\")", "false").await;
        assert_evals_same(&mut runtime, "a1", "Set(1).add(2)", "Set(1, 2)").await;
        assert_evals_same(&mut runtime, "d1", "Set(1, 2).delete(1)", "Set(2)").await;
        assert_evals_same(
            &mut runtime,
            "d2",
            "{\"a\": 1, \"b\": 2}.delete(\"a\")",
            "{\"b\": 2}",
        )
        .await;
        eval_err(&mut runtime, "@\"e1\"!(Set(1).add())").await;
        eval_err(&mut runtime, "@\"e2\"!(42.get(1))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.contains(1))").await;
        eval_err(&mut runtime, "@\"e4\"!(42.delete(1))").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn union_diff_intersection_methods() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(
            &mut runtime,
            "u1",
            "{\"a\": 1}.union({\"b\": 2})",
            "{\"a\": 1, \"b\": 2}",
        )
        .await;
        assert_evals_same(&mut runtime, "u2", "Set(1).union(Set(2))", "Set(1, 2)").await;
        assert_evals_same(
            &mut runtime,
            "df1",
            "{\"a\": 1, \"b\": 2}.diff({\"a\": 1})",
            "{\"b\": 2}",
        )
        .await;
        assert_evals_same(&mut runtime, "df2", "Set(1, 2).diff(Set(2))", "Set(1)").await;
        eval_err(&mut runtime, "@\"e1\"!(42.union(Set(1)))").await;
        eval_err(&mut runtime, "@\"e2\"!(Set(1).union(42))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.diff(Set(1)))").await;
        eval_err(&mut runtime, "@\"e4\"!(Set(1).union())").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pathmap_union_intersection_restriction_drophead_run() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(
            &mut runtime,
            "pm1",
            "{| [\"a\"], [\"b\"] |}.union({| [\"c\"] |})",
            "{| [\"a\"], [\"b\"], [\"c\"] |}",
        )
        .await;
        eval_ok(
            &mut runtime,
            "@\"pmi\"!({| [\"a\"], [\"b\"] |}.intersection({| [\"a\"] |}))",
        )
        .await;
        assert!(!data_at(&runtime, "pmi").await.is_empty());
        eval_ok(
            &mut runtime,
            "@\"pmr\"!({| [\"a\", \"b\"] |}.restriction({| [\"a\"] |}))",
        )
        .await;
        assert!(!data_at(&runtime, "pmr").await.is_empty());
        assert_evals_same(
            &mut runtime,
            "pm2",
            "{| [\"a\", \"b\"], [\"c\", \"d\"] |}.dropHead(1)",
            "{| [\"b\"], [\"d\"] |}",
        )
        .await;
        eval_ok(&mut runtime, "@\"pmrun\"!({| [\"a\"] |}.run(1))").await;
        assert!(!data_at(&runtime, "pmrun").await.is_empty());
        eval_err(&mut runtime, "@\"e1\"!(42.restriction(1))").await;
        eval_err(&mut runtime, "@\"e2\"!(42.dropHead(1))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.run(1))").await;
        eval_err(&mut runtime, "@\"e4\"!({| [\"a\"] |}.dropHead(-1))").await;
        eval_err(&mut runtime, "@\"e5\"!({| [\"a\"] |}.dropHead())").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slice_take_and_length_arms() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(
            &mut runtime,
            "s1",
            "\"ff0201\".hexToBytes().slice(1, 3)",
            "\"0201\".hexToBytes()",
        )
        .await;
        assert_evals_same(&mut runtime, "t1", "[1, 2, 3].take(2)", "[1, 2]").await;
        eval_err(&mut runtime, "@\"e1\"!(42.slice(0, 1))").await;
        eval_err(&mut runtime, "@\"e2\"!([1].slice(0))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.take(1))").await;
        eval_err(&mut runtime, "@\"e4\"!([1].take())").await;
        eval_err(&mut runtime, "@\"e5\"!(42.length())").await;
        eval_err(&mut runtime, "@\"e6\"!(\"abc\".length(1))").await;
        eval_err(&mut runtime, "@\"e7\"!(42.size())").await;
        eval_err(&mut runtime, "@\"e8\"!(42.keys())").await;
        eval_err(&mut runtime, "@\"e9\"!(42.getOrElse(1, 2))").await;
        eval_err(&mut runtime, "@\"e10\"!(42.set(1, 2))").await;
        eval_err(&mut runtime, "@\"e11\"!(42.toList())").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_method_and_singularity_errors() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "@\"e1\"!(42.frobnicate())").await;
        eval_err(&mut runtime, "@\"e2\"!(\"zz\".hexToBytes(1))").await;
        eval_err(&mut runtime, "@\"e3\"!(42.hexToBytes())").await;
        eval_err(&mut runtime, "@\"e4\"!(42.bytesToHex())").await;
        eval_err(&mut runtime, "@\"e5\"!(\"ff\".hexToBytes().bytesToHex(1))").await;
        eval_err(&mut runtime, "@\"e6\"!(42.toUtf8Bytes())").await;
        assert_evals_same(
            &mut runtime,
            "b1",
            "\"ff00\".hexToBytes().bytesToHex()",
            "\"ff00\"",
        )
        .await;
        assert_evals_same(&mut runtime, "b2", "\"hi\".toUtf8Bytes().length()", "2").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operator_error_arms() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "@\"e1\"!(1 ++ 1)").await;
        eval_err(&mut runtime, "@\"e2\"!(\"a\" ++ 1)").await;
        eval_err(&mut runtime, "@\"e3\"!([1] ++ 1)").await;
        eval_err(&mut runtime, "@\"e4\"!(Set(1) ++ 1)").await;
        eval_err(&mut runtime, "@\"e5\"!({\"a\": 1} ++ 1)").await;
        eval_err(&mut runtime, "@\"e6\"!(1 -- 1)").await;
        eval_err(&mut runtime, "@\"e7\"!(Set(1) -- 1)").await;
        eval_err(&mut runtime, "@\"e8\"!(1 %% {\"a\": 1})").await;
        eval_err(&mut runtime, "@\"e9\"!(\"${x}\" %% 1)").await;
        eval_err(&mut runtime, "@\"e10\"!(\"${x}\" %% {\"x\": [1]})").await;
        eval_err(&mut runtime, "@\"e11\"!(\"${x}\" %% {1: 2})").await;
        eval_err(&mut runtime, "@\"e12\"!(true + true)").await;
        eval_err(&mut runtime, "@\"e13\"!(1 + \"a\")").await;
        eval_err(&mut runtime, "@\"e14\"!(1 - \"a\")").await;
        eval_err(&mut runtime, "@\"e15\"!(true * true)").await;
        eval_err(&mut runtime, "@\"e16\"!(true / true)").await;
        eval_err(&mut runtime, "@\"e17\"!(true % true)").await;
        eval_err(&mut runtime, "@\"e18\"!(-true)").await;
        eval_err(&mut runtime, "@\"e19\"!(not 1)").await;
        eval_err(&mut runtime, "@\"e20\"!(1 and true)").await;
        eval_err(&mut runtime, "@\"e21\"!(true or 1)").await;
        eval_err(&mut runtime, "@\"e22\"!(1 < true)").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn string_and_bool_comparisons() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(&mut runtime, "c1", "\"a\" < \"b\"", "true").await;
        assert_evals_same(&mut runtime, "c2", "\"b\" <= \"b\"", "true").await;
        assert_evals_same(&mut runtime, "c3", "\"c\" > \"b\"", "true").await;
        assert_evals_same(&mut runtime, "c4", "\"a\" >= \"b\"", "false").await;
        assert_evals_same(&mut runtime, "c5", "false < true", "true").await;
        assert_evals_same(&mut runtime, "c6", "true <= true", "true").await;
        assert_evals_same(&mut runtime, "c7", "true > false", "true").await;
        assert_evals_same(&mut runtime, "c8", "false >= true", "false").await;
        assert_evals_same(&mut runtime, "c9", "\"a\" == \"a\"", "true").await;
        assert_evals_same(&mut runtime, "c10", "\"a\" != \"b\"", "true").await;
        assert_evals_same(&mut runtime, "c11", "true and false", "false").await;
        assert_evals_same(&mut runtime, "c12", "false or true", "true").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interpolation_success_and_empty_cases() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(&mut runtime, "i1", "\"\" %% {}", "\"\"").await;
        assert_evals_same(
            &mut runtime,
            "i2",
            "\"x is ${x}!\" %% {\"x\": 7}",
            "\"x is 7!\"",
        )
        .await;
        assert_evals_same(
            &mut runtime,
            "i3",
            "\"${a} and ${b}\" %% {\"a\": \"one\", \"b\": true}",
            "\"one and true\"",
        )
        .await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn match_failure_and_wildcard_arms() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_ok(&mut runtime, "match 42 { 1 => @\"nomatch\"!(1) }").await;
        assert!(
            data_at(&runtime, "nomatch").await.is_empty(),
            "a match with no matching case must reduce to a no-op"
        );
        eval_ok(
            &mut runtime,
            "match 42 { 1 => @\"m1\"!(\"one\") _ => @\"m1\"!(\"other\") }",
        )
        .await;
        let matched = data_at(&runtime, "m1").await;
        assert_eq!(matched, vec![string_channel("other")]);
        eval_ok(
            &mut runtime,
            "match [1, 2, 3] { [x, y, z] => @\"m2\"!(x + y + z) }",
        )
        .await;
        assert!(!data_at(&runtime, "m2").await.is_empty());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bundle_polarity_violations() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "new x in { @{bundle0{*x}}!(1) }").await;
        eval_err(
            &mut runtime,
            "new x in { for (_ <- @{bundle0{*x}}) { Nil } }",
        )
        .await;
        eval_ok(
            &mut runtime,
            "new x in { @{bundle+{*x}}!(1) | for (@v <- x) { @\"b1\"!(v) } }",
        )
        .await;
        assert!(!data_at(&runtime, "b1").await.is_empty());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persistent_send_and_contract_receive() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "@\"src\"!!(7) | for (@v <- @\"src\") { @\"p1\"!(v) } | for (@v <- @\"src\") { @\"p2\"!(v) }",
        )
        .await;
        assert!(!data_at(&runtime, "p1").await.is_empty());
        assert!(!data_at(&runtime, "p2").await.is_empty());

        eval_ok(
            &mut runtime,
            "contract @\"double\"(@x, ret) = { ret!(x * 2) } | @\"double\"!(21, \"d1\") | @\"double\"!(5, \"d2\")",
        )
        .await;
        assert!(!data_at(&runtime, "d1").await.is_empty());
        assert!(!data_at(&runtime, "d2").await.is_empty());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unforgeable_to_string_error_arms() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "new x in { @\"u1\"!((*x).toString()) }").await;
        eval_err(&mut runtime, "new x in { @\"u2\"!((*x).toString(1)) }").await;
        eval_err(&mut runtime, "@\"u3\"!(42.toString())").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eval_if_conditionals_via_injection() {
    with_runtime("reduce-deep-", |runtime| async move {
        runtime
            .cost
            .set(rholang::rust::interpreter::accounting::costs::Cost::unsafe_max());
        let send_to = |name: &str, value: i64| Par {
            sends: vec![models::rhoapi::Send {
                chan: Some(string_channel(name)),
                data: vec![Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(value)),
                    }],
                    ..Default::default()
                }],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };
        let bool_par = |b: bool| Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GBool(b)),
            }],
            ..Default::default()
        };

        let conditional = |cond: Par, then_val: i64, else_val: i64| Par {
            conditionals: vec![If {
                condition: Some(cond),
                if_true: Some(send_to("if-true", then_val)),
                if_false: Some(send_to("if-false", else_val)),
                locally_free: vec![],
                connective_used: false,
            }],
            ..Default::default()
        };

        let rand = crypto::rust::hash::blake2b512_random::Blake2b512Random::create_from_bytes(&[]);
        runtime
            .inj(conditional(bool_par(true), 1, 2), Env::new(), rand.clone())
            .await
            .unwrap();
        assert!(!data_at(&runtime, "if-true").await.is_empty());
        assert!(data_at(&runtime, "if-false").await.is_empty());

        runtime
            .inj(conditional(bool_par(false), 1, 2), Env::new(), rand.clone())
            .await
            .unwrap();
        assert!(!data_at(&runtime, "if-false").await.is_empty());

        let bad = runtime
            .inj(
                conditional(string_channel("not-a-bool"), 1, 2),
                Env::new(),
                rand,
            )
            .await;
        assert!(bad.is_err());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_with_unknown_urn_fails() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "new x(`rho:this:does:not:exist`) in { Nil }").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn type_connective_patterns_in_match() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_ok(&mut runtime, "match 1 { Int => @\"pc1\"!(true) }").await;
        assert!(!data_at(&runtime, "pc1").await.is_empty());
        eval_ok(&mut runtime, "match \"s\" { String => @\"pc2\"!(true) }").await;
        assert!(!data_at(&runtime, "pc2").await.is_empty());
        eval_ok(&mut runtime, "match true { Bool => @\"pc3\"!(true) }").await;
        assert!(!data_at(&runtime, "pc3").await.is_empty());
        eval_ok(
            &mut runtime,
            "match `rho:some:uri` { Uri => @\"pc4\"!(true) }",
        )
        .await;
        assert!(!data_at(&runtime, "pc4").await.is_empty());
        eval_ok(
            &mut runtime,
            "match \"aa\".hexToBytes() { ByteArray => @\"pc5\"!(true) }",
        )
        .await;
        assert!(!data_at(&runtime, "pc5").await.is_empty());
        eval_ok(
            &mut runtime,
            "match 5 { x /\\ Int => @\"pc6\"!(x) _ => Nil }",
        )
        .await;
        assert!(!data_at(&runtime, "pc6").await.is_empty());
        eval_ok(
            &mut runtime,
            "match 5 { 1 \\/ 5 => @\"pc7\"!(true) _ => Nil }",
        )
        .await;
        assert!(!data_at(&runtime, "pc7").await.is_empty());
        eval_ok(&mut runtime, "match 5 { ~6 => @\"pc8\"!(true) _ => Nil }").await;
        assert!(!data_at(&runtime, "pc8").await.is_empty());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matches_operator_with_patterns() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        assert_evals_same(&mut runtime, "mo1", "1 matches _", "true").await;
        assert_evals_same(&mut runtime, "mo2", "(1, 2) matches (1, _)", "true").await;
        assert_evals_same(&mut runtime, "mo3", "[1, 2] matches [1, 3]", "false").await;
        assert_evals_same(&mut runtime, "mo4", "5 matches Int", "true").await;
        assert_evals_same(&mut runtime, "mo5", "\"x\" matches Int", "false").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn process_position_expressions() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "@\"box\"!(@\"inner\"!(1)) | for (@p <- @\"box\") { p }",
        )
        .await;
        assert!(
            !data_at(&runtime, "inner").await.is_empty(),
            "evaluating a received process variable must run the quoted send"
        );
        eval_ok(&mut runtime, "new x in { @\"ulf\"!([*x], (*x, 1)) }").await;
        assert!(!data_at(&runtime, "ulf").await.is_empty());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn arithmetic_type_errors_across_operand_types() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_err(&mut runtime, "@\"e1\"!(1 + [1])").await;
        eval_err(&mut runtime, "@\"e2\"!(1 + (1, 2))").await;
        eval_err(&mut runtime, "@\"e3\"!(1 + Set(1))").await;
        eval_err(&mut runtime, "@\"e4\"!(1 + {\"a\": 1})").await;
        eval_err(&mut runtime, "@\"e5\"!(1 + `rho:some:uri`)").await;
        eval_err(&mut runtime, "@\"e6\"!(1 + \"aa\".hexToBytes())").await;
        eval_err(&mut runtime, "@\"e7\"!(1 * \"a\")").await;
        eval_err(&mut runtime, "@\"e8\"!(1 / \"a\")").await;
        eval_err(&mut runtime, "@\"e9\"!(1 % \"a\")").await;
        eval_err(&mut runtime, "@\"e10\"!(\"a\" - 1)").await;
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_errors_are_aggregated() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        let res = runtime
            .evaluate_with_term("@\"x\"!(1 / 0) | @\"y\"!(2 / 0)")
            .await
            .unwrap();
        assert!(
            res.errors.len() >= 2,
            "expected both division errors, got {:?}",
            res.errors
        );
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joined_receive_and_remainder_patterns() {
    with_runtime("reduce-deep-", |mut runtime| async move {
        eval_ok(
            &mut runtime,
            "for (@a <- @\"ja\"; @b <- @\"jb\") { @\"j1\"!(a + b) } | @\"ja\"!(1) | @\"jb\"!(2)",
        )
        .await;
        assert!(!data_at(&runtime, "j1").await.is_empty());

        eval_ok(
            &mut runtime,
            "for (@[head ...tail] <- @\"list\") { @\"r1\"!(head) | @\"r2\"!(tail) } | @\"list\"!([9, 8, 7])",
        )
        .await;
        assert!(!data_at(&runtime, "r1").await.is_empty());
        assert!(!data_at(&runtime, "r2").await.is_empty());
    })
    .await
}
