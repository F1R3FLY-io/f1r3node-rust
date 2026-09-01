// UC-09 — ContainsTimeExpiredDeploy persists without economic evidence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-09.
// Theorem: T-9.3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_09_contains_time_expired_deploy_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 14);
    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);
    assert!(!harness.has_record("v0", 13));
    assert!(harness.dag.invalid.contains(&hash));
}
