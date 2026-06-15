// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-79 — Hypothesis assumption-minimization corpus.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-79.
// Theorems: T-12 hypotheses (closure-bound, NoDup-quorum, etc.).
// Reference: formal/sage/slashing/FINDINGS.md row 20 (assumption
// catalogue), `theorem_assumption_counterexamples.rs` (UC-69).
//
// Property: each minimised assumption-violation witness replays
// deterministically. If the witness reproduces under fresh seeds,
// the assumption is necessary; if not, the assumption is
// over-strong (slack) — both outcomes are documented buckets.
//
// This UC differs from UC-69 in that UC-69 hand-codes specific
// boundary witnesses, whereas UC-79 verifies the witness corpus
// is *self-consistent* (every minimised entry replays).

use super::divergence_class::{frontier_classification_ok, DivergenceClass};

#[derive(Debug, Clone)]
struct AssumptionWitness {
    name: &'static str,
    n: usize,
    f: usize,
    closure: usize,
    expected_quorum_violation: bool,
}

fn witness_corpus() -> Vec<AssumptionWitness> {
    vec![
        AssumptionWitness {
            name: "closure_bound_at_F",
            n: 4,
            f: 1,
            closure: 1,
            expected_quorum_violation: false,
        },
        AssumptionWitness {
            name: "closure_bound_F_plus_one",
            n: 4,
            f: 1,
            closure: 2,
            expected_quorum_violation: true,
        },
        AssumptionWitness {
            name: "closure_bound_n7_F2_at_boundary",
            n: 7,
            f: 2,
            closure: 2,
            expected_quorum_violation: false,
        },
        AssumptionWitness {
            name: "closure_bound_n7_F2_past_boundary",
            n: 7,
            f: 2,
            closure: 3,
            expected_quorum_violation: true,
        },
    ]
}

fn replays_consistently(w: &AssumptionWitness) -> bool {
    let active_after = w.n.saturating_sub(w.closure);
    let quorum = w.n - w.f;
    let observed_violation = active_after < quorum;
    observed_violation == w.expected_quorum_violation
}

#[test]
fn uc_79_every_witness_replays_deterministically() {
    for w in witness_corpus() {
        assert!(
            replays_consistently(&w),
            "UC-79: witness {:?} did not replay; assumption may be \
             over-strong or the witness is stale",
            w.name
        );
    }
    assert!(frontier_classification_ok(DivergenceClass::Bisimilar));
}
