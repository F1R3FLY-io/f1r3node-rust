// T-9.2 — 3-thread atomic-RMW interleaving model check.
//
// Theorem: T-9.2 (atomic record insert).
// Reference: docs/theory/slashing/slashing-specification.md §10.2,
// design/14-test-plan.md §14.8.6 (concurrency coverage runs over
// thread counts 2, 4, 8).
//
// Companion to `loom_t_9_2_atomic_record.rs` (2-thread). This
// file exercises the same atomicity property over three threads
// — the loom state space grows roughly factorially with thread
// count, so 3 is the highest that fits within the PR-gate CI
// budget. The 4-thread variant is in
// `loom_t_9_2_n_threads_4.rs` and runs only in the nightly job.
//
// Run with the rest of the suite:
//   cargo test -p casper -- slashing::loom_t_9_2_n_threads_3

use std::collections::{BTreeMap, BTreeSet};

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Default)]
struct AbstractTracker {
    inner: BTreeMap<(u8, u64), BTreeSet<u64>>,
}

impl AbstractTracker {
    fn add(&mut self, key: (u8, u64), witnesses: BTreeSet<u64>) {
        self.inner.insert(key, witnesses);
    }
    fn get_clone(&self, key: &(u8, u64)) -> BTreeSet<u64> {
        self.inner.get(key).cloned().unwrap_or_default()
    }
}

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

#[test]
fn t_9_2_three_thread_atomic_rmw_preserves_all_witnesses() {
    loom::model(|| {
        let tracker = Arc::new(Mutex::new(AbstractTracker::default()));
        let t1 = tracker.clone();
        let t2 = tracker.clone();
        let t3 = tracker.clone();

        let h1 = thread::spawn(move || record_evidence_locked(&t1, 0, 4, 100));
        let h2 = thread::spawn(move || record_evidence_locked(&t2, 0, 4, 200));
        let h3 = thread::spawn(move || record_evidence_locked(&t3, 0, 4, 300));
        h1.join().unwrap();
        h2.join().unwrap();
        h3.join().unwrap();

        let final_tracker = tracker.lock().unwrap();
        let witnesses = final_tracker.get_clone(&(0, 4));
        assert!(witnesses.contains(&100));
        assert!(witnesses.contains(&200));
        assert!(witnesses.contains(&300));
        assert_eq!(
            witnesses.len(),
            3,
            "T-9.2 (3-thread): all witnesses preserved across every interleaving"
        );
    });
}
