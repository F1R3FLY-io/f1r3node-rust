use proptest::prelude::*;

use super::*;

fn integer(value: i64) -> Par {
    Par {
        exprs: vec![models::rhoapi::Expr {
            expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(value)),
        }],
        ..Default::default()
    }
}

async fn joined_replay(
    arity: usize,
    produce: bool,
    persistence: u8,
    peek_mask: u8,
    outcome: NativeReplayOutcome,
    reverse_seed: bool,
) {
    let channels: Vec<_> = (0..arity).map(|index| integer(index as i64)).collect();
    let auth = authority(3);
    let data: Vec<_> = (0..arity)
        .map(|index| ListParWithRandom {
            pars: vec![integer(100 + index as i64)],
            random_state: vec![index as u8; 17],
            cost_authority: Some(auth.clone()),
            ..Default::default()
        })
        .collect();
    let patterns = vec![
        BindPattern {
            patterns: vec![models::rust::utils::new_freevar_par(0, Vec::new())],
            free_count: 1,
            remainder: None,
        };
        arity
    ];
    let continuation = TaggedContinuation {
        cost_authority: Some(auth.clone()),
        ..Default::default()
    };
    let peeks: BTreeSet<_> = (0..arity)
        .filter(|index| peek_mask & (1 << index) != 0)
        .map(|index| index as i32)
        .collect();
    let persistent = |index: usize| persistence & (1_u8 << index) != 0;
    let continuation_persistent = persistence & (1 << arity) != 0;
    let trigger = arity - 1;
    let source = Produce::create(&channels[trigger], &data[trigger], persistent(trigger));
    let consumer = Consume::create(&channels, &patterns, &continuation, continuation_persistent);
    let introduction = if produce {
        observation_construction::produce_introduction(
            &source,
            &channels[trigger],
            &data[trigger],
            &auth,
        )
    } else {
        observation_construction::consume_introduction(
            &consumer,
            &channels,
            &patterns,
            &continuation,
            &auth,
        )
    }
    .unwrap();
    let limit = match outcome {
        NativeReplayOutcome::DeniedIntroduction => 0,
        NativeReplayOutcome::DeniedComm => introduction.measurement.introduction_bytes,
        _ => u64::MAX,
    };
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let mut seed: Vec<_> = (0..arity).collect();
    if reverse_seed {
        seed.reverse();
    }
    for index in seed {
        if index != trigger || (!produce && outcome != NativeReplayOutcome::Stored) {
            assert!(play
                .produce(
                    channels[index].clone(),
                    data[index].clone(),
                    persistent(index)
                )
                .await
                .unwrap()
                .is_none());
        }
    }
    if produce && outcome != NativeReplayOutcome::Stored {
        assert!(play
            .consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                continuation_persistent,
                peeks.clone()
            )
            .await
            .unwrap()
            .is_none());
    }
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(limit, [0, 1, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    budget
        .register_introduction_authority(introduction.event_id, introduction.kind, &auth)
        .unwrap();
    play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
    let order = || OperationOrder {
        session: budget.deploy_id(),
        path: vec![(0, 0)].into(),
    };
    let played = operation_context::scope(order(), async {
        if produce {
            play.produce(
                channels[trigger].clone(),
                data[trigger].clone(),
                persistent(trigger),
            )
            .await
            .map(|result| result.map(|(continuation, data, _)| (continuation, data)))
        } else {
            play.consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                continuation_persistent,
                peeks.clone(),
            )
            .await
        }
    })
    .await;
    match outcome {
        NativeReplayOutcome::Stored => assert!(played.as_ref().unwrap().is_none()),
        NativeReplayOutcome::Matched => {
            assert_eq!(played.as_ref().unwrap().as_ref().unwrap().1.len(), arity)
        }
        _ => assert_eq!(played, Err(RSpaceError::OutOfPhlogistons)),
    }
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let rows = budget.native_operation_recording().unwrap().unwrap();
    let log = play.create_soft_checkpoint().await.log;
    let host = config(limit, [0, 1, 1, 0]).host_work;
    let trace = with_contract(limit, [0, 1, 1, 0], |contract| {
        contract
            .check_operation_journal(recording.session, &recording, rows, journal_limits(), &host)
            .unwrap()
    })
    .bind_trace(
        log.into(),
        NativeOperationTraceLimits {
            events: 10,
            source_entries: 100,
            source_bytes: 100_000,
            telemetry_items: 100,
            telemetry_bytes: 100_000,
        },
        &host,
    )
    .unwrap();
    let replay = trace
        .into_session(history, Arc::new(Box::new(Matcher)), host)
        .unwrap();
    let mut checkpoint = Some(replay.checkpoint().await.unwrap());
    if !produce {
        let mut changed_peeks = peeks.clone();
        if !changed_peeks.remove(&0) {
            changed_peeks.insert(0);
        }
        let changed = operation_context::scope(
            order(),
            replay.consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                continuation_persistent,
                changed_peeks,
                &auth,
            ),
        )
        .await;
        assert!(
            changed.is_err(),
            "a changed peek set must not complete the recorded consume: {changed:?}"
        );
        assert!(replay.check_complete().await.is_err());
    }
    for round in 0..2 {
        let replayed = operation_context::scope(order(), async {
            if produce {
                replay
                    .produce(
                        channels[trigger].clone(),
                        data[trigger].clone(),
                        persistent(trigger),
                        &auth,
                    )
                    .await
                    .map(|result| result.map(|(continuation, data, _)| (continuation, data)))
            } else {
                replay
                    .consume(
                        channels.clone(),
                        patterns.clone(),
                        continuation.clone(),
                        continuation_persistent,
                        peeks.clone(),
                        &auth,
                    )
                    .await
            }
        })
        .await;
        assert_eq!(replayed, played, "arity={arity}, produce={produce}, persistence={persistence}, peeks={peek_mask}, outcome={outcome:?}");
        replay.check_complete().await.unwrap();
        for channel in &channels {
            assert_eq!(
                replay.get_data(channel).await.unwrap(),
                play.get_data(channel).await
            );
        }
        assert_eq!(
            replay.get_continuations(&channels).await.unwrap(),
            play.get_store().get_continuations(&channels)
        );
        if round == 0 {
            replay.restore(checkpoint.take().unwrap()).await.unwrap();
            assert!(replay.check_complete().await.is_err());
        }
    }
}

#[tokio::test]
async fn joined_native_replay_preserves_peeks_persistence_denials_and_restoration() {
    for produce in [false, true] {
        for peeks in [0, 1, 2, 3] {
            for persistence in 0..8 {
                for outcome in [
                    NativeReplayOutcome::Stored,
                    NativeReplayOutcome::Matched,
                    NativeReplayOutcome::DeniedIntroduction,
                    NativeReplayOutcome::DeniedComm,
                ] {
                    joined_replay(2, produce, persistence, peeks, outcome, false).await;
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_joined_replay_matches_play(
        arity in 2usize..5,
        produce in any::<bool>(),
        persistence in any::<u8>(),
        peeks in any::<u8>(),
        outcome in 0u8..4,
        reverse_seed in any::<bool>(),
    ) {
        let outcome = [NativeReplayOutcome::Stored, NativeReplayOutcome::Matched, NativeReplayOutcome::DeniedIntroduction, NativeReplayOutcome::DeniedComm][usize::from(outcome)];
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
            .block_on(joined_replay(arity, produce, persistence, peeks, outcome, reverse_seed));
    }
}
