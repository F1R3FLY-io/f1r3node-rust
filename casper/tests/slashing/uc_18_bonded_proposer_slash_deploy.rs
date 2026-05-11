// UC-18 — Bonded proposer enumerates pending slash targets without self-cost.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-18.
// Theorems: T-9.8.
//
// Scenario: an offender v0 equivocates and the dispatcher records the
// evidence. A *bonded* proposer v1 then enumerates the pending slash
// candidates. The proposer-side bond must remain unchanged — issuing a
// SlashDeploy is a duty, not a cost — and v0 must appear in the target
// list exactly once.

use super::harness::SlashingTestHarness;

#[test]
fn uc_18_bonded_proposer_emits_pending_slash_deploy() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    let targets = harness.simulate_slash_proposal("v1");
    assert_eq!(targets, vec!["v0".to_string()]);
    assert_eq!(harness.bond("v1"), 100);
}
