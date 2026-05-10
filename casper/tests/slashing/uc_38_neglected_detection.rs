// UC-38 — `detect_neglected` soundness and completeness.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-38.
// Theorem: T-6 (`detect_neglected`,
// formal/rocq/slashing/theories/EquivocationDetector.v).
// Reference: design/04-detection-and-pipeline.md §4.4,
// design/08-two-level-and-collusion.md.
//
// Two complementary properties:
//   • Soundness: a block flagged NeglectedEquivocation actually
//     cites an equivocator (the witness has an outstanding record).
//   • Completeness: a block that cites an equivocator without
//     issuing a SlashDeploy is flagged NeglectedEquivocation.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_38_soundness_no_false_positive_neglect() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // No outstanding records — no validator can be neglected.
    let block = harness.sign_block("v1", 6);
    let s = harness.dispatch(block);
    assert_eq!(
        s,
        Status::Valid,
        "soundness: a block citing nobody-with-records is Valid"
    );
}

#[test]
fn uc_38_completeness_neglect_fired_when_record_unslashed() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record exists.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 cites v0's invalid block without slashing → NeglectedEquivocation.
    let citing_no_slash = harness.sign_block_citing("v1", 6, bad);
    let s = harness.dispatch(citing_no_slash);
    assert_eq!(
        s,
        Status::NeglectedEquivocation,
        "completeness: cite-without-slash triggers NeglectedEquivocation"
    );
}

#[test]
fn uc_38_honest_slasher_not_flagged() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record exists.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v2 cites v0 AND issues a SlashDeploy targeting v0 → Valid.
    let honest = harness.sign_block_citing_with_slash("v2", 7, bad, "v0");
    let s = harness.dispatch(honest);
    assert_eq!(
        s,
        Status::Valid,
        "soundness: honest slasher (cite + slash) is not classified Neglected"
    );
}

#[test]
fn uc_38_validator_not_in_justifications_does_not_trigger() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record exists.
    let _ = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v2 publishes a block whose justifications do NOT cite v0.
    // Even though v0 has an outstanding record, v2 didn't observe
    // it, so v2 is not Neglected.
    let unrelated = harness.sign_block("v2", 8);
    let s = harness.dispatch(unrelated);
    assert_eq!(
        s,
        Status::Valid,
        "soundness: only blocks citing the equivocator can be Neglected"
    );
}
