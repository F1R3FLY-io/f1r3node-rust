// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-88 — Objective-guided frontier excludes UnexpectedDivergence rows.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-88.
// Reference: formal/sage/objective_frontier_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Sage objective frontier scores each search row by class + extra-stake +
// slash-delay; the frontier is the set of rows that survive scoring. The
// post-fix invariant is: any row classed `UnexpectedDivergence` is excluded
// regardless of its stake/delay weights — economic levers cannot disguise
// a bisimilarity break. This test pins a frozen-objective row against the
// classifier so a regression in `DivergenceClass` definitions would fail.

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectiveRow {
    name: &'static str,
    class: DivergenceClass,
    extra_stake: i64,
    slash_delay: i64,
}

fn objective_score(row: ObjectiveRow) -> i64 {
    let class_score = match row.class {
        DivergenceClass::UnexpectedDivergence => 1_000,
        DivergenceClass::CandidateBoundaryDivergence => 100,
        DivergenceClass::PermittedBugFix => 20,
        DivergenceClass::Bisimilar => 0,
    };
    class_score + row.extra_stake + row.slash_delay
}

#[test]
fn uc_88_objective_guided_frontier_excludes_unexpected_rows() {
    let rows = [
        ObjectiveRow {
            name: "weighted_damage",
            class: classify(DivergenceReason::PreconditionFuzzingBoundary),
            extra_stake: 8,
            slash_delay: 0,
        },
        ObjectiveRow {
            name: "retention_delay",
            class: classify(DivergenceReason::ProjectionBoundary),
            extra_stake: 0,
            slash_delay: 5,
        },
        ObjectiveRow {
            name: "direct_detection",
            class: DivergenceClass::Bisimilar,
            extra_stake: 0,
            slash_delay: 0,
        },
    ];

    assert!(rows.iter().all(|row| frontier_classification_ok(row.class)));
    assert!(rows
        .iter()
        .all(|row| row.class != DivergenceClass::UnexpectedDivergence));
}

#[test]
fn uc_88_objective_ranking_prioritizes_boundary_review() {
    let boundary = ObjectiveRow {
        name: "boundary",
        class: classify(DivergenceReason::ProjectionBoundary),
        extra_stake: 0,
        slash_delay: 0,
    };
    let bisimilar = ObjectiveRow {
        name: "bisimilar",
        class: DivergenceClass::Bisimilar,
        extra_stake: 99,
        slash_delay: 0,
    };
    assert!(objective_score(boundary) > objective_score(bisimilar));
}
