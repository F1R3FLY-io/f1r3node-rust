//! # The shared corpus for the `bincode_decoder` differential
//!
//! ## Why a CONSTRUCTED corpus and not just `generate_par`
//!
//! `models::rust::test_utils::generate_par` is the workspace's property-test
//! generator and it is used here — but on its own it would make this
//! differential **nearly vacuous for most of the schema**. It reaches four of
//! the thirty-six `ExprInstance` arms (`GBool`, `GInt`, `GString`, `ENotBody`),
//! never populates `Par::conditionals` or `Par::unforgeables`, never builds an
//! `EPathMap`, an `EZipper`, an `EMap`, a `New.injections` entry, a
//! `MatchCase.guard` or a `Receive.condition`, and never produces a
//! `TaggedContinuation` at all. A decoder arm that is never exercised is a
//! decoder arm that is not tested, and this module's job is to make "every arm
//! of the machine ran" a checkable statement rather than a hope.
//!
//! So the corpus has three layers, and the differential runs over all three:
//!
//! ```text
//!   layer            what it covers                       vacuity guard
//!   ──────────────── ──────────────────────────────────── ────────────────────
//!   EXHAUSTIVE       one representative of every          arm counts asserted
//!   (this module)    ExprInstance (36) and Connective-    against
//!                    Instance (9) arm, every Par field,   EXPR_INSTANCE_-
//!                    both EPathMap serialize arms, and    VARIANT_COUNT etc.
//!                    every root type
//!
//!   EDGE             scalar extremes: i32/i64 MIN+MAX,    each named
//!                    u32::MAX, empty and multi-byte
//!                    strings, empty collections
//!
//!   RANDOM           `generate_par(3)`, non-vacuous       the generator's own
//!                    since the `0..1` fix                 anti-vacuity guard
//! ```
//!
//! ## Depth
//!
//! [`deep_par`] builds the requested depth in one final-root pass. Callers that
//! compare with recursive serde oracles keep their inputs shallow; that is an
//! oracle constraint, not a construction or traversal limit. The independent
//! fixed-stack structural gate retains depth 20,000.

// Three test binaries share this module (`bincode_decoder_differential`,
// `bincode_decoder_malformed`, `bincode_decoder_shapes`) and each uses a different
// subset of the builders. Rust's dead-code analysis is per-binary, so without
// this every binary warns about the builders the *other* two use.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature_atom::Form;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, CostBoundSignature, CostClauseRegion,
    CostRegion, CostSignature, CostSignatureAtom, CostTokenStack, EAnd, EDiv, EEq, EGt, EGte,
    EList, ELt, ELte, EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot,
    EOr, EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational,
    GDeployId, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken, GUnforgeable, If, KeyValuePair,
    ListBindPatterns, ListParWithRandom, Match, MatchCase, New, Par, ParWithRandom, Receive,
    ReceiveBind, Send, TaggedContinuation, Var, VarRef,
};
use models::rust::rhoapi_ext::EPathMap;

// ---------------------------------------------------------------------------
// Small builders
// ---------------------------------------------------------------------------

pub fn nil() -> Par { Par::default() }

pub fn gint(n: i64) -> Par { par_of(ExprInstance::GInt(n)) }

pub fn tagged(tag: u8) -> Par {
    let mut out = Par::default();
    out.replace_locally_free(vec![tag]);
    out
}

fn par_of(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

fn some_var() -> Option<Var> {
    Some(Var {
        var_instance: Some(VarInstance::BoundVar(7)),
    })
}

/// A NON-ground `EPathMap` (`connective_used` defeats `eval_stable_epathmap`),
/// so `Serialize` emits `ps` in construction order.
pub fn nonground_pathmap() -> EPathMap {
    EPathMap::new(vec![gint(11), gint(12)], vec![0x5A, 0x5B], true, some_var())
}

/// A GROUND `EPathMap`: `Serialize` reorders `ps` into canonical trie order.
pub fn ground_pathmap() -> EPathMap {
    EPathMap::new(vec![gint(9), gint(2), gint(5)], Vec::new(), false, None)
}

// ---------------------------------------------------------------------------
// Layer 1: EXHAUSTIVE — one representative per oneof arm
// ---------------------------------------------------------------------------

/// One representative of **every** `ExprInstance` arm, each carrying a
/// non-default payload so that a decoder which produced the right *variant*
/// with the wrong *contents* still fails.
pub fn every_expr_instance() -> Vec<(&'static str, ExprInstance)> {
    let a = || Some(gint(101));
    let b = || Some(gint(102));
    vec![
        ("GBool", ExprInstance::GBool(true)),
        ("GInt", ExprInstance::GInt(-9_007_199_254_740_993)),
        ("GString", ExprInstance::GString("λ→∀ mixed 𝔘".into())),
        ("GUri", ExprInstance::GUri("rho:registry:lookup".into())),
        ("GByteArray", ExprInstance::GByteArray(vec![0, 255, 128])),
        ("ENotBody", ExprInstance::ENotBody(ENot { p: a() })),
        ("ENegBody", ExprInstance::ENegBody(ENeg { p: a() })),
        (
            "EMultBody",
            ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
        ),
        (
            "EDivBody",
            ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }),
        ),
        (
            "EPlusBody",
            ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
        ),
        (
            "EMinusBody",
            ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
        ),
        ("ELtBody", ExprInstance::ELtBody(ELt { p1: a(), p2: b() })),
        (
            "ELteBody",
            ExprInstance::ELteBody(ELte { p1: a(), p2: b() }),
        ),
        ("EGtBody", ExprInstance::EGtBody(EGt { p1: a(), p2: b() })),
        (
            "EGteBody",
            ExprInstance::EGteBody(EGte { p1: a(), p2: b() }),
        ),
        ("EEqBody", ExprInstance::EEqBody(EEq { p1: a(), p2: b() })),
        (
            "ENeqBody",
            ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }),
        ),
        (
            "EAndBody",
            ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }),
        ),
        ("EOrBody", ExprInstance::EOrBody(EOr { p1: a(), p2: b() })),
        (
            "EVarBody",
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            }),
        ),
        (
            "EListBody",
            ExprInstance::EListBody(EList {
                ps: vec![gint(1), gint(2)],
                locally_free: vec![9],
                connective_used: true,
                remainder: some_var(),
            }),
        ),
        (
            "ETupleBody",
            ExprInstance::ETupleBody(ETuple {
                ps: vec![gint(3)],
                locally_free: vec![8, 7],
                connective_used: false,
            }),
        ),
        (
            "ESetBody",
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(4), gint(5), gint(6)],
                locally_free: vec![],
                connective_used: true,
                remainder: None,
            }),
        ),
        (
            "EMapBody",
            ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: a(),
                        value: b(),
                    },
                    KeyValuePair {
                        key: None,
                        value: Some(gint(103)),
                    },
                ],
                locally_free: vec![6],
                connective_used: false,
                remainder: some_var(),
            }),
        ),
        (
            "EMethodBody",
            ExprInstance::EMethodBody(EMethod {
                method_name: "toByteArray".into(),
                target: a(),
                arguments: vec![gint(7), gint(8)],
                locally_free: vec![5],
                connective_used: true,
            }),
        ),
        (
            "EPathmapBody",
            ExprInstance::EPathmapBody(nonground_pathmap()),
        ),
        (
            "EZipperBody",
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(ground_pathmap()),
                current_path: vec![vec![3, 2], vec![], vec![255]],
                is_write_zipper: true,
                locally_free: vec![4],
                connective_used: true,
                cursor_kind: 2,
            }),
        ),
        (
            "EMatchesBody",
            ExprInstance::EMatchesBody(EMatches {
                target: a(),
                pattern: b(),
            }),
        ),
        (
            "EPercentPercentBody",
            ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
        ),
        (
            "EPlusPlusBody",
            ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
        ),
        (
            "EMinusMinusBody",
            ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
        ),
        (
            "EModBody",
            ExprInstance::EModBody(EMod { p1: a(), p2: b() }),
        ),
        (
            "GDouble",
            // A NaN with a non-canonical payload: `fixed64` carries the raw
            // bits, so a decoder that round-tripped through `f64` would lose it.
            ExprInstance::GDouble(0x7FF8_0000_DEAD_BEEF),
        ),
        ("GBigInt", ExprInstance::GBigInt(vec![0x80, 0x00, 0x01])),
        (
            "GBigRat",
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![1, 2, 3],
                denominator: vec![4],
            }),
        ),
        (
            "GFixedPoint",
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![9, 9],
                scale: u32::MAX,
            }),
        ),
    ]
}

/// One representative of **every** `ConnectiveInstance` arm.
pub fn every_connective_instance() -> Vec<(&'static str, ConnectiveInstance)> {
    vec![
        (
            "ConnAndBody",
            ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![gint(21), gint(22)],
            }),
        ),
        (
            "ConnOrBody",
            ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: vec![] }),
        ),
        (
            "ConnNotBody",
            // A bare `Par`, NOT an `Option<Par>` — no tag byte on the wire.
            ConnectiveInstance::ConnNotBody(gint(23)),
        ),
        (
            "VarRefBody",
            ConnectiveInstance::VarRefBody(VarRef {
                index: i32::MIN,
                depth: i32::MAX,
            }),
        ),
        ("ConnBool", ConnectiveInstance::ConnBool(true)),
        ("ConnInt", ConnectiveInstance::ConnInt(false)),
        ("ConnString", ConnectiveInstance::ConnString(true)),
        ("ConnUri", ConnectiveInstance::ConnUri(false)),
        ("ConnByteArray", ConnectiveInstance::ConnByteArray(true)),
    ]
}

/// Every `UnfInstance` arm plus the absent oneof.
pub fn every_unforgeable() -> Vec<GUnforgeable> {
    vec![
        GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![1, 2, 3] })),
        },
        GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![] })),
        },
        GUnforgeable {
            unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                public_key: vec![255],
            })),
        },
        GUnforgeable {
            unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
        },
        GUnforgeable { unf_instance: None },
    ]
}

/// Every `VarInstance` arm plus the absent oneof and the absent `Option<Var>`.
pub fn every_opt_var() -> Vec<Option<Var>> {
    vec![
        None,
        Some(Var { var_instance: None }),
        Some(Var {
            var_instance: Some(VarInstance::BoundVar(i32::MIN)),
        }),
        Some(Var {
            var_instance: Some(VarInstance::FreeVar(i32::MAX)),
        }),
        Some(Var {
            var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
        }),
    ]
}

// ---------------------------------------------------------------------------
// The `Par` corpus
// ---------------------------------------------------------------------------

/// A `Par` whose every field is populated, including the two that
/// `generate_par` never reaches (`conditionals`, `unforgeables`).
pub fn all_par_fields() -> Par {
    let mut par = Par::default();
    par.replace_sends(vec![
        Send {
            chan: Some(gint(1)),
            data: vec![gint(2), gint(3)],
            persistent: true,
            locally_free: vec![0x11, 0x12],
            connective_used: true,
            cost_region: None,
        },
        Send {
            chan: None,
            data: vec![],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
            cost_region: None,
        },
    ]);
    par.replace_receives(vec![Receive {
        binds: vec![
            ReceiveBind {
                patterns: vec![gint(4), gint(5)],
                source: Some(gint(6)),
                remainder: some_var(),
                free_count: -7,
                cost_region: None,
            },
            ReceiveBind {
                patterns: vec![],
                source: None,
                remainder: None,
                free_count: 0,
                cost_region: None,
            },
        ],
        body: Some(gint(7)),
        persistent: true,
        peek: true,
        bind_count: i32::MIN,
        locally_free: vec![0x21],
        connective_used: true,
        // `Receive.condition` — a slot `generate_par` never fills.
        condition: Some(gint(8)),
        cost_region: None,
    }]);
    par.replace_news(vec![
        New {
            bind_count: 3,
            p: Some(gint(9)),
            uri: vec!["rho:io:stdout".into(), String::new()],
            injections: {
                let mut m = BTreeMap::new();
                m.insert("alpha".to_string(), gint(10));
                m.insert("beta".to_string(), gint(11));
                m.insert(String::new(), gint(12));
                m
            },
            locally_free: vec![0x31],
        },
        New {
            bind_count: 0,
            p: None,
            uri: vec![],
            injections: BTreeMap::new(),
            locally_free: vec![],
        },
    ]);
    par.replace_exprs({
        let mut exprs: Vec<Expr> = every_expr_instance()
            .into_iter()
            .map(|(_, instance)| Expr {
                expr_instance: Some(instance),
            })
            .collect();
        exprs.push(Expr {
            expr_instance: None,
        });
        exprs
    });
    par.replace_matches(vec![Match {
        target: Some(gint(13)),
        cases: vec![
            MatchCase {
                pattern: Some(gint(14)),
                source: Some(gint(15)),
                free_count: 2,
                // `MatchCase.guard` — another slot `generate_par` never fills.
                guard: Some(gint(16)),
            },
            MatchCase {
                pattern: None,
                source: None,
                free_count: -1,
                guard: None,
            },
        ],
        locally_free: vec![0xA1],
        connective_used: true,
    }]);
    par.replace_unforgeables(every_unforgeable());
    par.replace_bundles(vec![
        Bundle {
            body: Some(gint(17)),
            write_flag: true,
            read_flag: false,
        },
        Bundle {
            body: None,
            write_flag: false,
            read_flag: true,
        },
    ]);
    par.replace_connectives({
        let mut cs: Vec<Connective> = every_connective_instance()
            .into_iter()
            .map(|(_, instance)| Connective {
                connective_instance: Some(instance),
            })
            .collect();
        cs.push(Connective {
            connective_instance: None,
        });
        cs
    });
    par.replace_conditionals(vec![If {
        condition: Some(gint(18)),
        if_true: Some(gint(19)),
        if_false: None,
        locally_free: vec![0xD1],
        connective_used: true,
    }]);
    par.replace_locally_free(vec![0xE1, 0xE2, 0xE3]);
    par.replace_connective_used(true);
    par
}

/// A left-spine `Par` of the requested nesting depth, built through `EList`.
///
/// Depth 48 is large enough to exercise every containment shape repeatedly
/// while remaining suitable for the retained serde differential in debug mode.
pub fn deep_par(depth: usize) -> Par {
    Par::wrap_unary_elist(depth, gint(0)).expect("requested unary fixture depth is representable")
}

/// A `Par` nested through a *different* slot at each level, so a decoder that
/// only kept the `EList` path aligned still fails. Cycles through eight
/// containment shapes.
pub fn deep_mixed_par(depth: usize) -> Par {
    let mut p = gint(0);
    for level in 0..depth {
        p = match level % 8 {
            0 => {
                let mut out = Par::default();
                out.replace_sends(vec![Send {
                    chan: Some(p),
                    data: vec![],
                    persistent: false,
                    locally_free: vec![level as u8],
                    connective_used: false,
                    cost_region: None,
                }]);
                out
            }
            1 => {
                let mut out = Par::default();
                out.replace_receives(vec![Receive {
                    binds: vec![ReceiveBind {
                        patterns: vec![p],
                        source: None,
                        remainder: None,
                        free_count: 1,
                        cost_region: None,
                    }],
                    body: None,
                    persistent: false,
                    peek: false,
                    bind_count: 0,
                    locally_free: vec![],
                    connective_used: false,
                    condition: None,
                    cost_region: None,
                }]);
                out
            }
            2 => {
                let mut out = Par::default();
                out.replace_news(vec![New {
                    bind_count: 1,
                    p: None,
                    uri: vec![],
                    injections: {
                        let mut m = BTreeMap::new();
                        m.insert("k".to_string(), p);
                        m
                    },
                    locally_free: vec![],
                }]);
                out
            }
            3 => {
                let mut out = Par::default();
                out.replace_matches(vec![Match {
                    target: None,
                    cases: vec![MatchCase {
                        pattern: None,
                        source: None,
                        free_count: 0,
                        guard: Some(p),
                    }],
                    locally_free: vec![],
                    connective_used: false,
                }]);
                out
            }
            4 => {
                let mut out = Par::default();
                out.replace_conditionals(vec![If {
                    condition: None,
                    if_true: None,
                    if_false: Some(p),
                    locally_free: vec![],
                    connective_used: false,
                }]);
                out
            }
            5 => {
                let mut out = Par::default();
                out.replace_connectives(vec![Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(p)),
                }]);
                out
            }
            6 => par_of(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: None,
                    value: Some(p),
                }],
                ..Default::default()
            })),
            _ => {
                let map = EPathMap::new(vec![p], vec![1], true, None);
                par_of(ExprInstance::EPathmapBody(map))
            }
        };
    }
    p
}

/// Construct the full `Par` corpus: exhaustive, edge, and structural-depth
/// cases. Callers use [`par_corpus`], which memoizes this expensive canonical
/// PathMap construction once per test binary and returns cheap CoW clones.
fn build_par_corpus() -> Vec<(String, Par)> {
    let mut out: Vec<(String, Par)> = Vec::new();

    out.push(("nil".to_string(), nil()));
    out.push(("all_par_fields".to_string(), all_par_fields()));

    // One `Par` per `ExprInstance` arm, so a failure names the arm.
    for (name, instance) in every_expr_instance() {
        out.push((format!("expr::{name}"), par_of(instance)));
    }
    // One `Par` per `ConnectiveInstance` arm.
    for (name, instance) in every_connective_instance() {
        let mut par = Par::default();
        par.replace_connectives(vec![Connective {
            connective_instance: Some(instance),
        }]);
        out.push((format!("connective::{name}"), par));
    }
    // Every `Option<Var>` shape, through `EList.remainder`.
    for (i, v) in every_opt_var().into_iter().enumerate() {
        out.push((
            format!("opt_var::{i}"),
            par_of(ExprInstance::EListBody(EList {
                ps: vec![],
                locally_free: vec![],
                connective_used: false,
                remainder: v,
            })),
        ));
    }
    // Every `GUnforgeable` shape.
    for (i, u) in every_unforgeable().into_iter().enumerate() {
        let mut par = Par::default();
        par.replace_unforgeables(vec![u]);
        out.push((format!("unforgeable::{i}"), par));
    }

    for depth in [1usize, 2, 7, 48] {
        out.push((format!("deep_list::{depth}"), deep_par(depth)));
        out.push((format!("deep_mixed::{depth}"), deep_mixed_par(depth)));
    }

    // Wide, to exercise the counted repeat past its first iteration.
    let mut wide = Par::default();
    wide.replace_sends(
        (0..64)
            .map(|i| Send {
                chan: Some(gint(i)),
                data: (0..3).map(gint).collect(),
                persistent: i % 2 == 0,
                locally_free: vec![i as u8],
                connective_used: i % 3 == 0,
                cost_region: None,
            })
            .collect(),
    );
    out.push(("wide_sends".to_string(), wide));

    out
}

/// The full `Par` corpus, materialized once per test binary. Multiple tests in
/// the same binary intentionally exercise different codec properties over the
/// identical population; rebuilding every nested canonical PathMap for each
/// property adds quadratic fixture work without increasing coverage.
pub fn par_corpus() -> Vec<(String, Par)> {
    static CORPUS: OnceLock<Vec<(String, Par)>> = OnceLock::new();
    CORPUS.get_or_init(build_par_corpus).clone()
}

// ---------------------------------------------------------------------------
// The non-`Par` root corpora
// ---------------------------------------------------------------------------

pub fn list_par_with_random_corpus() -> Vec<(String, ListParWithRandom)> {
    vec![
        ("empty".to_string(), ListParWithRandom {
            pars: vec![],
            random_state: vec![],
            cost_region: None,
        }),
        ("loaded".to_string(), ListParWithRandom {
            pars: vec![all_par_fields(), deep_mixed_par(16), nil()],
            random_state: vec![0xF1, 0xF2, 0xF3],
            cost_region: None,
        }),
    ]
}

pub fn bind_pattern_corpus() -> Vec<(String, BindPattern)> {
    let mut out = vec![("empty".to_string(), BindPattern {
        patterns: vec![],
        remainder: None,
        free_count: 0,
    })];
    for (i, v) in every_opt_var().into_iter().enumerate() {
        out.push((format!("remainder::{i}"), BindPattern {
            patterns: vec![all_par_fields(), gint(1)],
            remainder: v,
            free_count: i32::MIN,
        }));
    }
    out
}

pub fn tagged_continuation_corpus() -> Vec<(String, TaggedContinuation)> {
    vec![
        ("absent_cont".to_string(), TaggedContinuation {
            guard: None,
            tagged_cont: None,
            cost_region: None,
            clause_regions: Vec::new(),
        }),
        ("par_body".to_string(), TaggedContinuation {
            guard: Some(all_par_fields()),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(deep_mixed_par(12)),
                random_state: vec![1, 2, 3],
            })),
            cost_region: None,
            clause_regions: Vec::new(),
        }),
        ("par_body_absent_inner".to_string(), TaggedContinuation {
            guard: None,
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: None,
                random_state: vec![],
            })),
            cost_region: None,
            clause_regions: Vec::new(),
        }),
        ("scala_ref".to_string(), TaggedContinuation {
            guard: Some(gint(1)),
            tagged_cont: Some(TaggedCont::ScalaBodyRef(i64::MIN)),
            cost_region: None,
            clause_regions: Vec::new(),
        }),
    ]
}

pub fn par_with_random_corpus() -> Vec<(String, ParWithRandom)> {
    vec![
        ("empty".to_string(), ParWithRandom {
            body: None,
            random_state: vec![],
        }),
        ("loaded".to_string(), ParWithRandom {
            body: Some(all_par_fields()),
            random_state: vec![7; 32],
        }),
    ]
}

pub fn list_bind_patterns_corpus() -> Vec<(String, ListBindPatterns)> {
    vec![
        ("empty".to_string(), ListBindPatterns { patterns: vec![] }),
        ("loaded".to_string(), ListBindPatterns {
            patterns: bind_pattern_corpus()
                .into_iter()
                .map(|(_, bp)| bp)
                .collect(),
        }),
    ]
}

// ---------------------------------------------------------------------------
// Cost-accounting metadata
// ---------------------------------------------------------------------------
//
// The corpora above leave every cost-accounting slot absent, so values that
// existing byte goldens pin are unchanged. These builders populate those
// slots separately, for coverage tests that must reach every generated
// message and oneof arm.

/// Every `cost_signature_atom::Form` arm.
pub fn every_cost_signature_atom() -> Vec<(&'static str, CostSignatureAtom)> {
    vec![
        ("Ground", CostSignatureAtom {
            form: Some(Form::Ground(vec![0xC1])),
        }),
        ("Quote", CostSignatureAtom {
            form: Some(Form::Quote(vec![0xC2, 0xC3])),
        }),
        ("Bound", CostSignatureAtom {
            form: Some(Form::Bound(CostBoundSignature { index: i32::MIN })),
        }),
        ("RuntimeName", CostSignatureAtom {
            form: Some(Form::RuntimeName(vec![])),
        }),
    ]
}

/// A signature carrying every `Form` arm.
pub fn every_form_cost_signature() -> CostSignature {
    CostSignature {
        atoms: every_cost_signature_atom()
            .into_iter()
            .map(|(_, atom)| atom)
            .collect(),
    }
}

/// A region whose every field is populated.
pub fn cost_region() -> CostRegion {
    CostRegion {
        version: 1,
        start_line: 2,
        start_column: 3,
        end_line: u64::MAX,
        end_column: 5,
        signer: Some(every_form_cost_signature()),
        ordinal: u32::MAX,
    }
}

/// A `Par` whose `Send`, `Receive` and `ReceiveBind` regions and
/// `cost_token_stacks` are populated.
pub fn cost_annotated_par() -> Par {
    let mut par = Par::default();
    par.replace_sends(vec![Send {
        chan: Some(gint(1)),
        data: vec![gint(2)],
        cost_region: Some(cost_region()),
        ..Send::default()
    }]);
    par.replace_receives(vec![Receive {
        binds: vec![ReceiveBind {
            patterns: vec![gint(3)],
            source: Some(gint(4)),
            cost_region: Some(cost_region()),
            ..ReceiveBind::default()
        }],
        body: Some(gint(5)),
        cost_region: Some(cost_region()),
        ..Receive::default()
    }]);
    par.replace_cost_token_stacks(vec![CostTokenStack {
        layers: vec![every_form_cost_signature(), CostSignature::default()],
    }]);
    par
}

/// A `ListParWithRandom` whose region is populated.
pub fn cost_annotated_list_par_with_random() -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![gint(6)],
        random_state: vec![0xC4],
        cost_region: Some(cost_region()),
    }
}

/// A `TaggedContinuation` whose region and per-clause regions are populated.
pub fn cost_annotated_tagged_continuation() -> TaggedContinuation {
    TaggedContinuation {
        guard: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(gint(7)),
            random_state: vec![0xC5],
        })),
        cost_region: Some(cost_region()),
        clause_regions: vec![CostClauseRegion {
            bind_index: u32::MAX,
            region: Some(cost_region()),
        }],
    }
}
