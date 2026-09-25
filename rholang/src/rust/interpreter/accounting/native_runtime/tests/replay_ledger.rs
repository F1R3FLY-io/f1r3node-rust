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

fn replay_fixture(budget: HostWorkBudget) -> NativeOperationReplay {
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
    };
    let log = events(&rows);
    bind(&recording, rows.into(), log)
        .unwrap()
        .into_replay(budget)
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
            path: row.occurrence.path.to_vec().into(),
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
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
        }]),
        retries: Arc::from([]),
        used: 1,
    };
    let mut row = operation(0, 0, 1, NativeObservationLink::Attempt(0));
    row.footprint = footprint.into();
    let rows = Arc::from([row]);
    let log = events(&rows);
    bind(&recording, rows, log)
        .unwrap()
        .into_replay(host())
        .unwrap()
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
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
fn footprint_host_rejection_keeps_the_operation_unobserved() {
    let budget = host();
    let replay = replay_fixture(budget.clone());
    let mut ticket = reserve_slot(&replay, 0).unwrap();
    assert!(work(&budget, HostWorkDimension::SearchStateBytes, usize::MAX).is_err());
    assert!(matches!(
        ticket.authenticate_footprint(&[0u8], &[]),
        Err(NativeReplayError::Host(_))
    ));
    assert!(matches!(
        ticket.observe_introduction(&observation(
            0,
            AuthorityByteEventKind::ProduceIntroduction,
            1
        )),
        Err(NativeReplayError::Stage)
    ));
    drop(ticket);
    assert_eq!(replay.completed_usage(), 0);
    assert!(replay.checkpoint().is_ok());
}

#[test]
fn stage_checks_reject_omissions_duplicates_and_out_of_order_observations() {
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
    let replay = bind(&recording, rows, log)
        .unwrap()
        .into_replay(host())
        .unwrap();
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
    let replay = bind(&recording, rows, log)
        .unwrap()
        .into_replay(host())
        .unwrap();
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
fn observation_host_rejection_does_not_publish_usage() {
    let budget = host();
    let replay = replay_fixture(budget.clone());
    let mut ticket = reserve_slot(&replay, 0).unwrap();
    ticket.authenticate_footprint(&[0u8], &[]).unwrap();
    assert!(work(&budget, HostWorkDimension::VerificationBytes, usize::MAX).is_err());
    assert!(matches!(
        ticket.observe_introduction(&observation(
            0,
            AuthorityByteEventKind::ProduceIntroduction,
            1
        )),
        Err(NativeReplayError::Budget(NativeBudgetTraceError::Work(_)))
    ));
    drop(ticket);
    assert_eq!(replay.completed_usage(), 0);
    assert!(replay.checkpoint().is_ok());
}

#[test]
fn all_wrong_outcomes_and_denial_stages_release_without_consumption() {
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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
    };
    let replay = bind(&recording, Arc::from([]), Vec::new())
        .unwrap()
        .into_replay(host())
        .unwrap();
    replay.check_complete().unwrap();
    let root = replay.checkpoint().unwrap();
    replay.restore(&root).unwrap();
    replay.check_complete().unwrap();
}

#[test]
fn stale_foreign_and_abandoned_checkpoint_tokens_cannot_restore() {
    let replay = replay_fixture(host());
    let foreign = replay_fixture(host()).checkpoint().unwrap();
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
    let replay = replay_fixture(host());
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
    let replay = replay_fixture(host());
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

#[test]
fn prepaid_publication_and_rollback_survive_later_host_exhaustion() {
    let budget = host();
    let replay = replay_fixture(budget.clone());
    let root = replay.checkpoint().unwrap();
    let publication = authenticate(reserve_slot(&replay, 0).unwrap())
        .prepare_completion(NativeReplayOutcome::Stored)
        .unwrap();
    assert!(work(
        &budget,
        HostWorkDimension::VerificationOperations,
        usize::MAX
    )
    .is_err());
    publication.publish();
    replay.restore(&root).unwrap();
    assert!(replay.checkpoint().is_ok());
    assert!(matches!(
        replay.check_complete(),
        Err(NativeReplayError::Incomplete)
    ));
}

#[test]
fn preparation_host_rejection_releases_its_reservation() {
    let budget = host();
    let replay = replay_fixture(budget.clone());
    let ticket = authenticate(reserve_slot(&replay, 0).unwrap());
    assert!(work(
        &budget,
        HostWorkDimension::VerificationOperations,
        usize::MAX
    )
    .is_err());
    assert!(matches!(
        ticket.prepare_completion(NativeReplayOutcome::Stored),
        Err(NativeReplayError::Host(_))
    ));
    assert!(replay.checkpoint().is_ok());
    assert!(matches!(
        replay.check_complete(),
        Err(NativeReplayError::Incomplete)
    ));
}

#[test]
fn context_and_full_source_authentication_precede_reservation() {
    let budget = host();
    let replay = replay_fixture(budget.clone());
    let source = producer(&produce(0));
    assert!(matches!(
        replay.reserve_current(RSpaceOperationSource::Produce(&source)),
        Err(NativeReplayError::Context)
    ));
    for mutation in 0..5 {
        let mut source = source.clone();
        let mut order = OperationOrder {
            session: [0; 32],
            path: vec![(0, 0)].into(),
        };
        match mutation {
            0 => order.session[0] = 1,
            1 => order.path.push_back((0, 0)),
            2 => order.path = vec![(8, 0)].into(),
            3 => source.channel_hash.0[0] ^= 1,
            _ => source.persistent = !source.persistent,
        }
        let before = budget.usage(HostWorkDimension::VerificationBytes).get();
        let result = operation_context::scope(order, async {
            replay.reserve_current(RSpaceOperationSource::Produce(&source))
        })
        .now_or_never()
        .unwrap();
        assert!(matches!(
            result,
            Err(NativeReplayError::Context | NativeReplayError::Source)
        ));
        assert!(budget.usage(HostWorkDimension::VerificationBytes).get() >= before + 64);
    }
    publish_slot(&replay, 0);
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
        let replay = replay_fixture(host());
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
