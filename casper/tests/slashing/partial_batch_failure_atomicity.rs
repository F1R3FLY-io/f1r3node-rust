// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-71 — Partial batch slash failure atomicity.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-71.
// Theorem: T-IdemMany boundary (`bm_slash_many_order_independent`,
// formal/rocq/slashing/theories/PoSContract.v).
// Reference: formal/sage/slashing/FINDINGS.md row 12 + 23 — "batch
// slash order is observationally independent in the finite model"
// and "abort-on-first-failure batch slash semantics are
// order-dependent" (the projection risk).
//
// Property: if a batch of slashes is processed atomically (all-or-
// nothing) OR independently (each succeeds/fails on its own merit),
// the post-state is order-independent. The implementation projection
// risk is abort-on-first-failure semantics, which IS order-dependent.

use super::harness::SlashingTestHarness;

fn run_in_order(targets: Vec<&str>) -> SlashingTestHarness {
    let mut h = SlashingTestHarness::new(4, 100);
    for t in targets {
        let _ = h.execute_slash(t);
    }
    h
}

#[test]
fn uc_71_batch_slash_order_independent_under_idempotence() {
    // T-IdemMany: slashing all 4 validators in any order yields
    // the same post-state. This is the Sage witness from row 12
    // (24 permutations all produce the same result).
    let bonds = [100i64, 100, 100, 100];
    let total = bonds.iter().sum::<i64>();

    let h_a = run_in_order(vec!["v0", "v1", "v2", "v3"]);
    let h_b = run_in_order(vec!["v3", "v2", "v1", "v0"]);
    let h_c = run_in_order(vec!["v1", "v3", "v0", "v2"]);

    // All three orders produce the same coop vault.
    assert_eq!(h_a.coop_vault(), total);
    assert_eq!(h_b.coop_vault(), total);
    assert_eq!(h_c.coop_vault(), total);

    // All three produce the same slashed set.
    let s_a: std::collections::BTreeSet<_> = h_a.pos_state.slashed.iter().collect();
    let s_b: std::collections::BTreeSet<_> = h_b.pos_state.slashed.iter().collect();
    let s_c: std::collections::BTreeSet<_> = h_c.pos_state.slashed.iter().collect();
    assert_eq!(s_a, s_b);
    assert_eq!(s_a, s_c);
}

#[test]
fn uc_71_repeat_slashes_idempotent_in_batch() {
    // Even with duplicate slash calls, the post-state matches a
    // single application (T-Idem).
    let h_dup = run_in_order(vec!["v0", "v0", "v1", "v1", "v0"]);
    let h_unique = run_in_order(vec!["v0", "v1"]);

    assert_eq!(h_dup.coop_vault(), h_unique.coop_vault());
    assert_eq!(
        h_dup.pos_state.slashed.len(),
        h_unique.pos_state.slashed.len()
    );
}
