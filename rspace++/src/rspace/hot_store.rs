use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use pathmap::PathMap;
#[cfg(test)]
use proptest::prelude::*;
#[cfg(test)]
use rand::{Rng, thread_rng};
use serde::Serialize;
use tracing::warn;

use super::bag::{BagDelta, BagError};
use super::data_index::{CandidatePool, DataIndexError, RSpaceDataStore};
use super::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteCanonicalCollection, DeleteCanonicalData, DeleteContinuations, DeleteData,
    DeleteJoins, HotStoreAction, InsertAction, InsertCanonicalCollection, InsertCanonicalData,
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
use crate::rspace::persistence::ContentTransition;
use crate::rspace::record_index::{RSpaceContinuationStore, RSpaceJoinStore, RecordIndexError};
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into,
};
use crate::rspace::trace::event::Consume;

const MAX_HISTORY_STORE_CACHE_ENTRIES: usize = 512;
const MAX_HISTORY_STORE_CACHE_CONT_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_DATA_ITEMS: usize = 8192;
const MAX_HISTORY_STORE_CACHE_JOIN_ITEMS: usize = 8192;
const HOT_STORE_STATE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;
const HOT_STORE_HISTORY_CACHE_METRICS_UPDATE_INTERVAL_MS: u64 = 250;

/// Revision-local journals captured as one coherent hot-store observation.
///
/// The optional fields preserve the capability distinction: an implementation
/// that cannot expose a namespace journal returns `None`, never an invented
/// empty journal. Implementations with one shared state lock should override
/// `HotStore::pending_deltas` to read all three roots under that lock.
#[derive(Clone, Debug, Default)]
pub(crate) struct PendingDeltas {
    pub data: Option<BagDelta>,
    pub continuations: Option<BagDelta>,
    pub continuation_content: Option<PathMap<ContentTransition>>,
    pub joins: Option<BagDelta>,
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
pub trait HotStore<C: Clone + Hash + Eq, P: Clone, A: Clone, K: Clone>: Sync + Send {
    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>>;
    fn get_continuations_versioned(
        &self,
        channels: &[C],
    ) -> (Option<ContinuationVersion>, Vec<WaitingContinuation<P, K>>) {
        (None, self.get_continuations(channels))
    }
    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool>;
    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()>;
    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()>;

    fn get_data(&self, channel: &C) -> Vec<Datum<A>>;
    fn get_candidate_pool(&self, channel: &C) -> CandidatePool<A>;
    fn put_datum(&self, channel: &C, d: Datum<A>) -> ();
    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError>;
    fn remove_datum_path(&self, path: &[u8]) -> Result<(), RSpaceError>;
    fn apply_datum_delta(&self, delta: &BagDelta) -> Result<(), RSpaceError>;
    /// Return the normalized datum-path changes retained since the current
    /// hot-store revision was created. `None` means this implementation does
    /// not expose a journal; it must never be interpreted as an empty journal.
    fn pending_data_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    /// Return normalized resting-join path changes, or `None` when the
    /// implementation has no join-journal capability.
    fn pending_join_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    /// Return normalized continuation-record path changes, or `None` when the
    /// implementation has no continuation journal capability.
    fn pending_continuation_delta(&self) -> Result<Option<BagDelta>, RSpaceError>;
    fn pending_continuation_content(
        &self,
    ) -> Result<Option<PathMap<ContentTransition>>, RSpaceError>;
    /// Capture all supported revision-local journals as one store snapshot.
    fn pending_deltas(&self) -> Result<PendingDeltas, RSpaceError> {
        Ok(PendingDeltas {
            data: self.pending_data_delta()?,
            continuations: self.pending_continuation_delta()?,
            continuation_content: self.pending_continuation_content()?,
            joins: self.pending_join_delta()?,
        })
    }
    /// Restore revision-local journals captured by a soft checkpoint.
    fn restore_pending_deltas(
        &self,
        data: BagDelta,
        continuations: BagDelta,
        continuation_content: PathMap<ContentTransition>,
        joins: BagDelta,
    ) -> Result<(), RSpaceError>;
    fn apply_accepted_match(
        &self,
        transition: AcceptedMatch<C>,
    ) -> Result<AcceptedMatchOutcome<P, K>, RSpaceError>;

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>>;
    fn put_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn install_join(&self, channel: &C, join: &[C]) -> Option<()>;
    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()>;

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>>;
    /// Checkpoint-specific actions. Implementations may override this boundary
    /// to preserve already-canonical resident bytes while `changes()` remains
    /// the typed diagnostic and compatibility surface.
    fn checkpoint_changes(&self) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError> {
        Ok(self.changes())
    }
    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>>;
    fn snapshot(&self) -> HotStoreState<C, P, A, K>;

    fn print(&self) -> ();
    fn clear(&self) -> ();

    // See rspace/src/test/scala/coop/rchain/rspace/test/package.scala
    fn is_empty(&self) -> bool;

    fn set_state(&self, state: HotStoreState<C, P, A, K>);
}

/// Opaque, limit-free freshness capability for one continuation group.
///
/// Equality is allocation identity, not payload equality. Replacing the
/// group's epoch invalidates every held address while those addresses keep the
/// old allocation alive, so allocator-address reuse cannot make a stale
/// candidate current again.
#[derive(Debug)]
struct ContinuationEpoch;

#[derive(Clone, Debug)]
pub struct ContinuationVersion {
    epoch: Arc<ContinuationEpoch>,
    group_prefix: Arc<[u8]>,
    ordinary_paths: Arc<OnceLock<Arc<[Arc<[u8]>]>>>,
}

impl PartialEq for ContinuationVersion {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.epoch, &other.epoch) && self.group_prefix == other.group_prefix
    }
}

impl Eq for ContinuationVersion {}

impl ContinuationVersion {
    fn new(epoch: Arc<ContinuationEpoch>, group_prefix: Arc<[u8]>) -> Self {
        Self {
            epoch,
            group_prefix,
            ordinary_paths: Arc::new(OnceLock::new()),
        }
    }

    fn bind_paths(&self, paths: Vec<Arc<[u8]>>) { let _ = self.ordinary_paths.set(paths.into()); }

    fn path(&self, index: usize) -> Option<Arc<[u8]>> {
        self.ordinary_paths
            .get()
            .and_then(|paths| paths.get(index))
            .cloned()
    }
}

#[derive(Clone, Debug)]
pub struct ContinuationAddress {
    pub(crate) version: Option<ContinuationVersion>,
    pub(crate) index: i32,
}

impl ContinuationAddress {
    pub(crate) fn new(version: Option<ContinuationVersion>, index: i32) -> Self {
        Self { version, index }
    }

    pub fn index(&self) -> i32 { self.index }
}

#[derive(Clone, Debug)]
pub struct StoredContinuationCommit {
    pub(crate) address: ContinuationAddress,
    pub(crate) source: Consume,
    pub(crate) persist: bool,
    pub(crate) peeks: BTreeSet<i32>,
}

#[derive(Clone, Debug)]
pub struct AcceptedMatch<C> {
    pub(crate) channels: Vec<C>,
    pub(crate) continuation: Option<StoredContinuationCommit>,
    pub(crate) datum_delta: BagDelta,
    pub(crate) required_persistent_paths: Vec<Arc<[u8]>>,
    pub(crate) join_channels: Vec<C>,
}

#[derive(Clone, Debug)]
pub enum AcceptedMatchOutcome<P: Clone, K: Clone> {
    Committed {
        /// The authoritative stored continuation, when this transaction named
        /// one. Callers use it instead of trusting public-facing candidate
        /// payloads after the commit boundary.
        continuation: Option<WaitingContinuation<P, K>>,
    },
    Stale,
}

fn with_installed<P: Clone, K: Clone>(
    installed: Option<WaitingContinuation<P, K>>,
    continuations: Vec<WaitingContinuation<P, K>>,
) -> Vec<WaitingContinuation<P, K>> {
    let Some(installed) = installed else {
        return continuations;
    };
    let mut result = Vec::with_capacity(continuations.len() + 1);
    result.push(installed);
    result.extend(continuations);
    result
}

pub fn new_hashmap<K: std::cmp::Eq + std::hash::Hash, V>() -> HashMap<K, V> { HashMap::new() }

#[derive(Default, Debug, Clone)]
pub struct HotStoreState<C, P, A, K>
where
    C: Eq + Hash,
    A: Clone,
    P: Clone,
    K: Clone,
{
    pub continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    pub installed_continuations: HashMap<Vec<C>, WaitingContinuation<P, K>>,
    pub data: HashMap<C, Vec<Datum<A>>>,
    pub joins: HashMap<C, Vec<Vec<C>>>,
    pub installed_joins: HashMap<C, Vec<Vec<C>>>,
}

struct InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    continuations: RSpaceContinuationStore<C, P, K>,
    data: RSpaceDataStore<C, A>,
    joins: RSpaceJoinStore<C>,
    continuation_base_epoch: Arc<ContinuationEpoch>,
    continuation_epochs: PathMap<Arc<ContinuationEpoch>>,
}

impl<C, P, A, K> InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + RhoPathEncode + ColdStoreDecode + Send + Sync,
    P: Clone + StableHashSerialize + Send + Sync,
    A: Clone + Serialize + RhoPathEncode + ColdStoreDecode + Send + Sync,
    K: Clone + StableHashSerialize + Send + Sync,
{
    fn from_snapshot(snapshot: HotStoreState<C, P, A, K>) -> Result<Self, DataIndexError> {
        let mut continuations = RSpaceContinuationStore::new();
        for (channels, values) in snapshot.continuations {
            continuations.set_group(channels, values);
        }
        for (channels, value) in snapshot.installed_continuations {
            continuations.install(&channels, value);
        }
        let mut data = RSpaceDataStore::new();
        for (channel, datums) in snapshot.data {
            data.set_channel(channel, datums)?;
        }
        let mut joins = RSpaceJoinStore::new();
        for (channel, values) in snapshot.joins {
            joins.set_channel(channel, values);
        }
        for (channel, values) in snapshot.installed_joins {
            for value in values {
                joins.install(&channel, &value);
            }
        }
        Ok(Self {
            continuations,
            data,
            joins,
            continuation_base_epoch: Arc::new(ContinuationEpoch),
            continuation_epochs: PathMap::new(),
        })
    }

    fn snapshot(&self) -> Result<HotStoreState<C, P, A, K>, DataIndexError> {
        Ok(HotStoreState {
            continuations: self
                .continuations
                .loaded_records()
                .into_iter()
                .map(|(channels, values)| (channels.to_vec(), values))
                .collect(),
            installed_continuations: self
                .continuations
                .installed_records()
                .into_iter()
                .map(|(channels, value)| (channels.to_vec(), value))
                .collect(),
            data: self.data.loaded_records()?.into_iter().collect(),
            joins: self.joins.loaded_records().into_iter().collect(),
            installed_joins: self.joins.installed_records().into_iter().collect(),
        })
    }
}

impl<C, P, A, K> Default for InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn default() -> Self {
        Self {
            continuations: RSpaceContinuationStore::default(),
            data: RSpaceDataStore::new(),
            joins: RSpaceJoinStore::default(),
            continuation_base_epoch: Arc::new(ContinuationEpoch),
            continuation_epochs: PathMap::new(),
        }
    }
}

impl<C, P, A, K> InMemHotStoreState<C, P, A, K>
where
    C: Clone + Eq + Hash + Send + Sync,
    A: Clone + Send + Sync,
    P: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn continuation_version(&self, channels: &[C]) -> Option<ContinuationVersion>
    where C: RhoPathEncode {
        let group_prefix = RhoPathV1::continuation_prefix(channels);
        let epoch = self
            .continuation_epochs
            .get(&group_prefix)
            .unwrap_or(&self.continuation_base_epoch);
        Some(ContinuationVersion::new(Arc::clone(epoch), group_prefix.into()))
    }

    fn bump_continuation_revision(&mut self, channels: &[C])
    where C: RhoPathEncode {
        self.continuation_epochs
            .insert(RhoPathV1::continuation_prefix(channels), Arc::new(ContinuationEpoch));
    }

    fn bump_continuation_generation(&mut self) {
        self.continuation_base_epoch = Arc::new(ContinuationEpoch);
        self.continuation_epochs = PathMap::new();
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
        let mut rng = thread_rng();
        (0..size)
            .map(|_| T::default())
            .collect::<Vec<T>>()
            .iter()
            .take(rng.gen_range(0..size + 1))
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

        HotStoreState {
            continuations: HashMap::from_iter(vec![(channels.clone(), continuations.clone())]),
            installed_continuations: HashMap::from_iter(vec![(
                channels.clone(),
                installed_continuations.clone(),
            )]),
            data: HashMap::from_iter(vec![(channel.clone(), data.clone())]),
            joins: HashMap::from_iter(vec![(channel.clone(), joins)]),
            installed_joins: HashMap::from_iter(vec![(channel, installed_joins)]),
        }
    }
}

#[derive(Default)]
struct HistoryStoreCache<C, P, A, K>
where
    C: Eq + Hash,
    A: Clone,
    P: Clone,
    K: Clone,
{
    continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    datums: HashMap<C, Vec<Datum<A>>>,
    joins: HashMap<C, Vec<Vec<C>>>,
}

struct InMemHotStore<C, P, A, K>
where
    C: Clone + Eq + Hash + Sync + Send,
    A: Clone + Sync + Send,
    P: Clone + Sync + Send,
    K: Clone + Sync + Send,
{
    state: std::sync::RwLock<InMemHotStoreState<C, P, A, K>>,
    history_cache: std::sync::RwLock<HistoryStoreCache<C, P, A, K>>,
    history_reader_base: Box<dyn HistoryReaderBase<C, P, A, K>>,
}

#[cfg(not(feature = "accepted-match-transition-instrumentation"))]
type HotStateWriteGuard<'a, T> = std::sync::RwLockWriteGuard<'a, T>;

#[cfg(feature = "accepted-match-transition-instrumentation")]
struct HotStateWriteGuard<'a, T> {
    guard: std::sync::RwLockWriteGuard<'a, T>,
    hold_started: std::time::Instant,
    measured: bool,
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> std::ops::Deref for HotStateWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target { &self.guard }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> std::ops::DerefMut for HotStateWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
impl<T> Drop for HotStateWriteGuard<'_, T> {
    fn drop(&mut self) {
        if self.measured {
            accepted_match_transition_experiment::record_write_hold(self.hold_started.elapsed());
        }
    }
}

#[cfg(not(feature = "accepted-match-transition-instrumentation"))]
#[inline(always)]
fn hot_state_write<T>(lock: &std::sync::RwLock<T>) -> HotStateWriteGuard<'_, T> {
    lock.write().expect("hot store state write lock")
}

#[cfg(feature = "accepted-match-transition-instrumentation")]
#[inline(always)]
fn hot_state_write<T>(lock: &std::sync::RwLock<T>) -> HotStateWriteGuard<'_, T> {
    let wait_started = std::time::Instant::now();
    let guard = lock.write().expect("hot store state write lock");
    let measured =
        accepted_match_transition_experiment::record_write_acquisition(wait_started.elapsed());
    HotStateWriteGuard {
        guard,
        hold_started: std::time::Instant::now(),
        measured,
    }
}

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
impl<C, P, A, K> HotStore<C, P, A, K> for InMemHotStore<C, P, A, K>
where
    C: Clone
        + Debug
        + Hash
        + Eq
        + RhoPathEncode
        + StableHashSerialize
        + ColdStoreDecode
        + Send
        + Sync,
    P: Clone + Debug + StableHashSerialize + Send + Sync,
    A: Clone + Debug + Serialize + RhoPathEncode + ColdStoreDecode + Send + Sync,
    K: Clone + Debug + StableHashSerialize + Send + Sync,
{
    fn snapshot(&self) -> HotStoreState<C, P, A, K> {
        let state = self.state.read().expect("hot store state read lock");
        state
            .snapshot()
            .expect("PathMap-backed hot store must decode values it encoded")
    }

    // Continuations

    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        self.get_continuations_versioned(channels).1
    }

    fn get_continuations_versioned(
        &self,
        channels: &[C],
    ) -> (Option<ContinuationVersion>, Vec<WaitingContinuation<P, K>>) {
        metrics::counter!(HOT_STORE_GET_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(records) = state.continuations.resting_records(channels) {
            let version = state.continuation_version(channels);
            if let Some(version) = &version {
                version.bind_paths(
                    records
                        .iter()
                        .map(|record| Arc::clone(record.path()))
                        .collect(),
                );
            }
            let result = with_installed(
                state.continuations.installed(channels),
                records
                    .into_iter()
                    .map(|record| record.value().clone())
                    .collect(),
            );
            return (version, result);
        }
        drop(state);
        metrics::counter!(HOT_STORE_GET_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let from_history_store = self.get_cont_from_history_store(channels);
        let mut state = self.state.write().expect("hot store state write lock");
        if !state.continuations.is_loaded(channels) {
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store);
        }
        let records = state
            .continuations
            .resting_records(channels)
            .expect("history-filled continuation group must be loaded");
        let version = state.continuation_version(channels);
        if let Some(version) = &version {
            version.bind_paths(
                records
                    .iter()
                    .map(|record| Arc::clone(record.path()))
                    .collect(),
            );
        }
        let result = records
            .into_iter()
            .map(|record| record.value().clone())
            .collect();
        (version, with_installed(state.continuations.installed(channels), result))
    }

    fn put_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<bool> {
        let __put_start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);

        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .continuations
            .is_loaded(channels);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_cont_from_history_store(channels))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.continuations.is_loaded(channels) {
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store.unwrap_or_default());
        }
        let existing_count = state.continuations.resting_len(channels).unwrap_or(0) as u64;
        metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(existing_count);

        let __ident_build_start = std::time::Instant::now();
        let wc_identity = encode_continuation_identity(&wc);
        let mut canonical = wc_identity.clone();
        encode_continuation_source_into(&wc, &mut canonical);
        metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_BUILD_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__ident_build_start.elapsed().as_nanos() as u64);
        let __cmp_start = std::time::Instant::now();
        let inserted = state
            .continuations
            .put_prepared(channels, wc, wc_identity, canonical);
        metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__cmp_start.elapsed().as_nanos() as u64);
        if inserted {
            state.bump_continuation_revision(channels);
        } else {
            metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
        }
        metrics::counter!(HOT_STORE_PUT_CONT_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__put_start.elapsed().as_nanos() as u64);
        Some(inserted)
    }

    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()> {
        let mut state = self.state.write().expect("hot store state write lock");
        state.continuations.install(channels, wc);
        state.bump_continuation_revision(channels);
        Some(())
    }

    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()> {
        let mut state = hot_state_write(&self.state);
        if !state.continuations.is_loaded(channels) {
            let from_history_store = self.get_cont_from_history_store(channels);
            state
                .continuations
                .set_group(channels.to_vec(), from_history_store);
        }
        let is_installed = state.continuations.contains_installed(channels);
        let removing_installed = is_installed && index == 0;
        let removed_index = if is_installed { index - 1 } else { index };

        let removed = if removing_installed {
            warn!("Attempted to remove an installed continuation");
            None
        } else {
            let records = state
                .continuations
                .resting_records(channels)
                .expect("continuation group was loaded before removal");
            let path = usize::try_from(removed_index)
                .ok()
                .and_then(|index| records.get(index))
                .map(|record| Arc::clone(record.path()));
            match path {
                Some(path) => state.continuations.remove_path(channels, &path).map(|_| ()),
                None => {
                    warn!(index, "Index out of bounds when removing continuation");
                    None
                }
            }
        };
        if removed.is_some() {
            state.bump_continuation_revision(channels);
        }
        removed
    }

    // Data

    fn get_data(&self, channel: &C) -> Vec<Datum<A>> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(data) = state
            .data
            .channel_data(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            return data;
        }
        drop(state);
        self.ensure_data_loaded(channel);
        self.state
            .read()
            .expect("hot store state read lock")
            .data
            .channel_data(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
            .expect("loaded channel must have a data view")
    }

    fn get_candidate_pool(&self, channel: &C) -> CandidatePool<A> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        if let Some(pool) = state
            .data
            .channel_pool(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            return pool;
        }
        drop(state);
        self.ensure_data_loaded(channel);
        self.state
            .read()
            .expect("hot store state read lock")
            .data
            .channel_pool(channel)
            .expect("PathMap-backed hot store must decode values it encoded")
            .expect("loaded channel must have a candidate pool")
    }

    fn put_datum(&self, channel: &C, d: Datum<A>) {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_DATUM_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_data_from_history_store(channel))
        };

        let mut state = hot_state_write(&self.state);
        if state.data.is_loaded(channel) {
            state
                .data
                .insert(channel, &d)
                .expect("datum multiplicity must fit in u64");
        } else {
            let mut new_data = from_history_store.unwrap_or_default();
            new_data.push(d);
            state
                .data
                .set_channel(channel.clone(), new_data)
                .expect("history data must fit the PathMap multiplicity model");
        }
        metrics::counter!(HOT_STORE_PUT_DATUM_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
    }

    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError> {
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            Some(self.get_data_from_history_store(channel))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.data.is_loaded(channel) {
            state
                .data
                .set_channel(channel.clone(), from_history_store.unwrap_or_default())?;
        }
        match state.data.remove_index(channel, index) {
            Ok(()) => Ok(()),
            Err(DataIndexError::IndexOutOfBounds { cardinality, .. }) => {
                Err(RSpaceError::BugFoundError(format!(
                    "Index {index} out of bounds when removing datum (len={cardinality})"
                )))
            }
            Err(error) => Err(error.into()),
        }
    }

    fn remove_datum_path(&self, path: &[u8]) -> Result<(), RSpaceError> {
        if self
            .state
            .write()
            .expect("hot store state write lock")
            .data
            .remove_path(path)
        {
            Ok(())
        } else {
            Err(RSpaceError::BugFoundError(
                "Stored datum path was absent during removal".to_string(),
            ))
        }
    }

    fn apply_datum_delta(&self, delta: &BagDelta) -> Result<(), RSpaceError> {
        hot_state_write(&self.state)
            .data
            .apply_delta(delta)
            .map_err(Into::into)
    }

    fn pending_data_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .data
                .pending_delta()
                .clone(),
        ))
    }

    fn pending_join_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .joins
                .pending_delta()
                .clone(),
        ))
    }

    fn pending_continuation_delta(&self) -> Result<Option<BagDelta>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .continuations
                .pending_delta()
                .clone(),
        ))
    }

    fn pending_continuation_content(
        &self,
    ) -> Result<Option<PathMap<ContentTransition>>, RSpaceError> {
        Ok(Some(
            self.state
                .read()
                .expect("hot store state read lock")
                .continuations
                .pending_content()
                .clone(),
        ))
    }

    fn pending_deltas(&self) -> Result<PendingDeltas, RSpaceError> {
        let state = self.state.read().expect("hot store state read lock");
        Ok(PendingDeltas {
            data: Some(state.data.pending_delta().clone()),
            continuations: Some(state.continuations.pending_delta().clone()),
            continuation_content: Some(state.continuations.pending_content().clone()),
            joins: Some(state.joins.pending_delta().clone()),
        })
    }

    fn restore_pending_deltas(
        &self,
        data: BagDelta,
        continuations: BagDelta,
        continuation_content: PathMap<ContentTransition>,
        joins: BagDelta,
    ) -> Result<(), RSpaceError> {
        let mut state = hot_state_write(&self.state);
        state.data.restore_pending_delta(data);
        state.continuations.restore_pending_delta(continuations);
        state
            .continuations
            .restore_pending_content(continuation_content);
        state.joins.restore_pending_delta(joins);
        Ok(())
    }

    fn apply_accepted_match(
        &self,
        transition: AcceptedMatch<C>,
    ) -> Result<AcceptedMatchOutcome<P, K>, RSpaceError> {
        // History reads are completed before the linearization lock. They are
        // proposals only: a stale transition returns without installing them
        // into the hot cache, so rejection is observably non-mutating.
        let missing_join_channels = {
            let state = self.state.read().expect("hot store state read lock");
            transition
                .join_channels
                .iter()
                .filter(|channel| !state.joins.is_loaded(*channel))
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut prepared_joins = missing_join_channels
            .into_iter()
            .map(|channel| {
                let joins = self.get_joins_from_history_store(&channel);
                (channel, joins)
            })
            .collect::<Vec<_>>();

        let mut state = hot_state_write(&self.state);

        let mut authoritative_continuation = None;
        let mut ordinary_removal_path = None;
        if let Some(expected) = &transition.continuation {
            let current_version = state.continuation_version(&transition.channels);
            if expected.address.version.is_none() || expected.address.version != current_version {
                return Ok(AcceptedMatchOutcome::Stale);
            }

            let installed = state.continuations.installed(&transition.channels);
            let (current, ordinary_path, is_installed) =
                if installed.is_some() && expected.address.index == 0 {
                    (installed, None, true)
                } else {
                    let adjusted = if installed.is_some() {
                        expected.address.index.checked_sub(1)
                    } else {
                        Some(expected.address.index)
                    };
                    let Some(index) = adjusted.and_then(|index| usize::try_from(index).ok()) else {
                        return Ok(AcceptedMatchOutcome::Stale);
                    };
                    let path = expected
                        .address
                        .version
                        .as_ref()
                        .and_then(|version| version.path(index));
                    let current = path
                        .as_deref()
                        .and_then(|path| state.continuations.get_path(&transition.channels, path))
                        .map(|record| record.value().clone());
                    (current, path, false)
                };
            let Some(current) = current else {
                return Ok(AcceptedMatchOutcome::Stale);
            };
            if current.source != expected.source ||
                current.persist != expected.persist ||
                current.peeks != expected.peeks ||
                (is_installed && !current.persist)
            {
                return Ok(AcceptedMatchOutcome::Stale);
            }
            authoritative_continuation = Some(current.clone());
            if !current.persist && !is_installed {
                ordinary_removal_path = ordinary_path;
            }
        }

        if transition
            .required_persistent_paths
            .iter()
            .any(|path| state.data.index().count_path(path) == 0)
        {
            return Ok(AcceptedMatchOutcome::Stale);
        }

        match state.data.apply_delta(&transition.datum_delta) {
            Ok(()) => {}
            Err(DataIndexError::Bag(BagError::InsufficientMultiplicity { .. })) => {
                return Ok(AcceptedMatchOutcome::Stale);
            }
            Err(error) => return Err(error.into()),
        }

        if let Some(path) = ordinary_removal_path {
            state
                .continuations
                .remove_path(&transition.channels, &path)
                .expect("a validated continuation path must remain present");
            state.bump_continuation_revision(&transition.channels);

            let group_remains = state.continuations.contains_installed(&transition.channels) ||
                state
                    .continuations
                    .resting_len(&transition.channels)
                    .is_some_and(|len| len != 0);
            if !group_remains {
                for channel in transition.join_channels {
                    if !state.joins.is_loaded(&channel) {
                        let prepared = prepared_joins
                            .iter_mut()
                            .find(|(prepared_channel, _)| prepared_channel == &channel)
                            .map(|(_, joins)| std::mem::take(joins))
                            .unwrap_or_default();
                        state.joins.set_channel(channel.clone(), prepared);
                    }
                    let _ = state.joins.remove(&channel, &transition.channels);
                }
            }
        }

        Ok(AcceptedMatchOutcome::Committed {
            continuation: authoritative_continuation,
        })
    }

    // Joins

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>> {
        metrics::counter!(HOT_STORE_GET_JOINS_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let state = self.state.read().expect("hot store state read lock");
        let joins = state.joins.resting_values(channel);
        let installed_joins = state.joins.installed_values(channel);
        drop(state);

        match joins {
            Some(joins_data) => {
                let mut result = installed_joins;
                result.extend(joins_data);
                result
            }
            None => {
                metrics::counter!(HOT_STORE_GET_JOINS_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history_store = self.get_joins_from_history_store(channel);
                let mut state = self.state.write().expect("hot store state write lock");
                if !state.joins.is_loaded(channel) {
                    state.joins.set_channel(channel.clone(), from_history_store);
                }
                let mut result = state.joins.installed_values(channel);
                result.extend(
                    state
                        .joins
                        .resting_values(channel)
                        .expect("history-filled join channel must be loaded"),
                );
                result
            }
        }
    }

    fn put_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let __start = std::time::Instant::now();
        metrics::counter!(HOT_STORE_PUT_JOIN_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let has_existing = self
            .state
            .read()
            .expect("hot store state read lock")
            .joins
            .is_loaded(channel);
        let from_history_store = if has_existing {
            None
        } else {
            metrics::counter!(HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            Some(self.get_joins_from_history_store(channel))
        };

        let mut state = self.state.write().expect("hot store state write lock");
        if !state.joins.is_loaded(channel) {
            state
                .joins
                .set_channel(channel.clone(), from_history_store.unwrap_or_default());
        }
        let _ = state.joins.put(channel, join);
        metrics::counter!(HOT_STORE_PUT_JOIN_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
        Some(())
    }

    fn install_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let mut state = self.state.write().expect("hot store state write lock");
        let _ = state.joins.install(channel, join);
        Some(())
    }

    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let mut state = hot_state_write(&self.state);
        let has_join_in_state = state.joins.is_loaded(channel);
        let current_continuations = {
            let mut conts = state
                .continuations
                .installed(join)
                .map(|c| vec![c.clone()])
                .unwrap_or_default();
            conts.extend(
                state
                    .continuations
                    .resting_values(join)
                    .unwrap_or_else(|| self.get_cont_from_history_store(join)),
            );
            conts
        };

        let do_remove = current_continuations.is_empty();

        if !do_remove {
            if !has_join_in_state {
                let joins_in_history_store = self.get_joins_from_history_store(channel);
                state
                    .joins
                    .set_channel(channel.clone(), joins_in_history_store);
            }
            Some(())
        } else {
            if !has_join_in_state {
                let joins_in_history_store = self.get_joins_from_history_store(channel);
                state
                    .joins
                    .set_channel(channel.clone(), joins_in_history_store);
            }
            if state.joins.remove(channel, join).is_none() {
                warn!("Join not found when removing join");
            }
            Some(())
        }
    }

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>> {
        let cache = self.state.read().expect("hot store state read lock");
        let capacity = cache
            .continuations
            .loaded_group_count()
            .checked_add(cache.data.loaded_channel_count())
            .and_then(|count| count.checked_add(cache.joins.loaded_channel_count()))
            .expect("resident group counts must fit the address space that stores them");
        let mut actions = Vec::with_capacity(capacity);

        cache
            .continuations
            .visit_loaded_records(|channels, continuations| {
                actions.push(if continuations.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
                        channels: channels.to_vec(),
                    }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
                        channels: channels.to_vec(),
                        continuations,
                    }))
                });
            });

        cache
            .data
            .try_visit_loaded_records(|channel, data| {
                actions.push(if data.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertData(InsertData { channel, data }))
                });
                Ok(())
            })
            .expect("PathMap-backed hot store must decode values it encoded");

        cache.joins.visit_loaded_records(|channel, joins| {
            actions.push(if joins.is_empty() {
                HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel }))
            } else {
                HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins { channel, joins }))
            });
        });

        actions
    }

    fn checkpoint_changes(&self) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError> {
        let cache = self.state.read().expect("hot store state read lock");
        let group_count = cache
            .continuations
            .loaded_group_count()
            .checked_add(cache.data.loaded_channel_count())
            .and_then(|count| count.checked_add(cache.joins.loaded_channel_count()))
            .ok_or(RecordIndexError::EncodedCardinalityOverflow(u128::MAX))?;
        let mut actions = Vec::new();
        actions
            .try_reserve_exact(group_count)
            .map_err(|_| RecordIndexError::AllocationFailure(group_count))?;

        cache
            .continuations
            .try_visit_loaded_canonical_plans(|plan| {
                actions.push(if plan.record_count() == 0 {
                    HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
                        DeleteCanonicalCollection::new(
                            plan.kind(),
                            plan.history_key_hash().clone(),
                        ),
                    ))
                } else {
                    let record = plan.into_record_group()?;
                    HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
                        InsertCanonicalCollection::from_trusted_parts(
                            record.kind,
                            record.history_key_hash,
                            record.encoded_persisted_data,
                            record.record_count,
                        ),
                    ))
                });
                Ok(())
            })?;

        cache.data.try_visit_loaded_canonical_records(|record| {
            actions.push(if record.datum_count == 0 {
                HotStoreAction::Delete(DeleteAction::DeleteCanonicalData(DeleteCanonicalData::new(
                    record.channel_hash,
                )))
            } else {
                HotStoreAction::Insert(InsertAction::InsertCanonicalData(
                    InsertCanonicalData::from_trusted_parts(
                        record.channel_hash,
                        record.encoded_persisted_data,
                        record.datum_count,
                    ),
                ))
            });
            Ok(())
        })?;

        cache.joins.try_visit_loaded_canonical_plans(|plan| {
            actions.push(if plan.record_count() == 0 {
                HotStoreAction::Delete(DeleteAction::DeleteCanonicalCollection(
                    DeleteCanonicalCollection::new(plan.kind(), plan.history_key_hash().clone()),
                ))
            } else {
                let record = plan.into_record_group()?;
                HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(
                    InsertCanonicalCollection::from_trusted_parts(
                        record.kind,
                        record.history_key_hash,
                        record.encoded_persisted_data,
                        record.record_count,
                    ),
                ))
            });
            Ok(())
        })?;

        debug_assert_eq!(actions.len(), group_count);
        Ok(actions)
    }

    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> {
        let state = self.state.read().expect("hot store state read lock");
        let mut map = HashMap::with_capacity(
            state.data.loaded_channel_count() +
                state.continuations.loaded_group_count() +
                state.continuations.installed_count(),
        );

        // Data and continuations use different key shapes: a datum is indexed
        // by one channel, while a joined continuation is indexed by its full
        // channel vector. Build the union of those domains. Iterating only the
        // data keys silently dropped continuation-only rows and every join.
        for (channel, data) in state
            .data
            .loaded_records()
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            if !data.is_empty() {
                map.insert(vec![channel], Row {
                    data,
                    wks: Vec::new(),
                });
            }
        }

        for (channels, continuations) in state.continuations.loaded_records() {
            if !continuations.is_empty() {
                map.entry(channels.to_vec())
                    .or_insert_with(|| Row {
                        data: Vec::new(),
                        wks: Vec::new(),
                    })
                    .wks
                    .extend(continuations);
            }
        }

        for (channels, continuation) in state.continuations.installed_records() {
            map.entry(channels.to_vec())
                .or_insert_with(|| Row {
                    data: Vec::new(),
                    wks: Vec::new(),
                })
                .wks
                .push(continuation);
        }

        map
    }

    fn print(&self) {
        let hot_store_state = self.state.read().expect("hot store state read lock");
        println!("\nHot Store");

        println!("Continuations:");
        for (key, value) in hot_store_state.continuations.loaded_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nInstalled Continuations:");
        for (key, value) in hot_store_state.continuations.installed_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nData:");
        for (key, value) in hot_store_state
            .data
            .loaded_records()
            .expect("PathMap-backed hot store must decode values it encoded")
        {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nJoins:");
        for (key, value) in hot_store_state.joins.loaded_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nInstalled Joins:");
        for (key, value) in hot_store_state.joins.installed_records() {
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        let history_cache_state = self.history_cache.read().expect("history cache read lock");
        println!("\nHistory Cache");

        println!("Continuations:");
        for entry in history_cache_state.continuations.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nData:");
        for entry in history_cache_state.datums.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }

        println!("\nJoins:");
        for entry in history_cache_state.joins.iter() {
            let (key, value) = (entry.0, entry.1);
            println!("Key: {:?}, Value: {:?}", key, value);
        }
    }

    fn clear(&self) {
        let mut state = self.state.write().expect("hot store state write lock");
        state.bump_continuation_generation();
        state.continuations.clear();
        state.data.clear();
        state.joins.clear();
        drop(state);

        let mut history_cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        history_cache.continuations.clear();
        history_cache.datums.clear();
        history_cache.joins.clear();
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(0.0);

        Self::update_hot_store_state_metrics(
            &self.state.read().expect("hot store state read lock"),
        );
    }

    // See rspace/src/test/scala/coop/rchain/rspace/test/package.scala
    fn is_empty(&self) -> bool {
        let state = self.state.read().expect("hot store state read lock");
        !state.continuations.has_nonempty_loaded_group() &&
            !state.data.has_nonempty_loaded_channel() &&
            !state.joins.has_nonempty_loaded_group()
    }

    fn set_state(&self, new_state: HotStoreState<C, P, A, K>) {
        let mut state = self
            .state
            .write()
            .expect("hot store state write lock for set_state");
        let replacement = InMemHotStoreState::from_snapshot(new_state)
            .expect("snapshot data must fit the PathMap multiplicity model");
        *state = replacement;
    }
}

impl<C, P, A, K> InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + RhoPathEncode + ColdStoreDecode + Sync + Send,
    P: Clone + Debug + StableHashSerialize + Sync + Send,
    A: Clone + Debug + Serialize + RhoPathEncode + ColdStoreDecode + Sync + Send,
    K: Clone + Debug + StableHashSerialize + Sync + Send,
{
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

    fn update_hot_store_state_metrics(state: &InMemHotStoreState<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(&LAST_EMIT_AT_MS, Self::state_metrics_update_interval_ms()) {
            return;
        }

        let cont_items = state.continuations.resting_record_count();
        let data_items = state.data.cardinality();
        let joins_items = state.joins.resting_record_count();
        let installed_cont_items = state.continuations.installed_count();
        let installed_joins_items = state.joins.installed_record_count();

        metrics::gauge!(HOT_STORE_STATE_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.continuations.loaded_group_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.data.loaded_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.joins.loaded_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.continuations.installed_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(state.joins.installed_channel_count() as f64);
        metrics::gauge!(HOT_STORE_STATE_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_items as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(installed_cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(installed_joins_items as f64);
    }

    fn update_history_cache_metrics(cache: &HistoryStoreCache<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(
            &LAST_EMIT_AT_MS,
            Self::history_cache_metrics_update_interval_ms(),
        ) {
            return;
        }

        let cont_items: usize = cache.continuations.values().map(|v| v.len()).sum();
        let data_items: usize = cache.datums.values().map(|v| v.len()).sum();
        let joins_items: usize = cache.joins.values().map(|v| v.len()).sum();

        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.continuations.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.datums.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cache.joins.len() as f64);
        metrics::gauge!(HOT_STORE_HISTORY_CONT_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(cont_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_DATA_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(data_items as f64);
        metrics::gauge!(HOT_STORE_HISTORY_JOINS_CACHE_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .set(joins_items as f64);
    }

    fn enforce_history_cache_bounds(cache: &mut HistoryStoreCache<C, P, A, K>) {
        let cont_items: usize = cache.continuations.values().map(|v| v.len()).sum();
        let data_items: usize = cache.datums.values().map(|v| v.len()).sum();
        let joins_items: usize = cache.joins.values().map(|v| v.len()).sum();

        if cache.continuations.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            cont_items >= MAX_HISTORY_STORE_CACHE_CONT_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.continuations.clear();
        }
        if cache.datums.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            data_items >= MAX_HISTORY_STORE_CACHE_DATA_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.datums.clear();
        }
        if cache.joins.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES ||
            joins_items >= MAX_HISTORY_STORE_CACHE_JOIN_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_JOINS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            cache.joins.clear();
        }
    }

    fn get_cont_from_history_store(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let channels_vec = channels.to_vec();
        let entry = cache.continuations.entry(channels_vec.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let ks = self.history_reader_base.get_continuations(&channels_vec);
                v.insert(ks.clone());
                ks
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }

    fn get_data_from_history_store(&self, channel: &C) -> Vec<Datum<A>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let entry = cache.datums.entry(channel.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let datums = self.history_reader_base.get_data(channel);
                v.insert(datums.clone());
                datums
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }

    fn ensure_data_loaded(&self, channel: &C) {
        if self
            .state
            .read()
            .expect("hot store state read lock")
            .data
            .is_loaded(channel)
        {
            return;
        }
        metrics::counter!(HOT_STORE_GET_DATA_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let data = self.get_data_from_history_store(channel);
        let mut state = self.state.write().expect("hot store state write lock");
        if !state.data.is_loaded(channel) {
            state
                .data
                .set_channel(channel.clone(), data)
                .expect("history data must fit the PathMap multiplicity model");
        }
    }

    fn get_joins_from_history_store(&self, channel: &C) -> Vec<Vec<C>> {
        let mut cache = self
            .history_cache
            .write()
            .expect("history cache write lock");
        Self::enforce_history_cache_bounds(&mut cache);
        let entry = cache.joins.entry(channel.clone());
        let result = match entry {
            Entry::Occupied(o) => o.get().clone(),
            Entry::Vacant(v) => {
                let joins = self.history_reader_base.get_joins(channel);
                v.insert(joins.clone());
                joins
            }
        };
        Self::update_history_cache_metrics(&cache);
        result
    }
}

#[cfg(test)]
#[path = "../../tests/hot_store/group_scoped_version.rs"]
mod group_scoped_version_tests;

#[cfg(feature = "accepted-match-transition-experiment")]
#[doc(hidden)]
#[path = "../../tests/hot_store/accepted_match_transition_experiment.rs"]
pub mod accepted_match_transition_experiment;

pub struct HotStoreInstances;

impl HotStoreInstances {
    pub fn create_from_hs_and_hr<C, P, A, K>(
        cache: HotStoreState<C, P, A, K>,
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        P: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + Send
            + Sync
            + 'static,
        K: Default + Clone + Debug + StableHashSerialize + Send + Sync + 'static,
    {
        Box::new(InMemHotStore {
            state: std::sync::RwLock::new(
                InMemHotStoreState::from_snapshot(cache)
                    .expect("snapshot data must fit the PathMap multiplicity model"),
            ),
            history_cache: std::sync::RwLock::new(HistoryStoreCache::default()),
            history_reader_base: history_reader,
        })
    }

    pub fn create_from_hr<C, P, A, K>(
        history_reader: Box<dyn HistoryReaderBase<C, P, A, K>>,
    ) -> Box<dyn HotStore<C, P, A, K>>
    where
        C: Default
            + Clone
            + Debug
            + Eq
            + Hash
            + RhoPathEncode
            + ColdStoreDecode
            + 'static
            + Send
            + Sync,
        P: Default + Clone + Debug + StableHashSerialize + 'static + Send + Sync,
        A: Default
            + Clone
            + Debug
            + Serialize
            + RhoPathEncode
            + ColdStoreDecode
            + 'static
            + Send
            + Sync,
        K: Default + Clone + Debug + StableHashSerialize + 'static + Send + Sync,
    {
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), history_reader)
    }
}
