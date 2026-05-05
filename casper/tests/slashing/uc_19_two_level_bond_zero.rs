// UC-19 — Two-level slash where the neglecter has zero bond.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-19.
// Theorems: T-11, T-9.5.
// Reference: design/08-two-level-and-collusion.md, §9.6.
//
// Scenario: validator A equivocates. Validator B (already slashed,
// so bond=0) cites A's invalid block without including a SlashDeploy.
// B is classified `NeglectedEquivocation` post-fix; the slash
// transition against B is a no-op (T-Idem) since the bond is
// already zero, but the invariant `active_implies_bonded` (T-9.5)
// is preserved throughout — B is no longer in the active set.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_19_two_level_with_bond_zero_neglecter() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // First, slash v1 to bring its bond to 0 (preconditions).
    let _ = harness.execute_slash("v1");
    assert_eq!(harness.bond("v1"), 0);
    assert!(!harness.is_active("v1"));

    // v0 equivocates.
    let _a1 = harness.sign_block("v0", 5);
    let a1_prime = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(a1_prime);
    assert!(harness.has_record("v0", 4));

    // v1 (bond 0) cites v0 without slashing — second-level neglect.
    let b_negligent = harness.sign_block_citing("v1", 7, a1_prime);
    let s = harness.dispatch(b_negligent);
    assert_eq!(s, Status::NeglectedEquivocation,
        "T-11 still fires even when the neglecter is already bond-zero");

    // Slash transition against bond-zero v1 is a no-op (T-Idem) but
    // the active_implies_bonded invariant (T-9.5) holds: v1 is not
    // active, so bond-zero is fine; v0 will be slashed next.
    let _ = harness.execute_slash("v1"); // already slashed; no-op
    assert_eq!(harness.bond("v1"), 0);
    assert_eq!(harness.coop_vault(), 100, "no double-charge from v1");

    // T-9.5 invariant: every active validator has positive bond.
    for i in 0..3 {
        let v = format!("v{}", i);
        if harness.is_active(&v) {
            assert!(harness.bond(&v) > 0,
                "active_implies_bonded: validator {} active and positive bond", v);
        }
    }
}
