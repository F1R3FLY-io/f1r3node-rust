// T-Idem (worked example): double-slashing a validator is idempotent.
//
// Maps to: docs/theory/slashing/slashing-specification.md §14 T-Idem.
// Reference: design/11-worked-examples.md.
//
// Scenario: slash v0 once (bond 100 → 0, coop vault 0 → 100). Slash v0
// again. The second call must (a) succeed, (b) not change bonds, (c)
// not double-charge the coop vault, (d) keep the slashed set
// idempotent. This is the smallest worked example for the idempotence
// property; the corresponding proptest is `prop_t_idem_slash_idempotence`.

use super::harness::SlashingTestHarness;

#[test]
fn slash_idempotence_double_slash_is_idempotent() {
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
    assert_eq!(
        harness.coop_vault(),
        100,
        "k-th slash never exceeds the original bond contribution"
    );
}
