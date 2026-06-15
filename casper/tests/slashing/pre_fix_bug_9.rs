// Pre-fix regression backstop for bug #9 (slash-system-deploy
// rejected by validation pre-fix).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.10.
// Out-of-band approach per design/14-test-plan.md §14.7.
//
// Post-fix #9 is already applied at `casper/src/rust/validate.rs:1018-1029`:
// a block whose `system_deploys` include a `Slash { invalid_block_hash,
// issuer_public_key }` targeting a known-equivocator does not trip
// the parents/follows or repeat-deploy checks. Pre-fix (Scala
// inheritance) the unconditional rejection meant honest-slasher
// blocks couldn't land — the network would converge on never
// punishing equivocators because every proposer's slash-bearing
// block was rejected.
//
// The harness's projection: a block built via
// `sign_block_citing_with_slash` mirrors the post-fix #9 admission
// path. Detection returns `Status::Valid` and no record is minted
// for the honest slasher; the equivocator's record (minted earlier
// via `dispatch`) remains intact.
//
// Pre-fix this assertion FAILS — the slasher's block would have
// been classified as `NeglectedEquivocation` (or rejected as
// `InvalidParents`) because the production validation pipeline
// pre-fix did not recognize the slash-system-deploy as a valid
// signal of intent.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn pre_fix_bug_9_self_correcting_block_admitted() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 equivocates → record minted.
    let _a1 = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);
    assert!(harness.has_record("v0", 4));

    // v1 publishes a self-correcting block — cites v0's bad block
    // AND issues a SlashDeploy targeting v0.
    let correcting = harness.sign_block_citing_with_slash("v1", 7, bad, "v0");
    let s = harness.dispatch(correcting);

    // Post-fix #9 invariant: validation admits this block.
    // Pre-fix it was classified as Neglected/InvalidParents and
    // rejected.
    assert_eq!(
        s,
        Status::Valid,
        "post-fix #9: validation admits self-correcting blocks; \
         pre-fix this returned Neglected/InvalidParents and the \
         honest slasher could never land their block"
    );
    assert!(
        !harness.has_record("v1", 6),
        "post-fix #9: honest slasher (v1) does NOT get a record"
    );
    assert!(
        !harness.dag.invalid.contains(&correcting),
        "post-fix #9: self-correcting block is NOT in invalid index"
    );

    // The equivocator's record remains intact — bug #9's fix
    // doesn't disturb the dispatcher's record-minting (bug #3).
    assert!(harness.has_record("v0", 4));
}
