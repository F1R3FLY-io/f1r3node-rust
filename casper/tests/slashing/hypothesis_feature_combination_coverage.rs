// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-82 — Feature-combination frontier coverage.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-82.
// Theorems: T-12HYP (hypothesis bound), T-15D (differential).
// Reference: formal/sage/slashing/FINDINGS.md (feature combinations).
//
// Property: every Cartesian-product combination of frontier features
// (current-validator filter ON/OFF × proposer-fairness ON/OFF ×
// projection arithmetic exact/fixed-width × …) lands in a documented
// bucket. The 2^k combinations cover all switch-state interactions.

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
struct FeatureSwitches {
    current_validator_filter: bool,
    proposer_fairness: bool,
    epoch_carryover: bool,
    fixed_width_arithmetic: bool,
}

fn classify_combination(s: FeatureSwitches) -> DivergenceClass {
    // Base case: every safety-relevant filter ON and exact arithmetic.
    if s.current_validator_filter
        && s.proposer_fairness
        && s.epoch_carryover
        && !s.fixed_width_arithmetic
    {
        return DivergenceClass::Bisimilar;
    }
    // Disabling current-validator filter → CurrentValidatorBoundary.
    if !s.current_validator_filter {
        return classify(DivergenceReason::CurrentValidatorBoundary);
    }
    // Disabling proposer fairness → ProposerFairnessBoundary.
    if !s.proposer_fairness {
        return classify(DivergenceReason::ProposerFairnessBoundary);
    }
    // Disabling epoch carryover → EpochCarryoverBoundary.
    if !s.epoch_carryover {
        return classify(DivergenceReason::EpochCarryoverBoundary);
    }
    // Fixed-width arithmetic → ProjectionBoundary.
    if s.fixed_width_arithmetic {
        return classify(DivergenceReason::ProjectionBoundary);
    }
    DivergenceClass::Bisimilar
}

#[test]
fn uc_82_all_2_4_combinations_documented() {
    for cv in [false, true] {
        for pf in [false, true] {
            for ec in [false, true] {
                for fw in [false, true] {
                    let s = FeatureSwitches {
                        current_validator_filter: cv,
                        proposer_fairness: pf,
                        epoch_carryover: ec,
                        fixed_width_arithmetic: fw,
                    };
                    let class = classify_combination(s);
                    assert!(
                        frontier_classification_ok(class),
                        "UC-82: combination {:?} produced UnexpectedDivergence",
                        s
                    );
                }
            }
        }
    }
}

#[test]
fn uc_82_all_filters_on_exact_arithmetic_is_bisimilar() {
    let s = FeatureSwitches {
        current_validator_filter: true,
        proposer_fairness: true,
        epoch_carryover: true,
        fixed_width_arithmetic: false,
    };
    assert_eq!(classify_combination(s), DivergenceClass::Bisimilar);
}
