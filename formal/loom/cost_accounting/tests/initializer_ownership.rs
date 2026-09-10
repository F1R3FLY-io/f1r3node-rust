use std::future::{pending, poll_fn, Future};
use std::task::Poll;

use proptest::prelude::*;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;

#[path = "../../../../node/src/rust/runtime/runtime_supervision.rs"]
mod runtime_supervision;
use runtime_supervision::{abort_and_drain, next_runtime_event, RuntimeEvent};

async fn cancel_pending(future: impl Future) {
    tokio::pin!(future);
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn legacy_take_before_await_detaches_on_competing_select() {
    let (complete_tx, complete_rx) = oneshot::channel::<()>();
    let (finished_tx, finished_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        complete_rx.await.unwrap();
        finished_tx.send(()).unwrap();
    });
    let abort = handle.abort_handle();
    let mut owner = Some(handle);
    tokio::select! {
        biased;
        result = async {
            match owner.take() {
                Some(handle) => Some(handle.await),
                None => None,
            }
        } => panic!("initialization unexpectedly completed: {result:?}"),
        () = std::future::ready(()) => {}
    }
    assert!(owner.is_none());
    assert!(!abort.is_finished());
    complete_tx.send(()).unwrap();
    finished_rx.await.unwrap();
}

#[tokio::test]
async fn join_set_retains_initializer_across_competing_select() {
    let (complete_tx, complete_rx) = oneshot::channel::<()>();
    let mut owner = tokio::task::JoinSet::new();
    owner.spawn(complete_rx);
    tokio::select! {
        biased;
        result = owner.join_next() => panic!("initialization unexpectedly completed: {result:?}"),
        () = std::future::ready(()) => {}
    }
    assert_eq!(owner.len(), 1);
    complete_tx.send(()).unwrap();
    assert_eq!(owner.join_next().await.unwrap().unwrap(), Ok(()));
    assert!(owner.is_empty());
}

struct DropNotice(Option<oneshot::Sender<()>>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        if let Some(notice) = self.0.take() {
            let _ = notice.send(());
        }
    }
}

#[tokio::test]
async fn join_set_drop_requests_cancellation_without_assuming_immediate_retirement() {
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let mut owner = tokio::task::JoinSet::<()>::new();
    let notice = DropNotice(Some(dropped_tx));
    owner.spawn(async move {
        let _notice = notice;
        pending::<()>().await;
    });
    drop(owner);
    dropped_rx.await.unwrap();
}

#[tokio::test]
async fn production_event_selection_preserves_all_initializer_outcomes() {
    for outcome in 0..3 {
        let (complete_tx, complete_rx) = oneshot::channel();
        let (_startup_tx, mut startup_rx) = watch::channel(None::<String>);
        let mut initializers = JoinSet::new();
        let mut critical = JoinSet::<()>::new();
        initializers.spawn(async move {
            complete_rx.await.unwrap();
            match outcome {
                0 => Ok(17),
                1 => Err(23),
                _ => panic!("initializer panic fixture"),
            }
        });
        cancel_pending(next_runtime_event(
            &mut initializers,
            &mut critical,
            &mut startup_rx,
            pending(),
        ))
        .await;
        assert_eq!(initializers.len(), 1);
        complete_tx.send(()).unwrap();
        let RuntimeEvent::Initialized(result) =
            next_runtime_event(&mut initializers, &mut critical, &mut startup_rx, pending()).await
        else {
            panic!("wrong event");
        };
        match outcome {
            0 => assert_eq!(result.unwrap(), Ok(17)),
            1 => assert_eq!(result.unwrap(), Err(23)),
            _ => assert!(result.unwrap_err().is_panic()),
        }
        assert!(initializers.is_empty());
        cancel_pending(next_runtime_event(
            &mut initializers,
            &mut critical,
            &mut startup_rx,
            pending(),
        ))
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_parent_cancellation_releases_pending_initializer() {
    let (started_tx, started_rx) = oneshot::channel();
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let notice = DropNotice(Some(dropped_tx));
    let parent = tokio::spawn(async move {
        let (_startup_tx, mut startup_rx) = watch::channel(());
        let mut initializers = JoinSet::new();
        let mut critical = JoinSet::<()>::new();
        initializers.spawn(async move {
            let _notice = notice;
            pending::<()>().await;
        });
        cancel_pending(next_runtime_event(
            &mut initializers,
            &mut critical,
            &mut startup_rx,
            pending(),
        ))
        .await;
        started_tx.send(()).unwrap();
        next_runtime_event(&mut initializers, &mut critical, &mut startup_rx, pending()).await
    });
    started_rx.await.unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    dropped_rx.await.unwrap();
}

#[tokio::test]
async fn production_shutdown_cancels_before_first_poll_and_drains_both_sets() {
    let entered = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut initializers = JoinSet::new();
    let mut critical = JoinSet::new();
    let mut retirements = Vec::new();
    for index in 0..9 {
        let (dropped_tx, dropped_rx) = oneshot::channel();
        retirements.push(dropped_rx);
        let notice = DropNotice(Some(dropped_tx));
        let entered = entered.clone();
        let task = async move {
            let _notice = notice;
            entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            pending::<()>().await;
        };
        if index == 0 {
            initializers.spawn(task);
        } else {
            critical.spawn(task);
        }
    }
    abort_and_drain(&mut initializers, &mut critical).await;
    assert!(initializers.is_empty());
    assert!(critical.is_empty());
    assert_eq!(entered.load(std::sync::atomic::Ordering::SeqCst), 0);
    for mut retirement in retirements {
        assert_eq!(retirement.try_recv(), Ok(()));
    }
}

#[tokio::test]
async fn production_shutdown_cancels_already_running_children() {
    let mut initializers = JoinSet::new();
    let mut critical = JoinSet::new();
    let mut retirements = Vec::new();
    let mut starts = Vec::new();
    for index in 0..3 {
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let (started_tx, started_rx) = oneshot::channel();
        retirements.push(dropped_rx);
        starts.push(started_rx);
        let notice = DropNotice(Some(dropped_tx));
        let task = async move {
            let _notice = notice;
            started_tx.send(()).unwrap();
            pending::<()>().await;
        };
        if index == 0 {
            initializers.spawn(task);
        } else {
            critical.spawn(task);
        }
    }
    for start in starts {
        start.await.unwrap();
    }
    abort_and_drain(&mut initializers, &mut critical).await;
    assert!(initializers.is_empty());
    assert!(critical.is_empty());
    for mut retirement in retirements {
        assert_eq!(retirement.try_recv(), Ok(()));
    }
}

#[tokio::test]
async fn production_ready_completion_and_shutdown_never_duplicate_observation() {
    for _ in 0..32 {
        let (_startup_tx, mut startup_rx) = watch::channel(());
        let mut initializers = JoinSet::new();
        let mut critical = JoinSet::<()>::new();
        let finished = initializers.spawn(async { 47 });
        while !finished.is_finished() {
            tokio::task::yield_now().await;
        }
        let event = next_runtime_event(
            &mut initializers,
            &mut critical,
            &mut startup_rx,
            std::future::ready(()),
        )
        .await;
        match event {
            RuntimeEvent::Initialized(result) => {
                assert_eq!(result.unwrap(), 47);
                assert!(initializers.is_empty());
            }
            RuntimeEvent::Shutdown => assert_eq!(initializers.len(), 1),
            _ => panic!("unexpected event"),
        }
        abort_and_drain(&mut initializers, &mut critical).await;
        assert!(initializers.is_empty());
        cancel_pending(next_runtime_event(
            &mut initializers,
            &mut critical,
            &mut startup_rx,
            pending(),
        ))
        .await;
    }
}

#[tokio::test]
async fn production_reports_closed_startup_watch() {
    let (startup_tx, mut startup_rx) = watch::channel(());
    let mut initializers = JoinSet::<()>::new();
    let mut critical = JoinSet::<()>::new();
    drop(startup_tx);
    assert!(matches!(
        next_runtime_event(&mut initializers, &mut critical, &mut startup_rx, pending(),).await,
        RuntimeEvent::StartupChanged(Err(_))
    ));
}

proptest! {
    #[test]
    fn production_arbitrary_competition_preserves_one_initializer(
        operations in proptest::collection::vec(0u8..4, 0..128),
        success in any::<bool>(),
        value in any::<u32>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (complete_tx, complete_rx) = oneshot::channel();
            let (startup_tx, mut startup_rx) = watch::channel(0usize);
            let mut initializers = JoinSet::new();
            let mut critical = JoinSet::new();
            initializers.spawn(complete_rx);
            for (index, operation) in operations.into_iter().enumerate() {
                match operation {
                    0 => cancel_pending(next_runtime_event(
                        &mut initializers, &mut critical, &mut startup_rx, pending(),
                    )).await,
                    1 => {
                        startup_tx.send(index + 1).unwrap();
                        assert!(matches!(next_runtime_event(
                            &mut initializers, &mut critical, &mut startup_rx, pending(),
                        ).await, RuntimeEvent::StartupChanged(Ok(()))));
                    }
                    2 => {
                        critical.spawn(async move { index });
                        let RuntimeEvent::Critical(result) = next_runtime_event(
                            &mut initializers, &mut critical, &mut startup_rx, pending(),
                        ).await else { panic!("critical result lost"); };
                        assert_eq!(result.unwrap(), index);
                    }
                    _ => tokio::task::yield_now().await,
                }
                assert_eq!(initializers.len(), 1);
            }
            let expected = if success { Ok(value) } else { Err(value) };
            complete_tx.send(expected).unwrap();
            let RuntimeEvent::Initialized(result) = next_runtime_event(
                &mut initializers, &mut critical, &mut startup_rx, pending(),
            ).await else { panic!("initializer result lost"); };
            assert_eq!(result.unwrap().unwrap(), expected);
            assert!(initializers.is_empty());
            abort_and_drain(&mut initializers, &mut critical).await;
        });
    }
}
