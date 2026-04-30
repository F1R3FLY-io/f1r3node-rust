use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    EAnd, EDiv, EEq, EGt, EGte, ELt, ELte, EMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPlus, EVar,
    Expr, Par, Var,
};

use crate::env::Env;
use crate::error::EvalError;
use crate::eval::eval;

fn par_of(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Par::default()
    }
}

fn gbool(b: bool) -> Par { par_of(ExprInstance::GBool(b)) }

fn gint(i: i64) -> Par { par_of(ExprInstance::GInt(i)) }

fn gstr(s: &str) -> Par { par_of(ExprInstance::GString(s.to_string())) }

fn evar(idx: i32) -> Par {
    par_of(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::BoundVar(idx)),
        }),
    }))
}

fn assert_bool(par: &Par, expected: bool) {
    assert_eq!(par, &gbool(expected));
}

fn assert_int(par: &Par, expected: i64) {
    assert_eq!(par, &gint(expected));
}

#[test]
fn ground_bool_passes_through() {
    let env = Env::<Par>::new();
    let result = eval(&gbool(true), &env).unwrap();
    assert_bool(&result, true);
}

#[test]
fn ground_int_passes_through() {
    let env = Env::<Par>::new();
    let result = eval(&gint(42), &env).unwrap();
    assert_int(&result, 42);
}

#[test]
fn evar_resolves_from_env() {
    let mut env = Env::<Par>::new();
    let env = env.put(gint(7));
    let result = eval(&evar(0), &env).unwrap();
    assert_int(&result, 7);
}

#[test]
fn evar_unbound_fails() {
    let env = Env::<Par>::new();
    let result = eval(&evar(99), &env);
    assert!(matches!(
        result,
        Err(EvalError::UnboundVariable { index: 99 })
    ));
}

#[test]
fn enot_negates_bool() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENotBody(ENot {
        p: Some(gbool(true)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn enot_on_non_bool_errors() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENotBody(ENot { p: Some(gint(5)) }));
    let result = eval(&expr, &env);
    assert!(matches!(
        result,
        Err(EvalError::OperatorTypeMismatch { op: "!", .. })
    ));
}

#[test]
fn eand_short_form() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EAndBody(EAnd {
        p1: Some(gbool(true)),
        p2: Some(gbool(false)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn eor_short_form() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EOrBody(EOr {
        p1: Some(gbool(true)),
        p2: Some(gbool(false)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn eeq_int_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(gint(3)),
        p2: Some(gint(3)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn eeq_string_string_unequal() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(gstr("a")),
        p2: Some(gstr("b")),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn eneq_inverts_eq() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENeqBody(ENeq {
        p1: Some(gint(3)),
        p2: Some(gint(4)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn elt_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ELtBody(ELt {
        p1: Some(gint(2)),
        p2: Some(gint(5)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn ele_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ELteBody(ELte {
        p1: Some(gint(5)),
        p2: Some(gint(5)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn egt_string() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EGtBody(EGt {
        p1: Some(gstr("b")),
        p2: Some(gstr("a")),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn egte_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EGteBody(EGte {
        p1: Some(gint(5)),
        p2: Some(gint(6)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn cmp_type_mismatch() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ELtBody(ELt {
        p1: Some(gint(2)),
        p2: Some(gstr("a")),
    }));
    assert!(matches!(
        eval(&expr, &env),
        Err(EvalError::OperatorTypeMismatch { op: "<", .. })
    ));
}

#[test]
fn eplus_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EPlusBody(EPlus {
        p1: Some(gint(3)),
        p2: Some(gint(4)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), 7);
}

#[test]
fn eminus_int_negative() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EMinusBody(EMinus {
        p1: Some(gint(2)),
        p2: Some(gint(5)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), -3);
}

#[test]
fn emult_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EMultBody(EMult {
        p1: Some(gint(6)),
        p2: Some(gint(7)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), 42);
}

#[test]
fn ediv_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EDivBody(EDiv {
        p1: Some(gint(20)),
        p2: Some(gint(3)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), 6);
}

#[test]
fn ediv_by_zero() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EDivBody(EDiv {
        p1: Some(gint(1)),
        p2: Some(gint(0)),
    }));
    assert_eq!(eval(&expr, &env), Err(EvalError::DivisionByZero));
}

#[test]
fn emod_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EModBody(EMod {
        p1: Some(gint(20)),
        p2: Some(gint(3)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), 2);
}

#[test]
fn eneg_int() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENegBody(ENeg { p: Some(gint(5)) }));
    assert_int(&eval(&expr, &env).unwrap(), -5);
}

#[test]
fn nested_expressions_resolve() {
    // (3 + 4) > 5 → true
    let env = Env::<Par>::new();
    let sum = par_of(ExprInstance::EPlusBody(EPlus {
        p1: Some(gint(3)),
        p2: Some(gint(4)),
    }));
    let cmp = par_of(ExprInstance::EGtBody(EGt {
        p1: Some(sum),
        p2: Some(gint(5)),
    }));
    assert_bool(&eval(&cmp, &env).unwrap(), true);
}

#[test]
fn evar_in_arithmetic() {
    // x + 1 where x = 41 → 42
    let mut env = Env::<Par>::new();
    let env = env.put(gint(41));
    let expr = par_of(ExprInstance::EPlusBody(EPlus {
        p1: Some(evar(0)),
        p2: Some(gint(1)),
    }));
    assert_int(&eval(&expr, &env).unwrap(), 42);
}

#[test]
fn process_level_par_content_is_preserved() {
    // A Par with a Send sitting alongside a bool Expr should keep the
    // Send unchanged in the output. This is the "side effects in
    // conditions are inert" property called out in the plan.
    use models::rhoapi::Send;

    let env = Env::<Par>::new();
    let par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GBool(true)),
        }],
        sends: vec![Send {
            chan: Some(gstr("c")),
            data: vec![gint(5)],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        }],
        ..Par::default()
    };

    let result = eval(&par, &env).unwrap();

    assert_eq!(result.exprs.len(), 1);
    assert!(matches!(
        result.exprs[0].expr_instance,
        Some(ExprInstance::GBool(true))
    ));
    assert_eq!(result.sends.len(), 1);
    assert_eq!(result.sends[0].data, vec![gint(5)]);
}

#[test]
fn unsupported_method_call_errors() {
    use models::rhoapi::EMethod;

    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EMethodBody(EMethod {
        method_name: "length".to_string(),
        target: Some(gstr("hi")),
        arguments: vec![],
        locally_free: vec![],
        connective_used: false,
    }));
    assert!(matches!(
        eval(&expr, &env),
        Err(EvalError::UnsupportedExpression {
            kind: "EMethodBody"
        })
    ));
}

#[test]
fn determinism_same_input_same_output() {
    // Smoke test: evaluating the same Par twice produces byte-identical
    // results. This is the property that makes rho-pure-eval safe under
    // casper replay.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EAndBody(EAnd {
        p1: Some(gbool(true)),
        p2: Some(par_of(ExprInstance::EGtBody(EGt {
            p1: Some(gint(10)),
            p2: Some(gint(3)),
        }))),
    }));
    let r1 = eval(&expr, &env).unwrap();
    let r2 = eval(&expr, &env).unwrap();
    assert_eq!(r1, r2);
}
