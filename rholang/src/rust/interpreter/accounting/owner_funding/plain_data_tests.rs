//! Added by G3 (DR-135, S3b H8): the tests of `plain_data`, the shape check of
//! the replies of the native recorded processes.
//!
//! - Every reply shape that a service builds is plain data, and so is the
//!   failure reply.
//! - The forged shapes of the review (a signed receive bind, a bare private
//!   name, a `new` with injections, a cost signed term, a cost stack, a bundle,
//!   a path map, a zipper and an expr with no instance) are not, and neither
//!   are the other processes, operators, variables, methods, connectives and
//!   collections with a remainder, a missing map key or value, or a
//!   connective. A forged shape below plain collections is found too.
//! - `plain_data` agrees with a recursive oracle that states the definition
//!   directly.
//! - The walk and the teardown of a reply 100,000 levels deep stay within a
//!   small stack.

use std::collections::{BTreeMap, HashMap};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, CostSignature, CostSignedTerm, CostStack, EList, EMap, EMethod, EPathMap,
    EPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational, GFixedPoint, GPrivate, GUnforgeable,
    If, KeyValuePair, Match, New, Par, Receive, ReceiveBind, Send, Var,
};
use models::rust::rholang::dismantle::dismantle_term;
use models::rust::rholang::implicits::concatenate_pars;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::io::errors::FserrCode;
use crate::rust::interpreter::io::response;
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoByteArray, RhoList, RhoMap, RhoNil, RhoNumber, RhoString, RhoTuple2,
};

/// The depth of the deep tests.
const DEEP_LEVELS: usize = 100_000;

/// The stack of the thread that walks and drops a deep reply.
const SMALL_STACK_BYTES: usize = 256 * 1024;

fn expr_par(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn ground_signature() -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Ground(vec![7])),
    }
}

fn bound_var() -> Var {
    Var {
        var_instance: Some(VarInstance::BoundVar(0)),
    }
}

fn private_name() -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1; 32] })),
    }])
}

/// One Par for each of the 15 grounds.
fn grounds() -> Vec<(&'static str, Par)> {
    vec![
        ("GBool", expr_par(ExprInstance::GBool(true))),
        ("GInt", expr_par(ExprInstance::GInt(-3))),
        ("GString", expr_par(ExprInstance::GString("s".to_owned()))),
        ("GUri", expr_par(ExprInstance::GUri("rho:id:x".to_owned()))),
        ("GByteArray", expr_par(ExprInstance::GByteArray(vec![1, 2]))),
        (
            "GDouble",
            expr_par(ExprInstance::GDouble(1.5_f64.to_bits())),
        ),
        ("GBigInt", expr_par(ExprInstance::GBigInt(vec![0x7f]))),
        (
            "GBigRat",
            expr_par(ExprInstance::GBigRat(GBigRational {
                numerator: vec![1],
                denominator: vec![3],
            })),
        ),
        (
            "GFixedPoint",
            expr_par(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![5],
                scale: 2,
            })),
        ),
        ("GUint64", expr_par(ExprInstance::GUint64(u64::MAX))),
        (
            "GFloat32",
            expr_par(ExprInstance::GFloat32(2.5_f32.to_bits())),
        ),
        ("GInt32", expr_par(ExprInstance::GInt32(-7))),
        ("GUint32", expr_par(ExprInstance::GUint32(7))),
        (
            "GUint16",
            expr_par(ExprInstance::GUint16(u32::from(u16::MAX))),
        ),
        ("GUint8", expr_par(ExprInstance::GUint8(u32::from(u8::MAX)))),
    ]
}

/// The reply shapes of the services and the failure reply. Each comes from
/// the builder that the service uses.
fn service_replies() -> Vec<(&'static str, Par)> {
    let metadata = RhoMap::create_par(HashMap::from([
        (
            RhoString::create_par("n".to_owned()),
            RhoNumber::create_par(3),
        ),
        (
            RhoString::create_par("b".to_owned()),
            RhoBoolean::create_par(true),
        ),
        (
            RhoString::create_par("s".to_owned()),
            RhoString::create_par("v".to_owned()),
        ),
    ]));
    let entry = |metadata: Par| {
        RhoTuple2::create_par((RhoString::create_par("document".to_owned()), metadata))
    };
    let query_result = RhoMap::create_par(HashMap::from([
        (
            RhoString::create_par("id1".to_owned()),
            entry(metadata.clone()),
        ),
        (
            RhoString::create_par("id2".to_owned()),
            entry(RhoNil::create_par()),
        ),
    ]));
    vec![
        ("nil (grpc_tell, chroma_create_collection)", Par::default()),
        (
            "string (gpt4, dalle3, ollama, chroma upsert and delete)",
            RhoString::create_par("answer".to_owned()),
        ),
        (
            "byte array (text_to_audio)",
            RhoByteArray::create_par(vec![0, 1, 2]),
        ),
        (
            "list of strings (ollama_models)",
            RhoList::create_par(vec![
                RhoString::create_par("m1".to_owned()),
                RhoString::create_par("m2".to_owned()),
            ]),
        ),
        ("collection metadata (chroma_get_collection_meta)", metadata),
        (
            "query results (chroma_query)",
            RhoList::create_par(vec![query_result]),
        ),
        (
            "failure reply",
            response::err(FserrCode("EXT_FAILED"), "gpt4: external service failed"),
        ),
    ]
}

/// The forged replies of the review, each of which a deploy could turn into
/// a capability or a signature.
pub(crate) fn review_forgeries() -> Vec<(&'static str, Par)> {
    vec![
        ("a signed receive bind", Par {
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![Par::default()],
                    source: Some(RhoString::create_par("c".to_owned())),
                    cost_signature: Some(ground_signature()),
                    ..Default::default()
                }],
                body: Some(Par::default()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ("a bare private name", private_name()),
        ("a new with injections", Par {
            news: vec![New {
                bind_count: 1,
                p: Some(Par::default()),
                uri: vec!["rho:io:stdout".to_owned()],
                injections: BTreeMap::from([("rho:io:stdout".to_owned(), private_name())]),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ("a cost signed term", Par {
            cost_signed_terms: vec![CostSignedTerm {
                body: Some(Par::default()),
                signature: Some(ground_signature()),
                held_price: None,
            }],
            ..Default::default()
        }),
        ("a cost stack", Par {
            cost_stacks: vec![CostStack {
                cells: vec![ground_signature()],
            }],
            ..Default::default()
        }),
        ("a bundle", Par {
            bundles: vec![Bundle {
                body: Some(RhoString::create_par("b".to_owned())),
                write_flag: true,
                read_flag: true,
            }],
            ..Default::default()
        }),
        (
            "a path map",
            expr_par(ExprInstance::EPathmapBody(EPathMap::default())),
        ),
        (
            "a zipper",
            expr_par(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::default()),
                ..Default::default()
            })),
        ),
        (
            "an expr with no instance",
            Par::default().with_exprs(vec![Expr {
                expr_instance: None,
            }]),
        ),
    ]
}

/// Other shapes that are not plain data.
fn other_non_plain() -> Vec<(&'static str, Par)> {
    let list = |connective_used, remainder| {
        expr_par(ExprInstance::EListBody(EList {
            ps: vec![RhoNumber::create_par(1)],
            connective_used,
            remainder,
            ..Default::default()
        }))
    };
    let set = |connective_used, remainder| {
        expr_par(ExprInstance::ESetBody(ESet {
            ps: vec![RhoNumber::create_par(1)],
            connective_used,
            remainder,
            ..Default::default()
        }))
    };
    let map = |key: Option<Par>, value: Option<Par>, connective_used, remainder| {
        expr_par(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair { key, value }],
            connective_used,
            remainder,
            ..Default::default()
        }))
    };
    let one = || Some(RhoNumber::create_par(1));
    vec![
        ("a send", Par {
            sends: vec![Send {
                chan: Some(RhoString::create_par("c".to_owned())),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ("an unsigned receive", Par {
            receives: vec![Receive::default()],
            ..Default::default()
        }),
        ("a match", Par {
            matches: vec![Match::default()],
            ..Default::default()
        }),
        ("a conditional", Par {
            conditionals: vec![If::default()],
            ..Default::default()
        }),
        ("a connective", Par {
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::VarRefBody(Default::default())),
            }],
            ..Default::default()
        }),
        ("a Par with a connective", Par {
            connective_used: true,
            ..RhoNumber::create_par(1)
        }),
        (
            "a variable",
            expr_par(ExprInstance::EVarBody(EVar {
                v: Some(bound_var()),
            })),
        ),
        (
            "a method",
            expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_owned(),
                target: Some(RhoList::create_par(vec![RhoNumber::create_par(1)])),
                arguments: vec![RhoNumber::create_par(0)],
                ..Default::default()
            })),
        ),
        (
            "an operator",
            expr_par(ExprInstance::EPlusBody(EPlus {
                p1: Some(RhoNumber::create_par(1)),
                p2: Some(RhoNumber::create_par(2)),
            })),
        ),
        ("a list with a remainder", list(false, Some(bound_var()))),
        ("a list with a connective", list(true, None)),
        ("a set with a remainder", set(false, Some(bound_var()))),
        ("a set with a connective", set(true, None)),
        (
            "a tuple with a connective",
            expr_par(ExprInstance::ETupleBody(ETuple {
                ps: vec![RhoNumber::create_par(1)],
                connective_used: true,
                ..Default::default()
            })),
        ),
        (
            "a map with a remainder",
            map(one(), one(), false, Some(bound_var())),
        ),
        ("a map with a connective", map(one(), one(), true, None)),
        ("a map pair without a key", map(None, one(), false, None)),
        ("a map pair without a value", map(one(), None, false, None)),
    ]
}

/// `forged` below a plain list, tuple, set and map, and beside a ground in one
/// Par.
fn nested(forged: &Par) -> Vec<Par> {
    vec![
        RhoList::create_par(vec![RhoNumber::create_par(1), forged.clone()]),
        RhoTuple2::create_par((RhoNumber::create_par(1), forged.clone())),
        expr_par(ExprInstance::ESetBody(ESet {
            ps: vec![forged.clone()],
            ..Default::default()
        })),
        expr_par(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(RhoString::create_par("k".to_owned())),
                value: Some(forged.clone()),
            }],
            ..Default::default()
        })),
        expr_par(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(forged.clone()),
                value: Some(RhoNumber::create_par(1)),
            }],
            ..Default::default()
        })),
        concatenate_pars(RhoNumber::create_par(1), forged.clone()),
    ]
}

/// Every reply shape of the services is plain data, and so is each ground.
#[test]
fn plain_data_accepts_every_service_reply_shape() {
    for (label, reply) in grounds().into_iter().chain(service_replies()) {
        assert!(plain_data(&reply), "{label} is plain data");
        assert!(
            plain_data(&RhoList::create_par(vec![reply.clone(), reply])),
            "a list of {label} is plain data"
        );
    }
}

/// The forged replies of the review are not plain data, nor are the other
/// shapes that carry code, a name, a signature, an operator, a variable or a
/// connective. The walk finds each of them below plain collections too.
#[test]
fn plain_data_rejects_code_names_signatures_and_open_collections() {
    for (label, forged) in review_forgeries().into_iter().chain(other_non_plain()) {
        assert!(!plain_data(&forged), "{label} is not plain data");
        for (index, nested) in nested(&forged).iter().enumerate() {
            assert!(
                !plain_data(nested),
                "{label} in nesting {index} is not plain data"
            );
        }
    }
}

/// The definition of plain data, stated directly, as a recursive oracle.
fn plain_oracle(par: &Par) -> bool {
    par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.matches.is_empty()
        && par.unforgeables.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_signed_terms.is_empty()
        && par.cost_stacks.is_empty()
        && !par.connective_used
        && par.exprs.iter().all(|expr| match &expr.expr_instance {
            Some(ExprInstance::EListBody(list)) => {
                !list.connective_used
                    && list.remainder.is_none()
                    && list.ps.iter().all(plain_oracle)
            }
            Some(ExprInstance::ETupleBody(tuple)) => {
                !tuple.connective_used && tuple.ps.iter().all(plain_oracle)
            }
            Some(ExprInstance::ESetBody(set)) => {
                !set.connective_used && set.remainder.is_none() && set.ps.iter().all(plain_oracle)
            }
            Some(ExprInstance::EMapBody(map)) => {
                !map.connective_used
                    && map.remainder.is_none()
                    && map.kvs.iter().all(|pair| match (&pair.key, &pair.value) {
                        (Some(key), Some(value)) => plain_oracle(key) && plain_oracle(value),
                        _ => false,
                    })
            }
            Some(instance) => is_ground(instance),
            None => false,
        })
}

fn is_ground(instance: &ExprInstance) -> bool {
    matches!(
        instance,
        ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_)
            | ExprInstance::GUint64(_)
            | ExprInstance::GFloat32(_)
            | ExprInstance::GInt32(_)
            | ExprInstance::GUint32(_)
            | ExprInstance::GUint16(_)
            | ExprInstance::GUint8(_)
    )
}

/// A leaf of a random reply: a ground, or now and then a shape that is not
/// plain data.
fn arb_leaf() -> impl Strategy<Value = Par> {
    let grounds: Vec<Par> = grounds().into_iter().map(|(_, par)| par).collect();
    let forged: Vec<Par> = review_forgeries()
        .into_iter()
        .chain(other_non_plain())
        .map(|(_, par)| par)
        .collect();
    prop_oneof![
        6 => prop::sample::select(grounds),
        2 => any::<i64>().prop_map(RhoNumber::create_par),
        1 => prop::sample::select(forged),
    ]
}

/// A random reply: plain collections of leaves, with now and then a
/// connective, a remainder, a missing map key or value, or several exprs in
/// one Par.
fn arb_reply() -> impl Strategy<Value = Par> {
    arb_leaf().prop_recursive(4, 48, 4, |inner| {
        let children = prop::collection::vec(inner.clone(), 0..4);
        let flag = || prop::bool::weighted(0.08);
        let remainder = || prop::option::weighted(0.08, Just(bound_var()));
        prop_oneof![
            (children.clone(), flag(), remainder()).prop_map(|(ps, connective_used, remainder)| {
                expr_par(ExprInstance::EListBody(EList {
                    ps,
                    connective_used,
                    remainder,
                    ..Default::default()
                }))
            }),
            (children.clone(), flag()).prop_map(|(ps, connective_used)| {
                expr_par(ExprInstance::ETupleBody(ETuple {
                    ps,
                    connective_used,
                    ..Default::default()
                }))
            }),
            (children.clone(), flag(), remainder()).prop_map(|(ps, connective_used, remainder)| {
                expr_par(ExprInstance::ESetBody(ESet {
                    ps,
                    connective_used,
                    remainder,
                    ..Default::default()
                }))
            }),
            (
                prop::collection::vec(
                    (
                        prop::option::weighted(0.95, inner.clone()),
                        prop::option::weighted(0.95, inner.clone())
                    ),
                    0..3
                ),
                flag(),
                remainder()
            )
                .prop_map(|(pairs, connective_used, remainder)| {
                    expr_par(ExprInstance::EMapBody(EMap {
                        kvs: pairs
                            .into_iter()
                            .map(|(key, value)| KeyValuePair { key, value })
                            .collect(),
                        connective_used,
                        remainder,
                        ..Default::default()
                    }))
                }),
            (children, flag()).prop_map(|(pars, connective_used)| {
                let mut par = pars.into_iter().fold(Par::default(), concatenate_pars);
                par.connective_used = par.connective_used || connective_used;
                par
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// `plain_data` agrees with the recursive oracle on random replies.
    #[test]
    fn plain_data_matches_an_oracle(reply in arb_reply()) {
        prop_assert_eq!(plain_data(&reply), plain_oracle(&reply));
    }
}

/// The random replies hold plain data and other shapes in good measure, so
/// the property checks both answers.
#[test]
fn the_random_replies_hold_both_answers() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = arb_reply();
    let mut plain = 0_usize;
    let mut other = 0_usize;
    for _ in 0..512 {
        let reply = strategy
            .new_tree(&mut runner)
            .expect("a random reply")
            .current();
        match plain_oracle(&reply) {
            true => plain += 1,
            false => other += 1,
        }
    }
    assert!(
        plain >= 64 && other >= 64,
        "plain {plain} and other {other} replies"
    );
}

/// `depth` levels of plain collections around `leaf`. The levels cycle
/// through a list, a tuple, a set and a map value. The loop builds the reply
/// from the inside out, so the construction never recurses.
fn deep_reply(depth: usize, leaf: Par) -> Par {
    let mut par = leaf;
    for level in 0..depth {
        par = match level % 4 {
            0 => RhoList::create_par(vec![par]),
            1 => expr_par(ExprInstance::ETupleBody(ETuple {
                ps: vec![par],
                ..Default::default()
            })),
            2 => expr_par(ExprInstance::ESetBody(ESet {
                ps: vec![par],
                ..Default::default()
            })),
            _ => expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(RhoNumber::create_par(1)),
                    value: Some(par),
                }],
                ..Default::default()
            })),
        };
    }
    par
}

/// The walk of a reply 100,000 levels deep, and its iterative teardown, stay
/// within a small stack. Prost decodes at most 100 levels, so the replay test
/// of `recorded_calls.rs` uses a shallow reply, and this test calls the
/// predicate directly.
#[test]
fn plain_data_walks_a_reply_100000_levels_deep_on_a_small_stack() {
    std::thread::Builder::new()
        .name("plain-data-deep".to_owned())
        .stack_size(SMALL_STACK_BYTES)
        .spawn(|| {
            // Each verdict is taken before its reply is dismantled, and the
            // asserts come after the teardown. A failed assert so never drops
            // a deep reply through the recursive derived `Drop`.
            let plain = deep_reply(DEEP_LEVELS, RhoString::create_par("leaf".to_owned()));
            let plain_verdict = plain_data(&plain);
            dismantle_term(plain);
            let forged = deep_reply(DEEP_LEVELS, private_name());
            let forged_verdict = plain_data(&forged);
            dismantle_term(forged);
            assert!(plain_verdict, "a deep plain reply is plain data");
            assert!(!forged_verdict, "a deep private name is not plain data");
        })
        .expect("spawn the deep thread")
        .join()
        // A failed assert of the thread fails the test with its own message.
        // A stack overflow aborts the process instead.
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
