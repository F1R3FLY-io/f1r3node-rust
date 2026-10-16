use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;

use super::*;

proptest! {
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
    fn shared_source_parser_preserves_normal_form_and_exact_structural_limits(value in any::<i32>()) {
        let source = format!("new c in {{ c!({value}) | for (@x <- c) {{ @0!(x) }} }}");
        let expected = Compiler::source_to_adt(&source).unwrap();
        let bytes = u64::try_from(source.len() + expected.encoded_len()).unwrap();
        let items = InterpreterImpl::structural_items(&expected).unwrap();
        for (dimension, deficit) in [
            (HostWorkDimension::StructuralBytes, 0),
            (HostWorkDimension::StructuralBytes, 1),
            (HostWorkDimension::StructuralItems, 1),
        ] {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(HostWorkDimension::StructuralBytes, HostWorkLimit::new(bytes));
            limits.set(HostWorkDimension::StructuralItems, HostWorkLimit::new(items));
            let limit = limits.get(dimension).get();
            limits.set(dimension, HostWorkLimit::new(limit - deficit));
            let host = HostWorkBudget::new(limits);
            let result = InterpreterImpl::parse_source(&source, HashMap::new(), Some(&host));
            if deficit == 0 {
                prop_assert_eq!(result.unwrap(), expected.clone());
                prop_assert_eq!(host.usage(HostWorkDimension::StructuralBytes).get(), bytes);
                prop_assert_eq!(host.usage(HostWorkDimension::StructuralItems).get(), items);
            } else {
                prop_assert!(matches!(result, Err(InterpreterError::HostWorkRejected)));
                prop_assert!(host.is_rejected());
            }
        }
        prop_assert_eq!(InterpreterImpl::parse_source(&source, HashMap::new(), None).unwrap(), expected);
    }
}

#[test]
fn shared_source_parser_rejects_unpaid_source_before_syntax_processing() {
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        InterpreterImpl::parse_source("new {", HashMap::new(), Some(&host)),
        Err(InterpreterError::HostWorkRejected),
    ));
    assert!(matches!(
        InterpreterImpl::parse_source("new {", HashMap::new(), None),
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
        InterpreterImpl::parse_source(source, env, None).unwrap(),
        expected
    );
}

#[test]
fn prepared_native_input_charges_the_same_structural_work_as_source_input() {
    let source = "new c in { c!(1) | for (@x <- c) { @0!(x) } }";
    let normalized = Compiler::source_to_adt(source).unwrap();
    let limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
    let source_host = HostWorkBudget::new(limits.clone());
    let prepared_host = HostWorkBudget::new(limits);
    let parsed = InterpreterImpl::parse_source(source, HashMap::new(), Some(&source_host)).unwrap();
    let prepared = PreparedProgram::from_normalized(normalized).into_par();
    InterpreterImpl::reserve_prepared_structure(
        &prepared,
        u64::try_from(source.len()).unwrap(),
        &prepared_host,
    )
    .unwrap();
    assert_eq!(parsed, prepared);
    for dimension in [
        HostWorkDimension::StructuralBytes,
        HostWorkDimension::StructuralItems,
    ] {
        assert_eq!(source_host.usage(dimension), prepared_host.usage(dimension));
    }
}
