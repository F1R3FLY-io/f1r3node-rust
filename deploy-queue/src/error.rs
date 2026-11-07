use http::StatusCode;

use crate::record::Sig;

/// Why the queue refused a deploy. The kinds and messages follow the node's
/// own deploy errors where the node has one, so a client sees the same
/// refusal from the queue that it would have read from the node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntakeError {
    #[error("{0}")]
    IllegalArgument(String),
    #[error("{0}")]
    RholangBadTerm(String),
    #[error("{0}")]
    DeployExpired(String),
    #[error("Deploy already known: {}", hex::encode(.0))]
    Duplicate(Sig),
    #[error(
        "deploy queue is full: the deploys ahead of this one would not land before its \
         validity window closes; retry in {retry_after_secs}s"
    )]
    QueueFull { retry_after_secs: u64 },
    #[error("deploy queue is not connected to its node; retry in {retry_after_secs}s")]
    NodeUnavailable { retry_after_secs: u64 },
    #[error("deploy queue is shutting down")]
    Stopped,
    #[error("{0}")]
    Store(String),
}

impl IntakeError {
    pub fn status(&self) -> StatusCode {
        match self {
            IntakeError::IllegalArgument(_)
            | IntakeError::RholangBadTerm(_)
            | IntakeError::DeployExpired(_) => StatusCode::BAD_REQUEST,
            IntakeError::Duplicate(_) => StatusCode::CONFLICT,
            IntakeError::QueueFull { .. }
            | IntakeError::NodeUnavailable { .. }
            | IntakeError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
            IntakeError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            IntakeError::IllegalArgument(_) => "illegal_argument",
            IntakeError::RholangBadTerm(_) => "rholang_bad_term",
            IntakeError::DeployExpired(_) => "deploy_expired",
            IntakeError::Duplicate(_) => "duplicate_deploy",
            IntakeError::QueueFull { .. } => "deploy_queue_full",
            IntakeError::NodeUnavailable { .. } => "node_unavailable",
            IntakeError::Stopped => "deploy_queue_stopped",
            IntakeError::Store(_) => "queue_store_error",
        }
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            IntakeError::QueueFull { retry_after_secs }
            | IntakeError::NodeUnavailable { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("deploy queue engine has stopped")]
pub struct EngineStopped;
