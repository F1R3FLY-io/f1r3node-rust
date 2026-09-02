use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pathmap::PathMap;

use super::{DisposeValue, DrainObserver, GuardedPathMap, TeardownBarrier, drain_pathmap_observed};

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

#[derive(Clone)]
struct MeasurementDisposer {
    disposed: Arc<AtomicUsize>,
}

impl DisposeValue<u64> for MeasurementDisposer {
    fn dispose(&self, _value: u64) { self.disposed.fetch_add(1, Ordering::Relaxed); }
}

struct MeasurementBarrier;

impl TeardownBarrier<u64> for MeasurementBarrier {
    fn make_barrier() -> u64 { u64::MAX }
}

fn measurement_map(
    shape: &str,
    size: usize,
) -> (GuardedPathMap<u64, MeasurementDisposer, MeasurementBarrier>, usize, Arc<AtomicUsize>) {
    let disposed = Arc::new(AtomicUsize::new(0));
    let mut guarded = GuardedPathMap::new_with_barrier(MeasurementDisposer {
        disposed: Arc::clone(&disposed),
    });

    let expected_values = match shape {
        "line" => {
            guarded.create_path(vec![0xa5; size]);
            0
        }
        "comb" => {
            guarded.with_write_cursor(|mut cursor| {
                let mut spine = Vec::with_capacity(size + 1);
                for ordinal in 0..size {
                    let ordinal = u64::try_from(ordinal).expect("measurement ordinal fits u64");
                    spine.push(1);
                    cursor.move_to_path(&spine);
                    cursor.set_val(ordinal);
                    spine.pop();
                    spine.push(0);
                }
                cursor.move_to_path(&spine);
                cursor.set_val(u64::try_from(size).expect("measurement size fits u64"));
            });
            size + 1
        }
        "dense" => {
            guarded.with_write_cursor(|mut cursor| {
                for ordinal in 0..size {
                    let ordinal = u64::try_from(ordinal).expect("measurement ordinal fits u64");
                    cursor.move_to_path(&ordinal.to_be_bytes());
                    cursor.set_val(ordinal);
                }
            });
            size
        }
        "realistic" => {
            const KEY_BYTES: usize = 939;
            const COMMON_PREFIX_BYTES: usize = 203;
            guarded.with_write_cursor(|mut cursor| {
                for ordinal in 0..size {
                    let ordinal = u64::try_from(ordinal).expect("measurement ordinal fits u64");
                    let mut path = Vec::with_capacity(KEY_BYTES);
                    path.resize(COMMON_PREFIX_BYTES, 0x52);
                    path.extend_from_slice(&ordinal.to_be_bytes());
                    let mut state = ordinal.wrapping_add(0x9e37_79b9_7f4a_7c15);
                    while path.len() < KEY_BYTES {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        path.push(state as u8);
                    }
                    cursor.move_to_path(&path);
                    cursor.set_val(ordinal);
                }
            });
            size
        }
        other => panic!("unknown RSPACE_TEARDOWN_SHAPE {other:?}"),
    };

    assert_eq!(guarded.iter().count(), expected_values);
    (guarded, expected_values, disposed)
}

#[test]
#[ignore = "measurement-only: run exact under a capped systemd unit"]
fn final_anchor_teardown_measurement() {
    let shape = env::var("RSPACE_TEARDOWN_SHAPE").unwrap_or_else(|_| "line".to_owned());
    let size = env::var("RSPACE_TEARDOWN_SIZE")
        .map(|raw| {
            raw.parse::<usize>()
                .expect("RSPACE_TEARDOWN_SIZE must be an unsigned integer")
        })
        .unwrap_or(4_096);
    let (guarded, expected_values, disposed) = measurement_map(&shape, size);

    let started = Instant::now();
    drop(black_box(guarded));
    let elapsed_ns = started.elapsed().as_nanos();

    assert_eq!(disposed.load(Ordering::Relaxed), expected_values);
    println!(
        "TEARDOWN_OBS\tshape={}\tsize={}\tvalues={}\telapsed_ns={}",
        shape, size, expected_values, elapsed_ns
    );
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
