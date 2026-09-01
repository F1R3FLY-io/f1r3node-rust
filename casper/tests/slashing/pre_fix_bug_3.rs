// Regression backstop for the certified-rejection dispatcher.
//
// Reference: docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.4.
// The dispatcher must persist every certified rejection. It must not create
// economic evidence for a non-equivocation rejection.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn certified_rejection_persists_without_false_economic_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // Simulate one non-equivocation rejection from upstream validation.
    let hash = harness.sign_block("v0", 7);
    let status = harness.dispatch_with_status(hash, Status::RejectedOther);

    assert_eq!(status, Status::RejectedOther);

    assert!(
        !harness.has_record("v0", 6),
        "non-equivocation rejection must not mint economic evidence"
    );
    assert!(harness.dag.invalid.contains(&hash));
}
