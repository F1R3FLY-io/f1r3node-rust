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
