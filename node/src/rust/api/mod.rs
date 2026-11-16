pub mod admin_web_api;
pub mod deploy_grpc_service_v1;
pub mod grpc_package;
pub mod lsp_grpc_service;
pub mod propose_grpc_service_v1;
pub mod repl_grpc_service;
pub mod serde_types;
pub mod web_api;

use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Default)]
pub(crate) struct DeployRequestLimiter {
    slots: Option<Arc<Semaphore>>,
}

#[derive(Clone)]
pub(crate) struct DeployRequestPermit {
    _slot: Option<Arc<OwnedSemaphorePermit>>,
}

#[derive(Debug, thiserror::Error)]
#[error("deploy_admission_busy: deploy submission concurrency limit reached. Retry later.")]
pub(crate) struct DeployIngressBusy;

impl DeployRequestLimiter {
    pub(crate) fn new(limit: Option<usize>) -> eyre::Result<Self> {
        if limit.is_some_and(|limit| limit == 0 || limit > Semaphore::MAX_PERMITS) {
            eyre::bail!(
                "casper.deploy-admission.max-inflight must be between 1 and {}",
                Semaphore::MAX_PERMITS
            );
        }
        Ok(Self {
            slots: limit.map(|limit| Arc::new(Semaphore::new(limit))),
        })
    }

    pub(crate) fn try_acquire(&self) -> Result<DeployRequestPermit, DeployIngressBusy> {
        let slot = self
            .slots
            .as_ref()
            .map(|slots| {
                slots
                    .clone()
                    .try_acquire_owned()
                    .map(Arc::new)
                    .map_err(|_| DeployIngressBusy)
            })
            .transpose()?;
        Ok(DeployRequestPermit { _slot: slot })
    }
}

#[cfg(test)]
mod deploy_ingress_tests {
    use super::*;
    use crate::rust::web::shared_handlers::offload;

    #[test]
    fn inflight_configuration_rejects_zero_and_unsupported_limits() {
        assert!(DeployRequestLimiter::new(Some(0)).is_err());
        assert!(DeployRequestLimiter::new(Some(Semaphore::MAX_PERMITS + 1)).is_err());
        assert!(DeployRequestLimiter::new(Some(1)).is_ok());
    }

    #[test]
    fn cloned_permits_share_one_slot_until_the_last_owner_drops() {
        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let permit = limiter.try_acquire().unwrap();
        let work = permit.clone();
        drop(permit);
        assert!(limiter.clone().try_acquire().is_err());
        drop(work);
        assert!(limiter.try_acquire().is_ok());
    }

    #[test]
    fn absent_configuration_preserves_unlimited_submission() {
        let limiter = DeployRequestLimiter::new(None).unwrap();
        let permits: Vec<_> = (0..1024).map(|_| limiter.try_acquire().unwrap()).collect();
        assert_eq!(permits.len(), 1024);
    }

    #[tokio::test]
    async fn canceled_submission_keeps_its_slot_until_blocking_work_finishes() {
        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let permit = limiter.try_acquire().unwrap();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let (finish, finish_rx) = std::sync::mpsc::channel();
        let (finished, finished_rx) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(offload(move || async move {
            started.send(()).unwrap();
            finish_rx.recv().unwrap();
            drop(permit);
            finished.send(()).unwrap();
            Ok(())
        }));
        started_rx.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert!(limiter.try_acquire().is_err());
        finish.send(()).unwrap();
        finished_rx.await.unwrap();
        assert!(limiter.try_acquire().is_ok());
    }

    #[tokio::test]
    async fn blocking_unwind_releases_the_submission_slot() {
        let limiter = DeployRequestLimiter::new(Some(1)).unwrap();
        let permit = limiter.try_acquire().unwrap();
        let result: eyre::Result<()> = offload(move || async move {
            let _permit = permit;
            panic!("submission test panic");
        })
        .await;
        assert!(result.is_err());
        assert!(limiter.try_acquire().is_ok());
    }
}
