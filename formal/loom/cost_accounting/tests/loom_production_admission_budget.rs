#![feature(arbitrary_self_types)]

use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../casper/src/rust/blocks/block_processing_queue/admission_budget.rs"]
mod admission_budget;

use admission_budget::BlockAdmissionBudget;

fn record_admission_bytes(_bytes: usize) {}

fn explore(test: impl Fn() + Send + Sync + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

#[test]
fn actual_reserve_and_drop_never_overdraw_or_leak() {
    explore(|| {
        let budget = Arc::new(BlockAdmissionBudget::new(3));
        let workers = [2, 2].map(|bytes| {
            let budget = budget.clone();
            thread::spawn(move || {
                if let Ok(reservation) = budget.try_reserve(bytes) {
                    assert_eq!(reservation.bytes(), bytes);
                    assert!(budget.used() <= budget.capacity());
                    thread::yield_now();
                    drop(reservation);
                }
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(budget.used(), 0);
    });
}

#[test]
fn actual_reservation_survives_queue_and_worker_transfer() {
    explore(|| {
        let budget = Arc::new(BlockAdmissionBudget::new(7));
        let queued = Arc::new(Mutex::new(Some(budget.try_reserve(7).unwrap())));
        let worker_queue = queued.clone();
        let worker_budget = budget.clone();
        let worker = thread::spawn(move || {
            let reservation = worker_queue.lock().unwrap().take().unwrap();
            assert_eq!(worker_budget.used(), 7);
            assert!(worker_budget.try_reserve(1).is_err());
            drop(reservation);
        });
        assert!(budget.used() <= budget.capacity());
        worker.join().unwrap();
        assert!(queued.lock().unwrap().is_none());
        assert_eq!(budget.used(), 0);
    });
}

#[test]
fn releasing_one_owner_cannot_release_a_replacement_reservation() {
    explore(|| {
        let budget = Arc::new(BlockAdmissionBudget::new(2));
        let original = budget.try_reserve(2).unwrap();
        let release = thread::spawn(move || drop(original));
        let retry_budget = budget.clone();
        let retry = thread::spawn(move || retry_budget.try_reserve(2).ok());
        release.join().unwrap();
        let replacement = retry
            .join()
            .unwrap()
            .unwrap_or_else(|| budget.try_reserve(2).unwrap());
        assert_eq!(budget.used(), 2);
        assert!(budget.try_reserve(1).is_err());
        drop(replacement);
        assert_eq!(budget.used(), 0);
    });
}

#[test]
fn actual_checked_add_cannot_wrap_under_competing_maximum_requests() {
    explore(|| {
        let budget = Arc::new(BlockAdmissionBudget::new(usize::MAX));
        let held = budget.try_reserve(usize::MAX - 1).unwrap();
        let workers = [1, usize::MAX].map(|bytes| {
            let budget = budget.clone();
            thread::spawn(move || {
                let reservation = budget.try_reserve(bytes);
                if bytes == usize::MAX {
                    assert!(reservation.is_err());
                }
                drop(reservation);
            })
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(budget.used(), usize::MAX - 1);
        drop(held);
        assert_eq!(budget.used(), 0);
    });
}
