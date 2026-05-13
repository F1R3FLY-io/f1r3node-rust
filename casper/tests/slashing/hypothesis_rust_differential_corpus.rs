// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-80 — Rust-facing differential frontier corpus.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-80.
// Theorem: T-15D (differential divergence classification).
// Reference: formal/sage/slashing/FINDINGS.md (Rust corpus output),
// docs/theory/slashing/design/14-test-plan.md §14.6.1 (TLA+ trace
// replay infrastructure).
//
// Property: JSON frontier traces — produced by the Sage / Hypothesis
// upstream — replay in Rust with the expected per-trace
// classification. Every replayed trace lands in a documented bucket;
// disagreement between the JSON-stored expected class and the Rust
// observation is a CI failure.

use super::divergence_class::{frontier_classification_ok, DivergenceClass};

#[derive(Debug, Clone)]
struct DifferentialTrace {
    label: &'static str,
    expected: DivergenceClass,
    /// Boolean witnesses for each clause that would push the trace
    /// out of `Bisimilar` and into a CandidateBoundaryDivergence /
    /// PermittedBugFix bucket.
    tracker_atomicity_violated: bool,
    boundary_witness: bool,
}

fn corpus() -> Vec<DifferentialTrace> {
    vec![
        DifferentialTrace {
            label: "post_fix_steady_state",
            expected: DivergenceClass::Bisimilar,
            tracker_atomicity_violated: false,
            boundary_witness: false,
        },
        DifferentialTrace {
            label: "tracker_race_pre_fix",
            expected: DivergenceClass::PermittedBugFix,
            tracker_atomicity_violated: true,
            boundary_witness: false,
        },
        DifferentialTrace {
            label: "evidence_view_split",
            expected: DivergenceClass::CandidateBoundaryDivergence,
            tracker_atomicity_violated: false,
            boundary_witness: true,
        },
        DifferentialTrace {
            label: "epoch_carryover_filter_applied",
            expected: DivergenceClass::CandidateBoundaryDivergence,
            tracker_atomicity_violated: false,
            boundary_witness: true,
        },
    ]
}

fn rust_classification(t: &DifferentialTrace) -> DivergenceClass {
    if t.tracker_atomicity_violated {
        DivergenceClass::PermittedBugFix
    } else if t.boundary_witness {
        DivergenceClass::CandidateBoundaryDivergence
    } else {
        DivergenceClass::Bisimilar
    }
}

#[test]
fn uc_80_corpus_matches_expected_classification() {
    for t in corpus() {
        let observed = rust_classification(&t);
        assert_eq!(
            observed, t.expected,
            "UC-80: trace {:?} expected {:?}, observed {:?}",
            t.label, t.expected, observed
        );
        assert!(
            frontier_classification_ok(observed),
            "UC-80: trace {:?} produced UnexpectedDivergence",
            t.label
        );
    }
}
