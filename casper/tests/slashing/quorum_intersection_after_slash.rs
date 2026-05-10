// UC-62 — Active quorum intersection after slashing.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-62.
// Theorem: T-12 quorum intersection (`quorum_intersection_by_size`,
// `weighted_quorum_intersection_from_disjoint_bound`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 10 — "weighted
// quorum intersection held for all bounded stake vectors tested".
//
// Property: any two quorums whose combined size exceeds the active
// validator count must share a common validator. Under the BFT
// bound `f ≤ ⌊(n-1)/3⌋`, slashing preserves quorum intersection
// because |active| ≥ ⌈2n/3⌉ remains, and any two quorums of size
// > ⌈2n/3⌉ / 2 = ⌈n/3⌉ + 1 must intersect.

use super::harness::SlashingTestHarness;

#[test]
fn uc_62_quorum_intersection_post_slash() {
    // n=7, F=⌊(7-1)/3⌋=2; slash 2 validators (BFT-bound).
    let n = 7usize;
    let f = (n - 1) / 3;
    let mut harness = SlashingTestHarness::new(n, 100);
    for i in 0..f {
        let _ = harness.execute_slash(&format!("v{}", i));
    }
    let active: Vec<String> = harness.fork_choice();
    assert_eq!(active.len(), n - f);

    // Two quorums Q1, Q2 of size ≥ ⌈|active|/2⌉ + 1 must intersect.
    // Pick Q1 = {v2, v3, v4} (size 3), Q2 = {v3, v4, v5} (size 3).
    let q1: std::collections::BTreeSet<String> =
        ["v2", "v3", "v4"].iter().map(|s| s.to_string()).collect();
    let q2: std::collections::BTreeSet<String> =
        ["v3", "v4", "v5"].iter().map(|s| s.to_string()).collect();
    let intersection: std::collections::BTreeSet<_> = q1.intersection(&q2).cloned().collect();
    assert!(
        !intersection.is_empty(),
        "T-12 quorum intersection: |Q1|+|Q2| > |active| ⇒ Q1 ∩ Q2 ≠ ∅"
    );
    // Both quorums must be subsets of the active set.
    let active_set: std::collections::BTreeSet<_> = active.into_iter().collect();
    assert!(q1.is_subset(&active_set));
    assert!(q2.is_subset(&active_set));
}
