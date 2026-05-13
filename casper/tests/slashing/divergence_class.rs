// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// DivergenceClass — Rust mirror of the Rocq classification at
// `formal/rocq/slashing/theories/Bisimulation.v:520`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.4
// (UC-76..UC-86 use this enum to classify Sage/Hypothesis frontier
// witnesses into documented buckets).
//
// `Bisimilar`              — observationally equivalent.
// `PermittedBugFix`        — the documented bug-fix delta is the
//                            sole reason for divergence (e.g. T-9.2
//                            tracker atomicity).
// `CandidateBoundaryDivergence` — boundary witness whose
//                            classification awaits implementation
//                            intent confirmation (current-validator,
//                            evidence-view, epoch-carryover,
//                            proposer-fairness, projection).
// `UnexpectedDivergence`   — must NOT occur under the audited
//                            invariants; surfacing one is a CI
//                            failure.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DivergenceClass {
    Bisimilar,
    PermittedBugFix,
    CandidateBoundaryDivergence,
    UnexpectedDivergence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DivergenceReason {
    TrackerAtomicity,
    DetectorTotalityDistinctChildren,
    CurrentValidatorBoundary,
    EvidenceViewBoundary,
    EpochCarryoverBoundary,
    ProposerFairnessBoundary,
    ProjectionBoundary,
    PreconditionFuzzingBoundary,
    PartitionGossipBoundary,
    ObjectiveGuidedBoundary,
    RustReplayProjectionBoundary,
    RustViewProjectionBoundary,
    DeepThreatModelBoundary,
    DagTraceBoundary,
    AdversarialCampaignBoundary,
    DifferentialOraclePipelineBoundary,
    HorizonCampaignBoundary,
    HorizonV2Boundary,
    Unexpected,
}

pub fn classify(reason: DivergenceReason) -> DivergenceClass {
    use DivergenceReason::*;
    match reason {
        TrackerAtomicity | DetectorTotalityDistinctChildren => DivergenceClass::PermittedBugFix,
        CurrentValidatorBoundary
        | EvidenceViewBoundary
        | EpochCarryoverBoundary
        | ProposerFairnessBoundary
        | ProjectionBoundary
        | PreconditionFuzzingBoundary
        | PartitionGossipBoundary
        | ObjectiveGuidedBoundary
        | RustReplayProjectionBoundary
        | RustViewProjectionBoundary
        | DeepThreatModelBoundary
        | DagTraceBoundary
        | AdversarialCampaignBoundary
        | DifferentialOraclePipelineBoundary
        | HorizonCampaignBoundary
        | HorizonV2Boundary => DivergenceClass::CandidateBoundaryDivergence,
        Unexpected => DivergenceClass::UnexpectedDivergence,
    }
}

pub fn divergence_allowed(class: DivergenceClass) -> bool {
    matches!(
        class,
        DivergenceClass::Bisimilar | DivergenceClass::PermittedBugFix
    )
}

/// Combined frontier-classification policy used by the
/// hypothesis-style UCs: every witness must land in
/// `Bisimilar`, `PermittedBugFix`, or
/// `CandidateBoundaryDivergence`. `UnexpectedDivergence` is a
/// CI failure.
pub fn frontier_classification_ok(class: DivergenceClass) -> bool {
    !matches!(class, DivergenceClass::UnexpectedDivergence)
}
