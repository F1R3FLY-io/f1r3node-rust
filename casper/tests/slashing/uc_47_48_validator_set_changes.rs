// UC-47 + UC-48 — Validator-set changes during pending slash.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12.
// Theorems: T-Idem, T-10 (fork-choice exclusion).
//
// UC-47: a new validator bonds while a slash is pending; the slash
// applies to the original target without affecting the new bond.
// UC-48: a validator (not the slash target) leaves the active set
// while a slash is pending; the slash still applies to its target.
// Both UCs validate that validator-set changes do not interfere
// with in-flight slashes — slash semantics depend only on the
// target's own state.

use super::harness::SlashingTestHarness;

#[test]
fn uc_47_new_validator_joins_during_pending_slash() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // v0 equivocates → record minted, slash pending.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // A new validator v_new joins the active set with a positive bond.
    let join_result = harness.try_bond("v_new", 50);
    assert!(join_result.is_ok());
    assert!(harness.is_active("v_new"));
    assert_eq!(harness.bond("v_new"), 50);

    // Now apply the slash to v0. The new validator is unaffected.
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));
    assert_eq!(harness.bond("v_new"), 50,
        "newly-bonded validator unaffected by v0's slash");
    assert!(harness.is_active("v_new"));
    assert_eq!(harness.coop_vault(), 100,
        "vault gains exactly v0's prior bond, not v_new's");
}

#[test]
fn uc_48_other_validator_leaves_during_pending_slash() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record minted.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // Independently, v2 leaves (slash for some other reason).
    let _ = harness.execute_slash("v2");
    assert!(!harness.is_active("v2"));

    // Now apply the pending slash to v0. v2's exit doesn't affect
    // v0's slash semantics.
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.bond("v0"), 0);
    assert!(!harness.is_active("v0"));

    // Coop vault has both bonds.
    assert_eq!(harness.coop_vault(), 200);

    // v1 (the only remaining bonded validator) is unaffected.
    assert_eq!(harness.bond("v1"), 100);
    assert!(harness.is_active("v1"));
}
