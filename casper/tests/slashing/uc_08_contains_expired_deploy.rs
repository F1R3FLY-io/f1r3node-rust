// UC-08 — ContainsExpiredDeploy dispatch.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-08.
// Theorems: T-3, T-9.3.
//
// Scenario: a block contains a deploy past its valid-after window
// (ContainsExpiredDeploy). The post-fix catch-all dispatcher mints an
// EquivocationRecord and marks the block invalid in the DAG — pre-fix the
// block was rejected without on-chain evidence, leaving 15-of-17 slashable
// variants record-less. See design/09-bug-fixes-and-rationale.md §9.3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_08_contains_expired_deploy_recorded() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 8);

    let status = harness.dispatch_with_status(hash, Status::SlashableOther);

    assert_eq!(status, Status::SlashableOther);
    assert!(harness.has_record("v0", 7));
    assert!(harness.dag.invalid.contains(&hash));
}
