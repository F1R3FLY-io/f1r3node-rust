use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;

#[cfg(test)]
use proptest::prelude::*;
#[cfg(test)]
use rand::{Rng, thread_rng};
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

// See rspace/src/main/scala/coop/rchain/rspace/HotStore.scala
pub trait HotStore<C: Clone + Hash + Eq, P: Clone, A: Clone, K: Clone>: Sync + Send {
    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>>;
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
            .cloned()
            .take(rng.gen_range(0..size + 1))
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

struct InMemHotStore<C, P, A, K>
where
    C: Eq + Hash + Sync + Send,
    A: Clone + Sync + Send,
    P: Clone + Sync + Send,
    K: Clone + Sync + Send,
{
    // Hot path: per-key concurrent access via DashMap shards.
    // All produce/consume operations during deploy execution use these directly
    // without any global lock.
    data: DashMap<C, Vec<Datum<A>>>,
    continuations: DashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    installed_continuations: DashMap<Vec<C>, WaitingContinuation<P, K>>,
    joins: DashMap<C, Vec<Vec<C>>>,
    installed_joins: DashMap<C, Vec<Vec<C>>>,
    history_cache_continuations: DashMap<Vec<C>, Vec<WaitingContinuation<P, K>>>,
    history_cache_datums: DashMap<C, Vec<Datum<A>>>,
    history_cache_joins: DashMap<C, Vec<Vec<C>>>,
    // Atomic item counters for O(1) cache bounds checking.
    // Updated on every insert/evict so enforce_history_cache_bounds never
    // has to iterate the DashMaps (which was 44% of CPU per the flame graph).
    history_cache_cont_items: AtomicUsize,
    history_cache_data_items: AtomicUsize,
    history_cache_joins_items: AtomicUsize,
    // Checkpoint path: acquired only by snapshot/changes/set_state/clear which
    // need a consistent view of all five maps at once. These are called at the
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
            continuations: self.continuations.iter().map(|e| (e.key().clone(), e.value().clone())).collect(),
            installed_continuations: self.installed_continuations.iter().map(|e| (e.key().clone(), e.value().clone())).collect(),
            data: self.data.iter().map(|e| (e.key().clone(), e.value().clone())).collect(),
            joins: self.joins.iter().map(|e| (e.key().clone(), e.value().clone())).collect(),
            installed_joins: self.installed_joins.iter().map(|e| (e.key().clone(), e.value().clone())).collect(),
        }
    }

    // Continuations

    fn get_continuations(&self, channels: &[C]) -> Vec<WaitingContinuation<P, K>> {
        metrics::counter!(HOT_STORE_GET_CONT_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let continuations = self.continuations.get(channels).map(|r| r.clone());
        let installed = self.installed_continuations.get(channels).map(|r| r.clone());

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
                let from_history = self.get_cont_from_history_store(channels);
                self.continuations.insert(channels.to_vec(), from_history.clone());
                let mut result = Vec::with_capacity(from_history.len() + 1);
                result.push(inst);
                result.extend(from_history);
                result
            }
            (None, None) => {
                metrics::counter!(HOT_STORE_GET_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let from_history = self.get_cont_from_history_store(channels);
                self.continuations.insert(channels.to_vec(), from_history.clone());
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

        let mut inserted = false;
        match self.continuations.entry(channels.to_vec()) {
            dashmap::Entry::Occupied(mut occupied) => {
                let existing_count = occupied.get().len() as u64;
                metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(existing_count);
                let __cmp_start = std::time::Instant::now();
                let dup = occupied.get().iter().any(|e| Self::continuation_identity(e) == wc_identity);
                metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(__cmp_start.elapsed().as_nanos() as u64);
                if !dup {
                    occupied.get_mut().insert(0, wc);
                    inserted = true;
                } else {
                    metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                }
            }
            dashmap::Entry::Vacant(vacant) => {
                metrics::counter!(HOT_STORE_PUT_CONT_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let mut new_continuations = self.get_cont_from_history_store(channels);
                let existing_count = new_continuations.len() as u64;
                metrics::counter!(HOT_STORE_PUT_CONT_EXISTING_COUNT_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(existing_count);
                let __cmp_start = std::time::Instant::now();
                let dup = new_continuations.iter().any(|e| Self::continuation_identity(e) == wc_identity);
                metrics::counter!(HOT_STORE_PUT_CONT_IDENTITY_COMPARE_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(__cmp_start.elapsed().as_nanos() as u64);
                if !dup {
                    new_continuations.insert(0, wc);
                    inserted = true;
                } else {
                    metrics::counter!(HOT_STORE_PUT_CONT_DUPLICATES_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                }
                vacant.insert(new_continuations);
            }
        }
        metrics::counter!(HOT_STORE_PUT_CONT_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__put_start.elapsed().as_nanos() as u64);
        Some(inserted)
    }

    fn install_continuation(&self, channels: &[C], wc: WaitingContinuation<P, K>) -> Option<()> {
        self.installed_continuations.insert(channels.to_vec(), wc);
        Some(())
    }

    fn remove_continuation(&self, channels: &[C], index: i32) -> Option<()> {
        let is_installed = self.installed_continuations.contains_key(channels);
        let removing_installed = is_installed && index == 0;
        let removed_index = if is_installed { index - 1 } else { index };

        if removing_installed {
            warn!("Attempted to remove an installed continuation");
            return None;
        }

        match self.continuations.entry(channels.to_vec()) {
            dashmap::Entry::Occupied(mut occupied) => {
                let len = occupied.get().len();
                let out_of_bounds = removed_index < 0 || removed_index as usize >= len;
                if out_of_bounds {
                    warn!(index, "Index out of bounds when removing continuation");
                    None
                } else {
                    occupied.get_mut().remove(removed_index as usize);
                    Some(())
                }
            }
            dashmap::Entry::Vacant(vacant) => {
                let mut from_history = self.get_cont_from_history_store(channels);
                let len = from_history.len();
                let out_of_bounds = removed_index < 0 || removed_index as usize >= len;
                if out_of_bounds {
                    warn!(index, "Index out of bounds when removing continuation");
                    vacant.insert(from_history);
                    None
                } else {
                    from_history.remove(removed_index as usize);
                    vacant.insert(from_history);
                    Some(())
                }
            }
        }
    }

    // Data

    fn get_data(&self, channel: &C) -> Vec<Datum<A>> {
        metrics::counter!(HOT_STORE_GET_DATA_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        if let Some(data) = self.data.get(channel).map(|r| r.clone()) {
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
        match self.data.entry(channel.clone()) {
            dashmap::Entry::Occupied(mut occupied) => {
                occupied.get_mut().insert(0, d);
            }
            dashmap::Entry::Vacant(vacant) => {
                metrics::counter!(HOT_STORE_PUT_DATUM_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let mut new_data = self.get_data_from_history_store(channel);
                new_data.insert(0, d);
                vacant.insert(new_data);
            }
        }
        metrics::counter!(HOT_STORE_PUT_DATUM_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
    }

    fn remove_datum(&self, channel: &C, index: i32) -> Result<(), RSpaceError> {
        match self.data.entry(channel.clone()) {
            dashmap::Entry::Occupied(mut occupied) => {
                let out_of_bounds = index < 0 || index as usize >= occupied.get().len();
                if out_of_bounds {
                    Err(RSpaceError::BugFoundError(format!(
                        "Index {} out of bounds when removing datum (len={})",
                        index,
                        occupied.get().len()
                    )))
                } else {
                    occupied.get_mut().remove(index as usize);
                    Ok(())
                }
            }
            dashmap::Entry::Vacant(vacant) => {
                let mut from_history = self.get_data_from_history_store(channel);
                let out_of_bounds = index < 0 || index as usize >= from_history.len();
                if out_of_bounds {
                    let len = from_history.len();
                    vacant.insert(from_history);
                    Err(RSpaceError::BugFoundError(format!(
                        "Index {} out of bounds when removing datum (len={})",
                        index, len
                    )))
                } else {
                    from_history.remove(index as usize);
                    vacant.insert(from_history);
                    Ok(())
                }
            }
        }
    }

    // Joins

    fn get_joins(&self, channel: &C) -> Vec<Vec<C>> {
        metrics::counter!(HOT_STORE_GET_JOINS_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let joins = self.joins.get(channel).map(|r| r.clone());
        let installed_joins = self.installed_joins.get(channel).map(|r| r.clone());

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
        match self.joins.entry(channel.clone()) {
            dashmap::Entry::Occupied(mut occupied) => {
                if !occupied.get().iter().any(|j| j.as_slice() == join) {
                    occupied.get_mut().insert(0, join.to_vec());
                }
            }
            dashmap::Entry::Vacant(vacant) => {
                metrics::counter!(HOT_STORE_PUT_JOIN_HISTORY_FILL_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                let mut joins = self.get_joins_from_history_store(channel);
                if !joins.iter().any(|j| j.as_slice() == join) {
                    joins.insert(0, join.to_vec());
                }
                vacant.insert(joins);
            }
        }
        metrics::counter!(HOT_STORE_PUT_JOIN_TIME_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(__start.elapsed().as_nanos() as u64);
        Some(())
    }

    fn install_join(&self, channel: &C, join: &[C]) -> Option<()> {
        match self.installed_joins.entry(channel.clone()) {
            dashmap::Entry::Occupied(mut occupied) => {
                if !occupied.get().iter().any(|j| j.as_slice() == join) {
                    occupied.get_mut().insert(0, join.to_vec());
                }
            }
            dashmap::Entry::Vacant(vacant) => {
                vacant.insert(vec![join.to_vec()]);
            }
        }
        Some(())
    }

    fn remove_join(&self, channel: &C, join: &[C]) -> Option<()> {
        let has_join_in_state = self.joins.contains_key(channel);
        let current_continuations = {
            let mut conts = self.installed_continuations
                .get(join)
                .map(|c| vec![c.clone()])
                .unwrap_or_default();
            conts.extend(
                self.continuations
                    .get(join)
                    .map(|r| r.clone())
                    .unwrap_or_else(|| self.get_cont_from_history_store(join)),
            );
            conts
        };

        let do_remove = current_continuations.is_empty();

        if !do_remove {
            if !has_join_in_state {
                let joins_in_history = self.get_joins_from_history_store(channel);
                self.joins.insert(channel.clone(), joins_in_history);
            }
            Some(())
        } else {
            match self.joins.entry(channel.clone()) {
                dashmap::Entry::Occupied(mut occupied) => {
                    if let Some(idx) = occupied.get().iter().position(|x| x.as_slice() == join) {
                        occupied.get_mut().remove(idx);
                    } else {
                        warn!("Join not found when removing join");
                    }
                    Some(())
                }
                dashmap::Entry::Vacant(vacant) => {
                    let mut joins_in_history = self.get_joins_from_history_store(channel);
                    if let Some(idx) = joins_in_history.iter().position(|x| x.as_slice() == join) {
                        joins_in_history.remove(idx);
                    } else {
                        warn!("Join not found when removing join");
                    }
                    vacant.insert(joins_in_history);
                    Some(())
                }
            }
        }
    }

    fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>> {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        let continuations: Vec<HotStoreAction<C, P, A, K>> = self
            .continuations
            .iter()
            .map(|entry| {
                let (k, v) = (entry.key().clone(), entry.value().clone());
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations { channels: k }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations { channels: k, continuations: v }))
                }
            })
            .collect();

        let data: Vec<HotStoreAction<C, P, A, K>> = self
            .data
            .iter()
            .map(|entry| {
                let (k, v) = (entry.key().clone(), entry.value().clone());
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData { channel: k }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertData(InsertData { channel: k, data: v }))
                }
            })
            .collect();

        let joins: Vec<HotStoreAction<C, P, A, K>> = self
            .joins
            .iter()
            .map(|entry| {
                let (k, v) = (entry.key().clone(), entry.value().clone());
                if v.is_empty() {
                    HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins { channel: k }))
                } else {
                    HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins { channel: k, joins: v }))
                }
            })
            .collect();

        [continuations, data, joins].concat()
    }

    fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>> {
        let data: HashMap<Vec<C>, Vec<Datum<A>>> = self
            .data
            .iter()
            .map(|e| (vec![e.key().clone()], e.value().clone()))
            .collect();

        let mut all_continuations: HashMap<Vec<C>, Vec<WaitingContinuation<P, K>>> = self
            .continuations
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();
        for entry in self.installed_continuations.iter() {
            all_continuations
                .entry(entry.key().clone())
                .or_insert_with(Vec::new)
                .push(entry.value().clone());
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
        for e in self.continuations.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nInstalled Continuations:");
        for e in self.installed_continuations.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nData:");
        for e in self.data.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nJoins:");
        for e in self.joins.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nInstalled Joins:");
        for e in self.installed_joins.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nHistory Cache Continuations:");
        for e in self.history_cache_continuations.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nHistory Cache Data:");
        for e in self.history_cache_datums.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
        println!("\nHistory Cache Joins:");
        for e in self.history_cache_joins.iter() { println!("Key: {:?}, Value: {:?}", e.key(), e.value()); }
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
        !self.changes().iter().any(|a| matches!(a, HotStoreAction::Insert(_)))
    }

    fn set_state(&self, new_state: HotStoreState<C, P, A, K>) {
        let _guard = self.checkpoint_lock.lock().expect("checkpoint lock");
        self.data.clear();
        self.continuations.clear();
        self.installed_continuations.clear();
        self.joins.clear();
        self.installed_joins.clear();
        for (k, v) in new_state.data { self.data.insert(k, v); }
        for (k, v) in new_state.continuations { self.continuations.insert(k, v); }
        for (k, v) in new_state.installed_continuations { self.installed_continuations.insert(k, v); }
        for (k, v) in new_state.joins { self.joins.insert(k, v); }
        for (k, v) in new_state.installed_joins { self.installed_joins.insert(k, v); }
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
        let cont_items: usize = store.continuations.iter().map(|e| e.value().len()).sum();
        let data_items: usize = store.data.iter().map(|e| e.value().len()).sum();
        let joins_items: usize = store.joins.iter().map(|e| e.value().len()).sum();
        let installed_cont_items = store.installed_continuations.len();
        let installed_joins_items: usize = store.installed_joins.iter().map(|e| e.value().len()).sum();
        metrics::gauge!(HOT_STORE_STATE_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.continuations.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.data.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.joins.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.installed_continuations.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_SIZE_METRIC, "source" => RSPACE_METRICS_SOURCE).set(store.installed_joins.len() as f64);
        metrics::gauge!(HOT_STORE_STATE_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_DATA_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(data_items as f64);
        metrics::gauge!(HOT_STORE_STATE_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(joins_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_CONT_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(installed_cont_items as f64);
        metrics::gauge!(HOT_STORE_STATE_INSTALLED_JOINS_ITEMS_METRIC, "source" => RSPACE_METRICS_SOURCE).set(installed_joins_items as f64);
    }

    fn update_history_cache_metrics(store: &InMemHotStore<C, P, A, K>) {
        static LAST_EMIT_AT_MS: AtomicU64 = AtomicU64::new(0);
        if !Self::should_emit_metrics(&LAST_EMIT_AT_MS, Self::history_cache_metrics_update_interval_ms()) {
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
        if self.history_cache_continuations.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES
            || self.history_cache_cont_items.load(Ordering::Relaxed) >= MAX_HISTORY_STORE_CACHE_CONT_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_CONT_METRIC, "source" => RSPACE_METRICS_SOURCE).increment(1);
            self.history_cache_continuations.clear();
            self.history_cache_cont_items.store(0, Ordering::Relaxed);
        }
        if self.history_cache_datums.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES
            || self.history_cache_data_items.load(Ordering::Relaxed) >= MAX_HISTORY_STORE_CACHE_DATA_ITEMS
        {
            metrics::counter!(HOT_STORE_HISTORY_CACHE_BULK_CLEAR_DATUMS_METRIC, "source" => RSPACE_METRICS_SOURCE).increment(1);
            self.history_cache_datums.clear();
            self.history_cache_data_items.store(0, Ordering::Relaxed);
        }
        if self.history_cache_joins.len() >= MAX_HISTORY_STORE_CACHE_ENTRIES
            || self.history_cache_joins_items.load(Ordering::Relaxed) >= MAX_HISTORY_STORE_CACHE_JOIN_ITEMS
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
                self.history_cache_cont_items.fetch_add(ks.len(), Ordering::Relaxed);
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
                self.history_cache_data_items.fetch_add(datums.len(), Ordering::Relaxed);
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
                self.history_cache_joins_items.fetch_add(joins.len(), Ordering::Relaxed);
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
            data: DashMap::new(),
            continuations: DashMap::new(),
            installed_continuations: DashMap::new(),
            joins: DashMap::new(),
            installed_joins: DashMap::new(),
            history_cache_continuations: DashMap::new(),
            history_cache_datums: DashMap::new(),
            history_cache_joins: DashMap::new(),
            history_cache_cont_items: AtomicUsize::new(0),
            history_cache_data_items: AtomicUsize::new(0),
            history_cache_joins_items: AtomicUsize::new(0),
            checkpoint_lock: std::sync::Mutex::new(()),
            history_reader_base: history_reader,
        };
        for (k, v) in cache.data { store.data.insert(k, v); }
        for (k, v) in cache.continuations { store.continuations.insert(k, v); }
        for (k, v) in cache.installed_continuations { store.installed_continuations.insert(k, v); }
        for (k, v) in cache.joins { store.joins.insert(k, v); }
        for (k, v) in cache.installed_joins { store.installed_joins.insert(k, v); }
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
