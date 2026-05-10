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
