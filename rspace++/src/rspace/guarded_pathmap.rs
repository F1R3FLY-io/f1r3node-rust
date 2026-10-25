//! Ownership guard for stack-safe destruction of [`PathMap`] values.
//!
//! A cloned guard shares one immutable `PathMap` header through an `Arc`.
//! Mutation is the only operation that creates another header, through
//! `Arc::make_mut`.  The final anchor owner drains both payloads and trie
//! topology with a borrowed zipper before the now-empty map is dropped.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use pathmap::PathMap;
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::utils::ByteMask;
use pathmap::zipper::{
    ReadZipperIter, ReadZipperUntracked, TrieRefBorrowed, WriteZipperUntracked, Zipper,
    ZipperForking, ZipperIteration, ZipperMoving, ZipperReadOnlyIteration, ZipperReadOnlySubtries,
    ZipperValues, ZipperWriting,
};

use super::pathmap_mutation::NativePathCursor;

/// Stack-safe disposal policy for values detached from a guarded map.
///
/// Production implementations are named, auditable policies.  In particular,
/// there is no blanket closure implementation: final teardown must not invoke
/// caller-supplied code that can unwind while a raw trie remains populated.
pub trait DisposeValue<V>: Clone + Send + Sync + 'static {
    fn dispose(&self, value: V);
}

#[derive(Clone, Copy)]
pub(crate) struct ValueFreeDisposer;

impl<V> DisposeValue<V> for ValueFreeDisposer {
    fn dispose(&self, _value: V) { std::process::abort(); }
}

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

pub trait BarrierProvider<V> {
    fn make_barrier(&self) -> V;

    fn discard_barrier(&self, barrier: V) { drop(barrier); }
}

struct StaticBarrier<B>(PhantomData<fn() -> B>);

impl<V, B: TeardownBarrier<V>> BarrierProvider<V> for StaticBarrier<B> {
    fn make_barrier(&self) -> V { B::make_barrier() }

    fn discard_barrier(&self, barrier: V) { B::discard_barrier(barrier); }
}

struct SyntheticBarrier<'a, V, B>
where B: BarrierProvider<V>
{
    value: Option<V>,
    barrier: &'a B,
}

impl<'a, V, B> SyntheticBarrier<'a, V, B>
where B: BarrierProvider<V>
{
    fn new(barrier: &'a B) -> Self {
        Self {
            value: Some(barrier.make_barrier()),
            barrier,
        }
    }

    fn into_value(mut self) -> V {
        self.value
            .take()
            .expect("a synthetic PathMap barrier is installed exactly once")
    }
}

impl<V, B> Drop for SyntheticBarrier<'_, V, B>
where B: BarrierProvider<V>
{
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.barrier.discard_barrier(value);
        }
    }
}

#[derive(Clone, Copy)]
enum ValuedLeafOrigin {
    Real,
    Synthetic,
}

#[derive(Default)]
struct BarrierPruneStats {
    real_values: usize,
    pruned_edges: usize,
    installed_barriers: usize,
    retired_barriers: usize,
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
    // Exact endpoint authority maintained by every guarded mutation gateway.
    // Final teardown consumes this certificate without invoking PathMap's
    // depth-recursive subtree count.
    value_count: usize,
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
            value_count: self.value_count,
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

    /// PathMap 0.3.4 may leave its private empty sentinel after pruning the
    /// final edge. That representation is safe to destroy but cannot be
    /// reopened for mutation. Cursor authority must already be closed before
    /// this normalization replaces it with the canonical fresh live root.
    fn normalize_live_empty(&mut self) {
        if self.map().is_empty() {
            debug_assert_eq!(self.value_count, 0);
            self.map = Some(PathMap::new());
        }
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
        let expected_values = self.value_count;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let removed_values = drain_pathmap::<V, B, _>(&mut quarantined, &self.dispose);
            assert_eq!(
                removed_values, expected_values,
                "guarded PathMap teardown must consume its exact endpoint authority",
            );
            self.value_count = 0;
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

impl<V, B> GuardedPathMap<V, ValueFreeDisposer, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V> + 'static,
{
    pub(crate) fn value_free_suffix(path: &[u8]) -> Self {
        let mut donor = Self::new_with_barrier(ValueFreeDisposer);
        donor.fresh_map_mut().map_mut().create_path(path);
        donor
    }

    pub(crate) fn graft_value_free_suffix(
        &self,
        destination: &mut WriteZipperUntracked<'_, '_, V>,
    ) {
        debug_assert_eq!(self.anchor.value_count, 0);
        destination.graft(&self.map().read_zipper());
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
        let map = if map.is_empty() { PathMap::new() } else { map };
        Self {
            anchor: Arc::new(AnchorRoot {
                map: Some(map),
                value_count: 0,
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

    #[cfg(test)]
    #[inline]
    pub fn is_empty(&self) -> bool { self.map().is_empty() }

    #[inline]
    pub fn contains<K: AsRef<[u8]>>(&self, path: K) -> bool { self.map().contains(path) }

    #[inline]
    pub fn path_exists<K: AsRef<[u8]>>(&self, path: K) -> bool { self.map().path_exists_at(path) }

    #[inline]
    pub fn get<K: AsRef<[u8]>>(&self, path: K) -> Option<&V> { self.map().get_val_at(path) }

    /// Visit the complete logical trie in canonical byte preorder without
    /// allocating a traversal stack.  Value-free leaves are included because
    /// they are part of PathMap's observable prefix-closed topology.
    #[cfg(test)]
    fn visit_topology(&self, mut visit: impl FnMut(&[u8], Option<&V>, bool)) {
        let mut source = self.map().read_zipper();
        loop {
            let first_child = source.child_mask().iter().next();
            let is_leaf = first_child.is_none();
            visit(source.path(), source.val(), is_leaf);

            if let Some(byte) = first_child {
                source.descend_to_byte(byte);
                continue;
            }
            loop {
                if source.to_next_sibling_byte() {
                    break;
                }
                if source.at_root() {
                    return;
                }
                let ascended = source.ascend_byte();
                debug_assert!(ascended);
            }
        }
    }

    /// Count endpoints iteratively.  This is deliberately independent of
    /// PathMap's depth-recursive subtree count and validates the guarded
    /// endpoint authority before a rebuilt root can be published.
    #[cfg(test)]
    fn iterative_value_count(&self) -> usize {
        let mut count = 0usize;
        self.visit_topology(|_, value, _| {
            if value.is_some() {
                count = count
                    .checked_add(1)
                    .expect("PathMap endpoint count fits usize");
            }
        });
        count
    }

    /// Rebuild a structurally edited version into a fresh trie allocation.
    ///
    /// A shallow PathMap clone is insufficient here: destructive pruning may
    /// retain shared nodes or cloned payloads between the source and candidate.
    /// Streaming retained endpoints and maximal value-free leaves into a fresh
    /// map establishes disjoint node and value ownership before publication.
    /// Removed values are never cloned.  The caller's fold runs only after the
    /// candidate has been built and validated, against the still-immutable
    /// source image.
    #[cfg(test)]
    fn rebuild_filter_fold<R, F, P>(
        &self,
        should_remove: P,
        initial: R,
        mut fold: F,
    ) -> (Self, usize, R)
    where
        F: FnMut(R, &V) -> R,
        P: Fn(&[u8]) -> bool,
    {
        let source_values = self.anchor.value_count;
        let mut removed = 0usize;
        let mut candidate = Self::new_with_barrier(self.anchor.dispose.clone());
        candidate.with_write_cursor(|mut target| {
            self.visit_topology(|path, value, is_leaf| {
                if should_remove(path) {
                    if value.is_some() {
                        removed = removed
                            .checked_add(1)
                            .expect("removed PathMap endpoint count fits usize");
                    }
                    return;
                }

                target.move_to_path(path);
                if let Some(value) = value {
                    target.set_val(value.clone());
                } else if is_leaf && !path.is_empty() {
                    target.create_path();
                }
            });
        });

        let expected_retained = source_values
            .checked_sub(removed)
            .expect("a structural rebuild removes only resident guarded endpoints");
        assert_eq!(
            candidate.anchor.value_count, expected_retained,
            "a rebuilt PathMap candidate carries exact endpoint authority",
        );
        assert_eq!(
            candidate.iterative_value_count(),
            expected_retained,
            "an iterative candidate audit agrees with guarded endpoint authority",
        );

        let mut accumulator = Some(initial);
        let mut folded = 0usize;
        self.visit_topology(|path, value, _| {
            if !should_remove(path) {
                return;
            }
            if let Some(value) = value {
                let current = accumulator
                    .take()
                    .expect("the structural-removal accumulator remains live");
                accumulator = Some(fold(current, value));
                folded = folded
                    .checked_add(1)
                    .expect("folded PathMap endpoint count fits usize");
            }
        });
        assert_eq!(folded, removed, "every removed endpoint is folded exactly once");

        (candidate, removed, accumulator.expect("the structural-removal accumulator is returned"))
    }

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
            _dispose: &self.anchor.dispose,
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
            _dispose: &self.anchor.dispose,
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
        let value_count = &mut anchor.value_count;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            operation(GuardedWriteCursor {
                inner: NativePathCursor::new_counted(map, &[], &anchor.dispose, value_count),
                dispose: &anchor.dispose,
            })
        }));
        anchor.normalize_live_empty();
        match outcome {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
    }

    /// Prefix-rooted counterpart to [`Self::with_write_cursor`].
    ///
    /// Paths supplied to the restricted cursor are relative to `path`, and
    /// the cursor cannot ascend above that immutable root.  This narrows the
    /// authority of the ordinary root cursor without exposing raw PathMap,
    /// owned-subtree, graft, or detach operations.
    #[cfg(test)]
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
        let value_count = &mut anchor.value_count;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            operation(GuardedWriteCursor {
                inner: NativePathCursor::new_counted(map, path, &anchor.dispose, value_count),
                dispose: &anchor.dispose,
            })
        }));
        anchor.normalize_live_empty();
        match outcome {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        }
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
        let mut incoming = Some(value);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.with_write_cursor(|mut cursor| {
                cursor.move_to_path(path.as_ref());
                let replaced = cursor.is_val();
                cursor.set_val(incoming.take().unwrap());
                replaced
            })
        }));
        match outcome {
            Ok(replaced) => replaced,
            Err(payload) => {
                if let Some(value) = incoming.take() {
                    self.anchor.dispose.dispose(value);
                }
                resume_unwind(payload)
            }
        }
    }

    /// Creates a value-free path.  Such topology is intentionally preserved
    /// and participates in final iterative teardown.
    #[cfg(test)]
    pub fn create_path<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        self.with_write_cursor(|mut cursor| {
            cursor.move_to_path(path.as_ref());
            cursor.inner.create_path()
        })
    }

    /// Remove one value and then remove its newly dangling path one logical
    /// edge at a time.  Every detached value goes through the disposal policy.
    #[cfg(test)]
    pub(crate) fn remove_val_at_and_prune<K: AsRef<[u8]>>(&mut self, path: K) -> bool {
        let path = path.as_ref();
        if self.get(path).is_none() {
            return false;
        }

        let (candidate, removed, ()) =
            self.rebuild_filter_fold(|candidate_path| candidate_path == path, (), |(), _| ());
        assert_eq!(removed, 1, "the preflighted exact endpoint is removed once");
        *self = candidate;
        true
    }

    /// Remove every value and edge rooted at `prefix` by publishing a freshly
    /// rebuilt, ownership-disjoint trie.  `fold` observes removed values on the
    /// immutable source only after the candidate is complete and validated.
    /// Publishing retires the old authority through the named stack-safe
    /// disposal path once no alias retains it.
    #[cfg(test)]
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
        if self.is_empty() || !self.path_exists(prefix) {
            return (0, initial);
        }

        let (candidate, removed, accumulator) = self.rebuild_filter_fold(
            |candidate_path| candidate_path.starts_with(prefix),
            initial,
            fold,
        );
        *self = candidate;
        (removed, accumulator)
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
    _dispose: &'map D,
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

    #[cfg(test)]
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
}

/// Restricted borrowed write cursor used by Bag bulk updates.
pub(crate) struct GuardedWriteCursor<'map, 'path, V, D, B>
where
    V: Clone + Send + Sync + Unpin + 'static,
    D: DisposeValue<V>,
    B: TeardownBarrier<V> + 'static,
{
    inner: NativePathCursor<'map, 'path, V, D, B>,
    dispose: &'map D,
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
    pub(crate) fn move_to_existing_value(&mut self, path: &[u8]) {
        self.inner.move_to_existing_value(path);
    }

    #[inline]
    pub(crate) fn set_val(&mut self, value: V) {
        if let Some(replaced) = self.inner.set_val(value) {
            self.dispose.dispose(replaced);
        }
    }

    #[inline]
    pub(crate) fn replace_copy(&mut self, value: V)
    where V: Copy {
        self.inner.replace_copy(value);
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn create_path(&mut self) { self.inner.create_path(); }

    #[inline]
    pub(crate) fn descend_first_byte(&mut self) -> bool { self.inner.descend_first_byte() }

    #[inline]
    pub(crate) fn descend_until(&mut self) -> bool { self.inner.descend_until() }

    #[inline]
    pub(crate) fn to_next_sibling_byte(&mut self) -> bool { self.inner.to_next_sibling_byte() }

    #[inline]
    pub(crate) fn ascend_byte(&mut self) -> bool { self.inner.ascend_byte() }

    /// Remove the focused value and prune only newly dangling logical edges.
    #[cfg(test)]
    pub(crate) fn remove_val_and_prune(&mut self) -> bool { self.inner.remove_val_and_prune() }
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

pub fn remove_value_preserving_path<V>(zipper: &mut impl ZipperWriting<V>) -> Option<V>
where V: Clone + Send + Sync + Unpin + 'static {
    if !zipper.is_val() {
        return None;
    }
    let prune = !zipper.at_root() && zipper.child_count() != 0;
    zipper.remove_val(prune)
}

fn advance_to_next_value<V>(zipper: &mut WriteZipperUntracked<'_, '_, V>) -> bool
where V: Clone + Send + Sync + Unpin + 'static {
    loop {
        if zipper.descend_first_byte() {
            if zipper.is_val() {
                return true;
            }
            if zipper.descend_until() && zipper.is_val() {
                return true;
            }
        } else {
            loop {
                if zipper.to_next_sibling_byte() {
                    if zipper.is_val() {
                        return true;
                    }
                    break;
                }
                zipper.ascend_byte();
                if zipper.at_root() {
                    return false;
                }
            }
        }
    }
}

/// Empties `map` in reverse byte/postorder.  Each `prune_ascend` is bounded by
/// a real or synthetic value at the immediate parent, so a detached physical
/// subtrie can never span multiple logical edges.
fn drain_pathmap<V, B, D>(map: &mut PathMap<V>, dispose: &D) -> usize
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
{
    let mut observer = NoopDrainObserver;
    drain_pathmap_observed::<V, B, D, _>(map, dispose, &mut observer)
}

pub fn retire_pathmap<V, B, D>(map: PathMap<V>, dispose: &D, barrier: &B) -> usize
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: BarrierProvider<V>,
    D: DisposeValue<V>,
{
    let mut quarantined = ManuallyDrop::new(map);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let removed =
            drain_pathmap_with_provider(&mut quarantined, dispose, barrier, &mut NoopDrainObserver);
        assert!(quarantined.is_empty());
        unsafe { ManuallyDrop::drop(&mut quarantined) };
        removed
    }));
    match outcome {
        Ok(removed) => removed,
        Err(_) => std::process::abort(),
    }
}

fn descend_to_rightmost_leaf<V>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    suffix: &mut Vec<u8>,
) -> bool
where
    V: Clone + Send + Sync + Unpin + 'static,
{
    suffix.clear();
    {
        let origin = zipper.fork_read_zipper();
        if origin.child_count() == 0 {
            return false;
        }
        let focus: TrieRefBorrowed<'_, V> = origin.trie_ref_at_path([]);
        let mut read = focus.fork_read_zipper();
        #[cfg(test)]
        assert!(
            pathmap::zipper::ZipperAbsolutePath::origin_path(&read).len() <= 48,
            "PathMap 0.3.4 borrowed navigation must not copy the global origin"
        );
        if !read.descend_last_path() {
            return false;
        }
        suffix.extend_from_slice(read.path());
    }
    zipper.descend_to(suffix.as_slice());
    debug_assert!(zipper.path_exists());
    debug_assert_eq!(zipper.child_count(), 0);
    true
}

fn drain_pathmap_observed<V, B, D, O>(map: &mut PathMap<V>, dispose: &D, observer: &mut O) -> usize
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
    O: DrainObserver,
{
    drain_pathmap_with_provider(map, dispose, &StaticBarrier::<B>(PhantomData), observer)
}

fn drain_pathmap_with_provider<V, B, D, O>(
    map: &mut PathMap<V>,
    dispose: &D,
    barrier: &B,
    observer: &mut O,
) -> usize
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: BarrierProvider<V>,
    D: DisposeValue<V>,
    O: DrainObserver,
{
    if map.is_empty() {
        return 0;
    }

    // Phase A: visit real values in canonical byte preorder, remove only the
    // value (never topology), and hand it to the named stack-safe disposer
    // before the next zipper movement.  The cursor is reset and closed before
    // Phase B is allowed to mutate topology.
    let mut removed_values = 0usize;
    {
        let mut zipper = map.write_zipper();
        loop {
            if zipper.is_val() {
                let removed = remove_value_preserving_path(&mut zipper)
                    .expect("a focused real PathMap endpoint contains its value");
                removed_values = removed_values
                    .checked_add(1)
                    .expect("PathMap endpoint count fits usize");
                observer.removed_value();
                dispose.dispose(removed);
            }
            if !advance_to_next_value(&mut zipper) {
                break;
            }
        }
        zipper.reset();
    }
    debug_assert!(
        map.read_zipper().into_iter().next().is_none(),
        "Phase A must close with no remaining real value"
    );

    if map.is_empty() {
        return removed_values;
    }

    // Phase B: the provenance established above is value-free.  Walk topology
    // in reverse byte/postorder and prune exactly one logical edge at a time.
    // Synthetic barriers bound PathMap's prune_ascend without ever entering the
    // real-value disposal policy.
    let stats = {
        let mut zipper = map.write_zipper();
        let mut stats = BarrierPruneStats::default();
        let mut suffix = Vec::new();
        loop {
            descend_to_rightmost_leaf(&mut zipper, &mut suffix);
            if zipper.at_root() && !zipper.is_val() && zipper.child_count() == 0 {
                break;
            }

            debug_assert!(!zipper.is_val(), "Phase B starts from value-free provenance");
            let replaced = zipper.set_val(SyntheticBarrier::new(barrier).into_value());
            debug_assert!(replaced.is_none());
            stats.installed_barriers += 1;
            let step = remove_valued_leaf_and_bubble_with_provider(
                &mut zipper,
                ValuedLeafOrigin::Synthetic,
                None,
                dispose,
                None,
                barrier,
            );
            stats.real_values += step.real_values;
            stats.pruned_edges += step.pruned_edges;
            stats.installed_barriers += step.installed_barriers;
            stats.retired_barriers += step.retired_barriers;
        }
        zipper.reset();
        stats
    };

    debug_assert!(map.is_empty());
    debug_assert_eq!(stats.real_values, 0, "Phase B must never consume a real value");
    for _ in 0..stats.installed_barriers {
        observer.installed_barrier();
    }
    for _ in 0..stats.pruned_edges {
        observer.pruned(1);
    }
    for _ in 0..stats.retired_barriers {
        observer.removed_barrier();
    }
    removed_values
}

#[cfg(test)]
fn remove_valued_leaf_and_bubble<V, B, D>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    origin: ValuedLeafOrigin,
    floor_depth: Option<usize>,
    dispose: &D,
) -> BarrierPruneStats
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
{
    remove_valued_leaf_and_bubble_counted::<V, B, D>(zipper, origin, floor_depth, dispose, None)
}

pub(crate) fn remove_leaf_with_floor<V, D, B>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    floor_depth: usize,
    dispose: &D,
    count: Option<&mut usize>,
) where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
{
    remove_valued_leaf_and_bubble_counted::<V, B, D>(
        zipper,
        ValuedLeafOrigin::Real,
        Some(floor_depth),
        dispose,
        count,
    );
}

fn take_counted_value<V>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    prune: bool,
    count: &mut Option<&mut usize>,
) -> V
where
    V: Clone + Send + Sync + Unpin + 'static,
{
    let next = count.as_ref().map(|count| {
        count
            .checked_sub(1)
            .expect("a removed real value contributes one endpoint")
    });
    let removed = zipper.remove_val(prune);
    if removed.is_some() {
        if let (Some(count), Some(next)) = (count, next) {
            **count = next;
        }
    }
    removed.expect("bounded real pruning starts from a valued leaf")
}

fn remove_valued_leaf_and_bubble_counted<V, B, D>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    origin: ValuedLeafOrigin,
    floor_depth: Option<usize>,
    dispose: &D,
    count: Option<&mut usize>,
) -> BarrierPruneStats
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: TeardownBarrier<V>,
    D: DisposeValue<V>,
{
    remove_valued_leaf_and_bubble_with_provider(
        zipper,
        origin,
        floor_depth,
        dispose,
        count,
        &StaticBarrier::<B>(PhantomData),
    )
}

fn remove_valued_leaf_and_bubble_with_provider<V, B, D>(
    zipper: &mut WriteZipperUntracked<'_, '_, V>,
    mut origin: ValuedLeafOrigin,
    floor_depth: Option<usize>,
    dispose: &D,
    mut count: Option<&mut usize>,
    barrier: &B,
) -> BarrierPruneStats
where
    V: Clone + Send + Sync + Unpin + 'static,
    B: BarrierProvider<V>,
    D: DisposeValue<V>,
{
    debug_assert!(zipper.path_exists());
    debug_assert!(zipper.is_val());
    debug_assert_eq!(zipper.child_count(), 0);
    let mut stats = BarrierPruneStats::default();

    loop {
        if zipper.at_root() {
            let removed = match origin {
                ValuedLeafOrigin::Real => take_counted_value(zipper, false, &mut count),
                ValuedLeafOrigin::Synthetic => zipper
                    .remove_val(false)
                    .expect("the carried synthetic leaf remains present at the root"),
            };
            match origin {
                ValuedLeafOrigin::Real => {
                    dispose.dispose(removed);
                    stats.real_values += 1;
                }
                ValuedLeafOrigin::Synthetic => {
                    barrier.discard_barrier(removed);
                    stats.retired_barriers += 1;
                }
            }
            break;
        }

        let child_byte = *zipper
            .path()
            .last()
            .expect("a non-root valued leaf has a final path byte");
        let child_depth = zipper.path().len();
        let ascended = zipper.ascend_byte();
        debug_assert!(ascended);
        let parent_depth = zipper.path().len();
        debug_assert_eq!(child_depth, parent_depth + 1);
        if let Some(floor_depth) = floor_depth {
            debug_assert!(parent_depth >= floor_depth);
        }
        let reached_floor = floor_depth == Some(parent_depth);
        let parent_had_real_value = zipper.is_val();
        let parent_had_surviving_fork = zipper.child_count() > 1;
        let parent_origin = if parent_had_real_value {
            ValuedLeafOrigin::Real
        } else {
            let replaced = zipper.set_val(SyntheticBarrier::new(barrier).into_value());
            debug_assert!(replaced.is_none());
            stats.installed_barriers += 1;
            ValuedLeafOrigin::Synthetic
        };

        let descended = zipper.descend_to_existing_byte(child_byte);
        debug_assert!(descended);
        debug_assert_eq!(zipper.path().len(), child_depth);
        debug_assert!(zipper.is_val());
        debug_assert_eq!(zipper.child_count(), 0);
        match origin {
            ValuedLeafOrigin::Real => {
                let removed = take_counted_value(zipper, true, &mut count);
                dispose.dispose(removed);
                stats.real_values += 1;
                if zipper.path_exists() {
                    debug_assert!(!zipper.is_val());
                    debug_assert_eq!(zipper.child_count(), 0);
                    let pruned = zipper.prune_ascend();
                    debug_assert_eq!(pruned, 1);
                    stats.pruned_edges += pruned;
                } else {
                    let ascended = zipper.ascend_byte();
                    debug_assert!(ascended);
                    stats.pruned_edges += 1;
                }
            }
            ValuedLeafOrigin::Synthetic => {
                let removed = zipper
                    .remove_val(false)
                    .expect("bounded synthetic pruning starts from a valued leaf");
                barrier.discard_barrier(removed);
                stats.retired_barriers += 1;
                let pruned = zipper.prune_ascend();
                debug_assert_eq!(pruned, 1);
                stats.pruned_edges += pruned;
            }
        }
        debug_assert_eq!(zipper.path().len(), parent_depth);
        debug_assert!(zipper.is_val());

        if reached_floor || parent_had_real_value || parent_had_surviving_fork {
            if matches!(parent_origin, ValuedLeafOrigin::Synthetic) {
                let removed_barrier = remove_value_preserving_path(zipper)
                    .expect("the temporary synthetic parent remains at the stopping frame");
                barrier.discard_barrier(removed_barrier);
                stats.retired_barriers += 1;
            }
            break;
        }

        debug_assert!(matches!(parent_origin, ValuedLeafOrigin::Synthetic));
        debug_assert_eq!(zipper.child_count(), 0);
        origin = ValuedLeafOrigin::Synthetic;
    }

    stats
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
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

    struct VerifyDuringUnwind<F: FnOnce()>(Option<F>);

    impl<F: FnOnce()> Drop for VerifyDuringUnwind<F> {
        fn drop(&mut self) {
            if std::thread::panicking() {
                self.0.take().unwrap()();
            }
        }
    }

    fn assert_guarded_unwind_child(child: &str, diagnostic: &str, marker_bytes: &[u8]) {
        let scratch = shared::rust::test_scratch::acquire("guarded-pathmap-unwind-");
        let marker = scratch.path().join("verified");
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", child, "--nocapture"])
            .env("GUARDED_PATHMAP_UNWIND_MARKER", &marker)
            .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
        let outcome = shared::rust::test_process::run_child_probe(
            &mut command,
            std::time::Duration::from_secs(30),
        )
        .expect("run guarded PathMap unwind child");
        match outcome {
            shared::rust::test_process::ProbeOutcome::Failed(failure) => {
                assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
                assert!(failure.stderr.contains(diagnostic), "{failure:?}");
            }
            shared::rust::test_process::ProbeOutcome::Passed => {
                panic!("guarded PathMap unwind child did not fail")
            }
        }
        assert_eq!(std::fs::read(marker).unwrap(), marker_bytes);
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
    fn live_emptying_normalizes_to_canonical_fresh_root_and_reopens_writes() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);

        guarded.set_val_at([0x11], Probe::tracked(1, &tracking));
        assert!(guarded.remove_val_at_and_prune([0x11]));
        assert!(guarded.is_empty());
        assert_eq!(guarded.iter().count(), 0);
        guarded.with_read_cursor(|cursor| {
            assert_eq!(cursor.val_count(), 0);
            assert_eq!(cursor.child_mask().iter().count(), 0);
        });
        let fresh = guarded_probe_map(&tracking, &order);
        assert_eq!(
            guarded.compact_u64_bytes(|probe| probe.id as u64),
            fresh.compact_u64_bytes(|probe| probe.id as u64),
            "a returned empty live root has the canonical fresh ACT image"
        );

        guarded.set_val_at([0x22, 0x33], Probe::tracked(2, &tracking));
        assert_eq!(guarded.get([0x22, 0x33]).map(|probe| probe.id), Some(2));
        assert!(guarded.remove_val_at_and_prune([0x22, 0x33]));
        assert!(guarded.is_empty());
        assert_eq!(guarded.iter().count(), 0);
        guarded.with_read_cursor(|cursor| {
            assert_eq!(cursor.val_count(), 0);
            assert_eq!(
                cursor.child_mask().iter().count(),
                0,
                "arbitrary empty/insert/remove cycles return a fresh live root"
            );
        });

        guarded.set_val_at([0x44], Probe::tracked(3, &tracking));
        assert_eq!(guarded.get([0x44]).map(|probe| probe.id), Some(3));
    }

    #[test]
    fn unwinding_writer_resets_and_normalizes_before_resuming_the_panic() {
        assert_guarded_unwind_child(
            "rspace::guarded_pathmap::tests::unwinding_writer_child",
            "injected writer unwind after final removal",
            b"normalized-and-reopened",
        );
    }

    #[test]
    #[ignore = "isolated guarded writer unwind child"]
    fn unwinding_writer_child() {
        let Some(marker) = std::env::var_os("GUARDED_PATHMAP_UNWIND_MARKER") else {
            return;
        };
        let marker = std::path::PathBuf::from(marker);
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let guarded = RefCell::new(guarded_probe_map(&tracking, &order));
        guarded
            .borrow_mut()
            .set_val_at([0x11], Probe::tracked(1, &tracking));
        let _verify = VerifyDuringUnwind(Some(|| {
            let mut guarded = guarded.borrow_mut();
            assert!(guarded.is_empty());
            guarded.with_read_cursor(|cursor| {
                assert_eq!(cursor.val_count(), 0);
                assert_eq!(cursor.child_mask().iter().count(), 0);
            });
            guarded.set_val_at([0x22], Probe::tracked(2, &tracking));
            assert_eq!(guarded.get([0x22]).map(|probe| probe.id), Some(2));
            std::fs::write(&marker, b"normalized-and-reopened").unwrap();
        }));
        guarded.borrow_mut().with_write_cursor(|mut cursor| {
            cursor.move_to_path(&[0x11]);
            assert!(cursor.remove_val_and_prune());
            panic!("injected writer unwind after final removal");
        });
    }

    #[test]
    fn coherent_exact_removal_preserves_same_node_sibling_in_both_clone_drop_orders() {
        for drop_snapshot_first in [false, true] {
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let shared_prefix = vec![0xA5; 4_096];
            let mut selected = shared_prefix.clone();
            selected.push(0x11);
            let mut sibling = shared_prefix.clone();
            sibling.push(0x22);

            let mut guarded = guarded_probe_map(&tracking, &order);
            guarded.set_val_at(&selected, Probe::tracked(1, &tracking));
            guarded.set_val_at(&sibling, Probe::tracked(2, &tracking));
            let snapshot = guarded.clone();

            assert!(guarded.remove_val_at_and_prune(&selected));
            assert!(guarded.get(&selected).is_none());
            assert_eq!(guarded.get(&sibling).map(|probe| probe.id), Some(2));
            assert_eq!(snapshot.get(&selected).map(|probe| probe.id), Some(1));
            assert_eq!(snapshot.get(&sibling).map(|probe| probe.id), Some(2));

            if drop_snapshot_first {
                drop(snapshot);
                assert_eq!(guarded.get(&sibling).map(|probe| probe.id), Some(2));
            } else {
                drop(guarded);
                assert_eq!(snapshot.get(&selected).map(|probe| probe.id), Some(1));
                assert_eq!(snapshot.get(&sibling).map(|probe| probe.id), Some(2));
            }
        }
    }

    #[test]
    fn exact_rholang_key_geometry_preserves_every_sibling_order_and_cow_image() {
        for selected_is_lower in [false, true] {
            for selected_inserted_first in [false, true] {
                for shared in [false, true] {
                    let drop_orders: &[bool] = if shared { &[false, true] } else { &[false] };
                    for &drop_snapshot_first in drop_orders {
                        let tracking = Arc::new(ProbeTracking::default());
                        let order = Arc::new(Mutex::new(Vec::new()));
                        let mut selected = vec![0xA5; 939];
                        let mut sibling = selected.clone();
                        if selected_is_lower {
                            selected[203] = 0x11;
                            sibling[203] = 0x22;
                        } else {
                            selected[203] = 0x22;
                            sibling[203] = 0x11;
                        }
                        assert_eq!(
                            selected
                                .iter()
                                .zip(&sibling)
                                .take_while(|(a, b)| a == b)
                                .count(),
                            203
                        );
                        assert_eq!(selected.len() - 203, 736);

                        let mut guarded = guarded_probe_map(&tracking, &order);
                        if selected_inserted_first {
                            guarded.set_val_at(&selected, Probe::tracked(1, &tracking));
                            guarded.set_val_at(&sibling, Probe::tracked(2, &tracking));
                        } else {
                            guarded.set_val_at(&sibling, Probe::tracked(2, &tracking));
                            guarded.set_val_at(&selected, Probe::tracked(1, &tracking));
                        }
                        let snapshot = shared.then(|| guarded.clone());

                        assert!(guarded.remove_val_at_and_prune(&selected));
                        assert!(!guarded.path_exists(&selected));
                        assert_eq!(guarded.get(&sibling).map(|probe| probe.id), Some(2));
                        if let Some(snapshot) = snapshot {
                            assert_eq!(snapshot.get(&selected).map(|probe| probe.id), Some(1));
                            assert_eq!(snapshot.get(&sibling).map(|probe| probe.id), Some(2));
                            if drop_snapshot_first {
                                drop(snapshot);
                                assert_eq!(guarded.get(&sibling).map(|probe| probe.id), Some(2));
                                drop(guarded);
                            } else {
                                drop(guarded);
                                assert_eq!(snapshot.get(&selected).map(|probe| probe.id), Some(1));
                                assert_eq!(snapshot.get(&sibling).map(|probe| probe.id), Some(2));
                                drop(snapshot);
                            }
                        } else {
                            drop(guarded);
                        }
                        assert_eq!(
                            tracking.drops.load(Ordering::Relaxed),
                            2 + tracking.clones.load(Ordering::Relaxed),
                            "every real payload instance is disposed exactly once"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn exact_removal_cleans_real_values_installed_over_explicit_topology() {
        for selected_inserted_first in [false, true] {
            for shared in [false, true] {
                let tracking = Arc::new(ProbeTracking::default());
                let order = Arc::new(Mutex::new(Vec::new()));
                let selected = [99, 57, 210];
                let sibling = [99, 55];
                let mut guarded = guarded_probe_map(&tracking, &order);

                if selected_inserted_first {
                    assert!(guarded.create_path(selected));
                    guarded.set_val_at(selected, Probe::tracked(1, &tracking));
                    guarded.set_val_at(sibling, Probe::tracked(2, &tracking));
                } else {
                    guarded.set_val_at(sibling, Probe::tracked(2, &tracking));
                    assert!(guarded.create_path(selected));
                    guarded.set_val_at(selected, Probe::tracked(1, &tracking));
                }
                let snapshot = shared.then(|| guarded.clone());

                assert!(guarded.remove_val_at_and_prune(selected));
                assert!(!guarded.path_exists(selected));
                assert_eq!(guarded.get(sibling).map(|probe| probe.id), Some(2));
                if let Some(snapshot) = snapshot {
                    assert_eq!(snapshot.get(selected).map(|probe| probe.id), Some(1));
                    assert_eq!(snapshot.get(sibling).map(|probe| probe.id), Some(2));
                    drop(snapshot);
                }
                drop(guarded);
                assert_eq!(
                    tracking.drops.load(Ordering::Relaxed),
                    2 + tracking.clones.load(Ordering::Relaxed)
                );
            }
        }
    }

    #[test]
    fn root_and_dense_children_drain_in_exact_byte_preorder_on_256k_stack() {
        small_stack(|| {
            let tracking = Arc::new(ProbeTracking::default());
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut guarded = guarded_probe_map(&tracking, &order);
            guarded.set_val_at([], Probe::tracked(256, &tracking));
            for byte in 0_u8..=255 {
                guarded.set_val_at([byte], Probe::tracked(byte as usize, &tracking));
            }
            drop(guarded);

            let mut expected = vec![256];
            expected.extend(0..=255);
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
    fn explicit_value_free_sibling_topology_is_pruned_one_parent_bounded_byte_at_a_time() {
        struct UnitBarrier;
        impl TeardownBarrier<()> for UnitBarrier {
            fn make_barrier() {}
        }

        let mut topology = PathMap::<()>::new();
        topology.create_path([99, 57, 210]);
        topology.create_path([99, 55]);
        let stats = drain_with_stats::<_, UnitBarrier, _>(&mut topology, |_| {});
        assert_eq!(stats.values, 0);
        assert_eq!(stats.edges, 4);
        assert_eq!(stats.widest_prune, 1);
        assert!(topology.is_empty());
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
    fn deep_comb_drain_is_stack_bounded_and_byte_preorder() {
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

            assert_eq!(*order.lock().unwrap(), (0..=DEPTH).rev().collect::<Vec<_>>());
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
        assert_eq!(
            *order.lock().unwrap(),
            vec![0, 1, 2, 3],
            "publishing a disjoint candidate retires the unique source in byte preorder"
        );

        drop(guarded);
        assert_eq!(
            tracking.clones.load(Ordering::Relaxed),
            2,
            "the two outside-prefix endpoints are cloned exactly once"
        );
        assert_eq!(
            tracking.drops.load(Ordering::Relaxed),
            4 + tracking.clones.load(Ordering::Relaxed),
            "every original and rebuilt payload instance is disposed exactly once"
        );
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
            assert_eq!(
                tracking.clones.load(Ordering::Relaxed),
                2,
                "only the two outside-prefix endpoints are cloned"
            );
            assert_eq!(
                tracking.drops.load(Ordering::Relaxed),
                DEPTH + 3 + tracking.clones.load(Ordering::Relaxed),
                "every original and retained payload instance is disposed exactly once"
            );
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
        assert_eq!(
            tracking.clones.load(Ordering::Relaxed),
            7,
            "each structural rebuild clones exactly its retained endpoints"
        );
        assert_eq!(
            tracking.drops.load(Ordering::Relaxed),
            5 + tracking.clones.load(Ordering::Relaxed),
            "every original and rebuilt payload instance is disposed exactly once"
        );
    }

    #[test]
    fn exact_removal_of_an_internal_value_preserves_all_descendants() {
        let tracking = Arc::new(ProbeTracking::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut guarded = guarded_probe_map(&tracking, &order);
        guarded.set_val_at([1], Probe::tracked(1, &tracking));
        guarded.set_val_at([1, 2], Probe::tracked(2, &tracking));
        guarded.set_val_at([1, 3, 4], Probe::tracked(3, &tracking));

        assert!(guarded.remove_val_at_and_prune([1]));
        assert!(guarded.get([1]).is_none());
        assert!(guarded.path_exists([1]));
        assert_eq!(guarded.get([1, 2]).map(|probe| probe.id), Some(2));
        assert_eq!(guarded.get([1, 3, 4]).map(|probe| probe.id), Some(3));
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
    fn value_free_prefix_drain_never_holds_more_than_two_synthetic_barriers() {
        static ACTIVE: AtomicUsize = AtomicUsize::new(0);
        static PEAK: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone)]
        struct BarrierValue {
            id: usize,
            synthetic: bool,
        }

        struct PeakBarrier;
        impl TeardownBarrier<BarrierValue> for PeakBarrier {
            fn make_barrier() -> BarrierValue {
                let active = ACTIVE.fetch_add(1, Ordering::SeqCst) + 1;
                PEAK.fetch_max(active, Ordering::SeqCst);
                BarrierValue {
                    id: usize::MAX,
                    synthetic: true,
                }
            }

            fn discard_barrier(barrier: BarrierValue) {
                assert!(barrier.synthetic);
                assert!(ACTIVE.fetch_sub(1, Ordering::SeqCst) > 0);
            }
        }

        ACTIVE.store(0, Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        let dispose = TestDisposer(|value: BarrierValue| {
            assert!(!value.synthetic);
            assert_eq!(ACTIVE.load(Ordering::SeqCst), 0);
        });
        let mut guarded = GuardedPathMap::<BarrierValue, _, PeakBarrier>::new_with_barrier(dispose);
        guarded.set_val_at([3], BarrierValue {
            id: 3,
            synthetic: false,
        });
        guarded.set_val_at([4, 7], BarrierValue {
            id: 7,
            synthetic: false,
        });
        assert!(guarded.create_path([4, 9, 8]));
        guarded.set_val_at([5], BarrierValue {
            id: 5,
            synthetic: false,
        });

        let (removed, ids) = guarded.remove_prefix_fold([4], Vec::new(), |mut ids, value| {
            assert_eq!(ACTIVE.load(Ordering::SeqCst), 0);
            ids.push(value.id);
            ids
        });
        assert_eq!(removed, 1);
        assert_eq!(ids, vec![7]);
        assert!(!guarded.path_exists([4]));
        assert_eq!(guarded.get([3]).map(|value| value.id), Some(3));
        assert_eq!(guarded.get([5]).map(|value| value.id), Some(5));
        assert_eq!(ACTIVE.load(Ordering::SeqCst), 0);
        assert!(
            PEAK.load(Ordering::SeqCst) <= 2,
            "the explicit prefix floor permits at most a child and temporary floor barrier"
        );
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
            Event::Install,
            Event::Install,
            Event::Prune(1),
            Event::Prune(1),
            Event::Remove,
            Event::Remove,
            Event::Remove,
        ]);
        assert!(map.is_empty());
    }

    #[test]
    fn payload_and_observer_callbacks_never_run_while_a_synthetic_barrier_is_live() {
        static ACTIVE_BARRIERS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone)]
        struct BarrierValue {
            synthetic: bool,
        }

        struct CountingBarrier;
        impl TeardownBarrier<BarrierValue> for CountingBarrier {
            fn make_barrier() -> BarrierValue {
                ACTIVE_BARRIERS.fetch_add(1, Ordering::SeqCst);
                BarrierValue { synthetic: true }
            }

            fn discard_barrier(barrier: BarrierValue) {
                assert!(barrier.synthetic);
                assert!(ACTIVE_BARRIERS.fetch_sub(1, Ordering::SeqCst) > 0);
            }
        }

        struct BarrierFreeObserver;
        impl DrainObserver for BarrierFreeObserver {
            fn removed_value(&mut self) {
                assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
            }

            fn pruned(&mut self, _width: usize) {
                assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
            }

            fn installed_barrier(&mut self) {
                assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
            }

            fn removed_barrier(&mut self) {
                assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
            }
        }

        ACTIVE_BARRIERS.store(0, Ordering::SeqCst);
        let mut map = PathMap::new();
        map.set_val_at([1, 2], BarrierValue { synthetic: false });
        let dispose = TestDisposer(|value: BarrierValue| {
            assert!(!value.synthetic);
            assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
        });
        let mut observer = BarrierFreeObserver;
        drain_pathmap_observed::<BarrierValue, CountingBarrier, _, _>(
            &mut map,
            &dispose,
            &mut observer,
        );
        assert_eq!(ACTIVE_BARRIERS.load(Ordering::SeqCst), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn panicking_disposer_clone_precedes_raw_header_clone() {
        assert_guarded_unwind_child(
            "rspace::guarded_pathmap::tests::panicking_disposer_clone_child",
            "injected disposer-clone failure",
            b"clone-refused-before-header",
        );
    }

    #[test]
    #[ignore = "isolated disposer clone unwind child"]
    fn panicking_disposer_clone_child() {
        let Some(marker) = std::env::var_os("GUARDED_PATHMAP_UNWIND_MARKER") else {
            return;
        };
        let marker = std::path::PathBuf::from(marker);
        struct PanicOnClone {
            attempts: Arc<AtomicUsize>,
        }

        impl Clone for PanicOnClone {
            fn clone(&self) -> Self {
                self.attempts.fetch_add(1, Ordering::Relaxed);
                panic!("injected disposer-clone failure");
            }
        }

        small_stack(move || {
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
            let maps = RefCell::new(Some((left, right)));
            let _verify = VerifyDuringUnwind(Some(|| {
                let (left, right) = maps.borrow_mut().take().unwrap();
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
                assert_eq!(disposed.load(Ordering::Relaxed), 1);
                drop(left);
                assert_eq!(disposed.load(Ordering::Relaxed), 1);
                drop(right);
                assert_eq!(disposed.load(Ordering::Relaxed), 2);
                assert_eq!(tracking.drops.load(Ordering::Relaxed), 2);
                std::fs::write(&marker, b"clone-refused-before-header").unwrap();
            }));
            maps.borrow_mut()
                .as_mut()
                .unwrap()
                .0
                .set_val_at([1], Probe::tracked(2, &tracking));
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
            expected_values.sort();

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

#[cfg(test)]
#[path = "../../tests/guarded_pathmap/two_phase_teardown_properties.rs"]
mod two_phase_teardown_properties;

#[cfg(test)]
#[path = "../../tests/guarded_pathmap/stateful_retirement.rs"]
mod stateful_retirement;
