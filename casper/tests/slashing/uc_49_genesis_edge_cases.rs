// UC-49 — Genesis-time edge cases.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-49.
// Reference: design/12-failure-modes.md §12.3.4.
//
// Scenario: equivocation at the very first sequence number (seq=0)
// — the genesis-adjacent boundary case. The post-fix dispatcher
// records invalid evidence but does not mint an equivocation-record
// key because the predecessor sequence is outside the natural-number
// record domain.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_49_genesis_seq_zero_equivocation() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates at the very first sequence number.
    let _b_0 = harness.sign_block("v0", 0);
    let bad = harness.sign_block_distinct("v0", 0);
    let s = harness.dispatch(bad);

    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(harness.dag.invalid.contains(&bad));
    assert!(
        !harness.has_record("v0", 0),
        "seq=0 has invalid evidence but no predecessor record key"
    );
}

#[test]
fn uc_49_seq_one_boundary() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // v0 publishes at seq=0 then equivocates at seq=1.
    let _b_0 = harness.sign_block("v0", 0);
    let _b_1 = harness.sign_block("v0", 1);
    let bad = harness.sign_block_distinct("v0", 1);
    let s = harness.dispatch(bad);

    assert_eq!(s, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 0),
        "seq=1 equivocation records at base=0"
    );
}
