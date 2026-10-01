use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

proptest! {
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
