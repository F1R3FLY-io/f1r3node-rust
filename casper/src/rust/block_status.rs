// See casper/src/main/scala/coop/rchain/casper/BlockStatus.scala

use models::rust::block_hash::BlockHash;
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

impl BlockStatus {
    pub fn valid() -> ValidBlock { ValidBlock::Valid }

    pub fn processed() -> BlockError { BlockError::Processed }

    pub fn casper_is_busy() -> BlockError { BlockError::CasperIsBusy }

    pub fn missing_blocks() -> BlockError { BlockError::MissingBlocks }

    pub fn admissible_equivocation() -> BlockError {
        BlockError::Invalid(InvalidBlock::AdmissibleEquivocation)
    }

    pub fn ignorable_equivocation() -> BlockError {
        BlockError::Invalid(InvalidBlock::IgnorableEquivocation)
    }

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
            InvalidBlock::AdmissibleEquivocation
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
            | InvalidBlock::InvalidBlockHash
            | InvalidBlock::UnauthorizedSlashDeploy
            | InvalidBlock::ContainsExpiredDeploy
            | InvalidBlock::ContainsTimeExpiredDeploy
            | InvalidBlock::ContainsFutureDeploy
            // IgnorableEquivocation is now slashable per Bug #1 (§9.1). On
            // dev this variant was a known DOS-vector TODO — equivocations
            // observed via someone else's justification produced no on-chain
            // evidence. The dispatcher (`engine::multi_parent_casper::handle_*`)
            // now mints an EquivocationRecord whenever this branch fires.
            | InvalidBlock::IgnorableEquivocation => true,

            // Non-slashable variants — listed explicitly so the compiler
            // catches new additions to the enum. Each represents a failure
            // attributable to the block's wire format or local node state,
            // NOT to Byzantine behavior the network can attribute and slash:
            //   • InvalidFormat/Signature/Sender/Version/Timestamp: malformed
            //     wire data; the sender is not identifiable (Signature) or
            //     the sender's identity can't be verified (Sender).
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
