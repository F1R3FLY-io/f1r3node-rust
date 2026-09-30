use std::cell::Cell;

use loom::sync::Arc;
use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::thread;
use shared::rust::clone_backing::{self, BackingError, BackingMeter};

struct Meter {
    remaining: AtomicUsize,
    accepted: AtomicUsize,
    atomic: bool,
}

impl BackingMeter for Meter {
    fn reserve(&self, _: usize, _: usize, _: usize) -> Result<(), BackingError> {
        if self.atomic {
            self.remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                .map_err(|_| BackingError::Rejected)?;
        } else {
            let before = self.remaining.load(Ordering::Acquire);
            let next = before.checked_sub(1).ok_or(BackingError::Rejected)?;
            thread::yield_now();
            self.remaining.store(next, Ordering::Release);
        }
        self.accepted.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

fn check(limit: usize, charges: usize, atomic: bool) {
    let meter = Arc::new(Meter {
        remaining: AtomicUsize::new(limit),
        accepted: AtomicUsize::new(0),
        atomic,
    });
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let meter = meter.clone();
            thread::spawn(move || {
                let original = vec!["retained".to_owned(), "payload".to_owned()];
                match clone_backing::reserve(&original, meter.as_ref()) {
                    Ok(()) => {
                        let copied = original.clone();
                        assert_eq!(copied, original);
                        1
                    }
                    Err(BackingError::Rejected) => 0,
                    Err(error) => panic!("unexpected traversal failure: {error:?}"),
                }
            })
        })
        .collect();
    let completed: usize = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .sum();
    assert!(meter.accepted.load(Ordering::Acquire) <= limit);
    assert!(completed * charges <= limit);
    if limit == charges * 2 {
        assert_eq!(completed, 2);
    }
}

#[test]
fn shared_clone_reservations_precede_owned_copies_under_concurrent_exhaustion() {
    let count = Cell::new(0);
    let original = vec!["retained".to_owned(), "payload".to_owned()];
    clone_backing::reserve(&original, &|_, _, _| {
        count.set(count.get() + 1);
        Ok(())
    })
    .unwrap();
    let charges = count.get();
    for limit in [0, 1, charges - 1, charges, charges * 2 - 1, charges * 2] {
        let mut model = loom::model::Builder::new();
        model.preemption_bound = Some(2);
        model.check(move || check(limit, charges, true));
    }
}

#[test]
#[should_panic]
fn separate_credit_checks_allow_unpaid_traversal_preparation() {
    loom::model(move || check(1, 1, false));
}
