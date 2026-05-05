// UC-13 — Replay determinism: running the same operation sequence
// twice yields identical post-states.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-13.
// Theorem: T-15 corollary (replay determinism); see
// design/10-bisimilarity.md §10.3 (replay-determinism vs.
// bisimilarity).
// Reference: design/12-failure-modes.md §12.2.3.
//
// Replay determinism is a foundational invariant: every node in
// the network must reach the same post-state after replaying the
// same sequence of blocks/operations, otherwise consensus splits.
// The harness's state machine is *purely deterministic* in its
// inputs — there are no clocks, no random choices, no nondeterministic
// IO — so replay produces a byte-identical projection.

use super::harness::SlashingTestHarness;

fn run_sequence(n: usize, stake: i64) -> SlashingTestHarness {
    let mut h = SlashingTestHarness::new(n, stake);
    let _b1 = h.sign_block("v0", 5);
    let bad = h.sign_block_distinct("v0", 5);
    let _ = h.dispatch(bad);
    let _ = h.execute_slash("v0");
    let cited = h.sign_block_citing("v1", 6, bad);
    let _ = h.dispatch(cited);
    let _ = h.execute_slash("v1");
    h
}

#[test]
fn uc_13_replay_yields_identical_projection() {
    let h1 = run_sequence(3, 100);
    let h2 = run_sequence(3, 100);

    // Bonds.
    for i in 0..3 {
        let v = format!("v{}", i);
        assert_eq!(h1.bond(&v), h2.bond(&v),
            "replay determinism: bond({}) must match", v);
        assert_eq!(h1.is_active(&v), h2.is_active(&v),
            "replay determinism: is_active({}) must match", v);
    }
    assert_eq!(h1.coop_vault(), h2.coop_vault(),
        "replay determinism: coop_vault must match");

    // Tracker key sets.
    let k1: std::collections::BTreeSet<_> = h1.tracker.records.keys().collect();
    let k2: std::collections::BTreeSet<_> = h2.tracker.records.keys().collect();
    assert_eq!(k1, k2, "replay determinism: tracker keys must match");

    // Slashed sets.
    let s1: std::collections::BTreeSet<_> = h1.pos_state.slashed.iter().collect();
    let s2: std::collections::BTreeSet<_> = h2.pos_state.slashed.iter().collect();
    assert_eq!(s1, s2, "replay determinism: slashed sets must match");
}
