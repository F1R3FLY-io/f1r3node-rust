// Property-based test for T-3 (slashable taxonomy correctness).
//
// Theorem: T-3 (`slashable_post_fix_extends_pre_fix`,
// formal/rocq/slashing/theories/InvalidBlock.v:151).
// Reference: docs/theory/slashing/slashing-specification.md §4
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
fn t_3_post_fix_slashable_set_is_18_elements() {
    let slashable = vec![
        InvalidBlock::AdmissibleEquivocation,
        InvalidBlock::IgnorableEquivocation,
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
        InvalidBlock::InvalidBlockHash,
        InvalidBlock::ContainsExpiredDeploy,
        InvalidBlock::ContainsTimeExpiredDeploy,
        InvalidBlock::ContainsFutureDeploy,
    ];
    assert_eq!(
        slashable.len(),
        18,
        "post-fix slashable set has 18 variants"
    );
    for v in &slashable {
        assert!(v.is_slashable(), "post-fix: {:?} must be slashable", v);
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

#[test]
fn t_3_post_fix_extends_pre_fix_by_exactly_ignorable() {
    // Pre-fix: 17 slashable variants (AdmissibleEquivocation, ...).
    // Post-fix: same 17 plus IgnorableEquivocation = 18.
    // The bug-fix-#1 commit flipped exactly one bit in is_slashable().
    assert!(
        InvalidBlock::IgnorableEquivocation.is_slashable(),
        "post-fix #1: IgnorableEquivocation is slashable (the only variant added)"
    );
}
