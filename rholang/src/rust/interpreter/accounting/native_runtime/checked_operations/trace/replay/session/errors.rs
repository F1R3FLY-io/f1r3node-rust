use models::rust::host_work::{
    HostWorkLimit, HostWorkLimits, HostWorkReservationError, HostWorkUnits,
};
use proptest::prelude::*;

use super::*;

fn exhausted() -> HostWorkReservationError {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)))
        .reserve(
            HostWorkDimension::VerificationOperations,
            HostWorkUnits::new(1),
        )
        .unwrap_err()
}

proptest! {
    #[test]
    fn native_error_transport_preserves_host_class(route in 0u8..5, hops in 0usize..64) {
        let cause = match route {
            0 => NativeReplayError::Host(InterpreterError::HostWorkRejected),
            1 => NativeReplayError::Budget(NativeBudgetTraceError::Work(
                FundingSearchError::HostWork(exhausted()),
            )),
            2 => NativeReplayError::Budget(NativeBudgetTraceError::Execution(
                NativePhloExecutionError::Work(FundingSearchError::HostWork(exhausted())),
            )),
            3 => NativeReplayError::Budget(NativeBudgetTraceError::Execution(
                NativePhloExecutionError::Authority(AuthorityError::HostWorkRejected),
            )),
            _ => NativeReplayError::Budget(NativeBudgetTraceError::Execution(
                NativePhloExecutionError::Region(NativePhloRegionError::Authority(
                    AuthorityError::HostWorkRejected,
                )),
            )),
        };
        let mut actual = error(cause);
        prop_assert_eq!(&actual, &RSpaceError::HostWorkRejected);
        for _ in 0..hops {
            actual = error(InterpreterError::from(actual));
            prop_assert_eq!(&actual, &RSpaceError::HostWorkRejected);
        }
    }
}

#[test]
fn native_error_transport_does_not_turn_mismatch_into_denial() {
    for cause in [
        NativeReplayError::Construction("Host work budget rejected evaluation.".into()),
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
