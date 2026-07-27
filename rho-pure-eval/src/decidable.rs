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
//! | `par.exprs[i]` | yes (`EvKont::ParK`) | yes |
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
//! carrying an unevaluated `EMethodBody` in its top-level `exprs` cannot be
//! produced by a reduction. Callers that want the residue closed anyway have
//! the run-time disposition for it — the gate is the first line, not the only
//! one.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMatches, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr,
    Par,
};

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

/// Every node of `guard` that [`crate::eval_with`] would refuse to decide, in
/// the order the evaluator would have reached them.
///
/// Empty ⟺ the guard is decidable: whatever verdict the evaluator reaches, it
/// reached it by evaluating, not by giving up. See the [module docs](self) for
/// the two properties this function is required to satisfy and for the exact
/// set of positions it walks.
///
/// ## Complexity
///
/// Θ(n) time in the number of walked nodes and Θ(d) auxiliary space in the
/// walk's maximum frontier — an explicit worklist, never the native stack, so
/// a deeply nested guard cannot overflow it. The frontier is preallocated at
/// the guard's own top-level `exprs` count, which is its exact size for the
/// overwhelmingly common flat guard.
pub fn undecidable_nodes(guard: &Par, spatial: SpatialSupport) -> Vec<UndecidableNode> {
    let mut found: Vec<UndecidableNode> = Vec::new();
    let mut work: Vec<&Par> = Vec::with_capacity(guard.exprs.len().max(1));
    work.push(guard);

    while let Some(par) = work.pop() {
        // Reverse push: the worklist is LIFO, so pushing in reverse makes the
        // exprs pop in source order and `found` come out in the order the
        // evaluator would have reached them.
        for expr in par.exprs.iter().rev() {
            push_expr(expr, spatial, &mut work, &mut found);
        }
    }

    found
}

/// Classify one `Expr` and push the sub-Pars the evaluator would descend into.
///
/// Operands are pushed in reverse for the same LIFO reason as the caller.
fn push_expr<'g>(
    expr: &'g Expr,
    spatial: SpatialSupport,
    work: &mut Vec<&'g Par>,
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
            push_operand(p.as_ref(), work);
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) | ExprInstance::EOrBody(EOr { p1, p2 }) => {
            push_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EEqBody(EEq { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::ELtBody(ELt { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::ELteBody(ELte { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::EGtBody(EGt { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::EGteBody(EGte { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::EPlusBody(EPlus { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
            push_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EMultBody(EMult { p1, p2 }) => push_operands(p1.as_ref(), p2.as_ref(), work),
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => {
            push_operands(p1.as_ref(), p2.as_ref(), work)
        }
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => {
            push_operands(p1.as_ref(), p2.as_ref(), work)
        }

        // ⚠ TARGET ONLY. The pattern's free variables are binders; the
        // evaluator never evaluates it, so a method call inside a pattern is
        // inert and must not be gated. Reached only under
        // `SpatialSupport::Available` — the `Absent` case was recorded above.
        ExprInstance::EMatchesBody(EMatches { target, .. }) => push_operand(target.as_ref(), work),

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

fn push_operand<'g>(p: Option<&'g Par>, work: &mut Vec<&'g Par>) {
    // A missing operand is `MissingExprInstance` at evaluation time — again a
    // malformed Par rather than an undecidable one.
    if let Some(par) = p {
        work.push(par);
    }
}

fn push_operands<'g>(p1: Option<&'g Par>, p2: Option<&'g Par>, work: &mut Vec<&'g Par>) {
    push_operand(p2, work);
    push_operand(p1, work);
}
