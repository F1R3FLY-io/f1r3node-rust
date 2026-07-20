//! EPathMap fix P4.3 — SPLICED-vs-DIRECT differential (amendment PM-1 gate).
//!
//! The intern-aware event-hash emitter (`models::rust::spliced_event_bytes`)
//! must produce byte-identical output to the direct `bincode::serialize`
//! path for EVERY value — with the intern cells UNFILLED (the emitter must
//! then BE the direct path) and FILLED (the spliced path must reproduce it).
//! Two layers:
//!
//!   1. deterministic SHAPE coverage — a filled-cell map embedded at every
//!      container position of the PM-1 spine (datum root, EList/ETuple/ESet/
//!      EMap element, EMethod target+arg, binary-op operand, EZipper,
//!      Send/Receive/New/Match/If/Bundle/Connective interiors, nested map,
//!      BindPattern pattern, TaggedContinuation guard+body);
//!   2. a PROPTEST over arbitrary bounded trees mixing interned and
//!      uninterned maps at random positions.
//!
//! Any mismatch is a STOP (consensus surface: event hashes).

mod fixtures;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, EList, EMap, EMatches, EMethod, EPathMap,
    EPlus, ETuple, ESet, EZipper, Expr, If, KeyValuePair, ListParWithRandom, Match, MatchCase,
    New, Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var,
    connective::ConnectiveInstance, var::VarInstance,
};
use models::rust::spliced_event_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use proptest::prelude::*;

use fixtures::{e6a_index_epathmap, epathmap_par, gstring_par, nested_epathmap_value};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// A filled-cell copy of the E-6a index map (the intern rendezvous fills the
/// shadow cell; clones propagate it).
fn interned_map() -> EPathMap {
    let map = e6a_index_epathmap();
    let _ = map.intern();
    assert!(map.interned_handle().is_some(), "intern() must fill the cell");
    map
}

fn datum(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: vec![0xAA; 32],
    }
}

fn assert_spliced_eq_direct_datum(label: &str, value: &ListParWithRandom) {
    let direct = bincode::serialize(value).expect("direct bincode");
    let spliced = event_hash_bytes_list_par_with_random(value);
    assert_eq!(
        spliced, direct,
        "{label}: spliced datum bytes must equal direct bincode bytes"
    );
}

fn assert_spliced_eq_direct_pattern(label: &str, value: &BindPattern) {
    let direct = bincode::serialize(value).expect("direct bincode");
    let spliced = event_hash_bytes_bind_pattern(value);
    assert_eq!(
        spliced, direct,
        "{label}: spliced pattern bytes must equal direct bincode bytes"
    );
}

fn assert_spliced_eq_direct_continuation(label: &str, value: &TaggedContinuation) {
    let direct = bincode::serialize(value).expect("direct bincode");
    let spliced = event_hash_bytes_tagged_continuation(value);
    assert_eq!(
        spliced, direct,
        "{label}: spliced continuation bytes must equal direct bincode bytes"
    );
}

fn expr_par(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Deterministic shape coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn spliced_equals_direct_for_every_container_position() {
    let map_par = || epathmap_par(interned_map());

    // Free-variable and wildcard vars for pattern shapes.
    let free_var = Var {
        var_instance: Some(VarInstance::FreeVar(0)),
    };

    let shapes: Vec<(&str, Par)> = vec![
        ("datum-root map", map_par()),
        (
            "elist element",
            expr_par(ExprInstance::EListBody(EList {
                ps: vec![gstring_par("head"), map_par()],
                locally_free: vec![1], // exercised: must serialize as EMPTY
                connective_used: false,
                remainder: Some(free_var),
            })),
        ),
        (
            "etuple element",
            expr_par(ExprInstance::ETupleBody(ETuple {
                ps: vec![map_par(), gstring_par("tail")],
                locally_free: Vec::new(),
                connective_used: true,
            })),
        ),
        (
            "eset element",
            expr_par(ExprInstance::ESetBody(ESet {
                ps: vec![map_par()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        ),
        (
            "emap value",
            expr_par(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: Some(gstring_par("k")),
                    value: Some(map_par()),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        ),
        (
            "emethod target+argument",
            expr_par(ExprInstance::EMethodBody(EMethod {
                method_name: "readZipperAt".to_string(),
                target: Some(map_par()),
                arguments: vec![map_par(), gstring_par("arg")],
                locally_free: vec![2],
                connective_used: false,
            })),
        ),
        (
            "binary-op operand",
            expr_par(ExprInstance::EPlusBody(EPlus {
                p1: Some(map_par()),
                p2: Some(gstring_par("rhs")),
            })),
        ),
        (
            "ematches target",
            expr_par(ExprInstance::EMatchesBody(EMatches {
                target: Some(map_par()),
                pattern: Some(gstring_par("pat")),
            })),
        ),
        (
            "ezipper pathmap",
            expr_par(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(interned_map()),
                current_path: vec![vec![1, 2, 0xFF]],
                is_write_zipper: false,
                locally_free: vec![1],
                connective_used: false,
            })),
        ),
        (
            "nested map (outer UNFILLED, inner filled)",
            expr_par(ExprInstance::EPathmapBody(EPathMap::new(
                vec![map_par()],
                Vec::new(),
                false,
                None,
            ))),
        ),
        ("send data+chan", {
            let mut par = Par::default();
            par.sends = vec![Send {
                chan: Some(map_par()),
                data: vec![map_par(), gstring_par("d")],
                persistent: true,
                locally_free: vec![4],
                connective_used: false,
            }];
            par
        }),
        ("receive bind+body+condition", {
            let mut par = Par::default();
            par.receives = vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![map_par()],
                    source: Some(map_par()),
                    remainder: Some(free_var),
                    free_count: 1,
                }],
                body: Some(map_par()),
                persistent: false,
                peek: true,
                bind_count: 1,
                locally_free: vec![8],
                connective_used: false,
                condition: Some(map_par()),
            }];
            par
        }),
        ("new body+injection", {
            let mut par = Par::default();
            let mut injections = std::collections::BTreeMap::new();
            injections.insert("rho:injection".to_string(), map_par());
            par.news = vec![New {
                bind_count: 2,
                p: Some(map_par()),
                uri: vec!["rho:io:stdout".to_string()],
                injections,
                locally_free: vec![16],
            }];
            par
        }),
        ("match target+case+guard", {
            let mut par = Par::default();
            par.matches = vec![Match {
                target: Some(map_par()),
                cases: vec![MatchCase {
                    pattern: Some(gstring_par("p")),
                    source: Some(map_par()),
                    free_count: 0,
                    guard: Some(map_par()),
                }],
                locally_free: Vec::new(),
                connective_used: true,
            }];
            par
        }),
        ("if all three arms", {
            let mut par = Par::default();
            par.conditionals = vec![If {
                condition: Some(map_par()),
                if_true: Some(map_par()),
                if_false: Some(map_par()),
                locally_free: vec![32],
                connective_used: false,
            }];
            par
        }),
        ("bundle body", {
            let mut par = Par::default();
            par.bundles = vec![Bundle {
                body: Some(map_par()),
                write_flag: true,
                read_flag: false,
            }];
            par
        }),
        ("connective and/or/not", {
            let mut par = Par::default();
            par.connectives = vec![
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                        ps: vec![map_par()],
                    })),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(map_par())),
                },
                Connective {
                    connective_instance: Some(ConnectiveInstance::ConnBool(true)),
                },
            ];
            par.connective_used = true;
            par
        }),
    ];

    for (label, par) in shapes {
        assert_spliced_eq_direct_datum(label, &datum(vec![par.clone(), gstring_par("sibling")]));
        assert_spliced_eq_direct_pattern(
            label,
            &BindPattern {
                patterns: vec![par.clone()],
                remainder: Some(free_var),
                free_count: 1,
            },
        );
        assert_spliced_eq_direct_continuation(
            label,
            &TaggedContinuation {
                guard: Some(par.clone()),
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(par),
                    random_state: vec![7; 16],
                })),
            },
        );
    }
}

/// Unfilled cells everywhere ⇒ the emitter must take the direct path AND the
/// hand walk (forced through a nested unfilled map that CONTAINS a filled
/// one) must still agree.
#[test]
fn spliced_equals_direct_with_unfilled_cells() {
    let plain = datum(vec![epathmap_par(nested_epathmap_value())]);
    assert_spliced_eq_direct_datum("all-unfilled", &plain);
}

/// ScalaBodyRef continuations (no Par at all) plus a guarded one.
#[test]
fn spliced_equals_direct_scala_body_ref() {
    assert_spliced_eq_direct_continuation(
        "scala-body-ref",
        &TaggedContinuation {
            guard: Some(epathmap_par(interned_map())),
            tagged_cont: Some(TaggedCont::ScalaBodyRef(42)),
        },
    );
    assert_spliced_eq_direct_continuation(
        "none-cont",
        &TaggedContinuation {
            guard: None,
            tagged_cont: None,
        },
    );
}

/// The cached serde bytes are shared across the intern family: hashing two
/// clones and a rebuilt same-content map must agree with direct bytes.
#[test]
fn spliced_serde_bytes_shared_across_family() {
    let original = interned_map();
    let clone = original.clone();
    let d1 = datum(vec![epathmap_par(original)]);
    let d2 = datum(vec![epathmap_par(clone)]);
    assert_spliced_eq_direct_datum("family-original", &d1);
    assert_spliced_eq_direct_datum("family-clone", &d2);
    assert_eq!(
        event_hash_bytes_list_par_with_random(&d1),
        event_hash_bytes_list_par_with_random(&d2),
        "clone-family datums must hash to the same bytes"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Proptest over arbitrary bounded trees
// ─────────────────────────────────────────────────────────────────────────────

/// Leaf Pars: ground scalars, an EVar, and (maybe-interned) EPathMaps.
fn leaf_par_strategy() -> impl Strategy<Value = Par> {
    prop_oneof![
        any::<i64>().prop_map(|i| expr_par(ExprInstance::GInt(i))),
        "[a-z]{0,8}".prop_map(|s| expr_par(ExprInstance::GString(s))),
        any::<bool>().prop_map(|b| expr_par(ExprInstance::GBool(b))),
        proptest::collection::vec(any::<u8>(), 0..8)
            .prop_map(|b| expr_par(ExprInstance::GByteArray(b))),
        (any::<bool>(), proptest::collection::vec("[a-z]{1,4}", 0..3)).prop_map(
            |(interned, entries)| {
                let map = EPathMap::new(
                    entries.into_iter().map(|s| gstring_par(&s)).collect(),
                    Vec::new(),
                    false,
                    None,
                );
                if interned {
                    let _ = map.intern();
                }
                epathmap_par(map)
            }
        ),
    ]
}

/// Bounded recursive Par trees over the emitter's container spine.
fn par_strategy() -> impl Strategy<Value = Par> {
    leaf_par_strategy().prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            // EList with an exercised locally_free (must blank in serde).
            (proptest::collection::vec(inner.clone(), 0..4), any::<bool>()).prop_map(
                |(ps, used)| expr_par(ExprInstance::EListBody(EList {
                    ps,
                    locally_free: vec![1],
                    connective_used: used,
                    remainder: None,
                }))
            ),
            (inner.clone(), inner.clone()).prop_map(|(p1, p2)| expr_par(
                ExprInstance::EPlusBody(EPlus {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            )),
            (inner.clone(), proptest::collection::vec(inner.clone(), 0..3)).prop_map(
                |(target, arguments)| expr_par(ExprInstance::EMethodBody(EMethod {
                    method_name: "m".to_string(),
                    target: Some(target),
                    arguments,
                    locally_free: Vec::new(),
                    connective_used: false,
                }))
            ),
            (inner.clone(), proptest::collection::vec(inner.clone(), 0..3)).prop_map(
                |(chan, data)| {
                    let mut par = Par::default();
                    par.sends = vec![Send {
                        chan: Some(chan),
                        data,
                        persistent: false,
                        locally_free: vec![2],
                        connective_used: false,
                    }];
                    par
                }
            ),
            (proptest::collection::vec(inner.clone(), 1..3), any::<bool>()).prop_map(
                |(ps, interned)| {
                    let map = EPathMap::new(ps, Vec::new(), false, None);
                    if interned {
                        let _ = map.intern();
                    }
                    epathmap_par(map)
                }
            ),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// THE PM-1 gate: hand-emitted bytes == bincode::serialize bytes for
    /// arbitrary trees on all three event-hash legs.
    #[test]
    fn spliced_matches_direct_arbitrary(pars in proptest::collection::vec(par_strategy(), 0..4),
                                        random_state in proptest::collection::vec(any::<u8>(), 0..48),
                                        free_count in 0..4i32) {
        let value = ListParWithRandom { pars: pars.clone(), random_state };
        prop_assert_eq!(
            event_hash_bytes_list_par_with_random(&value),
            bincode::serialize(&value).expect("direct"),
            "datum leg diverged"
        );

        let pattern = BindPattern { patterns: pars.clone(), remainder: None, free_count };
        prop_assert_eq!(
            event_hash_bytes_bind_pattern(&pattern),
            bincode::serialize(&pattern).expect("direct"),
            "pattern leg diverged"
        );

        let continuation = TaggedContinuation {
            guard: pars.first().cloned(),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: pars.last().cloned(),
                random_state: vec![3; 8],
            })),
        };
        prop_assert_eq!(
            event_hash_bytes_tagged_continuation(&continuation),
            bincode::serialize(&continuation).expect("direct"),
            "continuation leg diverged"
        );
    }
}
