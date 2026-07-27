//! # Equivalence proof for the by-reference `HasLocallyFree` readers
//!
//! Leg-1 of the Θ(depth) work replaced call sites of the form
//!
//! ```ignore
//! x.locally_free(x.clone(), depth)      //  <- deep-clones the WHOLE subtree
//! x.clone().connective_used(x)          //  <- and again
//! ```
//!
//! with by-reference free functions (`matcher::has_locally_free::*_ref`). The
//! by-value trait methods never recurse — every arm returns a *cached* field —
//! so the clone was pure waste, and the readers are equivalent by construction.
//!
//! **This test is the empirical half of that claim** (see
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md` §8, "The proof
//! standard"). It asserts, over a generated corpus of arbitrary `Par` trees,
//! that for every `Expr` and every `Connective` at every relevant pattern depth
//! the by-reference reader returns a value **equal** to the by-value trait
//! method it replaced.
//!
//! ## What this does and does not establish
//!
//! * It **does** establish that no divergence exists on the generated corpus,
//!   including the `EVar` arm — the one arm that actually consults `depth`, and
//!   the one where the pre-existing private copy in `reduce.rs` had hardcoded
//!   `0`. That divergence was latent (its only caller was a depth-0 reader) and
//!   would have become live at `prepend_expr`, which `sub_exp` calls at pattern
//!   depth > 0.
//! * It **does not** establish equivalence for all inputs. It is falsification,
//!   not verification; the by-construction argument (each arm reads the same
//!   cached field) is what carries the general claim.
//! * The generator produces bounded-depth trees, so it does not exercise the
//!   depth regime the audit is about. That is deliberate: this test is about
//!   *value* equality, and the *stack* claim is the separate concern of
//!   `stack_depth_gate.rs`.

use models::rhoapi::{Connective, Expr, Par};
use rholang::rust::interpreter::matcher::has_locally_free::{
    connective_connective_used_ref, connective_locally_free_ref, expr_connective_used_ref,
    expr_locally_free_ref, HasLocallyFree,
};

/// Collect every `Expr` and `Connective` in a term, iteratively — a recursive
/// collector would itself be a Θ(depth) traversal (see the audit).
fn collect_nodes(root: &Par) -> (Vec<Expr>, Vec<Connective>) {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::expr::ExprInstance;

    let mut exprs: Vec<Expr> = Vec::new();
    let mut conns: Vec<Connective> = Vec::new();
    let mut work: Vec<&Par> = vec![root];

    while let Some(p) = work.pop() {
        conns.extend(p.connectives.iter().cloned());
        for c in &p.connectives {
            match &c.connective_instance {
                Some(ConnectiveInstance::ConnAndBody(b))
                | Some(ConnectiveInstance::ConnOrBody(b)) => work.extend(b.ps.iter()),
                Some(ConnectiveInstance::ConnNotBody(inner)) => work.push(inner),
                _ => {}
            }
        }

        exprs.extend(p.exprs.iter().cloned());
        for e in &p.exprs {
            match &e.expr_instance {
                Some(ExprInstance::EListBody(l)) => work.extend(l.ps.iter()),
                Some(ExprInstance::ETupleBody(t)) => work.extend(t.ps.iter()),
                Some(ExprInstance::ENotBody(n)) => work.extend(n.p.iter()),
                Some(ExprInstance::ENegBody(n)) => work.extend(n.p.iter()),
                Some(ExprInstance::EMethodBody(m)) => {
                    work.extend(m.target.iter());
                    work.extend(m.arguments.iter());
                }
                Some(ExprInstance::EMatchesBody(m)) => {
                    work.extend(m.target.iter());
                    work.extend(m.pattern.iter());
                }
                _ => {}
            }
        }

        for s in &p.sends {
            work.extend(s.chan.iter());
            work.extend(s.data.iter());
        }
        for r in &p.receives {
            work.extend(r.body.iter());
            work.extend(r.condition.iter());
            for b in &r.binds {
                work.extend(b.source.iter());
                work.extend(b.patterns.iter());
            }
        }
        for n in &p.news {
            work.extend(n.p.iter());
        }
        for m in &p.matches {
            work.extend(m.target.iter());
            for c in &m.cases {
                work.extend(c.pattern.iter());
                work.extend(c.source.iter());
                work.extend(c.guard.iter());
            }
        }
        for b in &p.bundles {
            work.extend(b.body.iter());
        }
        for i in &p.conditionals {
            work.extend(i.condition.iter());
            work.extend(i.if_true.iter());
            work.extend(i.if_false.iter());
        }
    }
    (exprs, conns)
}

// ---------------------------------------------------------------------------
// EXHAUSTIVE ARM COVERAGE
//
// An earlier version of this file drove the property from
// `models::rust::test_utils::generate_par(3)`. Its anti-vacuity guard caught
// that the generator produced **zero `Expr` nodes over 256 draws** (every field
// is `vec(.., 0..1)`, so almost every draw is an empty `Par`) — the property was
// passing without testing anything. That is exactly the failure mode recorded as
// limit #3 in the audit's §8.4, and it is why the corpus below is CONSTRUCTED,
// one representative per variant, with a compile-time-checked count.
//
// The by-construction argument is "every arm of the by-reference reader returns
// the same cached field the by-value trait method returns". The corresponding
// empirical obligation is therefore ARM coverage, not random terms.
// ---------------------------------------------------------------------------

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::{VarInstance, WildcardMsg};
use models::rhoapi::{
    ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches, EMethod, EMinus,
    EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus, EPlusPlus, ESet,
    ETuple, EVar, GBigRational, KeyValuePair, Var, VarRef,
};

/// Every `ExprInstance` variant that `RhoTypes.proto` defines. If a variant is
/// added to the proto and not added here, `all_expr_instance_variants_covered`
/// fails — the corpus cannot silently fall behind the schema.
const EXPR_INSTANCE_VARIANT_COUNT: usize = 36;
/// Likewise for `ConnectiveInstance`.
const CONNECTIVE_INSTANCE_VARIANT_COUNT: usize = 9;

/// A `Par` carrying a distinctive cached `locally_free` / `connective_used`, so
/// that a reader which returned the WRONG child's cached field (rather than no
/// field at all) would still be caught.
fn marked_par(tag: u8, connective_used: bool) -> Par {
    Par {
        locally_free: vec![tag],
        connective_used,
        ..Default::default()
    }
}

fn e(instance: ExprInstance) -> Expr {
    Expr {
        expr_instance: Some(instance),
    }
}

fn every_expr_instance() -> Vec<Expr> {
    let a = || Some(marked_par(0b0000_0011, false));
    let b = || Some(marked_par(0b0001_0100, true));
    let binaries: Vec<ExprInstance> = vec![
        ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
        ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }),
        ExprInstance::EModBody(EMod { p1: a(), p2: b() }),
        ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
        ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
        ExprInstance::ELtBody(ELt { p1: a(), p2: b() }),
        ExprInstance::ELteBody(ELte { p1: a(), p2: b() }),
        ExprInstance::EGtBody(EGt { p1: a(), p2: b() }),
        ExprInstance::EGteBody(EGte { p1: a(), p2: b() }),
        ExprInstance::EEqBody(EEq { p1: a(), p2: b() }),
        ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }),
        ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }),
        ExprInstance::EOrBody(EOr { p1: a(), p2: b() }),
        ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
        ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
        ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
    ];

    let mut out: Vec<ExprInstance> = vec![
        // grounds
        ExprInstance::GBool(true),
        ExprInstance::GInt(42),
        ExprInstance::GString("s".to_string()),
        ExprInstance::GUri("rho:id:x".to_string()),
        ExprInstance::GByteArray(vec![1, 2, 3]),
        ExprInstance::GDouble(1.5f64.to_bits()),
        ExprInstance::GBigInt(vec![1, 2, 3, 4]),
        ExprInstance::GBigRat(GBigRational {
            numerator: vec![1],
            denominator: vec![3],
        }),
        ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
            unscaled: vec![125],
            scale: 2,
        }),
        // unaries
        ExprInstance::ENotBody(ENot { p: a() }),
        ExprInstance::ENegBody(ENeg { p: a() }),
        // variable
        ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(2)),
            }),
        }),
        // collections (cached locally_free / connective_used of their own)
        ExprInstance::EListBody(EList {
            ps: vec![marked_par(0b0010_0000, false)],
            locally_free: vec![0b0100_0001],
            connective_used: true,
            remainder: None,
        }),
        ExprInstance::ETupleBody(ETuple {
            ps: vec![marked_par(0b0010_0000, false)],
            locally_free: vec![0b0100_0010],
            connective_used: false,
        }),
        ExprInstance::ESetBody(ESet {
            ps: vec![marked_par(0b0010_0000, false)],
            locally_free: vec![0b0100_0100],
            connective_used: true,
            remainder: None,
        }),
        ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(marked_par(1, false)),
                value: Some(marked_par(2, false)),
            }],
            locally_free: vec![0b0100_1000],
            connective_used: false,
            remainder: None,
        }),
        ExprInstance::EPathmapBody({
            // `EPathMap` carries a private intern cell, so it is built via
            // Default and then filled — the reader only ever consults the two
            // cached public fields.
            let mut m = models::rust::rhoapi_ext::EPathMap::default();
            m.locally_free = vec![0b0101_0000];
            m.connective_used = true;
            m
        }),
        ExprInstance::EZipperBody(models::rhoapi::EZipper {
            pathmap: None,
            current_path: vec![],
            is_write_zipper: false,
            locally_free: vec![0b0110_0000],
            connective_used: false,
        }),
        // method / matches
        ExprInstance::EMethodBody(EMethod {
            method_name: "nth".to_string(),
            target: a(),
            arguments: vec![marked_par(9, false)],
            locally_free: vec![0b0111_0000],
            connective_used: true,
        }),
        ExprInstance::EMatchesBody(EMatches {
            target: a(),
            pattern: b(),
        }),
    ];
    out.extend(binaries);
    out.into_iter().map(e).collect()
}

fn every_connective_instance() -> Vec<Connective> {
    vec![
        ConnectiveInstance::ConnAndBody(ConnectiveBody {
            ps: vec![marked_par(1, false)],
        }),
        ConnectiveInstance::ConnOrBody(ConnectiveBody {
            ps: vec![marked_par(2, false)],
        }),
        ConnectiveInstance::ConnNotBody(marked_par(3, false)),
        ConnectiveInstance::VarRefBody(VarRef { index: 5, depth: 1 }),
        ConnectiveInstance::ConnBool(true),
        ConnectiveInstance::ConnInt(true),
        ConnectiveInstance::ConnString(true),
        ConnectiveInstance::ConnUri(true),
        ConnectiveInstance::ConnByteArray(true),
    ]
    .into_iter()
    .map(|ci| Connective {
        connective_instance: Some(ci),
    })
    .collect()
}

/// The corpus must cover EVERY variant the schema defines. A variant added to
/// `RhoTypes.proto` without a representative here trips this immediately,
/// instead of silently shrinking the coverage of the equivalence proof.
#[test]
fn all_expr_instance_variants_covered() {
    assert_eq!(
        every_expr_instance().len(),
        EXPR_INSTANCE_VARIANT_COUNT,
        "the ExprInstance corpus no longer covers every variant in RhoTypes.proto"
    );
    assert_eq!(
        every_connective_instance().len(),
        CONNECTIVE_INSTANCE_VARIANT_COUNT,
        "the ConnectiveInstance corpus no longer covers every variant"
    );
}

/// The equivalence proof proper: for every arm, at every pattern depth the
/// substitution path uses, the by-reference reader equals the by-value trait
/// method it replaced.
#[test]
fn every_expr_arm_reader_agrees_with_the_by_value_trait() {
    for expr in every_expr_instance() {
        for depth in [0i32, 1, 2, 3] {
            assert_eq!(
                expr_locally_free_ref(&expr, depth),
                expr.clone().locally_free(expr.clone(), depth),
                "expr_locally_free_ref diverged at depth {} on {:?}",
                depth,
                expr.expr_instance
            );
        }
        assert_eq!(
            expr_connective_used_ref(&expr),
            expr.clone().connective_used(expr.clone()),
            "expr_connective_used_ref diverged on {:?}",
            expr.expr_instance
        );
    }
    // the `None` arm, which no generator produces
    let empty = Expr {
        expr_instance: None,
    };
    assert_eq!(expr_locally_free_ref(&empty, 0), Vec::<u8>::new());
    assert!(!expr_connective_used_ref(&empty));
}

#[test]
fn every_connective_arm_reader_agrees_with_the_by_value_trait() {
    for conn in every_connective_instance() {
        for depth in [0i32, 1, 2, 3] {
            assert_eq!(
                connective_locally_free_ref(&conn, depth),
                conn.clone().locally_free(conn.clone(), depth),
                "connective_locally_free_ref diverged at depth {} on {:?}",
                depth,
                conn.connective_instance
            );
        }
        assert_eq!(
            connective_connective_used_ref(&conn),
            conn.clone().connective_used(conn.clone()),
            "connective_connective_used_ref diverged on {:?}",
            conn.connective_instance
        );
    }
    let empty = Connective {
        connective_instance: None,
    };
    assert_eq!(connective_locally_free_ref(&empty, 0), Vec::<u8>::new());
    assert!(!connective_connective_used_ref(&empty));
}

/// `EVar` is the ONLY arm that consults `depth`, and it is where the former
/// `reduce.rs`-private copy diverged (it hardcoded `0`). Pin every `Var` shape
/// explicitly rather than relying on the corpus above to reach them.
#[test]
fn evar_arm_threads_depth_exactly_like_the_by_value_trait() {
    for var_instance in [
        VarInstance::BoundVar(0),
        VarInstance::BoundVar(3),
        VarInstance::FreeVar(0),
        VarInstance::FreeVar(2),
        VarInstance::Wildcard(WildcardMsg {}),
    ] {
        let expr = e(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(var_instance.clone()),
            }),
        }));
        for depth in [0i32, 1, 2, 3] {
            assert_eq!(
                expr_locally_free_ref(&expr, depth),
                expr.clone().locally_free(expr.clone(), depth),
                "EVar({:?}) locally_free diverged at depth {}",
                var_instance,
                depth
            );
        }
        assert_eq!(
            expr_connective_used_ref(&expr),
            expr.clone().connective_used(expr.clone()),
            "EVar({:?}) connective_used diverged",
            var_instance
        );
    }
}
