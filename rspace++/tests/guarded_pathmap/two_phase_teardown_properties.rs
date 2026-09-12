use std::env;
use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pathmap::PathMap;
use pathmap::zipper::{
    TrieRefBorrowed, Zipper, ZipperAbsolutePath, ZipperForking, ZipperIteration, ZipperMoving,
    ZipperReadOnlySubtries, ZipperValues, ZipperWriting,
};
use proptest::prelude::*;

use super::{DisposeValue, DrainObserver, GuardedPathMap, TeardownBarrier, drain_pathmap_observed};

fn navigation_topology(map: &PathMap<usize>) -> Vec<(usize, Option<u8>, Option<usize>)> {
    let mut cursor = map.read_zipper();
    let mut nodes = Vec::new();
    loop {
        nodes.push((cursor.path().len(), cursor.path().last().copied(), cursor.val().copied()));
        if cursor.descend_first_byte() {
            continue;
        }
        loop {
            if cursor.at_root() {
                return nodes;
            }
            if cursor.to_next_sibling_byte() {
                break;
            }
            assert!(cursor.ascend_byte());
        }
    }
}

fn scalar_greatest_route(map: &PathMap<usize>, focus: &[u8]) -> (Vec<u8>, Option<usize>) {
    let mut cursor = map.read_zipper();
    cursor.move_to_path(focus);
    assert!(focus.is_empty() || cursor.path_exists());
    while cursor.descend_last_byte() {}
    (cursor.path()[focus.len()..].to_vec(), cursor.val().copied())
}

fn assert_navigation_at(map: &mut PathMap<usize>, focus: &[u8]) {
    let topology = navigation_topology(map);
    let snapshot = map.clone();
    let (expected, value) = scalar_greatest_route(&snapshot, focus);
    for rooted_at_focus in [false, true] {
        {
            let mut cursor = if rooted_at_focus {
                map.write_zipper_at_path(focus)
            } else {
                let mut cursor = map.write_zipper();
                cursor.move_to_path(focus);
                cursor
            };
            {
                let borrowed = cursor.fork_read_zipper();
                assert!(borrowed.path().is_empty());
                assert_eq!(borrowed.origin_path(), focus);
                if borrowed.child_count() != 0 {
                    let subtree: TrieRefBorrowed<'_, usize> = borrowed.trie_ref_at_path([]);
                    let mut rebased = subtree.fork_read_zipper();
                    const PATHMAP_0_2_2_MAX_NODE_KEY_BYTES: usize = 48;
                    assert!(rebased.path().is_empty());
                    assert_eq!(rebased.path_exists(), borrowed.path_exists());
                    assert_eq!(rebased.origin_path(), rebased.root_prefix_path());
                    assert!(focus.ends_with(rebased.root_prefix_path()));
                    assert!(rebased.origin_path().len() <= PATHMAP_0_2_2_MAX_NODE_KEY_BYTES);
                    assert_eq!(rebased.child_mask(), borrowed.child_mask());
                    assert_eq!(rebased.child_count(), borrowed.child_count());
                    assert_eq!(
                        rebased.val().map(std::ptr::from_ref),
                        borrowed.val().map(std::ptr::from_ref)
                    );
                    if focus.len() > PATHMAP_0_2_2_MAX_NODE_KEY_BYTES {
                        assert!(rebased.origin_path().len() < borrowed.origin_path().len());
                    }
                    let root_prefix = rebased.root_prefix_path().to_vec();
                    assert!(rebased.descend_last_path());
                    assert_eq!(rebased.root_prefix_path(), root_prefix);
                    assert_eq!(rebased.path(), expected);
                    assert_eq!(rebased.val().copied(), value);
                    assert_eq!(rebased.child_count(), 0);
                }
            }
            let before = cursor.path().to_vec();
            let mut suffix = vec![81, 82, 83];
            assert_eq!(
                super::descend_to_rightmost_leaf(&mut cursor, &mut suffix),
                !expected.is_empty()
            );
            assert_eq!(suffix, expected);
            assert_eq!(cursor.path(), [before.as_slice(), expected.as_slice()].concat());
            assert_eq!(cursor.child_count(), 0);
            assert_eq!(cursor.val().copied(), value);
            let leaf = cursor.path().to_vec();
            suffix.extend_from_slice(&[91, 92]);
            assert!(!super::descend_to_rightmost_leaf(&mut cursor, &mut suffix));
            assert!(suffix.is_empty());
            assert_eq!(cursor.path(), leaf);
            cursor.reset();
        }
        assert_eq!(navigation_topology(map), topology);
        assert_eq!(navigation_topology(&snapshot), topology);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum NavigationDrainEvent {
    Removed,
    Installed,
    Pruned(usize),
    Retired,
}

#[derive(Default)]
struct NavigationObserver(Vec<NavigationDrainEvent>);

impl DrainObserver for NavigationObserver {
    fn removed_value(&mut self) { self.0.push(NavigationDrainEvent::Removed); }
    fn installed_barrier(&mut self) { self.0.push(NavigationDrainEvent::Installed); }
    fn pruned(&mut self, width: usize) { self.0.push(NavigationDrainEvent::Pruned(width)); }
    fn removed_barrier(&mut self) { self.0.push(NavigationDrainEvent::Retired); }
}

fn navigation_reference_drain(
    map: &mut PathMap<usize>,
    compressed: bool,
) -> (Vec<usize>, Vec<(Vec<u8>, Vec<u8>, usize, usize, usize)>, NavigationObserver) {
    let mut removed = Vec::new();
    let mut observer = NavigationObserver::default();
    {
        let mut cursor = map.write_zipper();
        loop {
            if cursor.is_val() {
                removed.push(cursor.remove_val(false).unwrap());
                observer.removed_value();
            }
            if !super::advance_to_next_value(&mut cursor) {
                break;
            }
        }
        cursor.reset();
    }
    let state = Arc::new(RetentionState::default());
    let disposer = OrderedDisposer {
        state: Arc::clone(&state),
        order: Arc::new(Mutex::new(Vec::new())),
    };
    let mut trace = Vec::new();
    let mut installed = 0;
    let mut pruned = 0;
    let mut retired = 0;
    if !map.is_empty() {
        let mut cursor = map.write_zipper();
        let mut suffix = Vec::new();
        loop {
            if compressed {
                super::descend_to_rightmost_leaf(&mut cursor, &mut suffix);
            } else {
                while cursor.descend_last_byte() {}
            }
            if cursor.at_root() && !cursor.is_val() && cursor.child_count() == 0 {
                break;
            }
            let leaf = cursor.path().to_vec();
            assert!(cursor.set_val(usize::MAX).is_none());
            let step = super::remove_valued_leaf_and_bubble::<_, UsizeBarrier, _>(
                &mut cursor,
                super::ValuedLeafOrigin::Synthetic,
                None,
                &disposer,
            );
            assert_eq!(step.real_values, 0);
            installed += step.installed_barriers + 1;
            pruned += step.pruned_edges;
            retired += step.retired_barriers;
            trace.push((
                leaf,
                cursor.path().to_vec(),
                step.installed_barriers + 1,
                step.pruned_edges,
                step.retired_barriers,
            ));
        }
        cursor.reset();
    }
    assert_eq!(state.disposed.load(Ordering::SeqCst), 0);
    for _ in 0..installed {
        observer.installed_barrier();
    }
    for _ in 0..pruned {
        observer.pruned(1);
    }
    for _ in 0..retired {
        observer.removed_barrier();
    }
    assert!(map.is_empty());
    (removed, trace, observer)
}

fn assert_navigation_drain(map: &PathMap<usize>) {
    let topology = navigation_topology(map);
    let mut scalar = map.clone();
    let mut compressed = map.clone();
    let mut production = map.clone();
    let reference = navigation_reference_drain(&mut scalar, false);
    let actual = navigation_reference_drain(&mut compressed, true);
    assert_eq!(reference.0, actual.0);
    assert_eq!(reference.1, actual.1);
    assert_eq!(reference.2.0, actual.2.0);
    let order = Arc::new(Mutex::new(Vec::new()));
    let disposer = OrderedDisposer {
        state: Arc::new(RetentionState::default()),
        order: Arc::clone(&order),
    };
    let mut observer = NavigationObserver::default();
    assert_eq!(
        drain_pathmap_observed::<_, UsizeBarrier, _, _>(&mut production, &disposer, &mut observer),
        reference.0.len()
    );
    assert_eq!(*order.lock().unwrap(), reference.0);
    assert_eq!(observer.0, reference.2.0);
    assert!(production.is_empty());
    assert_eq!(navigation_topology(map), topology);
}

#[test]
fn compressed_navigation_preserves_empty_root_and_long_dangling_line() {
    let mut empty = PathMap::new();
    assert_navigation_at(&mut empty, &[]);
    assert_navigation_drain(&empty);
    empty.set_val_at([], 17);
    assert_navigation_at(&mut empty, &[]);
    assert_navigation_drain(&empty);
    let path = vec![255; 8192];
    let mut line = PathMap::new();
    {
        let mut cursor = line.write_zipper();
        cursor.move_to_path(&path);
        cursor.create_path();
        cursor.reset();
    }
    {
        let cursor = line.write_zipper();
        let mut borrowed = cursor.fork_read_zipper();
        assert!(borrowed.descend_first_byte());
        let start = borrowed.path().len();
        borrowed.descend_until();
        assert!(borrowed.path().len() - start > 1);
        assert_eq!(borrowed.path(), path);
        assert_eq!(borrowed.child_count(), 0);
    }
    for width in [0, 1, 4096, 8192] {
        assert_navigation_at(&mut line, &path[..width]);
    }
    assert_navigation_drain(&line);
}

#[test]
fn compressed_navigation_preserves_dense_comb_and_asymmetric_siblings() {
    let mut map = PathMap::new();
    for byte in 0..=255u8 {
        map.set_val_at([byte], usize::from(byte));
    }
    for depth in 1..33 {
        let mut path = vec![0; depth];
        path.push(255);
        map.set_val_at(&path, 300 + depth);
    }
    let mut greatest = vec![255; 37];
    greatest.push(0);
    map.set_val_at(&greatest, 900);
    for focus in [vec![], vec![0], vec![0; 16], vec![255], vec![255; 19], greatest.clone()] {
        assert_navigation_at(&mut map, &focus);
    }
    assert_eq!(scalar_greatest_route(&map, &[]).0, greatest);
    assert_navigation_drain(&map);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn compressed_navigation_matches_scalar_generated_topology_histories(
        history in prop::collection::vec((prop::collection::vec(any::<u8>(), 0..17), 0u8..3), 0..17),
    ) {
        let mut map = PathMap::new();
        for (index, (path, operation)) in history.iter().enumerate() {
            let mut cursor = map.write_zipper();
            cursor.move_to_path(path);
            match operation {
                0 => { cursor.set_val(index); }
                1 => { cursor.create_path(); }
                _ => { cursor.remove_val(false); }
            }
            cursor.reset();
        }
        assert_navigation_at(&mut map, &[]);
        let mut focuses = std::collections::BTreeSet::new();
        for (path, _) in &history {
            for width in 1..=path.len() { focuses.insert(path[..width].to_vec()); }
        }
        for focus in focuses {
            let exists = {
                let mut cursor = map.read_zipper();
                cursor.move_to_path(&focus);
                cursor.path_exists()
            };
            if exists { assert_navigation_at(&mut map, &focus); }
        }
        assert_navigation_drain(&map);
    }
}

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
