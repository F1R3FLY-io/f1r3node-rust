// P2-13 — Non-reentrancy contract for `access_equivocations_tracker`.
//
// The trait contract in
// `block-storage/src/rust/dag/equivocations_access.rs` states that
// the closure passed to `access_equivocations_tracker` MUST NOT
// recursively call back into the same method, nor any operation
// that acquires the implementor's internal lock (in production,
// `BlockDagKeyValueStorage::global_lock`). Doing so deadlocks the
// `parking_lot::RwLock<()>`-based implementation: a write guard is
// held for the closure's duration and a second `write()` from the
// same thread blocks forever.
//
// This file model-checks both branches of that contract using
// `loom::sync::RwLock<()>` as a stand-in for the production lock.
// Loom enumerates *every* preemption-bounded thread interleaving;
// the property we want is:
//
//   (1) Two parallel threads each doing the disciplined (single
//       acquisition) RMW always complete. No deadlock under any
//       interleaving.
//
//   (2) A thread that attempts a reentrant acquisition inside its
//       own closure deadlocks (loom panics with "model-check found
//       a deadlock"). We assert this branch is detected — the
//       contract is *enforced* by the lock, not by convention.
//
// Why a separate file (not bolted onto the existing T-9.2 loom
// tests):  the T-9.2 tests cover atomicity (locked vs. lock-free
// RMW preserves witnesses); this file covers *deadlock under
// reentrancy*, which is a different property. Both live next to
// the production lock in `block-storage` rather than in `casper`
// because the lock primitive itself is the unit-under-test.
//
// Run with:
//   cargo test -p block-storage --test loom_equivocations_tracker
//
// Loom enumerates interleavings whenever its API is invoked; no
// `--cfg loom` flag is needed because the tests use `loom::sync::*`
// types directly. See `casper/tests/slashing/loom_t_9_2_atomic_record.rs`
// for the broader rationale.

use loom::sync::{Arc, RwLock};
use loom::thread;

/// Shadow of the production `global_lock` — a parameterless RwLock.
/// The closures below mutate a shared counter to give loom an
/// observation point, but the contract under test is purely about
/// the lock primitive's reentrancy behaviour, not the counter.
fn shadow_critical_section<F>(lock: &Arc<RwLock<u64>>, f: F)
where
    F: FnOnce(&mut u64),
{
    let mut guard = lock.write().unwrap();
    f(&mut *guard);
}

/// Disciplined RMW: each invocation acquires the lock exactly once,
/// mutates, then releases. This mirrors the production
/// `access_equivocations_tracker` contract — the closure body does
/// not recursively call back into the lock-acquiring API.
fn disciplined_rmw(lock: &Arc<RwLock<u64>>) {
    shadow_critical_section(lock, |counter| {
        *counter = counter.saturating_add(1);
    });
}

/// Property (1): two parallel disciplined RMW threads always
/// complete. Loom enumerates every interleaving; if the lock were
/// somehow reentrancy-vulnerable to disciplined callers, this test
/// would surface a deadlock or stuck state on at least one
/// schedule.
#[test]
fn disciplined_parallel_rmw_terminates_under_every_interleaving() {
    loom::model(|| {
        let lock = Arc::new(RwLock::new(0u64));

        let h1 = {
            let lock = lock.clone();
            thread::spawn(move || disciplined_rmw(&lock))
        };
        let h2 = {
            let lock = lock.clone();
            thread::spawn(move || disciplined_rmw(&lock))
        };

        h1.join().expect("thread 1 should complete");
        h2.join().expect("thread 2 should complete");

        // Both increments observed.
        let final_value = *lock.read().unwrap();
        assert_eq!(final_value, 2, "both disciplined RMWs must commit");
    });
}

/// Property (2): the lock is *not* reentrant — a thread that holds
/// the write guard cannot re-acquire it from within the same
/// closure. We assert this directly via `try_write`, which is
/// observationally non-blocking: while the outer write guard is
/// live, an inner `try_write()` call yields `Err` (the contended
/// path). Calling the blocking `write()` from the same place would
/// instead self-deadlock — loom would surface that as a model-check
/// failure, but `try_write` lets us assert the property cleanly
/// without depending on loom's deadlock detection.
///
/// Loom enumerates every preemption interleaving; the inner
/// `try_write()` must observe contention on every schedule, because
/// the outer write guard is always live at that program point.
#[test]
fn reentrant_write_attempt_is_observably_contended() {
    loom::model(|| {
        let lock = Arc::new(RwLock::new(0u64));

        // Thread A: simulate the disallowed reentrant pattern. Acquire
        // the write guard, then inside the closure attempt a second
        // write acquisition via the non-blocking `try_write`. The
        // attempt MUST fail (lock is held exclusively by us), proving
        // the lock is not reentrant.
        let reentrant_probe = {
            let lock = lock.clone();
            thread::spawn(move || {
                let mut outer = lock.write().unwrap();
                *outer = outer.saturating_add(1);
                assert!(
                    lock.try_write().is_err(),
                    "inner try_write must observe contention from the outer write guard \
                     held by the same thread — proves the lock is non-reentrant"
                );
                drop(outer);
            })
        };

        // Thread B: disciplined RMW running concurrently. Must finish
        // on every interleaving where thread A's outer guard is
        // eventually released.
        let disciplined = {
            let lock = lock.clone();
            thread::spawn(move || disciplined_rmw(&lock))
        };

        reentrant_probe.join().expect("reentrant probe completes");
        disciplined.join().expect("disciplined RMW completes");

        let final_value = *lock.read().unwrap();
        assert_eq!(
            final_value, 2,
            "both the reentrant probe's outer write and the disciplined RMW must commit"
        );
    });
}
