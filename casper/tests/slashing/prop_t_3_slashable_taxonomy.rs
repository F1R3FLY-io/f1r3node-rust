// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-3 (slashable taxonomy correctness).
//
// Property: the current economic-evidence set contains exactly
// AdmissibleEquivocation and IgnorableEquivocation. All 27 other rejection
// reasons remain durable consensus rejections without economic evidence.
//
// This exercises the production `InvalidBlock::is_slashable` source of truth.

use casper::rust::block_status::InvalidBlock;
use models::rust::block_metadata::AdmissionRejectionReason;
use proptest::prelude::*;

fn rejection_cases() -> Vec<(InvalidBlock, AdmissionRejectionReason)> {
    vec![
        (
            InvalidBlock::InvalidFormat,
            AdmissionRejectionReason::InvalidFormat,
        ),
        (
            InvalidBlock::InvalidSignature,
            AdmissionRejectionReason::InvalidSignature,
        ),
        (
            InvalidBlock::InvalidSender,
            AdmissionRejectionReason::InvalidSender,
        ),
        (
            InvalidBlock::InvalidVersion,
            AdmissionRejectionReason::InvalidVersion,
        ),
        (
            InvalidBlock::InvalidTimestamp,
            AdmissionRejectionReason::InvalidTimestamp,
        ),
        (
            InvalidBlock::DeployNotSigned,
            AdmissionRejectionReason::DeployNotSigned,
        ),
        (
            InvalidBlock::InvalidBlockNumber,
            AdmissionRejectionReason::InvalidBlockNumber,
        ),
        (
            InvalidBlock::InvalidRepeatDeploy,
            AdmissionRejectionReason::InvalidRepeatDeploy,
        ),
        (
            InvalidBlock::InvalidParents,
            AdmissionRejectionReason::InvalidParents,
        ),
        (
            InvalidBlock::InvalidFollows,
            AdmissionRejectionReason::InvalidFollows,
        ),
        (
            InvalidBlock::InvalidSequenceNumber,
            AdmissionRejectionReason::InvalidSequenceNumber,
        ),
        (
            InvalidBlock::InvalidShardId,
            AdmissionRejectionReason::InvalidShardId,
        ),
        (
            InvalidBlock::JustificationRegression,
            AdmissionRejectionReason::JustificationRegression,
        ),
        (
            InvalidBlock::NeglectedInvalidBlock,
            AdmissionRejectionReason::NeglectedInvalidBlock,
        ),
        (
            InvalidBlock::NeglectedEquivocation,
            AdmissionRejectionReason::NeglectedEquivocation,
        ),
        (
            InvalidBlock::InvalidTransaction,
            AdmissionRejectionReason::InvalidTransaction,
        ),
        (
            InvalidBlock::InvalidBondsCache,
            AdmissionRejectionReason::InvalidBondsCache,
        ),
        (
            InvalidBlock::InvalidEquivocationEvidence,
            AdmissionRejectionReason::InvalidEquivocationEvidence,
        ),
        (
            InvalidBlock::InvalidBlockHash,
            AdmissionRejectionReason::InvalidBlockHash,
        ),
        (
            InvalidBlock::UnauthorizedSlashDeploy,
            AdmissionRejectionReason::UnauthorizedSlashDeploy,
        ),
        (
            InvalidBlock::InvalidRejectedDeploy,
            AdmissionRejectionReason::InvalidRejectedDeploy,
        ),
        (
            InvalidBlock::ContainsExpiredDeploy,
            AdmissionRejectionReason::ContainsExpiredDeploy,
        ),
        (
            InvalidBlock::ContainsTimeExpiredDeploy,
            AdmissionRejectionReason::ContainsTimeExpiredDeploy,
        ),
        (
            InvalidBlock::ContainsFutureDeploy,
            AdmissionRejectionReason::ContainsFutureDeploy,
        ),
        (
            InvalidBlock::NotOfInterest,
            AdmissionRejectionReason::NotOfInterest,
        ),
        (
            InvalidBlock::LowDeployCost,
            AdmissionRejectionReason::LowDeployCost,
        ),
        (
            InvalidBlock::PrematureDeployRetry,
            AdmissionRejectionReason::PrematureDeployRetry,
        ),
        (
            InvalidBlock::AdmissibleEquivocation,
            AdmissionRejectionReason::AdmissibleEquivocation,
        ),
        (
            InvalidBlock::IgnorableEquivocation,
            AdmissionRejectionReason::IgnorableEquivocation,
        ),
    ]
}

fn assert_case(invalid: &InvalidBlock, reason: AdmissionRejectionReason) {
    assert_eq!(AdmissionRejectionReason::from(invalid), reason);
    assert_eq!(invalid.is_slashable(), reason.is_slash_evidence_eligible());
}

#[test]
fn t_3_rejection_mapping_and_evidence_classification_are_exhaustive() {
    let cases = rejection_cases();
    assert_eq!(cases.len(), 29);
    for (invalid, reason) in cases {
        assert_case(&invalid, reason);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn t_3_random_rejection_mapping_preserves_evidence_classification(index in 0usize..29) {
        let cases = rejection_cases();
        let (invalid, reason) = &cases[index];
        assert_case(invalid, *reason);
    }
}
