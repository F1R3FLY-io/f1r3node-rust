use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug)]
struct Event {
    id: usize,
    weight: usize,
}

struct Budget {
    limit: usize,
    max_slots: usize,
    consumed: AtomicUsize,
    slots: AtomicUsize,
    success_log: Mutex<Vec<usize>>,
    oop: Mutex<Option<usize>>,
}

impl Budget {
    fn reserve_slot(&self) -> bool {
        let mut current = self.slots.load(Ordering::Acquire);
        loop {
            if current >= self.max_slots {
                return false;
            }
            match self.slots.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(next) => current = next,
            }
        }
    }

    fn release_slot(&self) {
        let previous = self.slots.fetch_sub(1, Ordering::AcqRel);
        assert!(previous > 0);
    }

    fn reserve(&self, event: Event) -> Result<(), ()> {
        if event.weight == 0 || !self.reserve_slot() {
            return Err(());
        }

        loop {
            let consumed = self.consumed.load(Ordering::Acquire);
            let next = consumed.saturating_add(event.weight);
            if next > self.limit {
                self.consumed.store(self.limit, Ordering::Release);
                let mut oop = self.oop.lock().unwrap();
                if oop.is_none() {
                    *oop = Some(event.id);
                } else {
                    self.release_slot();
                }
                return Err(());
            }

            if self
                .consumed
                .compare_exchange(consumed, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.success_log.lock().unwrap().push(event.id);
                return Ok(());
            }
        }
    }

    fn event_count(&self) -> usize {
        self.success_log.lock().unwrap().len() + usize::from(self.oop.lock().unwrap().is_some())
    }

    fn finalized_event_count(&self) -> usize {
        self.event_count()
    }
}

#[test]
fn trace_slots_stay_bounded_under_repeated_oop_race() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            limit: 1,
            max_slots: 1,
            consumed: AtomicUsize::new(0),
            slots: AtomicUsize::new(0),
            success_log: Mutex::new(Vec::new()),
            oop: Mutex::new(None),
        });

        let a = {
            let budget = budget.clone();
            thread::spawn(move || {
                let _ = budget.reserve(Event { id: 1, weight: 2 });
            })
        };
        let b = {
            let budget = budget.clone();
            thread::spawn(move || {
                let _ = budget.reserve(Event { id: 2, weight: 2 });
            })
        };

        a.join().unwrap();
        b.join().unwrap();

        assert!(budget.slots.load(Ordering::Acquire) <= 1);
        assert!(budget.event_count() <= 1);
    });
}

#[test]
fn invalid_admission_does_not_reserve_trace_slot() {
    loom::model(|| {
        let budget = Budget {
            limit: 10,
            max_slots: 1,
            consumed: AtomicUsize::new(0),
            slots: AtomicUsize::new(0),
            success_log: Mutex::new(Vec::new()),
            oop: Mutex::new(None),
        };

        assert!(budget.reserve(Event { id: 1, weight: 0 }).is_err());
        assert_eq!(budget.consumed.load(Ordering::Acquire), 0);
        assert_eq!(budget.slots.load(Ordering::Acquire), 0);
        assert_eq!(budget.event_count(), 0);
    });
}

#[test]
fn finalization_after_workers_observes_complete_trace_count() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            limit: 4,
            max_slots: 4,
            consumed: AtomicUsize::new(0),
            slots: AtomicUsize::new(0),
            success_log: Mutex::new(Vec::new()),
            oop: Mutex::new(None),
        });

        let a = {
            let budget = budget.clone();
            thread::spawn(move || {
                budget.reserve(Event { id: 1, weight: 1 }).unwrap();
            })
        };
        let b = {
            let budget = budget.clone();
            thread::spawn(move || {
                budget.reserve(Event { id: 2, weight: 1 }).unwrap();
            })
        };

        a.join().unwrap();
        b.join().unwrap();

        assert_eq!(budget.consumed.load(Ordering::Acquire), 2);
        assert_eq!(budget.finalized_event_count(), 2);
    });
}
