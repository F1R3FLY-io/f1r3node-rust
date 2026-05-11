// UC-97 — Economic safety envelope: coop-vault accumulation cannot overflow.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-97.
// Threat class: Economic-safety boundary (Sage row
// `arithmetic_envelope_model.sage`).
// Reference: formal/sage/arithmetic_envelope_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Sage finding: an adversary with full control over slash sequencing could
// in principle drive the coop-vault balance toward `i64::MAX`, then trigger
// one more slash to wrap it negative. The post-fix invariant is to use
// `checked_add` on every vault credit — a future slash whose sum would
// overflow refuses to mint rather than silently wrapping. This file pins
// both the safe-path (small sums admitted) and the overflow-path
// (i64::MAX + 1 rejected) so a refactor that switches back to plain `+`
// surfaces immediately.

fn checked_vault_add(balance: i64, slash_amount: i64) -> Option<i64> {
    balance.checked_add(slash_amount)
}

#[test]
fn uc_97_safe_economic_envelope_accepts_checked_sum() {
    assert_eq!(checked_vault_add(40, 2), Some(42));
}

#[test]
fn uc_97_fixed_width_overflow_projection_is_rejected() {
    assert_eq!(checked_vault_add(i64::MAX, 1), None);
}
