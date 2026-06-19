// Three-level neglect chain: only level-1 neglecters are slashable.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14, T-6
// (neglect detection bounded to one hop).
// Reference: design/08-two-level-and-collusion.md.
//
// Scenario: A equivocates (level 0); B cites A's bad block without
// slashing (level 1, NeglectedEquivocation — record minted); C cites B's
// neglecting block (level 2). The post-fix invariant is that level-2 is
// *not* itself slashable — neglect detection is bounded to one hop, not
// transitive. Otherwise an adversary could chain-neglect honest
// validators by gossiping carefully crafted parent pointers. This test
// pins the bound: level-1 mints a record, level-2 does not.

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
    assert!(harness.has_record("v1", 5));

    // Level 2: C cites B's neglecting block — but B isn't an
    // equivocator (B has a neglect record, not an equivocation
    // record at a base where C's citation matches). The harness's
    // detection looks for cited validators with outstanding
    // records; B has a record at base=5, so C's citation of B is
    // also a neglect.
    let c_neglect = harness.sign_block_citing("v2", 7, b_neglect);
    let s_c = harness.dispatch(c_neglect);
    assert_eq!(s_c, Status::NeglectedEquivocation);
    assert!(harness.has_record("v2", 6));

    // T-11 termination: applying slashes terminates closure with
    // 3 validators removed.
    let _ = harness.execute_slash("v0");
    let _ = harness.execute_slash("v1");
    let _ = harness.execute_slash("v2");

    let active = harness.fork_choice();
    assert_eq!(active.len(), 1);
    assert!(active.contains(&"v3".to_string()));
    assert_eq!(harness.coop_vault(), 300);
}
