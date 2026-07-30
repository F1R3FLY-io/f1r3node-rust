use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMatches, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr,
    Par, Var,
};
use models::rust::rholang::drive::{drive, Outcome, Step, Traversal};

use crate::env::Env;
use crate::error::EvalError;
use crate::oracle::{NoSpatialMatch, SpatialMatch};

/// Evaluates the `exprs` slot of a Par against the given environment.
///
/// Process-level Par fields (sends, receives, news, matches, bundles,
/// unforgeables, connectives, conditionals) are preserved unchanged in
/// the returned Par. The `exprs` slot is replaced with the evaluated
/// values.
///
/// Equivalent to [`eval_with`] under the [`NoSpatialMatch`] oracle: no
/// spatial matcher is available, so `EMatches` yields
/// `EvalError::UnsupportedExpression { kind: "EMatchesBody" }`. Callers
/// that can supply a matcher (i.e. `rholang`, which owns one) get the
/// `EMatches` arm by calling [`eval_with`] instead; every other caller
/// is unaffected.
pub fn eval(par: &Par, env: &Env<Par>) -> Result<Par, EvalError> {
    eval_with(par, env, &NoSpatialMatch)
}

/// [`eval`], plus a caller-supplied spatial-match oracle for `EMatches`.
///
/// `matcher` decides `target matches pattern` for every `EMatchesBody`
/// encountered anywhere in `par`, at any nesting depth. It must be pure,
/// total and deterministic — see [`SpatialMatch`] for the contract this
/// crate's own determinism guarantee rests on.
pub fn eval_with(par: &Par, env: &Env<Par>, matcher: &dyn SpatialMatch) -> Result<Par, EvalError> {
    let mut traversal = EvalTraversal { env, matcher };
    let mut state = ();
    drive(
        &mut traversal,
        &mut state,
        Step::Descend(EvNode::Eval {
            par,
            extract: false,
        }),
    )
    .map(|v| match v {
        EvVal::Par(p) => p,
        EvVal::Inst(_) => unreachable!("eval_with: a non-extracting root must yield a Par"),
    })
}

/// The pre-conversion recursive body, retained verbatim as the ORACLE.
///
/// Θ(depth) on purpose: it is the reference the worklist is compared against,
/// exactly as `reduce.rs` keeps `eval_expr_recursive` beside its trampoline and
/// `score_tree.rs` keeps `compare_score_recursive` beside `compare_score`.
#[cfg(test)]
pub(crate) fn eval_with_recursive(
    par: &Par,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
) -> Result<Par, EvalError> {
    let mut acc = Par {
        sends: par.sends.clone(),
        receives: par.receives.clone(),
        news: par.news.clone(),
        matches: par.matches.clone(),
        unforgeables: par.unforgeables.clone(),
        bundles: par.bundles.clone(),
        connectives: par.connectives.clone(),
        conditionals: par.conditionals.clone(),
        exprs: Vec::new(),
        locally_free: par.locally_free.clone(),
        connective_used: par.connective_used,
    };

    for expr in &par.exprs {
        let evaled = eval_expr_to_par_recursive(expr, env, matcher)?;
        acc = concatenate(acc, evaled);
    }

    Ok(acc)
}

fn concatenate(a: Par, b: Par) -> Par {
    Par {
        sends: [a.sends, b.sends].concat(),
        receives: [a.receives, b.receives].concat(),
        news: [a.news, b.news].concat(),
        exprs: [a.exprs, b.exprs].concat(),
        matches: [a.matches, b.matches].concat(),
        unforgeables: [a.unforgeables, b.unforgeables].concat(),
        bundles: [a.bundles, b.bundles].concat(),
        connectives: [a.connectives, b.connectives].concat(),
        conditionals: [a.conditionals, b.conditionals].concat(),
        locally_free: union_bytes(a.locally_free, b.locally_free),
        connective_used: a.connective_used || b.connective_used,
    }
}

fn union_bytes(mut a: Vec<u8>, b: Vec<u8>) -> Vec<u8> {
    if b.len() > a.len() {
        a.resize(b.len(), 0);
    }
    for (i, byte) in b.iter().enumerate() {
        a[i] |= byte;
    }
    a
}

#[cfg(test)]
fn eval_expr_to_par_recursive(
    expr: &Expr,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
) -> Result<Par, EvalError> {
    let instance = expr
        .expr_instance
        .as_ref()
        .ok_or(EvalError::MissingExprInstance)?;

    match instance {
        // Ground values - pass through.
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => Ok(par_with_expr(expr.clone())),

        // Collections - pass through unchanged. Their elements were
        // already values when the Par was constructed.
        ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_) => Ok(par_with_expr(expr.clone())),

        // Variable reference: look up in env, recurse on the result so
        // that any nested EVar / EOp gets resolved.
        ExprInstance::EVarBody(evar) => {
            let v = evar.v.as_ref().ok_or(EvalError::MissingExprInstance)?;
            let p = resolve_var(v, env)?;
            eval_with_recursive(&p, env, matcher)
        }

        ExprInstance::ENotBody(ENot { p }) => {
            let inner = require_par(p.as_ref())?;
            let evaled = eval_with_recursive(&inner, env, matcher)?;
            let v = single_expr_instance(&evaled)?;
            match v {
                ExprInstance::GBool(b) => Ok(par_with_bool(!b)),
                other => Err(operator_mismatch_unary("!", &other)),
            }
        }

        ExprInstance::ENegBody(ENeg { p }) => {
            let inner = require_par(p.as_ref())?;
            let evaled = eval_with_recursive(&inner, env, matcher)?;
            let v = single_expr_instance(&evaled)?;
            match v {
                ExprInstance::GInt(i) => i
                    .checked_neg()
                    .map(par_with_int)
                    .ok_or(EvalError::ArithmeticOverflow { op: "-" }),
                other => Err(operator_mismatch_unary("-", &other)),
            }
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) => {
            bool_binop_recursive("&&", p1.as_ref(), p2.as_ref(), env, matcher, |a, b| a && b)
        }
        ExprInstance::EOrBody(EOr { p1, p2 }) => {
            bool_binop_recursive("||", p1.as_ref(), p2.as_ref(), env, matcher, |a, b| a || b)
        }

        ExprInstance::EEqBody(EEq { p1, p2 }) => {
            eq_binop_recursive(p1.as_ref(), p2.as_ref(), env, matcher, true)
        }
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => {
            eq_binop_recursive(p1.as_ref(), p2.as_ref(), env, matcher, false)
        }

        ExprInstance::ELtBody(ELt { p1, p2 }) => {
            cmp_binop_recursive("<", p1.as_ref(), p2.as_ref(), env, matcher, |c| c == -1)
        }
        ExprInstance::ELteBody(ELte { p1, p2 }) => {
            cmp_binop_recursive("<=", p1.as_ref(), p2.as_ref(), env, matcher, |c| c <= 0)
        }
        ExprInstance::EGtBody(EGt { p1, p2 }) => {
            cmp_binop_recursive(">", p1.as_ref(), p2.as_ref(), env, matcher, |c| c == 1)
        }
        ExprInstance::EGteBody(EGte { p1, p2 }) => {
            cmp_binop_recursive(">=", p1.as_ref(), p2.as_ref(), env, matcher, |c| c >= 0)
        }

        ExprInstance::EPlusBody(EPlus { p1, p2 }) => int_binop_checked_recursive(
            "+",
            p1.as_ref(),
            p2.as_ref(),
            env,
            matcher,
            i64::checked_add,
        ),
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => int_binop_checked_recursive(
            "-",
            p1.as_ref(),
            p2.as_ref(),
            env,
            matcher,
            i64::checked_sub,
        ),
        ExprInstance::EMultBody(EMult { p1, p2 }) => int_binop_checked_recursive(
            "*",
            p1.as_ref(),
            p2.as_ref(),
            env,
            matcher,
            i64::checked_mul,
        ),
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => int_div_or_mod_recursive(
            "/",
            p1.as_ref(),
            p2.as_ref(),
            env,
            matcher,
            i64::checked_div,
        ),
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => int_div_or_mod_recursive(
            "%",
            p1.as_ref(),
            p2.as_ref(),
            env,
            matcher,
            i64::checked_rem,
        ),

        // Spatial match. Mirrors `Reduce::combine_matches`
        // (rholang/src/rust/interpreter/reduce.rs) minus its two
        // `substitute_and_charge` calls, which are already performed
        // upstream: `eval_receive` substitutes the whole guard at depth 1
        // before it is stored on the TaggedContinuation, and
        // `substitute`'s own `EMatchesBody` arm descends into BOTH
        // operands at that same depth — so the guard's pattern has
        // already had exactly the depth-1 substitution `combine_matches`
        // would apply to it, and (because `maybe_substitute_var` is the
        // identity at depth != 0) no variable inside it has been
        // resolved away.
        //
        // The target IS evaluated: its variables are ordinary references
        // to the bound values, resolved against `env`. The pattern is
        // NOT: its free variables are binders, and evaluating them would
        // raise `UnboundVariable` instead of matching.
        ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
            if !matcher.is_available() {
                // No oracle injected (the `eval` default). Refuse before
                // touching either operand — byte-identical to the
                // behaviour that predates this seam.
                return Err(EvalError::UnsupportedExpression {
                    kind: "EMatchesBody",
                });
            }
            let evaled_target = eval_with_recursive(&require_par(target.as_ref())?, env, matcher)?;
            let verbatim_pattern = require_par(pattern.as_ref())?;
            Ok(par_with_bool(
                matcher.matches(&evaled_target, &verbatim_pattern),
            ))
        }

        // Stubs — supported in the full reducer but not here yet.
        ExprInstance::EMethodBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMethodBody",
        }),
        ExprInstance::EPercentPercentBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPercentPercentBody",
        }),
        ExprInstance::EPlusPlusBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPlusPlusBody",
        }),
        ExprInstance::EMinusMinusBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMinusMinusBody",
        }),
        ExprInstance::EPathmapBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPathmapBody",
        }),
        ExprInstance::EZipperBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EZipperBody",
        }),
    }
}

fn resolve_var(v: &Var, env: &Env<Par>) -> Result<Par, EvalError> {
    match v.var_instance.as_ref() {
        Some(VarInstance::BoundVar(idx)) => env
            .get(idx)
            .ok_or(EvalError::UnboundVariable { index: *idx }),
        Some(VarInstance::FreeVar(idx)) => Err(EvalError::UnboundVariable { index: *idx }),
        Some(VarInstance::Wildcard(_)) | None => Err(EvalError::MissingExprInstance),
    }
}

/// ⚠ ORACLE-ONLY. The production machine uses [`require_par_ref`], which
/// borrows: this spelling `cloned()` every operand of every operator — a
/// Θ(depth) `<Par as Clone>::clone` per operand — and the machine only reads
/// its operands. Retained `#[cfg(test)]` so the oracle stays a faithful copy of
/// the pre-conversion form.
#[cfg(test)]
fn require_par(p: Option<&Par>) -> Result<Par, EvalError> {
    p.cloned().ok_or(EvalError::MissingExprInstance)
}

fn par_with_expr(e: Expr) -> Par {
    Par {
        exprs: vec![e],
        ..Par::default()
    }
}

fn par_with_bool(b: bool) -> Par {
    par_with_expr(Expr {
        expr_instance: Some(ExprInstance::GBool(b)),
    })
}

fn par_with_int(i: i64) -> Par {
    par_with_expr(Expr {
        expr_instance: Some(ExprInstance::GInt(i)),
    })
}

/// Returns the single ExprInstance carried by `par` if it has exactly
/// one Expr and no other content. Mirrors the precondition that the
/// reducer's `eval_single_expr` upholds.
fn single_expr_instance(par: &Par) -> Result<ExprInstance, EvalError> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.unforgeables.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || par.exprs.len() != 1
    {
        return Err(EvalError::NotASingleValue {
            actual: Box::new(par.clone()),
        });
    }
    par.exprs[0]
        .expr_instance
        .clone()
        .ok_or(EvalError::MissingExprInstance)
}

#[cfg(test)]
fn bool_binop_recursive<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(bool, bool) -> bool,
{
    let v1 = single_expr_instance(&eval_with_recursive(&require_par(p1)?, env, matcher)?)?;
    let v2 = single_expr_instance(&eval_with_recursive(&require_par(p2)?, env, matcher)?)?;
    match (&v1, &v2) {
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => Ok(par_with_bool(f(*b1, *b2))),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

#[cfg(test)]
fn eq_binop_recursive(
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    expect_eq: bool,
) -> Result<Par, EvalError> {
    let lhs = eval_with_recursive(&require_par(p1)?, env, matcher)?;
    let rhs = eval_with_recursive(&require_par(p2)?, env, matcher)?;
    // IEEE 754: any comparison involving NaN is false (so == is false and
    // != is true). Mirrors `par_contains_nan_double` in the full reducer.
    let eq = if par_contains_nan_double(&lhs) || par_contains_nan_double(&rhs) {
        false
    } else {
        lhs == rhs
    };
    Ok(par_with_bool(if expect_eq { eq } else { !eq }))
}

fn par_contains_nan_double(par: &Par) -> bool {
    use models::rhoapi::expr::ExprInstance::{EListBody, EMapBody, ESetBody, ETupleBody, GDouble};
    par.exprs.iter().any(|e| match &e.expr_instance {
        Some(GDouble(bits)) => f64::from_bits(*bits).is_nan(),
        Some(EListBody(list)) => list.ps.iter().any(par_contains_nan_double),
        Some(ETupleBody(tuple)) => tuple.ps.iter().any(par_contains_nan_double),
        Some(ESetBody(set)) => set.ps.iter().any(par_contains_nan_double),
        Some(EMapBody(map)) => map.kvs.iter().any(|kv| {
            kv.key.as_ref().is_some_and(par_contains_nan_double)
                || kv.value.as_ref().is_some_and(par_contains_nan_double)
        }),
        _ => false,
    })
}

#[cfg(test)]
fn cmp_binop_recursive<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    interpret: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64) -> bool,
{
    let v1 = single_expr_instance(&eval_with_recursive(&require_par(p1)?, env, matcher)?)?;
    let v2 = single_expr_instance(&eval_with_recursive(&require_par(p2)?, env, matcher)?)?;
    let order: i64 = match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => i64::from(i1.cmp(i2) as i8),
        (ExprInstance::GString(s1), ExprInstance::GString(s2)) => i64::from(s1.cmp(s2) as i8),
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => i64::from(b1.cmp(b2) as i8),
        _ => return Err(operator_mismatch_binary(op, &v1, &v2)),
    };
    Ok(par_with_bool(interpret(order)))
}

#[cfg(test)]
fn int_binop_checked_recursive<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64, i64) -> Option<i64>,
{
    let v1 = single_expr_instance(&eval_with_recursive(&require_par(p1)?, env, matcher)?)?;
    let v2 = single_expr_instance(&eval_with_recursive(&require_par(p2)?, env, matcher)?)?;
    match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => f(*i1, *i2)
            .map(par_with_int)
            .ok_or(EvalError::ArithmeticOverflow { op }),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

#[cfg(test)]
fn int_div_or_mod_recursive<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64, i64) -> Option<i64>,
{
    let v1 = single_expr_instance(&eval_with_recursive(&require_par(p1)?, env, matcher)?)?;
    let v2 = single_expr_instance(&eval_with_recursive(&require_par(p2)?, env, matcher)?)?;
    match (&v1, &v2) {
        (ExprInstance::GInt(_), ExprInstance::GInt(0)) => Err(EvalError::DivisionByZero),
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => f(*i1, *i2)
            .map(par_with_int)
            .ok_or(EvalError::ArithmeticOverflow { op }),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

fn operator_mismatch_unary(op: &'static str, instance: &ExprInstance) -> EvalError {
    EvalError::OperatorTypeMismatch {
        op,
        left: type_name(instance).to_string(),
        right: None,
    }
}

fn operator_mismatch_binary(op: &'static str, a: &ExprInstance, b: &ExprInstance) -> EvalError {
    EvalError::OperatorTypeMismatch {
        op,
        left: type_name(a).to_string(),
        right: Some(type_name(b).to_string()),
    }
}

fn type_name(instance: &ExprInstance) -> &'static str {
    match instance {
        ExprInstance::GBool(_) => "Bool",
        ExprInstance::GInt(_) => "Int",
        ExprInstance::GBigInt(_) => "BigInt",
        ExprInstance::GBigRat(_) => "BigRat",
        ExprInstance::GFixedPoint(_) => "FixedPoint",
        ExprInstance::GString(_) => "String",
        ExprInstance::GUri(_) => "Uri",
        ExprInstance::GByteArray(_) => "ByteArray",
        ExprInstance::GDouble(_) => "Double",
        ExprInstance::EListBody(_) => "List",
        ExprInstance::ETupleBody(_) => "Tuple",
        ExprInstance::ESetBody(_) => "Set",
        ExprInstance::EMapBody(_) => "Map",
        ExprInstance::EVarBody(_) => "EVar",
        _ => "Expr",
    }
}

// ===========================================================================
// Leg-2 Stage E / Stage F-1: `eval_with` as an instance of the SHARED driver
// ===========================================================================
//
// ★ Stage F-1. This machine was originally a bespoke `eval_drive` loop with its
// own copy of the deficit invariant. It is now an instance of
// `models::rust::rholang::drive` — the one trampoline the `Par` family's
// traversals share — and `eval_with` was chosen as the REHEARSAL SUBJECT for
// that driver deliberately: it is the smallest member, it takes no budget
// handle, and it has no consensus exposure, so the driver's ergonomics get
// found here rather than on the sorter's canonical form.
//
// What moved out of this file: the LIFO loop, the value stack, the work stack,
// the deficit invariant and the final-configuration assertion. What stayed: the
// alphabet (`EvNode`, `EvKont`, `EvVal`), the per-arm descend and combine
// halves, and every semantic note below.
//
// `eval_with` itself was already a loop over `par.exprs`; the Θ(depth)
// recursion lived in `eval_expr_to_par`, which re-entered `eval_with` at
// ELEVEN sites — the `EVar` resolution, the two unary arms, the `EMatches`
// target, and two operands in each of the five binop helpers. Measured on a
// nested-`ENot` chain: **21,584 bytes of native stack per level** in debug,
// 3,359 in release. That is its OWN SCC — a nested `EList` chain does not
// reach it at all (the `EListBody` arm returns without descending, so on that
// shape the probe measures `<Par as Clone>::clone` and nothing else).
//
// ## ★ The interleaving that a naive post-order would silently break
//
// The five binop helpers do NOT evaluate both operands and then check them.
// They do, per operand:
//
// ```ignore
// let v1 = single_expr_instance(&eval_with(p1)?)?;   // <- check p1 HERE
// let v2 = single_expr_instance(&eval_with(p2)?)?;   // <- only then touch p2
// ```
//
// So if `p1` evaluates but is not a single value, `p2` is **never evaluated**.
// A worklist that evaluated both children and then extracted would report
// `p2`'s error where the recursive form reports `p1`'s — an observable
// divergence in `EvalError`, on a path that decides `where`-clause guards.
//
// The machine therefore carries an `extract` flag on the operand work item and
// applies `single_expr_instance` **at that operand's completion**. Because the
// operands are pushed in reverse, `p1`'s entire subtree runs to completion
// before `p2` is popped, so the check lands at exactly the recursive form's
// point.
//
// ## ⚠ The one re-entry that stays a nested drive
//
// `EVarBody` resolves through `resolve_var`, which returns a value **cloned out
// of the environment** — an owned intermediate, not a sub-term of the input, so
// it cannot be borrowed onto this worklist. It runs its own bounded drive. The
// residual is bounded by the depth of the BOUND VALUE and by the environment's
// chain length, never by the depth of the term being evaluated — the same
// disposition as `substitute_deep_binding`.

/// A produced value. `Inst` is an operand that was extracted with
/// `single_expr_instance` at its own completion; see the module note above.
///
/// ⚠ `clippy::large_enum_variant` objects to the 504-vs-248-byte spread and
/// suggests boxing `Inst`. **Deliberately not taken**, and the reason is the
/// access pattern rather than a preference:
///
/// * `EvVal` is the element type of the driver's value stack,
///   `Vec<EvVal>` with `with_capacity(32)`, so the enum's size is a
///   one-off ~16 KiB reservation per `eval_drive` call — not a per-value cost.
/// * `Inst` is **not** a rare variant. It is pushed by
///   `single_expr_instance(&evaled).map(EvVal::Inst)` and popped by
///   `into_inst()` in `EvKont::Not`, `Neg`, the boolean, `Eq`, comparison,
///   integer and div/mod combiners — i.e. once per OPERAND of essentially
///   every operator the evaluator has.
///
/// So boxing would trade a stack-slot saving on a fixed-size buffer for a heap
/// allocation and a free on the evaluator's hottest path, twice per binary
/// operation. `ExprInstance` is a prost-generated consensus type and is not
/// ours to shrink.
#[allow(clippy::large_enum_variant)]
enum EvVal {
    Par(Par),
    Inst(ExprInstance),
}

impl EvVal {
    fn into_par(self) -> Par {
        match self {
            EvVal::Par(p) => p,
            EvVal::Inst(_) => unreachable!("eval_drive: expected a Par on the value stack"),
        }
    }
    fn into_inst(self) -> ExprInstance {
        match self {
            EvVal::Inst(i) => i,
            EvVal::Par(_) => unreachable!("eval_drive: expected an ExprInstance"),
        }
    }
}

/// An input node. Every reference borrows the input term (`'t`).
///
/// `Copy`, as [`Traversal::Node`] requires: pushing a child is a reference move,
/// never a `<Par as Clone>::clone`.
#[derive(Clone, Copy)]
enum EvNode<'t> {
    /// `eval_with(par)`, optionally followed immediately by
    /// `single_expr_instance` — the per-operand check the helpers interleave.
    Eval { par: &'t Par, extract: bool },
    /// `eval_expr_to_par(expr)`.
    Expr(&'t Expr),
}

/// Post-order continuation. Carries the borrowed shell and the operator, never
/// a child value.
enum EvKont<'t> {
    /// Fold `concatenate` over `n` evaluated exprs onto the shell of `par`.
    ParK { par: &'t Par, n: usize },
    Not,
    Neg,
    BoolK {
        op: &'static str,
        f: fn(bool, bool) -> bool,
    },
    EqK {
        expect_eq: bool,
    },
    CmpK {
        op: &'static str,
        interpret: fn(i64) -> bool,
    },
    IntK {
        op: &'static str,
        f: fn(i64, i64) -> Option<i64>,
    },
    DivModK {
        op: &'static str,
        f: fn(i64, i64) -> Option<i64>,
    },
    MatchesK { pattern: &'t Par },
    /// Apply `single_expr_instance` to the value just produced.
    ///
    /// ⚠ This is what preserves the helpers' per-operand interleaving: it is
    /// pushed BEFORE the operand's `ParK`, so it runs immediately after that
    /// operand finishes evaluating and strictly before the next operand is
    /// popped — which is exactly where the recursive form applies it.
    Extract,
}

/// The visitor: `eval_with`'s immutable configuration.
///
/// The environment and the spatial-match oracle are read, never written, so
/// they live on the visitor and [`Traversal::State`] is `()`. Nothing here is
/// per-node state; the machine's entire mutable configuration is the driver's
/// two stacks.
struct EvalTraversal<'e> {
    env: &'e Env<Par>,
    matcher: &'e dyn SpatialMatch,
}

impl<'e> Traversal for EvalTraversal<'e> {
    // ⚠ `where Self: 't` is restated here because this is the ONE visitor in the
    // workspace that is not zero-sized: it borrows its `Env` and its
    // `SpatialMatch` for `'e`. The bound reads `'e: 't` — the environment must
    // outlive the term being evaluated, which it does, since the term is
    // evaluated *in* that environment. ★ It is also why `drive.rs` states the
    // precise bound `Self: 't` rather than `Self: 'static`: `'static` would make
    // this impl illegal outright.
    type Node<'t>
        = EvNode<'t>
    where
        Self: 't;
    type Val = EvVal;
    type Kont<'t>
        = EvKont<'t>
    where
        Self: 't;
    type State = ();
    type Err = EvalError;

    /// ★ Every bounded part of a node runs straight-line here and the call
    /// returns at the first descent — the contract `drive.rs`'s module docs
    /// derive from `wire.rs` §A2's 1.7×-slower table interpretation. `descend`
    /// is a direct, monomorphized call (the trait is not object-safe), so the
    /// cost is one call per node per suspension, never one per field.
    fn descend<'t>(
        &mut self,
        _state: &mut (),
        node: EvNode<'t>,
        work: &mut Vec<Step<'t, Self>>,
        vals: &mut Vec<EvVal>,
    ) -> Result<(), EvalError> {
        match node {
            // `eval_with`'s own body: a `ParK` continuation over the Par's
            // exprs. `extract` rides on the continuation's RESULT, so it is
            // applied when this whole sub-evaluation completes — which is the
            // point the recursive helpers apply it. `Extract` is pushed FIRST
            // so it pops LAST, after `ParK` has produced this sub-evaluation's
            // `Par`.
            //
            // ⚠ This region is why `drive.rs`'s Invariant 1 is a right-to-left
            // scan rather than the hand-written machines' "one `Combine` then
            // exactly `arity` children": `[Extract, ParK{n}, child × n]` NESTS
            // two continuations, and the narrower rule rejected it.
            EvNode::Eval { par, extract } => {
                if extract {
                    work.push(Step::Combine(EvKont::Extract));
                }
                work.push(Step::Combine(EvKont::ParK {
                    par,
                    n: par.exprs.len(),
                }));
                for expr in par.exprs.iter().rev() {
                    work.push(Step::Descend(EvNode::Expr(expr)));
                }
                Ok(())
            }
            EvNode::Expr(expr) => descend_ev_expr(expr, self.env, self.matcher, work, vals),
        }
    }

    fn combine<'t>(
        &mut self,
        _state: &mut (),
        kont: EvKont<'t>,
        vals: &mut Vec<EvVal>,
    ) -> Result<Outcome<EvVal, EvNode<'t>>, EvalError>
    where
        Self: 't,
    {
        // ⚠ NO EARLY EXIT. `eval_with` computes a value rather than deciding a
        // predicate: every continuation's result is a child of the next one, so
        // there is no configuration in which a combine already holds the
        // answer. `Outcome::Done` is for the comparison traversals, where it
        // restores the short-circuit of the `&&` chain it replaces.
        run_ev_combine(kont, self.matcher, vals).map(Outcome::Value)
    }

    /// ★ Deliberately duplicates the pop counts in [`run_ev_combine`], so the
    /// driver's deficit invariant can cross-check them. Exhaustive, no `_` arm.
    fn arity(kont: &EvKont<'_>) -> usize {
        match kont {
            EvKont::ParK { n, .. } => *n,
            EvKont::Not | EvKont::Neg => 1,
            EvKont::BoolK { .. } => 2,
            EvKont::EqK { .. } => 2,
            EvKont::CmpK { .. } => 2,
            EvKont::IntK { .. } => 2,
            EvKont::DivModK { .. } => 2,
            EvKont::MatchesK { .. } => 1,
            EvKont::Extract => 1,
        }
    }
}

/// `require_par`, WITHOUT the clone.
///
/// The original was `p.cloned().ok_or(…)` — a Θ(depth) `<Par as Clone>::clone`
/// on **every operand of every operator**. The machine only reads its operands,
/// so it borrows them; same value, no copy.
fn require_par_ref(p: Option<&Par>) -> Result<&Par, EvalError> {
    p.ok_or(EvalError::MissingExprInstance)
}

/// Push the two operands of a binary arm so they are POPPED p1-then-p2, with
/// the per-operand `single_expr_instance` check applied at each completion.
fn push_binary<'t, 'e>(
    work: &mut Vec<Step<'t, EvalTraversal<'e>>>,
    kont: EvKont<'t>,
    p1: Option<&'t Par>,
    p2: Option<&'t Par>,
    extract: bool,
) -> Result<(), EvalError> {
    // ⚠ Both `require_par_ref`s are evaluated BEFORE either operand runs, which
    // is what the recursive form does too: `require_par(p1)?` sits inside the
    // `eval_with(&require_par(p1)?)` argument, so a missing p1 raises before p2
    // is touched. Evaluating p1's check first preserves which error surfaces.
    let a = require_par_ref(p1)?;
    let b = require_par_ref(p2)?;
    work.push(Step::Combine(kont));
    work.push(Step::Descend(EvNode::Eval { par: b, extract }));
    work.push(Step::Descend(EvNode::Eval { par: a, extract }));
    Ok(())
}

// ---- the shared per-arm COMBINE half, used by the driver AND the oracle ----

fn combine_not(v: ExprInstance) -> Result<Par, EvalError> {
    match v {
        ExprInstance::GBool(b) => Ok(par_with_bool(!b)),
        other => Err(operator_mismatch_unary("!", &other)),
    }
}

fn combine_neg(v: ExprInstance) -> Result<Par, EvalError> {
    match v {
        ExprInstance::GInt(i) => i
            .checked_neg()
            .map(par_with_int)
            .ok_or(EvalError::ArithmeticOverflow { op: "-" }),
        other => Err(operator_mismatch_unary("-", &other)),
    }
}

fn combine_bool(
    op: &'static str,
    f: impl Fn(bool, bool) -> bool,
    v1: ExprInstance,
    v2: ExprInstance,
) -> Result<Par, EvalError> {
    match (&v1, &v2) {
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => Ok(par_with_bool(f(*b1, *b2))),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

fn combine_eq(expect_eq: bool, lhs: Par, rhs: Par) -> Result<Par, EvalError> {
    // IEEE 754: any comparison involving NaN is false (so == is false and
    // != is true). Mirrors `par_contains_nan_double` in the full reducer.
    let eq = if par_contains_nan_double(&lhs) || par_contains_nan_double(&rhs) {
        false
    } else {
        lhs == rhs
    };
    Ok(par_with_bool(if expect_eq { eq } else { !eq }))
}

fn combine_cmp(
    op: &'static str,
    interpret: impl Fn(i64) -> bool,
    v1: ExprInstance,
    v2: ExprInstance,
) -> Result<Par, EvalError> {
    let order: i64 = match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => i64::from(i1.cmp(i2) as i8),
        (ExprInstance::GString(s1), ExprInstance::GString(s2)) => i64::from(s1.cmp(s2) as i8),
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => i64::from(b1.cmp(b2) as i8),
        _ => return Err(operator_mismatch_binary(op, &v1, &v2)),
    };
    Ok(par_with_bool(interpret(order)))
}

fn combine_int(
    op: &'static str,
    f: impl Fn(i64, i64) -> Option<i64>,
    v1: ExprInstance,
    v2: ExprInstance,
) -> Result<Par, EvalError> {
    match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => f(*i1, *i2)
            .map(par_with_int)
            .ok_or(EvalError::ArithmeticOverflow { op }),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

fn combine_div_or_mod(
    op: &'static str,
    f: impl Fn(i64, i64) -> Option<i64>,
    v1: ExprInstance,
    v2: ExprInstance,
) -> Result<Par, EvalError> {
    match (&v1, &v2) {
        (ExprInstance::GInt(_), ExprInstance::GInt(0)) => Err(EvalError::DivisionByZero),
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => f(*i1, *i2)
            .map(par_with_int)
            .ok_or(EvalError::ArithmeticOverflow { op }),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

/// Descend one `Expr`, pushing its continuation and then its operands in
/// REVERSE so they pop in source order.
fn descend_ev_expr<'t, 'e>(
    expr: &'t Expr,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    work: &mut Vec<Step<'t, EvalTraversal<'e>>>,
    vals: &mut Vec<EvVal>,
) -> Result<(), EvalError> {
    let instance = expr
        .expr_instance
        .as_ref()
        .ok_or(EvalError::MissingExprInstance)?;

    match instance {
        // Ground values - pass through.
        ExprInstance::GBool(_)
        | ExprInstance::GInt(_)
        | ExprInstance::GString(_)
        | ExprInstance::GUri(_)
        | ExprInstance::GByteArray(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_) => {
            vals.push(EvVal::Par(par_with_expr(expr.clone())));
            Ok(())
        }

        // Collections - pass through unchanged. Their elements were
        // already values when the Par was constructed.
        ExprInstance::EListBody(_)
        | ExprInstance::ETupleBody(_)
        | ExprInstance::ESetBody(_)
        | ExprInstance::EMapBody(_) => {
            vals.push(EvVal::Par(par_with_expr(expr.clone())));
            Ok(())
        }

        // ⚠ The one re-entry that stays a NESTED DRIVE: `resolve_var` returns a
        // value cloned out of the environment, an owned intermediate rather
        // than a sub-term of the input, so it cannot be borrowed onto `work`.
        // Bounded by the depth of the BOUND VALUE and the environment's chain
        // length, never by the depth of the term being evaluated — the same
        // disposition as `substitute_deep_binding`.
        ExprInstance::EVarBody(evar) => {
            let v = evar.v.as_ref().ok_or(EvalError::MissingExprInstance)?;
            let p = resolve_var(v, env)?;
            vals.push(EvVal::Par(eval_with(&p, env, matcher)?));
            Ok(())
        }

        ExprInstance::ENotBody(ENot { p }) => {
            let inner = require_par_ref(p.as_ref())?;
            work.push(Step::Combine(EvKont::Not));
            work.push(Step::Descend(EvNode::Eval {
                par: inner,
                extract: true,
            }));
            Ok(())
        }

        ExprInstance::ENegBody(ENeg { p }) => {
            let inner = require_par_ref(p.as_ref())?;
            work.push(Step::Combine(EvKont::Neg));
            work.push(Step::Descend(EvNode::Eval {
                par: inner,
                extract: true,
            }));
            Ok(())
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) => push_binary(
            work,
            EvKont::BoolK { op: "&&", f: |a, b| a && b },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EOrBody(EOr { p1, p2 }) => push_binary(
            work,
            EvKont::BoolK { op: "||", f: |a, b| a || b },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),

        // ⚠ `eq` compares WHOLE Pars, so its operands are NOT extracted.
        ExprInstance::EEqBody(EEq { p1, p2 }) => push_binary(
            work,
            EvKont::EqK { expect_eq: true },
            p1.as_ref(),
            p2.as_ref(),
            false,
        ),
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => push_binary(
            work,
            EvKont::EqK { expect_eq: false },
            p1.as_ref(),
            p2.as_ref(),
            false,
        ),

        ExprInstance::ELtBody(ELt { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK { op: "<", interpret: |c| c == -1 },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::ELteBody(ELte { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK { op: "<=", interpret: |c| c <= 0 },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EGtBody(EGt { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK { op: ">", interpret: |c| c == 1 },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EGteBody(EGte { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK { op: ">=", interpret: |c| c >= 0 },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),

        ExprInstance::EPlusBody(EPlus { p1, p2 }) => push_binary(
            work,
            EvKont::IntK { op: "+", f: i64::checked_add },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => push_binary(
            work,
            EvKont::IntK { op: "-", f: i64::checked_sub },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EMultBody(EMult { p1, p2 }) => push_binary(
            work,
            EvKont::IntK { op: "*", f: i64::checked_mul },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => push_binary(
            work,
            EvKont::DivModK { op: "/", f: i64::checked_div },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => push_binary(
            work,
            EvKont::DivModK { op: "%", f: i64::checked_rem },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),

        ExprInstance::EMatchesBody(EMatches { target, pattern }) => {
            if !matcher.is_available() {
                // No oracle injected (the `eval` default). Refuse before
                // touching either operand — byte-identical to the
                // behaviour that predates this seam.
                return Err(EvalError::UnsupportedExpression {
                    kind: "EMatchesBody",
                });
            }
            // ⚠ The TARGET is evaluated; the PATTERN is not. Its free variables
            // are binders, and evaluating them would raise `UnboundVariable`
            // instead of matching.
            let t = require_par_ref(target.as_ref())?;
            let pat = require_par_ref(pattern.as_ref())?;
            work.push(Step::Combine(EvKont::MatchesK { pattern: pat }));
            work.push(Step::Descend(EvNode::Eval {
                par: t,
                extract: false,
            }));
            Ok(())
        }

        // Stubs — supported in the full reducer but not here yet.
        ExprInstance::EMethodBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMethodBody",
        }),
        ExprInstance::EPercentPercentBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPercentPercentBody",
        }),
        ExprInstance::EPlusPlusBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPlusPlusBody",
        }),
        ExprInstance::EMinusMinusBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMinusMinusBody",
        }),
        ExprInstance::EPathmapBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EPathmapBody",
        }),
        ExprInstance::EZipperBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EZipperBody",
        }),
    }
}

/// Post-order reassembly. Children were produced in push order, so they pop in
/// reverse: `p2` first, then `p1`.
fn run_ev_combine(
    kont: EvKont<'_>,
    matcher: &dyn SpatialMatch,
    vals: &mut Vec<EvVal>,
) -> Result<EvVal, EvalError> {
    let pop = |vals: &mut Vec<EvVal>| vals.pop().expect("eval_drive: value stack underflow");
    match kont {
        EvKont::ParK { par, n } => {
            let mut evaled: Vec<Par> = Vec::with_capacity(n);
            for _ in 0..n {
                evaled.push(pop(vals).into_par());
            }
            evaled.reverse();
            let mut acc = Par {
                sends: par.sends.clone(),
                receives: par.receives.clone(),
                news: par.news.clone(),
                matches: par.matches.clone(),
                unforgeables: par.unforgeables.clone(),
                bundles: par.bundles.clone(),
                connectives: par.connectives.clone(),
                conditionals: par.conditionals.clone(),
                exprs: Vec::new(),
                locally_free: par.locally_free.clone(),
                connective_used: par.connective_used,
            };
            for e in evaled {
                acc = concatenate(acc, e);
            }
            Ok(EvVal::Par(acc))
        }
        EvKont::Not => combine_not(pop(vals).into_inst()).map(EvVal::Par),
        EvKont::Neg => combine_neg(pop(vals).into_inst()).map(EvVal::Par),
        EvKont::BoolK { op, f } => {
            let v2 = pop(vals).into_inst();
            let v1 = pop(vals).into_inst();
            combine_bool(op, f, v1, v2).map(EvVal::Par)
        }
        EvKont::EqK { expect_eq } => {
            let rhs = pop(vals).into_par();
            let lhs = pop(vals).into_par();
            combine_eq(expect_eq, lhs, rhs).map(EvVal::Par)
        }
        EvKont::CmpK { op, interpret } => {
            let v2 = pop(vals).into_inst();
            let v1 = pop(vals).into_inst();
            combine_cmp(op, interpret, v1, v2).map(EvVal::Par)
        }
        EvKont::IntK { op, f } => {
            let v2 = pop(vals).into_inst();
            let v1 = pop(vals).into_inst();
            combine_int(op, f, v1, v2).map(EvVal::Par)
        }
        EvKont::DivModK { op, f } => {
            let v2 = pop(vals).into_inst();
            let v1 = pop(vals).into_inst();
            combine_div_or_mod(op, f, v1, v2).map(EvVal::Par)
        }
        EvKont::MatchesK { pattern } => {
            let evaled_target = pop(vals).into_par();
            Ok(EvVal::Par(par_with_bool(
                matcher.matches(&evaled_target, pattern),
            )))
        }
        EvKont::Extract => {
            let evaled = pop(vals).into_par();
            single_expr_instance(&evaled).map(EvVal::Inst)
        }
    }
}

// ---------------------------------------------------------------------------
// ★ THE LOOP IS NOT HERE
//
// It is `models::rust::rholang::drive::drive`, and this file's only remaining
// obligation to it is the [`Traversal`] impl above. What used to sit here — the
// LIFO loop, the two stacks, the deficit invariant `|V| + D + C - Sum arity == 1`
// and the final-configuration assertion — moved there verbatim in Stage F-1,
// where the sorter's and the codec's machines can share it. The invariant's
// derivation, the two-invariant split and the reason the final configuration is
// checked UNCONDITIONALLY are in that module's docs.
//
// One check genuinely CHANGED shape rather than moving, and it changed to
// admit this file: the old local assertion was "`descend` pushes its `Combine`
// first, then exactly `arity` children", which the `EvNode::Eval` arm's nested
// `[Extract, ParK{n}, child x n]` region violates. The driver's Invariant 1 is
// the right-to-left value-availability scan that accepts it and still rejects a
// continuation pushed with fewer children than its `arity()` claims.
// ---------------------------------------------------------------------------


// ===========================================================================
// The differential: worklist machine vs. the recursive oracle
// ===========================================================================

#[cfg(test)]
mod differential_eval_with {
    //! ★ The obligation, and the trap this module is shaped around.
    //!
    //! A binary `Combine` pops two children. If it pops them in the wrong
    //! order the result is the *same multiset* of operands — which is
    //! indistinguishable from correct for every COMMUTATIVE operator (`+`,
    //! `*`, `==`, `&&`). It is only visible on the non-commutative ones:
    //! `-`, `/`, `%`, `<`, `<=`, `>`, `>=`.
    //!
    //! So the corpus drives **both operand orders** for every operator, and —
    //! this is the part that makes the differential non-vacuous —
    //! [`swapping_operands_actually_changes_the_answer`] asserts that the
    //! non-commutative cases really do disagree under swap. Without it, an
    //! ordered-equality differential could pass on a corpus where order never
    //! mattered, looking rigorous while proving nothing. That is the same
    //! vacuity mode the gate subjects and the `eval_par_split` trace were
    //! hardened against.

    use super::*;
    use models::rhoapi::{EDiv, EMod};

    /// An **available** oracle, so `EMatchesBody` reaches its arm at all.
    ///
    /// ⚠ Without this the differential never exercised `EvKont::MatchesK`:
    /// every case ran under [`NoSpatialMatch`], whose `is_available()` is
    /// `false`, so both arms refused before touching either operand and the
    /// continuation's `arity` was never cross-checked against its pops.
    ///
    /// The verdict is a structural equality rather than the production spatial
    /// matcher — that matcher lives in `rholang`, which depends on this crate,
    /// so calling it here would be a dependency cycle (see [`SpatialMatch`]'s
    /// module docs). What the differential needs from an oracle is that BOTH
    /// arms consult the SAME pure function on the SAME evaluated target, which
    /// this satisfies; it is not a claim about what the real matcher decides.
    struct StructuralMatch;

    impl SpatialMatch for StructuralMatch {
        fn matches(&self, target: &Par, pattern: &Par) -> bool {
            target == pattern
        }
    }

    fn gint(i: i64) -> Par {
        par_with_int(i)
    }
    fn gbool(b: bool) -> Par {
        par_with_bool(b)
    }
    fn gstr(s: &str) -> Par {
        par_with_expr(Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        })
    }
    fn wrap(instance: ExprInstance) -> Par {
        par_with_expr(Expr {
            expr_instance: Some(instance),
        })
    }

    /// Every binary arm, at a given operand pair.
    fn binaries(a: Par, b: Par) -> Vec<(&'static str, Par)> {
        let (p1, p2) = (Some(a.clone()), Some(b.clone()));
        vec![
            ("+", wrap(ExprInstance::EPlusBody(EPlus { p1: p1.clone(), p2: p2.clone() }))),
            ("-", wrap(ExprInstance::EMinusBody(EMinus { p1: p1.clone(), p2: p2.clone() }))),
            ("*", wrap(ExprInstance::EMultBody(EMult { p1: p1.clone(), p2: p2.clone() }))),
            ("/", wrap(ExprInstance::EDivBody(EDiv { p1: p1.clone(), p2: p2.clone() }))),
            ("%", wrap(ExprInstance::EModBody(EMod { p1: p1.clone(), p2: p2.clone() }))),
            ("<", wrap(ExprInstance::ELtBody(ELt { p1: p1.clone(), p2: p2.clone() }))),
            ("<=", wrap(ExprInstance::ELteBody(ELte { p1: p1.clone(), p2: p2.clone() }))),
            (">", wrap(ExprInstance::EGtBody(EGt { p1: p1.clone(), p2: p2.clone() }))),
            (">=", wrap(ExprInstance::EGteBody(EGte { p1: p1.clone(), p2: p2.clone() }))),
            ("==", wrap(ExprInstance::EEqBody(EEq { p1: p1.clone(), p2: p2.clone() }))),
            ("!=", wrap(ExprInstance::ENeqBody(ENeq { p1: p1.clone(), p2: p2.clone() }))),
            ("&&", wrap(ExprInstance::EAndBody(EAnd { p1: p1.clone(), p2: p2.clone() }))),
            ("||", wrap(ExprInstance::EOrBody(EOr { p1, p2 }))),
        ]
    }

    /// Operand pairs, each used in BOTH orders.
    fn operand_pairs() -> Vec<(Par, Par)> {
        vec![
            (gint(7), gint(3)),
            (gint(3), gint(7)),
            (gint(-7), gint(3)),
            (gint(0), gint(5)),
            (gint(5), gint(0)),          // division by zero, one way only
            (gint(i64::MIN), gint(-1)),  // overflow on / and *
            (gbool(true), gbool(false)),
            (gbool(false), gbool(true)),
            (gstr("a"), gstr("b")),
            (gstr("b"), gstr("a")),
            (gint(1), gbool(true)),      // operator mismatch
            (gbool(true), gint(1)),      // mismatch, other way
        ]
    }

    /// Unary and nested shapes, including the `ENot` chain the probe drives.
    fn unary_corpus() -> Vec<Par> {
        let mut out = vec![
            wrap(ExprInstance::ENotBody(ENot { p: Some(gbool(true)) })),
            wrap(ExprInstance::ENotBody(ENot { p: Some(gint(1)) })),
            wrap(ExprInstance::ENegBody(ENeg { p: Some(gint(5)) })),
            wrap(ExprInstance::ENegBody(ENeg { p: Some(gint(i64::MIN)) })),
            wrap(ExprInstance::ENegBody(ENeg { p: Some(gbool(true)) })),
            Par::default(),
            gint(42),
        ];
        // Nested `!(!(…))` — `eval_with`'s own SCC, the shape the audit
        // measured at 21,584 B/level.
        let mut deep = gbool(true);
        for _ in 0..12 {
            deep = wrap(ExprInstance::ENotBody(ENot { p: Some(deep) }));
        }
        out.push(deep);
        out
    }

    // -----------------------------------------------------------------------
    // ★ DEEP and WIDE shapes
    //
    // ⚠ The depths here are bounded by the ORACLE, not by the machine. The
    // recursive twin costs ~21,584 bytes of native stack per level in debug, so
    // a differential that reached the machine's real ceiling would abort in the
    // arm it is comparing against. `DIFFERENTIAL_DEPTH` is chosen so the oracle
    // survives on the smallest thread stack this suite can be run with; the
    // claim that the machine has NO such ceiling is a different obligation and
    // is discharged by a different guard — `depth_gate`, below, which drives
    // both arms in child processes on an explicitly sized stack and requires
    // the recursive one to FAIL where the driver succeeds.
    // -----------------------------------------------------------------------

    /// Nesting depth for the deep differential shapes.
    ///
    /// 32 levels x ~25 KiB/level (the oracle's debug frame) is ~800 KiB, which
    /// clears a 2 MiB default thread stack with room for the harness. It is
    /// deliberately NOT a claim about the machine's ceiling.
    const DIFFERENTIAL_DEPTH: usize = 32;

    /// Sibling count for the wide differential shapes.
    ///
    /// The width axis is a `for` loop in BOTH arms (`par.exprs` in the oracle,
    /// the `ParK { n }` fold in the machine), so it is not stack-bounded on
    /// either side and can be large.
    const DIFFERENTIAL_WIDTH: usize = 256;

    fn eplus(a: Par, b: Par) -> Par {
        wrap(ExprInstance::EPlusBody(EPlus { p1: Some(a), p2: Some(b) }))
    }
    fn eminus(a: Par, b: Par) -> Par {
        wrap(ExprInstance::EMinusBody(EMinus { p1: Some(a), p2: Some(b) }))
    }
    fn enot(inner: Par) -> Par {
        wrap(ExprInstance::ENotBody(ENot { p: Some(inner) }))
    }
    fn ematches(target: Par, pattern: Par) -> Par {
        wrap(ExprInstance::EMatchesBody(EMatches {
            target: Some(target),
            pattern: Some(pattern),
        }))
    }

    /// `!(!(…(inner)…))`, `depth` levels, built ITERATIVELY so the builder is
    /// never itself the constraint.
    fn deep_nots(depth: usize, inner: Par) -> Par {
        let mut p = inner;
        for _ in 0..depth {
            p = enot(p);
        }
        p
    }

    /// `(((leaf - arg) - arg) - arg)` — the chain hangs off `p1`.
    ///
    /// ★ The LEFT spine is the one that matters for the interleaving the
    /// machine has to preserve: the recursive helpers run `p1`'s entire subtree
    /// and apply `single_expr_instance` to it BEFORE `p2` is touched, so a
    /// machine that evaluated both operands and then extracted would report
    /// `p2`'s error where the oracle reports `p1`'s.
    fn deep_left(depth: usize, leaf: Par, arg: Par) -> Par {
        let mut p = leaf;
        for _ in 0..depth {
            p = eminus(p, arg.clone());
        }
        p
    }

    /// `(arg - (arg - (arg - leaf)))` — the chain hangs off `p2`.
    fn deep_right(depth: usize, leaf: Par, arg: Par) -> Par {
        let mut p = leaf;
        for _ in 0..depth {
            p = eminus(arg.clone(), p);
        }
        p
    }

    /// A Par whose `exprs` slot holds `width` expressions, so the `ParK` fold
    /// over `concatenate` runs with `n = width`.
    fn wide_par(width: usize) -> Par {
        let mut exprs = Vec::with_capacity(width);
        for i in 0..width {
            exprs.push(Expr {
                expr_instance: Some(ExprInstance::GInt(i as i64)),
            });
        }
        Par { exprs, ..Par::default() }
    }

    /// A Par of `width` *operator* expressions, so the fold's children are
    /// themselves suspensions rather than leaves.
    fn wide_par_of_operators(width: usize) -> Par {
        let mut exprs = Vec::with_capacity(width);
        for i in 0..width {
            exprs.extend(eplus(gint(i as i64), gint(1)).exprs);
        }
        Par { exprs, ..Par::default() }
    }

    fn corpus() -> Vec<Par> {
        let mut out = unary_corpus();
        for (a, b) in operand_pairs() {
            for (_, term) in binaries(a, b) {
                out.push(term);
            }
        }
        // A Par whose `exprs` slot holds SEVERAL expressions, so the `ParK`
        // fold over `concatenate` is exercised with more than one child.
        out.push(Par {
            exprs: vec![
                Expr { expr_instance: Some(ExprInstance::GInt(1)) },
                Expr { expr_instance: Some(ExprInstance::GInt(2)) },
                Expr { expr_instance: Some(ExprInstance::GInt(3)) },
            ],
            ..Par::default()
        });

        // ---- DEEP ----
        let d = DIFFERENTIAL_DEPTH;
        // A deep unary chain that SUCCEEDS.
        out.push(deep_nots(d, gbool(true)));
        // A deep unary chain that FAILS at the very bottom: the same error must
        // travel back up through `d` suspensions in both arms.
        out.push(deep_nots(d, gint(1)));
        // Deep left and right binary spines, succeeding.
        out.push(deep_left(d, gint(0), gint(1)));
        out.push(deep_right(d, gint(0), gint(1)));
        // Deep left spine whose LEAF is the wrong type: the mismatch is raised
        // at the innermost operand, `d` levels down.
        out.push(deep_left(d, gbool(true), gint(1)));
        // A deep p1 beside a wrong-typed p2 at the TOP: the oracle checks p1
        // first, so the error that surfaces is p1's — a deep one — not this
        // shallow one.
        out.push(eminus(deep_left(d, gbool(true), gint(1)), gbool(false)));
        // A deep chain under `!`, so `Extract` sits above `ParK` at every level.
        out.push(enot(deep_left(d, gint(0), gint(1))));
        // A deep chain inside an `EMatches` TARGET (the pattern is never
        // evaluated, so it stays shallow).
        out.push(ematches(deep_left(d, gint(0), gint(1)), gint(-(d as i64))));
        out.push(ematches(deep_left(d, gint(0), gint(1)), gint(999)));

        // ---- WIDE ----
        let w = DIFFERENTIAL_WIDTH;
        out.push(wide_par(w));
        out.push(wide_par_of_operators(w));
        // A wide Par as an OPERAND: it evaluates to `w` exprs, so
        // `single_expr_instance` refuses it — the same refusal, at the same
        // operand, in both arms.
        out.push(eplus(wide_par(w), gint(1)));
        out.push(eplus(gint(1), wide_par(w)));
        // Wide AND deep: `w` siblings each carrying a `d`-deep chain.
        let mut wide_deep = Vec::with_capacity(w);
        for i in 0..w {
            wide_deep.extend(deep_left(d, gint(i as i64), gint(1)).exprs);
        }
        out.push(Par { exprs: wide_deep, ..Par::default() });

        // ---- EMatches, which only has an arm when an oracle is AVAILABLE ----
        out.push(ematches(gint(1), gint(1)));
        out.push(ematches(gint(1), gint(2)));
        out.push(ematches(eplus(gint(1), gint(1)), gint(2)));
        out.push(ematches(eplus(gint(1), gbool(true)), gint(2)));
        out.push(enot(ematches(gint(1), gint(1))));
        out
    }

    /// Compare the two arms on one term under one oracle.
    fn agree_on(i: usize, term: &Par, matcher: &dyn SpatialMatch, oracle_name: &str) {
        let env: Env<Par> = Env::new();
        let machine = eval_with(term, &env, matcher);
        let oracle = eval_with_recursive(term, &env, matcher);
        match (&machine, &oracle) {
            (Ok(a), Ok(b)) => assert_eq!(
                a, b,
                "corpus[{i}] under {oracle_name}: the machine and the oracle produced \
                 different VALUES"
            ),
            (Err(a), Err(b)) => assert_eq!(
                format!("{a:?}"),
                format!("{b:?}"),
                "corpus[{i}] under {oracle_name}: the machine and the oracle produced \
                 different ERRORS. The five binop helpers interleave `eval` and \
                 `single_expr_instance` PER OPERAND, so a machine that evaluated both \
                 children before checking either would report the second operand's error \
                 where the recursive form reports the first's."
            ),
            _ => panic!(
                "corpus[{i}] under {oracle_name}: one side succeeded and the other \
                 failed.\n  machine = {machine:?}\n  oracle  = {oracle:?}"
            ),
        }
    }

    #[test]
    fn the_machine_agrees_with_the_recursive_oracle_on_every_term() {
        for (i, term) in corpus().iter().enumerate() {
            agree_on(i, term, &NoSpatialMatch, "NoSpatialMatch");
        }
    }

    /// ★ The same corpus with an **available** oracle, which is the only
    /// configuration in which `EMatchesBody` has an arm at all.
    ///
    /// Under [`NoSpatialMatch`] both sides refuse before touching either
    /// operand, so `EvKont::MatchesK` is never built and its `arity` is never
    /// cross-checked against its pops. Running the corpus twice is what makes
    /// that continuation covered.
    #[test]
    fn the_machine_agrees_with_the_recursive_oracle_under_an_available_oracle() {
        for (i, term) in corpus().iter().enumerate() {
            agree_on(i, term, &StructuralMatch, "StructuralMatch");
        }
    }

    /// ★ ANTI-VACUITY FOR THE TEST ABOVE.
    ///
    /// `the_machine_agrees_…_under_an_available_oracle` would pass unchanged if
    /// the available oracle happened never to be consulted. This asserts that
    /// the corpus really does reach the `EMatches` arm and really does get
    /// *both* verdicts out of it — so a `MatchesK` that popped the wrong number
    /// of values, or consulted the wrong term, had somewhere to show up.
    #[test]
    fn the_available_oracle_is_actually_consulted_and_decides_both_ways() {
        let env: Env<Par> = Env::new();
        let mut trues = 0usize;
        let mut falses = 0usize;
        for term in corpus() {
            let Ok(out) = eval_with(&term, &env, &StructuralMatch) else {
                continue;
            };
            match single_expr_instance(&out) {
                Ok(ExprInstance::GBool(true)) => trues += 1,
                Ok(ExprInstance::GBool(false)) => falses += 1,
                _ => {}
            }
        }
        // `ematches(gint(1), gint(1))` is true, `ematches(gint(1), gint(2))` is
        // false, and the `!`-wrapped one flips a true to a false — so both
        // verdicts are produced by the `EMatches` arm specifically, not only by
        // the comparison operators elsewhere in the corpus.
        let by_matches_true = eval_with(&ematches(gint(1), gint(1)), &env, &StructuralMatch)
            .and_then(|p| single_expr_instance(&p));
        let by_matches_false = eval_with(&ematches(gint(1), gint(2)), &env, &StructuralMatch)
            .and_then(|p| single_expr_instance(&p));
        assert_eq!(
            by_matches_true.expect("an available oracle decides `1 matches 1`"),
            ExprInstance::GBool(true),
            "VACUOUS: the `EMatches` arm did not produce a positive verdict, so the \
             available-oracle differential proves nothing about `EvKont::MatchesK`."
        );
        assert_eq!(
            by_matches_false.expect("an available oracle decides `1 matches 2`"),
            ExprInstance::GBool(false),
            "VACUOUS: the `EMatches` arm did not produce a negative verdict."
        );
        assert!(
            trues > 0 && falses > 0,
            "VACUOUS: the corpus produced {trues} true and {falses} false boolean results \
             under the available oracle; both must occur for the differential to \
             distinguish a verdict that was inverted or read off the wrong operand."
        );
    }

    /// ★ THE ANTI-VACUITY GUARD FOR THE TEST ABOVE.
    ///
    /// An ordered-equality differential is satisfied by a corpus in which
    /// operand order never mattered. This asserts that the non-commutative
    /// operators really do disagree when their operands are swapped, so a
    /// reversed `Combine` pop would have been caught.
    #[test]
    fn swapping_operands_actually_changes_the_answer() {
        let env: Env<Par> = Env::new();
        let mut disagreements = 0usize;
        for (op, term) in binaries(gint(7), gint(3)) {
            let (_, swapped) = binaries(gint(3), gint(7))
                .into_iter()
                .find(|(o, _)| *o == op)
                .expect("same operator set both ways");
            let a = eval_with(&term, &env, &NoSpatialMatch);
            let b = eval_with(&swapped, &env, &NoSpatialMatch);
            if format!("{a:?}") != format!("{b:?}") {
                disagreements += 1;
            }
        }
        // `-`, `/`, `%`, `<`, `<=`, `>`, `>=` all disagree on (7, 3) vs (3, 7).
        assert!(
            disagreements >= 7,
            "VACUOUS DIFFERENTIAL: only {disagreements} of the binary operators \
             distinguish their operand order on this corpus, so \
             `the_machine_agrees_with_the_recursive_oracle_on_every_term` could pass \
             with a reversed `Combine` pop and prove nothing."
        );
    }

    // -----------------------------------------------------------------------
    // ⚠★ DELETED HERE, ON PURPOSE: `the_machine_survives_a_depth_the_oracle_
    // could_not`.
    //
    // It was a `#[test]` that built a 20,000-level `ENot` chain, ran
    // `eval_with` on it, and asserted the boolean came back. Three things were
    // wrong with it, and the third is the one that matters:
    //
    // 1. ITS NAME CLAIMED A COMPARISON IT NEVER MADE. It never called
    //    `eval_with_recursive`. "the oracle could not" was an arithmetic
    //    argument in a comment — 20,000 x 21,584 B/level ~ 412 MiB — derived
    //    from a constant measured in a different harness, not an observation
    //    made here. A test may not assert in its name what it does not execute.
    //
    // 2. IT RAN ON WHATEVER STACK THE HARNESS GAVE IT. No `stack_size`, so
    //    `RUST_MIN_STACK`, `ulimit -s` and libtest's thread default silently
    //    decided the verdict. A regression to a Theta(depth) form with a small
    //    per-level cost would have passed it.
    //
    // 3. ★ AND IT ONLY PASSED BECAUSE OF ONE CALL IN THE FIXTURE. Its final
    //    statement was `par_children::dismantle(deep)` — which NO production
    //    caller makes. `drop_in_place::<Par>` is itself Theta(depth), so
    //    without that call the term's DESTRUCTOR, not `eval_with`, sets the
    //    ceiling. This is the same shape that let the big gate's own headline
    //    subject certify source depth 100,000 while a 4,415-deep term aborted
    //    the node: *the distance between "100,000 is fine" and "4,415 aborts
    //    the node" is one call in a fixture.*
    //
    // The property it was named for is REAL and is now gated properly, by
    // [`super::depth_gate`]: both arms, in child processes, on an explicitly
    // sized stack, with the recursive arm REQUIRED to fail where the driver
    // survives — and with the destructor's contribution recorded as its own
    // measured rung (`driver_and_drop`) instead of silently excluded. Deleting
    // this test rather than leaving it beside that gate is deliberate: two
    // statements of one property do not stay equal, and the weaker one is the
    // one a reader reaches first.
    // -----------------------------------------------------------------------
}

// ===========================================================================
// The DEPTH guard: the driver survives what the oracle cannot
// ===========================================================================

#[cfg(test)]
mod depth_gate {
    //! ★ The property this whole conversion exists for, watched RED on every
    //! run.
    //!
    //! ## Why it is a child process and not an assertion
    //!
    //! A stack overflow is a `SIGSEGV`, not a catchable error: there is no
    //! `Result` to inspect and `catch_unwind` does not see it. And this
    //! workspace forbids a test that *expects a panic* outright, because a
    //! panic that fails to unwind aborts printing nothing. So the subject runs
    //! in a **child process**, on a thread with an **explicit `stack_size`**,
    //! and the parent reads the child's **exit status**. That is the same
    //! mechanism `rholang/tests/stack_depth_gate.rs` uses for the whole
    //! Θ(depth) family, and it is used here for the same reason.
    //!
    //! ## Why the stack size is explicit
    //!
    //! Neither `RUST_MIN_STACK` nor `ulimit -s` can then mask a regression: the
    //! thread gets exactly [`GATE_STACK`] bytes whatever the environment says.
    //!
    //! ## What makes it non-vacuous
    //!
    //! At [`SHALLOW`] **both** arms must survive on the same stack — so a child
    //! that failed for a reason unrelated to depth (a missing binary, a bad
    //! argument, a panic in the term builder) fails the gate immediately and
    //! cannot be mistaken for the property. Only then does the gate require the
    //! recursive arm to fail at [`DEEP`] and the driver to survive it. That
    //! middle assertion is RED on purpose, permanently: a driver that silently
    //! reverted to recursion would make it green and the gate would fail.
    //!
    //! ## ★★ The destructor is MEASURED here, not excluded here
    //!
    //! `run_arm` finishes with [`dismantle`], because the subject is
    //! `eval_with`'s native stack and `drop_in_place::<Par>` is a **different**
    //! Θ(depth) traversal (gated separately, as `par_drop`, in
    //! `rholang/tests/stack_depth_gate.rs`). Without that call every reading
    //! would be `max(eval_with, drop_in_place::<Par>)` — the destructor's
    //! number wearing the driver's name. One traversal per number.
    //!
    //! ⚠ But a fixture that ends in `dismantle` is exactly how a guard comes to
    //! certify a ceiling **no production caller has**: the deleted smoke test
    //! this module replaces passed at 20,000 levels for that reason and for no
    //! other, and the big gate's own headline subject once certified source
    //! depth 100,000 while a 4,415-deep term aborted the node. So the
    //! assumption is not left implicit. [`DEEP_DROP`] drives a third arm,
    //! `driver_and_drop`, which lets the term fall out of scope normally, and
    //! the gate asserts that it **fails** where `driver` succeeds. The claim
    //! this module makes is therefore exact and bounded:
    //!
    //! | claim | rung | verdict |
    //! |---|---|---|
    //! | `eval_with`'s own native stack is O(1) in nesting depth | `driver` @ [`DEEP_DROP`] | survives 1 MiB |
    //! | the recursive twin's is not | `recursive` @ [`DEEP`] | aborts 1 MiB |
    //! | **the composition with the term's destructor is NOT depth-independent** | `driver_and_drop` @ [`DEEP_DROP`] | aborts 1 MiB |
    //!
    //! The third row is a tripwire, not a defect being ratified: it is the
    //! statement that a caller holding a deep term still owes it an iterative
    //! teardown, and it is written as an executed assertion so that a future
    //! `Drop`-side conversion turns it red and gets read.

    use super::*;
    use models::rust::rholang::par_children::dismantle;
    use std::process::{Command, Stdio};

    /// Which implementation the child runs.
    const GATE_ARM: &str = "RHO_PURE_EVAL_GATE_ARM";
    /// `ENot` nesting levels.
    const GATE_DEPTH: &str = "RHO_PURE_EVAL_GATE_DEPTH";
    /// The child thread's stack, in bytes.
    const GATE_STACK_VAR: &str = "RHO_PURE_EVAL_GATE_STACK";

    /// The child entry point's full test path, as libtest's `--exact` wants it.
    const CHILD: &str = "eval::depth_gate::gate_child";

    /// 1 MiB. Large enough for the harness, the term builder and the driver's
    /// own frame; far too small for `DEEP` levels of recursion in EITHER
    /// profile (the oracle measures 21,584 B/level debug and 3,359 release, so
    /// `DEEP` needs ~84 MiB / ~13 MiB).
    const GATE_STACK: usize = 1024 * 1024;

    /// The rung at which both arms must work — the gate's own control.
    const SHALLOW: usize = 4;

    /// The rung at which the recursive arm must fail and the driver must not.
    ///
    /// Chosen so the recursive requirement holds with more than an order of
    /// magnitude to spare in the CHEAPER (release) profile, which is the
    /// binding one: 4,096 x 3,359 B is ~13.1 MiB against a 1 MiB stack.
    const DEEP: usize = 4_096;

    /// The rung at which the DESTRUCTOR's contribution becomes visible on this
    /// stack in **both** profiles, so the composition assertion is
    /// profile-independent by construction rather than by a lucky constant.
    ///
    /// `drop_in_place::<Par>` measures ~470 B/level in debug and ~32 B/level at
    /// `-O2` (`rholang/tests/stack_depth_gate.rs`, subject `par_drop`; the
    /// release figure is the small one because the drop glue holds a tail
    /// pointer and the saved registers and nothing else). The RELEASE number is
    /// the binding one: 131,072 x 32 B is ~4.0 MiB against a 1 MiB stack, a 4x
    /// margin, and in debug it is ~59 MiB, a 59x margin.
    ///
    /// It is also the rung the `driver` arm is asserted to SURVIVE, which makes
    /// it a strictly stronger statement than the 20,000-on-an-ambient-stack
    /// claim the deleted smoke test made.
    const DEEP_DROP: usize = 131_072;

    fn nots(depth: usize) -> Par {
        let mut p = par_with_bool(true);
        for _ in 0..depth {
            p = par_with_expr(Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(p) })),
            });
        }
        p
    }

    /// Run one arm at one depth, on whatever stack the caller set up.
    fn run_arm(arm: &str, depth: usize) {
        let deep = nots(depth);
        let env: Env<Par> = Env::new();
        let out = match arm {
            "recursive" => eval_with_recursive(&deep, &env, &NoSpatialMatch),
            "driver" | "driver_and_drop" => eval_with(&deep, &env, &NoSpatialMatch),
            other => panic!("depth_gate: unknown {GATE_ARM}={other:?}"),
        }
        .expect("depth_gate: the chain evaluates");
        assert_eq!(
            single_expr_instance(&out).expect("depth_gate: a single value"),
            // An even number of negations of `true` is `true`.
            ExprInstance::GBool(depth % 2 == 0),
            "depth_gate: {depth} negations of `true` under arm {arm:?}"
        );

        if arm == "driver_and_drop" {
            // ★ THE COMPOSITION ARM. The term falls out of scope normally, so
            // this reading is `max(eval_with, drop_in_place::<Par>)` — which is
            // the whole point of it. See the module docs: the gate asserts this
            // arm FAILS at `DEEP_DROP`, so the destructor's contribution is a
            // recorded number instead of a fixture's silent exclusion.
            drop(deep);
            return;
        }

        // ⚠ Tear the term down ITERATIVELY, so this reading is `eval_with`'s
        // and nothing else. `drop_in_place::<Par>` is a SEPARATE Θ(depth)
        // traversal (~470 B/level debug, ~32 release) with its own tripwire
        // (`par_drop`, in `rholang/tests/stack_depth_gate.rs`); leaving it in
        // the measurement would put the destructor's number under the driver's
        // name. One traversal per number.
        dismantle(deep);
    }

    /// The child entry point.
    ///
    /// ⚠ A NO-OP when its environment is absent. `cargo test --
    /// --include-ignored` and `cargo nextest run --run-ignored all` execute
    /// every `#[ignore]`d test, so a child that *required* its environment
    /// would fail the suite for a reason unrelated to the property. Skipping is
    /// correct here precisely because this test is a mechanism: the assertions
    /// live in its caller.
    #[test]
    #[ignore = "child process of the eval depth gate; driven via RHO_PURE_EVAL_GATE_ARM"]
    fn gate_child() {
        let Ok(arm) = std::env::var(GATE_ARM) else {
            println!("gate_child: no {GATE_ARM} — not a child invocation, nothing to do");
            return;
        };
        let depth: usize = std::env::var(GATE_DEPTH)
            .expect("GATE_DEPTH must accompany GATE_ARM")
            .parse()
            .expect("GATE_DEPTH must be an integer");
        let stack: usize = std::env::var(GATE_STACK_VAR)
            .expect("GATE_STACK must accompany GATE_ARM")
            .parse()
            .expect("GATE_STACK must be an integer");

        std::thread::Builder::new()
            .stack_size(stack)
            .name("eval-depth-gate".to_string())
            .spawn(move || run_arm(&arm, depth))
            .expect("depth_gate: failed to spawn")
            .join()
            .expect("depth_gate: the subject panicked");
    }

    /// Run one probe point in a child process. `true` iff it survived.
    fn runs_within(arm: &str, depth: usize, stack: usize) -> bool {
        let exe = std::env::current_exe().expect("depth_gate: current_exe");
        Command::new(exe)
            .args(["--ignored", "--exact", CHILD])
            .env(GATE_ARM, arm)
            .env(GATE_DEPTH, depth.to_string())
            .env(GATE_STACK_VAR, stack.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("depth_gate: failed to run the child")
            .success()
    }

    #[test]
    fn the_driver_survives_a_depth_that_overflows_the_recursive_oracle() {
        // ---- the control: at a shallow depth, the harness works for BOTH ----
        assert!(
            runs_within("recursive", SHALLOW, GATE_STACK),
            "CONTROL FAILED: the recursive arm did not survive {SHALLOW} levels on a \
             {GATE_STACK}-byte stack. Nothing below this line means anything until it \
             does — the child is failing for a reason that is not depth."
        );
        assert!(
            runs_within("driver", SHALLOW, GATE_STACK),
            "CONTROL FAILED: the driver arm did not survive {SHALLOW} levels on a \
             {GATE_STACK}-byte stack."
        );

        // ---- the RED half: the oracle must NOT survive ----
        assert!(
            !runs_within("recursive", DEEP, GATE_STACK),
            "THE GUARD HAS GONE VACUOUS: the recursive oracle survived {DEEP} levels on a \
             {GATE_STACK}-byte stack, so this gate is no longer distinguishing a \
             heap-bounded machine from a stack-bounded one. Either the per-level cost \
             collapsed (check `rholang/tests/stack_depth_gate.rs`'s measured constants) or \
             the child stopped running the arm it was asked for."
        );

        // ---- the property ----
        assert!(
            runs_within("driver", DEEP, GATE_STACK),
            "REGRESSION: `eval_with` did not survive {DEEP} levels on a {GATE_STACK}-byte \
             stack. Its native stack must be O(1) in nesting depth — the recursion belongs \
             in `drive`'s heap work stack. Something has re-introduced a native \
             re-entry: check `descend_ev_expr`'s `EVarBody` arm (the one deliberate nested \
             drive, bounded by the ENVIRONMENT rather than by the term) and any new arm \
             that calls `eval_with` directly."
        );
        assert!(
            runs_within("driver", DEEP_DROP, GATE_STACK),
            "REGRESSION: `eval_with` did not survive {DEEP_DROP} levels on a \
             {GATE_STACK}-byte stack. This rung is {DEEP_DROP} deep specifically so the \
             claim is not one a fixture could have manufactured — see the module docs."
        );

        // ---- the COMPOSITION, recorded rather than excluded ----
        //
        // ★ This is the assertion the deleted smoke test needed and did not
        // have. Its fixture ended in `dismantle`, so it certified a ceiling no
        // production caller has; here the arm that does NOT dismantle is driven
        // explicitly and its failure is the record of what the destructor
        // still costs.
        assert!(
            !runs_within("driver_and_drop", DEEP_DROP, GATE_STACK),
            "THE COMPOSITION TRIPWIRE HAS FLIPPED: `eval_with` followed by letting a \
             {DEEP_DROP}-level term fall out of scope SURVIVED a {GATE_STACK}-byte stack. \
             That is a change worth reading, not a failure to paper over — it means \
             `drop_in_place::<Par>` is no longer Θ(depth) on this shape (an `impl Drop for \
             Par` landed, or the drop glue changed). Re-measure `par_drop` in \
             `rholang/tests/stack_depth_gate.rs`, then retire this assertion DELIBERATELY \
             rather than by loosening it: with the destructor bounded, `dismantle` in \
             `run_arm` stops being load-bearing and every caller holding a deep term stops \
             owing it an iterative teardown."
        );
    }
}
