use loom::sync::{Arc, Mutex};
use loom::thread;

const ALL_USERS: usize = 0b1111;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FinalAttempt {
    window: usize,
    admitted: usize,
    rejected: usize,
    deferred: usize,
    suffix: usize,
    certified: bool,
    checkpoint_succeeded: bool,
}

impl FinalAttempt {
    fn partition_is_valid(self) -> bool {
        let terminal = self.admitted | self.rejected;
        self.certified
            && self.checkpoint_succeeded
            && terminal & self.deferred == 0
            && self.admitted & self.rejected == 0
            && terminal | self.deferred == self.window
            && self.window & self.suffix == 0
            && self.window | self.suffix == ALL_USERS
    }

    fn terminal(self) -> usize { self.admitted | self.rejected }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CustodySnapshot {
    stored: usize,
    settled: usize,
    published: Option<FinalAttempt>,
}

struct SharedCustody {
    state: Mutex<CustodySnapshot>,
}

impl SharedCustody {
    fn new() -> Self {
        Self {
            state: Mutex::new(CustodySnapshot {
                stored: ALL_USERS,
                settled: 0,
                published: None,
            }),
        }
    }

    fn publish_final(&self, attempt: FinalAttempt) -> bool {
        if !attempt.partition_is_valid() {
            return false;
        }
        let terminal = attempt.terminal();
        let mut state = self.state.lock().unwrap();
        if state.published.is_some() || state.stored & terminal != terminal {
            return false;
        }
        state.stored &= !terminal;
        state.settled = attempt.admitted;
        state.published = Some(attempt);
        true
    }

    fn snapshot(&self) -> CustodySnapshot { *self.state.lock().unwrap() }
}

fn final_attempt() -> FinalAttempt {
    FinalAttempt {
        window: 0b0111,
        admitted: 0b0001,
        rejected: 0b0010,
        deferred: 0b0100,
        suffix: 0b1000,
        certified: true,
        checkpoint_succeeded: true,
    }
}

#[test]
fn final_publication_and_storage_observation_are_atomic() {
    loom::model(|| {
        let custody = Arc::new(SharedCustody::new());
        let writer = custody.clone();
        let publish = thread::spawn(move || writer.publish_final(final_attempt()));
        let reader = custody.clone();
        let observe = thread::spawn(move || reader.snapshot());

        assert!(publish.join().unwrap());
        let observed = observe.join().unwrap();
        let initial = CustodySnapshot {
            stored: ALL_USERS,
            settled: 0,
            published: None,
        };
        let completed = custody.snapshot();
        assert!(observed == initial || observed == completed);
        assert_eq!(completed.stored, 0b1100);
        assert_eq!(completed.settled, 0b0001);
        assert_eq!(completed.published, Some(final_attempt()));
    });
}

#[test]
fn failed_checkpoint_cannot_mutate_shared_custody() {
    loom::model(|| {
        let custody = Arc::new(SharedCustody::new());
        let failed = FinalAttempt {
            checkpoint_succeeded: false,
            ..final_attempt()
        };
        let writer = custody.clone();
        let publish = thread::spawn(move || writer.publish_final(failed));
        let reader = custody.clone();
        let observe = thread::spawn(move || reader.snapshot());

        assert!(!publish.join().unwrap());
        assert_eq!(observe.join().unwrap(), CustodySnapshot {
            stored: ALL_USERS,
            settled: 0,
            published: None,
        });
        assert_eq!(custody.snapshot(), CustodySnapshot {
            stored: ALL_USERS,
            settled: 0,
            published: None,
        });
    });
}

#[test]
fn incomplete_or_mixed_partition_cannot_compete_with_final_attempt() {
    loom::model(|| {
        let custody = Arc::new(SharedCustody::new());
        let mixed = FinalAttempt {
            admitted: 0b0011,
            rejected: 0b0010,
            deferred: 0b0100,
            ..final_attempt()
        };
        let invalid_writer = custody.clone();
        let invalid = thread::spawn(move || invalid_writer.publish_final(mixed));
        let final_writer = custody.clone();
        let valid = thread::spawn(move || final_writer.publish_final(final_attempt()));

        assert!(!invalid.join().unwrap());
        assert!(valid.join().unwrap());
        let state = custody.snapshot();
        assert_eq!(state.stored, 0b1100);
        assert_eq!(state.settled, 0b0001);
        assert_eq!(state.published, Some(final_attempt()));
    });
}

#[test]
fn independent_validator_custody_has_no_global_lock() {
    loom::model(|| {
        let left = Arc::new(SharedCustody::new());
        let right = Arc::new(SharedCustody::new());
        let left_writer = left.clone();
        let left_publish = thread::spawn(move || left_writer.publish_final(final_attempt()));
        let right_writer = right.clone();
        let right_publish = thread::spawn(move || right_writer.publish_final(final_attempt()));

        assert!(left_publish.join().unwrap());
        assert!(right_publish.join().unwrap());
        assert_eq!(left.snapshot(), right.snapshot());
        assert_eq!(left.snapshot().stored, 0b1100);
    });
}
