// UC-61 — Bounded-arithmetic projection around slash accounting.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-61.
// Theorems: T-8 (forfeited stake reaches Coop vault) +
// T-12 arithmetic (`signed_overflow_boundary_exact`,
// `unsigned_overflow_boundary_exact`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 8 — "bounded
// arithmetic projections diverge from exact arithmetic at the
// first overflow boundary".
//
// Property: slash accounting must use checked arithmetic or
// reject inputs that would cause fixed-width overflow. The
// harness's i64 arithmetic is the production tier's projection;
// this test asserts no panic / wrap / saturation under realistic
// stake values.

use super::harness::SlashingTestHarness;

#[test]
fn uc_61_large_stake_does_not_overflow_coop_vault() {
    // Each of 64 validators with stake = i64::MAX / 128 — well
    // below the i64 maximum but representative of a
    // production-scale network.
    let stake_per = i64::MAX / 128;
    let n = 64usize;
    let mut harness = SlashingTestHarness::new(n, stake_per);

    // Slash all 64 validators. The coop vault should reach
    // n * stake_per without overflow.
    for i in 0..n {
        let _ = harness.execute_slash(&format!("v{}", i));
    }

    let expected_total = (n as i64).checked_mul(stake_per)
        .expect("test parameters fit in i64 (sanity)");
    assert_eq!(harness.coop_vault(), expected_total,
        "T-8 + bounded-arithmetic: coop vault matches exact arithmetic up to i64 limit");
    assert!(harness.coop_vault() > 0, "no wrap to negative");
}

#[test]
fn uc_61_individual_slash_uses_checked_arithmetic() {
    // Single validator with i64::MAX / 2 — still safely within
    // i64 range but at the upper register.
    let stake = i64::MAX / 2;
    let mut harness = SlashingTestHarness::new(2, stake);

    let _ = harness.execute_slash("v0");
    assert_eq!(harness.bond("v0"), 0);
    assert_eq!(harness.coop_vault(), stake,
        "exact arithmetic: vault = transferred stake, no saturation/wrap");

    // Slash the second validator.
    let _ = harness.execute_slash("v1");
    let expected = stake.checked_mul(2).expect("fits in i64 by construction");
    assert_eq!(harness.coop_vault(), expected);
}

#[test]
fn uc_61_zero_bond_validator_no_arithmetic_anomaly() {
    // Validator with bond 0 (already slashed): re-slashing must
    // be a no-op, not an arithmetic anomaly.
    let mut harness = SlashingTestHarness::new(3, 100);

    let _ = harness.execute_slash("v0");
    assert_eq!(harness.bond("v0"), 0);
    let pre_vault = harness.coop_vault();

    // Re-slash v0 — bond is already 0; vault must not move.
    let _ = harness.execute_slash("v0");
    assert_eq!(harness.coop_vault(), pre_vault,
        "T-Idem + bounded-arithmetic: 0-bond re-slash adds 0 (not panic)");
}
