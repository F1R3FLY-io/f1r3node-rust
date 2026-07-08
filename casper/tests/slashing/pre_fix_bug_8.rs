// Pre-fix regression backstop for bug #8 (`prepare_slashing_deploys`
// doesn't check the proposer's own bond).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.9.
// Out-of-band approach: this asserts the post-fix invariant — an
// unbonded proposer emits an empty slash list. The pre-fix path
// returned the equivocator anyway (filtered only by *target* bond,
// not by *proposer* bond); running this test against the parent of
// the bug-#8 fix commit reproduces the bug.

use super::harness::SlashingTestHarness;

#[test]
fn pre_fix_bug_8_unbonded_proposer_skips_slash() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record exists.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // Bring v1's bond to 0 by slashing.
    let _ = harness.execute_slash("v1");
    assert_eq!(harness.bond("v1"), 0);

    // Post-fix #8 invariant: an unbonded proposer (v1) emits an
    // empty SlashDeploy list. Pre-fix this assertion FAILS: the
    // pre-fix code returned `vec!["v0"]` because it never checked
    // the proposer's own bond.
    let proposal = harness.simulate_slash_proposal("v1");
    assert!(
        proposal.is_empty(),
        "post-fix #8: unbonded v1 must not propose a slash; pre-fix returns {:?}",
        proposal
    );
}
