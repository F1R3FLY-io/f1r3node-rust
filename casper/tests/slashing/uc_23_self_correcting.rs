// UC-23 — Self-correcting block (Rust widening, T-9.9).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-23.
// Theorem: T-9.9 (`t_9_9_self_correcting_admitted`,
// formal/rocq/slashing/theories/MainTheorem.v).
// Reference: design/09-bug-fixes-and-rationale.md §9.10.
//
// Bug #9's post-fix is in `validate.rs:1018-1029` (the only fix
// already applied in the Rust source pre-design): a block whose
// system_deploys include a slash for a known equivocator is
// admitted by validation, not rejected. Pre-fix (Scala original)
// the unconditional rejection meant honest-slasher blocks couldn't
// land. The harness models this via
// `sign_block_citing_with_slash` + the dispatcher's Valid arm.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_23_self_correcting_admitted() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Step 1: v0 equivocates → record minted.
    let _a1 = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // Step 2: v1 publishes a self-correcting block — cites v0's bad
    // block AND issues a SlashDeploy targeting v0.
    let correcting = harness.sign_block_citing_with_slash("v1", 7, bad, "v0");
    let s = harness.dispatch(correcting);

    // Post-fix #9: validation admits this block (Status::Valid).
    // Pre-fix: it was rejected as "Neglected" because the rejection
    // path didn't recognize the slash-system-deploy.
    assert_eq!(s, Status::Valid,
        "post-fix #9: validation admits self-correcting blocks");
    assert!(!harness.has_record("v1", 6),
        "honest slasher does NOT get a record minted");
    assert!(!harness.dag.invalid.contains(&correcting),
        "self-correcting block is not in the invalid index");
}
