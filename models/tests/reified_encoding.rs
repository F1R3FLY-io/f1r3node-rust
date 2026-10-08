// P2 tests of the Reified RSpaces marker-tuple encoding (plan decisions 18, 21 and 27).
//
// The property tests have the names that formal/reified-rspaces-invariant-map.json maps to
// the headlines E1-E6 of formal/rocq/reified_rspaces/theories/Encoding.v. The models crate
// builds no doctests, so the encoding examples of the design are cases of these tests.

#[path = "support/reified_strategies.rs"]
mod reified_strategies;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EMap, ETuple, Expr, GBigRational, GDeployId, GFixedPoint, GUnforgeable, KeyValuePair, Par, Var,
};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::reified::builders::{
    efree, new_space, push_process_expr, query_spec, send_params, use_block,
};
use models::rust::reified::encoding::{
    decode_free_value, decode_process_tuple, decode_query_source, encode, encode_free,
    encode_new_space, encode_params, encode_query, is_identifier, is_process_marker_tuple,
    marker_of, sole_marker_tuple, split_trailing_params, ExpectedElement, FreeTheory, Marker,
    NewSpace, QuerySpec, ReifiedConstruct, ReifiedDecodeError, ReifiedEncodeError, SendParams,
    UseBlock, REIFIED_TAG,
};
use models::rust::reified::params::{collect_send_params, SendParam};
use models::rust::reified::query_spec::{check_query_params, collect_query_params};
use models::rust::reified::validation::{
    is_param_key, is_theory_name, validate_param_value, IdentifierPosition, ReifiedSyntaxError,
    METRIC_VOCABULARY, REJECTED_PARAM_KEYS, REJECTED_THEORY_NAMES,
};
use models::rust::utils::union;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::{RngSeed, TestRunner};
use prost::Message;
use reified_strategies::{
    arb_construct, arb_par, arb_param_key, bits, contains_marker, gbool, gint, gstring, list, map,
    new_term, position_par, private_name, send, sole_tuple, tuple,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use serde_json::Value;

const PROPERTY_SEED: u64 = 0x0E0D_2026_1004_0002;
const ORDINARY_CORPUS: &str = "test-vectors/ordinary-execution-v1.json";
const SCHEMA_PINS: &str = "test-vectors/protobuf-schema-head-4d67b5ccc.json";
const SCHEMA_BASE: &str = "4d67b5ccc42c4e510162d075f7ed423bf8175563";

/// The 32 keywords of grammar `02cef80`: its word-shaped string tokens (plan decision 27).
const KEYWORDS_02CEF80: [&str; 32] = [
    "Bool",
    "ByteArray",
    "Int",
    "Nil",
    "Set",
    "String",
    "Uri",
    "agent",
    "and",
    "bundle",
    "bundle0",
    "catch",
    "constructor",
    "contract",
    "default",
    "else",
    "false",
    "for",
    "if",
    "in",
    "let",
    "match",
    "matches",
    "method",
    "new",
    "not",
    "or",
    "private",
    "select",
    "true",
    "try",
    "where",
];

fn property_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the models crate lives in the repository root")
        .to_path_buf()
}

fn tuple_of(par: &Par) -> ETuple {
    sole_tuple(par)
        .expect("the Par holds exactly one tuple")
        .clone()
}

fn tuple_expr(tuple: ETuple) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::ETupleBody(tuple)),
    }
}

/// A tuple `Par` with a marker head and the canonical flags of its elements.
fn marker_tuple(marker: Marker, fields: Vec<Par>) -> Par {
    let mut ps = Vec::with_capacity(fields.len() + 1);
    ps.push(marker.par());
    ps.extend(fields);
    tuple(ps)
}

/// Decodes `construct` from its own position with the decoder of that position.
fn decode_in_position(construct: &ReifiedConstruct) -> Result<ReifiedConstruct, String> {
    let par = position_par(construct);
    match construct {
        ReifiedConstruct::Use(_) | ReifiedConstruct::NewSpace(_) => {
            decode_process_tuple(&tuple_of(&par))
                .map(ReifiedConstruct::from)
                .map_err(|error| error.to_string())
        }
        ReifiedConstruct::Params(_) => match split_trailing_params(&[gint(7), par]) {
            Ok(Some(params)) => Ok(ReifiedConstruct::Params(params)),
            other => Err(format!("{other:?}")),
        },
        ReifiedConstruct::Query(_) => match decode_query_source(&par) {
            Ok(Some(query)) => Ok(ReifiedConstruct::Query(query)),
            other => Err(format!("{other:?}")),
        },
        ReifiedConstruct::Free(_) => match decode_free_value(&par) {
            Ok(Some(theory)) => Ok(ReifiedConstruct::Free(theory)),
            other => Err(format!("{other:?}")),
        },
    }
}

proptest! {
    #![proptest_config(property_config())]

    /// E1 (decode_encode): each decoder inverts the encoder in its position.
    #[test]
    fn reified_encoding_roundtrip(construct in arb_construct()) {
        prop_assert_eq!(decode_in_position(&construct), Ok(construct.clone()));
        let expr = encode(&construct).expect("a generated construct is well formed");
        let positioned = position_par(&construct);
        prop_assert_eq!(Some(&expr), positioned.exprs.first());
    }

    /// E2 (encode_injective): two constructs have one encoding only when they are equal.
    #[test]
    fn reified_encoding_injective((left, right) in arb_construct_pair()) {
        let left_expr = encode(&left).expect("a generated construct is well formed");
        let right_expr = encode(&right).expect("a generated construct is well formed");
        prop_assert_eq!(left_expr == right_expr, left == right);
        prop_assert_eq!(position_par(&left) == position_par(&right), left == right);
    }

    /// E3 (decode_sound): a decoder accepts a term only when the term is the canonical
    /// encoding of the decoded construct.
    #[test]
    fn reified_decode_accepts_only_canonical_image(
        (construct, mutation) in (arb_construct(), arb_mutation())
    ) {
        let original = position_par(&construct);
        let candidate = mutate(&original, mutation);
        let accepted = check_decoders_sound(&candidate)?;
        prop_assert!(accepted <= 1);
        match mutation {
            Mutation::Identity => {
                prop_assert_eq!(accepted, 1);
                prop_assert_eq!(decode_in_position(&construct), Ok(construct.clone()));
            }
            Mutation::FlipTupleFlag => prop_assert_eq!(accepted, 0),
            _ if candidate == original => prop_assert_eq!(accepted, 1),
            _ => {}
        }
    }

    /// E6 (named_params_canonical): the named params take one key order, the bytewise order
    /// of `String::cmp`, whatever the source order.
    #[test]
    fn reified_params_canonical_key_order(
        (entries, rotation) in (
            proptest::collection::btree_map(arb_param_key(), arb_par(), 1..6),
            any::<usize>(),
        )
    ) {
        let entries: Vec<(String, Par)> = entries.into_iter().collect();
        let mut reversed = entries.clone();
        reversed.reverse();
        let mut rotated = entries.clone();
        rotated.rotate_left(rotation % entries.len());
        let encodings = [entries.clone(), reversed, rotated].map(|order| {
            send_params(order.into_iter().map(|(key, value)| SendParam::Named(key, value)).collect())
                .expect("unique param keys build")
                .expect("named params give a PARAMS element")
        });
        prop_assert_eq!(&encodings[0], &encodings[1]);
        prop_assert_eq!(&encodings[0], &encodings[2]);
        let keys = map_keys(&encodings[0]);
        let expected: Vec<String> = entries.iter().map(|(key, _)| key.clone()).collect();
        prop_assert_eq!(&keys, &expected);
        prop_assert!(keys.windows(2).all(|pair| pair[0].as_bytes() < pair[1].as_bytes()));
        let named: BTreeMap<String, Par> = entries.into_iter().collect();
        prop_assert_eq!(
            split_trailing_params(&encodings[0..1]),
            Ok(Some(SendParams { positional: Vec::new(), named }))
        );
        if keys.len() >= 2 {
            let unsorted = with_map_kvs(&encodings[0], 2, |kvs| kvs.reverse());
            prop_assert_eq!(
                split_trailing_params(&[unsorted]),
                Err(ReifiedDecodeError::NonCanonical { marker: Marker::Params })
            );
        }
    }
}

/// A pair of constructs: two unrelated ones, two equal ones, or two near ones.
fn arb_construct_pair() -> impl Strategy<Value = (ReifiedConstruct, ReifiedConstruct)> {
    prop_oneof![
        (arb_construct(), arb_construct()),
        arb_construct().prop_map(|construct| (construct.clone(), construct)),
        arb_construct().prop_flat_map(|construct| {
            let near = select(nearby(&construct));
            (Just(construct), near)
        }),
    ]
}

/// Well-formed constructs near `construct`: each differs in one field.
fn nearby(construct: &ReifiedConstruct) -> Vec<ReifiedConstruct> {
    let mut near = Vec::with_capacity(4);
    match construct {
        ReifiedConstruct::Use(block) => {
            near.push(ReifiedConstruct::Use(UseBlock {
                space: block.body.clone(),
                body: block.space.clone(),
            }));
            near.push(ReifiedConstruct::Use(UseBlock {
                space: block.space.clone(),
                body: list(vec![block.body.clone()]),
            }));
        }
        ReifiedConstruct::NewSpace(new_space) => {
            let mut annotations = new_space.annotations.clone();
            annotations.push(None);
            let mut wider = new_space.new.clone();
            wider.bind_count += 1;
            near.push(ReifiedConstruct::NewSpace(NewSpace {
                annotations,
                new: wider,
            }));
            let mut nil_annotation = new_space.annotations.clone();
            let first = nil_annotation
                .iter()
                .position(Option::is_some)
                .expect("a NEWSPACE construct has an annotation");
            nil_annotation[first] = Some(Par::default());
            near.push(ReifiedConstruct::NewSpace(NewSpace {
                annotations: nil_annotation,
                new: new_space.new.clone(),
            }));
        }
        ReifiedConstruct::Params(params) => {
            let mut moved = params.clone();
            match moved.positional.pop() {
                Some(value) => {
                    moved.named.insert("moved".to_owned(), value);
                }
                None => moved.positional.push(Par::default()),
            }
            near.push(ReifiedConstruct::Params(moved));
            let mut wrapped = params.clone();
            wrapped.positional = vec![list(params.positional.clone())];
            near.push(ReifiedConstruct::Params(wrapped));
        }
        ReifiedConstruct::Query(query) => {
            for payload in [vec![tuple(query.payload.clone())], vec![list(
                query.payload.clone(),
            )]] {
                if !query.payload.is_empty() {
                    near.push(ReifiedConstruct::Query(QuerySpec {
                        payload,
                        ..query.clone()
                    }));
                }
            }
            near.push(ReifiedConstruct::Query(QuerySpec {
                channel: list(vec![query.channel.clone()]),
                ..query.clone()
            }));
        }
        ReifiedConstruct::Free(theory) => {
            near.push(ReifiedConstruct::Free(FreeTheory {
                name: theory.name.clone(),
                args: vec![list(theory.args.clone())],
            }));
            near.push(ReifiedConstruct::Free(FreeTheory {
                name: format!("{}x", theory.name),
                args: theory.args.clone(),
            }));
        }
    }
    near
}

/// A change of a term in a marker position. The flags of the tuple and its `Par` keep their
/// canonical values except under the flag mutations.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    Identity,
    FlipTupleFlag,
    FlipParFlag,
    DropLast,
    AppendNil,
    ElementToNil(usize),
    ElementToInt(usize),
    ListRemainder(usize),
    MapReversed(usize),
    MapDuplicate(usize),
    KeyToInt(usize),
    KeyInvalid(usize),
    ExtraSend,
    HeadMarker(usize),
    HeadConnective,
    AnnotationPair,
    BindCountUp,
}

fn arb_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        Just(Mutation::Identity),
        Just(Mutation::FlipTupleFlag),
        Just(Mutation::FlipParFlag),
        Just(Mutation::DropLast),
        Just(Mutation::AppendNil),
        (1usize..4).prop_map(Mutation::ElementToNil),
        (1usize..4).prop_map(Mutation::ElementToInt),
        (1usize..4).prop_map(Mutation::ListRemainder),
        (1usize..4).prop_map(Mutation::MapReversed),
        (1usize..4).prop_map(Mutation::MapDuplicate),
        (1usize..4).prop_map(Mutation::KeyToInt),
        (1usize..4).prop_map(Mutation::KeyInvalid),
        Just(Mutation::ExtraSend),
        (0usize..5).prop_map(Mutation::HeadMarker),
        Just(Mutation::HeadConnective),
        Just(Mutation::AnnotationPair),
        Just(Mutation::BindCountUp),
    ]
}

/// Rebuilds the tuple of `par` with `change` and the canonical flags of the new elements.
fn with_elements(par: &Par, change: impl FnOnce(&mut Vec<Par>)) -> Par {
    let mut ps = tuple_of(par).ps;
    change(&mut ps);
    tuple(ps)
}

/// Rebuilds the map at tuple index `index` of `par` with `change` on its entries.
fn with_map_kvs(par: &Par, index: usize, change: impl FnOnce(&mut Vec<KeyValuePair>)) -> Par {
    with_elements(par, |ps| {
        if let Some(element) = ps.get_mut(index) {
            if let Some(Expr {
                expr_instance: Some(ExprInstance::EMapBody(emap)),
            }) = element.exprs.first_mut()
            {
                change(&mut emap.kvs);
            }
        }
    })
}

fn mutate(par: &Par, mutation: Mutation) -> Par {
    match mutation {
        Mutation::Identity => par.clone(),
        Mutation::FlipTupleFlag => {
            let mut tuple = tuple_of(par);
            tuple.connective_used = !tuple.connective_used;
            Par {
                exprs: vec![tuple_expr(tuple)],
                ..par.clone()
            }
        }
        Mutation::FlipParFlag => Par {
            connective_used: !par.connective_used,
            ..par.clone()
        },
        Mutation::DropLast => with_elements(par, |ps| {
            ps.pop();
        }),
        Mutation::AppendNil => with_elements(par, |ps| ps.push(Par::default())),
        Mutation::ElementToNil(index) => with_elements(par, |ps| {
            if let Some(element) = ps.get_mut(index) {
                *element = Par::default();
            }
        }),
        Mutation::ElementToInt(index) => with_elements(par, |ps| {
            if let Some(element) = ps.get_mut(index) {
                *element = gint(1);
            }
        }),
        Mutation::ListRemainder(index) => with_elements(par, |ps| {
            if let Some(element) = ps.get_mut(index) {
                if let Some(Expr {
                    expr_instance: Some(ExprInstance::EListBody(elist)),
                }) = element.exprs.first_mut()
                {
                    elist.remainder = Some(Var {
                        var_instance: Some(VarInstance::FreeVar(0)),
                    });
                }
            }
        }),
        Mutation::MapReversed(index) => with_map_kvs(par, index, |kvs| kvs.reverse()),
        Mutation::MapDuplicate(index) => with_map_kvs(par, index, |kvs| {
            if let Some(first) = kvs.first().cloned() {
                kvs.push(first);
            }
        }),
        Mutation::KeyToInt(index) => with_map_kvs(par, index, |kvs| {
            if let Some(first) = kvs.first_mut() {
                first.key = Some(gint(3));
            }
        }),
        Mutation::KeyInvalid(index) => with_map_kvs(par, index, |kvs| {
            if let Some(first) = kvs.first_mut() {
                first.key = Some(gstring("Nil"));
            }
        }),
        Mutation::ExtraSend => {
            let mut extended = par.clone();
            extended.sends = send(gint(9), vec![]).sends;
            extended
        }
        Mutation::HeadMarker(index) => with_elements(par, |ps| {
            if let Some(head) = ps.first_mut() {
                *head = Marker::ALL[index].par();
            }
        }),
        Mutation::HeadConnective => with_elements(par, |ps| {
            if let Some(head) = ps.first_mut() {
                head.connective_used = true;
            }
        }),
        Mutation::AnnotationPair => with_elements(par, |ps| {
            if let Some(element) = ps.get_mut(1) {
                if let Some(Expr {
                    expr_instance: Some(ExprInstance::EListBody(elist)),
                }) = element.exprs.first_mut()
                {
                    elist.ps.insert(0, list(vec![gint(1), gint(2)]));
                }
            }
        }),
        Mutation::BindCountUp => with_elements(par, |ps| {
            if let Some(element) = ps.get_mut(2) {
                if let Some(new) = element.news.first_mut() {
                    new.bind_count += 1;
                }
            }
        }),
    }
}

/// Runs every decoder on `candidate` and checks that each accepted construct encodes back to
/// `candidate`. Returns the number of decoders that accepted it.
fn check_decoders_sound(candidate: &Par) -> Result<usize, TestCaseError> {
    let mut accepted = 0;
    if let Some(tuple) = sole_tuple(candidate) {
        if let Ok(construct) = decode_process_tuple(tuple) {
            let expected = encode(&ReifiedConstruct::from(construct))
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            prop_assert_eq!(expected, tuple_expr(tuple.clone()));
            accepted += 1;
        }
    }
    if let Ok(Some(params)) = split_trailing_params(std::slice::from_ref(candidate)) {
        prop_assert_eq!(encode_params(&params), Ok(Some(candidate.clone())));
        accepted += 1;
    }
    if let Ok(Some(query)) = decode_query_source(candidate) {
        prop_assert_eq!(encode_query(&query), Ok(candidate.clone()));
        accepted += 1;
    }
    if let Ok(Some(theory)) = decode_free_value(candidate) {
        prop_assert_eq!(encode_free(&theory), Ok(candidate.clone()));
        accepted += 1;
    }
    Ok(accepted)
}

fn map_keys(par: &Par) -> Vec<String> {
    let tuple = tuple_of(par);
    match tuple.ps.get(2).and_then(|element| element.exprs.first()) {
        Some(Expr {
            expr_instance: Some(ExprInstance::EMapBody(emap)),
        }) => emap
            .kvs
            .iter()
            .map(
                |kv| match kv.key.as_ref().and_then(|key| key.exprs.first()) {
                    Some(Expr {
                        expr_instance: Some(ExprInstance::GString(key)),
                    }) => key.clone(),
                    other => panic!("a param key is a string, found {other:?}"),
                },
            )
            .collect(),
        other => panic!("PARAMS element 2 is a map, found {other:?}"),
    }
}

/// A list `Par` with a remainder.
fn list_with_remainder(ps: Vec<Par>) -> Par {
    let mut par = list(ps);
    if let Some(Expr {
        expr_instance: Some(ExprInstance::EListBody(elist)),
    }) = par.exprs.first_mut()
    {
        elist.remainder = Some(Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        });
    }
    par
}

/// A map `Par` with the entries `kvs` in this order, without sorting or deduplication.
fn raw_map(kvs: Vec<(Par, Option<Par>)>) -> Par {
    let kvs: Vec<KeyValuePair> = kvs
        .into_iter()
        .map(|(key, value)| KeyValuePair {
            key: Some(key),
            value,
        })
        .collect();
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Par::default()
    }
}

fn new_par(new: models::rhoapi::New) -> Par {
    let locally_free = new.locally_free.clone();
    let connective_used = new.p.as_ref().is_some_and(|body| body.connective_used);
    Par {
        news: vec![new],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

fn params_error(par: Par) -> ReifiedDecodeError {
    split_trailing_params(&[par]).expect_err("the PARAMS tuple is malformed")
}

fn query_error(par: Par) -> ReifiedDecodeError {
    decode_query_source(&par).expect_err("the QUERY tuple is malformed")
}

fn free_error(par: Par) -> ReifiedDecodeError {
    decode_free_value(&par).expect_err("the FREE tuple is malformed")
}

fn process_error(par: Par) -> ReifiedDecodeError {
    decode_process_tuple(&tuple_of(&par)).expect_err("the process tuple is malformed")
}

/// Each malformed marker term fails closed with its typed error, and a term without the
/// marker of a position is inert there.
#[test]
fn reified_decode_typed_errors() {
    use ReifiedDecodeError as E;
    let space = private_name(vec![5; 32]);
    let body = send(gint(1), vec![gint(2)]);
    let one_binder = new_term(1, Vec::new(), Par::default());

    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![list(vec![gint(1)])])),
        E::WrongArity {
            marker: Marker::Params,
            expected: 3,
            actual: 2
        }
    );
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![gint(1), map(vec![])])),
        E::ElementKind {
            index: 1,
            expected: ExpectedElement::List
        }
    );
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![
            list(vec![gint(1)]),
            gint(2)
        ])),
        E::ElementKind {
            index: 2,
            expected: ExpectedElement::Map
        }
    );
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![
            list_with_remainder(vec![gint(1)]),
            map(vec![])
        ])),
        E::Remainder { index: 1 }
    );
    let mut map_with_remainder = map(vec![("k".to_owned(), gint(1))]);
    if let Some(Expr {
        expr_instance: Some(ExprInstance::EMapBody(emap)),
    }) = map_with_remainder.exprs.first_mut()
    {
        emap.remainder = Some(Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        });
    }
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![
            list(vec![]),
            map_with_remainder
        ])),
        E::Remainder { index: 2 }
    );
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![
            list(vec![]),
            raw_map(vec![(gint(1), Some(gint(2)))])
        ])),
        E::NonStringKey
    );
    for invalid in ["true", "Nil", "1k", "", "_", "a-b"] {
        assert_eq!(
            params_error(marker_tuple(Marker::Params, vec![
                list(vec![]),
                raw_map(vec![(gstring(invalid), Some(gint(2)))])
            ])),
            E::InvalidIdentifier {
                name: invalid.to_owned(),
                position: IdentifierPosition::ParamKey
            }
        );
    }
    assert_eq!(
        params_error(marker_tuple(Marker::Params, vec![
            list(vec![]),
            map(vec![])
        ])),
        E::EmptyCanonicalForm {
            marker: Marker::Params
        }
    );
    for kvs in [
        vec![(gstring("b"), Some(gint(1))), (gstring("a"), Some(gint(2)))],
        vec![(gstring("k"), Some(gint(1))), (gstring("k"), Some(gint(1)))],
        vec![(gstring("k"), None)],
    ] {
        assert_eq!(
            params_error(marker_tuple(Marker::Params, vec![
                list(vec![]),
                raw_map(kvs)
            ])),
            E::NonCanonical {
                marker: Marker::Params
            }
        );
    }
    let canonical_params = marker_tuple(Marker::Params, vec![list(vec![gint(1)]), map(vec![])]);
    assert_eq!(
        params_error(mutate(&canonical_params, Mutation::FlipTupleFlag)),
        E::NonCanonical {
            marker: Marker::Params
        }
    );
    assert_eq!(
        params_error(mutate(&canonical_params, Mutation::FlipParFlag)),
        E::NonCanonical {
            marker: Marker::Params
        }
    );

    assert_eq!(
        query_error(marker_tuple(Marker::Query, vec![
            space.clone(),
            list(vec![gint(1)])
        ])),
        E::WrongArity {
            marker: Marker::Query,
            expected: 4,
            actual: 3
        }
    );
    assert_eq!(
        query_error(marker_tuple(Marker::Query, vec![
            space.clone(),
            gint(1),
            map(vec![])
        ])),
        E::ElementKind {
            index: 2,
            expected: ExpectedElement::List
        }
    );
    assert_eq!(
        query_error(marker_tuple(Marker::Query, vec![
            space.clone(),
            list(vec![]),
            map(vec![])
        ])),
        E::EmptyCanonicalForm {
            marker: Marker::Query
        }
    );
    assert_eq!(
        query_error(marker_tuple(Marker::Query, vec![
            space.clone(),
            list(vec![gint(1)]),
            map(vec![("Nil".to_owned(), gint(1))])
        ])),
        E::InvalidIdentifier {
            name: "Nil".to_owned(),
            position: IdentifierPosition::ParamKey
        }
    );

    assert_eq!(
        free_error(marker_tuple(Marker::Free, vec![gint(1), list(vec![])])),
        E::ElementKind {
            index: 1,
            expected: ExpectedElement::String
        }
    );
    for invalid in ["new", "true", "Nat-1", "1Nat", ""] {
        assert_eq!(
            free_error(marker_tuple(Marker::Free, vec![
                gstring(invalid),
                list(vec![])
            ])),
            E::InvalidIdentifier {
                name: invalid.to_owned(),
                position: IdentifierPosition::TheoryName
            }
        );
    }
    assert_eq!(
        free_error(marker_tuple(Marker::Free, vec![gstring("Nat"), gint(1)])),
        E::ElementKind {
            index: 2,
            expected: ExpectedElement::List
        }
    );
    assert_eq!(
        free_error(marker_tuple(Marker::Free, vec![gstring("Nat")])),
        E::WrongArity {
            marker: Marker::Free,
            expected: 3,
            actual: 2
        }
    );

    assert_eq!(
        process_error(marker_tuple(Marker::Use, vec![space.clone()])),
        E::WrongArity {
            marker: Marker::Use,
            expected: 3,
            actual: 2
        }
    );
    for head in [
        Marker::Params.par(),
        Marker::Query.par(),
        Marker::Free.par(),
        gint(1),
    ] {
        assert_eq!(
            process_error(tuple(vec![head, space.clone(), body.clone()])),
            E::ElementKind {
                index: 0,
                expected: ExpectedElement::ProcessMarker
            }
        );
    }
    let new_space_par = new_par(one_binder.clone());
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            gint(1),
            new_space_par.clone()
        ])),
        E::ElementKind {
            index: 1,
            expected: ExpectedElement::List
        }
    );
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            list(vec![list(vec![space.clone(), space.clone()])]),
            new_space_par.clone()
        ])),
        E::AnnotationShape { binder: 0 }
    );
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            list(vec![space.clone()]),
            new_space_par.clone()
        ])),
        E::AnnotationShape { binder: 0 }
    );
    let mut two_news = new_space_par.clone();
    two_news.news.push(one_binder.clone());
    for not_sole in [Par::default(), two_news, gint(3)] {
        assert_eq!(
            process_error(marker_tuple(Marker::NewSpace, vec![
                list(vec![list(vec![space.clone()])]),
                not_sole
            ])),
            E::NotSoleNew
        );
    }
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            list(vec![list(vec![space.clone()])]),
            new_par(new_term(2, Vec::new(), Par::default()))
        ])),
        E::AnnotationCount {
            annotations: 1,
            bind_count: 2,
            uris: 0
        }
    );
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            list(vec![list(vec![])]),
            new_space_par.clone()
        ])),
        E::EmptyCanonicalForm {
            marker: Marker::NewSpace
        }
    );
    let use_tuple = marker_tuple(Marker::Use, vec![space.clone(), body.clone()]);
    assert_eq!(
        process_error(mutate(&use_tuple, Mutation::FlipTupleFlag)),
        E::NonCanonical {
            marker: Marker::Use
        }
    );
    let mut flagged_new = new_space_par.clone();
    flagged_new.connective_used = true;
    assert_eq!(
        process_error(marker_tuple(Marker::NewSpace, vec![
            list(vec![list(vec![space.clone()])]),
            flagged_new
        ])),
        E::NonCanonical {
            marker: Marker::NewSpace
        }
    );

    // Inert terms: no marker of the position, or not exactly one marker tuple.
    assert_eq!(split_trailing_params(&[]), Ok(None));
    assert_eq!(
        split_trailing_params(&[canonical_params.clone(), gint(1)]),
        Ok(None)
    );
    assert_eq!(
        split_trailing_params(std::slice::from_ref(&use_tuple)),
        Ok(None)
    );
    assert_eq!(decode_query_source(&canonical_params), Ok(None));
    assert_eq!(decode_free_value(&use_tuple), Ok(None));
    let mut with_send = canonical_params.clone();
    with_send.sends = send(gint(9), vec![]).sends;
    assert_eq!(split_trailing_params(&[with_send]), Ok(None));
    let flagged_head = with_elements(&canonical_params, |ps| ps[0].connective_used = true);
    assert_eq!(split_trailing_params(&[flagged_head]), Ok(None));
    for error in [E::NonStringKey, E::NotSoleNew, E::NonCanonical {
        marker: Marker::Use,
    }] {
        assert!(!error.to_string().is_empty());
    }
}

/// E4 (markers_pairwise_distinct) and E5 (markers_disjoint_reserved): the five marker ids
/// are distinct, and no other id of the campaign is a marker.
#[test]
fn reified_markers_disjoint() {
    let ids: BTreeSet<[u8; 2]> = Marker::ALL.iter().map(|marker| marker.id()).collect();
    assert_eq!(ids.len(), Marker::ALL.len());
    assert_eq!(Marker::ALL.map(|marker| marker.id()), [
        [0xE0, 0xD0],
        [0xE0, 0xD1],
        [0xE0, 0xD2],
        [0xE0, 0xD3],
        [0xE0, 0xD4]
    ]);
    assert_eq!(Marker::ALL.map(Marker::tuple_arity), [3, 3, 3, 4, 3]);
    for marker in Marker::ALL {
        let complete = match marker {
            Marker::Use | Marker::NewSpace | Marker::Params | Marker::Query | Marker::Free => {
                Marker::ALL.contains(&marker)
            }
        };
        assert!(complete);
        assert_eq!(Marker::from_id(&marker.id()), Some(marker));
        assert_eq!(marker_of(&marker.par()), Some(marker));
        assert_eq!(marker.id()[0], REIFIED_TAG);
    }

    // Every 2-byte id: only [0xE0, 0xD0..=0xD4] names a marker.
    for first in 0..=u8::MAX {
        for second in 0..=u8::MAX {
            let expected = first == REIFIED_TAG && (0xD0..=0xD4).contains(&second);
            assert_eq!(Marker::from_id(&[first, second]).is_some(), expected);
        }
    }
    // The other [0xE0, idx] ids of the plan: factories, getSpaceAgent, assertEqual, vector,
    // the internal ops and AGENT_TAG.
    let reserved: Vec<u8> = (0x00..=0x23).chain(0x30..=0x32).chain([0xFF]).collect();
    for index in reserved {
        assert_eq!(Marker::from_id(&[REIFIED_TAG, index]), None);
        assert_eq!(marker_of(&private_name(vec![REIFIED_TAG, index])), None);
    }
    // Ids of every other length: byte names (1), new names (32), storage and MeTTaIL ids (34
    // or more), and ids that start with a marker id.
    for length in [0usize, 1, 3, 32, 33, 34, 35, 64] {
        for marker in Marker::ALL {
            let mut id = marker.id().to_vec();
            id.resize(length, 0x44);
            assert_eq!(Marker::from_id(&id), None, "an id of {length} bytes");
            assert_eq!(marker_of(&private_name(id)), None);
        }
    }
    // A marker name is exact: no flag, no other component and no other unforgeable kind.
    for marker in Marker::ALL {
        let mut flagged = marker.par();
        flagged.connective_used = true;
        assert_eq!(marker_of(&flagged), None);
        let mut extended = marker.par();
        extended.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        });
        assert_eq!(marker_of(&extended), None);
        let deploy_id = Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId {
                    sig: marker.id().to_vec(),
                })),
            }],
            ..Par::default()
        };
        assert_eq!(marker_of(&deploy_id), None);
    }
}

/// The builders and the encoder compute `locally_free` and `connective_used` with the
/// formulas of the normalizer, and the examples of the design encode as stated.
#[test]
fn reified_builders_match_normalizer_formulas() {
    let mut runner = TestRunner::new(property_config());
    runner
        .run(&(arb_construct(), arb_par()), |(construct, process)| {
            check_construct_formulas(&construct, &process)
        })
        .expect("the encodings follow the normalizer formulas");
    check_design_examples();
}

fn check_tuple_fields(tuple: &ETuple) -> Result<(), TestCaseError> {
    let (locally_free, connective_used) = bits(&tuple.ps);
    prop_assert_eq!(&tuple.locally_free, &locally_free);
    prop_assert_eq!(tuple.connective_used, connective_used);
    Ok(())
}

fn check_list_fields(par: &Par) -> Result<(), TestCaseError> {
    match par.exprs.first() {
        Some(Expr {
            expr_instance: Some(ExprInstance::EListBody(elist)),
        }) => {
            let (locally_free, connective_used) = bits(&elist.ps);
            prop_assert_eq!(&elist.locally_free, &locally_free);
            prop_assert_eq!(elist.connective_used, connective_used);
            prop_assert_eq!(&par.locally_free, &locally_free);
            prop_assert_eq!(par.connective_used, connective_used);
            Ok(())
        }
        other => Err(TestCaseError::fail(format!("not a list: {other:?}"))),
    }
}

fn check_map_fields(par: &Par, named: &BTreeMap<String, Par>) -> Result<(), TestCaseError> {
    let pairs: Vec<(Par, Par)> = named
        .iter()
        .map(|(key, value)| (gstring(key), value.clone()))
        .collect();
    let expected = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(pairs));
    match par.exprs.first() {
        Some(Expr {
            expr_instance: Some(ExprInstance::EMapBody(emap)),
        }) => {
            prop_assert_eq!(emap, &expected);
            prop_assert_eq!(&emap.locally_free, &expected.locally_free);
            prop_assert_eq!(&par.locally_free, &expected.locally_free);
            prop_assert_eq!(par.connective_used, expected.connective_used);
            Ok(())
        }
        other => Err(TestCaseError::fail(format!("not a map: {other:?}"))),
    }
}

fn check_construct_formulas(
    construct: &ReifiedConstruct,
    process: &Par,
) -> Result<(), TestCaseError> {
    let par = position_par(construct);
    let tuple = tuple_of(&par);
    check_tuple_fields(&tuple)?;
    prop_assert_eq!(&par.locally_free, &tuple.locally_free);
    prop_assert_eq!(par.connective_used, tuple.connective_used);
    match construct {
        ReifiedConstruct::Use(block) => {
            prop_assert_eq!(&tuple.ps, &vec![
                Marker::Use.par(),
                block.space.clone(),
                block.body.clone()
            ]);
        }
        ReifiedConstruct::NewSpace(new_space) => {
            check_list_fields(&tuple.ps[1])?;
            prop_assert_eq!(&tuple.ps[2], &new_par(new_space.new.clone()));
            prop_assert_eq!(&tuple.ps[2].locally_free, &new_space.new.locally_free);
        }
        ReifiedConstruct::Params(params) => {
            check_list_fields(&tuple.ps[1])?;
            check_map_fields(&tuple.ps[2], &params.named)?;
        }
        ReifiedConstruct::Query(query) => {
            check_list_fields(&tuple.ps[2])?;
            check_map_fields(&tuple.ps[3], &query.named)?;
        }
        ReifiedConstruct::Free(theory) => {
            prop_assert_eq!(&tuple.ps[1], &gstring(&theory.name));
            check_list_fields(&tuple.ps[2])?;
        }
    }
    // push_process_expr is prepend_expr: the tuple goes first, and the bit sets merge.
    let pushed = push_process_expr(process.clone(), tuple.clone());
    let mut exprs = vec![tuple_expr(tuple.clone())];
    exprs.extend(process.exprs.clone());
    prop_assert_eq!(&pushed.exprs, &exprs);
    prop_assert_eq!(
        &pushed.locally_free,
        &union(process.locally_free.clone(), tuple.locally_free.clone())
    );
    prop_assert_eq!(
        pushed.connective_used,
        process.connective_used || tuple.connective_used
    );
    prop_assert_eq!(&pushed.sends, &process.sends);
    Ok(())
}

/// The examples of design §1, built through the builders.
fn check_design_examples() {
    let d = gstring("d");
    // `ch!(d; 3)` gives the data `[d, (PARAMS, [3], {})]`.
    assert_eq!(
        send_params(vec![SendParam::Positional(gint(3))]),
        Ok(Some(marker_tuple(Marker::Params, vec![
            list(vec![gint(3)]),
            map(vec![])
        ])))
    );
    // `ch!(d; 3, priority=4)` keeps both values.
    assert_eq!(
        send_params(vec![
            SendParam::Positional(gint(3)),
            SendParam::Named("priority".to_owned(), gint(4)),
        ]),
        Ok(Some(marker_tuple(Marker::Params, vec![
            list(vec![gint(3)]),
            map(vec![("priority".to_owned(), gint(4))])
        ])))
    );
    // `ch!(d, 3)` has the data `[d, 3]`, which differs from `[d, (PARAMS, [3], {})]`.
    let with_params = vec![
        d.clone(),
        send_params(vec![SendParam::Positional(gint(3))])
            .expect("one positional param builds")
            .expect("one positional param gives PARAMS"),
    ];
    assert_ne!(vec![d.clone(), gint(3)], with_params);
    assert_eq!(split_trailing_params(&[d.clone(), gint(3)]), Ok(None));
    // `ch!(; k=1)` gives `[(PARAMS, [], {"k": 1})]`.
    assert_eq!(
        send_params(vec![SendParam::Named("k".to_owned(), gint(1))]),
        Ok(Some(marker_tuple(Marker::Params, vec![
            list(vec![]),
            map(vec![("k".to_owned(), gint(1))])
        ])))
    );
    // `x!(a;)` encodes as `x!(a)`: an empty params section is erased.
    assert_eq!(send_params(Vec::new()), Ok(None));
    // `ch ~ (v1, v2; k=3)`, `ch ~ ((v1, v2); k=3)` and `ch ~ ([v1, v2]; k=3)` differ.
    let (v1, v2, ch) = (gint(1), gint(2), gstring("ch"));
    let k3 = || vec![SendParam::Named("k".to_owned(), gint(3))];
    let flat = query_spec(ch.clone(), vec![v1.clone(), v2.clone()], k3()).expect("a query builds");
    let in_tuple = query_spec(ch.clone(), vec![tuple(vec![v1.clone(), v2.clone()])], k3())
        .expect("a tuple payload builds");
    let in_list = query_spec(ch.clone(), vec![list(vec![v1.clone(), v2.clone()])], k3())
        .expect("a list payload builds");
    assert_ne!(flat, in_tuple);
    assert_ne!(in_tuple, in_list);
    assert_ne!(flat, in_list);
    assert_eq!(
        flat,
        marker_tuple(Marker::Query, vec![
            ch.clone(),
            list(vec![v1.clone(), v2]),
            map(vec![("k".to_owned(), gint(3))])
        ])
    );
    // `ch ~ (e;)` encodes as `ch ~ e`: the payload `[e]` without params.
    assert_eq!(
        query_spec(ch.clone(), vec![v1.clone()], Vec::new()),
        Ok(marker_tuple(Marker::Query, vec![
            ch.clone(),
            list(vec![v1]),
            map(vec![])
        ]))
    );
    // `ch ~ (; priority=0)` has the empty payload.
    assert_eq!(
        query_spec(ch.clone(), Vec::new(), vec![SendParam::Named(
            "priority".to_owned(),
            gint(0)
        )]),
        Ok(marker_tuple(Marker::Query, vec![
            ch,
            list(vec![]),
            map(vec![("priority".to_owned(), gint(0))])
        ]))
    );
    // `free Nat()` gives `(FREE, "Nat", [])`.
    assert_eq!(
        efree("Nat", Vec::new()).map(|tuple| push_process_expr(Par::default(), tuple)),
        Ok(marker_tuple(Marker::Free, vec![
            gstring("Nat"),
            list(vec![])
        ]))
    );
    // `use s { P }` gives `(USE, s, P)`.
    let s = private_name(vec![7; 32]);
    let p = send(gint(1), vec![gint(2)]);
    assert_eq!(
        push_process_expr(Par::default(), use_block(s.clone(), p.clone())),
        marker_tuple(Marker::Use, vec![s.clone(), p.clone()])
    );
    // `new a(`rho:io:stdout`), x : s, y in P`: the URI binder sorts last, and the
    // annotation list covers x and y in source order.
    let new = new_term(2, vec!["rho:io:stdout".to_owned()], p);
    let built =
        new_space(new.clone(), vec![Some(s.clone()), None, None]).expect("an annotated new builds");
    assert_eq!(
        push_process_expr(Par::default(), built),
        marker_tuple(Marker::NewSpace, vec![
            list(vec![list(vec![s]), list(vec![])]),
            new_par(new)
        ])
    );
}

/// The builders and the validators reject the syntax that the frontend cannot produce.
#[test]
fn reified_builders_reject_invalid_syntax() {
    use ReifiedSyntaxError as S;
    // The two word lists of plan decision 27, derived from the keywords of grammar 02cef80.
    let key_keywords = ["catch", "constructor", "default", "in", "method", "private"];
    let derived_keys: BTreeSet<&str> = KEYWORDS_02CEF80
        .iter()
        .copied()
        .filter(|word| !key_keywords.contains(word))
        .collect();
    assert_eq!(derived_keys, REJECTED_PARAM_KEYS.iter().copied().collect());
    for word in [
        "Bool",
        "ByteArray",
        "Int",
        "Nil",
        "String",
        "Uri",
        "true",
        "false",
    ] {
        assert!(REJECTED_PARAM_KEYS.contains(&word));
    }
    let theory_keywords = [
        "Bool",
        "ByteArray",
        "Int",
        "Nil",
        "String",
        "Uri",
        "agent",
        "bundle0",
        "catch",
        "constructor",
        "default",
        "method",
        "private",
        "try",
    ];
    let derived_theories: BTreeSet<&str> = KEYWORDS_02CEF80
        .iter()
        .copied()
        .filter(|word| !theory_keywords.contains(word))
        .collect();
    assert_eq!(
        derived_theories,
        REJECTED_THEORY_NAMES.iter().copied().collect()
    );
    assert_eq!(
        (REJECTED_PARAM_KEYS.len(), REJECTED_THEORY_NAMES.len()),
        (26, 18)
    );
    for words in [&REJECTED_PARAM_KEYS[..], &REJECTED_THEORY_NAMES[..]] {
        assert!(words
            .windows(2)
            .all(|pair| pair[0].as_bytes() < pair[1].as_bytes()));
    }

    for word in REJECTED_PARAM_KEYS {
        assert!(is_identifier(word) && !is_param_key(word));
        assert_eq!(
            send_params(vec![SendParam::Named(word.to_owned(), gint(1))]),
            Err(S::InvalidIdentifier {
                name: word.to_owned(),
                position: IdentifierPosition::ParamKey
            })
        );
    }
    for word in key_keywords
        .iter()
        .chain(["use", "free", "priority", "x'", "_a"].iter())
    {
        assert!(is_param_key(word), "{word} is a param key");
    }
    for word in REJECTED_THEORY_NAMES {
        assert!(is_identifier(word) && !is_theory_name(word));
        assert_eq!(
            efree(word, Vec::new()),
            Err(S::InvalidIdentifier {
                name: word.to_owned(),
                position: IdentifierPosition::TheoryName
            })
        );
    }
    for word in theory_keywords
        .iter()
        .chain(["Nat", "Int", "String", "Bool", "Any"].iter())
    {
        assert!(is_theory_name(word), "{word} is a theory name");
        assert!(efree(word, Vec::new()).is_ok());
    }
    for token in ["", "_", "1k", "a-b", "é", "a b", "'a"] {
        assert!(!is_identifier(token), "{token:?} is not a var token");
        assert!(!is_param_key(token) && !is_theory_name(token));
    }
    for token in ["a", "Z", "_0", "a'", "x_y'z", "__"] {
        assert!(is_identifier(token), "{token:?} is a var token");
    }

    assert_eq!(
        collect_send_params(vec![
            SendParam::Named("k".to_owned(), gint(1)),
            SendParam::Named("k".to_owned(), gint(2)),
        ]),
        Err(S::DuplicateKey {
            key: "k".to_owned()
        })
    );
    assert_eq!(
        send_params(vec![
            SendParam::Named("k".to_owned(), gint(1)),
            SendParam::Positional(gint(2)),
        ]),
        Err(S::PositionalAfterNamed)
    );
    assert_eq!(
        query_spec(gstring("ch"), vec![gint(1)], vec![SendParam::Positional(
            gint(2)
        )]),
        Err(S::PositionalQueryParam)
    );
    assert_eq!(
        query_spec(gstring("ch"), vec![gint(1)], vec![
            SendParam::Named("k".to_owned(), gint(3)),
            SendParam::Named("results".to_owned(), gstring("all")),
        ]),
        Err(S::KAndResults)
    );
    assert_eq!(
        collect_query_params(vec![
            SendParam::Named("p".to_owned(), gint(3)),
            SendParam::Named("p".to_owned(), gint(4)),
        ]),
        Err(S::DuplicateKey {
            key: "p".to_owned()
        })
    );
    let both: BTreeMap<String, Par> = [("k", gint(1)), ("results", gstring("all"))]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    assert_eq!(check_query_params(&both), Err(S::KAndResults));
    // A query takes a threshold or a radius, not both (decision 56).
    assert_eq!(
        query_spec(gstring("ch"), vec![gint(1)], vec![
            SendParam::Named("radius".to_owned(), gint(2)),
            SendParam::Named("threshold".to_owned(), gstring("x")),
        ]),
        Err(S::ConflictingParams)
    );
    let bounds: BTreeMap<String, Par> = [("radius", gint(1)), ("threshold", gint(0))]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    assert_eq!(check_query_params(&bounds), Err(S::ConflictingParams));
    let radius_only: BTreeMap<String, Par> = [("k", gint(1)), ("radius", gint(1))]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    assert_eq!(check_query_params(&radius_only), Ok(()));
    assert_eq!(
        query_spec(gstring("ch"), Vec::new(), Vec::new()),
        Err(S::EmptyQuery)
    );
    assert_eq!(
        encode(&ReifiedConstruct::Params(SendParams::default())),
        Err(ReifiedEncodeError::EmptyParams)
    );
    assert_eq!(S::from(ReifiedEncodeError::EmptyParams), S::EmptyParams);

    let new = new_term(2, vec!["rho:io:stdout".to_owned()], Par::default());
    assert_eq!(
        new_space(new.clone(), vec![None, None, None]),
        Err(S::NoAnnotation)
    );
    assert_eq!(
        new_space(new.clone(), vec![Some(gint(1)), None]),
        Err(S::AnnotationCount {
            annotations: 2,
            bind_count: 3,
            uris: 1
        })
    );
    assert_eq!(
        new_space(new.clone(), vec![Some(gint(1)), None, Some(gint(2))]),
        Err(S::SpaceTypeOnUriBinding)
    );
    assert_eq!(
        new_space(new.clone(), vec![None, None, Some(gint(2))]),
        Err(S::SpaceTypeOnUriBinding)
    );
    let mut negative = new.clone();
    negative.bind_count = -1;
    assert_eq!(
        new_space(negative.clone(), vec![Some(gint(1))]),
        Err(S::AnnotationCount {
            annotations: 1,
            bind_count: -1,
            uris: 1
        })
    );
    assert_eq!(
        encode_new_space(&NewSpace {
            annotations: vec![Some(gint(1))],
            new: negative
        }),
        Err(ReifiedEncodeError::AnnotationCount {
            annotations: 1,
            bind_count: -1,
            uris: 1
        })
    );
    assert!(new_space(new, vec![Some(gint(1)), None, None]).is_ok());

    // The value ranges of plan decision 21 and the design ledger.
    let double = |value: f64| {
        reified_strategies::expr_par(ExprInstance::GDouble(value.to_bits()), Vec::new(), false)
    };
    let rational = |numerator: Vec<u8>, denominator: Vec<u8>| {
        reified_strategies::expr_par(
            ExprInstance::GBigRat(GBigRational {
                numerator,
                denominator,
            }),
            Vec::new(),
            false,
        )
    };
    // A fixed-point threshold (decision 56): `unscaled / 10^scale`, with the unscaled value as
    // big-endian two's complement bytes.
    let fixed = |unscaled: Vec<u8>, scale: u32| {
        reified_strategies::expr_par(
            ExprInstance::GFixedPoint(GFixedPoint { unscaled, scale }),
            Vec::new(),
            false,
        )
    };
    // 10^20 and 10^20 + 1.
    let ten_to_twenty = vec![0x05, 0x6B, 0xC7, 0x5E, 0x2D, 0x63, 0x10, 0x00, 0x00];
    let above_ten_to_twenty = vec![0x05, 0x6B, 0xC7, 0x5E, 0x2D, 0x63, 0x10, 0x00, 0x01];
    let accepted = [
        ("priority", gint(0)),
        ("priority", gint(7)),
        ("k", gint(1)),
        ("k", gint(10)),
        ("results", gstring("all")),
        ("threshold", double(0.0)),
        ("threshold", double(0.85)),
        ("threshold", double(1.0)),
        ("threshold", double(-0.0)),
        ("threshold", rational(vec![1], vec![2])),
        ("threshold", rational(vec![0], vec![5])),
        ("threshold", rational(vec![0x00, 0x80], vec![0x00, 0xFF])),
        ("threshold", rational(vec![5], vec![5])),
        ("threshold", fixed(vec![5], 1)),
        ("threshold", fixed(vec![1], 0)),
        ("threshold", fixed(vec![0], 3)),
        ("threshold", fixed(Vec::new(), 2)),
        ("threshold", fixed(vec![100], 2)),
        ("threshold", fixed(vec![0x00, 0xFF], 3)),
        ("threshold", fixed(vec![1], 4096)),
        ("threshold", fixed(ten_to_twenty, 20)),
        ("ttl", gstring("anything")),
    ];
    for (key, value) in accepted {
        assert_eq!(
            validate_param_value(key, &value),
            Ok(()),
            "{key} = {value:?}"
        );
    }
    for metric in METRIC_VOCABULARY {
        assert_eq!(validate_param_value("metric", &gstring(metric)), Ok(()));
    }
    let rejected = [
        ("priority", gint(-1)),
        ("priority", gstring("0")),
        ("k", gint(0)),
        ("k", gbool(true)),
        ("results", gstring("some")),
        ("results", gint(1)),
        ("threshold", double(-0.1)),
        ("threshold", double(1.1)),
        ("threshold", double(f64::NAN)),
        ("threshold", double(f64::INFINITY)),
        ("threshold", gint(1)),
        ("threshold", rational(vec![3], vec![2])),
        ("threshold", rational(vec![0xFF], vec![2])),
        ("threshold", rational(vec![0x00, 0xFF], vec![0x00, 0x80])),
        ("threshold", rational(vec![1], vec![0])),
        ("threshold", fixed(vec![0xFF], 1)),
        ("threshold", fixed(vec![11], 1)),
        ("threshold", fixed(vec![101], 2)),
        ("threshold", fixed(vec![2], 0)),
        ("threshold", fixed(above_ten_to_twenty, 20)),
        ("metric", gstring("cos")),
        ("metric", gint(1)),
    ];
    for (key, value) in rejected {
        match validate_param_value(key, &value) {
            Err(S::OutOfRange { key: named, .. }) => assert_eq!(named, key),
            other => panic!("{key} = {value:?} gave {other:?}"),
        }
    }
}

struct CorpusTerm {
    name: String,
    par: Par,
    prost: Vec<u8>,
    bincode: Vec<u8>,
    stable_hash: Vec<u8>,
}

fn hex_field(term: &Value, field: &str) -> Vec<u8> {
    hex::decode(
        term[field]
            .as_str()
            .unwrap_or_else(|| panic!("the corpus term has no {field} string")),
    )
    .unwrap_or_else(|error| panic!("the {field} field is not hex: {error}"))
}

/// The terms of section R7 of the HEAD corpus `ordinary-execution-v1.json`.
fn corpus_terms() -> Vec<CorpusTerm> {
    let text = fs::read_to_string(repository_root().join(ORDINARY_CORPUS))
        .expect("read the ordinary-execution corpus");
    let corpus: Value = serde_json::from_str(&text).expect("the corpus is JSON");
    let terms = corpus["r7_term_prost_and_bincode_bytes"]["terms"]
        .as_object()
        .expect("section R7 maps term names to terms");
    terms
        .iter()
        .map(|(name, term)| {
            let prost = hex_field(term, "prost");
            CorpusTerm {
                name: name.clone(),
                par: Par::decode(prost.as_slice()).expect("a corpus term decodes as a Par"),
                prost,
                bincode: hex_field(term, "bincode"),
                stable_hash: hex_field(term, "stable_hash"),
            }
        })
        .collect()
}

/// Item 16: the 20 ordinary terms of the HEAD corpus keep their prost bytes, hold no marker,
/// and are inert for every decoder.
#[test]
fn ordinary_prost_unchanged() {
    let terms = corpus_terms();
    assert_eq!(terms.len(), 20);
    for term in &terms {
        assert_eq!(term.par.encode_to_vec(), term.prost, "{}", term.name);
        assert!(
            !contains_marker(&term.par),
            "{} holds a marker id",
            term.name
        );
        assert_eq!(sole_marker_tuple(&term.par), None);
        assert_eq!(decode_query_source(&term.par), Ok(None));
        assert_eq!(decode_free_value(&term.par), Ok(None));
        for sent in &term.par.sends {
            assert_eq!(split_trailing_params(&sent.data), Ok(None), "{}", term.name);
        }
        for expr in &term.par.exprs {
            if let Some(ExprInstance::ETupleBody(tuple)) = &expr.expr_instance {
                assert!(!is_process_marker_tuple(tuple), "{}", term.name);
            }
        }
    }
}

/// Item 16: the 20 ordinary terms keep their bincode bytes and their store hashes.
#[test]
fn ordinary_bincode_unchanged() {
    let terms = corpus_terms();
    assert_eq!(terms.len(), 20);
    for term in &terms {
        let bincode = bincode::serialize(&term.par).expect("bincode-serialize the Par");
        assert_eq!(bincode, term.bincode, "{}", term.name);
        let stable_hash = hash(&term.par);
        assert_eq!(stable_hash.bytes(), term.stable_hash, "{}", term.name);
        assert_eq!(stable_hash, Blake2b256Hash::new(&bincode), "{}", term.name);
    }
}

fn proto_files(root: &Path, directory: &Path, found: &mut BTreeSet<String>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("read a directory entry");
        let file_type = entry.file_type().expect("read a file type");
        let path = entry.path();
        let name = entry.file_name();
        if file_type.is_dir() && name != "target" && name != ".git" {
            proto_files(root, &path, found);
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "proto")
        {
            let relative = path
                .strip_prefix(root)
                .expect("a found file lies under the root");
            found.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Item 16: every `.proto` file, `models/build.rs` and `serde_helpers.rs` equal the files at
/// the base revision, and no `.proto` file was added or removed.
#[test]
fn protobuf_schema_unchanged_vs_head() {
    let root = repository_root();
    let text = fs::read_to_string(root.join(SCHEMA_PINS)).expect("read the schema pins");
    let pins: Value = serde_json::from_str(&text).expect("the schema pins are JSON");
    assert_eq!(pins["base_revision"].as_str(), Some(SCHEMA_BASE));
    let files = pins["files"]
        .as_object()
        .expect("the pins map paths to digests");
    assert_eq!(files.len(), 18);
    for (path, digest) in files {
        let bytes =
            fs::read(root.join(path)).unwrap_or_else(|error| panic!("read {path}: {error}"));
        assert_eq!(
            hex::encode(Blake2b256::hash(bytes)),
            digest.as_str().expect("a digest is a string"),
            "{path} differs from the base revision"
        );
    }
    let pinned: BTreeSet<String> = files
        .keys()
        .filter(|path| path.ends_with(".proto"))
        .cloned()
        .collect();
    assert_eq!(pinned.len(), 16);
    let mut found = BTreeSet::new();
    proto_files(&root, &root, &mut found);
    assert_eq!(found, pinned);
    assert!(files.contains_key("models/build.rs"));
    assert!(files.contains_key("models/src/rust/serde_helpers.rs"));
}
