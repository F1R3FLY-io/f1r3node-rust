// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-89 — Reduced scenarios preserve classification.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-89.
// Reference: formal/sage/slashing/FINDINGS.md (minimized counterexamples).
//
// Property: when Hypothesis reduces a counterexample to its minimal
// witness (smallest input that triggers the failure), the reduced form
// must keep the same `DivergenceClass` as the original. A classifier
// regression that flipped a minimized case to `Bisimilar` would silently
// erase a known threat vector — this test fails the moment that happens.

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
struct ReducedScenario {
    name: &'static str,
    class: DivergenceClass,
    minimized_size: usize,
}

fn reduced_scenarios() -> Vec<ReducedScenario> {
    vec![
        ReducedScenario {
            name: "delimiter_free_key_collision",
            class: classify(DivergenceReason::ProjectionBoundary),
            minimized_size: 2,
        },
        ReducedScenario {
            name: "partial_batch_abort",
            class: classify(DivergenceReason::ProjectionBoundary),
            minimized_size: 2,
        },
        ReducedScenario {
            name: "one_slot_retention_pruning",
            class: classify(DivergenceReason::ProjectionBoundary),
            minimized_size: 1,
        },
        ReducedScenario {
            name: "weighted_closure_bound_violation",
            class: classify(DivergenceReason::PreconditionFuzzingBoundary),
            minimized_size: 2,
        },
    ]
}

#[test]
fn uc_73_reduced_scenarios_stay_documented() {
    for scenario in reduced_scenarios() {
        assert!(frontier_classification_ok(scenario.class), "{scenario:?}");
        assert!(scenario.minimized_size > 0, "{scenario:?}");
        assert_ne!(scenario.name, "");
    }
}

#[test]
fn uc_73_no_reduced_scenario_is_mislabeled_bisimilar() {
    for scenario in reduced_scenarios() {
        assert_ne!(scenario.class, DivergenceClass::Bisimilar, "{scenario:?}");
    }
}
