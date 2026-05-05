// UC-27 — NeglectedInvalidBlock dispatch.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-27.
// Theorems: T-3, T-6, T-9.3.
//
// Scenario: a block is classified `NeglectedInvalidBlock` (the
// "block cites an invalid block in its justifications without
// slashing the offender" variant). The post-fix catch-all dispatcher
// mints a record so the proposing layer can later issue a SlashDeploy.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_27_neglected_invalid_block_dispatches_record() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let hash = harness.sign_block("v1", 9);

    // The harness models NeglectedInvalidBlock under the
    // `SlashableOther` umbrella; the production code uses the
    // dedicated InvalidBlock::NeglectedInvalidBlock variant.
    let status = harness.dispatch_with_status(hash, Status::SlashableOther);
    assert_eq!(status, Status::SlashableOther);

    assert!(harness.has_record("v1", 8),
        "post-fix #3: dispatcher mints record for NeglectedInvalidBlock");
    assert!(harness.dag.invalid.contains(&hash));
}
