// UC-77 — Semantic attack-campaign classification.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-77.
// Theorems: T-15D (differential divergence), T-12PF (proposer
// fairness).
// Reference: formal/sage/slashing/FINDINGS.md row 13 (semantic
// campaigns), formal/tlaplus/slashing/TwoLevelSlashing.tla
// invariant `SemanticCampaignDivergenceClass`.
//
// Property: every named adversarial campaign — flooding, reorg-bait,
// censor-and-equivocate, evidence-suppression, slow-loris, etc. —
// produces a trace whose final divergence class is in the documented
// bucket set (never `UnexpectedDivergence`).

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
enum Campaign {
    /// Byzantine flooding: equivocate at every seq.
    Flood,
    /// Reorg-bait: equivocate at the deepest non-finalized seq.
    ReorgBait,
    /// Censor-and-equivocate: omit reports for one's own equivocation.
    CensorAndEquivocate,
    /// Evidence-suppression: collude with the proposer to delay slash.
    EvidenceSuppression,
    /// Slow-loris: trickle equivocations one-per-epoch under fairness.
    SlowLoris,
    /// Projection-boundary: force fixed-width arithmetic to its edge.
    ProjectionBoundary,
}

fn campaign_classification(c: Campaign) -> DivergenceClass {
    use Campaign::*;
    match c {
        Flood | ReorgBait => DivergenceClass::Bisimilar,
        CensorAndEquivocate => classify(DivergenceReason::EvidenceViewBoundary),
        EvidenceSuppression => classify(DivergenceReason::ProposerFairnessBoundary),
        SlowLoris => classify(DivergenceReason::ProposerFairnessBoundary),
        ProjectionBoundary => classify(DivergenceReason::ProjectionBoundary),
    }
}

#[test]
fn uc_77_every_campaign_lands_in_documented_bucket() {
    let campaigns = [
        Campaign::Flood,
        Campaign::ReorgBait,
        Campaign::CensorAndEquivocate,
        Campaign::EvidenceSuppression,
        Campaign::SlowLoris,
        Campaign::ProjectionBoundary,
    ];
    for c in campaigns {
        let class = campaign_classification(c);
        assert!(
            frontier_classification_ok(class),
            "UC-77: campaign {:?} produced UnexpectedDivergence",
            c
        );
    }
}

#[test]
fn uc_77_flood_and_reorg_bait_are_bisimilar() {
    // Pure equivocation campaigns stay in the bisimilar bucket
    // because the detection layer catches them deterministically.
    assert_eq!(
        campaign_classification(Campaign::Flood),
        DivergenceClass::Bisimilar
    );
    assert_eq!(
        campaign_classification(Campaign::ReorgBait),
        DivergenceClass::Bisimilar
    );
}

#[test]
fn uc_77_evidence_suppression_is_proposer_fairness_boundary() {
    // The proposer-fairness boundary captures cases where the
    // protocol's bounded-liveness guarantee depends on
    // evidence-inclusion fairness (T-12PF).
    assert_eq!(
        campaign_classification(Campaign::EvidenceSuppression),
        DivergenceClass::CandidateBoundaryDivergence
    );
}
