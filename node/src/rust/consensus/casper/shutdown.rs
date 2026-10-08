use std::sync::Arc;

use consensus_api::ConsensusError;
use consensus_runtime::TaskScope;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use crate::rust::soak_observer::RunningObserver;

pub(super) async fn cleanup(
    tasks: [Arc<TaskScope>; 5],
    observer: Option<RunningObserver>,
    mut store_manager: Box<dyn KeyValueStoreManager>,
) -> Result<(), ConsensusError> {
    for scope in &tasks {
        scope.abort();
    }
    for scope in &tasks {
        scope.shutdown().await;
    }
    if let Some(observer) = observer {
        observer.stop().await;
    }
    store_manager.shutdown().await.map_err(super::native_error)
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
                .send(Box::pin(cleanup(tasks, None, Box::new(store))))
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
                scope.spawn("pending", async move {
                    let _guard = guard;
                    std::future::pending().await
                });
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
}
