use std::collections::BTreeSet;

use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Debug)]
struct DispatchIdentity {
    live: AtomicBool,
}

#[derive(Debug)]
struct RetryDispatch {
    key: u8,
    identity: Arc<DispatchIdentity>,
}

impl Drop for RetryDispatch {
    fn drop(&mut self) { self.identity.live.store(false, Ordering::SeqCst); }
}

#[derive(Debug)]
struct RetryEntry {
    key: u8,
    identity: Option<Arc<DispatchIdentity>>,
    attempts: u32,
}

#[derive(Debug)]
struct RetryWindow {
    active: Option<RetryEntry>,
    backoff: bool,
}

impl RetryWindow {
    fn register_and_dispatch(&mut self, key: u8) -> RetryDispatch {
        let identity = Arc::new(DispatchIdentity {
            live: AtomicBool::new(true),
        });
        self.active = Some(RetryEntry {
            key,
            identity: Some(identity.clone()),
            attempts: 1,
        });
        self.backoff = false;
        RetryDispatch { key, identity }
    }

    fn finish(&mut self, dispatch: RetryDispatch) {
        if self.active.as_ref().is_some_and(|entry| {
            entry.key == dispatch.key
                && entry
                    .identity
                    .as_ref()
                    .is_some_and(|identity| Arc::ptr_eq(identity, &dispatch.identity))
        }) {
            self.active.as_mut().unwrap().identity = None;
            self.backoff = true;
        }
    }

    fn recycle_and_dispatch(&mut self, key: u8) -> RetryDispatch {
        self.active = None;
        self.register_and_dispatch(key)
    }

    fn resolve(&mut self, key: u8) {
        if self.active.as_ref().is_some_and(|entry| entry.key == key) {
            self.active = None;
        }
    }

    fn progress(&mut self, key: u8) {
        if let Some(entry) = self.active.as_mut().filter(|entry| entry.key == key) {
            entry.identity = None;
            entry.attempts = 0;
            self.backoff = false;
        }
    }

    fn defer(&mut self, key: u8) {
        if let Some(entry) = self.active.as_mut().filter(|entry| entry.key == key) {
            entry.identity = None;
            entry.attempts = u32::MAX;
            self.backoff = true;
        }
    }

    fn reclaim_abandoned(&mut self, lease_expired: bool) -> bool {
        let abandoned = self
            .active
            .as_ref()
            .and_then(|entry| entry.identity.as_ref())
            .is_some_and(|identity| lease_expired && !identity.live.load(Ordering::SeqCst));
        if abandoned {
            self.active.as_mut().unwrap().identity = None;
            self.backoff = true;
        }
        abandoned
    }
}

#[derive(Debug)]
struct DurableRecoveryLedger {
    capacity: usize,
    charges: BTreeSet<(u8, u8, u8, u8)>,
    usage: usize,
}

impl DurableRecoveryLedger {
    fn commit(&mut self, charge: (u8, u8, u8, u8)) -> bool {
        if self.charges.contains(&charge) {
            return true;
        }
        if self.usage >= self.capacity {
            return false;
        }
        self.charges.insert(charge);
        self.usage += 1;
        true
    }

    fn restart(&mut self) {}
}

#[test]
fn stale_completion_cannot_clear_a_reused_key() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let stale = window.lock().unwrap().register_and_dispatch(7);

        let finish = {
            let window = window.clone();
            thread::spawn(move || {
                thread::yield_now();
                window.lock().unwrap().finish(stale);
            })
        };
        let recycle = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().recycle_and_dispatch(7))
        };

        finish.join().unwrap();
        let replacement = recycle.join().unwrap();
        let state = window.lock().unwrap();
        let active = state.active.as_ref().unwrap();
        assert!(Arc::ptr_eq(
            active.identity.as_ref().unwrap(),
            &replacement.identity
        ));
        assert_eq!(active.attempts, 1);
    });
}

#[test]
fn abandoned_dispatch_releases_only_its_matching_identity() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let abandoned = window.lock().unwrap().register_and_dispatch(7);
        let drop_handle = thread::spawn(move || drop(abandoned));
        let unrelated = {
            let window = window.clone();
            thread::spawn(move || {
                thread::yield_now();
                window.lock().unwrap().recycle_and_dispatch(8)
            })
        };

        drop_handle.join().unwrap();
        let replacement = unrelated.join().unwrap();
        let mut state = window.lock().unwrap();
        assert!(!state.reclaim_abandoned(true));
        let active = state.active.as_ref().unwrap();
        assert_eq!(active.key, 8);
        assert!(Arc::ptr_eq(
            active.identity.as_ref().unwrap(),
            &replacement.identity
        ));
    });
}

#[test]
fn abandoned_dispatch_becomes_bounded_backoff_work() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let abandoned = window.lock().unwrap().register_and_dispatch(7);
        thread::spawn(move || drop(abandoned)).join().unwrap();

        let mut state = window.lock().unwrap();
        assert!(state.reclaim_abandoned(true));
        assert!(state.active.as_ref().unwrap().identity.is_none());
        assert!(state.backoff);
    });
}

#[test]
fn resolution_racing_completion_cannot_recreate_a_key() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let dispatch = window.lock().unwrap().register_and_dispatch(7);
        let finish = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().finish(dispatch))
        };
        let resolve = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().resolve(7))
        };

        finish.join().unwrap();
        resolve.join().unwrap();
        assert!(window.lock().unwrap().active.is_none());
    });
}

#[test]
fn exact_progress_racing_completion_preserves_the_reset() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let dispatch = window.lock().unwrap().register_and_dispatch(7);
        let finish = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().finish(dispatch))
        };
        let progress = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().progress(7))
        };

        finish.join().unwrap();
        progress.join().unwrap();
        let state = window.lock().unwrap();
        let entry = state.active.as_ref().unwrap();
        assert_eq!(entry.attempts, 0);
        assert!(entry.identity.is_none());
        assert!(!state.backoff);
    });
}

#[test]
fn maximum_deferral_racing_completion_preserves_the_penalty() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let dispatch = window.lock().unwrap().register_and_dispatch(7);
        let finish = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().finish(dispatch))
        };
        let defer = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().defer(7))
        };

        finish.join().unwrap();
        defer.join().unwrap();
        let state = window.lock().unwrap();
        let entry = state.active.as_ref().unwrap();
        assert_eq!(entry.attempts, u32::MAX);
        assert!(entry.identity.is_none());
        assert!(state.backoff);
    });
}

#[test]
fn cancellation_and_deadline_eventually_restore_bounded_backoff() {
    loom::model(|| {
        let window = Arc::new(Mutex::new(RetryWindow {
            active: None,
            backoff: false,
        }));
        let dispatch = window.lock().unwrap().register_and_dispatch(7);
        let cancel = thread::spawn(move || drop(dispatch));
        let deadline = {
            let window = window.clone();
            thread::spawn(move || window.lock().unwrap().reclaim_abandoned(true))
        };

        cancel.join().unwrap();
        deadline.join().unwrap();
        let mut state = window.lock().unwrap();
        state.reclaim_abandoned(true);
        assert!(state.active.as_ref().unwrap().identity.is_none());
        assert!(state.backoff);
    });
}

#[test]
fn duplicate_concurrent_charge_commits_once() {
    loom::model(|| {
        let ledger = Arc::new(Mutex::new(DurableRecoveryLedger {
            capacity: 2,
            charges: BTreeSet::new(),
            usage: 0,
        }));
        let charge = (1, 2, 3, 4);
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.lock().unwrap().commit(charge))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.lock().unwrap().commit(charge))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        let state = ledger.lock().unwrap();
        assert_eq!(state.charges, BTreeSet::from([charge]));
        assert_eq!(state.usage, 1);
    });
}

#[test]
fn concurrent_distinct_charges_respect_episode_capacity_across_restart() {
    loom::model(|| {
        let ledger = Arc::new(Mutex::new(DurableRecoveryLedger {
            capacity: 1,
            charges: BTreeSet::new(),
            usage: 0,
        }));
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.lock().unwrap().commit((1, 2, 3, 4)))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.lock().unwrap().commit((1, 5, 3, 4)))
        };
        let restart = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.lock().unwrap().restart())
        };

        let committed = [left.join().unwrap(), right.join().unwrap()]
            .into_iter()
            .filter(|result| *result)
            .count();
        restart.join().unwrap();
        let state = ledger.lock().unwrap();
        assert_eq!(committed, 1);
        assert_eq!(state.charges.len(), 1);
        assert_eq!(state.usage, 1);
    });
}
