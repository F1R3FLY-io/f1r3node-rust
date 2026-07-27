//! An UNDECIDABLE `where` guard must be distinguishable from a FALSE one.
//!
//! # The defect this file pins
//!
//! A lowered `where` guard is decided by `rho-pure-eval` inside the RSpace
//! matcher (`Matcher::check_commit` → `guard_passes`). That decider implements
//! a *subset* of `Reduce::eval_expr`; every node kind outside the subset yields
//! `EvalError::UnsupportedExpression { kind }`. `guard_passes` mapped **every**
//! `Err` to `false`, and `false` is exactly the verdict a genuinely refuted
//! guard produces — so a guard the node could not decide at all was reported as
//! a guard the node had decided against.
//!
//! The user-visible consequence is silence. The program normalizes, the deploy
//! runs, no error is raised, the COMM never fires, and nothing distinguishes
//! *"the guard was false for every datum"* from *"the guard could not be
//! evaluated at all."*
//!
//! ## The witness
//!
//! `terms.nth(0) == terms.nth(0)` is a syntactic **tautology**: both operands
//! are the same expression, so under any total evaluation semantics it is
//! `true`. A decider that answers `false` to it is wrong by construction, which
//! is what makes it the sharpest possible test datum — no appeal to what `nth`
//! *means* is required to know the answer.
//!
//! # What is asserted here
//!
//! ```text
//!                        ┌──────────────── COMPILE LAYER ────────────────┐
//!   Rholang source ─────▶│ p_input_normalizer / p_match_normalizer       │──▶ Par
//!                        │  reject_undecidable_guard  ⇒ UndecidableGuard │
//!                        └───────────────────────────────────────────────┘
//!                                                                          │
//!   Par built directly ─────────────────────────────────────────────────┐  │
//!   (embedders never meet the normalizer)                               ▼  ▼
//!                        ┌───────────────── REDUCE LAYER ────────────────────┐
//!                        │ eval_receive (before consume) / eval_match        │
//!                        │  reject_undecidable_guard  ⇒ UndecidableGuard     │
//!                        └───────────────────────────────────────────────────┘
//!                                                                          │
//!                        ┌──────────────── MATCHER LAYER ────────────────┐  ▼
//!                        │ guard_disposition ⇒ Admits │ Refutes │ …      │
//!                        │  Undecidable is now unreachable, and SAID SO  │
//!                        └───────────────────────────────────────────────┘
//! ```
//!
//! §1 pins the compile layer on the measured witness, §2 the whole enumerated
//! class, §3 the reduce layer (which is what covers programmatically-built
//! `Par`s), and §4 ★ the separation itself — three guards, three distinct
//! observable outcomes.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, EEq, EMethod, Expr, ListParWithRandom, Par, Receive, ReceiveBind, Send,
    TaggedContinuation,
};
use models::rust::utils::{
    new_boundvar_par, new_elist_par, new_freevar_par, new_gint_par, new_gstring_par,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::matcher::r#match::{guard_disposition, GuardDisposition};
use rholang::rust::interpreter::test_utils::persistent_store_tester::create_test_space;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────

/// `for (@terms <- c where <guard>) { … }` with one datum on `c`.
///
/// The receive is written so the *only* thing that varies between the rows
/// below is the guard text: same channel, same datum, same body. Any
/// difference in outcome is therefore attributable to the guard alone.
fn program_with_guard(guard: &str) -> String {
    format!(
        r#"
        new c, out in {{
            c!([1]) |
            for (@terms <- c where {guard}) {{ out!("fired") }}
        }}
        "#
    )
}

fn compile(src: &str) -> Result<(), String> {
    Compiler::source_to_adt_with_normalizer_env(src, HashMap::new())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// `terms.nth(0)` as a Par. `BoundVar(0)` is the receive's first (and only)
/// bound variable, which the matcher puts at de Bruijn index 0.
fn nth_of_first_bound() -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EMethodBody(EMethod {
            method_name: "nth".to_string(),
            target: Some(new_boundvar_par(0, models::create_bit_vector(&[0]), false)),
            arguments: vec![new_gint_par(0, Vec::new(), false)],
            locally_free: Vec::new(),
            connective_used: false,
        })),
    }])
}

fn eq(p1: Par, p2: Par) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EEqBody(EEq {
            p1: Some(p1),
            p2: Some(p2),
        })),
    }])
}

/// ★ `terms.nth(0) == terms.nth(0)` — the tautology, built directly as a Par
/// so it bypasses the normalizer's gate and reaches the reduce layer.
fn tautological_method_guard() -> Par { eq(nth_of_first_bound(), nth_of_first_bound()) }

/// `terms == [2]` — decidable, and FALSE for the datum `[1]`.
fn decidably_false_guard() -> Par {
    eq(
        new_boundvar_par(0, models::create_bit_vector(&[0]), false),
        one_element_list(2),
    )
}

/// `terms == [1]` — decidable, and TRUE for the datum `[1]`.
fn decidably_true_guard() -> Par {
    eq(
        new_boundvar_par(0, models::create_bit_vector(&[0]), false),
        one_element_list(1),
    )
}

fn one_element_list(n: i64) -> Par {
    new_elist_par(
        vec![new_gint_par(n, Vec::new(), false)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )
}

fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&Vec::new()) }

/// The outcome of running `c!([1]) | for (@terms <- c where G) { }` on a real
/// space, reported as the two things a user can actually observe.
#[derive(Debug, PartialEq)]
struct RunOutcome {
    /// What the deploy reported. `None` = it ran cleanly.
    error: Option<InterpreterError>,
    /// Did the COMM fire? Measured as "the tuple space is empty", which is
    /// what the Phase-7 guard tests in `reduce_spec.rs` measure.
    comm_fired: bool,
}

/// Runs the fixed program with `guard` on a fresh space.
///
/// The datum, the channel and the pattern are identical in every call, so the
/// guard is the only independent variable.
async fn run_with_guard(guard: Par) -> RunOutcome {
    let (space, reducer) =
        create_test_space::<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>()
            .await;

    let chan = new_gstring_par("c".to_string(), Vec::new(), false);
    let send = Par::default().with_sends(vec![Send {
        chan: Some(chan.clone()),
        data: vec![one_element_list(1)],
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
    }]);
    let receive = Par::default().with_receives(vec![Receive {
        binds: vec![ReceiveBind {
            patterns: vec![new_freevar_par(0, Vec::new())],
            source: Some(chan),
            remainder: None,
            free_count: 1,
        }],
        body: Some(Par::default()),
        persistent: false,
        peek: false,
        bind_count: 1,
        locally_free: Vec::new(),
        connective_used: false,
        condition: Some(guard),
    }]);

    let env: Env<Par> = Env::new();
    reducer
        .eval(send, &env, rand().split_byte(0))
        .await
        .expect("the send must not fail — it is the control half of the fixture");
    let error = reducer
        .eval(receive, &env, rand().split_byte(1))
        .await
        .err();

    RunOutcome {
        error,
        comm_fired: space.to_map().await.is_empty(),
    }
}

// =========================================================================
// §1. The compile layer, on the measured witness
// =========================================================================

#[test]
fn the_tautological_method_guard_is_refused_at_compile_time() {
    // ★ THE WITNESS. `terms.nth(0) == terms.nth(0)` cannot be false. Before
    // the gate this normalized cleanly, ran, and admitted nothing.
    let err = compile(&program_with_guard("terms.nth(0) == terms.nth(0)"))
        .expect_err("a guard the decider cannot decide must not normalize");
    assert!(
        err.contains("cannot be decided"),
        "refusal must be the guard-specific error, got: {err}"
    );
}

#[test]
fn the_refusal_names_the_node_kind_the_guard_and_the_remedy() {
    // A refusal the user cannot act on is only a quieter silence. The message
    // must say WHICH construct is undecidable, WHERE it was found, and what to
    // do instead — the workaround (bind it in the PATTERN) is undiscoverable
    // from a silent failure, which is most of what made this defect severe.
    let err =
        compile(&program_with_guard("terms.nth(0) == terms.nth(0)")).expect_err("must be refused");
    assert!(
        err.contains("method call"),
        "must name the construct: {err}"
    );
    assert!(err.contains("`nth`"), "must name the method: {err}");
    assert!(err.contains("`where`"), "must name the guard clause: {err}");
    assert!(err.contains("PATTERN"), "must name the remedy: {err}");
}

#[test]
fn the_match_case_guard_is_refused_too() {
    // `match … where` is decided by the SAME evaluator (reduce.rs's case-guard
    // arm and the matcher both call `guard_disposition_in_env`), so it inherits
    // the same undecidable class and must inherit the same refusal. Fixing only
    // `for … where` is how this defect class resurfaced.
    let src = r#"
        new out in {
            match [1] {
                terms where terms.nth(0) == terms.nth(0) => { out!("fired") }
                _ => { out!("fell through") }
            }
        }
    "#;
    let err = compile(src).expect_err("an undecidable case guard must not normalize");
    assert!(err.contains("match"), "must name the case clause: {err}");
    assert!(err.contains("cannot be decided"), "got: {err}");
}

// =========================================================================
// §2. The WHOLE enumerated class, and its complement
// =========================================================================

#[test]
fn every_undecidable_node_kind_reachable_from_source_is_refused() {
    // `EMethodBody` is the arm that was MEASURED. It is not the arm that
    // matters — the class does. `rho-pure-eval`'s `ExprInstance` match is
    // exhaustive with no catch-all, so the undecidable set is closed and
    // finite. These are its members that have a surface syntax; the two that
    // do not (`EZipperBody` is only reachable *through* a method call, so the
    // method is refused first) are covered at the Par level by
    // `rho-pure-eval`'s own `the_gate_and_the_evaluator_agree_on_every_variant`.
    let rows: [(&str, &str, &str); 4] = [
        ("EMethodBody", "terms.nth(0) == terms.nth(0)", "method call"),
        (
            "EPercentPercentBody",
            r#"("x: {}" %% {"s": 1}) == "y""#,
            "string interpolation",
        ),
        ("EPlusPlusBody", "(terms ++ [2]) == [1, 2]", "concatenation"),
        ("EMinusMinusBody", "(terms -- [1]) == []", "difference"),
    ];
    for (kind, guard, phrase) in rows {
        let err = compile(&program_with_guard(guard))
            .expect_err(&format!("`{kind}` guard `{guard}` must be refused"));
        assert!(
            err.contains(phrase),
            "`{kind}` must be refused and named `{phrase}`, got: {err}"
        );
    }
}

#[test]
fn a_pathmap_literal_in_a_guard_is_refused() {
    // Split out because its surface (`{| … |}`) is unlike the operator rows
    // and a reader should be able to see the spelling that produces it.
    let err = compile(&program_with_guard("{| [1] |} == {| [1] |}"))
        .expect_err("a pathmap literal is not decidable in a guard");
    assert!(err.contains("pathmap literal"), "got: {err}");
}

#[test]
fn a_decidable_guard_still_compiles() {
    // ⚠ THE FALSE-ALARM CONTROL. The gate must reject exactly the undecidable
    // class and nothing else; a gate that also refuses working guards has
    // traded a silent failure for a loud one. `matches` is the sharpest row:
    // it is refused by `rho_pure_eval::eval` and DECIDABLE at a guard site,
    // because both guard sites inject a spatial oracle.
    for guard in [
        "true",
        "terms == [1]",
        "terms != [2]",
        "not (terms == [2])",
        "terms == [1] and true",
        "terms == [1] or false",
        "terms matches [1]",
        "terms matches [_]",
        "1 + 1 == 2",
        "1 < 2",
        "(3 - 1) * 2 / 4 % 3 == 1",
        "-1 < 0",
        // ★ A method in an UNEVALUATED position. The evaluator passes
        // collection literals through without descending, so this method is
        // inert and gating it would refuse a guard that works.
        "terms == [1]  and  [1] == [1]",
    ] {
        compile(&program_with_guard(guard))
            .unwrap_or_else(|e| panic!("decidable guard `{guard}` must still compile: {e}"));
    }
}

// =========================================================================
// §3. The reduce layer — the one that covers Pars built without a normalizer
// =========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_programmatically_built_undecidable_guard_is_refused_by_the_reducer() {
    // ★ The layer that matters for embedders. This `Receive` never met the
    // normalizer — it is a `Par` handed straight to `Reduce` — so the compile
    // gate cannot have fired. The reducer must refuse it anyway, BEFORE the
    // consume, which is why the error arrives with the space untouched.
    let outcome = run_with_guard(tautological_method_guard()).await;
    match outcome.error {
        Some(InterpreterError::UndecidableGuard {
            clause,
            obstructions,
        }) => {
            assert_eq!(clause, "where");
            assert_eq!(obstructions, vec![
                "a method call (`nth`)".to_string(),
                "a method call (`nth`)".to_string()
            ]);
        }
        other => panic!("expected UndecidableGuard, got {other:?}"),
    }
}

#[test]
fn the_matcher_reports_an_undecidable_guard_instead_of_refuting_it() {
    // Below every gate, at the decider itself. This is the arm that used to
    // return `false` — the single bit that made the whole defect silent.
    assert_eq!(
        guard_disposition(&tautological_method_guard(), &[one_element_list(1)]),
        GuardDisposition::Undecidable {
            kind: "EMethodBody"
        },
        "the tautology must be reported undecidable, never refuted"
    );
}

#[test]
fn the_matcher_still_refutes_a_decidably_false_guard() {
    // The control that keeps the fix honest: `[1] == [2]` is decidable and
    // false, and must stay `Refutes`. If this became `Undecidable` the fix
    // would be turning ordinary non-matching data into deploy failures.
    assert_eq!(
        guard_disposition(&decidably_false_guard(), &[one_element_list(1)]),
        GuardDisposition::Refutes
    );
    assert!(!guard_disposition(&decidably_false_guard(), &[one_element_list(1)]).commits());
}

#[test]
fn the_matcher_admits_a_decidably_true_guard() {
    assert_eq!(
        guard_disposition(&decidably_true_guard(), &[one_element_list(1)]),
        GuardDisposition::Admits
    );
    assert!(guard_disposition(&decidably_true_guard(), &[one_element_list(1)]).commits());
}

// =========================================================================
// §4. ★ THE SEPARATION — the property the whole task turns on
// =========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn false_and_undecidable_produce_different_observable_outcomes() {
    // ★★ THE HEADLINE. Three guards, one program, one datum. Before the fix
    // the first two rows were BYTE-IDENTICAL from outside: clean compile,
    // clean run, no error, no publication. They are now three distinct
    // outcomes, and a user can tell which one they got without reading the
    // node's source.
    //
    //   guard                         error            COMM
    //   ───────────────────────────── ──────────────── ──────
    //   terms == [1]      (TRUE)      none             FIRES
    //   terms == [2]      (FALSE)     none             rests     ← was
    //   terms.nth(0)==…   (UNDECID.)  UndecidableGuard rests     ←  identical

    let admits = run_with_guard(decidably_true_guard()).await;
    assert_eq!(admits.error, None, "a true guard must not raise");
    assert!(admits.comm_fired, "a true guard must fire the COMM");

    let refutes = run_with_guard(decidably_false_guard()).await;
    assert_eq!(
        refutes.error, None,
        "a FALSE guard is an ordinary outcome and must NOT raise"
    );
    assert!(
        !refutes.comm_fired,
        "a false guard must leave the datum resting"
    );

    let undecidable = run_with_guard(tautological_method_guard()).await;
    assert!(
        matches!(
            undecidable.error,
            Some(InterpreterError::UndecidableGuard { .. })
        ),
        "an UNDECIDABLE guard must raise, got {:?}",
        undecidable.error
    );
    assert!(
        !undecidable.comm_fired,
        "and it must still fail closed — no COMM on a guard that was never decided"
    );

    // The separation, stated as the inequality that used to be an equality.
    assert_ne!(
        refutes, undecidable,
        "a refuted guard and an undecidable one must not be observationally equal"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_undecidable_guard_fails_closed_as_well_as_loudly() {
    // ⚠ Loud must not mean permissive. The refusal happens BEFORE the consume,
    // so the datum is still resting and the continuation was never installed —
    // the space is exactly as the send left it. A "loud" fix that also fired
    // the COMM would be unsound in the firing direction, which is worse than
    // the silence it replaced.
    let outcome = run_with_guard(tautological_method_guard()).await;
    assert!(outcome.error.is_some());
    assert!(
        !outcome.comm_fired,
        "the space must still hold the un-consumed datum"
    );
}

// =========================================================================
// §5. ★ THE COMPLETE ENUMERATION — every `EvalError` a lowered guard can
//     reach, and which of them the gate covers
//
// Fixing the ONE arm that was measured and leaving its siblings is how this
// class recurred, so the class is enumerated here in full and each member
// carries an executed witness. `EvalError` has exactly seven variants
// (`rho-pure-eval/src/error.rs`); they split into two groups that must be
// treated differently, and the split is the whole design:
//
//   ┌─ DECIDER GAPS ── the answer does not exist on this node ──────────────┐
//   │ UnsupportedExpression{kind}, kind ∈ { EMethodBody, EPercentPercent-   │
//   │   Body, EPlusPlusBody, EMinusMinusBody, EPathmapBody, EZipperBody }   │
//   │ (+ EMatchesBody, but only without an oracle — both guard sites inject │
//   │  one, so it is decidable in a guard and is NOT in the gated class)    │
//   │ ⇒ REFUSED. A pure function of the guard term, so it can be refused    │
//   │   before anything runs. This is what §1-§4 pin.                       │
//   └───────────────────────────────────────────────────────────────────────┘
//   ┌─ DATA-DEPENDENT FAILURES ── the answer exists, this datum breaks it ──┐
//   │ UnboundVariable, OperatorTypeMismatch, DivisionByZero,                │
//   │ ArithmeticOverflow, NotASingleValue, MissingExprInstance              │
//   │ ⇒ NOT refused, and deliberately: none is a function of the guard      │
//   │   term alone (`x / y` is fine until `y` is 0), so no compile-time     │
//   │   gate can decide them, and raising per-datum would have to happen    │
//   │   inside `check_commit`. They get their own disposition               │
//   │   (`Failed`/`NotABoolean`) and an ERROR-level event instead of being  │
//   │   conflated with `Refutes`.                                           │
//   └───────────────────────────────────────────────────────────────────────┘
//
// The tests below EXECUTE the second group, so the claim "these are the ones
// that remain" is measured rather than asserted.
// =========================================================================

/// Builds `<lhs> <op> <rhs>` as a guard Par without going through the parser,
/// so shapes the surface syntax cannot express are still reachable.
fn binop(instance: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(instance),
    }])
}

#[test]
fn the_data_dependent_failures_are_reachable_and_are_not_refutations() {
    use models::rhoapi::{EDiv, EPlus};
    use rho_pure_eval::EvalError;

    let datum = [one_element_list(1)];
    let bound = || new_boundvar_par(0, models::create_bit_vector(&[0]), false);

    // ── UnboundVariable — a guard reading past the arrived bindings.
    assert_eq!(
        guard_disposition(
            &eq(new_boundvar_par(7, Vec::new(), false), one_element_list(1)),
            &datum
        ),
        GuardDisposition::Failed(EvalError::UnboundVariable { index: 7 })
    );

    // ── OperatorTypeMismatch — `terms + 1`, where `terms` is a list.
    assert_eq!(
        guard_disposition(
            &eq(
                binop(ExprInstance::EPlusBody(EPlus {
                    p1: Some(bound()),
                    p2: Some(new_gint_par(1, Vec::new(), false)),
                })),
                new_gint_par(2, Vec::new(), false)
            ),
            &datum
        ),
        GuardDisposition::Failed(EvalError::OperatorTypeMismatch {
            op: "+",
            left: "List".to_string(),
            right: Some("Int".to_string())
        })
    );

    // ── DivisionByZero — `1 / 0 == 1`.
    assert_eq!(
        guard_disposition(
            &eq(
                binop(ExprInstance::EDivBody(EDiv {
                    p1: Some(new_gint_par(1, Vec::new(), false)),
                    p2: Some(new_gint_par(0, Vec::new(), false)),
                })),
                new_gint_par(1, Vec::new(), false)
            ),
            &datum
        ),
        GuardDisposition::Failed(EvalError::DivisionByZero)
    );

    // ── ArithmeticOverflow — `i64::MAX + 1 == 0`.
    assert_eq!(
        guard_disposition(
            &eq(
                binop(ExprInstance::EPlusBody(EPlus {
                    p1: Some(new_gint_par(i64::MAX, Vec::new(), false)),
                    p2: Some(new_gint_par(1, Vec::new(), false)),
                })),
                new_gint_par(0, Vec::new(), false)
            ),
            &datum
        ),
        GuardDisposition::Failed(EvalError::ArithmeticOverflow { op: "+" })
    );

    // ── NotABoolean — a guard that evaluates cleanly to a non-verdict.
    assert_eq!(
        guard_disposition(&new_gint_par(1, Vec::new(), false), &datum),
        GuardDisposition::NotABoolean
    );

    // ⇒ NONE of them is `Refutes`. Before this change every one of them WAS
    // `false`, which is what `Refutes` means, and that is the conflation.
    for disposition in [
        guard_disposition(
            &eq(new_boundvar_par(7, Vec::new(), false), one_element_list(1)),
            &datum,
        ),
        guard_disposition(&new_gint_par(1, Vec::new(), false), &datum),
    ] {
        assert_ne!(disposition, GuardDisposition::Refutes);
        // …and every one still fails closed. Distinguishable is not permissive.
        assert!(!disposition.commits());
    }
}

#[test]
fn missing_expr_instance_and_not_a_single_value_complete_the_enumeration() {
    use rho_pure_eval::EvalError;

    let datum = [one_element_list(1)];

    // ── MissingExprInstance — a malformed Par (`expr_instance: None`). Not
    // producible from source; reachable only from a hand-built Par, which is
    // exactly why it is enumerated rather than assumed away.
    let malformed = Par::default().with_exprs(vec![Expr {
        expr_instance: None,
    }]);
    assert_eq!(
        guard_disposition(&malformed, &datum),
        GuardDisposition::Failed(EvalError::MissingExprInstance)
    );

    // ── NotASingleValue — an operand of a COMPARISON that reduces to more
    // than one expr. `<` extracts a single ground value from each operand;
    // a two-expr Par (`1 | 2`) has none to extract.
    let two_exprs = Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        },
        Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        },
    ]);
    let cmp = binop(ExprInstance::ELtBody(models::rhoapi::ELt {
        p1: Some(two_exprs),
        p2: Some(new_gint_par(3, Vec::new(), false)),
    }));
    assert!(
        matches!(
            guard_disposition(&cmp, &datum),
            GuardDisposition::Failed(EvalError::NotASingleValue { .. })
        ),
        "got {:?}",
        guard_disposition(&cmp, &datum)
    );
}

#[test]
fn a_data_dependent_failure_is_not_refused_at_compile_time() {
    // ⚠ THE BOUNDARY OF THE GATE, stated as a test so it cannot be mistaken
    // for an oversight. `x / y` is undecidable only for `y = 0`, which is a
    // fact about the DATUM and not about the guard term — so refusing it at
    // compile time would refuse every division. The gate covers exactly the
    // class that is a function of the term alone.
    compile(&program_with_guard("1 / 0 == 1"))
        .expect("a division that WILL fail at run time still compiles — by design");
    compile(&program_with_guard("terms == [1] and 1 / 0 == 1"))
        .expect("…including under a connective");
    // The same shape with a variable divisor, which is the general case: the
    // term is identical whether or not the datum makes it fail.
    compile(&program_with_guard("1 / 1 == 1")).expect("and its succeeding twin");
    // ⇒ Neither the failing nor the succeeding spelling is refused, because
    // the gate does not — and cannot — evaluate. Contrast §1: the method
    // guard is refused for EVERY datum, since no datum could make it work.
}

// =========================================================================
// §6. The refusal is a pure function of the guard term
//
// ⚠ CONSENSUS PROPERTY. The refusal changes a deploy's outcome, so every node
// must reach it identically — and the message it carries must not read
// anything an operator can vary. `system_deploy_user_error.rs` records the
// precedent: an error message that reached for its environment produced
// different bytes on different nodes for the same failing deploy.
// =========================================================================

#[test]
fn the_refusal_message_depends_on_nothing_but_the_guard() {
    // Constructed twice, from two independently built guard Pars: byte-equal.
    // The message is assembled from the clause label, the `ExprInstance`
    // variant's fixed phrase and the method's own name — no paths, no
    // addresses, no clock, no iteration order.
    let first = run_reject(tautological_method_guard());
    let second = run_reject(tautological_method_guard());
    assert_eq!(first, second);

    // A DIFFERENT guard must produce a DIFFERENT message, or the message is
    // not carrying the information it claims to.
    let other = run_reject(eq(
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "length".to_string(),
                target: Some(new_boundvar_par(0, models::create_bit_vector(&[0]), false)),
                arguments: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]),
        new_gint_par(1, Vec::new(), false),
    ));
    assert_ne!(first, other);
    assert!(other.contains("`length`"), "got: {other}");
}

fn run_reject(guard: Par) -> String {
    rholang::rust::interpreter::guard::reject_undecidable_guard(
        &guard,
        rholang::rust::interpreter::guard::RECEIVE_WHERE,
    )
    .expect_err("the fixture guards are all undecidable")
    .to_string()
}

#[test]
fn an_absent_or_empty_guard_is_trivially_decidable() {
    // Both guard sites treat `Par::default()` as "no guard" and commit, so the
    // gate must accept it — refusing it would break every un-guarded receive.
    assert!(rholang::rust::interpreter::guard::reject_undecidable_guard(
        &Par::default(),
        rholang::rust::interpreter::guard::RECEIVE_WHERE
    )
    .is_ok());
    assert!(
        rholang::rust::interpreter::guard::reject_undecidable_guard_opt(
            None,
            rholang::rust::interpreter::guard::RECEIVE_WHERE
        )
        .is_ok()
    );
}

// =========================================================================
// §7. ★ The refusal does not depend on WHICH matcher decides guards
//
// `RSpace::create` takes the `Match` trait object that will decide every
// `where` guard at COMM time, so an embedder can substitute its own decider
// without changing f1r3node at all — and at least one does. A fix that lived
// only inside `rholang`'s `Matcher` would therefore leave every such embedder
// exactly as silent as before.
//
// The refusal is placed in `Reduce::eval_receive`, ahead of the consume, so it
// fires before ANY matcher is consulted. The test below measures that: it runs
// the undecidable guard on a space whose matcher COUNTS its `check_commit`
// calls, and the count must be zero.
// =========================================================================

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rholang::rust::interpreter::accounting::cost_accounting::CostAccounting;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::reduce::DebruijnInterpreter;
use rholang::rust::interpreter::rho_runtime::RhoISpace;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

/// f1r3node's spatial matcher with a counter bolted onto the guard hook —
/// the shape of every embedder-supplied decider.
struct CountingMatcher {
    spatial: Matcher,
    guard_calls: Arc<AtomicUsize>,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for CountingMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.spatial.get(pattern, data)
    }

    fn check_commit(&self, k: &TaggedContinuation, matched: &[&ListParWithRandom]) -> bool {
        self.guard_calls.fetch_add(1, Ordering::SeqCst);
        self.spatial.check_commit(k, matched)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_refusal_fires_before_any_matcher_is_consulted() {
    let guard_calls = Arc::new(AtomicUsize::new(0));
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-mem store");
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> = RSpace::create(
        store,
        Arc::new(Box::new(CountingMatcher {
            spatial: Matcher,
            guard_calls: guard_calls.clone(),
        })),
    )
    .expect("rspace");
    let cost = CostAccounting::empty_cost();
    cost.set(Cost::create(i64::MAX, "guard gate test".to_string()));
    let rspace: RhoISpace = Arc::new(Box::new(space.clone()));
    let reducer = DebruijnInterpreter::new(
        rspace,
        Arc::new(HashMap::new()),
        Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        Arc::new(HashMap::new()),
        cost,
    );

    let chan = new_gstring_par("c".to_string(), Vec::new(), false);
    let env: Env<Par> = Env::new();
    reducer
        .eval(
            Par::default().with_sends(vec![Send {
                chan: Some(chan.clone()),
                data: vec![one_element_list(1)],
                persistent: false,
                locally_free: Vec::new(),
                connective_used: false,
            }]),
            &env,
            rand().split_byte(0),
        )
        .await
        .expect("the send is the control half");

    // CONTROL: a DECIDABLE guard reaches the matcher, so the counter moves.
    // Without this the zero below would be unfalsifiable — a counter that
    // never increments proves nothing about where the refusal happened.
    reducer
        .eval(
            receive_on(chan.clone(), decidably_true_guard()),
            &env,
            rand().split_byte(1),
        )
        .await
        .expect("a decidable guard must not raise");
    let after_control = guard_calls.load(Ordering::SeqCst);
    assert!(
        after_control > 0,
        "the instrument is dead: a decidable guard must reach check_commit"
    );

    // ★ THE MEASUREMENT: an undecidable guard raises without the matcher ever
    // being asked. ⇒ the refusal holds for EVERY `Match` implementation, not
    // just f1r3node's own.
    reducer
        .eval(
            receive_on(chan, tautological_method_guard()),
            &env,
            rand().split_byte(2),
        )
        .await
        .expect_err("an undecidable guard must raise");
    assert_eq!(
        guard_calls.load(Ordering::SeqCst),
        after_control,
        "check_commit must NOT have been consulted for the undecidable guard"
    );
}

fn receive_on(chan: Par, guard: Par) -> Par {
    Par::default().with_receives(vec![Receive {
        binds: vec![ReceiveBind {
            patterns: vec![new_freevar_par(0, Vec::new())],
            source: Some(chan),
            remainder: None,
            free_count: 1,
        }],
        body: Some(Par::default()),
        persistent: false,
        peek: false,
        bind_count: 1,
        locally_free: Vec::new(),
        connective_used: false,
        condition: Some(guard),
    }])
}
