use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceAccountingObserver};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use serde::{Deserialize, Serialize};

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
        }
    }
}

impl RSpaceAccountingObserver<String, Any, String, String> for Observer {
    fn observe_produce(
        &self,
        _: &Produce,
        _: &String,
        _: &String,
        _: bool,
    ) -> Result<(), RSpaceError> {
        self.produces.fetch_add(1, Ordering::AcqRel);
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
