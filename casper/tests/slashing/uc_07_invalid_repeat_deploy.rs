// UC-07 — InvalidRepeatDeploy persists without economic evidence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §12 UC-07.
// Theorem: T-9.3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_07_invalid_repeat_deploy_persists_without_evidence() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 6);
    let status = harness.dispatch_with_status(hash, Status::RejectedOther);
    assert_eq!(status, Status::RejectedOther);
    assert!(!harness.has_record("v0", 5));
    assert!(harness.dag.invalid.contains(&hash));
}
