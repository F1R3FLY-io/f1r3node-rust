// Equivocation at seq=0 (genesis-adjacent) does NOT mint a record.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14, boundary
// case of T-4 / T-5 (record uniqueness + monotonicity).
// Reference: see `casper/src/rust/slashing_authorization.rs::checked_base_seq`
// (`seq <= 0 → None`) and commit db0b979.
//
// Scenario: validator v0 publishes two distinct blocks at seq=0. The
// detector classifies the second as Ignorable, but the dispatcher does
// NOT mint a record — `checked_base_seq(0)` returns None, so there is no
// valid base seq for the record key. This is the contrapositive of UC-49
// (genesis-edge invariants). A downstream observer that cites v0's bad
// block at seq=0 cannot be classed Neglected because there's no record
// to neglect.

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
    assert!(harness.dag.invalid.contains(&bad));
    assert!(!harness.has_record("v0", 0));

    // B cites A's bad block without slashing.
    let b_negligent = harness.sign_block_citing("v1", 1, bad);
    let s2 = harness.dispatch(b_negligent);
    assert_eq!(s2, Status::Valid);
    assert!(!harness.has_record("v1", 0));
}
