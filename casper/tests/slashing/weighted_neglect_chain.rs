// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-55 — Stake-weighted neglect chain.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-55.
// Theorem: T-12 weighted (`weighted_slash_iter_quorum_preservation`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 4 — "damage
// optimization finds chain amplification".
//
// Sage witness: n=4, stakes [3,3,3,3], fault=3, equivocators [3],
// edges [[0,1],[1,2],[2,3]], closure [0,1,2,3], extra slashed
// stake 9, depth 3.
//
// Property: a neglect chain that reaches a direct equivocator
// transitively slashes the entire chain. The total slashed stake
// equals the stakes of every validator in the closure; remaining
// active stake must satisfy the weighted-quorum bound.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_55_weighted_neglect_chain_amplification() {
    // Set up 4 validators each with stake 3 — Sage damage-optimizer
    // witness.
    let mut harness = SlashingTestHarness::new(4, 3);

    // v3 is the direct equivocator at seq=5.
    let _v3a = harness.sign_block("v3", 5);
    let bad = harness.sign_block_distinct("v3", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v3", 4));

    // Chain v0 → v1 → v2 → v3 of cite-without-slash neglect:
    //   v2 cites v3's bad block without slashing.
    let v2_neg = harness.sign_block_citing("v2", 6, bad);
    let s2 = harness.dispatch(v2_neg);
    assert_eq!(s2, Status::NeglectedEquivocation);
    assert!(harness.has_record("v2", 5));

    //   v1 cites v2's neglect block without slashing v2.
    let v1_neg = harness.sign_block_citing("v1", 7, v2_neg);
    let s1 = harness.dispatch(v1_neg);
    assert_eq!(s1, Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 6));

    //   v0 cites v1's neglect block without slashing v1.
    let v0_neg = harness.sign_block_citing("v0", 8, v1_neg);
    let s0 = harness.dispatch(v0_neg);
    assert_eq!(s0, Status::NeglectedEquivocation);
    assert!(harness.has_record("v0", 7));

    // T-12 weighted closure: all four validators end up slashed.
    let _ = harness.execute_slash("v3");
    let _ = harness.execute_slash("v2");
    let _ = harness.execute_slash("v1");
    let _ = harness.execute_slash("v0");

    assert_eq!(
        harness.coop_vault(),
        12,
        "Sage witness: total slashed stake = sum of all four bonds"
    );
    assert_eq!(
        harness.fork_choice().len(),
        0,
        "every validator slashed → empty active set"
    );
}
