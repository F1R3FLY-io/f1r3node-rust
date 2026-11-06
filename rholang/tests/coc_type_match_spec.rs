// CoC Phase 3.2: end-to-end integration for type-directed pattern
// matching. Source text runs through `Compiler::source_to_adt` to the
// Par shape consumed by the spatial matcher. The matcher's Pi-pattern
// integration (Phase 3.1) decides match results.
//
// These tests pin the full pipeline: parser → normalizer → matcher.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Match, Par};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

fn compile(src: &str) -> Par {
    Compiler::source_to_adt(src).unwrap_or_else(|e| panic!("compile failed for {src:?}: {e:?}"))
}

fn expect_single_match(par: &Par) -> &Match {
    assert_eq!(par.matches.len(), 1, "expected a single Match in Par");
    &par.matches[0]
}

// The Pi pattern `Int -> Int` in a match-case normalizes to an EPi
// Par with `connective_used = true` (Int is a connective pattern).
#[test]
fn match_with_pi_pattern_normalizes_to_epi_par() {
    let par = compile("match 0 { Int -> Int => Nil }");
    let m = expect_single_match(&par);
    assert_eq!(m.cases.len(), 1);
    let pattern = m.cases[0]
        .pattern
        .as_ref()
        .expect("case pattern is populated");
    assert!(pattern.connective_used, "Pi pattern uses connectives");
    match pattern.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
        Some(ExprInstance::EPiBody(pi)) => {
            assert_eq!(pi.bind_count, 1, "non-dependent sugar binds one slot");
            assert_eq!(pi.param_types.len(), 1);
        }
        other => panic!("expected EPi pattern, got {other:?}"),
    }
}

// A fully-annotated lambda value matches a `Int -> Int` pattern via
// the matcher's Phase 3.1 type-directed path.
#[test]
fn lambda_value_matches_matching_pi_source_pattern() {
    let lambda_par = compile("(x: Int) => x");
    let match_par = compile("match 0 { Int -> Int => Nil }");
    let pattern = match_par.matches[0]
        .cases[0]
        .pattern
        .clone()
        .expect("pattern");
    let result = SpatialMatcherContext::new().match_result(&lambda_par, pattern);
    assert!(
        result.is_some(),
        "lambda (x: Int) => x should match pattern Int -> Int"
    );
}

// Mismatch: `(x: Int) => x` is not an `Int -> Bool`; the Phase 3.1
// matcher path rejects via typechecker.
#[test]
fn lambda_value_rejects_non_matching_pi_source_pattern() {
    let lambda_par = compile("(x: Int) => x");
    let match_par = compile("match 0 { Int -> Bool => Nil }");
    let pattern = match_par.matches[0]
        .cases[0]
        .pattern
        .clone()
        .expect("pattern");
    let result = SpatialMatcherContext::new().match_result(&lambda_par, pattern);
    assert!(
        result.is_none(),
        "lambda typed Int -> Int must not match pattern Int -> Bool"
    );
}

// A non-function value (plain Int) is not matched by any Pi pattern.
#[test]
fn ground_value_rejects_pi_source_pattern() {
    let int_par = compile("42");
    let match_par = compile("match 0 { Int -> Int => Nil }");
    let pattern = match_par.matches[0]
        .cases[0]
        .pattern
        .clone()
        .expect("pattern");
    let result = SpatialMatcherContext::new().match_result(&int_par, pattern);
    assert!(
        result.is_none(),
        "42 has type Int, not Int -> Int; Pi pattern must reject it"
    );
}

// Dependent Pi in the pattern: `(x: Int) -> Int`. A matching lambda
// still passes via typecheck.
#[test]
fn lambda_matches_dependent_pi_source_pattern() {
    let lambda_par = compile("(x: Int) => x");
    let match_par = compile("match 0 { (x: Int) -> Int => Nil }");
    let pattern = match_par.matches[0]
        .cases[0]
        .pattern
        .clone()
        .expect("pattern");
    let result = SpatialMatcherContext::new().match_result(&lambda_par, pattern);
    assert!(result.is_some(), "dependent Pi pattern should accept the lambda");
}
