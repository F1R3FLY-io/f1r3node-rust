use std::sync::Arc;

use loom::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use loom::sync::Mutex;
use loom::thread;

struct WakeState {
    parked: Mutex<Option<(u64, u64, u64)>>,
    head_revision: AtomicU64,
    dag_generation: AtomicU64,
    requests: AtomicUsize,
    progress: AtomicUsize,
}

impl WakeState {
    fn new() -> Self {
        Self {
            parked: Mutex::new(None),
            head_revision: AtomicU64::new(7),
            dag_generation: AtomicU64::new(0),
            requests: AtomicUsize::new(0),
            progress: AtomicUsize::new(0),
        }
    }

    fn park(&self, revision: u64, floor: u64, post_state: u64) {
        let mut parked = self.parked.lock().unwrap();
        if parked.is_none_or(|current| current.0 <= revision) {
            *parked = Some((revision, floor, post_state));
        }
    }

    fn take(&self, revision: u64, floor: u64, post_state: u64, _certificate_digest: u64) -> bool {
        let mut parked = self.parked.lock().unwrap();
        if *parked != Some((revision, floor, post_state)) {
            return false;
        }
        *parked = None;
        true
    }

    fn request(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
        self.progress.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn causal_carrier_admission_is_consumed_or_wakes_exactly_once() {
    loom::model(|| {
        let state = Arc::new(WakeState::new());
        let finalizer = {
            let state = state.clone();
            thread::spawn(move || {
                let captured_generation = state.dag_generation.load(Ordering::SeqCst);
                let captured_revision = state.head_revision.load(Ordering::SeqCst);
                thread::yield_now();
                if captured_generation != 0 {
                    state.progress.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                state.park(captured_revision, 11, 12);
                let changed = state.dag_generation.load(Ordering::SeqCst) != captured_generation
                    || state.head_revision.load(Ordering::SeqCst) != captured_revision;
                if changed && state.take(captured_revision, 11, 12, 21) {
                    state.request();
                }
            })
        };
        let admission = {
            let state = state.clone();
            thread::spawn(move || {
                state.dag_generation.fetch_add(1, Ordering::SeqCst);
                if state.head_revision.load(Ordering::SeqCst) == 7 && state.take(7, 11, 12, 22) {
                    state.request();
                }
            })
        };
        finalizer.join().unwrap();
        admission.join().unwrap();
        assert_eq!(state.progress.load(Ordering::SeqCst), 1);
        assert!(state.requests.load(Ordering::SeqCst) <= 1);
        assert_eq!(*state.parked.lock().unwrap(), None);
    });
}

#[test]
fn duplicate_matching_admissions_coalesce_to_one_wake() {
    loom::model(|| {
        let state = Arc::new(WakeState::new());
        state.park(7, 11, 12);
        let first = {
            let state = state.clone();
            thread::spawn(move || {
                if state.take(7, 11, 12, 22) {
                    state.request();
                }
            })
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || {
                if state.take(7, 11, 12, 23) {
                    state.request();
                }
            })
        };
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(state.requests.load(Ordering::SeqCst), 1);
        assert_eq!(*state.parked.lock().unwrap(), None);
    });
}

#[test]
fn divergent_witness_digest_wakes_only_for_the_same_floor_state() {
    loom::model(|| {
        let state = Arc::new(WakeState::new());
        state.park(7, 11, 12);
        let wrong_state = {
            let state = state.clone();
            thread::spawn(move || state.take(7, 11, 13, 22))
        };
        let equivalent_foreign_witness = {
            let state = state.clone();
            thread::spawn(move || {
                thread::yield_now();
                state.take(7, 11, 12, 22)
            })
        };
        assert!(!wrong_state.join().unwrap());
        assert!(equivalent_foreign_witness.join().unwrap());
        assert_eq!(*state.parked.lock().unwrap(), None);
    });
}
