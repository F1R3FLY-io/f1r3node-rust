// UC-24 — Slash idempotence (T-Idem) trace.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-24.
// Theorem: T-Idem.
//
// Companion to `uc_25_slash_idempotent.rs`; UC-24 in the spec
// table is also tagged T-Idem. The two UCs differ in framing:
// UC-24 emphasizes the operational semantics (k-th slash is no-op),
// UC-25 emphasizes the equivalence semantics (post-state at k+1
// equals post-state at k). Both are required by the §14 catalogue.

use super::harness::SlashingTestHarness;

#[test]
fn uc_24_kth_slash_is_no_op_for_all_k() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // First slash: v0 → bond zeroed, vault gains 100.
    let r1 = harness.execute_slash("v0");
    assert!(r1.success);
    let bond_1 = harness.bond("v0");
    let coop_1 = harness.coop_vault();
    let active_1 = harness.is_active("v0");

    // Slashes 2..N: identical post-state.
    for k in 2..=10 {
        let rk = harness.execute_slash("v0");
        assert!(rk.success, "T-Idem: slash #{k} returns success");
        assert_eq!(harness.bond("v0"), bond_1, "T-Idem: bond stable at slash #{k}");
        assert_eq!(harness.coop_vault(), coop_1,
            "T-Idem: coop vault stable at slash #{k}");
        assert_eq!(harness.is_active("v0"), active_1,
            "T-Idem: is_active stable at slash #{k}");
    }
}
