// UC-16 — Slashed validator's blocks stay in the DAG but exit fork choice.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-16.
// Theorems: T-7 (fork-choice exclusion of slashed validators).
// Reference: design/07-fork-choice-and-lifecycle.md.
//
// Scenario: v0 publishes a block, v1 cites it as parent, then v0 is
// slashed. Post-slash, both blocks remain in `dag.blocks` (we never
// delete history) but v0 disappears from the fork-choice tip — only the
// still-bonded v1 contributes weight.

use super::harness::SlashingTestHarness;

#[test]
fn uc_16_slashed_parent_remains_in_dag_but_not_fork_choice() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let parent = harness.sign_block("v0", 1);
    let child = harness.sign_block_citing("v1", 2, parent);
    let result = harness.execute_slash("v0");

    assert!(result.success);
    assert!(harness.dag.blocks.contains_key(&parent));
    assert!(harness.dag.blocks.contains_key(&child));
    assert!(!harness.fork_choice().contains(&"v0".to_string()));
    assert!(harness.fork_choice().contains(&"v1".to_string()));
}
