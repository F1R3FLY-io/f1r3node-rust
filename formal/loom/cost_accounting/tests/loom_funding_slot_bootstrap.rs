use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LedgerState {
    sponsor: usize,
    outer: usize,
    slot: usize,
    burned: usize,
    outer_exists: bool,
    slot_exists: bool,
    activated: bool,
}

struct Ledger {
    state: Mutex<LedgerState>,
}

impl Ledger {
    fn empty(sponsor: usize) -> Self {
        Self {
            state: Mutex::new(LedgerState {
                sponsor,
                outer: 0,
                slot: 0,
                burned: 0,
                outer_exists: false,
                slot_exists: false,
                activated: false,
            }),
        }
    }

    fn funded(sponsor: usize, outer: usize, slot: usize) -> Self {
        Self {
            state: Mutex::new(LedgerState {
                sponsor,
                outer,
                slot,
                burned: 0,
                outer_exists: true,
                slot_exists: true,
                activated: false,
            }),
        }
    }

    fn fund_batch(&self, outer: usize, slot: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(total) = outer.checked_add(slot) else {
            return false;
        };
        let Some(sponsor) = state.sponsor.checked_sub(total) else {
            return false;
        };
        let Some(outer_balance) = state.outer.checked_add(outer) else {
            return false;
        };
        let Some(slot_balance) = state.slot.checked_add(slot) else {
            return false;
        };
        state.sponsor = sponsor;
        state.outer = outer_balance;
        state.slot = slot_balance;
        state.outer_exists = true;
        state.slot_exists = true;
        true
    }

    fn activate(
        &self,
        outer_bound: usize,
        slot_bound: usize,
        outer_cost: usize,
        slot_cost: usize,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.activated
            || !state.outer_exists
            || !state.slot_exists
            || state.outer < outer_bound
            || state.slot < slot_bound
            || outer_cost > outer_bound
            || slot_cost > slot_bound
        {
            return false;
        }
        state.outer -= outer_cost;
        state.slot -= slot_cost;
        state.burned += outer_cost + slot_cost;
        state.activated = true;
        true
    }

    fn read(&self) -> LedgerState { *self.state.lock().unwrap() }
}

fn accounted_value(state: LedgerState) -> usize {
    state.sponsor + state.outer + state.slot + state.burned
}

#[test]
fn underfunded_batch_racing_activation_has_no_partial_effect() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::empty(5));
        let funding = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.fund_batch(3, 3))
        };
        let activation = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.activate(2, 2, 1, 1))
        };

        assert!(!funding.join().unwrap());
        assert!(!activation.join().unwrap());
        assert_eq!(ledger.read(), LedgerState {
            sponsor: 5,
            outer: 0,
            slot: 0,
            burned: 0,
            outer_exists: false,
            slot_exists: false,
            activated: false,
        });
    });
}

#[test]
fn funding_racing_activation_is_linearizable() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::empty(10));
        let funding = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.fund_batch(4, 6))
        };
        let activation = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.activate(2, 2, 1, 2))
        };

        assert!(funding.join().unwrap());
        let activated = activation.join().unwrap();
        let state = ledger.read();
        assert_eq!(accounted_value(state), 10);
        assert!(state.outer_exists);
        assert!(state.slot_exists);
        assert_eq!(state.activated, activated);
        if activated {
            assert_eq!((state.outer, state.slot, state.burned), (3, 4, 3));
        } else {
            assert_eq!((state.outer, state.slot, state.burned), (4, 6, 0));
        }
    });
}

#[test]
fn competing_batches_cannot_overspend_the_sponsor() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::empty(10));
        let first = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.fund_batch(3, 3))
        };
        let second = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.fund_batch(3, 3))
        };

        let first = first.join().unwrap();
        let second = second.join().unwrap();
        assert_ne!(first, second);
        let state = ledger.read();
        assert_eq!(accounted_value(state), 10);
        assert_eq!((state.sponsor, state.outer, state.slot), (4, 3, 3));
    });
}

#[test]
fn concurrent_top_up_does_not_change_the_activation_bound() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::funded(4, 5, 5));
        let top_up = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.fund_batch(2, 2))
        };
        let activation = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.activate(5, 5, 3, 4))
        };

        assert!(top_up.join().unwrap());
        assert!(activation.join().unwrap());
        let state = ledger.read();
        assert_eq!(accounted_value(state), 14);
        assert_eq!((state.sponsor, state.outer, state.slot), (0, 4, 3));
        assert_eq!(state.burned, 7);
    });
}
