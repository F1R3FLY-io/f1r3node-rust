use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
struct PreconditionCase {
    name: &'static str,
    holds: bool,
    class: DivergenceClass,
}

#[test]
fn uc_92_dropped_preconditions_are_classified_not_exploitable() {
    let cases = [
        PreconditionCase {
            name: "closure_bound",
            holds: false,
            class: classify(DivergenceReason::PreconditionFuzzingBoundary),
        },
        PreconditionCase {
            name: "canonical_record_key",
            holds: false,
            class: classify(DivergenceReason::ProjectionBoundary),
        },
        PreconditionCase {
            name: "checked_arithmetic",
            holds: false,
            class: classify(DivergenceReason::ProjectionBoundary),
        },
        PreconditionCase {
            name: "all",
            holds: true,
            class: DivergenceClass::Bisimilar,
        },
    ];

    for case in cases {
        assert!(frontier_classification_ok(case.class), "{case:?}");
        if case.holds {
            assert_eq!(case.class, DivergenceClass::Bisimilar, "{case:?}");
        } else {
            assert_ne!(case.class, DivergenceClass::Bisimilar, "{case:?}");
        }
        assert_ne!(case.name, "");
    }
}
