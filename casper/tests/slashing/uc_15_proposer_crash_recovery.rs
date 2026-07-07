// UC-15 — Slash survives proposer crash via on-chain record.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-15.
// Theorems: T-3, T-9.8.
//
// Scenario: validator v0 equivocates, but the proposer that observed it
// crashes before issuing a SlashDeploy. Because the dispatcher minted an
// EquivocationRecord (post-fix #1/#3), the *next* bonded proposer v1
// can read the record from the DAG and emit the pending slash. The slash
// then completes successfully — bond zeroed, coop vault credited.

use super::harness::SlashingTestHarness;

#[test]
fn uc_15_next_bonded_proposer_can_emit_pending_slash() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    let targets = harness.simulate_slash_proposal("v1");
    assert_eq!(targets, vec!["v0".to_string()]);

    let result = harness.execute_slash("v0");
    assert!(result.success);
    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.coop_vault(), 100);
}
