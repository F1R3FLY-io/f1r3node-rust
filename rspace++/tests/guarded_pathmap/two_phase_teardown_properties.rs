use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pathmap::PathMap;

use super::{DisposeValue, DrainObserver, TeardownBarrier, drain_pathmap_observed};

#[derive(Default)]
struct RetentionState {
    removed: AtomicUsize,
    disposed: AtomicUsize,
    retained_peak: AtomicUsize,
}

#[derive(Clone)]
struct OrderedDisposer {
    state: Arc<RetentionState>,
    order: Arc<Mutex<Vec<usize>>>,
}

impl DisposeValue<usize> for OrderedDisposer {
    fn dispose(&self, value: usize) {
        self.order.lock().unwrap().push(value);
        self.state.disposed.fetch_add(1, Ordering::SeqCst);
    }
}

struct RetentionObserver {
    state: Arc<RetentionState>,
}

impl DrainObserver for RetentionObserver {
    fn removed_value(&mut self) {
        let removed = self.state.removed.fetch_add(1, Ordering::SeqCst) + 1;
        let disposed = self.state.disposed.load(Ordering::SeqCst);
        self.state
            .retained_peak
            .fetch_max(removed - disposed, Ordering::SeqCst);
    }
}

struct UsizeBarrier;

impl TeardownBarrier<usize> for UsizeBarrier {
    fn make_barrier() -> usize { usize::MAX }
}

#[test]
fn phase_a_disposes_in_byte_preorder_with_constant_retained_payloads() {
    const CHILDREN: usize = 32;
    let state = Arc::new(RetentionState::default());
    let order = Arc::new(Mutex::new(Vec::new()));
    let disposer = OrderedDisposer {
        state: Arc::clone(&state),
        order: Arc::clone(&order),
    };
    let mut observer = RetentionObserver {
        state: Arc::clone(&state),
    };
    let mut map = PathMap::new();
    map.set_val_at([], 0);
    for byte in 0..CHILDREN as u8 {
        map.set_val_at([byte], usize::from(byte) + 1);
    }

    drain_pathmap_observed::<_, UsizeBarrier, _, _>(&mut map, &disposer, &mut observer);

    let expected = (0..=CHILDREN).collect::<Vec<_>>();
    assert_eq!(*order.lock().unwrap(), expected, "Phase A is byte-preorder, including the root");
    assert_eq!(state.removed.load(Ordering::SeqCst), CHILDREN + 1);
    assert_eq!(state.disposed.load(Ordering::SeqCst), CHILDREN + 1);
    assert!(
        state.retained_peak.load(Ordering::SeqCst) <= 1,
        "each detached payload must be disposed before the next traversal step"
    );
    assert!(map.is_empty(), "Phase B must consume all value-free topology");
}
