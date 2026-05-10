// Pre-fix regression backstop for bug #4 (PoS slash transfer-failure
// FIXME).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.5.
// Out-of-band approach: this asserts the post-fix behaviour the
// PoS.rhox `match transferResult { (true, _) | (_, errorMessage) }`
// idiom guarantees. Pre-fix the slash flow at PoS.rhox:469 had a
// bare `for (_ <- transferDoneCh) { ...stateUpdateCh!(...) }` that
// unconditionally committed the new state and hung if the transfer
// returned `(false, errorMsg)`, breaking replay determinism.
//
// Post-fix invariant: when transfer fails, the slash returns
// `(false, "transfer failed")` deterministically AND the PoS state
// is left unchanged so the validator stays in EquivocatorRecorded
// for the next proposer to retry.

use super::harness::SlashingTestHarness;

#[test]
fn pre_fix_bug_4_transfer_failure_leaves_state_unchanged() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let initial_bond = harness.bond("v0");
    let initial_active = harness.is_active("v0");
    let initial_coop = harness.coop_vault();

    // Force the PoS transfer to fail.
    let result = harness.execute_slash_with_transfer_outcome("v0", false);
    assert!(
        !result.success,
        "post-fix #4: failed transfer returns (false, ...)"
    );
    assert_eq!(
        result.error.as_deref(),
        Some("transfer failed"),
        "post-fix #4: deterministic error message"
    );

    // Post-fix #4 invariant: state is unchanged. Pre-fix this
    // assertion FAILS because the bare `for (_ <- transferDoneCh)`
    // unconditionally committed `stateUpdateCh!(...)` even on
    // failure, mutating the bonds map and active set non-deterministically.
    assert_eq!(
        harness.bond("v0"),
        initial_bond,
        "post-fix #4: bond unchanged after failed transfer"
    );
    assert_eq!(
        harness.is_active("v0"),
        initial_active,
        "post-fix #4: active-set membership unchanged"
    );
    assert_eq!(
        harness.coop_vault(),
        initial_coop,
        "post-fix #4: coop vault unchanged"
    );
    assert!(
        !harness.pos_state.slashed.contains("v0"),
        "post-fix #4: validator is NOT marked slashed (retry is possible)"
    );
}

#[test]
fn pre_fix_bug_4_successful_transfer_unchanged_semantics() {
    // Sanity: when transfer succeeds, the slash applies as usual.
    let mut harness = SlashingTestHarness::new(2, 100);
    let result = harness.execute_slash_with_transfer_outcome("v0", true);
    assert!(result.success);
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.coop_vault(), 100);
}
