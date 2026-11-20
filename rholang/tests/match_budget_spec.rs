use models::rhoapi::connective::ConnectiveInstance::{ConnBool, ConnInt};
use models::rhoapi::expr::ExprInstance::GBool;
use models::rhoapi::{Connective, Expr, Par};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_freevar_expr, new_gint_expr};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcherContext, StepBudget};

/// `0 | 1 | … | n-1 | true`
fn ints_then_true(n: usize) -> Par {
    let mut exprs: Vec<Expr> = (0..n as i64).map(new_gint_expr).collect();
    exprs.push(Expr {
        expr_instance: Some(GBool(true)),
    });
    Par::default().with_exprs(exprs)
}

/// `{ Int | Bool | rest }`
fn int_bool_rest() -> Par {
    vector_par(Vec::new(), true)
        .with_exprs(vec![new_freevar_expr(0)])
        .with_connectives(vec![
            Connective {
                connective_instance: Some(ConnInt(true)),
            },
            Connective {
                connective_instance: Some(ConnBool(true)),
            },
        ])
}

fn matches_within(steps: usize, n: usize) -> bool {
    SpatialMatcherContext::with_budget(StepBudget::new(steps))
        .into_match_result(ints_then_true(n), int_bool_rest())
        .is_some()
}

/// The fewest steps with which the match of `n` succeeds.
fn needed(n: usize) -> usize {
    let (mut fails, mut succeeds) = (0, 1 << 30);
    assert!(matches_within(succeeds, n));
    while succeeds - fails > 1 {
        let mid = (fails + succeeds) / 2;
        if matches_within(mid, n) {
            succeeds = mid;
        } else {
            fails = mid;
        }
    }
    succeeds
}

#[test]
fn a_match_succeeds_on_the_steps_it_needs_and_fails_on_one_fewer() {
    let steps = needed(20);
    assert!(matches_within(steps, 20));
    assert!(!matches_within(steps - 1, 20));
    // The step count decides which matches pass, so it is consensus behaviour:
    // a change to it must show up here.
    assert_eq!(steps, 348);
}

#[test]
fn the_steps_a_match_needs_grow_linearly_with_the_target() {
    assert!(needed(1000) <= 20 * 1000, "{}", needed(1000));
}

#[test]
fn a_match_that_runs_out_of_steps_fails_the_same_way_every_time() {
    for _ in 0..3 {
        assert!(!matches_within(4_096, 1_000));
    }
    assert!(matches_within(usize::MAX, 1_000));
}
