// UC-20 — Slash applied during the propose round of another block.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-20.
// Theorems: T-Idem, T-9.8 (proposer-bond gate).
//
// Variant of UC-22: a previously-bonded proposer gets slashed
// mid-round; their slash deploys are dropped (post-fix #8 gate).

use super::harness::SlashingTestHarness;

#[test]
fn uc_20_slashed_proposer_emits_no_slash_deploys() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Outstanding equivocation record for v0.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 (bonded) would issue a slash deploy.
    let bonded_proposal = harness.simulate_slash_proposal("v1");
    assert_eq!(bonded_proposal, vec!["v0".to_string()]);

    // v1 itself gets slashed (e.g. for an unrelated reason).
    let _ = harness.execute_slash("v1");
    assert_eq!(harness.bond("v1"), 0);

    // Now v1 (unbonded) emits no slash deploys per post-fix #8.
    let unbonded_proposal = harness.simulate_slash_proposal("v1");
    assert!(unbonded_proposal.is_empty(),
        "post-fix #8: slashed-mid-round proposer drops slashes");
}
