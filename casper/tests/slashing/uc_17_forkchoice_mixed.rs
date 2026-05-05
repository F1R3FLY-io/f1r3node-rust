// UC-17 — Fork choice with mixed slashed/active validators.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-17.
// Theorem: T-10 (`fork_choice_exclusion`,
// formal/rocq/slashing/theories/ForkChoice.v).
// Reference: design/07-fork-choice-and-lifecycle.md.
//
// Property: the GHOST estimator counts only validators in the
// post-fix active set — slashed validators are excluded. After a
// mixed-state slashing event (some validators slashed, others
// still bonded), `fork_choice` reflects exactly the surviving
// active set.

use super::harness::SlashingTestHarness;

#[test]
fn uc_17_forkchoice_excludes_slashed_includes_active() {
    let mut harness = SlashingTestHarness::new(5, 100);

    // Initial: all five validators active.
    let fc0 = harness.fork_choice();
    assert_eq!(fc0.len(), 5);
    for i in 0..5 {
        assert!(fc0.contains(&format!("v{}", i)));
    }

    // Slash v0 and v3.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v3");

    let fc1 = harness.fork_choice();
    assert_eq!(fc1.len(), 3,
        "fork-choice excludes the two slashed validators");
    assert!(!fc1.contains(&"v0".to_string()));
    assert!(!fc1.contains(&"v3".to_string()));
    assert!(fc1.contains(&"v1".to_string()));
    assert!(fc1.contains(&"v2".to_string()));
    assert!(fc1.contains(&"v4".to_string()));

    // Each remaining active validator still has positive bond
    // (active_implies_bonded preserved).
    for v in &fc1 {
        assert!(harness.bond(v) > 0);
    }

    // Coop vault holds both slashed bonds.
    assert_eq!(harness.coop_vault(), 200);
}
