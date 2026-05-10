// UC-67 — Report-time closure shrinkage.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-67.
// Theorem: T-12 report suppression (Sage finding 14 — "evidence
// propagation over time is not monotone once reports are modeled").
// Reference: formal/sage/slashing/FINDINGS.md row 14.
//
// Property: a later honest slash report can REMOVE a neglect edge
// and SHRINK the accountability closure. The correct invariant is
// edge admissibility (visible AND unreported), not closure
// monotonicity over report time.
//
// The harness models this by transitioning a validator from
// "neglecting" (citing-without-slashing) to "honest slasher"
// (citing-with-slash). The honest version produces no record.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_67_report_removes_neglect_edge() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 publishes a NEGLECTING block (cite without slash) —
    // creates an active neglect edge.
    let v1_neg = harness.sign_block_citing("v1", 6, bad);
    let s_neg = harness.dispatch(v1_neg);
    assert_eq!(s_neg, Status::NeglectedEquivocation);
    assert!(
        harness.has_record("v1", 5),
        "active neglect edge produces a v1 record"
    );

    // v2 publishes an HONEST SLASHER block (cite + slash) —
    // suppresses the same evidence at report time. v2's edge
    // is reported; v2 stays Valid.
    let v2_honest = harness.sign_block_citing_with_slash("v2", 7, bad, "v0");
    let s_honest = harness.dispatch(v2_honest);
    assert_eq!(
        s_honest,
        Status::Valid,
        "honest slasher (cite + slash) is reported, not Neglected"
    );
    assert!(
        !harness.has_record("v2", 6),
        "report suppression: honest slasher's edge produces NO record"
    );

    // v1's record (the unreported edge) is still present —
    // shrinkage applies only to the reported edge, not retroactively
    // to other validators' unreported edges.
    assert!(harness.has_record("v1", 5));
}
