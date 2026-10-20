use futures::FutureExt;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};

use super::*;

fn observation(amount: u64) -> Arc<ByteObservation> {
    Arc::new(ByteObservation {
        event_id: [9; 32],
        kind: authority::AuthorityByteEventKind::Comm,
        authority: authority(1),
        measurement: Some(ByteCharge {
            introduction_bytes: 0,
            transfer_bytes: amount,
            trace_bytes: 0,
        }),
        legacy_amount: None,
    })
}

#[test]
fn native_recording_keeps_denials_overflow_and_compatible_retries_distinct() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(5, [0, 0, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let auth = authority(1);
    reserve(&budget, &auth, 2, 0, 3).unwrap();
    reserve(&budget, &auth, 2, 0, 3).unwrap();
    assert!(matches!(
        reserve(&budget, &auth, 3, 2, 4),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    reserve(&budget, &auth, 5, 0, 0).unwrap();
    reserve(&budget, &auth, 5, 0, 0).unwrap();
    let result = budget.native_budget_recording().unwrap().unwrap();
    assert_eq!(result.used, 3);
    assert_eq!(
        result
            .attempts
            .iter()
            .map(|row| row.granted)
            .collect::<Vec<_>>(),
        [true, false, true, true]
    );
    assert_eq!(result.retries.len(), 1);
    assert_eq!(result.retries[0].accepted_attempt, 0);
    assert_eq!(result.retries[0].fresh_before, 1);
    assert_eq!(
        result.retries[0].observation,
        result.attempts[0].observation
    );
    assert_ne!(result.retries[0].occurrence, result.attempts[0].occurrence);
    assert_eq!(budget.byte_observations().rows.len(), 3);
}

#[test]
fn retry_cuts_count_fresh_denials_and_zero_charges_without_counting_retries() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(1, [0, 0, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    let auth = authority(1);
    reserve(&budget, &auth, 2, 0, 1).unwrap();
    reserve(&budget, &auth, 2, 0, 1).unwrap();
    assert!(matches!(
        reserve(&budget, &auth, 3, 2, 1),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    reserve(&budget, &auth, 2, 0, 1).unwrap();
    reserve(&budget, &auth, 5, 0, 0).unwrap();
    reserve(&budget, &auth, 2, 0, 1).unwrap();
    reserve(&budget, &auth, 2, 0, 1).unwrap();
    let recording = budget.native_budget_recording().unwrap().unwrap();
    assert_eq!(recording.used, 1);
    assert_eq!(recording.attempts.len(), 3);
    assert_eq!(
        recording
            .retries
            .iter()
            .map(|retry| retry.fresh_before)
            .collect::<Vec<_>>(),
        [1, 2, 3, 3],
    );
    assert!(recording
        .retries
        .iter()
        .all(|retry| retry.accepted_attempt == 0));
}

#[test]
fn preparation_and_reservation_overflow_record_denials_without_receipts() {
    for (weight, first, second) in [(2, 0, u64::MAX), (1, u64::MAX, 1)] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(u64::MAX, [0, 0, weight, 0]))
            .unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        reserve(&budget, &authority(1), 1, 0, first).unwrap();
        assert!(matches!(
            reserve(&budget, &authority(1), 3, 0, second),
            Err(InterpreterError::OutOfPhlogistonsError)
        ));
        let result = budget.native_budget_recording().unwrap().unwrap();
        assert_eq!(result.used, first);
        assert_eq!(result.attempts.len(), 2);
        assert!(result.attempts[0].granted);
        assert!(!result.attempts[1].granted);
        assert_eq!(
            result.attempts[1]
                .observation
                .measurement
                .unwrap()
                .transfer_bytes,
            second
        );
        assert_eq!(budget.byte_observations().rows.len(), 1);
    }
}

#[test]
fn stale_preparation_cannot_charge_or_invalidate_a_new_execution() {
    for amount in [1, u64::MAX] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(u64::MAX, [0, 0, 2, 0]))
            .unwrap();
        let prepared = with_operation(&budget, || {
            budget.prepare_native_observation(&observation(amount))
        })
        .unwrap()
        .unwrap();
        assert!(budget.native_budget_recording().is_err());
        budget
            .reset_for_native_execution(config(u64::MAX, [0, 0, 2, 0]))
            .unwrap();
        {
            let mut state = budget.authority_state.lock().unwrap();
            assert!(budget
                .reserve_observation_usage(
                    &mut state,
                    Some(&prepared),
                    [9; 32],
                    BillableKind::Comm,
                    0,
                    "stale native test"
                )
                .is_err());
        }
        drop(prepared);
        let result = budget.native_budget_recording().unwrap().unwrap();
        assert_eq!(result.used, 0);
        assert!(result.attempts.is_empty());
        assert!(result.retries.is_empty());
    }
}

#[test]
fn abandoned_preparation_prevents_evidence_export() {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(10, [0, 0, 1, 0]))
        .unwrap();
    let prepared = with_operation(&budget, || {
        budget.prepare_native_observation(&observation(1))
    })
    .unwrap()
    .unwrap();
    assert!(budget.native_budget_recording().is_err());
    assert!(budget.native_operation_recording().is_err());
    drop(prepared);
    assert!(budget.native_budget_recording().is_err());
    assert!(budget.native_operation_recording().is_err());
    assert_eq!(budget.native_phlo_usage(), Some(0));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Changed by DR-131: the allocation-failure branch went with the
    /// recorder's allocation hook. The retry failure branch stays.
    #[test]
    fn recording_failure_blocks_other_preparations_before_ticket_drop(
        accepted_count in 1usize..8,
        amount in 1u64..20,
    ) {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget.reset_for_native_execution(config(1000, [0, 0, 1, 0])).unwrap();
        for _ in 0..accepted_count {
            let accepted = with_operation(&budget, || {
                budget.prepare_native_observation(&observation(amount))
            }).unwrap().unwrap();
            budget.authority_state.lock().unwrap().native.as_mut().unwrap()
                .record_charge(&accepted).unwrap();
        }
        let failed = with_operation(&budget, || {
            budget.prepare_native_observation(&observation(amount + 1))
        }).unwrap().unwrap();
        let other = with_operation(&budget, || {
            budget.prepare_native_observation(&observation(amount))
        }).unwrap().unwrap();
        {
            let mut state = budget.authority_state.lock().unwrap();
            let native = state.native.as_mut().unwrap();
            prop_assert!(native.record_retry(&failed).is_err());
        }
        {
            let mut state = budget.authority_state.lock().unwrap();
            let native = state.native.as_mut().unwrap();
            prop_assert!(native.record_charge(&other).is_err(),
                "another preparation published after recording failure but before ticket drop");
            prop_assert_eq!(native.recording.attempts.len(), accepted_count);
            prop_assert!(native.recording.retries.is_empty());
            prop_assert_eq!(native.reservation.used(), accepted_count as u64 * amount);
        }
        drop(failed);
        drop(other);
        prop_assert!(budget.native_budget_recording().is_err());
    }
}

#[test]
fn missing_foreign_or_oversized_operation_context_cannot_publish_evidence() {
    for context in [
        None,
        Some(OperationOrder {
            session: [1; 32],
            path: vec![(0, 0)].into(),
        }),
        Some(OperationOrder {
            session: [0; 32],
            path: vec![(0, 0), (1, 0)].into(),
        }),
    ] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let mut settings = config(10, [0, 0, 1, 0]);
        settings.limits.path_segments = 1;
        budget.reset_for_native_execution(settings).unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        let call = || {
            budget.reserve_comm_authority_measured([9; 32], &authority(1), ByteCharge {
                introduction_bytes: 0,
                transfer_bytes: 1,
                trace_bytes: 0,
            })
        };
        let result = match context {
            Some(context) => operation_context::scope(context, async { call() })
                .now_or_never()
                .unwrap(),
            None => call(),
        };
        assert!(result.is_err());
        assert!(budget.native_budget_recording().is_err());
        assert_eq!(budget.native_phlo_usage(), Some(0));
        assert!(budget.byte_observations().rows.is_empty());
    }
}

#[test]
fn duplicate_occurrence_and_trace_limit_fail_without_extra_debits() {
    for duplicate in [true, false] {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        let mut settings = config(10, [0, 0, 1, 0]);
        settings.limits.attempts = if duplicate { 10 } else { 1 };
        budget.reset_for_native_execution(settings).unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        let call = |path| {
            operation_context::scope(
                OperationOrder {
                    session: [0; 32],
                    path: vec![(path, 0)].into(),
                },
                async {
                    budget.reserve_produce_introduction_measured(
                        [9; 32],
                        &authority(1),
                        ByteCharge {
                            introduction_bytes: 0,
                            transfer_bytes: 1,
                            trace_bytes: 0,
                        },
                        false,
                    )
                },
            )
            .now_or_never()
            .unwrap()
        };
        call(0).unwrap();
        assert!(call(if duplicate { 0 } else { 1 }).is_err());
        assert_eq!(budget.native_phlo_usage(), Some(1));
        assert_eq!(budget.byte_observations().rows.len(), 1);
        assert!(budget.native_budget_recording().is_err());
    }
}
