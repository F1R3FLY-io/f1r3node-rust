use super::*;

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
