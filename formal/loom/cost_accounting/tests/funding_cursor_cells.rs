use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

fn explore(test: impl Fn() + Sync + Send + 'static) {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder.check(test);
}

struct Cells {
    available: AtomicUsize,
    values: [Mutex<(usize, usize)>; 3],
}

impl Cells {
    fn new() -> Self {
        Self {
            available: AtomicUsize::new(7),
            values: std::array::from_fn(|_| Mutex::new((0, 0))),
        }
    }

    fn acquire(&self, required: usize) -> bool {
        self.available
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                (available & required == required).then_some(available & !required)
            })
            .is_ok()
    }

    fn settle(&self, required: usize, fail: bool) -> bool {
        if !self.acquire(required) {
            return false;
        }
        let captured: Vec<_> = (0..3)
            .filter(|scope| required & (1 << scope) != 0)
            .map(|scope| (scope, *self.values[scope].lock().unwrap()))
            .collect();
        let accepted = !fail && captured.iter().all(|(_, (revision, _))| *revision == 0);
        for (scope, (revision, charges)) in &captured {
            assert_eq!(revision, charges);
            if accepted {
                *self.values[*scope].lock().unwrap() = (revision + 1, charges + 1);
            }
        }
        for (scope, _) in captured {
            self.available.fetch_or(1 << scope, Ordering::Release);
        }
        accepted
    }

    fn check(&self) {
        assert_eq!(self.available.load(Ordering::Acquire), 7);
        for value in &self.values {
            let (revision, charges) = *value.lock().unwrap();
            assert_eq!(revision, charges);
            assert!(charges <= 1);
        }
    }
}

#[test]
fn overlapping_roles_restore_or_publish_each_acquired_cell() {
    for (left, right) in [(3, 3), (3, 6), (1, 6), (0, 3)] {
        for fail in [false, true] {
            explore(move || {
                let cells = Arc::new(Cells::new());
                let first = cells.clone();
                let second = cells.clone();
                let first = thread::spawn(move || first.settle(left, fail));
                let second = thread::spawn(move || second.settle(right, false));
                let a = first.join().unwrap();
                let b = second.join().unwrap();
                if left & right != 0 {
                    assert!(!(a && b));
                }
                if left & right == 0 {
                    assert!(b);
                    assert_eq!(a, !fail);
                }
                cells.check();
            });
        }
    }
}

#[test]
fn a_waiting_overlap_holds_nothing_and_disjoint_cells_remain_available() {
    explore(|| {
        let cells = Arc::new(Cells::new());
        assert!(cells.acquire(3));
        let other = cells.clone();
        thread::spawn(move || {
            assert!(!other.acquire(6));
            assert!(other.acquire(4));
            other.available.fetch_or(4, Ordering::Release);
        })
        .join()
        .unwrap();
        cells.available.fetch_or(3, Ordering::Release);
        cells.check();
    });
}
