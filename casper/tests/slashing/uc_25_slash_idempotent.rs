// UC-25 — Slash idempotence (T-Idem) example trace.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-25.
// Theorem: T-Idem (alias T-9, `t_idem_slash_idempotent`,
// formal/rocq/slashing/theories/PoSContract.v:117).
// Reference: design/06-proposing-and-effect.md §6.5.
//
// Concrete trace of T-Idem (the property-test version is at
// `prop_t_idem_slash_idempotence.rs`): the second slash on an
// already-slashed validator is a no-op — bond stays at 0, coop
// vault is not double-charged, slashed-set membership is
// preserved.

use super::harness::SlashingTestHarness;

#[test]
fn uc_25_double_slash_is_idempotent() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // First slash: v0 has bond 100; coop vault gains 100.
    let r1 = harness.execute_slash("v0");
    assert!(r1.success);
    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.coop_vault(), 100);
    assert!(!harness.is_active("v0"));
    assert!(harness.pos_state.slashed.contains("v0"));

    // Second slash: idempotent no-op.
    let r2 = harness.execute_slash("v0");
    assert!(r2.success);
    assert_eq!(harness.bond("v0"), 0, "bond stays at 0");
    assert_eq!(harness.coop_vault(), 100, "coop vault NOT double-charged");
    assert!(!harness.is_active("v0"));
    assert!(harness.pos_state.slashed.contains("v0"));

    // Third slash through to confirm stability.
    let r3 = harness.execute_slash("v0");
    assert!(r3.success);
    assert_eq!(harness.coop_vault(), 100,
        "k-th slash never exceeds the original bond contribution");
}
