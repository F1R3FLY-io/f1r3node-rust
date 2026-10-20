use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
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
use crate::rust::interpreter::accounting::native_runtime::{
    CheckedNativeOperationTrace, NativeOperationTraceLimits,
};
use crate::rust::interpreter::accounting::{
    observation_construction, NativeBudgetRecording, RuntimeBudget,
};
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::rho_runtime::test_accounting_observer;

mod histories;
mod joins;

fn empty_trace() -> CheckedNativeOperationTrace {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([]),
        retries: Arc::from([]),
        used: 0,
        paths: crate::rust::interpreter::accounting::native_runtime::tests::fixture_paths(),
    };
    with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
        contract.check_operation_journal(
            recording.session,
            &recording,
            Arc::from([]),
            journal_limits(),
        )
    })
    .expect("empty checked journal")
    .bind_trace(Arc::from([]), NativeOperationTraceLimits {
        events: 10,
        source_entries: 100,
        source_bytes: 100_000,
        telemetry_items: 100,
        telemetry_bytes: 100_000,
    })
    .expect("empty checked trace")
}

#[tokio::test]
async fn replay_constructors_preserve_selected_source_epoch_and_legacy_defaults() {
    for selected in [
        None,
        Some(EventCodecEpoch::Legacy),
        Some(EventCodecEpoch::CostV1),
        Some(EventCodecEpoch::CostV2),
    ] {
        for installed in [false, true] {
            let epoch = selected.unwrap_or(EventCodecEpoch::Legacy);
            let trace = empty_trace();
            let mut stores = InMemoryStoreManager::new();
            let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
                Arc::new(Box::new(Matcher));
            let (play, _) = RSpace::create_with_replay_for_epoch(
                epoch,
                stores.r_space_stores().await.expect("in-memory stores"),
                matcher.clone(),
            )
            .expect("epoch-bound live space");
            let history = play.get_history_repository();
            let replay = match (selected, installed) {
                (None, false) => trace.into_session(history, matcher),
                (None, true) => trace.into_installed_session(history, matcher, std::iter::empty()),
                (Some(epoch), false) => trace.into_session_for_epoch(epoch, history, matcher),
                (Some(epoch), true) => trace.into_installed_session_for_epoch(
                    epoch,
                    history,
                    matcher,
                    std::iter::empty(),
                ),
            }
            .expect("epoch-bound replay session");
            assert_eq!(replay.inner.execution_context(), epoch);
            let mut channel = Par::default();
            if epoch != EventCodecEpoch::Legacy {
                channel
                    .cost_stacks
                    .push(models::rhoapi::CostStack::default());
            }
            let cost_authority = (epoch != EventCodecEpoch::Legacy).then(|| authority(1));
            let data = ListParWithRandom {
                random_state: vec![7; 17],
                cost_authority: cost_authority.clone(),
                ..Default::default()
            };
            let continuation = TaggedContinuation {
                cost_authority,
                ..Default::default()
            };
            let producer = Produce::create_for_epoch(epoch, &channel, &data, false)
                .expect("expected epoch-bound produce");
            let channels = vec![channel.clone()];
            let patterns = vec![BindPattern::default()];
            let consumer =
                Consume::create_for_epoch(epoch, &channels, &patterns, &continuation, false)
                    .expect("expected epoch-bound consume");
            let producer_seen = std::cell::Cell::new(false);
            let result = replay
                .inner
                .produce_with_authority(channel, data, false, |source| {
                    assert_eq!(source, &producer);
                    producer_seen.set(true);
                    Err::<IntroductionRecord, _>(authority_rejection())
                })
                .await;
            assert_eq!(result, Err(authority_rejection()));
            assert!(producer_seen.get());
            let consumer_seen = std::cell::Cell::new(false);
            let result = replay
                .inner
                .consume_with_authority(
                    channels,
                    patterns,
                    continuation,
                    false,
                    BTreeSet::new(),
                    |source| {
                        assert_eq!(source, &consumer);
                        consumer_seen.set(true);
                        Err::<IntroductionRecord, _>(authority_rejection())
                    },
                )
                .await;
            assert_eq!(result, Err(authority_rejection()));
            assert!(consumer_seen.get());
            replay
                .check_complete()
                .await
                .expect("no operation published");
        }
    }
}

#[tokio::test]
async fn runtime_replay_constructor_requires_the_exact_budget_execution_context() {
    for context in [
        None,
        Some(EventCodecEpoch::Legacy),
        Some(EventCodecEpoch::CostV1),
        Some(EventCodecEpoch::CostV2),
    ] {
        for requested_epoch in [
            EventCodecEpoch::Legacy,
            EventCodecEpoch::CostV1,
            EventCodecEpoch::CostV2,
        ] {
            let settings = config(u64::MAX, [0, 1, 1, 0]);
            let budget = RuntimeBudget::new(Cost::unsafe_max());
            if let Some(epoch) = context {
                budget
                    .reset_for_native_execution(settings.with_event_codec_epoch(epoch))
                    .expect("valid native configuration");
            }
            let trace = empty_trace();
            let mut stores = InMemoryStoreManager::new();
            let matcher: Arc<Box<dyn Match<BindPattern, ListParWithRandom, TaggedContinuation>>> =
                Arc::new(Box::new(Matcher));
            let (play, _) = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay_for_epoch(
                context.unwrap_or(EventCodecEpoch::Legacy),
                stores.r_space_stores().await.expect("in-memory stores"),
                matcher.clone(),
            ).expect("epoch-bound live space");
            let result = trace.into_runtime_session(
                play.get_history_repository(),
                matcher,
                std::iter::empty(),
                budget.clone(),
                requested_epoch,
            );
            if let Some(expected) = context.filter(|epoch| *epoch == requested_epoch) {
                let replay = result.expect("matching execution context");
                assert_eq!(replay.inner.execution_context(), expected);
                assert_eq!(budget.native_event_codec_epoch(), Some(expected));
                let checkpoint = replay.checkpoint().await.expect("empty replay checkpoint");
                replay
                    .restore(checkpoint)
                    .await
                    .expect("same-session restore");
                assert_eq!(replay.inner.execution_context(), expected);
                assert_eq!(budget.native_event_codec_epoch(), Some(expected));
                replay
                    .check_complete()
                    .await
                    .expect("empty trace is complete");
                assert_eq!(
                    replay.completed_usage().await.expect("empty replay usage"),
                    0
                );
            } else {
                assert!(matches!(result, Err(RSpaceError::InterpreterError(message))
                    if message == "native replay codec epoch differs from bound runtime"));
                assert_eq!(budget.native_event_codec_epoch(), context);
                assert_eq!(budget.native_phlo_usage(), context.map(|_| 0));
                assert!(budget.byte_observations().rows.is_empty());
            }
        }
    }
}

fn unmeasured(authority: &CostAuthority) -> IntroductionRecord {
    IntroductionRecord {
        authority: authority.clone(),
        measurement: None,
    }
}

/// Changed by DR-131 S6: the error that a failing authority resolution
/// returns. It was `RSpaceError::HostWorkRejected`, which is deleted. The
/// replay returns the error of the resolution unchanged.
fn authority_rejection() -> RSpaceError {
    RSpaceError::BugFoundError("authority rejected".to_owned())
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
        pars: vec![models::par_from_default! {
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
    let epoch = EventCodecEpoch::CostV1;
    let producer = Produce::create_for_epoch(epoch, &channel, &data, data_persistent)
        .expect("cost-bearing produce source");
    let consumer = Consume::create_for_epoch(
        epoch,
        &channels,
        &patterns,
        &continuation,
        continuation_persistent,
    )
    .expect("cost-bearing consume source");
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
    let (play, _) = RSpace::create_with_replay_for_epoch(
        epoch,
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
    let trace = with_contract(limit, [0, 1, 1, 0], |contract| {
        contract
            .check_operation_journal(recording.session, &recording, rows, journal_limits())
            .unwrap()
    })
    .bind_trace(log.into(), NativeOperationTraceLimits {
        events: 10,
        source_entries: 100,
        source_bytes: 100_000,
        telemetry_items: 100,
        telemetry_bytes: 100_000,
    })
    .unwrap();
    let replay = trace
        .into_session_for_epoch(play.codec_epoch(), history, Arc::new(Box::new(Matcher)))
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
                    Err::<IntroductionRecord, _>(authority_rejection())
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
                        Err::<IntroductionRecord, _>(authority_rejection())
                    },
                )
                .await
                .map(|_| ())
        }
    })
    .await;
    assert_eq!(rejected, Err(authority_rejection()));
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
                            Ok(unmeasured(&auth))
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
                            Ok(unmeasured(&auth))
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
            play.get_data(&channel).await.expect("live data query")
        );
        assert_eq!(
            replay.get_continuations(&channels).await.unwrap(),
            play.get_store()
                .get_continuations(&channels)
                .expect("live continuation query")
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

impl OverlappingMatcher {
    fn rendezvous(&self) {
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
    }
}

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for OverlappingMatcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        self.rendezvous();
        Matcher.get(pattern, data)
    }

    fn check_commit_by_reference(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        Matcher.check_commit_by_reference(continuation, matched)
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
            models::par_from_default! {
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
    let (play, _) = RSpace::create_with_replay_for_epoch(
        EventCodecEpoch::CostV1,
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
        let source = Produce::create_for_epoch(play.codec_epoch(), channel, datum, false)
            .expect("cost-bearing readiness source");
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
    let trace = with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
        contract
            .check_operation_journal(recording.session, &recording, rows, journal_limits())
            .unwrap()
    })
    .bind_trace(log.into(), NativeOperationTraceLimits {
        events: 10,
        source_entries: 100,
        source_bytes: 100_000,
        telemetry_items: 100,
        telemetry_bytes: 100_000,
    })
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
    let replay = trace
        .into_session_for_epoch(play.codec_epoch(), history, matcher)
        .unwrap();
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
                play.get_data(channel).await.expect("live data query")
            );
            assert_eq!(
                replay
                    .get_continuations(std::slice::from_ref(channel))
                    .await
                    .unwrap(),
                play.get_store()
                    .get_continuations(std::slice::from_ref(channel))
                    .expect("live continuation query")
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
            play.get_data(channel).await.expect("live data query")
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

    fn check_commit_by_reference(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        self.0.check_commit_by_reference(continuation, matched)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_private_session_runs_independent_matches_concurrently() {
    exercise_readiness(false, true).await;
}
