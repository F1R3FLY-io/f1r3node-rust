use super::harness::SlashingTestHarness;

#[test]
fn uc_25_slash_transfers_prior_bond_to_coop_vault() {
    let mut harness = SlashingTestHarness::new(3, 100);

    let pre_bond = harness.bond("v2");
    let pre_coop = harness.coop_vault();
    let result = harness.execute_slash("v2");

    assert!(result.success);
    assert_eq!(harness.bond("v2"), 0);
    assert_eq!(harness.coop_vault(), pre_coop + pre_bond);
    assert!(!harness.is_active("v2"));
    assert_eq!(harness.bond("v0"), 100);
    assert_eq!(harness.bond("v1"), 100);
}
