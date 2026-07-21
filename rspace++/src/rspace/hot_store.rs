use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
#[cfg(test)]
use proptest::prelude::*;
#[cfg(test)]
use rand::Rng;
use tracing::warn;

use super::errors::RSpaceError;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, HotStoreAction, InsertAction,
    InsertContinuations, InsertData, InsertJoins,
};
use crate::rspace::internal::{Datum, Row, WaitingContinuation};
use crate::rspace::metrics_constants::{
    HOT_STORE_GET_CONT_CALLS_METRIC, HOT_STORE_GET_CONT_HISTORY_FILL_METRIC,
    HOT_STORE_GET_DATA_CALLS_METRIC, HOT_STORE_GET_DATA_HISTORY_FILL_METRIC,
    HOT_STORE_GET_JOINS_CALLS_METRIC, HOT_STORE_GET_JOINS_HISTORY_FILL_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC,
    HOT_STORE_HISTORY_CACHE_BULK_CLEAR_JOINS_METRIC, HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC,
    HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, HOT_STORE_PUT_CONT_CALLS_METRIC,
    HOT_STORE_PUT_CONT_DUPLICATES_METRIC, HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC,
    HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, HOT_STORE_PUT_CONT_IDENTITY_BUILD_NS_METRIC,
    HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, HOT_STORE_PUT_CONT_TIME_NS_METRIC,
    HOT_STORE_PUT_DATUM_CALLS_METRIC, HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC,
    HOT_STORE_PUT_DATUM_TIME_NS_METRIC, HOT_STORE_PUT_JOIN_CALLS_METRIC,
    HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, HOT_STORE_PUT_JOIN_TIME_NS_METRIC,
    HOT_STORE_STATE_CONT_ITEMS_METRIC, HOT_STORE_STATE_CONT_SIZE_METRIC,
    HOT_STORE_STATE_DATA_ITEMS_METRIC, HOT_STORE_STATE_DATA_SIZE_METRIC,
    HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC,
    HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC,
    HOT_STORE_STATE_JOINS_ITEMS_METRIC, HOT_STORE_STATE_JOINS_SIZE_METRIC, RSPACE_METRICS_SOURCE,
};

const MAX_HISTORY_STORE_CACHE_ENTRIES: usize = 512;
const MAX_HISTORY_STORE_CACHE_CONT_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_DATA_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_JOIN_ITEMS: usize = 8192;
const HOT_STORE_STATE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;
const HOT_STORE_HISTORY_CACHE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;

// Number of independent shards backing each of the five HotStore state maps
// (data, continuations, installed_continuations, joins, installed_joins).
// Matches the 256-way striped-lock scheme already used for phase_a_locks /
// phase_b_locks in rspace.rs / replay_rspace.rs (PR #72 fix #4) -- same
// tradeoff: coarser than per-key, but contention stays negligible in
// practice while keeping the shard count (and therefore snapshot cost)
// bounded and small.
const NUM_SHARDS: usize = 256;

fn shard_of<K: Hash>(k: &K) -> usize {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut hasher);
    (hasher.finish() as usize) % NUM_SHARDS
}

// Sharded persistent map backing InMemHotStore's five state fields.
//
// Investigation: f1r3node-rust#43 (see issue-43 branch / asi-chain-testbed
// issues/01-rholang-execution-scales-backwards-with-cpu.md, Update
// 2026-07-21c). `create_soft_checkpoint()` -- called unconditionally before
// every user deploy, purely to have a rollback point on failure -- used to
// call `HotStore::snapshot()`, which deep-cloned every entry in all five
// DashMaps into plain HashMaps: O(total store size), not O(deploy work).
// Confirmed empirically (rspace++/tests/concurrent_rspace_test.rs,
// `soft_checkpoint_cost_scales_with_accumulated_store_size_not_deploy_size`):
// 100x more pre-existing store state cost ~31x more time per snapshot call,
// with zero additional deploy work -- exactly the mechanism behind EVAL ms
// growing with block size on real replay.
//
// Fix: back each state map with NUM_SHARDS independent `imbl::HashMap`
// (persistent, structural-sharing) slots instead of a `DashMap`. Mutation
// (produce/consume) goes through one shard's write lock and does an
// insert/remove against that shard's persistent map -- O(log n) instead of
// DashMap's amortized O(1), but bounded and small, and never blocks other
// shards. `snapshot()` reads each shard's *current* persistent-map value
// directly -- an O(1) refcount-bump clone per shard -- so total snapshot
// cost is O(NUM_SHARDS), independent of how many entries the store holds.
// This is the same asymmetric trade PR #72 already made elsewhere in this
// file (e.g. fix #3: O(n) history-cache bounds scan -> O(1) atomic
// counters): a small, bounded cost on the hot mutation path in exchange for
// eliminating an unbounded cost on a path that runs once per deploy.
struct ShardedMap<K, V> {
    shards: Vec<std::sync::RwLock<imbl::HashMap<K, V>>>,
}

#[allow(dead_code)]
impl<K, V> ShardedMap<K, V>
where
    K: Clone + Hash + Eq,
    V: Clone,
{
    fn new() -> Self {
        Self {
            shards: (0..NUM_SHARDS).map(|_| std::sync::RwLock::new(imbl::HashMap::new())).collect(),
        }
    }

    fn from_shards(shards: Vec<imbl::HashMap<K, V>>) -> Self {
        debug_assert_eq!(shards.len(), NUM_SHARDS);
        Self {
            shards: shards.into_iter().map(std::sync::RwLock::new).collect(),
        }
    }

    fn get(&self, k: &K) -> Option<V> {
        self.shards[shard_of(k)].read().expect("shard read lock").get(k).cloned()
    }

    fn contains_key(&self, k: &K) -> bool {
        self.shards[shard_of(k)].read().expect("shard read lock").contains_key(k)
    }

    fn insert(&self, k: K, v: V) -> Option<V> {
        let idx = shard_of(&k);
        self.shards[idx].write().expect("shard write lock").insert(k, v)
    }

    fn remove(&self, k: &K) -> Option<V> {
        self.shards[shard_of(k)].write().expect("shard write lock").remove(k)
    }

    // Runs a read-modify-write closure against the shard holding `k`. Covers
    // the DashMap::entry() Occupied/Vacant pattern used throughout the
    // original produce/consume methods -- one shard-scoped write lock, one
    // in-place (COW) mutation of that shard's persistent map.
    fn with_entry<R>(&self, k: &K, f: impl FnOnce(&mut imbl::HashMap<K, V>) -> R) -> R {
        let idx = shard_of(k);
        let mut guard = self.shards[idx].write().expect("shard write lock");
        f(&mut guard)
    }

    fn len(&self) -> usize {
        self.shards.iter().map(|s| s.read().expect("shard read lock").len()).sum()
    }

    fn clear(&self) {
        for s in &self.shards {
            *s.write().expect("shard write lock") = imbl::HashMap::new();
        }
    }

    // O(NUM_SHARDS): clone each shard's current persistent map. This is the
    // whole point of the sharded design -- see the type-level doc comment.
    fn snapshot_shards(&self) -> Vec<imbl::HashMap<K, V>> {
        self.shards.iter().map(|s| s.read().expect("shard read lock").clone()).collect()
    }

    // O(NUM_SHARDS): atomically (per-shard) replace this map's contents with
    // a previously captured sharded snapshot.
    fn restore_shards(&self, shards: Vec<imbl::HashMap<K, V>>) {
        debug_assert_eq!(shards.len(), NUM_SHARDS);
        for (slot, new_map) in self.shards.iter().zip(shards) {
            *slot.write().expect("shard write lock") = new_map;
        }
    }

    // Materializes every entry across all shards. O(total entries) -- only
    // for non-hot paths (changes(), to_map(), print(), metrics), never on
    // the per-deploy checkpoint path.
    fn iter_flat(&self) -> Vec<(K, V)> {
        self.shards
            .iter()
            .flat_map(|s| {
                s.read()
                    .expect("shard read lock")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
pub trait HotStore<C: Clone + Hash + Eq, P: Clone, A: Clone, K: Clone>: Sync + Send {
    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>>;
    // Hot-path variant: returns Arc-shared continuations so the matcher can
    // probe patterns without deep-cloning the continuation body.
    fn get_continuations_arc(&self, channels: &[C]) -> Vec<Arc<WaitingContinuation<P, K>>>;
    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool>;
    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()>;
    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()>;

    fn get_data(&self, channel: &C) -> Vec<Datum<A>>;
    fn put_datum(&self, channel: &C, d: Datum<A>) -> ();
    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError>;

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>>;
    fn put_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn install_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()>;

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>>;
    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>>;
    fn snapshot(&self) -> HotStoreState<C, P, A, K>;

    fn print(&self) -> ();
    fn clear(&self) -> ();

    // See rspace/src/test/scala/coop/rchain/rspace/test/package.scala
    fn is_empty(&self) -> bool;

    fn set_state(&self, state: HotStoreState<C, P, A, K>);
}

pub fn new_hashmap<K: std::cmp::Eq + std::hash::Hash, V>() -> HashMap<K, V> { HashMap::new() }

// Sharded snapshot of InMemHotStore's five state maps. Each field is
// NUM_SHARDS independent `imbl::HashMap`s -- the same partitioning the live
// store uses internally -- so cloning/restoring a `HotStoreState` (what
// `create_soft_checkpoint`/`revert_to_soft_checkpoint` do on every deploy)
// costs O(NUM_SHARDS), not O(total entries). See `ShardedMap`'s doc comment
// for the full rationale.
//
// `continuations` keeps values `Arc`-wrapped (matching the live store's
// representation, see PR #72 fix #5) so building a snapshot never
// deep-clones a continuation body; callers that need an owned
// `WaitingContinuation` (e.g. `to_map()`, FFI/RPC-facing code) deref-clone
// at the point of use instead of paying that cost on every checkpoint.
#[derive(Debug, Clone)]
pub struct HotStoreState<C, P, A, K>
where
    C: Eq + Hash,
    A: Clone,
    P: Clone,
    K: Clone,
{
    pub continuations: Vec<imbl::HashMap<Vec<C>, Vec<Arc<WaitingContinuation<P, K>>>>>,
    pub installed_continuations: Vec<imbl::HashMap<Vec<C>, WaitingContinuation<P, K>>>,
    pub data: Vec<imbl::HashMap<C, Vec<Datum<A>>>>,
    pub joins: Vec<imbl::HashMap<C, Vec<Vec<C>>>>,
    pub installed_joins: Vec<imbl::HashMap<C, Vec<Vec<C>>>>,
}

impl<C, P, A, K> Default for HotStoreState<C, P, A, K>
where
    C: Eq + Hash + Clone,
    A: Clone,
    P: Clone,
    K: Clone,
{
    fn default() -> Self {
        HotStoreState {
            continuations: (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect(),
            installed_continuations: (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect(),
            data: (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect(),
            joins: (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect(),
            installed_joins: (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect(),
        }
    }
}

impl<C, P, A, K> HotStoreState<C, P, A, K>
where
    C: Eq + Hash + Clone,
    A: Clone,
    P: Clone,
    K: Clone,
{
    // Builds a sharded HotStoreState from plain flat maps -- the shape
    // tests and other call sites naturally construct fixture data in.
    // Shards each flat map by key, same as the live ShardedMap would.
    pub fn from_flat_maps(
        continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
        installed_continuations: HashMap<Vec<C>, WaitingContinuation<P, K>>,
        data: HashMap<C, Vec<Datum<A>>>,
        joins: HashMap<C, Vec<Vec<C>>>,
        installed_joins: HashMap<C, Vec<Vec<C>>>,
    ) -> Self {
        fn shard_flat<K: Clone + Hash + Eq, V: Clone>(flat: HashMap<K, V>) -> Vec<imbl::HashMap<K, V>> {
            let mut shards: Vec<imbl::HashMap<K, V>> =
                (0..NUM_SHARDS).map(|_| imbl::HashMap::new()).collect();
            for (k, v) in flat {
                let idx = shard_of(&k);
                shards[idx].insert(k, v);
            }
            shards
        }

        HotStoreState {
            continuations: shard_flat(
                continuations
                    .into_iter()
                    .map(|(k, v)| (k, v.into_iter().map(Arc::new).collect()))
                    .collect(),
            ),
            installed_continuations: shard_flat(installed_continuations),
            data: shard_flat(data),
            joins: shard_flat(joins),
            installed_joins: shard_flat(installed_joins),
        }
    }

    // Flattens a sharded field back into a plain map -- O(total entries),
    // for test assertions and other cold-path consumers only.
    pub fn continuations_flat(&self) -> HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>> {
        self.continuations
            .iter()
            .flat_map(|shard| {
                shard
                    .iter()
                    .map(|(k, v)| (k.clone(), v.iter().map(|a| (**a).clone()).collect()))
            })
            .collect()
    }

    pub fn installed_continuations_flat(&self) -> HashMap<Vec<C>, WaitingContinuation<P, K>> {
        self.installed_continuations
            .iter()
            .flat_map(|shard| shard.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect()
    }

    pub fn data_flat(&self) -> HashMap<C, Vec<Datum<A>>> {
        self.data
            .iter()
            .flat_map(|shard| shard.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect()
    }

    pub fn joins_flat(&self) -> HashMap<C, Vec<Vec<C>>> {
        self.joins
            .iter()
            .flat_map(|shard| shard.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect()
    }

    pub fn installed_joins_flat(&self) -> HashMap<C, Vec<Vec<C>>> {
        self.installed_joins
            .iter()
            .flat_map(|shard| shard.iter().map(|(k, v)| (k.clone(), v.clone())))
            .collect()
    }
}

// This impl is needed for hot_store_spec.rs
#[cfg(test)]
impl<C, P, A, K> HotStoreState<C, P, A, K>
where
    C: Eq + Hash + Debug + Arbitrary + Default + Clone,
    A: Clone + Debug + Arbitrary + Default,
    P: Clone + Debug + Arbitrary + Default,
    K: Clone + Debug + Arbitrary + Default,
{
    fn random_vec<T>(size: usize) -> Vec<T>
    where T: Default + Clone {
        let mut rng = rand::rng();
        (0..size)
            .map(|_| T::default())
            .collect::<Vec<T>>()
            .iter()
            .take(rng.random_range(0..size + 1))
            .cloned()
            .collect()
    }

    pub fn random_state() -> Self {
        let channels: Vec<C> = HotStoreState::<C, P, A, K>::random_vec(10);
        let continuations: Vec<WaitingContinuation<P, K>> =
            HotStoreState::<C, P, A, K>::random_vec(10);
        let installed_continuations = WaitingContinuation::default();
        let data: Vec<Datum<A>> = HotStoreState::<C, P, A, K>::random_vec(10);
        let channel = C::default();
        let joins: Vec<Vec<C>> = HotStoreState::<C, P, A, K>::random_vec(10);
        let installed_joins: Vec<Vec<C>> = HotStoreState::<C, P, A, K>::random_vec(10);

        HotStoreState::from_flat_maps(
            HashMap::from_iter(vec![(channels.clone(), continuations.clone())]),
            HashMap::from_iter(vec![(channels.clone(), installed_continuations.clone())]),
            HashMap::from_iter(vec![(channel.clone(), data.clone())]),
            HashMap::from_iter(vec![(channel.clone(), joins)]),
            HashMap::from_iter(vec![(channel, installed_joins)]),
        )
    }
}

struct InMemHotStore<C, P, A, K>
where
    C: Eq + Hash + Sync + Send,
    A: Clone + Sync + Send,
    P: Clone + Sync + Send,
    K: Clone + Sync + Send,
{
    // Hot path: per-key concurrent access via sharded persistent maps.
    // All produce/consume operations during deploy execution use these
    // directly without any global lock -- see `ShardedMap`'s doc comment
    // for why this replaced a plain DashMap.
    data: ShardedMap<C, Vec<Datum<A>>>,
    // Continuations are stored behind Arc so the produce/consume matcher can
    // probe them (get_continuations_arc) with a cheap pointer bump instead of
    // deep-cloning the continuation body K on every operation. The continuation
    // body of a persistent contract was being cloned once per produce on its
    // channel — O(total_produces) deep clones.
    continuations: ShardedMap<Vec<C>, Vec<Arc<WaitingContinuation<P, K>>>>,
    installed_continuations: ShardedMap<Vec<C>, WaitingContinuation<P, K>>,
    joins: ShardedMap<C, Vec<Vec<C>>>,
    installed_joins: ShardedMap<C, Vec<Vec<C>>>,
    history_cache_continuations: DashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    history_cache_datums: DashMap<C, Vec<Datum<A>>>,
    history_cache_joins: DashMap<C, Vec<Vec<C>>>,
    // Atomic item counters for O(1) cache bounds checking.
    // Updated on every insert/evict so enforce_history_cache_bounds never
    // has to iterate the DashMaps (which was 44% of CPU per the flame graph).
    history_cache_cont_items: AtomicUsize,
    history_cache_data_items: AtomicUsize,
    history_cache_joins_items: AtomicUsize,
    // Checkpoint path: acquired only by snapshot/changes/set_state/clear/to_map
    // which need a consistent view of all five maps at once. These are called at the
    // end of a deploy after all par-branches have joined, so the lock is never
    // contended during normal execution — it only guards the rare whole-state ops.
    checkpoint_lock: std::sync::Mutex<()>,
    history_reader_base: Box<dyn HistoryReaderBase<C, P, A, K>>,
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
impl<C, P, A, K> HotStore<C, P, A, K> for InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + Send + Sync,
    P: Clone + Debug + Send + Sync,
    A: Clone + Debug + Send + Sync,
    K: Clone + Debug + Send + Sync,
{
    fn snapshot(&self) -> HotStoreState<C, P, A, K> {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        HotStoreState {
            continuations: self.continuations.snapshot_shards(),
            installed_continuations: self.installed_continuations.snapshot_shards(),
            data: self.data.snapshot_shards(),
            joins: self.joins.snapshot_shards(),
            installed_joins: self.installed_joins.snapshot_shards(),
        }
    }

    // Continuations

    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        // Compat path: deref-clone the Arc-shared continuations into owned values.
        // Used by external/FFI/test callers; the hot matcher path uses the _arc
        // variant and never deep-clones here.
        self.get_continuations_arc(channels)
            .into_iter()
            .map(|a| (*a).clone())
            .collect()
    }

    fn get_continuations_arc(&self, channels: &[C]) -> Vec<Arc<WaitingContinuation<P, K>>> {
        metrics::counter!(HOT_STORE_GET_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let channels_vec = channels.to_vec();
        // Cloning a Vec<Arc<_>> is just refcount bumps — no continuation-body clone.
        let continuations = self.continuations.get(&channels_vec);
        let installed = self.installed_continuations.get(&channels_vec).map(Arc::new);

        match (continuations, installed) {
            (Some(conts), Some(inst)) => {
                let mut result = Vec::with_capacity(conts.len() + 1);
                result.push(inst);
                result.extend(conts);
                result
            }
            (Some(conts), None) => conts,
            (None, Some(inst)) => {
                metrics::counter!(HOT_STORE_GET_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history: Vec<Arc<WaitingContinuation<P, K>>> = self
                    .get_cont_from_history_store(channels)
                    .into_iter()
                    .map(Arc::new)
                    .collect();
                self.continuations.insert(channels_vec, from_history.clone());
                let mut result = Vec::with_capacity(from_history.len() + 1);
                result.push(inst);
                result.extend(from_history);
                result
            }
            (None, None) => {
                metrics::counter!(HOT_STORE_GET_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history: Vec<Arc<WaitingContinuation<P, K>>> = self
                    .get_cont_from_history_store(channels)
                    .into_iter()
                    .map(Arc::new)
                    .collect();
                self.continuations.insert(channels_vec, from_history.clone());
                from_history
            }
        }
    }

    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool> {
        let __put_start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);

        let __ident_build_start = std::time::Instant::now();
        let wc_identity = Self::continuation_identity(&wc);
        metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_BUILD_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__ident_build_start.elapsed().as_nanos() as u64);

        let channels_vec = channels.to_vec();
        let mut inserted = false;
        self.continuations.with_entry(&channels_vec, |map| {
            match map.get(&channels_vec).cloned() {
                Some(mut existing) => {
                    let existing_count = existing.len() as u64;
                    metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(existing_count);
                    let __cmp_start = std::time::Instant::now();
                    let dup = existing
                        .iter()
                        .any(|e| Self::continuation_identity(e.as_ref()) == wc_identity);
                    metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(__cmp_start.elapsed().as_nanos() as u64);
                    if !dup {
                        existing.insert(0, Arc::new(wc));
                        inserted = true;
                        map.insert(channels_vec.clone(), existing);
                    } else {
                        metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                            .increment(1);
                    }
                }
                None => {
                    metrics::counter!(HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                    let mut new_continuations: Vec<Arc<WaitingContinuation<P, K>>> = self
                        .get_cont_from_history_store(channels)
                        .into_iter()
                        .map(Arc::new)
                        .collect();
                    let existing_count = new_continuations.len() as u64;
                    metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(existing_count);
                    let __cmp_start = std::time::Instant::now();
                    let dup = new_continuations
                        .iter()
                        .any(|e| Self::continuation_identity(e.as_ref()) == wc_identity);
                    metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(__cmp_start.elapsed().as_nanos() as u64);
                    if !dup {
                        new_continuations.insert(0, Arc::new(wc));
                        inserted = true;
                    } else {
                        metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                            .increment(1);
                    }
                    map.insert(channels_vec.clone(), new_continuations);
                }
            }
        });
        metrics::counter!(HOT_STORE_PUT_CONT_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__put_start.elapsed().as_nanos() as u64);
        Some(inserted)
    }

    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()> {
        self.installed_continuations.insert(channels.to_vec(), wc);
        Some(())
    }

    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()> {
        let channels_vec = channels.to_vec();
        let is_installed = self.installed_continuations.contains_key(&channels_vec);
        let removing_installed = is_installed && index == 0;
        let removed_index = if is_installed { index - 1 } else { index };

        if removing_installed {
            warn!("Attempted to remove an installed continuation");
            return None;
        }

        self.continuations.with_entry(&channels_vec, |map| {
            match map.get(&channels_vec).cloned() {
                Some(mut existing) => {
                    let len = existing.len();
                    let out_of_bounds = removed_index < 0 || removed_index as usize >= len;
                    if out_of_bounds {
                        warn!(index, "Index out of bounds when removing continuation");
                        None
                    } else {
                        existing.remove(removed_index as usize);
                        map.insert(channels_vec.clone(), existing);
                        Some(())
                    }
                }
                None => {
                    let mut from_history: Vec<Arc<WaitingContinuation<P, K>>> = self
                        .get_cont_from_history_store(channels)
                        .into_iter()
                        .map(Arc::new)
                        .collect();
                    let len = from_history.len();
                    let out_of_bounds = removed_index < 0 || removed_index as usize >= len;
                    if out_of_bounds {
                        warn!(index, "Index out of bounds when removing continuation");
                        map.insert(channels_vec.clone(), from_history);
                        None
                    } else {
                        from_history.remove(removed_index as usize);
                        map.insert(channels_vec.clone(), from_history);
                        Some(())
                    }
                }
            }
        })
    }

    // Data

    fn get_data(&self, channel: &C) -> Vec<Datum<A>> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        if let Some(data) = self.data.get(channel) {
            data
        } else {
            metrics::counter!(HOT_STORE_GET_DATA_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            let data = self.get_data_from_history_store(channel);
            self.data.insert(channel.clone(), data.clone());
            data
        }
    }

    fn put_datum(&self, channel: &C, d: Datum<A>) {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_DATUM_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        self.data.with_entry(channel, |map| match map.get(channel).cloned() {
            Some(mut existing) => {
                existing.insert(0, d);
                map.insert(channel.clone(), existing);
            }
            None => {
                metrics::counter!(HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let mut new_data = self.get_data_from_history_store(channel);
                new_data.insert(0, d);
                map.insert(channel.clone(), new_data);
            }
        });
        metrics::counter!(HOT_STORE_PUT_DATUM_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
    }

    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError> {
        self.data.with_entry(channel, |map| match map.get(channel).cloned() {
            Some(mut existing) => {
                let out_of_bounds = index < 0 || index as usize >= existing.len();
                if out_of_bounds {
                    Err(RSpaceError::BugFoundError(format!(
                        "Index {} out of bounds when removing datum (len={})",
                        index,
                        existing.len()
                    )))
                } else {
                    existing.remove(index as usize);
                    map.insert(channel.clone(), existing);
                    Ok(())
                }
            }
            None => {
                let mut from_history = self.get_data_from_history_store(channel);
                let out_of_bounds = index < 0 || index as usize >= from_history.len();
                if out_of_bounds {
                    let len = from_history.len();
                    map.insert(channel.clone(), from_history);
                    Err(RSpaceError::BugFoundError(format!(
                        "Index {} out of bounds when removing datum (len={})",
                        index, len
                    )))
                } else {
                    from_history.remove(index as usize);
                    map.insert(channel.clone(), from_history);
                    Ok(())
                }
            }
        })
    }

    // Joins

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>> {
        metrics::counter!(HOT_STORE_GET_JOINS_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let joins = self.joins.get(channel);
        let installed_joins = self.installed_joins.get(channel);

        match joins {
            Some(joins_data) => {
                let mut result = Vec::new();
                if let Some(installed) = installed_joins {
                    result.extend(installed);
                }
                result.extend(joins_data);
                result
            }
            None => {
                metrics::counter!(HOT_STORE_GET_JOINS_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history = self.get_joins_from_history_store(channel);
                self.joins.insert(channel.clone(), from_history.clone());
                let mut result = Vec::new();
                if let Some(installed) = installed_joins {
                    result.extend(installed);
                }
                result.extend(from_history);
                result
            }
        }
    }

    fn put_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_JOIN_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        self.joins.with_entry(channel, |map| match map.get(channel).cloned() {
            Some(mut existing) => {
                if !existing.iter().any(|j| j.as_slice() == join) {
                    existing.insert(0, join.to_vec());
                    map.insert(channel.clone(), existing);
                }
            }
            None => {
                metrics::counter!(HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let mut joins = self.get_joins_from_history_store(channel);
                if !joins.iter().any(|j| j.as_slice() == join) {
                    joins.insert(0, join.to_vec());
                }
                map.insert(channel.clone(), joins);
            }
        });
        metrics::counter!(HOT_STORE_PUT_JOIN_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
        Some(())
    }

    fn install_join(&self, channel: &C, join: &[C]) -> Option<()> {
        self.installed_joins.with_entry(channel, |map| match map.get(channel).cloned() {
            Some(mut existing) => {
                if !existing.iter().any(|j| j.as_slice() == join) {
                    existing.insert(0, join.to_vec());
                    map.insert(channel.clone(), existing);
                }
            }
            None => {
                map.insert(channel.clone(), vec![join.to_vec()]);
            }
        });
        Some(())
    }

    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let has_join_in_state = self.joins.contains_key(channel);
        let join_vec = join.to_vec();
        // Only emptiness matters here — avoid deep-cloning continuations.
        let do_remove = if self.installed_continuations.contains_key(&join_vec) {
            false
        } else {
            match self.continuations.get(&join_vec).map(|v| v.len()) {
                Some(len) => len == 0,
                None => self.get_cont_from_history_store(join).is_empty(),
            }
        };

        if !do_remove {
            if !has_join_in_state {
                let joins_in_history = self.get_joins_from_history_store(channel);
                self.joins.insert(channel.clone(), joins_in_history);
            }
            Some(())
        } else {
            self.joins.with_entry(channel, |map| match map.get(channel).cloned() {
                Some(mut existing) => {
                    if let Some(idx) = existing.iter().position(|x| x.as_slice() == join) {
                        existing.remove(idx);
                    } else {
                        warn!("Join not found when removing join");
                    }
                    map.insert(channel.clone(), existing);
                    Some(())
                }
                None => {
                    let mut joins_in_history = self.get_joins_from_history_store(channel);
                    if let Some(idx) = joins_in_history.iter().position(|x| x.as_slice() == join) {
                        joins_in_history.remove(idx);
                    } else {
                        warn!("Join not found when removing join");
                    }
                    map.insert(channel.clone(), joins_in_history);
                    Some(())
                }
            })
        }
    }

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>> {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        let continuations: Vec<HotStoreAction<C, P, A, K>> = self
            .continuations
            .iter_flat()
            .into_iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
                        channels: k,
                    }))
                } else {
                    let v: Vec<WaitingContinuation<P, K>> = v.iter().map(|a| (**a).clone()).collect();
                    HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                        channels: k,
                        continuations: v,
                    }))
                }
            })
            .collect();

        let data: Vec<HotStoreAction<C, P, A, K>> = self
            .data
            .iter_flat()
            .into_iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel: k }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                        channel: k,
                        data: v,
                    }))
                }
            })
            .collect();

        let joins: Vec<HotStoreAction<C, P, A, K>> = self
            .joins
            .iter_flat()
            .into_iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel: k }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                        channel: k,
                        joins: v,
                    }))
                }
            })
            .collect();

        [continuations, data, joins].concat()
    }

    // Acquires `checkpoint_lock` for a consistent view of all five maps (see
    // the field comment). `std::sync::Mutex` is not reentrant, so `to_map` must
    // not be called while already holding this lock — no current caller does:
    // the other holders (snapshot/changes/set_state/clear) never call `to_map`,
    // and every external caller (rspace/replay_rspace/reporting_rspace) delegates
    // from outside any checkpoint-lock scope.
    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        let data: HashMap<Vec<C>, Vec<Datum<A>>> = self
            .data
            .iter_flat()
            .into_iter()
            .map(|(k, v)| (vec![k], v))
            .collect();

        let mut all_continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>> = self
            .continuations
            .iter_flat()
            .into_iter()
            .map(|(k, v)| (k, v.iter().map(|a| (**a).clone()).collect()))
            .collect();
        for (k, v) in self.installed_continuations.iter_flat() {
            all_continuations.entry(k).or_default().push(v);
        }

        let mut map = HashMap::new();
        for (k, v) in data {
            let row = Row {
                data: v,
                wks: all_continuations.get(&k).cloned().unwrap_or_default(),
            };
            if !(row.data.is_empty() && row.wks.is_empty()) {
                map.insert(k, row);
            }
        }
        map
    }

    fn print(&self) {
        println!("\nHot Store");
        println!("Continuations:");
        for (k, v) in self.continuations.iter_flat() {
            println!("Key: {:?}, Value: {:?}", k, v);
        }
        println!("\nInstalled Continuations:");
        for (k, v) in self.installed_continuations.iter_flat() {
            println!("Key: {:?}, Value: {:?}", k, v);
        }
        println!("\nData:");
        for (k, v) in self.data.iter_flat() {
            println!("Key: {:?}, Value: {:?}", k, v);
        }
        println!("\nJoins:");
        for (k, v) in self.joins.iter_flat() {
            println!("Key: {:?}, Value: {:?}", k, v);
        }
        println!("\nInstalled Joins:");
        for (k, v) in self.installed_joins.iter_flat() {
            println!("Key: {:?}, Value: {:?}", k, v);
        }
        println!("\nHistory Cache Continuations:");
        for e in self.history_cache_continuations.iter() {
            println!("Key: {:?}, Value: {:?}", e.key(), e.value());
        }
        println!("\nHistory Cache Data:");
        for e in self.history_cache_datums.iter() {
            println!("Key: {:?}, Value: {:?}", e.key(), e.value());
        }
        println!("\nHistory Cache Joins:");
        for e in self.history_cache_joins.iter() {
            println!("Key: {:?}, Value: {:?}", e.key(), e.value());
        }
    }

    fn clear(&self) {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        self.continuations.clear();
        self.installed_continuations.clear();
        self.data.clear();
        self.joins.clear();
        self.installed_joins.clear();
        self.history_cache_continuations.clear();
        self.history_cache_datums.clear();
        self.history_cache_joins.clear();
        self.history_cache_cont_items.store(0, Ordering::Relaxed);
        self.history_cache_data_items.store(0, Ordering::Relaxed);
        self.history_cache_joins_items.store(0, Ordering::Relaxed);
        for g in [
            HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC,
            HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC,
            HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC,
            HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC,
            HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC,
            HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC,
            HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC,
            HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC,
            HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC,
            HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC,
        ] {
            metrics::gauge!(g, "source" => RSPACE_METRICS_SOURCE).set(0.0);
        }
        Self::update_hot_store_state_metrics(self);
    }

    fn is_empty(&self) -> bool {
        !self
            .changes()
            .iter()
            .any(|a| matches!(a, HotStoreAction::Insert(_)))
    }

    fn set_state(&self, new_state: HotStoreState<C, P, A, K>) {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        self.data.restore_shards(new_state.data);
        self.continuations.restore_shards(new_state.continuations);
        self.installed_continuations.restore_shards(new_state.installed_continuations);
        self.joins.restore_shards(new_state.joins);
        self.installed_joins.restore_shards(new_state.installed_joins);
    }
}

impl<C, P, A, K> InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + Sync + Send,
    P: Clone + Debug + Sync + Send,
    A: Clone + Debug + Sync + Send,
    K: Clone + Debug + Sync + Send,
{
    fn continuation_identity(wc: &WaitingContinuation<P, K>) -> String {
        format!("{:?}|{:?}|{}|{:?}", wc.patterns, wc.continuation, wc.persist, wc.peeks)
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn state_metrics_update_interval_ms() -> u64 { HOT_STORE_STATE_METRICS_UPDATE_INTERVAL_MS }

    fn history_cache_metrics_update_interval_ms() -> u64 {
        HOT_STORE_HISTORY_CACHE_METRICS_UPDATE_INTERVAL_MS
    }

    fn should_emit_metrics(last_emit_at_ms: &AtomicU64, update_interval_ms: u64) -> bool {
        if update_interval_ms == 0 {
            return true;
        }

        let now = Self::now_millis();
        loop {
            let last = last_emit_at_ms.load(Ordering::Relaxed);
            if now.saturating_sub(last) < update_interval_ms {
                return false;
            }
            if last_emit_at_ms
                .compare_exchange_weak(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn update_hot_store_state_metrics(store: &InMemHotStore<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(&LAST_EMIT_AT_MS, Self::state_metrics_update_interval_ms()) {
            return;
        }
        let cont_flat = store.continuations.iter_flat();
        let cont_items: usize = cont_flat.iter().map(|(_, v)| v.len()).sum();
        let cont_size = cont_flat.len();
        let data_flat = store.data.iter_flat();
        let data_items: usize = data_flat.iter().map(|(_, v)| v.len()).sum();
        let data_size = data_flat.len();
        let joins_flat = store.joins.iter_flat();
        let joins_items: usize = joins_flat.iter().map(|(_, v)| v.len()).sum();
        let joins_size = joins_flat.len();
        let installed_cont_items = store.installed_continuations.len();
        let installed_joins_flat = store.installed_joins.iter_flat();
        let installed_joins_items: usize = installed_joins_flat.iter().map(|(_, v)| v.len()).sum();
        metrics::gauge!(HOT_STORE_STATE_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_size as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_size as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_size as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.installed_continuations.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.installed_joins.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_items as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(installed_cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(installed_joins_items as f64);
    }

    fn update_history_cache_metrics(store: &InMemHotStore<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(
            &LAST_EMIT_AT_MS,
            Self::history_cache_metrics_update_interval_ms(),
        ) {
            return;
        }
        let cont_items = store.history_cache_cont_items.load(Ordering::Relaxed);
        let data_items = store.history_cache_data_items.load(Ordering::Relaxed);
        let joins_items = store.history_cache_joins_items.load(Ordering::Relaxed);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.history_cache_continuations.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.history_cache_datums.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.history_cache_joins.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(cont_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(data_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(joins_items as f64);
    }

    fn enforce_history_cache_bounds(&self) {
        // O(1): read atomic counters instead of iterating DashMaps.
        if self.history_cache_continuations.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            self.history_cache_cont_items.load(Ordering::Relaxed) >=
                MAX_HISTORY_STORE_CACHE_CONT_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC, "source" => RSPACE_METRICS_SOURCE).increment(1);
            self.history_cache_continuations.clear();
            self.history_cache_cont_items.store(0, Ordering::Relaxed);
        }
        if self.history_cache_datums.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            self.history_cache_data_items.load(Ordering::Relaxed) >=
                MAX_HISTORY_STORE_CACHE_DATA_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC, "source" => RSPACE_METRICS_SOURCE).increment(1);
            self.history_cache_datums.clear();
            self.history_cache_data_items.store(0, Ordering::Relaxed);
        }
        if self.history_cache_joins.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            self.history_cache_joins_items.load(Ordering::Relaxed) >=
                MAX_HISTORY_STORE_CACHE_JOIN_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_JOINS_METRIC, "source" => RSPACE_METRICS_SOURCE).increment(1);
            self.history_cache_joins.clear();
            self.history_cache_joins_items.store(0, Ordering::Relaxed);
        }
    }

    fn get_cont_from_history_store(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        self.enforce_history_cache_bounds();
        let channels_vec = channels.to_vec();
        let result = match self.history_cache_continuations.entry(channels_vec.clone()) {
            dashmap::Entry::Occupied(o) => o.get().clone(),
            dashmap::Entry::Vacant(v) => {
                let ks = self.history_reader_base.get_continuations(&channels_vec);
                self.history_cache_cont_items
                    .fetch_add(ks.len(), Ordering::Relaxed);
                v.insert(ks.clone());
                ks
            }
        };
        Self::update_history_cache_metrics(self);
        result
    }

    fn get_data_from_history_store(&self, channel: &C) -> Vec<Datum<A>> {
        self.enforce_history_cache_bounds();
        let result = match self.history_cache_datums.entry(channel.clone()) {
            dashmap::Entry::Occupied(o) => o.get().clone(),
            dashmap::Entry::Vacant(v) => {
                let datums = self.history_reader_base.get_data(channel);
                self.history_cache_data_items
                    .fetch_add(datums.len(), Ordering::Relaxed);
                v.insert(datums.clone());
                datums
            }
        };
        Self::update_history_cache_metrics(self);
        result
    }

    fn get_joins_from_history_store(&self, channel: &C) -> Vec<Vec<C>> {
        self.enforce_history_cache_bounds();
        let result = match self.history_cache_joins.entry(channel.clone()) {
            dashmap::Entry::Occupied(o) => o.get().clone(),
            dashmap::Entry::Vacant(v) => {
                let joins = self.history_reader_base.get_joins(channel);
                self.history_cache_joins_items
                    .fetch_add(joins.len(), Ordering::Relaxed);
                v.insert(joins.clone());
                joins
            }
        };
        Self::update_history_cache_metrics(self);
        result
    }
}

pub struct HotStoreInstances;

impl HotStoreInstances {
    pub fn create_from_hs_and_hr<C, P, A, K>(
        cache: HotStoreState<C, P, A, K>,
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default + Clone + Debug + Eq + Hash + Send + Sync + 'static,
        P: Default + Clone + Debug + Send + Sync + 'static,
        A: Default + Clone + Debug + Send + Sync + 'static,
        K: Default + Clone + Debug + Send + Sync + 'static,
    {
        let store = InMemHotStore {
            data: ShardedMap::from_shards(cache.data),
            continuations: ShardedMap::from_shards(cache.continuations),
            installed_continuations: ShardedMap::from_shards(cache.installed_continuations),
            joins: ShardedMap::from_shards(cache.joins),
            installed_joins: ShardedMap::from_shards(cache.installed_joins),
            history_cache_continuations: DashMap::new(),
            history_cache_datums: DashMap::new(),
            history_cache_joins: DashMap::new(),
            history_cache_cont_items: AtomicUsize::new(0),
            history_cache_data_items: AtomicUsize::new(0),
            history_cache_joins_items: AtomicUsize::new(0),
            checkpoint_lock: std::sync::Mutex::new(()),
            history_reader_base: history_reader,
        };
        Box::new(store)
    }

    pub fn create_from_hr<C, P, A, K>(
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default + Clone + Debug + Eq + Hash + 'static + Send + Sync,
        P: Default + Clone + Debug + 'static + Send + Sync,
        A: Default + Clone + Debug + 'static + Send + Sync,
        K: Default + Clone + Debug + 'static + Send + Sync,
    {
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), history_reader)
    }
}
