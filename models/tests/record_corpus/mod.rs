//! # The shared corpus of the `record_decoder` tests (DR-131 P2b)
//!
//! Ported from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/tests/par_corpus/mod.rs`) to this branch's schema:
//!
//! - `ExprInstance` has 42 arms here, with the five numeric arms `GUint64`,
//!   `GInt32`, `GUint32`, `GUint16` and `GUint8`.
//! - `EPathMap` is a plain generated message (`ps`, `locally_free`,
//!   `connective_used`, `remainder`) with derived serde. `EZipper` has no
//!   `cursor_kind`.
//! - The cost family (`CostSignature`, `CostSignedTerm`, `CostStack`,
//!   `CostRegion`, `CostAuthority`) is part of `Par`, `ReceiveBind`,
//!   `ListParWithRandom` and `TaggedContinuation`, so the corpus fills it.
//! - [`random_par`] replaces the workspace generator `generate_par`. On this
//!   branch and on dev, every collection range of that generator is `0..1`,
//!   and proptest reads `0..1` as exactly zero elements. So `generate_par`
//!   gives only the empty `Par`, with either value of `connective_used`.
//!   `feature/mettail` fixed those ranges
//!   (`5108d8543:models/src/rust/test_utils/test_utils.rs`). At the merge of
//!   that branch, [`random_par`] can give way to the fixed generator.
//!
//! At the merge of `feature/mettail`, its own decoder tests
//! (`models/tests/bincode_decoder_*.rs`, `models/tests/par_corpus/`) arrive
//! beside these files. The reconciliation of the two decoders (E0119 on the
//! `ColdStoreDecode` implementations) also reconciles the two test sets.
//!
//! ## The three layers
//!
//! A random generator reaches few arms of the schema. A decoder arm that no
//! test reaches is an arm that no test checks, so the corpus has three layers:
//!
//! ```text
//!   layer        what it covers                            check against vacuity
//!   ──────────── ───────────────────────────────────────── ─────────────────────────
//!   EXHAUSTIVE   one value of every ExprInstance (42),      the reached arms are
//!                ConnectiveInstance (9) and cost signature  compared with the full
//!                (6) arm, every Par field, every root type  alphabets
//!
//!   EDGE         scalar extremes: i32 and i64 MIN and MAX,  each case is named
//!                u32::MAX, empty and multi-byte strings,
//!                empty collections
//!
//!   RANDOM       random_par(3): combinations of four        the generator test of
//!                expression arms and seven Par fields        record_decoder_differential
//! ```
//!
//! ## Depth
//!
//! [`deep_par`] and [`deep_mixed_par`] stay at 48 levels in the differential.
//! The oracle is the derived decoder, which recurses once for each level, so a
//! differential cannot go much deeper. The machine alone is tested deeper:
//! `cold_store_decode_deep.rs` (100,000 levels), and the deep tests of
//! `record_decoder_shapes.rs` and `record_decoder_differential.rs`.

// Four test binaries share this module, and each uses a different subset of
// the builders. Dead-code analysis is per binary.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value as CostValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::{VarInstance, WildcardMsg};
// Changed by G3 (DR-135): the corpus also builds the caller of a region.
// use models::rhoapi::{
//     BindPattern, Bundle, Connective, ConnectiveBody, CostAuthority, CostRegion, CostSignature,
use models::rhoapi::{
    BindPattern, Bundle, Connective, ConnectiveBody, CostAuthority, CostCaller, CostRegion,
    CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, EAnd, EDiv, EEq, EGt, EGte,
    EList, ELt, ELte, EMap, EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot,
    EOr, EPathMap, EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr,
    GAuthorityId, GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrincipalId, GPrivate,
    GSysAuthToken, GUnforgeable, If, KeyValuePair, ListBindPatterns, ListParWithRandom, Match,
    MatchCase, New, Par, ParWithRandom, Receive, ReceiveBind, Send, TaggedContinuation, Var,
    VarRef,
};
use models::rust::rholang::record_decoder::{
    connective_instance_variant_index, cost_value_variant_index, expr_instance_variant_index,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Small builders
// ---------------------------------------------------------------------------

pub fn nil() -> Par { Par::default() }

pub fn gint(n: i64) -> Par { par_of(ExprInstance::GInt(n)) }

pub fn par_of(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn some_var() -> Option<Var> {
    Some(Var {
        var_instance: Some(VarInstance::BoundVar(7)),
    })
}

/// An `EPathMap` with every field set.
pub fn pathmap() -> EPathMap {
    EPathMap {
        ps: vec![gint(11), gint(12)],
        locally_free: vec![0x5A, 0x5B],
        connective_used: true,
        remainder: some_var(),
    }
}

/// An `EPathMap` with three elements and no remainder.
pub fn plain_pathmap() -> EPathMap {
    EPathMap {
        ps: vec![gint(9), gint(2), gint(5)],
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }
}

pub fn unit_signature() -> CostSignature {
    CostSignature {
        value: Some(CostValue::Unit(true)),
    }
}

/// A compound signature with one element of each scalar arm and one absent
/// value.
pub fn compound_signature() -> CostSignature {
    CostSignature {
        value: Some(CostValue::Compound(CostSignatureCompound {
            elements: vec![
                CostSignature {
                    value: Some(CostValue::Ground(vec![0, 255])),
                },
                CostSignature {
                    value: Some(CostValue::BoundLevel(-7)),
                },
                unit_signature(),
                CostSignature { value: None },
            ],
        })),
    }
}

pub fn cost_authority() -> CostAuthority {
    CostAuthority {
        regions: vec![
            CostRegion {
                instance_id: vec![1, 2, 3],
                signature: Some(compound_signature()),
                // Added by G3 (DR-135): an unpriced region without a caller.
                price: 0,
                caller: None,
            },
            CostRegion {
                instance_id: Vec::new(),
                signature: None,
                // Added by G3 (DR-135): the widest price, and a caller with no
                // signature.
                price: u64::MAX,
                caller: Some(CostCaller {
                    signature: None,
                    price: 0,
                }),
            },
            // Added by G3 (DR-135): a priced region whose caller quotes a
            // `Par`, so the caller's signature is a machine value (SA3).
            CostRegion {
                instance_id: vec![9; 32],
                signature: Some(unit_signature()),
                price: 3,
                caller: Some(quoting_caller()),
            },
        ],
    }
}

/// Added by G3 (DR-135): a caller whose signature quotes a `Par` with every
/// field, at a price whose eight bytes all differ.
pub fn quoting_caller() -> CostCaller {
    CostCaller {
        signature: Some(CostSignature {
            value: Some(CostValue::Quote(all_par_fields())),
        }),
        price: 0x0102_0304_0506_0708,
    }
}

pub fn cost_stack() -> CostStack {
    CostStack {
        cells: vec![
            CostSignature { value: None },
            unit_signature(),
            compound_signature(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Layer 1: EXHAUSTIVE, one value for each oneof arm
// ---------------------------------------------------------------------------

/// One value of every `ExprInstance` arm. Each holds a payload that is not the
/// default, so a decoder that produced the right arm with the wrong contents
/// still fails.
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
        ("EPathmapBody", ExprInstance::EPathmapBody(pathmap())),
        (
            "EZipperBody",
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(plain_pathmap()),
                current_path: vec![vec![3, 2], vec![], vec![255]],
                is_write_zipper: true,
                locally_free: vec![4],
                connective_used: true,
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
            // A NaN with a payload that is not canonical. `fixed64` holds the
            // raw bits, so a decoder that went through `f64` would lose it.
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
        ("GUint64", ExprInstance::GUint64(u64::MAX - 1)),
        ("GFloat32", ExprInstance::GFloat32(0x7FC0_DEAD)),
        ("GInt32", ExprInstance::GInt32(i32::MIN)),
        ("GUint32", ExprInstance::GUint32(u32::MAX)),
        ("GUint16", ExprInstance::GUint16(0xBEEF)),
        ("GUint8", ExprInstance::GUint8(0xA5)),
    ]
}

/// One value of every `ConnectiveInstance` arm.
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
            // A bare `Par`, not an `Option<Par>`: no tag byte on the wire.
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

/// One value of every `CostSignature.Value` arm, and the absent value.
pub fn every_cost_signature() -> Vec<(&'static str, CostSignature)> {
    vec![
        ("Ground", CostSignature {
            value: Some(CostValue::Ground(vec![0x5A, 0x01])),
        }),
        ("BoundLevel", CostSignature {
            value: Some(CostValue::BoundLevel(i32::MIN)),
        }),
        ("Quote", CostSignature {
            value: Some(CostValue::Quote(gint(31))),
        }),
        ("Compound", compound_signature()),
        ("Name", CostSignature {
            value: Some(CostValue::Name(gint(32))),
        }),
        ("Unit", unit_signature()),
        ("absent", CostSignature { value: None }),
    ]
}

/// Every `UnfInstance` arm and the absent oneof.
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
        GUnforgeable {
            unf_instance: Some(UnfInstance::GAuthorityIdBody(GAuthorityId { id: vec![4] })),
        },
        GUnforgeable {
            unf_instance: Some(UnfInstance::GPrincipalIdBody(GPrincipalId {
                key_family: 2,
                public_key: vec![5],
            })),
        },
        GUnforgeable { unf_instance: None },
    ]
}

/// Every `VarInstance` arm, the absent oneof and the absent `Option<Var>`.
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

/// A `Par` with every field populated, including the fields that a random
/// generator does not reach: `conditionals`, `unforgeables` and the cost
/// fields.
pub fn all_par_fields() -> Par {
    Par {
        sends: vec![
            Send {
                chan: Some(gint(1)),
                data: vec![gint(2), gint(3)],
                persistent: true,
                locally_free: vec![0x11, 0x12],
                connective_used: true,
            },
            Send {
                chan: None,
                data: vec![],
                persistent: false,
                locally_free: vec![],
                connective_used: false,
            },
        ],
        receives: vec![Receive {
            binds: vec![
                ReceiveBind {
                    patterns: vec![gint(4), gint(5)],
                    source: Some(gint(6)),
                    remainder: some_var(),
                    free_count: -7,
                    cost_signature: Some(compound_signature()),
                },
                ReceiveBind {
                    patterns: vec![],
                    source: None,
                    remainder: None,
                    free_count: 0,
                    cost_signature: None,
                },
            ],
            body: Some(gint(7)),
            persistent: true,
            peek: true,
            bind_count: i32::MIN,
            locally_free: vec![0x21],
            connective_used: true,
            condition: Some(gint(8)),
        }],
        news: vec![
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
        ],
        exprs: {
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
        },
        matches: vec![Match {
            target: Some(gint(13)),
            cases: vec![
                MatchCase {
                    pattern: Some(gint(14)),
                    source: Some(gint(15)),
                    free_count: 2,
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
        }],
        unforgeables: every_unforgeable(),
        bundles: vec![
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
        ],
        connectives: {
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
        },
        conditionals: vec![If {
            condition: Some(gint(18)),
            if_true: Some(gint(19)),
            if_false: None,
            locally_free: vec![0xD1],
            connective_used: true,
        }],
        locally_free: vec![0xE1, 0xE2, 0xE3],
        connective_used: true,
        cost_signed_terms: vec![
            CostSignedTerm {
                body: Some(gint(24)),
                signature: Some(unit_signature()),
                // Added by G3 (DR-135, S3b): a held term, so the corpus holds
                // a held price.
                held_price: Some(25),
            },
            CostSignedTerm {
                body: None,
                signature: None,
                // Added by G3 (DR-135, S3b): no held price.
                held_price: None,
            },
        ],
        cost_stacks: vec![
            CostStack {
                cells: every_cost_signature()
                    .into_iter()
                    .map(|(_, signature)| signature)
                    .collect(),
            },
            CostStack { cells: vec![] },
        ],
    }
}

/// A chain of `EList`s of the requested nesting depth.
///
/// See the module header for why the differential stays at 48 levels: the
/// oracle recurses once for each level, the machine does not.
pub fn deep_par(depth: usize) -> Par {
    let mut p = gint(0);
    for _ in 0..depth {
        p = par_of(ExprInstance::EListBody(EList {
            ps: vec![p],
            ..Default::default()
        }));
    }
    p
}

/// The number of containment shapes that [`deep_mixed_par`] cycles through.
pub const MIXED_SHAPES: usize = 10;

/// A `Par` nested through a different position at each level, so that a
/// decoder that keeps only the `EList` path aligned still fails. It cycles
/// through ten containment shapes, two of them through the cost family.
pub fn deep_mixed_par(depth: usize) -> Par {
    let mut p = gint(0);
    for level in 0..depth {
        p = match level % MIXED_SHAPES {
            0 => Par {
                sends: vec![Send {
                    chan: Some(p),
                    data: vec![],
                    persistent: false,
                    locally_free: vec![level as u8],
                    connective_used: false,
                }],
                ..Default::default()
            },
            1 => Par {
                receives: vec![Receive {
                    binds: vec![ReceiveBind {
                        patterns: vec![p],
                        source: None,
                        remainder: None,
                        free_count: 1,
                        cost_signature: None,
                    }],
                    body: None,
                    persistent: false,
                    peek: false,
                    bind_count: 0,
                    locally_free: vec![],
                    connective_used: false,
                    condition: None,
                }],
                ..Default::default()
            },
            2 => Par {
                news: vec![New {
                    bind_count: 1,
                    p: None,
                    uri: vec![],
                    injections: {
                        let mut m = BTreeMap::new();
                        m.insert("k".to_string(), p);
                        m
                    },
                    locally_free: vec![],
                }],
                ..Default::default()
            },
            3 => Par {
                matches: vec![Match {
                    target: None,
                    cases: vec![MatchCase {
                        pattern: None,
                        source: None,
                        free_count: 0,
                        guard: Some(p),
                    }],
                    locally_free: vec![],
                    connective_used: false,
                }],
                ..Default::default()
            },
            4 => Par {
                conditionals: vec![If {
                    condition: None,
                    if_true: None,
                    if_false: Some(p),
                    locally_free: vec![],
                    connective_used: false,
                }],
                ..Default::default()
            },
            5 => Par {
                connectives: vec![Connective {
                    connective_instance: Some(ConnectiveInstance::ConnNotBody(p)),
                }],
                ..Default::default()
            },
            6 => par_of(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: None,
                    value: Some(p),
                }],
                ..Default::default()
            })),
            7 => par_of(ExprInstance::EPathmapBody(EPathMap {
                ps: vec![p],
                locally_free: vec![1],
                connective_used: true,
                remainder: None,
            })),
            8 => Par {
                cost_signed_terms: vec![CostSignedTerm {
                    body: Some(p),
                    signature: None,
                    // Added by G3 (DR-135, S3b): the held price follows the
                    // body in the stream, so a deep chain interleaves both.
                    held_price: Some(level as u64),
                }],
                ..Default::default()
            },
            _ => Par {
                cost_stacks: vec![CostStack {
                    cells: vec![CostSignature {
                        value: Some(CostValue::Quote(p)),
                    }],
                }],
                ..Default::default()
            },
        };
    }
    p
}

/// The full `Par` corpus: exhaustive, edge and depth cases.
pub fn par_corpus() -> Vec<(String, Par)> {
    let mut out: Vec<(String, Par)> = Vec::with_capacity(96);

    out.push(("nil".to_string(), nil()));
    out.push(("all_par_fields".to_string(), all_par_fields()));

    // One `Par` for each `ExprInstance` arm, so a failure names the arm.
    for (name, instance) in every_expr_instance() {
        out.push((format!("expr::{name}"), par_of(instance)));
    }
    // One `Par` for each `ConnectiveInstance` arm.
    for (name, instance) in every_connective_instance() {
        out.push((format!("connective::{name}"), Par {
            connectives: vec![Connective {
                connective_instance: Some(instance),
            }],
            ..Default::default()
        }));
    }
    // One `Par` for each cost-signature arm, in a cost stack.
    for (name, signature) in every_cost_signature() {
        out.push((format!("cost_signature::{name}"), Par {
            cost_stacks: vec![CostStack {
                cells: vec![signature],
            }],
            ..Default::default()
        }));
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
        out.push((format!("unforgeable::{i}"), Par {
            unforgeables: vec![u],
            ..Default::default()
        }));
    }

    for depth in [1usize, 2, 7, 48] {
        out.push((format!("deep_list::{depth}"), deep_par(depth)));
        out.push((format!("deep_mixed::{depth}"), deep_mixed_par(depth)));
    }

    // Wide, to run the counted repeat past its first iteration.
    out.push(("wide_sends".to_string(), Par {
        sends: (0..64)
            .map(|i| Send {
                chan: Some(gint(i)),
                data: (0..3).map(gint).collect(),
                persistent: i % 2 == 0,
                locally_free: vec![i as u8],
                connective_used: i % 3 == 0,
            })
            .collect(),
        ..Default::default()
    }));

    out
}

// ---------------------------------------------------------------------------
// The corpora of the other roots
// ---------------------------------------------------------------------------

pub fn list_par_with_random_corpus() -> Vec<(String, ListParWithRandom)> {
    vec![
        ("empty".to_string(), ListParWithRandom {
            pars: vec![],
            random_state: vec![],
            cost_authority: None,
            cost_stack: None,
        }),
        ("loaded".to_string(), ListParWithRandom {
            pars: vec![all_par_fields(), deep_mixed_par(16), nil()],
            random_state: vec![0xF1, 0xF2, 0xF3],
            cost_authority: None,
            cost_stack: None,
        }),
        ("costed".to_string(), ListParWithRandom {
            pars: vec![gint(1)],
            random_state: vec![],
            cost_authority: Some(cost_authority()),
            cost_stack: Some(cost_stack()),
        }),
        ("empty_cost".to_string(), ListParWithRandom {
            pars: vec![],
            random_state: vec![7],
            cost_authority: Some(CostAuthority::default()),
            cost_stack: Some(CostStack::default()),
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
            cost_authority: None,
            tagged_cont: None,
        }),
        ("par_body".to_string(), TaggedContinuation {
            guard: Some(all_par_fields()),
            cost_authority: None,
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(deep_mixed_par(12)),
                random_state: vec![1, 2, 3],
            })),
        }),
        ("par_body_absent_inner".to_string(), TaggedContinuation {
            guard: None,
            cost_authority: None,
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: None,
                random_state: vec![],
            })),
        }),
        ("scala_ref".to_string(), TaggedContinuation {
            guard: Some(gint(1)),
            cost_authority: None,
            tagged_cont: Some(TaggedCont::ScalaBodyRef(i64::MIN)),
        }),
        ("authority".to_string(), TaggedContinuation {
            guard: None,
            cost_authority: Some(cost_authority()),
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(gint(2)),
                random_state: vec![4],
            })),
        }),
        ("authority_absent_cont".to_string(), TaggedContinuation {
            guard: Some(gint(3)),
            cost_authority: Some(CostAuthority::default()),
            tagged_cont: None,
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
// The arms that a corpus reaches
// ---------------------------------------------------------------------------

/// The serde variant indices of the arms that a set of roots reaches,
/// anywhere in their trees.
#[derive(Default)]
pub struct ReachedArms {
    pub exprs: BTreeSet<u32>,
    pub connectives: BTreeSet<u32>,
    pub cost_values: BTreeSet<u32>,
}

/// Walks every `Par` and cost signature that the roots hold, with a heap work
/// list, and records the arm of every expression, connective and cost
/// signature. The indices come from the decoder's own exhaustive index
/// functions, so this measurement uses the numbering that the decoder uses.
pub fn reached_arms<'a>(roots: impl IntoIterator<Item = &'a Par>) -> ReachedArms {
    let mut reached = ReachedArms::default();
    let mut pars: Vec<&Par> = roots.into_iter().collect();
    let mut signatures: Vec<&CostSignature> = Vec::new();
    loop {
        if let Some(par) = pars.pop() {
            for send in &par.sends {
                pars.extend(send.chan.iter());
                pars.extend(send.data.iter());
            }
            for receive in &par.receives {
                for bind in &receive.binds {
                    pars.extend(bind.patterns.iter());
                    pars.extend(bind.source.iter());
                    signatures.extend(bind.cost_signature.iter());
                }
                pars.extend(receive.body.iter());
                pars.extend(receive.condition.iter());
            }
            for new in &par.news {
                pars.extend(new.p.iter());
                pars.extend(new.injections.values());
            }
            for expr in &par.exprs {
                if let Some(instance) = &expr.expr_instance {
                    reached.exprs.insert(expr_instance_variant_index(instance));
                    push_expr_operands(instance, &mut pars);
                }
            }
            for matched in &par.matches {
                pars.extend(matched.target.iter());
                for case in &matched.cases {
                    pars.extend(case.pattern.iter());
                    pars.extend(case.source.iter());
                    pars.extend(case.guard.iter());
                }
            }
            for bundle in &par.bundles {
                pars.extend(bundle.body.iter());
            }
            for connective in &par.connectives {
                if let Some(instance) = &connective.connective_instance {
                    reached
                        .connectives
                        .insert(connective_instance_variant_index(instance));
                    match instance {
                        ConnectiveInstance::ConnAndBody(body)
                        | ConnectiveInstance::ConnOrBody(body) => pars.extend(body.ps.iter()),
                        ConnectiveInstance::ConnNotBody(body) => pars.push(body),
                        ConnectiveInstance::VarRefBody(_)
                        | ConnectiveInstance::ConnBool(_)
                        | ConnectiveInstance::ConnInt(_)
                        | ConnectiveInstance::ConnString(_)
                        | ConnectiveInstance::ConnUri(_)
                        | ConnectiveInstance::ConnByteArray(_) => {}
                    }
                }
            }
            for conditional in &par.conditionals {
                pars.extend(conditional.condition.iter());
                pars.extend(conditional.if_true.iter());
                pars.extend(conditional.if_false.iter());
            }
            for signed in &par.cost_signed_terms {
                pars.extend(signed.body.iter());
                signatures.extend(signed.signature.iter());
            }
            for stack in &par.cost_stacks {
                signatures.extend(stack.cells.iter());
            }
        } else if let Some(signature) = signatures.pop() {
            if let Some(value) = &signature.value {
                reached.cost_values.insert(cost_value_variant_index(value));
                match value {
                    CostValue::Quote(par) | CostValue::Name(par) => pars.push(par),
                    CostValue::Compound(compound) => signatures.extend(compound.elements.iter()),
                    CostValue::Ground(_) | CostValue::BoundLevel(_) | CostValue::Unit(_) => {}
                }
            }
        } else {
            break;
        }
    }
    reached
}

/// Pushes the `Par` operands of one expression. The match has no wildcard
/// arm, so a new arm fails to compile here too.
fn push_expr_operands<'a>(instance: &'a ExprInstance, pars: &mut Vec<&'a Par>) {
    match instance {
        ExprInstance::ENotBody(body) => pars.extend(body.p.iter()),
        ExprInstance::ENegBody(body) => pars.extend(body.p.iter()),
        ExprInstance::EMultBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EDivBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EPlusBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EMinusBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::ELtBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::ELteBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EGtBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EGteBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EEqBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::ENeqBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EAndBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EOrBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EMatchesBody(body) => {
            pars.extend(body.target.iter().chain(body.pattern.iter()))
        }
        ExprInstance::EPercentPercentBody(body) => {
            pars.extend(body.p1.iter().chain(body.p2.iter()))
        }
        ExprInstance::EPlusPlusBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EMinusMinusBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EModBody(body) => pars.extend(body.p1.iter().chain(body.p2.iter())),
        ExprInstance::EListBody(list) => pars.extend(list.ps.iter()),
        ExprInstance::ETupleBody(tuple) => pars.extend(tuple.ps.iter()),
        ExprInstance::ESetBody(set) => pars.extend(set.ps.iter()),
        ExprInstance::EMapBody(map) => {
            for pair in &map.kvs {
                pars.extend(pair.key.iter());
                pars.extend(pair.value.iter());
            }
        }
        ExprInstance::EMethodBody(method) => {
            pars.extend(method.target.iter());
            pars.extend(method.arguments.iter());
        }
        ExprInstance::EPathmapBody(map) => pars.extend(map.ps.iter()),
        ExprInstance::EZipperBody(zipper) => {
            pars.extend(zipper.pathmap.iter().flat_map(|map| map.ps.iter()))
        }
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::EVarBody(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GFloat32(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Layer 3: RANDOM
// ---------------------------------------------------------------------------

/// The expression arms that [`random_par`] can produce.
pub const RANDOM_EXPR_ARMS: [&str; 4] = ["GBool", "GInt", "GString", "ENotBody"];

/// A random `Par` with the shape of the fixed generator of
/// `feature/f1r3lang-mettail-only`
/// (`e5102e945:models/src/rust/test_utils/test_utils.rs:121-445`). Each
/// collection holds 0 to 2 elements. Expressions are the four arms of
/// [`RANDOM_EXPR_ARMS`], and connectives are `ConnNotBody` and `ConnBool`.
/// The generator reaches few arms on purpose: the constructed corpus covers
/// the arms, and this generator covers combinations.
pub fn random_par(depth: usize) -> BoxedStrategy<Par> {
    if depth == 0 {
        return Just(Par::default()).boxed();
    }
    (
        prop::collection::vec(random_send(depth - 1), 0..=2),
        prop::collection::vec(random_receive(depth - 1), 0..=2),
        prop::collection::vec(random_new(depth - 1), 0..=2),
        prop::collection::vec(random_expr(depth - 1), 0..=2),
        prop::collection::vec(random_match(depth - 1), 0..=2),
        prop::collection::vec(random_bundle(depth - 1), 0..=2),
        prop::collection::vec(random_connective(depth - 1), 0..=2),
        prop::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(
                sends,
                receives,
                news,
                exprs,
                matches,
                bundles,
                connectives,
                locally_free,
                connective_used,
            )| Par {
                sends,
                receives,
                news,
                exprs,
                matches,
                bundles,
                connectives,
                locally_free,
                connective_used,
                ..Default::default()
            },
        )
        .boxed()
}

fn random_send(depth: usize) -> BoxedStrategy<Send> {
    if depth == 0 {
        return Just(Send {
            chan: Some(Par::default()),
            ..Default::default()
        })
        .boxed();
    }
    (
        random_par(depth - 1),
        prop::collection::vec(random_par(depth - 1), 0..=2),
        any::<bool>(),
        prop::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(chan, data, persistent, locally_free, connective_used)| Send {
                chan: Some(chan),
                data,
                persistent,
                locally_free,
                connective_used,
            },
        )
        .boxed()
}

fn random_var() -> BoxedStrategy<Var> {
    prop_oneof![
        any::<i32>().prop_map(|v| Var {
            var_instance: Some(VarInstance::BoundVar(v)),
        }),
        any::<i32>().prop_map(|v| Var {
            var_instance: Some(VarInstance::FreeVar(v)),
        }),
        Just(Var {
            var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
        }),
    ]
    .boxed()
}

fn random_receive(depth: usize) -> BoxedStrategy<Receive> {
    if depth == 0 {
        return Just(Receive {
            body: Some(Par::default()),
            ..Default::default()
        })
        .boxed();
    }
    (
        prop::collection::vec(
            (
                prop::collection::vec(random_par(depth - 1), 0..=2),
                random_par(depth - 1),
                prop::option::of(random_var()),
                any::<i32>(),
            )
                .prop_map(|(patterns, source, remainder, free_count)| ReceiveBind {
                    patterns,
                    source: Some(source),
                    remainder,
                    free_count,
                    cost_signature: None,
                }),
            0..=2,
        ),
        random_par(depth - 1),
        any::<bool>(),
        any::<bool>(),
        any::<i32>(),
        prop::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(binds, body, persistent, peek, bind_count, locally_free, connective_used)| Receive {
                binds,
                body: Some(body),
                persistent,
                peek,
                bind_count,
                locally_free,
                connective_used,
                condition: None,
            },
        )
        .boxed()
}

fn random_new(depth: usize) -> BoxedStrategy<New> {
    if depth == 0 {
        return Just(New {
            p: Some(Par::default()),
            ..Default::default()
        })
        .boxed();
    }
    (
        any::<i32>(),
        random_par(depth - 1),
        prop::collection::vec(any::<String>(), 0..=2),
        prop::collection::btree_map(any::<String>(), random_par(depth - 1), 0..=2),
        prop::collection::vec(any::<u8>(), 0..=2),
    )
        .prop_map(|(bind_count, p, uri, injections, locally_free)| New {
            bind_count,
            p: Some(p),
            uri,
            injections,
            locally_free,
        })
        .boxed()
}

fn random_expr(depth: usize) -> BoxedStrategy<Expr> {
    if depth == 0 {
        return Just(Expr {
            expr_instance: None,
        })
        .boxed();
    }
    prop_oneof![
        any::<bool>().prop_map(|v| Expr {
            expr_instance: Some(ExprInstance::GBool(v)),
        }),
        any::<i64>().prop_map(|v| Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }),
        any::<String>().prop_map(|v| Expr {
            expr_instance: Some(ExprInstance::GString(v)),
        }),
        random_par(depth - 1).prop_map(|p| Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(p) })),
        }),
    ]
    .boxed()
}

fn random_match(depth: usize) -> BoxedStrategy<Match> {
    if depth == 0 {
        return Just(Match {
            target: Some(Par::default()),
            ..Default::default()
        })
        .boxed();
    }
    (
        random_par(depth - 1),
        prop::collection::vec(
            (random_par(depth - 1), random_par(depth - 1), any::<i32>()).prop_map(
                |(pattern, source, free_count)| MatchCase {
                    pattern: Some(pattern),
                    source: Some(source),
                    free_count,
                    guard: None,
                },
            ),
            0..=2,
        ),
        prop::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(|(target, cases, locally_free, connective_used)| Match {
            target: Some(target),
            cases,
            locally_free,
            connective_used,
        })
        .boxed()
}

fn random_bundle(depth: usize) -> BoxedStrategy<Bundle> {
    if depth == 0 {
        return Just(Bundle {
            body: Some(Par::default()),
            write_flag: false,
            read_flag: false,
        })
        .boxed();
    }
    (random_par(depth - 1), any::<bool>(), any::<bool>())
        .prop_map(|(body, write_flag, read_flag)| Bundle {
            body: Some(body),
            write_flag,
            read_flag,
        })
        .boxed()
}

fn random_connective(depth: usize) -> BoxedStrategy<Connective> {
    if depth == 0 {
        return Just(Connective {
            connective_instance: None,
        })
        .boxed();
    }
    prop_oneof![
        random_par(depth - 1).prop_map(|p| Connective {
            connective_instance: Some(ConnectiveInstance::ConnNotBody(p)),
        }),
        any::<bool>().prop_map(|v| Connective {
            connective_instance: Some(ConnectiveInstance::ConnBool(v)),
        }),
    ]
    .boxed()
}
