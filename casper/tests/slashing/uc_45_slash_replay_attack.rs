// UC-45 — Replay attack on a slash deploy.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-45.
// Theorems: T-Idem, T-9.8.
// Reference: design/06-proposing-and-effect.md §6.5.
//
// Adversary replays an old slash deploy after the validator has
// already been slashed. T-Idem guarantees the second application
// is a no-op — bond stays at 0, vault is not double-charged,
// active set stays correct.

use super::harness::SlashingTestHarness;

#[test]
fn uc_45_slash_replay_is_no_op() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Initial slash applies normally.
    let r1 = harness.execute_slash("v0");
    assert!(r1.success);
    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.coop_vault(), 100);

    // Adversary replays the same slash deploy three more times.
    for _ in 0..3 {
        let result = harness.execute_slash("v0");
        assert!(result.success, "replay still returns success (T-Idem)");
        assert_eq!(
            harness.bond("v0"),
            0,
            "bond stays at 0 across replay attempts"
        );
        assert_eq!(
            harness.coop_vault(),
            100,
            "T-Idem: vault never double-charged"
        );
    }

    // Other validators unaffected.
    assert_eq!(harness.bond("v1"), 100);
    assert_eq!(harness.bond("v2"), 100);
    assert!(harness.is_active("v1"));
    assert!(harness.is_active("v2"));
}
