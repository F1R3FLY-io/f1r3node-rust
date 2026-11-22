use models::rhoapi::connective::ConnectiveInstance::{ConnBool, ConnInt};
use models::rhoapi::expr::ExprInstance::GBool;
use models::rhoapi::{Connective, Expr, Par};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_freevar_expr, new_gint_expr, new_wildcard_expr};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcherContext, StepBudget};

/// `0 | 1 | … | n-1 | true`
fn ints_then_true(n: usize) -> Par {
    let mut exprs: Vec<Expr> = (0..n as i64).map(new_gint_expr).collect();
    exprs.push(Expr {
        expr_instance: Some(GBool(true)),
    });
    Par::default().with_exprs(exprs)
}

/// `{ Int | Bool | <remainder> }`
fn int_bool_and(remainder: Expr) -> Par {
    vector_par(Vec::new(), true)
        .with_exprs(vec![remainder])
        .with_connectives(vec![
            Connective {
                connective_instance: Some(ConnInt(true)),
            },
            Connective {
                connective_instance: Some(ConnBool(true)),
            },
        ])
}

fn matches(n: usize, pattern: Par) -> bool {
    SpatialMatcherContext::with_budget(StepBudget::new(usize::MAX))
        .into_match_result(ints_then_true(n), pattern)
        .is_some()
}

#[test]
fn a_remainder_takes_any_number_of_elements_of_a_kind() {
    for n in [1_001, 1_002, 5_000] {
        assert!(matches(n, int_bool_and(new_freevar_expr(0))), "rest with {n} ints");
        assert!(matches(n, int_bool_and(new_wildcard_expr())), "wildcard with {n} ints");
    }
}
