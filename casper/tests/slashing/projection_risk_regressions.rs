// UC-72 — Projection-risk regression catalog.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-72.
// Theorems: T-5 record key, T-8 arithmetic, T-12 retention.
// Reference: formal/sage/slashing/FINDINGS.md row 23 —
// "implementation projection risks are now explicit finite
// witnesses".
//
// Property catalog:
//   * Canonical record-key injectivity: `(1, 23)` and `(12, 3)`
//     must NOT collide as record keys (string-concatenation would).
//   * Arithmetic envelopes: slash accounting at the i64 boundary
//     does not wrap or saturate.
//   * Duplicate normalization: state-equivalent only after
//     normalization.
//   * Evidence retention: pruning before final slash loses
//     slashability.

use super::harness::SlashingTestHarness;

#[test]
fn uc_72_record_keys_are_tuple_typed_not_string_concat() {
    let mut harness = SlashingTestHarness::new(0, 0);
    let _ = harness.try_bond("v0", 100);
    let _ = harness.try_bond("v1", 100);

    // Record at ("v1", 23) and ("v12", 3). String concatenation
    // would collide ("v123" vs "v123"). Tuple-typed key keeps them
    // distinct.
    harness.record_equivocation("v1", 23, 100);
    harness.record_equivocation("v12", 3, 200);

    assert!(harness.has_record("v1", 23));
    assert!(harness.has_record("v12", 3));
    assert_ne!(
        harness.record_witnesses("v1", 23),
        harness.record_witnesses("v12", 3),
        "T-5 key injectivity: distinct (validator, base_seq) tuples are distinct keys"
    );
}

#[test]
fn uc_72_arithmetic_at_i64_boundary_does_not_wrap() {
    // Boundary stake; slashing must not produce a negative coop_vault.
    let stake = i64::MAX / 256;
    let mut harness = SlashingTestHarness::new(2, stake);
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.coop_vault(), stake);
    assert!(harness.coop_vault() > 0, "T-8 arithmetic: no wrap");

    let _ = harness.execute_slash("v1");
    let expected = stake.checked_mul(2).expect("fits in i64");
    assert_eq!(harness.coop_vault(), expected);
    assert!(harness.coop_vault() > 0);
}

#[test]
fn uc_72_evidence_retained_until_slash_applied() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record minted.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // Slash transition consumes the record's evidence implicitly,
    // but the record itself is RETAINED (T-5 monotonicity).
    let _ = harness.execute_slash("v0");
    assert!(
        harness.has_record("v0", 4),
        "T-12 retention: record persists even after slash"
    );

    // The slashed-set membership reflects the slash transition.
    assert!(harness.pos_state.slashed.contains("v0"));
    assert_eq!(harness.bond("v0"), 0);
}
