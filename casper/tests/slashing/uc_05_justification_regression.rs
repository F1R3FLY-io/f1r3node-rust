use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_05_justification_regression_mints_record() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let hash = harness.sign_block("v0", 6);

    let status = harness.dispatch_with_status(hash, Status::JustificationRegression);

    assert_eq!(status, Status::JustificationRegression);
    assert!(harness.has_record("v0", 5));
    assert!(harness.dag.invalid.contains(&hash));
}
