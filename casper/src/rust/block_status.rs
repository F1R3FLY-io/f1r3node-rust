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
        match error {
            CasperError::BlockNotHeld(hash) => BlockError::Undecidable(hash),
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
    // SlashDeploy. See docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.1.
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

    pub fn exception(ex: CasperError) -> BlockError { BlockError::BlockException(ex) }

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

impl From<KvStoreError> for BlockError {
    fn from(error: KvStoreError) -> Self { BlockError::BlockException(CasperError::from(error)) }
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

    /// The classifier above only helps where validation actually calls it.
    /// A site that names `BlockException` itself skips it, and the skip is
    /// invisible from the outside: the block is judged rather than deferred,
    /// and the proposer collects an `InvalidTransaction` record for history
    /// THIS node does not hold. That is not hypothetical — the classifier was
    /// wired at three sites out of twenty-three, and a joiner recorded nine
    /// such verdicts against three honest validators before the shard froze.
    ///
    /// So the rule is enforced here rather than left to review: every error
    /// leaving validation goes through the classifier, which passes all but
    /// `BlockNotHeld` straight through unchanged. Guarding at the boundary is
    /// what makes the next error class that means "ask me later" impossible to
    /// route into the slashable bucket by accident.
    #[test]
    fn validation_routes_every_error_through_the_classifier() {
        const VALIDATE_RS: &str = include_str!("validate.rs");

        let direct: Vec<&str> = VALIDATE_RS
            .lines()
            .filter(|line| line.contains("BlockError::BlockException("))
            .collect();

        assert!(
            direct.is_empty(),
            "validate.rs must reach BlockException only through \
             BlockError::from_validation_error, so absence keeps its name; {} site(s) \
             construct it directly:\n{}",
            direct.len(),
            direct
                .iter()
                .map(|l| format!("  {}", l.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
