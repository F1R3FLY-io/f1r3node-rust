// Three-level neglect chain: rejection does not create slash evidence.
//
// Maps to: docs/casper/theory/slashing/slashing-specification.md §14, T-6
// (neglect detection bounded to one hop).
// Reference: design/08-two-level-and-collusion.md.
//
// Scenario: A equivocates. B cites A's bad block without slashing. B receives
// NeglectedEquivocation, but B does not receive an evidence record. C cites
// B's rejected block. C cannot inherit economic evidence from B's rejection.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn three_level_neglect_chain() {
    let mut harness = SlashingTestHarness::new(4, 100);

    // Level 0: A equivocates.
    let _a1 = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // Level 1: B cites A's bad block without slashing.
    let b_neglect = harness.sign_block_citing("v1", 6, bad);
    let s_b = harness.dispatch(b_neglect);
    assert_eq!(s_b, Status::NeglectedEquivocation);
    assert!(!harness.has_record("v1", 5));

    let c_neglect = harness.sign_block_citing("v2", 7, b_neglect);
    let s_c = harness.dispatch(c_neglect);
    assert_eq!(s_c, Status::Valid);
    assert!(!harness.has_record("v2", 6));

    let _ = harness.execute_slash("v0");

    let active = harness.fork_choice();
    assert_eq!(active.len(), 3);
    assert!(!active.contains(&"v0".to_string()));
    assert_eq!(harness.coop_vault(), 100);
}
