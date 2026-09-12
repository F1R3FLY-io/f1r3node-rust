use super::*;

#[test]
fn test_simple_int() {
    let par = par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(42)),
        }],
        ..Default::default()
    };
    assert_eq!(ParToSExpr::par_to_sexpr(&par), "42");
}

#[test]
fn test_simple_string() {
    let par = par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GString("hello".to_string())),
        }],
        ..Default::default()
    };
    assert_eq!(ParToSExpr::par_to_sexpr(&par), "\"hello\"");
}

#[test]
fn test_list() {
    let par = par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(crate::rhoapi::EList {
                ps: vec![
                    par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GString("a".to_string())),
                        }],
                        ..Default::default()
                    },
                    par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GString("b".to_string())),
                        }],
                        ..Default::default()
                    },
                ],
                locally_free: vec![],
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Default::default()
    };
    assert_eq!(ParToSExpr::par_to_sexpr(&par), "[\"a\" \"b\"]");
}

#[test]
fn depth_256_uses_a_fixed_128_kib_native_stack() {
    std::thread::Builder::new()
        .name("par-to-sexpr-pda".to_owned())
        .stack_size(128 * 1024)
        .spawn(|| {
            // A recursive formatter exhausts 128 KiB well before this
            // depth. The dedicated protobuf and bincode gates carry the
            // 4,096-level corpus; repeating that rung here mostly measures
            // construction of every immutable encoded prefix in an
            // unoptimized unit-test build.
            let depth = 256usize;
            let mut par = par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                }],
                ..Default::default()
            };
            for _ in 0..depth {
                par = par_from_default! {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EListBody(crate::rhoapi::EList {
                            ps: vec![par],
                            locally_free: Vec::new(),
                            connective_used: false,
                            remainder: None,
                        })),
                    }],
                    ..Default::default()
                };
            }

            let rendered = ParToSExpr::par_to_sexpr(&par);
            assert_eq!(rendered.len(), depth * 2 + 1);
            assert!(rendered.starts_with("[[[["));
            assert!(rendered.ends_with("]]]]"));
        })
        .expect("spawn fixed-stack ParToSExpr probe")
        .join()
        .expect("ParToSExpr must not grow the native stack with AST depth");
}

#[path = "decoded_sexpr_oracle.rs"]
mod decoded_oracle;

use proptest::prelude::*;

use crate::rhoapi::*;

fn expression(instance: Option<ExprInstance>) -> Par {
    par_from_default! {
        exprs: vec![Expr { expr_instance: instance }],
        ..Default::default()
    }
}

fn integer(value: i64) -> Par { expression(Some(ExprInstance::GInt(value))) }

fn assert_projection(par: &Par, expected: &str) {
    assert_eq!(
        decoded_oracle::DecodedSexprOracle::par_to_sexpr(par),
        expected
    );
    assert_eq!(ParToSExpr::par_to_sexpr(par), expected);
}

#[test]
fn decoded_baseline_covers_formal_constructor_observations() {
    let mut cases: Vec<(&str, Par, &str)> = vec![
        ("ParZero", Par::default(), "Nil"),
        ("ParOne", integer(1), "1"),
        ("MissingExprCase", expression(None), "Nil"),
        (
            "BoolCase",
            expression(Some(ExprInstance::GBool(false))),
            "false",
        ),
        ("IntCase", integer(i64::MIN), "-9223372036854775808"),
        (
            "StringCase",
            expression(Some(ExprInstance::GString("a\"λ".to_owned()))),
            "\"a\"λ\"",
        ),
        (
            "UriCase",
            expression(Some(ExprInstance::GUri("rho:`λ".to_owned()))),
            "`rho:`λ`",
        ),
        (
            "BytesCase",
            expression(Some(ExprInstance::GByteArray(vec![0, 1, 255]))),
            "0x0001ff",
        ),
        (
            "ListCase",
            expression(Some(ExprInstance::EListBody(EList {
                ps: vec![integer(2), integer(1)],
                ..Default::default()
            }))),
            "[2 1]",
        ),
        (
            "TupleCase",
            expression(Some(ExprInstance::ETupleBody(ETuple::default()))),
            "(tuple )",
        ),
        (
            "SetCase",
            expression(Some(ExprInstance::ESetBody(ESet::default()))),
            "(set )",
        ),
        (
            "MapCase",
            expression(Some(ExprInstance::EMapBody(EMap::default()))),
            "(map )",
        ),
        (
            "PairCase",
            expression(Some(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair::default()],
                ..Default::default()
            }))),
            "(map (Nil : Nil))",
        ),
        (
            "MissingVarCase",
            expression(Some(ExprInstance::EVarBody(EVar::default()))),
            "var",
        ),
        (
            "NegCase",
            expression(Some(ExprInstance::ENegBody(ENeg::default()))),
            "(- Nil)",
        ),
        (
            "NotCase",
            expression(Some(ExprInstance::ENotBody(ENot::default()))),
            "(not Nil)",
        ),
        (
            "MultCase",
            expression(Some(ExprInstance::EMultBody(EMult {
                p1: Some(integer(2)),
                p2: Some(integer(1)),
            }))),
            "(* 2 1)",
        ),
        (
            "DivCase",
            expression(Some(ExprInstance::EDivBody(EDiv {
                p1: Some(integer(2)),
                p2: Some(integer(1)),
            }))),
            "(/ 2 1)",
        ),
        (
            "PlusCase",
            expression(Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(integer(2)),
                p2: Some(integer(1)),
            }))),
            "(+ 2 1)",
        ),
        (
            "MinusCase",
            expression(Some(ExprInstance::EMinusBody(EMinus {
                p1: Some(integer(2)),
                p2: Some(integer(1)),
            }))),
            "(- 2 1)",
        ),
        (
            "MethodCase",
            expression(Some(ExprInstance::EMethodBody(EMethod::default()))),
            "(Nil. )",
        ),
        (
            "FallbackExprCase",
            expression(Some(ExprInstance::EModBody(EMod::default()))),
            "(expr)",
        ),
    ];
    cases.push((
        "ParMany",
        par_from_default! {
            exprs: vec![Expr { expr_instance: Some(ExprInstance::GInt(1)) }; 2],
            ..Default::default()
        },
        "(par 1 1)",
    ));
    cases.push(("SendCase", par_from_default! {
        sends: vec![Send { chan: Some(integer(2)), data: vec![integer(1), integer(3)], ..Default::default() }],
        ..Default::default()
    }, "(! 2 1 3)"));
    cases.push((
        "MissingParCase",
        par_from_default! {
            sends: vec![Send::default()], ..Default::default()
        },
        "(! Nil )",
    ));
    cases.push((
        "ReceiveCase",
        par_from_default! {
            receives: vec![Receive::default()], ..Default::default()
        },
        "(for () Nil)",
    ));
    cases.push((
        "BindCase",
        par_from_default! {
            receives: vec![Receive { binds: vec![ReceiveBind::default()], ..Default::default() }],
            ..Default::default()
        },
        "(for ((bind <- Nil)) Nil)",
    ));
    cases.push((
        "NewCase",
        par_from_default! {
            news: vec![New::default()], ..Default::default()
        },
        "(new  Nil)",
    ));
    cases.push((
        "BundleCase",
        par_from_default! {
            bundles: vec![Bundle::default()], ..Default::default()
        },
        "(bundle Nil)",
    ));
    for (name, variant, expected) in [
        ("BoundCase", VarInstance::BoundVar(-2), "_-2"),
        ("FreeCase", VarInstance::FreeVar(i32::MIN), "$-2147483648"),
        (
            "WildcardCase",
            VarInstance::Wildcard(crate::rhoapi::var::WildcardMsg {}),
            "_",
        ),
    ] {
        cases.push((
            name,
            expression(Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(variant),
                }),
            }))),
            expected,
        ));
    }
    cases.push((
        "IgnoredFamilyCase",
        par_from_default! {
            matches: vec![Match::default()],
            unforgeables: vec![GUnforgeable::default()],
            connectives: vec![Connective::default()],
            conditionals: vec![If::default()],
            ..Default::default()
        },
        "Nil",
    ));
    assert_eq!(cases.len(), 33);
    let mut names = std::collections::BTreeSet::new();
    for (name, par, expected) in cases {
        assert!(names.insert(name));
        assert_eq!(
            decoded_oracle::DecodedSexprOracle::par_to_sexpr(&par),
            expected,
            "{name}"
        );
        assert_eq!(ParToSExpr::par_to_sexpr(&par), expected, "{name}");
    }
}

#[test]
fn decoded_baseline_preserves_empty_and_signed_binder_forms() {
    for (count, expected) in [
        (-2, "(new  Nil)"),
        (0, "(new  Nil)"),
        (2, "(new x0 x1 Nil)"),
    ] {
        let par = par_from_default! {
            news: vec![New { bind_count: count, ..Default::default() }],
            ..Default::default()
        };
        assert_projection(&par, expected);
    }
    assert_projection(&integer(i64::MAX), "9223372036854775807");
    assert_projection(
        &expression(Some(ExprInstance::GByteArray(Vec::new()))),
        "0x",
    );
    assert_projection(
        &expression(Some(ExprInstance::EListBody(EList::default()))),
        "[]",
    );
    assert_projection(
        &expression(Some(ExprInstance::EVarBody(EVar {
            v: Some(Var::default()),
        }))),
        "var",
    );
}

#[test]
fn decoded_baseline_preserves_every_generated_fallback_arm() {
    use prost::Message;
    use prost::encoding::{WireType, encode_key, encode_varint};

    use crate::rust::rholang::protobuf_schema::ProtobufKind;
    use crate::rust::rholang::protobuf_schema_tables::EXPR_INSTANCE_PROTOBUF_VARIANTS;

    let mut tested = std::collections::BTreeSet::new();
    for field in EXPR_INSTANCE_PROTOBUF_VARIANTS {
        if matches!(field.tag, 1..=10 | 19..=25) {
            continue;
        }
        let mut bytes = Vec::new();
        match field.kind {
            ProtobufKind::Message | ProtobufKind::Bytes => {
                encode_key(field.tag, WireType::LengthDelimited, &mut bytes);
                encode_varint(0, &mut bytes);
            }
            ProtobufKind::Fixed64 => {
                encode_key(field.tag, WireType::SixtyFourBit, &mut bytes);
                bytes.extend_from_slice(&0u64.to_le_bytes());
            }
            kind => panic!(
                "add a canonical fixture for fallback {}: {kind:?}",
                field.name
            ),
        }
        let expr = Expr::decode(bytes.as_slice()).expect("generated fallback field fixture");
        let par = par_from_default! { exprs: vec![expr], ..Default::default() };
        assert_projection(&par, "(expr)");
        assert!(tested.insert(field.tag));
    }
    assert!(tested.len() >= 19);
    assert!(tested.contains(&32));
    assert!(tested.contains(&33));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn decoded_baseline_matches_generated_shallow_terms(
        par in crate::rust::test_utils::test_utils::generate_par(1),
    ) {
        prop_assert_eq!(
            ParToSExpr::par_to_sexpr(&par),
            decoded_oracle::DecodedSexprOracle::par_to_sexpr(&par)
        );
    }
}
