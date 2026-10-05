//! Shared proptest strategies of the Reified RSpaces encoding tests (P2).
//!
//! `models/tests/reified_encoding.rs` and `rholang/tests/reified_marker_recognition.rs`
//! include this file with `#[path]`. Each generated `Par` is sorted and carries the
//! `locally_free` and `connective_used` values that the normalizer computes. A map value thus
//! keeps its form through `ParMap::create_from_vec`, and each encoding is canonical.

#![allow(dead_code)]

use std::collections::BTreeMap;

use models::create_bit_vector;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Connective, EList, ETuple, EVar, Expr, GPrivate, GUnforgeable, New, Par, Send, Var,
};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::reified::builders::push_process_expr;
use models::rust::reified::encoding::{
    encode, encode_free, encode_params, encode_query, FreeTheory, Marker, NewSpace, QuerySpec,
    ReifiedConstruct, SendParams, UseBlock,
};
use models::rust::reified::validation::{is_param_key, is_theory_name};
use models::rust::rholang::implicits::concatenate_pars;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::union;
use proptest::collection::{btree_map, vec};
use proptest::option;
use proptest::prelude::*;
use proptest::sample::select;
use prost::Message;

/// URNs for the URI binders of a generated `New`.
pub const URIS: [&str; 3] = ["rho:io:stderr", "rho:io:stdout", "rho:registry:lookup"];

/// A `Par` that holds exactly one expression, as `prepend_expr` builds it.
pub fn expr_par(instance: ExprInstance, locally_free: Vec<u8>, connective_used: bool) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

pub fn gint(value: i64) -> Par { expr_par(ExprInstance::GInt(value), Vec::new(), false) }

pub fn gstring(value: &str) -> Par {
    expr_par(ExprInstance::GString(value.to_owned()), Vec::new(), false)
}

pub fn gbool(value: bool) -> Par { expr_par(ExprInstance::GBool(value), Vec::new(), false) }

/// An unforgeable name with the id `id`.
pub fn private_name(id: Vec<u8>) -> Par {
    Par {
        unforgeables: vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        }],
        ..Par::default()
    }
}

/// The bound variable `index` at depth 0, with its `locally_free` bit.
pub fn bound_var(index: i32) -> Par {
    let level = usize::try_from(index).expect("a bound variable index is not negative");
    expr_par(
        ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(index)),
            }),
        }),
        create_bit_vector(&[level]),
        false,
    )
}

/// The process `Int`: a connective that sets `connective_used`.
pub fn int_connective() -> Par {
    Par {
        connectives: vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnInt(true)),
        }],
        connective_used: true,
        ..Par::default()
    }
}

/// The `locally_free` union and the `connective_used` disjunction of `pars`.
pub fn bits(pars: &[Par]) -> (Vec<u8>, bool) {
    pars.iter().fold(
        (Vec::new(), false),
        |(locally_free, connective_used), par| {
            (
                union(locally_free, par.locally_free.clone()),
                connective_used || par.connective_used,
            )
        },
    )
}

pub fn list(ps: Vec<Par>) -> Par {
    let (locally_free, connective_used) = bits(&ps);
    expr_par(
        ExprInstance::EListBody(EList {
            ps,
            locally_free: locally_free.clone(),
            connective_used,
            remainder: None,
        }),
        locally_free,
        connective_used,
    )
}

pub fn tuple(ps: Vec<Par>) -> Par {
    let (locally_free, connective_used) = bits(&ps);
    expr_par(
        ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free: locally_free.clone(),
            connective_used,
        }),
        locally_free,
        connective_used,
    )
}

/// A map with string keys. A later entry replaces an earlier entry with the same key.
pub fn map(entries: Vec<(String, Par)>) -> Par {
    let entries: BTreeMap<String, Par> = entries.into_iter().collect();
    let pairs = entries
        .into_iter()
        .map(|(key, value)| (gstring(&key), value))
        .collect();
    let emap = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(pairs));
    let (locally_free, connective_used) = (emap.locally_free.clone(), emap.connective_used);
    expr_par(ExprInstance::EMapBody(emap), locally_free, connective_used)
}

/// The process `chan!(data…)`.
pub fn send(chan: Par, data: Vec<Par>) -> Par {
    let (data_free, data_connectives) = bits(&data);
    let locally_free = union(chan.locally_free.clone(), data_free);
    let connective_used = chan.connective_used || data_connectives;
    Par {
        sends: vec![Send {
            chan: Some(chan),
            data,
            persistent: false,
            locally_free: locally_free.clone(),
            connective_used,
        }],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

pub fn sorted(par: Par) -> Par { ParSortMatcher::sort_match(&par).term }

fn leaf(variables: bool) -> BoxedStrategy<Par> {
    let ground = prop_oneof![
        (-3i64..1000).prop_map(gint),
        "[a-z]{0,5}".prop_map(|value| gstring(&value)),
        any::<bool>().prop_map(gbool),
        vec(any::<u8>(), 32).prop_map(private_name),
        Just(Par::default()),
    ];
    match variables {
        true => prop_oneof![
            4 => ground,
            1 => (0i32..3).prop_map(bound_var),
            1 => Just(int_connective()),
        ]
        .boxed(),
        false => ground.boxed(),
    }
}

fn par_with(variables: bool) -> BoxedStrategy<Par> {
    leaf(variables)
        .prop_recursive(3, 24, 3, |inner| {
            prop_oneof![
                vec(inner.clone(), 0..3).prop_map(list),
                vec(inner.clone(), 1..3).prop_map(tuple),
                vec(("[a-z]{1,3}", inner.clone()), 0..3).prop_map(map),
                (inner.clone(), vec(inner.clone(), 0..3)).prop_map(|(chan, data)| send(chan, data)),
                (inner.clone(), inner).prop_map(|(left, right)| concatenate_pars(left, right)),
            ]
        })
        .prop_map(sorted)
        .boxed()
}

/// A sorted `Par` with bound variables and connectives.
pub fn arb_par() -> BoxedStrategy<Par> { par_with(true) }

/// A sorted ground `Par`: no variable and no connective.
pub fn arb_ground_par() -> BoxedStrategy<Par> { par_with(false) }

/// A key that the parser accepts for a named param.
pub fn arb_param_key() -> BoxedStrategy<String> {
    prop_oneof![
        select(vec![
            "priority",
            "k",
            "results",
            "threshold",
            "metric",
            "Z",
            "_a",
            "catch",
            "in",
            "x'",
        ])
        .prop_map(String::from),
        "[a-zA-Z][a-zA-Z0-9_']{0,5}".prop_filter("a param key", |key| is_param_key(key)),
    ]
    .boxed()
}

/// A theory name that the parser accepts after `free`.
pub fn arb_theory_name() -> BoxedStrategy<String> {
    prop_oneof![
        select(vec!["Nat", "Int", "String", "Bool", "Any", "catch", "try"]).prop_map(String::from),
        "[A-Z][a-zA-Z0-9_]{0,5}".prop_filter("a theory name", |name| is_theory_name(name)),
    ]
    .boxed()
}

pub fn arb_named(values: BoxedStrategy<Par>) -> BoxedStrategy<BTreeMap<String, Par>> {
    btree_map(arb_param_key(), values, 0..4).boxed()
}

/// A normalized `New` with `plain` plain binders, the URI binders `uris` and the body `body`.
pub fn new_term(plain: usize, uris: Vec<String>, body: Par) -> New {
    let bind_count = plain + uris.len();
    let locally_free = body
        .locally_free
        .get(bind_count..)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    New {
        bind_count: i32::try_from(bind_count).expect("a generated New has few binders"),
        p: Some(body),
        uri: uris,
        injections: BTreeMap::new(),
        locally_free,
    }
}

pub fn arb_use_block(values: BoxedStrategy<Par>) -> BoxedStrategy<UseBlock> {
    (values.clone(), values)
        .prop_map(|(space, body)| UseBlock { space, body })
        .boxed()
}

pub fn arb_new_space(values: BoxedStrategy<Par>) -> BoxedStrategy<NewSpace> {
    (
        1usize..4,
        vec(select(URIS.to_vec()), 0..3),
        values.clone(),
        any::<usize>(),
    )
        .prop_flat_map(move |(plain, uris, body, forced)| {
            let mut uris: Vec<String> = uris.into_iter().map(String::from).collect();
            uris.sort();
            uris.dedup();
            (
                vec(option::of(values.clone()), plain),
                values.clone(),
                Just((plain, uris, body, forced % plain)),
            )
        })
        .prop_map(
            |(mut annotations, annotation, (plain, uris, body, forced))| {
                annotations[forced] = Some(annotation);
                NewSpace {
                    annotations,
                    new: new_term(plain, uris, body),
                }
            },
        )
        .boxed()
}

pub fn arb_send_params(values: BoxedStrategy<Par>) -> BoxedStrategy<SendParams> {
    (vec(values.clone(), 0..3), arb_named(values))
        .prop_filter("a send with params", |(positional, named)| {
            !(positional.is_empty() && named.is_empty())
        })
        .prop_map(|(positional, named)| SendParams { positional, named })
        .boxed()
}

pub fn arb_query_spec(values: BoxedStrategy<Par>) -> BoxedStrategy<QuerySpec> {
    (values.clone(), vec(values.clone(), 0..3), arb_named(values))
        .prop_filter("a query with a payload or params", |(_, payload, named)| {
            !(payload.is_empty() && named.is_empty())
        })
        .prop_map(|(channel, payload, named)| QuerySpec {
            channel,
            payload,
            named,
        })
        .boxed()
}

pub fn arb_free_theory(values: BoxedStrategy<Par>) -> BoxedStrategy<FreeTheory> {
    (arb_theory_name(), vec(values, 0..3))
        .prop_map(|(name, args)| FreeTheory { name, args })
        .boxed()
}

/// A well-formed construct of any marker over the values `values`.
pub fn arb_construct_with(values: BoxedStrategy<Par>) -> BoxedStrategy<ReifiedConstruct> {
    prop_oneof![
        arb_use_block(values.clone()).prop_map(ReifiedConstruct::Use),
        arb_new_space(values.clone()).prop_map(ReifiedConstruct::NewSpace),
        arb_send_params(values.clone()).prop_map(ReifiedConstruct::Params),
        arb_query_spec(values.clone()).prop_map(ReifiedConstruct::Query),
        arb_free_theory(values).prop_map(ReifiedConstruct::Free),
    ]
    .boxed()
}

pub fn arb_construct() -> BoxedStrategy<ReifiedConstruct> { arb_construct_with(arb_par()) }

pub fn arb_ground_construct() -> BoxedStrategy<ReifiedConstruct> {
    arb_construct_with(arb_ground_par())
}

/// The encoding of `construct` in its own position: the process `Par` of a USE or NEWSPACE
/// tuple, the trailing data element of PARAMS, the bind source of QUERY and the value of
/// FREE.
pub fn position_par(construct: &ReifiedConstruct) -> Par {
    match construct {
        ReifiedConstruct::Use(_) | ReifiedConstruct::NewSpace(_) => {
            match encode(construct)
                .expect("a generated construct is well formed")
                .expr_instance
            {
                Some(ExprInstance::ETupleBody(tuple)) => push_process_expr(Par::default(), tuple),
                other => panic!("encode returned {other:?} instead of a tuple"),
            }
        }
        ReifiedConstruct::Params(params) => encode_params(params)
            .expect("generated params are well formed")
            .expect("generated params are not empty"),
        ReifiedConstruct::Query(query) => {
            encode_query(query).expect("a generated query is well formed")
        }
        ReifiedConstruct::Free(theory) => {
            encode_free(theory).expect("a generated theory is well formed")
        }
    }
}

/// The marker tuple of a `Par` that holds exactly one tuple expression.
pub fn sole_tuple(par: &Par) -> Option<&ETuple> {
    match par.exprs.as_slice() {
        [Expr {
            expr_instance: Some(ExprInstance::ETupleBody(tuple)),
        }] => Some(tuple),
        _ => None,
    }
}

/// True when the prost bytes of `par` hold the `GPrivate` id of a marker anywhere.
pub fn contains_marker(par: &Par) -> bool {
    let bytes = par.encode_to_vec();
    Marker::ALL.iter().any(|marker| {
        let [tag, index] = marker.id();
        let pattern = [0x0a, 0x02, tag, index];
        bytes.windows(pattern.len()).any(|window| window == pattern)
    })
}
