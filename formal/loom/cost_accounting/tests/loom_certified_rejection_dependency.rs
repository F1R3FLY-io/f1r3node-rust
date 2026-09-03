use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RejectionReason {
    InvalidSequenceNumber,
    AdmissibleEquivocation,
}

#[derive(Debug)]
struct State {
    metadata: Option<RejectionReason>,
    dependency_buffered: bool,
    child_buffered: bool,
    child_rejected: bool,
    evidence_count: usize,
}

fn evidence_eligible(reason: RejectionReason) -> bool {
    matches!(reason, RejectionReason::AdmissibleEquivocation)
}

fn persist_rejection(state: &Mutex<State>, reason: RejectionReason) {
    let mut state = state.lock().unwrap();
    match state.metadata {
        None => state.metadata = Some(reason),
        Some(stored) => assert_eq!(stored, reason),
    }
    state.dependency_buffered = false;
    if evidence_eligible(reason) {
        state.evidence_count = 1;
    }
}

fn classify_child(state: &Mutex<State>) {
    let mut state = state.lock().unwrap();
    if state.metadata.is_some() {
        state.child_buffered = false;
        state.child_rejected = true;
    }
}

fn initial_state() -> State {
    State {
        metadata: None,
        dependency_buffered: true,
        child_buffered: true,
        child_rejected: false,
        evidence_count: 0,
    }
}

#[test]
fn concurrent_dependency_delivery_and_child_retry_converge() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(initial_state()));
        let deliver = {
            let state = state.clone();
            thread::spawn(move || persist_rejection(&state, RejectionReason::InvalidSequenceNumber))
        };
        let classify = {
            let state = state.clone();
            thread::spawn(move || classify_child(&state))
        };

        deliver.join().unwrap();
        classify.join().unwrap();
        classify_child(&state);

        let state = state.lock().unwrap();
        assert_eq!(state.metadata, Some(RejectionReason::InvalidSequenceNumber));
        assert!(!state.dependency_buffered);
        assert!(!state.child_buffered);
        assert!(state.child_rejected);
        assert_eq!(state.evidence_count, 0);
    });
}

#[test]
fn duplicate_non_slashable_deliveries_are_idempotent() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(initial_state()));
        let first = {
            let state = state.clone();
            thread::spawn(move || persist_rejection(&state, RejectionReason::InvalidSequenceNumber))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || persist_rejection(&state, RejectionReason::InvalidSequenceNumber))
        };

        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.metadata, Some(RejectionReason::InvalidSequenceNumber));
        assert!(!state.dependency_buffered);
        assert_eq!(state.evidence_count, 0);
    });
}

#[test]
fn slashable_duplicate_deliveries_mint_one_evidence_record() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(initial_state()));
        let first = {
            let state = state.clone();
            thread::spawn(move || {
                persist_rejection(&state, RejectionReason::AdmissibleEquivocation)
            })
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || {
                persist_rejection(&state, RejectionReason::AdmissibleEquivocation)
            })
        };

        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(
            state.metadata,
            Some(RejectionReason::AdmissibleEquivocation)
        );
        assert!(!state.dependency_buffered);
        assert_eq!(state.evidence_count, 1);
    });
}
