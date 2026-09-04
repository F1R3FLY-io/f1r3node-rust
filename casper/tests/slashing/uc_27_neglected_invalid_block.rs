// UC-27 — NeglectedInvalidBlock dispatch.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-27.
// Theorems: T-3, T-6, T-9.3.
//
// Scenario: a block cites a rejected dependency. The dispatcher persists the
// derived rejection without creating recursive economic evidence.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_27_neglected_invalid_block_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let hash = harness.sign_block("v1", 9);

    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);

    assert!(
        !harness.has_record("v1", 8),
        "NeglectedInvalidBlock must not mint economic evidence"
    );
    assert!(harness.dag.invalid.contains(&hash));
}
