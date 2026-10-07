use std::collections::BTreeSet;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TransferState {
    source: u8,
    target: u8,
    seen: BTreeSet<u8>,
}

struct TransferLedger {
    state: Mutex<TransferState>,
}

impl TransferLedger {
    fn new(source: u8) -> Self {
        Self {
            state: Mutex::new(TransferState {
                source,
                target: 0,
                seen: BTreeSet::new(),
            }),
        }
    }

    fn transfer(&self, event: u8, cells: u8) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.seen.contains(&event) || state.source < cells {
            return false;
        }
        state.source -= cells;
        state.target += cells;
        assert!(state.seen.insert(event));
        true
    }
}

#[test]
fn concurrent_duplicate_stack_transfer_debits_and_produces_once() {
    loom::model(|| {
        let ledger = Arc::new(TransferLedger::new(4));
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.transfer(7, 2))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.transfer(7, 2))
        };

        let successes = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
        let state = ledger.state.lock().unwrap().clone();
        assert_eq!(successes, 1);
        assert_eq!(state.source, 2);
        assert_eq!(state.target, 2);
        assert_eq!(state.seen, BTreeSet::from([7]));
        assert_eq!(state.source + state.target, 4);
    });
}

#[test]
fn concurrent_distinct_stack_transfers_conserve_the_shared_source() {
    loom::model(|| {
        let ledger = Arc::new(TransferLedger::new(4));
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.transfer(7, 2))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.transfer(9, 2))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        let state = ledger.state.lock().unwrap().clone();
        assert_eq!(state.source, 0);
        assert_eq!(state.target, 4);
        assert_eq!(state.seen, BTreeSet::from([7, 9]));
        assert_eq!(state.source + state.target, 4);
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FrontierState {
    known: BTreeSet<u8>,
    capacity: u8,
}

struct FrontierLedger {
    state: Mutex<FrontierState>,
}

impl FrontierLedger {
    fn discover(&self, authority: u8, backing: u8) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.known.insert(authority) {
            return false;
        }
        state.capacity += backing;
        true
    }
}

#[test]
fn concurrent_duplicate_frontier_discovery_credits_authenticated_backing_once() {
    loom::model(|| {
        let ledger = Arc::new(FrontierLedger {
            state: Mutex::new(FrontierState {
                known: BTreeSet::from([1]),
                capacity: 1,
            }),
        });
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.discover(2, 3))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || ledger.discover(2, 3))
        };

        let expansions = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
        let state = ledger.state.lock().unwrap().clone();
        assert_eq!(expansions, 1);
        assert_eq!(state.known, BTreeSet::from([1, 2]));
        assert_eq!(state.capacity, 4);
    });
}
