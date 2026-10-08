use std::sync::{Arc, Mutex};
use std::time::Duration;

use consensus_api::{
    AdapterContext, Capabilities, ConsensusAdapter, ConsensusError, Phase, ProtocolDescriptor,
};
use consensus_runtime::{RuntimeBuilder, RuntimeConfig, TaskScope};
use tokio::time::Instant;

type Events = Arc<Mutex<Vec<&'static str>>>;

enum Exit {
    Graceful,
    Failure,
    StartupFailure,
    Panic,
    Stuck,
}

enum Cleanup {
    Complete(Duration),
    Failure,
    Panic,
    Stuck,
}

struct ExecutionGuard(Events);

impl Drop for ExecutionGuard {
    fn drop(&mut self) { self.0.lock().unwrap().push("execution dropped"); }
}

struct Adapter {
    exit: Exit,
    cleanup: Cleanup,
    events: Events,
}

#[async_trait::async_trait]
impl ConsensusAdapter for Adapter {
    async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
        let Self {
            exit,
            cleanup,
            events,
        } = *self;
        let _guard = ExecutionGuard(events.clone());
        context
            .cleanup
            .send(Box::pin(async move {
                events.lock().unwrap().push("cleanup started");
                match cleanup {
                    Cleanup::Complete(delay) => {
                        tokio::time::sleep(delay).await;
                        events.lock().unwrap().push("cleanup finished");
                        Ok(())
                    }
                    Cleanup::Failure => {
                        Err(ConsensusError::Protocol("storage cleanup failed".into()))
                    }
                    Cleanup::Panic => panic!("cleanup panic"),
                    Cleanup::Stuck => std::future::pending().await,
                }
            }))
            .map_err(|_| ConsensusError::Stopped)?;
        if matches!(exit, Exit::StartupFailure) {
            return Err(ConsensusError::Protocol("launch failed".into()));
        }
        context.control.ready();
        context.control.cancelled().await;
        match exit {
            Exit::Graceful => Ok(()),
            Exit::Failure => Err(ConsensusError::Protocol("worker failed".into())),
            Exit::Panic => panic!("execution panic"),
            Exit::Stuck => std::future::pending().await,
            Exit::StartupFailure => unreachable!(),
        }
    }
}

fn builder(config: RuntimeConfig) -> Result<RuntimeBuilder, ConsensusError> {
    RuntimeBuilder::new(
        ProtocolDescriptor {
            id: "shutdown-test".into(),
            version: 1,
            capabilities: Capabilities::NONE,
        },
        config,
    )
}

async fn exercise(
    exit: Exit,
    cleanup: Cleanup,
) -> (
    Result<(), ConsensusError>,
    Phase,
    Vec<&'static str>,
    Duration,
) {
    let startup_failure = matches!(exit, Exit::StartupFailure);
    let events = Events::default();
    let builder = builder(RuntimeConfig {
        drain_timeout: Duration::from_secs(5),
        cleanup_timeout: Duration::from_secs(30),
        ..RuntimeConfig::default()
    })
    .unwrap();
    let handle = builder.handle();
    let mut runtime = builder
        .build(Box::new(Adapter {
            exit,
            cleanup,
            events: events.clone(),
        }))
        .start();
    let start = Instant::now();
    if !startup_failure {
        handle.wait_ready().await.unwrap();
        handle.request_shutdown();
    }
    let result = runtime.wait().await;
    assert_eq!(runtime.wait().await, result);
    let recorded = events.lock().unwrap().clone();
    (result, handle.status().phase, recorded, start.elapsed())
}

#[tokio::test(start_paused = true)]
async fn graceful_shutdown_cleans_up_after_execution_stops() {
    let (result, phase, events, _) =
        exercise(Exit::Graceful, Cleanup::Complete(Duration::ZERO)).await;
    assert_eq!(result, Ok(()));
    assert_eq!(phase, Phase::Stopped);
    assert_eq!(events, [
        "execution dropped",
        "cleanup started",
        "cleanup finished"
    ]);
}

#[tokio::test(start_paused = true)]
async fn drain_timeout_preserves_the_cleanup_budget() {
    let (result, phase, events, elapsed) =
        exercise(Exit::Stuck, Cleanup::Complete(Duration::from_secs(20))).await;
    assert_eq!(result, Err(ConsensusError::ShutdownTimeout));
    assert_eq!(phase, Phase::Failed);
    assert_eq!(elapsed, Duration::from_secs(25));
    assert_eq!(events, [
        "execution dropped",
        "cleanup started",
        "cleanup finished"
    ]);
}

#[tokio::test(start_paused = true)]
async fn execution_error_does_not_skip_cleanup() {
    let (result, phase, events, _) =
        exercise(Exit::Failure, Cleanup::Complete(Duration::ZERO)).await;
    assert_eq!(
        result,
        Err(ConsensusError::Protocol("worker failed".into()))
    );
    assert_eq!(phase, Phase::Failed);
    assert_eq!(events, [
        "execution dropped",
        "cleanup started",
        "cleanup finished"
    ]);
}

#[tokio::test(start_paused = true)]
async fn startup_failure_does_not_skip_cleanup() {
    let (result, phase, events, _) =
        exercise(Exit::StartupFailure, Cleanup::Complete(Duration::ZERO)).await;
    assert_eq!(
        result,
        Err(ConsensusError::Protocol("launch failed".into()))
    );
    assert_eq!(phase, Phase::Failed);
    assert_eq!(events, [
        "execution dropped",
        "cleanup started",
        "cleanup finished"
    ]);
}

#[tokio::test(start_paused = true)]
async fn execution_panic_does_not_skip_cleanup() {
    let (result, phase, events, _) = exercise(Exit::Panic, Cleanup::Complete(Duration::ZERO)).await;
    assert!(matches!(result, Err(ConsensusError::TaskFailed { task, .. }) if task == "adapter"));
    assert_eq!(phase, Phase::Failed);
    assert_eq!(events, [
        "execution dropped",
        "cleanup started",
        "cleanup finished"
    ]);
}

#[tokio::test(start_paused = true)]
async fn cleanup_error_is_not_reported_as_a_clean_shutdown() {
    let (result, phase, _, _) = exercise(Exit::Graceful, Cleanup::Failure).await;
    assert_eq!(
        result,
        Err(ConsensusError::Protocol("storage cleanup failed".into()))
    );
    assert_eq!(phase, Phase::Failed);
}

#[tokio::test(start_paused = true)]
async fn cleanup_panic_is_reported() {
    let (result, phase, _, _) = exercise(Exit::Graceful, Cleanup::Panic).await;
    assert!(matches!(result, Err(ConsensusError::TaskFailed { task, .. }) if task == "cleanup"));
    assert_eq!(phase, Phase::Failed);
}

#[tokio::test(start_paused = true)]
async fn cleanup_timeout_is_distinct_from_drain_timeout() {
    let (result, phase, events, elapsed) = exercise(Exit::Graceful, Cleanup::Stuck).await;
    assert_eq!(result, Err(ConsensusError::CleanupTimeout));
    assert_eq!(phase, Phase::Failed);
    assert_eq!(elapsed, Duration::from_secs(30));
    assert_eq!(events, ["execution dropped", "cleanup started"]);
}

#[tokio::test(start_paused = true)]
async fn execution_and_cleanup_errors_are_both_preserved() {
    let (result, phase, _, _) = exercise(Exit::Failure, Cleanup::Failure).await;
    assert_eq!(
        result,
        Err(ConsensusError::ShutdownFailed {
            primary: Box::new(ConsensusError::Protocol("worker failed".into())),
            cleanup: Box::new(ConsensusError::Protocol("storage cleanup failed".into())),
        })
    );
    assert_eq!(phase, Phase::Failed);
}

#[tokio::test(start_paused = true)]
async fn total_shutdown_is_bounded_by_both_budgets() {
    let (result, phase, _, elapsed) = exercise(Exit::Stuck, Cleanup::Stuck).await;
    assert_eq!(
        result,
        Err(ConsensusError::ShutdownFailed {
            primary: Box::new(ConsensusError::ShutdownTimeout),
            cleanup: Box::new(ConsensusError::CleanupTimeout),
        })
    );
    assert_eq!(phase, Phase::Failed);
    assert_eq!(elapsed, Duration::from_secs(35));
}

#[tokio::test]
async fn interrupted_scope_shutdown_can_still_join_its_tasks() {
    let scope = TaskScope::default();
    let events = Events::default();
    let guard = ExecutionGuard(events.clone());
    scope.spawn("pending", async move {
        let _guard = guard;
        std::future::pending().await
    });
    let mut first_shutdown = Box::pin(scope.shutdown());
    assert!(futures::poll!(&mut first_shutdown).is_pending());
    drop(first_shutdown);
    assert_eq!(scope.len(), 1);
    scope.shutdown().await;
    assert!(scope.is_empty());
    assert_eq!(*events.lock().unwrap(), ["execution dropped"]);
}

#[test]
fn zero_cleanup_budget_is_rejected() {
    assert!(builder(RuntimeConfig {
        cleanup_timeout: Duration::ZERO,
        ..RuntimeConfig::default()
    })
    .is_err());
}
