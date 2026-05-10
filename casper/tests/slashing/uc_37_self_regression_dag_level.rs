// UC-37 — DAG-level self-regression with a witness block.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-37.
// Theorem: T-9.6 (DAG-level), `t_9_6_self_regression_in_dag`,
// formal/rocq/slashing/theories/BugFixSelfRegression.v.
// Reference: design/09-bug-fixes-and-rationale.md §9.7.
//
// Variation of UC-06 (which exercises the validation-time
// classification): UC-37 confirms the DAG-level invariant — the
// post-fix dispatcher records the offender, and a witness block
// (a third validator's block citing both v0's later block and the
// regressing block) sees the inconsistency and is part of the
// detection chain.

use super::harness::SlashingTestHarness;
use super::types::{BlockMeta, Status};

#[test]
fn uc_37_self_regression_dag_with_witness() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Step 1: v0 publishes a block at seq=10.
    let b_high = harness.sign_block("v0", 10);

    // Step 2: v0 publishes a regressing block at seq=5 whose
    // creator-justification points to b_high.
    let regressing = b_high.wrapping_add(50_000);
    harness.dag.blocks.insert(regressing, BlockMeta {
        hash: regressing,
        sender: "v0".into(),
        seq: 5,
        justifications: vec![("v0".into(), b_high)],
        slash_targets: vec![],
    });

    // Dispatch the regressing block — JustificationRegression
    // classification (post-fix #6) and record minted (post-fix #3).
    let s = harness.dispatch(regressing);
    assert_eq!(s, Status::JustificationRegression);
    assert!(harness.has_record("v0", 4));

    // Step 3: a witness block by v1 cites the regressing block.
    // The harness's neglect detection (post-fix #1+#3) sees that
    // v0 has an outstanding record AND v1's block does not slash
    // v0 → v1 is NeglectedEquivocation.
    let witness = harness.sign_block_citing("v1", 12, regressing);
    let ws = harness.dispatch(witness);
    assert_eq!(
        ws,
        Status::NeglectedEquivocation,
        "witness that observed but didn't slash v0 is itself slashable"
    );
    assert!(
        harness.has_record("v1", 11),
        "the witness's neglect is recorded"
    );

    // Confirm the records partition correctly.
    let v0_records: Vec<_> = harness
        .tracker
        .records
        .keys()
        .filter(|(v, _)| v == "v0")
        .collect();
    let v1_records: Vec<_> = harness
        .tracker
        .records
        .keys()
        .filter(|(v, _)| v == "v1")
        .collect();
    assert_eq!(v0_records.len(), 1, "one record for v0 (the equivocation)");
    assert_eq!(v1_records.len(), 1, "one record for v1 (the neglect)");
}
