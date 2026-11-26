use http::StatusCode;

use crate::record::Sig;

/// What holds deploys back when the queue cannot fit one more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Congestion {
    /// The validator takes fewer deploys than arrive.
    Validator,
    /// The validator created no block over a citable depth while the shard
    /// moved on: it takes no deploys until it creates blocks again.
    ValidatorIdle,
    /// The shard's finality trails the tip or has stalled: the node holds its
    /// validator back, or the queue holds release.
    Shard,
    /// The signed deploys the queue holds fill its storage budget.
    Storage,
}

impl std::fmt::Display for Congestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Congestion::Validator => "the validator is taking fewer deploys than arrive",
            Congestion::ValidatorIdle => {
                "the validator has created no block while the shard moved on"
            }
            Congestion::Shard => "the shard's finality is holding the validator back",
            Congestion::Storage => "the signed deploys the queue holds fill its storage budget",
        })
    }
}

/// Why the queue refused a deploy. The kinds and messages follow the node's
/// own deploy errors where the node has one, so a client sees the same
/// refusal from the queue that it would have read from the node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntakeError {
    #[error("{0}")]
    IllegalArgument(String),
    #[error("deploy_expired: {0}")]
    DeployExpired(String),
    #[error("invalid_deploy_window: {0}")]
    InvalidDeployWindow(String),
    #[error("Deploy already known: {}", hex::encode(.0))]
    Duplicate(Sig),
    #[error(
        "deploy queue is full: {cause}, so the deploys ahead of this one would not land \
         before its validity window closes; sign it again with a current \
         validAfterBlockNumber and retry in {retry_after_secs}s"
    )]
    QueueFull {
        retry_after_secs: u64,
        cause: Congestion,
    },
    #[error(
        "deploy queue is full for this deployer: it holds {held} of the {limit} deploys one \
         deployer may hold, so that other deployers keep room; retry in {retry_after_secs}s"
    )]
    DeployerShareFull {
        held: usize,
        limit: usize,
        retry_after_secs: u64,
    },
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
            | IntakeError::DeployExpired(_)
            | IntakeError::InvalidDeployWindow(_) => StatusCode::BAD_REQUEST,
            IntakeError::Duplicate(_) => StatusCode::CONFLICT,
            IntakeError::QueueFull { .. }
            | IntakeError::DeployerShareFull { .. }
            | IntakeError::NodeUnavailable { .. }
            | IntakeError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
            IntakeError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            IntakeError::IllegalArgument(_) => "illegal_argument",
            IntakeError::DeployExpired(_) => "deploy_expired",
            IntakeError::InvalidDeployWindow(_) => "invalid_deploy_window",
            IntakeError::Duplicate(_) => "duplicate_deploy",
            IntakeError::QueueFull { .. } | IntakeError::DeployerShareFull { .. } => {
                "deploy_queue_full"
            }
            IntakeError::NodeUnavailable { .. } => "node_unavailable",
            IntakeError::Stopped => "deploy_queue_stopped",
            IntakeError::Store(_) => "queue_store_error",
        }
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            IntakeError::QueueFull {
                retry_after_secs, ..
            }
            | IntakeError::DeployerShareFull {
                retry_after_secs, ..
            }
            | IntakeError::NodeUnavailable { retry_after_secs } => Some(*retry_after_secs),
            _ => None,
        }
    }

    pub fn cause(&self) -> Option<&'static str> {
        match self {
            IntakeError::QueueFull { cause, .. } => Some(match cause {
                Congestion::Validator => "validator",
                Congestion::ValidatorIdle => "validator_idle",
                Congestion::Shard => "shard",
                Congestion::Storage => "storage",
            }),
            IntakeError::DeployerShareFull { .. } => Some("deployer_share"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("deploy queue engine has stopped")]
pub struct EngineStopped;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_queue_names_its_cause_and_keeps_its_code_and_wait() {
        let full = |cause| IntakeError::QueueFull {
            retry_after_secs: 12,
            cause,
        };
        let refusals = [
            (full(Congestion::Validator), "validator"),
            (full(Congestion::ValidatorIdle), "validator_idle"),
            (full(Congestion::Shard), "shard"),
            (full(Congestion::Storage), "storage"),
            (
                IntakeError::DeployerShareFull {
                    held: 12,
                    limit: 12,
                    retry_after_secs: 12,
                },
                "deployer_share",
            ),
        ];
        for (refusal, cause) in refusals {
            assert_eq!(
                (
                    refusal.status(),
                    refusal.kind(),
                    refusal.retry_after_secs(),
                    refusal.cause()
                ),
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "deploy_queue_full",
                    Some(12),
                    Some(cause)
                ),
            );
        }
    }

    #[test]
    fn other_refusals_name_no_cause() {
        let refusals = [
            IntakeError::IllegalArgument("bad shard".to_string()),
            IntakeError::DeployExpired("too old".to_string()),
            IntakeError::InvalidDeployWindow("too far ahead".to_string()),
            IntakeError::Duplicate(vec![7; 64]),
            IntakeError::NodeUnavailable {
                retry_after_secs: 1,
            },
            IntakeError::Stopped,
            IntakeError::Store("disk full".to_string()),
        ];
        for refusal in refusals {
            assert_eq!(refusal.cause(), None, "{}", refusal.kind());
        }
    }
}
