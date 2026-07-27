//! # The constructed substitution corpus
//!
//! ## Why this exists rather than a generator
//!
//! The audit's §8.4 limit #3 records that
//! `models::rust::test_utils::generate_par` produced **zero `Expr` nodes over
//! 256 draws** (every collection range was an exclusive `0..1`, i.e. always
//! empty). The ranges are fixed now, but the generator is still a `Union` of
//! four `ExprInstance` variants out of thirty-six, and it never populates an
//! environment, never sets `bind_count`, never sets `free_count`, and never
//! emits a `VarRef`. Substitution's subtlest behaviour lives in exactly those
//! places:
//!
//! * `maybe_substitute_var` fires only at pattern depth 0 and only for a
//!   `BoundVar` whose index is bound in the environment;
//! * `maybe_substitute_var_ref` fires only when `VarRef.depth == depth`;
//! * `Receive`, `New` and `MatchCase` shift the environment by `bind_count` /
//!   `free_count` before descending, and `set_bits_until` truncates
//!   `locally_free` at `env.shift`;
//! * `EPathmapBody` and `EZipperBody` are **not descended into** and must stay
//!   that way (see
//!   `models::rust::rholang::par_children::substitute_descends_into`).
//!
//! A random generator that cannot reach those shapes gives a green differential
//! that licenses false confidence. This corpus is therefore *constructed*, one
//! representative per schema arm, with a companion test that fails if the
//! schema gains an arm the corpus does not cover — the same discipline
//! `rholang/tests/by_reference_readers_equivalence.rs` uses.
//!
//! ## What a consumer gets
//!
//! * [`populated_env`] — a three-binding environment whose bound values include
//!   a deep term, so `Env::shift`'s `HashMap` clone has something expensive to
//!   copy and a `BoundVar` substitution actually splices a subtree in.
//! * [`substitution_corpus`] — named `(term, depth)` cases spanning every
//!   `ExprInstance` arm, every `ConnectiveInstance` arm, binder-carrying
//!   `Receive`/`New`/`Match`, nested `Bundle`, and `VarRef` at both a matching
//!   and a non-matching depth.
//! * [`EXPR_INSTANCE_VARIANT_COUNT`] / [`CONNECTIVE_INSTANCE_VARIANT_COUNT`] —
//!   re-exported from the canonical child-slot table so a schema change breaks
//!   one constant, not several.

use std::collections::BTreeMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap,
    EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent,
    EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, Expr, GBigRational, GFixedPoint, If,
    KeyValuePair, Match, MatchCase, New, Par, Receive, ReceiveBind, Send, Var, VarRef,
};
use models::rust::rhoapi_ext::EPathMap;

pub use models::rust::rholang::par_children::{
    CONNECTIVE_INSTANCE_VARIANT_COUNT, EXPR_INSTANCE_VARIANT_COUNT,
};

use crate::rust::interpreter::env::Env;

// ---------------------------------------------------------------------------
// building blocks
// ---------------------------------------------------------------------------

/// A `Par` carrying a distinguishing `locally_free` byte, so a traversal that
/// returned the WRONG sub-term (rather than none) is still caught.
pub fn tagged(tag: u8) -> Par {
    Par {
        locally_free: vec![tag],
        ..Default::default()
    }
}

pub fn expr_par(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

pub fn gint(n: i64) -> Par {
    expr_par(ExprInstance::GInt(n))
}

/// `[[[…[0]…]]]` with `depth` bracket levels, built iteratively.
pub fn nested_list(depth: usize) -> Par {
    let mut p = gint(0);
    for _ in 0..depth {
        p = expr_par(ExprInstance::EListBody(EList {
            ps: vec![p],
            locally_free: vec![],
            connective_used: false,
            remainder: None,
        }));
    }
    p
}

/// `BoundVar(index)` as a `Par` — the only shape `maybe_substitute_var`
/// rewrites, and only at pattern depth 0.
pub fn bound_var(index: i32) -> Par {
    expr_par(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::BoundVar(index)),
        }),
    }))
}

/// `FreeVar(index)` as a `Par`.
pub fn free_var(index: i32) -> Par {
    expr_par(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::FreeVar(index)),
        }),
    }))
}

/// A `Par` whose only content is a `VarRef` connective at pattern depth
/// `depth`. `maybe_substitute_var_ref` rewrites it iff `VarRef.depth` equals
/// the depth substitution is running at.
pub fn var_ref(index: i32, depth: i32) -> Par {
    Par {
        connectives: vec![Connective {
            connective_instance: Some(ConnectiveInstance::VarRefBody(VarRef { index, depth })),
        }],
        connective_used: true,
        ..Default::default()
    }
}

fn pathmap_of(ps: Vec<Par>) -> EPathMap {
    EPathMap::new(ps, vec![9], false, None)
}

// ---------------------------------------------------------------------------
// the environment
// ---------------------------------------------------------------------------

/// A populated environment: three bindings, the last of which is a **deep**
/// term.
///
/// Depth matters for two independent reasons.
///
/// 1. `Env::shift(j)` is `Env { shift: self.shift + j, ..(*self).clone() }` — a
///    full `HashMap<i32, Par>` clone, hence a `<Par as Clone>::clone` of every
///    bound value, at every binder level. `Par::clone` is itself Θ(depth)
///    (15,875 B/level debug), so a worklist that constructs owned `Env`s per
///    item stays Θ(depth) in native stack no matter how the traversal itself is
///    written. That is measurable: `subst_binders` costs 48,878 B/level under
///    this environment against 33,242 B/level under a ground one — a difference
///    of 15,636 B/level, which is `Par::clone`'s constant to within 1.5%.
/// 2. Substituting a `BoundVar` splices the bound value into the result, so a
///    deep binding is also what makes the *result* deep.
pub fn populated_env() -> Env<Par> {
    let mut env: Env<Par> = Env::new();
    let mut env = env.put(gint(101));
    let mut env = env.put(expr_par(ExprInstance::GString("bound".to_string())));
    env.put(nested_list(3))
}

/// The empty environment — `maybe_substitute_var` finds nothing and every
/// `BoundVar` is returned unchanged. Half the corpus should be run under each,
/// because "found" and "not found" are different code paths.
pub fn empty_env() -> Env<Par> {
    Env::new()
}

// ---------------------------------------------------------------------------
// per-arm corpora
// ---------------------------------------------------------------------------

/// One representative of every `ExprInstance` variant, each carrying child
/// `Par`s that are themselves substitutable (bound vars and nested lists), so
/// that an arm which forgot to descend into a slot produces a *different*
/// result rather than an equal one.
pub fn every_expr_instance() -> Vec<(&'static str, ExprInstance)> {
    let a = || Some(bound_var(0));
    let b = || Some(nested_list(2));
    vec![
        ("GBool", ExprInstance::GBool(true)),
        ("GInt", ExprInstance::GInt(-7)),
        ("GString", ExprInstance::GString("s".to_string())),
        ("GUri", ExprInstance::GUri("rho:id:x".to_string())),
        ("GByteArray", ExprInstance::GByteArray(vec![1, 2, 3])),
        ("GDouble", ExprInstance::GDouble((-1.5f64).to_bits())),
        ("GBigInt", ExprInstance::GBigInt(vec![9, 9])),
        (
            "GBigRat",
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![1],
                denominator: vec![3],
            }),
        ),
        (
            "GFixedPoint",
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![5],
                scale: 2,
            }),
        ),
        (
            "EVarBody/BoundVar",
            ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            }),
        ),
        ("ENotBody", ExprInstance::ENotBody(ENot { p: a() })),
        ("ENegBody", ExprInstance::ENegBody(ENeg { p: a() })),
        (
            "EMultBody",
            ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
        ),
        ("EDivBody", ExprInstance::EDivBody(EDiv { p1: a(), p2: b() })),
        ("EModBody", ExprInstance::EModBody(EMod { p1: a(), p2: b() })),
        (
            "EPlusBody",
            ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
        ),
        // ⚠ The dead `SubstituteTrait<Expr>::substitute` used to rebuild this
        // arm as an `EPlusBody`. Its live twin does not. Keeping the arm in the
        // corpus is what would have caught that divergence had the dead method
        // ever acquired a caller.
        (
            "EMinusBody",
            ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
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
            "EPercentPercentBody",
            ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
        ),
        ("ELtBody", ExprInstance::ELtBody(ELt { p1: a(), p2: b() })),
        ("ELteBody", ExprInstance::ELteBody(ELte { p1: a(), p2: b() })),
        ("EGtBody", ExprInstance::EGtBody(EGt { p1: a(), p2: b() })),
        ("EGteBody", ExprInstance::EGteBody(EGte { p1: a(), p2: b() })),
        ("EEqBody", ExprInstance::EEqBody(EEq { p1: a(), p2: b() })),
        ("ENeqBody", ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() })),
        ("EAndBody", ExprInstance::EAndBody(EAnd { p1: a(), p2: b() })),
        ("EOrBody", ExprInstance::EOrBody(EOr { p1: a(), p2: b() })),
        (
            "EMatchesBody",
            ExprInstance::EMatchesBody(EMatches {
                target: a(),
                pattern: b(),
            }),
        ),
        (
            "EListBody",
            ExprInstance::EListBody(EList {
                ps: vec![bound_var(0), nested_list(2), gint(3)],
                locally_free: vec![0xff, 0xff],
                connective_used: false,
                remainder: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            }),
        ),
        (
            "ETupleBody",
            ExprInstance::ETupleBody(ETuple {
                ps: vec![bound_var(0), gint(4)],
                locally_free: vec![0xff, 0xff],
                connective_used: false,
            }),
        ),
        (
            "ESetBody",
            ExprInstance::ESetBody(ESet {
                ps: vec![gint(2), gint(1), bound_var(0)],
                locally_free: vec![0xff, 0xff],
                connective_used: false,
                remainder: None,
            }),
        ),
        (
            "EMapBody",
            ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(gint(2)),
                        value: Some(bound_var(0)),
                    },
                    KeyValuePair {
                        key: Some(gint(1)),
                        value: Some(nested_list(2)),
                    },
                ],
                locally_free: vec![0xff, 0xff],
                connective_used: false,
                remainder: None,
            }),
        ),
        // ⚠ NOT descended into by `Substitute` today. The corpus carries a
        // substitutable child on purpose: if a conversion ever starts
        // descending, the differential reports a changed result rather than
        // letting a consensus-visible "fix" ride along unnoticed.
        (
            "EPathmapBody",
            ExprInstance::EPathmapBody(pathmap_of(vec![bound_var(0), gint(5)])),
        ),
        (
            "EZipperBody",
            // `..Default::default()` rather than an exhaustive struct literal:
            // `EZipper` is a `prost` message and gains fields additively (the
            // `cursor_kind` cursor-semantics tag, for one). A corpus entry that
            // enumerated every field would break this file every time the
            // schema grew, for no coverage — the fields that matter to
            // substitution are the ones set here, and the exhaustiveness that
            // DOES matter (one entry per `ExprInstance` variant) is enforced by
            // `corpus_covers_every_expr_instance_variant`.
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(pathmap_of(vec![bound_var(0)])),
                current_path: vec![vec![1, 2]],
                is_write_zipper: false,
                locally_free: vec![0xff],
                connective_used: false,
                ..Default::default()
            }),
        ),
        (
            "EMethodBody",
            ExprInstance::EMethodBody(EMethod {
                method_name: "nth".to_string(),
                target: a(),
                arguments: vec![gint(0), bound_var(0)],
                locally_free: vec![0xff, 0xff],
                connective_used: false,
            }),
        ),
    ]
}

/// One representative of every `ConnectiveInstance` variant. The `VarRefBody`
/// entry is at depth 0 so that it *fires* when the corpus is substituted at
/// depth 0; [`substitution_corpus`] adds a non-matching-depth twin.
pub fn every_connective_instance() -> Vec<(&'static str, ConnectiveInstance)> {
    vec![
        (
            "ConnAndBody",
            ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![bound_var(0), nested_list(2)],
            }),
        ),
        (
            "ConnOrBody",
            ConnectiveInstance::ConnOrBody(ConnectiveBody {
                ps: vec![bound_var(0), gint(1)],
            }),
        ),
        (
            "ConnNotBody",
            ConnectiveInstance::ConnNotBody(bound_var(0)),
        ),
        (
            "VarRefBody@0",
            ConnectiveInstance::VarRefBody(VarRef { index: 0, depth: 0 }),
        ),
        ("ConnBool", ConnectiveInstance::ConnBool(true)),
        ("ConnInt", ConnectiveInstance::ConnInt(true)),
        ("ConnString", ConnectiveInstance::ConnString(true)),
        ("ConnUri", ConnectiveInstance::ConnUri(true)),
        ("ConnByteArray", ConnectiveInstance::ConnByteArray(true)),
    ]
}

// ---------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------

/// One case of the corpus: a named term and the pattern depth to substitute at.
#[derive(Clone, Debug)]
pub struct SubstitutionCase {
    pub name: String,
    pub term: Par,
    pub depth: i32,
}

fn case(name: impl Into<String>, term: Par, depth: i32) -> SubstitutionCase {
    SubstitutionCase {
        name: name.into(),
        term,
        depth,
    }
}

/// Every schema arm, plus the binder-carrying and depth-sensitive shapes a
/// random generator does not reach.
///
/// Each `ExprInstance` and `ConnectiveInstance` arm appears **twice**: once at
/// pattern depth 0 (where `maybe_substitute_var` / `maybe_substitute_var_ref`
/// fire) and once at depth 1 (where they must return the term untouched).
pub fn substitution_corpus() -> Vec<SubstitutionCase> {
    let mut out: Vec<SubstitutionCase> = Vec::with_capacity(128);

    for (name, instance) in every_expr_instance() {
        for depth in [0i32, 1] {
            out.push(case(
                format!("expr/{name}@d{depth}"),
                expr_par(instance.clone()),
                depth,
            ));
        }
    }

    for (name, instance) in every_connective_instance() {
        for depth in [0i32, 1] {
            out.push(case(
                format!("conn/{name}@d{depth}"),
                Par {
                    connectives: vec![Connective {
                        connective_instance: Some(instance.clone()),
                    }],
                    connective_used: true,
                    ..Default::default()
                },
                depth,
            ));
        }
    }

    // ---- VarRef at a NON-matching depth: must be left alone ----
    out.push(case("conn/VarRefBody@mismatch", var_ref(0, 3), 0));
    out.push(case("conn/VarRefBody@match-at-1", var_ref(0, 1), 1));

    // ---- binder-carrying process forms ----

    // `for (x, y <- @0 if <guard>) { … }` — bind_count 2, a guard, a bound var
    // in the body (which the shifted env must NOT resolve at shift 2) and in
    // the source channel (which it must).
    out.push(case(
        "receive/bind_count=2+guard",
        Par {
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![free_var(0), free_var(1)],
                    source: Some(bound_var(0)),
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                    }),
                    free_count: 2,
                }],
                body: Some(Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                            p1: Some(bound_var(0)),
                            p2: Some(bound_var(2)),
                        })),
                    }],
                    locally_free: vec![0xff, 0xff, 0xff, 0xff],
                    ..Default::default()
                }),
                persistent: false,
                peek: false,
                bind_count: 2,
                locally_free: vec![0xff, 0xff, 0xff, 0xff],
                connective_used: false,
                condition: Some(bound_var(1)),
            }],
            ..Default::default()
        },
        0,
    ));

    // `new x, y in { … }` with a URI-bound injection.
    out.push(case(
        "new/bind_count=2+injections",
        Par {
            news: vec![New {
                bind_count: 2,
                p: Some(Par {
                    exprs: vec![Expr {
                        expr_instance: Some(ExprInstance::EListBody(EList {
                            ps: vec![bound_var(0), bound_var(2)],
                            locally_free: vec![0xff, 0xff],
                            connective_used: false,
                            remainder: None,
                        })),
                    }],
                    locally_free: vec![0xff, 0xff, 0xff],
                    ..Default::default()
                }),
                uri: vec!["rho:io:stdout".to_string()],
                injections: {
                    let mut m = BTreeMap::new();
                    m.insert("rho:io:stdout".to_string(), bound_var(0));
                    m
                },
                locally_free: vec![0xff, 0xff, 0xff],
            }],
            ..Default::default()
        },
        0,
    ));

    // `match … { case … if <guard> => … }` — free_count > 0 and a guard, so the
    // case's source and guard are substituted under a SHIFTED env while its
    // pattern is substituted at depth + 1.
    out.push(case(
        "match/free_count=1+guard",
        Par {
            matches: vec![Match {
                target: Some(bound_var(0)),
                cases: vec![
                    MatchCase {
                        pattern: Some(free_var(0)),
                        source: Some(bound_var(0)),
                        free_count: 1,
                        guard: Some(bound_var(1)),
                    },
                    MatchCase {
                        pattern: Some(nested_list(2)),
                        source: Some(gint(9)),
                        free_count: 0,
                        guard: None,
                    },
                ],
                locally_free: vec![0xff, 0xff, 0xff],
                connective_used: false,
            }],
            ..Default::default()
        },
        0,
    ));

    // Nested bundles: `bundle+ { bundle0 { … } }`. The inner-bundle merge
    // (`single_bundle` / `BundleOps::merge`) is a post-order rewrite that a
    // worklist has to reproduce exactly.
    out.push(case(
        "bundle/nested-merge",
        Par {
            bundles: vec![Bundle {
                body: Some(Par {
                    bundles: vec![Bundle {
                        body: Some(bound_var(0)),
                        write_flag: false,
                        read_flag: true,
                    }],
                    ..Default::default()
                }),
                write_flag: true,
                read_flag: true,
            }],
            ..Default::default()
        },
        0,
    ));

    // A `Send` on a bound channel with several data terms.
    out.push(case(
        "send/bound-channel",
        Par {
            sends: vec![Send {
                chan: Some(bound_var(0)),
                data: vec![bound_var(0), nested_list(2), gint(1)],
                persistent: true,
                locally_free: vec![0xff, 0xff],
                connective_used: false,
            }],
            ..Default::default()
        },
        0,
    ));

    // A first-class conditional.
    out.push(case(
        "if/three-branches",
        Par {
            conditionals: vec![If {
                condition: Some(bound_var(0)),
                if_true: Some(nested_list(2)),
                if_false: Some(bound_var(1)),
                locally_free: vec![0xff, 0xff],
                connective_used: false,
            }],
            ..Default::default()
        },
        0,
    ));

    // A `Par` whose every slot is occupied at once — the shape that proves the
    // driver's per-slot ordering, not just each slot in isolation.
    out.push(case(
        "par/all-slots-occupied",
        Par {
            exprs: vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(2)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::EVarBody(EVar {
                        v: Some(Var {
                            var_instance: Some(VarInstance::BoundVar(0)),
                        }),
                    })),
                },
            ],
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(bound_var(0))),
            }],
            sends: vec![Send {
                chan: Some(gint(0)),
                data: vec![bound_var(0)],
                ..Default::default()
            }],
            bundles: vec![Bundle {
                body: Some(bound_var(0)),
                write_flag: true,
                read_flag: false,
            }],
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![free_var(0)],
                    source: Some(gint(0)),
                    remainder: None,
                    free_count: 1,
                }],
                body: Some(bound_var(0)),
                bind_count: 1,
                locally_free: vec![0xff, 0xff],
                ..Default::default()
            }],
            news: vec![New {
                bind_count: 1,
                p: Some(bound_var(0)),
                uri: vec![],
                injections: BTreeMap::new(),
                locally_free: vec![0xff, 0xff],
            }],
            matches: vec![Match {
                target: Some(bound_var(0)),
                cases: vec![],
                locally_free: vec![0xff],
                connective_used: false,
            }],
            conditionals: vec![If {
                condition: Some(bound_var(0)),
                if_true: Some(gint(1)),
                if_false: Some(gint(0)),
                locally_free: vec![0xff],
                connective_used: false,
            }],
            locally_free: vec![0xff, 0xff, 0xff, 0xff],
            connective_used: true,
            unforgeables: vec![],
        },
        0,
    ));

    // Deep nesting under binders — the shape whose environment handling the
    // audit calls out as subtlest, and the shape the generator cannot reach.
    let mut binders = gint(0);
    for level in 0..6 {
        binders = Par {
            news: vec![New {
                bind_count: 1,
                p: Some(Par {
                    receives: vec![Receive {
                        binds: vec![ReceiveBind {
                            patterns: vec![free_var(0)],
                            source: Some(bound_var(level)),
                            remainder: None,
                            free_count: 1,
                        }],
                        body: Some(binders),
                        persistent: false,
                        peek: false,
                        bind_count: 1,
                        locally_free: vec![0xff, 0xff, 0xff, 0xff],
                        connective_used: false,
                        condition: None,
                    }],
                    ..Default::default()
                }),
                uri: vec![],
                injections: BTreeMap::new(),
                locally_free: vec![0xff, 0xff, 0xff, 0xff],
            }],
            ..Default::default()
        };
    }
    out.push(case("binders/nested-new-receive-x6", binders, 0));

    // A deep pure chain: the reported reproducer's shape.
    out.push(case("chain/nested-list-x12", nested_list(12), 0));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::rust::rholang::par_children::reachable_pars;

    #[test]
    fn corpus_covers_every_expr_instance_variant() {
        assert_eq!(
            every_expr_instance().len(),
            EXPR_INSTANCE_VARIANT_COUNT,
            "the substitution corpus no longer covers every ExprInstance variant in \
             RhoTypes.proto — a differential driven by it would silently stop testing \
             the new arm"
        );
    }

    #[test]
    fn corpus_covers_every_connective_instance_variant() {
        assert_eq!(
            every_connective_instance().len(),
            CONNECTIVE_INSTANCE_VARIANT_COUNT,
            "the substitution corpus no longer covers every ConnectiveInstance variant"
        );
    }

    #[test]
    fn corpus_reaches_the_shapes_it_claims_to() {
        let corpus = substitution_corpus();
        let has = |predicate: fn(&Par) -> bool| {
            corpus
                .iter()
                .any(|c| reachable_pars(&c.term).into_iter().any(predicate))
        };

        assert!(
            has(|p| p.receives.iter().any(|r| r.bind_count > 0)),
            "corpus has no Receive with bind_count > 0"
        );
        assert!(
            has(|p| p.receives.iter().any(|r| r.condition.is_some())),
            "corpus has no Receive with a where-clause guard"
        );
        assert!(
            has(|p| p.news.iter().any(|n| n.bind_count > 0)),
            "corpus has no New with binders"
        );
        assert!(
            has(|p| p.news.iter().any(|n| !n.injections.is_empty())),
            "corpus has no New with injections"
        );
        assert!(
            has(|p| p
                .matches
                .iter()
                .any(|m| m.cases.iter().any(|c| c.free_count > 0))),
            "corpus has no MatchCase with free_count > 0"
        );
        assert!(
            has(|p| p
                .matches
                .iter()
                .any(|m| m.cases.iter().any(|c| c.guard.is_some()))),
            "corpus has no MatchCase with a guard"
        );
        assert!(
            has(|p| p
                .bundles
                .iter()
                .any(|b| b.body.iter().any(|inner| !inner.bundles.is_empty()))),
            "corpus has no nested Bundle"
        );
        assert!(
            has(|p| p.conditionals.iter().any(|_| true)),
            "corpus has no If"
        );
        assert!(
            corpus.iter().any(|c| c.name.contains("VarRefBody@mismatch")),
            "corpus has no VarRef at a NON-matching depth"
        );
        assert!(
            corpus.iter().any(|c| c.depth == 0) && corpus.iter().any(|c| c.depth == 1),
            "corpus must exercise both pattern depth 0 and pattern depth 1"
        );
    }

    #[test]
    fn populated_env_binds_a_deep_value() {
        let env = populated_env();
        assert_eq!(env.level, 3, "populated_env must carry three bindings");
        let deep = env
            .get(&0)
            .expect("populated_env: the most recent binding must resolve at index 0");
        assert!(
            reachable_pars(&deep).len() > 3,
            "populated_env's most recent binding must be DEEP — a shallow one cannot \
             exercise Env::shift's per-level `Par::clone`"
        );
    }
}
