use std::sync::Arc;

use futures::future::BoxFuture;

use crate::rust::errors::CasperError;

pub type BackgroundTaskSpawner = Arc<
    dyn Fn(&'static str, BoxFuture<'static, Result<(), CasperError>>) -> Result<(), CasperError>
        + Send
        + Sync,
>;

pub fn spawn(
    spawner: &Option<BackgroundTaskSpawner>,
    name: &'static str,
    task: BoxFuture<'static, Result<(), CasperError>>,
) -> Result<(), CasperError> {
    match spawner {
        Some(spawner) => spawner(name, task),
        None => {
            tokio::spawn(async move {
                if let Err(error) = task.await {
                    tracing::error!(task = name, %error, "Background task failed");
                }
            });
            Ok(())
        }
    }
}
