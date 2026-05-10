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
