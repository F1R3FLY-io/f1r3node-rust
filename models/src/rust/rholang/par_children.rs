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
//!    `EZipperBody` is deliberately **not** descended into by substitution;
//!    `EPathmapBody` uses its own incremental owned-trie cursor.
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
/// The number of variants `connective.ConnectiveInstance` has.
/// ★ GENERATED, never a literal. See [`EXPR_INSTANCE_VARIANT_COUNT`].
pub use crate::rust::rholang::bincode_schema_tables::CONNECTIVE_INSTANCE_VARIANT_COUNT;
/// The number of variants `expr.ExprInstance` has in `RhoTypes.proto`.
///
/// Bumping this without extending every corpus that asserts against it is the
/// mistake this constant exists to prevent.
/// ★ GENERATED, never a literal. Re-exported from the schema-codegen table
/// (`models/codegen/schema.rs`) so the count IS
/// `EXPR_INSTANCE_VARIANTS.len()`. It used to read `= 36`, which meant a 37th
/// arm would leave every assertion measured against it passing while the new
/// arm went untested — the exact failure mode generation exists to remove.
pub use crate::rust::rholang::bincode_schema_tables::EXPR_INSTANCE_VARIANT_COUNT;

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
        // Structural ownership, not logical set membership: `PathMap<()>`
        // owns byte keys and contributes no borrowed `Par`; `PathMap<Par>`
        // contributes only its associated values.
        ExprInstance::EPathmapBody(x) => x.entry_trie().extend_owned_par_refs(out),
        ExprInstance::EZipperBody(x) => {
            for pm in x.pathmap.iter() {
                pm.entry_trie().extend_owned_par_refs(out);
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
        // ★ BY MOVE, like every arm above. These two used to read
        // `out.extend(x.ps().iter().cloned())` — the only arms in this function that CLONED
        // while their siblings consumed — which meant teardown did the work twice and *still*
        // left the originals to the recursive destructor when `x` fell out of scope.
        //
        // ⚠ It was worse than one stray copy: `ps()` forces a MEMOISED `Vec<Par>` built by
        // cloning every entry, and a freshly built map's memo is cold — so the teardown path
        // materialised a full second copy in order to destroy the first.
        //
        // `drain_owned_pars` consumes the value and hands over both retainers (the memo and the
        // trie, plus the interned handle for `EPathMap`); see its doc for the uniqueness test
        // and why a shared trie's copy-on-write is never a regression.
        ExprInstance::EPathmapBody(x) => x.drain_owned_pars(out),
        ExprInstance::EZipperBody(x) => {
            if let Some(pm) = x.pathmap {
                pm.drain_owned_pars(out);
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

/// Detach and release every recursive child of an already borrowed [`Par`].
///
/// This is the implementation primitive for the generated `Drop for Par`.
/// `Drop::drop` cannot move `self`, so the root is emptied in place and all
/// detached children are processed on one explicit heap worklist. Each child
/// shell reaches its own generated `Drop` only after its recursive fields have
/// been taken, making that nested call constant-depth and allocation-free.
pub(crate) fn dismantle_in_place(root: &mut Par) {
    let mut work = Vec::new();
    take_par_child_pars(root, &mut work);
    while let Some(mut child) = work.pop() {
        take_par_child_pars(&mut child, &mut work);
    }
}

// ===========================================================================
// PER-TRAVERSAL DISPOSITION
// ===========================================================================

/// Does `Substitute::substitute_no_sort` descend into this `ExprInstance`?
///
/// `EPathmapBody` is a specialized descent: set keys and map key/value pairs
/// are moved from an owned PathMap cursor, substituted one entry at a time, and
/// reinserted directly into the same homogeneous specialization. `EZipperBody`
/// remains opaque runtime cursor state.
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

        // Runtime cursor state is compared as one value; it is not surface
        // syntax with substitutable children.
        ExprInstance::EZipperBody(_) => false,

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
        | ExprInstance::EMethodBody(_)
        | ExprInstance::EPathmapBody(_) => true,
    }
}

/// Does `SpatialMatcher<Expr, Expr>::spatial_match` descend into this
/// `ExprInstance`?
///
/// ## Why this has to be written down
///
/// `spatial_match` is a `match` over **pairs** of `ExprInstance` terminated by
/// `_ => None`. In that position a catch-all is not a default, it is a silent
/// decision: a pattern containing an unlisted variant matches nothing, so the
/// `COMM` never fires, the receive rests forever, the node exits `0`, and no
/// diagnostic is produced anywhere. Because `spatial_match` decides `COMM`
/// firing, that decision is **consensus visible** — two implementations that
/// disagree about it disagree about which blocks are valid.
///
/// This function turns the catch-all into an enumeration. Every variant is
/// listed, there is no `_` arm, and
/// `rholang/tests/spatial_matcher_disposition.rs` runs the matcher over one
/// probe per **generated** variant and asserts that the measured behaviour and
/// this declaration agree. A variant can therefore be *implemented* or
/// *consciously excluded*, but not forgotten.
///
/// ## The two exclusions, and the invariant that forces them
///
/// `EPathmapBody` and `EZipperBody` carry child `Par`s and are **not**
/// descended into. The reason is not that matching is dangerous in itself —
/// matching does not rewrite anything — but that the matcher and
/// [`substitute_descends_into`] run over *the same installed pattern*: a `for`
/// bind is substituted before it is stored in RSpace and matched after. Neither
/// of these two variants is descended into by `Substitute` (see that function),
/// so a `VarRef` or a shifted `BoundVar` sitting inside a path map is still in
/// its **pre-substitution** form when the matcher reaches it. Descending here
/// would bind out of bytes substitution never rewrote.
///
/// Hence the invariant, checked by
/// `spatial_match_never_descends_where_substitute_does_not`:
///
/// ```text
///     spatial_match_descends_into(e)  ⟹  substitute_descends_into(e)
/// ```
///
/// The matcher's frontier may not outrun substitution's. That makes the
/// exclusion set **derived** rather than chosen: it is exactly the set of
/// child-bearing variants substitution declines, and it will shrink the moment
/// substitution's does — which is a separate, deliberate, protocol-visible
/// decision, and F1r3node's to take.
///
/// ⚠ For `EPathmapBody` the exclusion is a **real gap**, not a formality. A
/// path-map literal is ordinary surface syntax (`{| a, b, ...rest |}`, see the
/// `pathmap` rule in the Rholang grammar) and the collection normalizer sets
/// its `connective_used` from its elements and its remainder exactly as it does
/// for a set, so a pattern like `for (@{| x, ...rest |} <- ch)` normalizes
/// fine and then matches nothing. Closing it needs two things this function
/// cannot supply on its own: substitution descent (above), and a decision about
/// what a path map's *entry multiset* means under matching — whether
/// `{| 1, 1 |}` and `{| 1 |}` are the same pattern, and in what canonical order
/// a bound `...rest` is reassembled. That order is the ground-map canonical
/// form whose bytes are the event-hash preimage, so it is a consensus decision,
/// not an implementation detail.
///
/// `EZipperBody` is excluded on the same invariant, and is additionally
/// unreachable as a pattern: no normalizer path constructs one. Every
/// `EZipper` in the tree is produced at *runtime* by `readZipper`,
/// `readZipperAt` and `writeZipper` (`reduce.rs`), by the decoder, or by the
/// sorter. Its identity also includes `current_path: Vec<Vec<u8>>`,
/// `cursor_kind` and `is_write_zipper` — encoded cursor state with no `Par`
/// beneath it, for which "descend" has no meaning. Equality is its complete
/// treatment, and `list_match::match_function` already applies equality to
/// every pattern that does not report `connective_used`.
///
/// ## What `false` means for the childless arms
///
/// The grounds and `EVarBody` also answer `false`, for the same reason they do
/// in [`substitute_descends_into`]: there is no child `Par` to descend into.
/// `spatial_match` does have an `EVarBody` arm — `guard(vp == vt)` — but that is
/// a comparison of the `Var` payload, not a descent, so the disposition tests
/// quantify only over child-bearing variants.
pub fn spatial_match_descends_into(e: &ExprInstance) -> bool {
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

        // ⚠ NO DESCENT, and they DO have children — see the note above. This is
        // forced by `substitute_descends_into`, which declines them too.
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
        // ⚠ `EMatchesBody` descends into its `target` slot ONLY. Its `pattern`
        // slot is a nested pattern at a different binding depth — which is why
        // `has_locally_free` reads `connective_used` from the target alone —
        // and is compared by equality, exactly as `ReceiveBind::patterns` and
        // `MatchCase::pattern` are. "Descends" is a per-variant verdict, as it
        // is for `Substitute`; the per-slot treatment lives in the arm.
        | ExprInstance::EMatchesBody(_)
        | ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        // `EMethodBody` descends into its receiver and its arguments; the
        // method NAME is compared by equality.
        | ExprInstance::EMethodBody(_) => true,
    }
}

// ===========================================================================
// SERDE VARIANT INDICES — the wire numbering of the two big oneofs
// ===========================================================================

/// The **serde variant index** of an `ExprInstance` arm.
///
/// `serde_derive` numbers enum variants by **declaration order**, and bincode
/// writes that number as a `u32` (fixint, little-endian) immediately after the
/// enclosing `Option`'s 1-byte tag. It is therefore a wire-visible, consensus-
/// visible quantity, and any decoder that reconstructs an `ExprInstance` from a
/// stream has to agree with it exactly.
///
/// ⚠ This is **not** proto tag order. `RhoTypes.proto` assigns
/// `EPathmapBody = 32` and `EMatchesBody = 27`, but serde numbers them 25 and
/// 27 respectively, because prost emits the oneof arms in `.proto` declaration
/// order and serde counts from zero. Reading the proto tags as serde indices
/// would silently mis-decode 12 of the 36 arms.
///
/// The match is exhaustive with no `_` arm — guard 1 of this module — so a
/// variant added to `RhoTypes.proto` fails to compile here rather than being
/// silently assigned an index by omission. The decoder in
/// `models/src/rust/rholang/bincode_decoder.rs` reconstructs arms from indices and is
/// gated against THIS function by `bincode_decoder_variant_indices_agree`, so the
/// numbering exists in exactly one place.
pub fn expr_instance_variant_index(e: &ExprInstance) -> u32 {
    match e {
        ExprInstance::GBool(_) => 0,
        ExprInstance::GInt(_) => 1,
        ExprInstance::GString(_) => 2,
        ExprInstance::GUri(_) => 3,
        ExprInstance::GByteArray(_) => 4,
        ExprInstance::ENotBody(_) => 5,
        ExprInstance::ENegBody(_) => 6,
        ExprInstance::EMultBody(_) => 7,
        ExprInstance::EDivBody(_) => 8,
        ExprInstance::EPlusBody(_) => 9,
        ExprInstance::EMinusBody(_) => 10,
        ExprInstance::ELtBody(_) => 11,
        ExprInstance::ELteBody(_) => 12,
        ExprInstance::EGtBody(_) => 13,
        ExprInstance::EGteBody(_) => 14,
        ExprInstance::EEqBody(_) => 15,
        ExprInstance::ENeqBody(_) => 16,
        ExprInstance::EAndBody(_) => 17,
        ExprInstance::EOrBody(_) => 18,
        ExprInstance::EVarBody(_) => 19,
        ExprInstance::EListBody(_) => 20,
        ExprInstance::ETupleBody(_) => 21,
        ExprInstance::ESetBody(_) => 22,
        ExprInstance::EMapBody(_) => 23,
        ExprInstance::EMethodBody(_) => 24,
        ExprInstance::EPathmapBody(_) => 25,
        ExprInstance::EZipperBody(_) => 26,
        ExprInstance::EMatchesBody(_) => 27,
        ExprInstance::EPercentPercentBody(_) => 28,
        ExprInstance::EPlusPlusBody(_) => 29,
        ExprInstance::EMinusMinusBody(_) => 30,
        ExprInstance::EModBody(_) => 31,
        ExprInstance::GDouble(_) => 32,
        ExprInstance::GBigInt(_) => 33,
        ExprInstance::GBigRat(_) => 34,
        ExprInstance::GFixedPoint(_) => 35,
    }
}

/// The **serde variant index** of a `ConnectiveInstance` arm. See
/// [`expr_instance_variant_index`] for why this numbering is wire-visible and
/// why it is not the proto tag. Exhaustive, no `_` arm.
pub fn connective_instance_variant_index(c: &ConnectiveInstance) -> u32 {
    match c {
        ConnectiveInstance::ConnAndBody(_) => 0,
        ConnectiveInstance::ConnOrBody(_) => 1,
        ConnectiveInstance::ConnNotBody(_) => 2,
        ConnectiveInstance::VarRefBody(_) => 3,
        ConnectiveInstance::ConnBool(_) => 4,
        ConnectiveInstance::ConnInt(_) => 5,
        ConnectiveInstance::ConnString(_) => 6,
        ConnectiveInstance::ConnUri(_) => 7,
        ConnectiveInstance::ConnByteArray(_) => 8,
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
        par_from_default! {
            locally_free: vec![tag],
            ..Default::default()
        }
    }

    fn pathmap_of(ps: Vec<Par>) -> EPathMap {
        // `EPathMap` has a private intern cell, so it is built through its
        // constructor rather than a struct literal.
        EPathMap::new(ps, Vec::new(), false, None)
    }

    fn pathmap_map_of(values: Vec<Par>) -> EPathMap {
        EPathMap::new_map(
            values.into_iter().enumerate().map(|(index, value)| {
                (
                    par_from_default! {
                        exprs: vec![Expr {
                            expr_instance: Some(ExprInstance::GInt(index as i64)),
                        }],
                        ..Default::default()
                    },
                    value,
                )
            }),
            Vec::new(),
            false,
            None,
        )
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
            (ExprInstance::EMultBody(EMult { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EDivBody(EDiv { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EModBody(EMod { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EPlusBody(EPlus { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EMinusBody(EMinus { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
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
            (ExprInstance::ELteBody(ELte { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EGtBody(EGt { p1: a(), p2: b() }), vec![1, 2]),
            (ExprInstance::EGteBody(EGte { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EEqBody(EEq { p1: a(), p2: b() }), vec![1, 2]),
            (ExprInstance::ENeqBody(ENeq { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
            (ExprInstance::EAndBody(EAnd { p1: a(), p2: b() }), vec![
                1, 2,
            ]),
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
                ExprInstance::EPathmapBody(pathmap_map_of(vec![tagged(1), tagged(2)])),
                vec![1, 2],
            ),
            (
                ExprInstance::EZipperBody(EZipper {
                    pathmap: Some(pathmap_map_of(vec![tagged(1)])),
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
                ConnectiveInstance::VarRefBody(VarRef { index: 0, depth: 0 }),
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
    fn set_mode_pathmap_keys_are_bytes_not_owned_par_children() {
        for instance in [
            ExprInstance::EPathmapBody(pathmap_of(vec![tagged(1), tagged(2)])),
            ExprInstance::EZipperBody(EZipper {
                pathmap: Some(pathmap_of(vec![tagged(1)])),
                ..Default::default()
            }),
        ] {
            let mut out = Vec::new();
            expr_instance_child_pars(&instance, &mut out);
            assert!(out.is_empty());
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
    /// How many live owners the children of this `ExprInstance` have.
    ///
    /// One for every arm except the two that hold an `EPathMap`: a map's entries live in the
    /// trie, in the projection memo once forced, and in the interned handle once interned.
    fn expr_instance_retainers(e: &ExprInstance) -> usize {
        match e {
            ExprInstance::EPathmapBody(m) => m.live_retainer_count(),
            ExprInstance::EZipperBody(z) => {
                z.pathmap.as_ref().map_or(1, |m| m.live_retainer_count())
            }
            _ => 1,
        }
    }

    /// ★ **by-move ⊇ by-reference, with a COUNTED allowance** (owner ruling, 2026-07-31).
    ///
    /// The two tables answer different questions, and after the pathmap arms became by-move the
    /// difference became observable:
    ///
    /// * **by-reference** reports the **logical children** — what a matcher or a collector wants.
    /// * **by-move** must release every **owned `Par`**. A set-mode `EPathMap`'s
    ///   `PathMap<()>` owns byte keys rather than `Par`s, while its legacy
    ///   projection owns decoded entries once forced; a map-mode
    ///   `PathMap<Par>` owns its values directly.
    ///
    /// ⚠ So strict equality is the WRONG assertion here: it would force the by-move table to
    /// leak whichever retainers it declined to drain. What must hold instead is **containment
    /// with the slack PINNED**, not open — `moved` is `borrowed` repeated once per live
    /// retainer, and the multiplier is asserted to be uniform rather than merely "≥".
    ///
    /// ⇒ A retainer added without being drained changes the multiplier and fails here. A
    /// retainer drained twice does too. Open-ended `⊇` would catch neither.
    fn assert_containment_with_counted_allowance(
        actual: &[u8],
        expected: &[u8],
        retainers: usize,
        table: &str,
    ) {
        if expected.is_empty() {
            assert!(
                actual.is_empty(),
                "{table}: the by-reference table reports NO children but the by-move table \
                 yielded {} — a by-move arm is releasing `Par`s the borrow table does not know \
                 about, which means the two tables disagree about what this variant CONTAINS.",
                actual.len()
            );
            return;
        }

        // ⚠⚠ `retainers` is PINNED FROM THE VALUE, never inferred from `actual.len() /
        // expected.len()`. That inference is what makes a counted allowance vacuous: drop a
        // retainer's drain and the ratio just becomes a smaller whole number, still "valid".
        // MEASURED — with the multiplier derived, removing the memo drain left this GREEN.
        assert_eq!(
            actual.len(),
            expected.len() * retainers,
            "{table}: the by-move table yielded {} `Par`s; the by-reference table reports {} \
             children and the value declares {retainers} live retainer(s), so {} were \
             expected.\n\n\
             by-move must release EVERY owned `Par` — only value-carrying trie slots and a forced \
             projection count. A shortfall is a retainer left to the \
             RECURSIVE destructor; a surplus is one drained twice.\n\
             moved = {actual:?}\n  borrowed = {expected:?}",
            actual.len(),
            expected.len(),
            expected.len() * retainers
        );
        let mut want: Vec<u8> = Vec::with_capacity(actual.len());
        for _ in 0..retainers {
            want.extend_from_slice(expected);
        }
        let (mut got_sorted, mut want_sorted) = (actual.to_vec(), want);
        got_sorted.sort_unstable();
        want_sorted.sort_unstable();

        assert_eq!(
            got_sorted, want_sorted,
            "{table}: the by-move table is not the by-reference table repeated {retainers}×.\n\n\
             Same COUNT but different CONTENT means a by-move arm released a different set of \
             `Par`s than the borrow table reports — not a retainer-count difference but a \
             genuine disagreement about which children the variant has.\n\
             moved = {actual:?}\n  borrowed = {expected:?}"
        );
    }

    #[test]
    fn move_and_borrow_tables_agree() {
        for (instance, _) in expr_instance_corpus() {
            let mut borrowed: Vec<&Par> = Vec::new();
            expr_instance_child_pars(&instance, &mut borrowed);
            let expected = tags(&borrowed);

            // ⚠ Computed AFTER the borrow pass, because that pass calls `ps()` and therefore
            // FORCES the projection memo — which is itself one of the retainers being counted.
            // Computing it first would under-count by exactly the memo.
            let retainers = expr_instance_retainers(&instance);

            let mut moved: Vec<Par> = Vec::new();
            take_expr_instance_child_pars(instance, &mut moved);
            let actual: Vec<u8> = moved
                .iter()
                .map(|p| *p.locally_free.first().unwrap_or(&0))
                .collect();

            assert_containment_with_counted_allowance(
                &actual,
                &expected,
                retainers,
                "ExprInstance",
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

            // No `ConnectiveInstance` arm holds an `EPathMap`, so every child has exactly one
            // owner. If that ever stops being true this call site must learn the same trick the
            // `ExprInstance` one uses.
            assert_containment_with_counted_allowance(&actual, &expected, 1, "ConnectiveInstance");
        }
    }

    /// Every `Par`-bearing field of `Par` itself must be reported. Built as one
    /// term carrying a distinct tag per slot, so a forgotten field shows up as
    /// a missing tag rather than as nothing at all.
    #[test]
    fn par_child_table_reaches_every_par_bearing_field() {
        let p = par_from_default! {
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
            p = par_from_default! {
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

    /// The variant-index tables must be BIJECTIONS onto `0..COUNT`. Without
    /// this, two arms could share an index (mis-decoding one as the other) or
    /// an index could be skipped (making a wire value undecodable) and every
    /// existing test would still pass.
    #[test]
    fn variant_index_tables_are_bijections_onto_their_ranges() {
        let mut expr_seen: Vec<u32> = expr_instance_corpus()
            .iter()
            .map(|(instance, _)| expr_instance_variant_index(instance))
            .collect();
        expr_seen.sort_unstable();
        assert_eq!(
            expr_seen,
            (0..EXPR_INSTANCE_VARIANT_COUNT as u32).collect::<Vec<u32>>(),
            "`expr_instance_variant_index` is not a bijection onto 0..{}. serde numbers \
             enum variants by DECLARATION ORDER and bincode writes that number to the \
             wire, so a collision or a gap here is a consensus-visible mis-decode.",
            EXPR_INSTANCE_VARIANT_COUNT
        );

        let mut conn_seen: Vec<u32> = connective_instance_corpus()
            .iter()
            .map(|(instance, _)| connective_instance_variant_index(instance))
            .collect();
        conn_seen.sort_unstable();
        assert_eq!(
            conn_seen,
            (0..CONNECTIVE_INSTANCE_VARIANT_COUNT as u32).collect::<Vec<u32>>(),
            "`connective_instance_variant_index` is not a bijection onto 0..{}",
            CONNECTIVE_INSTANCE_VARIANT_COUNT
        );
    }

    /// The variant index must equal the `u32` bincode actually writes. This is
    /// the check that stops the table above from being a plausible-looking
    /// hand-count: it reads the real bytes.
    ///
    /// Layout of a `Some(oneof)` in bincode legacy fixint-LE: one `Option` tag
    /// byte (`0x01`), then the variant index as 4 bytes little-endian.
    #[test]
    fn variant_index_tables_match_the_bytes_bincode_writes() {
        for (instance, _) in expr_instance_corpus() {
            let expected = expr_instance_variant_index(&instance);
            let bytes = bincode::serialize(&Expr {
                expr_instance: Some(instance),
            })
            .expect("serialize Expr");
            assert_eq!(bytes[0], 1, "Some(..) must be tag 1");
            let on_wire = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            assert_eq!(
                on_wire, expected,
                "expr_instance_variant_index disagrees with the bytes bincode writes"
            );
        }
        for (instance, _) in connective_instance_corpus() {
            let expected = connective_instance_variant_index(&instance);
            let bytes = bincode::serialize(&Connective {
                connective_instance: Some(instance),
            })
            .expect("serialize Connective");
            assert_eq!(bytes[0], 1, "Some(..) must be tag 1");
            let on_wire = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            assert_eq!(
                on_wire, expected,
                "connective_instance_variant_index disagrees with the bytes bincode writes"
            );
        }
    }

    /// The twin of `substitute_disposition_excludes_exactly_the_zipper_arm`
    /// for the matcher. It pins the DECLARATION; the behaviour it declares is
    /// measured against the running matcher by
    /// `rholang/tests/spatial_matcher_disposition.rs`, which lives in the
    /// `rholang` crate because that is where `spatial_match` lives.
    #[test]
    fn spatial_match_disposition_excludes_exactly_the_two_pathmap_arms() {
        let not_descended: Vec<std::mem::Discriminant<ExprInstance>> = expr_instance_corpus()
            .iter()
            .filter(|(instance, expected_children)| {
                !expected_children.is_empty() && !spatial_match_descends_into(instance)
            })
            .map(|(instance, _)| std::mem::discriminant(instance))
            .collect();

        let expected = vec![
            std::mem::discriminant(&ExprInstance::EPathmapBody(pathmap_of(vec![]))),
            std::mem::discriminant(&ExprInstance::EZipperBody(EZipper::default())),
        ];

        assert_eq!(
            not_descended, expected,
            "the set of child-bearing ExprInstance variants that `spatial_match` does NOT \
             descend into has changed. That set is consensus-visible: it decides which \
             patterns can fire a COMM, and a pattern the matcher declines is INERT — the \
             receive rests forever with no diagnostic. See `spatial_match_descends_into`."
        );
    }

    /// The matcher's descent frontier may not outrun substitution's.
    ///
    /// A `for` bind is substituted before it is stored in RSpace and matched
    /// after, over the same bytes. If `spatial_match` descended into a sub-term
    /// `Substitute` declines, it would bind out of a `VarRef` that was never
    /// resolved or a `BoundVar` that was never shifted. This is the invariant
    /// that makes the two exclusions above **derived** rather than chosen.
    #[test]
    fn spatial_match_never_descends_where_substitute_does_not() {
        let outrunning: Vec<std::mem::Discriminant<ExprInstance>> = expr_instance_corpus()
            .iter()
            .filter(|(instance, _)| {
                spatial_match_descends_into(instance) && !substitute_descends_into(instance)
            })
            .map(|(instance, _)| std::mem::discriminant(instance))
            .collect();

        assert!(
            outrunning.is_empty(),
            "`spatial_match_descends_into` claims descent into a variant \
             `substitute_descends_into` declines. The matcher would then bind out of a \
             sub-term substitution never rewrote. Fix substitution first, or withdraw the \
             claim. Offending discriminants: {:?}",
            outrunning
        );
    }

    #[test]
    fn substitute_disposition_excludes_exactly_the_zipper_arm() {
        let not_descended: Vec<std::mem::Discriminant<ExprInstance>> = expr_instance_corpus()
            .iter()
            .filter(|(instance, expected_children)| {
                !expected_children.is_empty() && !substitute_descends_into(instance)
            })
            .map(|(instance, _)| std::mem::discriminant(instance))
            .collect();

        let expected = vec![std::mem::discriminant(&ExprInstance::EZipperBody(
            EZipper::default(),
        ))];

        assert_eq!(
            not_descended, expected,
            "the set of child-bearing ExprInstance variants that `Substitute` does NOT \
             descend into has changed. That set is consensus-visible (it decides which \
             sub-terms get substituted, hence the signed bytes). See \
             `substitute_descends_into`."
        );
    }
}
