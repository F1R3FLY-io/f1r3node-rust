#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMatches, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr,
    Par, Var,
};
#[cfg(test)]
use models::rust::par_bag::ParBagVisitError;
use models::rust::par_bag::ParComponentItem;
#[cfg(test)]
use models::rust::par_construction::ParConstruction;
use models::rust::rholang::drive::{drive, Outcome, Step, Traversal};

use crate::decidable::{decode_encoded_enot_terminal, EncodedEnotTerminal};
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
/// `models/tests/support/score_tree_oracle.rs` keeps the recursive comparator.
#[cfg(test)]
pub(crate) fn eval_with_recursive(
    par: &Par,
    env: &Env<Par>,
    matcher: &dyn SpatialMatch,
) -> Result<Par, EvalError> {
    let mut acc = par.with_exprs(Vec::new());
    let visited = par.try_visit_occurrences::<Expr, EvalError>(|expr| {
        let evaled = eval_expr_to_par_recursive(expr, env, matcher)?;
        acc = concatenate(std::mem::take(&mut acc), evaled)?;
        Ok(())
    });
    match visited {
        Ok(()) => {}
        Err(ParBagVisitError::Visitor(error)) => return Err(error),
        Err(ParBagVisitError::Codec(error)) => {
            panic!("Par invariant: expression endpoints are canonical: {error}")
        }
    }

    Ok(acc)
}

fn concatenate(a: Par, b: Par) -> Result<Par, EvalError> { a.parallel(b).map_err(EvalError::from) }

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
    models::par_from_default! {
        exprs: vec![e],
        ..Par::default()
    }
}

fn par_with_bool(b: bool) -> Par {
    par_with_expr(Expr {
        expr_instance: Some(ExprInstance::GBool(b)),
    })
}

#[cfg(test)]
pub(crate) fn planned_nots(depth: usize, terminal: &Par) -> Par {
    let mut arena = ParConstruction::new();
    let mut current = arena.import_par(terminal).expect("import ENot terminal");
    let shell = Expr {
        expr_instance: Some(ExprInstance::ENotBody(ENot {
            p: Some(Par::default()),
        })),
    };
    for _ in 0..depth {
        let component = arena
            .expr_component(&shell, &[current], false)
            .expect("plan ENot");
        current = arena
            .singleton_component(component, Vec::new(), false)
            .expect("plan singleton ENot");
    }
    arena.finish(current).expect("finish ENot spine")
}

#[cfg(test)]
pub(crate) fn assert_planned_nots(par: &Par, depth: usize, terminal: &Par) {
    assert!(par.locally_free().is_empty());
    assert!(!par.connective_used());
    assert_eq!(par.cardinality(), 1);
    assert_eq!(par.family_cardinality::<Expr>(), 1);
    let terminal = terminal
        .protobuf_bytes()
        .expect("terminal protobuf encoding must succeed");
    let mut visits = 0usize;
    par.visit_encoded::<Expr>(|entry| {
        visits += 1;
        assert_eq!(entry.multiplicity().get(), 1);
        let (observed_depth, observed_terminal) = entry
            .canonical_unary_enot_parts()
            .expect("exact unary ENot spine");
        assert_eq!(observed_depth, depth);
        assert_eq!(observed_terminal, terminal);
    });
    assert_eq!(visits, 1);
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
    if par.cardinality() != 1 || par.family_cardinality::<Expr>() != 1 {
        return Err(EvalError::NotASingleValue {
            actual: Box::new(par.clone()),
        });
    }
    par.first::<Expr>()
        .expect("Par invariant: expression endpoints are canonical")
        .expect("the cardinality check proved one Expr")
        .0
        .expr_instance
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

    // Keep borrowed and owned work in homogeneous stacks. A single mixed enum
    // would be as wide as `Expr` (currently 504 bytes) even for borrowed
    // entries; boxing each PathMap-decoded endpoint would instead add an
    // allocation per distinct expression. Four contiguous stacks avoid both
    // costs, and visitation order is unobservable for this pure predicate.
    let mut borrowed_pars = vec![par];
    let mut owned_pars = Vec::<Par>::new();
    let mut borrowed_exprs = Vec::<&Expr>::new();
    let mut owned_exprs = Vec::<Expr>::new();

    loop {
        if let Some(expr) = borrowed_exprs.pop() {
            match expr.expr_instance.as_ref() {
                Some(GDouble(bits)) if f64::from_bits(*bits).is_nan() => return true,
                Some(EListBody(list)) => {
                    borrowed_pars.extend(list.ps.iter());
                }
                Some(ETupleBody(tuple)) => {
                    borrowed_pars.extend(tuple.ps.iter());
                }
                Some(ESetBody(set)) => {
                    borrowed_pars.extend(set.ps.iter());
                }
                Some(EMapBody(map)) => {
                    for pair in &map.kvs {
                        if let Some(key) = pair.key.as_ref() {
                            borrowed_pars.push(key);
                        }
                        if let Some(value) = pair.value.as_ref() {
                            borrowed_pars.push(value);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        if let Some(par) = borrowed_pars.pop() {
            par.try_visit_components::<Expr, std::convert::Infallible>(|expr, _multiplicity| {
                match expr {
                    ParComponentItem::Borrowed(expr) => borrowed_exprs.push(expr),
                    ParComponentItem::Owned(expr) => owned_exprs.push(expr),
                }
                Ok(())
            })
            .expect("Par invariant: expression endpoints are canonical");
            continue;
        }

        if let Some(expr) = owned_exprs.pop() {
            match expr.expr_instance {
                Some(GDouble(bits)) if f64::from_bits(bits).is_nan() => return true,
                Some(EListBody(list)) => {
                    owned_pars.extend(list.ps);
                }
                Some(ETupleBody(tuple)) => {
                    owned_pars.extend(tuple.ps);
                }
                Some(ESetBody(set)) => {
                    owned_pars.extend(set.ps);
                }
                Some(EMapBody(map)) => {
                    for pair in map.kvs {
                        if let Some(key) = pair.key {
                            owned_pars.push(key);
                        }
                        if let Some(value) = pair.value {
                            owned_pars.push(value);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        let Some(par) = owned_pars.pop() else {
            break;
        };
        par.try_into_components::<Expr, std::convert::Infallible>(|expr, _multiplicity| {
            owned_exprs.push(expr);
            Ok(())
        })
        .expect("Par invariant: expression endpoints are canonical");
    }
    false
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
// Before the typed-cursor migration, `eval_with` itself was already a loop over
// the generated Expr repeated field; the Θ(depth) recursion lived in
// `eval_expr_to_par`, which re-entered `eval_with` at
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

/// An input node. Generated-Protobuf components borrow the input term; the
/// repository-owned PathMap representation yields one owned decode per
/// distinct endpoint and carries its multiplicity separately.
///
/// Borrowed children remain reference moves. Owned endpoint values are boxed
/// once so their address stays stable while their descendants are scheduled;
/// [`EvKont::KeepOwnedExpr`] owns that box until the subtree finishes.
enum EvNode<'t> {
    /// `eval_with(par)`, optionally followed immediately by
    /// `single_expr_instance` — the per-operand check the helpers interleave.
    Eval {
        par: &'t Par,
        extract: bool,
    },
    OwnedExpr(Box<Expr>),
    OwnedPar {
        owner: Box<Par>,
        extract: bool,
    },
}

/// Post-order continuation. Carries the borrowed shell and the operator, never
/// a child value.
enum EvKont<'t> {
    /// Fold `concatenate` over `n` evaluated distinct Expr endpoints onto the
    /// shell of `par`. [`Self::Scale`] applies each endpoint's multiplicity
    /// before it reaches this continuation.
    ParK {
        par: &'t Par,
        n: usize,
    },
    Scale(u64),
    KeepOwnedExpr(Box<Expr>),
    KeepOwnedPar(Box<Par>),
    Not,
    NotSpine(usize),
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
    MatchesK {
        pattern: &'t Par,
    },
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

#[cfg(test)]
static ENOT_SPINE_PREPARATIONS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ENOT_SPINE_TERMINAL_DECODES: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ENOT_SPINE_COMBINES: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ENOT_SPINE_LEVELS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
fn record_enot_spine_preparation(depth: usize) {
    ENOT_SPINE_PREPARATIONS.fetch_add(1, AtomicOrdering::Relaxed);
    ENOT_SPINE_TERMINAL_DECODES.fetch_add(1, AtomicOrdering::Relaxed);
    ENOT_SPINE_LEVELS.fetch_add(depth, AtomicOrdering::Relaxed);
}

#[cfg(not(test))]
fn record_enot_spine_preparation(_depth: usize) {}

#[cfg(test)]
fn record_enot_spine_combine() { ENOT_SPINE_COMBINES.fetch_add(1, AtomicOrdering::Relaxed); }

#[cfg(not(test))]
fn record_enot_spine_combine() {}

#[cfg(test)]
fn reset_enot_spine_counters() {
    ENOT_SPINE_PREPARATIONS.store(0, AtomicOrdering::Relaxed);
    ENOT_SPINE_TERMINAL_DECODES.store(0, AtomicOrdering::Relaxed);
    ENOT_SPINE_COMBINES.store(0, AtomicOrdering::Relaxed);
    ENOT_SPINE_LEVELS.store(0, AtomicOrdering::Relaxed);
}

#[cfg(test)]
fn enot_spine_counters() -> (usize, usize, usize, usize) {
    (
        ENOT_SPINE_PREPARATIONS.load(AtomicOrdering::Relaxed),
        ENOT_SPINE_TERMINAL_DECODES.load(AtomicOrdering::Relaxed),
        ENOT_SPINE_COMBINES.load(AtomicOrdering::Relaxed),
        ENOT_SPINE_LEVELS.load(AtomicOrdering::Relaxed),
    )
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
    where Self: 't;
    type Val = EvVal;
    type Kont<'t>
        = EvKont<'t>
    where Self: 't;
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
                let mut expressions = Vec::with_capacity(par.family_distinct_len::<Expr>());
                par.visit_encoded::<Expr>(|entry| {
                    let multiplicity = entry.multiplicity().get();
                    let prepared = match decode_encoded_enot_terminal(&entry) {
                        EncodedEnotTerminal::Decoded { depth, terminal } => {
                            record_enot_spine_preparation(depth);
                            (
                                EvNode::OwnedPar {
                                    owner: Box::new(terminal),
                                    extract: true,
                                },
                                Some(depth),
                            )
                        }
                        EncodedEnotTerminal::Invalid => {
                            panic!("Par invariant: ENot terminal is canonical")
                        }
                        EncodedEnotTerminal::NotEnot => (
                            EvNode::OwnedExpr(Box::new(
                                entry
                                    .decode()
                                    .expect("Par invariant: expression endpoint is canonical"),
                            )),
                            None,
                        ),
                    };
                    expressions.push((prepared.0, multiplicity, prepared.1));
                });
                if extract {
                    work.push(Step::Combine(EvKont::Extract));
                }
                work.push(Step::Combine(EvKont::ParK {
                    par,
                    n: expressions.len(),
                }));
                for (expr, multiplicity, enot_depth) in expressions.into_iter().rev() {
                    if multiplicity != 1 {
                        work.push(Step::Combine(EvKont::Scale(multiplicity)));
                    }
                    if let Some(depth) = enot_depth {
                        work.push(Step::Combine(EvKont::NotSpine(depth)));
                    }
                    work.push(Step::Descend(expr));
                }
                Ok(())
            }
            EvNode::OwnedExpr(expr) => {
                let stable = expr.as_ref() as *const Expr;
                let work_floor = work.len();
                let value_floor = vals.len();
                // SAFETY: `expr` retains the Box during `descend_ev_expr`. If
                // that call schedules borrowed child obligations, the Box is
                // moved into a continuation inserted below the whole new
                // region before this method returns. Box movement does not
                // move its allocation. A leaf schedules no borrowed work and
                // has already cloned or resolved its result before `expr` is
                // dropped here.
                let stable: &'t Expr = unsafe { &*stable };
                if let Err(error) = descend_ev_expr(stable, self.env, self.matcher, work, vals) {
                    work.truncate(work_floor);
                    return Err(error);
                }
                if work.len() == work_floor {
                    debug_assert_eq!(vals.len(), value_floor + 1);
                } else {
                    debug_assert_eq!(vals.len(), value_floor);
                    work.insert(work_floor, Step::Combine(EvKont::KeepOwnedExpr(expr)));
                }
                Ok(())
            }
            EvNode::OwnedPar { owner, extract } => {
                let stable = owner.as_ref() as *const Par;
                let stable: &'t Par = unsafe { &*stable };
                work.push(Step::Combine(EvKont::KeepOwnedPar(owner)));
                work.push(Step::Descend(EvNode::Eval {
                    par: stable,
                    extract,
                }));
                Ok(())
            }
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
            EvKont::Scale(_) | EvKont::KeepOwnedExpr(_) | EvKont::KeepOwnedPar(_) => 1,
            EvKont::Not | EvKont::NotSpine(_) | EvKont::Neg => 1,
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

fn combine_not_spine(depth: usize, v: ExprInstance) -> Result<Par, EvalError> {
    match v {
        ExprInstance::GBool(value) => Ok(par_with_bool(if depth.is_multiple_of(2) {
            value
        } else {
            !value
        })),
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
            EvKont::BoolK {
                op: "&&",
                f: |a, b| a && b,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EOrBody(EOr { p1, p2 }) => push_binary(
            work,
            EvKont::BoolK {
                op: "||",
                f: |a, b| a || b,
            },
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
            EvKont::CmpK {
                op: "<",
                interpret: |c| c == -1,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::ELteBody(ELte { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK {
                op: "<=",
                interpret: |c| c <= 0,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EGtBody(EGt { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK {
                op: ">",
                interpret: |c| c == 1,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EGteBody(EGte { p1, p2 }) => push_binary(
            work,
            EvKont::CmpK {
                op: ">=",
                interpret: |c| c >= 0,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),

        ExprInstance::EPlusBody(EPlus { p1, p2 }) => push_binary(
            work,
            EvKont::IntK {
                op: "+",
                f: i64::checked_add,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => push_binary(
            work,
            EvKont::IntK {
                op: "-",
                f: i64::checked_sub,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EMultBody(EMult { p1, p2 }) => push_binary(
            work,
            EvKont::IntK {
                op: "*",
                f: i64::checked_mul,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => push_binary(
            work,
            EvKont::DivModK {
                op: "/",
                f: i64::checked_div,
            },
            p1.as_ref(),
            p2.as_ref(),
            true,
        ),
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => push_binary(
            work,
            EvKont::DivModK {
                op: "%",
                f: i64::checked_rem,
            },
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
            let mut acc = par.with_exprs(Vec::new());
            for e in evaled {
                acc = concatenate(acc, e)?;
            }
            Ok(EvVal::Par(acc))
        }
        EvKont::Scale(factor) => {
            let value = pop(vals).into_par();
            value
                .scale_occurrences(factor)
                .map(EvVal::Par)
                .map_err(EvalError::from)
        }
        EvKont::KeepOwnedExpr(_owner) => Ok(pop(vals)),
        EvKont::KeepOwnedPar(_owner) => Ok(pop(vals)),
        EvKont::Not => combine_not(pop(vals).into_inst()).map(EvVal::Par),
        EvKont::NotSpine(depth) => {
            record_enot_spine_combine();
            combine_not_spine(depth, pop(vals).into_inst()).map(EvVal::Par)
        }
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

    use models::rhoapi::{EDiv, EMethod, EMod, Send};

    use super::*;

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
        fn matches(&self, target: &Par, pattern: &Par) -> bool { target == pattern }
    }

    fn gint(i: i64) -> Par { par_with_int(i) }
    fn gbool(b: bool) -> Par { par_with_bool(b) }
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
            (
                "+",
                wrap(ExprInstance::EPlusBody(EPlus {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "-",
                wrap(ExprInstance::EMinusBody(EMinus {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "*",
                wrap(ExprInstance::EMultBody(EMult {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "/",
                wrap(ExprInstance::EDivBody(EDiv {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "%",
                wrap(ExprInstance::EModBody(EMod {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "<",
                wrap(ExprInstance::ELtBody(ELt {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "<=",
                wrap(ExprInstance::ELteBody(ELte {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                ">",
                wrap(ExprInstance::EGtBody(EGt {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                ">=",
                wrap(ExprInstance::EGteBody(EGte {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "==",
                wrap(ExprInstance::EEqBody(EEq {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "!=",
                wrap(ExprInstance::ENeqBody(ENeq {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
            (
                "&&",
                wrap(ExprInstance::EAndBody(EAnd {
                    p1: p1.clone(),
                    p2: p2.clone(),
                })),
            ),
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
            (gint(5), gint(0)),         // division by zero, one way only
            (gint(i64::MIN), gint(-1)), // overflow on / and *
            (gbool(true), gbool(false)),
            (gbool(false), gbool(true)),
            (gstr("a"), gstr("b")),
            (gstr("b"), gstr("a")),
            (gint(1), gbool(true)), // operator mismatch
            (gbool(true), gint(1)), // mismatch, other way
        ]
    }

    /// Unary and nested shapes, including the `ENot` chain the probe drives.
    fn unary_corpus() -> Vec<Par> {
        let mut out = vec![
            wrap(ExprInstance::ENotBody(ENot {
                p: Some(gbool(true)),
            })),
            wrap(ExprInstance::ENotBody(ENot { p: Some(gint(1)) })),
            wrap(ExprInstance::ENegBody(ENeg { p: Some(gint(5)) })),
            wrap(ExprInstance::ENegBody(ENeg {
                p: Some(gint(i64::MIN)),
            })),
            wrap(ExprInstance::ENegBody(ENeg {
                p: Some(gbool(true)),
            })),
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
    /// The width axis is typed Expr-family iteration in BOTH arms (the
    /// occurrence visitor in the oracle, the `ParK { n }` fold in the
    /// machine), so it is not stack-bounded on either side and can be large.
    const DIFFERENTIAL_WIDTH: usize = 256;

    fn eplus(a: Par, b: Par) -> Par {
        wrap(ExprInstance::EPlusBody(EPlus {
            p1: Some(a),
            p2: Some(b),
        }))
    }
    fn eminus(a: Par, b: Par) -> Par {
        wrap(ExprInstance::EMinusBody(EMinus {
            p1: Some(a),
            p2: Some(b),
        }))
    }
    fn enot(inner: Par) -> Par { wrap(ExprInstance::ENotBody(ENot { p: Some(inner) })) }
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

    fn method(name: &str) -> Par {
        wrap(ExprInstance::EMethodBody(EMethod {
            method_name: name.to_owned(),
            target: Some(Par::default()),
            arguments: Vec::new(),
            locally_free: Vec::new(),
            connective_used: false,
        }))
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
        models::par_from_default! { exprs, ..Par::default() }
    }

    /// A Par of `width` *operator* expressions, so the fold's children are
    /// themselves suspensions rather than leaves.
    fn wide_par_of_operators(width: usize) -> Par {
        let mut exprs = Vec::with_capacity(width);
        for i in 0..width {
            let mut operator = eplus(gint(i as i64), gint(1));
            exprs.append(&mut operator.take_occurrences::<Expr>());
        }
        models::par_from_default! { exprs, ..Par::default() }
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
        out.push(models::par_from_default! {
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
            let mut branch = deep_left(d, gint(i as i64), gint(1));
            wide_deep.append(&mut branch.take_occurrences::<Expr>());
        }
        out.push(models::par_from_default! { exprs: wide_deep, ..Par::default() });

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

    #[test]
    fn compressed_enot_spines_match_the_recursive_oracle_at_semantic_edges() {
        let terminals = [
            gbool(true),
            gbool(false),
            gint(7),
            Par::default(),
            models::par_from_default! {
                exprs: vec![
                    Expr { expr_instance: Some(ExprInstance::GInt(1)) },
                    Expr { expr_instance: Some(ExprInstance::GInt(2)) },
                ],
                ..Par::default()
            },
            method("unsupported"),
            eplus(gint(1), gbool(true)),
        ];
        let mut index = 0usize;
        for depth in [1, 2, 3, 32] {
            for terminal in &terminals {
                agree_on(
                    index,
                    &deep_nots(depth, terminal.clone()),
                    &NoSpatialMatch,
                    "compressed ENot edge",
                );
                index += 1;
            }
        }

        let single = deep_nots(4, gbool(true));
        let expr = single.first::<Expr>().unwrap().unwrap().0;
        let mut duplicate = Par::default().with_exprs(vec![expr.clone(), expr]);
        duplicate.replace_sends(vec![Send::default()]);
        agree_on(
            index,
            &duplicate,
            &NoSpatialMatch,
            "compressed ENot multiplicity",
        );
        index += 1;
        agree_on(
            index,
            &enot(duplicate),
            &NoSpatialMatch,
            "compressed ENot extraction",
        );
    }

    #[test]
    fn not_spine_continuation_preserves_type_checks_and_parity() {
        assert_eq!(
            <EvalTraversal<'_> as Traversal>::arity(&EvKont::NotSpine(2)),
            1
        );
        assert_eq!(
            combine_not_spine(2, ExprInstance::GBool(true)).unwrap(),
            gbool(true)
        );
        assert_eq!(
            combine_not_spine(3, ExprInstance::GBool(true)).unwrap(),
            gbool(false)
        );
        assert_eq!(
            format!(
                "{:?}",
                combine_not_spine(2, ExprInstance::GInt(7)).unwrap_err()
            ),
            format!("{:?}", combine_not(ExprInstance::GInt(7)).unwrap_err())
        );
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

    /// The generated transitional representation reports every occurrence
    /// separately, so ordinary differential inputs never build `Scale`.
    /// Exercise it directly to keep the PathMap endpoint-multiplicity path
    /// non-vacuous before the extern-PathMap cutover makes it routine.
    #[test]
    fn the_multiplicity_continuation_scales_one_value_and_consumes_one_slot() {
        let mut vals = vec![EvVal::Par(gint(7))];
        let output = run_ev_combine(EvKont::Scale(3), &NoSpatialMatch, &mut vals)
            .expect("multiplicity scaling")
            .into_par();

        assert!(vals.is_empty(), "Scale must consume exactly one value");
        assert_eq!(EvalTraversal::arity(&EvKont::Scale(3)), 1);
        assert_eq!(output.cardinality(), 3);
        assert_eq!(output.family_cardinality::<Expr>(), 3);
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
            if a != b {
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
    // 3. ★ AT THE TIME, IT ONLY PASSED BECAUSE OF ONE CALL IN THE FIXTURE. Its
    //    final statement was `par_children::dismantle(deep)`, while ordinary
    //    `drop_in_place::<Par>` was still Theta(depth). The generated `Drop for
    //    Par` has since closed that destructor gap by delegating to
    //    `par_children::dismantle_in_place`.
    //
    // The property it was named for is REAL and is now gated properly, by
    // [`super::depth_gate`]: both arms, in child processes, on an explicitly
    // sized stack, with the recursive arm REQUIRED to fail where the driver
    // and ordinary generated destructor survive together. Deleting this test
    // rather than leaving it beside that gate is deliberate: two statements
    // of one property do not stay equal, and the weaker one is the one a
    // reader reaches first.
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
    //! A native stack overflow terminates the process rather than producing a
    //! catchable `Result`; `catch_unwind` does not see it. And this
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
    //! recursive arm to report the Rust runtime's stack-overflow signature at
    //! [`RECURSIVE_DEEP`] and the driver to survive it. A timeout, OOM kill or
    //! unrelated panic is a distinct failing outcome, never evidence for the
    //! property. That middle assertion is RED on purpose, permanently: a
    //! driver that silently reverted to recursion would make it green and the
    //! gate would fail.
    //!
    //! ## ★★ The production composition is exercised directly
    //!
    //! The generated `Drop for Par` delegates to the schema-exhaustive heap
    //! worklist in `par_children::dismantle_in_place`. `run_arm` therefore lets
    //! the input term fall out of scope normally: there is no fixture-only
    //! teardown and no destructor cost hidden from the measurement. The main
    //! `rholang/tests/stack_depth_gate.rs` independently measures `par_drop`
    //! and `normalize_drop` across its full depth ladder; this gate checks the
    //! actual `eval_with` plus ordinary-`Drop` composition at a much deeper
    //! rung. The claim is exact and bounded:
    //!
    //! | claim | rung | verdict |
    //! |---|---|---|
    //! | `eval_with` plus ordinary `Par::drop` is O(1) in nesting depth | `driver` @ [`DEEP_DROP`] | survives 1 MiB |
    //! | the recursive twin's is not | `recursive` @ [`RECURSIVE_DEEP`] | reports native stack overflow on 1 MiB |

    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    /// Which implementation the child runs.
    const GATE_ARM: &str = "RHO_PURE_EVAL_GATE_ARM";
    /// `ENot` nesting levels.
    const GATE_DEPTH: &str = "RHO_PURE_EVAL_GATE_DEPTH";
    /// The child thread's stack, in bytes.
    const GATE_STACK_VAR: &str = "RHO_PURE_EVAL_GATE_STACK";

    /// The child entry point's full test path, as libtest's `--exact` wants it.
    const CHILD: &str = "eval::depth_gate::gate_child";

    /// 1 MiB. Large enough for the harness, the term builder and the driver's
    /// own frame; smaller than the recursive slope-only demand at the
    /// profile-specific [`RECURSIVE_DEEP`] rung.
    const GATE_STACK: usize = 1024 * 1024;

    /// The rung at which both arms must work — the gate's own control.
    const SHALLOW: usize = 4;

    /// The rung at which the recursive arm must overflow and the driver must
    /// survive, derived from the measured profile-specific native-stack slope.
    ///
    /// Debug measured 21,584 B/level, so 96 levels demand 2,072,064 B before
    /// the positive harness intercept. Release measured 3,359 B/level, so 512
    /// levels demand 1,719,808 B. Both exceed the explicit 1 MiB stack while
    /// avoiding the repeated-suffix PathMap work of the obsolete 4,096 rung.
    #[cfg(debug_assertions)]
    const RECURSIVE_DEEP: usize = 96;
    #[cfg(not(debug_assertions))]
    const RECURSIVE_DEEP: usize = 512;

    /// A harness safety ceiling, not a performance assertion. Timing out is a
    /// failing outcome and can therefore never prove stack unsafety.
    const CHILD_TIMEOUT: Duration = Duration::from_secs(60);

    /// The production-composition rung. At 131,072 levels it is far beyond the
    /// recursive oracle's ceiling on this 1 MiB stack and strictly stronger
    /// than the deleted 20,000-level ambient-stack smoke test.
    const DEEP_DROP: usize = 131_072;

    fn nots(depth: usize) -> Par {
        let terminal = par_with_bool(true);
        let result = planned_nots(depth, &terminal);
        assert_planned_nots(&result, depth, &terminal);
        result
    }

    /// Run one arm at one depth, on whatever stack the caller set up.
    fn run_arm(arm: &str, depth: usize) {
        let deep = nots(depth);
        let env: Env<Par> = Env::new();
        reset_enot_spine_counters();
        let out = match arm {
            "recursive" => eval_with_recursive(&deep, &env, &NoSpatialMatch),
            "driver" => eval_with(&deep, &env, &NoSpatialMatch),
            other => panic!("depth_gate: unknown {GATE_ARM}={other:?}"),
        }
        .expect("depth_gate: the chain evaluates");
        let counters = enot_spine_counters();
        match arm {
            "driver" => assert_eq!(counters, (1, 1, 1, depth)),
            "recursive" => assert_eq!(counters, (0, 0, 0, 0)),
            _ => unreachable!("the arm was validated above"),
        }
        assert_eq!(
            single_expr_instance(&out).expect("depth_gate: a single value"),
            // An even number of negations of `true` is `true`.
            ExprInstance::GBool(depth.is_multiple_of(2)),
            "depth_gate: {depth} negations of `true` under arm {arm:?}"
        );

        // `deep` falls out of scope through the generated stack-safe `Drop for
        // Par`, so this arm measures the production composition rather than a
        // fixture-only teardown path.
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

    #[derive(Debug, PartialEq, Eq)]
    enum ChildOutcome {
        Success,
        StackOverflow,
        TimedOut,
        Failed { status: String, stderr: String },
    }

    /// Run one probe point in a child process and classify its exact outcome.
    /// In particular, an arbitrary non-zero exit is never accepted as proof
    /// that the recursive arm exhausted its native stack.
    fn child_outcome(arm: &str, depth: usize, stack: usize) -> ChildOutcome {
        let exe = std::env::current_exe().expect("depth_gate: current_exe");
        let mut child = Command::new(exe)
            .args(["--ignored", "--exact", CHILD])
            .env(GATE_ARM, arm)
            .env(GATE_DEPTH, depth.to_string())
            .env(GATE_STACK_VAR, stack.to_string())
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("depth_gate: failed to run the child");
        let deadline = Instant::now() + CHILD_TIMEOUT;

        loop {
            if let Some(status) = child
                .try_wait()
                .expect("depth_gate: failed to poll the child")
            {
                let mut bytes = Vec::new();
                child
                    .stderr
                    .take()
                    .expect("depth_gate: piped child stderr disappeared")
                    .read_to_end(&mut bytes)
                    .expect("depth_gate: failed to read child stderr");
                let stderr = String::from_utf8_lossy(&bytes).into_owned();
                if status.success() {
                    return ChildOutcome::Success;
                }
                if stderr.contains("has overflowed its stack")
                    && stderr.contains("fatal runtime error: stack overflow")
                {
                    return ChildOutcome::StackOverflow;
                }
                return ChildOutcome::Failed {
                    status: status.to_string(),
                    stderr,
                };
            }

            if Instant::now() >= deadline {
                child
                    .kill()
                    .expect("depth_gate: failed to kill timed-out child");
                child
                    .wait()
                    .expect("depth_gate: failed to reap timed-out child");
                return ChildOutcome::TimedOut;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_driver_survives_a_depth_that_overflows_the_recursive_oracle() {
        // ---- the control: at a shallow depth, the harness works for BOTH ----
        assert_eq!(
            child_outcome("recursive", SHALLOW, GATE_STACK),
            ChildOutcome::Success,
            "CONTROL FAILED: the recursive arm did not survive {SHALLOW} levels on a \
             {GATE_STACK}-byte stack. Nothing below this line means anything until it \
             does — the child is failing for a reason that is not depth."
        );
        assert_eq!(
            child_outcome("driver", SHALLOW, GATE_STACK),
            ChildOutcome::Success,
            "CONTROL FAILED: the driver arm did not survive {SHALLOW} levels on a \
             {GATE_STACK}-byte stack."
        );

        // ---- the RED half: the oracle must NOT survive ----
        assert_eq!(
            child_outcome("recursive", RECURSIVE_DEEP, GATE_STACK),
            ChildOutcome::StackOverflow,
            "THE GUARD HAS GONE VACUOUS: the recursive oracle did not report native stack \
             overflow at {RECURSIVE_DEEP} levels on a {GATE_STACK}-byte stack. A success \
             means the measured slope changed; a timeout, kill or unrelated panic is a \
             harness failure, not proof of stack unsafety."
        );

        // ---- the property ----
        assert_eq!(
            child_outcome("driver", RECURSIVE_DEEP, GATE_STACK),
            ChildOutcome::Success,
            "REGRESSION: `eval_with` did not survive {RECURSIVE_DEEP} levels on a {GATE_STACK}-byte \
             stack. Its native stack must be O(1) in nesting depth — the recursion belongs \
             in `drive`'s heap work stack. Something has re-introduced a native \
             re-entry: check `descend_ev_expr`'s `EVarBody` arm (the one deliberate nested \
             drive, bounded by the ENVIRONMENT rather than by the term) and any new arm \
             that calls `eval_with` directly."
        );
        assert_eq!(
            child_outcome("driver", DEEP_DROP, GATE_STACK),
            ChildOutcome::Success,
            "REGRESSION: `eval_with` did not survive {DEEP_DROP} levels on a \
             {GATE_STACK}-byte stack. This rung is {DEEP_DROP} deep specifically so the \
             claim is not one a fixture could have manufactured — see the module docs."
        );
    }

    #[test]
    fn an_unrelated_child_panic_is_not_stack_overflow_evidence() {
        assert!(matches!(
            child_outcome("unknown-arm", SHALLOW, GATE_STACK),
            ChildOutcome::Failed { .. }
        ));
    }
}
