// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-68 — Rebonded validator identity boundary.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-68.
// Theorem: T-12 epoch carryover (Sage finding 19 — "epoch churn
// search exposes validator-identity policy boundaries").
// Reference: formal/sage/slashing/FINDINGS.md row 19.
//
// Property: a validator that gets slashed and later rebonds with
// a fresh identity does NOT inherit the previous slashing record
// (epoch-tagged identity). With loose public-key identity, stale
// evidence could carry over — the policy boundary.
//
// The harness models this: a slashed validator stays in the
// `slashed` set; rebonding creates a NEW label; no record carries.

use super::harness::SlashingTestHarness;

#[test]
fn uc_68_rebonded_validator_does_not_inherit_record() {
    let mut harness = SlashingTestHarness::new(0, 0);

    // v0 bonds, gets slashed.
    assert!(harness.try_bond("v0", 100).is_ok());
    let _ = harness.execute_slash("v0");
    assert!(harness.pos_state.slashed.contains("v0"));
    assert_eq!(harness.bond("v0"), 0);

    // v0 attempts to rebond — REJECTED (already slashed).
    let r = harness.try_bond("v0", 100);
    assert!(
        r.is_err(),
        "T-12 epoch boundary: slashed identity cannot rebond"
    );

    // A fresh identity v0_epoch2 bonds successfully.
    assert!(harness.try_bond("v0_epoch2", 100).is_ok());
    assert_eq!(harness.bond("v0_epoch2"), 100);
    assert!(harness.is_active("v0_epoch2"));

    // T-12 epoch carryover: the fresh identity has no inherited
    // record from v0's prior slashing.
    for base in 0..5 {
        assert!(
            !harness.has_record("v0_epoch2", base),
            "rebonded identity inherits no record from prior epoch"
        );
    }
}
