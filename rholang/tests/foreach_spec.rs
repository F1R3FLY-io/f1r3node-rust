use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;

#[test]
fn a_foreach_comprehension_is_refused_as_unsupported() {
    let result = Compiler::source_to_adt("foreach ( x <- [1, 2, 3] ) yield ( rslt ) { Nil }");
    assert!(
        matches!(&result, Err(InterpreterError::NormalizerError(message)) if message.contains("foreach")),
        "{result:?}"
    );
}
