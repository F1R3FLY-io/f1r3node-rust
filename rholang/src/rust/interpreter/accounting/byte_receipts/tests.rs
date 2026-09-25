use std::sync::{Arc, Barrier};

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostSignature};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::{self, ResourceMultiset};
use crate::rust::interpreter::accounting::byte_accounting::BYTE_COST_SCHEDULE_V1;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::errors::InterpreterError;

fn authority(unit: bool) -> CostAuthority {
    let signature = CostSignature {
        value: Some(if unit {
            Value::Unit(true)
        } else {
            Value::Ground(b"byte receipt payer".to_vec())
        }),
    };
    CostAuthority {
        regions: vec![authority::cost_region(&signature, b"byte receipts", 0).unwrap()],
    }
}

fn budget() -> RuntimeBudget { RuntimeBudget::new(Cost::create(1_000_000, "byte receipts")) }

fn reserve(
    budget: &RuntimeBudget,
    identity: [u8; 32],
    kind: u8,
    charge: ByteCharge,
    persistent: bool,
    measured: bool,
) -> Result<(), InterpreterError> {
    let authority = authority(false);
    let amount = charge.cost(BYTE_COST_SCHEDULE_V1).unwrap();
    match (kind, measured) {
        (0, true) => {
            budget.reserve_produce_introduction_measured(identity, &authority, charge, persistent)
        }
        (0, false) => {
            budget.reserve_produce_introduction_identity(identity, &authority, amount, persistent)
        }
        (1, true) => {
            budget.reserve_consume_introduction_measured(identity, &authority, charge, persistent)
        }
        (1, false) => {
            budget.reserve_consume_introduction_identity(identity, &authority, amount, persistent)
        }
        (2, true) => budget.reserve_comm_authority_measured(identity, &authority, charge),
        (2, false) => {
            budget.reserve_comm_authority_identity_with_byte_cost(identity, &authority, amount)
        }
        _ => unreachable!(),
    }
}

fn assert_paired(snapshot: &ByteObservationSnapshot) {
    assert!(snapshot.has_complete_measurements());
    for row in &snapshot.rows {
        let charge = row
            .measurement
            .unwrap()
            .cost(BYTE_COST_SCHEDULE_V1)
            .unwrap();
        assert_eq!(row.legacy_amount, (charge > 0).then_some(charge));
        assert_eq!(
            row.legacy_event().map(|event| event.amount),
            row.legacy_amount
        );
    }
}

#[test]
fn measured_bytes_preserve_exact_legacy_events_cost_and_trace() {
    let measured = budget();
    let legacy = budget();
    let _measured_scope = measured.enter_comm_accounting_scope();
    let _legacy_scope = legacy.enter_comm_accounting_scope();
    for kind in 0..3 {
        let charge = ByteCharge {
            introduction_bytes: 3,
            transfer_bytes: 5,
            trace_bytes: 7,
        };
        for _ in 0..2 {
            reserve(&measured, [kind; 32], kind, charge, true, true).unwrap();
            reserve(&legacy, [kind; 32], kind, charge, true, false).unwrap();
        }
    }
    let snapshot = measured.byte_observations();
    assert_paired(&snapshot);
    assert_eq!(snapshot.rows.len(), 3);
    assert_eq!(snapshot.legacy_events(), legacy.authority_byte_events());
    assert_eq!(measured.authority_events(), legacy.authority_events());
    assert_eq!(measured.total_cost(), legacy.total_cost());
    assert_eq!(measured.cost_trace_digest(), legacy.cost_trace_digest());
    assert_eq!(
        measured.cost_trace_event_count(),
        legacy.cost_trace_event_count()
    );
    assert!(!legacy.byte_observations().has_complete_measurements());
}

#[test]
fn unit_comm_preserves_raw_measurement_without_legacy_charge() {
    let budget = RuntimeBudget::new(Cost::create(0, "unit receipt"));
    let _scope = budget.enter_comm_accounting_scope();
    budget.install_authority_allocation(ResourceMultiset::default());
    let measurement = ByteCharge {
        introduction_bytes: 0,
        transfer_bytes: 17,
        trace_bytes: 31,
    };
    budget
        .reserve_comm_authority_measured([1; 32], &authority(true), measurement)
        .unwrap();
    let snapshot = budget.byte_observations();
    assert!(snapshot.has_complete_measurements());
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].measurement, Some(measurement));
    assert_eq!(snapshot.rows[0].legacy_amount, None);
    assert_eq!(snapshot.rows[0].legacy_event(), None);
    assert!(snapshot.legacy_events().is_empty());
    assert!(budget.authority_realized().0.is_empty());
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.authority_events().len(), 1);
}

#[test]
fn zero_introductions_reject_while_zero_legacy_comm_retains_missing_measurement() {
    for kind in 0..3 {
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        let result = reserve(
            &budget,
            [kind; 32],
            kind,
            ByteCharge::default(),
            true,
            false,
        );
        if kind < 2 {
            assert_eq!(result, Err(InterpreterError::OutOfPhlogistonsError));
            assert!(budget.byte_observations().rows.is_empty());
            assert_eq!(
                reserve(&budget, [kind; 32], kind, ByteCharge::default(), true, true),
                Err(InterpreterError::OutOfPhlogistonsError)
            );
            assert!(budget.byte_observations().rows.is_empty());
            continue;
        }
        result.unwrap();
        let before = budget.byte_observations();
        assert_eq!(before.rows.len(), 1);
        assert_eq!(before.rows[0].measurement, None);
        assert_eq!(before.rows[0].legacy_amount, None);
        assert!(!before.has_complete_measurements());
        assert!(reserve(&budget, [kind; 32], kind, ByteCharge::default(), true, true).is_err());
        assert_eq!(budget.byte_observations(), before);
    }
}

#[test]
fn measured_retry_cannot_retrofit_a_positive_legacy_receipt() {
    let charge = ByteCharge {
        introduction_bytes: 1,
        ..ByteCharge::default()
    };
    for kind in 0..3 {
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        reserve(&budget, [kind; 32], kind, charge, true, false).unwrap();
        let before = budget.byte_observations();
        assert!(!before.has_complete_measurements());
        assert!(reserve(&budget, [kind; 32], kind, charge, true, true).is_err());
        assert_eq!(budget.byte_observations(), before);
    }
}

#[test]
fn measured_retry_requires_exact_components_not_only_equal_weighted_cost() {
    let first = ByteCharge {
        introduction_bytes: 3,
        transfer_bytes: 5,
        trace_bytes: 7,
    };
    let changed = ByteCharge {
        introduction_bytes: 4,
        transfer_bytes: 4,
        trace_bytes: 7,
    };
    assert_eq!(
        first.cost(BYTE_COST_SCHEDULE_V1),
        changed.cost(BYTE_COST_SCHEDULE_V1)
    );
    for kind in 0..3 {
        let budget = budget();
        let _scope = budget.enter_comm_accounting_scope();
        reserve(&budget, [kind; 32], kind, first, true, true).unwrap();
        let before = budget.byte_observations();
        reserve(&budget, [kind; 32], kind, first, true, true).unwrap();
        assert!(reserve(&budget, [kind; 32], kind, changed, true, true).is_err());
        reserve(&budget, [kind; 32], kind, first, true, false).unwrap();
        assert_eq!(budget.byte_observations(), before);
    }
}

#[test]
fn nonpersistent_introductions_retain_every_measurement_occurrence() {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let charge = ByteCharge {
        introduction_bytes: 2,
        ..ByteCharge::default()
    };
    for kind in 0..2 {
        for _ in 0..5 {
            reserve(&budget, [1; 32], kind, charge, false, true).unwrap();
        }
    }
    let snapshot = budget.byte_observations();
    assert_paired(&snapshot);
    assert_eq!(snapshot.rows.len(), 10);
    assert_eq!(snapshot.legacy_events().len(), 10);
    assert_eq!(budget.total_cost().value, 20);
}

#[test]
fn completeness_requires_active_metering_and_survives_only_a_full_reset_after_history_loss() {
    let budget = budget();
    assert!(!budget.byte_observations().has_complete_measurements());
    let scope = budget.enter_comm_accounting_scope();
    assert!(budget.byte_observations().has_complete_measurements());
    budget.install_authority_allocation(ResourceMultiset::default());
    assert!(budget.byte_observations().has_complete_measurements());
    let charge = ByteCharge {
        introduction_bytes: 2,
        ..ByteCharge::default()
    };
    reserve(&budget, [1; 32], 0, charge, true, true).unwrap();
    let retained = budget.byte_observations();
    budget.install_authority_allocation(ResourceMultiset::default());
    let cleared = budget.byte_observations();
    assert!(cleared.rows.is_empty());
    assert!(cleared.history_lost);
    assert!(!cleared.has_complete_measurements());
    reserve(&budget, [1; 32], 0, charge, true, true).unwrap();
    assert_eq!(budget.byte_observations(), cleared);
    assert!(retained.has_complete_measurements());
    assert_eq!(retained.rows.len(), 1);
    budget.reset_for_system_deploy();
    assert!(budget.byte_observations().has_complete_measurements());
    assert!(!budget.byte_observations().history_lost);
    reserve(&budget, [1; 32], 0, charge, true, true).unwrap();
    assert_eq!(budget.byte_observations().rows.len(), 1);
    {
        let _unmetered = budget.enter_unmetered_scope();
        assert!(!budget.byte_observations().has_complete_measurements());
        reserve(&budget, [2; 32], 1, charge, true, true).unwrap();
        assert_eq!(budget.byte_observations().rows.len(), 1);
    }
    assert!(budget.byte_observations().has_complete_measurements());
    drop(scope);
    assert!(!budget.byte_observations().has_complete_measurements());
}

#[test]
fn concurrent_snapshots_never_observe_unpaired_measurement_and_charge() {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    let start = Arc::new(Barrier::new(5));
    std::thread::scope(|threads| {
        for worker in 0..4_u8 {
            let budget = budget.clone();
            let start = Arc::clone(&start);
            threads.spawn(move || {
                start.wait();
                for sequence in 0..32_u8 {
                    let mut id = [0; 32];
                    id[0] = worker;
                    id[1] = sequence;
                    let charge = ByteCharge {
                        introduction_bytes: u64::from(sequence),
                        transfer_bytes: 1,
                        trace_bytes: 2,
                    };
                    reserve(&budget, id, worker % 3, charge, true, true).unwrap();
                    reserve(&budget, id, worker % 3, charge, true, true).unwrap();
                    assert_paired(&budget.byte_observations());
                }
            });
        }
        start.wait();
        for _ in 0..128 {
            assert_paired(&budget.byte_observations());
            std::thread::yield_now();
        }
    });
    let snapshot = budget.byte_observations();
    assert_paired(&snapshot);
    assert_eq!(snapshot.rows.len(), 128);
    assert_eq!(snapshot.legacy_events(), budget.authority_byte_events());
}

#[test]
fn legacy_comm_without_authority_cannot_claim_complete_measurements() {
    let budget = budget();
    let _scope = budget.enter_comm_accounting_scope();
    assert!(budget.byte_observations().has_complete_measurements());
    budget.reserve_comm_identity([7; 32]).unwrap();
    assert!(!budget.byte_observations().has_complete_measurements());
    budget.install_authority_allocation(ResourceMultiset::default());
    assert!(!budget.byte_observations().has_complete_measurements());
    budget.reset_for_system_deploy();
    assert!(budget.byte_observations().has_complete_measurements());
}

fn paired_row(identity: u8) -> Arc<ByteObservation> {
    Arc::new(ByteObservation {
        event_id: [identity; 32],
        kind: AuthorityByteEventKind::Comm,
        authority: authority(false),
        measurement: Some(ByteCharge {
            introduction_bytes: 1,
            transfer_bytes: 2,
            trace_bytes: 3,
        }),
        legacy_amount: Some(6),
    })
}

#[test]
fn log_partial_clear_is_sticky_and_snapshots_retain_original_flags() {
    let mut log = ByteObservationLog::default();
    log.clear_partial_history(false);
    assert!(log.snapshot(true).has_complete_measurements());
    log.clear_partial_history(true);
    assert!(!log.snapshot(true).has_complete_measurements());
    log.clear_partial_history(false);
    assert!(log.snapshot(true).history_lost);
    log.reset();
    log.try_reserve(1).unwrap();
    log.push(paired_row(1));
    let retained = log.snapshot(true);
    let cloned = log.clone();
    log.clear_partial_history(false);
    assert!(log.snapshot(true).rows.is_empty());
    assert!(log.snapshot(true).history_lost);
    assert_paired(&retained);
    assert_eq!(cloned.snapshot(true), retained);
    assert!(!cloned.snapshot(false).has_complete_measurements());
    log.reset();
    assert!(log.snapshot(true).has_complete_measurements());
    assert_eq!(retained.rows.len(), 1);
}

#[test]
fn loom_log_parallel_publication_retains_atomic_raw_and_legacy_pairs() {
    let rows = [paired_row(1), paired_row(2)];
    loom::model(move || {
        let log = loom::sync::Arc::new(loom::sync::Mutex::new(ByteObservationLog::default()));
        let mut writers = Vec::new();
        for row in &rows {
            let log = loom::sync::Arc::clone(&log);
            let row = Arc::clone(row);
            writers.push(loom::thread::spawn(move || {
                let mut guard = log.lock().unwrap();
                guard.try_reserve(1).unwrap();
                guard.push(row);
            }));
        }
        let observed = log.lock().unwrap().snapshot(true);
        assert_component_pairs(&observed);
        for writer in writers {
            writer.join().unwrap();
        }
        let completed = log.lock().unwrap().snapshot(true);
        assert_component_pairs(&completed);
        assert_eq!(completed.rows.len(), 2);
        for row in observed.rows {
            assert!(completed
                .rows
                .iter()
                .any(|candidate| Arc::ptr_eq(candidate, &row)));
        }
    });
}

#[test]
fn loom_owned_log_snapshot_survives_quiescent_reset_and_new_publication() {
    let first = paired_row(1);
    let second = paired_row(2);
    loom::model(move || {
        let mut initial = ByteObservationLog::default();
        initial.try_reserve(1).unwrap();
        initial.push(Arc::clone(&first));
        let retained = initial.snapshot(true);
        let log = loom::sync::Arc::new(loom::sync::Mutex::new(initial));
        let writer_log = loom::sync::Arc::clone(&log);
        let second = Arc::clone(&second);
        let writer = loom::thread::spawn(move || {
            let mut guard = writer_log.lock().unwrap();
            guard.reset();
            guard.try_reserve(1).unwrap();
            guard.push(second);
        });
        assert_component_pairs(&retained);
        assert_eq!(retained.rows[0].event_id, [1; 32]);
        writer.join().unwrap();
        let completed = log.lock().unwrap().snapshot(true);
        assert_component_pairs(&completed);
        assert_eq!(completed.rows.len(), 1);
        assert_eq!(completed.rows[0].event_id, [2; 32]);
        assert_eq!(retained.rows[0].event_id, [1; 32]);
    });
}

fn assert_component_pairs(snapshot: &ByteObservationSnapshot) {
    assert!(snapshot.has_complete_measurements());
    let checked = snapshot.checked_measurements(snapshot.rows.len()).unwrap();
    assert_eq!(checked.rows(), snapshot.rows.as_slice());
    assert_eq!(checked.totals(), ByteCharge {
        introduction_bytes: snapshot.rows.len() as u64,
        transfer_bytes: snapshot.rows.len() as u64 * 2,
        trace_bytes: snapshot.rows.len() as u64 * 3,
    });
    for row in &snapshot.rows {
        let measurement = row.measurement.unwrap();
        assert_eq!(
            (
                measurement.introduction_bytes,
                measurement.transfer_bytes,
                measurement.trace_bytes
            ),
            (1, 2, 3)
        );
        assert_eq!(row.legacy_amount, Some(6));
    }
}

#[test]
fn counted_measurements_preserve_repeated_raw_only_rows_without_expansion() {
    let raw = ByteCharge {
        introduction_bytes: u64::MAX / 2,
        transfer_bytes: u64::MAX / 2,
        trace_bytes: u64::MAX / 2,
    };
    let row = Arc::new(ByteObservation {
        authority: authority(true),
        measurement: Some(raw),
        legacy_amount: None,
        ..(*paired_row(1)).clone()
    });
    let snapshot = ByteObservationSnapshot {
        rows: vec![Arc::clone(&row), Arc::clone(&row)],
        metered_context: true,
        history_lost: false,
    };
    let counted = snapshot.checked_measurements(2).unwrap();
    assert!(std::ptr::eq(counted.rows(), snapshot.rows.as_slice()));
    assert_eq!(Arc::strong_count(&row), 3);
    assert_eq!(counted.totals(), ByteCharge {
        introduction_bytes: u64::MAX - 1,
        transfer_bytes: u64::MAX - 1,
        trace_bytes: u64::MAX - 1,
    });
    assert!(snapshot.legacy_events().is_empty());
    assert_eq!(
        snapshot.checked_measurements(1),
        Err(ByteMeasurementError::EntryLimit)
    );
}

#[test]
fn counted_measurements_require_complete_history_and_measurement_context() {
    for metered_context in [false, true] {
        for history_lost in [false, true] {
            for measured in [false, true] {
                let snapshot = ByteObservationSnapshot {
                    rows: vec![Arc::new(ByteObservation {
                        measurement: measured.then_some(ByteCharge::default()),
                        ..(*paired_row(1)).clone()
                    })],
                    metered_context,
                    history_lost,
                };
                let before = snapshot.clone();
                let captured = snapshot.checked_measurements(1);
                assert_eq!(
                    captured.is_ok(),
                    metered_context && !history_lost && measured
                );
                if captured.is_err() {
                    assert_eq!(captured, Err(ByteMeasurementError::Incomplete));
                }
                assert_eq!(snapshot, before);
            }
        }
    }
    let empty = ByteObservationSnapshot {
        metered_context: true,
        ..Default::default()
    };
    assert_eq!(
        empty.checked_measurements(0).unwrap().totals(),
        ByteCharge::default()
    );
    assert_eq!(
        ByteObservationSnapshot::default().checked_measurements(0),
        Err(ByteMeasurementError::Incomplete)
    );
}

#[test]
fn counted_measurements_reject_each_dimension_overflow_without_mutation() {
    for raw in [
        ByteCharge {
            introduction_bytes: u64::MAX,
            ..Default::default()
        },
        ByteCharge {
            transfer_bytes: u64::MAX,
            ..Default::default()
        },
        ByteCharge {
            trace_bytes: u64::MAX,
            ..Default::default()
        },
    ] {
        let first = Arc::new(ByteObservation {
            measurement: Some(raw),
            ..(*paired_row(1)).clone()
        });
        let mut snapshot = ByteObservationSnapshot {
            rows: vec![first],
            metered_context: true,
            history_lost: false,
        };
        assert_eq!(snapshot.checked_measurements(2).unwrap().totals(), raw);
        snapshot.rows.push(paired_row(2));
        let before = snapshot.clone();
        assert_eq!(
            snapshot.checked_measurements(2),
            Err(ByteMeasurementError::Overflow)
        );
        assert_eq!(snapshot, before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn counted_measurements_match_wide_reference_and_preserve_permutations(
        samples in prop_oneof![
            prop::collection::vec((0_u64..1000, 0_u64..1000, 0_u64..1000,
                any::<u16>(), any::<bool>(), any::<bool>()), 0..65),
            prop::collection::vec((any::<u64>(), any::<u64>(), any::<u64>(),
                any::<u16>(), any::<bool>(), any::<bool>()), 0..65),
        ],
        maximum_entries in 0_usize..65,
    ) {
        let mut reference = [0_u128; 3];
        let rows: Vec<_> = samples.iter().map(|&(introduction_bytes, transfer_bytes, trace_bytes, _, unit, legacy)| {
            reference[0] += u128::from(introduction_bytes);
            reference[1] += u128::from(transfer_bytes);
            reference[2] += u128::from(trace_bytes);
            Arc::new(ByteObservation {
                authority: authority(unit),
                measurement: Some(ByteCharge { introduction_bytes, transfer_bytes, trace_bytes }),
                legacy_amount: legacy.then_some(7),
                ..(*paired_row(1)).clone()
            })
        }).collect();
        let snapshot = ByteObservationSnapshot { rows, metered_context: true, history_lost: false };
        let before = snapshot.clone();
        let result = snapshot.checked_measurements(maximum_entries);
        let fits = samples.len() <= maximum_entries && reference.iter().all(|&n| n <= u128::from(u64::MAX));
        prop_assert_eq!(result.is_ok(), fits);
        if let Ok(ref checked) = result {
            prop_assert_eq!(checked.rows(), snapshot.rows.as_slice());
            prop_assert!(std::ptr::eq(checked.rows(), snapshot.rows.as_slice()));
            let totals = checked.totals();
            prop_assert_eq!([u128::from(totals.introduction_bytes), u128::from(totals.transfer_bytes), u128::from(totals.trace_bytes)], reference);
        }
        let mut order: Vec<_> = (0..samples.len()).collect();
        order.sort_by_key(|&i| samples[i].3);
        let reordered = ByteObservationSnapshot {
            rows: order.iter().map(|&i| Arc::clone(&snapshot.rows[i])).collect(),
            metered_context: true,
            history_lost: false,
        };
        prop_assert_eq!(result.map(|checked| checked.totals()), reordered.checked_measurements(maximum_entries).map(|checked| checked.totals()));
        prop_assert_eq!(snapshot, before);
    }

    #[test]
    fn arbitrary_event_order_preserves_raw_legacy_pairing_and_persistent_multiplicity(
        cases in prop::collection::vec(prop_oneof![
            Just((0_u8, false, 0_u64, 0_u64, 0_u64, 0_u16)),
            Just((1_u8, true, 0_u64, 0_u64, 0_u64, 0_u16)),
            (0_u8..3, any::<bool>(), 0_u64..20, 0_u64..20, 0_u64..20, any::<u16>()),
        ], 0..17),
    ) {
        let measured = budget();
        let legacy = budget();
        let reordered = budget();
        let _measured_scope = measured.enter_comm_accounting_scope();
        let _legacy_scope = legacy.enter_comm_accounting_scope();
        let _reordered_scope = reordered.enter_comm_accounting_scope();
        let mut order: Vec<_> = (0..cases.len()).collect();
        order.sort_by_key(|index| cases[*index].5);
        for (target, measured_mode, indices) in [
            (&measured, true, (0..cases.len()).collect::<Vec<_>>()),
            (&legacy, false, (0..cases.len()).collect::<Vec<_>>()),
            (&reordered, true, order),
        ] {
            for index in indices {
                let (kind, persistent, introduction_bytes, transfer_bytes, trace_bytes, _) = cases[index];
                let charge = ByteCharge { introduction_bytes, transfer_bytes, trace_bytes };
                for _ in 0..2 {
                    let result = reserve(target, [index as u8; 32], kind, charge, persistent, measured_mode);
                    if kind < 2 && charge == ByteCharge::default() {
                        prop_assert_eq!(result, Err(InterpreterError::OutOfPhlogistonsError));
                    } else {
                        result.unwrap();
                    }
                }
            }
        }
        let snapshot = measured.byte_observations();
        assert_paired(&snapshot);
        assert_paired(&reordered.byte_observations());
        let expected_rows: usize = cases.iter().map(|(kind, persistent, introduction, transfer, trace, _)| {
            if *kind < 2 && introduction + transfer + trace == 0 { 0 }
            else if *kind == 2 || *persistent { 1 }
            else { 2 }
        }).sum();
        prop_assert_eq!(snapshot.rows.len(), expected_rows);
        prop_assert_eq!(snapshot.legacy_events(), legacy.authority_byte_events());
        prop_assert_eq!(snapshot.legacy_events(), reordered.authority_byte_events());
        prop_assert_eq!(measured.total_cost(), legacy.total_cost());
        prop_assert_eq!(measured.total_cost(), reordered.total_cost());
        prop_assert_eq!(measured.cost_trace_digest(), legacy.cost_trace_digest());
        prop_assert_eq!(measured.cost_trace_digest(), reordered.cost_trace_digest());
        let sorted_rows = |snapshot: ByteObservationSnapshot| {
            let mut rows = snapshot.rows;
            rows.sort_by_key(|row| (row.event_id, row.kind.tag()));
            rows
        };
        prop_assert_eq!(sorted_rows(snapshot), sorted_rows(reordered.byte_observations()));
    }
}
