use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn neglect_at_seq_zero_boundary() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // A equivocates at seq=0 (genesis-adjacent).
    let _a1 = harness.sign_block("v0", 0);
    let bad = harness.sign_block_distinct("v0", 0);
    let s1 = harness.dispatch(bad);
    assert_eq!(s1, Status::IgnorableEquivocation);
    // base_seq = saturating_sub(0, 1) = 0.
    assert!(harness.has_record("v0", 0));

    // B cites A's bad block without slashing.
    let b_negligent = harness.sign_block_citing("v1", 1, bad);
    let s2 = harness.dispatch(b_negligent);
    assert_eq!(s2, Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 0));
}
