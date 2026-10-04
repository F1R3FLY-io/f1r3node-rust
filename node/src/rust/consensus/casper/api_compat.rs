use std::sync::Arc;

use casper::rust::api::block_report_api::BlockReportAPI;
use consensus_api::ConsensusError;
use consensus_runtime::ConsensusHandle;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;

use crate::rust::api::admin_web_api::AdminWebApi;
use crate::rust::api::web_api::WebApi;
use crate::rust::runtime::api_servers::APIServers;
use crate::rust::web::reporting_routes::ReportingHttpRoutes;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ApiFailure {
    pub status: axum::http::StatusCode,
    pub kind: &'static str,
    pub message: String,
}

pub struct PreparedApplication {
    pub api_servers: APIServers,
    pub(crate) reporting_routes: ReportingHttpRoutes,
    pub web_api: Arc<dyn WebApi + Send + Sync>,
    pub admin_web_api: Arc<dyn AdminWebApi + Send + Sync>,
    pub(crate) block_report_api: Arc<BlockReportAPI>,
}

pub async fn submit(handle: &ConsensusHandle, deploy: Signed<DeployData>) -> eyre::Result<String> {
    handle
        .submit(DeployData::to_proto(deploy).encode_to_vec())
        .await
        .map_err(|error| match error {
            ConsensusError::UnsupportedCapability(_) => {
                eyre::Report::new(casper::rust::api::block_api::DeployValidationError {
                    message: "Deploy was rejected because node is running in read-only mode."
                        .into(),
                })
            }
            ConsensusError::NotReady => {
                eyre::eyre!("Error: Could not deploy, casper instance was not available yet.")
            }
            error => restore_error(error),
        })
}

pub async fn propose(handle: &ConsensusHandle, is_async: bool) -> eyre::Result<String> {
    handle.propose(is_async).await.map_err(|error| match error {
        ConsensusError::UnsupportedCapability(_) => {
            eyre::Report::new(casper::rust::api::block_api::ProposeReadOnlyError)
        }
        ConsensusError::NotReady => {
            eyre::eyre!("Failure: casper instance is not available.")
        }
        error => restore_error(error),
    })
}

pub fn command_error(error: eyre::Report) -> ConsensusError {
    use casper::rust::api::block_api::{DeployValidationError, NoNewDeploysError};
    use casper::rust::casper::DeployError;
    let message = error.to_string();
    for cause in error.chain() {
        if let Some(DeployError::DuplicateDeploy(id)) = cause.downcast_ref::<DeployError>() {
            return ConsensusError::Rejected {
                code: "casper.duplicate-deploy".into(),
                message,
                detail: id.to_vec(),
            };
        }
        if cause.downcast_ref::<DeployValidationError>().is_some() {
            return ConsensusError::Rejected {
                code: "casper.deploy-validation".into(),
                message,
                detail: vec![],
            };
        }
        if cause.downcast_ref::<NoNewDeploysError>().is_some() {
            return ConsensusError::Rejected {
                code: "casper.no-new-deploys".into(),
                message,
                detail: vec![],
            };
        }
    }
    let (status, code, message) = crate::rust::web::shared_handlers::classify_error(&error);
    ConsensusError::Rejected {
        code,
        message,
        detail: status.as_u16().to_le_bytes().to_vec(),
    }
}

fn restore_error(error: ConsensusError) -> eyre::Report {
    use casper::rust::api::block_api::{DeployValidationError, NoNewDeploysError};
    use casper::rust::casper::DeployError;
    match error {
        ConsensusError::Rejected {
            code,
            message,
            detail,
        } => match code {
            "casper.duplicate-deploy" => {
                eyre::Report::new(DeployError::DuplicateDeploy(detail.into()))
            }
            "casper.deploy-validation" => eyre::Report::new(DeployValidationError { message }),
            "casper.no-new-deploys" => eyre::Report::new(NoNewDeploysError),
            _ => {
                let status = detail
                    .as_slice()
                    .try_into()
                    .ok()
                    .map(u16::from_le_bytes)
                    .and_then(|status| axum::http::StatusCode::from_u16(status).ok())
                    .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
                eyre::Report::new(ApiFailure {
                    status,
                    kind: code,
                    message,
                })
            }
        },
        error => eyre::eyre!(error),
    }
}

#[cfg(test)]
mod tests {
    use casper::rust::api::block_api::{DeployValidationError, NoNewDeploysError};
    use casper::rust::casper::DeployError;

    use super::*;

    #[test]
    fn typed_api_errors_survive_the_runtime_boundary() {
        let duplicate = restore_error(command_error(
            DeployError::DuplicateDeploy(vec![1; 64].into()).into(),
        ));
        assert!(matches!(
            duplicate.downcast_ref::<DeployError>(),
            Some(DeployError::DuplicateDeploy(_))
        ));
        let validation = restore_error(command_error(
            DeployValidationError {
                message: "wrong shard".into(),
            }
            .into(),
        ));
        assert!(validation.downcast_ref::<DeployValidationError>().is_some());
        let empty = restore_error(command_error(NoNewDeploysError.into()));
        assert!(empty.downcast_ref::<NoNewDeploysError>().is_some());
        let native = casper::rust::errors::CasperError::RuntimeError("runtime failed".into());
        let bridged = restore_error(command_error(native.into()));
        let (status, kind, _) = crate::rust::web::shared_handlers::classify_error(&bridged);
        assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(kind, "runtime_error");
    }
}
