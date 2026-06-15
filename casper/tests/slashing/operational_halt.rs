// Operational-halt boundary: slashing past the BFT bound shrinks the
// active set below quorum.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14
// (operational liveness).
// Reference: design/12-failure-modes.md.
//
// Scenario: n=7 validators, BFT bound F=⌊(n-1)/3⌋=2. Slashing F+2=4
// validators leaves only 3 active — below the n-F=5 quorum required for
// safety + liveness. This test does NOT assert "the system recovers" —
// it asserts the operator-visible *symptom*: the active set is exactly
// n - slashed. A misclassification or a partial-slash bug would leave
// the active count higher than the slash budget allows.

use super::harness::SlashingTestHarness;

#[test]
fn majority_slashed_active_set_below_quorum() {
    // n = 7, F = ⌊(7-1)/3⌋ = 2.
    let n = 7usize;
    let f = (n - 1) / 3;
    assert_eq!(f, 2);

    let mut harness = SlashingTestHarness::new(n, 50);

    // Slash F+2 = 4 validators (well above the BFT bound).
    for i in 0..(f + 2) {
        let v = format!("v{}", i);
        let _ = harness.execute_slash(&v);
    }

    let active = harness.fork_choice();
    assert_eq!(
        active.len(),
        n - (f + 2),
        "active set drops by exactly the slashed count"
    );
    assert!(
        active.len() < n - f,
        "quorum lost: active={} < n-F={}",
        active.len(),
        n - f
    );

    // The vault collected (f+2) * 50.
    assert_eq!(harness.coop_vault() as usize, (f + 2) * 50);

    // Every slashed validator has bond=0 and is excluded from
    // fork-choice.
    for i in 0..(f + 2) {
        let v = format!("v{}", i);
        assert_eq!(harness.bond(&v), 0);
        assert!(!active.contains(&v));
    }
}

#[test]
fn all_validators_slashed_active_set_empty() {
    // Pathological case: every validator is slashed.
    let n = 4usize;
    let mut harness = SlashingTestHarness::new(n, 100);

    for i in 0..n {
        let v = format!("v{}", i);
        let _ = harness.execute_slash(&v);
    }

    assert_eq!(
        harness.fork_choice().len(),
        0,
        "active set empty when every validator is slashed"
    );
    assert_eq!(harness.coop_vault() as usize, n * 100);
    assert_eq!(harness.pos_state.slashed.len(), n);
}
