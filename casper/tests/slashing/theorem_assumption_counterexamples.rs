// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-69 — Theorem-assumption counterexample catalog.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-69.
// Theorem: T-12 hypotheses (Sage finding 20 — "the assumption
// counterexample catalog confirms the main theorem hypotheses are
// necessary").
// Reference: formal/sage/slashing/FINDINGS.md row 20.
//
// Property: each hypothesis in the main theorems (closure bound,
// quorum intersection bound, NoDup quorum inputs, etc.) is
// NECESSARY — removing it produces a small concrete failure.
//
// The harness models this with explicit boundary witnesses: at
// the BFT-bound boundary slashing is safe; one validator past it
// breaks quorum.

use super::harness::SlashingTestHarness;

#[test]
fn uc_69_closure_bound_is_necessary() {
    // n=4, F=1. Slash F validators → safe (active=3 ≥ n-F).
    let n = 4usize;
    let f = (n - 1) / 3;
    let mut harness_safe = SlashingTestHarness::new(n, 100);
    for i in 0..f {
        let _ = harness_safe.execute_slash(&format!("v{}", i));
    }
    assert!(
        harness_safe.fork_choice().len() >= n - f,
        "T-12 with closure ≤ F: quorum preserved"
    );

    // Slash F+1 validators → boundary violated, quorum lost.
    let mut harness_unsafe = SlashingTestHarness::new(n, 100);
    for i in 0..(f + 1) {
        let _ = harness_unsafe.execute_slash(&format!("v{}", i));
    }
    assert!(
        harness_unsafe.fork_choice().len() < n - f,
        "T-12 counterexample: closure > F violates quorum bound"
    );
}

#[test]
fn uc_69_active_implies_bonded_hypothesis_is_necessary() {
    // T-9.5 hypothesis: every active validator has bond > 0.
    // Removing this would let zero-stake validators be active.
    // The harness's try_bond enforces the hypothesis at the bond
    // contract level (post-fix #5).
    let mut harness = SlashingTestHarness::new(0, 0);
    assert!(
        harness.try_bond("v_zero", 0).is_err(),
        "T-9.5 hypothesis: zero stake cannot bond"
    );
}
