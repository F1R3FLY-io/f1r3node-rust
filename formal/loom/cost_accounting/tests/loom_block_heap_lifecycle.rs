use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

struct BlockHeapLifecycle {
    completions_since_trim: AtomicUsize,
    retained_units: Mutex<usize>,
    semantic_commits: AtomicUsize,
    trim_count: AtomicUsize,
}

impl BlockHeapLifecycle {
    fn new() -> Self {
        Self {
            completions_since_trim: AtomicUsize::new(0),
            retained_units: Mutex::new(0),
            semantic_commits: AtomicUsize::new(0),
            trim_count: AtomicUsize::new(0),
        }
    }

    fn complete(&self, interval: usize) {
        self.semantic_commits.fetch_add(1, Ordering::Relaxed);
        *self.retained_units.lock().unwrap() += 1;
        if self.record_completion(interval) {
            *self.retained_units.lock().unwrap() = 0;
            self.trim_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_completion(&self, interval: usize) -> bool {
        if interval == 0 {
            return false;
        }
        let mut current = self.completions_since_trim.load(Ordering::Relaxed);
        loop {
            let (next, should_trim) = if current >= interval - 1 {
                (0, true)
            } else {
                (current + 1, false)
            };
            match self.completions_since_trim.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return should_trim,
                Err(observed) => current = observed,
            }
        }
    }
}

#[test]
fn concurrent_default_boundaries_reclaim_every_completed_block() {
    loom::model(|| {
        let lifecycle = Arc::new(BlockHeapLifecycle::new());
        let left = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.complete(1))
        };
        let right = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.complete(1))
        };

        left.join().unwrap();
        right.join().unwrap();
        assert_eq!(lifecycle.semantic_commits.load(Ordering::Relaxed), 2);
        assert_eq!(lifecycle.trim_count.load(Ordering::Relaxed), 2);
        assert_eq!(lifecycle.completions_since_trim.load(Ordering::Relaxed), 0);
        assert_eq!(*lifecycle.retained_units.lock().unwrap(), 0);
    });
}

#[test]
fn concurrent_periodic_boundaries_preserve_the_interval_bound() {
    loom::model(|| {
        let lifecycle = Arc::new(BlockHeapLifecycle::new());
        let handles = (0..3)
            .map(|_| {
                let lifecycle = lifecycle.clone();
                thread::spawn(move || lifecycle.complete(2))
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(lifecycle.semantic_commits.load(Ordering::Relaxed), 3);
        assert_eq!(lifecycle.trim_count.load(Ordering::Relaxed), 1);
        assert_eq!(lifecycle.completions_since_trim.load(Ordering::Relaxed), 1);
        assert!(*lifecycle.retained_units.lock().unwrap() <= 1);
    });
}

#[test]
fn disabled_reclamation_never_mutates_semantic_state_or_counter() {
    loom::model(|| {
        let lifecycle = Arc::new(BlockHeapLifecycle::new());
        let left = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.complete(0))
        };
        let right = {
            let lifecycle = lifecycle.clone();
            thread::spawn(move || lifecycle.complete(0))
        };

        left.join().unwrap();
        right.join().unwrap();
        assert_eq!(lifecycle.semantic_commits.load(Ordering::Relaxed), 2);
        assert_eq!(lifecycle.trim_count.load(Ordering::Relaxed), 0);
        assert_eq!(lifecycle.completions_since_trim.load(Ordering::Relaxed), 0);
        assert_eq!(*lifecycle.retained_units.lock().unwrap(), 2);
    });
}
