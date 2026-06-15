// UC-12 — Concurrent insert: post-fix preserves both witnesses;
// pre-fix loses one. Sequential proxy for T-9.2.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-12.
// Theorem: T-9.2 (atomic record insert).
// Reference: design/09-bug-fixes-and-rationale.md §9.2.
//
// The full thread-interleaving coverage lives in
// `loom_t_9_2_atomic_record.rs` (run unconditionally as a regular
// test). UC-12 is the §14.3.1 example trace mirroring the loom
// test's success branch in a sequential, fast format.
//
// (See `pre_fix_bug_2.rs` for a closely-related test that pins
// the same invariant from the regression-backstop direction.)

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_12_concurrent_admissible_preserves_both_witnesses() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let _b1 = harness.sign_block("v0", 5);
    let bad_a = harness.sign_block_distinct("v0", 5);
    let bad_b = harness.sign_block_distinct("v0", 5);

    // Sequential dispatch — both observations get folded into the
    // same record. Loom verifies this property holds across all
    // possible thread interleavings.
    let s1 = harness.dispatch(bad_a);
    let s2 = harness.dispatch(bad_b);
    assert_eq!(s1, Status::IgnorableEquivocation);
    assert_eq!(s2, Status::AdmissibleEquivocation);

    // T-9.2 invariant: both witnesses preserved.
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(witnesses.contains(&bad_a));
    assert!(witnesses.contains(&bad_b));
    assert_eq!(witnesses.len(), 2);
}
