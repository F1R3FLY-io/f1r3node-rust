// See casper/src/main/scala/coop/rchain/casper/BlockStatus.scala

use models::rust::block_metadata::{
    AdmissionRejectionReason, CertifiedAdmissionOutcome, CertifiedSenderAuthority,
};
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::history::Either;
use shared::rust::store::key_value_store::KvStoreError;

use super::errors::CasperError;

/// Represents the status of a block in the system
#[derive(Debug, Clone)]
pub enum BlockStatus {
    Valid(ValidBlock),
    Error(BlockError),
}

/// Represents a valid block
#[derive(Debug, Clone, PartialEq)]
pub enum ValidBlock {
    Valid,
}

/// Represents an error with a block
#[derive(Debug, Clone, PartialEq)]
pub enum BlockError {
    Processed,
    CasperIsBusy,
    MissingBlocks,
    BlockException(CasperError),
    Invalid(InvalidBlock),
}

/// Represents an invalid block
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InvalidBlock {
    InvalidFormat,
    InvalidSignature,
    InvalidSender,
    InvalidVersion,
    InvalidTimestamp,

    DeployNotSigned,
    InvalidBlockNumber,
    InvalidRepeatDeploy,
    InvalidParents,
    InvalidFollows,
    InvalidSequenceNumber,
    InvalidShardId,
    JustificationRegression,
    NeglectedInvalidBlock,
    NeglectedEquivocation,
    InvalidTransaction,
    InvalidBondsCache,
    InvalidEquivocationEvidence,
    InvalidBlockHash,
    // UnauthorizedSlashDeploy: a block carries a `Slash` system deploy that
    // fails the authorization predicate (wrong epoch, missing/non-invalid
    // evidence, unbonded offender, duplicate target, or issuer ≠ sender).
    // Raised by `Validate::slash_deploy_authorization`; the rules are in
    // `slashing_authorization.rs::validate_received_slash_deploys` and
    // proven sufficient by Theorem T-9.13 (see
    // `formal/rocq/slashing/theories/BugFixSlashAuthorization.v`).
    UnauthorizedSlashDeploy,
    InvalidRejectedDeploy,
    ContainsExpiredDeploy,
    ContainsTimeExpiredDeploy,
    ContainsFutureDeploy,
    NotOfInterest,
    LowDeployCost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquivocationObservation {
    RequestedDependency,
    Unsolicited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationDisposition {
    Accept,
    ObjectiveInvalid,
    MissingDependency,
    LocalFault,
    AlreadyProcessed,
}

#[derive(Clone, Debug)]
pub enum CertifiedBlockValidation {
    Accepted {
        sender_authority: CertifiedSenderAuthority,
        admission_outcome: CertifiedAdmissionOutcome,
        equivocation_observation: Option<EquivocationObservation>,
    },
    ObjectiveRejected {
        invalid: InvalidBlock,
        sender_authority: CertifiedSenderAuthority,
        admission_outcome: CertifiedAdmissionOutcome,
    },
    UnattributableRejected {
        invalid: InvalidBlock,
    },
    MissingDependency,
    LocalFault(CasperError),
    CasperBusy,
    AlreadyProcessed,
}

impl CertifiedBlockValidation {
    pub fn unattributable(invalid: InvalidBlock) -> Self {
        Self::UnattributableRejected { invalid }
    }

    pub fn local_fault(error: CasperError) -> Self { Self::LocalFault(error) }

    pub fn from_uncertified_error(error: BlockError) -> Result<Self, CasperError> {
        match error {
            BlockError::Invalid(invalid) => Ok(Self::unattributable(invalid)),
            BlockError::MissingBlocks => Ok(Self::MissingDependency),
            BlockError::BlockException(error) => Ok(Self::LocalFault(error)),
            BlockError::CasperIsBusy => Ok(Self::CasperBusy),
            BlockError::Processed => Ok(Self::AlreadyProcessed),
        }
    }

    pub fn certified(
        block: &BlockMessage,
        status: Either<BlockError, ValidBlock>,
        sender_authority: CertifiedSenderAuthority,
    ) -> Result<Self, CasperError> {
        Self::certified_with_observation(block, status, sender_authority, None)
    }

    pub fn certified_with_observation(
        block: &BlockMessage,
        status: Either<BlockError, ValidBlock>,
        sender_authority: CertifiedSenderAuthority,
        equivocation_observation: Option<EquivocationObservation>,
    ) -> Result<Self, CasperError> {
        match status {
            Either::Right(ValidBlock::Valid) => {
                let admission_outcome =
                    CertifiedAdmissionOutcome::accepted(block, &sender_authority)
                        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
                Ok(Self::Accepted {
                    sender_authority,
                    admission_outcome,
                    equivocation_observation,
                })
            }
            Either::Left(BlockError::Invalid(invalid)) => {
                let admission_outcome = CertifiedAdmissionOutcome::rejected(
                    block,
                    &sender_authority,
                    AdmissionRejectionReason::from(&invalid),
                )
                .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
                Ok(Self::ObjectiveRejected {
                    invalid,
                    sender_authority,
                    admission_outcome,
                })
            }
            Either::Left(error) => Self::from_uncertified_error(error),
        }
    }

    pub fn status(&self) -> Either<BlockError, ValidBlock> {
        match self {
            Self::Accepted { .. } => Either::Right(ValidBlock::Valid),
            Self::ObjectiveRejected { invalid, .. } | Self::UnattributableRejected { invalid } => {
                Either::Left(BlockError::Invalid(invalid.clone()))
            }
            Self::MissingDependency => Either::Left(BlockError::MissingBlocks),
            Self::LocalFault(error) => Either::Left(BlockError::BlockException(error.clone())),
            Self::CasperBusy => Either::Left(BlockError::CasperIsBusy),
            Self::AlreadyProcessed => Either::Left(BlockError::Processed),
        }
    }

    pub fn sender_authority(&self) -> Option<&CertifiedSenderAuthority> {
        match self {
            Self::Accepted {
                sender_authority, ..
            }
            | Self::ObjectiveRejected {
                sender_authority, ..
            } => Some(sender_authority),
            _ => None,
        }
    }

    pub fn admission_outcome(&self) -> Option<&CertifiedAdmissionOutcome> {
        match self {
            Self::Accepted {
                admission_outcome, ..
            }
            | Self::ObjectiveRejected {
                admission_outcome, ..
            } => Some(admission_outcome),
            _ => None,
        }
    }

    pub fn equivocation_observation(&self) -> Option<EquivocationObservation> {
        match self {
            Self::Accepted {
                equivocation_observation,
                ..
            } => *equivocation_observation,
            _ => None,
        }
    }
}

impl From<&InvalidBlock> for AdmissionRejectionReason {
    fn from(value: &InvalidBlock) -> Self {
        match value {
            InvalidBlock::InvalidFormat => Self::InvalidFormat,
            InvalidBlock::InvalidSignature => Self::InvalidSignature,
            InvalidBlock::InvalidSender => Self::InvalidSender,
            InvalidBlock::InvalidVersion => Self::InvalidVersion,
            InvalidBlock::InvalidTimestamp => Self::InvalidTimestamp,
            InvalidBlock::DeployNotSigned => Self::DeployNotSigned,
            InvalidBlock::InvalidBlockNumber => Self::InvalidBlockNumber,
            InvalidBlock::InvalidRepeatDeploy => Self::InvalidRepeatDeploy,
            InvalidBlock::InvalidParents => Self::InvalidParents,
            InvalidBlock::InvalidFollows => Self::InvalidFollows,
            InvalidBlock::InvalidSequenceNumber => Self::InvalidSequenceNumber,
            InvalidBlock::InvalidShardId => Self::InvalidShardId,
            InvalidBlock::JustificationRegression => Self::JustificationRegression,
            InvalidBlock::NeglectedInvalidBlock => Self::NeglectedInvalidBlock,
            InvalidBlock::NeglectedEquivocation => Self::NeglectedEquivocation,
            InvalidBlock::InvalidTransaction => Self::InvalidTransaction,
            InvalidBlock::InvalidBondsCache => Self::InvalidBondsCache,
            InvalidBlock::InvalidEquivocationEvidence => Self::InvalidEquivocationEvidence,
            InvalidBlock::InvalidBlockHash => Self::InvalidBlockHash,
            InvalidBlock::UnauthorizedSlashDeploy => Self::UnauthorizedSlashDeploy,
            InvalidBlock::InvalidRejectedDeploy => Self::InvalidRejectedDeploy,
            InvalidBlock::ContainsExpiredDeploy => Self::ContainsExpiredDeploy,
            InvalidBlock::ContainsTimeExpiredDeploy => Self::ContainsTimeExpiredDeploy,
            InvalidBlock::ContainsFutureDeploy => Self::ContainsFutureDeploy,
            InvalidBlock::NotOfInterest => Self::NotOfInterest,
            InvalidBlock::LowDeployCost => Self::LowDeployCost,
        }
    }
}

impl BlockStatus {
    pub fn valid() -> ValidBlock { ValidBlock::Valid }

    pub fn processed() -> BlockError { BlockError::Processed }

    pub fn casper_is_busy() -> BlockError { BlockError::CasperIsBusy }

    pub fn exception(ex: CasperError) -> BlockError { BlockError::BlockException(ex) }

    pub fn missing_blocks() -> BlockError { BlockError::MissingBlocks }

    pub fn invalid_format() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidFormat) }

    pub fn invalid_signature() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidSignature) }

    pub fn invalid_sender() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidSender) }

    pub fn invalid_version() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidVersion) }

    pub fn invalid_timestamp() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidTimestamp) }

    pub fn deploy_not_signed() -> BlockError { BlockError::Invalid(InvalidBlock::DeployNotSigned) }

    pub fn invalid_block_number() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidBlockNumber)
    }

    pub fn invalid_repeat_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidRepeatDeploy)
    }

    pub fn invalid_parents() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidParents) }

    pub fn invalid_follows() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidFollows) }

    pub fn invalid_sequence_number() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidSequenceNumber)
    }

    pub fn invalid_shard_id() -> BlockError { BlockError::Invalid(InvalidBlock::InvalidShardId) }

    pub fn justification_regression() -> BlockError {
        BlockError::Invalid(InvalidBlock::JustificationRegression)
    }

    pub fn neglected_invalid_block() -> BlockError {
        BlockError::Invalid(InvalidBlock::NeglectedInvalidBlock)
    }

    pub fn neglected_equivocation() -> BlockError {
        BlockError::Invalid(InvalidBlock::NeglectedEquivocation)
    }

    pub fn invalid_transaction() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidTransaction)
    }

    pub fn invalid_bonds_cache() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidBondsCache)
    }

    pub fn invalid_block_hash() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidBlockHash)
    }

    pub fn unauthorized_slash_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::UnauthorizedSlashDeploy)
    }

    pub fn invalid_rejected_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::InvalidRejectedDeploy)
    }

    pub fn contains_expired_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::ContainsExpiredDeploy)
    }

    pub fn contains_time_expired_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::ContainsTimeExpiredDeploy)
    }

    pub fn contains_future_deploy() -> BlockError {
        BlockError::Invalid(InvalidBlock::ContainsFutureDeploy)
    }

    pub fn not_of_interest() -> BlockError { BlockError::Invalid(InvalidBlock::NotOfInterest) }

    pub fn low_deploy_cost() -> BlockError { BlockError::Invalid(InvalidBlock::LowDeployCost) }

    pub fn is_in_dag(&self) -> bool {
        match self {
            BlockStatus::Valid(_) => true,
            BlockStatus::Error(BlockError::Invalid(_)) => true,
            _ => false,
        }
    }

    pub fn disposition(&self) -> ValidationDisposition {
        match self {
            BlockStatus::Valid(_) => ValidationDisposition::Accept,
            BlockStatus::Error(BlockError::Invalid(_)) => ValidationDisposition::ObjectiveInvalid,
            BlockStatus::Error(BlockError::MissingBlocks) => {
                ValidationDisposition::MissingDependency
            }
            BlockStatus::Error(BlockError::BlockException(_))
            | BlockStatus::Error(BlockError::CasperIsBusy) => ValidationDisposition::LocalFault,
            BlockStatus::Error(BlockError::Processed) => ValidationDisposition::AlreadyProcessed,
        }
    }
}

impl InvalidBlock {
    pub fn is_slashable(&self) -> bool {
        // Exhaustive match (no catch-all). Adding a new `InvalidBlock`
        // variant without updating this function would silently default
        // to non-slashable under a `_ => false` wildcard; the explicit
        // enumeration forces a compiler error and a deliberate decision
        // about whether the new variant is slashable. This is the
        // future-correctness footgun protection T-9.3 depends on at the
        // dispatcher catch-all.
        match self {
            InvalidBlock::DeployNotSigned
            | InvalidBlock::InvalidBlockNumber
            | InvalidBlock::InvalidRepeatDeploy
            | InvalidBlock::InvalidParents
            | InvalidBlock::InvalidFollows
            | InvalidBlock::InvalidSequenceNumber
            | InvalidBlock::InvalidShardId
            | InvalidBlock::JustificationRegression
            | InvalidBlock::NeglectedInvalidBlock
            | InvalidBlock::NeglectedEquivocation
            | InvalidBlock::InvalidTransaction
            | InvalidBlock::InvalidBondsCache
            | InvalidBlock::InvalidEquivocationEvidence
            | InvalidBlock::UnauthorizedSlashDeploy
            | InvalidBlock::ContainsExpiredDeploy
            | InvalidBlock::ContainsTimeExpiredDeploy
            | InvalidBlock::ContainsFutureDeploy => true,

            // Non-slashable variants — listed explicitly so the compiler
            // catches new additions to the enum. Each represents a failure
            // attributable to the block's wire format or local node state,
            // NOT to Byzantine behavior the network can attribute and slash:
            //   • InvalidFormat/Signature/Sender/Version/Timestamp: malformed
            //     wire data; the sender is not identifiable (Signature) or
            //     the sender's identity can't be verified (Sender).
            //   • InvalidRejectedDeploy: rejected-deploy tracking; not a
            //     consensus offense.
            //   • NotOfInterest: local node filtering decision.
            //   • LowDeployCost: per-deploy cost threshold; rejected at
            //     admission, not on-chain accountable.
            InvalidBlock::InvalidFormat
            | InvalidBlock::InvalidSignature
            | InvalidBlock::InvalidSender
            | InvalidBlock::InvalidVersion
            | InvalidBlock::InvalidTimestamp
            | InvalidBlock::InvalidBlockHash
            | InvalidBlock::InvalidRejectedDeploy
            | InvalidBlock::NotOfInterest
            | InvalidBlock::LowDeployCost => false,
        }
    }
}

impl From<KvStoreError> for BlockError {
    fn from(error: KvStoreError) -> Self { BlockError::BlockException(CasperError::from(error)) }
}

#[cfg(test)]
mod validation_disposition_tests {
    use super::*;

    #[test]
    fn local_faults_and_missing_dependencies_are_never_objective_invalidity() {
        let local = BlockStatus::Error(BlockError::BlockException(CasperError::RuntimeError(
            "unknown local root".to_string(),
        )));
        let missing = BlockStatus::Error(BlockError::MissingBlocks);

        assert_eq!(local.disposition(), ValidationDisposition::LocalFault);
        assert_eq!(
            missing.disposition(),
            ValidationDisposition::MissingDependency
        );
        assert!(!local.is_in_dag());
        assert!(!missing.is_in_dag());
    }

    #[test]
    fn only_explicit_invalidity_is_objective() {
        let invalid = BlockStatus::Error(BlockError::Invalid(InvalidBlock::InvalidTransaction));

        assert_eq!(
            invalid.disposition(),
            ValidationDisposition::ObjectiveInvalid
        );
        assert!(invalid.is_in_dag());
    }
}
