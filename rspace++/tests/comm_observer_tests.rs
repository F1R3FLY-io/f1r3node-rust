use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, RSpaceAccountingObserver, RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use serde::{Deserialize, Serialize};

#[path = "comm_observer_tests/properties.rs"]
mod properties;

#[path = "comm_observer_tests/native_directive.rs"]
mod native_directive;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Any;

#[derive(Clone)]
struct AnyMatch;

impl Match<Any, String, String> for AnyMatch {
    fn get(&self, _: &Any, datum: &String) -> Option<String> { Some(datum.clone()) }
}

struct Observer {
    count: AtomicUsize,
    reject: AtomicBool,
    reject_operation: AtomicBool,
    produces: AtomicUsize,
    consumes: AtomicUsize,
    observed: Mutex<Vec<COMM>>,
    reject_start: AtomicBool,
    operations: Mutex<Vec<OperationEvent>>,
    on_start: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum OperationEvent {
    Start {
        source: Vec<u8>,
        channels: Vec<String>,
        joins: Vec<Vec<String>>,
    },
    Introduction,
    Comm,
    Finish {
        source: Vec<u8>,
        completion: RSpaceOperationCompletion,
    },
}

fn operation_source_bytes(source: RSpaceOperationSource<'_>) -> Vec<u8> {
    let event = match source {
        RSpaceOperationSource::Produce(produce) => IOEvent::Produce(produce.clone()),
        RSpaceOperationSource::Consume(consume) => IOEvent::Consume(consume.clone()),
    };
    bincode::serialize(&event).unwrap()
}

impl Observer {
    fn new(reject: bool) -> Self {
        Self {
            count: AtomicUsize::new(0),
            reject: AtomicBool::new(reject),
            reject_operation: AtomicBool::new(false),
            produces: AtomicUsize::new(0),
            consumes: AtomicUsize::new(0),
            observed: Mutex::new(Vec::new()),
            reject_start: AtomicBool::new(false),
            operations: Mutex::new(Vec::new()),
            on_start: Mutex::new(None),
        }
    }
}

impl RSpaceAccountingObserver<String, Any, String, String> for Observer {
    fn observe_operation_start(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[String],
        joins: &[Vec<String>],
    ) -> Result<(), RSpaceError> {
        if self.reject_start.load(Ordering::Acquire) {
            return Err(RSpaceError::OutOfPhlogistons);
        }
        self.operations.lock().unwrap().push(OperationEvent::Start {
            source: operation_source_bytes(source),
            channels: channels.to_vec(),
            joins: joins.to_vec(),
        });
        let on_start = self.on_start.lock().unwrap().take();
        if let Some(on_start) = on_start {
            on_start();
        }
        Ok(())
    }

    fn observe_operation_finish(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        self.operations
            .lock()
            .unwrap()
            .push(OperationEvent::Finish {
                source: operation_source_bytes(source),
                completion,
            });
    }

    fn observe_produce(
        &self,
        _: &Produce,
        _: &String,
        _: &String,
        _: bool,
    ) -> Result<(), RSpaceError> {
        self.produces.fetch_add(1, Ordering::AcqRel);
        self.operations
            .lock()
            .unwrap()
            .push(OperationEvent::Introduction);
        if self.reject_operation.load(Ordering::Acquire) {
            Err(RSpaceError::OutOfPhlogistons)
        } else {
            Ok(())
        }
    }

    fn observe_consume(
        &self,
        _: &Consume,
        _: &[String],
        _: &[Any],
        _: &String,
        _: bool,
        _: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        self.consumes.fetch_add(1, Ordering::AcqRel);
        self.operations
            .lock()
            .unwrap()
            .push(OperationEvent::Introduction);
        if self.reject_operation.load(Ordering::Acquire) {
            Err(RSpaceError::OutOfPhlogistons)
        } else {
            Ok(())
        }
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        _: &String,
        _: bool,
        _: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        self.count.fetch_add(1, Ordering::AcqRel);
        self.operations.lock().unwrap().push(OperationEvent::Comm);
        self.observed.lock().unwrap().push(comm.clone());
        if self.reject.load(Ordering::Acquire) {
            Err(RSpaceError::OutOfPhlogistons)
        } else {
            Ok(())
        }
    }
}

type TestSpace = RSpace<String, Any, String, String>;

async fn space(observer: Arc<Observer>) -> TestSpace {
    let mut stores = InMemoryStoreManager::new();
    let space =
        RSpace::create(stores.r_space_stores().await.unwrap(), Arc::new(Box::new(AnyMatch)))
            .unwrap();
    space.set_accounting_observer(Some(observer));
    space
}

#[tokio::test]
async fn observes_exactly_once_for_either_trigger_side() {
    let consume_observer = Arc::new(Observer::new(false));
    let consume_triggered = space(consume_observer.clone()).await;
    assert!(
        consume_triggered
            .produce("channel".to_string(), "datum".to_string(), false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(consume_observer.count.load(Ordering::Acquire), 0);
    assert!(
        consume_triggered
            .consume(
                vec!["channel".to_string()],
                vec![Any],
                "continuation".to_string(),
                false,
                BTreeSet::new(),
            )
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(consume_observer.count.load(Ordering::Acquire), 1);
    assert_eq!(consume_observer.produces.load(Ordering::Acquire), 1);
    assert_eq!(consume_observer.consumes.load(Ordering::Acquire), 1);

    let produce_observer = Arc::new(Observer::new(false));
    let produce_triggered = space(produce_observer.clone()).await;
    assert!(
        produce_triggered
            .consume(
                vec!["channel".to_string()],
                vec![Any],
                "continuation".to_string(),
                false,
                BTreeSet::new(),
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(produce_observer.count.load(Ordering::Acquire), 0);
    assert!(
        produce_triggered
            .produce("channel".to_string(), "datum".to_string(), false)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(produce_observer.count.load(Ordering::Acquire), 1);
    assert_eq!(produce_observer.produces.load(Ordering::Acquire), 1);
    assert_eq!(produce_observer.consumes.load(Ordering::Acquire), 1);
    assert_eq!(
        *consume_observer.observed.lock().unwrap(),
        *produce_observer.observed.lock().unwrap()
    );
    assert_eq!(
        consume_observer.observed.lock().unwrap()[0].cost_identity(),
        produce_observer.observed.lock().unwrap()[0].cost_identity()
    );
}

#[tokio::test]
async fn produce_triggered_join_rejection_is_state_trace_and_counter_atomic() {
    let observer = Arc::new(Observer::new(true));
    let space = space(observer.clone()).await;
    let channels = vec!["left".to_string(), "right".to_string()];
    assert!(
        space
            .consume(channels.clone(), vec![Any, Any], "join".to_string(), false, BTreeSet::new(),)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        space
            .produce("left".to_string(), "one".to_string(), false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        space
            .produce("right".to_string(), "two".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert_eq!(observer.count.load(Ordering::Acquire), 1);
    assert_eq!(observer.consumes.load(Ordering::Acquire), 1);
    assert_eq!(observer.produces.load(Ordering::Acquire), 2);
    assert_eq!(space.get_data(&"left".to_string()).await.len(), 1);
    assert!(space.get_data(&"right".to_string()).await.is_empty());
    assert_eq!(
        space
            .get_waiting_continuations(channels.clone())
            .await
            .len(),
        1
    );

    observer.reject.store(false, Ordering::Release);
    assert!(
        space
            .produce("right".to_string(), "two".to_string(), false)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(observer.count.load(Ordering::Acquire), 2);
    assert_eq!(observer.produces.load(Ordering::Acquire), 3);
    assert!(space.get_data(&"left".to_string()).await.is_empty());
    assert!(space.get_data(&"right".to_string()).await.is_empty());
    assert!(space.get_waiting_continuations(channels).await.is_empty());
    {
        let observed = observer.observed.lock().unwrap();
        assert_eq!(observed[1].times_repeated.len(), 2);
        assert!(observed[1].times_repeated.values().all(|count| *count == 1));
    }
    let log = space.take_event_log().await;
    assert_eq!(log.len(), 4);
    assert!(matches!(log[0], Event::IoEvent(IOEvent::Consume(_))));
    assert!(matches!(log[1], Event::IoEvent(IOEvent::Produce(_))));
    assert!(matches!(log[2], Event::IoEvent(IOEvent::Produce(_))));
    assert!(matches!(log[3], Event::Comm(_)));
}

#[tokio::test]
async fn consume_triggered_join_rejection_is_state_and_trace_atomic() {
    let observer = Arc::new(Observer::new(true));
    let space = space(observer.clone()).await;
    let channels = vec!["left".to_string(), "right".to_string()];
    for (channel, datum) in [("left", "one"), ("right", "two")] {
        assert!(
            space
                .produce(channel.to_string(), datum.to_string(), false)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        space
            .consume(channels.clone(), vec![Any, Any], "join".to_string(), false, BTreeSet::new(),)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert_eq!(observer.count.load(Ordering::Acquire), 1);
    assert_eq!(space.get_data(&"left".to_string()).await.len(), 1);
    assert_eq!(space.get_data(&"right".to_string()).await.len(), 1);
    assert!(
        space
            .get_waiting_continuations(channels.clone())
            .await
            .is_empty()
    );

    observer.reject.store(false, Ordering::Release);
    assert!(
        space
            .consume(channels.clone(), vec![Any, Any], "join".to_string(), false, BTreeSet::new(),)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(observer.count.load(Ordering::Acquire), 2);
    assert_eq!(observer.consumes.load(Ordering::Acquire), 2);
    assert!(space.get_data(&"left".to_string()).await.is_empty());
    assert!(space.get_data(&"right".to_string()).await.is_empty());
    assert!(space.get_waiting_continuations(channels).await.is_empty());
    {
        let observed = observer.observed.lock().unwrap();
        assert_eq!(observed[1].times_repeated.len(), 2);
        assert!(observed[1].times_repeated.values().all(|count| *count == 1));
    }
    let log = space.take_event_log().await;
    assert_eq!(log.len(), 4);
    assert!(matches!(log[0], Event::IoEvent(IOEvent::Produce(_))));
    assert!(matches!(log[1], Event::IoEvent(IOEvent::Produce(_))));
    assert!(matches!(log[2], Event::IoEvent(IOEvent::Consume(_))));
    assert!(matches!(log[3], Event::Comm(_)));
}

#[tokio::test]
async fn persistent_produce_introduction_is_charged_once_and_each_consume_is_charged() {
    let observer = Arc::new(Observer::new(false));
    let space = space(observer.clone()).await;

    space
        .produce("persistent".to_string(), "datum".to_string(), true)
        .await
        .unwrap();
    assert_eq!(observer.produces.load(Ordering::Acquire), 1);

    for continuation in ["first", "second"] {
        assert!(
            space
                .consume(
                    vec!["persistent".to_string()],
                    vec![Any],
                    continuation.to_string(),
                    false,
                    BTreeSet::new(),
                )
                .await
                .unwrap()
                .is_some()
        );
    }

    assert_eq!(observer.produces.load(Ordering::Acquire), 1);
    assert_eq!(observer.consumes.load(Ordering::Acquire), 2);
    assert_eq!(observer.count.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn persistent_consume_introduction_is_charged_once_and_each_produce_is_charged() {
    let observer = Arc::new(Observer::new(false));
    let space = space(observer.clone()).await;

    assert!(
        space
            .consume(
                vec!["persistent-consume".to_string()],
                vec![Any],
                "continuation".to_string(),
                true,
                BTreeSet::new(),
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(observer.consumes.load(Ordering::Acquire), 1);

    for datum in ["first", "second"] {
        assert!(
            space
                .produce("persistent-consume".to_string(), datum.to_string(), false,)
                .await
                .unwrap()
                .is_some()
        );
    }

    assert_eq!(observer.consumes.load(Ordering::Acquire), 1);
    assert_eq!(observer.produces.load(Ordering::Acquire), 2);
    assert_eq!(observer.count.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn peek_removal_does_not_refund_or_recharge_an_introduction() {
    let observer = Arc::new(Observer::new(false));
    let space = space(observer.clone()).await;

    assert!(
        space
            .produce("peek".to_string(), "datum".to_string(), false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(observer.produces.load(Ordering::Acquire), 1);

    assert!(
        space
            .consume(
                vec!["peek".to_string()],
                vec![Any],
                "continuation".to_string(),
                false,
                BTreeSet::from([0]),
            )
            .await
            .unwrap()
            .is_some()
    );

    assert!(space.get_data(&"peek".to_string()).await.is_empty());
    assert_eq!(observer.produces.load(Ordering::Acquire), 1);
    assert_eq!(observer.consumes.load(Ordering::Acquire), 1);
    assert_eq!(observer.count.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn operation_rejection_precedes_store_and_trace_mutation() {
    let observer = Arc::new(Observer::new(false));
    observer.reject_operation.store(true, Ordering::Release);
    let space = space(observer.clone()).await;

    assert_eq!(
        space
            .produce("channel".to_string(), "datum".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert!(space.get_data(&"channel".to_string()).await.is_empty());
    assert!(space.take_event_log().await.is_empty());

    assert_eq!(
        space
            .consume(
                vec!["channel".to_string()],
                vec![Any],
                "continuation".to_string(),
                false,
                BTreeSet::new(),
            )
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert!(
        space
            .get_waiting_continuations(vec!["channel".to_string()])
            .await
            .is_empty()
    );
    assert!(space.take_event_log().await.is_empty());
}

#[tokio::test]
async fn operation_lifecycle_covers_storage_matches_and_both_rejection_stages() {
    for produce_trigger in [false, true] {
        for reject_introduction in [false, true] {
            for reject_comm in [false, true] {
                let observer = Arc::new(Observer::new(false));
                let space = space(observer.clone()).await;
                let channels = vec!["channel".to_string()];
                if produce_trigger {
                    space
                        .consume(
                            channels.clone(),
                            vec![Any],
                            "body".to_string(),
                            false,
                            BTreeSet::new(),
                        )
                        .await
                        .unwrap();
                } else {
                    space
                        .produce(channels[0].clone(), "data".to_string(), false)
                        .await
                        .unwrap();
                }
                {
                    let mut events = observer.operations.lock().unwrap();
                    assert_eq!(events.len(), 3);
                    assert!(matches!(events[1], OperationEvent::Introduction));
                    let OperationEvent::Start { source, .. } = &events[0] else {
                        panic!("missing start")
                    };
                    assert_eq!(events[2], OperationEvent::Finish {
                        source: source.clone(),
                        completion: RSpaceOperationCompletion::Stored,
                    });
                    events.clear();
                }
                observer
                    .reject_operation
                    .store(reject_introduction, Ordering::Release);
                observer.reject.store(reject_comm, Ordering::Release);
                let result = if produce_trigger {
                    space
                        .produce(channels[0].clone(), "data".to_string(), false)
                        .await
                        .map(|value| value.is_some())
                } else {
                    space
                        .consume(
                            channels.clone(),
                            vec![Any],
                            "body".to_string(),
                            false,
                            BTreeSet::new(),
                        )
                        .await
                        .map(|value| value.is_some())
                };
                let rejected = reject_introduction || reject_comm;
                assert_eq!(
                    result,
                    if rejected {
                        Err(RSpaceError::OutOfPhlogistons)
                    } else {
                        Ok(true)
                    }
                );
                {
                    let events = observer.operations.lock().unwrap();
                    assert_eq!(events.len(), if reject_introduction { 3 } else { 4 });
                    let OperationEvent::Start {
                        source,
                        channels: footprint,
                        joins,
                    } = &events[0]
                    else {
                        panic!("missing start")
                    };
                    assert_eq!(footprint, &channels);
                    assert_eq!(
                        joins,
                        &if produce_trigger {
                            vec![channels.clone()]
                        } else {
                            Vec::new()
                        }
                    );
                    assert!(matches!(events[1], OperationEvent::Introduction));
                    if !reject_introduction {
                        assert!(matches!(events[2], OperationEvent::Comm));
                    }
                    assert_eq!(events.last().unwrap(), &OperationEvent::Finish {
                        source: source.clone(),
                        completion: if rejected {
                            RSpaceOperationCompletion::Rejected
                        } else {
                            RSpaceOperationCompletion::Matched
                        },
                    });
                }
                let log = space.take_event_log().await;
                assert_eq!(log.len(), if rejected { 1 } else { 3 });
                if rejected {
                    assert!(log.iter().all(|event| !matches!(event, Event::Comm(_))));
                    assert_eq!(
                        space.get_data(&channels[0]).await.len(),
                        usize::from(!produce_trigger)
                    );
                    assert_eq!(
                        space.get_waiting_continuations(channels).await.len(),
                        usize::from(produce_trigger)
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn operation_start_rejection_precedes_all_observations_and_mutations() {
    let observer = Arc::new(Observer::new(false));
    observer.reject_start.store(true, Ordering::Release);
    let space = space(observer.clone()).await;
    assert_eq!(
        space
            .produce("channel".to_string(), "data".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert_eq!(
        space
            .consume(
                vec!["channel".to_string()],
                vec![Any],
                "body".to_string(),
                false,
                BTreeSet::new()
            )
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert_eq!(observer.produces.load(Ordering::Acquire), 0);
    assert_eq!(observer.consumes.load(Ordering::Acquire), 0);
    assert_eq!(observer.count.load(Ordering::Acquire), 0);
    assert!(observer.operations.lock().unwrap().is_empty());
    assert!(space.take_event_log().await.is_empty());
    assert!(space.get_data(&"channel".to_string()).await.is_empty());
    assert!(
        space
            .get_waiting_continuations(vec!["channel".to_string()])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn produce_operation_captures_current_join_footprint_even_when_comm_is_denied() {
    let observer = Arc::new(Observer::new(true));
    let space = space(observer.clone()).await;
    let channels = vec!["left".to_string(), "right".to_string()];
    space
        .produce(channels[0].clone(), "one".to_string(), false)
        .await
        .unwrap();
    space
        .consume(channels.clone(), vec![Any, Any], "body".to_string(), false, BTreeSet::new())
        .await
        .unwrap();
    observer.operations.lock().unwrap().clear();
    assert_eq!(
        space
            .produce(channels[1].clone(), "two".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    let events = observer.operations.lock().unwrap();
    let OperationEvent::Start {
        channels: direct,
        joins,
        ..
    } = &events[0]
    else {
        panic!("missing start")
    };
    assert_eq!(direct, &[channels[1].clone()]);
    assert_eq!(joins, &[channels]);
    assert!(matches!(events.last().unwrap(), OperationEvent::Finish {
        completion: RSpaceOperationCompletion::Rejected,
        ..
    }));
}

#[tokio::test]
async fn observer_replacement_cannot_split_an_inflight_operation() {
    for produce_trigger in [false, true] {
        for detach in [false, true] {
            let original = Arc::new(Observer::new(false));
            let replacement = Arc::new(Observer::new(true));
            let space = space(original.clone()).await;
            if produce_trigger {
                space
                    .consume(
                        vec!["channel".to_string()],
                        vec![Any],
                        "body".to_string(),
                        false,
                        BTreeSet::new(),
                    )
                    .await
                    .unwrap();
            } else {
                space
                    .produce("channel".to_string(), "data".to_string(), false)
                    .await
                    .unwrap();
            }
            let changed_space = space.clone();
            let next = replacement.clone();
            *original.on_start.lock().unwrap() = Some(Box::new(move || {
                changed_space.set_accounting_observer(if detach { None } else { Some(next) });
            }));
            let result = if produce_trigger {
                space
                    .produce("channel".to_string(), "data".to_string(), false)
                    .await
                    .map(|value| value.is_some())
            } else {
                space
                    .consume(
                        vec!["channel".to_string()],
                        vec![Any],
                        "body".to_string(),
                        false,
                        BTreeSet::new(),
                    )
                    .await
                    .map(|value| value.is_some())
            };
            assert_eq!(result, Ok(true));
            assert_eq!(original.count.load(Ordering::Acquire), 1);
            assert_eq!(original.produces.load(Ordering::Acquire), 1);
            assert_eq!(original.consumes.load(Ordering::Acquire), 1);
            assert_eq!(replacement.count.load(Ordering::Acquire), 0);
            assert_eq!(replacement.produces.load(Ordering::Acquire), 0);
            assert_eq!(replacement.consumes.load(Ordering::Acquire), 0);
            assert!(replacement.operations.lock().unwrap().is_empty());
            assert_eq!(space.take_event_log().await.len(), 3);
        }
    }
}

type TestReplaySpace = ReplayRSpace<String, Any, String, String>;

async fn replay_lifecycle_space(produce_trigger: bool) -> TestReplaySpace {
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = TestSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(AnyMatch)),
    )
    .unwrap();
    let initial = play.create_checkpoint().await.unwrap();
    if produce_trigger {
        play.consume(
            vec!["channel".to_string()],
            vec![Any],
            "body".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .unwrap();
        play.produce("channel".to_string(), "data".to_string(), false)
            .await
            .unwrap();
    } else {
        play.produce("channel".to_string(), "data".to_string(), false)
            .await
            .unwrap();
        play.consume(
            vec!["channel".to_string()],
            vec![Any],
            "body".to_string(),
            false,
            BTreeSet::new(),
        )
        .await
        .unwrap();
    }
    let recorded = play.create_checkpoint().await.unwrap();
    replay
        .rig_and_reset(initial.root, recorded.log)
        .await
        .unwrap();
    replay
}

async fn replay_lifecycle_operation(
    replay: &TestReplaySpace,
    produce: bool,
) -> Result<bool, RSpaceError> {
    if produce {
        replay
            .produce("channel".to_string(), "data".to_string(), false)
            .await
            .map(|result| result.is_some())
    } else {
        replay
            .consume(
                vec!["channel".to_string()],
                vec![Any],
                "body".to_string(),
                false,
                BTreeSet::new(),
            )
            .await
            .map(|result| result.is_some())
    }
}

#[tokio::test]
async fn replay_operation_lifecycle_covers_storage_matches_and_rejections() {
    for produce_trigger in [false, true] {
        for rejection in 0..4 {
            let replay = replay_lifecycle_space(produce_trigger).await;
            let observer = Arc::new(Observer::new(false));
            replay.set_accounting_observer(Some(observer.clone()));
            assert_eq!(replay_lifecycle_operation(&replay, !produce_trigger).await, Ok(false));
            {
                let mut events = observer.operations.lock().unwrap();
                assert_eq!(events.len(), 3);
                let OperationEvent::Start { source, .. } = &events[0] else {
                    panic!("missing replay operation start")
                };
                assert_eq!(events[1], OperationEvent::Introduction);
                assert_eq!(events[2], OperationEvent::Finish {
                    source: source.clone(),
                    completion: RSpaceOperationCompletion::Stored,
                });
                events.clear();
            }
            observer
                .reject_start
                .store(rejection == 1, Ordering::Release);
            observer
                .reject_operation
                .store(rejection == 2, Ordering::Release);
            observer.reject.store(rejection == 3, Ordering::Release);
            let result = replay_lifecycle_operation(&replay, produce_trigger).await;
            assert_eq!(
                result,
                if rejection == 0 {
                    Ok(true)
                } else {
                    Err(RSpaceError::OutOfPhlogistons)
                }
            );
            {
                let events = observer.operations.lock().unwrap();
                if rejection == 1 {
                    assert!(events.is_empty());
                } else {
                    assert_eq!(events.len(), if rejection == 2 { 3 } else { 4 });
                    let OperationEvent::Start {
                        source,
                        channels,
                        joins,
                    } = &events[0]
                    else {
                        panic!("missing replay operation start")
                    };
                    assert_eq!(channels, &vec!["channel".to_string()]);
                    assert_eq!(
                        joins,
                        &if produce_trigger {
                            vec![channels.clone()]
                        } else {
                            Vec::new()
                        }
                    );
                    assert_eq!(events[1], OperationEvent::Introduction);
                    if rejection != 2 {
                        assert_eq!(events[2], OperationEvent::Comm);
                    }
                    assert_eq!(events.last().unwrap(), &OperationEvent::Finish {
                        source: source.clone(),
                        completion: if rejection == 0 {
                            RSpaceOperationCompletion::Matched
                        } else {
                            RSpaceOperationCompletion::Rejected
                        },
                    });
                }
            }
            if rejection == 0 {
                replay.check_replay_data().await.unwrap();
            } else {
                assert!(replay.check_replay_data().await.is_err());
                assert_eq!(
                    replay.get_data(&"channel".to_string()).await.len(),
                    usize::from(!produce_trigger)
                );
                assert_eq!(
                    replay
                        .get_waiting_continuations(vec!["channel".to_string()])
                        .await
                        .len(),
                    usize::from(produce_trigger)
                );
            }
        }
    }
}

#[tokio::test]
async fn replay_observer_replacement_cannot_split_an_inflight_operation() {
    for produce_trigger in [false, true] {
        for detach in [false, true] {
            let replay = replay_lifecycle_space(produce_trigger).await;
            let original = Arc::new(Observer::new(false));
            let replacement = Arc::new(Observer::new(true));
            replay.set_accounting_observer(Some(original.clone()));
            assert_eq!(replay_lifecycle_operation(&replay, !produce_trigger).await, Ok(false));
            original.operations.lock().unwrap().clear();
            let changed_space = replay.clone();
            let next = replacement.clone();
            *original.on_start.lock().unwrap() = Some(Box::new(move || {
                changed_space.set_accounting_observer(if detach { None } else { Some(next) });
            }));
            assert_eq!(replay_lifecycle_operation(&replay, produce_trigger).await, Ok(true));
            assert_eq!(original.count.load(Ordering::Acquire), 1);
            assert_eq!(original.produces.load(Ordering::Acquire), 1);
            assert_eq!(original.consumes.load(Ordering::Acquire), 1);
            assert_eq!(original.operations.lock().unwrap().len(), 4);
            assert!(replacement.operations.lock().unwrap().is_empty());
            assert_eq!(replacement.count.load(Ordering::Acquire), 0);
            assert_eq!(replacement.produces.load(Ordering::Acquire), 0);
            assert_eq!(replacement.consumes.load(Ordering::Acquire), 0);
            replay.check_replay_data().await.unwrap();
        }
    }
}
