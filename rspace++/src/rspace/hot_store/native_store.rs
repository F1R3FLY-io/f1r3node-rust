//! D-S1 (D-C2c, DR-96): the digest-keyed store of the native replay session.
//!
//! The store keeps the five maps of the native hot store in digest shards
//! (`native_index.rs`). Each method ports the `InMemHotStore` method of the
//! same name in `native.rs`. The work on the key's own value keeps its
//! legacy charge; the shard walks of the legacy charges (`lookup`,
//! `reserve_replace_with`, `native_insert_new_with`) become the fixed index
//! charges, and keys are compared as digests. The legacy methods stay for
//! the play path and as the test oracle.

use std::mem::size_of;
use std::sync::RwLockWriteGuard;

use shared::rust::clone_backing::CloneBacking;

use super::native::{buffer, continuation_identity_metered, merge};
use super::native_index::{
    DigestShard, DigestShards, DigestSnapshot, KeyLimit, NATIVE_STORE_KEY_BOUND, NativeEntry,
    ORD_CHUNK, StoreKey, ord_levels_bound, ord_node_bytes, view_charge,
};
use super::*;
use crate::rspace::hashing::native_source::GroupKeys;
use crate::rspace::hot_store_action::NativeExportAction;
use crate::rspace::native_backing::{self, arc_allocation_bytes};
use crate::rspace::rspace_interface::RSpaceResult;

type Continuations<P, K> = Vec<Arc<WaitingContinuation<P, K>>>;

/// The charge of storing a key value in a new entry: its copy, its release,
/// and the allocation of its `Arc`.
fn reserve_key<T: CloneBacking>(key: &T, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
    native_backing::reserve_copy_and_cleanup(key, meter)?;
    let bytes = arc_allocation_bytes::<T>().ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(2, bytes, bytes)
}

/// The charge of storing a channel group as the key value of a new entry.
fn reserve_slice_key<T: CloneBacking>(
    key: &[T],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    native_backing::reserve_slice_copy_and_cleanup(key, meter)?;
    let bytes = arc_allocation_bytes::<Vec<T>>().ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(2, bytes, bytes)
}

/// The iteration of a shard with `entries` entries: the cursor stack of
/// imbl's `Cursor::init`, one slot of 16 bytes per level, and one read of
/// every node. An insert-built tree has at most `entries / 8` leaves and
/// `leaves / 7 + levels` branches, so at most `entries / 7 + levels` nodes.
fn reserve_iteration(entries: usize, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
    if entries == 0 {
        return Ok(());
    }
    let levels = ord_levels_bound(entries);
    let stack = levels
        .checked_mul(2 * size_of::<usize>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(levels.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, stack, stack)?;
    let nodes = entries
        .div_ceil(ORD_CHUNK / 2 - 1)
        .checked_add(levels)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let node = ord_node_bytes().ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(
        nodes,
        nodes
            .checked_mul(node)
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )
}

fn key_comparison(meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
    meter.reserve(1, 2 * size_of::<StoreKey>(), 0)
}

/// D-C2e (DR-96): two keys with one digest. The store rejects them, the same
/// way on every replay.
fn collision() -> RSpaceError {
    RSpaceError::InterpreterError("native store digest collision".to_owned())
}

/// D-C2e (DR-96): the entry found by a channel's digest must hold the
/// channel. The check is one comparison of the two channels.
fn check_channel<C: CloneBacking + PartialEq>(
    stored: &C,
    sought: &C,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    native_backing::inspect(stored, meter)?;
    native_backing::inspect(sought, meter)?;
    meter.reserve(1, 0, 0)?;
    if stored == sought {
        Ok(())
    } else {
        Err(collision())
    }
}

/// D-C2e (DR-96): the entry found by a group's digest must hold the group.
/// The check is one comparison of the two channel lists.
fn check_group<C: CloneBacking + PartialEq>(
    stored: &[C],
    sought: &[C],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    native_backing::inspect_slice(stored, meter)?;
    native_backing::inspect_slice(sought, meter)?;
    meter.reserve(1, 0, 0)?;
    if stored == sought {
        Ok(())
    } else {
        Err(collision())
    }
}

struct DataRetirement<'a, C, A: Clone> {
    shards: Vec<(usize, RwLockWriteGuard<'a, DigestShard<C, Vec<Datum<A>>>>)>,
    // Changed by D-C2e (D-S1, DR-96): an update records the data position of
    // its channel, so a repeated key compares the two channels.
    // updates: Vec<(StoreKey, Vec<Datum<A>>)>,
    updates: Vec<(StoreKey, usize, Vec<Datum<A>>)>,
}

impl<C, A: Clone> DataRetirement<'_, C, A> {
    fn empty() -> Self {
        Self {
            shards: Vec::new(),
            updates: Vec::new(),
        }
    }

    fn publish(&mut self) {
        // Changed by D-C2e (D-S1, DR-96): the updates carry positions.
        // for (key, values) in std::mem::take(&mut self.updates) {
        for (key, _, values) in std::mem::take(&mut self.updates) {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(index, _)| *index == key.shard())
                .expect("requested data shard");
            DigestShards::replace(shard, &key, values);
        }
    }
}

struct JoinRetirement<'a, C> {
    shards: Vec<(usize, RwLockWriteGuard<'a, DigestShard<C, Vec<Vec<C>>>>)>,
    // Changed by D-C2e (D-S1, DR-96): an update records the position of its
    // channel, so a repeated key compares the two channels.
    // updates: Vec<(StoreKey, Vec<Vec<C>>)>,
    updates: Vec<(StoreKey, usize, Vec<Vec<C>>)>,
}

impl<C> JoinRetirement<'_, C> {
    fn empty() -> Self {
        Self {
            shards: Vec::new(),
            updates: Vec::new(),
        }
    }

    fn publish(mut self) {
        // Changed by D-C2e (D-S1, DR-96): the updates carry positions.
        // for (key, values) in std::mem::take(&mut self.updates) {
        for (key, _, values) in std::mem::take(&mut self.updates) {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(index, _)| *index == key.shard())
                .expect("requested join shard");
            DigestShards::replace(shard, &key, values);
        }
    }
}

/// Write guards on the requested shards, in ascending order.
fn lock_requested<'a, KV, V>(
    index: &'a DigestShards<KV, V>,
    requested: [bool; NUM_SHARDS],
    meter: &dyn SourceMeter,
) -> Result<Vec<(usize, RwLockWriteGuard<'a, DigestShard<KV, V>>)>, RSpaceError> {
    meter.reserve(NUM_SHARDS, 0, 0)?;
    let shard_count = requested.iter().filter(|needed| **needed).count();
    let guard_bytes = shard_count
        .checked_mul(size_of::<(usize, RwLockWriteGuard<'a, DigestShard<KV, V>>)>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(shard_count, guard_bytes, guard_bytes)?;
    let mut shards = Vec::new();
    shards
        .try_reserve_exact(shard_count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    for (shard, needed) in requested.into_iter().enumerate() {
        if needed {
            shards.push((shard, index.write_shard(shard)));
        }
    }
    Ok(shards)
}

fn update_buffer<T>(count: usize, meter: &dyn SourceMeter) -> Result<Vec<T>, RSpaceError> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count, bytes, bytes)?;
    let mut updates = Vec::new();
    updates
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(updates)
}

/// D-C3 (D-S3, DR-97): the entries of the exported maps that a publishing
/// write changed since the session began, each map in digest order. The
/// pointers share the entries with the store, so the export reads the values
/// by reference.
pub(crate) struct NativeDirtyEntries<C, P: Clone, A: Clone, K: Clone> {
    continuations: Vec<Arc<NativeEntry<Vec<C>, Continuations<P, K>>>>,
    data: Vec<Arc<NativeEntry<C, Vec<Datum<A>>>>>,
    joins: Vec<Arc<NativeEntry<C, Vec<Vec<C>>>>>,
}

/// D-C3 (D-S3, DR-97): the dirty entries of one map, in digest order. The
/// pointer vector has room for every key of the map. Each visited entry
/// charges its visit and the read of its flag, and each dirty entry one
/// pointer copy.
fn dirty_of<KV, V>(
    index: &DigestShards<KV, V>,
    meter: &dyn SourceMeter,
) -> Result<Vec<Arc<NativeEntry<KV, V>>>, RSpaceError> {
    let keys = index.key_count();
    let bytes = keys
        .checked_mul(size_of::<Arc<NativeEntry<KV, V>>>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(
        keys.checked_mul(2)
            .and_then(|work| work.checked_add(1))
            .ok_or(RSpaceError::HostWorkRejected)?,
        bytes,
        bytes,
    )?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(keys)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    for shard_index in 0..NUM_SHARDS {
        let shard = index.read_shard(shard_index);
        reserve_iteration(shard.len(), meter)?;
        for entry in shard.values() {
            meter.reserve(1, 1, 0)?;
            if entry.dirty {
                if entries.len() == entries.capacity() {
                    return Err(RSpaceError::HostWorkRejected);
                }
                view_charge().reserve(meter)?;
                entries.push(Arc::clone(entry));
            }
        }
    }
    Ok(entries)
}

impl<C, P: Clone, A: Clone, K: Clone> NativeDirtyEntries<C, P, A, K> {
    /// The export's changes by reference: continuations, data and joins,
    /// each map in digest order. An empty value is a deletion.
    pub(crate) fn actions(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<NativeExportAction<'_, C, A, Arc<WaitingContinuation<P, K>>>>, RSpaceError>
    {
        let count = self
            .continuations
            .len()
            .checked_add(self.data.len())
            .and_then(|count| count.checked_add(self.joins.len()))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let mut actions = buffer(count, meter)?;
        let continuation_entry = size_of::<NativeEntry<Vec<C>, Continuations<P, K>>>();
        for entry in &self.continuations {
            meter.reserve(1, continuation_entry, 0)?;
            actions.push(if entry.value.is_empty() {
                NativeExportAction::DeleteContinuations {
                    channels: entry.key.as_slice(),
                }
            } else {
                NativeExportAction::InsertContinuations {
                    channels: entry.key.as_slice(),
                    continuations: &entry.value,
                }
            });
        }
        let data_entry = size_of::<NativeEntry<C, Vec<Datum<A>>>>();
        for entry in &self.data {
            meter.reserve(1, data_entry, 0)?;
            actions.push(if entry.value.is_empty() {
                NativeExportAction::DeleteData {
                    channel: entry.key.as_ref(),
                }
            } else {
                NativeExportAction::InsertData {
                    channel: entry.key.as_ref(),
                    data: &entry.value,
                }
            });
        }
        let join_entry = size_of::<NativeEntry<C, Vec<Vec<C>>>>();
        for entry in &self.joins {
            meter.reserve(1, join_entry, 0)?;
            actions.push(if entry.value.is_empty() {
                NativeExportAction::DeleteJoins {
                    channel: entry.key.as_ref(),
                }
            } else {
                NativeExportAction::InsertJoins {
                    channel: entry.key.as_ref(),
                    joins: &entry.value,
                }
            });
        }
        Ok(actions)
    }
}

pub(crate) struct NativeHotStore<C, P: Clone, A: Clone, K: Clone> {
    data: DigestShards<C, Vec<Datum<A>>>,
    continuations: DigestShards<Vec<C>, Continuations<P, K>>,
    installed_continuations: DigestShards<Vec<C>, Arc<WaitingContinuation<P, K>>>,
    joins: DigestShards<C, Vec<Vec<C>>>,
    installed_joins: DigestShards<C, Vec<Vec<C>>>,
}

/// The five maps of a [`NativeHotStore`] at a checkpoint.
pub(crate) struct NativeStoreState<C, P: Clone, A: Clone, K: Clone> {
    data: DigestSnapshot<C, Vec<Datum<A>>>,
    continuations: DigestSnapshot<Vec<C>, Continuations<P, K>>,
    installed_continuations: DigestSnapshot<Vec<C>, Arc<WaitingContinuation<P, K>>>,
    joins: DigestSnapshot<C, Vec<Vec<C>>>,
    installed_joins: DigestSnapshot<C, Vec<Vec<C>>>,
}

impl<C, P: Clone, A: Clone, K: Clone> Default for NativeHotStore<C, P, A, K> {
    fn default() -> Self { Self::new() }
}

impl<C, P: Clone, A: Clone, K: Clone> NativeHotStore<C, P, A, K> {
    // Changed by D-C2e (D-S1, DR-96): the five maps share one key limit.
    // pub(crate) fn new() -> Self {
    //     Self {
    //         data: DigestShards::new(),
    //         continuations: DigestShards::new(),
    //         installed_continuations: DigestShards::new(),
    //         joins: DigestShards::new(),
    //         installed_joins: DigestShards::new(),
    //     }
    // }
    pub(crate) fn new() -> Self { Self::with_key_bound(NATIVE_STORE_KEY_BOUND) }

    /// D-C2e (DR-96): a store whose five maps hold at most `bound` keys
    /// together.
    pub(crate) fn with_key_bound(bound: usize) -> Self {
        let limit = Arc::new(KeyLimit::new(bound));
        Self {
            data: DigestShards::with_limit(Arc::clone(&limit)),
            continuations: DigestShards::with_limit(Arc::clone(&limit)),
            installed_continuations: DigestShards::with_limit(Arc::clone(&limit)),
            joins: DigestShards::with_limit(Arc::clone(&limit)),
            installed_joins: DigestShards::with_limit(limit),
        }
    }

    /// The operations and bytes of a new store in its `Arc`.
    pub(crate) fn constructor_layout() -> Option<(usize, usize)> {
        let layouts = [
            DigestShards::<C, Vec<Datum<A>>>::constructor_layout(),
            DigestShards::<Vec<C>, Continuations<P, K>>::constructor_layout(),
            DigestShards::<Vec<C>, Arc<WaitingContinuation<P, K>>>::constructor_layout(),
            DigestShards::<C, Vec<Vec<C>>>::constructor_layout(),
            DigestShards::<C, Vec<Vec<C>>>::constructor_layout(),
        ];
        let mut operations: usize = 2;
        let mut bytes = arc_allocation_bytes::<Self>()?;
        // D-C2e (DR-96): the key limit that the five maps share, and one
        // pointer clone for each map.
        operations = operations.checked_add(2)?.checked_add(layouts.len())?;
        bytes = bytes.checked_add(arc_allocation_bytes::<KeyLimit>()?)?;
        for (layout_operations, layout_bytes) in layouts {
            operations = operations.checked_add(layout_operations)?;
            bytes = bytes.checked_add(layout_bytes)?;
        }
        Some((operations, bytes))
    }

    /// The shard count and the bytes of one checkpoint state.
    pub(crate) fn snapshot_layout() -> (usize, usize) {
        let layouts = [
            DigestShards::<C, Vec<Datum<A>>>::snapshot_layout(),
            DigestShards::<Vec<C>, Continuations<P, K>>::snapshot_layout(),
            DigestShards::<Vec<C>, Arc<WaitingContinuation<P, K>>>::snapshot_layout(),
            DigestShards::<C, Vec<Vec<C>>>::snapshot_layout(),
            DigestShards::<C, Vec<Vec<C>>>::snapshot_layout(),
        ];
        layouts
            .into_iter()
            .fold((0, 0), |(shards, bytes), (layout_shards, layout_bytes)| {
                (shards + layout_shards, bytes + layout_bytes)
            })
    }

    /// The five maps at this point. The caller holds the session's
    /// exclusive gate.
    pub(crate) fn snapshot(&self) -> NativeStoreState<C, P, A, K> {
        NativeStoreState {
            data: self.data.snapshot(),
            continuations: self.continuations.snapshot(),
            installed_continuations: self.installed_continuations.snapshot(),
            joins: self.joins.snapshot(),
            installed_joins: self.installed_joins.snapshot(),
        }
    }

    pub(crate) fn restore(&self, state: NativeStoreState<C, P, A, K>) {
        self.data.restore(state.data);
        self.continuations.restore(state.continuations);
        self.installed_continuations
            .restore(state.installed_continuations);
        self.joins.restore(state.joins);
        self.installed_joins.restore(state.installed_joins);
    }

    /// The distinct keys of the data, continuation, installed continuation,
    /// join and installed join maps.
    #[cfg(test)]
    pub(crate) fn entry_counts(&self) -> [usize; 5] {
        [
            self.data.key_count(),
            self.continuations.key_count(),
            self.installed_continuations.key_count(),
            self.joins.key_count(),
            self.installed_joins.key_count(),
        ]
    }

    /// The keys that the shared key limit counts (D-C2e).
    #[cfg(test)]
    pub(crate) fn keys_used(&self) -> usize { self.data.limit_used() }
}

impl<C, P, A, K> NativeHotStore<C, P, A, K>
where
    C: Clone + Debug + PartialEq + CloneBacking + Send + Sync,
    P: Clone + Debug + CloneBacking + Send + Sync,
    A: Clone + Debug + CloneBacking + Send + Sync,
    K: Clone + Debug + CloneBacking + Send + Sync,
{
    /// The data of `channel` (port of `InMemHotStore::native_data`).
    pub(crate) fn data(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        let warm = {
            let shard = self.data.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel, meter)?;
                    native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    Some(entry.value.clone())
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return Ok(values);
        }
        let values = read()?;
        native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let cached = values.clone();
        reserve_key(channel, meter)?;
        let mut shard = self.data.write(&key);
        self.data
            .insert_new(&mut shard, key, Arc::new(channel.clone()), cached, false, meter)?;
        Ok(values)
    }

    /// A copy-free view of the data of `channel` (port of
    /// `InMemHotStore::native_data_view`). A warm read clones the entry
    /// pointer; a cold read prepays the release of the decoded data and moves
    /// them into the store.
    pub(crate) fn data_view(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<NativeDataView<C, A>, RSpaceError> {
        let warm = {
            let shard = self.data.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel, meter)?;
                    view_charge().reserve(meter)?;
                    Some(Arc::clone(entry))
                }
                None => None,
            }
        };
        if let Some(entry) = warm {
            return Ok(NativeDataView::from_entry(entry));
        }
        let values = read()?;
        native_backing::reserve_cleanup(&values, meter)?;
        reserve_key(channel, meter)?;
        let mut shard = self.data.write(&key);
        let entry = self.data.insert_new(
            &mut shard,
            key,
            Arc::new(channel.clone()),
            values,
            false,
            meter,
        )?;
        Ok(NativeDataView::from_entry(entry))
    }

    /// The joins of `channel`, installed joins first (port of
    /// `InMemHotStore::native_joins`).
    pub(crate) fn joins(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Vec<C>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<Vec<C>>, RSpaceError> {
        let installed = {
            let shard = self.installed_joins.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel, meter)?;
                    native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    entry.value.clone()
                }
                None => Vec::new(),
            }
        };
        let warm = {
            let shard = self.joins.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel, meter)?;
                    native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    Some(entry.value.clone())
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return merge(installed, values, meter);
        }
        let values = read()?;
        native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let cached = values.clone();
        let result = merge(installed, values, meter)?;
        reserve_key(channel, meter)?;
        let mut shard = self.joins.write(&key);
        self.joins
            .insert_new(&mut shard, key, Arc::new(channel.clone()), cached, false, meter)?;
        Ok(result)
    }

    // Changed by D-C2e (D-S1, DR-96): the lookup takes the group's channels
    // for the collision check.
    // /// The installed continuation of a group, as a deep copy, and the
    // /// search that found it.
    // fn installed_copy(
    //     &self,
    //     key: &StoreKey,
    //     meter: &dyn SourceMeter,
    // ) -> Result<Option<WaitingContinuation<P, K>>, RSpaceError> {
    //     let shard = self.installed_continuations.read(key);
    //     match DigestShards::get(&shard, key, meter)? {
    //         Some(entry) => {
    //             native_backing::reserve_copy_and_cleanup(entry.value.as_ref(),
    // meter)?;             Ok(Some(entry.value.as_ref().clone()))
    //         }
    //         None => Ok(None),
    //     }
    // }
    /// The installed continuation of a group, as a deep copy, and the
    /// search that found it.
    fn installed_copy(
        &self,
        channels: &[C],
        key: &StoreKey,
        meter: &dyn SourceMeter,
    ) -> Result<Option<WaitingContinuation<P, K>>, RSpaceError> {
        let shard = self.installed_continuations.read(key);
        match DigestShards::get(&shard, key, meter)? {
            Some(entry) => {
                // D-C2e (DR-96): the entry must hold the sought group.
                check_group(entry.key.as_ref(), channels, meter)?;
                native_backing::reserve_copy_and_cleanup(entry.value.as_ref(), meter)?;
                Ok(Some(entry.value.as_ref().clone()))
            }
            None => Ok(None),
        }
    }

    /// The continuations of `channels`, the installed continuation first
    /// (port of `InMemHotStore::native_continuations`).
    pub(crate) fn continuations(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        let key = keys.group;
        // Changed by D-C2e (D-S1, DR-96): the lookup checks the group.
        // let installed = self.installed_copy(&key, meter)?;
        let installed = self.installed_copy(channels, &key, meter)?;
        let mut prefix = buffer(usize::from(installed.is_some()), meter)?;
        if let Some(installed) = installed {
            prefix.push(installed);
        }
        let warm = {
            let shard = self.continuations.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels, meter)?;
                    let mut result = buffer(entry.value.len(), meter)?;
                    for value in &entry.value {
                        native_backing::reserve_copy_and_cleanup(value.as_ref(), meter)?;
                        result.push(value.as_ref().clone());
                    }
                    Some(result)
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return merge(prefix, values, meter);
        }
        let values = read()?;
        let allocation = arc_allocation_bytes::<WaitingContinuation<P, K>>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        let mut cached = buffer(values.len(), meter)?;
        for value in &values {
            native_backing::reserve_copy_and_cleanup(value, meter)?;
            meter.reserve(2, allocation, allocation)?;
            cached.push(Arc::new(value.clone()));
        }
        let result = merge(prefix, values, meter)?;
        reserve_slice_key(channels, meter)?;
        let mut shard = self.continuations.write(&key);
        self.continuations.insert_new(
            &mut shard,
            key,
            Arc::new(channels.to_vec()),
            cached,
            false,
            meter,
        )?;
        Ok(result)
    }

    /// The continuations of `channels` as shared views, the installed
    /// continuation first (port of `InMemHotStore::native_continuation_views`).
    /// The installed continuation is stored as an `Arc`, so its view is a
    /// pointer copy.
    pub(crate) fn continuation_views(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Continuations<P, K>, RSpaceError> {
        let key = keys.group;
        let pointer = size_of::<Arc<WaitingContinuation<P, K>>>();
        let allocation = arc_allocation_bytes::<WaitingContinuation<P, K>>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        let installed = {
            let shard = self.installed_continuations.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels, meter)?;
                    meter.reserve(1, pointer, 0)?;
                    Some(Arc::clone(&entry.value))
                }
                None => None,
            }
        };
        let mut prefix = buffer(usize::from(installed.is_some()), meter)?;
        if let Some(installed) = installed {
            prefix.push(installed);
        }
        let warm = {
            let shard = self.continuations.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels, meter)?;
                    let mut result = buffer(entry.value.len(), meter)?;
                    for value in &entry.value {
                        meter.reserve(1, pointer, 0)?;
                        result.push(Arc::clone(value));
                    }
                    Some(result)
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return merge(prefix, values, meter);
        }
        let values = read()?;
        let mut shared = buffer(values.len(), meter)?;
        let mut cached = buffer(values.len(), meter)?;
        for value in values {
            native_backing::reserve_cleanup(&value, meter)?;
            meter.reserve(2, allocation, allocation)?;
            let value = Arc::new(value);
            meter.reserve(1, pointer, 0)?;
            cached.push(Arc::clone(&value));
            shared.push(value);
        }
        let result = merge(prefix, shared, meter)?;
        reserve_slice_key(channels, meter)?;
        let mut shard = self.continuations.write(&key);
        self.continuations.insert_new(
            &mut shard,
            key,
            Arc::new(channels.to_vec()),
            cached,
            false,
            meter,
        )?;
        Ok(result)
    }

    /// Stores a waiting continuation and its joins (port of
    /// `InMemHotStore::native_store_consume`). Returns whether the
    /// continuation was new, and the number of continuations of its group.
    pub(crate) fn store_consume(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        waiting: WaitingContinuation<P, K>,
        meter: &dyn SourceMeter,
    ) -> Result<(bool, usize), RSpaceError> {
        native_backing::reserve_cleanup(&waiting, meter)?;
        if keys.channels.len() != channels.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        let key = keys.group;
        let mut identity: Option<String> = None;
        let installed = {
            let shard = self.installed_continuations.read(&key);
            // Changed by D-C2e (D-S1, DR-96): a found entry must hold the
            // group.
            // usize::from(DigestShards::get(&shard, &key, meter)?.is_some())
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    check_group(entry.key.as_ref(), channels, meter)?;
                    1
                }
                None => 0,
            }
        };
        let mut continuation_shard = self.continuations.write(&key);
        // Changed by D-C2e (D-S1, DR-96): the entry must hold the group.
        // let existing = &DigestShards::get(&continuation_shard, &key, meter)?
        //     .ok_or_else(|| {
        //         RSpaceError::InterpreterError(
        //             "native continuation cache is absent at publication".to_owned(),
        //         )
        //     })?
        //     .value;
        let entry = DigestShards::get(&continuation_shard, &key, meter)?.ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at publication".to_owned(),
            )
        })?;
        check_group(entry.key.as_ref(), channels, meter)?;
        let existing = &entry.value;
        let mut duplicate = false;
        // D-S4 (DR-90): identities are built and compared only for stored
        // continuations with the same source hash.
        let hash_bytes = waiting
            .source
            .hash
            .0
            .len()
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?;
        for value in existing {
            meter.reserve(1, hash_bytes, 0)?;
            if value.source.hash != waiting.source.hash {
                continue;
            }
            if identity.is_none() {
                identity = Some(continuation_identity_metered(&waiting, meter)?);
            }
            let identity = identity.as_ref().expect("identity built above");
            let prior = continuation_identity_metered(value, meter)?;
            meter.reserve(
                1,
                identity
                    .len()
                    .checked_add(prior.len())
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            if prior == *identity {
                duplicate = true;
                break;
            }
        }
        let depth = existing
            .len()
            .checked_add(usize::from(!duplicate))
            .and_then(|count| count.checked_add(installed))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let continuation_update = if duplicate {
            None
        } else {
            DigestShards::<Vec<C>, Continuations<P, K>>::reserve_replace(meter)?;
            native_backing::reserve_shared_copy_and_cleanup(existing, meter)?;
            let mut values = existing.clone();
            let count = values
                .len()
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let bytes = count
                .checked_mul(size_of::<Arc<WaitingContinuation<P, K>>>())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(count, bytes, bytes)?;
            values
                .try_reserve_exact(1)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            let arc_bytes = arc_allocation_bytes::<WaitingContinuation<P, K>>()
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(2, arc_bytes, arc_bytes)?;
            values.insert(0, Arc::new(waiting));
            Some(values)
        };

        meter.reserve(keys.channels.len(), 0, 0)?;
        let mut requested = [false; NUM_SHARDS];
        for channel_key in &keys.channels {
            requested[channel_key.shard()] = true;
        }
        let mut join_shards = lock_requested(&self.joins, requested, meter)?;
        // Changed by D-C2e (D-S1, DR-96): a staged update records the position
        // of its channel, so a repeated key compares the two channels.
        // let mut join_updates: Vec<(StoreKey, Vec<Vec<C>>)> =
        //     update_buffer(keys.channels.len(), meter)?;
        // for channel_key in &keys.channels {
        //     let mut repeated = false;
        //     for (prior, _) in &join_updates {
        //         key_comparison(meter)?;
        //         if prior == channel_key {
        //             repeated = true;
        //             break;
        //         }
        //     }
        let mut join_updates: Vec<(StoreKey, usize, Vec<Vec<C>>)> =
            update_buffer(keys.channels.len(), meter)?;
        for (position, (channel, channel_key)) in channels.iter().zip(&keys.channels).enumerate() {
            let mut repeated = false;
            for (prior, prior_position, _) in &join_updates {
                key_comparison(meter)?;
                if prior == channel_key {
                    check_channel(&channels[*prior_position], channel, meter)?;
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            meter.reserve(
                join_updates
                    .len()
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
                0,
            )?;
            let index = channel_key.shard();
            meter.reserve(join_shards.len(), 0, 0)?;
            let (_, shard) = join_shards
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            // Changed by D-C2e (D-S1, DR-96): the entry must hold the channel.
            // let values = &DigestShards::get(shard, channel_key, meter)?
            //     .ok_or_else(|| {
            //         RSpaceError::InterpreterError(
            //             "native join cache is absent at publication".to_owned(),
            //         )
            //     })?
            //     .value;
            let entry = DigestShards::get(shard, channel_key, meter)?.ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at publication".to_owned(),
                )
            })?;
            check_channel(entry.key.as_ref(), channel, meter)?;
            let values = &entry.value;
            let mut present = false;
            for join in values {
                native_backing::inspect_slice(join, meter)?;
                native_backing::inspect_slice(channels, meter)?;
                meter.reserve(1, 0, 0)?;
                if join.as_slice() == channels {
                    present = true;
                    break;
                }
            }
            if present {
                continue;
            }
            DigestShards::<C, Vec<Vec<C>>>::reserve_replace(meter)?;
            native_backing::reserve_copy_and_cleanup(values, meter)?;
            let mut updated = values.clone();
            let count = updated
                .len()
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let bytes = count
                .checked_mul(size_of::<Vec<C>>())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(count, bytes, bytes)?;
            updated
                .try_reserve_exact(1)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
            updated.insert(0, channels.to_vec());
            // Changed by D-C2e (D-S1, DR-96): the update records its position.
            // join_updates.push((*channel_key, updated));
            join_updates.push((*channel_key, position, updated));
        }
        if let Some(values) = continuation_update {
            DigestShards::replace(&mut continuation_shard, &key, values);
        }
        // Changed by D-C2e (D-S1, DR-96): the updates carry positions.
        // for (channel_key, values) in join_updates {
        for (channel_key, _, values) in join_updates {
            let index = channel_key.shard();
            let (_, shard) = join_shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            DigestShards::replace(shard, &channel_key, values);
        }
        Ok((!duplicate, depth))
    }

    // Changed by D-C2e (D-S1, DR-96): the publication takes the channel for
    // the collision check.
    // /// Adds a datum to the cached data of a channel (port of
    // /// `InMemHotStore::native_put_datum`).
    // pub(crate) fn put_datum(
    //     &self,
    //     key: StoreKey,
    //     datum: Datum<A>,
    //     meter: &dyn SourceMeter,
    // ) -> Result<(), RSpaceError> {
    //     let mut shard = self.data.write(&key);
    //     let values = &DigestShards::get(&shard, &key, meter)?
    //         .ok_or_else(|| {
    //             RSpaceError::InterpreterError(
    //                 "native datum cache is absent at publication".to_owned(),
    //             )
    //         })?
    //         .value;
    /// Adds a datum to the cached data of a channel (port of
    /// `InMemHotStore::native_put_datum`).
    pub(crate) fn put_datum(
        &self,
        channel: &C,
        key: StoreKey,
        datum: Datum<A>,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        let mut shard = self.data.write(&key);
        let entry = DigestShards::get(&shard, &key, meter)?.ok_or_else(|| {
            RSpaceError::InterpreterError("native datum cache is absent at publication".to_owned())
        })?;
        // D-C2e (DR-96): the entry must hold the channel.
        check_channel(entry.key.as_ref(), channel, meter)?;
        let values = &entry.value;
        let count = values
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let vector_bytes = count
            .checked_mul(size_of::<Datum<A>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(count, vector_bytes, vector_bytes)?;
        native_backing::reserve_cleanup(&datum, meter)?;
        DigestShards::<C, Vec<Datum<A>>>::reserve_replace(meter)?;
        native_backing::reserve_copy_and_cleanup(values, meter)?;
        let mut updated = Vec::new();
        updated
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        updated.push(datum);
        updated.extend(values.iter().cloned());
        DigestShards::replace(&mut shard, &key, updated);
        Ok(())
    }

    /// Removes matched data (port of `InMemHotStore::native_retire_data`).
    /// `keys[position]` is the key of `data[position].channel`.
    pub(crate) fn retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        keys: &[StoreKey],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        self.prepare_retire_data(data, keys, retirement, meter)?
            .publish();
        Ok(())
    }

    fn prepare_retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        keys: &[StoreKey],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<DataRetirement<'_, C, A>, RSpaceError> {
        if retirement.is_empty() {
            return Ok(DataRetirement::empty());
        }
        if keys.len() != data.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        let mut requested = [false; NUM_SHARDS];
        for (position, _) in retirement {
            let key = keys.get(*position).ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(1, 0, 0)?;
            requested[key.shard()] = true;
        }
        let shards = lock_requested(&self.data, requested, meter)?;
        // Changed by D-C2e (D-S1, DR-96): an update records the data position
        // of its channel; a repeated key compares the two channels, and a
        // found entry must hold the channel.
        // let mut updates: Vec<(StoreKey, Vec<Datum<A>>)> =
        // update_buffer(retirement.len(), meter)?; for (position, datum_index)
        // in retirement {     let key = keys[*position];
        //     let mut staged = None;
        //     for (index, (existing, _)) in updates.iter().enumerate() {
        //         key_comparison(meter)?;
        //         if *existing == key {
        //             staged = Some(index);
        //             break;
        //         }
        //     }
        //     let staged = if let Some(index) = staged {
        //         index
        //     } else {
        //         meter.reserve(shards.len(), 0, 0)?;
        //         let (_, shard) = shards
        //             .iter()
        //             .find(|(index, _)| *index == key.shard())
        //             .expect("requested data shard");
        //         let values = &DigestShards::get(shard, &key, meter)?
        //             .ok_or_else(|| {
        //                 RSpaceError::InterpreterError(
        //                     "native datum cache is absent at retirement".to_owned(),
        //                 )
        //             })?
        //             .value;
        //         DigestShards::<C, Vec<Datum<A>>>::reserve_replace(meter)?;
        //         native_backing::reserve_copy_and_cleanup(values, meter)?;
        //         updates.push((key, values.clone()));
        //         updates.len() - 1
        //     };
        //     let values = &mut updates[staged].1;
        let mut updates: Vec<(StoreKey, usize, Vec<Datum<A>>)> =
            update_buffer(retirement.len(), meter)?;
        for (position, datum_index) in retirement {
            let key = keys[*position];
            let channel = &data[*position].channel;
            let mut staged = None;
            for (index, (existing, staged_position, _)) in updates.iter().enumerate() {
                key_comparison(meter)?;
                if *existing == key {
                    check_channel(&data[*staged_position].channel, channel, meter)?;
                    staged = Some(index);
                    break;
                }
            }
            let staged = if let Some(index) = staged {
                index
            } else {
                meter.reserve(shards.len(), 0, 0)?;
                let (_, shard) = shards
                    .iter()
                    .find(|(index, _)| *index == key.shard())
                    .expect("requested data shard");
                let entry = DigestShards::get(shard, &key, meter)?.ok_or_else(|| {
                    RSpaceError::InterpreterError(
                        "native datum cache is absent at retirement".to_owned(),
                    )
                })?;
                check_channel(entry.key.as_ref(), channel, meter)?;
                let values = &entry.value;
                DigestShards::<C, Vec<Datum<A>>>::reserve_replace(meter)?;
                native_backing::reserve_copy_and_cleanup(values, meter)?;
                updates.push((key, *position, values.clone()));
                updates.len() - 1
            };
            let values = &mut updates[staged].2;
            if *datum_index < 0 || *datum_index as usize >= values.len() {
                return Err(RSpaceError::InterpreterError(
                    "native datum retirement index is invalid".to_owned(),
                ));
            }
            let index = *datum_index as usize;
            native_backing::reserve_cleanup(&values[index], meter)?;
            let moved = values
                .len()
                .checked_sub(index)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let bytes = moved
                .checked_mul(size_of::<Datum<A>>())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(moved, bytes, 0)?;
            values.remove(index);
        }
        Ok(DataRetirement { shards, updates })
    }

    /// Removes the data and the continuation of a produce match (port of
    /// `InMemHotStore::native_retire_produce_match`). `keys` are the keys of
    /// the matched continuation's group; data position `i` is channel `i`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retire_produce_match(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        continuation_index: i32,
        continuation_persistent: bool,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        let mut data_update = self.prepare_retire_data(data, &keys.channels, retirement, meter)?;
        let key = keys.group;
        // Changed by D-C2e (D-S1, DR-96): a found entry must hold the group.
        // let installed = {
        //     let shard = self.installed_continuations.read(&key);
        //     DigestShards::get(&shard, &key, meter)?.is_some()
        // };
        // let mut continuation_shard = self.continuations.write(&key);
        // let existing = &DigestShards::get(&continuation_shard, &key, meter)?
        //     .ok_or_else(|| {
        //         RSpaceError::InterpreterError(
        //             "native continuation cache is absent at match
        // retirement".to_owned(),         )
        //     })?
        //     .value;
        let installed = {
            let shard = self.installed_continuations.read(&key);
            match DigestShards::get(&shard, &key, meter)? {
                Some(entry) => {
                    check_group(entry.key.as_ref(), channels, meter)?;
                    true
                }
                None => false,
            }
        };
        let mut continuation_shard = self.continuations.write(&key);
        let entry = DigestShards::get(&continuation_shard, &key, meter)?.ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at match retirement".to_owned(),
            )
        })?;
        check_group(entry.key.as_ref(), channels, meter)?;
        let existing = &entry.value;
        let continuation_update = if continuation_persistent {
            None
        } else {
            let index = continuation_index
                .checked_sub(i32::from(installed))
                .ok_or(RSpaceError::HostWorkRejected)?;
            if index < 0 || index as usize >= existing.len() {
                return Err(RSpaceError::InterpreterError(
                    "native continuation retirement index is invalid".to_owned(),
                ));
            }
            DigestShards::<Vec<C>, Continuations<P, K>>::reserve_replace(meter)?;
            native_backing::reserve_shared_copy_and_cleanup(existing, meter)?;
            let mut values = existing.clone();
            let index = index as usize;
            native_backing::reserve_shared_cleanup(&values[index], meter)?;
            let moved = values
                .len()
                .checked_sub(index)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let bytes = moved
                .checked_mul(size_of::<Arc<WaitingContinuation<P, K>>>())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(moved, bytes, 0)?;
            values.remove(index);
            Some(values)
        };
        let regular_count = continuation_update.as_ref().unwrap_or(existing).len();
        let join_update = if !installed && regular_count == 0 {
            self.prepare_retire_joins(data, channels, &keys.channels, meter)?
        } else {
            JoinRetirement::empty()
        };
        data_update.publish();
        if let Some(values) = continuation_update {
            DigestShards::replace(&mut continuation_shard, &key, values);
        }
        join_update.publish();
        drop(data_update);
        Ok(())
    }

    fn prepare_retire_joins(
        &self,
        data: &[RSpaceResult<C, A>],
        join: &[C],
        keys: &[StoreKey],
        meter: &dyn SourceMeter,
    ) -> Result<JoinRetirement<'_, C>, RSpaceError> {
        // Changed by D-C2e (D-S1, DR-96): key `i` is the key of `join[i]`, the
        // channel that the collision check compares.
        // if keys.len() != data.len() {
        //     return Err(RSpaceError::HostWorkRejected);
        // }
        if keys.len() != data.len() || keys.len() != join.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        let mut requested = [false; NUM_SHARDS];
        for key in keys {
            meter.reserve(1, 0, 0)?;
            requested[key.shard()] = true;
        }
        let shards = lock_requested(&self.joins, requested, meter)?;
        // Changed by D-C2e (D-S1, DR-96): an update records the position of its
        // channel; a repeated key compares the two channels, and a found entry
        // must hold the channel.
        // let mut updates: Vec<(StoreKey, Vec<Vec<C>>)> = update_buffer(keys.len(),
        // meter)?; for key in keys {
        //     let mut repeated = false;
        //     for (previous, _) in &updates {
        //         key_comparison(meter)?;
        //         if previous == key {
        //             repeated = true;
        //             break;
        //         }
        //     }
        let mut updates: Vec<(StoreKey, usize, Vec<Vec<C>>)> = update_buffer(keys.len(), meter)?;
        for (channel_position, (channel, key)) in join.iter().zip(keys).enumerate() {
            let mut repeated = false;
            for (previous, previous_position, _) in &updates {
                key_comparison(meter)?;
                if previous == key {
                    check_channel(&join[*previous_position], channel, meter)?;
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            meter.reserve(shards.len(), 0, 0)?;
            let (_, shard) = shards
                .iter()
                .find(|(candidate, _)| *candidate == key.shard())
                .expect("requested join shard");
            // let existing = &DigestShards::get(shard, key, meter)?
            //     .ok_or_else(|| {
            //         RSpaceError::InterpreterError(
            //             "native join cache is absent at match retirement".to_owned(),
            //         )
            //     })?
            //     .value;
            let entry = DigestShards::get(shard, key, meter)?.ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at match retirement".to_owned(),
                )
            })?;
            check_channel(entry.key.as_ref(), channel, meter)?;
            let existing = &entry.value;
            let mut position = None;
            for (candidate, value) in existing.iter().enumerate() {
                native_backing::inspect_slice(value, meter)?;
                native_backing::inspect_slice(join, meter)?;
                meter.reserve(1, 0, 0)?;
                if value.as_slice() == join {
                    position = Some(candidate);
                    break;
                }
            }
            if let Some(position) = position {
                DigestShards::<C, Vec<Vec<C>>>::reserve_replace(meter)?;
                native_backing::reserve_copy_and_cleanup(existing, meter)?;
                let mut values = existing.clone();
                native_backing::reserve_cleanup(&values[position], meter)?;
                let moved = values
                    .len()
                    .checked_sub(position)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let bytes = moved
                    .checked_mul(size_of::<Vec<C>>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                meter.reserve(moved, bytes, 0)?;
                values.remove(position);
                // Changed by D-C2e (D-S1, DR-96): the update records its
                // position.
                // updates.push((*key, values));
                updates.push((*key, channel_position, values));
            }
        }
        Ok(JoinRetirement { shards, updates })
    }

    /// Installs a continuation (port of
    /// `InMemHotStore::native_install_continuation`). It is stored as an
    /// `Arc`, so a view of it is a pointer copy.
    pub(crate) fn install_continuation(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        value: WaitingContinuation<P, K>,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        native_backing::reserve_cleanup(&value, meter)?;
        let allocation = arc_allocation_bytes::<WaitingContinuation<P, K>>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(2, allocation, allocation)?;
        let key = keys.group;
        let mut shard = self.installed_continuations.write(&key);
        // Changed by D-C2e (D-S1, DR-96): a found entry must hold the group.
        // if DigestShards::get(&shard, &key, meter)?.is_some() {
        let found = match DigestShards::get(&shard, &key, meter)? {
            Some(entry) => {
                check_group(entry.key.as_ref(), channels, meter)?;
                true
            }
            None => false,
        };
        if found {
            DigestShards::<Vec<C>, Arc<WaitingContinuation<P, K>>>::reserve_replace(meter)?;
            DigestShards::replace(&mut shard, &key, Arc::new(value));
        } else {
            reserve_slice_key(channels, meter)?;
            self.installed_continuations.insert_new(
                &mut shard,
                key,
                Arc::new(channels.to_vec()),
                Arc::new(value),
                true,
                meter,
            )?;
        }
        Ok(())
    }

    /// Installs a join of `channel` (port of
    /// `InMemHotStore::native_install_join`).
    pub(crate) fn install_join(
        &self,
        channel: &C,
        key: StoreKey,
        join: &[C],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        let mut shard = self.installed_joins.write(&key);
        let existing = DigestShards::get(&shard, &key, meter)?;
        let present = existing.is_some();
        let mut values = match existing {
            Some(entry) => {
                // D-C2e (DR-96): the entry must hold the channel.
                check_channel(entry.key.as_ref(), channel, meter)?;
                native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                entry.value.clone()
            }
            None => Vec::new(),
        };
        for existing in &values {
            native_backing::inspect_slice(existing, meter)?;
            native_backing::inspect_slice(join, meter)?;
            meter.reserve(1, 0, 0)?;
            if existing.as_slice() == join {
                return Ok(());
            }
        }
        let count = values
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let bytes = count
            .checked_mul(size_of::<Vec<C>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(count, bytes, bytes)?;
        values
            .try_reserve_exact(1)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        native_backing::reserve_slice_copy_and_cleanup(join, meter)?;
        values.insert(0, join.to_vec());
        if present {
            DigestShards::<C, Vec<Vec<C>>>::reserve_replace(meter)?;
            DigestShards::replace(&mut shard, &key, values);
        } else {
            reserve_key(channel, meter)?;
            self.installed_joins.insert_new(
                &mut shard,
                key,
                Arc::new(channel.clone()),
                values,
                true,
                meter,
            )?;
        }
        Ok(())
    }

    /// D-C3 (D-S3, DR-97): the dirty entries of the exported maps, for the
    /// export by reference. The session's exclusive gate serializes the
    /// export with every write.
    pub(crate) fn dirty_entries(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<NativeDirtyEntries<C, P, A, K>, RSpaceError> {
        meter.reserve(NUM_SHARDS * 3, 0, 0)?;
        Ok(NativeDirtyEntries {
            continuations: dirty_of(&self.continuations, meter)?,
            data: dirty_of(&self.data, meter)?,
            joins: dirty_of(&self.joins, meter)?,
        })
    }

    // Changed by D-C3 (D-S3, DR-97): the export borrows cached values and emits
    // dirty keys only (`dirty_entries`). The full export of deep copies stays
    // as the test oracle of the root-equality tests.
    /// The store's changes for the history: every continuation, data and
    /// join entry, each map in digest order (port of
    /// `InMemHotStore::native_changes`). The session's exclusive gate
    /// serializes the export with every write.
    #[cfg(test)]
    pub(crate) fn changes(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError> {
        meter.reserve(NUM_SHARDS * 3, 0, 0)?;
        let count = self
            .continuations
            .key_count()
            .checked_add(self.data.key_count())
            .and_then(|count| count.checked_add(self.joins.key_count()))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let bytes = count
            .checked_mul(size_of::<HotStoreAction<C, P, A, K>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            count
                .checked_mul(2)
                .and_then(|work| work.checked_add(1))
                .ok_or(RSpaceError::HostWorkRejected)?,
            bytes,
            bytes,
        )?;
        let mut actions = Vec::new();
        actions
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for index in 0..NUM_SHARDS {
            let shard = self.continuations.read_shard(index);
            reserve_iteration(shard.len(), meter)?;
            for entry in shard.values() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(entry.key.as_ref(), meter)?;
                let channels = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteContinuations(
                        DeleteContinuations { channels },
                    )));
                } else {
                    let mut continuations = buffer(entry.value.len(), meter)?;
                    for value in &entry.value {
                        native_backing::reserve_copy_and_cleanup(value.as_ref(), meter)?;
                        continuations.push(value.as_ref().clone());
                    }
                    actions.push(HotStoreAction::Insert(InsertAction::InsertContinuations(
                        InsertContinuations {
                            channels,
                            continuations,
                        },
                    )));
                }
            }
        }
        for index in 0..NUM_SHARDS {
            let shard = self.data.read_shard(index);
            reserve_iteration(shard.len(), meter)?;
            for entry in shard.values() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(entry.key.as_ref(), meter)?;
                let channel = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
                        channel,
                    })));
                } else {
                    native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    actions.push(HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                        channel,
                        data: entry.value.clone(),
                    })));
                }
            }
        }
        for index in 0..NUM_SHARDS {
            let shard = self.joins.read_shard(index);
            reserve_iteration(shard.len(), meter)?;
            for entry in shard.values() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(entry.key.as_ref(), meter)?;
                let channel = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
                        channel,
                    })));
                } else {
                    native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    actions.push(HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                        channel,
                        joins: entry.value.clone(),
                    })));
                }
            }
        }
        Ok(actions)
    }
}

#[cfg(test)]
mod tests;
