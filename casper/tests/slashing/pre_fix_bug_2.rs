// Pre-fix regression backstop for bug #2 (lock-free RMW on the
// equivocation tracker).
//
// Reference: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.2.
// Out-of-band approach per design/14-test-plan.md §14.7.
//
// Post-fix invariant: routing every read-modify-write on the
// EquivocationTrackerStore through
// `BlockDagKeyValueStorage::access_equivocations_tracker(closure)`
// — which holds `global_lock: Arc<Mutex<()>>` for the closure's
// duration — makes concurrent dispatches preserve every witness.
//
// True interleaving coverage lives in
// `loom_t_9_2_atomic_record.rs` (run via
// `RUSTFLAGS="--cfg loom" cargo test -p casper --test mod -- slashing::loom_t_9_2`).
// This file records the post-fix invariant as a SEQUENTIAL trace
// so a reader can understand what the loom test is verifying
// without needing to install or run loom.
//
// Pre-fix this assertion failed under racing dispatchers because
// the put-one inside `EquivocationTrackerStore::add` is atomic
// individually but the surrounding read-then-decide-then-write
// was not. Two threads observing absence in the same window would
// both insert a fresh `EquivocationRecord { witnesses: BTreeSet::new() }`,
// overwriting each other's witnesses.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn pre_fix_bug_2_atomic_rmw_preserves_all_witnesses() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Prepare three distinct equivocating blocks at (v0, seq=5) —
    // the workload that loom_t_9_2 exhaustively interleaves.
    let _b1 = harness.sign_block("v0", 5);
    let bad_a = harness.sign_block_distinct("v0", 5);
    let bad_b = harness.sign_block_distinct("v0", 5);
    let bad_c = harness.sign_block_distinct("v0", 5);

    // Sequentially dispatch — single-threaded simulation of three
    // concurrent observers.
    assert_eq!(harness.dispatch(bad_a), Status::IgnorableEquivocation);
    assert_eq!(harness.dispatch(bad_b), Status::AdmissibleEquivocation);
    assert_eq!(harness.dispatch(bad_c), Status::AdmissibleEquivocation);

    // Post-fix #2 invariant: exactly one record at (v0, base=4),
    // all three witnesses preserved. Pre-fix this could yield a
    // record with one or two witnesses depending on the lost-update
    // pattern.
    let v0_keys: Vec<_> = harness
        .tracker
        .records
        .keys()
        .filter(|(v, _)| v == "v0")
        .collect();
    assert_eq!(
        v0_keys.len(),
        1,
        "T-4: at most one record per (validator, base_seq)"
    );

    let witnesses = harness.record_witnesses("v0", 4);
    assert!(witnesses.contains(&bad_a));
    assert!(witnesses.contains(&bad_b));
    assert!(witnesses.contains(&bad_c));
    assert_eq!(
        witnesses.len(),
        3,
        "post-fix #2: atomic RMW preserves every witness; pre-fix \
         this would fail under racing dispatchers (see loom_t_9_2_atomic_record)"
    );
}
