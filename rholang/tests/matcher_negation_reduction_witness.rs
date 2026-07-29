//! ★ THE CONSENSUS-VISIBLE WITNESS: an ordinary Rholang program that reduces
//! down the **wrong branch**, and an ordinary Rholang expression that evaluates
//! to **`false`** where `true` is correct.
//!
//! `matcher_negation_isolation.rs` pins the mechanism at the matcher's own
//! seams. This file carries it out through the real reducer and the real
//! tuplespace, because a defect that changes which `match` case fires changes
//! the data a continuation receives, which changes the post-state hash, which
//! is consensus.
//!
//! # The chain, end to end
//!
//! ```text
//!   ~{x /\ 8}                  the body binds level 0 and THEN demands 8,
//!                              so against 7 it writes before it refuses
//!                                        │
//!   ConnNotBody inverts        the inner attempt refused ⇒ the negation
//!   (spatial_matcher.rs)       SUCCEEDS — carrying a binding out with it
//!                                        │
//!   ~ and \/ are numbered      each body has its OWN fresh FreeMap, so BOTH
//!   from zero, per body        siblings' leaks are level 0
//!   (p_negation_normalizer)              │
//!   δ₁ = {0 ↦ 7}, δ₂ = {0 ↦ 6}           │
//!                                        ▼
//!   aggregate_updates          two claimed matches added the SAME level ⇒
//!   (list_match.rs)            refuse the whole list match
//!                                        │
//!                                        ▼
//!   the reducer                the first `match` case does not fire and the
//!                              fall-through case does
//! ```
//!
//! ★ The `aggregate_updates` backstop is *correct* — a linearly-normalised
//! pattern cannot make two claimed matches bind the same level. Its premise
//! simply is not true yet: linearity holds **within one free map**, and `~`/`\/`
//! bodies each get a fresh one. Once the negation stops leaking, the premise
//! holds and the backstop goes back to being unreachable.
//!
//! # Where a binding-bodied negation can reach the matcher
//!
//! Only through a `match` case pattern and the right-hand side of `matches`.
//! `fail_on_invalid_connective` rejects `~`/`\/` in `for`/`contract` patterns
//! (at `bound_map_chain.depth() == 0`), and every nested pattern position is
//! compared by *structural equality* rather than spatially matched. That is why
//! the programs below are `match`/`matches` and not `for`.
//!
//! ★ No test here expects a panic; every assertion is on a value read back out
//! of the tuplespace (`shared/tests/panic_expectation_gate.rs`).

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

fn gstring_channel(name: &str) -> Par {
    Par::default().with_exprs(vec![models::rhoapi::Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }])
}

/// Evaluate `program` on a fresh runtime and return the single expression sent
/// to `@"out"`.
async fn out_expr(prefix: &str, program: &str) -> ExprInstance {
    let program = program.to_string();
    with_runtime(prefix, move |mut runtime: RhoRuntimeImpl| async move {
        let result = runtime
            .evaluate_with_term(&program)
            .await
            .expect("evaluation must not fail structurally");
        assert!(
            result.errors.is_empty(),
            "evaluation raised interpreter errors: {:?}",
            result.errors
        );

        let data = runtime.get_data(&gstring_channel("out")).await;
        assert_eq!(data.len(), 1, "expected exactly one datum at @\"out\"");
        let pars = &data[0].a.pars;
        assert_eq!(pars.len(), 1, "expected a single Par at @\"out\"");
        pars[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.clone())
            .expect("@\"out\" carries an expression")
    })
    .await
}

async fn out_string(prefix: &str, program: &str) -> String {
    match out_expr(prefix, program).await {
        ExprInstance::GString(s) => s,
        other => panic!("@\"out\" expected a string, got {other:?}"),
    }
}

async fn out_bool(prefix: &str, program: &str) -> bool {
    match out_expr(prefix, program).await {
        ExprInstance::GBool(b) => b,
        other => panic!("@\"out\" expected a boolean, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The REDs — a real program computing a real wrong answer
// ─────────────────────────────────────────────────────────────────────────

/// ★★ THE WITNESS. Two sibling sends, each carrying a negation whose body
/// binds. Both negations succeed — `~{x /\ 8}` against `7` demands `8`, which
/// `7` is not — so the first case is the one that should fire.
///
/// At HEAD the fall-through case fires instead, and
/// `RHOLANG-MATCHER-AGGREGATE-UPDATES-REFUSAL: level 0 …` is printed during the
/// reduction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_match_over_two_binding_negations_takes_the_first_case() {
    let program = r#"
        match { @0!(7) | @1!(6) } {
            @0!(~{x /\ 8}) | @1!(~{y /\ 9}) => @"out"!("case")
            _                               => @"out"!("fall-through")
        }
    "#;
    assert_eq!(
        out_string("neg-witness-match-", program).await,
        "case",
        "★ both negations succeed, so the first case must fire"
    );
}

/// The same defect through `matches`, which answers with a boolean rather than
/// by branching — the most compact form of the wrong answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matches_answers_true_for_two_binding_negations() {
    let program = r#"
        @"out"!( { @0!(7) | @1!(6) } matches { @0!(~{x /\ 8}) | @1!(~{y /\ 9}) } )
    "#;
    assert!(
        out_bool("neg-witness-matches-", program).await,
        "★ `matches` must answer true — at HEAD it answers false"
    );
}

/// The `Set` route, which reaches the production `ESetBody` `ListMatch<Par>`
/// rather than the `ListMatch<Send>` the two tests above drive. Same defect,
/// different list.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_set_pattern_of_two_binding_negations_takes_the_first_case() {
    let program = r#"
        match Set(7, 6) {
            Set(~{x /\ 8}, ~{y /\ 9}) => @"out"!("case")
            _                         => @"out"!("fall-through")
        }
    "#;
    assert_eq!(
        out_string("neg-witness-set-", program).await,
        "case",
        "★ a Set of two succeeding negations must match"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Controls — must NOT discriminate (green at HEAD and after)
// ─────────────────────────────────────────────────────────────────────────

/// Control: **one** sibling. A single delta cannot duplicate itself, so the
/// aggregation has nothing to refuse and the verdict is right on both sides.
/// This is what makes the REDs evidence of the *duplication*, not of "a
/// negation stopped matching".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_one_binding_negation_takes_the_first_case() {
    let program = r#"
        match { @0!(7) } {
            @0!(~{x /\ 8}) => @"out"!("case")
            _              => @"out"!("fall-through")
        }
    "#;
    assert_eq!(
        out_string("neg-control-one-", program).await,
        "case",
        "one sibling: right on both sides of the fix"
    );
}

/// Control: two siblings whose negation bodies **do not bind**. No leak, so no
/// duplicated level, so no refusal — on either side.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_two_non_binding_negations_take_the_first_case() {
    let program = r#"
        match { @0!(7) | @1!(6) } {
            @0!(~8) | @1!(~9) => @"out"!("case")
            _                 => @"out"!("fall-through")
        }
    "#;
    assert_eq!(
        out_string("neg-control-nonbinding-", program).await,
        "case",
        "non-binding negations never leaked, so this was always right"
    );
}

/// Control: a pattern that genuinely does **not** match still falls through.
/// The fix un-refuses matches that should have fired; it must not start
/// accepting matches that should not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_a_genuinely_failing_negation_still_falls_through() {
    let program = r#"
        match { @0!(7) | @1!(6) } {
            @0!(~{x /\ 7}) | @1!(~{y /\ 9}) => @"out"!("case")
            _                               => @"out"!("fall-through")
        }
    "#;
    assert_eq!(
        out_string("neg-control-fail-", program).await,
        "fall-through",
        "★ `~{{x /\\ 7}}` against 7: the inner conjunction MATCHES, so the \
         negation refuses and the case must not fire"
    );
}
