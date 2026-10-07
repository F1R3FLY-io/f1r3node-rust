use loom::sync::atomic::{AtomicU64, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/state.rs"]
mod production;

use production::{LedgerError, ReplayState, SlotState};

fn state(count: usize) -> ReplayState {
    ReplayState::new(
        vec![SlotState::Available; count],
        Vec::with_capacity(count),
        (1..=count as u64).sum(),
    )
}

#[test]
fn duplicate_reservations_have_exactly_one_publisher() {
    loom::model(|| {
        let ledger = Arc::new(Mutex::new(state(1)));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let ledger = Arc::clone(&ledger);
                thread::spawn(move || {
                    let reserved = ledger.lock().unwrap().reserve(0);
                    thread::yield_now();
                    match reserved {
                        Ok(serial) => {
                            ledger.lock().unwrap().publish(0, serial, 1);
                            1
                        }
                        Err(LedgerError::Unavailable) => 0,
                        Err(error) => panic!("{error:?}"),
                    }
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .sum::<usize>(),
            1
        );
        ledger.lock().unwrap().check_complete().unwrap();
    });
}

#[test]
fn completed_snapshot_boundary_excludes_parallel_publication_and_restore() {
    loom::model(|| {
        let ledger = Arc::new(Mutex::new(state(2)));
        let writers: Vec<_> = (0..2)
            .map(|slot| {
                let ledger = Arc::clone(&ledger);
                thread::spawn(move || {
                    let reserved = ledger.lock().unwrap().reserve(slot);
                    thread::yield_now();
                    match reserved {
                        Ok(serial) => ledger
                            .lock()
                            .unwrap()
                            .publish(slot, serial, slot as u64 + 1),
                        Err(LedgerError::Busy) => (),
                        other => panic!("{other:?}"),
                    }
                })
            })
            .collect();
        let reader = Arc::clone(&ledger);
        let snapshot = thread::spawn(move || {
            if reader.lock().unwrap().begin_boundary().is_err() {
                return;
            }
            let before = reader.lock().unwrap().completed_usage_at_boundary();
            let checkpoint = reader.lock().unwrap().checkpoint();
            thread::yield_now();
            let mut state = reader.lock().unwrap();
            assert_eq!(state.completed_usage_at_boundary(), before);
            assert_eq!(state.checkpoint(), checkpoint);
            assert_eq!(state.begin_boundary(), Err(LedgerError::Busy));
            assert!(matches!(before, Ok(3) | Err(LedgerError::Incomplete)));
            state.validate_restore(0, 0).unwrap();
            state.restore(0);
            assert_eq!(
                state.completed_usage_at_boundary(),
                Err(LedgerError::Incomplete)
            );
            state.end_boundary();
        });
        for writer in writers {
            writer.join().unwrap();
        }
        snapshot.join().unwrap();
    });
}

#[test]
fn independent_slots_publish_or_cancel_without_losing_ownership() {
    for cancelled in [false, true] {
        loom::model(move || {
            let ledger = Arc::new(Mutex::new(state(2)));
            let workers: Vec<_> = (0..2)
                .map(|slot| {
                    let ledger = Arc::clone(&ledger);
                    thread::spawn(move || {
                        let serial = ledger.lock().unwrap().reserve(slot).unwrap();
                        thread::yield_now();
                        if cancelled && slot == 0 {
                            ledger.lock().unwrap().cancel(slot, serial);
                        } else {
                            ledger
                                .lock()
                                .unwrap()
                                .publish(slot, serial, slot as u64 + 1);
                        }
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
            let mut ledger = ledger.lock().unwrap();
            assert_eq!(ledger.check_complete().is_ok(), !cancelled);
            assert_eq!(ledger.completed_usage(), if cancelled { 2 } else { 3 });
            if cancelled {
                let serial = ledger.reserve(0).unwrap();
                ledger.publish(0, serial, 1);
            }
            ledger.check_complete().unwrap();
            ledger.begin_boundary().unwrap();
            ledger.check_complete_at_boundary().unwrap();
            assert_eq!(ledger.completed_usage_at_boundary(), Ok(3));
            let prefix = ledger.checkpoint();
            ledger.validate_restore(prefix.0, prefix.1).unwrap();
            ledger.validate_restore(0, 0).unwrap();
            ledger.restore(0);
            assert_eq!(ledger.completed_usage(), 0);
            assert_eq!(
                ledger.completed_usage_at_boundary(),
                Err(LedgerError::Incomplete)
            );
            ledger.end_boundary();
            for slot in 0..2 {
                let serial = ledger.reserve(slot).unwrap();
                ledger.publish(slot, serial, slot as u64 + 1);
            }
            ledger.begin_boundary().unwrap();
            assert_eq!(ledger.completed_usage(), 3);
            assert_eq!(
                ledger.validate_restore(prefix.0, prefix.1),
                Err(LedgerError::Checkpoint)
            );
            ledger.end_boundary();
        });
    }
}

#[test]
fn exclusive_boundary_blocks_reservations_through_restore_or_close() {
    for close in [false, true] {
        loom::model(move || {
            let ledger = Arc::new(Mutex::new(state(1)));
            let worker_ledger = Arc::clone(&ledger);
            let worker = thread::spawn(move || {
                let reserved = worker_ledger.lock().unwrap().reserve(0);
                thread::yield_now();
                match reserved {
                    Ok(serial) => worker_ledger.lock().unwrap().publish(0, serial, 1),
                    Err(LedgerError::Busy | LedgerError::Closed) => (),
                    Err(error) => panic!("{error:?}"),
                }
            });
            let boundary_ledger = Arc::clone(&ledger);
            let boundary = thread::spawn(move || {
                let acquired = boundary_ledger.lock().unwrap().begin_boundary();
                match acquired {
                    Ok(()) => {
                        thread::yield_now();
                        let mut ledger = boundary_ledger.lock().unwrap();
                        let query = ledger.completed_usage_at_boundary();
                        assert!(matches!(query, Ok(1) | Err(LedgerError::Incomplete)));
                        ledger.validate_restore(0, 0).unwrap();
                        ledger.restore(0);
                        assert_eq!(
                            ledger.completed_usage_at_boundary(),
                            Err(LedgerError::Incomplete)
                        );
                        if close {
                            ledger.close();
                            assert_eq!(
                                ledger.completed_usage_at_boundary(),
                                Err(LedgerError::Closed)
                            );
                        }
                        ledger.end_boundary();
                    }
                    Err(LedgerError::Busy) => (),
                    Err(error) => panic!("{error:?}"),
                }
            });
            worker.join().unwrap();
            boundary.join().unwrap();
            let mut ledger = ledger.lock().unwrap();
            match ledger.begin_boundary() {
                Ok(()) => {
                    ledger.checkpoint();
                    ledger.end_boundary();
                }
                Err(LedgerError::Closed) if close => (),
                other => panic!("{other:?}"),
            }
        });
    }
}

fn check_export_boundary(persist: bool) {
    loom::model(move || {
        let mut initial = state(1);
        let serial = initial.reserve(0).unwrap();
        initial.publish(0, serial, 1);
        let ledger = Arc::new(Mutex::new(initial));
        let disk = Arc::new(AtomicU64::new(0));
        let result = Arc::new(AtomicU64::new(0));
        let restoring = Arc::clone(&ledger);
        let restorer = thread::spawn(move || {
            let acquired = restoring.lock().unwrap().begin_boundary();
            match acquired {
                Ok(()) => {
                    thread::yield_now();
                    let mut state = restoring.lock().unwrap();
                    state.validate_restore(0, 0).unwrap();
                    state.restore(0);
                    state.end_boundary();
                }
                Err(LedgerError::Busy | LedgerError::Closed) => (),
                other => panic!("{other:?}"),
            }
        });
        let observed_ledger = Arc::clone(&ledger);
        let observed_disk = Arc::clone(&disk);
        let observed_result = Arc::clone(&result);
        let observer = thread::spawn(move || {
            let exported = observed_result.load(Ordering::Acquire);
            if exported != 0 {
                assert_eq!(
                    observed_disk.load(Ordering::Acquire),
                    exported,
                    "export must follow persistence"
                );
                assert_eq!(
                    observed_ledger.lock().unwrap().begin_boundary(),
                    Err(LedgerError::Closed)
                );
            }
        });
        let acquired = ledger.lock().unwrap().begin_boundary();
        if acquired.is_ok() {
            let evidence = ledger.lock().unwrap().completed_usage_at_boundary();
            if let Ok(usage) = evidence {
                if persist {
                    disk.store(usage, Ordering::Release);
                }
                thread::yield_now();
                let mut state = ledger.lock().unwrap();
                assert_eq!(state.completed_usage_at_boundary(), Ok(usage));
                state.close();
                state.end_boundary();
                drop(state);
                result.store(usage, Ordering::Release);
            } else {
                ledger.lock().unwrap().end_boundary();
            }
        }
        restorer.join().unwrap();
        observer.join().unwrap();
    });
}

#[test]
fn export_boundary_keeps_evidence_stable_until_persistence_and_closure() {
    check_export_boundary(true);
}

#[test]
#[should_panic(expected = "export must follow persistence")]
fn exporting_before_persistence_violates_the_publication_contract() {
    check_export_boundary(false);
}
