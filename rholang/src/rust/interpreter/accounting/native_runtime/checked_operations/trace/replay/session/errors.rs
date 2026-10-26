use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchError;
use crate::rust::interpreter::accounting::native_phlo_rules::NativePhloExecutionError;

#[test]
fn native_error_transport_does_not_turn_mismatch_into_denial() {
    for cause in [
        NativeReplayError::Construction("Out of phlogistons".into()),
        NativeReplayError::Outcome,
        NativeReplayError::Budget(NativeBudgetTraceError::Decision),
        NativeReplayError::Budget(NativeBudgetTraceError::Execution(
            NativePhloExecutionError::BoundExceeded,
        )),
        NativeReplayError::Budget(NativeBudgetTraceError::Work(
            FundingSearchError::AllocationFailed,
        )),
    ] {
        assert!(matches!(error(cause), RSpaceError::InterpreterError(_)));
    }
    assert_eq!(
        error(InterpreterError::OutOfPhlogistonsError),
        RSpaceError::OutOfPhlogistons,
    );
}
