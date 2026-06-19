// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-92 — Dropped preconditions classify as not exploitable.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-92.
// Reference: formal/sage/theorem_assumption_counterexamples.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Threat model: an attacker constructs scenarios that *violate* a stated
// theorem precondition (e.g. canonical-record-key uniqueness, closure
// bound). The detector / dispatcher must downgrade those cases to a
// non-exploitable class (boundary divergence) rather than treating them
// as discovered counterexamples — a dropped precondition is by definition
// outside the theorem's domain and cannot disprove it.

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
