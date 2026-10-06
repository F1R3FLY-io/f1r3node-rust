use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

const PENDING: usize = 0;
const SUCCEEDED: usize = 1;
const FAILED: usize = 2;
const UNMINTED_STATE: usize = 0;

struct EpochAttempt {
    targets: [AtomicUsize; 2],
    committed_state: Arc<AtomicUsize>,
}

impl EpochAttempt {
    fn new(committed_state: Arc<AtomicUsize>) -> Self {
        Self {
            targets: [AtomicUsize::new(PENDING), AtomicUsize::new(PENDING)],
            committed_state,
        }
    }

    fn complete(&self, target: usize, result: usize) {
        let _ = self.targets[target].compare_exchange(
            PENDING,
            result,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn publish(&self, epoch: usize) -> bool {
        let complete = self
            .targets
            .iter()
            .all(|target| target.load(Ordering::Acquire) == SUCCEEDED);
        if !complete {
            return false;
        }
        loop {
            let current = self.committed_state.load(Ordering::Acquire);
            let allowed = (current == UNMINTED_STATE && matches!(epoch, 0 | 1))
                || (current > UNMINTED_STATE && epoch == current);
            if epoch < current || !allowed {
                return false;
            }
            if self
                .committed_state
                .compare_exchange(current, epoch + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }
}

fn assert_observable_state(state: usize, maximum_epoch: usize) {
    assert!(state == UNMINTED_STATE || state <= maximum_epoch + 1);
}

#[test]
fn concurrent_disjoint_mints_publish_one_complete_epoch_root() {
    loom::model(|| {
        let committed = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let attempt = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        let left = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || attempt.complete(0, SUCCEEDED))
        };
        let right = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || attempt.complete(1, SUCCEEDED))
        };
        let observer = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || {
                assert_observable_state(attempt.committed_state.load(Ordering::Acquire), 1);
            })
        };

        left.join().unwrap();
        right.join().unwrap();
        assert!(attempt.publish(1));
        observer.join().unwrap();
        assert_eq!(committed.load(Ordering::Acquire), 2);
    });
}

#[test]
fn concurrent_failure_preserves_the_complete_pre_epoch_root() {
    loom::model(|| {
        let committed = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let attempt = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        let success = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || attempt.complete(0, SUCCEEDED))
        };
        let failure = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || attempt.complete(1, FAILED))
        };
        let observer = {
            let attempt = Arc::clone(&attempt);
            thread::spawn(move || {
                assert_observable_state(attempt.committed_state.load(Ordering::Acquire), 1);
            })
        };

        success.join().unwrap();
        failure.join().unwrap();
        assert!(!attempt.publish(1));
        observer.join().unwrap();
        assert_eq!(committed.load(Ordering::Acquire), UNMINTED_STATE);
    });
}

#[test]
fn duplicate_completion_and_retry_credit_each_target_once() {
    loom::model(|| {
        let committed = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let first_attempt = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        let original = {
            let attempt = Arc::clone(&first_attempt);
            thread::spawn(move || attempt.complete(0, FAILED))
        };
        let duplicate = {
            let attempt = Arc::clone(&first_attempt);
            thread::spawn(move || attempt.complete(0, SUCCEEDED))
        };
        original.join().unwrap();
        duplicate.join().unwrap();
        assert!(!first_attempt.publish(1));
        assert_eq!(committed.load(Ordering::Acquire), UNMINTED_STATE);

        let retry = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        let left = {
            let attempt = Arc::clone(&retry);
            thread::spawn(move || attempt.complete(0, SUCCEEDED))
        };
        let right = {
            let attempt = Arc::clone(&retry);
            thread::spawn(move || attempt.complete(1, SUCCEEDED))
        };
        left.join().unwrap();
        right.join().unwrap();
        assert!(retry.publish(1));
        assert!(!retry.publish(1));
        assert_eq!(committed.load(Ordering::Acquire), 2);
    });
}

#[test]
fn sibling_epoch_closes_publish_exactly_one_complete_frontier() {
    loom::model(|| {
        let committed = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let left = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        let right = Arc::new(EpochAttempt::new(Arc::clone(&committed)));
        for attempt in [&left, &right] {
            attempt.complete(0, SUCCEEDED);
            attempt.complete(1, SUCCEEDED);
        }
        let left_thread = {
            let attempt = Arc::clone(&left);
            thread::spawn(move || attempt.publish(1))
        };
        let right_thread = {
            let attempt = Arc::clone(&right);
            thread::spawn(move || attempt.publish(1))
        };
        let published =
            usize::from(left_thread.join().unwrap()) + usize::from(right_thread.join().unwrap());
        assert_eq!(published, 1);
        assert_eq!(committed.load(Ordering::Acquire), 2);
    });
}

#[test]
fn frontier_accepts_both_bootstraps_and_rejects_gaps() {
    loom::model(|| {
        let epoch_zero_state = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let epoch_zero = EpochAttempt::new(Arc::clone(&epoch_zero_state));
        epoch_zero.complete(0, SUCCEEDED);
        epoch_zero.complete(1, SUCCEEDED);
        assert!(epoch_zero.publish(0));
        assert_eq!(epoch_zero_state.load(Ordering::Acquire), 1);

        let epoch_one_state = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let epoch_one = EpochAttempt::new(Arc::clone(&epoch_one_state));
        epoch_one.complete(0, SUCCEEDED);
        epoch_one.complete(1, SUCCEEDED);
        assert!(epoch_one.publish(1));
        assert_eq!(epoch_one_state.load(Ordering::Acquire), 2);

        let gap_state = Arc::new(AtomicUsize::new(UNMINTED_STATE));
        let gap = EpochAttempt::new(Arc::clone(&gap_state));
        gap.complete(0, SUCCEEDED);
        gap.complete(1, SUCCEEDED);
        assert!(!gap.publish(2));
        assert_eq!(gap_state.load(Ordering::Acquire), UNMINTED_STATE);
    });
}
