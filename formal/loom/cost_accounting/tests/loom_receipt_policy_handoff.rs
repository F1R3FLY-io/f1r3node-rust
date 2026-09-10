use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/engine/block_retriever/receipt_policy.rs"]
mod receipt_policy;

struct Request {
    received: bool,
    in_buffer: bool,
    timestamp: u64,
    initial_timestamp: u64,
    attempts: u32,
}

#[test]
fn maintenance_uses_current_receipt_state_after_concurrent_handoff() {
    loom::model(|| {
        let request = Arc::new(Mutex::new(Request {
            received: true,
            in_buffer: false,
            timestamp: 1,
            initial_timestamp: 1,
            attempts: 7,
        }));
        let maintenance_state = request.clone();
        let maintenance = thread::spawn(move || {
            let observed = maintenance_state.lock().unwrap().received;
            thread::yield_now();
            if observed {
                let mut state = maintenance_state.lock().unwrap();
                let Request {
                    received,
                    in_buffer,
                    timestamp,
                    initial_timestamp,
                    ..
                } = &mut *state;
                receipt_policy::reopen_stale_receipt(
                    received,
                    *in_buffer,
                    timestamp,
                    *initial_timestamp,
                    100,
                    10,
                );
            }
        });
        let worker_state = request.clone();
        let worker = thread::spawn(move || {
            let mut state = worker_state.lock().unwrap();
            state.received = false;
            state.timestamp = 20;
        });
        maintenance.join().unwrap();
        worker.join().unwrap();
        let final_state = request.lock().unwrap();
        assert!(!final_state.received);
        assert_eq!(final_state.timestamp, 20);
        assert_eq!(final_state.initial_timestamp, 1);
        assert_eq!(final_state.attempts, 7);
    });
}

#[test]
#[should_panic(expected = "stale maintenance overwrote worker handoff")]
fn trusting_the_old_receipt_snapshot_is_an_unsafe_control() {
    loom::model(|| {
        let request = Arc::new(Mutex::new((true, 1)));
        let maintenance_state = request.clone();
        let maintenance = thread::spawn(move || {
            let observed = maintenance_state.lock().unwrap().0;
            thread::yield_now();
            if observed {
                *maintenance_state.lock().unwrap() = (false, 100);
            }
        });
        let worker_state = request.clone();
        let worker = thread::spawn(move || {
            *worker_state.lock().unwrap() = (false, 20);
        });
        maintenance.join().unwrap();
        worker.join().unwrap();
        assert_eq!(
            request.lock().unwrap().1,
            20,
            "stale maintenance overwrote worker handoff"
        );
    });
}
