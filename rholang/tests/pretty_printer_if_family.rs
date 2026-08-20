//! Regression coverage for the `If` member of the canonical `ParBag` family.
//!
//! `Par` has nine component families. The printer historically enumerated only
//! eight, so an `If`-only value rendered as `Nil` and a conditional in a mixed
//! value disappeared. These public-entry-point tests pin both the standalone
//! bytes and the family order used by the REPL, CLI, storage diagnostics, and
//! consensus error rendering.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, If, Par};
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;

fn expr(instance: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn conditional() -> If {
    If {
        condition: Some(expr(ExprInstance::GBool(true))),
        if_true: Some(expr(ExprInstance::GInt(1))),
        if_false: Some(expr(ExprInstance::GInt(2))),
        locally_free: vec![],
        connective_used: false,
    }
}

#[test]
fn an_if_only_par_renders_all_three_children_instead_of_nil() {
    let term = models::par_from_default! {
        conditionals: vec![conditional()],
        ..Default::default()
    };

    assert_eq!(
        PrettyPrinter::new().build_string_from_message(&term),
        "if (true) {\n  1\n} else {\n  2\n}"
    );
}

#[test]
fn a_mixed_par_preserves_the_canonical_expr_then_if_family_order() {
    let term = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }],
        conditionals: vec![conditional()],
        ..Default::default()
    };

    assert_eq!(
        PrettyPrinter::new().build_string_from_message(&term),
        "7 |\nif (true) {\n  1\n} else {\n  2\n}"
    );
}
