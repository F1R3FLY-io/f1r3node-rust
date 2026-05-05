// UC-46 — Network partition then merge with both-side equivocations.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-46.
// Theorems: T-1 (soundness), T-9.2 (atomic record insert under
// concurrent dispatch), T-15 (bisimilarity).
// Reference: design/12-failure-modes.md §12.3.3.
//
// Scenario: a validator participates in two partitions, signing
// distinct blocks at the same sequence on each side. After merge,
// both equivocating blocks become visible; the dispatcher's
// atomic-tracker insert (post-fix #2) captures both witnesses
// without losing either.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_46_partition_merge_both_witnesses_preserved() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Pre-merge: v0 signs in partition A.
    let partition_a = harness.sign_block_distinct("v0", 5);

    // Pre-merge: v0 also signs (different hash) in partition B.
    let partition_b = harness.sign_block_distinct("v0", 5);

    // Post-merge: dispatcher observes both blocks. The first is
    // Ignorable (no record yet); the second is Admissible (record
    // already exists from the first).
    let s_a = harness.dispatch(partition_a);
    assert_eq!(s_a, Status::IgnorableEquivocation);
    let s_b = harness.dispatch(partition_b);
    assert_eq!(s_b, Status::AdmissibleEquivocation);

    // Post-fix #2 invariant: BOTH witnesses end up in the same
    // record's set. Pre-fix this was the race window where one
    // witness could be lost.
    assert!(harness.has_record("v0", 4));
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(witnesses.contains(&partition_a),
        "partition-A witness preserved across merge");
    assert!(witnesses.contains(&partition_b),
        "partition-B witness preserved across merge");
    assert_eq!(witnesses.len(), 2,
        "exactly two witnesses (one per partition)");

    // Slash applies normally; both partitions converge on the same
    // post-state.
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.coop_vault(), 100);
}
