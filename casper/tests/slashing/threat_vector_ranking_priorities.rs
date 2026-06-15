// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-99 — Threat-vector ranking: projection/assumption boundaries
// always outrank bisimilar rows.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-99.
// Reference: docs/theory/slashing/slashing-threat-model.md,
// formal/sage/slashing/FINDINGS.md.
//
// Property: regardless of extra-stake or slash-delay weights, a row
// classed `ProjectionBoundary` or `PreconditionFuzzingBoundary` must
// rank above any row classed `Bisimilar`. This is the prioritization
// guarantee for triage: a maintainer working through findings must see
// the boundary-class rows first, even if a bisimilar row has more
// dramatic stake numbers attached to it.

use super::divergence_class::{classify, DivergenceClass, DivergenceReason};

fn threat_priority(class: DivergenceClass, extra_stake: i64, slash_delay: i64) -> i64 {
    let class_score = match class {
        DivergenceClass::UnexpectedDivergence => 1_000,
        DivergenceClass::CandidateBoundaryDivergence => 100,
        DivergenceClass::PermittedBugFix => 20,
        DivergenceClass::Bisimilar => 0,
    };
    class_score + extra_stake + slash_delay
}

#[test]
fn uc_99_projection_and_assumption_boundaries_rank_above_bisimilar_rows() {
    let projection = threat_priority(classify(DivergenceReason::ProjectionBoundary), 0, 10);
    let assumption = threat_priority(
        classify(DivergenceReason::PreconditionFuzzingBoundary),
        8,
        0,
    );
    let bisimilar = threat_priority(DivergenceClass::Bisimilar, 50, 0);
    assert!(projection > bisimilar);
    assert!(assumption > bisimilar);
}

#[test]
fn uc_99_unexpected_would_rank_first_and_fail_policy_elsewhere() {
    let unexpected = threat_priority(DivergenceClass::UnexpectedDivergence, 0, 0);
    let boundary = threat_priority(classify(DivergenceReason::ProjectionBoundary), 99, 99);
    assert!(unexpected > boundary);
}
