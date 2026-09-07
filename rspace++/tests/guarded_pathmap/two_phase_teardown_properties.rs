use std::env;
use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pathmap::PathMap;
use proptest::prelude::*;

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
    let trials = env::var("RSPACE_TEARDOWN_TRIALS")
        .map(|raw| {
            raw.parse::<usize>()
                .expect("RSPACE_TEARDOWN_TRIALS must be an unsigned integer")
        })
        .unwrap_or(1);
    assert_ne!(trials, 0, "RSPACE_TEARDOWN_TRIALS must be nonzero");

    for trial in 0..trials {
        let (guarded, expected_values, disposed) = measurement_map(&shape, size);
        let started = Instant::now();
        drop(black_box(guarded));
        let elapsed_ns = started.elapsed().as_nanos();

        assert_eq!(disposed.load(Ordering::Relaxed), expected_values);
        println!(
            "TEARDOWN_OBS\tshape={}\tsize={}\ttrial={}\tvalues={}\telapsed_ns={}",
            shape, size, trial, expected_values, elapsed_ns
        );
    }
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

type LiveMap = GuardedPathMap<u64, MeasurementDisposer, MeasurementBarrier>;
type LiveCursor<'a, 'p> =
    super::GuardedWriteCursor<'a, 'p, u64, MeasurementDisposer, MeasurementBarrier>;

fn live_map() -> LiveMap {
    GuardedPathMap::new_with_barrier(MeasurementDisposer {
        disposed: Arc::new(AtomicUsize::new(0)),
    })
}

fn check_live_gateway(path: &[u8]) {
    for prefix_gateway in [false, true] {
        for unwind in [false, true] {
            for shared in [false, true] {
                let mut map = live_map();
                map.set_val_at(path, 17);
                let original = map.compact_u64_bytes(|value| *value);
                let snapshot = shared.then(|| map.clone());
                let anchor = map.anchor_identity();
                let outcome = Arc::new(73_u64);
                let remove = |mut cursor: LiveCursor<'_, '_>| {
                    cursor.move_to_path(path);
                    assert!(cursor.remove_val_and_prune());
                    if unwind {
                        panic_any(Arc::clone(&outcome));
                    }
                    Arc::clone(&outcome)
                };
                let result = catch_unwind(AssertUnwindSafe(|| {
                    if prefix_gateway {
                        map.with_write_cursor_at(&[], remove)
                    } else {
                        map.with_write_cursor(remove)
                    }
                }));
                let returned = if unwind {
                    *result.unwrap_err().downcast::<Arc<u64>>().unwrap()
                } else {
                    result.unwrap()
                };
                assert!(Arc::ptr_eq(&returned, &outcome));
                assert!(map.is_empty());
                assert_eq!(map.anchor.value_count, 0);
                assert_eq!(map.iter().count(), 0);
                assert_eq!(map.compact_u64_bytes(|v| *v), live_map().compact_u64_bytes(|v| *v));
                let normalized = map.compact_u64_bytes(|v| *v);
                map.with_write_cursor(|_| ());
                map.with_write_cursor_at(&[], |_| ());
                assert_eq!(map.compact_u64_bytes(|v| *v), normalized);
                if !shared {
                    assert_eq!(map.anchor_identity(), anchor);
                }
                map.set_val_at([0x22, 0x44], 99);
                assert_eq!(map.get([0x22, 0x44]), Some(&99));
                assert_eq!(map.anchor.value_count, 1);
                if let Some(snapshot) = snapshot {
                    assert_eq!(snapshot.compact_u64_bytes(|v| *v), original);
                    assert_eq!(snapshot.get(path), Some(&17));
                    drop(map);
                    assert_eq!(snapshot.compact_u64_bytes(|v| *v), original);
                }
            }
        }
    }
}

#[test]
fn live_write_gateways_preserve_normal_and_unwind_outcomes() {
    for path in [Vec::new(), vec![0x11], vec![0xa5; 256]] {
        check_live_gateway(&path);
    }
}

#[test]
fn live_write_gateways_preserve_nonempty_topology_and_siblings() {
    for prefix_gateway in [false, true] {
        let mut map = live_map();
        map.create_path([1, 2, 3]);
        let before = map.compact_u64_bytes(|v| *v);
        let anchor = map.anchor_identity();
        assert_eq!(map.anchor.value_count, 0);
        if prefix_gateway {
            map.with_write_cursor_at(&[1], |_| ());
        } else {
            map.with_write_cursor(|_| ());
        }
        assert_eq!(map.anchor_identity(), anchor);
        assert!(map.path_exists([1, 2, 3]));
        assert_eq!(map.compact_u64_bytes(|v| *v), before);
    }
    for (unwind, with_sibling) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut map = live_map();
        map.set_val_at([1, 2], 17);
        if with_sibling {
            map.set_val_at([3, 4], 55);
        }
        let outcome = Arc::new(73_u64);
        let result = catch_unwind(AssertUnwindSafe(|| {
            map.with_write_cursor_at(&[1], |mut cursor| {
                cursor.move_to_path(&[2]);
                assert!(cursor.remove_val_and_prune());
                if unwind {
                    panic_any(Arc::clone(&outcome));
                }
                Arc::clone(&outcome)
            })
        }));
        let returned = if unwind {
            *result.unwrap_err().downcast::<Arc<u64>>().unwrap()
        } else {
            result.unwrap()
        };
        assert!(Arc::ptr_eq(&returned, &outcome));
        assert!(map.get([1, 2]).is_none());
        assert_eq!(map.get([3, 4]), with_sibling.then_some(&55));
        assert_eq!(map.anchor.value_count, usize::from(with_sibling));
        if map.is_empty() {
            assert_eq!(map.compact_u64_bytes(|v| *v), live_map().compact_u64_bytes(|v| *v));
        } else {
            assert!(map.path_exists([1]) || with_sibling);
        }
        map.set_val_at([1, 9], 42);
        assert_eq!(map.get([1, 9]), Some(&42));
        assert_eq!(map.get([3, 4]), with_sibling.then_some(&55));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn live_write_generated_gateway_observations(path in prop::collection::vec(any::<u8>(), 0..257)) {
        check_live_gateway(&path);
    }
}
