// UC-54 — Combined record-store invariants (T-4 + T-5 example).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-54.
// Theorems: T-4 (record uniqueness), T-5 (witness monotonicity).
// Reference: design/05-storage-and-records.md.
//
// Concrete trace asserting both invariants together:
//   • At most one record per (validator, base_seq).
//   • Witness sets only grow under repeated dispatch.
//   • Records keyed by different (validator, base_seq) pairs are
//     independent — slashing one does not affect the other.

use super::harness::SlashingTestHarness;

#[test]
fn uc_54_combined_record_invariants() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Two distinct equivocations by the same validator at different
    // base sequences.
    let _ = harness.sign_block("v0", 5);
    let bad_5_a = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad_5_a);
    let bad_5_b = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad_5_b);
    let bad_5_c = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad_5_c);

    let _ = harness.sign_block("v0", 10);
    let bad_10_a = harness.sign_block_distinct("v0", 10);
    let _ = harness.dispatch(bad_10_a);

    // T-4: exactly two records — (v0, 4) and (v0, 9). Three
    // dispatches at seq=5 collapsed into a single record under (v0, 4).
    let v0_records: Vec<_> = harness
        .tracker
        .records
        .keys()
        .filter(|(v, _)| v == "v0")
        .collect();
    assert_eq!(
        v0_records.len(),
        2,
        "T-4: exactly two records for v0 (one per distinct base_seq)"
    );

    // T-5: the (v0, 4) witness set has all three dispatched hashes.
    let w_4 = harness.record_witnesses("v0", 4);
    assert!(w_4.contains(&bad_5_a));
    assert!(w_4.contains(&bad_5_b));
    assert!(w_4.contains(&bad_5_c));
    assert_eq!(w_4.len(), 3, "T-5: witnesses grew monotonically to 3");

    // The (v0, 9) witness set has the single seq=10 hash.
    let w_9 = harness.record_witnesses("v0", 9);
    assert!(w_9.contains(&bad_10_a));
    assert_eq!(w_9.len(), 1);
    assert!(
        !w_9.contains(&bad_5_a),
        "records are partitioned by base_seq — no cross-pollution"
    );

    // Slashing v0 does not affect any other validator's records.
    assert!(
        harness.tracker.records.keys().all(|(v, _)| v == "v0"),
        "no records for v1 or v2 — partitioning by validator preserved"
    );
}
