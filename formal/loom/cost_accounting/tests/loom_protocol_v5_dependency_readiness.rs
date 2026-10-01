use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

const PARENT: usize = 1 << 0;
const JUSTIFICATION: usize = 1 << 1;
const UNARY: usize = 1 << 2;
const OBJECTIVE_FIRST: usize = 1 << 3;
const OBJECTIVE_SECOND: usize = 1 << 4;
const HEADER_FIRST: usize = 1 << 5;
const HEADER_SECOND: usize = 1 << 6;
const REQUIRED: usize = PARENT
    | JUSTIFICATION
    | UNARY
    | OBJECTIVE_FIRST
    | OBJECTIVE_SECOND
    | HEADER_FIRST
    | HEADER_SECOND;

struct ReadinessState {
    admitted_metadata: AtomicUsize,
    invalid_index: AtomicUsize,
    tracker: AtomicUsize,
    direct_ready: AtomicBool,
    buffer_ready: AtomicBool,
}

impl ReadinessState {
    fn new() -> Self {
        Self {
            admitted_metadata: AtomicUsize::new(0),
            invalid_index: AtomicUsize::new(0),
            tracker: AtomicUsize::new(0),
            direct_ready: AtomicBool::new(false),
            buffer_ready: AtomicBool::new(false),
        }
    }

    fn admitted_snapshot(&self) -> usize { self.admitted_metadata.load(Ordering::Acquire) }

    fn resolve_direct(&self) -> bool {
        let snapshot = self.admitted_snapshot();
        let ready = snapshot & REQUIRED == REQUIRED;
        if ready {
            self.direct_ready.store(true, Ordering::Release);
        }
        ready
    }

    fn resolve_buffer(&self) -> bool {
        let snapshot = self.admitted_snapshot();
        let ready = snapshot & REQUIRED == REQUIRED;
        if ready {
            self.buffer_ready.store(true, Ordering::Release);
        }
        ready
    }
}

#[test]
fn concurrent_metadata_publication_preserves_direct_buffer_readiness_parity() {
    loom::model(|| {
        let state = Arc::new(ReadinessState::new());
        let structural = {
            let state = state.clone();
            thread::spawn(move || {
                state
                    .admitted_metadata
                    .fetch_or(PARENT | JUSTIFICATION | UNARY, Ordering::Release);
                state.invalid_index.fetch_or(UNARY, Ordering::Release);
            })
        };
        let certified = {
            let state = state.clone();
            thread::spawn(move || {
                state.admitted_metadata.fetch_or(
                    OBJECTIVE_FIRST | OBJECTIVE_SECOND | HEADER_FIRST | HEADER_SECOND,
                    Ordering::Release,
                );
                state.tracker.fetch_or(OBJECTIVE_FIRST, Ordering::Release);
            })
        };
        let resolver = {
            let state = state.clone();
            thread::spawn(move || {
                let direct = state.resolve_direct();
                let buffer = state.resolve_buffer();
                if direct || buffer {
                    assert_eq!(state.admitted_snapshot() & REQUIRED, REQUIRED);
                }
            })
        };

        structural.join().unwrap();
        certified.join().unwrap();
        resolver.join().unwrap();

        assert_eq!(state.admitted_snapshot() & REQUIRED, REQUIRED);
        assert!(state.resolve_direct());
        assert!(state.resolve_buffer());
        assert!(state.direct_ready.load(Ordering::Acquire));
        assert!(state.buffer_ready.load(Ordering::Acquire));
    });
}

#[test]
fn objective_pair_requires_both_admitted_metadata_records() {
    loom::model(|| {
        let state = Arc::new(ReadinessState::new());
        state
            .admitted_metadata
            .store(REQUIRED & !OBJECTIVE_SECOND, Ordering::Release);

        let hint_publisher = {
            let state = state.clone();
            thread::spawn(move || {
                state
                    .invalid_index
                    .fetch_or(OBJECTIVE_SECOND, Ordering::Release);
                state.tracker.fetch_or(OBJECTIVE_SECOND, Ordering::Release);
            })
        };
        let resolver = {
            let state = state.clone();
            thread::spawn(move || {
                assert!(!state.resolve_direct());
                assert!(!state.resolve_buffer());
            })
        };

        hint_publisher.join().unwrap();
        resolver.join().unwrap();

        assert!(!state.resolve_direct());
        assert!(!state.resolve_buffer());
        state
            .admitted_metadata
            .fetch_or(OBJECTIVE_SECOND, Ordering::Release);
        assert!(state.resolve_direct());
        assert!(state.resolve_buffer());
    });
}

#[test]
fn header_evidence_requires_both_admitted_metadata_records() {
    loom::model(|| {
        let state = Arc::new(ReadinessState::new());
        state
            .admitted_metadata
            .store(REQUIRED & !HEADER_SECOND, Ordering::Release);
        state.invalid_index.store(HEADER_SECOND, Ordering::Release);
        state.tracker.store(HEADER_SECOND, Ordering::Release);

        assert!(!state.resolve_direct());
        assert!(!state.resolve_buffer());

        state
            .admitted_metadata
            .fetch_or(HEADER_SECOND, Ordering::Release);
        assert!(state.resolve_direct());
        assert!(state.resolve_buffer());
    });
}
