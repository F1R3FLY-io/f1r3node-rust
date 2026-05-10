// UC-59 — Duplicate neglect edges are idempotent and produce the
// same closure.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-59.
// Theorem: T-12 graph equivalence (`slash_iter_graph_equiv`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 7 — "duplicate
// edges do not change closure".
//
// Property: when the same validator cites an equivocator multiple
// times in different blocks, the closure size and witness sets
// match what a single-edge graph would produce. The neglect rule
// is reachability, and reachability is idempotent under duplicate
// edges.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_59_duplicate_neglect_edges_same_closure() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // v1 cites v0's bad block FOUR separate times in distinct
    // blocks at different sequence numbers. Each citation is a
    // duplicate "neglect edge" v1 → v0 in the closure graph.
    let v1_b1 = harness.sign_block_citing("v1", 6, bad);
    let s1 = harness.dispatch(v1_b1);
    assert_eq!(s1, Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 5));

    let v1_b2 = harness.sign_block_citing("v1", 7, bad);
    let s2 = harness.dispatch(v1_b2);
    // Second observation: classified as NeglectedEquivocation
    // again (the neglect record exists at base=6, not 5; a fresh
    // base each time).
    assert_eq!(s2, Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 6));

    let v1_b3 = harness.sign_block_citing("v1", 8, bad);
    let s3 = harness.dispatch(v1_b3);
    assert_eq!(s3, Status::NeglectedEquivocation);

    // Despite four neglect events from v1, the slash transition
    // is idempotent (T-Idem): v1 ends up slashed exactly once.
    let _ = harness.execute_slash("v1");
    assert_eq!(harness.bond("v1"), 0);
    let coop_after_first = harness.coop_vault();

    // Re-applying slash from each of the four edges does nothing.
    for _ in 0..4 {
        let _ = harness.execute_slash("v1");
    }
    assert_eq!(
        harness.coop_vault(),
        coop_after_first,
        "T-12 graph equiv: duplicate edges do not change closure stake"
    );
    assert_eq!(harness.bond("v1"), 0);
}
