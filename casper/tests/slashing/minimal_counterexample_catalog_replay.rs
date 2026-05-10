use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
struct CatalogCase {
    name: &'static str,
    class: DivergenceClass,
    mitigation: &'static str,
}

fn catalog() -> Vec<CatalogCase> {
    vec![
        CatalogCase {
            name: "weighted_closure_bound",
            class: classify(DivergenceReason::PreconditionFuzzingBoundary),
            mitigation: "enforce_closure_bound",
        },
        CatalogCase {
            name: "delimiter_free_record_key",
            class: classify(DivergenceReason::ProjectionBoundary),
            mitigation: "canonical_pair_key",
        },
        CatalogCase {
            name: "proposer_fairness",
            class: classify(DivergenceReason::ProposerFairnessBoundary),
            mitigation: "detect_neglect_or_include_report",
        },
        CatalogCase {
            name: "checked_arithmetic",
            class: classify(DivergenceReason::ProjectionBoundary),
            mitigation: "checked_add",
        },
    ]
}

#[test]
fn uc_98_every_minimal_counterexample_has_mitigation() {
    for case in catalog() {
        assert!(frontier_classification_ok(case.class), "{case:?}");
        assert_ne!(
            case.class,
            DivergenceClass::UnexpectedDivergence,
            "{case:?}"
        );
        assert_ne!(case.name, "");
        assert_ne!(case.mitigation, "");
    }
}
