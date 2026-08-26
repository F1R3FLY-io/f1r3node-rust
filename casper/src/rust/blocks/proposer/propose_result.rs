// See casper/src/main/scala/coop/rchain/casper/blocks/proposer/ProposeResult.scala

use std::fmt;

use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use uuid::Uuid;

use crate::rust::block_status::ValidBlock;

/// Propose ID type
pub type ProposeID = Uuid;

/// Result of a block proposal attempt
#[derive(Debug, Clone)]
pub struct ProposeResult {
    pub propose_status: ProposeStatus,
}

/// Status of a block proposal
#[derive(Debug, Clone)]
pub enum ProposeStatus {
    Success(ProposeSuccess),
    Failure(ProposeFailure),
}

/// Successful proposal
#[derive(Debug, Clone)]
pub struct ProposeSuccess {
    pub result: ValidBlock,
}

/// Failed proposal
#[derive(Debug, Clone)]
pub enum ProposeFailure {
    NoNewDeploys,
    RecoveryDeferred(RecoveryDeferralReason),
    InternalDeployError,
    BugError,
    CheckConstraintsFailure(CheckProposeConstraintsFailure),
}

/// Check constraints result
#[derive(Debug, Clone)]
pub enum CheckProposeConstraintsResult {
    Success,
    Failure(CheckProposeConstraintsFailure),
}

/// Constraints check failure
#[derive(Debug, Clone)]
pub enum CheckProposeConstraintsFailure {
    NotBonded,
    NotEnoughNewBlocks,
    TooFarAheadOfLastFinalized,
}

/// Block creator result
#[derive(Debug, Clone)]
pub enum BlockCreatorResult {
    NoNewDeploys,
    RecoveryDeferred(RecoveryDeferralReason),
    /// The created block together with the pre- and post-state hashes that were computed
    /// during `compute_deploys_checkpoint`. Carrying these hashes avoids re-running the
    /// expensive checkpoint replay during self-validation.
    Created(BlockMessage, Bytes, Bytes),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDeferralReason {
    FinalizedFloorMaterializationPending,
    CandidateFloorRegression,
    CandidateFloorConflict,
    CertifiedContextMismatch,
    IncompleteCandidateCommitteeSlots,
    InactiveCandidateValidator,
    StaleRecoveryPermit,
}

impl CheckProposeConstraintsResult {
    pub fn success() -> Self { CheckProposeConstraintsResult::Success }

    pub fn not_bonded() -> Self {
        CheckProposeConstraintsResult::Failure(CheckProposeConstraintsFailure::NotBonded)
    }

    pub fn not_enough_new_block() -> Self {
        CheckProposeConstraintsResult::Failure(CheckProposeConstraintsFailure::NotEnoughNewBlocks)
    }

    pub fn too_far_ahead_of_last_finalized() -> Self {
        CheckProposeConstraintsResult::Failure(
            CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized,
        )
    }
}

impl ProposeResult {
    pub fn no_new_deploys() -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(ProposeFailure::NoNewDeploys),
        }
    }

    pub fn internal_deploy_error() -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(ProposeFailure::InternalDeployError),
        }
    }

    pub fn not_bonded() -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                CheckProposeConstraintsFailure::NotBonded,
            )),
        }
    }

    pub fn not_enough_blocks() -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                CheckProposeConstraintsFailure::NotEnoughNewBlocks,
            )),
        }
    }

    pub fn too_far_ahead_of_last_finalized() -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized,
            )),
        }
    }

    pub fn success(status: ValidBlock) -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Success(ProposeSuccess { result: status }),
        }
    }

    pub fn failure(status: ProposeFailure) -> Self {
        ProposeResult {
            propose_status: ProposeStatus::Failure(status),
        }
    }

    pub fn is_no_new_deploys(&self) -> bool {
        matches!(
            self.propose_status,
            ProposeStatus::Failure(ProposeFailure::NoNewDeploys)
        )
    }

    pub fn is_recovery_deferred(&self) -> bool {
        matches!(
            self.propose_status,
            ProposeStatus::Failure(ProposeFailure::RecoveryDeferred(_))
        )
    }
}

impl BlockCreatorResult {
    pub fn no_new_deploys() -> Self { BlockCreatorResult::NoNewDeploys }

    pub fn recovery_deferred(reason: RecoveryDeferralReason) -> Self {
        BlockCreatorResult::RecoveryDeferred(reason)
    }

    pub fn created(b: BlockMessage, pre_state_hash: Bytes, post_state_hash: Bytes) -> Self {
        BlockCreatorResult::Created(b, pre_state_hash, post_state_hash)
    }
}

impl fmt::Display for ProposeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposeStatus::Success(r) => write!(f, "Propose succeed: {:?}", r.result),
            ProposeStatus::Failure(failure) => match failure {
                ProposeFailure::NoNewDeploys => write!(f, "Proposal failed: NoNewDeploys. No unprocessed deploys in pool. If you just deployed, the deploy may have already been included by the auto-proposer."),
                ProposeFailure::RecoveryDeferred(reason) => {
                    write!(f, "Proposal deferred: {}", reason)
                }
                ProposeFailure::InternalDeployError => {
                    write!(f, "Proposal failed: internal deploy error")
                }
                ProposeFailure::BugError => write!(f, "Proposal failed: BugError"),
                ProposeFailure::CheckConstraintsFailure(check_failure) => match check_failure {
                    CheckProposeConstraintsFailure::NotBonded => {
                        write!(f, "Proposal failed: validator is not bonded")
                    }
                    CheckProposeConstraintsFailure::NotEnoughNewBlocks => {
                        write!(
                            f,
                            "Proposal failed: Must wait for more blocks from other validators"
                        )
                    }
                    CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized => {
                        write!(
                            f,
                            "Proposal failed: too far ahead of the last finalized block"
                        )
                    }
                },
            },
        }
    }
}

impl fmt::Display for RecoveryDeferralReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryDeferralReason::FinalizedFloorMaterializationPending => {
                write!(f, "certified finalized floor is not materialized yet")
            }
            RecoveryDeferralReason::CandidateFloorRegression => {
                write!(
                    f,
                    "candidate finalized floor regresses the materialized floor"
                )
            }
            RecoveryDeferralReason::CandidateFloorConflict => {
                write!(
                    f,
                    "candidate finalized floor conflicts with the materialized floor"
                )
            }
            RecoveryDeferralReason::CertifiedContextMismatch => {
                write!(
                    f,
                    "candidate and materialized certified contexts disagree at one floor"
                )
            }
            RecoveryDeferralReason::IncompleteCandidateCommitteeSlots => {
                write!(f, "candidate committee latest-message slots are incomplete")
            }
            RecoveryDeferralReason::InactiveCandidateValidator => {
                write!(f, "proposer is inactive in the candidate committee")
            }
            RecoveryDeferralReason::StaleRecoveryPermit => {
                write!(f, "finality-recovery permit is stale")
            }
        }
    }
}

impl RecoveryDeferralReason {
    pub fn requires_finalization_request(self) -> bool {
        self == RecoveryDeferralReason::FinalizedFloorMaterializationPending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_floor_materialization_deferral_requests_finalization() {
        assert!(RecoveryDeferralReason::FinalizedFloorMaterializationPending
            .requires_finalization_request());
        assert!(!RecoveryDeferralReason::CandidateFloorRegression.requires_finalization_request());
        assert!(!RecoveryDeferralReason::CandidateFloorConflict.requires_finalization_request());
        assert!(!RecoveryDeferralReason::CertifiedContextMismatch.requires_finalization_request());
        assert!(!RecoveryDeferralReason::IncompleteCandidateCommitteeSlots
            .requires_finalization_request());
        assert!(!RecoveryDeferralReason::InactiveCandidateValidator.requires_finalization_request());
        assert!(!RecoveryDeferralReason::StaleRecoveryPermit.requires_finalization_request());
    }
}
