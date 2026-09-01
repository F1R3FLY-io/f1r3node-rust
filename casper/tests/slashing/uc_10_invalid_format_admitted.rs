// UC-10 — InvalidFormat persists without economic evidence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-10.
// Theorem: T-3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_10_invalid_format_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 5);

    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);

    assert!(!harness.has_record("v0", 4));
    assert!(harness.dag.invalid.contains(&hash));
}
