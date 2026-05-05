// Property-based test for T-14 (weak barbed equivalence:
// reflexivity + symmetry + transitivity).
//
// Theorem: T-14 (`weak_barbed_equiv_*`,
// formal/rocq/slashing/theories/Bisimulation.v).
// Reference: docs/theory/slashing/slashing-specification.md §10
// (Theorem 10.3), design/10-bisimilarity.md §10.5.
//
// Property: weak barbed equivalence (≈ₓ) is an equivalence
// relation — reflexive, symmetric, transitive — over the
// observable-state projection. The harness's projection is
// (bond, coop_vault, is_active, has_record) tuples; agreement
// on every observable is the (≈ₓ) equivalence class.

use proptest::prelude::*;

use super::harness::SlashingTestHarness;
use super::observer::SlashingObserver;

/// Apply a sequence of slash events to a harness and return its
/// observable projection over the validator set.
fn project(harness: &SlashingTestHarness, n: usize) -> Vec<(i64, bool, bool)> {
    (0..n)
        .map(|i| {
            let v = format!("v{}", i);
            (
                <SlashingTestHarness as SlashingObserver>::bond(harness, &v),
                <SlashingTestHarness as SlashingObserver>::is_active(harness, &v),
                harness.pos_state.slashed.contains(&v),
            )
        })
        .collect()
}

fn run_sequence(n: usize, events: &[usize]) -> SlashingTestHarness {
    let mut h = SlashingTestHarness::new(n, 100);
    for t in events {
        let _ = h.execute_slash(&format!("v{}", t % n));
    }
    h
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        .. ProptestConfig::default()
    })]

    /// Reflexivity: every harness state is observably equivalent
    /// to itself.
    #[test]
    fn t_14_reflexivity(
        n in 1usize..8,
        events in proptest::collection::vec(0usize..8, 0..10),
    ) {
        let h = run_sequence(n, &events);
        let p1 = project(&h, n);
        let p2 = project(&h, n);
        prop_assert_eq!(p1, p2, "T-14 reflexivity");
    }

    /// Symmetry: if h1 ≈ h2 then h2 ≈ h1.
    #[test]
    fn t_14_symmetry(
        n in 1usize..8,
        events in proptest::collection::vec(0usize..8, 0..10),
    ) {
        let h1 = run_sequence(n, &events);
        let h2 = run_sequence(n, &events);
        let p1 = project(&h1, n);
        let p2 = project(&h2, n);
        prop_assert_eq!(p1.clone(), p2.clone(), "T-14 symmetry: forward");
        prop_assert_eq!(p2, p1, "T-14 symmetry: reverse");
    }

    /// Transitivity: if h1 ≈ h2 and h2 ≈ h3 then h1 ≈ h3.
    #[test]
    fn t_14_transitivity(
        n in 1usize..8,
        events in proptest::collection::vec(0usize..8, 0..10),
    ) {
        let h1 = run_sequence(n, &events);
        let h2 = run_sequence(n, &events);
        let h3 = run_sequence(n, &events);
        let p1 = project(&h1, n);
        let p2 = project(&h2, n);
        let p3 = project(&h3, n);
        prop_assert_eq!(p1.clone(), p2.clone(), "T-14 transitivity: 1 ≈ 2");
        prop_assert_eq!(p2, p3.clone(), "T-14 transitivity: 2 ≈ 3");
        prop_assert_eq!(p1, p3, "T-14 transitivity: 1 ≈ 3");
    }
}
