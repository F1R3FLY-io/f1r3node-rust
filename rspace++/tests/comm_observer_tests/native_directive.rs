use proptest::prelude::*;
use rspace_plus_plus::rspace::rspace_interface::ReplayOperationDirective;

use super::*;

#[derive(Clone, Copy)]
enum Response {
    Grant,
    Deny,
    Other,
}

impl Response {
    fn result(self) -> Result<(), RSpaceError> {
        match self {
            Self::Grant => Ok(()),
            Self::Deny => Err(RSpaceError::OutOfPhlogistons),
            Self::Other => Err(RSpaceError::InterpreterError("host rejection".to_string())),
        }
    }
}

struct DirectedObserver {
    inner: Observer,
    directive: ReplayOperationDirective,
    introduction: Response,
    comm: Response,
}

impl RSpaceAccountingObserver<String, Any, String, String> for DirectedObserver {
    fn replay_operation_directive(
        &self,
        _: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayOperationDirective>, RSpaceError> {
        Ok(Some(self.directive.clone()))
    }

    fn observe_operation_start(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[String],
        joins: &[Vec<String>],
    ) -> Result<(), RSpaceError> {
        self.inner.observe_operation_start(source, channels, joins)
    }

    fn observe_operation_finish(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        self.inner.observe_operation_finish(source, completion);
    }

    fn observe_produce(
        &self,
        source: &Produce,
        channel: &String,
        data: &String,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_produce(source, channel, data, persistent)?;
        self.introduction.result()
    }

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[String],
        patterns: &[Any],
        continuation: &String,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_consume(source, channels, patterns, continuation, persistent, peeks)?;
        self.introduction.result()
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &String,
        persistent: bool,
        data: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        self.inner
            .observe_comm(comm, continuation, persistent, data)?;
        self.comm.result()
    }
}

struct Fixture {
    replay: TestReplaySpace,
    expected: COMM,
    produce: bool,
    persistent_data: bool,
    persistent_continuation: bool,
    peeks: BTreeSet<i32>,
    channels: Vec<String>,
}

impl Fixture {
    async fn trigger(&self) -> Result<bool, RSpaceError> {
        if self.produce {
            self.replay
                .produce("channel".into(), "data".into(), self.persistent_data)
                .await
                .map(|result| result.is_some())
        } else {
            self.replay
                .consume(
                    self.channels.clone(),
                    vec![Any; self.channels.len()],
                    "body".into(),
                    self.persistent_continuation,
                    self.peeks.clone(),
                )
                .await
                .map(|result| result.is_some())
        }
    }

    fn observer(
        &self,
        directive: ReplayOperationDirective,
        introduction: Response,
        comm: Response,
    ) -> Arc<DirectedObserver> {
        let observer = Arc::new(DirectedObserver {
            inner: Observer::new(false),
            directive,
            introduction,
            comm,
        });
        self.replay.set_accounting_observer(Some(observer.clone()));
        observer
    }
}

async fn fixture(
    produce: bool,
    pd: bool,
    pc: bool,
    peek: bool,
    repeats: usize,
    accepted: bool,
) -> Fixture {
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = TestSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(AnyMatch)),
    )
    .unwrap();
    let initial = play.create_checkpoint().await.unwrap();
    let channels = vec!["channel".to_string(); repeats];
    let peeks = if peek {
        BTreeSet::from([0])
    } else {
        BTreeSet::new()
    };
    if produce {
        play.consume(channels.clone(), vec![Any; repeats], "body".into(), pc, peeks.clone())
            .await
            .unwrap();
        if !pd {
            for _ in 1..repeats {
                play.produce("channel".into(), "data".into(), false)
                    .await
                    .unwrap();
            }
        }
    } else {
        for _ in 0..if pd { 1 } else { repeats } {
            play.produce("channel".into(), "data".into(), pd)
                .await
                .unwrap();
        }
    }
    let observer = Arc::new(Observer::new(!accepted));
    play.set_accounting_observer(Some(observer.clone()));
    let result = if produce {
        play.produce("channel".into(), "data".into(), pd)
            .await
            .map(|v| v.is_some())
    } else {
        play.consume(channels.clone(), vec![Any; repeats], "body".into(), pc, peeks.clone())
            .await
            .map(|v| v.is_some())
    };
    assert_eq!(
        result,
        if accepted {
            Ok(true)
        } else {
            Err(RSpaceError::OutOfPhlogistons)
        }
    );
    let expected = observer.observed.lock().unwrap()[0].clone();
    let trace = play.create_checkpoint().await.unwrap().log;
    replay.rig_and_reset(initial.root, trace).await.unwrap();
    if produce {
        replay
            .consume(channels.clone(), vec![Any; repeats], "body".into(), pc, peeks.clone())
            .await
            .unwrap();
        if !pd {
            for _ in 1..repeats {
                replay
                    .produce("channel".into(), "data".into(), false)
                    .await
                    .unwrap();
            }
        }
    } else {
        for _ in 0..if pd { 1 } else { repeats } {
            replay
                .produce("channel".into(), "data".into(), pd)
                .await
                .unwrap();
        }
    }
    Fixture {
        replay,
        expected,
        produce,
        persistent_data: pd,
        persistent_continuation: pc,
        peeks,
        channels,
    }
}

async fn fingerprint(
    replay: &TestReplaySpace,
) -> (String, Vec<u8>, String, Vec<String>, Vec<Vec<String>>) {
    replay.get_data(&"channel".to_string()).await;
    let mut joins = replay.get_joins("channel".into()).await;
    joins.push(vec!["channel".to_string()]);
    for channels in joins {
        replay.get_waiting_continuations(channels).await;
    }
    let mut rows: Vec<_> = replay
        .to_map()
        .await
        .into_iter()
        .map(|(key, value)| (key, format!("{value:?}")))
        .collect();
    rows.sort();
    let checkpoint = replay.create_soft_checkpoint().await;
    replay
        .revert_to_soft_checkpoint(checkpoint.clone())
        .await
        .unwrap();
    let mut bindings: Vec<_> = replay
        .replay_data
        .lock()
        .unwrap()
        .map
        .iter()
        .map(|entry| format!("{:?}:{:?}", entry.key(), entry.value()))
        .collect();
    bindings.sort();
    (
        format!("{rows:?}"),
        bincode::serialize(&checkpoint.produce_counter).unwrap(),
        format!("{:?}", checkpoint.log),
        bindings,
        replay.get_joins("channel".into()).await,
    )
}

#[tokio::test]
async fn legacy_trace_alone_omits_denied_comm() {
    for produce in [false, true] {
        let f = fixture(produce, false, false, false, 1, false).await;
        let observer = Arc::new(Observer::new(true));
        f.replay.set_accounting_observer(Some(observer.clone()));
        assert_eq!(f.trigger().await, Ok(false));
        assert_eq!(observer.count.load(Ordering::Acquire), 0);
    }
}

#[tokio::test]
async fn denied_comm_requires_actual_match_and_preserves_all_state() {
    for produce in [false, true] {
        for pd in [false, true] {
            for pc in [false, true] {
                for peek in [false, true] {
                    for repeats in 1..=3 {
                        let f = fixture(produce, pd, pc, peek, repeats, false).await;
                        let before = fingerprint(&f.replay).await;
                        let observer = f.observer(
                            ReplayOperationDirective::RejectedComm(Arc::new(f.expected.clone())),
                            Response::Grant,
                            Response::Deny,
                        );
                        assert_eq!(f.trigger().await, Err(RSpaceError::OutOfPhlogistons));
                        assert_eq!(fingerprint(&f.replay).await, before);
                        assert_eq!(observer.inner.count.load(Ordering::Acquire), 1);
                        assert!(matches!(
                            observer.inner.operations.lock().unwrap().last(),
                            Some(OperationEvent::Finish {
                                completion: RSpaceOperationCompletion::Rejected,
                                ..
                            })
                        ));
                        f.replay.check_replay_data().await.unwrap();
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn directive_truth_table_matches_formal_contract() {
    for produce in [false, true] {
        for directive in 0..4 {
            for introduction in [Response::Grant, Response::Deny, Response::Other] {
                for comm in [Response::Grant, Response::Deny, Response::Other] {
                    for binding in [false, true] {
                        let f = fixture(produce, false, false, false, 1, binding).await;
                        let before = fingerprint(&f.replay).await;
                        let expected = Arc::new(f.expected.clone());
                        let chosen = match directive {
                            0 => ReplayOperationDirective::RejectedIntroduction,
                            1 => ReplayOperationDirective::AcceptedComm(expected),
                            2 => ReplayOperationDirective::RejectedComm(expected),
                            _ => ReplayOperationDirective::Store,
                        };
                        f.observer(chosen, introduction, comm);
                        let result = f.trigger().await;
                        let denied = (directive == 0 && matches!(introduction, Response::Deny)) ||
                            (directive == 2 &&
                                matches!(introduction, Response::Grant) &&
                                matches!(comm, Response::Deny));
                        let committed = directive == 1 &&
                            binding &&
                            matches!(introduction, Response::Grant) &&
                            matches!(comm, Response::Grant);
                        assert_eq!(result == Err(RSpaceError::OutOfPhlogistons), denied);
                        assert_eq!(result == Ok(true), committed);
                        if !committed {
                            assert_eq!(fingerprint(&f.replay).await, before);
                        } else {
                            f.replay.check_replay_data().await.unwrap();
                        }
                    }
                }
            }
        }
    }
}

fn corrupt(comm: &mut COMM, field: u8) {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    match field {
        0 => comm.consume.hash = Blake2b256Hash::new(b"wrong"),
        1 => comm.consume.persistent = !comm.consume.persistent,
        2 => comm.consume.channel_hashes.clear(),
        3 => comm.produces[0].hash = Blake2b256Hash::new(b"wrong"),
        4 => comm.produces[0].channel_hash = Blake2b256Hash::new(b"wrong"),
        5 => comm.produces[0].persistent = !comm.produces[0].persistent,
        6 => comm.produces[0].is_deterministic = false,
        7 => comm.produces[0].output_value.push(b"forged".to_vec()),
        8 => comm.produces[0].failed = true,
        9 => {
            comm.peeks.insert(7);
        }
        10 => {
            comm.times_repeated.clear();
        }
        11 => {
            comm.produces.push(comm.produces[0].clone());
        }
        12 => {
            let (mut key, value) = comm.times_repeated.pop_first().unwrap();
            key.failed = true;
            comm.times_repeated.insert(key, value);
        }
        _ => {
            comm.times_repeated
                .insert(comm.produces[0].clone(), i32::MAX);
        }
    }
}

#[tokio::test]
async fn full_comm_fields_are_authenticated_before_callback() {
    for produce in [false, true] {
        for field in 0..14 {
            let f = fixture(produce, false, false, false, 1, false).await;
            let before = fingerprint(&f.replay).await;
            let mut forged = f.expected.clone();
            corrupt(&mut forged, field);
            let observer = f.observer(
                ReplayOperationDirective::RejectedComm(Arc::new(forged)),
                Response::Grant,
                Response::Deny,
            );
            assert!(
                matches!(f.trigger().await, Err(RSpaceError::InterpreterError(_))),
                "field {field}"
            );
            assert_eq!(observer.inner.count.load(Ordering::Acquire), 0);
            assert_eq!(fingerprint(&f.replay).await, before);
        }
    }
}

#[tokio::test]
async fn missing_candidate_is_not_an_economic_denial() {
    for produce in [false, true] {
        let f = fixture(produce, false, false, false, 1, false).await;
        if produce {
            f.replay
                .remove_all_continuations(f.channels.clone())
                .await
                .unwrap();
        } else {
            f.replay
                .remove_all_data(&"channel".to_string())
                .await
                .unwrap();
        }
        let before = fingerprint(&f.replay).await;
        let observer = f.observer(
            ReplayOperationDirective::RejectedComm(Arc::new(f.expected.clone())),
            Response::Grant,
            Response::Deny,
        );
        assert!(matches!(f.trigger().await, Err(RSpaceError::InterpreterError(_))));
        assert_eq!(observer.inner.count.load(Ordering::Acquire), 0);
        assert_eq!(fingerprint(&f.replay).await, before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn repeated_denial_never_changes_state(produce in any::<bool>(), pd in any::<bool>(),
        pc in any::<bool>(), peek in any::<bool>(), repeats in 1usize..5, attempts in 1usize..8) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let f = fixture(produce, pd, pc, peek, repeats, false).await;
            let before = fingerprint(&f.replay).await;
            let observer = f.observer(ReplayOperationDirective::RejectedComm(Arc::new(f.expected.clone())), Response::Grant, Response::Deny);
            for _ in 0..attempts {
                assert_eq!(f.trigger().await, Err(RSpaceError::OutOfPhlogistons));
                assert_eq!(fingerprint(&f.replay).await, before);
            }
            assert_eq!(observer.inner.count.load(Ordering::Acquire), attempts);
        });
    }
}

struct OrderedMatch(Vec<String>);

impl Match<Any, String, String> for OrderedMatch {
    fn get(&self, _: &Any, datum: &String) -> Option<String> { Some(datum.clone()) }
    fn check_commit(&self, _: &String, data: &[String]) -> bool { data == self.0 }
}

#[tokio::test]
async fn repeated_channel_bindings_follow_play_order_not_insertion_order() {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::internal::Datum;
    let channel = "channel".to_string();
    let mut values = vec!["first".to_string(), "second".to_string()];
    values.sort_by_key(|value| {
        Blake2b256Hash::new(
            &bincode::serialize(&Datum::create(&channel, value.clone(), false)).unwrap(),
        )
    });
    for produce in [false, true] {
        let mut expected_order = values.clone();
        if produce {
            expected_order.insert(0, "incoming".into());
        }
        let width = expected_order.len();
        let channels = vec![channel.clone(); width];
        let mut stores = InMemoryStoreManager::new();
        let (play, replay) = TestSpace::create_with_replay(
            stores.r_space_stores().await.unwrap(),
            Arc::new(Box::new(OrderedMatch(expected_order))),
        )
        .unwrap();
        let initial = play.create_checkpoint().await.unwrap();
        for value in &values {
            play.produce(channel.clone(), value.clone(), false)
                .await
                .unwrap();
        }
        if produce {
            play.consume(channels.clone(), vec![Any; width], "body".into(), false, BTreeSet::new())
                .await
                .unwrap();
        }
        let play_observer = Arc::new(Observer::new(true));
        play.set_accounting_observer(Some(play_observer.clone()));
        let result = if produce {
            play.produce(channel.clone(), "incoming".into(), false)
                .await
                .map(|v| v.is_some())
        } else {
            play.consume(channels.clone(), vec![Any; width], "body".into(), false, BTreeSet::new())
                .await
                .map(|v| v.is_some())
        };
        assert_eq!(result, Err(RSpaceError::OutOfPhlogistons));
        let comm = play_observer.observed.lock().unwrap()[0].clone();
        replay
            .rig_and_reset(initial.root, play.create_checkpoint().await.unwrap().log)
            .await
            .unwrap();
        for value in &values {
            replay
                .produce(channel.clone(), value.clone(), false)
                .await
                .unwrap();
        }
        if produce {
            replay
                .consume(channels.clone(), vec![Any; width], "body".into(), false, BTreeSet::new())
                .await
                .unwrap();
        }
        replay.set_accounting_observer(Some(Arc::new(DirectedObserver {
            inner: Observer::new(false),
            directive: ReplayOperationDirective::RejectedComm(Arc::new(comm)),
            introduction: Response::Grant,
            comm: Response::Deny,
        })));
        let before = fingerprint(&replay).await;
        let result = if produce {
            replay
                .produce(channel.clone(), "incoming".into(), false)
                .await
                .map(|v| v.is_some())
        } else {
            replay
                .consume(channels.clone(), vec![Any; width], "body".into(), false, BTreeSet::new())
                .await
                .map(|v| v.is_some())
        };
        assert_eq!(result, Err(RSpaceError::OutOfPhlogistons));
        assert_eq!(fingerprint(&replay).await, before);
    }
}

#[tokio::test]
async fn accepted_directive_returns_recorded_external_result() {
    for failed in [false, true] {
        let f = fixture(true, false, false, false, 1, true).await;
        let mut recorded = f.expected.clone();
        let saved = recorded.produces[0]
            .clone()
            .mark_as_non_deterministic(vec![b"saved result".to_vec()]);
        let saved = if failed { saved.with_error() } else { saved };
        recorded.produces[0] = saved.clone();
        let count = recorded.times_repeated.remove(&saved).unwrap();
        recorded.times_repeated.insert(saved.clone(), count);
        f.replay
            .rig(vec![Event::IoEvent(IOEvent::Produce(saved.clone())), Event::Comm(recorded)])
            .await
            .unwrap();
        f.observer(
            ReplayOperationDirective::AcceptedComm(Arc::new(f.expected.clone())),
            Response::Grant,
            Response::Grant,
        );
        let (_, _, returned) = f
            .replay
            .produce("channel".into(), "data".into(), false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bincode::serialize(&returned).unwrap(), bincode::serialize(&saved).unwrap());
        f.replay.check_replay_data().await.unwrap();
    }
}

struct GuardRejects;

impl Match<Any, String, String> for GuardRejects {
    fn get(&self, _: &Any, datum: &String) -> Option<String> { Some(datum.clone()) }
    fn check_commit(&self, _: &String, _: &[String]) -> bool { false }
}

#[tokio::test]
async fn guard_rejection_cannot_be_reported_as_phlo_denial() {
    for produce in [false, true] {
        let mut f = fixture(produce, false, false, false, 1, false).await;
        let checkpoint = f.replay.create_soft_checkpoint().await;
        let replacement = TestReplaySpace::apply(
            f.replay.get_history_repository(),
            f.replay.get_store(),
            Arc::new(Box::new(GuardRejects)),
        );
        replacement
            .revert_to_soft_checkpoint(checkpoint)
            .await
            .unwrap();
        f.replay = replacement;
        let before = fingerprint(&f.replay).await;
        let observer = f.observer(
            ReplayOperationDirective::RejectedComm(Arc::new(f.expected.clone())),
            Response::Grant,
            Response::Deny,
        );
        assert!(matches!(f.trigger().await, Err(RSpaceError::InterpreterError(_))));
        assert_eq!(observer.inner.count.load(Ordering::Acquire), 0);
        assert_eq!(fingerprint(&f.replay).await, before);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_denials_do_not_publish_or_consume_accepted_bindings() {
    for produce in [false, true] {
        let f = fixture(produce, false, false, false, 1, true).await;
        let before = fingerprint(&f.replay).await;
        let observer = f.observer(
            ReplayOperationDirective::RejectedComm(Arc::new(f.expected.clone())),
            Response::Grant,
            Response::Deny,
        );
        let (left, right) = tokio::join!(f.trigger(), f.trigger());
        assert_eq!(left, Err(RSpaceError::OutOfPhlogistons));
        assert_eq!(right, Err(RSpaceError::OutOfPhlogistons));
        assert_eq!(observer.inner.count.load(Ordering::Acquire), 2);
        assert_eq!(fingerprint(&f.replay).await, before);
    }
}

fn store_observer() -> Arc<DirectedObserver> {
    Arc::new(DirectedObserver {
        inner: Observer::new(false),
        directive: ReplayOperationDirective::Store,
        introduction: Response::Grant,
        comm: Response::Grant,
    })
}

#[tokio::test]
async fn store_preserves_future_bindings_and_counts_introduction_once() {
    for produce in [false, true] {
        let replay = replay_lifecycle_space(!produce).await;
        let observer = store_observer();
        replay.set_accounting_observer(Some(observer.clone()));
        let before = fingerprint(&replay).await;
        assert_eq!(replay_lifecycle_operation(&replay, produce).await, Ok(false));
        let after = fingerprint(&replay).await;
        assert_eq!(before.3, after.3);
        assert_eq!(observer.inner.count.load(Ordering::Acquire), 0);
        assert!(matches!(
            observer.inner.operations.lock().unwrap().last(),
            Some(OperationEvent::Finish {
                completion: RSpaceOperationCompletion::Stored,
                ..
            })
        ));
        let checkpoint = replay.create_soft_checkpoint().await;
        assert_eq!(checkpoint.produce_counter.values().sum::<i32>(), i32::from(produce));
    }
}

#[tokio::test]
async fn store_consumes_greedy_guard_veto_without_exhaustive_search() {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::internal::Datum;
    let channel = "channel".to_string();
    let mut values = vec!["one".to_string(), "two".to_string()];
    values.sort_by_key(|value| {
        Blake2b256Hash::new(
            &bincode::serialize(&Datum::create(&channel, value.clone(), false)).unwrap(),
        )
    });
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = TestSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(OrderedMatch(vec![values[1].clone()]))),
    )
    .unwrap();
    for value in &values {
        play.produce(channel.clone(), value.clone(), false)
            .await
            .unwrap();
    }
    let before = play.create_checkpoint().await.unwrap();
    assert!(
        play.consume(vec![channel.clone()], vec![Any], "body".into(), false, BTreeSet::new())
            .await
            .unwrap()
            .is_none()
    );
    let expected = play.to_map().await;
    replay
        .rig_and_reset(before.root, play.create_checkpoint().await.unwrap().log)
        .await
        .unwrap();
    replay.set_accounting_observer(Some(store_observer()));
    assert!(
        replay
            .consume(vec![channel.clone()], vec![Any], "body".into(), false, BTreeSet::new())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(replay.to_map().await, expected);
    assert_eq!(replay.get_data(&channel).await.len(), 2);
}

struct NamedGuard {
    rejected: String,
    reject_all: bool,
}

impl Match<Any, String, String> for NamedGuard {
    fn get(&self, _: &Any, datum: &String) -> Option<String> { Some(datum.clone()) }
    fn check_commit(&self, continuation: &String, _: &[String]) -> bool {
        !self.reject_all && continuation != &self.rejected
    }
}

#[tokio::test]
async fn store_produce_uses_the_same_guard_veto_loop_as_play() {
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::internal::WaitingContinuation;
    let channels = vec!["channel".to_string()];
    let mut continuations = vec!["first".to_string(), "second".to_string()];
    continuations.sort_by_key(|continuation| {
        Blake2b256Hash::new(
            &bincode::serialize(&WaitingContinuation::create(
                &channels,
                &vec![Any],
                continuation,
                false,
                BTreeSet::new(),
            ))
            .unwrap(),
        )
    });
    for reject_all in [false, true] {
        let mut stores = InMemoryStoreManager::new();
        let (play, replay) = TestSpace::create_with_replay(
            stores.r_space_stores().await.unwrap(),
            Arc::new(Box::new(NamedGuard {
                rejected: continuations[0].clone(),
                reject_all,
            })),
        )
        .unwrap();
        for continuation in &continuations {
            play.consume(channels.clone(), vec![Any], continuation.clone(), false, BTreeSet::new())
                .await
                .unwrap();
        }
        let before = play.create_checkpoint().await.unwrap();
        let selected = play
            .produce(channels[0].clone(), "data".into(), false)
            .await
            .unwrap();
        assert_eq!(selected.is_none(), reject_all);
        let expected_state = play.to_map().await;
        replay
            .rig_and_reset(before.root, play.create_checkpoint().await.unwrap().log)
            .await
            .unwrap();
        let snapshot = fingerprint(&replay).await;
        replay.set_accounting_observer(Some(store_observer()));
        let result = replay
            .produce(channels[0].clone(), "data".into(), false)
            .await;
        if reject_all {
            assert!(result.unwrap().is_none());
            assert_eq!(replay.to_map().await, expected_state);
        } else {
            assert!(matches!(result, Err(RSpaceError::InterpreterError(_))));
            assert_eq!(fingerprint(&replay).await, snapshot);
        }
    }
}
