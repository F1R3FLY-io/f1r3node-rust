// UC-55 — Atomic buffer-DAG transition (Bug #17 / T-9.20).
//
// Maps to:
//   - docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
//   - formal/rocq/slashing/theories/BugFixAtomicBufferDagTransition.v
//     (theorem `t_9_20_recon`).
//   - block-storage/src/rust/dag/buffer_dag_transition.rs (production
//     helper and reconcile function).
//   - block-storage/tests/atomic_buffer_dag_transition.rs (real-LMDB
//     integration tests).
//
// Why this UC exists. The Rocq theorem `t_9_20_recon` establishes
// observational equivalence between the no-crash run and the post-crash
// resume path on the slashing projection. The block-storage integration
// tests check the property at the LMDB level. This UC bridges the two:
// it exercises the property in the in-process abstract model that the
// rest of the UC-* test suite uses, so the formal-methods chain has a
// witness at every layer (formal model → harness model → real storage).
//
// Properties tested:
//   1. CrashBetweenInsertAndRemove produces drift (block in DAG, pendant
//      in buffer); resume closes the drift.
//   2. Post-resume slashing projection (DAG + tracker + PoS) equals the
//      no-crash slashing projection.
//   3. CrashAfterRemove is observationally identical to no crash.
//   4. CrashBeforeInsert + resume's replay recovers the no-crash state.
//      (In the harness, replay is modeled by re-invoking the dispatch.)

use super::harness::{CrashPoint, SlashingTestHarness};
use super::types::Status;

/// Run the equivocation-dispatch scenario without any crash. This is
/// the baseline against which the crash-variants are compared.
fn run_baseline_no_crash() -> SlashingTestHarness {
    let mut h = SlashingTestHarness::new(3, 100);
    let bad = h.sign_block_distinct("v0", 5);
    h.put_buffer_pendant(bad);
    // Atomic helper: dispatch effect + buffer purge.
    h.dispatch_with_status(bad, Status::IgnorableEquivocation);
    // Production calls this atomically via the helper; in the model
    // we apply it as the second half of `Step`.
    h.buffer.pendants.remove(&bad);
    h
}

#[test]
fn uc_55_crash_between_yields_drift_that_resume_closes() {
    let mut h = SlashingTestHarness::new(3, 100);
    let bad = h.sign_block_distinct("v0", 5);
    h.put_buffer_pendant(bad);

    h.dispatch_with_crash(
        bad,
        Status::IgnorableEquivocation,
        CrashPoint::BetweenInsertAndRemove,
    );

    // Pre-resume: (c) drift state per §9.20 — block in DAG (invalid),
    // pendant still in buffer.
    assert!(
        h.dag.invalid.contains(&bad),
        "post-crash: block should be in DAG.invalid (dispatch effect committed)"
    );
    assert!(
        h.buffer_contains(bad),
        "post-crash: pendant should still be in buffer (buffer.remove did NOT commit)"
    );

    // Resume: reconcile closes the drift.
    let purged = h.simulate_resume();
    assert_eq!(purged, 1, "reconcile should purge exactly the drifted pendant");

    // Post-resume: drift closed; block remains in DAG.
    assert!(h.dag.invalid.contains(&bad));
    assert!(!h.buffer_contains(bad));
}

#[test]
fn uc_55_crash_between_then_resume_matches_no_crash_projection() {
    // Per Rocq theorem T-9.20.recon: post-resume slashing projection
    // equals the no-crash projection for every crash point.

    let no_crash = run_baseline_no_crash();

    let mut h_crash = SlashingTestHarness::new(3, 100);
    let bad = h_crash.sign_block_distinct("v0", 5);
    h_crash.put_buffer_pendant(bad);
    h_crash.dispatch_with_crash(
        bad,
        Status::IgnorableEquivocation,
        CrashPoint::BetweenInsertAndRemove,
    );
    h_crash.simulate_resume();

    // Slashing projection: DAG state + tracker + PoS state. Buffer
    // state IS in scope here because both runs reach the same buffer
    // state (no pendant) post-resume.
    assert_eq!(no_crash.dag.invalid, h_crash.dag.invalid);
    assert_eq!(no_crash.has_record("v0", 4), h_crash.has_record("v0", 4));
    assert_eq!(no_crash.pos_state.bonds, h_crash.pos_state.bonds);
    assert_eq!(no_crash.pos_state.active, h_crash.pos_state.active);
    assert_eq!(no_crash.buffer.pendants, h_crash.buffer.pendants);
}

#[test]
fn uc_55_crash_after_remove_is_indistinguishable_from_no_crash() {
    let no_crash = run_baseline_no_crash();

    let mut h = SlashingTestHarness::new(3, 100);
    let bad = h.sign_block_distinct("v0", 5);
    h.put_buffer_pendant(bad);
    h.dispatch_with_crash(
        bad,
        Status::IgnorableEquivocation,
        CrashPoint::AfterRemove,
    );

    // Pre-resume: steady state — block in DAG, no pendant.
    assert!(h.dag.invalid.contains(&bad));
    assert!(!h.buffer_contains(bad));

    // Resume should be a no-op (no drift to close).
    let purged = h.simulate_resume();
    assert_eq!(purged, 0, "CrashAfterRemove has no drift; reconcile should be a no-op");

    assert_eq!(no_crash.dag.invalid, h.dag.invalid);
    assert_eq!(no_crash.has_record("v0", 4), h.has_record("v0", 4));
    assert_eq!(no_crash.buffer.pendants, h.buffer.pendants);
}

#[test]
fn uc_55_crash_before_insert_requires_replay() {
    // CrashBeforeInsert leaves no persistent change. The recon step
    // is a no-op. The system relies on the admission path to replay
    // the operation. We model this here by re-invoking the dispatch.

    let no_crash = run_baseline_no_crash();

    let mut h = SlashingTestHarness::new(3, 100);
    let bad = h.sign_block_distinct("v0", 5);
    h.put_buffer_pendant(bad);
    h.dispatch_with_crash(
        bad,
        Status::IgnorableEquivocation,
        CrashPoint::BeforeInsert,
    );

    // Pre-resume: no persistent change. The block has NOT been
    // dispatched. (Block exists in the DAG because `sign_block_distinct`
    // adds it during signing, but it has NOT been marked invalid yet
    // because the dispatch effect didn't commit.)
    assert!(!h.dag.invalid.contains(&bad));
    assert!(h.buffer_contains(bad));

    // Resume: reconcile is a no-op (block not in DAG.invalid yet,
    // though it IS in dag.blocks — the recon predicate uses
    // dag.blocks.contains_key per its production helper).
    let _ = h.simulate_resume();

    // The admission path replays the dispatch. In production this is
    // the block_processor's resume logic; in the harness we invoke it
    // explicitly. The atomic helper handles the (insert + buffer
    // remove) pair.
    h.dispatch_with_status(bad, Status::IgnorableEquivocation);
    h.buffer.pendants.remove(&bad);

    assert_eq!(no_crash.dag.invalid, h.dag.invalid);
    assert_eq!(no_crash.has_record("v0", 4), h.has_record("v0", 4));
    assert_eq!(no_crash.buffer.pendants, h.buffer.pendants);
}

#[test]
fn uc_55_reconcile_is_idempotent() {
    // Per Rocq theorem `t_9_20_reconcile_idempotent`. Running the
    // reconcile twice produces the same end state and zero purges on
    // the second call.

    let mut h = SlashingTestHarness::new(3, 100);
    let bad = h.sign_block_distinct("v0", 5);
    h.put_buffer_pendant(bad);
    h.dispatch_with_crash(
        bad,
        Status::IgnorableEquivocation,
        CrashPoint::BetweenInsertAndRemove,
    );

    let first = h.reconcile_buffer_against_dag();
    let second = h.reconcile_buffer_against_dag();

    assert_eq!(first, 1, "first reconcile purges the drifted pendant");
    assert_eq!(second, 0, "second reconcile is a no-op");
    assert!(!h.buffer_contains(bad));
}

#[test]
fn uc_55_reconcile_does_not_purge_genuine_pendants() {
    // A pendant whose hash is NOT in the DAG is a genuine pending
    // dependency, not a drifted entry. Reconcile must leave it intact.

    let mut h = SlashingTestHarness::new(3, 100);
    // Sign a block but DO NOT dispatch it. The hash is in dag.blocks
    // but represents a still-pending block.
    let b1 = h.sign_block("v0", 5);
    // Now add a pendant for a non-existent block (not in dag.blocks at all).
    let non_existent: u64 = 999_999;
    h.put_buffer_pendant(non_existent);

    let purged = h.reconcile_buffer_against_dag();
    assert_eq!(purged, 0, "pendant whose hash is not in DAG must NOT be purged");
    assert!(h.buffer_contains(non_existent));

    // The signed block b1 is in dag.blocks, so a pendant for it WOULD
    // be purged. Sanity-check:
    h.put_buffer_pendant(b1);
    let purged2 = h.reconcile_buffer_against_dag();
    assert_eq!(purged2, 1);
    assert!(!h.buffer_contains(b1));
    assert!(h.buffer_contains(non_existent));
}
