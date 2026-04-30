use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EEq, EGt, EGte, ELt, ELte, EMinus, EMult, ENeg, ENeq, ENot, EOr, EPlus, Expr, Par, Var,
};

use crate::env::Env;
use crate::error::EvalError;

/// Evaluates the `exprs` slot of a Par against the given environment.
///
/// Process-level Par fields (sends, receives, news, matches, bundles,
/// unforgeables, connectives, conditionals) are preserved unchanged in
/// the returned Par. The `exprs` slot is replaced with the evaluated
/// values.
pub fn eval(par: &Par, env: &Env<Par>) -> Result<Par, EvalError> {
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
        let evaled = eval_expr_to_par(expr, env)?;
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

fn eval_expr_to_par(expr: &Expr, env: &Env<Par>) -> Result<Par, EvalError> {
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
            eval(&p, env)
        }

        ExprInstance::ENotBody(ENot { p }) => {
            let inner = require_par(p.as_ref())?;
            let evaled = eval(&inner, env)?;
            let v = single_expr_instance(&evaled)?;
            match v {
                ExprInstance::GBool(b) => Ok(par_with_bool(!b)),
                other => Err(operator_mismatch_unary("!", &other)),
            }
        }

        ExprInstance::ENegBody(ENeg { p }) => {
            let inner = require_par(p.as_ref())?;
            let evaled = eval(&inner, env)?;
            let v = single_expr_instance(&evaled)?;
            match v {
                ExprInstance::GInt(i) => Ok(par_with_int(-i)),
                other => Err(operator_mismatch_unary("-", &other)),
            }
        }

        ExprInstance::EAndBody(EAnd { p1, p2 }) => {
            bool_binop("&&", p1.as_ref(), p2.as_ref(), env, |a, b| a && b)
        }
        ExprInstance::EOrBody(EOr { p1, p2 }) => {
            bool_binop("||", p1.as_ref(), p2.as_ref(), env, |a, b| a || b)
        }

        ExprInstance::EEqBody(EEq { p1, p2 }) => eq_binop(p1.as_ref(), p2.as_ref(), env, true),
        ExprInstance::ENeqBody(ENeq { p1, p2 }) => eq_binop(p1.as_ref(), p2.as_ref(), env, false),

        ExprInstance::ELtBody(ELt { p1, p2 }) => {
            cmp_binop("<", p1.as_ref(), p2.as_ref(), env, |c| c == -1)
        }
        ExprInstance::ELteBody(ELte { p1, p2 }) => {
            cmp_binop("<=", p1.as_ref(), p2.as_ref(), env, |c| c <= 0)
        }
        ExprInstance::EGtBody(EGt { p1, p2 }) => {
            cmp_binop(">", p1.as_ref(), p2.as_ref(), env, |c| c == 1)
        }
        ExprInstance::EGteBody(EGte { p1, p2 }) => {
            cmp_binop(">=", p1.as_ref(), p2.as_ref(), env, |c| c >= 0)
        }

        ExprInstance::EPlusBody(EPlus { p1, p2 }) => {
            int_binop("+", p1.as_ref(), p2.as_ref(), env, i64::wrapping_add)
        }
        ExprInstance::EMinusBody(EMinus { p1, p2 }) => {
            int_binop("-", p1.as_ref(), p2.as_ref(), env, i64::wrapping_sub)
        }
        ExprInstance::EMultBody(EMult { p1, p2 }) => {
            int_binop("*", p1.as_ref(), p2.as_ref(), env, i64::wrapping_mul)
        }
        ExprInstance::EDivBody(models::rhoapi::EDiv { p1, p2 }) => {
            int_div_or_mod("/", p1.as_ref(), p2.as_ref(), env, |a, b| a / b)
        }
        ExprInstance::EModBody(models::rhoapi::EMod { p1, p2 }) => {
            int_div_or_mod("%", p1.as_ref(), p2.as_ref(), env, |a, b| a % b)
        }

        // Stubs — supported in the full reducer but not here yet.
        ExprInstance::EMethodBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMethodBody",
        }),
        ExprInstance::EMatchesBody(_) => Err(EvalError::UnsupportedExpression {
            kind: "EMatchesBody",
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

fn bool_binop<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(bool, bool) -> bool,
{
    let v1 = single_expr_instance(&eval(&require_par(p1)?, env)?)?;
    let v2 = single_expr_instance(&eval(&require_par(p2)?, env)?)?;
    match (&v1, &v2) {
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => Ok(par_with_bool(f(*b1, *b2))),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

fn eq_binop(
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    expect_eq: bool,
) -> Result<Par, EvalError> {
    let lhs = eval(&require_par(p1)?, env)?;
    let rhs = eval(&require_par(p2)?, env)?;
    let eq = lhs == rhs;
    Ok(par_with_bool(if expect_eq { eq } else { !eq }))
}

fn cmp_binop<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    interpret: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64) -> bool,
{
    let v1 = single_expr_instance(&eval(&require_par(p1)?, env)?)?;
    let v2 = single_expr_instance(&eval(&require_par(p2)?, env)?)?;
    let order: i64 = match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => i64::from(i1.cmp(i2) as i8),
        (ExprInstance::GString(s1), ExprInstance::GString(s2)) => i64::from(s1.cmp(s2) as i8),
        (ExprInstance::GBool(b1), ExprInstance::GBool(b2)) => i64::from(b1.cmp(b2) as i8),
        _ => return Err(operator_mismatch_binary(op, &v1, &v2)),
    };
    Ok(par_with_bool(interpret(order)))
}

fn int_binop<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64, i64) -> i64,
{
    let v1 = single_expr_instance(&eval(&require_par(p1)?, env)?)?;
    let v2 = single_expr_instance(&eval(&require_par(p2)?, env)?)?;
    match (&v1, &v2) {
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => Ok(par_with_int(f(*i1, *i2))),
        _ => Err(operator_mismatch_binary(op, &v1, &v2)),
    }
}

fn int_div_or_mod<F>(
    op: &'static str,
    p1: Option<&Par>,
    p2: Option<&Par>,
    env: &Env<Par>,
    f: F,
) -> Result<Par, EvalError>
where
    F: Fn(i64, i64) -> i64,
{
    let v1 = single_expr_instance(&eval(&require_par(p1)?, env)?)?;
    let v2 = single_expr_instance(&eval(&require_par(p2)?, env)?)?;
    match (&v1, &v2) {
        (ExprInstance::GInt(_), ExprInstance::GInt(0)) => Err(EvalError::DivisionByZero),
        (ExprInstance::GInt(i1), ExprInstance::GInt(i2)) => Ok(par_with_int(f(*i1, *i2))),
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
