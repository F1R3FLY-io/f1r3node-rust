// P3b tests of the normalizer arms of the Reified RSpaces syntax (plan decisions 18, 21, 23
// and 27, P3 audit §4).
//
// Each arm is a thin adapter over the frontend-neutral builders of `models::rust::reified`.
// The tests normalize source text with the v3 parser and compare the result with the output
// of the builders. Where the parser rejects an input, a test builds the AST by hand and checks
// that the builders and the validators re-check the guarantee of the parser.

#[path = "../../models/tests/support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, HashMap};

use models::create_bit_vector;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, New, Par, Send};
use models::rust::reified::builders::{
    efree, new_space, push_process_expr, query_spec, send_params, use_block,
};
use models::rust::reified::encoding::{split_trailing_params, SendParams};
use models::rust::reified::params::SendParam;
use models::rust::reified::validation::{IdentifierPosition, ReifiedSyntaxError};
use models::rust::rholang::implicits::concatenate_pars;
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::RngSeed;
use prost::Message;
use reified_strategies::{
    bound_var, contains_marker, expr_par, gint, gstring, list, new_term, send, sorted, tuple,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::compiler::normalize::{normalize_ann_proc, ProcVisitInputs};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::util::prepend_new;
use rholang_parser::ast::{
    AnnProc, Bind, Hyperparam, HyperparamList, Id, Name, NameDecl, Names, Proc, QuerySpec,
    SendType, Source, Uri, Var,
};
use rholang_parser::{RholangParser, SourcePos, SourceSpan};
use smallvec::{smallvec, SmallVec};

const PROPERTY_SEED: u64 = 0x0E0D_2026_1005_0301;

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn compile(source: &str) -> Par {
    Compiler::source_to_adt(source)
        .unwrap_or_else(|error| panic!("{source} must normalize, but failed with {error:?}"))
}

fn compile_error(source: &str) -> InterpreterError {
    match Compiler::source_to_adt(source) {
        Ok(par) => panic!("{source} must be rejected, but normalized to {par:?}"),
        Err(error) => error,
    }
}

fn prost(source: &str) -> Vec<u8> { compile(source).encode_to_vec() }

/// The normalizer error of a builder or validator rejection.
fn syntax_error(error: ReifiedSyntaxError) -> InterpreterError {
    InterpreterError::NormalizerError(format!("reified syntax: {error}"))
}

fn out_of_range(key: &str, expected: &'static str) -> InterpreterError {
    syntax_error(ReifiedSyntaxError::OutOfRange {
        key: key.to_owned(),
        expected,
    })
}

fn gdouble(value: f64) -> Par {
    expr_par(ExprInstance::GDouble(value.to_bits()), Vec::new(), false)
}

/// The process `Par` that holds exactly the marker tuple `tuple`.
fn process(tuple: ETuple) -> Par { push_process_expr(Par::default(), tuple) }

fn new_par(new: New) -> Par { prepend_new(Par::default(), new) }

fn persistent_send(chan: Par, data: Vec<Par>) -> Par {
    let mut par = send(chan, data);
    for send in &mut par.sends {
        send.persistent = true;
    }
    par
}

fn params(params: Vec<SendParam>) -> Par {
    send_params(params)
        .expect("the test params are well formed")
        .expect("the test params are not empty")
}

fn named(key: &str, value: Par) -> SendParam { SendParam::Named(key.to_owned(), value) }

fn sole_send(par: &Par) -> &Send {
    match par.sends.as_slice() {
        [send] => send,
        other => panic!("expected exactly one send, found {other:?}"),
    }
}

fn sole_new_body(par: &Par) -> &Par {
    match par.news.as_slice() {
        [New { p: Some(body), .. }] => body,
        other => panic!("expected exactly one new, found {other:?}"),
    }
}

fn sole_tuple(par: &Par) -> &ETuple {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::ETupleBody(tuple)),
        }] => tuple,
        other => panic!("expected exactly one tuple expression, found {other:?}"),
    }
}

/// The source of the single bind of the single receive of `par`.
fn sole_bind_source(par: &Par) -> &Par {
    match par.receives.as_slice() {
        [receive] => match receive.binds.as_slice() {
            [bind] => bind.source.as_ref().expect("a receive bind has a source"),
            other => panic!("expected exactly one bind, found {other:?}"),
        },
        other => panic!("expected exactly one receive, found {other:?}"),
    }
}

fn span() -> SourceSpan { SourceSpan::default() }

fn id(name: &str) -> Id<'_> {
    Id {
        name,
        pos: SourcePos::default(),
    }
}

fn quoted_string<'ast>(parser: &'ast RholangParser<'ast>, value: &'ast str) -> Name<'ast> {
    Name::Quote(parser.ast_builder().alloc_string_literal(value).ann(span()))
}

fn long<'ast>(parser: &'ast RholangParser<'ast>, value: i64) -> AnnProc<'ast> {
    parser.ast_builder().alloc_long_literal(value).ann(span())
}

fn normalize_ast<'ast>(
    proc: AnnProc<'ast>,
    parser: &'ast RholangParser<'ast>,
) -> Result<Par, InterpreterError> {
    normalize_ann_proc(&proc, ProcVisitInputs::new(), &HashMap::new(), parser)
        .map(|output| sorted(output.par))
}

/// `@"x"!(1; params)`, built by hand so that it can hold what the parser rejects.
fn hand_built_send<'ast>(
    parser: &'ast RholangParser<'ast>,
    params: HyperparamList<'ast>,
) -> Result<Par, InterpreterError> {
    let send = parser.ast_builder().alloc_send_with_params(
        SendType::Single,
        quoted_string(parser, "x"),
        &[long(parser, 1)],
        params,
    );
    normalize_ast(send.ann(span()), parser)
}

/// `for (y <- source ~ query) { Nil }`, built by hand.
fn hand_built_query<'ast>(
    parser: &'ast RholangParser<'ast>,
    source: Source<'ast>,
    query: QuerySpec<'ast>,
) -> Result<Par, InterpreterError> {
    let bind = Bind::Linear {
        lhs: Names::single(Name::NameVar(Var::Id(id("y")))),
        rhs: source,
        query_spec: Some(Box::new(query)),
    };
    let body = parser.ast_builder().const_nil().ann(span());
    let receive = parser.ast_builder().alloc_for(vec![vec![bind]], body);
    normalize_ast(receive.ann(span()), parser)
}

// ---------------------------------------------------------------------------
// `use s { P }`
// ---------------------------------------------------------------------------

#[test]
fn p_use_block_builds_the_use_tuple() {
    let body = send(gstring("x"), vec![gint(1)]);
    assert_eq!(
        compile(r#"use @"space" { @"x"!(1) }"#),
        sorted(process(use_block(gstring("space"), body)))
    );
}

#[test]
fn p_use_block_space_and_body_see_the_current_scope() {
    let par = compile("new s in { use s { s!(1) } }");
    let body = send(bound_var(0), vec![gint(1)]);
    let tuple = use_block(bound_var(0), body);
    assert_eq!(
        par,
        sorted(new_par(new_term(1, Vec::new(), process(tuple.clone()))))
    );
    let normalized = sole_tuple(sole_new_body(&par));
    assert_eq!(normalized, &tuple);
    assert_eq!(normalized.locally_free, create_bit_vector(&[0]));
}

#[test]
fn p_use_block_nests_and_composes_in_par() {
    let inner = process(use_block(bound_var(0), send(gstring("x"), vec![gint(1)])));
    let outer = process(use_block(bound_var(1), inner));
    let body = concatenate_pars(outer, send(gstring("y"), vec![gint(2)]));
    assert_eq!(
        compile(r#"new s, t in { use s { use t { @"x"!(1) } } | @"y"!(2) }"#),
        sorted(new_par(new_term(2, Vec::new(), body)))
    );
}

#[test]
fn p_use_block_identifier_use_is_an_ordinary_name() {
    let keyword_name = compile("new use in { use!(1) | for (x <- use) { Nil } }");
    assert!(!contains_marker(&keyword_name));
    assert_eq!(
        keyword_name.encode_to_vec(),
        prost("new u in { u!(1) | for (x <- u) { Nil } }")
    );
}

// ---------------------------------------------------------------------------
// `new x : s in P`
// ---------------------------------------------------------------------------

#[test]
fn normalize_new_space_annotations_follow_the_binding_order() {
    let par = compile(
        r#"new s in { new a, b : s, out(`rho:io:stdout`), c : @"t" in { b!(1) | c!(2) } }"#,
    );
    let body = concatenate_pars(
        send(bound_var(2), vec![gint(1)]),
        send(bound_var(1), vec![gint(2)]),
    );
    let inner = new_term(3, vec!["rho:io:stdout".to_owned()], sorted(body));
    let tuple = new_space(inner, vec![
        None,
        Some(bound_var(0)),
        Some(gstring("t")),
        None,
    ])
    .expect("the binding-order annotations are well formed");
    assert_eq!(
        par,
        sorted(new_par(new_term(1, Vec::new(), process(tuple))))
    );
}

#[test]
fn normalize_new_space_annotation_names_bind_outside_the_new() {
    let par = compile("new x in { new x : x in { x!(1) } }");
    let inner = new_term(1, Vec::new(), send(bound_var(0), vec![gint(1)]));
    let tuple =
        new_space(inner, vec![Some(bound_var(0))]).expect("one annotated binder is well formed");
    assert_eq!(
        par,
        sorted(new_par(new_term(1, Vec::new(), process(tuple))))
    );
    assert_eq!(
        sole_tuple(sole_new_body(&par)).locally_free,
        create_bit_vector(&[0])
    );
    assert!(matches!(
        compile_error("new x : x in { x!(1) }"),
        InterpreterError::TopLevelFreeVariablesNotAllowedError(_)
    ));
}

#[test]
fn normalize_new_space_unannotated_new_is_ordinary_and_uri_annotation_is_rejected() {
    let ordinary = compile("new x, out(`rho:io:stdout`) in { x!(1) | out!(2) }");
    assert!(!contains_marker(&ordinary));
    let body = concatenate_pars(
        send(bound_var(1), vec![gint(1)]),
        send(bound_var(0), vec![gint(2)]),
    );
    assert_eq!(
        ordinary,
        sorted(new_par(new_term(1, vec!["rho:io:stdout".to_owned()], body)))
    );

    let parser = RholangParser::new();
    let body = parser.ast_builder().const_nil().ann(span());
    let uri_annotated = vec![
        NameDecl {
            id: id("x"),
            space: Some(quoted_string(&parser, "s")),
            uri: None,
        },
        NameDecl {
            id: id("out"),
            space: Some(quoted_string(&parser, "s")),
            uri: Some(Uri::from("rho:io:stdout")),
        },
    ];
    let new = parser.ast_builder().alloc_new(body, uri_annotated);
    assert_eq!(
        normalize_ast(new.ann(span()), &parser),
        Err(syntax_error(ReifiedSyntaxError::SpaceTypeOnUriBinding))
    );
}

// ---------------------------------------------------------------------------
// `free Th(args)`
// ---------------------------------------------------------------------------

#[test]
fn normalize_free_theory_builds_the_free_value() {
    let nat = process(efree("Nat", Vec::new()).expect("Nat is a theory name"));
    let theory = process(efree("Th", vec![gint(1), bound_var(0)]).expect("Th is a theory name"));
    assert_eq!(
        compile("new f in { f!(free Nat(), free Th(1, *f)) }"),
        sorted(new_par(new_term(
            1,
            Vec::new(),
            send(bound_var(0), vec![nat.clone(), theory])
        )))
    );
    assert_eq!(
        compile(r#"@"x"!(1) | free Nat()"#),
        sorted(concatenate_pars(send(gstring("x"), vec![gint(1)]), nat))
    );
    for builtin in ["Nat", "Int", "String", "Bool", "Any"] {
        let value = process(efree(builtin, Vec::new()).expect("a builtin theory name"));
        assert_eq!(
            compile(&format!(r#"@"x"!(free {builtin}())"#)),
            sorted(send(gstring("x"), vec![value]))
        );
    }
}

#[test]
fn normalize_free_theory_rejects_names_outside_the_theory_position() {
    let parser = RholangParser::new();
    for name in ["true", "in", "new", "1x", "_"] {
        let theory = parser.ast_builder().alloc_free_theory(id(name), &[]);
        assert_eq!(
            normalize_ast(theory.ann(span()), &parser),
            Err(syntax_error(ReifiedSyntaxError::InvalidIdentifier {
                name: name.to_owned(),
                position: IdentifierPosition::TheoryName,
            })),
            "the theory name {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Send params `x!(d…; p…, k=v…)`
// ---------------------------------------------------------------------------

#[test]
fn normalize_send_params_build_the_trailing_params_tuple() {
    let par = compile(r#"@"x"!(1; 2, k=3)"#);
    let trailing = params(vec![SendParam::Positional(gint(2)), named("k", gint(3))]);
    assert_eq!(par, sorted(send(gstring("x"), vec![gint(1), trailing])));
    assert_eq!(
        split_trailing_params(&sole_send(&par).data),
        Ok(Some(SendParams {
            positional: vec![gint(2)],
            named: BTreeMap::from([("k".to_owned(), gint(3))]),
        }))
    );
}

fn arb_named_params() -> BoxedStrategy<Vec<(String, i64)>> {
    btree_map(
        select(vec![
            "priority", "k", "a", "zeta", "catch", "in", "x1", "_w", "B'",
        ]),
        1i64..100,
        1..5,
    )
    .prop_map(|entries| {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<Vec<_>>()
    })
    .prop_flat_map(|entries| Just(entries.clone()).prop_shuffle())
    .boxed()
}

fn render_send(positional: &[i64], named: &[(String, i64)]) -> String {
    let params: Vec<String> = positional
        .iter()
        .map(i64::to_string)
        .chain(named.iter().map(|(key, value)| format!("{key}={value}")))
        .collect();
    format!(r#"@"x"!(0; {})"#, params.join(", "))
}

proptest! {
    #![proptest_config(property_config())]

    /// The order of the named params in the source text does not change the normalized send:
    /// the builder sorts the keys bytewise (plan decision 21).
    #[test]
    fn normalize_send_params_key_permutation_invariant(
        positional in vec(0i64..100, 0..3),
        shuffled in arb_named_params(),
    ) {
        let mut sorted_named = shuffled.clone();
        sorted_named.sort();
        let from_shuffled = prost(&render_send(&positional, &shuffled));
        prop_assert_eq!(&from_shuffled, &prost(&render_send(&positional, &sorted_named)));
        let mut built = Vec::with_capacity(positional.len() + shuffled.len());
        built.extend(positional.iter().map(|value| SendParam::Positional(gint(*value))));
        built.extend(shuffled.iter().map(|(key, value)| named(key, gint(*value))));
        let expected = sorted(send(gstring("x"), vec![gint(0), params(built)]));
        prop_assert_eq!(from_shuffled, expected.encode_to_vec());
    }
}

#[test]
fn normalize_send_params_empty_section_is_erased() {
    let pairs = [
        (r#"@"x"!(1;)"#, r#"@"x"!(1)"#),
        (r#"@"x"!!(1;)"#, r#"@"x"!!(1)"#),
        (r#"@"x"!(;)"#, r#"@"x"!()"#),
        (r#"@"x"!?(1;)."#, r#"@"x"!?(1)."#),
        (r#"@"x"!m(1;)."#, r#"@"x"!m(1)."#),
        (
            r#"for (y <- @"x"!?(1;)) { Nil }"#,
            r#"for (y <- @"x"!?(1)) { Nil }"#,
        ),
        (
            r#"for (y <- @"x"!m(1;)) { Nil }"#,
            r#"for (y <- @"x"!m(1)) { Nil }"#,
        ),
    ];
    for (erased, ordinary) in pairs {
        let par = compile(erased);
        assert!(!contains_marker(&par), "{erased}");
        assert_eq!(par.encode_to_vec(), prost(ordinary), "{erased}");
    }
}

#[test]
fn normalize_send_params_params_only_send() {
    assert_eq!(
        compile(r#"@"x"!(; k=1)"#),
        sorted(send(gstring("x"), vec![params(vec![named("k", gint(1))])]))
    );
    assert_eq!(
        compile(r#"@"x"!(; 7)"#),
        sorted(send(gstring("x"), vec![params(vec![
            SendParam::Positional(gint(7))
        ])]))
    );
}

#[test]
fn normalize_send_params_persistent_send_keeps_its_params() {
    let par = compile(r#"@"x"!!(1; k=1)"#);
    assert_eq!(
        par,
        sorted(persistent_send(gstring("x"), vec![
            gint(1),
            params(vec![named("k", gint(1))])
        ]))
    );
    assert!(sole_send(&par).persistent);
}

#[test]
fn normalize_send_params_sync_send_appends_params_after_the_reply_name() {
    let par = compile(r#"@"x"!?(1; k=1)."#);
    assert_eq!(
        par.encode_to_vec(),
        prost(r#"new r in { @"x"!(*r, 1; k=1) | for (_ <- r) { Nil } }"#)
    );
    let reply_send = sole_send(sole_new_body(&par));
    assert_eq!(reply_send.data, vec![
        bound_var(0),
        gint(1),
        params(vec![named("k", gint(1))])
    ]);
    assert_eq!(
        prost(r#"@"x"!?(1; k=1); @"y"!(2)"#),
        prost(r#"new r in { @"x"!(*r, 1; k=1) | for (_ <- r) { @"y"!(2) } }"#)
    );
}

#[test]
fn normalize_send_params_method_sugar_carries_params() {
    assert_eq!(
        prost(r#"@"x"!m(1; k=1)."#),
        prost(r#"new r in { @"x"!(*r, "m", 1; k=1) | for (_ <- r) { Nil } }"#)
    );
    assert_eq!(
        prost(r#"@"x"!m(; 2)."#),
        prost(r#"new r in { @"x"!(*r, "m"; 2) | for (_ <- r) { Nil } }"#)
    );
}

#[test]
fn normalize_send_params_send_receive_sources_carry_params() {
    assert_eq!(
        prost(r#"for (y <- @"x"!?(1; k=1)) { y!(2) }"#),
        prost(r#"new r in { @"x"!(*r, 1; k=1) | for (y <- r) { y!(2) } }"#)
    );
    assert_eq!(
        prost(r#"for (y <- @"x"!m(1; k=1)) { y!(2) }"#),
        prost(r#"new r in { @"x"!(*r, "m", 1; k=1) | for (y <- r) { y!(2) } }"#)
    );
}

#[test]
fn normalize_send_params_values_bind_in_the_current_scope() {
    let par = compile("new x, v in { x!(1; k=*v) }");
    let trailing = params(vec![named("k", bound_var(0))]);
    assert_eq!(
        par,
        sorted(new_par(new_term(
            2,
            Vec::new(),
            send(bound_var(1), vec![gint(1), trailing.clone()])
        )))
    );
    let normalized = sole_send(sole_new_body(&par));
    assert_eq!(normalized.locally_free, create_bit_vector(&[0, 1]));
    let normalized_params = normalized
        .data
        .last()
        .expect("the send has a trailing params element");
    assert_eq!(normalized_params, &trailing);
    assert_eq!(normalized_params.locally_free, create_bit_vector(&[0]));
    assert_eq!(
        compile("new x, v in { x!(1; *v) }"),
        sorted(new_par(new_term(
            2,
            Vec::new(),
            send(bound_var(1), vec![
                gint(1),
                params(vec![SendParam::Positional(bound_var(0))])
            ])
        )))
    );
}

#[test]
fn normalize_send_params_recheck_rejects_invalid_params() {
    let parser = RholangParser::new();
    let named_param = |key: &'static str, value: i64| Hyperparam::Named {
        key: id(key),
        value: long(&parser, value),
    };
    let cases: Vec<(HyperparamList, ReifiedSyntaxError)> = vec![
        (
            smallvec![named_param("k", 1), named_param("k", 2)],
            ReifiedSyntaxError::DuplicateKey {
                key: "k".to_owned(),
            },
        ),
        (
            smallvec![
                named_param("k", 1),
                Hyperparam::Positional(long(&parser, 2))
            ],
            ReifiedSyntaxError::PositionalAfterNamed,
        ),
        (
            smallvec![named_param("true", 1)],
            ReifiedSyntaxError::InvalidIdentifier {
                name: "true".to_owned(),
                position: IdentifierPosition::ParamKey,
            },
        ),
        (
            smallvec![named_param("1", 1)],
            ReifiedSyntaxError::InvalidIdentifier {
                name: "1".to_owned(),
                position: IdentifierPosition::ParamKey,
            },
        ),
    ];
    for (params, error) in cases {
        assert_eq!(
            hand_built_send(&parser, params),
            Err(syntax_error(error.clone())),
            "{error}"
        );
    }

    assert_eq!(
        compile_error(r#"@"x"!(1; priority=-1)"#),
        out_of_range("priority", "an Int >= 0")
    );
    assert_eq!(
        compile_error(r#"@"x"!(1; k=0)"#),
        out_of_range("k", "an Int >= 1")
    );
    assert_eq!(
        compile_error(r#"@"x"!(1; results="some")"#),
        out_of_range("results", "the string \"all\"")
    );
    assert_eq!(
        compile_error(r#"@"x"!(1; metric="cos")"#),
        out_of_range(
            "metric",
            "one of cosine, dot, euclidean, manhattan, hamming, jaccard"
        )
    );
    let deferred = compile(r#"new v in { @"x"!(1; priority=*v) }"#);
    assert!(contains_marker(&deferred));
}

// ---------------------------------------------------------------------------
// Receive queries `for (y <- ch ~ q) P`
// ---------------------------------------------------------------------------

fn query(channel: Par, payload: Vec<Par>, params: Vec<SendParam>) -> Par {
    query_spec(channel, payload, params).expect("the test query is well formed")
}

#[test]
fn normalize_receive_query_bare_payload_wraps_the_channel() {
    let par = compile(r#"for (y <- @"x" ~ 5) { Nil }"#);
    assert_eq!(
        sole_bind_source(&par),
        &query(gstring("x"), vec![gint(5)], Vec::new())
    );
    let mut unwrapped = par.clone();
    for receive in &mut unwrapped.receives {
        for bind in &mut receive.binds {
            bind.source = Some(gstring("x"));
        }
    }
    assert_eq!(unwrapped, compile(r#"for (y <- @"x") { Nil }"#));
}

#[test]
fn normalize_receive_query_group_without_params_equals_the_bare_form() {
    assert_eq!(
        prost(r#"for (y <- @"x" ~ (5;)) { Nil }"#),
        prost(r#"for (y <- @"x" ~ 5) { Nil }"#)
    );
    assert_eq!(
        prost(r#"for (y <- @"x" ~ ([1, 2];)) { Nil }"#),
        prost(r#"for (y <- @"x" ~ [1, 2]) { Nil }"#)
    );
}

#[test]
fn normalize_receive_query_tuple_without_semicolon_is_one_payload() {
    assert_eq!(
        sole_bind_source(&compile(r#"for (y <- @"x" ~ (1, 2)) { Nil }"#)),
        &query(
            gstring("x"),
            vec![tuple(vec![gint(1), gint(2)])],
            Vec::new()
        )
    );
    assert_eq!(
        sole_bind_source(&compile(r#"for (y <- @"x" ~ (1, 2;)) { Nil }"#)),
        &query(gstring("x"), vec![gint(1), gint(2)], Vec::new())
    );
}

#[test]
fn normalize_receive_query_named_params_and_params_only_groups() {
    let par = compile(
        r#"for (@doc <- @"db" ~ ([0.5, 0.25]; metric="cosine", threshold=0.8, k=3)) { Nil }"#,
    );
    assert_eq!(
        sole_bind_source(&par),
        &query(
            gstring("db"),
            vec![list(vec![gdouble(0.5), gdouble(0.25)])],
            vec![
                named("metric", gstring("cosine")),
                named("threshold", gdouble(0.8)),
                named("k", gint(3)),
            ]
        )
    );
    assert_eq!(
        sole_bind_source(&compile(
            r#"for (@task <- @"tasks" ~ (; priority=0)) { Nil }"#
        )),
        &query(gstring("tasks"), Vec::new(), vec![named(
            "priority",
            gint(0)
        )])
    );
}

#[test]
fn normalize_receive_query_binds_in_the_scope_outside_the_receive() {
    let par = compile(r#"new y in { for (y <- @"x" ~ *y) { y!(1) } }"#);
    let body = sole_new_body(&par);
    assert_eq!(
        sole_bind_source(body),
        &query(gstring("x"), vec![bound_var(0)], Vec::new())
    );
    let receive = body.receives.first().expect("the body is one receive");
    assert_eq!(
        receive.body,
        Some(sorted(send(bound_var(0), vec![gint(1)])))
    );
    assert_eq!(receive.locally_free, create_bit_vector(&[0]));
    let plain = compile(r#"new y in { for (y <- @"x") { y!(1) } }"#);
    let plain_receive = sole_new_body(&plain)
        .receives
        .first()
        .expect("the body is one receive");
    assert!(plain_receive.locally_free.is_empty());
    assert!(matches!(
        compile_error(r#"for (y <- @"x" ~ *y) { Nil }"#),
        InterpreterError::TopLevelFreeVariablesNotAllowedError(_)
    ));
}

#[test]
fn normalize_receive_query_composes_with_joins_guards_and_receipts() {
    let join = compile(r#"for (@a <- @"x" ~ 1 & @b <- @"y" where a == b) { Nil }"#);
    let receive = join.receives.first().expect("one receive");
    assert!(receive.condition.is_some());
    let mut sources: Vec<Par> = receive
        .binds
        .iter()
        .map(|bind| bind.source.clone().expect("a bind has a source"))
        .collect();
    let mut expected = vec![
        query(gstring("x"), vec![gint(1)], Vec::new()),
        sorted(gstring("y")),
    ];
    sources.sort_by_key(Message::encode_to_vec);
    expected.sort_by_key(Message::encode_to_vec);
    assert_eq!(sources, expected);

    let receipts = compile(r#"for (@a <- @"x" ~ 1; @b <- @"y" ~ (; k=2)) { Nil }"#);
    assert_eq!(
        sole_bind_source(&receipts),
        &query(gstring("x"), vec![gint(1)], Vec::new())
    );
    let inner = receipts.receives[0]
        .body
        .as_ref()
        .expect("the outer receive has a body");
    assert_eq!(
        sole_bind_source(inner),
        &query(gstring("y"), Vec::new(), vec![named("k", gint(2))])
    );
}

#[test]
fn normalize_receive_query_rejects_invalid_values() {
    assert_eq!(
        compile_error(r#"for (y <- @"x" ~ (1; k=1, results="all")) { Nil }"#),
        syntax_error(ReifiedSyntaxError::KAndResults)
    );
    assert_eq!(
        compile_error(r#"for (y <- @"x" ~ (1; k=0)) { Nil }"#),
        out_of_range("k", "an Int >= 1")
    );
    assert_eq!(
        compile_error(r#"for (y <- @"x" ~ (1; threshold=1.5)) { Nil }"#),
        out_of_range("threshold", "a Float or a BigRat in [0, 1]")
    );
    assert_eq!(
        compile_error(r#"for (y <- @"x" ~ (1; metric="cos")) { Nil }"#),
        out_of_range(
            "metric",
            "one of cosine, dot, euclidean, manhattan, hamming, jaccard"
        )
    );
    let deferred = compile(r#"new n in { for (y <- @"x" ~ (1; k=*n)) { Nil } }"#);
    assert!(contains_marker(&deferred));
}

#[test]
fn normalize_receive_query_recheck_rejects_what_the_parser_rejects() {
    let parser = RholangParser::new();
    let simple = || Source::Simple {
        name: quoted_string(&parser, "x"),
    };
    let payload = |value: i64| -> SmallVec<[AnnProc; 1]> { smallvec![long(&parser, value)] };
    let named_param = |key: &'static str, value: i64| Hyperparam::Named {
        key: id(key),
        value: long(&parser, value),
    };
    let cases: Vec<(Source, QuerySpec, InterpreterError)> = vec![
        (
            simple(),
            QuerySpec {
                payload: payload(1),
                params: Some(smallvec![Hyperparam::Positional(long(&parser, 2))]),
            },
            syntax_error(ReifiedSyntaxError::PositionalQueryParam),
        ),
        (
            simple(),
            QuerySpec {
                payload: SmallVec::new(),
                params: None,
            },
            syntax_error(ReifiedSyntaxError::EmptyQuery),
        ),
        (
            simple(),
            QuerySpec {
                payload: SmallVec::new(),
                params: Some(SmallVec::new()),
            },
            syntax_error(ReifiedSyntaxError::EmptyQuery),
        ),
        (
            simple(),
            QuerySpec {
                payload: payload(1),
                params: Some(smallvec![named_param("k", 1), named_param("k", 2)]),
            },
            syntax_error(ReifiedSyntaxError::DuplicateKey {
                key: "k".to_owned(),
            }),
        ),
        (
            simple(),
            QuerySpec {
                payload: payload(1),
                params: Some(smallvec![named_param("else", 1)]),
            },
            syntax_error(ReifiedSyntaxError::InvalidIdentifier {
                name: "else".to_owned(),
                position: IdentifierPosition::ParamKey,
            }),
        ),
        (
            Source::SendReceive {
                name: quoted_string(&parser, "x"),
                inputs: payload(1),
                params: None,
            },
            QuerySpec {
                payload: payload(2),
                params: None,
            },
            InterpreterError::NormalizerError(
                "reified syntax: receive query on a non-simple source".to_owned(),
            ),
        ),
        (
            Source::ReceiveSend {
                name: quoted_string(&parser, "x"),
            },
            QuerySpec {
                payload: payload(2),
                params: None,
            },
            InterpreterError::NormalizerError(
                "reified syntax: receive query on a non-simple source".to_owned(),
            ),
        ),
    ];
    for (source, query, error) in cases {
        assert_eq!(
            hand_built_query(&parser, source, query),
            Err(error.clone()),
            "{error:?}"
        );
    }
    assert!(matches!(
        RholangParser::new().parse(r#"for (y <- @"x" ~ 1) { Nil }"#),
        validated::Validated::Good(ref procs)
            if matches!(procs.as_slice(), [AnnProc { proc: Proc::ForComprehension { .. }, .. }])
    ));
}
