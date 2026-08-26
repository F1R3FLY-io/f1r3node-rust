use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

struct Budget {
    capacity: usize,
    used: AtomicUsize,
}

struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let previous = self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
        assert!(previous >= self.bytes);
    }
}

impl Budget {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: AtomicUsize::new(0),
        }
    }

    fn try_reserve(budget: &Arc<Self>, bytes: usize) -> Option<Reservation> {
        let mut used = budget.used.load(Ordering::Acquire);
        loop {
            let next = used
                .checked_add(bytes)
                .filter(|next| *next <= budget.capacity)?;
            match budget
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    return Some(Reservation {
                        budget: budget.clone(),
                        bytes,
                    });
                }
                Err(observed) => used = observed,
            }
        }
    }
}

#[test]
fn concurrent_reservations_never_exceed_the_byte_cap_or_leak() {
    loom::model(|| {
        let budget = Arc::new(Budget::new(3));
        let admitted = Arc::new(Mutex::new(Vec::new()));
        let handles = [2usize, 2usize].map(|bytes| {
            let budget = budget.clone();
            let admitted = admitted.clone();
            thread::spawn(move || {
                if let Some(reservation) = Budget::try_reserve(&budget, bytes) {
                    assert!(budget.used.load(Ordering::Acquire) <= budget.capacity);
                    admitted.lock().unwrap().push(bytes);
                    drop(reservation);
                }
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
        assert!(!admitted.lock().unwrap().is_empty());
    });
}

#[test]
fn release_and_reacquire_are_linearizable() {
    loom::model(|| {
        let budget = Arc::new(Budget::new(2));
        let initial = Budget::try_reserve(&budget, 2).expect("initial reservation");
        let released = { thread::spawn(move || drop(initial)) };
        let retried = {
            let budget = budget.clone();
            thread::spawn(move || Budget::try_reserve(&budget, 2))
        };
        released.join().unwrap();
        let admitted = retried.join().unwrap();
        if let Some(reservation) = admitted {
            drop(reservation);
        } else {
            drop(Budget::try_reserve(&budget, 2).expect("retry after release"));
        }
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    });
}

#[test]
fn admission_deferral_atomically_reopens_retriever_state() {
    loom::model(|| {
        #[derive(Clone, Copy)]
        struct Request {
            received: bool,
            deferred: bool,
        }

        let request = Arc::new(Mutex::new(Request {
            received: true,
            deferred: false,
        }));
        let budget = Arc::new(Budget::new(1));
        let initial = Budget::try_reserve(&budget, 1).expect("initial reservation");

        let defer = {
            let request = request.clone();
            let budget = budget.clone();
            thread::spawn(move || {
                if let Some(reservation) = Budget::try_reserve(&budget, 1) {
                    drop(reservation);
                } else {
                    let mut request = request.lock().unwrap();
                    request.received = false;
                    request.deferred = true;
                }
            })
        };
        let complete = thread::spawn(move || drop(initial));
        defer.join().unwrap();
        complete.join().unwrap();

        let state = *request.lock().unwrap();
        if state.deferred {
            assert!(!state.received);
        }
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    });
}

#[test]
fn untracked_deferral_preserves_both_caps_and_remains_readmittable() {
    loom::model(|| {
        struct Tracker {
            capacity: usize,
            used: Mutex<usize>,
        }

        impl Tracker {
            fn try_track(&self) -> bool {
                let mut used = self.used.lock().unwrap();
                if *used == self.capacity {
                    false
                } else {
                    *used += 1;
                    true
                }
            }

            fn release(&self) {
                let mut used = self.used.lock().unwrap();
                assert!(*used > 0);
                *used -= 1;
            }
        }

        let tracker = Arc::new(Tracker {
            capacity: 1,
            used: Mutex::new(1),
        });
        let budget = Arc::new(Budget::new(1));
        let existing = Budget::try_reserve(&budget, 1).expect("existing payload");

        let arrival = {
            let tracker = tracker.clone();
            let budget = budget.clone();
            thread::spawn(move || match Budget::try_reserve(&budget, 1) {
                Some(reservation) => {
                    drop(reservation);
                    None
                }
                None => Some(tracker.try_track()),
            })
        };
        let completion = {
            let tracker = tracker.clone();
            thread::spawn(move || {
                drop(existing);
                tracker.release();
            })
        };

        let deferral = arrival.join().unwrap();
        completion.join().unwrap();
        assert!(budget.used.load(Ordering::Acquire) <= budget.capacity);
        assert!(*tracker.used.lock().unwrap() <= tracker.capacity);

        if deferral == Some(false) {
            assert!(tracker.try_track());
        }
        if *tracker.used.lock().unwrap() == 1 {
            tracker.release();
        }
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
        assert_eq!(*tracker.used.lock().unwrap(), 0);
    });
}

#[test]
fn shared_scan_lock_allows_only_one_materialized_payload() {
    loom::model(|| {
        let scan_lock = Arc::new(Mutex::new(()));
        let live = Arc::new(AtomicUsize::new(0));
        let handles = [(), ()].map(|()| {
            let scan_lock = scan_lock.clone();
            let live = live.clone();
            thread::spawn(move || {
                let _guard = scan_lock.lock().unwrap();
                let previous = live.fetch_add(1, Ordering::AcqRel);
                assert_eq!(previous, 0);
                thread::yield_now();
                assert_eq!(live.fetch_sub(1, Ordering::AcqRel), 1);
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(live.load(Ordering::Acquire), 0);
    });
}
