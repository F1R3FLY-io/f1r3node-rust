use std::sync::Barrier;

use futures::FutureExt;
use rspace_plus_plus::rspace::operation_context;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;

use super::*;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetReplayDecision, NativeBudgetTraceError,
};

#[path = "replay_dependencies.rs"]
mod dependencies;

fn replay_fixture() -> NativeOperationReplay {
    let mut attempts = Vec::new();
    let mut rows = Vec::new();
    for index in 0..4 {
        let start = attempts.len();
        attempts.push(NativeBudgetAttempt {
            occurrence: occurrence(index, NativeAttemptStage::ProduceIntroduction),
            observation: observation(
                index as u8,
                AuthorityByteEventKind::ProduceIntroduction,
                if index == 2 { 101 } else { index + 1 },
            ),
            granted: index != 2,
            // Added by G3 (DR-135, S4).
            owner_refused: false,
            // Added by G3 (DR-135, S4 review).
            restorations: Arc::from([]),
        });
        let mut row = operation(
            index,
            start,
            start + 1,
            NativeObservationLink::Attempt(start),
        );
        if index == 1 || index == 3 {
            let source = produce(index as u8);
            row.comm = Some(NativeCommRecord {
                source: Arc::new(NativeCommSource {
                    consume: NativeConsumeSource {
                        channels: Arc::from([source.channel]),
                        hash: source.hash,
                        persistent: true,
                    },
                    produces: Arc::from([source.clone()]),
                    peeks: Arc::from([]),
                    repetitions: Arc::from([(source, 0)]),
                }),
                observation: NativeObservationLink::Attempt(attempts.len()),
            });
            attempts.push(NativeBudgetAttempt {
                occurrence: occurrence(index, NativeAttemptStage::Comm),
                observation: observation(
                    index as u8,
                    AuthorityByteEventKind::Comm,
                    if index == 3 { 101 } else { 2 },
                ),
                granted: index == 1,
                // Added by G3 (DR-135, S4).
                owner_refused: false,
                // Added by G3 (DR-135, S4 review).
                restorations: Arc::from([]),
            });
            row.budget_end += 1;
        }
        row.completion = match index {
            0 => RSpaceOperationCompletion::Stored,
            1 => RSpaceOperationCompletion::Matched,
            _ => RSpaceOperationCompletion::Rejected,
        };
        rows.push(row);
    }
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: attempts.into(),
        retries: Arc::from([]),
        used: 9,
        paths: fixture_paths(),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    };
    let log = events(&rows);
    bind(&recording, rows.into(), log)
        .unwrap()
        .into_replay()
        .unwrap()
}

fn reserve_slot(
    replay: &NativeOperationReplay,
    slot: usize,
) -> Result<NativeReplayReservation, NativeReplayError> {
    let row = replay.trace().operation(slot).unwrap();
    let source = match &row.source {
        NativeOperationSource::Produce(source) => IOEvent::Produce(producer(source)),
        NativeOperationSource::Consume(source) => IOEvent::Consume(consumer(source)),
    };
    operation_context::scope(
        OperationOrder {
            session: row.occurrence.session,
            path: CausalPath::from(fixture_paths().segments(row.occurrence.path)),
        },
        async {
            replay.reserve_current(match &source {
                IOEvent::Produce(source) => RSpaceOperationSource::Produce(source),
                IOEvent::Consume(source) => RSpaceOperationSource::Consume(source),
            })
        },
    )
    .now_or_never()
    .unwrap()
}

fn authenticate(mut ticket: NativeReplayReservation) -> NativeReplayReservation {
    let slot = ticket.slot();
    ticket.authenticate_footprint(&[slot as u8], &[]).unwrap();
    ticket
        .observe_introduction(&observation(
            slot as u8,
            AuthorityByteEventKind::ProduceIntroduction,
            if slot == 2 { 101 } else { slot as u64 + 1 },
        ))
        .unwrap();
    if slot == 1 || slot == 3 {
        let source = actual_comm(&ticket.operation().comm.as_ref().unwrap().source);
        ticket
            .observe_comm(
                &source,
                &observation(
                    slot as u8,
                    AuthorityByteEventKind::Comm,
                    if slot == 3 { 101 } else { 2 },
                ),
            )
            .unwrap();
    }
    ticket
}

fn actual_comm(source: &NativeCommSource) -> COMM {
    COMM {
        consume: consumer(&source.consume),
        produces: source.produces.iter().map(producer).collect(),
        peeks: source.peeks.iter().copied().collect(),
        times_repeated: source
            .repetitions
            .iter()
            .map(|(source, count)| (producer(source), *count))
            .collect(),
    }
}

fn publish_slot(replay: &NativeOperationReplay, slot: usize) {
    let ticket = authenticate(reserve_slot(replay, slot).unwrap());
    let expected = ticket.expected_outcome();
    ticket.prepare_completion(expected).unwrap().publish();
}

#[test]
fn every_slot_including_denials_must_complete_exactly_once() {
    let replay = replay_fixture();
    for slot in [3, 1, 2, 0] {
        assert!(matches!(
            replay.check_complete(),
            Err(NativeReplayError::Incomplete)
        ));
        publish_slot(&replay, slot);
        assert!(matches!(
            reserve_slot(&replay, slot),
            Err(NativeReplayError::Unavailable)
        ));
    }
    replay.check_complete().unwrap();
}

#[test]
fn completion_requires_actual_budget_observations() {
    let replay = replay_fixture();
    for slot in 0..4 {
        let mut ticket = reserve_slot(&replay, slot).unwrap();
        ticket.authenticate_footprint(&[slot as u8], &[]).unwrap();
        let expected = ticket.expected_outcome();
        assert!(
            ticket.prepare_completion(expected).is_err(),
            "unobserved stage published"
        );
        drop(reserve_slot(&replay, slot).unwrap());
    }
}

#[test]
fn completion_requires_the_actual_locked_footprint() {
    let replay = replay_fixture();
    let mut ticket = reserve_slot(&replay, 0).unwrap();
    let result = ticket.observe_introduction(&observation(
        0,
        AuthorityByteEventKind::ProduceIntroduction,
        1,
    ));
    let accepted = result.is_ok()
        && ticket
            .prepare_completion(NativeReplayOutcome::Stored)
            .is_ok();
    assert!(
        !accepted,
        "operation completed without authenticating the locked footprint"
    );
}

fn single_footprint_fixture(footprint: Vec<Arc<[u8]>>) -> NativeOperationReplay {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([NativeBudgetAttempt {
            occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
            observation: observation(0, AuthorityByteEventKind::ProduceIntroduction, 1),
            granted: true,
            // Added by G3 (DR-135, S4).
            owner_refused: false,
            // Added by G3 (DR-135, S4 review).
            restorations: Arc::from([]),
        }]),
        retries: Arc::from([]),
        used: 1,
        paths: fixture_paths(),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    };
    let mut row = operation(0, 0, 1, NativeObservationLink::Attempt(0));
    row.footprint = footprint.into();
    let rows = Arc::from([row]);
    let log = events(&rows);
    bind(&recording, rows, log).unwrap().into_replay().unwrap()
}

#[test]
fn footprint_rejects_omitted_join_extra_channel_and_duplicate_authentication() {
    let replay = single_footprint_fixture(vec![Arc::from([0u8]), Arc::from([1u8])]);
    let mut ticket = reserve_slot(&replay, 0).unwrap();
    for (channels, joins) in [
        (vec![0u8], vec![]),
        (vec![0u8], vec![vec![1, 2]]),
        (vec![1u8], vec![]),
    ] {
        assert!(matches!(
            ticket.authenticate_footprint(&channels, &joins),
            Err(NativeReplayError::Footprint)
        ));
    }
    ticket
        .authenticate_footprint(&[0u8], &[vec![1, 0, 1]])
        .unwrap();
    assert!(matches!(
        ticket.authenticate_footprint(&[0u8], &[vec![1]]),
        Err(NativeReplayError::Stage)
    ));
    ticket
        .observe_introduction(&observation(
            0,
            AuthorityByteEventKind::ProduceIntroduction,
            1,
        ))
        .unwrap();
    ticket
        .prepare_completion(NativeReplayOutcome::Stored)
        .unwrap()
        .publish();
    replay.check_complete().unwrap();
}

#[test]
fn forged_comm_with_authentic_budget_evidence_cannot_advance_or_charge() {
    let replay = replay_fixture();
    let expected = actual_comm(
        &replay
            .trace()
            .operation(1)
            .unwrap()
            .comm
            .as_ref()
            .unwrap()
            .source,
    );
    for mutation in 0..13 {
        let mut ticket = reserve_slot(&replay, 1).unwrap();
        ticket.authenticate_footprint(&[1u8], &[]).unwrap();
        ticket
            .observe_introduction(&observation(
                1,
                AuthorityByteEventKind::ProduceIntroduction,
                2,
            ))
            .unwrap();
        let mut changed = expected.clone();
        match mutation {
            0 => changed.consume.hash.0[0] ^= 1,
            1 => changed.consume.channel_hashes[0].0[0] ^= 1,
            2 => changed.consume.persistent = !changed.consume.persistent,
            3 => changed.produces[0].hash.0[0] ^= 1,
            4 => changed.produces[0].channel_hash.0[0] ^= 1,
            5 => changed.produces[0].persistent = !changed.produces[0].persistent,
            6 => {
                changed.peeks.insert(0);
            }
            7 => {
                changed.produces.clear();
            }
            8 => {
                *changed.times_repeated.values_mut().next().unwrap() += 1;
            }
            9 => {
                changed.times_repeated.clear();
            }
            10 => {
                let (mut source, count) = changed.times_repeated.pop_first().unwrap();
                source.channel_hash.0[0] ^= 1;
                changed.times_repeated.insert(source, count);
            }
            11 => changed.produces.push(changed.produces[0].clone()),
            _ => changed.consume.channel_hashes.clear(),
        }
        let observed = observation(1, AuthorityByteEventKind::Comm, 2);
        assert!(matches!(
            ticket.observe_comm(&changed, &observed),
            Err(NativeReplayError::CommSource)
        ));
        assert_eq!(replay.completed_usage(), 0);
        ticket.observe_comm(&expected, &observed).unwrap();
        drop(
            ticket
                .prepare_completion(NativeReplayOutcome::Matched)
                .unwrap(),
        );
    }
}

#[test]
fn logical_comm_authentication_does_not_use_post_dispatch_telemetry() {
    let replay = replay_fixture();
    let mut ticket = reserve_slot(&replay, 1).unwrap();
    ticket.authenticate_footprint(&[1u8], &[]).unwrap();
    ticket
        .observe_introduction(&observation(
            1,
            AuthorityByteEventKind::ProduceIntroduction,
            2,
        ))
        .unwrap();
    let mut source = actual_comm(&ticket.operation().comm.as_ref().unwrap().source);
    source.produces[0].output_value = vec![vec![7]];
    source.produces[0].is_deterministic = false;
    source.produces[0].failed = true;
    ticket
        .observe_comm(&source, &observation(1, AuthorityByteEventKind::Comm, 2))
        .unwrap();
    ticket
        .prepare_completion(NativeReplayOutcome::Matched)
        .unwrap()
        .publish();
    assert_eq!(replay.completed_usage(), 4);
}

#[test]
fn stage_checks_reject_omissions_duplicates_and_out_of_order_observations() {
    let replay = replay_fixture();
    for slot in 0..4 {
        let mut ticket = reserve_slot(&replay, slot).unwrap();
        ticket.authenticate_footprint(&[slot as u8], &[]).unwrap();
        let source = actual_comm(
            &replay
                .trace()
                .operation(if slot == 3 { 3 } else { 1 })
                .unwrap()
                .comm
                .as_ref()
                .unwrap()
                .source,
        );
        let intro = observation(
            slot as u8,
            AuthorityByteEventKind::ProduceIntroduction,
            if slot == 2 { 101 } else { slot as u64 + 1 },
        );
        let comm = observation(
            slot as u8,
            AuthorityByteEventKind::Comm,
            if slot == 3 { 101 } else { 2 },
        );
        assert!(matches!(
            ticket.observe_comm(&source, &comm),
            Err(NativeReplayError::Stage)
        ));
        ticket.observe_introduction(&intro).unwrap();
        assert!(matches!(
            ticket.observe_introduction(&intro),
            Err(NativeReplayError::Stage)
        ));
        if slot == 1 || slot == 3 {
            let outcome = ticket.expected_outcome();
            assert!(matches!(
                ticket.prepare_completion(outcome),
                Err(NativeReplayError::Stage)
            ));
            ticket = reserve_slot(&replay, slot).unwrap();
            ticket.authenticate_footprint(&[slot as u8], &[]).unwrap();
            ticket.observe_introduction(&intro).unwrap();
            ticket.observe_comm(&source, &comm).unwrap();
        }
        assert!(matches!(
            ticket.observe_comm(&source, &comm),
            Err(NativeReplayError::Stage)
        ));
        let outcome = ticket.expected_outcome();
        drop(ticket.prepare_completion(outcome).unwrap());
        assert_eq!(replay.completed_usage(), 0);
    }
}

#[test]
fn actual_observation_mutations_do_not_advance_the_stage() {
    let replay = replay_fixture();
    for mutation in 0..7 {
        let mut ticket = reserve_slot(&replay, 0).unwrap();
        ticket.authenticate_footprint(&[0u8], &[]).unwrap();
        let actual = observation(0, AuthorityByteEventKind::ProduceIntroduction, 1);
        let mut changed = actual.as_ref().clone();
        match mutation {
            0 => changed.event_id[0] ^= 1,
            1 => changed.kind = AuthorityByteEventKind::Comm,
            2 => changed.authority = authority(2),
            3 => changed.measurement = None,
            4 => changed.measurement.as_mut().unwrap().transfer_bytes += 1,
            5 => changed.measurement.as_mut().unwrap().trace_bytes += 1,
            _ => changed.legacy_amount = Some(0),
        }
        assert!(matches!(
            ticket.observe_introduction(&changed),
            Err(NativeReplayError::Budget(
                NativeBudgetTraceError::Observation
            ))
        ));
        ticket.observe_introduction(&actual).unwrap();
        drop(
            ticket
                .prepare_completion(NativeReplayOutcome::Stored)
                .unwrap(),
        );
        assert_eq!(replay.completed_usage(), 0);
    }
}

#[test]
fn retry_checks_the_original_observation_without_a_second_charge() {
    let (recording, rows, log) = trace_fixture();
    let replay = bind(&recording, rows, log).unwrap().into_replay().unwrap();
    let root = replay.checkpoint().unwrap();
    for _ in 0..2 {
        for slot in 0..3 {
            let mut ticket = reserve_slot(&replay, slot).unwrap();
            ticket
                .authenticate_footprint(&[if slot == 1 { 1u8 } else { 0u8 }], &[])
                .unwrap();
            let intro = &recording.attempts[if slot == 1 { 1 } else { 0 }].observation;
            let decision = ticket.observe_introduction(intro).unwrap();
            assert_eq!(decision, NativeBudgetReplayDecision::Accepted {
                usage: if slot == 2 { 0 } else { 1 }
            });
            if slot == 1 {
                let source = actual_comm(&ticket.operation().comm.as_ref().unwrap().source);
                ticket
                    .observe_comm(&source, &recording.attempts[2].observation)
                    .unwrap();
            }
            let outcome = ticket.expected_outcome();
            ticket.prepare_completion(outcome).unwrap().publish();
        }
        assert_eq!(replay.completed_usage(), 3);
        replay.check_complete().unwrap();
        replay.restore(&root).unwrap();
        assert_eq!(replay.completed_usage(), 0);
    }
}

#[test]
fn direct_retry_reservation_cannot_bypass_its_channel_predecessor() {
    let (recording, rows, log) = trace_fixture();
    let replay = bind(&recording, rows, log).unwrap().into_replay().unwrap();
    let root = replay.checkpoint().unwrap();
    for _ in 0..2 {
        assert!(
            reserve_slot(&replay, 2).is_err(),
            "retry reserved before its channel predecessor completed"
        );
        let mut owner = reserve_slot(&replay, 0).unwrap();
        owner.authenticate_footprint(&[0_u8], &[]).unwrap();
        owner
            .observe_introduction(&recording.attempts[0].observation)
            .unwrap();
        assert!(
            reserve_slot(&replay, 2).is_err(),
            "unpublished predecessor authorized its retry"
        );
        owner
            .prepare_completion(NativeReplayOutcome::Stored)
            .unwrap()
            .publish();
        let mut retry = reserve_slot(&replay, 2).unwrap();
        retry.authenticate_footprint(&[0_u8], &[]).unwrap();
        assert_eq!(
            retry
                .observe_introduction(&recording.attempts[0].observation)
                .unwrap(),
            NativeBudgetReplayDecision::Accepted { usage: 0 }
        );
        retry
            .prepare_completion(NativeReplayOutcome::Stored)
            .unwrap()
            .publish();
        assert_eq!(replay.completed_usage(), 1);
        replay.restore(&root).unwrap();
    }
}

#[test]
fn all_wrong_outcomes_and_denial_stages_release_without_consumption() {
    let replay = replay_fixture();
    let outcomes = [
        NativeReplayOutcome::Stored,
        NativeReplayOutcome::Matched,
        NativeReplayOutcome::DeniedIntroduction,
        NativeReplayOutcome::DeniedComm,
    ];
    for slot in 0..4 {
        for outcome in outcomes {
            let ticket = authenticate(reserve_slot(&replay, slot).unwrap());
            assert_eq!(ticket.expected_outcome(), outcomes[slot]);
            if outcome == outcomes[slot] {
                drop(ticket.prepare_completion(outcome).unwrap());
            } else {
                assert!(matches!(
                    ticket.prepare_completion(outcome),
                    Err(NativeReplayError::Outcome)
                ));
            }
        }
        drop(reserve_slot(&replay, slot).unwrap());
    }
    assert!(matches!(
        replay.check_complete(),
        Err(NativeReplayError::Incomplete)
    ));
}

#[test]
fn checkpoint_and_restore_require_an_exclusive_quiescent_boundary() {
    let replay = replay_fixture();
    let root = replay.checkpoint().unwrap();
    let ticket = reserve_slot(&replay, 0).unwrap();
    assert!(matches!(replay.checkpoint(), Err(NativeReplayError::Busy)));
    assert!(matches!(
        replay.restore(&root),
        Err(NativeReplayError::Busy)
    ));
    assert!(matches!(replay.close(), Err(NativeReplayError::Busy)));
    drop(ticket);
    let boundary = replay.begin_boundary().unwrap();
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Busy)
    ));
    assert!(matches!(
        replay.begin_boundary(),
        Err(NativeReplayError::Busy)
    ));
    assert!(matches!(
        replay.check_complete(),
        Err(NativeReplayError::Busy)
    ));
    let restored = boundary.prepare_restore(&root).unwrap();
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Busy)
    ));
    restored.publish();
    publish_slot(&replay, 0);
}

#[test]
fn dropping_a_prepared_restore_keeps_the_original_state_and_releases_the_boundary() {
    let replay = replay_fixture();
    let root = replay.checkpoint().unwrap();
    publish_slot(&replay, 0);
    let prepared = replay
        .begin_boundary()
        .unwrap()
        .prepare_restore(&root)
        .unwrap();
    drop(prepared);
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Unavailable)
    ));
    publish_slot(&replay, 1);
    replay.restore(&root).unwrap();
    publish_slot(&replay, 0);
}

#[test]
fn empty_replay_is_complete_and_supports_root_checkpoint() {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([]),
        retries: Arc::from([]),
        used: 0,
        paths: fixture_paths(),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    };
    let replay = bind(&recording, Arc::from([]), Vec::new())
        .unwrap()
        .into_replay()
        .unwrap();
    replay.check_complete().unwrap();
    let root = replay.checkpoint().unwrap();
    replay.restore(&root).unwrap();
    replay.check_complete().unwrap();
}

#[test]
fn stale_foreign_and_abandoned_checkpoint_tokens_cannot_restore() {
    let replay = replay_fixture();
    let foreign = replay_fixture().checkpoint().unwrap();
    let root = replay.checkpoint().unwrap();
    publish_slot(&replay, 0);
    let abandoned = replay.checkpoint().unwrap();
    publish_slot(&replay, 1);
    assert!(matches!(
        replay.restore(&foreign),
        Err(NativeReplayError::Checkpoint)
    ));
    assert!(matches!(
        reserve_slot(&replay, 1),
        Err(NativeReplayError::Unavailable)
    ));
    replay.restore(&root).unwrap();
    assert!(matches!(
        replay.restore(&abandoned),
        Err(NativeReplayError::Checkpoint)
    ));
    publish_slot(&replay, 0);
    assert!(matches!(
        replay.restore(&abandoned),
        Err(NativeReplayError::Checkpoint)
    ));
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Unavailable)
    ));
    replay.close().unwrap();
    assert!(matches!(
        replay.restore(&root),
        Err(NativeReplayError::Closed)
    ));
    assert!(matches!(
        reserve_slot(&replay, 1),
        Err(NativeReplayError::Closed)
    ));
}

#[test]
fn independent_publications_can_finish_in_reverse_reservation_order() {
    let replay = replay_fixture();
    let root = replay.checkpoint().unwrap();
    let first = authenticate(reserve_slot(&replay, 0).unwrap())
        .prepare_completion(NativeReplayOutcome::Stored)
        .unwrap();
    let second = authenticate(reserve_slot(&replay, 1).unwrap())
        .prepare_completion(NativeReplayOutcome::Matched)
        .unwrap();
    std::thread::scope(|scope| {
        scope.spawn(move || second.publish()).join().unwrap();
    });
    first.publish();
    let prefix = replay.checkpoint().unwrap();
    publish_slot(&replay, 2);
    replay.restore(&prefix).unwrap();
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Unavailable)
    ));
    assert!(matches!(
        reserve_slot(&replay, 1),
        Err(NativeReplayError::Unavailable)
    ));
    drop(reserve_slot(&replay, 2).unwrap());
    replay.restore(&root).unwrap();
    for slot in 0..4 {
        publish_slot(&replay, slot);
    }
    replay.check_complete().unwrap();
}

#[test]
fn concurrent_duplicate_reservations_have_one_owner() {
    let replay = replay_fixture();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    let ticket = reserve_slot(&replay, 0);
                    barrier.wait();
                    match ticket {
                        Ok(ticket) => {
                            authenticate(ticket)
                                .prepare_completion(NativeReplayOutcome::Stored)
                                .unwrap()
                                .publish();
                            true
                        }
                        Err(NativeReplayError::Unavailable) => false,
                        Err(other) => panic!("{other}"),
                    }
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| usize::from(worker.join().unwrap()))
                .sum::<usize>(),
            1
        );
    });
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Unavailable)
    ));
}

/// Changed by DR-131: the check that each authentication paid its bytes went
/// with host work. The test keeps its check that authentication precedes the
/// reservation.
#[test]
// Changed by DR-132 (D1-A, T11): native replay finds each row by its recorded
// source and reads no operation name. A missing or changed name is no longer
// an error (FF-3), so the test keeps only the source mutations.
// fn context_and_full_source_authentication_precede_reservation() {
//     let replay = replay_fixture();
//     let source = producer(&produce(0));
//     assert!(matches!(
//         replay.reserve_current(RSpaceOperationSource::Produce(&source)),
//         Err(NativeReplayError::Context)
//     ));
//     for mutation in 0..5 {
//         let mut source = source.clone();
//         let mut order = OperationOrder {
//             session: [0; 32],
//             path: vec![(0, 0)].into(),
//         };
//         match mutation {
//             0 => order.session[0] = 1,
//             1 => order.path.push_back((0, 0)),
//             2 => order.path = vec![(8, 0)].into(),
//             3 => source.channel_hash.0[0] ^= 1,
//             _ => source.persistent = !source.persistent,
//         }
//         let result = operation_context::scope(order, async {
//             replay.reserve_current(RSpaceOperationSource::Produce(&source))
//         })
//         .now_or_never()
//         .unwrap();
//         assert!(matches!(
//             result,
//             Err(NativeReplayError::Context | NativeReplayError::Source)
//         ));
//     }
//     publish_slot(&replay, 0);
// }
fn full_source_authentication_precedes_reservation() {
    let replay = replay_fixture();
    let source = producer(&produce(0));
    for mutation in 0..2 {
        let mut source = source.clone();
        match mutation {
            0 => source.channel_hash.0[0] ^= 1,
            _ => source.persistent = !source.persistent,
        }
        assert!(matches!(
            replay.reserve_current(RSpaceOperationSource::Produce(&source)),
            Err(NativeReplayError::Source)
        ));
    }
    publish_slot(&replay, 0);
}

/// Added by DR-132 (FF-3, failing first): with no outer operation name, native
/// replay finds each recorded row by its RSpace source. Racing evaluation sets
/// no name, so the source is the only key. Before DR-132 the replay needed the
/// name of the deterministic scheduler and failed with `Context`.
#[test]
fn native_replay_finds_each_row_by_its_source() {
    let replay = replay_fixture();
    for slot in 0..4 {
        let row = replay.trace().operation(slot).expect("the fixture row");
        let NativeOperationSource::Produce(recorded) = &row.source else {
            panic!("every fixture row is a produce");
        };
        let source = producer(recorded);
        let ticket = replay
            .reserve_current(RSpaceOperationSource::Produce(&source))
            .expect("the replay finds the row by its source");
        assert_eq!(ticket.slot(), slot);
        let ticket = authenticate(ticket);
        let expected = ticket.expected_outcome();
        ticket
            .prepare_completion(expected)
            .expect("the recorded outcome completes")
            .publish();
    }
    replay.check_complete().expect("every row completes");
}

/// Added by DR-132 (P-2): the number of sources of the generated journals.
/// Each source has its own channel.
const P2_SOURCES: u8 = 6;

/// Added by DR-132 (P-2): a journal in which row `i` has the source and the
/// channel `sources[i]`. Each row lists the row before it on its channel as
/// its predecessor, so the rows of one source form a chain among unrelated
/// rows.
fn source_chain_fixture(sources: &[u8]) -> (NativeOperationReplay, NativeBudgetRecording) {
    let attempts: Vec<_> = (0..sources.len())
        .map(|id| NativeBudgetAttempt {
            occurrence: occurrence(id as u64, NativeAttemptStage::ProduceIntroduction),
            observation: observation(id as u8, AuthorityByteEventKind::ProduceIntroduction, 1),
            granted: true,
            // Added by G3 (DR-135, S4).
            owner_refused: false,
            // Added by G3 (DR-135, S4 review).
            restorations: Arc::from([]),
        })
        .collect();
    let mut latest = [None; P2_SOURCES as usize];
    let rows: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(id, source)| {
            let mut row = operation(id as u64, id, id + 1, NativeObservationLink::Attempt(id));
            row.source = NativeOperationSource::Produce(produce(*source));
            row.footprint = Arc::from([Arc::from([*source])]);
            row.predecessors = latest[usize::from(*source)]
                .into_iter()
                .collect::<Vec<_>>()
                .into();
            latest[usize::from(*source)] = Some(id);
            row
        })
        .collect();
    let log = events(&rows);
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: attempts.into(),
        retries: Arc::from([]),
        used: sources.len() as u64,
        paths: fixture_paths(),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    };
    let replay = bind(&recording, rows.into(), log)
        .expect("the generated journal binds")
        .into_replay()
        .expect("the generated journal replays");
    (replay, recording)
}

/// Added by DR-132 (P-2): one live call of `source` with no outer name.
fn reserve_source(
    replay: &NativeOperationReplay,
    source: u8,
) -> Result<NativeReplayReservation, NativeReplayError> {
    let live = producer(&produce(source));
    replay.reserve_current(RSpaceOperationSource::Produce(&live))
}

/// Added by DR-132 (P-2): completes a reservation of `source` with the
/// recorded introduction of its row.
fn complete_source(
    mut ticket: NativeReplayReservation,
    source: u8,
    recording: &NativeBudgetRecording,
) {
    let slot = ticket.slot();
    ticket
        .authenticate_footprint(&[source], &[])
        .expect("the footprint of the source");
    ticket
        .observe_introduction(&recording.attempts[slot].observation)
        .expect("the recorded introduction");
    ticket
        .prepare_completion(NativeReplayOutcome::Stored)
        .expect("the recorded outcome")
        .publish();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Added by DR-132 (P-2): native replay binds each live occurrence of a
    /// source to the first open row of that source, in journal order. The
    /// generated journals hold chains of one source among unrelated rows.
    /// Each case replays them in a random interleaving that keeps the order
    /// of each channel, and it races two live calls of one source for two
    /// rows. The k-th completed occurrence of a source takes its k-th row,
    /// and no call meets `Unavailable`. It replaces the test of the removed
    /// digest index of paths (`replay_digest_lookup_equals_binary_search`).
    #[test]
    fn replay_binds_each_source_occurrence_to_the_first_open_row(
        sources in prop::collection::vec(0_u8..P2_SOURCES, 1..41),
        picks in prop::collection::vec(any::<prop::sample::Index>(), 0..80),
        race in any::<prop::sample::Index>(),
    ) {
        let (replay, recording) = source_chain_fixture(&sources);
        let mut rows: Vec<std::collections::VecDeque<usize>> =
            vec![std::collections::VecDeque::new(); usize::from(P2_SOURCES)];
        for (slot, source) in sources.iter().enumerate() {
            rows[usize::from(*source)].push_back(slot);
        }
        let with_rows = |rows: &Vec<std::collections::VecDeque<usize>>, least: usize| -> Vec<u8> {
            (0..P2_SOURCES)
                .filter(|source| rows[usize::from(*source)].len() >= least)
                .collect()
        };
        let racing = with_rows(&rows, 2);
        if !racing.is_empty() {
            let source = racing[race.index(racing.len())];
            let barrier = Barrier::new(2);
            let results: Vec<_> = std::thread::scope(|scope| {
                let workers: Vec<_> = (0..2)
                    .map(|_| scope.spawn(|| {
                        barrier.wait();
                        reserve_source(&replay, source)
                    }))
                    .collect();
                workers.into_iter().map(|worker| worker.join().expect("the racing call joins")).collect()
            });
            let first = rows[usize::from(source)][0];
            let mut winners = 0;
            for result in results {
                match result {
                    Ok(ticket) => {
                        prop_assert_eq!(ticket.slot(), first);
                        complete_source(ticket, source, &recording);
                        winners += 1;
                    }
                    Err(error) => prop_assert!(
                        matches!(error, NativeReplayError::Dependency),
                        "the losing call met {:?}", error
                    ),
                }
            }
            prop_assert_eq!(winners, 1);
            rows[usize::from(source)].pop_front();
        }
        let mut picks = picks.into_iter();
        loop {
            let open = with_rows(&rows, 1);
            if open.is_empty() {
                break;
            }
            let source = open[picks.next().map_or(0, |pick| pick.index(open.len()))];
            let expected = rows[usize::from(source)].pop_front().expect("an open row");
            let ticket = reserve_source(&replay, source);
            prop_assert!(ticket.is_ok(), "source {} met {:?}", source, ticket.as_ref().err());
            let ticket = ticket.expect("checked above");
            prop_assert_eq!(ticket.slot(), expected);
            complete_source(ticket, source, &recording);
        }
        prop_assert!(replay.check_complete().is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn actual_footprint_matches_the_canonical_union_of_channels_and_joins(
        channels in prop::collection::vec(0u8..16, 0..16),
        joins in prop::collection::vec(prop::collection::vec(0u8..16, 0..8), 0..8),
    ) {
        let unique: std::collections::BTreeSet<_> = channels.iter().chain(joins.iter().flatten()).copied().collect();
        let expected = unique.into_iter().map(|channel| Arc::from([channel])).collect();
        let replay = single_footprint_fixture(expected);
        let mut ticket = reserve_slot(&replay, 0).unwrap();
        ticket.authenticate_footprint(&channels, &joins).unwrap();
        ticket.observe_introduction(&observation(0, AuthorityByteEventKind::ProduceIntroduction, 1)).unwrap();
        ticket.prepare_completion(NativeReplayOutcome::Stored).unwrap().publish();
        replay.check_complete().unwrap();
    }

    #[test]
    fn arbitrary_completion_drop_and_rollback_sequences_preserve_the_exact_prefix(
        commands in prop::collection::vec((0u8..5, 0usize..4), 0..100),
    ) {
        let replay = replay_fixture();
        let root = replay.checkpoint().unwrap();
        let mut completed = Vec::<(usize, usize)>::new();
        let mut serial = 0;
        let mut saved = vec![(root, Vec::<(usize, usize)>::new())];
        for (command, slot) in commands {
            match command {
                0 if !completed.iter().any(|entry| entry.0 == slot) => {
                    publish_slot(&replay, slot);
                    completed.push((slot, serial));
                    serial += 1;
                }
                1 if !completed.iter().any(|entry| entry.0 == slot) => { drop(reserve_slot(&replay, slot).unwrap()); }
                2 => { saved.push((replay.checkpoint().unwrap(), completed.clone())); }
                3 => {
                    let (token, snapshot) = &saved[slot % saved.len()];
                    let valid = completed.starts_with(snapshot);
                    prop_assert_eq!(replay.restore(token).is_ok(), valid);
                    if valid { completed = snapshot.clone(); }
                }
                _ => (),
            }
            for check_slot in 0..4 {
                let ticket = reserve_slot(&replay, check_slot);
                prop_assert_eq!(ticket.is_ok(), !completed.iter().any(|entry| entry.0 == check_slot));
                drop(ticket);
            }
            prop_assert_eq!(replay.check_complete().is_ok(), completed.len() == 4);
            let expected_usage: u64 = completed.iter().map(|entry| [1, 4, 0, 4][entry.0]).sum();
            prop_assert_eq!(replay.completed_usage(), expected_usage);
        }
    }
}

/// Added by DR-132 (D2-B, review m5): an aborted task that holds a
/// reservation cancels it, and its live token drops after the reservation,
/// because the counted wrapper binds the token first. The counts stay exact,
/// no stall is declared, and the slot opens again.
#[tokio::test]
async fn an_aborted_task_releases_its_reservation_before_its_count() {
    use crate::rust::interpreter::execution_space::EvaluationProgress;
    use crate::rust::interpreter::reduce::{spawn_counted, with_counted_evaluation};

    let replay = replay_fixture();
    let progress: Arc<dyn EvaluationProgress> = Arc::new(replay.clone());
    let observed = replay.clone();
    let ((), errors) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        with_counted_evaluation(progress, async move {
            let (reserved, held) = tokio::sync::oneshot::channel::<()>();
            let holder = replay.clone();
            let abort = spawn_counted(async move {
                let ticket = reserve_slot(&holder, 0).expect("slot 0 reserves");
                reserved
                    .send(())
                    .expect("the root waits for the reservation");
                std::future::pending::<()>().await;
                drop(ticket);
                Ok(())
            });
            held.await.expect("the task reserved slot 0");
            assert_eq!(replay.progress_counts(), (2, 0));
            assert!(matches!(
                reserve_slot(&replay, 0),
                Err(NativeReplayError::Unavailable)
            ));
            abort.abort();
            let mut polls = 0;
            while replay.progress_counts().0 != 1 {
                polls += 1;
                assert!(polls < 1_000, "the aborted task never finished");
                tokio::task::yield_now().await;
            }
            assert_eq!(replay.progress_counts(), (1, 0));
            drop(reserve_slot(&replay, 0).expect("the slot is open again"));
        }),
    )
    .await
    .expect("the evaluation ends")
    .expect("the evaluation begins");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        format!("{errors:?}").contains("task cancelled"),
        "{errors:?}"
    );
    assert!(!observed.progress_stalled());
    assert!(observed.progress_idle());
}
