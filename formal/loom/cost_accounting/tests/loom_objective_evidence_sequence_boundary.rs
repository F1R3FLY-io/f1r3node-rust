use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct DurableAdmission {
    sequence: i32,
    metadata: bool,
    evidence: bool,
}

fn persist_metadata(state: &Mutex<DurableAdmission>) { state.lock().unwrap().metadata = true; }

fn repair_evidence(state: &Mutex<DurableAdmission>) {
    let mut state = state.lock().unwrap();
    if state.metadata && state.sequence >= 0 {
        state.evidence = true;
    }
}

#[test]
fn negative_sequence_cannot_enter_evidence_during_parallel_persistence() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(DurableAdmission {
            sequence: -2,
            metadata: false,
            evidence: false,
        }));
        let persist = {
            let state = state.clone();
            thread::spawn(move || persist_metadata(&state))
        };
        let index = {
            let state = state.clone();
            thread::spawn(move || repair_evidence(&state))
        };
        persist.join().unwrap();
        index.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.metadata);
        assert!(!state.evidence);
    });
}

#[test]
fn duplicate_retry_and_reconciliation_preserve_negative_sequence_exclusion() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(DurableAdmission {
            sequence: -1,
            metadata: true,
            evidence: false,
        }));
        let duplicate_retry = {
            let state = state.clone();
            thread::spawn(move || repair_evidence(&state))
        };
        let reconciliation = {
            let state = state.clone();
            thread::spawn(move || repair_evidence(&state))
        };
        duplicate_retry.join().unwrap();
        reconciliation.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.metadata);
        assert!(!state.evidence);
    });
}

#[test]
fn eligible_sequence_repair_is_idempotent_under_parallel_retry() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(DurableAdmission {
            sequence: 0,
            metadata: true,
            evidence: false,
        }));
        let first = {
            let state = state.clone();
            thread::spawn(move || repair_evidence(&state))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || repair_evidence(&state))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert!(state.metadata);
        assert!(state.evidence);
    });
}
