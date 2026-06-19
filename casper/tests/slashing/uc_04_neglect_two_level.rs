// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-04 — Two-level neglect closure.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-04.
// Theorem: T-11 (`level_2_termination`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: design/08-two-level-and-collusion.md.
//
// Scenario: validator A equivocates. Validator B then publishes a
// block whose justifications cite A's invalid block but does NOT
// include a SlashDeploy targeting A. Per the two-level closure rule,
// B is itself slashable for "neglecting to slash" (NeglectedEquivocation
// classification). Validator C, by contrast, includes the slash
// deploy and stays valid.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_04_neglecter_classified_as_neglected_equivocation() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Step 1: A equivocates at seq=5.
    let _a1 = harness.sign_block("v0", 5);
    let a1_prime = harness.sign_block_distinct("v0", 5);
    let s1 = harness.dispatch(a1_prime);
    assert_eq!(s1, Status::IgnorableEquivocation);
    assert!(
        harness.has_record("v0", 4),
        "post-fix #1 + #3: record minted for A's equivocation"
    );

    // Step 2: B (v1) cites A's invalid block but does NOT slash A.
    let b_negligent = harness.sign_block_citing("v1", 6, a1_prime);
    let s2 = harness.dispatch(b_negligent);
    assert_eq!(
        s2,
        Status::NeglectedEquivocation,
        "T-11: B is classified NeglectedEquivocation for failing to slash A"
    );

    // Bug #3 catch-all: a record is minted for B too — the post-fix
    // dispatcher mints for every slashable status.
    assert!(
        harness.has_record("v1", 5),
        "post-fix #3: dispatcher mints record for NeglectedEquivocation"
    );
}

#[test]
fn uc_04_honest_slasher_stays_valid() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // A equivocates.
    let _a1 = harness.sign_block("v0", 5);
    let a1_prime = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(a1_prime);

    // C (v2) cites A AND issues a SlashDeploy for A.
    let c_honest = harness.sign_block_citing_with_slash("v2", 7, a1_prime, "v0");
    let s = harness.dispatch(c_honest);
    assert_eq!(
        s,
        Status::Valid,
        "honest slasher (citing + slashing) is not classified as Neglecting"
    );
    assert!(
        !harness.has_record("v2", 6),
        "honest slasher does not get a record minted"
    );
}
