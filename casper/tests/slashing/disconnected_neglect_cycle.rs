// UC-60 — A neglect cycle with no path to a direct offender is
// not slashed.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-60.
// Theorem: T-12 reachability (`no_reachability_no_level2_slash`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 7 — "a cycle
// disconnected from a direct offender is not slashed".
//
// Property: validators that mutually cite each other's blocks
// without any reachability path to a direct equivocator are NOT
// in the slash closure. The neglect rule is reverse-reachability
// to a direct offender; cycles disconnected from any offender are
// idempotent and benign.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_60_disconnected_cycle_not_slashed() {
    let mut harness = SlashingTestHarness::new(4, 100);

    // No equivocations injected — the tracker is empty.
    // v1, v2, v3 publish blocks that mutually cite each other in
    // a cycle (v1 → v2 → v3 → v1). None of them has a record,
    // so cite-without-slash never triggers neglect.
    let v1_b = harness.sign_block("v1", 5);
    let v2_b = harness.sign_block_citing("v2", 6, v1_b);
    let v3_b = harness.sign_block_citing("v3", 7, v2_b);
    let v1_b2 = harness.sign_block_citing("v1", 8, v3_b);

    // None of these blocks classify as Neglected because no
    // citee has an outstanding record.
    let s2 = harness.dispatch(v2_b);
    let s3 = harness.dispatch(v3_b);
    let s1 = harness.dispatch(v1_b2);
    assert_eq!(s2, Status::Valid);
    assert_eq!(s3, Status::Valid);
    assert_eq!(s1, Status::Valid);

    // T-12 reachability: no validator gets a record because
    // there's no direct offender for the cycle to be reachable to.
    for i in 0..4 {
        let v = format!("v{}", i);
        for base in 0..10 {
            assert!(
                !harness.has_record(&v, base),
                "T-12 reachability: {} has no record at base={}",
                v,
                base
            );
        }
    }

    // All four validators remain active and bonded.
    assert_eq!(harness.fork_choice().len(), 4);
    for i in 0..4 {
        assert_eq!(harness.bond(&format!("v{}", i)), 100);
    }
}

#[test]
fn uc_60_connected_cycle_slashed_via_offender() {
    // Companion: when the cycle DOES have a path to a direct
    // offender, the entire cycle's reachable set IS slashed
    // (T-12 reachability inverse).
    let mut harness = SlashingTestHarness::new(4, 100);

    // v0 equivocates → direct offender.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 cites v0's bad block (path from v1 to direct offender).
    let v1_b = harness.sign_block_citing("v1", 6, bad);
    let s1 = harness.dispatch(v1_b);
    assert_eq!(
        s1,
        Status::NeglectedEquivocation,
        "v1 reachable to v0 → neglect"
    );
    assert!(harness.has_record("v1", 5));
}
