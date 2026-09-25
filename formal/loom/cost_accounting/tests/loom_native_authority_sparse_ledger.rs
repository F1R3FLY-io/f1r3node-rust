use std::collections::BTreeMap;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../rholang/src/rust/interpreter/accounting/native_runtime/replay_authority/sparse_ledger.rs"]
mod production;

#[derive(Default)]
struct Authority {
    held: BTreeMap<u8, u64>,
    published: BTreeMap<u8, u64>,
    pending: BTreeMap<u8, BTreeMap<u8, u64>>,
}

impl Authority {
    fn check(&self, capacity: &BTreeMap<u8, u64>) {
        let mut expected = self.published.clone();
        for debit in self.pending.values() {
            for (key, amount) in debit {
                *expected.entry(*key).or_default() += amount;
            }
        }
        assert_eq!(self.held, expected);
        assert!(self
            .held
            .iter()
            .all(|(key, amount)| amount <= &capacity[key]));
    }
}

#[test]
fn publication_cancellation_and_reentry_preserve_every_purse() {
    for capacity in [1, 2] {
        for cancel in [false, true] {
            loom::model(move || {
                let limits = BTreeMap::from([(0, capacity), (1, 1), (2, 1)]);
                let ledger = Arc::new(Mutex::new(Authority::default()));
                let workers: Vec<_> = (0..2)
                    .map(|id| {
                        let ledger = Arc::clone(&ledger);
                        let limits = limits.clone();
                        thread::spawn(move || {
                            let debit = BTreeMap::from([(0, 1), (id + 1, 1)]);
                            for attempt in 0..2 {
                                {
                                    let mut state = ledger.lock().unwrap();
                                    if production::validate_add(&state.held, &debit, Some(&limits))
                                        .is_err()
                                    {
                                        state.check(&limits);
                                        return;
                                    }
                                    production::add_assign(&mut state.held, &debit).unwrap();
                                    assert!(state.pending.insert(id, debit.clone()).is_none());
                                    state.check(&limits);
                                }
                                thread::yield_now();
                                let mut state = ledger.lock().unwrap();
                                let debit = state.pending.remove(&id).unwrap();
                                if cancel && id == 0 && attempt == 0 {
                                    production::sub_assign(&mut state.held, &debit).unwrap();
                                    state.check(&limits);
                                } else {
                                    production::add_assign(&mut state.published, &debit).unwrap();
                                    state.check(&limits);
                                    return;
                                }
                            }
                        })
                    })
                    .collect();
                for worker in workers {
                    worker.join().unwrap();
                }
                let state = ledger.lock().unwrap();
                state.check(&limits);
                assert!(state.pending.is_empty());
                assert_eq!(state.held, state.published);
                assert_eq!(state.published[&0], capacity);
            });
        }
    }
}

#[test]
fn concurrent_failed_updates_leave_all_balances_unchanged() {
    loom::model(|| {
        let initial = BTreeMap::from([(0, 3), (1, u64::MAX)]);
        let ledger = Arc::new(Mutex::new(initial.clone()));
        let handles: Vec<_> = (0..2)
            .map(|id| {
                let ledger = Arc::clone(&ledger);
                thread::spawn(move || {
                    let mut ledger = ledger.lock().unwrap();
                    if id == 0 {
                        assert_eq!(
                            production::add_assign(&mut ledger, &BTreeMap::from([(0, 1), (1, 1)])),
                            Err(production::LedgerError::Overflow),
                        );
                    } else {
                        assert_eq!(
                            production::sub_assign(&mut ledger, &BTreeMap::from([(0, 1), (2, 1)])),
                            Err(production::LedgerError::Insufficient),
                        );
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(*ledger.lock().unwrap(), initial);
    });
}
