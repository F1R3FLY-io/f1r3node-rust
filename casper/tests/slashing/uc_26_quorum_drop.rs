// UC-26 — F-neglectful quorum drop (BFT bound exceeded).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-26.
// Theorem: T-12 counter-example (when `|closure| > F`).
// Reference: design/08-two-level-and-collusion.md §8.4,
// design/12-failure-modes.md §12.3.1.
//
// Scenario: with n=4 validators and F = ⌊(n-1)/3⌋ = 1, slashing 2
// validators violates the BFT precondition. Active size drops to
// n - 2 = 2, which is below the required quorum n - F = 3. The
// protocol cannot achieve consensus until manual intervention.
//
// This is a *failure-mode* test — it asserts the system enters the
// documented post-quorum-loss state (active < n - F), not that
// consensus continues. Recovery is operator-driven (re-bond honest
// validators, update the validator set).

use super::harness::SlashingTestHarness;

#[test]
fn uc_26_two_of_four_breaks_quorum() {
    let n = 4usize;
    let f = (n - 1) / 3; // = 1
    assert_eq!(f, 1);

    let mut harness = SlashingTestHarness::new(n, 100);
    assert_eq!(
        (0..n).filter(|i| harness.is_active(&format!("v{}", i))).count(),
        n,
        "all four validators active initially"
    );

    // Slash 2 validators — exceeding F = 1.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v1");
    let active = (0..n)
        .filter(|i| harness.is_active(&format!("v{}", i)))
        .count();
    assert_eq!(active, 2, "only v2 and v3 remain active");

    // The BFT precondition is violated: active < n - F = 3.
    assert!(active < n - f, "quorum is lost (active={} < n-F={})", active, n - f);

    // The two slashed validators carry the documented after-effects.
    assert!(!harness.is_active("v0"));
    assert!(!harness.is_active("v1"));
    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.bond("v1"), 0);
    assert_eq!(harness.coop_vault(), 200);
}
