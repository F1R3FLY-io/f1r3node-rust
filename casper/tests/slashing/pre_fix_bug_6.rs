// Pre-fix regression backstop for bug #6 (self-regression filter).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.7.
// Out-of-band approach: this asserts the post-fix invariant that
// would FAIL on the parent of the bug-#6 fix commit (where
// validate.rs:895-899 had a `.filter(|(v, _)| v != &b.sender)` step
// that excluded the sender's own latest message from the
// regression check, letting self-regressing blocks slip past).
//
// Post-fix invariant: a block whose own creator-justification
// references a *later* sender-block is classified as
// JustificationRegression and the dispatcher mints a record.

use super::harness::SlashingTestHarness;
use super::types::{BlockMeta, Status};

#[test]
fn pre_fix_bug_6_self_regression_caught() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // v0 publishes a block at seq=10.
    let b_high = harness.sign_block("v0", 10);

    // v0 publishes a *regressing* block at seq=5 whose own self-
    // justification points to b_high (later seq). Pre-fix this
    // slipped past validation.
    let regressing = b_high.wrapping_add(7000);
    harness.dag.blocks.insert(regressing, BlockMeta {
        hash: regressing,
        sender: "v0".into(),
        seq: 5,
        justifications: vec![("v0".into(), b_high)],
        slash_targets: vec![],
    });

    let status = harness.dispatch(regressing);
    assert_eq!(
        status,
        Status::JustificationRegression,
        "post-fix #6: self-regression is detected; pre-fix this returned Valid"
    );

    // Bug #3 catch-all also mints a record.
    assert!(
        harness.has_record("v0", 4),
        "post-fix #3: dispatcher mints record for JustificationRegression"
    );
}
