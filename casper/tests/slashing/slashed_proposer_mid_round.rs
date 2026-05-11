// Post-fix #8 enforcement: a proposer slashed mid-round emits no slash deploys.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14, T-9.8.
// Rocq: formal/rocq/slashing/theories/BugFixUnbondedProposer.v.
//
// Scenario: v1 is the bonded proposer with v0 as a pending slash target.
// v1 itself is then slashed (e.g. for an unrelated reason). After v1 is
// unbonded, calling `simulate_slash_proposal("v1")` must return an empty
// list — pre-fix #8 the proposer still emitted slash deploys despite
// being unbonded, producing UnauthorizedSlashDeploy blocks that other
// nodes would then slash *them* for. Cascading-slash bug.

use super::harness::SlashingTestHarness;

#[test]
fn slashed_proposer_emits_no_slash_deploys() {
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
    assert!(
        unbonded_proposal.is_empty(),
        "post-fix #8: slashed-mid-round proposer drops slashes"
    );
}
