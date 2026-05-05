// UC-40 — Coop vault accounting under failed transfer (post-fix #4).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-40.
// Theorems: T-8 (forfeited stake reaches Coop vault when transfer
// succeeds), T-9.4 (transfer-failure safety).
// Reference: design/06-proposing-and-effect.md §6.5,
// design/12-failure-modes.md §12.2.4.
//
// Property: the Coop vault reflects exactly the bonds of validators
// whose slash transfer succeeded — failed transfers do not
// contribute. Successive slashes against the same validator are
// idempotent; failed transfers leave the vault untouched.

use super::harness::SlashingTestHarness;

#[test]
fn uc_40_vault_accounting_under_mixed_outcomes() {
    let mut harness = SlashingTestHarness::new(4, 250);

    // First slash: v0, transfer succeeds → vault gains 250.
    let r1 = harness.execute_slash_with_transfer_outcome("v0", true);
    assert!(r1.success);
    assert_eq!(harness.coop_vault(), 250);

    // Second slash: v1, transfer fails → vault unchanged, v1 still
    // has its bond.
    let r2 = harness.execute_slash_with_transfer_outcome("v1", false);
    assert!(!r2.success);
    assert_eq!(harness.coop_vault(), 250,
        "post-fix #4: failed transfer does NOT add to coop vault");
    assert_eq!(harness.bond("v1"), 250,
        "post-fix #4: failed transfer leaves bond intact");
    assert!(harness.is_active("v1"),
        "post-fix #4: validator stays in active set after failed transfer");

    // Third slash: v1 retried with success → vault gains 250 → 500.
    let r3 = harness.execute_slash_with_transfer_outcome("v1", true);
    assert!(r3.success);
    assert_eq!(harness.coop_vault(), 500);
    assert_eq!(harness.bond("v1"), 0);

    // Fourth slash: v2, transfer succeeds → vault gains 250 → 750.
    let r4 = harness.execute_slash_with_transfer_outcome("v2", true);
    assert!(r4.success);
    assert_eq!(harness.coop_vault(), 750);

    // T-Idem: re-slashing v0 (already slashed) is a no-op even if we
    // pretend the transfer succeeded.
    let r5 = harness.execute_slash_with_transfer_outcome("v0", true);
    assert!(r5.success);
    assert_eq!(harness.coop_vault(), 750,
        "T-Idem: re-slashing already-slashed validator does not double-charge");
}
