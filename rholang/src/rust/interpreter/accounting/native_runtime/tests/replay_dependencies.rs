use super::*;

#[test]
fn retry_waits_for_its_exact_accepted_owner_while_independent_work_proceeds() {
    let accepted = observation(2, AuthorityByteEventKind::ProduceIntroduction, 1);
    let attempts = vec![
        NativeBudgetAttempt {
            occurrence: occurrence(2, NativeAttemptStage::ProduceIntroduction),
            observation: accepted.clone(),
            granted: true,
        },
        NativeBudgetAttempt {
            occurrence: occurrence(1, NativeAttemptStage::ProduceIntroduction),
            observation: observation(1, AuthorityByteEventKind::ProduceIntroduction, 2),
            granted: true,
        },
    ];
    let retry = NativeBudgetRetry {
        occurrence: occurrence(0, NativeAttemptStage::ProduceIntroduction),
        observation: accepted,
        accepted_attempt: 0,
        fresh_before: 2,
    };
    let mut rows = vec![
        operation(2, 0, 1, NativeObservationLink::Attempt(0)),
        operation(1, 1, 2, NativeObservationLink::Attempt(1)),
        operation(0, 2, 2, NativeObservationLink::Retry(0)),
    ];
    rows[2].source = rows[0].source.clone();
    let log = events(&rows);
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: attempts.into(),
        retries: Arc::from([retry]),
        used: 3,
    };
    let replay = bind(&recording, rows.into(), log)
        .unwrap()
        .into_replay(host())
        .unwrap();
    assert_eq!(replay.trace().journal_index(0), Some(2));
    assert_eq!(replay.trace().journal_index(2), Some(0));
    let root = replay.checkpoint().unwrap();
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Dependency)
    ));
    drop(reserve_slot(&replay, 2).unwrap());
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Dependency)
    ));
    let mut independent = reserve_slot(&replay, 1).unwrap();
    independent.authenticate_footprint(&[1u8], &[]).unwrap();
    independent
        .observe_introduction(&observation(
            1,
            AuthorityByteEventKind::ProduceIntroduction,
            2,
        ))
        .unwrap();
    independent
        .prepare_completion(NativeReplayOutcome::Stored)
        .unwrap()
        .publish();
    assert_eq!(replay.completed_usage(), 2);
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Dependency)
    ));
    let mut owner = reserve_slot(&replay, 2).unwrap();
    owner.authenticate_footprint(&[2u8], &[]).unwrap();
    owner
        .observe_introduction(&observation(
            2,
            AuthorityByteEventKind::ProduceIntroduction,
            1,
        ))
        .unwrap();
    owner
        .prepare_completion(NativeReplayOutcome::Stored)
        .unwrap()
        .publish();
    drop(reserve_slot(&replay, 0).unwrap());
    replay.restore(&root).unwrap();
    assert!(matches!(
        reserve_slot(&replay, 0),
        Err(NativeReplayError::Dependency)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn direct_reservations_follow_only_recorded_channel_dependencies(
        channels in prop::collection::vec(0_u8..8, 1..33),
        commands in prop::collection::vec((any::<usize>(), any::<bool>()), 0..129),
    ) {
        let attempts: Vec<_> = channels.iter().enumerate().map(|(id, _)| NativeBudgetAttempt {
            occurrence: occurrence(id as u64, NativeAttemptStage::ProduceIntroduction),
            observation: observation(id as u8, AuthorityByteEventKind::ProduceIntroduction, 1),
            granted: true,
        }).collect();
        let mut latest = [None; 8];
        let rows: Vec<_> = channels.iter().enumerate().map(|(id, channel)| {
            let mut row = operation(id as u64, id, id + 1, NativeObservationLink::Attempt(id));
            row.footprint = Arc::from([Arc::from([*channel])]);
            row.predecessors = latest[usize::from(*channel)].into_iter().collect::<Vec<_>>().into();
            latest[usize::from(*channel)] = Some(id);
            row
        }).collect();
        let log = events(&rows);
        let recording = NativeBudgetRecording {
            session: [0; 32], attempts: attempts.into(), retries: Arc::from([]), used: channels.len() as u64,
        };
        let replay = bind(&recording, rows.into(), log).unwrap().into_replay(host()).unwrap();
        let root = replay.checkpoint().unwrap();
        for _ in 0..2 {
            let mut completed = vec![false; channels.len()];
            for (index, publish) in commands.iter().copied().chain((0..channels.len()).map(|index| (index, true))) {
                let index = index % channels.len();
                let ready = !completed[index] && replay.trace().operation(index).unwrap().predecessors.iter().all(|prior| completed[*prior]);
                let actual = reserve_slot(&replay, index);
                prop_assert_eq!(actual.is_ok(), ready);
                if let Ok(mut ticket) = actual {
                    ticket.authenticate_footprint(&[channels[index]], &[]).unwrap();
                    ticket.observe_introduction(&recording.attempts[index].observation).unwrap();
                    if publish {
                        ticket.prepare_completion(NativeReplayOutcome::Stored).unwrap().publish();
                        completed[index] = true;
                    }
                }
                prop_assert_eq!(replay.completed_usage(), completed.iter().filter(|value| **value).count() as u64);
            }
            replay.check_complete().unwrap();
            replay.restore(&root).unwrap();
        }
    }
}
