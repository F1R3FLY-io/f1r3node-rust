// UC-33 — DeployNotSigned persists without economic evidence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-33.
// The harness simulates the certified rejection from upstream validation.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_33_deploy_not_signed_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 8);

    // Upstream validation classifies the block as DeployNotSigned.
    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);

    assert!(
        !harness.has_record("v0", 7),
        "non-equivocation rejection must not mint economic evidence"
    );
    assert!(
        harness.dag.invalid.contains(&hash),
        "the offending block is added to the invalid index"
    );
}
