use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake};

use consensus_api::ConsensusError;
use consensus_runtime::TaskScope;

#[derive(Default)]
struct WakeFlag(AtomicBool);

impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) { self.0.store(true, Ordering::SeqCst); }
}

#[tokio::test]
async fn closing_an_empty_scope_wakes_existing_joiners() {
    for shutdown in [false, true] {
        let scope = TaskScope::default();
        let first_wake = Arc::new(WakeFlag::default());
        let second_wake = Arc::new(WakeFlag::default());
        let mut first = Box::pin(scope.join_next());
        let mut second = Box::pin(scope.join_next());
        for (join, wake) in [(&mut first, &first_wake), (&mut second, &second_wake)] {
            let waker = wake.clone().into();
            assert!(join
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
        }
        let mut cleanup = Box::pin(scope.shutdown());
        if shutdown {
            assert!(futures::poll!(&mut cleanup).is_pending());
        } else {
            scope.abort();
        }
        assert!(first_wake.0.load(Ordering::SeqCst));
        assert_eq!(first.await, Ok(None));
        assert!(second_wake.0.load(Ordering::SeqCst));
        assert_eq!(second.await, Ok(None));
        cleanup.await;
        assert_eq!(scope.join_next().await, Ok(None));
    }
}

#[tokio::test]
async fn an_open_scope_keeps_waiting_after_its_last_task_finishes() {
    let scope = TaskScope::default();
    scope.spawn("first", async { Ok(()) }).unwrap();
    assert_eq!(scope.join_next().await.unwrap().as_deref(), Some("first"));
    let mut waiting = Box::pin(scope.join_next());
    assert!(futures::poll!(&mut waiting).is_pending());
    scope.spawn("second", async { Ok(()) }).unwrap();
    assert_eq!(waiting.await.unwrap().as_deref(), Some("second"));
    scope.shutdown().await;
    assert_eq!(scope.join_next().await, Ok(None));
}

#[tokio::test]
async fn abort_reports_unjoined_tasks_before_completion() {
    let scope = TaskScope::default();
    let (started, ready) = tokio::sync::oneshot::channel();
    scope
        .spawn("pending", async move {
            started.send(()).unwrap();
            std::future::pending().await
        })
        .unwrap();
    ready.await.unwrap();
    scope.abort();
    assert!(matches!(
        scope.join_next().await,
        Err(ConsensusError::TaskFailed { .. })
    ));
    assert_eq!(scope.join_next().await, Ok(None));
}

#[tokio::test]
async fn shutdown_and_join_can_wait_on_the_same_task() {
    let scope = TaskScope::default();
    scope.spawn("pending", std::future::pending()).unwrap();
    let mut waiting = Box::pin(scope.join_next());
    assert!(futures::poll!(&mut waiting).is_pending());
    let (_, result) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(scope.shutdown(), waiting)
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(ConsensusError::TaskFailed { .. })));
    assert!(scope.is_empty());
    assert_eq!(scope.join_next().await, Ok(None));
}

#[tokio::test]
async fn cancelling_a_joiner_does_not_block_shutdown() {
    let scope = TaskScope::default();
    let mut waiting = Box::pin(scope.join_next());
    assert_eq!(futures::poll!(&mut waiting), Poll::Pending);
    drop(waiting);
    scope.shutdown().await;
    assert_eq!(scope.join_next().await, Ok(None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn racing_spawn_and_shutdown_leave_no_unowned_tasks() {
    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
    }

    for _ in 0..64 {
        let scope = Arc::new(TaskScope::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = DropFlag(dropped.clone());
        let spawner = {
            let scope = scope.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                scope.spawn("racing task", async move {
                    let _guard = guard;
                    std::future::pending().await
                })
            })
        };
        barrier.wait().await;
        scope.shutdown().await;
        assert!(matches!(
            spawner.await.unwrap(),
            Ok(()) | Err(ConsensusError::Stopped)
        ));
        assert!(dropped.load(Ordering::SeqCst));
        assert!(scope.is_empty());
        assert_eq!(
            scope.spawn("late task", async { Ok(()) }),
            Err(ConsensusError::Stopped)
        );
        assert_eq!(scope.join_next().await, Ok(None));
    }
}
