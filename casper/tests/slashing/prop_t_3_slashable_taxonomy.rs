// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-3 (slashable taxonomy correctness).
//
// Property: the protocol-v5 intrinsic slashable set is exactly the 17-element
// set below. Contextual requested/unsolicited equivocation observations are not
// `InvalidBlock` variants. The 9 remaining variants (InvalidFormat,
// InvalidSignature, InvalidSender, InvalidVersion, InvalidTimestamp,
// InvalidBlockHash, InvalidRejectedDeploy, NotOfInterest, LowDeployCost) are not
// slashable.
//
// This exercises the production `InvalidBlock::is_slashable` source of truth.

use casper::rust::block_status::InvalidBlock;

#[test]
fn t_3_post_fix_slashable_set_is_17_elements() {
    let slashable = vec![
        InvalidBlock::NeglectedEquivocation,
        InvalidBlock::NeglectedInvalidBlock,
        InvalidBlock::JustificationRegression,
        InvalidBlock::InvalidParents,
        InvalidBlock::InvalidFollows,
        InvalidBlock::InvalidBlockNumber,
        InvalidBlock::InvalidSequenceNumber,
        InvalidBlock::InvalidShardId,
        InvalidBlock::InvalidRepeatDeploy,
        InvalidBlock::DeployNotSigned,
        InvalidBlock::InvalidTransaction,
        InvalidBlock::InvalidBondsCache,
        InvalidBlock::InvalidEquivocationEvidence,
        InvalidBlock::UnauthorizedSlashDeploy,
        InvalidBlock::ContainsExpiredDeploy,
        InvalidBlock::ContainsTimeExpiredDeploy,
        InvalidBlock::ContainsFutureDeploy,
    ];
    assert_eq!(
        slashable.len(),
        17,
        "post-fix slashable set has 17 variants"
    );
    for v in &slashable {
        assert!(v.is_slashable(), "post-fix: {:?} must be slashable", v);
    }
}

#[test]
fn t_3_non_slashable_set_is_9_elements() {
    let non_slashable = vec![
        InvalidBlock::InvalidFormat,
        InvalidBlock::InvalidSignature,
        InvalidBlock::InvalidSender,
        InvalidBlock::InvalidVersion,
        InvalidBlock::InvalidTimestamp,
        InvalidBlock::InvalidBlockHash,
        InvalidBlock::InvalidRejectedDeploy,
        InvalidBlock::NotOfInterest,
        InvalidBlock::LowDeployCost,
    ];
    assert_eq!(non_slashable.len(), 9, "non-slashable set has 9 variants");
    for v in &non_slashable {
        assert!(
            !v.is_slashable(),
            "non-slashable: {:?} must not be slashable",
            v
        );
    }
}

#[test]
fn malformed_equivocation_evidence_and_unauthorized_slash_are_intrinsic_offenses() {
    assert!(
        InvalidBlock::InvalidEquivocationEvidence.is_slashable(),
        "noncanonical block-carried evidence is an attributable intrinsic offense"
    );
    assert!(
        InvalidBlock::UnauthorizedSlashDeploy.is_slashable(),
        "an unauthorized slash attempt is an attributable intrinsic offense"
    );
}
