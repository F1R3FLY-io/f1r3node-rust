use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;

fn refused(source: &str) {
    match Compiler::source_to_adt(source) {
        Err(InterpreterError::NormalizerError(message)) => {
            assert!(message.contains("cost-accounting syntax"), "{source}: {message}")
        }
        other => panic!("{source}: expected a normalizer error, got {other:?}"),
    }
}

#[test]
fn signed_terms_are_refused_until_the_normalizer_supports_them() {
    refused("new s in { {% Nil %}[s] }");
    refused("new s, x in { {% for (y <- x) { Nil } %}[s] }");
}

#[test]
fn token_stacks_are_refused_until_the_normalizer_supports_them() {
    refused("new s in { s :: () }");
    refused("new s, t in { s :: t :: () }");
}

#[test]
fn signed_binds_are_refused_until_the_normalizer_supports_them() {
    refused("new s, x in { for ({% y <- x %}[s]) { Nil } }");
}

#[test]
fn ordinary_terms_still_compile() {
    assert!(Compiler::source_to_adt("new x in { for (y <- x) { Nil } | x!(1) }").is_ok());
}
