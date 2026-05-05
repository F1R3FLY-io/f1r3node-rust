// T-9.2 — 4-thread atomic-RMW interleaving model check
// (nightly-only).
//
// Theorem: T-9.2 (atomic record insert).
// Reference: docs/theory/slashing/slashing-specification.md §10.2,
// design/14-test-plan.md §14.8.6 (thread counts 2, 4, 8).
//
// Loom's state space grows factorially with thread count; 4-thread
// exhaustive enumeration takes minutes-to-hours depending on the
// preemption budget. We gate this test behind the
// `RUN_NIGHTLY_LOOM` environment variable so PR-gate runs skip
// it; the nightly extended-proptest CI job sets the env var.

use loom::sync::{Arc, Mutex};
use loom::thread;
use std::collections::{BTreeMap, BTreeSet};

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
fn t_9_2_four_thread_atomic_rmw_preserves_all_witnesses() {
    if std::env::var("RUN_NIGHTLY_LOOM").is_err() {
        // PR-gate skips this; nightly extended-proptest job sets
        // the env var. The 2-thread and 3-thread companions
        // already cover the property at PR-gate.
        return;
    }

    loom::model(|| {
        let tracker = Arc::new(Mutex::new(AbstractTracker::default()));
        let t1 = tracker.clone();
        let t2 = tracker.clone();
        let t3 = tracker.clone();
        let t4 = tracker.clone();

        let h1 = thread::spawn(move || record_evidence_locked(&t1, 0, 4, 100));
        let h2 = thread::spawn(move || record_evidence_locked(&t2, 0, 4, 200));
        let h3 = thread::spawn(move || record_evidence_locked(&t3, 0, 4, 300));
        let h4 = thread::spawn(move || record_evidence_locked(&t4, 0, 4, 400));
        h1.join().unwrap();
        h2.join().unwrap();
        h3.join().unwrap();
        h4.join().unwrap();

        let final_tracker = tracker.lock().unwrap();
        let witnesses = final_tracker.get_clone(&(0, 4));
        assert!(witnesses.contains(&100));
        assert!(witnesses.contains(&200));
        assert!(witnesses.contains(&300));
        assert!(witnesses.contains(&400));
        assert_eq!(witnesses.len(), 4,
            "T-9.2 (4-thread): all witnesses preserved across every interleaving");
    });
}
