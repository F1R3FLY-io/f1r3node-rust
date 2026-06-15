// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// T-9.2 — Exhaustive thread-interleaving model check for the
// atomic-RMW property on the equivocation tracker.
//
// Theorem: T-9.2 (`t_9_2_atomic_record_insert`,
// formal/rocq/slashing/theories/BugFixAtomicTracker.v).
// Reference: docs/theory/slashing/slashing-specification.md §10.2,
// design/09-bug-fixes-and-rationale.md §9.2,
// design/14-test-plan.md §14.5.
//
// Why this is a separate file (not a regular `proptest`):
//   `proptest` samples random schedules; loom **enumerates every
//   interleaving** of two threads under preemption budget — the
//   only sound way to prove a concurrency property holds for *all*
//   reachable schedules.
//
// Why this is a SHADOW IMPLEMENTATION rather than `cfg(loom)`-shimmed
// production code:
//   docs/theory/slashing/design/14-test-plan.md §14.7 forbids
//   carrying alternate code paths for production-vs-test variants
//   into the production source tree (the same prohibition that
//   eliminated the `pre-fix-bug-N` Cargo features). The atomicity
//   property at the source-of-truth level is *type-level*: every
//   read-modify-write on the tracker routes through
//   `BlockDagKeyValueStorage::access_equivocations_tracker(closure)`
//   which holds `global_lock` for the closure's duration. The role
//   of loom here is to verify the abstract atomicity *specification*
//   (locked RMW preserves all witnesses; lock-free RMW does not),
//   not to instrument the production `Mutex<()>`.
//
// Trait-level contract:
//   The production type (`BlockDagKeyValueStorage`) implements
//   `block_storage::rust::dag::equivocations_access::EquivocationsAccess`.
//   The trait demands that the closure run inside a critical
//   section; the `record_evidence_locked` shadow below is a
//   faithful expression of that contract using `loom::sync::Mutex`
//   (so the loom executor enumerates every interleaving where the
//   closure body runs while holding the lock). The
//   `record_evidence_lockfree` shadow is the contract violation:
//   it splits the read and write into two independent lock
//   acquisitions, which the trait's `FnOnce(&...) -> Result<...>`
//   shape forbids by construction in production code.
//
// Run with the rest of the suite:
//   cargo test -p casper -- slashing::loom_t_9_2
// (loom enumerates interleavings whenever its API is invoked; no
// `--cfg loom` flag is needed because we use `loom::sync::*`
// types directly rather than swapping `std::sync::*` for them.)

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as StdArc;

use loom::sync::{Arc, Mutex};
use loom::thread;

/// Abstract tracker — a (validator, base_seq) → witness-set map.
/// Mirrors the production `EquivocationTrackerStore` to the level of
/// detail T-9.2 cares about (witness preservation under RMW).
#[derive(Default)]
struct AbstractTracker {
    inner: BTreeMap<(u8, u64), BTreeSet<u64>>,
}

impl AbstractTracker {
    /// Mirrors `EquivocationTrackerStore::add(record)` semantics:
    /// full overwrite-by-key. The bug exposed by T-9.2 is that
    /// `add` is a put-one (not a merge), so racing threads each
    /// computing a stale witness set will overwrite each other.
    fn add(&mut self, key: (u8, u64), witnesses: BTreeSet<u64>) {
        self.inner.insert(key, witnesses);
    }

    fn get_clone(&self, key: &(u8, u64)) -> BTreeSet<u64> {
        self.inner.get(key).cloned().unwrap_or_default()
    }

    fn len_at(&self, key: &(u8, u64)) -> usize { self.inner.get(key).map(|s| s.len()).unwrap_or(0) }
}

/// Post-fix `record_evidence`: the entire read-modify-write runs
/// under the `Mutex<AbstractTracker>` guard, mirroring the
/// production `access_equivocations_tracker(|tracker| { ... })`.
fn record_evidence_locked(
    tracker_lock: &Arc<Mutex<AbstractTracker>>,
    validator: u8,
    base_seq: u64,
    new_witness: u64,
) {
    let mut t = tracker_lock.lock().unwrap();
    let key = (validator, base_seq);
    let mut witnesses = t.get_clone(&key);
    witnesses.insert(new_witness);
    t.add(key, witnesses);
}

/// Pre-fix `record_evidence`: the read and the write each acquire
/// the lock independently, with the decide-step running outside
/// any lock — the race window. Mirrors what the production
/// dispatcher looked like before the bug-#2 fix.
fn record_evidence_lockfree(
    tracker_lock: &Arc<Mutex<AbstractTracker>>,
    validator: u8,
    base_seq: u64,
    new_witness: u64,
) {
    let key = (validator, base_seq);
    // READ under brief lock.
    let existing = {
        let t = tracker_lock.lock().unwrap();
        t.get_clone(&key)
    }; // lock released — RACE WINDOW
    let mut witnesses = existing;
    witnesses.insert(new_witness);
    // WRITE under separate lock acquisition.
    {
        let mut t = tracker_lock.lock().unwrap();
        t.add(key, witnesses);
    }
}

#[test]
fn t_9_2_post_fix_atomic_rmw_preserves_all_witnesses() {
    loom::model(|| {
        let tracker = Arc::new(Mutex::new(AbstractTracker::default()));
        let t1 = tracker.clone();
        let t2 = tracker.clone();

        // Two threads racing on (validator=0, base_seq=4) with
        // distinct witness hashes 100 and 200.
        let h1 = thread::spawn(move || {
            record_evidence_locked(&t1, 0, 4, 100);
        });
        let h2 = thread::spawn(move || {
            record_evidence_locked(&t2, 0, 4, 200);
        });
        h1.join().unwrap();
        h2.join().unwrap();

        // For EVERY interleaving, both witnesses survive.
        let final_tracker = tracker.lock().unwrap();
        let witnesses = final_tracker.get_clone(&(0, 4));
        assert!(
            witnesses.contains(&100),
            "T-9.2: post-fix locked RMW preserves witness 100"
        );
        assert!(
            witnesses.contains(&200),
            "T-9.2: post-fix locked RMW preserves witness 200"
        );
        assert_eq!(
            witnesses.len(),
            2,
            "T-9.2 + T-5: exactly two witnesses, no overwrite"
        );
    });
}

#[test]
fn t_9_2_pre_fix_lockfree_loses_a_witness() {
    // Bug-existence proof: assert at least one loom interleaving
    // produces a final state with fewer than two witnesses for the
    // lockfree shadow. If this test passes, the bug is real (in
    // the lockfree variant); if it fails, our pre-fix shadow does
    // not exhibit the race we claim it does.
    //
    // The bug-observed flag uses `std::sync::atomic` (not loom's)
    // because it lives *outside* loom's execution model — it is
    // observed across the entire enumeration of interleavings, not
    // within any single one.
    let bug_observed = StdArc::new(AtomicBool::new(false));
    let bug_observed_outer = bug_observed.clone();

    loom::model(move || {
        let tracker = Arc::new(Mutex::new(AbstractTracker::default()));
        let t1 = tracker.clone();
        let t2 = tracker.clone();
        let observed = bug_observed.clone();

        let h1 = thread::spawn(move || {
            record_evidence_lockfree(&t1, 0, 4, 100);
        });
        let h2 = thread::spawn(move || {
            record_evidence_lockfree(&t2, 0, 4, 200);
        });
        h1.join().unwrap();
        h2.join().unwrap();

        let final_size = tracker.lock().unwrap().len_at(&(0, 4));
        if final_size < 2 {
            observed.store(true, Ordering::SeqCst);
        }
    });

    assert!(
        bug_observed_outer.load(Ordering::SeqCst),
        "T-9.2 bug-existence proof: at least one loom interleaving \
         must produce <2 witnesses for the lockfree shadow. The bug \
         is real and the post-fix's atomic-RMW routing closes it."
    );
}
