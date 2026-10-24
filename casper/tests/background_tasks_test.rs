use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use casper::rust::background_tasks::{spawn, BackgroundTaskSpawner};
use casper::rust::errors::CasperError;

#[tokio::test]
async fn unmanaged_tasks_report_acceptance_and_execute() {
    let (sent, received) = tokio::sync::oneshot::channel();
    spawn(
        &None,
        "unmanaged",
        Box::pin(async move {
            sent.send(()).unwrap();
            Ok(())
        }),
    )
    .unwrap();
    received.await.unwrap();
}

#[tokio::test]
async fn managed_task_rejection_reaches_the_caller_and_drops_the_future() {
    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
    }

    let spawner: BackgroundTaskSpawner = Arc::new(|name, _task| {
        assert_eq!(name, "rejected");
        Err(CasperError::RuntimeError("scope stopped".into()))
    });
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(dropped.clone());
    let result = spawn(
        &Some(spawner),
        "rejected",
        Box::pin(async move {
            let _guard = guard;
            panic!("Rejected task was polled");
        }),
    );
    assert_eq!(
        result,
        Err(CasperError::RuntimeError("scope stopped".into()))
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn managed_tasks_report_acceptance_and_execute() {
    let spawner: BackgroundTaskSpawner = Arc::new(|name, task| {
        assert_eq!(name, "managed");
        tokio::spawn(task);
        Ok(())
    });
    let (sent, received) = tokio::sync::oneshot::channel();
    spawn(
        &Some(spawner),
        "managed",
        Box::pin(async move {
            sent.send(()).unwrap();
            Ok(())
        }),
    )
    .unwrap();
    received.await.unwrap();
}
