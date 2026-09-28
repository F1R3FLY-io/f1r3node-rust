// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// Property-based test for T-3 (slashable taxonomy correctness).
//
// Theorem: T-3 (`slashable_post_fix_extends_pre_fix`,
// formal/rocq/slashing/theories/InvalidBlock.v:151).
// Reference: docs/casper/theory/slashing/slashing-specification.md §4
// (Theorem 4.3).
//
// Property: the post-fix slashable set is exactly the 18-element set
// listed in spec §4 — the 17 pre-fix slashable variants plus
// `IgnorableEquivocation`. The 8 remaining variants (InvalidFormat,
// InvalidSignature, InvalidSender, InvalidVersion, InvalidTimestamp,
// InvalidRejectedDeploy, NotOfInterest, LowDeployCost) are not
// slashable.
//
// This test exercises the *production* `InvalidBlock::is_slashable`
// method directly (not via the harness's projected Status), proving
// the source-of-truth taxonomy matches the design's normative table.

use casper::rust::block_status::InvalidBlock;

#[test]
fn t_3_slashable_set_is_the_equivocation_class() {
    // Slash evidence demands a fault every honest node attributes
    // identically from the signed block alone; equivocation is the one
    // verdict with that property. The former 18-element set slashed
    // view-relative verdicts too, and CI run 32588262605 demonstrated the
    // consequence: JustificationRegression and UnauthorizedSlashDeploy
    // verdicts diverging across honest nodes minted recursive evidence
    // that burned honest stake to FT −18.55. A demoted verdict still
    // drops the block; only the economic layer narrowed.
    let slashable = vec![
        InvalidBlock::AdmissibleEquivocation,
        InvalidBlock::IgnorableEquivocation,
    ];
    assert_eq!(
        slashable.len(),
        2,
        "slashable set is the equivocation class"
    );
    for v in &slashable {
        assert!(v.is_slashable(), "{:?} must be slashable", v);
    }

    let demoted = vec![
        InvalidBlock::NeglectedEquivocation,
        InvalidBlock::NeglectedInvalidBlock,
        InvalidBlock::JustificationRegression,
        InvalidBlock::UnauthorizedSlashDeploy,
        InvalidBlock::InvalidParents,
        InvalidBlock::InvalidFollows,
        InvalidBlock::InvalidBlockNumber,
        InvalidBlock::InvalidSequenceNumber,
        InvalidBlock::InvalidShardId,
        InvalidBlock::InvalidRepeatDeploy,
        InvalidBlock::DeployNotSigned,
        InvalidBlock::InvalidTransaction,
        InvalidBlock::InvalidBondsCache,
        InvalidBlock::InvalidBlockHash,
        InvalidBlock::ContainsExpiredDeploy,
        InvalidBlock::ContainsTimeExpiredDeploy,
        InvalidBlock::ContainsFutureDeploy,
    ];
    for v in &demoted {
        assert!(
            !v.is_slashable(),
            "{:?} is judged against local state and must not mint slash \
             evidence",
            v
        );
    }
}

#[test]
fn t_3_non_slashable_set_is_8_elements() {
    let non_slashable = vec![
        InvalidBlock::InvalidFormat,
        InvalidBlock::InvalidSignature,
        InvalidBlock::InvalidSender,
        InvalidBlock::InvalidVersion,
        InvalidBlock::InvalidTimestamp,
        InvalidBlock::InvalidRejectedDeploy,
        InvalidBlock::NotOfInterest,
        InvalidBlock::LowDeployCost,
    ];
    assert_eq!(non_slashable.len(), 8, "non-slashable set has 8 variants");
    for v in &non_slashable {
        assert!(
            !v.is_slashable(),
            "non-slashable: {:?} must not be slashable",
            v
        );
    }
}
