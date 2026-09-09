use std::future::Future;

use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};

#[derive(Debug)]
pub(crate) enum RuntimeEvent<I, C> {
    StartupChanged(Result<(), watch::error::RecvError>),
    Critical(Result<C, JoinError>),
    Initialized(Result<I, JoinError>),
    Shutdown,
}

pub(crate) async fn next_runtime_event<I: 'static, C: 'static, E>(
    initializers: &mut JoinSet<I>,
    critical_tasks: &mut JoinSet<C>,
    startup_failure: &mut watch::Receiver<E>,
    shutdown: impl Future<Output = ()>,
) -> RuntimeEvent<I, C> {
    tokio::select! {
        changed = startup_failure.changed() => RuntimeEvent::StartupChanged(changed),
        Some(result) = critical_tasks.join_next() => RuntimeEvent::Critical(result),
        Some(result) = initializers.join_next() => RuntimeEvent::Initialized(result),
        () = shutdown => RuntimeEvent::Shutdown,
    }
}

pub(crate) async fn abort_and_drain<I: 'static, C: 'static>(
    initializers: &mut JoinSet<I>,
    critical_tasks: &mut JoinSet<C>,
) {
    initializers.abort_all();
    critical_tasks.abort_all();
    tokio::join!(initializers.shutdown(), critical_tasks.shutdown());
}
