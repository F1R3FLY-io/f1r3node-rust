use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};
use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_74_fair_proposer_reports_visible_equivocation() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    assert_eq!(harness.dispatch(bad), Status::IgnorableEquivocation);

    let report = harness.sign_block_citing_with_slash("v1", 6, bad, "v0");
    assert_eq!(harness.dispatch(report), Status::Valid);
    assert!(!harness.has_record("v1", 5));
    assert_eq!(
        harness.simulate_slash_proposal("v1"),
        vec!["v0".to_string()]
    );
}

#[test]
fn uc_74_withheld_visible_evidence_is_detectable_neglect() {
    let mut harness = SlashingTestHarness::new(2, 100);
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    assert_eq!(harness.dispatch(bad), Status::IgnorableEquivocation);

    let withheld = harness.sign_block_citing("v1", 6, bad);
    assert_eq!(harness.dispatch(withheld), Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 5));

    let class = classify(DivergenceReason::ProposerFairnessBoundary);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));
}
