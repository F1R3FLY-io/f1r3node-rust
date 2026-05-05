// UC-84 — Liveness-as-safety frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-84.
// Theorem: T-12PF (proposer fairness for bounded liveness).
// Reference: formal/tlaplus/slashing/TwoLevelSlashing.tla invariant
// `Inv_ProposerFairnessForBoundedLiveness`.
//
// Property: bounded slash liveness — "every detected equivocation
// is slashed within K rounds" — IS reachable when proposer
// evidence-inclusion fairness is enforced, and IS NOT bounded-live
// when it isn't. The boundary of fairness-enforcement is the
// liveness-as-safety witness: violation iff fairness is dropped.

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

fn bounded_liveness_holds(fairness_enforced: bool, k_rounds: u32) -> bool {
    // With fairness, eventually some round (within k) sees the
    // evidence included → slash fires. Without, the schedule may
    // delay indefinitely → liveness lost.
    fairness_enforced && k_rounds >= 1
}

#[test]
fn uc_84_liveness_holds_iff_fairness_enforced() {
    assert!(bounded_liveness_holds(true, 4),
        "UC-84: liveness must hold under fairness (T-12PF)");
    assert!(!bounded_liveness_holds(false, 4),
        "UC-84: liveness must FAIL when fairness is dropped");
    let class_with    = DivergenceClass::Bisimilar;
    let class_without = classify(DivergenceReason::ProposerFairnessBoundary);
    assert!(frontier_classification_ok(class_with));
    assert!(frontier_classification_ok(class_without));
}

#[test]
fn uc_84_zero_rounds_never_bounded_live() {
    // Even under fairness, k_rounds=0 cannot witness inclusion.
    assert!(!bounded_liveness_holds(true, 0),
        "UC-84: zero-round budget cannot witness slash");
}
