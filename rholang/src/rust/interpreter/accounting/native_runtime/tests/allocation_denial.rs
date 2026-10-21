use models::rust::phlo_wire::PhloWireLimits;

use super::*;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::canonical_observation_authority;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetTraceError, NativeDenialCause, NativeStackTransferEvent, NativeStackTransferKind,
};

const LIMIT: u64 = 1_000;
const WEIGHTS: [u64; 4] = [0, 0, 1, 0];

fn charge(amount: u64) -> ByteCharge {
    ByteCharge {
        transfer_bytes: amount,
        ..ByteCharge::default()
    }
}

fn lane(amount: u64) -> ResourceMultiset<[u8; 32]> {
    let demand = authority::authority_demand(&authority(1)).unwrap();
    let (key, _) = demand.0.into_iter().next().unwrap();
    ResourceMultiset::singleton(key, amount)
}

fn played(allocation: u64) -> RuntimeBudget {
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(LIMIT, WEIGHTS))
        .unwrap();
    budget.install_authority_allocation(lane(allocation));
    budget
}

fn comm(budget: &RuntimeBudget, id: u8) -> Result<(), InterpreterError> {
    with_operation(budget, || {
        budget.reserve_comm_authority_measured([id; 32], &authority(1), charge(u64::from(id)))
    })
}

fn cells(count: usize) -> Vec<CostSignature> {
    vec![authority(1).regions[0].signature.clone().unwrap(); count]
}

fn check(
    recording: &NativeBudgetRecording,
    allocation: Option<u64>,
) -> Result<u64, NativeBudgetTraceError> {
    let limits = config(LIMIT, WEIGHTS).limits;
    with_contract(LIMIT, WEIGHTS, |contract| {
        let contract = match allocation {
            Some(amount) => contract.with_authority_allocation(lane(amount)),
            None => contract,
        };
        contract
            .check_budget_evidence(
                recording.session,
                Arc::clone(&recording.attempts),
                Arc::clone(&recording.transfers),
                limits,
            )
            .map(|evidence| evidence.total())
    })
}

fn with_transfers(
    recording: &NativeBudgetRecording,
    transfers: Vec<NativeStackTransferEvent>,
) -> NativeBudgetRecording {
    NativeBudgetRecording {
        transfers: transfers.into(),
        ..recording.clone()
    }
}

fn wire_limits() -> NativeRecordingWireLimits {
    NativeRecordingWireLimits {
        wire: PhloWireLimits {
            total_bytes: 1 << 20,
            field_bytes: 1 << 16,
        },
        budget: config(LIMIT, WEIGHTS).limits,
        operations: 64,
        source_entries: 64,
        footprint_entries: 64,
        footprint_bytes: 4_096,
        predecessor_edges: 64,
        total_path_segments: 4_096,
    }
}

#[test]
fn a_comm_denied_by_its_allocation_is_recorded_with_its_cause() {
    let budget = played(1);
    let _scope = budget.enter_comm_accounting_scope();
    comm(&budget, 1).unwrap();
    assert!(matches!(
        comm(&budget, 2),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    let recording = budget
        .native_budget_recording()
        .expect("an allocation denial keeps the recording complete")
        .expect("native execution records");
    let decisions: Vec<_> = recording
        .attempts
        .iter()
        .map(|row| (row.observation.event_id[0], row.granted, row.cause))
        .collect();
    let expected = vec![
        (1, true, NativeDenialCause::Budget),
        (2, false, NativeDenialCause::Allocation),
    ];
    assert_eq!(decisions, expected);
    assert!(recording.transfers.is_empty());
    assert!(budget
        .authority_state
        .lock()
        .unwrap()
        .frontier
        .contains_key(&[2; 32]));
    assert_eq!(check(&recording, Some(1)).ok(), Some(recording.used));
    assert!(matches!(
        check(&recording, None),
        Err(NativeBudgetTraceError::Decision)
    ));
    assert!(matches!(
        check(&recording, Some(2)),
        Err(NativeBudgetTraceError::Decision)
    ));
    let mut forged = recording.attempts.to_vec();
    forged[0].cause = NativeDenialCause::Allocation;
    assert!(matches!(
        check(
            &NativeBudgetRecording {
                attempts: forged.into(),
                ..recording.clone()
            },
            Some(1)
        ),
        Err(NativeBudgetTraceError::Decision)
    ));
}

#[test]
fn an_allocation_denial_is_recorded_only_for_a_comm() {
    let budget = played(0);
    let _scope = budget.enter_comm_accounting_scope();
    with_operation(&budget, || {
        budget.reserve_produce_introduction_measured([1; 32], &authority(1), charge(1), false)
    })
    .unwrap();
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let mut forged = recording.attempts.to_vec();
    forged[0].granted = false;
    forged[0].cause = NativeDenialCause::Allocation;
    assert!(matches!(
        check(
            &NativeBudgetRecording {
                attempts: forged.into(),
                ..recording
            },
            Some(0)
        ),
        Err(NativeBudgetTraceError::Decision)
    ));
}

#[test]
fn stack_transfers_are_recorded_in_their_place_among_the_attempts() {
    let budget = played(2);
    let _scope = budget.enter_comm_accounting_scope();
    let first = budget
        .prepare_authority_stack_transfer([10; 32], cells(1), &authority(1))
        .unwrap();
    assert!(matches!(
        budget.prepare_authority_stack_transfer([11; 32], cells(2), &authority(1)),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    comm(&budget, 3).unwrap();
    drop(first);
    comm(&budget, 4).unwrap();
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let canonical = canonical_observation_authority(&authority(1)).unwrap();
    let expected = [
        NativeStackTransferEvent {
            kind: NativeStackTransferKind::Reserve {
                produce_hash: [10; 32],
                authority: canonical.clone(),
                cells: 1,
                granted: true,
            },
            fresh_before: 0,
        },
        NativeStackTransferEvent {
            kind: NativeStackTransferKind::Reserve {
                produce_hash: [11; 32],
                authority: canonical,
                cells: 2,
                granted: false,
            },
            fresh_before: 0,
        },
        NativeStackTransferEvent {
            kind: NativeStackTransferKind::Release { reserve: 0 },
            fresh_before: 1,
        },
    ];
    assert_eq!(recording.transfers.as_ref(), expected);
    assert!(recording.attempts.iter().all(|row| row.granted));
    assert_eq!(check(&recording, Some(2)).ok(), Some(recording.used));
    assert!(matches!(
        check(&recording, None),
        Err(NativeBudgetTraceError::Decision)
    ));

    let mut transfers = recording.transfers.to_vec();
    transfers[2].fresh_before = 2;
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Decision)
    ));
    let mut transfers = recording.transfers.to_vec();
    transfers.pop();
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Decision)
    ));
    let mut transfers = recording.transfers.to_vec();
    transfers[2].kind = NativeStackTransferKind::Release { reserve: 1 };
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Transfer)
    ));
    let mut transfers = recording.transfers.to_vec();
    transfers.push(transfers[2].clone());
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Transfer)
    ));
    let mut transfers = recording.transfers.to_vec();
    transfers[1].fresh_before = 1;
    transfers[2].fresh_before = 0;
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Transfer)
    ));
    let mut transfers = recording.transfers.to_vec();
    if let NativeStackTransferKind::Reserve { produce_hash, .. } = &mut transfers[1].kind {
        *produce_hash = [10; 32];
    }
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Duplicate)
    ));
    let mut transfers = recording.transfers.to_vec();
    if let NativeStackTransferKind::Reserve { granted, .. } = &mut transfers[1].kind {
        *granted = true;
    }
    assert!(matches!(
        check(&with_transfers(&recording, transfers), Some(2)),
        Err(NativeBudgetTraceError::Decision)
    ));
}

#[test]
fn a_reused_stack_transfer_hash_leaves_the_recording_incomplete() {
    let budget = played(4);
    let _scope = budget.enter_comm_accounting_scope();
    drop(
        budget
            .prepare_authority_stack_transfer([10; 32], cells(1), &authority(1))
            .unwrap(),
    );
    drop(
        budget
            .prepare_authority_stack_transfer([10; 32], cells(1), &authority(1))
            .unwrap(),
    );
    assert!(budget.native_budget_recording().is_err());
}

#[test]
fn replay_takes_each_stack_transfer_decision_from_the_checked_trace() {
    let transfers: ReplayStackTransfers = Arc::from([
        (
            [10; 32],
            canonical_observation_authority(&authority(1)).unwrap(),
            1,
            true,
        ),
        (
            [11; 32],
            canonical_observation_authority(&authority(1)).unwrap(),
            2,
            false,
        ),
    ]);
    let budget = played(0);
    let session = budget.deploy_id();
    let _binding = replay_authority::ReplayAuthorityBinding::with_transfers(
        budget.clone(),
        session,
        transfers,
    )
    .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    budget
        .prepare_authority_stack_transfer([10; 32], cells(1), &authority(1))
        .expect("a recorded grant holds against a live allocation of zero")
        .commit();
    assert!(matches!(
        budget.prepare_authority_stack_transfer([11; 32], cells(2), &authority(1)),
        Err(InterpreterError::OutOfPhlogistonsError)
    ));
    assert!(matches!(
        budget.prepare_authority_stack_transfer([12; 32], cells(1), &authority(1)),
        Err(InterpreterError::ReduceError(_))
    ));
    assert!(matches!(
        budget.prepare_authority_stack_transfer([11; 32], cells(1), &authority(1)),
        Err(InterpreterError::ReduceError(_))
    ));
    assert_eq!(budget.authority_realized(), lane(1));
    let state = budget.authority_state.lock().unwrap();
    assert!(state
        .frontier
        .keys()
        .any(|identity| { *identity == authority::stack_transfer_event_id(&[11; 32], 1) }));
}

#[test]
fn the_wire_keeps_allocation_denials_and_stack_transfers() {
    let budget = played(2);
    let _scope = budget.enter_comm_accounting_scope();
    let first = budget
        .prepare_authority_stack_transfer([10; 32], cells(1), &authority(1))
        .unwrap();
    comm(&budget, 3).unwrap();
    assert!(comm(&budget, 4).is_err());
    drop(first);
    let recording = budget.native_budget_recording().unwrap().unwrap();
    assert_eq!(recording.attempts[1].cause, NativeDenialCause::Allocation);
    let encoded = encode_native_budget_recording(&recording, wire_limits()).unwrap();
    let decoded = decode_native_budget_recording(&encoded, wire_limits()).unwrap();
    assert_eq!(decoded.attempts, recording.attempts);
    assert_eq!(decoded.transfers, recording.transfers);
    assert_eq!(decoded.used, recording.used);
    assert_eq!(check(&decoded, Some(2)).ok(), Some(recording.used));

    let plain = RuntimeBudget::new(Cost::unsafe_max());
    plain
        .reset_for_native_execution(config(LIMIT, WEIGHTS))
        .unwrap();
    let _plain_scope = plain.enter_comm_accounting_scope();
    comm(&plain, 3).unwrap();
    let plain = plain.native_budget_recording().unwrap().unwrap();
    let encoded = encode_native_budget_recording(&plain, wire_limits()).unwrap();
    assert!(!encoded
        .windows(b"native-budget-recording:v3".len())
        .any(|window| window == b"native-budget-recording:v3"));
    assert!(encoded
        .windows(b"native-budget-recording:v2".len())
        .any(|window| window == b"native-budget-recording:v2"));
}
