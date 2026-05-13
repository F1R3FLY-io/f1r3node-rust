// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-02 — Sequential simulation of concurrent admissible equivocations.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-02.
// Theorem: T-9.2 (atomic record insert under concurrent dispatch),
// formal/rocq/slashing/theories/BugFixAtomicTracker.v.
// Reference: design/05-storage-and-records.md.
//
// True concurrent dispatch (two threads racing on the same
// (validator, baseSeq)) requires `loom` for exhaustive interleaving
// — out of scope for the harness suite. This UC sequentially
// simulates the same workload: multiple equivocations at the same
// (validator, base_seq) merge into a single record with all
// witnesses preserved (T-4 + T-5 invariants under repeated dispatch).

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_02_two_admissible_at_same_base_merge_into_one_record() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // First equivocation at (v0, seq=5).
    let _b1 = harness.sign_block("v0", 5);
    let bad_a = harness.sign_block_distinct("v0", 5);
    let s1 = harness.dispatch(bad_a);
    assert_eq!(s1, Status::IgnorableEquivocation);
    assert!(harness.has_record("v0", 4));

    // Second equivocation at (v0, seq=5) (re-racing the same base).
    let bad_b = harness.sign_block_distinct("v0", 5);
    let s2 = harness.dispatch(bad_b);
    assert_eq!(
        s2,
        Status::AdmissibleEquivocation,
        "second observation has a pre-existing record → Admissible"
    );

    // Third equivocation at (v0, seq=5).
    let bad_c = harness.sign_block_distinct("v0", 5);
    let s3 = harness.dispatch(bad_c);
    assert_eq!(s3, Status::AdmissibleEquivocation);

    // T-4: still exactly one record under (v0, 4).
    let v0_records: Vec<_> = harness
        .tracker
        .records
        .keys()
        .filter(|(v, _)| v == "v0")
        .collect();
    assert_eq!(
        v0_records.len(),
        1,
        "T-4: one record per (validator, base_seq)"
    );

    // T-5: all three witnesses preserved (no overwrites — would have
    // happened pre-fix #2 if the RMW were lock-free under racing
    // threads).
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(witnesses.contains(&bad_a));
    assert!(witnesses.contains(&bad_b));
    assert!(witnesses.contains(&bad_c));
    assert_eq!(
        witnesses.len(),
        3,
        "T-5: all dispatched witnesses survived (atomic RMW)"
    );
}
