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

fn gdouble(f: f64) -> Par { par_of(ExprInstance::GDouble(f.to_bits())) }

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
fn ediv_i64_min_by_neg_one_is_overflow() {
    // i64::MIN / -1 overflows i64; native `/` would panic. The reducer
    // raises "Arithmetic overflow in division" — pure-eval mirrors that
    // via ArithmeticOverflow.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EDivBody(EDiv {
        p1: Some(gint(i64::MIN)),
        p2: Some(gint(-1)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "/" })
    );
}

#[test]
fn emod_i64_min_by_neg_one_is_overflow() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EModBody(EMod {
        p1: Some(gint(i64::MIN)),
        p2: Some(gint(-1)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "%" })
    );
}

#[test]
fn eneg_i64_min_is_overflow() {
    // -i64::MIN can't be represented in i64; native `-` would panic.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENegBody(ENeg {
        p: Some(gint(i64::MIN)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "-" })
    );
}

#[test]
fn eplus_overflow_at_i64_max() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EPlusBody(EPlus {
        p1: Some(gint(i64::MAX)),
        p2: Some(gint(1)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "+" })
    );
}

#[test]
fn eminus_overflow_at_i64_min() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EMinusBody(EMinus {
        p1: Some(gint(i64::MIN)),
        p2: Some(gint(1)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "-" })
    );
}

#[test]
fn emult_overflow() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EMultBody(EMult {
        p1: Some(gint(i64::MAX)),
        p2: Some(gint(2)),
    }));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::ArithmeticOverflow { op: "*" })
    );
}

#[test]
fn eeq_nan_with_nan_is_false() {
    // IEEE 754: NaN == NaN is always false. Reducer enforces this; pure-eval
    // must match so guard semantics agree with the reducer.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(gdouble(f64::NAN)),
        p2: Some(gdouble(f64::NAN)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn eeq_nan_with_number_is_false() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(gdouble(f64::NAN)),
        p2: Some(gdouble(1.0)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), false);
}

#[test]
fn eneq_nan_with_nan_is_true() {
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::ENeqBody(ENeq {
        p1: Some(gdouble(f64::NAN)),
        p2: Some(gdouble(f64::NAN)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
}

#[test]
fn eeq_double_normal_equality() {
    // Sanity: NaN handling didn't break ordinary double equality.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(gdouble(1.5)),
        p2: Some(gdouble(1.5)),
    }));
    assert_bool(&eval(&expr, &env).unwrap(), true);
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

// =====================================================================
// M-1a: the `SpatialMatch` seam (EMatchesBody).
//
// The oracle that decides `target matches pattern` lives in `rholang`,
// which depends on this crate, so it is injected by the caller rather
// than called directly. These tests pin four things:
//
//   (a) the default (`NoSpatialMatch`, i.e. plain `eval`) still refuses
//       EMatches with exactly the pre-seam error, and refuses it BEFORE
//       evaluating either operand;
//   (b) an injected oracle produces the right boolean either way;
//   (c) the verdict is deterministic (same pair ⇒ same answer);
//   (d) the TARGET is evaluated (env-resolved) and the PATTERN is handed
//       over verbatim — the depth-1 substitution question.
// =====================================================================

use std::sync::Mutex;

use models::rhoapi::EMatches;

use crate::eval::eval_with;
use crate::oracle::{NoSpatialMatch, SpatialMatch};

/// A free variable Par — what a pattern's binders look like. Evaluating
/// one is an error (`resolve_var` rejects `FreeVar`), which is precisely
/// why the EMatches arm must not evaluate its pattern.
fn free_var(idx: i32) -> Par {
    par_of(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(VarInstance::FreeVar(idx)),
        }),
    }))
}

fn ematches(target: Par, pattern: Par) -> Par {
    par_of(ExprInstance::EMatchesBody(EMatches {
        target: Some(target),
        pattern: Some(pattern),
    }))
}

/// Stand-in oracle: structural equality. Sound for the ground-vs-ground
/// fragment of spatial matching (the real matcher's own first arm is
/// `guard(match_pars(target, pattern))` when the pattern uses no
/// connectives), and enough to exercise the seam without dragging the
/// `rholang` matcher into this crate — which is the whole point of the
/// seam.
struct StructuralEqualityOracle;

impl SpatialMatch for StructuralEqualityOracle {
    fn matches(&self, target: &Par, pattern: &Par) -> bool { target == pattern }
}

/// Oracle that answers a fixed verdict and records every question asked,
/// so a test can assert WHAT was handed to the matcher — evaluated
/// target, verbatim pattern — and how many times.
struct RecordingOracle {
    verdict: bool,
    asked: Mutex<Vec<(Par, Par)>>,
}

impl RecordingOracle {
    fn new(verdict: bool) -> Self {
        RecordingOracle {
            verdict,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn questions(&self) -> Vec<(Par, Par)> {
        self.asked
            .lock()
            .expect("RecordingOracle mutex poisoned")
            .clone()
    }
}

impl SpatialMatch for RecordingOracle {
    fn matches(&self, target: &Par, pattern: &Par) -> bool {
        self.asked
            .lock()
            .expect("RecordingOracle mutex poisoned")
            .push((target.clone(), pattern.clone()));
        self.verdict
    }
}

// --- (a) the NoSpatialMatch default -----------------------------------

#[test]
fn ematches_without_oracle_is_unsupported() {
    let env = Env::<Par>::new();
    let expr = ematches(gint(5), gint(5));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::UnsupportedExpression {
            kind: "EMatchesBody"
        })
    );
}

#[test]
fn ematches_with_explicit_no_spatial_match_is_unsupported() {
    // `eval` is defined as `eval_with(.., &NoSpatialMatch)`; spelling the
    // default out explicitly must give the identical error.
    let env = Env::<Par>::new();
    let expr = ematches(gint(5), gint(5));
    assert_eq!(
        eval_with(&expr, &env, &NoSpatialMatch),
        Err(EvalError::UnsupportedExpression {
            kind: "EMatchesBody"
        })
    );
}

#[test]
fn ematches_without_oracle_does_not_evaluate_its_operands() {
    // The target here would raise `UnboundVariable` if it were evaluated.
    // Seeing `UnsupportedExpression` instead proves the unsupported check
    // short-circuits first — the pre-seam behaviour, preserved exactly.
    let env = Env::<Par>::new();
    let expr = ematches(evar(99), free_var(0));
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::UnsupportedExpression {
            kind: "EMatchesBody"
        })
    );
}

#[test]
fn eval_agrees_with_eval_with_no_spatial_match_on_every_shape() {
    // The `eval` shim must be behaviourally transparent: for inputs that
    // never reach the EMatches arm it is the same function, and for
    // inputs that do it is the same error. Assert it over a battery
    // covering each family of arms.
    let mut env = Env::<Par>::new();
    let env = env.put(gint(7));

    let cases: Vec<Par> = vec![
        gint(42),
        gbool(true),
        gstr("s"),
        evar(0),
        evar(99),
        par_of(ExprInstance::ENotBody(ENot {
            p: Some(gbool(false)),
        })),
        par_of(ExprInstance::ENegBody(ENeg { p: Some(gint(3)) })),
        par_of(ExprInstance::EAndBody(EAnd {
            p1: Some(gbool(true)),
            p2: Some(gbool(false)),
        })),
        par_of(ExprInstance::EOrBody(EOr {
            p1: Some(gbool(true)),
            p2: Some(gint(1)),
        })),
        par_of(ExprInstance::EEqBody(EEq {
            p1: Some(gint(1)),
            p2: Some(gint(1)),
        })),
        par_of(ExprInstance::ENeqBody(ENeq {
            p1: Some(gdouble(f64::NAN)),
            p2: Some(gdouble(f64::NAN)),
        })),
        par_of(ExprInstance::ELtBody(ELt {
            p1: Some(gint(1)),
            p2: Some(gint(2)),
        })),
        par_of(ExprInstance::ELteBody(ELte {
            p1: Some(gstr("a")),
            p2: Some(gstr("b")),
        })),
        par_of(ExprInstance::EGtBody(EGt {
            p1: Some(gint(2)),
            p2: Some(gint(1)),
        })),
        par_of(ExprInstance::EGteBody(EGte {
            p1: Some(gbool(true)),
            p2: Some(gbool(false)),
        })),
        par_of(ExprInstance::EPlusBody(EPlus {
            p1: Some(gint(i64::MAX)),
            p2: Some(gint(1)),
        })),
        par_of(ExprInstance::EMinusBody(EMinus {
            p1: Some(gint(9)),
            p2: Some(gint(4)),
        })),
        par_of(ExprInstance::EMultBody(EMult {
            p1: Some(gint(6)),
            p2: Some(gint(7)),
        })),
        par_of(ExprInstance::EDivBody(EDiv {
            p1: Some(gint(9)),
            p2: Some(gint(0)),
        })),
        par_of(ExprInstance::EModBody(EMod {
            p1: Some(gint(9)),
            p2: Some(gint(4)),
        })),
        ematches(gint(5), gint(5)),
    ];

    for case in &cases {
        assert_eq!(
            eval(case, &env),
            eval_with(case, &env, &NoSpatialMatch),
            "eval and eval_with(&NoSpatialMatch) diverged on {case:?}"
        );
    }
}

// --- (b) an injected oracle decides ----------------------------------

#[test]
fn ematches_with_oracle_returns_true_for_a_matching_pair() {
    let env = Env::<Par>::new();
    let expr = ematches(gint(5), gint(5));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        true,
    );
}

#[test]
fn ematches_with_oracle_returns_false_for_a_non_matching_pair() {
    let env = Env::<Par>::new();
    let expr = ematches(gint(5), gstr("five"));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        false,
    );
}

#[test]
fn ematches_composes_with_the_propositional_guard_language() {
    // A guard is an ordinary boolean Par: the spatial verdict has to
    // compose with `and` / `not` / comparisons like any other operand.
    let env = Env::<Par>::new();
    let expr = par_of(ExprInstance::EAndBody(EAnd {
        p1: Some(ematches(gint(5), gint(5))),
        p2: Some(par_of(ExprInstance::ENotBody(ENot {
            p: Some(ematches(gint(5), gstr("five"))),
        }))),
    }));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        true,
    );
}

#[test]
fn ematches_target_is_evaluated_before_matching() {
    // `x matches 12` with x bound to 5+7: the oracle must be asked about
    // the VALUE 12, not about the unevaluated sum.
    let env = Env::<Par>::new();
    let sum = par_of(ExprInstance::EPlusBody(EPlus {
        p1: Some(gint(5)),
        p2: Some(gint(7)),
    }));
    let expr = ematches(sum, gint(12));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        true,
    );
}

#[test]
fn ematches_propagates_an_error_from_its_target() {
    // An unbound target is an eval error, not a `false` verdict. The
    // guard layer above collapses both to guard-fail, but the evaluator
    // must keep them distinguishable for diagnostics.
    let env = Env::<Par>::new();
    let expr = ematches(evar(99), gint(1));
    assert_eq!(
        eval_with(&expr, &env, &StructuralEqualityOracle),
        Err(EvalError::UnboundVariable { index: 99 })
    );
}

// --- (c) determinism --------------------------------------------------

#[test]
fn ematches_verdict_is_deterministic_across_repeated_calls() {
    // The determinism contract (lib.rs) is what makes this crate safe to
    // call from the rspace matcher and from casper replay. Asking the
    // same (target, pattern) twice must give byte-identical results, and
    // the oracle must see byte-identical questions — no state may carry
    // over between calls.
    let mut env = Env::<Par>::new();
    let env = env.put(gint(7));
    let expr = ematches(evar(0), gint(7));

    let oracle = RecordingOracle::new(true);
    let first = eval_with(&expr, &env, &oracle).expect("EMatches must evaluate");
    let second = eval_with(&expr, &env, &oracle).expect("EMatches must evaluate");

    assert_eq!(first, second);
    assert_bool(&first, true);

    let questions = oracle.questions();
    assert_eq!(questions.len(), 2, "one question per evaluation");
    assert_eq!(
        questions[0], questions[1],
        "the same expression must produce the same question"
    );
}

#[test]
fn ematches_repeated_within_one_evaluation_is_stable() {
    // Two occurrences of the same spatial test inside one guard must
    // agree — the property that would break first if an oracle carried
    // mutable state between calls.
    let env = Env::<Par>::new();
    let one = ematches(gint(5), gint(5));
    let expr = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(one.clone()),
        p2: Some(one),
    }));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        true,
    );
}

// --- (d) the depth-1 substitution question ----------------------------

#[test]
fn ematches_pattern_is_passed_verbatim_not_evaluated() {
    // A pattern's free variables are BINDERS. `eval_receive` substitutes
    // the guard at depth 1, and `maybe_substitute_var` is the identity at
    // depth != 0, so those binders are still present when the guard
    // reaches this evaluator. Evaluating the pattern would therefore
    // raise `UnboundVariable` and turn every spatial guard into a
    // guard-fail; the arm must hand the pattern over untouched.
    let env = Env::<Par>::new();
    let pattern = free_var(0);

    // Control: evaluating that pattern on its own IS an error.
    assert_eq!(
        eval_with(&pattern, &env, &StructuralEqualityOracle),
        Err(EvalError::UnboundVariable { index: 0 })
    );

    // Treatment: as an EMatches pattern it survives verbatim.
    let oracle = RecordingOracle::new(true);
    let expr = ematches(gint(5), pattern.clone());
    assert_bool(
        &eval_with(&expr, &env, &oracle).expect("EMatches must evaluate"),
        true,
    );
    let questions = oracle.questions();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].1, pattern, "pattern must arrive verbatim");
}

#[test]
fn ematches_pattern_referencing_an_outer_bound_variable_is_not_resolved() {
    // The guard for `for (x <- ch) where (x matches <pattern mentioning x>)`.
    // BoundVar(0) is the receive-bound `x`: the matcher put its value in
    // the env, so the TARGET occurrence resolves to that value, while the
    // PATTERN occurrence stays a variable. This is exactly what the
    // reducer does in `combine_matches`, which substitutes the target at
    // depth 0 (resolving) and the pattern at depth 1 (identity on EVars).
    let mut env = Env::<Par>::new();
    let env = env.put(gint(7));

    let oracle = RecordingOracle::new(false);
    let expr = ematches(evar(0), evar(0));
    assert_bool(
        &eval_with(&expr, &env, &oracle).expect("EMatches must evaluate"),
        false,
    );

    let questions = oracle.questions();
    assert_eq!(questions.len(), 1);
    let (asked_target, asked_pattern) = &questions[0];
    assert_eq!(
        asked_target,
        &gint(7),
        "the target must be env-resolved before matching"
    );
    assert_eq!(
        asked_pattern,
        &evar(0),
        "the pattern must NOT be env-resolved"
    );
}

#[test]
fn ematches_nested_under_an_operator_still_sees_the_oracle() {
    // The oracle has to be threaded through every recursive arm, not just
    // the top level — a guard is almost always a compound expression.
    let mut env = Env::<Par>::new();
    let env = env.put(gint(7));
    let expr = par_of(ExprInstance::EOrBody(EOr {
        p1: Some(par_of(ExprInstance::ENotBody(ENot {
            p: Some(ematches(evar(0), gint(7))),
        }))),
        p2: Some(gbool(false)),
    }));
    assert_bool(
        &eval_with(&expr, &env, &StructuralEqualityOracle).expect("EMatches must evaluate"),
        false,
    );
}
