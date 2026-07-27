//! # The canonical child-`Par` slot table for the recursive `Par` family
//!
//! Every traversal over the `Par` / `Expr` / `ExprInstance` / `Connective`
//! family has to answer the same question at every node: *which sub-`Par`s does
//! this node contain, and in what order?* Before this module, that question was
//! answered independently in at least four places —
//! `substitute.rs`, the `sorter/` family, `pretty_printer.rs`, and the ad-hoc
//! `collect_nodes` in `rholang/tests/by_reference_readers_equivalence.rs` — and
//! a fifth was about to be added for each explicit-worklist driver. Independent
//! enumerations of a 36-variant schema drift; this module makes drift a
//! **compile error**.
//!
//! Full analysis and proof standard:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
//!
//! ## The three guards
//!
//! 1. **Exhaustiveness.** Every `match` in this module lists every variant and
//!    has **no `_` arm**. A variant added to `RhoTypes.proto` therefore fails
//!    to compile here, rather than silently falling into a catch-all in four
//!    separate files.
//! 2. **Variant count.** [`EXPR_INSTANCE_VARIANT_COUNT`] and
//!    [`CONNECTIVE_INSTANCE_VARIANT_COUNT`] pin the schema size, and the tests
//!    in this file construct one representative per variant and assert the
//!    count. That catches a variant added to the schema *and* to the matches
//!    here but not to any corpus.
//! 3. **Per-traversal disposition.** Not every traversal descends into every
//!    structural child. [`substitute_descends_into`] records — exhaustively,
//!    and therefore checkably — which variants `Substitute` walks today. Two
//!    variants (`EPathmapBody`, `EZipperBody`) are deliberately **NOT**
//!    descended into by substitution; see that function's documentation.
//!
//! ## Two tables, deliberately
//!
//! * [`expr_child_pars`] and friends give the **structural** children: exactly
//!   what `drop_in_place` walks. Consumers that must reach every reachable
//!   `Par` (teardown, the stack-depth gate, node collectors) use these.
//! * [`substitute_descends_into`] gives `Substitute`'s **disposition**, which
//!   is a strict subset. Keeping them separate is what stops a worklist
//!   conversion from silently "fixing" a variant the recursive form never
//!   visited — a change that would alter consensus-visible output.
//!
//! ## Ordering
//!
//! The order in which children are pushed is the order a post-order traversal
//! must visit them, and it matches the field order of the corresponding arm in
//! `Substitute::substitute_no_sort`. It is part of the contract: a driver that
//! visits children in a different order can still produce the same result, but
//! only if nothing in the traversal is order-sensitive. Do not reorder.

use std::collections::BTreeMap;

use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Connective, Expr, Par};

/// The number of variants `expr.ExprInstance` has in `RhoTypes.proto`.
///
/// Bumping this without extending every corpus that asserts against it is the
/// mistake this constant exists to prevent.
pub const EXPR_INSTANCE_VARIANT_COUNT: usize = 36;

/// The number of variants `connective.ConnectiveInstance` has.
pub const CONNECTIVE_INSTANCE_VARIANT_COUNT: usize = 9;

// ===========================================================================
// STRUCTURAL children, by reference
// ===========================================================================

/// Append every child `Par` structurally contained in `e`, in traversal order.
///
/// "Structurally contained" means *reachable by `drop_in_place`* — including
/// `EPathmapBody` and `EZipperBody`, which `Substitute` does not descend into
/// but teardown certainly does.
pub fn expr_instance_child_pars<'a>(e: &'a ExprInstance, out: &mut Vec<&'a Par>) {
    match e {
        // ---- grounds: no child Par ----
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => {}

        // ---- a variable is a `Var`, not a `Par` ----
        ExprInstance::EVarBody(_) => {}

        // ---- unary ----
        ExprInstance::ENotBody(x) => out.extend(x.p.iter()),
        ExprInstance::ENegBody(x) => out.extend(x.p.iter()),

        // ---- binary (p1 then p2) ----
        ExprInstance::EMultBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EDivBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EModBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EPlusBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EMinusBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EPlusPlusBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EMinusMinusBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EPercentPercentBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::ELtBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::ELteBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EGtBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EGteBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EEqBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::ENeqBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EAndBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EOrBody(x) => {
            out.extend(x.p1.iter());
            out.extend(x.p2.iter());
        }
        ExprInstance::EMatchesBody(x) => {
            out.extend(x.target.iter());
            out.extend(x.pattern.iter());
        }

        // ---- collections ----
        ExprInstance::EListBody(x) => out.extend(x.ps.iter()),
        ExprInstance::ETupleBody(x) => out.extend(x.ps.iter()),
        ExprInstance::ESetBody(x) => out.extend(x.ps.iter()),
        ExprInstance::EMapBody(x) => {
            for kv in &x.kvs {
                out.extend(kv.key.iter());
                out.extend(kv.value.iter());
            }
        }
        ExprInstance::EPathmapBody(x) => out.extend(x.ps.iter()),
        ExprInstance::EZipperBody(x) => {
            for pm in x.pathmap.iter() {
                out.extend(pm.ps.iter());
            }
        }

        // ---- method ----
        ExprInstance::EMethodBody(x) => {
            out.extend(x.target.iter());
            out.extend(x.arguments.iter());
        }
    }
}

/// Append every child `Par` structurally contained in `c`, in traversal order.
pub fn connective_instance_child_pars<'a>(c: &'a ConnectiveInstance, out: &mut Vec<&'a Par>) {
    match c {
        ConnectiveInstance::ConnAndBody(b) => out.extend(b.ps.iter()),
        ConnectiveInstance::ConnOrBody(b) => out.extend(b.ps.iter()),
        ConnectiveInstance::ConnNotBody(p) => out.push(p),
        // A `VarRef` is a (index, depth) pair; the connective grounds carry a
        // single `bool` each.
        ConnectiveInstance::VarRefBody(_)
        | ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_) => {}
    }
}

/// Append every child `Par` of `p`, in traversal order.
///
/// Field order matches `SubstituteTrait<Par>::substitute_no_sort`: exprs,
/// connectives, sends, bundles, receives, news, matches, conditionals.
/// `unforgeables` carries no `Par` (`GUnforgeable`'s four variants are all
/// byte arrays), which is why it is absent rather than forgotten.
pub fn par_child_pars<'a>(p: &'a Par, out: &mut Vec<&'a Par>) {
    for e in &p.exprs {
        if let Some(instance) = &e.expr_instance {
            expr_instance_child_pars(instance, out);
        }
    }
    for c in &p.connectives {
        if let Some(instance) = &c.connective_instance {
            connective_instance_child_pars(instance, out);
        }
    }
    for s in &p.sends {
        out.extend(s.chan.iter());
        out.extend(s.data.iter());
    }
    for b in &p.bundles {
        out.extend(b.body.iter());
    }
    for r in &p.receives {
        for bind in &r.binds {
            out.extend(bind.source.iter());
            out.extend(bind.patterns.iter());
        }
        out.extend(r.body.iter());
        out.extend(r.condition.iter());
    }
    for n in &p.news {
        out.extend(n.p.iter());
        out.extend(n.injections.values());
    }
    for m in &p.matches {
        out.extend(m.target.iter());
        for case in &m.cases {
            out.extend(case.pattern.iter());
            out.extend(case.source.iter());
            out.extend(case.guard.iter());
        }
    }
    for i in &p.conditionals {
        out.extend(i.condition.iter());
        out.extend(i.if_true.iter());
        out.extend(i.if_false.iter());
    }
}

/// Every `Par` reachable from `root`, `root` included, collected **without
/// recursion**. A recursive collector would itself be Θ(depth) and would
/// therefore overflow before the traversal it is meant to inspect.
pub fn reachable_pars(root: &Par) -> Vec<&Par> {
    let mut seen: Vec<&Par> = Vec::new();
    let mut work: Vec<&Par> = vec![root];
    let mut buf: Vec<&Par> = Vec::new();
    while let Some(p) = work.pop() {
        seen.push(p);
        buf.clear();
        par_child_pars(p, &mut buf);
        work.extend(buf.iter().copied());
    }
    seen
}

/// Every `Expr` and every `Connective` reachable from `root`, by reference,
/// collected without recursion.
pub fn reachable_exprs_and_connectives(root: &Par) -> (Vec<&Expr>, Vec<&Connective>) {
    let pars = reachable_pars(root);
    let mut exprs: Vec<&Expr> = Vec::new();
    let mut conns: Vec<&Connective> = Vec::new();
    for p in pars {
        exprs.extend(p.exprs.iter());
        conns.extend(p.connectives.iter());
    }
    (exprs, conns)
}

// ===========================================================================
// STRUCTURAL children, by move — the generalized iterative teardown
// ===========================================================================

/// Move every child `Par` out of `e`, leaving a child-free shell that drops in
/// `O(1)`. The by-move twin of [`expr_instance_child_pars`]; the two matches
/// must stay in step, which the test `move_and_borrow_tables_agree` enforces.
fn take_expr_instance_child_pars(e: ExprInstance, out: &mut Vec<Par>) {
    match e {
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => {}

        ExprInstance::EVarBody(_) => {}

        ExprInstance::ENotBody(x) => out.extend(x.p),
        ExprInstance::ENegBody(x) => out.extend(x.p),

        ExprInstance::EMultBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EDivBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EModBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EPlusBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EMinusBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EPlusPlusBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EMinusMinusBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EPercentPercentBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::ELtBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::ELteBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EGtBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EGteBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EEqBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::ENeqBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EAndBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EOrBody(x) => {
            out.extend(x.p1);
            out.extend(x.p2);
        }
        ExprInstance::EMatchesBody(x) => {
            out.extend(x.target);
            out.extend(x.pattern);
        }

        ExprInstance::EListBody(x) => out.extend(x.ps),
        ExprInstance::ETupleBody(x) => out.extend(x.ps),
        ExprInstance::ESetBody(x) => out.extend(x.ps),
        ExprInstance::EMapBody(x) => {
            for kv in x.kvs {
                out.extend(kv.key);
                out.extend(kv.value);
            }
        }
        ExprInstance::EPathmapBody(x) => out.extend(x.ps.into_vec()),
        ExprInstance::EZipperBody(x) => {
            if let Some(pm) = x.pathmap {
                out.extend(pm.ps.into_vec());
            }
        }

        ExprInstance::EMethodBody(x) => {
            out.extend(x.target);
            out.extend(x.arguments);
        }
    }
}

/// By-move twin of [`connective_instance_child_pars`].
fn take_connective_instance_child_pars(c: ConnectiveInstance, out: &mut Vec<Par>) {
    match c {
        ConnectiveInstance::ConnAndBody(b) => out.extend(b.ps),
        ConnectiveInstance::ConnOrBody(b) => out.extend(b.ps),
        ConnectiveInstance::ConnNotBody(p) => out.push(p),
        ConnectiveInstance::VarRefBody(_)
        | ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_) => {}
    }
}

/// By-move twin of [`par_child_pars`]: strips `p` of every child `Par`.
fn take_par_child_pars(p: &mut Par, out: &mut Vec<Par>) {
    for e in std::mem::take(&mut p.exprs) {
        if let Some(instance) = e.expr_instance {
            take_expr_instance_child_pars(instance, out);
        }
    }
    for c in std::mem::take(&mut p.connectives) {
        if let Some(instance) = c.connective_instance {
            take_connective_instance_child_pars(instance, out);
        }
    }
    for mut s in std::mem::take(&mut p.sends) {
        out.extend(s.chan.take());
        out.append(&mut s.data);
    }
    for mut b in std::mem::take(&mut p.bundles) {
        out.extend(b.body.take());
    }
    for mut r in std::mem::take(&mut p.receives) {
        for mut bind in std::mem::take(&mut r.binds) {
            out.extend(bind.source.take());
            out.append(&mut bind.patterns);
        }
        out.extend(r.body.take());
        out.extend(r.condition.take());
    }
    for mut n in std::mem::take(&mut p.news) {
        out.extend(n.p.take());
        out.extend(std::mem::take(&mut n.injections).into_values());
    }
    for mut m in std::mem::take(&mut p.matches) {
        out.extend(m.target.take());
        for mut case in std::mem::take(&mut m.cases) {
            out.extend(case.pattern.take());
            out.extend(case.source.take());
            out.extend(case.guard.take());
        }
    }
    for mut i in std::mem::take(&mut p.conditionals) {
        out.extend(i.condition.take());
        out.extend(i.if_true.take());
        out.extend(i.if_false.take());
    }
}

/// Tear `root` down **iteratively**.
///
/// `Drop` for this family is itself a Θ(depth) recursive traversal (measured
/// 470 B/level debug — see the audit's §5, row 10). A harness that let a deep
/// term drop normally would therefore measure `Drop` rather than the traversal
/// under test, and a driver that produced a deep term on a small stack could
/// still abort while releasing it.
///
/// This walks the term with an explicit worklist, detaching each node's
/// children before the (now child-free) shell falls out of scope, so native
/// stack is `O(1)` in both nesting depth and sibling width.
pub fn dismantle(root: Par) {
    let mut work: Vec<Par> = vec![root];
    while let Some(mut p) = work.pop() {
        take_par_child_pars(&mut p, &mut work);
        // `p` now owns no `Par`, so dropping it here is O(1).
    }
}

/// [`dismantle`] for anything that can hand back its `Par`s — used by callers
/// holding a `Vec<Par>`, a `BTreeMap<String, Par>` or an `Option<Par>`.
pub fn dismantle_all<I: IntoIterator<Item = Par>>(pars: I) {
    let mut work: Vec<Par> = pars.into_iter().collect();
    while let Some(mut p) = work.pop() {
        take_par_child_pars(&mut p, &mut work);
    }
}

/// [`dismantle_all`] over an injection map.
pub fn dismantle_injections(injections: BTreeMap<String, Par>) {
    dismantle_all(injections.into_values());
}

// ===========================================================================
// PER-TRAVERSAL DISPOSITION
// ===========================================================================

/// Does `Substitute::substitute_no_sort` descend into this `ExprInstance`?
///
/// ⚠ **This is a record of current behaviour, not of desirable behaviour.**
/// `EPathmapBody` and `EZipperBody` both carry `Par` payloads and both fall to
/// the catch-all arm of `SubstituteTrait<Expr>::substitute_no_sort`
/// (`substitute.rs`, the `other => Ok(Expr { expr_instance: Some(other) })`
/// arm), so a `Par` inside a path map is returned **unsubstituted**. Any
/// worklist conversion must reproduce that verbatim: descending into them
/// would change the substituted term, hence its protobuf bytes, hence the
/// signature computed over them — a consensus fork dressed up as a bug fix.
///
/// Changing it is a separate, deliberate, protocol-visible decision.
pub fn substitute_descends_into(e: &ExprInstance) -> bool {
    match e {
        // grounds and vars: nothing to descend into in the first place
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::EVarBody(_) => false,

        // ⚠ NO DESCENT, and they DO have children — see the note above.
        ExprInstance::EPathmapBody(_) | ExprInstance::EZipperBody(_) => false,

        ExprInstance::ENotBody(_)
        | ExprInstance::ENegBody(_)
        | ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EMinusMinusBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_)
        | ExprInstance::EEqBody(_)
        | ExprInstance::ENeqBody(_)
        | ExprInstance::EAndBody(_)
        | ExprInstance::EOrBody(_)
        | ExprInstance::EMatchesBody(_)
        | ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        | ExprInstance::EMethodBody(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::{
        Bundle, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap, EMatches,
        EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPercentPercent, EPlus,
        EPlusPlus, ESet, ETuple, EVar, EZipper, GBigRational, GFixedPoint, If, KeyValuePair, Match,
        MatchCase, New, Receive, ReceiveBind, Send, Var, VarRef,
    };
    use crate::rust::rhoapi_ext::EPathMap;

    /// A `Par` with a distinguishing `locally_free` tag, so a table that
    /// returned the WRONG child (rather than none) is still caught.
    fn tagged(tag: u8) -> Par {
        Par {
            locally_free: vec![tag],
            ..Default::default()
        }
    }

    fn pathmap_of(ps: Vec<Par>) -> EPathMap {
        // `EPathMap` has a private intern cell, so it is built through its
        // constructor rather than a struct literal.
        EPathMap::new(ps, Vec::new(), false, None)
    }

    /// One representative of every `ExprInstance` variant, each paired with the
    /// `locally_free` tags of the children the table must report, in order.
    fn expr_instance_corpus() -> Vec<(ExprInstance, Vec<u8>)> {
        let a = || Some(tagged(1));
        let b = || Some(tagged(2));
        vec![
            (ExprInstance::GBool(true), vec![]),
            (ExprInstance::GInt(1), vec![]),
            (ExprInstance::GString("s".into()), vec![]),
            (ExprInstance::GUri("rho:id:x".into()), vec![]),
            (ExprInstance::GByteArray(vec![7]), vec![]),
            (ExprInstance::GDouble(1.0f64.to_bits()), vec![]),
            (ExprInstance::GBigInt(vec![1]), vec![]),
            (
                ExprInstance::GBigRat(GBigRational {
                    numerator: vec![1],
                    denominator: vec![2],
                }),
                vec![],
            ),
            (
                ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled: vec![1],
                    scale: 2,
                }),
                vec![],
            ),
            (
                ExprInstance::EVarBody(EVar {
                    v: Some(Var::default()),
                }),
                vec![],
            ),
            (ExprInstance::ENotBody(ENot { p: a() }), vec![1]),
            (ExprInstance::ENegBody(ENeg { p: a() }), vec![1]),
            (
                ExprInstance::EMultBody(EMult { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EModBody(EMod { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EPlusPlusBody(EPlusPlus { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EMinusMinusBody(EMinusMinus { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EPercentPercentBody(EPercentPercent { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (ExprInstance::ELtBody(ELt { p1: a(), p2: b() }), vec![1, 2]),
            (
                ExprInstance::ELteBody(ELte { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (ExprInstance::EGtBody(EGt { p1: a(), p2: b() }), vec![1, 2]),
            (
                ExprInstance::EGteBody(EGte { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (ExprInstance::EEqBody(EEq { p1: a(), p2: b() }), vec![1, 2]),
            (
                ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (
                ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }),
                vec![1, 2],
            ),
            (ExprInstance::EOrBody(EOr { p1: a(), p2: b() }), vec![1, 2]),
            (
                ExprInstance::EMatchesBody(EMatches {
                    target: a(),
                    pattern: b(),
                }),
                vec![1, 2],
            ),
            (
                ExprInstance::EListBody(EList {
                    ps: vec![tagged(1), tagged(2)],
                    ..Default::default()
                }),
                vec![1, 2],
            ),
            (
                ExprInstance::ETupleBody(ETuple {
                    ps: vec![tagged(1), tagged(2)],
                    ..Default::default()
                }),
                vec![1, 2],
            ),
            (
                ExprInstance::ESetBody(ESet {
                    ps: vec![tagged(1), tagged(2)],
                    ..Default::default()
                }),
                vec![1, 2],
            ),
            (
                ExprInstance::EMapBody(EMap {
                    kvs: vec![KeyValuePair {
                        key: a(),
                        value: b(),
                    }],
                    ..Default::default()
                }),
                vec![1, 2],
            ),
            (
                ExprInstance::EPathmapBody(pathmap_of(vec![tagged(1), tagged(2)])),
                vec![1, 2],
            ),
            (
                ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(pathmap_of(vec![tagged(1)])),
                    ..Default::default()
                }),
                vec![1],
            ),
            (
                ExprInstance::EMethodBody(EMethod {
                    target: a(),
                    arguments: vec![tagged(2)],
                    ..Default::default()
                }),
                vec![1, 2],
            ),
        ]
    }

    fn connective_instance_corpus() -> Vec<(ConnectiveInstance, Vec<u8>)> {
        vec![
            (
                ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![tagged(1), tagged(2)],
                }),
                vec![1, 2],
            ),
            (
                ConnectiveInstance::ConnOrBody(ConnectiveBody {
                    ps: vec![tagged(3)],
                }),
                vec![3],
            ),
            (ConnectiveInstance::ConnNotBody(tagged(4)), vec![4]),
            (
                ConnectiveInstance::VarRefBody(VarRef {
                    index: 0,
                    depth: 0,
                }),
                vec![],
            ),
            (ConnectiveInstance::ConnBool(true), vec![]),
            (ConnectiveInstance::ConnInt(true), vec![]),
            (ConnectiveInstance::ConnString(true), vec![]),
            (ConnectiveInstance::ConnUri(true), vec![]),
            (ConnectiveInstance::ConnByteArray(true), vec![]),
        ]
    }

    fn tags(pars: &[&Par]) -> Vec<u8> {
        pars.iter()
            .map(|p| *p.locally_free.first().unwrap_or(&0))
            .collect()
    }

    #[test]
    fn corpus_covers_every_expr_instance_variant() {
        assert_eq!(
            expr_instance_corpus().len(),
            EXPR_INSTANCE_VARIANT_COUNT,
            "the ExprInstance corpus no longer covers every variant in RhoTypes.proto — \
             a variant was added to the schema and to the (exhaustive) matches in this \
             module, but not to the corpus that checks their slot lists"
        );
    }

    #[test]
    fn corpus_covers_every_connective_instance_variant() {
        assert_eq!(
            connective_instance_corpus().len(),
            CONNECTIVE_INSTANCE_VARIANT_COUNT,
            "the ConnectiveInstance corpus no longer covers every variant"
        );
    }

    #[test]
    fn every_expr_instance_reports_the_expected_child_slots() {
        for (instance, expected) in expr_instance_corpus() {
            let mut out: Vec<&Par> = Vec::new();
            expr_instance_child_pars(&instance, &mut out);
            assert_eq!(
                tags(&out),
                expected,
                "child-slot mismatch for {:?}",
                std::mem::discriminant(&instance)
            );
        }
    }

    #[test]
    fn every_connective_instance_reports_the_expected_child_slots() {
        for (instance, expected) in connective_instance_corpus() {
            let mut out: Vec<&Par> = Vec::new();
            connective_instance_child_pars(&instance, &mut out);
            assert_eq!(
                tags(&out),
                expected,
                "child-slot mismatch for {:?}",
                std::mem::discriminant(&instance)
            );
        }
    }

    /// The by-move table must detach exactly the children the by-reference
    /// table reports — same set, same order. Without this the two drift and
    /// `dismantle` silently leaks a subtree back onto the recursive `Drop`.
    #[test]
    fn move_and_borrow_tables_agree() {
        for (instance, _) in expr_instance_corpus() {
            let mut borrowed: Vec<&Par> = Vec::new();
            expr_instance_child_pars(&instance, &mut borrowed);
            let expected = tags(&borrowed);

            let mut moved: Vec<Par> = Vec::new();
            take_expr_instance_child_pars(instance, &mut moved);
            let actual: Vec<u8> = moved
                .iter()
                .map(|p| *p.locally_free.first().unwrap_or(&0))
                .collect();

            assert_eq!(
                actual, expected,
                "the by-move and by-reference ExprInstance tables disagree"
            );
        }

        for (instance, _) in connective_instance_corpus() {
            let mut borrowed: Vec<&Par> = Vec::new();
            connective_instance_child_pars(&instance, &mut borrowed);
            let expected = tags(&borrowed);

            let mut moved: Vec<Par> = Vec::new();
            take_connective_instance_child_pars(instance, &mut moved);
            let actual: Vec<u8> = moved
                .iter()
                .map(|p| *p.locally_free.first().unwrap_or(&0))
                .collect();

            assert_eq!(
                actual, expected,
                "the by-move and by-reference ConnectiveInstance tables disagree"
            );
        }
    }

    /// Every `Par`-bearing field of `Par` itself must be reported. Built as one
    /// term carrying a distinct tag per slot, so a forgotten field shows up as
    /// a missing tag rather than as nothing at all.
    #[test]
    fn par_child_table_reaches_every_par_bearing_field() {
        let p = Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(tagged(1)) })),
            }],
            connectives: vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnNotBody(tagged(2))),
            }],
            sends: vec![Send {
                chan: Some(tagged(3)),
                data: vec![tagged(4)],
                ..Default::default()
            }],
            bundles: vec![Bundle {
                body: Some(tagged(5)),
                ..Default::default()
            }],
            receives: vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![tagged(7)],
                    source: Some(tagged(6)),
                    remainder: None,
                    free_count: 0,
                }],
                body: Some(tagged(8)),
                condition: Some(tagged(9)),
                ..Default::default()
            }],
            news: vec![New {
                p: Some(tagged(10)),
                injections: {
                    let mut m = BTreeMap::new();
                    m.insert("k".to_string(), tagged(11));
                    m
                },
                ..Default::default()
            }],
            matches: vec![Match {
                target: Some(tagged(12)),
                cases: vec![MatchCase {
                    pattern: Some(tagged(13)),
                    source: Some(tagged(14)),
                    guard: Some(tagged(15)),
                    free_count: 0,
                }],
                ..Default::default()
            }],
            conditionals: vec![If {
                condition: Some(tagged(16)),
                if_true: Some(tagged(17)),
                if_false: Some(tagged(18)),
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut out: Vec<&Par> = Vec::new();
        par_child_pars(&p, &mut out);
        assert_eq!(tags(&out), (1u8..=18).collect::<Vec<u8>>());
    }

    /// `dismantle` must reach every node — otherwise the shells it drops still
    /// own a deep subtree and the recursive `Drop` runs after all.
    #[test]
    fn dismantle_detaches_the_whole_term() {
        // 512 levels: deeper than any Θ(depth) member survives on this test
        // thread's stack, so a `dismantle` that missed a slot would abort here.
        let mut p = Par::default();
        for _ in 0..512 {
            p = Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![p],
                        ..Default::default()
                    })),
                }],
                ..Default::default()
            };
        }
        dismantle(p);
    }

    #[test]
    fn substitute_disposition_excludes_exactly_the_two_pathmap_arms() {
        let not_descended: Vec<std::mem::Discriminant<ExprInstance>> = expr_instance_corpus()
            .iter()
            .filter(|(instance, expected_children)| {
                !expected_children.is_empty() && !substitute_descends_into(instance)
            })
            .map(|(instance, _)| std::mem::discriminant(instance))
            .collect();

        let expected = vec![
            std::mem::discriminant(&ExprInstance::EPathmapBody(pathmap_of(vec![]))),
            std::mem::discriminant(&ExprInstance::EZipperBody(EZipper::default())),
        ];

        assert_eq!(
            not_descended, expected,
            "the set of child-bearing ExprInstance variants that `Substitute` does NOT \
             descend into has changed. That set is consensus-visible (it decides which \
             sub-terms get substituted, hence the signed bytes). See \
             `substitute_descends_into`."
        );
    }
}
