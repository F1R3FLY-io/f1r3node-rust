// UC-42 — Certified non-equivocation rejection persistence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-42.
// Theorem: T-9.3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_42_certified_rejection_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 7);

    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);
    assert!(
        !harness.has_record("v0", 6),
        "non-equivocation rejection must not mint economic evidence"
    );
    assert!(harness.dag.invalid.contains(&hash));
}
