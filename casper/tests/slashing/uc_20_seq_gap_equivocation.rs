use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_20_seq_gap_equivocation_is_detected() {
    let mut harness = SlashingTestHarness::new(2, 100);

    let _ = harness.sign_block("v0", 0);
    let _ = harness.sign_block("v0", 2);
    let bad = harness.sign_block_distinct("v0", 2);
    let status = harness.dispatch(bad);

    assert_eq!(status, Status::IgnorableEquivocation);
    assert!(harness.has_record("v0", 1));
}
