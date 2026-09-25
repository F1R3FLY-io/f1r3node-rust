use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::native_runtime::tests::{
    authority, config, journal_limits, with_contract,
};
use crate::rust::interpreter::accounting::native_runtime::NativeOperationTraceLimits;
use crate::rust::interpreter::accounting::{observation_construction, RuntimeBudget};
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::rho_runtime::test_accounting_observer;

mod histories;
mod joins;

#[tokio::test]
async fn live_observer_preserves_host_rejection_without_mutating_tuples() {
    for produce in [false, true] {
        let mut stores = InMemoryStoreManager::new();
        let (play, _) = RSpace::create_with_replay(
            stores.r_space_stores().await.unwrap(),
            Arc::new(Box::new(Matcher)),
        )
        .unwrap();
        let before = play.create_checkpoint().await.unwrap();
        let config = config(u64::MAX, [0, 1, 1, 0]);
        let host = config.host_work();
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget.reset_for_native_execution(config).unwrap();
        let _scope = budget.enter_comm_accounting_scope();
        play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
        assert!(host
            .reserve(HostWorkDimension::VerificationOperations, u64::MAX.into())
            .is_err());
        let rejected = operation_context::scope(
            OperationOrder {
                session: budget.deploy_id(),
                path: vec![(0, 0)].into(),
            },
            async {
                if produce {
                    play.produce(Par::default(), ListParWithRandom::default(), false)
                        .await
                        .err()
                } else {
                    play.consume(
                        vec![Par::default()],
                        vec![BindPattern::default()],
                        TaggedContinuation::default(),
                        false,
                        BTreeSet::new(),
                    )
                    .await
                    .err()
                }
            },
        )
        .await
        .expect("the host budget must reject the operation");
        assert_eq!(
            InterpreterError::from(rejected),
            InterpreterError::HostWorkRejected
        );
        let after = play.create_checkpoint().await.unwrap();
        assert_eq!(before.root, after.root);
        assert!(after.log.is_empty());
    }
}

async fn exercise(
    produce: bool,
    outcome: NativeReplayOutcome,
    data_persistent: bool,
    continuation_persistent: bool,
) {
    let channel = Par::default();
    let auth = authority(1);
    let data = ListParWithRandom {
        pars: vec![Par {
            exprs: vec![models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(7)),
            }],
            ..Default::default()
        }],
        random_state: vec![7; 17],
        cost_authority: Some(auth.clone()),
        ..Default::default()
    };
    let continuation = TaggedContinuation {
        cost_authority: Some(auth.clone()),
        ..Default::default()
    };
    let channels = vec![channel.clone()];
    let patterns = vec![BindPattern {
        patterns: vec![models::rust::utils::new_freevar_par(0, Vec::new())],
        free_count: 1,
        remainder: None,
    }];
    let producer = Produce::create(&channel, &data, data_persistent);
    let consumer = Consume::create(&channels, &patterns, &continuation, continuation_persistent);
    let intro = if produce {
        observation_construction::produce_introduction(&producer, &channel, &data, &auth)
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
        NativeReplayOutcome::DeniedComm => intro.measurement.introduction_bytes,
        _ => u64::MAX,
    };
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    if outcome != NativeReplayOutcome::Stored {
        if produce {
            play.consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                continuation_persistent,
                BTreeSet::new(),
            )
            .await
            .unwrap();
        } else {
            play.produce(channel.clone(), data.clone(), data_persistent)
                .await
                .unwrap();
        }
    }
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(limit, [0, 1, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    budget
        .register_introduction_authority(
            intro.event_id,
            if produce {
                AuthorityByteEventKind::ProduceIntroduction
            } else {
                AuthorityByteEventKind::ConsumeIntroduction
            },
            &auth,
        )
        .unwrap();
    play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
    let order = || OperationOrder {
        session: budget.deploy_id(),
        path: vec![(0, 0)].into(),
    };
    let played = operation_context::scope(order(), async {
        if produce {
            play.produce(channel.clone(), data.clone(), data_persistent)
                .await
                .map(|result| result.map(|(continuation, data, _)| (continuation, data)))
        } else {
            play.consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                continuation_persistent,
                BTreeSet::new(),
            )
            .await
        }
    })
    .await;
    let denied = matches!(
        outcome,
        NativeReplayOutcome::DeniedIntroduction | NativeReplayOutcome::DeniedComm
    );
    if denied {
        assert!(matches!(played, Err(RSpaceError::OutOfPhlogistons)));
    } else {
        assert_eq!(
            played.as_ref().unwrap().is_some(),
            outcome == NativeReplayOutcome::Matched
        );
        if let Some((_, matched)) = played.as_ref().unwrap() {
            assert_eq!(matched[0].matched_datum.pars, data.pars);
        }
    }
    let log = play.create_soft_checkpoint().await.log;
    assert_eq!(
        log.len(),
        if denied {
            0
        } else if outcome == NativeReplayOutcome::Stored {
            1
        } else {
            2
        }
    );
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let rows = budget.native_operation_recording().unwrap().unwrap();
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
    operation_context::scope(order(), async {
        if produce {
            let mut altered = data.clone();
            altered.random_state[0] ^= 1;
            assert!(replay
                .produce(channel.clone(), altered, data_persistent, &auth)
                .await
                .is_err());
        } else {
            assert!(replay
                .consume(
                    channels.clone(),
                    vec![BindPattern {
                        free_count: 1,
                        ..Default::default()
                    }],
                    continuation.clone(),
                    continuation_persistent,
                    BTreeSet::new(),
                    &auth
                )
                .await
                .is_err());
        }
    })
    .await;
    assert!(replay.check_complete().await.is_err());
    let rejected = operation_context::scope(order(), async {
        if produce {
            replay
                .inner
                .produce_with_authority(channel.clone(), data.clone(), data_persistent, |source| {
                    assert_eq!(source.hash, producer.hash);
                    Err::<CostAuthority, _>(RSpaceError::HostWorkRejected)
                })
                .await
                .map(|_| ())
        } else {
            replay
                .inner
                .consume_with_authority(
                    channels.clone(),
                    patterns.clone(),
                    continuation.clone(),
                    continuation_persistent,
                    BTreeSet::new(),
                    |source| {
                        assert_eq!(source, &consumer);
                        Err::<CostAuthority, _>(RSpaceError::HostWorkRejected)
                    },
                )
                .await
                .map(|_| ())
        }
    })
    .await;
    assert!(matches!(rejected, Err(RSpaceError::HostWorkRejected)));
    assert!(replay.check_complete().await.is_err());
    for round in 0..2 {
        let resolutions = std::cell::Cell::new(0);
        let replayed = operation_context::scope(order(), async {
            if produce && round == 1 {
                replay
                    .inner
                    .produce_with_authority(
                        channel.clone(),
                        data.clone(),
                        data_persistent,
                        |source| {
                            resolutions.set(resolutions.get() + 1);
                            assert_eq!(
                                bincode::serialize(source).unwrap(),
                                bincode::serialize(&producer).unwrap()
                            );
                            Ok(auth.clone())
                        },
                    )
                    .await
                    .map(|result| result.map(|(continuation, data, _)| (continuation, data)))
            } else if round == 1 {
                replay
                    .inner
                    .consume_with_authority(
                        channels.clone(),
                        patterns.clone(),
                        continuation.clone(),
                        continuation_persistent,
                        BTreeSet::new(),
                        |source| {
                            resolutions.set(resolutions.get() + 1);
                            assert_eq!(source, &consumer);
                            Ok(auth.clone())
                        },
                    )
                    .await
            } else if produce {
                replay
                    .produce(channel.clone(), data.clone(), data_persistent, &auth)
                    .await
                    .map(|result| result.map(|(continuation, data, _)| (continuation, data)))
            } else {
                replay
                    .consume(
                        channels.clone(),
                        patterns.clone(),
                        continuation.clone(),
                        continuation_persistent,
                        BTreeSet::new(),
                        &auth,
                    )
                    .await
            }
        })
        .await;
        assert_eq!(resolutions.get(), usize::from(round == 1));
        if denied {
            assert!(
                matches!(replayed, Err(RSpaceError::OutOfPhlogistons)),
                "{replayed:?}"
            );
        } else {
            assert_eq!(replayed.as_ref().unwrap(), played.as_ref().unwrap());
        }
        replay.check_complete().await.unwrap();
        assert_eq!(
            replay.get_data(&channel).await.unwrap(),
            play.get_data(&channel).await
        );
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
async fn actual_private_session_matches_play_for_every_outcome_and_persistence_pair() {
    for produce in [false, true] {
        for data_persistent in [false, true] {
            for continuation_persistent in [false, true] {
                for outcome in [
                    NativeReplayOutcome::Stored,
                    NativeReplayOutcome::Matched,
                    NativeReplayOutcome::DeniedIntroduction,
                    NativeReplayOutcome::DeniedComm,
                ] {
                    exercise(produce, outcome, data_persistent, continuation_persistent).await;
                }
            }
        }
    }
}

struct OverlappingMatcher {
    arrivals: std::sync::Mutex<usize>,
    changed: std::sync::Condvar,
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for OverlappingMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let mut arrivals = self.arrivals.lock().unwrap();
        *arrivals += 1;
        self.changed.notify_all();
        let (arrivals, _) = self
            .changed
            .wait_timeout_while(arrivals, std::time::Duration::from_secs(15), |count| {
                *count < 2
            })
            .unwrap();
        assert!(*arrivals >= 2, "independent matchers could not overlap");
        Matcher.get(pattern, data)
    }
}

async fn exercise_readiness(shared_channel: bool, concurrent_matches: bool) {
    use futures::FutureExt;

    let channel = Par::default();
    let channels = [
        channel.clone(),
        if shared_channel {
            channel.clone()
        } else {
            Par {
                exprs: vec![models::rhoapi::Expr {
                    expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(1)),
                }],
                ..Default::default()
            }
        },
    ];
    let auth = authority(1);
    let data: Vec<_> = [1, 2]
        .into_iter()
        .map(|byte| ListParWithRandom {
            random_state: vec![byte; 17],
            cost_authority: Some(auth.clone()),
            ..Default::default()
        })
        .collect();
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    if concurrent_matches {
        for channel in &channels {
            play.consume(
                vec![channel.clone()],
                vec![BindPattern::default()],
                TaggedContinuation {
                    cost_authority: Some(auth.clone()),
                    ..Default::default()
                },
                false,
                BTreeSet::new(),
            )
            .await
            .unwrap();
        }
        play.create_checkpoint().await.unwrap();
    }
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
    let order = |index| OperationOrder {
        session: budget.deploy_id(),
        path: vec![(9 - index, 0)].into(),
    };
    for (index, datum) in data.iter().enumerate() {
        let channel = &channels[index];
        let source = Produce::create(channel, datum, false);
        let observation =
            observation_construction::produce_introduction(&source, channel, datum, &auth).unwrap();
        budget
            .register_introduction_authority(
                observation.event_id,
                AuthorityByteEventKind::ProduceIntroduction,
                &auth,
            )
            .unwrap();
        operation_context::scope(
            order(index as u64),
            play.produce(channel.clone(), datum.clone(), false),
        )
        .await
        .unwrap();
    }
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let rows = budget.native_operation_recording().unwrap().unwrap();
    assert!(rows[0].predecessors.is_empty());
    assert_eq!(
        rows[1].predecessors.as_ref(),
        if shared_channel { &[0][..] } else { &[][..] }
    );
    let log = play.create_soft_checkpoint().await.log;
    let host = config(u64::MAX, [0, 1, 1, 0]).host_work;
    let trace = with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
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
    assert_eq!(trace.journal_index(0), Some(1));
    let overlapping = Arc::new(OverlappingMatcher {
        arrivals: std::sync::Mutex::new(0),
        changed: std::sync::Condvar::new(),
    });
    let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
        if concurrent_matches {
            Arc::new(Box::new(SharedMatcher(overlapping.clone())))
        } else {
            Arc::new(Box::new(Matcher))
        };
    let replay = trace.into_session(history, matcher, host).unwrap();
    let root = replay.checkpoint().await.unwrap();
    if concurrent_matches {
        let replay = Arc::new(replay);
        let mut tasks = Vec::new();
        for index in 0..2 {
            let replay = replay.clone();
            let channel = channels[index].clone();
            let datum = data[index].clone();
            let auth = auth.clone();
            let context = order(index as u64);
            tasks.push(tokio::spawn(async move {
                operation_context::scope(context, replay.produce(channel, datum, false, &auth))
                    .await
            }));
        }
        for task in tasks {
            assert!(task.await.unwrap().unwrap().is_some());
        }
        assert_eq!(*overlapping.arrivals.lock().unwrap(), 2);
        replay.check_complete().await.unwrap();
        for channel in &channels {
            assert_eq!(
                replay.get_data(channel).await.unwrap(),
                play.get_data(channel).await
            );
            assert_eq!(
                replay
                    .get_continuations(std::slice::from_ref(channel))
                    .await
                    .unwrap(),
                play.get_store()
                    .get_continuations(std::slice::from_ref(channel))
            );
        }
        replay.restore(root).await.unwrap();
        assert!(replay.check_complete().await.is_err());
        for channel in &channels {
            assert_eq!(
                replay
                    .get_continuations(std::slice::from_ref(channel))
                    .await
                    .unwrap()
                    .len(),
                1
            );
        }
        return;
    }
    let mut dependent = Box::pin(operation_context::scope(
        order(1),
        replay.produce(channels[1].clone(), data[1].clone(), false, &auth),
    ));
    let polled = dependent.as_mut().now_or_never();
    if shared_channel {
        assert!(
            polled.is_none(),
            "dependent operation completed before its channel predecessor"
        );
    } else {
        assert!(
            matches!(polled, Some(Ok(None))),
            "independent operation waited unnecessarily"
        );
    }
    let checkpoint = replay
        .checkpoint()
        .now_or_never()
        .expect("dependency wait retained the checkpoint gate")
        .unwrap();
    operation_context::scope(
        order(0),
        replay.produce(channel.clone(), data[0].clone(), false, &auth),
    )
    .await
    .unwrap();
    if shared_channel {
        dependent.await.unwrap();
    } else {
        drop(dependent);
    }
    replay.check_complete().await.unwrap();
    for channel in &channels {
        assert_eq!(
            replay.get_data(channel).await.unwrap(),
            play.get_data(channel).await
        );
    }
    drop(checkpoint);
    replay.restore(root).await.unwrap();
    for channel in &channels {
        assert!(replay.get_data(channel).await.unwrap().is_empty());
    }
    assert!(replay.check_complete().await.is_err());
    if !shared_channel {
        return;
    }
    let mut waiting = Box::pin(operation_context::scope(
        order(1),
        replay.produce(channel.clone(), data[1].clone(), false, &auth),
    ));
    assert!(waiting.as_mut().now_or_never().is_none());
    replay.close().await.unwrap();
    assert!(waiting
        .now_or_never()
        .expect("closure did not wake dependency wait")
        .is_err());
}

#[tokio::test]
async fn actual_private_session_waits_for_channel_predecessors_without_holding_checkpoint_gate() {
    exercise_readiness(true, false).await;
}

#[tokio::test]
async fn actual_private_session_keeps_independent_operations_ready_in_reverse_order() {
    exercise_readiness(false, false).await;
}

struct SharedMatcher(Arc<OverlappingMatcher>);

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for SharedMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.0.get(pattern, data)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_private_session_runs_independent_matches_concurrently() {
    exercise_readiness(false, true).await;
}
