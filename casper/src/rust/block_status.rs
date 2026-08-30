// See casper/src/main/scala/coop/rchain/casper/BlockStatus.scala

use models::rust::block_hash::BlockHash;
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
    /// Dependencies were not ready when the block arrived — established before
    /// validation, and the block is already buffered against them.
    MissingBlocks,
    /// Validation could not reach a verdict: it needed the named block and this
    /// node does not hold it. NOT a judgement of the block — a node restored
    /// from a sync anchor legitimately lacks history its peers have, and the
    /// only correct response is to fetch the named block and try again.
    Undecidable(BlockHash),
    /// Flow signal, not a verdict: the block is settled history below this
    /// node's sync anchor and was admitted hash-checked and unjudged — the
    /// same door LFS restore used. See `block_processor::admit_as_settled`.
    AdmittedSettled,
    /// Validation could not reach a verdict: replay needed the named state
    /// root and this node does not hold it. The state twin of
    /// [`BlockError::Undecidable`] — a statement about this node's sync,
    /// never about the block — and the response is the same: fetch the
    /// artifact (via the state requester) and try again.
    AwaitingState(rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash),
    BlockException(CasperError),
    Invalid(InvalidBlock),
}

impl BlockError {
    /// Classify an error raised during validation.
    ///
    /// `BlockException` is converted to `InvalidTransaction` downstream, which
    /// `is_slashable`, so anything folded into it becomes evidence against the
    /// block's proposer. A block this node does not hold says nothing about the
    /// proposer and must stay distinguishable.
    pub fn from_validation_error(error: CasperError) -> Self {
        use rholang::rust::interpreter::errors::InterpreterError;
        use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RootError};

        match error {
            CasperError::BlockNotHeld(hash) => BlockError::Undecidable(hash),
            // The state twin: a replay that needed a root this node never
            // fetched. The chain is fully typed from rspace up, so absence
            // keeps its name without a string search.
            CasperError::InterpreterError(InterpreterError::RSpaceError(
                RSpaceError::HistoryError(HistoryError::RootError(RootError::RootNotFound(root))),
            )) => BlockError::AwaitingState(root),
            other => BlockError::BlockException(other),
        }
    }
}

/// Represents an invalid block
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InvalidBlock {
    // AdmissibleEquivocation are blocks that would create an equivocation but are
    // pulled in through a justification of another block
    AdmissibleEquivocation,
    // IgnorableEquivocation: an equivocating block we observe via someone
    // else's justification but did not pull in as a dependency. Slashable —
    // the dispatcher mints an EquivocationRecord so the proposer can issue a
    // SlashDeploy. See docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.1.
    IgnorableEquivocation,

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
    // PrematureDeployRetry: a rejected sig re-included before its latest
    // kept rejection settled into the block's frozen floor closure.
    // Raised by `Validate::repeat_deploy`'s gated recovery exemption.
    PrematureDeployRetry,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationDeferral {
    AlreadyBuffered,
    AwaitingBlock(BlockHash),
    AwaitingState(rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash),
}

impl ValidationDeferral {
    pub fn status(&self) -> BlockError {
        match self {
            Self::AlreadyBuffered => BlockError::MissingBlocks,
            Self::AwaitingBlock(hash) => BlockError::Undecidable(hash.clone()),
            Self::AwaitingState(root) => BlockError::AwaitingState(root.clone()),
        }
    }
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
    MissingDependency(ValidationDeferral),
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
            BlockError::MissingBlocks => {
                Ok(Self::MissingDependency(ValidationDeferral::AlreadyBuffered))
            }
            BlockError::Undecidable(hash) => Ok(Self::MissingDependency(
                ValidationDeferral::AwaitingBlock(hash),
            )),
            BlockError::AwaitingState(root) => Ok(Self::MissingDependency(
                ValidationDeferral::AwaitingState(root),
            )),
            BlockError::AdmittedSettled => Ok(Self::AlreadyProcessed),
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
            Self::MissingDependency(deferral) => Either::Left(deferral.status()),
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
            InvalidBlock::AdmissibleEquivocation => Self::AdmissibleEquivocation,
            InvalidBlock::IgnorableEquivocation => Self::IgnorableEquivocation,
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
            InvalidBlock::PrematureDeployRetry => Self::PrematureDeployRetry,
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

    pub fn premature_deploy_retry() -> BlockError {
        BlockError::Invalid(InvalidBlock::PrematureDeployRetry)
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
            BlockStatus::Error(BlockError::Undecidable(_))
            | BlockStatus::Error(BlockError::AwaitingState(_)) => {
                ValidationDisposition::MissingDependency
            }
            BlockStatus::Error(BlockError::AdmittedSettled) => {
                ValidationDisposition::AlreadyProcessed
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
        // Slash evidence demands a fault every honest node attributes
        // identically from the signed block alone. Equivocation is the one
        // verdict with that property: two signed blocks at the same seq are
        // proof wherever they are examined, in any order. Every other verdict
        // is judged against the receiver's own state — its invalid records,
        // its equivocation tracker, its parents' replay, its admission order
        // — so two honest nodes can disagree on it, and minting evidence
        // from it lets whoever shapes message delivery burn honest stake
        // (CI run 32588262605: JustificationRegression verdicts issued by
        // one mid-catch-up node, UnauthorizedSlashDeploy verdicts on the
        // resulting carriers, recursive evidence, FT −18.55 of honest
        // weight). A demoted verdict still drops the block — invalidity is
        // untouched; only the economic layer narrows to provable faults.
        match self {
            // IgnorableEquivocation is slashable per Bug #1 (§9.1). On
            // dev this variant was a known DOS-vector TODO — equivocations
            // observed via someone else's justification produced no on-chain
            // evidence. The dispatcher (`engine::multi_parent_casper::handle_*`)
            // mints an EquivocationRecord whenever this branch fires.
            InvalidBlock::AdmissibleEquivocation | InvalidBlock::IgnorableEquivocation => true,

            // Non-slashable variants — listed explicitly so the compiler
            // catches new additions to the enum, forcing a deliberate
            // decision instead of a wildcard default.
            //   • InvalidFormat/Signature/Sender/Version/Timestamp: malformed
            //     wire data; the sender is not identifiable (Signature) or
            //     the sender's identity can't be verified (Sender).
            //   • JustificationRegression / UnauthorizedSlashDeploy /
            //     NeglectedInvalidBlock / NeglectedEquivocation: judged
            //     against the receiver's own records or tracker — the
            //     view-relative family observed diverging across honest
            //     nodes in the run above.
            //   • InvalidTransaction / InvalidBondsCache / InvalidParents /
            //     InvalidFollows / InvalidRepeatDeploy / InvalidBlockNumber /
            //     InvalidSequenceNumber / InvalidShardId / InvalidBlockHash /
            //     DeployNotSigned / ContainsExpiredDeploy /
            //     ContainsTimeExpiredDeploy / ContainsFutureDeploy: judged
            //     against local replay state or dependency availability;
            //     several are admission-order-sensitive in practice.
            //     Individually provable ones can be promoted onto the
            //     slashable list once their checks are shown
            //     admission-order-free — demotion costs only economics,
            //     never validity.
            //   • InvalidRejectedDeploy: rejected-deploy tracking; not a
            //     consensus offense.
            //   • PrematureDeployRetry: a retry ahead of the gate. The gate
            //     is a pure function of the block, so every honest node
            //     declines the block identically — admission does all the
            //     enforcement, and a gate rule in its proving phase must
            //     never be able to burn honest stake through its own bugs.
            //   • NotOfInterest: local node filtering decision.
            //   • LowDeployCost: per-deploy cost threshold; rejected at
            //     admission, not on-chain accountable.
            InvalidBlock::InvalidFormat
            | InvalidBlock::InvalidSignature
            | InvalidBlock::InvalidSender
            | InvalidBlock::InvalidVersion
            | InvalidBlock::InvalidTimestamp
            | InvalidBlock::DeployNotSigned
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
            | InvalidBlock::InvalidBlockHash
            | InvalidBlock::UnauthorizedSlashDeploy
            | InvalidBlock::ContainsExpiredDeploy
            | InvalidBlock::ContainsTimeExpiredDeploy
            | InvalidBlock::ContainsFutureDeploy
            | InvalidBlock::InvalidRejectedDeploy
            | InvalidBlock::PrematureDeployRetry
            | InvalidBlock::NotOfInterest
            | InvalidBlock::LowDeployCost => false,
        }
    }
}

/// A store error becoming a block verdict goes through the classifier like any
/// other. `CasperError::from` already turns a missing block into `BlockNotHeld`,
/// and wrapping that in `BlockException` directly — as this did — hands an
/// `InvalidTransaction` record to the proposer of a block this node simply
/// cannot read.
impl From<KvStoreError> for BlockError {
    fn from(error: KvStoreError) -> Self {
        BlockError::from_validation_error(CasperError::from(error))
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_hash::BlockHash;

    use super::*;

    /// Validation raises for two unrelated reasons, and only one of them is a
    /// statement about the block. A storage failure means this node is broken;
    /// a block it does not hold means its history is short, which is the normal
    /// condition of a node restored from a sync anchor. They must not share an
    /// outcome: `BlockException` is converted to `InvalidTransaction`, which is
    /// slashable, so folding the second into it mints evidence against an
    /// honest proposer for history this node never had.
    #[test]
    fn a_block_not_held_is_undecidable_not_an_exception() {
        let missing = BlockHash::from(b"not-held".to_vec());

        let undecidable =
            BlockError::from_validation_error(CasperError::BlockNotHeld(missing.clone()));
        assert_eq!(
            undecidable,
            BlockError::Undecidable(missing),
            "a block this node does not hold must carry through as Undecidable, naming \
             the block so the caller can fetch it"
        );

        let broken = BlockError::from_validation_error(CasperError::RuntimeError("disk".into()));
        assert!(
            matches!(broken, BlockError::BlockException(_)),
            "every other failure is still an exception; this must not become a \
             catch-all that swallows real storage faults"
        );
    }

    /// State absence is the second artifact class, and it must classify like
    /// the first. A replay that needs a root this node never fetched is a
    /// statement about this node's sync, not about the block: the block's
    /// parent was admitted as settled history with its bytes but not its
    /// state, every other node replays the same block cleanly, and the verdict
    /// this used to produce — InvalidTransaction, slashable — seeded a
    /// NeglectedInvalidBlock cascade that condemned ninety-one honest blocks
    /// from four false seeds. The chain arrives fully typed from rspace, so
    /// the classification is a match, not a string search.
    #[test]
    fn a_missing_state_root_is_awaiting_state_not_a_verdict() {
        use rholang::rust::interpreter::errors::InterpreterError;
        use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError, RootError};
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

        let root = Blake2b256Hash::from_bytes(vec![0xAB; 32]);
        let chain = CasperError::InterpreterError(InterpreterError::RSpaceError(
            RSpaceError::HistoryError(HistoryError::RootError(RootError::RootNotFound(
                root.clone(),
            ))),
        ));

        assert_eq!(
            BlockError::from_validation_error(chain),
            BlockError::AwaitingState(root),
            "a root this node does not hold must classify as the absence of a verdict, \
             naming the root so the state requester can fetch it"
        );

        let prose = CasperError::InterpreterError(InterpreterError::RSpaceError(
            RSpaceError::HistoryError(HistoryError::RootError(RootError::UnknownRootError(
                "no root found".to_string(),
            ))),
        ));
        assert!(
            matches!(
                BlockError::from_validation_error(prose),
                BlockError::BlockException(_)
            ),
            "the prose variant reports storewide conditions, not a fetchable root, and \
             stays in the exception class"
        );
    }

    #[test]
    fn certified_deferrals_preserve_the_named_artifact() {
        use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

        let missing = BlockHash::from(vec![0x41; 32]);
        let block = CertifiedBlockValidation::from_uncertified_error(BlockError::Undecidable(
            missing.clone(),
        ))
        .expect("block deferral");
        assert_eq!(
            block.status(),
            Either::Left(BlockError::Undecidable(missing))
        );

        let root = Blake2b256Hash::from_bytes(vec![0x42; 32]);
        let state = CertifiedBlockValidation::from_uncertified_error(BlockError::AwaitingState(
            root.clone(),
        ))
        .expect("state deferral");
        assert_eq!(
            state.status(),
            Either::Left(BlockError::AwaitingState(root))
        );

        let buffered = CertifiedBlockValidation::from_uncertified_error(BlockError::MissingBlocks)
            .expect("buffered deferral");
        assert_eq!(buffered.status(), Either::Left(BlockError::MissingBlocks));
    }
}

#[cfg(test)]
mod floor_data_tests {
    use models::rust::block_hash::BlockHash;

    use super::*;

    /// A floor derivation that dies on a block this node does not hold is an
    /// availability event, never block invalidity: the store error carries
    /// its name through `CasperError::BlockNotHeld` to `Undecidable`, so the
    /// caller defers and fetches instead of recording a verdict.
    #[test]
    fn missing_floor_data_is_not_block_invalidity() {
        let missing = BlockHash::from(vec![0xAB; 32]);
        let status = BlockError::from(KvStoreError::MissingBlock {
            hash: missing.clone(),
            context: "floor derivation".to_string(),
        });

        assert_eq!(status, BlockError::Undecidable(missing));
        assert!(!matches!(status, BlockError::Invalid(_)));
    }
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
