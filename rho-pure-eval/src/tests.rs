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
    models::par_from_default! {
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
    let par = models::par_from_default! {
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

// =====================================================================
// The DECIDABILITY GATE — `crate::decidable`.
//
// ★ The gate exists so a caller can refuse a guard it cannot decide,
// instead of reporting `false` for one. It is only worth anything if it
// agrees with the evaluator, so the tests below are DIFFERENTIALS: they
// run both and compare, over a corpus carrying one representative of
// every `ExprInstance` variant.
// =====================================================================

use crate::decidable::{undecidable_nodes, SpatialSupport, UndecidableNode};

fn method(name: &str, target: Par) -> Par {
    par_of(ExprInstance::EMethodBody(models::rhoapi::EMethod {
        method_name: name.to_string(),
        target: Some(target),
        arguments: vec![gint(0)],
        locally_free: Vec::new(),
        connective_used: false,
    }))
}

fn elist(items: Vec<Par>) -> Par {
    par_of(ExprInstance::EListBody(models::rhoapi::EList {
        ps: items,
        locally_free: Vec::new(),
        connective_used: false,
        remainder: None,
    }))
}

/// One representative of EVERY `ExprInstance` variant, paired with the
/// `kind` the evaluator raises for it (or `None` when the evaluator has a
/// real arm).
///
/// ★ The `SpatialSupport` here is `Available`, so `EMatchesBody` is
/// expected to be decidable — this is the world both `rholang` guard
/// sites are in. `NoSpatialMatch`'s world is covered separately by
/// [`the_gate_tracks_the_oracle_for_ematches`].
fn every_variant() -> Vec<(&'static str, Par, Option<&'static str>)> {
    vec![
        ("GBool", gbool(true), None),
        ("GInt", gint(1), None),
        ("GString", gstr("s"), None),
        (
            "GUri",
            par_of(ExprInstance::GUri("rho:io:stdout".to_string())),
            None,
        ),
        (
            "GByteArray",
            par_of(ExprInstance::GByteArray(vec![1, 2])),
            None,
        ),
        ("GDouble", gdouble(1.0), None),
        ("GBigInt", par_of(ExprInstance::GBigInt(vec![1])), None),
        (
            "GBigRat",
            par_of(ExprInstance::GBigRat(models::rhoapi::GBigRational {
                numerator: vec![1],
                denominator: vec![2],
            })),
            None,
        ),
        (
            "GFixedPoint",
            par_of(ExprInstance::GFixedPoint(models::rhoapi::GFixedPoint {
                unscaled: vec![1],
                scale: 2,
            })),
            None,
        ),
        ("EList", elist(vec![gint(1)]), None),
        (
            "ETuple",
            par_of(ExprInstance::ETupleBody(models::rhoapi::ETuple {
                ps: vec![gint(1)],
                locally_free: Vec::new(),
                connective_used: false,
            })),
            None,
        ),
        (
            "ESet",
            par_of(ExprInstance::ESetBody(models::rhoapi::ESet {
                ps: vec![gint(1)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
            None,
        ),
        (
            "EMap",
            par_of(ExprInstance::EMapBody(models::rhoapi::EMap {
                kvs: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
            None,
        ),
        ("EVar", evar(0), None),
        (
            "ENot",
            par_of(ExprInstance::ENotBody(ENot {
                p: Some(gbool(true)),
            })),
            None,
        ),
        (
            "ENeg",
            par_of(ExprInstance::ENegBody(ENeg { p: Some(gint(1)) })),
            None,
        ),
        (
            "EAnd",
            par_of(ExprInstance::EAndBody(EAnd {
                p1: Some(gbool(true)),
                p2: Some(gbool(true)),
            })),
            None,
        ),
        (
            "EOr",
            par_of(ExprInstance::EOrBody(EOr {
                p1: Some(gbool(true)),
                p2: Some(gbool(false)),
            })),
            None,
        ),
        (
            "EEq",
            par_of(ExprInstance::EEqBody(EEq {
                p1: Some(gint(1)),
                p2: Some(gint(1)),
            })),
            None,
        ),
        (
            "ENeq",
            par_of(ExprInstance::ENeqBody(ENeq {
                p1: Some(gint(1)),
                p2: Some(gint(2)),
            })),
            None,
        ),
        (
            "ELt",
            par_of(ExprInstance::ELtBody(ELt {
                p1: Some(gint(1)),
                p2: Some(gint(2)),
            })),
            None,
        ),
        (
            "ELte",
            par_of(ExprInstance::ELteBody(ELte {
                p1: Some(gint(1)),
                p2: Some(gint(2)),
            })),
            None,
        ),
        (
            "EGt",
            par_of(ExprInstance::EGtBody(EGt {
                p1: Some(gint(2)),
                p2: Some(gint(1)),
            })),
            None,
        ),
        (
            "EGte",
            par_of(ExprInstance::EGteBody(EGte {
                p1: Some(gint(2)),
                p2: Some(gint(1)),
            })),
            None,
        ),
        (
            "EPlus",
            par_of(ExprInstance::EPlusBody(EPlus {
                p1: Some(gint(1)),
                p2: Some(gint(1)),
            })),
            None,
        ),
        (
            "EMinus",
            par_of(ExprInstance::EMinusBody(EMinus {
                p1: Some(gint(1)),
                p2: Some(gint(1)),
            })),
            None,
        ),
        (
            "EMult",
            par_of(ExprInstance::EMultBody(EMult {
                p1: Some(gint(2)),
                p2: Some(gint(3)),
            })),
            None,
        ),
        (
            "EDiv",
            par_of(ExprInstance::EDivBody(EDiv {
                p1: Some(gint(6)),
                p2: Some(gint(3)),
            })),
            None,
        ),
        (
            "EMod",
            par_of(ExprInstance::EModBody(EMod {
                p1: Some(gint(7)),
                p2: Some(gint(3)),
            })),
            None,
        ),
        ("EMatches", ematches(gint(5), gint(5)), None),
        (
            "EMethod",
            method("nth", elist(vec![gint(1)])),
            Some("EMethodBody"),
        ),
        (
            "EPercentPercent",
            par_of(ExprInstance::EPercentPercentBody(
                models::rhoapi::EPercentPercent {
                    p1: Some(gstr("{}")),
                    p2: Some(gint(1)),
                },
            )),
            Some("EPercentPercentBody"),
        ),
        (
            "EPlusPlus",
            par_of(ExprInstance::EPlusPlusBody(models::rhoapi::EPlusPlus {
                p1: Some(gstr("a")),
                p2: Some(gstr("b")),
            })),
            Some("EPlusPlusBody"),
        ),
        (
            "EMinusMinus",
            par_of(ExprInstance::EMinusMinusBody(models::rhoapi::EMinusMinus {
                p1: Some(elist(vec![gint(1)])),
                p2: Some(elist(vec![gint(1)])),
            })),
            Some("EMinusMinusBody"),
        ),
        (
            "EPathmap",
            par_of(ExprInstance::EPathmapBody(
                models::rhoapi::EPathMap::default(),
            )),
            Some("EPathmapBody"),
        ),
        (
            "EZipper",
            par_of(ExprInstance::EZipperBody(models::rhoapi::EZipper::default())),
            Some("EZipperBody"),
        ),
    ]
}

#[test]
fn the_corpus_covers_every_expr_instance_variant() {
    // ⚠ ANTI-VACUITY. Every differential below is only as good as the
    // corpus it ranges over, so the corpus size is pinned against the
    // number of `ExprInstance` variants counted from RhoTypes.proto's
    // `oneof expr_instance`: 9 grounds + 4 collections + EVar + 15
    // operators + EMatches + 6 undecidable = 36. Adding a variant to the
    // protobuf without adding a row here fails HERE, before it can fail
    // silently in production.
    assert_eq!(
        every_variant().len(),
        36,
        "the differential corpus must carry one representative of every \
         ExprInstance variant"
    );
}

#[test]
fn the_gate_and_the_evaluator_agree_on_every_variant() {
    // ★ THE DIFFERENTIAL. For each variant: does the evaluator raise
    // `UnsupportedExpression`, and does the gate say so? Both directions
    // at once — a gate that under-reports restores the silence, and one
    // that over-reports refuses guards that work.
    let mut env = Env::<Par>::new();
    let env = env.put(gint(1));
    for (name, par, expected_kind) in every_variant() {
        let evaluated = eval_with(&par, &env, &StructuralEqualityOracle);
        let gated = undecidable_nodes(&par, SpatialSupport::Available);

        let evaluator_kind = match &evaluated {
            Err(EvalError::UnsupportedExpression { kind }) => Some(*kind),
            _ => None,
        };
        assert_eq!(
            evaluator_kind, expected_kind,
            "{name}: the evaluator's verdict moved; got {evaluated:?}"
        );
        assert_eq!(
            gated.iter().map(|n| n.kind).collect::<Vec<_>>(),
            expected_kind.into_iter().collect::<Vec<_>>(),
            "{name}: the gate disagrees with the evaluator"
        );
    }
}

#[test]
fn a_gated_guard_never_evaluates_cleanly() {
    // PRECISION, stated as the contrapositive that matters: if the gate
    // objects, the evaluator would have failed anyway — so refusing costs
    // no guard that used to work.
    let mut env = Env::<Par>::new();
    let env = env.put(elist(vec![gint(1)]));
    for (name, par, expected_kind) in every_variant() {
        if expected_kind.is_none() {
            continue;
        }
        assert!(
            undecidable_nodes(&par, SpatialSupport::Available).is_empty()
                == eval_with(&par, &env, &StructuralEqualityOracle).is_ok(),
            "{name}: gate-objects and evaluator-fails must coincide"
        );
    }
}

#[test]
fn the_gate_tracks_the_oracle_for_ematches() {
    // `EMatches` is decidable exactly when an oracle is injected, and the
    // gate must say the same thing — otherwise `eval`'s callers get a
    // gate that lies in one direction and `eval_with`'s in the other.
    let expr = ematches(gint(5), gint(5));
    let env = Env::<Par>::new();

    assert!(undecidable_nodes(&expr, SpatialSupport::Available).is_empty());
    assert!(eval_with(&expr, &env, &StructuralEqualityOracle).is_ok());

    assert_eq!(undecidable_nodes(&expr, SpatialSupport::Absent), vec![
        UndecidableNode {
            kind: "EMatchesBody",
            detail: None
        }
    ]);
    assert_eq!(
        eval(&expr, &env),
        Err(EvalError::UnsupportedExpression {
            kind: "EMatchesBody"
        })
    );
}

#[test]
fn the_gate_finds_a_method_nested_under_operators() {
    // ★ THE MEASURED WITNESS: `terms.nth(0) == terms.nth(0)`, a syntactic
    // tautology whose guard admitted nothing. A gate that only looked at
    // the top-level expr would miss it — the method sits two levels down,
    // under `==`.
    let nth = || method("nth", evar(0));
    let guard = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(nth()),
        p2: Some(nth()),
    }));
    assert_eq!(
        undecidable_nodes(&guard, SpatialSupport::Available),
        vec![
            UndecidableNode {
                kind: "EMethodBody",
                detail: Some("`nth`".to_string())
            },
            UndecidableNode {
                kind: "EMethodBody",
                detail: Some("`nth`".to_string())
            },
        ],
        "both occurrences must be reported, in source order"
    );
}

#[test]
fn the_gate_ignores_a_method_the_evaluator_never_reaches() {
    // ⚠ PRECISION, the direction that is easy to get wrong. Collections
    // are passed through UNCHANGED by `eval_drive` — their interiors are
    // never evaluated — so a method inside a list literal is inert and
    // gating it would refuse a guard that works today.
    let inert = elist(vec![method("nth", elist(vec![gint(1)]))]);
    let guard = par_of(ExprInstance::EEqBody(EEq {
        p1: Some(inert.clone()),
        p2: Some(inert),
    }));
    assert!(
        undecidable_nodes(&guard, SpatialSupport::Available).is_empty(),
        "a method inside a collection literal is never evaluated"
    );
    // And the evaluator confirms it: the guard decides, cleanly, as true.
    let env = Env::<Par>::new();
    assert_bool(
        &eval_with(&guard, &env, &StructuralEqualityOracle)
            .expect("an inert method must not stop the guard"),
        true,
    );
}

#[test]
fn the_gate_ignores_a_method_inside_a_matches_pattern() {
    // The same precision question for the OTHER unevaluated position. A
    // `matches` pattern is handed to the oracle verbatim; the evaluator
    // never walks it, so a method there is inert too.
    let guard = ematches(gint(5), method("nth", elist(vec![gint(1)])));
    assert!(
        undecidable_nodes(&guard, SpatialSupport::Available).is_empty(),
        "a `matches` pattern is never evaluated"
    );
    let env = Env::<Par>::new();
    assert!(eval_with(&guard, &env, &StructuralEqualityOracle).is_ok());
}

#[test]
fn the_gate_does_not_walk_process_level_fields() {
    // A guard Par may carry sends/receives alongside its exprs; the
    // evaluator carries them through inert. A method inside a receive
    // BODY is a piece of the program, not a piece of the predicate.
    let mut guard = par_of(ExprInstance::GBool(true));
    guard.sends = vec![models::rhoapi::Send {
        chan: Some(gstr("c")),
        data: vec![method("nth", elist(vec![gint(1)]))],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    }];
    assert!(
        undecidable_nodes(&guard, SpatialSupport::Available).is_empty(),
        "process-level fields are not part of the decided expression"
    );
}

#[test]
fn the_gate_is_stack_safe_on_a_deeply_nested_guard() {
    // The walk is an explicit worklist, never the native stack, so a guard
    // nested far past any recursive walker's ceiling is classified without
    // overflowing.
    //
    // ⚠ MEASURED CEILING, and it is NOT this walk's. At 32 000 the test
    // aborts in `<Par as Drop>::drop` — prost's derived `Drop` is recursive,
    // a pre-existing Θ(depth) SCC that has nothing to do with the gate. The
    // depth below is the largest that can be BUILT AND DROPPED in a debug
    // test thread; [`the_gate_walks_deeper_than_the_par_can_be_dropped`]
    // takes the walk past that ceiling by never dropping the guard, which
    // is what isolates the two.
    const DEPTH: usize = 16_000;
    let mut guard = method("nth", evar(0));
    for _ in 0..DEPTH {
        guard = par_of(ExprInstance::ENotBody(ENot { p: Some(guard) }));
    }
    assert_eq!(
        undecidable_nodes(&guard, SpatialSupport::Available)
            .iter()
            .map(|n| n.kind)
            .collect::<Vec<_>>(),
        vec!["EMethodBody"]
    );
}

#[test]
fn the_gate_walks_deeper_than_the_par_can_be_dropped() {
    // ★ The isolation. `Box::leak` removes `<Par as Drop>::drop` — the ONLY
    // recursive step in the previous test — and the same walk then completes
    // at 4× the depth that aborted with the drop in place. So the abort was
    // the destructor's and the walk's own space is not depth-bounded.
    //
    // Deliberately leaks: reclaiming the memory is exactly the recursive
    // teardown being excluded, and the process is about to exit.
    const DEPTH: usize = 128_000;
    let mut guard = method("nth", evar(0));
    for _ in 0..DEPTH {
        guard = par_of(ExprInstance::ENotBody(ENot { p: Some(guard) }));
    }
    let guard: &'static Par = Box::leak(Box::new(guard));
    assert_eq!(
        undecidable_nodes(guard, SpatialSupport::Available)
            .iter()
            .map(|n| n.kind)
            .collect::<Vec<_>>(),
        vec!["EMethodBody"]
    );
}
