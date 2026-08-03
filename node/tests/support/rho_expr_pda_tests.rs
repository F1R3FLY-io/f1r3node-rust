//! Differential and depth gates for the production RhoExpr PDA.

use std::collections::HashMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EMap, ENeg, Expr, KeyValuePair, Par};

use super::{
    rho_expr_conversion_oracle as oracle, rho_expr_pda, RhoExpr, RhoPathMap, RhoPathMapBinding,
    RhoUnforg,
};

#[path = "../../../models/tests/par_corpus/mod.rs"]
mod par_corpus;

fn par_of(instance: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn json(value: &RhoExpr) -> String {
    serde_json::to_string(value).expect("RhoExpr serialization must succeed")
}

#[test]
fn conversion_pda_matches_recursive_oracle_for_every_expr_variant() {
    let corpus = par_corpus::every_expr_instance();
    assert_eq!(corpus.len(), 36, "corpus must cover every ExprInstance arm");

    for (name, instance) in corpus {
        let input = Expr {
            expr_instance: Some(instance),
        };
        let expected = oracle::from_expr(input.clone());
        let actual = rho_expr_pda::from_expr(input);

        match (expected.as_ref(), actual.as_ref()) {
            (Some(expected), Some(actual)) => assert_eq!(
                serde_json::to_value(actual).expect("PDA result must serialize"),
                serde_json::to_value(expected).expect("oracle result must serialize"),
                "recursive/PDA mismatch for {name}",
            ),
            (None, None) => {}
            _ => panic!("recursive/PDA optionality mismatch for {name}"),
        }
    }
}

#[test]
fn conversion_pda_matches_recursive_oracle_for_par_boundaries() {
    let fixtures = vec![
        Par::default(),
        par_corpus::all_par_fields(),
        models::par_from_default! {
            exprs: vec![Expr { expr_instance: Some(ExprInstance::GInt(1)) }],
            bundles: vec![models::rhoapi::Bundle {
                body: Some(par_of(ExprInstance::GString("bundle".to_owned()))),
                write_flag: true,
                read_flag: false,
            }],
            ..Default::default()
        },
    ];

    for (index, input) in fixtures.into_iter().enumerate() {
        let expected = oracle::from_par(input.clone());
        let actual = rho_expr_pda::from_par(input);
        assert_eq!(
            expected.as_ref().map(json),
            actual.as_ref().map(json),
            "recursive/PDA Par mismatch at fixture {index}",
        );
    }
}

fn int(data: i64) -> RhoExpr { RhoExpr::ExprInt { data } }

fn assert_json(expr: RhoExpr, expected: &str) {
    assert_eq!(json(&expr), expected);
    assert_eq!(
        json(&expr.clone()),
        expected,
        "Clone changed JSON semantics"
    );
    assert_eq!(
        format!("{expr:?}"),
        expected,
        "Debug must remain stack-safe"
    );
}

macro_rules! assert_binary_json {
    ($variant:ident, $name:literal) => {
        assert_json(
            RhoExpr::$variant {
                left: Box::new(int(1)),
                right: Box::new(int(2)),
            },
            concat!(
                "{\"",
                $name,
                "\":{\"left\":{\"ExprInt\":{\"data\":1}},\"right\":{\"ExprInt\":{\"data\":2}}}}"
            ),
        );
    };
}

#[test]
fn stack_safe_traits_preserve_derived_json_shapes() {
    assert_json(
        RhoExpr::ExprPar { data: vec![int(1)] },
        r#"{"ExprPar":{"data":[{"ExprInt":{"data":1}}]}}"#,
    );
    assert_json(
        RhoExpr::ExprTuple { data: vec![int(1)] },
        r#"{"ExprTuple":{"data":[{"ExprInt":{"data":1}}]}}"#,
    );
    assert_json(
        RhoExpr::ExprList { data: vec![int(1)] },
        r#"{"ExprList":{"data":[{"ExprInt":{"data":1}}]}}"#,
    );
    assert_json(
        RhoExpr::ExprSet { data: vec![int(1)] },
        r#"{"ExprSet":{"data":[{"ExprInt":{"data":1}}]}}"#,
    );
    assert_json(
        RhoExpr::ExprMap {
            data: HashMap::from([("key".to_owned(), int(1))]),
        },
        r#"{"ExprMap":{"data":{"key":{"ExprInt":{"data":1}}}}}"#,
    );
    assert_json(
        RhoExpr::ExprPathMap {
            data: RhoPathMap::Empty,
        },
        r#"{"ExprPathMap":{"data":"Empty"}}"#,
    );
    assert_json(
        RhoExpr::ExprPathMap {
            data: RhoPathMap::Set {
                entries: vec![int(1)],
            },
        },
        r#"{"ExprPathMap":{"data":{"Set":{"entries":[{"ExprInt":{"data":1}}]}}}}"#,
    );
    assert_json(
        RhoExpr::ExprPathMap {
            data: RhoPathMap::Map {
                entries: vec![RhoPathMapBinding {
                    key: int(1),
                    value: int(2),
                }],
            },
        },
        r#"{"ExprPathMap":{"data":{"Map":{"entries":[{"key":{"ExprInt":{"data":1}},"value":{"ExprInt":{"data":2}}}]}}}}"#,
    );
    assert_json(
        RhoExpr::ExprBool { data: true },
        r#"{"ExprBool":{"data":true}}"#,
    );
    assert_json(RhoExpr::ExprInt { data: -7 }, r#"{"ExprInt":{"data":-7}}"#);
    assert_json(
        RhoExpr::ExprString {
            data: "a\n\"b".to_owned(),
        },
        r#"{"ExprString":{"data":"a\n\"b"}}"#,
    );
    assert_json(
        RhoExpr::ExprUri {
            data: "rho:x".to_owned(),
        },
        r#"{"ExprUri":{"data":"rho:x"}}"#,
    );
    assert_json(
        RhoExpr::ExprBytes {
            data: "00ff".to_owned(),
        },
        r#"{"ExprBytes":{"data":"00ff"}}"#,
    );
    assert_json(
        RhoExpr::ExprFloat { data: 1.5 },
        r#"{"ExprFloat":{"data":1.5}}"#,
    );
    assert_json(
        RhoExpr::ExprBigInt {
            data: "123".to_owned(),
        },
        r#"{"ExprBigInt":{"data":"123"}}"#,
    );
    assert_json(
        RhoExpr::ExprBigRat {
            numerator: "1".to_owned(),
            denominator: "2".to_owned(),
        },
        r#"{"ExprBigRat":{"numerator":"1","denominator":"2"}}"#,
    );
    assert_json(
        RhoExpr::ExprFixedPoint {
            value: "314".to_owned(),
            scale: 2,
        },
        r#"{"ExprFixedPoint":{"value":"314","scale":2}}"#,
    );
    assert_json(
        RhoExpr::ExprUnforg {
            data: RhoUnforg::UnforgPrivate {
                data: "aa".to_owned(),
            },
        },
        r#"{"ExprUnforg":{"data":{"UnforgPrivate":{"data":"aa"}}}}"#,
    );
    assert_json(
        RhoExpr::ExprBundle {
            data: Box::new(int(1)),
            read: true,
            write: false,
        },
        r#"{"ExprBundle":{"data":{"ExprInt":{"data":1}},"read":true,"write":false}}"#,
    );
    assert_json(
        RhoExpr::ExprNot {
            data: Box::new(int(1)),
        },
        r#"{"ExprNot":{"data":{"ExprInt":{"data":1}}}}"#,
    );
    assert_json(
        RhoExpr::ExprNeg {
            data: Box::new(int(1)),
        },
        r#"{"ExprNeg":{"data":{"ExprInt":{"data":1}}}}"#,
    );
    assert_binary_json!(ExprPlus, "ExprPlus");
    assert_binary_json!(ExprMinus, "ExprMinus");
    assert_binary_json!(ExprMult, "ExprMult");
    assert_binary_json!(ExprDiv, "ExprDiv");
    assert_binary_json!(ExprMod, "ExprMod");
    assert_binary_json!(ExprLt, "ExprLt");
    assert_binary_json!(ExprLte, "ExprLte");
    assert_binary_json!(ExprGt, "ExprGt");
    assert_binary_json!(ExprGte, "ExprGte");
    assert_binary_json!(ExprEq, "ExprEq");
    assert_binary_json!(ExprNeq, "ExprNeq");
    assert_binary_json!(ExprAnd, "ExprAnd");
    assert_binary_json!(ExprOr, "ExprOr");
    assert_binary_json!(ExprConcat, "ExprConcat");
    assert_binary_json!(ExprInterpolate, "ExprInterpolate");
    assert_binary_json!(ExprDiff, "ExprDiff");
    assert_json(
        RhoExpr::ExprMatches {
            target: Box::new(int(1)),
            pattern: Box::new(int(2)),
        },
        r#"{"ExprMatches":{"target":{"ExprInt":{"data":1}},"pattern":{"ExprInt":{"data":2}}}}"#,
    );
    assert_json(
        RhoExpr::ExprMethod {
            target: Box::new(int(1)),
            name: "nth".to_owned(),
            args: vec![int(2)],
        },
        r#"{"ExprMethod":{"target":{"ExprInt":{"data":1}},"name":"nth","args":[{"ExprInt":{"data":2}}]}}"#,
    );
    assert_json(
        RhoExpr::ExprVar { index: -1 },
        r#"{"ExprVar":{"index":-1}}"#,
    );
    assert_json(RhoExpr::ExprSysAuthToken, r#""ExprSysAuthToken""#);
    assert_json(
        RhoExpr::ExprUnknown {
            type_name: "Nil".to_owned(),
        },
        r#"{"ExprUnknown":{"type_name":"Nil"}}"#,
    );
}

#[test]
fn legacy_map_clone_preserves_json_iteration_order() {
    let expr = RhoExpr::ExprMap {
        data: (0..32)
            .map(|index| (format!("key-{index}"), int(index)))
            .collect(),
    };
    assert_eq!(json(&expr.clone()), json(&expr));
}

#[test]
fn deep_conversion_clone_json_debug_and_drop_fit_small_stack() {
    const DEPTH: usize = 16_384;
    const STACK_BYTES: usize = 256 * 1024;

    std::thread::Builder::new()
        .name("rho-expr-small-stack".to_owned())
        .stack_size(STACK_BYTES)
        .spawn(|| {
            let mut input = par_of(ExprInstance::GInt(7));
            for depth in 0..DEPTH {
                input = if depth % 2 == 0 {
                    par_of(ExprInstance::ENegBody(ENeg { p: Some(input) }))
                } else {
                    par_of(ExprInstance::EMapBody(EMap {
                        kvs: vec![KeyValuePair {
                            key: Some(par_of(ExprInstance::GString("key".to_owned()))),
                            value: Some(input),
                        }],
                        ..Default::default()
                    }))
                };
            }

            let value = rho_expr_pda::from_par(input).expect("nested expression must convert");
            let cloned = value.clone();
            let encoded = json(&cloned);
            let debug = format!("{value:?}");
            assert_eq!(encoded, debug);
            assert!(encoded.len() > DEPTH * 20);
            drop(cloned);
            drop(value);
        })
        .expect("small-stack test thread must spawn")
        .join()
        .expect("stack-safe RhoExpr lifecycle must not overflow");
}
