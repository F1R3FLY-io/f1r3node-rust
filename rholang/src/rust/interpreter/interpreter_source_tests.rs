use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;

use super::*;

proptest! {
    /// Changed by DR-131: the structural limits went with host work. The test
    /// keeps its check of the normal form.
    #[test]
    fn accounting_scope_tracks_event_codec_epoch_and_restores_nesting(
        epoch_index in 0_u8..3,
        existing_scopes in 0_usize..4,
    ) {
        let epoch = match epoch_index {
            0 => EventCodecEpoch::Legacy,
            1 => EventCodecEpoch::CostV1,
            _ => EventCodecEpoch::CostV2,
        };
        let budget = RuntimeBudget::new(Cost::create(100, "epoch scope property"));
        let baseline = (0..existing_scopes)
            .map(|_| budget.enter_comm_accounting_scope())
            .collect::<Vec<_>>();
        let scope = budget.enter_comm_accounting_scope_for_epoch(epoch);
        prop_assert_eq!(scope.is_some(), epoch != EventCodecEpoch::Legacy);
        prop_assert_eq!(
            budget.has_comm_accounting_scope(),
            existing_scopes != 0 || epoch != EventCodecEpoch::Legacy,
        );
        drop(scope);
        prop_assert_eq!(budget.has_comm_accounting_scope(), existing_scopes != 0);
        drop(baseline);
        prop_assert!(!budget.has_comm_accounting_scope());
    }

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

#[test]
fn prepared_native_input_preserves_source_normal_form() {
    let source = "new c in { c!(1) | for (@x <- c) { @0!(x) } }";
    let normalized = Compiler::source_to_adt(source).unwrap();
    let parsed = InterpreterImpl::parse_source(source, HashMap::new()).unwrap();
    let prepared = PreparedProgram::from_normalized(normalized).into_par();
    assert_eq!(parsed, prepared);
}
