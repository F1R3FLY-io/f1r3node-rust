use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

fn compile(source: &str) -> Par {
    Compiler::source_to_adt(source).unwrap_or_else(|e| panic!("{source}: {e:?}"))
}

#[test]
fn signed_terms_token_stacks_and_signed_binds_normalize() {
    let signed = compile("new s in { {% Nil %}[s] }");
    assert_eq!(
        signed.news[0].p.as_ref().unwrap().cost_signed_terms.len(),
        1
    );

    let stack = compile("new s, t in { s :: t :: () }");
    let cells = &stack.news[0].p.as_ref().unwrap().cost_stacks[0].cells;
    assert_eq!(cells.len(), 2);

    let bind = compile("new s, x in { for ({% y <- x %}[s]) { Nil } }");
    let receive = &bind.news[0].p.as_ref().unwrap().receives[0];
    assert!(receive.binds[0].cost_signature.is_some());
}

#[test]
fn ordinary_terms_are_unchanged_by_the_new_fields() {
    let par = compile("new x in { for (y <- x) { Nil } | x!(1) }");
    assert!(par.cost_signed_terms.is_empty() && par.cost_stacks.is_empty());
    let body = par.news[0].p.as_ref().unwrap();
    assert!(body.receives[0]
        .binds
        .iter()
        .all(|b| b.cost_signature.is_none()));
}

#[test]
fn printed_cost_syntax_normalizes_to_the_same_term() {
    for (source, expected) in [
        (r#"{% 1 %}[s]"#, r#"{% 1 %}[s]"#),
        (r#"{% 1 %}[b (*) a]"#, r#"{% 1 %}[a (*) b]"#),
        (r#"{% Nil %}[# { 7 }]"#, r#"{% Nil %}[# { 7 }]"#),
        (r#"s :: t :: ()"#, r#"s :: t :: ()"#),
        (r#"{% {% 1 %}[b] %}[a]"#, r#"{% 1 %}[a (*) b]"#),
    ] {
        let par = compile(source);
        let printed = PrettyPrinter::new().build_string_from_message(&par);
        assert_eq!(printed, expected, "{source}");
        assert_eq!(compile(&printed), par, "{source} printed as {printed}");
    }

    let bind = compile(r#"for ({% @y <- @"c" %}[s]) { Nil }"#);
    let printed = PrettyPrinter::new().build_string_from_message(&bind);
    assert!(printed.starts_with("for( {% "), "{printed}");
    assert!(printed.contains(" %}[s] )"), "{printed}");
    assert_eq!(compile(&printed), bind, "printed as {printed}");
}

async fn evaluation_errors(source: &'static str) -> Vec<InterpreterError> {
    with_runtime("cost-syntax-", |runtime| async move {
        runtime
            .evaluate(
                source,
                Cost::create(i64::MAX, "cost syntax spec".to_string()),
                HashMap::new(),
                Blake2b512Random::create_from_bytes(&[]),
            )
            .await
            .expect("evaluation runs")
            .errors
    })
    .await
}

#[tokio::test]
async fn a_sent_signed_term_is_inert_data() {
    let errors = evaluation_errors(r#"new s in { @"out"!({% Nil %}[s]) }"#).await;
    assert!(errors.is_empty(), "{errors:?}");
}
