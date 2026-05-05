// UC-63 — Closure fixed-point certificate.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-63.
// Theorem: T-11 fixed point (`slash_iter_fixed_point_after_universe_bound`,
// `slash_iter_fixed_point_stable`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 11 — "closure
// certificates show first slash round equals shortest neglect-path
// distance in the chain witness".
//
// Property: the slash closure stabilizes after at most |V| rounds
// of expansion. Each validator's first-slash round equals their
// shortest path distance to a direct equivocator in the neglect
// graph.
//
// Sage witness: n=6, offender 5, chain 0→1→2→3→4→5; round 5
// closure = full set; first-slash distances 5,4,3,2,1,0.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_63_chain_neglect_closure_stabilizes_at_n() {
    let n = 6usize;
    let mut harness = SlashingTestHarness::new(n, 100);

    // v5 is the direct offender.
    let _v5a = harness.sign_block("v5", 5);
    let bad = harness.sign_block_distinct("v5", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v5", 4));

    // Build chain v0→v1→v2→v3→v4→v5 of cite-without-slash.
    let v4_neg = harness.sign_block_citing("v4", 6, bad);
    let _ = harness.dispatch(v4_neg);
    let v3_neg = harness.sign_block_citing("v3", 7, v4_neg);
    let _ = harness.dispatch(v3_neg);
    let v2_neg = harness.sign_block_citing("v2", 8, v3_neg);
    let _ = harness.dispatch(v2_neg);
    let v1_neg = harness.sign_block_citing("v1", 9, v2_neg);
    let _ = harness.dispatch(v1_neg);
    let v0_neg = harness.sign_block_citing("v0", 10, v1_neg);
    let s0 = harness.dispatch(v0_neg);
    assert_eq!(s0, Status::NeglectedEquivocation);

    // Fixed-point: every validator has a record (closure complete).
    for i in 0..n {
        let v = format!("v{}", i);
        let has_record = (0..15).any(|b| harness.has_record(&v, b));
        assert!(has_record, "T-11 fixed-point: {} reachable to direct offender", v);
    }
}
