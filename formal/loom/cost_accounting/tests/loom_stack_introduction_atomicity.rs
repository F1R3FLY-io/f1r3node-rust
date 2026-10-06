use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct State {
    available: usize,
    pending: [usize; 2],
    committed: [usize; 2],
    rspace_visible: [bool; 2],
    birth_visible: [bool; 2],
    attempted_bytes: [usize; 2],
}

struct Ledger {
    state: Mutex<State>,
}

struct Reservation {
    ledger: Arc<Ledger>,
    operation: usize,
    active: bool,
}

impl Ledger {
    fn new(available: usize) -> Self {
        Self {
            state: Mutex::new(State {
                available,
                pending: [0; 2],
                committed: [0; 2],
                rspace_visible: [false; 2],
                birth_visible: [false; 2],
                attempted_bytes: [0; 2],
            }),
        }
    }

    fn prepare(ledger: &Arc<Self>, operation: usize, cells: usize) -> Option<Reservation> {
        let mut state = ledger.state.lock().unwrap();
        if cells == 0
            || state.pending[operation] != 0
            || state.committed[operation] != 0
            || state.available < cells
        {
            return None;
        }
        state.available -= cells;
        state.pending[operation] = cells;
        Some(Reservation {
            ledger: ledger.clone(),
            operation,
            active: true,
        })
    }

    fn read(&self) -> State { *self.state.lock().unwrap() }

    fn charge_bytes(&self, operation: usize, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.attempted_bytes[operation] += bytes;
    }

    fn rollback_failed_deploy(&self) {
        let mut state = self.state.lock().unwrap();
        state.available += state.pending.iter().sum::<usize>();
        state.available += state.committed.iter().sum::<usize>();
        state.pending = [0; 2];
        state.committed = [0; 2];
        state.rspace_visible = [false; 2];
        state.birth_visible = [false; 2];
    }
}

impl Reservation {
    fn mark_rspace_visible(&self) {
        let mut state = self.ledger.state.lock().unwrap();
        assert!(state.pending[self.operation] > 0);
        assert_eq!(state.committed[self.operation], 0);
        state.rspace_visible[self.operation] = true;
    }

    fn commit(mut self) {
        let mut state = self.ledger.state.lock().unwrap();
        let cells = state.pending[self.operation];
        assert!(cells > 0);
        assert_eq!(state.committed[self.operation], 0);
        assert!(state.rspace_visible[self.operation]);
        state.pending[self.operation] = 0;
        state.committed[self.operation] = cells;
        state.birth_visible[self.operation] = true;
        self.active = false;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.ledger.state.lock().unwrap();
        let cells = state.pending[self.operation];
        state.pending[self.operation] = 0;
        state.available += cells;
        state.rspace_visible[self.operation] = false;
        self.active = false;
    }
}

#[test]
fn concurrent_commit_and_byte_rejection_preserve_capacity_and_witness_visibility() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(3));
        let rejected = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let reservation = Ledger::prepare(&ledger, 0, 2).unwrap();
                ledger.charge_bytes(0, 5);
                let state = ledger.read();
                assert_eq!(state.committed[0], 0);
                assert!(!state.rspace_visible[0]);
                assert!(!state.birth_visible[0]);
                drop(reservation);
            })
        };
        let committed = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let reservation = Ledger::prepare(&ledger, 1, 1).unwrap();
                ledger.charge_bytes(1, 3);
                reservation.mark_rspace_visible();
                reservation.commit();
            })
        };

        rejected.join().unwrap();
        committed.join().unwrap();
        let state = ledger.read();
        assert_eq!(state.available, 2);
        assert_eq!(state.pending, [0, 0]);
        assert_eq!(state.committed, [0, 1]);
        assert_eq!(state.rspace_visible, [false, true]);
        assert_eq!(state.birth_visible, [false, true]);
        assert_eq!(state.attempted_bytes, [5, 3]);
        assert_eq!(
            state.available
                + state.pending.iter().sum::<usize>()
                + state.committed.iter().sum::<usize>(),
            3
        );
    });
}

#[test]
fn a_pending_reservation_blocks_oversubscription_until_abort() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(2));
        let reservation = Ledger::prepare(&ledger, 0, 2).unwrap();
        assert!(Ledger::prepare(&ledger, 1, 1).is_none());
        drop(reservation);
        let reservation = Ledger::prepare(&ledger, 1, 1).unwrap();
        reservation.mark_rspace_visible();
        reservation.commit();

        let state = ledger.read();
        assert_eq!(state.available, 1);
        assert_eq!(state.pending, [0, 0]);
        assert_eq!(state.committed, [0, 1]);
    });
}

#[test]
fn cancellation_while_waiting_for_rspace_aborts_only_an_unpublished_reservation() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(2));
        let cancelled = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let reservation = Ledger::prepare(&ledger, 0, 2).unwrap();
                ledger.charge_bytes(0, 1);
                drop(reservation);
            })
        };
        let observer = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let state = ledger.read();
                assert_eq!(state.committed[0], 0);
                assert!(!state.birth_visible[0]);
            })
        };

        cancelled.join().unwrap();
        observer.join().unwrap();
        let state = ledger.read();
        assert_eq!(state.available, 2);
        assert_eq!(state.pending, [0, 0]);
        assert_eq!(state.committed, [0, 0]);
        assert_eq!(state.rspace_visible, [false, false]);
        assert_eq!(state.birth_visible, [false, false]);
        assert_eq!(state.attempted_bytes, [1, 0]);
    });
}

#[test]
fn enclosing_deploy_failure_rolls_back_all_linear_effects_and_keeps_attempt_charges() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(3));
        let first = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let reservation = Ledger::prepare(&ledger, 0, 2).unwrap();
                ledger.charge_bytes(0, 5);
                reservation.mark_rspace_visible();
                reservation.commit();
            })
        };
        let second = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                let reservation = Ledger::prepare(&ledger, 1, 1).unwrap();
                ledger.charge_bytes(1, 3);
                reservation.mark_rspace_visible();
                reservation.commit();
            })
        };

        first.join().unwrap();
        second.join().unwrap();
        ledger.rollback_failed_deploy();
        let state = ledger.read();
        assert_eq!(state.available, 3);
        assert_eq!(state.pending, [0, 0]);
        assert_eq!(state.committed, [0, 0]);
        assert_eq!(state.rspace_visible, [false, false]);
        assert_eq!(state.birth_visible, [false, false]);
        assert_eq!(state.attempted_bytes, [5, 3]);
    });
}
