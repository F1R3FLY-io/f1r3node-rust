use std::sync::Arc;

use consensus_api::ConsensusError;
use consensus_runtime::TaskScope;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use crate::rust::soak_observer::RunningObserver;

pub(super) async fn drain_requests(requests: &TaskScope) -> Result<(), ConsensusError> {
    while !requests.is_empty() {
        requests.join_next().await?;
    }
    Ok(())
}

pub(super) async fn cleanup(
    tasks: [Arc<TaskScope>; 5],
    engine: Option<casper::rust::engine::engine_cell::EngineCell>,
    observer: Option<RunningObserver>,
    mut store_manager: Box<dyn KeyValueStoreManager>,
) -> Result<(), ConsensusError> {
    for scope in &tasks {
        scope.abort();
    }
    for scope in &tasks {
        scope.shutdown().await;
    }
    let mut background_result = Ok(());
    if let Some(engine) = engine {
        if let Some(casper) = engine.get().await.with_casper() {
            background_result = casper
                .shutdown_background_tasks()
                .await
                .map_err(super::native_error);
        }
    }
    if let Some(observer) = observer {
        observer.stop().await;
    }
    let storage_result = store_manager.shutdown().await.map_err(super::native_error);
    background_result.and(storage_result)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use consensus_api::{AdapterContext, Capabilities, ConsensusAdapter, ProtocolDescriptor};
    use consensus_runtime::{RuntimeBuilder, RuntimeConfig};
    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;

    type Events = Arc<Mutex<Vec<&'static str>>>;

    struct TaskGuard(Events);

    impl Drop for TaskGuard {
        fn drop(&mut self) { self.0.lock().unwrap().push("task stopped"); }
    }

    struct Store {
        events: Events,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl KeyValueStoreManager for Store {
        async fn store(&mut self, _: String) -> Result<Arc<dyn KeyValueStore>, heed::Error> {
            unreachable!()
        }

        async fn shutdown(&mut self) -> Result<(), heed::Error> {
            self.events.lock().unwrap().push("storage shutdown");
            if self.fail {
                Err(heed::Error::Io(std::io::Error::other("store close failed")))
            } else {
                Ok(())
            }
        }
    }

    enum Exit {
        Success,
        Failure,
        Stuck,
    }

    struct Adapter {
        exit: Exit,
        tasks: [Arc<TaskScope>; 5],
        store: Store,
    }

    #[async_trait::async_trait]
    impl ConsensusAdapter for Adapter {
        async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
            let Self { exit, tasks, store } = *self;
            context
                .cleanup
                .send(Box::pin(cleanup(tasks, None, None, Box::new(store))))
                .map_err(|_| ConsensusError::Stopped)?;
            context.control.ready();
            context.control.cancelled().await;
            match exit {
                Exit::Success => Ok(()),
                Exit::Failure => Err(ConsensusError::Protocol("request failed".into())),
                Exit::Stuck => std::future::pending().await,
            }
        }
    }

    #[tokio::test]
    async fn storage_shutdown_follows_task_cleanup_after_success_error_and_timeout() {
        for exit in [Exit::Success, Exit::Failure, Exit::Stuck] {
            let events = Events::default();
            let tasks = std::array::from_fn(|_| Arc::new(TaskScope::default()));
            for scope in &tasks {
                let guard = TaskGuard(events.clone());
                scope
                    .spawn("pending", async move {
                        let _guard = guard;
                        std::future::pending().await
                    })
                    .unwrap();
            }
            let expected = match exit {
                Exit::Success => Ok(()),
                Exit::Failure => Err(ConsensusError::Protocol("request failed".into())),
                Exit::Stuck => Err(ConsensusError::ShutdownTimeout),
            };
            let runtime = RuntimeBuilder::new(
                ProtocolDescriptor {
                    id: "cleanup-test".into(),
                    version: 1,
                    capabilities: Capabilities::NONE,
                },
                RuntimeConfig {
                    drain_timeout: Duration::from_millis(10),
                    cleanup_timeout: Duration::from_secs(5),
                    ..RuntimeConfig::default()
                },
            )
            .unwrap()
            .build(Box::new(Adapter {
                exit,
                tasks: tasks.clone(),
                store: Store {
                    events: events.clone(),
                    fail: false,
                },
            }))
            .start();
            runtime.handle().wait_ready().await.unwrap();
            assert_eq!(runtime.shutdown().await, expected);
            assert!(tasks.iter().all(|scope| scope.is_empty()));
            assert_eq!(*events.lock().unwrap(), [
                "task stopped",
                "task stopped",
                "task stopped",
                "task stopped",
                "task stopped",
                "storage shutdown",
            ]);
        }
    }

    #[tokio::test]
    async fn storage_shutdown_error_is_returned() {
        let events = Events::default();
        let result = cleanup(
            std::array::from_fn(|_| Arc::new(TaskScope::default())),
            None,
            None,
            Box::new(Store {
                events: events.clone(),
                fail: true,
            }),
        )
        .await;
        assert!(
            matches!(result, Err(ConsensusError::Protocol(reason)) if reason.contains("store close failed"))
        );
        assert_eq!(*events.lock().unwrap(), ["storage shutdown"]);
    }
    struct DrainingAdapter {
        requests: Arc<TaskScope>,
        store: Store,
        draining: tokio::sync::oneshot::Sender<()>,
    }

    #[async_trait::async_trait]
    impl ConsensusAdapter for DrainingAdapter {
        async fn run(self: Box<Self>, mut context: AdapterContext) -> Result<(), ConsensusError> {
            let requests = self.requests;
            let mut scopes = std::array::from_fn(|_| Arc::new(TaskScope::default()));
            scopes[1] = requests.clone();
            context
                .cleanup
                .send(Box::pin(cleanup(scopes, None, None, Box::new(self.store))))
                .map_err(|_| ConsensusError::Stopped)?;
            context.control.ready();
            context.control.cancelled().await;
            self.draining.send(()).unwrap();
            drain_requests(&requests).await
        }
    }

    async fn drain_case(finish: bool) {
        let events = Events::default();
        let requests = Arc::new(TaskScope::default());
        let (release, released) = tokio::sync::oneshot::channel();
        let (started, start_rx) = tokio::sync::oneshot::channel();
        let task_events = events.clone();
        requests
            .spawn("active request", async move {
                let _guard = TaskGuard(task_events.clone());
                started.send(()).unwrap();
                released.await.unwrap();
                task_events.lock().unwrap().push("request finished");
                Ok(())
            })
            .unwrap();
        let (draining, draining_rx) = tokio::sync::oneshot::channel();
        let runtime = RuntimeBuilder::new(
            ProtocolDescriptor {
                id: "drain-test".into(),
                version: 1,
                capabilities: Capabilities::NONE,
            },
            RuntimeConfig {
                drain_timeout: if finish {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(10)
                },
                cleanup_timeout: Duration::from_secs(5),
                ..RuntimeConfig::default()
            },
        )
        .unwrap()
        .build(Box::new(DrainingAdapter {
            requests: requests.clone(),
            store: Store {
                events: events.clone(),
                fail: false,
            },
            draining,
        }))
        .start();
        runtime.handle().wait_ready().await.unwrap();
        start_rx.await.unwrap();
        runtime.handle().request_shutdown();
        draining_rx.await.unwrap();
        assert!(events.lock().unwrap().is_empty());
        if finish {
            release.send(()).unwrap();
            runtime.shutdown().await.unwrap();
            assert_eq!(*events.lock().unwrap(), [
                "request finished",
                "task stopped",
                "storage shutdown"
            ]);
        } else {
            assert_eq!(
                runtime.shutdown().await,
                Err(ConsensusError::ShutdownTimeout)
            );
            assert_eq!(*events.lock().unwrap(), [
                "task stopped",
                "storage shutdown"
            ]);
            assert!(release.send(()).is_err());
        }
        assert!(requests.is_empty());
    }

    #[tokio::test]
    async fn graceful_drain_finishes_requests_before_storage_shutdown() { drain_case(true).await; }

    #[tokio::test]
    async fn drain_timeout_aborts_requests_before_storage_shutdown() { drain_case(false).await; }
}
