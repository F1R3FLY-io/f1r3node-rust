use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMatches, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr,
    Par, Var,
};

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
    eval_drive(EvWork::Eval { par, extract: false }, env, matcher).map(|v| match v {
        EvVal::Par(p) => p,
        EvVal::Inst(_) => unreachable!("eval_drive: a non-extracting root must yield a Par"),
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
// Leg-2 Stage E: the explicit-worklist machine for `eval_with`
// ===========================================================================
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

/// A pending unit of work. Every reference borrows the input term (`'t`).
enum EvWork<'t> {
    /// `eval_with(par)`, optionally followed immediately by
    /// `single_expr_instance` — the per-operand check the helpers interleave.
    Eval { par: &'t Par, extract: bool },
    /// `eval_expr_to_par(expr)`.
    Expr(&'t Expr),
    Combine(EvKont<'t>),
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

impl EvKont<'_> {
    /// The number of values this continuation pops.
    ///
    /// ★ Deliberately duplicates the pop counts in [`run_ev_combine`], so the
    /// deficit invariant can cross-check them. Exhaustive, no `_` arm.
    fn arity(&self) -> usize {
        match self {
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
fn push_binary<'t>(
    work: &mut Vec<EvWork<'t>>,
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
    work.push(EvWork::Combine(kont));
    work.push(EvWork::Eval { par: b, extract });
    work.push(EvWork::Eval { par: a, extract });
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
fn descend_ev_expr<'t>(
    expr: &'t Expr,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
    work: &mut Vec<EvWork<'t>>,
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
            work.push(EvWork::Combine(EvKont::Not));
            work.push(EvWork::Eval {
                par: inner,
                extract: true,
            });
            Ok(())
        }

        ExprInstance::ENegBody(ENeg { p }) => {
            let inner = require_par_ref(p.as_ref())?;
            work.push(EvWork::Combine(EvKont::Neg));
            work.push(EvWork::Eval {
                par: inner,
                extract: true,
            });
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
            work.push(EvWork::Combine(EvKont::MatchesK { pattern: pat }));
            work.push(EvWork::Eval {
                par: t,
                extract: false,
            });
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

/// The single LIFO loop. Native stack is `O(1)`; the recursion lives in `work`.
///
/// A `?` abort discards `work` and `vals`, identical to the recursive form's
/// `?`, which discards its pending frames. Nothing here charges, so there is no
/// cost state to unwind.
///
/// The deficit invariant `|V| + D + C − Σ arity(k) == 1` is asserted at the head
/// of every iteration, exactly as in the sorter's machine; see
/// `models/src/rust/rholang/sorter/sort_drive.rs` for its derivation and for
/// why the shorter-looking `|V| + D == 1 + Σ arity` is off by `C`.
fn eval_drive(
    root: EvWork<'_>,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
) -> Result<EvVal, EvalError> {
    let mut work: Vec<EvWork<'_>> = Vec::with_capacity(32);
    let mut vals: Vec<EvVal> = Vec::with_capacity(32);
    work.push(root);

    let mut descends: usize = 1;
    let mut combines: usize = 0;
    let mut sum_arity: usize = 0;

    loop {
        debug_assert_eq!(
            vals.len() + descends + combines,
            1 + sum_arity,
            "eval_drive: DEFICIT INVARIANT VIOLATED (|vals|={}, descends={}, combines={}, \
             Sum arity={})",
            vals.len(),
            descends,
            combines,
            sum_arity
        );
        let Some(w) = work.pop() else { break };
        match w {
            EvWork::Combine(kont) => {
                combines -= 1;
                sum_arity -= kont.arity();
                let v = run_ev_combine(kont, matcher, &mut vals)?;
                vals.push(v);
            }
            EvWork::Eval { par, extract } => {
                descends -= 1;
                // `eval_with`'s own body: a `ParK` continuation over the Par's
                // exprs. `extract` rides on the continuation's RESULT, so it is
                // applied when this whole sub-evaluation completes — which is
                // the point the recursive helpers apply it.
                // `Extract` is pushed FIRST so it pops LAST — after `ParK`
                // has produced this sub-evaluation's `Par`.
                if extract {
                    work.push(EvWork::Combine(EvKont::Extract));
                    combines += 1;
                    sum_arity += 1;
                }
                work.push(EvWork::Combine(EvKont::ParK {
                    par,
                    n: par.exprs.len(),
                }));
                for expr in par.exprs.iter().rev() {
                    work.push(EvWork::Expr(expr));
                }
                combines += 1;
                sum_arity += par.exprs.len();
                descends += par.exprs.len();
            }
            EvWork::Expr(expr) => {
                descends -= 1;
                let before_v = vals.len();
                let before_w = work.len();
                descend_ev_expr(expr, env, matcher, &mut work, &mut vals)?;
                let pushed = work.len() - before_w;
                if pushed > 0 {
                    let arity = match &work[before_w] {
                        EvWork::Combine(k) => k.arity(),
                        _ => unreachable!("descend_ev_expr must push its Combine first"),
                    };
                    debug_assert_eq!(pushed - 1, arity);
                    combines += 1;
                    sum_arity += arity;
                    descends += pushed - 1;
                } else {
                    debug_assert_eq!(vals.len(), before_v + 1, "a leaf must produce one value");
                }
            }
        }
    }

    debug_assert_eq!(vals.len(), 1, "eval_drive: exactly one value must remain");
    Ok(vals.pop().expect("eval_drive: exactly one value must remain"))
}


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
        out
    }

    #[test]
    fn the_machine_agrees_with_the_recursive_oracle_on_every_term() {
        let env: Env<Par> = Env::new();
        for (i, term) in corpus().iter().enumerate() {
            let machine = eval_with(term, &env, &NoSpatialMatch);
            let oracle = eval_with_recursive(term, &env, &NoSpatialMatch);
            match (&machine, &oracle) {
                (Ok(a), Ok(b)) => assert_eq!(
                    a, b,
                    "corpus[{i}]: the machine and the oracle produced different VALUES"
                ),
                (Err(a), Err(b)) => assert_eq!(
                    format!("{a:?}"),
                    format!("{b:?}"),
                    "corpus[{i}]: the machine and the oracle produced different ERRORS. \
                     The five binop helpers interleave `eval` and `single_expr_instance` \
                     PER OPERAND, so a machine that evaluated both children before \
                     checking either would report the second operand's error where the \
                     recursive form reports the first's."
                ),
                _ => panic!(
                    "corpus[{i}]: one side succeeded and the other failed.\n  machine = \
                     {machine:?}\n  oracle  = {oracle:?}"
                ),
            }
        }
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

    /// The machine must survive an expression nesting the oracle could not.
    #[test]
    fn the_machine_survives_a_depth_the_oracle_could_not() {
        // 20,000 `ENot` levels: at the measured 21,584 B/level the recursive
        // form would need ~412 MiB.
        let mut deep = gbool(true);
        for _ in 0..20_000 {
            deep = wrap(ExprInstance::ENotBody(ENot { p: Some(deep) }));
        }
        let env: Env<Par> = Env::new();
        let out = eval_with(&deep, &env, &NoSpatialMatch).expect("deep ENot chain evaluates");
        assert_eq!(
            single_expr_instance(&out).expect("a single value"),
            ExprInstance::GBool(true),
            "20,000 negations of `true` is `true`"
        );
    }
}
