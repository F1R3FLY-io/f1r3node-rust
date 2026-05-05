// UC-81 — Bundle-based evidence-lifecycle frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-81.
// Theorems: T-12 visibility, T-12 retention.
// Reference: formal/sage/slashing/FINDINGS.md (bundle reuse),
// formal/tlaplus/slashing/TwoLevelSlashing.tla invariants
// `Inv_NeglectEdgesVisibleUnreported` /
// `Inv_EvidenceRetentionForDirectOffenders`.
//
// Property: bundle-based reuse of validators / edges across
// epoch / view boundaries preserves active-edge admissibility.
// Every bundle witness lands in a documented bucket.

use std::collections::BTreeSet;

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone)]
struct Bundle {
    /// Validators reused across epochs.
    validators: BTreeSet<u8>,
    /// (citer, offender) edges sampled from a per-bundle pool.
    edges: BTreeSet<(u8, u8)>,
    /// Whether the bundle is filtered to current-validators only.
    filtered: bool,
}

fn classify_bundle(b: &Bundle) -> DivergenceClass {
    if b.filtered {
        // Filtered bundles obey current-validator boundary: any
        // edge whose citer / offender escapes the current set is
        // dropped. Documented CandidateBoundaryDivergence.
        return classify(DivergenceReason::CurrentValidatorBoundary);
    }
    let active = !b.validators.is_empty();
    let admissible = b
        .edges
        .iter()
        .all(|(c, o)| b.validators.contains(c) && b.validators.contains(o));
    if active && admissible {
        DivergenceClass::Bisimilar
    } else {
        // Non-admissible edges in an unfiltered bundle indicate
        // evidence-view boundary — must be flagged for filtering.
        classify(DivergenceReason::EvidenceViewBoundary)
    }
}

#[test]
fn uc_81_filtered_bundle_is_boundary() {
    let b = Bundle {
        validators: [1u8, 2, 3].iter().copied().collect(),
        edges:      [(1u8, 2u8), (2, 3)].iter().copied().collect(),
        filtered:   true,
    };
    assert_eq!(
        classify_bundle(&b),
        DivergenceClass::CandidateBoundaryDivergence,
        "UC-81: filtered bundles always classify as boundary"
    );
}

#[test]
fn uc_81_admissible_bundle_is_bisimilar() {
    let b = Bundle {
        validators: [1u8, 2, 3].iter().copied().collect(),
        edges:      [(1u8, 2u8), (2, 3)].iter().copied().collect(),
        filtered:   false,
    };
    assert_eq!(classify_bundle(&b), DivergenceClass::Bisimilar);
}

#[test]
fn uc_81_non_admissible_edge_is_evidence_view_boundary() {
    let b = Bundle {
        validators: [1u8, 2].iter().copied().collect(),
        edges:      [(1u8, 99u8)].iter().copied().collect(), // 99 not in V
        filtered:   false,
    };
    let class = classify_bundle(&b);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));
}
