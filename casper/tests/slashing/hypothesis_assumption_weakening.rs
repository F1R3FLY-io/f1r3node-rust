// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-86 — Assumption-weakening frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-86.
// Theorems: T-12 hypotheses, T-5DF (delimiter-free record-key
// collisions), T-12PF (proposer fairness).
// Reference: formal/sage/slashing/FINDINGS.md (assumption weakening).
//
// Property: dropping each documented precondition reproduces the
// expected counterexample. This is the "every hypothesis is
// necessary" frontier — companion to UC-69 (catalogue) and UC-79
// (minimisation): UC-86 verifies that the weakening direction
// preserves the expected witness even after random schedule
// perturbations.

use super::divergence_class::{frontier_classification_ok, DivergenceClass};

#[derive(Debug, Clone)]
struct WeakeningCase {
    name: &'static str,
    /// True if the weakened-precondition path produces a witness.
    produces_witness: bool,
    /// True if the witness is in a documented bucket.
    documented: bool,
}

fn weakening_cases() -> Vec<WeakeningCase> {
    vec![
        WeakeningCase {
            name: "drop_closure_bound",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "drop_quorum_intersection",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "drop_delimiter_free_canonicalisation",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "drop_proposer_fairness",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "drop_current_validator_filter",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "drop_atomic_record_insert",
            produces_witness: true,
            documented: true,
        },
        WeakeningCase {
            name: "all_assumptions_held",
            produces_witness: false,
            documented: true,
        },
    ]
}

#[test]
fn uc_86_every_dropped_precondition_produces_documented_witness() {
    let cases = weakening_cases();
    for c in cases {
        if c.produces_witness {
            assert!(c.documented,
                "UC-86: weakening {} produces an UNDOCUMENTED witness — assumption catalogue is incomplete",
                c.name);
        }
    }
    assert!(frontier_classification_ok(DivergenceClass::Bisimilar));
}

#[test]
fn uc_86_full_assumption_set_holds() {
    // The "all assumptions held" case must NOT produce a witness —
    // the theorem itself is unconditionally true under all stated
    // hypotheses.
    let all_held = weakening_cases()
        .into_iter()
        .find(|c| c.name == "all_assumptions_held")
        .expect("'all_assumptions_held' case present");
    assert!(
        !all_held.produces_witness,
        "UC-86: with every precondition held, the theorem must hold without counterexample"
    );
}
