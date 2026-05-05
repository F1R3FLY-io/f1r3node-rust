// UC-06 — Self-regression is detected post-fix.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-06.
// Theorem: T-9.6 (`t_9_6_self_regression_caught`,
// formal/rocq/slashing/theories/BugFixSelfRegression.v).
// Bug fix:  #6 (validate.rs justification_regressions self-filter
// removal). See design/09-bug-fixes-and-rationale.md §9.7.
//
// Pre-fix behaviour: `justification_regressions` filtered the sender's
// own latest-message entry out of the regression check (mirroring
// Scala's `filterNot(_._1 == sender)`). A block whose own
// creator-justification pointed to a later sender-block — without a
// distinct-hash equivocation — slipped past both checks: the
// equivocation detector compared hashes only, and the regression
// check skipped the self entry.
//
// Post-fix invariant: the regression check includes the sender's own
// self-justification, so a self-regressing block is classified as
// `JustificationRegression` and the post-fix dispatcher mints an
// EquivocationRecord (bug fix #3 catch-all).

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_06_self_regression_caught() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // v0 publishes b_high at seq=10 first.
    let b_high = harness.sign_block("v0", 10);
    // v0 then publishes b_low at seq=5 citing b_high in its
    // creator-justification — i.e. the new block's own self-justification
    // points to a *later* sender-block. The harness's sign_block uses
    // the sender's current latest as the auto-justification, so we
    // synthesize the regressing block by hand below: hash, sender,
    // seq=5, justifications=[(v0, b_high)].
    use super::types::BlockMeta;
    let regressing = b_high.wrapping_add(1000);
    harness.dag.blocks.insert(
        regressing,
        BlockMeta {
            hash: regressing,
            sender: "v0".into(),
            seq: 5,
            justifications: vec![("v0".into(), b_high)],
        },
    );

    let status = harness.dispatch(regressing);
    assert_eq!(
        status,
        Status::JustificationRegression,
        "post-fix #6: self-regression is detected via JustificationRegression"
    );

    // Bug #3 post-fix catch-all: the dispatcher mints a record for
    // every slashable status, including JustificationRegression.
    assert!(
        harness.has_record("v0", 4),
        "post-fix #3: dispatcher mints record for JustificationRegression"
    );
}
