// UC-03 — Unrequested equivocation (Ignorable variant) is recorded post-fix.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-03.
// Theorem: T-9.1 (`t_9_1_ignorable_recorded`,
// formal/rocq/slashing/theories/BugFixIgnorable.v).
// Bug fix:  #1 (block_status::is_slashable IgnorableEquivocation = true)
//           paired with #3 (dispatcher mints record for every slashable
//           variant). See design/09-bug-fixes-and-rationale.md §9.1, §9.3.
//
// Pre-fix behaviour (bug present): the dispatcher silently dropped
// `IgnorableEquivocation` blocks via early-return. No `EquivocationRecord`
// was minted, so a Byzantine validator could equivocate freely as long
// as no honest validator pulled the bad block in as a dependency. DOS
// vector documented in design §9.1.
//
// Post-fix invariant: every detected equivocation — Admissible *or*
// Ignorable — produces an `EquivocationRecord` that the proposing layer
// can later turn into a `SlashDeploy`.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_03_ignorable_equivocation_is_recorded() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // v0 signs two distinct blocks at seq=5 — an equivocation.
    let _b1 = harness.sign_block("v0", 5);
    let b1_prime = harness.sign_block_distinct("v0", 5);

    // No other block cites b1_prime (unsolicited / unrequested),
    // so the dispatcher classifies it as IgnorableEquivocation.
    let status = harness.dispatch(b1_prime);
    assert_eq!(
        status,
        Status::IgnorableEquivocation,
        "unrequested equivocation classifies as Ignorable"
    );

    // Bug #1 + #3 post-fix invariant: the record is minted regardless
    // of the variant. Pre-fix this assertion failed (the dispatcher
    // returned Ok(dag.clone()) with no record).
    assert!(
        harness.has_record("v0", 4),
        "post-fix #1+#3: dispatcher mints EquivocationRecord for IgnorableEquivocation"
    );
    let witnesses = harness.record_witnesses("v0", 4);
    assert!(
        witnesses.contains(&b1_prime),
        "the recorded witnesses include the offending block hash"
    );
}

#[test]
fn uc_03b_admissible_after_record_exists() {
    // Variant: once an EquivocationRecord exists, a freshly observed
    // equivocation by the same validator at the same base_seq is
    // classified as Admissible (the dispatcher's earlier match arm).
    let mut harness = SlashingTestHarness::new(3, 100);
    let b1 = harness.sign_block("v0", 5);
    harness.record_equivocation("v0", 4, b1); // pre-existing record
    let b1_prime = harness.sign_block_distinct("v0", 5);
    let status = harness.dispatch(b1_prime);
    assert_eq!(status, Status::AdmissibleEquivocation);
    assert!(harness.has_record("v0", 4));
}
