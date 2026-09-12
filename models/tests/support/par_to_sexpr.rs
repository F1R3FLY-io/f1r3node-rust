use super::*;

#[test]
fn byte_native_populated_method_and_binds_preserve_selected_fields_and_ignore_metadata() {
    let method = expression(Some(ExprInstance::EMethodBody(EMethod {
        method_name: "λ.method".to_owned(),
        target: Some(integer(7)),
        arguments: vec![
            integer(-2),
            expression(Some(ExprInstance::GString("雪".to_owned()))),
        ],
        locally_free: vec![1, 2],
        connective_used: true,
    })));
    assert_projection(&method, "(7.λ.method -2 \"雪\")");

    let mut receive = Receive {
        binds: vec![
            ReceiveBind {
                source: Some(integer(7)),
                ..Default::default()
            },
            ReceiveBind {
                source: Some(integer(3)),
                ..Default::default()
            },
        ],
        body: Some(integer(9)),
        ..Default::default()
    };
    let original = par_from_default! { receives: vec![receive.clone()], ..Default::default() };
    let expected = decoded_oracle::DecodedSexprOracle::par_to_sexpr(&original);
    assert_eq!(ParToSExpr::par_to_sexpr(&original), expected);
    for bind in &mut receive.binds {
        bind.patterns = vec![integer(21)];
        bind.remainder = Some(Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        });
        bind.free_count = 1;
    }
    receive.condition = Some(integer(88));
    receive.persistent = true;
    receive.peek = true;
    receive.bind_count = 2;
    receive.locally_free = vec![1];
    receive.connective_used = true;
    let changed = par_from_default! { receives: vec![receive], ..Default::default() };
    assert_projection(&changed, &expected);
    let nested = expression(Some(ExprInstance::ETupleBody(ETuple {
        ps: vec![method, changed],
        ..Default::default()
    })));
    let expected_nested = decoded_oracle::DecodedSexprOracle::par_to_sexpr(&nested);
    assert_eq!(ParToSExpr::par_to_sexpr(&nested), expected_nested);

    let new = New {
        bind_count: 2,
        p: Some(integer(4)),
        uri: vec!["rho:unused".to_owned()],
        injections: [("unused".to_owned(), integer(99))].into_iter().collect(),
        locally_free: vec![1, 2],
    };
    let par = par_from_default! { news: vec![new], ..Default::default() };
    assert_projection(&par, "(new x0 x1 4)");
}

thread_local! {
    static COMPONENT_DECODES: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn record_component_decode() {
    COMPONENT_DECODES.with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

fn count_decodes<T>(action: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { COMPONENT_DECODES.with(|count| count.set(None)); }
    }
    COMPONENT_DECODES.with(|count| {
        assert!(count.replace(Some(0)).is_none());
    });
    let _reset = Reset;
    let result = action();
    let count = COMPONENT_DECODES.with(|count| count.get().unwrap());
    (result, count)
}

fn delimited(tag: u32, payload: &[u8], out: &mut Vec<u8>) {
    prost::encoding::encode_key(tag, prost::encoding::WireType::LengthDelimited, out);
    prost::encoding::encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

fn direct_render(bytes: &[u8], kind: Kind) -> Renderer {
    let mut renderer = Renderer::default();
    renderer.render(bytes, kind);
    renderer
}

#[test]
fn byte_native_reuses_endpoint_storage_and_repeats_only_the_rendered_occurrence() {
    use crate::rust::par_bag::Multiplicity;

    let mut par = Par::default();
    for (value, copies) in [
        (ExprInstance::GString("λ".repeat(130)), 3),
        (
            ExprInstance::EListBody(EList {
                ps: vec![
                    integer(-7),
                    expression(Some(ExprInstance::GString("雪".to_owned()))),
                ],
                ..Default::default()
            }),
            2,
        ),
    ] {
        par.insert_component_many(
            &Expr {
                expr_instance: Some(value),
            },
            Multiplicity::new(copies).unwrap(),
            Vec::new(),
            false,
        )
        .unwrap();
    }
    let snapshot = par.clone();
    let ((expected, _), decoded) = count_decodes(|| {
        (
            decoded_oracle::DecodedSexprOracle::par_to_sexpr(&par),
            par.cardinality(),
        )
    });
    assert!(
        decoded > 0,
        "the frozen decoder is a positive observation control"
    );
    let (actual, decoded) = count_decodes(|| ParToSExpr::par_to_sexpr(&par));
    assert_eq!(actual, expected);
    assert_eq!(decoded, 0);
    assert_eq!(ParToSExpr::par_to_sexpr(&snapshot), expected);
    let mut renderer = Renderer::default();
    renderer.family::<Expr>(&par, Kind::Expr, &mut true);
    assert_eq!(renderer.endpoints, 2);
    assert!(renderer.work.is_empty());
    assert_eq!(format!("(par {})", renderer.out), expected);
}

#[test]
fn byte_native_repeated_fields_have_width_independent_pending_work_and_bounded_reads() {
    use crate::rust::rholang::protobuf_cursor::par_count_header_work::measure;
    use crate::rust::rholang::protobuf_encoder;

    let mut pending = None;
    for width in [1, 16, 256] {
        let expr = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: (0..width)
                    .map(|index| {
                        expression(Some(ExprInstance::GString(format!(
                            "{index}:{}",
                            "λ".repeat(70)
                        ))))
                    })
                    .collect(),
                locally_free: vec![1; 129],
                connective_used: true,
                remainder: None,
            })),
        };
        let bytes = protobuf_encoder::encode_to_vec(&expr);
        let (renderer, reads) = measure(|| direct_render(&bytes, Kind::Expr));
        let expected = decoded_oracle::DecodedSexprOracle::par_to_sexpr(&expression(
            expr.expr_instance.clone(),
        ));
        assert_eq!(renderer.out, expected);
        assert!(renderer.work.is_empty());
        assert!(
            reads.maximum() <= 4,
            "maximum header visits: {}",
            reads.maximum()
        );
        assert!(reads.total() <= bytes.len() * 4);
        match pending {
            None => pending = Some(renderer.maximum_pending),
            Some(first) => assert!(renderer.maximum_pending <= first + 1),
        }
        eprintln!(
            "sexpr width={width} header_reads={} maximum_reads={} pending={}",
            reads.total(),
            reads.maximum(),
            renderer.maximum_pending
        );
    }
}

#[test]
fn byte_native_deep_offsets_are_stack_safe_and_read_headers_a_bounded_number_of_times() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            use crate::rust::rholang::protobuf_cursor::par_count_header_work::measure;
            let depth = 4096;
            let mut bytes = vec![0x10, 0x02];
            for _ in 0..depth {
                let mut par = Vec::new();
                delimited(5, &bytes, &mut par);
                let mut list = Vec::new();
                delimited(1, &par, &mut list);
                bytes.clear();
                delimited(20, &list, &mut bytes);
            }
            let ((renderer, reads), decodes) =
                count_decodes(|| measure(|| direct_render(&bytes, Kind::Expr)));
            assert_eq!(
                renderer.out,
                format!("{}1{}", "[".repeat(depth), "]".repeat(depth))
            );
            assert_eq!(decodes, 0);
            assert!(reads.maximum() <= 4);
            assert!(reads.total() <= bytes.len() * 4);
            assert!(renderer.maximum_pending <= depth + 4);
            assert!(renderer.work.is_empty());
            eprintln!(
                "sexpr depth={depth} bytes={} header_reads={} maximum_reads={} pending={}",
                bytes.len(),
                reads.total(),
                reads.maximum(),
                renderer.maximum_pending
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn byte_native_field_view_premises_have_nonvacuous_negative_controls() {
    use prost::Message;
    let duplicated = [0x08, 0x02, 0x08, 0x04];
    let selected = field(
        &duplicated,
        Span {
            start: 0,
            end: duplicated.len(),
        },
        1,
    )
    .unwrap();
    assert_eq!(signed(selected.expect_varint().unwrap()), 1);
    assert_eq!(New::decode(duplicated.as_slice()).unwrap().bind_count, 2);

    let mut split = Vec::new();
    delimited(1, b"a", &mut split);
    delimited(2, b"", &mut split);
    delimited(1, b"b", &mut split);
    let span = Span {
        start: 0,
        end: split.len(),
    };
    let contiguous = run(&split, span, 1);
    let collect = |span| {
        let mut cursor = fields(&split, span);
        let mut selected = Vec::new();
        while let Some(value) = next_field(&mut cursor) {
            if value.number == 1 {
                selected.push(value.expect_length_delimited().unwrap());
            }
        }
        selected
    };
    assert_eq!(collect(contiguous), vec![b"a".as_slice()]);
    assert_eq!(collect(span), vec![b"a".as_slice(), b"b".as_slice()]);
}

#[test]
fn byte_native_family_mapping_metadata_and_cardinality_are_exact() {
    assert_eq!(classify([u64::MAX, u64::MAX, 1, 0, 0]), 2);
    assert_eq!(classify([0, 0, 0, 0, 0]), 0);
    assert_eq!(classify([0, 0, 1, 0, 0]), 1);
    let par = par_from_default! {
        sends: vec![Send { chan: Some(integer(1)), data: vec![integer(2)], ..Default::default() }],
        receives: vec![Receive { body: Some(integer(3)), ..Default::default() }],
        news: vec![New { bind_count: 2, p: Some(integer(4)), ..Default::default() }],
        exprs: vec![Expr { expr_instance: Some(ExprInstance::GInt(5)) }],
        bundles: vec![Bundle { body: Some(integer(6)), ..Default::default() }],
        ..Default::default()
    };
    let expected = decoded_oracle::DecodedSexprOracle::par_to_sexpr(&par);
    assert_eq!(
        expected,
        "(par (! 1 2) (for () 3) (new x0 x1 4) 5 (bundle 6))"
    );
    assert_eq!(ParToSExpr::par_to_sexpr(&par), expected);
    let nested = expression(Some(ExprInstance::EListBody(EList {
        ps: vec![par],
        ..Default::default()
    })));
    assert_projection(&nested, &format!("[{expected}]"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn byte_native_ordered_nested_fields_match_the_decoded_reference(
        values in prop::collection::vec(any::<i64>(), 0..20),
        bind_count in -3i32..5,
        metadata in prop::collection::vec(any::<u8>(), 0..12),
    ) {
        let par = par_from_default! {
            news: vec![New {
                bind_count,
                p: Some(expression(Some(ExprInstance::ETupleBody(ETuple {
                    ps: values.into_iter().map(integer).collect(),
                    locally_free: metadata.clone(),
                    connective_used: !metadata.is_empty(),
                })))),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (actual, decodes) = count_decodes(|| ParToSExpr::par_to_sexpr(&par));
        prop_assert_eq!(decodes, 0);
        prop_assert_eq!(actual, decoded_oracle::DecodedSexprOracle::par_to_sexpr(&par));
    }
}
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;

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
    use prost::encoding::{encode_key, encode_varint, WireType};
    use prost::Message;

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
