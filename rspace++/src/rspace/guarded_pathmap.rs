//! Ownership guard for stack-safe destruction of [`PathMap`] values.
//!
//! A cloned guard shares one immutable `PathMap` header through an `Arc`.
//! Mutation is the only operation that creates another header, through
//! `Arc::make_mut`.  The final anchor owner drains both payloads and trie
//! topology with a borrowed zipper before the now-empty map is dropped.

use std::marker::PhantomData;
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperWriting};

/// Supplies a temporary value that stops `prune_ascend` at the immediate
/// parent of the edge being removed.
///
/// The policy has no runtime representation: only its associated functions
/// are used.  `make_barrier` must return a value whose destruction by
/// `discard_barrier` is bounded and non-panicking.  A barrier is never passed
/// to the original-value disposal sink.
pub trait TeardownBarrier<V> {
    fn make_barrier() -> V;

    fn discard_barrier(barrier: V) { drop(barrier); }
}

/// Uses `V::default()` as the temporary one-edge pruning barrier.
///
/// This is appropriate only when the default value has bounded destruction.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultValueBarrier;

impl<V: Default> TeardownBarrier<V> for DefaultValueBarrier {
    #[inline]
    fn make_barrier() -> V { V::default() }
}

/// A copy-on-write `PathMap` version whose final owner tears it down without
/// recursive traversal.
///
/// `D` receives every real value removed by mutation or final teardown.  It
/// must dispose of that value stack-safely and must not panic.  The guard does
/// not expose an owned map, owned zipper, or owned iterator.
///
/// Draining one of several copy-on-write versions necessarily makes shared
/// PathMap nodes unique along the destructive zipper route.  Consumers must
/// benchmark clone-then-drop workloads for their concrete value/topology mix;
/// the test observer below keeps prune width and payload counts measurable
/// without adding counters to production teardown.
pub struct GuardedPathMap<V, D, B = DefaultValueBarrier>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    anchor: Arc<AnchorRoot<V, D, B>>,
}

struct AnchorRoot<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    // `Option` is the move-out slot used by Drop.  At every other point an
    // anchor contains exactly one raw PathMap header.
    map: Option<PathMap<V>>,
    dispose: D,
    // Policies are types rather than stored values, so barriers cost no bytes
    // in either the guard or the anchor.
    barrier: PhantomData<fn() -> B>,
}

impl<V, D, B> Clone for AnchorRoot<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    fn clone(&self) -> Self {
        // Evaluation order is an ownership invariant.  A user-supplied
        // disposer may have a panicking Clone implementation.  Clone it before
        // the raw PathMap header so such a panic leaves no temporary nonempty
        // map to fall through ordinary recursive Drop.
        let dispose = self.dispose.clone();
        let map = self.map().clone();
        Self {
            // This clone is reachable only from the guard's mutation path via
            // Arc::make_mut.  Cloning a guard itself never clones this header.
            map: Some(map),
            dispose,
            barrier: PhantomData,
        }
    }
}

impl<V, D, B> AnchorRoot<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    #[inline]
    fn map(&self) -> &PathMap<V> {
        self.map
            .as_ref()
            .expect("a live PathMap anchor always contains its map")
    }

    #[inline]
    fn map_mut(&mut self) -> &mut PathMap<V> {
        self.map
            .as_mut()
            .expect("a live PathMap anchor always contains its map")
    }
}

impl<V, D, B> Drop for AnchorRoot<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    fn drop(&mut self) {
        let Some(mut map) = self.map.take() else {
            return;
        };
        drain_pathmap::<V, B, _>(&mut map, &self.dispose);
        // The ordinary PathMap destructor now sees no values and no edges.
        drop(map);
    }
}

impl<V, D> GuardedPathMap<V, D, DefaultValueBarrier>
where
    V: Clone + Default + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
{
    pub fn new(dispose: D) -> Self { Self::new_with_barrier(dispose) }
}

impl<V, D, B> GuardedPathMap<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    pub fn new_with_barrier(dispose: D) -> Self { Self::from_map(PathMap::new(), dispose) }

    fn from_map(map: PathMap<V>, dispose: D) -> Self {
        Self {
            anchor: Arc::new(AnchorRoot {
                map: Some(map),
                dispose,
                barrier: PhantomData,
            }),
        }
    }

    #[inline]
    fn map(&self) -> &PathMap<V> { self.anchor.map() }

    /// The sole route to a mutable raw header.  Shared immutable versions fork
    /// here, and only here.
    #[inline]
    fn fresh_map_mut(&mut self) -> &mut AnchorRoot<V, D, B> { Arc::make_mut(&mut self.anchor) }

    #[inline]
    pub fn is_empty(&self) -> bool { self.map().is_empty() }

    #[inline]
    pub fn contains<K: AsRef<[u8]>>(&self, path: K) -> bool { self.map().contains(path) }

    #[inline]
    pub fn path_exists<K: AsRef<[u8]>>(&self, path: K) -> bool { self.map().path_exists_at(path) }

    #[inline]
    pub fn get<K: AsRef<[u8]>>(&self, path: K) -> Option<&V> { self.map().get_val_at(path) }

    /// Sets a value and sends any replaced value through the configured
    /// stack-safe disposal path.  Returns whether a value was replaced.
    pub fn set_val_at<K: AsRef<[u8]>>(&mut self, path: K, value: V) -> bool {
        let anchor = self.fresh_map_mut();
        match anchor.map_mut().set_val_at(path, value) {
            Some(replaced) => {
                (anchor.dispose)(replaced);
                true
            }
            None => false,
        }
    }

    /// Removes a value without allowing PathMap to recursively prune a
    /// detached branch.  The value is sent through the disposal sink; topology
    /// is retained for final one-edge teardown.
    pub fn remove_val_at<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        let anchor = self.fresh_map_mut();
        match anchor.map_mut().remove_val_at(path, false) {
            Some(removed) => {
                (anchor.dispose)(removed);
                true
            }
            None => false,
        }
    }

    /// Creates a value-free path.  Such topology is intentionally preserved
    /// and participates in final iterative teardown.
    pub fn create_path<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        self.fresh_map_mut().map_mut().create_path(path)
    }
}

impl<V, D, B> Clone for GuardedPathMap<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: Fn(V) + Clone + Send + Sync + 'static,
    B: TeardownBarrier<V> + 'static,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            anchor: Arc::clone(&self.anchor),
        }
    }
}

trait DrainObserver {
    #[inline]
    fn removed_value(&mut self) {}

    #[inline]
    fn pruned(&mut self, _width: usize) {}
}

struct NoopDrainObserver;

impl DrainObserver for NoopDrainObserver {}

/// Empties `map` in reverse byte/postorder.  Each `prune_ascend` is bounded by
/// a real or synthetic value at the immediate parent, so a detached physical
/// subtrie can never span multiple logical edges.
fn drain_pathmap<V, B, D>(map: &mut PathMap<V>, dispose: D)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: Fn(V),
{
    let mut observer = NoopDrainObserver;
    drain_pathmap_observed::<V, B, D, _>(map, dispose, &mut observer);
}

fn drain_pathmap_observed<V, B, D, O>(map: &mut PathMap<V>, dispose: D, observer: &mut O)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: Fn(V),
    O: DrainObserver,
{
    if map.is_empty() {
        return;
    }

    {
        let mut zipper = map.write_zipper();
        loop {
            while zipper.descend_last_byte() {}

            if let Some(value) = zipper.remove_val(false) {
                observer.removed_value();
                dispose(value);
            }

            if zipper.at_root() {
                break;
            }

            let child_byte = *zipper
                .path()
                .last()
                .expect("a non-root zipper focus has a final path byte");
            let ascended = zipper.ascend_byte();
            debug_assert!(ascended);

            let installed_barrier = !zipper.is_val();
            if installed_barrier {
                let replaced = zipper.set_val(B::make_barrier());
                debug_assert!(replaced.is_none());
            }

            let descended = zipper.descend_to_existing_byte(child_byte);
            debug_assert!(descended);
            let width = zipper.prune_ascend();
            observer.pruned(width);
            debug_assert_eq!(width, 1, "the parent barrier must limit pruning to one byte");

            if installed_barrier {
                let barrier = zipper
                    .remove_val(false)
                    .expect("the temporary teardown barrier must remain at its parent");
                B::discard_barrier(barrier);
            }
        }
    }

    debug_assert!(map.is_empty());
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    use pathmap::PathMap;
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::{DrainObserver, GuardedPathMap, TeardownBarrier, drain_pathmap_observed};

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct DrainStats {
        values: usize,
        edges: usize,
        widest_prune: usize,
    }

    impl DrainObserver for DrainStats {
        fn removed_value(&mut self) { self.values += 1; }

        fn pruned(&mut self, width: usize) {
            self.edges += width;
            self.widest_prune = self.widest_prune.max(width);
        }
    }

    fn drain_with_stats<V, B, D>(map: &mut PathMap<V>, dispose: D) -> DrainStats
    where
        V: Clone + Send + Sync + Unpin + 'static,
        B: TeardownBarrier<V>,
        D: Fn(V),
    {
        let mut stats = DrainStats::default();
        drain_pathmap_observed::<V, B, D, _>(map, dispose, &mut stats);
        stats
    }

    #[derive(Debug)]
    struct Probe {
        id: usize,
        tracking: Option<Arc<ProbeTracking>>,
    }

    #[derive(Debug, Default)]
    struct ProbeTracking {
        clones: AtomicUsize,
        drops: AtomicUsize,
    }

    impl Probe {
        fn tracked(id: usize, tracking: &Arc<ProbeTracking>) -> Self {
            Self {
                id,
                tracking: Some(Arc::clone(tracking)),
            }
        }

        fn barrier() -> Self {
            Self {
                id: usize::MAX,
                tracking: None,
            }
        }
    }

    impl Clone for Probe {
        fn clone(&self) -> Self {
            if let Some(tracking) = &self.tracking {
                tracking.clones.fetch_add(1, Ordering::Relaxed);
            }
            Self {
                id: self.id,
                tracking: self.tracking.clone(),
            }
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            if let Some(tracking) = &self.tracking {
                tracking.drops.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    struct ProbeBarrier;

    impl TeardownBarrier<Probe> for ProbeBarrier {
        fn make_barrier() -> Probe { Probe::barrier() }
    }

    fn small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let handle = thread::Builder::new()
            .name("guarded-pathmap-256k".to_owned())
            .stack_size(256 * 1024)
            .spawn(f)
            .expect("the fixed-stack test thread must spawn");
        match handle.join() {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn guarded_probe_map(
        tracking: &Arc<ProbeTracking>,
        order: &Arc<Mutex<Vec<usize>>>,
    ) -> GuardedPathMap<Probe, impl Fn(Probe) + Clone + Send + Sync + 'static, ProbeBarrier> {
        let tracking = Arc::clone(tracking);
        let order = Arc::clone(order);
        GuardedPathMap::new_with_barrier(move |probe: Probe| {
            assert!(probe.tracking.is_some(), "synthetic barriers never reach the payload sink");
            order.lock().unwrap().push(probe.id);
            drop(probe);
            let _ = &tracking;
        })
    }

    #[test]
    fn root_and_dense_children_drain_in_exact_reverse_postorder_on_256k_stack() {
        small_stack(|| {
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut guarded = guarded_probe_map(&tracking, &order);
            guarded.set_val_at([], Probe::tracked(256, &tracking));
            for byte in 0_u8..=255 {
                guarded.set_val_at([byte], Probe::tracked(byte as usize, &tracking));
            }
            drop(guarded);

            let mut expected: Vec<_> = (0..=255).rev().collect();
            expected.push(256);
            assert_eq!(*order.lock().unwrap(), expected);
            assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
            assert_eq!(tracking.drops.load(Ordering::Relaxed), 257);
        });
    }

    #[test]
    fn deep_value_and_value_free_lines_drain_on_256k_stack() {
        small_stack(|| {
            const DEPTH: usize = 65_536;
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let path = vec![0x5a; DEPTH];
            let mut guarded = guarded_probe_map(&tracking, &order);
            guarded.create_path(&path);
            guarded.set_val_at(&path, Probe::tracked(7, &tracking));
            drop(guarded);
            assert_eq!(*order.lock().unwrap(), vec![7]);
            assert_eq!(tracking.drops.load(Ordering::Relaxed), 1);

            struct UnitBarrier;
            impl TeardownBarrier<()> for UnitBarrier {
                fn make_barrier() {}
            }
            let mut topology = PathMap::<()>::new();
            topology.create_path(&path);
            let stats = drain_with_stats::<_, UnitBarrier, _>(&mut topology, |_| {});
            assert_eq!(stats.values, 0);
            assert_eq!(stats.edges, DEPTH);
            assert_eq!(stats.widest_prune, 1);
            assert!(topology.is_empty());
        });
    }

    #[test]
    fn guarded_topology_only_line_uses_final_anchor_teardown_on_256k_stack() {
        small_stack(|| {
            const DEPTH: usize = 65_536;
            struct UnitBarrier;
            impl TeardownBarrier<()> for UnitBarrier {
                fn make_barrier() {}
            }
            let disposed = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&disposed);
            let mut guarded = GuardedPathMap::<(), _, UnitBarrier>::new_with_barrier(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
            });
            assert!(guarded.create_path(vec![0xa5; DEPTH]));
            assert!(!guarded.is_empty(), "value-free topology is not an empty PathMap");
            drop(guarded);
            assert_eq!(disposed.load(Ordering::Relaxed), 0);
        });
    }

    #[test]
    fn deep_comb_drain_is_stack_bounded_and_reverse_postorder() {
        small_stack(|| {
            const DEPTH: usize = 4_096;
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut guarded = guarded_probe_map(&tracking, &order);
            let mut spine = Vec::with_capacity(DEPTH + 1);
            for id in 0..DEPTH {
                let mut tooth = spine.clone();
                tooth.push(1);
                guarded.set_val_at(&tooth, Probe::tracked(id, &tracking));
                spine.push(0);
            }
            guarded.set_val_at(&spine, Probe::tracked(DEPTH, &tracking));
            drop(guarded);

            assert_eq!(*order.lock().unwrap(), (0..=DEPTH).collect::<Vec<_>>());
            assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
            assert_eq!(tracking.drops.load(Ordering::Relaxed), DEPTH + 1);
        });
    }

    #[test]
    fn cloning_shares_one_anchor_and_only_the_final_alias_tears_down() {
        for aliases in [2, 3, 8, 32] {
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut original = guarded_probe_map(&tracking, &order);
            original.set_val_at([1, 2, 3], Probe::tracked(aliases, &tracking));
            let start = Arc::new(Barrier::new(aliases));
            let mut threads = Vec::with_capacity(aliases - 1);
            for _ in 1..aliases {
                let alias = original.clone();
                let start = Arc::clone(&start);
                threads.push(
                    thread::Builder::new()
                        .stack_size(256 * 1024)
                        .spawn(move || {
                            start.wait();
                            drop(alias);
                        })
                        .unwrap(),
                );
            }
            assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
            start.wait();
            for handle in threads {
                handle.join().unwrap();
            }
            assert!(order.lock().unwrap().is_empty(), "a surviving alias owns the anchor");
            drop(original);
            assert_eq!(*order.lock().unwrap(), vec![aliases]);
            assert_eq!(tracking.drops.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn make_mut_forks_a_version_and_preserves_the_sibling_image() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut left = guarded_probe_map(&tracking, &order);
        left.set_val_at([1, 2], Probe::tracked(1, &tracking));
        left.set_val_at([1, 3], Probe::tracked(2, &tracking));
        let right = left.clone();
        assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);

        left.set_val_at([1, 4], Probe::tracked(3, &tracking));
        assert!(left.contains([1, 4]));
        assert!(!right.contains([1, 4]));
        assert_eq!(left.get([1, 2]).map(|probe| probe.id), Some(1));
        assert_eq!(right.get([1, 2]).map(|probe| probe.id), Some(1));

        drop(left);
        assert!(right.contains([1, 2]));
        drop(right);

        let mut ids = order.lock().unwrap().clone();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 1, 2, 2, 3]);
        let instances = 3 + tracking.clones.load(Ordering::Relaxed);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), instances);
    }

    #[test]
    fn panicking_disposer_clone_precedes_raw_header_clone() {
        struct PanicOnClone {
            attempts: Arc<AtomicUsize>,
        }

        impl Clone for PanicOnClone {
            fn clone(&self) -> Self {
                self.attempts.fetch_add(1, Ordering::Relaxed);
                panic!("injected disposer-clone failure");
            }
        }

        small_stack(|| {
            let tracking = Arc::new(ProbeTracking::default());
            let disposed = Arc::new(AtomicUsize::new(0));
            let attempts = Arc::new(AtomicUsize::new(0));
            let panic_on_clone = PanicOnClone {
                attempts: Arc::clone(&attempts),
            };
            let disposed_in_sink = Arc::clone(&disposed);
            let mut left =
                GuardedPathMap::<Probe, _, ProbeBarrier>::new_with_barrier(move |probe: Probe| {
                    let _ = &panic_on_clone;
                    disposed_in_sink.fetch_add(1, Ordering::Relaxed);
                    drop(probe);
                });
            left.set_val_at([0; 4_096], Probe::tracked(1, &tracking));
            let right = left.clone();

            let result = catch_unwind(AssertUnwindSafe(|| {
                left.set_val_at([1], Probe::tracked(2, &tracking));
            }));
            assert!(result.is_err(), "the injected disposer clone must panic");
            assert_eq!(attempts.load(Ordering::Relaxed), 1);
            assert_eq!(
                tracking.clones.load(Ordering::Relaxed),
                0,
                "the raw PathMap header and payloads must not be cloned first"
            );
            assert!(left.contains([0; 4_096]));
            assert!(right.contains([0; 4_096]));
            assert!(!left.contains([1]));
            assert!(!right.contains([1]));

            drop(left);
            assert_eq!(disposed.load(Ordering::Relaxed), 0);
            drop(right);
            assert_eq!(disposed.load(Ordering::Relaxed), 1);
            // The rejected value never entered either map and is dropped once
            // while unwinding the failed mutation.
            assert_eq!(tracking.drops.load(Ordering::Relaxed), 2);
        });
    }

    #[derive(Clone, Debug)]
    struct PathPayload {
        path: Vec<u8>,
        original: bool,
    }

    struct PathPayloadBarrier;
    impl TeardownBarrier<PathPayload> for PathPayloadBarrier {
        fn make_barrier() -> PathPayload {
            PathPayload {
                path: Vec::new(),
                original: false,
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn arbitrary_value_and_topology_tries_conserve_every_edge_and_value(
            entries in vec((vec(any::<u8>(), 0..9), any::<bool>()), 0..48)
        ) {
            let mut canonical = BTreeMap::<Vec<u8>, bool>::new();
            for (path, has_value) in entries {
                canonical
                    .entry(path)
                    .and_modify(|old| *old |= has_value)
                    .or_insert(has_value);
            }

            let mut map = PathMap::<PathPayload>::new();
            let mut edges = BTreeSet::new();
            let mut expected_values = Vec::new();
            for (path, has_value) in &canonical {
                for length in 1..=path.len() {
                    edges.insert(path[..length].to_vec());
                }
                let is_redundant_value_free_prefix = !*has_value
                    && canonical
                        .keys()
                        .any(|other| other.len() > path.len() && other.starts_with(path));
                if !*has_value && !is_redundant_value_free_prefix {
                    map.create_path(path);
                }
                if *has_value {
                    map.set_val_at(path, PathPayload {
                        path: path.clone(),
                        original: true,
                    });
                    expected_values.push(path.clone());
                }
            }
            expected_values.sort_by(|left, right| right.cmp(left));

            let disposed = RefCell::new(Vec::new());
            let stats = drain_with_stats::<_, PathPayloadBarrier, _>(&mut map, |payload| {
                assert!(payload.original, "barriers must bypass the payload sink");
                disposed.borrow_mut().push(payload.path);
            });

            prop_assert_eq!(disposed.into_inner(), expected_values);
            prop_assert_eq!(stats.values, canonical.values().filter(|value| **value).count());
            prop_assert_eq!(stats.edges, edges.len());
            prop_assert!(stats.widest_prune <= 1);
            prop_assert!(map.is_empty());
        }
    }

    #[test]
    fn implementation_contains_no_snapshot_count_or_owned_raw_escape() {
        let source = include_str!("guarded_pathmap.rs");
        assert!(!source.contains(concat!("Arc::strong_", "count")));
        assert!(!source.contains(concat!("into_", "write_zipper")));
        assert!(!source.contains(concat!("OwnedZipper", "Iter")));
        assert!(!source.contains(concat!("pub fn into_", "map")));
    }
}
