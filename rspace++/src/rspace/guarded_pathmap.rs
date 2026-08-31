//! Ownership guard for stack-safe destruction of [`PathMap`] values.
//!
//! A cloned guard shares one immutable `PathMap` header through an `Arc`.
//! Mutation is the only operation that creates another header, through
//! `Arc::make_mut`.  The final anchor owner drains both payloads and trie
//! topology with a borrowed zipper before the now-empty map is dropped.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::utils::ByteMask;
use pathmap::zipper::{
    ReadZipperIter, ReadZipperUntracked, WriteZipperUntracked, Zipper, ZipperIteration,
    ZipperMoving, ZipperReadOnlyIteration, ZipperSubtries, ZipperValues, ZipperWriting,
};

/// Stack-safe disposal policy for values detached from a guarded map.
///
/// Production implementations are named, auditable policies.  In particular,
/// there is no blanket closure implementation: final teardown must not invoke
/// caller-supplied code that can unwind while a raw trie remains populated.
pub(crate) trait DisposeValue<V>: Clone + Send + Sync + 'static {
    fn dispose(&self, value: V);
}

/// Supplies a temporary value that stops `prune_ascend` at the immediate
/// parent of the edge being removed.
///
/// The policy has no runtime representation: only its associated functions
/// are used.  `make_barrier` must return a value whose destruction by
/// `discard_barrier` is bounded and non-panicking.  A barrier is never passed
/// to the original-value disposal sink.
pub(crate) trait TeardownBarrier<V> {
    fn make_barrier() -> V;

    fn discard_barrier(barrier: V) { drop(barrier); }
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
pub(crate) struct GuardedPathMap<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    anchor: Arc<AnchorRoot<V, D, B>>,
}

struct AnchorRoot<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
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
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    fn clone(&self) -> Self {
        // Evaluation order is an ownership invariant.  Clone the policy before
        // the raw PathMap header so even a defective policy implementation
        // cannot strand a temporary nonempty map during this step.
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
    D: DisposeValue<V>,
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
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    fn drop(&mut self) {
        let Some(map) = self.map.take() else {
            return;
        };
        // Quarantine the raw header before invoking any teardown policy.  An
        // unexpected unwind must never recursively drop a still-populated
        // PathMap while unwinding this destructor.  Production policies are
        // closed, named, and non-panicking; the boundary below is the
        // fail-stop backstop proved by PathMapReversePruneTeardown.v.
        let mut quarantined = ManuallyDrop::new(map);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            drain_pathmap::<V, B, _>(&mut quarantined, &self.dispose);
            // The ordinary PathMap destructor now sees no values and no
            // edges.  Keep this inside the boundary so even an impossible
            // empty-header destructor failure is terminal.
            unsafe { ManuallyDrop::drop(&mut quarantined) };
        }));
        if outcome.is_err() {
            // Continuing would retain unaudited ownership authority; resuming
            // unwinding would recursively drop the quarantined raw header.
            // Abort is therefore the only safe outcome.
            std::process::abort();
        }
    }
}

impl<V, D, B> GuardedPathMap<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
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

    #[cfg(test)]
    pub(crate) fn anchor_identity(&self) -> usize { Arc::as_ptr(&self.anchor) as usize }

    /// Borrowed iterator whose lifetime is anchored to this guarded version.
    /// The wrapper intentionally exposes none of PathMap's owned-subtree APIs.
    pub(crate) fn iter(&self) -> GuardedPathMapIter<'_, V> {
        GuardedPathMapIter {
            inner: self.map().read_zipper().into_iter(),
        }
    }

    /// Run an operation against a restricted borrowed root cursor.  The
    /// facade exposes navigation and values, but deliberately implements none
    /// of PathMap's owned-subtree traits.
    pub(crate) fn with_read_cursor<R>(
        &self,
        operation: impl for<'map> FnOnce(GuardedReadCursor<'map, 'static, V, D, B>) -> R,
    ) -> R {
        operation(GuardedReadCursor {
            inner: self.map().read_zipper(),
            dispose: &self.anchor.dispose,
            barrier: PhantomData,
        })
    }

    /// Prefix-rooted counterpart to [`Self::with_read_cursor`].
    pub(crate) fn with_read_cursor_at<'path, R>(
        &self,
        path: &'path [u8],
        operation: impl for<'map> FnOnce(GuardedReadCursor<'map, 'path, V, D, B>) -> R,
    ) -> R {
        operation(GuardedReadCursor {
            inner: self.map().read_zipper_at_borrowed_path(path),
            dispose: &self.anchor.dispose,
            barrier: PhantomData,
        })
    }

    /// Run a mutation against the sole fresh-header path through a facade that
    /// has no raw-detachment, graft, or owned-map operations.
    pub(crate) fn with_write_cursor<R>(
        &mut self,
        operation: impl for<'map> FnOnce(GuardedWriteCursor<'map, 'static, V, D, B>) -> R,
    ) -> R {
        let anchor = self.fresh_map_mut();
        let map = anchor
            .map
            .as_mut()
            .expect("a live PathMap anchor always contains its map");
        operation(GuardedWriteCursor {
            inner: map.write_zipper(),
            dispose: &anchor.dispose,
            barrier: PhantomData,
        })
    }

    /// Prefix-rooted counterpart to [`Self::with_write_cursor`].
    ///
    /// Paths supplied to the restricted cursor are relative to `path`, and
    /// the cursor cannot ascend above that immutable root.  This narrows the
    /// authority of the ordinary root cursor without exposing raw PathMap,
    /// owned-subtree, graft, or detach operations.
    pub(crate) fn with_write_cursor_at<'path, R>(
        &mut self,
        path: &'path [u8],
        operation: impl for<'map> FnOnce(GuardedWriteCursor<'map, 'path, V, D, B>) -> R,
    ) -> R {
        let anchor = self.fresh_map_mut();
        let map = anchor
            .map
            .as_mut()
            .expect("a live PathMap anchor always contains its map");
        operation(GuardedWriteCursor {
            inner: map.write_zipper_at_path(path),
            dispose: &anchor.dispose,
            barrier: PhantomData,
        })
    }

    /// Canonical ACTree03 encoding without exposing the source zipper.
    pub(crate) fn compact_u64_bytes(&self, encode: impl Fn(&V) -> u64) -> Vec<u8> {
        ArenaCompactTree::from_zipper(self.map().read_zipper(), encode)
            .get_data()
            .to_vec()
    }

    /// Sets a value and sends any replaced value through the configured
    /// stack-safe disposal path.  Returns whether a value was replaced.
    pub fn set_val_at<K: AsRef<[u8]>>(&mut self, path: K, value: V) -> bool {
        let anchor = self.fresh_map_mut();
        match anchor.map_mut().set_val_at(path, value) {
            Some(replaced) => {
                anchor.dispose.dispose(replaced);
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

    /// Remove one value and then remove its newly dangling path one logical
    /// edge at a time.  Every detached value goes through the disposal policy.
    pub(crate) fn remove_val_at_and_prune<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        let path = path.as_ref();
        let anchor = self.fresh_map_mut();
        let removed = anchor.map_mut().remove_val_at(path, false);
        let Some(removed) = removed else {
            return false;
        };
        anchor.dispose.dispose(removed);
        prune_dangling_path::<V, B>(anchor.map_mut(), path);
        true
    }

    /// Drain every value and edge rooted at `prefix` without detaching a raw
    /// subtree.  `fold` observes each value immediately before its named
    /// disposal, allowing metadata to be accumulated during the same walk.
    /// The remaining prefix path is pruned toward the nearest live value/fork
    /// in one-edge segments.
    pub(crate) fn remove_prefix_fold<K, R, F>(
        &mut self,
        prefix: K,
        initial: R,
        fold: F,
    ) -> (usize, R)
    where
        K: AsRef<[u8]>,
        F: FnMut(R, &V) -> R,
    {
        let prefix = prefix.as_ref();
        let anchor = self.fresh_map_mut();
        let map = anchor
            .map
            .as_mut()
            .expect("a live PathMap anchor always contains its map");
        drain_prefix_fold::<V, B, D, R, F>(map, prefix, &anchor.dispose, initial, fold)
    }
}

impl<V, D, B> Clone for GuardedPathMap<V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            anchor: Arc::clone(&self.anchor),
        }
    }
}

pub(crate) struct GuardedPathMapIter<'a, V>
where V: Clone + Send + Sync + Unpin + 'static
{
    inner: ReadZipperIter<'a, 'static, V>,
}

/// Restricted borrowed read cursor.  No raw zipper is exposed and this type
/// does not implement `ZipperSubtries`, so callers cannot manufacture an
/// unanchored owned PathMap.
#[derive(Clone)]
pub(crate) struct GuardedReadCursor<'map, 'path, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    inner: ReadZipperUntracked<'map, 'path, V>,
    dispose: &'map D,
    barrier: PhantomData<fn() -> B>,
}

impl<V, D, B> GuardedReadCursor<'_, '_, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    #[inline]
    pub(crate) fn path_exists(&self) -> bool { self.inner.path_exists() }

    #[inline]
    pub(crate) fn path(&self) -> &[u8] { self.inner.path() }

    #[inline]
    pub(crate) fn val(&self) -> Option<&V> { self.inner.val() }

    #[inline]
    pub(crate) fn get_val(&self) -> Option<&V> { self.inner.val() }

    #[inline]
    pub(crate) fn val_count(&self) -> usize { self.inner.val_count() }

    #[inline]
    pub(crate) fn child_mask(&self) -> ByteMask { self.inner.child_mask() }

    #[inline]
    pub(crate) fn reset(&mut self) { self.inner.reset(); }

    #[inline]
    pub(crate) fn descend_to_existing<K: AsRef<[u8]>>(&mut self, path: K) -> usize {
        self.inner.descend_to_existing(path)
    }

    #[inline]
    pub(crate) fn descend_to_byte(&mut self, byte: u8) { self.inner.descend_to_byte(byte); }

    #[inline]
    pub(crate) fn ascend_byte(&mut self) -> bool { self.inner.ascend_byte() }

    #[inline]
    pub(crate) fn to_next_val(&mut self) -> bool { self.inner.to_next_val() }

    #[inline]
    pub(crate) fn to_next_get_val(&mut self) -> Option<&V> { self.inner.to_next_get_val() }

    /// Snapshot the current focus directly into another guarded anchor.  The
    /// disposer is cloned before PathMap creates the raw subtree, so a
    /// panicking policy clone cannot strand a temporary raw owner.
    pub(crate) fn guarded_focus_snapshot(&self) -> GuardedPathMap<V, D, B> {
        let dispose = self.dispose.clone();
        let root_value = self.inner.val().cloned();
        let mut snapshot =
            GuardedPathMap::from_map(self.inner.make_map().unwrap_or_default(), dispose);
        if let Some(root_value) = root_value {
            snapshot.set_val_at([], root_value);
        }
        snapshot
    }
}

/// Restricted borrowed write cursor used by Bag bulk updates.
pub(crate) struct GuardedWriteCursor<'map, 'path, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    inner: WriteZipperUntracked<'map, 'path, V>,
    dispose: &'map D,
    barrier: PhantomData<fn() -> B>,
}

impl<V, D, B> GuardedWriteCursor<'_, '_, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    #[inline]
    pub(crate) fn path(&self) -> &[u8] { self.inner.path() }

    #[inline]
    pub(crate) fn at_root(&self) -> bool { self.inner.at_root() }

    #[inline]
    pub(crate) fn is_val(&self) -> bool { self.inner.is_val() }

    #[inline]
    pub(crate) fn val(&self) -> Option<&V> { self.inner.val() }

    #[inline]
    pub(crate) fn move_to_path(&mut self, path: &[u8]) { self.inner.move_to_path(path); }

    #[inline]
    pub(crate) fn set_val(&mut self, value: V) {
        if let Some(replaced) = self.inner.set_val(value) {
            self.dispose.dispose(replaced);
        }
    }

    #[inline]
    pub(crate) fn descend_first_byte(&mut self) -> bool { self.inner.descend_first_byte() }

    #[inline]
    pub(crate) fn descend_until(&mut self) -> bool { self.inner.descend_until() }

    #[inline]
    pub(crate) fn to_next_sibling_byte(&mut self) -> bool { self.inner.to_next_sibling_byte() }

    #[inline]
    pub(crate) fn ascend_byte(&mut self) -> bool { self.inner.ascend_byte() }

    /// Remove the focused value and prune only newly dangling logical edges.
    pub(crate) fn remove_val_and_prune(&mut self) -> bool {
        let Some(removed) = self.inner.remove_val(false) else {
            return false;
        };
        self.dispose.dispose(removed);
        while !self.inner.at_root() && !self.inner.is_val() && self.inner.child_count() == 0 {
            prune_one_edge::<V, B>(&mut self.inner);
        }
        true
    }
}

impl<'a, V> Iterator for GuardedPathMapIter<'a, V>
where V: Clone + Send + Sync + Unpin + 'static
{
    type Item = (Vec<u8>, &'a V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> { self.inner.next() }
}

trait DrainObserver {
    #[inline]
    fn removed_value(&mut self) {}

    #[inline]
    fn pruned(&mut self, _width: usize) {}

    #[inline]
    fn installed_barrier(&mut self) {}

    #[inline]
    fn removed_barrier(&mut self) {}
}

struct NoopDrainObserver;

impl DrainObserver for NoopDrainObserver {}

/// Empties `map` in reverse byte/postorder.  Each `prune_ascend` is bounded by
/// a real or synthetic value at the immediate parent, so a detached physical
/// subtrie can never span multiple logical edges.
fn drain_pathmap<V, B, D>(map: &mut PathMap<V>, dispose: &D)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
{
    let mut observer = NoopDrainObserver;
    drain_pathmap_observed::<V, B, D, _>(map, dispose, &mut observer);
}

fn drain_pathmap_observed<V, B, D, O>(map: &mut PathMap<V>, dispose: &D, observer: &mut O)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
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
                dispose.dispose(value);
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
                observer.installed_barrier();
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
                observer.removed_barrier();
            }
        }
    }

    debug_assert!(map.is_empty());
}

/// Remove the empty focus at `path` toward the nearest retained value or fork.
/// A real or synthetic parent value bounds every prune to exactly one byte.
fn prune_dangling_path<V, B>(map: &mut PathMap<V>, path: &[u8])
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
{
    let mut zipper = map.write_zipper();
    zipper.descend_to(path);
    if !zipper.path_exists() || zipper.is_val() || zipper.child_count() != 0 {
        return;
    }
    loop {
        if zipper.at_root() || zipper.is_val() || zipper.child_count() != 0 {
            break;
        }
        prune_one_edge::<V, B>(&mut zipper);
    }
}

/// Drain only the subtree rooted at `prefix`, returning its real value count
/// and the caller's single-pass metadata fold.
fn drain_prefix_fold<V, B, D, R, F>(
    map: &mut PathMap<V>,
    prefix: &[u8],
    dispose: &D,
    mut accumulator: R,
    mut fold: F,
) -> (usize, R)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
    F: FnMut(R, &V) -> R,
{
    let mut removed = 0;
    let mut zipper = map.write_zipper();
    zipper.descend_to(prefix);
    if !zipper.path_exists() {
        return (0, accumulator);
    }
    let prefix_depth = zipper.path().len();
    loop {
        while zipper.descend_last_byte() {}
        if let Some(value) = zipper.remove_val(false) {
            removed += 1;
            accumulator = fold(accumulator, &value);
            dispose.dispose(value);
        }
        if zipper.path().len() == prefix_depth {
            break;
        }
        prune_one_edge::<V, B>(&mut zipper);
    }
    while !zipper.at_root() && !zipper.is_val() && zipper.child_count() == 0 {
        prune_one_edge::<V, B>(&mut zipper);
    }
    (removed, accumulator)
}

/// Prune the empty edge at the current focus and leave the zipper at its
/// immediate parent.  The temporary barrier prevents PathMap from recursively
/// pruning beyond that one logical edge.
fn prune_one_edge<V, B>(zipper: &mut WriteZipperUntracked<'_, '_, V>)
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
{
    debug_assert!(!zipper.at_root());
    debug_assert!(!zipper.is_val());
    debug_assert_eq!(zipper.child_count(), 0);
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
    debug_assert_eq!(width, 1, "the parent barrier must limit pruning to one byte");
    if installed_barrier {
        let barrier = zipper
            .remove_val(false)
            .expect("the temporary teardown barrier must remain at its parent");
        B::discard_barrier(barrier);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    use pathmap::PathMap;
    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::{
        DisposeValue, DrainObserver, GuardedPathMap, TeardownBarrier, drain_pathmap_observed,
    };

    #[derive(Clone)]
    struct TestDisposer<F>(F);

    impl<V, F> DisposeValue<V> for TestDisposer<F>
    where F: Fn(V) + Clone + Send + Sync + 'static
    {
        fn dispose(&self, value: V) { (self.0)(value); }
    }

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

    fn drain_with_stats<V, B, F>(map: &mut PathMap<V>, dispose: F) -> DrainStats
    where
        V: Clone + Send + Sync + Unpin + 'static,
        B: TeardownBarrier<V>,
        F: Fn(V) + Clone + Send + Sync + 'static,
    {
        let mut stats = DrainStats::default();
        let dispose = TestDisposer(dispose);
        drain_pathmap_observed::<V, B, _, _>(map, &dispose, &mut stats);
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
    ) -> GuardedPathMap<
        Probe,
        TestDisposer<impl Fn(Probe) + Clone + Send + Sync + 'static>,
        ProbeBarrier,
    > {
        let tracking = Arc::clone(tracking);
        let order = Arc::clone(order);
        GuardedPathMap::new_with_barrier(TestDisposer(move |probe: Probe| {
            assert!(probe.tracking.is_some(), "synthetic barriers never reach the payload sink");
            order.lock().unwrap().push(probe.id);
            drop(probe);
            let _ = &tracking;
        }))
    }

    #[test]
    fn prefix_rooted_writer_cannot_escape_or_address_a_sibling_as_relative() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);
        guarded.set_val_at([1, 9], Probe::tracked(1, &tracking));
        let anchor = guarded.anchor_identity();

        guarded.with_write_cursor_at(&[2, 8], |mut cursor| {
            assert!(cursor.at_root());
            cursor.move_to_path(&[3, 7]);
            assert_eq!(cursor.path(), [3, 7]);
            cursor.set_val(Probe::tracked(2, &tracking));
            while cursor.ascend_byte() {}
            assert!(cursor.at_root());
            assert!(!cursor.ascend_byte(), "the restricted cursor cannot ascend above its root");
        });

        assert_eq!(guarded.anchor_identity(), anchor, "a unique anchor mutates in place");
        assert_eq!(guarded.get([1, 9]).map(|probe| probe.id), Some(1));
        assert_eq!(guarded.get([2, 8, 3, 7]).map(|probe| probe.id), Some(2));
        assert!(guarded.get([3, 7]).is_none());
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
            let mut guarded =
                GuardedPathMap::<(), _, UnitBarrier>::new_with_barrier(TestDisposer(move |_| {
                    observed.fetch_add(1, Ordering::Relaxed);
                }));
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
    fn prefix_drain_removes_values_and_value_free_topology_in_one_walk() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);
        guarded.set_val_at([], Probe::tracked(0, &tracking));
        guarded.set_val_at([1], Probe::tracked(1, &tracking));
        guarded.set_val_at([1, 2], Probe::tracked(2, &tracking));
        guarded.create_path([1, 3, 4]);
        guarded.set_val_at([2], Probe::tracked(3, &tracking));

        let (removed, mut folded) =
            guarded.remove_prefix_fold([1], Vec::new(), |mut ids, probe| {
                ids.push(probe.id);
                ids
            });
        folded.sort_unstable();
        assert_eq!(removed, 2);
        assert_eq!(folded, vec![1, 2]);
        assert!(!guarded.path_exists([1]));
        assert_eq!(guarded.get([]).map(|probe| probe.id), Some(0));
        assert_eq!(guarded.get([2]).map(|probe| probe.id), Some(3));
        assert_eq!(*order.lock().unwrap(), vec![2, 1]);

        drop(guarded);
        assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn prefix_drain_of_comb_preserves_every_outside_sibling() {
        small_stack(|| {
            const DEPTH: usize = 1_024;
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut guarded = guarded_probe_map(&tracking, &order);
            guarded.set_val_at([3], Probe::tracked(10_000, &tracking));
            guarded.set_val_at([5, 0], Probe::tracked(10_001, &tracking));

            let mut spine = vec![4];
            for id in 0..DEPTH {
                let mut tooth = spine.clone();
                tooth.push(1);
                guarded.set_val_at(&tooth, Probe::tracked(id, &tracking));
                spine.push(0);
            }
            guarded.set_val_at(&spine, Probe::tracked(DEPTH, &tracking));

            let (removed, cardinality) = guarded.remove_prefix_fold([4], 0usize, |sum, _| sum + 1);
            assert_eq!(removed, DEPTH + 1);
            assert_eq!(cardinality, DEPTH + 1);
            assert!(!guarded.path_exists([4]));
            assert_eq!(guarded.get([3]).map(|probe| probe.id), Some(10_000));
            assert_eq!(guarded.get([5, 0]).map(|probe| probe.id), Some(10_001));

            drop(guarded);
            assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
            assert_eq!(tracking.drops.load(Ordering::Relaxed), DEPTH + 3);
        });
    }

    #[test]
    fn exact_removal_prunes_only_to_a_real_parent_or_fork() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);
        guarded.set_val_at([1], Probe::tracked(1, &tracking));
        guarded.set_val_at([1, 2], Probe::tracked(2, &tracking));
        guarded.set_val_at([1, 3], Probe::tracked(3, &tracking));
        guarded.set_val_at([4, 5], Probe::tracked(4, &tracking));
        guarded.set_val_at([4, 6], Probe::tracked(5, &tracking));

        assert!(guarded.remove_val_at_and_prune([1, 2]));
        assert_eq!(guarded.get([1]).map(|probe| probe.id), Some(1));
        assert_eq!(guarded.get([1, 3]).map(|probe| probe.id), Some(3));
        assert!(!guarded.path_exists([1, 2]));

        assert!(guarded.remove_val_at_and_prune([4, 5]));
        assert!(guarded.path_exists([4]));
        assert_eq!(guarded.get([4, 6]).map(|probe| probe.id), Some(5));
        assert!(!guarded.path_exists([4, 5]));
        drop(guarded);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn shared_anchor_prefix_removal_cows_and_preserves_the_other_image() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut left = guarded_probe_map(&tracking, &order);
        left.set_val_at([1, 2], Probe::tracked(1, &tracking));
        left.set_val_at([1, 3], Probe::tracked(2, &tracking));
        left.set_val_at([2], Probe::tracked(3, &tracking));
        let right = left.clone();

        let (removed, sum) = left.remove_prefix_fold([1], 0usize, |sum, _| sum + 1);
        assert_eq!((removed, sum), (2, 2));
        assert!(!left.path_exists([1]));
        assert!(left.contains([2]));
        assert!(right.contains([1, 2]));
        assert!(right.contains([1, 3]));
        assert!(right.contains([2]));

        drop(left);
        assert!(right.contains([1, 2]));
        drop(right);
        let instances = 3 + tracking.clones.load(Ordering::Relaxed);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), instances);
    }

    #[test]
    fn dropping_a_borrowed_iterator_early_preserves_anchor_authority() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);
        guarded.set_val_at([1], Probe::tracked(1, &tracking));
        guarded.set_val_at([2], Probe::tracked(2, &tracking));
        {
            let mut iterator = guarded.iter();
            assert!(iterator.next().is_some());
        }
        assert!(guarded.contains([1]));
        assert!(guarded.contains([2]));
        assert!(order.lock().unwrap().is_empty());
        drop(guarded);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn unconstrained_final_alias_race_drains_exactly_once() {
        const ALIASES: usize = 32;
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut original = guarded_probe_map(&tracking, &order);
        original.set_val_at([1, 2, 3], Probe::tracked(7, &tracking));
        let mut aliases = Vec::with_capacity(ALIASES);
        for _ in 1..ALIASES {
            aliases.push(original.clone());
        }
        aliases.push(original);

        let start = Arc::new(Barrier::new(ALIASES));
        let threads: Vec<_> = aliases
            .into_iter()
            .map(|alias| {
                let start = Arc::clone(&start);
                thread::Builder::new()
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        start.wait();
                        drop(alias);
                    })
                    .unwrap()
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![7]);
        assert_eq!(tracking.clones.load(Ordering::Relaxed), 0);
        assert_eq!(tracking.drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn unexpected_teardown_panic_is_process_fail_stop() {
        const CHILD: &str = "RSPACE_GUARDED_PATHMAP_ABORT_CHILD";

        #[derive(Clone)]
        struct PanickingDisposer;
        impl DisposeValue<Probe> for PanickingDisposer {
            fn dispose(&self, _value: Probe) {
                panic!("injected teardown policy failure");
            }
        }

        if std::env::var_os(CHILD).is_some() {
            let worker = thread::Builder::new()
                .name("guarded-pathmap-abort-child".to_owned())
                .stack_size(256 * 1024)
                .spawn(|| {
                    let mut guarded = GuardedPathMap::<Probe, _, ProbeBarrier>::new_with_barrier(
                        PanickingDisposer,
                    );
                    guarded.set_val_at([0x5a; 4_096], Probe::barrier());
                    drop(guarded);
                })
                .unwrap();
            worker.join().unwrap();
            panic!("a panicking teardown policy incorrectly returned to the live process");
        }

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("unexpected_teardown_panic_is_process_fail_stop")
            .arg("--nocapture")
            .env(CHILD, "1")
            .output()
            .expect("the isolated teardown-failure child must launch");
        assert!(
            !output.status.success(),
            "unexpected teardown failure must terminate the isolated process"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("injected teardown policy failure"),
            "the isolated failure must reach the guarded teardown boundary"
        );
    }

    #[test]
    fn barrier_install_prune_remove_order_is_observable_and_exact() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Event {
            Value,
            Install,
            Prune(usize),
            Remove,
        }

        struct EventLog(Vec<Event>);
        impl DrainObserver for EventLog {
            fn removed_value(&mut self) { self.0.push(Event::Value); }

            fn pruned(&mut self, width: usize) { self.0.push(Event::Prune(width)); }

            fn installed_barrier(&mut self) { self.0.push(Event::Install); }

            fn removed_barrier(&mut self) { self.0.push(Event::Remove); }
        }

        let mut map = PathMap::new();
        map.set_val_at([1, 2], Probe::barrier());
        let dispose = TestDisposer(drop);
        let mut events = EventLog(Vec::new());
        drain_pathmap_observed::<Probe, ProbeBarrier, _, _>(&mut map, &dispose, &mut events);
        assert_eq!(events.0, vec![
            Event::Value,
            Event::Install,
            Event::Prune(1),
            Event::Remove,
            Event::Install,
            Event::Prune(1),
            Event::Remove,
        ]);
        assert!(map.is_empty());
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
            let mut left = GuardedPathMap::<Probe, _, ProbeBarrier>::new_with_barrier(
                TestDisposer(move |probe: Probe| {
                    let _ = &panic_on_clone;
                    disposed_in_sink.fetch_add(1, Ordering::Relaxed);
                    drop(probe);
                }),
            );
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

            let disposed = Arc::new(Mutex::new(Vec::new()));
            let observed = Arc::clone(&disposed);
            let stats = drain_with_stats::<_, PathPayloadBarrier, _>(&mut map, move |payload| {
                assert!(payload.original, "barriers must bypass the payload sink");
                observed.lock().unwrap().push(payload.path);
            });

            let actual_values = disposed.lock().unwrap().clone();
            prop_assert_eq!(actual_values, expected_values);
            prop_assert_eq!(stats.values, canonical.values().filter(|value| **value).count());
            prop_assert_eq!(stats.edges, edges.len());
            prop_assert!(stats.widest_prune <= 1);
            prop_assert!(map.is_empty());
        }
    }

    #[test]
    fn implementation_contains_no_snapshot_count_or_owned_raw_escape() {
        let source = include_str!("guarded_pathmap.rs");
        let bag_source = include_str!("bag.rs");
        assert!(!source.contains(concat!("Arc::strong_", "count")));
        assert!(!source.contains(concat!("into_", "write_zipper")));
        assert!(!source.contains(concat!("OwnedZipper", "Iter")));
        assert!(!source.contains(concat!("pub fn into_", "map")));
        assert!(!bag_source.contains(concat!("from_", "pathmap")));
        assert!(!bag_source.contains(concat!("as_", "pathmap")));
        assert!(!bag_source.contains(concat!("into_", "pathmap")));
        assert!(!bag_source.contains("use pathmap::PathMap"));
        assert!(!bag_source.contains("entries: PathMap<"));
    }
}
