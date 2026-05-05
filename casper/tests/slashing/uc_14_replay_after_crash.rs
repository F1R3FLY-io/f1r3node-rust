// UC-14 — Replay determinism across simulated crash boundary.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-14.
// Theorems: T-15 (bisim) corollary on replay determinism.
// Reference: design/12-failure-modes.md §12.2.3.
//
// Models a "crash" as constructing a fresh harness and replaying
// the same operation sequence. The harness state machine is purely
// deterministic in its inputs, so replay yields a byte-identical
// projection. Production-path version of this test (against real
// LMDB persistence) is part of Track 2's integration_admissible.rs.

use super::harness::SlashingTestHarness;

fn run(seed_offset: i64) -> SlashingTestHarness {
    let mut h = SlashingTestHarness::new(3, 100 + seed_offset);
    let _b = h.sign_block("v0", 5);
    let bad = h.sign_block_distinct("v0", 5);
    let _ = h.dispatch(bad);
    let _ = h.execute_slash("v0");
    let cited = h.sign_block_citing("v1", 6, bad);
    let _ = h.dispatch(cited);
    h
}

#[test]
fn uc_14_replay_yields_identical_projection() {
    let h1 = run(0);
    let h2 = run(0);

    for i in 0..3 {
        let v = format!("v{}", i);
        assert_eq!(h1.bond(&v), h2.bond(&v));
        assert_eq!(h1.is_active(&v), h2.is_active(&v));
    }
    assert_eq!(h1.coop_vault(), h2.coop_vault());

    let k1: std::collections::BTreeSet<_> = h1.tracker.records.keys().collect();
    let k2: std::collections::BTreeSet<_> = h2.tracker.records.keys().collect();
    assert_eq!(k1, k2);
}

#[test]
fn uc_14_different_seed_yields_different_projection() {
    // Sanity: changing the input changes the output (replay is
    // not vacuously satisfied by always returning the same value).
    // Initial stake = 100 + seed_offset; slashing v0 transfers
    // that into the coop vault.
    let h1 = run(0);
    let h2 = run(50);
    assert_ne!(h1.coop_vault(), h2.coop_vault(),
        "different stake must yield different coop_vault observable");
}
