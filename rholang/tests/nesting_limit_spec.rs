use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::{new_elist_par, new_gint_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::compiler::process_depth::process_depth;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::nesting::{check_nesting, MAX_NESTING_DEPTH};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rholang_parser::RholangParser;

fn nested_list_send(lists: usize) -> String {
    format!("@0!({}0{})", "[".repeat(lists), "]".repeat(lists))
}

fn depth_of(source: &str) -> usize {
    match RholangParser::new().parse(source) {
        validated::Validated::Good(procs) => procs.iter().map(process_depth).max().unwrap_or(0),
        validated::Validated::Fail(_) => panic!("{source} does not parse"),
    }
}

fn nested_value(depth: usize) -> Par {
    let mut par = new_gint_par(0, Vec::new(), false);
    for _ in 1..depth {
        par = new_elist_par(vec![par], Vec::new(), false, None, Vec::new(), false);
    }
    par
}

fn is_nesting_error(error: &InterpreterError) -> bool {
    error
        .to_string()
        .contains("Nesting is deeper than the limit")
}

#[test]
fn process_depth_counts_nesting_inside_quoted_names_and_not_parallel_composition() {
    assert_eq!(
        depth_of("@0!(0)"),
        depth_of("@0!(0) | @1!(1) | @2!(2) | @3!(3)")
    );
    assert_eq!(depth_of("@*@*@*@0!(0)"), depth_of("@0!(0)") + 3);
    assert_eq!(
        depth_of("for (x <- @*@*@0) { Nil }"),
        depth_of("for (x <- @0) { Nil }") + 2
    );
    assert_eq!(
        depth_of(&nested_list_send(10)),
        depth_of(&nested_list_send(0)) + 10
    );
}

#[test]
fn a_term_compiles_up_to_the_nesting_limit_and_not_beyond_it() {
    let lists_at_limit = MAX_NESTING_DEPTH - depth_of(&nested_list_send(0));
    assert_eq!(
        depth_of(&nested_list_send(lists_at_limit)),
        MAX_NESTING_DEPTH
    );

    assert!(Compiler::source_to_adt(&nested_list_send(lists_at_limit)).is_ok());
    assert!(matches!(
        Compiler::source_to_adt(&nested_list_send(lists_at_limit + 1)),
        Err(InterpreterError::NestingLimitExceeded { .. })
    ));
}

#[test]
fn a_term_far_beyond_the_limit_is_rejected_without_recursing() {
    assert!(matches!(
        Compiler::source_to_adt(&nested_list_send(100_000)),
        Err(InterpreterError::NestingLimitExceeded { .. })
    ));
}

#[test]
fn a_value_passes_up_to_the_nesting_limit_and_not_beyond_it() {
    assert!(check_nesting(&nested_value(MAX_NESTING_DEPTH)).is_ok());
    assert!(matches!(
        check_nesting(&nested_value(MAX_NESTING_DEPTH + 1)),
        Err(InterpreterError::NestingLimitExceeded { .. })
    ));
}

async fn errors_of(term: &str) -> Vec<InterpreterError> {
    let term = term.to_string();
    with_runtime("nesting-limit-", |runtime| async move {
        runtime
            .evaluate(
                &term,
                Cost::create(100_000_000, "nesting limit"),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(&[]),
            )
            .await
            .expect("evaluation returns a result")
            .errors
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unbounded_lambda_application_fails_the_deploy_instead_of_the_node() {
    let errors = errors_of("@0!(((x) => x(x))(((x) => x(x))))").await;
    assert!(errors.iter().any(is_nesting_error), "{errors:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_value_built_deeper_than_the_limit_at_runtime_fails_the_deploy() {
    let errors = errors_of(
        "new loop in { contract loop(@x, @n) = { if (n > 0) { loop!([x], n - 1) } else { @0!(x) } } | loop!(0, 1000) }",
    )
    .await;
    assert!(errors.iter().any(is_nesting_error), "{errors:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_value_at_the_limit_goes_through_every_operation_on_worker_stacks() {
    let errors = errors_of(&format!(
        "new loop, out, stash in {{
           contract loop(@x, @n) = {{
             if (n > 0) {{ loop!([x], n - 1) }} else {{
               stash!(x) |
               for (@y <- stash) {{
                 out!([y == x, y.toByteArray().length(), Set(y, [y]).size(), {{y : y}}.get(y) == y]) |
                 match y {{ [[[z]]] => out!(z) }} |
                 for (@[[w]] <- stash) {{ out!(w) }} |
                 stash!(y)
               }}
             }}
           }} |
           loop!(0, {})
         }}",
        MAX_NESTING_DEPTH - 8
    ))
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}
