// Pre-fix regression backstop for bug #5 (stake-0 silent
// classification — fix is in PoS bond contract).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.6.
// Out-of-band approach: this asserts the post-fix #5 invariant on
// the bond contract — `amount <= 0` is rejected with a deterministic
// error. The Rust detector branch at
// `equivocation_detector.rs:194-221` is option (b) ("future work")
// per design §9.6; option (a) — the explicit `amount <= 0` check
// in PoS.rhox — is the canonical post-fix and is what this test
// validates.

use super::harness::SlashingTestHarness;

#[test]
fn pre_fix_bug_5_zero_stake_bond_rejected() {
    let mut harness = SlashingTestHarness::new(0, 0);

    // Post-fix #5: bond with amount=0 is rejected.
    let zero = harness.try_bond("v_new", 0);
    assert!(zero.is_err());
    assert_eq!(
        zero.err().as_deref(),
        Some("Bond amount must be positive."),
        "post-fix #5: explicit error message for amount=0; pre-fix this passed silently"
    );

    // Negative amount also rejected.
    let neg = harness.try_bond("v_new2", -1);
    assert!(neg.is_err());

    // Positive amount succeeds.
    let pos = harness.try_bond("v_new3", 50);
    assert!(pos.is_ok());
    assert_eq!(harness.bond("v_new3"), 50);
    assert!(harness.is_active("v_new3"));
}

#[test]
fn pre_fix_bug_5_active_implies_bonded_holds() {
    // T-9.5 corollary: every active validator has positive bond.
    // This is the invariant Bug #5's fix preserves.
    let mut harness = SlashingTestHarness::new(0, 0);
    let _ = harness.try_bond("v_active", 100);

    // Try to make a stake-0 bonded validator: rejected.
    let _ = harness.try_bond("v_zero", 0);
    assert!(
        !harness.is_active("v_zero"),
        "post-fix #5: zero-stake validator never enters active set"
    );

    // The active set contains only bonded validators.
    for v in &harness.pos_state.active {
        assert!(
            harness.bond(v) > 0,
            "active_implies_bonded: {} has bond {}",
            v,
            harness.bond(v)
        );
    }
}
