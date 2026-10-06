use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

struct Budget {
    usage: [AtomicUsize; 2],
    rejected: AtomicBool,
}

impl Budget {
    fn new() -> Self {
        Self {
            usage: [AtomicUsize::new(0), AtomicUsize::new(0)],
            rejected: AtomicBool::new(false),
        }
    }

    fn reserve(&self, dimension: usize, requested: usize, limit: usize) -> bool {
        if requested == 0 {
            return true;
        }
        if self.rejected.load(Ordering::Acquire) {
            return false;
        }
        let counter = &self.usage[dimension];
        let mut current = counter.load(Ordering::Acquire);
        loop {
            let Some(candidate) = current.checked_add(requested) else {
                self.rejected.store(true, Ordering::Release);
                return false;
            };
            if candidate > limit {
                self.rejected.store(true, Ordering::Release);
                return false;
            }
            match counter.compare_exchange_weak(
                current,
                candidate,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }
}

#[test]
fn same_dimension_concurrent_reservations_commit_the_exact_sum() {
    loom::model(|| {
        let budget = Arc::new(Budget::new());
        let left = {
            let budget = budget.clone();
            thread::spawn(move || budget.reserve(0, 1, 2))
        };
        let right = {
            let budget = budget.clone();
            thread::spawn(move || budget.reserve(0, 1, 2))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        assert_eq!(budget.usage[0].load(Ordering::Acquire), 2);
        assert!(!budget.rejected.load(Ordering::Acquire));
    });
}

#[test]
fn one_over_limit_reservation_rejects_the_complete_transaction() {
    loom::model(|| {
        let budget = Arc::new(Budget::new());
        let tentative_state = Arc::new(AtomicUsize::new(0));
        let left = {
            let budget = budget.clone();
            let tentative_state = tentative_state.clone();
            thread::spawn(move || {
                if budget.reserve(0, 1, 1) {
                    tentative_state.fetch_add(1, Ordering::AcqRel);
                }
            })
        };
        let right = {
            let budget = budget.clone();
            let tentative_state = tentative_state.clone();
            thread::spawn(move || {
                if budget.reserve(0, 1, 1) {
                    tentative_state.fetch_add(1, Ordering::AcqRel);
                }
            })
        };

        left.join().unwrap();
        right.join().unwrap();
        assert!(budget.rejected.load(Ordering::Acquire));
        assert_eq!(budget.usage[0].load(Ordering::Acquire), 1);
        let published_state = if budget.rejected.load(Ordering::Acquire) {
            0
        } else {
            tentative_state.load(Ordering::Acquire)
        };
        assert_eq!(published_state, 0);
    });
}

#[test]
fn independent_dimensions_reserve_without_cross_subsidy() {
    loom::model(|| {
        let budget = Arc::new(Budget::new());
        let left = {
            let budget = budget.clone();
            thread::spawn(move || budget.reserve(0, 1, 1))
        };
        let right = {
            let budget = budget.clone();
            thread::spawn(move || budget.reserve(1, 1, 1))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        assert_eq!(budget.usage[0].load(Ordering::Acquire), 1);
        assert_eq!(budget.usage[1].load(Ordering::Acquire), 1);
        assert!(!budget.rejected.load(Ordering::Acquire));
    });
}

#[test]
fn zero_reservation_remains_a_noop_after_rejection() {
    loom::model(|| {
        let budget = Arc::new(Budget::new());
        assert!(!budget.reserve(0, 2, 1));
        assert!(budget.reserve(1, 0, 0));
        assert_eq!(budget.usage[0].load(Ordering::Acquire), 0);
        assert_eq!(budget.usage[1].load(Ordering::Acquire), 0);
        assert!(budget.rejected.load(Ordering::Acquire));
    });
}

#[test]
fn shard_budgets_reject_independently() {
    loom::model(|| {
        let left_shard = Arc::new(Budget::new());
        let right_shard = Arc::new(Budget::new());
        let reject_left = {
            let budget = left_shard.clone();
            thread::spawn(move || budget.reserve(0, 2, 1))
        };
        let accept_right = {
            let budget = right_shard.clone();
            thread::spawn(move || budget.reserve(0, 1, 1))
        };

        assert!(!reject_left.join().unwrap());
        assert!(accept_right.join().unwrap());
        assert!(left_shard.rejected.load(Ordering::Acquire));
        assert!(!right_shard.rejected.load(Ordering::Acquire));
        assert_eq!(right_shard.usage[0].load(Ordering::Acquire), 1);
    });
}
