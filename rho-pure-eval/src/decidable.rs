//! Which guards this evaluator can decide — the STATIC half of [`crate::eval`].
//!
//! # Why this module exists
//!
//! [`crate::eval_with`] implements a *subset* of `Reduce::eval_expr`. Every
//! `ExprInstance` outside the subset returns
//! [`EvalError::UnsupportedExpression`][crate::EvalError::UnsupportedExpression].
//! A caller that maps every `Err` to a boolean verdict therefore reports
//! "the guard is false" for a guard it never evaluated — and "false" is
//! indistinguishable from a guard that was evaluated and refuted. That
//! conflation is silent by construction: no error is raised, no COMM fires,
//! and nothing in the program's output separates the two.
//!
//! The cure is to refuse such a guard **before** it is ever decided, at a
//! point where refusing is loud. Doing so requires a predicate — *"can this
//! guard be decided at all?"* — and that predicate must be derived from the
//! evaluator's own arms, or it drifts: gating `EMethodBody` alone and leaving
//! its five siblings is exactly how the class recurs.
//!
//! # The contract
//!
//! Let `g` be a Par, `spatial` the availability of a
//! [`SpatialMatch`][crate::SpatialMatch] oracle, and `E` any environment.
//! Write `U(g) = undecidable_nodes(g, spatial)`. Then:
//!
//! | # | property | what it buys |
//! | --- | --- | --- |
//! | **Soundness** | `eval_with(g, E, o) = Err(UnsupportedExpression{k})` `⟹` `k ∈ kinds(U(g))` | no undecidable guard slips past the gate — **no silence** |
//! | **Precision** | `U(g) ≠ ∅` `⟹` `eval_with(g, E, o)` is `Err` for every `E` | the gate never refuses a guard that would have been decided — **no false alarm** |
//!
//! Both are pinned by [`crate::tests`]'s differential over a corpus that
//! carries one representative of every `ExprInstance` variant.
//!
//! ## Why Precision needs no environment quantifier in practice
//!
//! [`crate::eval_with`] is **strict**: `push_binary` pushes both operands of
//! every binary operator before its continuation, and `EAnd`/`EOr` are no
//! exception (there is no short-circuit). So a node in an evaluated position
//! is reached unless an *earlier* error fires first — and an earlier error is
//! still an `Err`. Precision therefore holds for every environment, not merely
//! for the ones that reach the node.
//!
//! # ★ The walk visits exactly the positions the evaluator evaluates
//!
//! Over-approximating is a defect in its own right — it refuses guards that
//! work today — so the walk mirrors `eval_drive` position for position:
//!
//! | position | evaluated by `eval_drive`? | walked here? |
//! | --- | --- | --- |
//! | each endpoint in the `Par` Expr family | yes (`EvKont::ParK`) | yes |
//! | `par.{sends,receives,news,matches,bundles,unforgeables,connectives,conditionals}` | no — carried through inert | no |
//! | `ENot.p`, `ENeg.p`, and every binary operator's `p1`/`p2` | yes | yes |
//! | `EMatches.target` | yes | yes |
//! | `EMatches.pattern` | **no** — its free variables are binders | **no** |
//! | `EList.ps`, `ETuple.ps`, `ESet`, `EMap` interiors | **no** — collections pass through unchanged | **no** |
//! | `EMethod.{target,arguments}` | unreachable — the arm refuses first | no (the `EMethod` itself is recorded) |
//! | the value an `EVar` resolves to | yes, but only at run time | ★ see below |
//!
//! ## The one position a static walk cannot see
//!
//! `ExprInstance::EVarBody` resolves against the environment and then
//! evaluates the *resolved value*. That value is not present in `g`, so no
//! static walk can classify it. It is not a hole in practice: a guard's
//! environment is built from data that the reducer already evaluated before
//! storing (`Reduce::eval_send` calls `eval_expr` on every datum), so a datum
//! carrying an unevaluated `EMethodBody` in its top-level Expr family cannot be
//! produced by a reduction. Callers that want the residue closed anyway have
//! the run-time disposition for it — the gate is the first line, not the only
//! one.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMatches, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr,
    Par,
};
use models::rust::par_bag::ParComponentItem;

/// Whether the caller can supply a [`SpatialMatch`][crate::SpatialMatch]
/// oracle, which is what decides `EMatchesBody`.
///
/// `EMatches` is the one node kind whose decidability is a property of the
/// *caller* rather than of this crate: [`crate::eval`] refuses it and
/// [`crate::eval_with`] decides it whenever the injected oracle reports
/// [`SpatialMatch::is_available`][crate::SpatialMatch::is_available]. A gate
/// that hard-coded either answer would be wrong at one of the two call sites,
/// so the caller states which world it is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialSupport {
    /// A real oracle will be injected — `EMatchesBody` is decidable.
    /// Both of `rholang`'s guard sites are in this world.
    Available,
    /// [`crate::eval`]'s world: `EMatchesBody` is refused.
    Absent,
}

/// One node of a guard that the evaluator cannot decide.
///
/// `kind` is the exact `&'static str` the evaluator would have put in
/// [`EvalError::UnsupportedExpression`][crate::EvalError::UnsupportedExpression],
/// so a diagnostic built from this struct and one built from a caught
/// evaluation error name the same thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndecidableNode {
    /// The `ExprInstance` variant name, e.g. `"EMethodBody"`.
    pub kind: &'static str,
    /// A human-readable name for what the user actually wrote, e.g.
    /// `` `nth` `` for a method call. Present only where the node carries a
    /// name worth quoting back.
    pub detail: Option<String>,
}

impl std::fmt::Display for UndecidableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(d) => write!(f, "{} ({})", describe(self.kind), d),
            None => write!(f, "{}", describe(self.kind)),
        }
    }
}

/// Plain English for a wire-format variant name.
///
/// The `ExprInstance` spelling is what a developer greps for; the phrase is
/// what a Rholang author recognises as the thing they typed. A diagnostic
/// carries both.
fn describe(kind: &'static str) -> &'static str {
    match kind {
        "EMethodBody" => "a method call",
        "EMatchesBody" => "a `matches` test",
        "EPercentPercentBody" => "a string interpolation (`%%`)",
        "EPlusPlusBody" => "a concatenation (`++`)",
        "EMinusMinusBody" => "a difference (`--`)",
        "EPathmapBody" => "a pathmap literal",
        "EZipperBody" => "a zipper",
        other => other,
    }
}

/// THE single source of truth for the undecidable class.
///
/// ★ Exhaustive with **no catch-all arm**, deliberately: a new `ExprInstance`
/// variant must fail to compile here and in `eval.rs` together, so the gate
/// and the evaluator cannot drift apart by omission. `None` means "this
/// variant has an arm in `eval.rs` that does not return
/// `UnsupportedExpression`" — not "unknown".
fn unsupported_kind(instance: &ExprInstance, spatial: SpatialSupport) -> Option<&'static str> {
    match instance {
        // Ground values — `eval.rs` passes them through.
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => None,

        // Collections — passed through unchanged, interiors never evaluated.
        ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_) => None,

        // Variable reference — resolved against the env at run time.
        ExprInstance::EVarBody(_) => None,

        // Operators — all have evaluating arms.
        ExprInstance::ENotBody(_)
        | ExprInstance::ENegBody(_)
        | ExprInstance::EAndBody(_)
        | ExprInstance::EOrBody(_)
        | ExprInstance::EEqBody(_)
        | ExprInstance::ENeqBody(_)
        | ExprInstance::ELtBody(_)
        | ExprInstance::ELteBody(_)
        | ExprInstance::EGtBody(_)
        | ExprInstance::EGteBody(_)
        | ExprInstance::EPlusBody(_)
        | ExprInstance::EMinusBody(_)
        | ExprInstance::EMultBody(_)
        | ExprInstance::EDivBody(_)
        | ExprInstance::EModBody(_) => None,

        // Decidable iff the caller injects an oracle — `eval.rs`'s
        // `EMatchesBody` arm consults `SpatialMatch::is_available` and refuses
        // before touching either operand when it is absent.
        ExprInstance::EMatchesBody(_) => match spatial {
            SpatialSupport::Available => None,
            SpatialSupport::Absent => Some("EMatchesBody"),
        },

        // ★ THE UNDECIDABLE CLASS. Each spelling is byte-identical to the
        // `kind` its `eval.rs` arm raises.
        ExprInstance::EMethodBody(_) => Some("EMethodBody"),
        ExprInstance::EPercentPercentBody(_) => Some("EPercentPercentBody"),
        ExprInstance::EPlusPlusBody(_) => Some("EPlusPlusBody"),
        ExprInstance::EMinusMinusBody(_) => Some("EMinusMinusBody"),
        ExprInstance::EPathmapBody(_) => Some("EPathmapBody"),
        ExprInstance::EZipperBody(_) => Some("EZipperBody"),
    }
}

/// The name to quote back to the author, when the node carries one.
fn detail_of(instance: &ExprInstance) -> Option<String> {
    match instance {
        ExprInstance::EMethodBody(m) => Some(format!("`{}`", m.method_name)),
        _ => None,
    }
}

/// Every structurally distinct node of `guard` that [`crate::eval_with`] would
/// refuse to decide, in the canonical order the evaluator reaches them.
///
/// Empty ⟺ the guard is decidable: whatever verdict the evaluator reaches, it
/// reached it by evaluating, not by giving up. See the [module docs](self) for
/// the two properties this function is required to satisfy and for the exact
/// set of positions it walks.
///
/// PathMap multiplicity does not repeat classification work: evaluating one
/// occurrence of an identical pure expression establishes whether all of its
/// occurrences are decidable. Distinct obstruction positions *inside* that
/// expression remain distinct report entries. This is the quotient needed by
/// the production caller, which decides only whether the returned vector is
/// empty and renders its entries as diagnostics. Until the extern-PathMap
/// cutover removes Prost's transitional repeated-field adapter, legacy duplicate
/// entries still arrive separately and can repeat an otherwise identical
/// diagnostic; that does not change the empty/non-empty decision.
///
/// ## Complexity
///
/// Θ(n) time in the number of distinct walked nodes and Θ(f) auxiliary space
/// in the walk's maximum frontier — explicit worklists, never the native stack,
/// so a deeply nested guard cannot overflow it. The frontier is preallocated at
/// the guard's top-level distinct Expr endpoint count, which is exact for the
/// overwhelmingly common flat guard. Owned PathMap decodes occupy reusable
/// contiguous slots only while they remain on that frontier; there is no
/// per-endpoint box and no depth-sized retained arena.
pub fn undecidable_nodes(guard: &Par, spatial: SpatialSupport) -> Vec<UndecidableNode> {
    let mut work = Vec::with_capacity(guard.family_distinct_len::<Expr>().max(1));
    let mut owned_pars = OwnedSlots::<Par>::default();
    let mut owned_exprs = OwnedSlots::<Expr>::default();
    work.push(GuardWork::BorrowedPar(guard));
    drive_guard_work(spatial, &mut work, &mut owned_pars, &mut owned_exprs)
}

fn drive_guard_work<'g>(
    spatial: SpatialSupport,
    work: &mut Vec<GuardWork<'g>>,
    owned_pars: &mut OwnedSlots<Par>,
    owned_exprs: &mut OwnedSlots<Expr>,
) -> Vec<UndecidableNode> {
    let mut found = Vec::new();
    while let Some(node) = work.pop() {
        match node {
            GuardWork::BorrowedPar(par) => {
                let mut family = Vec::with_capacity(par.family_distinct_len::<Expr>());
                par.try_visit_components::<Expr, std::convert::Infallible>(
                    |expr, _multiplicity| {
                        family.push(match expr {
                            ParComponentItem::Borrowed(expr) => GuardWork::BorrowedExpr(expr),
                            ParComponentItem::Owned(expr) => {
                                GuardWork::OwnedExpr(owned_exprs.insert(expr))
                            }
                        });
                        Ok(())
                    },
                )
                .expect("Par invariant: expression endpoints are canonical");
                // The worklist is LIFO. Reverse the canonical family stream so
                // its first endpoint is classified first.
                work.extend(family.into_iter().rev());
            }
            GuardWork::OwnedPar(index) => {
                let par = owned_pars.take(index);
                let mut family = Vec::with_capacity(par.family_distinct_len::<Expr>());
                par.try_into_components::<Expr, std::convert::Infallible>(|expr, _multiplicity| {
                    family.push(GuardWork::OwnedExpr(owned_exprs.insert(expr)));
                    Ok(())
                })
                .expect("Par invariant: expression endpoints are canonical");
                work.extend(family.into_iter().rev());
            }
            GuardWork::BorrowedExpr(expr) => {
                push_borrowed_expr(expr, spatial, work, &mut found);
            }
            GuardWork::OwnedExpr(index) => {
                push_owned_expr(
                    owned_exprs.take(index),
                    spatial,
                    owned_pars,
                    work,
                    &mut found,
                );
            }
        }
    }

    found
}

#[derive(Clone, Copy)]
enum GuardWork<'g> {
    BorrowedPar(&'g Par),
    OwnedPar(usize),
    BorrowedExpr(&'g Expr),
    OwnedExpr(usize),
}

/// Reusable storage behind the compact owned-work tokens above.
///
/// A slot is returned to the free list as soon as its token is popped, so the
/// allocation follows maximum frontier width rather than traversal depth.
struct OwnedSlots<T> {
    values: Vec<Option<T>>,
    free: Vec<usize>,
}

impl<T> Default for OwnedSlots<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> OwnedSlots<T> {
    fn insert(&mut self, value: T) -> usize {
        if let Some(index) = self.free.pop() {
            debug_assert!(self.values[index].is_none());
            self.values[index] = Some(value);
            index
        } else {
            let index = self.values.len();
            self.values.push(Some(value));
            index
        }
    }

    fn take(&mut self, index: usize) -> T {
        let value = self.values[index]
            .take()
            .expect("decidability work token names a live owned value");
        self.free.push(index);
        value
    }
}

/// Classify one `Expr` and push the sub-Pars the evaluator would descend into.
///
/// Operands are pushed in reverse for the same LIFO reason as the caller.
fn push_borrowed_expr<'g>(
    expr: &'g Expr,
    spatial: SpatialSupport,
    work: &mut Vec<GuardWork<'g>>,
    found: &mut Vec<UndecidableNode>,
) {
    // A `None` instance is `EvalError::MissingExprInstance` — a malformed Par,
    // not an undecidable one. It is a different fact and gets a different
    // report, so it is deliberately not recorded here.
    let Some(instance) = expr.expr_instance.as_ref() else {
        return;
    };

    if let Some(kind) = unsupported_kind(instance, spatial) {
        found.push(UndecidableNode {
            kind,
            detail: detail_of(instance),
        });
        // Do not descend. The evaluator's arm raises before touching the
        // node's operands, so any node underneath is unreachable and
        // reporting it would over-state the obstruction.
        return;
    }

    match instance {
        ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p }) => {
            push_borrowed_operand(p.as_ref(), work);
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) | ExprInstance::EOrBody(EOr { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EEqBody(EEq { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::ELtBody(ELt { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::ELteBody(ELte { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EGtBody(EGt { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EGteBody(EGte { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EMultBody(EMult { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => {
            push_borrowed_operands(p1.as_ref(), p2.as_ref(), work)
        }

        // ⚠ TARGET ONLY. The pattern's free variables are binders; the
        // evaluator never evaluates it, so a method call inside a pattern is
        // inert and must not be gated. Reached only under
        // `SpatialSupport::Available` — the `Absent` case was recorded above.
        ExprInstance::EMatchesBody(EMatches { target, .. }) => {
            push_borrowed_operand(target.as_ref(), work)
        }

        // Not descended, exactly as `eval_drive` does not descend them:
        // grounds and `EVar` have nothing to descend into, and collection
        // interiors are passed through unevaluated.
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        | ExprInstance::EVarBody(_) => {}

        // Recorded above and returned from; the compiler cannot see that, so
        // the arms are named rather than swept under a `_`.
        ExprInstance::EMethodBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EMinusMinusBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_) => {}
    }
}

fn push_owned_expr<'g>(
    expr: Expr,
    spatial: SpatialSupport,
    owned_pars: &mut OwnedSlots<Par>,
    work: &mut Vec<GuardWork<'g>>,
    found: &mut Vec<UndecidableNode>,
) {
    let Some(instance) = expr.expr_instance else {
        return;
    };

    if let Some(kind) = unsupported_kind(&instance, spatial) {
        found.push(UndecidableNode {
            kind,
            detail: detail_of(&instance),
        });
        return;
    }

    match instance {
        ExprInstance::ENotBody(ENot { p }) | ExprInstance::ENegBody(ENeg { p }) => {
            push_owned_operand(p, owned_pars, work);
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) | ExprInstance::EOrBody(EOr { p1, p2 }) => {
            push_owned_operands(p1, p2, owned_pars, work)
        }
        ExprInstance::EEqBody(EEq { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::ELtBody(ELt { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::ELteBody(ELte { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::EGtBody(EGt { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::EGteBody(EGte { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::EPlusBody(EPlus { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
            push_owned_operands(p1, p2, owned_pars, work)
        }
        ExprInstance::EMultBody(EMult { p1, p2 }) => push_owned_operands(p1, p2, owned_pars, work),
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => {
            push_owned_operands(p1, p2, owned_pars, work)
        }
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => {
            push_owned_operands(p1, p2, owned_pars, work)
        }

        ExprInstance::EMatchesBody(EMatches { target, .. }) => {
            push_owned_operand(target, owned_pars, work)
        }

        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_)
        | ExprInstance::EVarBody(_) => {}

        ExprInstance::EMethodBody(_)
        | ExprInstance::EPercentPercentBody(_)
        | ExprInstance::EPlusPlusBody(_)
        | ExprInstance::EMinusMinusBody(_)
        | ExprInstance::EPathmapBody(_)
        | ExprInstance::EZipperBody(_) => {}
    }
}

fn push_borrowed_operand<'g>(p: Option<&'g Par>, work: &mut Vec<GuardWork<'g>>) {
    // A missing operand is `MissingExprInstance` at evaluation time — again a
    // malformed Par rather than an undecidable one.
    if let Some(par) = p {
        work.push(GuardWork::BorrowedPar(par));
    }
}

fn push_borrowed_operands<'g>(
    p1: Option<&'g Par>,
    p2: Option<&'g Par>,
    work: &mut Vec<GuardWork<'g>>,
) {
    push_borrowed_operand(p2, work);
    push_borrowed_operand(p1, work);
}

fn push_owned_operand<'g>(
    p: Option<Par>,
    owned_pars: &mut OwnedSlots<Par>,
    work: &mut Vec<GuardWork<'g>>,
) {
    if let Some(par) = p {
        work.push(GuardWork::OwnedPar(owned_pars.insert(par)));
    }
}

fn push_owned_operands<'g>(
    p1: Option<Par>,
    p2: Option<Par>,
    owned_pars: &mut OwnedSlots<Par>,
    work: &mut Vec<GuardWork<'g>>,
) {
    push_owned_operand(p2, owned_pars, work);
    push_owned_operand(p1, owned_pars, work);
}

#[cfg(test)]
mod owned_endpoint_tests {
    use models::rhoapi::{EEq, EMethod};

    use super::*;

    fn method(name: &str) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                    method_name: name.to_string(),
                    target: Some(Par::default()),
                    arguments: Vec::new(),
                    locally_free: Vec::new(),
                    connective_used: false,
                })),
            }],
            ..Par::default()
        }
    }

    #[test]
    fn owned_decoded_endpoint_preserves_order_and_reclaims_every_slot() {
        let root = Expr {
            expr_instance: Some(ExprInstance::EEqBody(EEq {
                p1: Some(method("left")),
                p2: Some(method("right")),
            })),
        };
        let mut owned_pars = OwnedSlots::default();
        let mut owned_exprs = OwnedSlots::default();
        let root_index = owned_exprs.insert(root);
        let mut work = vec![GuardWork::OwnedExpr(root_index)];

        let found = drive_guard_work(
            SpatialSupport::Available,
            &mut work,
            &mut owned_pars,
            &mut owned_exprs,
        );

        assert_eq!(found, vec![
            UndecidableNode {
                kind: "EMethodBody",
                detail: Some("`left`".to_string()),
            },
            UndecidableNode {
                kind: "EMethodBody",
                detail: Some("`right`".to_string()),
            },
        ]);
        assert!(work.is_empty());
        assert_eq!(owned_pars.free.len(), owned_pars.values.len());
        assert_eq!(owned_exprs.free.len(), owned_exprs.values.len());
    }
}
