use std::sync::Arc;

use loom::sync::Mutex;
use loom::thread;

#[derive(Clone, Copy, Debug)]
struct RequestState {
    unresolved: bool,
    dependency_evidence: bool,
    received: bool,
    quarantine_until: Option<u64>,
}

struct Lifecycle {
    request: Mutex<Option<RequestState>>,
    attempts: Mutex<u32>,
}

impl Lifecycle {
    fn new() -> Self {
        Self {
            request: Mutex::new(Some(RequestState {
                unresolved: true,
                dependency_evidence: true,
                received: false,
                quarantine_until: None,
            })),
            attempts: Mutex::new(32),
        }
    }

    fn exhaust_retry_budget(&self) {
        if let Some(request) = self.request.lock().unwrap().as_mut() {
            request.quarantine_until = Some(10_000);
        }
        thread::yield_now();
        *self.attempts.lock().unwrap() = 0;
    }

    fn receive(&self) {
        if let Some(request) = self.request.lock().unwrap().as_mut() {
            request.received = true;
        }
        thread::yield_now();
        *self.attempts.lock().unwrap() = 0;
        if let Some(request) = self.request.lock().unwrap().as_mut() {
            request.quarantine_until = None;
        }
    }

    fn admit(&self) {
        *self.request.lock().unwrap() = None;
        thread::yield_now();
        *self.attempts.lock().unwrap() = 0;
    }
}

#[test]
fn concurrent_receipt_and_retry_exhaustion_preserve_dependency_evidence() {
    loom::model(|| {
        let lifecycle = Arc::new(Lifecycle::new());
        let exhaustion = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.exhaust_retry_budget())
        };
        let receipt = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.receive())
        };

        exhaustion.join().unwrap();
        receipt.join().unwrap();

        let request = lifecycle.request.lock().unwrap().expect("request evidence");
        assert!(request.unresolved);
        assert!(request.dependency_evidence);
        assert!(request.received);
        assert_eq!(*lifecycle.attempts.lock().unwrap(), 0);
    });
}

#[test]
fn durable_admission_dominates_concurrent_retry_exhaustion() {
    loom::model(|| {
        let lifecycle = Arc::new(Lifecycle::new());
        let exhaustion = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.exhaust_retry_budget())
        };
        let admission = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.admit())
        };

        exhaustion.join().unwrap();
        admission.join().unwrap();

        assert!(lifecycle.request.lock().unwrap().is_none());
        assert_eq!(*lifecycle.attempts.lock().unwrap(), 0);
    });
}
