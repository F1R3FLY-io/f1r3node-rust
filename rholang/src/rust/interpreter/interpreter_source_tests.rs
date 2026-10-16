use proptest::prelude::*;

use super::*;

proptest! {
    /// Changed by DR-131: the structural limits went with host work. The test
    /// keeps its check of the normal form.
    #[test]
    fn shared_source_parser_preserves_normal_form(value in any::<i32>()) {
        let source = format!("new c in {{ c!({value}) | for (@x <- c) {{ @0!(x) }} }}");
        let expected = Compiler::source_to_adt(&source).unwrap();
        prop_assert_eq!(InterpreterImpl::parse_source(&source, HashMap::new()).unwrap(), expected);
    }
}

/// Changed by DR-131: the parser no longer checks a host-work payment before
/// the syntax. The test keeps its check of the syntax error.
#[test]
fn shared_source_parser_reports_a_syntax_error() {
    assert!(matches!(
        InterpreterImpl::parse_source("new {", HashMap::new()),
        Err(InterpreterError::ParserError(_)),
    ));
}

#[test]
fn shared_source_parser_retains_the_envelope_normalizer_environment() {
    let env = HashMap::from([(
        "rho:test:value".to_owned(),
        models::rust::utils::new_gint_par(7, Vec::new(), false),
    )]);
    let source = "new value(`rho:test:value`) in { value!(1) }";
    let expected = Compiler::source_to_adt_with_normalizer_env(source, env.clone()).unwrap();
    assert_eq!(
        InterpreterImpl::parse_source(source, env).unwrap(),
        expected
    );
}
