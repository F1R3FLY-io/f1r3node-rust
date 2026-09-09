use std::sync::Arc;

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::Mutex;
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Available,
    InFlight(u8),
    Admitted,
}

#[derive(Clone, Copy, Debug)]
struct RegistryState {
    phase: Phase,
    evidence: bool,
    durable: bool,
    buffer_edge: bool,
    cleanup_pending: bool,
    ordinary_validation: bool,
}

struct Ticket {
    registry: Mutex<RegistryState>,
    budget: AtomicUsize,
}

impl Ticket {
    fn new() -> Self {
        Self {
            registry: Mutex::new(RegistryState {
                phase: Phase::Available,
                evidence: true,
                durable: false,
                buffer_edge: true,
                cleanup_pending: false,
                ordinary_validation: false,
            }),
            budget: AtomicUsize::new(0),
        }
    }

    fn restart(durable: bool, buffer_edge: bool) -> Self {
        Self {
            registry: Mutex::new(RegistryState {
                phase: if durable {
                    Phase::Admitted
                } else {
                    Phase::Available
                },
                evidence: buffer_edge,
                durable,
                buffer_edge,
                cleanup_pending: durable && buffer_edge,
                ordinary_validation: false,
            }),
            budget: AtomicUsize::new(usize::from(durable)),
        }
    }

    fn claim(&self, owner: u8) -> bool {
        let mut state = self.registry.lock().unwrap();
        if state.phase != Phase::Available || !state.evidence {
            return false;
        }
        state.phase = Phase::InFlight(owner);
        true
    }

    fn reserve(&self, owner: u8) -> bool {
        if self.registry.lock().unwrap().phase != Phase::InFlight(owner) {
            return false;
        }
        self.budget
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    fn rollback(&self, owner: u8, reserved: bool) {
        let mut state = self.registry.lock().unwrap();
        if state.phase != Phase::InFlight(owner) {
            return;
        }
        if reserved {
            self.budget.fetch_sub(1, Ordering::Relaxed);
        }
        state.phase = Phase::Available;
    }

    fn commit_dag(&self, owner: u8) -> bool {
        let mut state = self.registry.lock().unwrap();
        if state.phase != Phase::InFlight(owner) || self.budget.load(Ordering::Relaxed) != 1 {
            return false;
        }
        state.phase = Phase::Admitted;
        state.durable = true;
        state.cleanup_pending = true;
        true
    }

    fn cleanup(&self) {
        let mut state = self.registry.lock().unwrap();
        if state.durable {
            state.evidence = false;
            state.buffer_edge = false;
            state.cleanup_pending = false;
        }
    }

    fn duplicate(&self) {
        let state = self.registry.lock().unwrap();
        if state.phase != Phase::Available {
            assert!(!state.ordinary_validation);
        }
    }
}

#[test]
fn equal_concurrent_deliveries_commit_once_without_validation() {
    loom::model(|| {
        let ticket = Arc::new(Ticket::new());
        let workers = [1, 2].map(|owner| {
            let ticket = ticket.clone();
            thread::spawn(move || {
                if ticket.claim(owner) {
                    assert!(ticket.reserve(owner));
                    thread::yield_now();
                    assert!(ticket.commit_dag(owner));
                } else {
                    ticket.duplicate();
                }
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        let state = ticket.registry.lock().unwrap();
        assert_eq!(state.phase, Phase::Admitted);
        assert!(state.durable);
        assert!(state.buffer_edge);
        assert!(state.cleanup_pending);
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
        assert!(!state.ordinary_validation);
    });
}

#[test]
fn rollback_retains_evidence_for_a_concurrent_retry() {
    loom::model(|| {
        let ticket = Arc::new(Ticket::new());
        assert!(ticket.claim(1));
        assert!(ticket.reserve(1));
        let failing = {
            let ticket = ticket.clone();
            thread::spawn(move || ticket.rollback(1, true))
        };
        let retry = {
            let ticket = ticket.clone();
            thread::spawn(move || loop {
                if ticket.claim(2) {
                    assert!(ticket.reserve(2));
                    assert!(ticket.commit_dag(2));
                    break;
                }
                ticket.duplicate();
                thread::yield_now();
            })
        };
        failing.join().unwrap();
        retry.join().unwrap();
        let state = ticket.registry.lock().unwrap();
        assert_eq!(state.phase, Phase::Admitted);
        assert!(state.durable);
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn cleanup_failure_keeps_a_committed_retryable_edge() {
    loom::model(|| {
        let ticket = Ticket::new();
        assert!(ticket.claim(1));
        assert!(ticket.reserve(1));
        assert!(ticket.commit_dag(1));
        let state = ticket.registry.lock().unwrap();
        assert_eq!(state.phase, Phase::Admitted);
        assert!(state.durable);
        assert!(state.evidence);
        assert!(state.buffer_edge);
        assert!(state.cleanup_pending);
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn cleanup_and_duplicate_preserve_one_durable_admission() {
    loom::model(|| {
        let ticket = Arc::new(Ticket::new());
        assert!(ticket.claim(1));
        assert!(ticket.reserve(1));
        assert!(ticket.commit_dag(1));
        let cleanup = {
            let ticket = ticket.clone();
            thread::spawn(move || ticket.cleanup())
        };
        let duplicate = {
            let ticket = ticket.clone();
            thread::spawn(move || ticket.duplicate())
        };
        cleanup.join().unwrap();
        duplicate.join().unwrap();
        let state = ticket.registry.lock().unwrap();
        assert_eq!(state.phase, Phase::Admitted);
        assert!(state.durable);
        assert!(!state.buffer_edge);
        assert!(!state.cleanup_pending);
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
    });
}

#[test]
fn restart_reconstructs_budget_and_cleanup_state() {
    loom::model(|| {
        let ticket = Ticket::restart(true, true);
        ticket.duplicate();
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
        ticket.cleanup();
        let state = ticket.registry.lock().unwrap();
        assert_eq!(state.phase, Phase::Admitted);
        assert!(state.durable);
        assert!(!state.buffer_edge);
        assert!(!state.cleanup_pending);
        assert_eq!(ticket.budget.load(Ordering::Relaxed), 1);
    });
}
