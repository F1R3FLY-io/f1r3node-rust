//! Loom model of the multi-sig pre-charge fan-out atomic-commit-or-
//! revert protocol (Phase 1.7 PoS Map-in-MVar refinement under the
//! outer soft-checkpoint scope at `casper/src/rust/rholang/runtime.rs`).
//!
//! The production runtime uses `std::sync` primitives and a heed-LMDB
//! soft-checkpoint, neither of which are loom-aware. This shadow model
//! exercises the structural invariant: for every interleaving of N
//! concurrent per-cosigner pre-charge attempts where ONE may fail,
//! the final PoS Map state is EITHER all N entries present (commit)
//! OR empty (revert). Never a partial-commit hazard.
//!
//! Phase 4.12 — companion to:
//! - `MultiSignerProtocol.tla::PartialFailureNoConsumption`,
//!   `FailureRevertsCharges`
//! - `MultiSignerRefinement.v::fifo_drain_*`
//! - `rholang/tests/loom_runtime_budget_reconciliation.rs` (the budget
//!   reconciliation analogue)

use std::collections::HashMap;

use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

/// Shadow PoS Map: deployerId → charged_amount. Production stores
/// this in MVar Rholang state; here it's a loom::Mutex-wrapped
/// HashMap.
struct ShadowPosMap {
    /// Whether a soft-checkpoint revert has been triggered.
    /// Once set, future commits MUST be discarded.
    revert_triggered: AtomicBool,
    /// The in-flight charges (committed before revert; cleared on
    /// revert).
    charges: Mutex<HashMap<u8, i64>>,
}

impl ShadowPosMap {
    fn new() -> Self {
        Self {
            revert_triggered: AtomicBool::new(false),
            charges: Mutex::new(HashMap::new()),
        }
    }

    /// Atomic commit: insert (deployer_id, amount) into the Map.
    /// Returns true on commit, false if revert was already triggered.
    /// The combined check-then-insert is held inside the lock so
    /// no in-flight commits leak past revert.
    fn try_commit(&self, deployer_id: u8, amount: i64) -> bool {
        let mut guard = self.charges.lock().unwrap();
        if self.revert_triggered.load(Ordering::SeqCst) {
            return false;
        }
        guard.insert(deployer_id, amount);
        true
    }

    /// Trigger the outer soft-checkpoint revert. Clears the Map.
    /// Once triggered, future commits are rejected.
    fn trigger_revert(&self) {
        let mut guard = self.charges.lock().unwrap();
        self.revert_triggered.store(true, Ordering::SeqCst);
        guard.clear();
    }

    fn snapshot(&self) -> HashMap<u8, i64> {
        let guard = self.charges.lock().unwrap();
        guard.clone()
    }
}

/// Loom model 1: N concurrent commits with NO failure — all must
/// succeed and the final Map has exactly N entries.
#[test]
fn loom_concurrent_pre_charge_no_failure_all_commit() {
    loom::model(|| {
        let map = Arc::new(ShadowPosMap::new());
        let m1 = map.clone();
        let m2 = map.clone();

        let t1 = thread::spawn(move || {
            m1.try_commit(0, 100);
        });
        let t2 = thread::spawn(move || {
            m2.try_commit(1, 200);
        });
        t1.join().unwrap();
        t2.join().unwrap();

        // Final state: both entries present.
        let snap = map.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.get(&0), Some(&100));
        assert_eq!(snap.get(&1), Some(&200));
    });
}

/// Loom model 2: N concurrent commits with one failure-triggering
/// thread. Under every interleaving, the final state is one of:
/// (a) All commits succeeded before the revert (then map cleared),
/// (b) Some commits raced with the revert and were rejected.
/// In BOTH cases, after revert the map MUST be empty. Never a
/// partial-commit-with-revert-in-progress state visible externally.
#[test]
fn loom_concurrent_pre_charge_with_revert_yields_empty_or_clean_state() {
    loom::model(|| {
        let map = Arc::new(ShadowPosMap::new());
        let m1 = map.clone();
        let m2 = map.clone();
        let m_revert = map.clone();

        let t1 = thread::spawn(move || {
            m1.try_commit(0, 100);
        });
        let t2 = thread::spawn(move || {
            m2.try_commit(1, 200);
        });
        let t_revert = thread::spawn(move || {
            m_revert.trigger_revert();
        });
        t1.join().unwrap();
        t2.join().unwrap();
        t_revert.join().unwrap();

        // After revert, map MUST be empty (atomic clear).
        let snap = map.snapshot();
        assert!(
            snap.is_empty(),
            "post-revert map must be empty, found {:?}",
            snap
        );
    });
}

/// Loom model 3: invocation count monotonicity under concurrent
/// invokes — the Phase 3 Bang-bounded-uses counter property.
/// N threads each attempt to increment a shared counter (decrement
/// uses_remaining); the counter must end at exactly initial-N if
/// all succeed, or at 0 if it bottoms out.
#[test]
fn loom_bang_bounded_counter_monotone_under_concurrent_invokes() {
    use loom::sync::atomic::AtomicI32;
    loom::model(|| {
        let counter = Arc::new(AtomicI32::new(3));
        let c1 = counter.clone();
        let c2 = counter.clone();

        let t1 = thread::spawn(move || {
            let v = c1.load(Ordering::SeqCst);
            if v > 0 {
                let _ = c1.compare_exchange(v, v - 1, Ordering::SeqCst, Ordering::SeqCst);
            }
        });
        let t2 = thread::spawn(move || {
            let v = c2.load(Ordering::SeqCst);
            if v > 0 {
                let _ = c2.compare_exchange(v, v - 1, Ordering::SeqCst, Ordering::SeqCst);
            }
        });
        t1.join().unwrap();
        t2.join().unwrap();

        // Final counter ∈ [1, 3] (CAS may fail under contention,
        // so 0–2 decrements may apply). NEVER negative, never > initial.
        let final_val = counter.load(Ordering::SeqCst);
        assert!(
            final_val >= 1 && final_val <= 3,
            "Bang counter went out of [1,3]: {}",
            final_val
        );
    });
}
