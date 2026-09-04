// UC-08 — ContainsExpiredDeploy dispatch.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-08.
// Theorems: T-3, T-9.3.
//
// Scenario: a block contains an expired deploy. The dispatcher persists the
// rejection without creating economic evidence.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_08_contains_expired_deploy_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 8);

    let status = harness.dispatch_with_status(hash, Status::RejectedOther);

    assert_eq!(status, Status::RejectedOther);
    assert!(!harness.has_record("v0", 7));
    assert!(harness.dag.invalid.contains(&hash));
}
