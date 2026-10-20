//! D-S1 (D-C2c, DR-96): the digest-keyed store of the native replay session.
//!
//! The store keeps the five maps of the native hot store in digest shards
//! (`native_index.rs`), and keys are compared as digests. DR-131 S6 deleted
//! the legacy native methods of `InMemHotStore`, which these methods ported,
//! together with their host-work charges.

use std::sync::RwLockWriteGuard;

use super::native_index::{DigestShard, DigestShards, DigestSnapshot, NativeEntry, StoreKey};
use super::*;
use crate::rspace::hashing::native_source::GroupKeys;
use crate::rspace::hot_store_action::NativeExportAction;
use crate::rspace::rspace_interface::RSpaceResult;

type Continuations<P, K> = Vec<Arc<WaitingContinuation<P, K>>>;

/// Changed by DR-131 S6: the identity of a waiting continuation, with the
/// format of `InMemHotStore::continuation_identity`.
fn continuation_identity<P: Clone + Debug, K: Clone + Debug>(
    waiting: &WaitingContinuation<P, K>,
) -> String {
    format!(
        "{:?}|{:?}|{}|{:?}",
        waiting.patterns, waiting.continuation, waiting.persist, waiting.peeks
    )
}

/// Changed by DR-131 S6: `left` followed by `right`, in one allocation.
fn merge<T>(left: Vec<T>, right: Vec<T>) -> Vec<T> {
    let mut result = Vec::with_capacity(left.len() + right.len());
    result.extend(left);
    result.extend(right);
    result
}

/// D-C2e (DR-96): two keys with one digest. The store rejects them, the same
/// way on every replay.
fn collision() -> RSpaceError {
    RSpaceError::InterpreterError("native store digest collision".to_owned())
}

/// D-C2e (DR-96): the entry found by a channel's digest must hold the
/// channel. The check is one comparison of the two channels.
fn check_channel<C: PartialEq>(stored: &C, sought: &C) -> Result<(), RSpaceError> {
    // Changed by D-O1 (DR-108): block accounting charges inline bytes
    // once per enclosing block.
    // native_backing::inspect(stored, meter)?;
    // native_backing::inspect(sought, meter)?;
    if stored == sought {
        Ok(())
    } else {
        Err(collision())
    }
}

/// D-C2e (DR-96): the entry found by a group's digest must hold the group.
/// The check is one comparison of the two channel lists.
fn check_group<C: PartialEq>(stored: &[C], sought: &[C]) -> Result<(), RSpaceError> {
    // Changed by D-O1 (DR-108): block accounting charges inline bytes
    // once per enclosing block.
    // native_backing::inspect_slice(stored, meter)?;
    // native_backing::inspect_slice(sought, meter)?;
    if stored == sought {
        Ok(())
    } else {
        Err(collision())
    }
}

/// Changed by DR-131 S6: the keys of one operation do not match its
/// channels. Every caller builds them together, so this is a broken
/// invariant of the native session.
fn mismatched_keys(what: &str) -> RSpaceError {
    RSpaceError::BugFoundError(format!("native replay {what} do not match"))
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
fn lock_requested<KV, V>(
    index: &DigestShards<KV, V>,
    requested: [bool; NUM_SHARDS],
) -> Vec<(usize, RwLockWriteGuard<'_, DigestShard<KV, V>>)> {
    let shard_count = requested.iter().filter(|needed| **needed).count();
    let mut shards = Vec::with_capacity(shard_count);
    for (shard, needed) in requested.into_iter().enumerate() {
        if needed {
            shards.push((shard, index.write_shard(shard)));
        }
    }
    shards
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
/// pointer vector has room for every key of the map.
fn dirty_of<KV, V>(index: &DigestShards<KV, V>) -> Vec<Arc<NativeEntry<KV, V>>> {
    let mut entries = Vec::with_capacity(index.key_count());
    for shard_index in 0..NUM_SHARDS {
        let shard = index.read_shard(shard_index);
        for entry in shard.values() {
            if entry.dirty {
                entries.push(Arc::clone(entry));
            }
        }
    }
    entries
}

impl<C, P: Clone, A: Clone, K: Clone> NativeDirtyEntries<C, P, A, K> {
    /// The export's changes by reference: continuations, data and joins,
    /// each map in digest order. An empty value is a deletion.
    pub(crate) fn actions(
        &self,
    ) -> Vec<NativeExportAction<'_, C, A, Arc<WaitingContinuation<P, K>>>> {
        let mut actions =
            Vec::with_capacity(self.continuations.len() + self.data.len() + self.joins.len());
        for entry in &self.continuations {
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
        for entry in &self.data {
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
        for entry in &self.joins {
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
        actions
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
    pub(crate) fn new() -> Self {
        Self {
            data: DigestShards::new(),
            continuations: DigestShards::new(),
            installed_continuations: DigestShards::new(),
            joins: DigestShards::new(),
            installed_joins: DigestShards::new(),
        }
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
}

impl<C, P, A, K> NativeHotStore<C, P, A, K>
where
    C: Clone + Debug + PartialEq + Send + Sync,
    P: Clone + Debug + Send + Sync,
    A: Clone + Debug + Send + Sync,
    K: Clone + Debug + Send + Sync,
{
    /// The data of `channel`. A cold read stores a copy of the decoded data.
    pub(crate) fn data(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        let warm = {
            let shard = self.data.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel)?;
                    // Changed by D-O1 (DR-108): block accounting charges inline bytes
                    // once per enclosing block.
                    // native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    Some(entry.value.clone())
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return Ok(values);
        }
        let values = read()?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let mut shard = self.data.write(&key);
        // Another reader can fill this key between our read-lock miss and
        // the write lock. Use its entry rather than treating that race as a
        // host-work refusal (or overwriting a concurrent store update).
        if let Some(entry) = DigestShards::get(&shard, &key) {
            check_channel(entry.key.as_ref(), channel)?;
            return Ok(entry.value.clone());
        }
        let cached = values.clone();
        self.data
            .insert_new(&mut shard, key, Arc::new(channel.clone()), cached, false)?;
        Ok(values)
    }

    /// A copy-free view of the data of `channel`. A warm read clones the
    /// entry pointer; a cold read moves the decoded data into the store.
    pub(crate) fn data_view(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
    ) -> Result<NativeDataView<C, A>, RSpaceError> {
        let warm = {
            let shard = self.data.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel)?;
                    Some(Arc::clone(entry))
                }
                None => None,
            }
        };
        if let Some(entry) = warm {
            return Ok(NativeDataView::from_entry(entry));
        }
        let values = read()?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_cleanup(&values, meter)?;
        let mut shard = self.data.write(&key);
        if let Some(entry) = DigestShards::get(&shard, &key) {
            check_channel(entry.key.as_ref(), channel)?;
            return Ok(NativeDataView::from_entry(Arc::clone(entry)));
        }
        let entry =
            self.data
                .insert_new(&mut shard, key, Arc::new(channel.clone()), values, false)?;
        Ok(NativeDataView::from_entry(entry))
    }

    /// The joins of `channel`, installed joins first.
    pub(crate) fn joins(
        &self,
        channel: &C,
        key: StoreKey,
        read: &dyn Fn() -> Result<Vec<Vec<C>>, RSpaceError>,
    ) -> Result<Vec<Vec<C>>, RSpaceError> {
        let installed = {
            let shard = self.installed_joins.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel)?;
                    // Changed by D-O1 (DR-108): block accounting charges inline bytes
                    // once per enclosing block.
                    // native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    entry.value.clone()
                }
                None => Vec::new(),
            }
        };
        let warm = {
            let shard = self.joins.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought channel.
                    check_channel(entry.key.as_ref(), channel)?;
                    // Changed by D-O1 (DR-108): block accounting charges inline bytes
                    // once per enclosing block.
                    // native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                    Some(entry.value.clone())
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return Ok(merge(installed, values));
        }
        let values = read()?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let mut shard = self.joins.write(&key);
        if let Some(entry) = DigestShards::get(&shard, &key) {
            check_channel(entry.key.as_ref(), channel)?;
            return Ok(merge(installed, entry.value.clone()));
        }
        let cached = values.clone();
        let result = merge(installed, values);
        self.joins
            .insert_new(&mut shard, key, Arc::new(channel.clone()), cached, false)?;
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
    //
    // native_backing::reserve_blocks_copy_and_cleanup(entry.value.as_ref(),
    // meter)?;             Ok(Some(entry.value.as_ref().clone()))
    //         }
    //         None => Ok(None),
    //     }
    // }
    /// The installed continuation of a group, as a deep copy.
    fn installed_copy(
        &self,
        channels: &[C],
        key: &StoreKey,
    ) -> Result<Option<WaitingContinuation<P, K>>, RSpaceError> {
        let shard = self.installed_continuations.read(key);
        match DigestShards::get(&shard, key) {
            Some(entry) => {
                // D-C2e (DR-96): the entry must hold the sought group.
                check_group(entry.key.as_ref(), channels)?;
                // Changed by D-O1 (DR-108): block accounting charges inline
                // bytes once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(entry.value.
                // as_ref(), meter)?;
                Ok(Some(entry.value.as_ref().clone()))
            }
            None => Ok(None),
        }
    }

    /// The continuations of `channels`, the installed continuation first.
    pub(crate) fn continuations(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        let key = keys.group;
        // Changed by D-C2e (D-S1, DR-96): the lookup checks the group.
        // let installed = self.installed_copy(&key, meter)?;
        let installed = self.installed_copy(channels, &key)?;
        let mut prefix = Vec::with_capacity(usize::from(installed.is_some()));
        if let Some(installed) = installed {
            prefix.push(installed);
        }
        let warm = {
            let shard = self.continuations.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels)?;
                    let mut result = Vec::with_capacity(entry.value.len());
                    for value in &entry.value {
                        // Changed by D-O1 (DR-108): block accounting charges
                        // inline bytes
                        // once per enclosing block.
                        // native_backing::reserve_copy_and_cleanup(value.
                        // as_ref(), meter)?;
                        result.push(value.as_ref().clone());
                    }
                    Some(result)
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return Ok(merge(prefix, values));
        }
        let values = read()?;
        let mut shard = self.continuations.write(&key);
        if let Some(entry) = DigestShards::get(&shard, &key) {
            check_group(entry.key.as_ref(), channels)?;
            let values = entry
                .value
                .iter()
                .map(|value| value.as_ref().clone())
                .collect();
            return Ok(merge(prefix, values));
        }
        let mut cached = Vec::with_capacity(values.len());
        for value in &values {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(value, meter)?;
            cached.push(Arc::new(value.clone()));
        }
        let result = merge(prefix, values);
        self.continuations.insert_new(
            &mut shard,
            key,
            Arc::new(channels.to_vec()),
            cached,
            false,
        )?;
        Ok(result)
    }

    /// The continuations of `channels` as shared views, the installed
    /// continuation first. The installed continuation is stored as an
    /// `Arc`, so its view is a pointer copy.
    pub(crate) fn continuation_views(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
    ) -> Result<Continuations<P, K>, RSpaceError> {
        let key = keys.group;
        let installed = {
            let shard = self.installed_continuations.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels)?;
                    Some(Arc::clone(&entry.value))
                }
                None => None,
            }
        };
        let mut prefix = Vec::with_capacity(usize::from(installed.is_some()));
        if let Some(installed) = installed {
            prefix.push(installed);
        }
        let warm = {
            let shard = self.continuations.read(&key);
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    // D-C2e (DR-96): the entry must hold the sought group.
                    check_group(entry.key.as_ref(), channels)?;
                    let mut result = Vec::with_capacity(entry.value.len());
                    for value in &entry.value {
                        result.push(Arc::clone(value));
                    }
                    Some(result)
                }
                None => None,
            }
        };
        if let Some(values) = warm {
            return Ok(merge(prefix, values));
        }
        let values = read()?;
        let mut shard = self.continuations.write(&key);
        if let Some(entry) = DigestShards::get(&shard, &key) {
            check_group(entry.key.as_ref(), channels)?;
            return Ok(merge(prefix, entry.value.clone()));
        }
        let mut shared = Vec::with_capacity(values.len());
        let mut cached = Vec::with_capacity(values.len());
        for value in values {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_cleanup(&value, meter)?;
            let value = Arc::new(value);
            cached.push(Arc::clone(&value));
            shared.push(value);
        }
        let result = merge(prefix, shared);
        self.continuations.insert_new(
            &mut shard,
            key,
            Arc::new(channels.to_vec()),
            cached,
            false,
        )?;
        Ok(result)
    }

    /// Stores a waiting continuation and its joins. Returns whether the
    /// continuation was new, and the number of continuations of its group.
    pub(crate) fn store_consume(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        waiting: WaitingContinuation<P, K>,
    ) -> Result<(bool, usize), RSpaceError> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_cleanup(&waiting, meter)?;
        if keys.channels.len() != channels.len() {
            return Err(mismatched_keys("channel keys and channels"));
        }
        let key = keys.group;
        let mut identity: Option<String> = None;
        let installed = {
            let shard = self.installed_continuations.read(&key);
            // Changed by D-C2e (D-S1, DR-96): a found entry must hold the
            // group.
            // usize::from(DigestShards::get(&shard, &key, meter)?.is_some())
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    check_group(entry.key.as_ref(), channels)?;
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
        //             "native continuation cache is absent at
        // publication".to_owned(),         )
        //     })?
        //     .value;
        let entry = DigestShards::get(&continuation_shard, &key).ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at publication".to_owned(),
            )
        })?;
        check_group(entry.key.as_ref(), channels)?;
        let existing = &entry.value;
        let mut duplicate = false;
        // D-S4 (DR-90): identities are built and compared only for stored
        // continuations with the same source hash.
        for value in existing {
            if value.source.hash != waiting.source.hash {
                continue;
            }
            if identity.is_none() {
                identity = Some(continuation_identity(&waiting));
            }
            let identity = identity.as_ref().expect("identity built above");
            if continuation_identity(value) == *identity {
                duplicate = true;
                break;
            }
        }
        let depth = existing.len() + usize::from(!duplicate) + installed;
        let continuation_update = if duplicate {
            None
        } else {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_shared_copy_and_cleanup(existing, meter)?;
            let mut values = existing.clone();
            values.reserve_exact(1);
            values.insert(0, Arc::new(waiting));
            Some(values)
        };

        let mut requested = [false; NUM_SHARDS];
        for channel_key in &keys.channels {
            requested[channel_key.shard()] = true;
        }
        let mut join_shards = lock_requested(&self.joins, requested);
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
            Vec::with_capacity(keys.channels.len());
        for (position, (channel, channel_key)) in channels.iter().zip(&keys.channels).enumerate() {
            let mut repeated = false;
            for (prior, prior_position, _) in &join_updates {
                if prior == channel_key {
                    check_channel(&channels[*prior_position], channel)?;
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            let index = channel_key.shard();
            let (_, shard) = join_shards
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            // Changed by D-C2e (D-S1, DR-96): the entry must hold the channel.
            // let values = &DigestShards::get(shard, channel_key, meter)?
            //     .ok_or_else(|| {
            //         RSpaceError::InterpreterError(
            //             "native join cache is absent at
            // publication".to_owned(),         )
            //     })?
            //     .value;
            let entry = DigestShards::get(shard, channel_key).ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at publication".to_owned(),
                )
            })?;
            check_channel(entry.key.as_ref(), channel)?;
            let values = &entry.value;
            let mut present = false;
            for join in values {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect_slice(join, meter)?;
                // native_backing::inspect_slice(channels, meter)?;
                if join.as_slice() == channels {
                    present = true;
                    break;
                }
            }
            if present {
                continue;
            }
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(values, meter)?;
            let mut updated = values.clone();
            updated.reserve_exact(1);
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
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
    /// Adds a datum to the cached data of a channel.
    pub(crate) fn put_datum(
        &self,
        channel: &C,
        key: StoreKey,
        datum: Datum<A>,
    ) -> Result<(), RSpaceError> {
        let mut shard = self.data.write(&key);
        let entry = DigestShards::get(&shard, &key).ok_or_else(|| {
            RSpaceError::InterpreterError("native datum cache is absent at publication".to_owned())
        })?;
        // D-C2e (DR-96): the entry must hold the channel.
        check_channel(entry.key.as_ref(), channel)?;
        let values = &entry.value;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_cleanup(&datum, meter)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(values, meter)?;
        let mut updated = Vec::with_capacity(values.len() + 1);
        updated.push(datum);
        updated.extend(values.iter().cloned());
        DigestShards::replace(&mut shard, &key, updated);
        Ok(())
    }

    /// Removes matched data. `keys[position]` is the key of
    /// `data[position].channel`.
    pub(crate) fn retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        keys: &[StoreKey],
        retirement: &[(usize, i32)],
    ) -> Result<(), RSpaceError> {
        self.prepare_retire_data(data, keys, retirement)?.publish();
        Ok(())
    }

    fn prepare_retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        keys: &[StoreKey],
        retirement: &[(usize, i32)],
    ) -> Result<DataRetirement<'_, C, A>, RSpaceError> {
        if retirement.is_empty() {
            return Ok(DataRetirement::empty());
        }
        if keys.len() != data.len() {
            return Err(mismatched_keys("data keys and data"));
        }
        let mut requested = [false; NUM_SHARDS];
        for (position, _) in retirement {
            let key = keys
                .get(*position)
                .ok_or_else(|| mismatched_keys("retirement positions and data"))?;
            requested[key.shard()] = true;
        }
        let shards = lock_requested(&self.data, requested);
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
        //                     "native datum cache is absent at
        // retirement".to_owned(),                 )
        //             })?
        //             .value;
        //         DigestShards::<C, Vec<Datum<A>>>::reserve_replace(meter)?;
        //         native_backing::reserve_blocks_copy_and_cleanup(values, meter)?;
        //         updates.push((key, values.clone()));
        //         updates.len() - 1
        //     };
        //     let values = &mut updates[staged].1;
        let mut updates: Vec<(StoreKey, usize, Vec<Datum<A>>)> =
            Vec::with_capacity(retirement.len());
        for (position, datum_index) in retirement {
            let key = keys[*position];
            let channel = &data[*position].channel;
            let mut staged = None;
            for (index, (existing, staged_position, _)) in updates.iter().enumerate() {
                if *existing == key {
                    check_channel(&data[*staged_position].channel, channel)?;
                    staged = Some(index);
                    break;
                }
            }
            let staged = if let Some(index) = staged {
                index
            } else {
                let (_, shard) = shards
                    .iter()
                    .find(|(index, _)| *index == key.shard())
                    .expect("requested data shard");
                let entry = DigestShards::get(shard, &key).ok_or_else(|| {
                    RSpaceError::InterpreterError(
                        "native datum cache is absent at retirement".to_owned(),
                    )
                })?;
                check_channel(entry.key.as_ref(), channel)?;
                let values = &entry.value;
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(values, meter)?;
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
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_cleanup(&values[index], meter)?;
            values.remove(index);
        }
        Ok(DataRetirement { shards, updates })
    }

    /// Removes the data and the continuation of a produce match. `keys` are
    /// the keys of the matched continuation's group; data position `i` is
    /// channel `i`.
    pub(crate) fn retire_produce_match(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        continuation_index: i32,
        continuation_persistent: bool,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
    ) -> Result<(), RSpaceError> {
        let mut data_update = self.prepare_retire_data(data, &keys.channels, retirement)?;
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
            match DigestShards::get(&shard, &key) {
                Some(entry) => {
                    check_group(entry.key.as_ref(), channels)?;
                    true
                }
                None => false,
            }
        };
        let mut continuation_shard = self.continuations.write(&key);
        let entry = DigestShards::get(&continuation_shard, &key).ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at match retirement".to_owned(),
            )
        })?;
        check_group(entry.key.as_ref(), channels)?;
        let existing = &entry.value;
        let continuation_update = if continuation_persistent {
            None
        } else {
            let invalid_index = || {
                RSpaceError::InterpreterError(
                    "native continuation retirement index is invalid".to_owned(),
                )
            };
            let index = continuation_index
                .checked_sub(i32::from(installed))
                .ok_or_else(invalid_index)?;
            if index < 0 || index as usize >= existing.len() {
                return Err(invalid_index());
            }
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_shared_copy_and_cleanup(existing, meter)?;
            let mut values = existing.clone();
            let index = index as usize;
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_shared_cleanup(&values[index], meter)?;
            values.remove(index);
            Some(values)
        };
        let regular_count = continuation_update.as_ref().unwrap_or(existing).len();
        let join_update = if !installed && regular_count == 0 {
            self.prepare_retire_joins(data, channels, &keys.channels)?
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
    ) -> Result<JoinRetirement<'_, C>, RSpaceError> {
        // Changed by D-C2e (D-S1, DR-96): key `i` is the key of `join[i]`, the
        // channel that the collision check compares.
        // if keys.len() != data.len() {
        //     return Err(RSpaceError::HostWorkRejected);
        // }
        if keys.len() != data.len() || keys.len() != join.len() {
            return Err(mismatched_keys("join keys, data and join"));
        }
        let mut requested = [false; NUM_SHARDS];
        for key in keys {
            requested[key.shard()] = true;
        }
        let shards = lock_requested(&self.joins, requested);
        // Changed by D-C2e (D-S1, DR-96): an update records the position of its
        // channel; a repeated key compares the two channels, and a found entry
        // must hold the channel.
        // let mut updates: Vec<(StoreKey, Vec<Vec<C>>)> =
        // update_buffer(keys.len(), meter)?; for key in keys {
        //     let mut repeated = false;
        //     for (previous, _) in &updates {
        //         key_comparison(meter)?;
        //         if previous == key {
        //             repeated = true;
        //             break;
        //         }
        //     }
        let mut updates: Vec<(StoreKey, usize, Vec<Vec<C>>)> = Vec::with_capacity(keys.len());
        for (channel_position, (channel, key)) in join.iter().zip(keys).enumerate() {
            let mut repeated = false;
            for (previous, previous_position, _) in &updates {
                if previous == key {
                    check_channel(&join[*previous_position], channel)?;
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            let (_, shard) = shards
                .iter()
                .find(|(candidate, _)| *candidate == key.shard())
                .expect("requested join shard");
            // let existing = &DigestShards::get(shard, key, meter)?
            //     .ok_or_else(|| {
            //         RSpaceError::InterpreterError(
            //             "native join cache is absent at match
            // retirement".to_owned(),         )
            //     })?
            //     .value;
            let entry = DigestShards::get(shard, key).ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at match retirement".to_owned(),
                )
            })?;
            check_channel(entry.key.as_ref(), channel)?;
            let existing = &entry.value;
            let mut position = None;
            for (candidate, value) in existing.iter().enumerate() {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect_slice(value, meter)?;
                // native_backing::inspect_slice(join, meter)?;
                if value.as_slice() == join {
                    position = Some(candidate);
                    break;
                }
            }
            if let Some(position) = position {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(existing, meter)?;
                let mut values = existing.clone();
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_cleanup(&values[position], meter)?;
                values.remove(position);
                // Changed by D-C2e (D-S1, DR-96): the update records its
                // position.
                // updates.push((*key, values));
                updates.push((*key, channel_position, values));
            }
        }
        Ok(JoinRetirement { shards, updates })
    }

    /// Installs a continuation. It is stored as an `Arc`, so a view of it
    /// is a pointer copy.
    pub(crate) fn install_continuation(
        &self,
        channels: &[C],
        keys: &GroupKeys,
        value: WaitingContinuation<P, K>,
    ) -> Result<(), RSpaceError> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_cleanup(&value, meter)?;
        let key = keys.group;
        let mut shard = self.installed_continuations.write(&key);
        // Changed by D-C2e (D-S1, DR-96): a found entry must hold the group.
        // if DigestShards::get(&shard, &key, meter)?.is_some() {
        let found = match DigestShards::get(&shard, &key) {
            Some(entry) => {
                check_group(entry.key.as_ref(), channels)?;
                true
            }
            None => false,
        };
        if found {
            DigestShards::replace(&mut shard, &key, Arc::new(value));
        } else {
            self.installed_continuations.insert_new(
                &mut shard,
                key,
                Arc::new(channels.to_vec()),
                Arc::new(value),
                true,
            )?;
        }
        Ok(())
    }

    /// Installs a join of `channel`.
    pub(crate) fn install_join(
        &self,
        channel: &C,
        key: StoreKey,
        join: &[C],
    ) -> Result<(), RSpaceError> {
        let mut shard = self.installed_joins.write(&key);
        let existing = DigestShards::get(&shard, &key);
        let present = existing.is_some();
        let mut values = match existing {
            Some(entry) => {
                // D-C2e (DR-96): the entry must hold the channel.
                check_channel(entry.key.as_ref(), channel)?;
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(&entry.value, meter)?;
                entry.value.clone()
            }
            None => Vec::new(),
        };
        for existing in &values {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::inspect_slice(existing, meter)?;
            // native_backing::inspect_slice(join, meter)?;
            if existing.as_slice() == join {
                return Ok(());
            }
        }
        values.reserve_exact(1);
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_slice_copy_and_cleanup(join, meter)?;
        values.insert(0, join.to_vec());
        if present {
            DigestShards::replace(&mut shard, &key, values);
        } else {
            self.installed_joins.insert_new(
                &mut shard,
                key,
                Arc::new(channel.clone()),
                values,
                true,
            )?;
        }
        Ok(())
    }

    /// D-C3 (D-S3, DR-97): the dirty entries of the exported maps, for the
    /// export by reference. The session's exclusive gate serializes the
    /// export with every write.
    pub(crate) fn dirty_entries(&self) -> NativeDirtyEntries<C, P, A, K> {
        NativeDirtyEntries {
            continuations: dirty_of(&self.continuations),
            data: dirty_of(&self.data),
            joins: dirty_of(&self.joins),
        }
    }

    // Changed by D-C3 (D-S3, DR-97): the export borrows cached values and emits
    // dirty keys only (`dirty_entries`). The full export of deep copies stays
    // as the test oracle of the root-equality tests.
    /// The store's changes for the history: every continuation, data and
    /// join entry, each map in digest order. The session's exclusive gate
    /// serializes the export with every write.
    #[cfg(test)]
    pub(crate) fn changes(&self) -> Vec<HotStoreAction<C, P, A, K>> {
        let count = self.continuations.key_count() + self.data.key_count() + self.joins.key_count();
        let mut actions = Vec::with_capacity(count);
        for index in 0..NUM_SHARDS {
            let shard = self.continuations.read_shard(index);
            for entry in shard.values() {
                let channels = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteContinuations(
                        DeleteContinuations { channels },
                    )));
                } else {
                    let mut continuations = Vec::with_capacity(entry.value.len());
                    for value in &entry.value {
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
            for entry in shard.values() {
                let channel = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
                        channel,
                    })));
                } else {
                    actions.push(HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                        channel,
                        data: entry.value.clone(),
                    })));
                }
            }
        }
        for index in 0..NUM_SHARDS {
            let shard = self.joins.read_shard(index);
            for entry in shard.values() {
                let channel = entry.key.as_ref().clone();
                if entry.value.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
                        channel,
                    })));
                } else {
                    actions.push(HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                        channel,
                        joins: entry.value.clone(),
                    })));
                }
            }
        }
        actions
    }
}

#[cfg(test)]
mod tests;
