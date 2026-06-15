// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-66 — Evidence-view divergence.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-66.
// Theorem: T-12 view closure (Sage finding 18 — "two observers
// with different visible evidence can compute different closure
// sets").
// Reference: formal/sage/slashing/FINDINGS.md row 18.
//
// Property: each observer's closure is computed from THEIR
// visible-unreported evidence. Two observers seeing different
// evidence subsets compute different closures; observers with
// identical active evidence views compute identical closures.
//
// Modeled as two independent harnesses with distinct event
// sequences applied — the closure is whatever each harness's
// tracker contains.

use super::harness::SlashingTestHarness;

#[test]
fn uc_66_different_views_produce_different_closures() {
    let mut observer_a = SlashingTestHarness::new(3, 100);
    let mut observer_b = SlashingTestHarness::new(3, 100);

    // Observer A sees v0's equivocation.
    let _v0a = observer_a.sign_block("v0", 5);
    let bad_a = observer_a.sign_block_distinct("v0", 5);
    let _ = observer_a.dispatch(bad_a);

    // Observer B does NOT see any equivocation.
    let _v0b = observer_b.sign_block("v0", 5);

    // Closure divergence: A has a record for v0, B does not.
    assert!(
        observer_a.has_record("v0", 4),
        "Observer A's view shows v0 equivocated"
    );
    assert!(
        !observer_b.has_record("v0", 4),
        "Observer B's view does NOT show the equivocation"
    );
}

#[test]
fn uc_66_identical_views_produce_identical_closures() {
    let mut observer_a = SlashingTestHarness::new(3, 100);
    let mut observer_b = SlashingTestHarness::new(3, 100);

    // Both observers see v0's equivocation.
    let _v0a_a = observer_a.sign_block("v0", 5);
    let bad_a = observer_a.sign_block_distinct("v0", 5);
    let _ = observer_a.dispatch(bad_a);

    let _v0a_b = observer_b.sign_block("v0", 5);
    let bad_b = observer_b.sign_block_distinct("v0", 5);
    let _ = observer_b.dispatch(bad_b);

    // Identical closures.
    assert_eq!(
        observer_a.has_record("v0", 4),
        observer_b.has_record("v0", 4)
    );
    assert!(observer_a.has_record("v0", 4));
}
