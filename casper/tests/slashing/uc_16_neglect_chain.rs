// UC-16 — Chain of neglect: A equivocates, B neglects, C neglects B.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-16.
// Theorem: T-11 (level-2 termination — the closure terminates
// even with cascading neglect).
//
// The post-fix dispatcher mints a record at every level of the
// neglect chain. Slashing applies to all three; closure size is 3.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_16_three_level_neglect_chain() {
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
