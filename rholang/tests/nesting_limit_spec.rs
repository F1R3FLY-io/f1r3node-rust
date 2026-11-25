use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use models::rust::utils::{new_elist_par, new_gint_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::nesting::{check_nesting, MAX_NESTING_DEPTH};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

/// The stack of a node's runtime worker threads.
const WORKER_STACK: usize = 2 * 1024 * 1024;

fn nested_list_send(lists: usize) -> String {
    format!("@0!({}0{})", "[".repeat(lists), "]".repeat(lists))
}

fn joined(n: usize, element: impl Fn(usize) -> String, separator: &str) -> String {
    (0..n).map(element).collect::<Vec<_>>().join(separator)
}

fn nested(n: usize, open: &str, close: &str) -> String {
    format!("{}Nil{}", open.repeat(n), close.repeat(n))
}

fn sequential_let(bindings: usize) -> String {
    format!(
        "let {} in {{ Nil }}",
        joined(bindings, |i| format!("x{i} <- {i}"), "; ")
    )
}

fn sequential_receipts(receipts: usize) -> String {
    format!(
        "for ({}) {{ Nil }}",
        joined(receipts, |i| format!("@x{i} <- @{i}"), "; ")
    )
}

fn sync_send_chain(sends: usize) -> String {
    format!("new x in {{ {}Nil }}", "x!?(1); ".repeat(sends))
}

fn nested_concurrent_let(lets: usize) -> String { nested(lets, "let y <- 1 & z <- 2 in { ", " }") }

fn nested_sync_receive(receives: usize) -> String {
    nested(receives, "for (@y <- @0!?(1)) { ", " }")
}

fn eval_quote_chain(evals: usize) -> String {
    format!("@0!({}0{})", "*@{".repeat(evals), "}".repeat(evals))
}

fn compile_on_worker_stack(source: String) -> Result<Par, InterpreterError> {
    std::thread::Builder::new()
        .stack_size(WORKER_STACK)
        .spawn(move || Compiler::source_to_adt(&source))
        .unwrap()
        .join()
        .unwrap()
}

fn refused(result: &Result<Par, InterpreterError>) -> bool {
    matches!(result, Err(InterpreterError::NestingLimitExceeded { .. }))
}

fn nested_value(depth: usize) -> Par {
    let mut par = new_gint_par(0, Vec::new(), false);
    for _ in 1..depth {
        par = new_elist_par(vec![par], Vec::new(), false, None, Vec::new(), false);
    }
    par
}

fn is_nesting_error(error: &InterpreterError) -> bool {
    matches!(error, InterpreterError::NestingLimitExceeded { .. })
}

#[test]
fn a_term_compiles_up_to_the_nesting_limit_and_not_beyond_it() {
    let at_limit = Compiler::source_to_adt(&nested_list_send(MAX_NESTING_DEPTH - 2)).unwrap();
    assert!(check_nesting(&at_limit).is_ok());
    let one_deeper = new_elist_par(vec![at_limit], Vec::new(), false, None, Vec::new(), false);
    assert!(check_nesting(&one_deeper).is_err());

    assert!(refused(&Compiler::source_to_adt(&nested_list_send(
        MAX_NESTING_DEPTH - 1
    ))));
}

#[test]
fn parallel_composition_adds_no_level_and_a_quoted_name_does() {
    let at_limit = nested_list_send(MAX_NESTING_DEPTH - 2);
    assert!(Compiler::source_to_adt(&format!("{at_limit} | @1!(0) | @2!(0)")).is_ok());
    assert!(refused(&Compiler::source_to_adt(&format!(
        "@{{{at_limit}}}!(0)"
    ))));
    assert!(Compiler::source_to_adt(&format!(
        "@{{{}}}!(0)",
        nested_list_send(MAX_NESTING_DEPTH - 3)
    ))
    .is_ok());
}

#[test]
fn a_desugared_term_is_measured_by_the_term_it_compiles_to() {
    let largest_compiling: [(&str, fn(usize) -> String, usize); 5] = [
        ("sequential let", sequential_let, 126),
        ("sequential receipts", sequential_receipts, 127),
        ("synchronous send chain", sync_send_chain, 63),
        ("nested concurrent let", nested_concurrent_let, 42),
        ("nested synchronous receive", nested_sync_receive, 63),
    ];
    for (shape, build, largest) in largest_compiling {
        assert!(
            compile_on_worker_stack(build(largest)).is_ok(),
            "{shape} of {largest}"
        );
        assert!(
            refused(&compile_on_worker_stack(build(largest + 1))),
            "{shape} of {}",
            largest + 1
        );
    }
}

#[test]
fn a_term_far_beyond_the_limit_is_refused_on_a_worker_stack() {
    let sources = [
        nested_list_send(100_000),
        sequential_let(50_000),
        sequential_receipts(50_000),
        sync_send_chain(50_000),
        nested_concurrent_let(10_000),
        nested_sync_receive(10_000),
        eval_quote_chain(10_000),
    ];
    for source in sources {
        let start: String = source.chars().take(40).collect();
        assert!(refused(&compile_on_worker_stack(source)), "{start}");
    }
}

#[test]
fn constructs_that_cancel_out_add_no_level() {
    assert!(compile_on_worker_stack(eval_quote_chain(2 * MAX_NESTING_DEPTH)).is_ok());
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
