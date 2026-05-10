// UC-58 — Partial evidence visibility does not create neglect
// edges absent visible-unreported evidence.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-58.
// Theorem: T-12 visibility (`visible_unreported_graph_in`,
// `visible_reachability_first_edge`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 6.
//
// Sage witness: n=4, equivocator [0], visibility [], reports [].
// Partial visibility closure is [0]; full-visibility closure is
// [0,1,2,3]; the gap is [1,2,3]. The post-fix model only fires
// neglect edges when evidence is BOTH visible AND unreported.
//
// Harness modeling: a validator that has not seen the equivocator's
// bad block in their justifications cannot be classified as
// neglecting it — the neglect rule requires citing-without-slashing
// (cite is the visibility carrier).

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_58_no_neglect_without_visible_evidence() {
    let mut harness = SlashingTestHarness::new(4, 100);

    // v0 equivocates → record minted in the tracker.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // v1, v2, v3 all publish blocks that do NOT cite v0's bad
    // block (they have not seen the evidence). None of them
    // should be classified as neglecting.
    let v1_unrelated = harness.sign_block("v1", 6);
    let s1 = harness.dispatch(v1_unrelated);
    assert_eq!(
        s1,
        Status::Valid,
        "T-12 visibility: v1 has not cited the equivocator → no neglect"
    );

    let v2_unrelated = harness.sign_block("v2", 6);
    let s2 = harness.dispatch(v2_unrelated);
    assert_eq!(s2, Status::Valid);

    let v3_unrelated = harness.sign_block("v3", 6);
    let s3 = harness.dispatch(v3_unrelated);
    assert_eq!(s3, Status::Valid);

    // None of v1/v2/v3 has a record minted.
    assert!(!harness.has_record("v1", 5));
    assert!(!harness.has_record("v2", 5));
    assert!(!harness.has_record("v3", 5));
}

#[test]
fn uc_58_neglect_fires_only_when_evidence_visible_and_unreported() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 cites v0's bad block AND issues a SlashDeploy → reported.
    let v1_honest = harness.sign_block_citing_with_slash("v1", 6, bad, "v0");
    let s = harness.dispatch(v1_honest);
    assert_eq!(
        s,
        Status::Valid,
        "T-12 visibility: visible AND reported = no neglect"
    );
    assert!(!harness.has_record("v1", 5));

    // v2 cites v0's bad block but does NOT slash → visible-unreported.
    let v2_neg = harness.sign_block_citing("v2", 7, bad);
    let s = harness.dispatch(v2_neg);
    assert_eq!(
        s,
        Status::NeglectedEquivocation,
        "T-12 visibility: visible AND unreported = neglect fires"
    );
    assert!(harness.has_record("v2", 6));
}
