// UC-22 — Unbonded proposer's `prepare_slashing_deploys` returns
// an empty list (post-fix #8).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-22.
// Theorem: T-9.8 (`t_9_8_unbonded_proposer_no_slash`,
// formal/rocq/slashing/theories/BugFixUnbondedProposer.v).
// Reference: design/09-bug-fixes-and-rationale.md §9.9.
//
// Pre-fix: `prepare_slashing_deploys` filtered targets by their bond
// without checking the proposer's own bond, so an unbonded proposer
// emitted SlashDeploy objects that the PoS contract would later
// reject — wasted work + non-deterministic block-byte content.
// Post-fix: skip emission entirely when proposer's bond is zero.

use super::harness::SlashingTestHarness;

#[test]
fn uc_22_unbonded_proposer_emits_empty_slash_list() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Set up an outstanding equivocation record.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // A bonded proposer (v1) would issue the slash.
    let bonded_proposal = harness.simulate_slash_proposal("v1");
    assert_eq!(bonded_proposal, vec!["v0".to_string()],
        "bonded proposer issues slash deploy for the equivocator");

    // Slash v2 to make it unbonded.
    let _ = harness.execute_slash("v2");
    assert_eq!(harness.bond("v2"), 0);

    // Post-fix #8: unbonded v2 emits an empty list — no SlashDeploys.
    let unbonded_proposal = harness.simulate_slash_proposal("v2");
    assert!(unbonded_proposal.is_empty(),
        "post-fix #8: unbonded proposer skips slash emission");

    // An entirely-unknown validator (bond = 0 by default) also emits
    // empty.
    let unknown_proposal = harness.simulate_slash_proposal("v999");
    assert!(unknown_proposal.is_empty(),
        "post-fix #8: unknown/unbonded proposer skips slash emission");
}
