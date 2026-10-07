use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/state.rs"]
mod production;

use production::{LedgerError, ReplayState, SlotState};

#[test]
fn dependent_reservation_requires_publication_not_only_a_live_ticket() {
    for cancel in [false, true] {
        loom::model(move || {
            let state = Arc::new(Mutex::new(ReplayState::new(
                vec![SlotState::Available; 2],
                Vec::with_capacity(2),
                2,
            )));
            let owner = Arc::clone(&state);
            let first = thread::spawn(move || {
                let serial = owner.lock().unwrap().reserve_ready(0, []).unwrap().unwrap();
                thread::yield_now();
                let mut state = owner.lock().unwrap();
                if cancel {
                    state.cancel(0, serial);
                } else {
                    state.publish(0, serial, 1);
                }
            });
            let dependent = Arc::clone(&state);
            let second = thread::spawn(move || {
                let mut state = dependent.lock().unwrap();
                let reserved = state.reserve_ready(1, [0]).unwrap();
                if let Some(serial) = reserved {
                    assert_eq!(state.completed_usage(), 1);
                    state.publish(1, serial, 1);
                    true
                } else {
                    assert!(state.completed_usage() < 1);
                    false
                }
            });
            first.join().unwrap();
            let published = second.join().unwrap();
            assert!(!cancel || !published);
            let mut state = state.lock().unwrap();
            if cancel {
                let serial = state.reserve_ready(0, []).unwrap().unwrap();
                state.publish(0, serial, 1);
            }
            if !published {
                let serial = state.reserve_ready(1, [0]).unwrap().unwrap();
                state.publish(1, serial, 1);
            }
            state.check_complete().unwrap();
            state.begin_boundary().unwrap();
            let (cursor, serial) = state.checkpoint();
            state.validate_restore(cursor, serial).unwrap();
            state.validate_restore(0, 0).unwrap();
            state.restore(0);
            assert_eq!(
                state.validate_restore(cursor, serial),
                Err(LedgerError::Checkpoint)
            );
            state.end_boundary();
            assert_eq!(state.reserve_ready(1, [0]).unwrap(), None);
        });
    }
}

#[test]
fn restore_between_readiness_and_reservation_cannot_bypass_a_predecessor() {
    loom::model(|| {
        let mut initial = ReplayState::new(vec![SlotState::Available; 2], Vec::with_capacity(2), 2);
        let serial = initial.reserve_ready(0, []).unwrap().unwrap();
        initial.publish(0, serial, 1);
        let state = Arc::new(Mutex::new(initial));
        let dependent = Arc::clone(&state);
        let worker = thread::spawn(move || {
            if !dependent.lock().unwrap().ready(1, [0]).unwrap() {
                return;
            }
            thread::yield_now();
            let mut state = dependent.lock().unwrap();
            match state.reserve_ready(1, [0]) {
                Ok(Some(serial)) => {
                    assert_eq!(state.completed_usage(), 1);
                    state.publish(1, serial, 1);
                }
                Ok(None) => assert_eq!(state.completed_usage(), 0),
                Err(LedgerError::Busy) => (),
                other => panic!("{other:?}"),
            }
        });
        let restorer = Arc::clone(&state);
        let restore = thread::spawn(move || {
            restorer.lock().unwrap().begin_boundary().unwrap();
            thread::yield_now();
            let mut state = restorer.lock().unwrap();
            state.validate_restore(0, 0).unwrap();
            state.restore(0);
            state.end_boundary();
        });
        worker.join().unwrap();
        restore.join().unwrap();
        let mut state = state.lock().unwrap();
        assert_eq!(state.completed_usage(), 0);
        assert_eq!(state.reserve_ready(1, [0]).unwrap(), None);
    });
}

#[test]
fn independent_reservations_remain_available_during_another_ticket() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplayState::new(
            vec![SlotState::Available; 2],
            Vec::with_capacity(2),
            2,
        )));
        let workers: Vec<_> = (0..2)
            .map(|slot| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let serial = state
                        .lock()
                        .unwrap()
                        .reserve_ready(slot, [])
                        .unwrap()
                        .unwrap();
                    thread::yield_now();
                    state.lock().unwrap().publish(slot, serial, 1);
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        state.lock().unwrap().check_complete().unwrap();
    });
}
