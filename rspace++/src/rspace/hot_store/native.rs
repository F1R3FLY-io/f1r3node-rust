use std::fmt::{self, Write};
use std::mem::size_of;

use shared::rust::collection_backing::persistent_insert_backing;

use super::*;
use crate::rspace::native_backing;

fn inspect_key<K: CloneBacking>(
    key: &K,
    repetitions: usize,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    native_backing::inspect(key, &|operations: usize, scanned: usize, backing| {
        meter.reserve(
            operations
                .checked_mul(repetitions)
                .ok_or(RSpaceError::HostWorkRejected)?,
            scanned
                .checked_mul(repetitions)
                .ok_or(RSpaceError::HostWorkRejected)?,
            backing,
        )
    })
}

fn lookup<K: Clone + Hash + Eq + CloneBacking, V: Clone>(
    map: &imbl::HashMap<K, V>,
    key: &K,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    let steps = map
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?, 0, 0)?;
    inspect_key(key, steps, meter)?;
    for existing in map.keys() {
        native_backing::inspect(existing, meter)?;
    }
    Ok(())
}

fn reserve_replace<K, V>(
    map: &imbl::HashMap<K, V>,
    key: &K,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError>
where
    K: Clone + Hash + Eq + CloneBacking,
    V: Clone + CloneBacking,
{
    reserve_replace_with(map, key, meter, |value, meter| {
        native_backing::reserve_copy_and_cleanup(value, meter)
    })
}

/// C5 (DR-83): the replace charge of a continuation shard. Its values are
/// store-owned vectors of shared pointers, each counted inline
/// (`native_backing::reserve_shared_copy_and_cleanup`).
fn reserve_replace_shared<K, T>(
    map: &imbl::HashMap<K, Vec<Arc<T>>>,
    key: &K,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError>
where
    K: Clone + Hash + Eq + CloneBacking,
    T: CloneBacking,
{
    reserve_replace_with(map, key, meter, |values, meter| {
        native_backing::reserve_shared_copy_and_cleanup(values, meter)
    })
}

fn reserve_replace_with<K, V>(
    map: &imbl::HashMap<K, V>,
    key: &K,
    meter: &dyn SourceMeter,
    reserve_value: impl Fn(&V, &dyn SourceMeter) -> Result<(), RSpaceError>,
) -> Result<(), RSpaceError>
where
    K: Clone + Hash + Eq + CloneBacking,
    V: Clone + CloneBacking,
{
    lookup(map, key, meter)?;
    let (operations, bytes) =
        persistent_insert_backing::<K, V>(map.len()).ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(operations, bytes, bytes)?;
    let count = map
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let repetitions = count
        .min(33)
        .checked_mul(3)
        .ok_or(RSpaceError::HostWorkRejected)?;
    inspect_key(key, count.checked_mul(3).ok_or(RSpaceError::HostWorkRejected)?, meter)?;
    native_backing::reserve_copy_and_cleanup(key, meter)?;
    for (existing, value) in map.iter() {
        inspect_key(existing, repetitions, meter)?;
        native_backing::reserve_copy_and_cleanup(existing, meter)?;
        reserve_value(value, meter)?;
    }
    Ok(())
}

struct IdentityWriter<'a> {
    value: String,
    meter: &'a dyn SourceMeter,
    failure: Option<RSpaceError>,
}

impl Write for IdentityWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let Some(length) = self.value.len().checked_add(text.len()) else {
            self.failure = Some(RSpaceError::HostWorkRejected);
            return Err(fmt::Error);
        };
        if let Err(error) = self.meter.reserve(1, text.len(), 0) {
            self.failure = Some(error);
            return Err(fmt::Error);
        }
        if length > self.value.capacity() {
            let Some(capacity) = self.value.capacity().checked_mul(2) else {
                self.failure = Some(RSpaceError::HostWorkRejected);
                return Err(fmt::Error);
            };
            let capacity = capacity.max(length).max(8);
            if let Err(error) = self.meter.reserve(1, 0, capacity) {
                self.failure = Some(error);
                return Err(fmt::Error);
            }
            if self
                .value
                .try_reserve_exact(capacity - self.value.len())
                .is_err()
            {
                self.failure = Some(RSpaceError::HostWorkRejected);
                return Err(fmt::Error);
            }
        }
        self.value.push_str(text);
        Ok(())
    }
}

fn continuation_identity_metered<
    P: Clone + Debug + CloneBacking,
    K: Clone + Debug + CloneBacking,
>(
    waiting: &WaitingContinuation<P, K>,
    meter: &dyn SourceMeter,
) -> Result<String, RSpaceError> {
    native_backing::inspect(waiting, meter)?;
    let mut writer = IdentityWriter {
        value: String::new(),
        meter,
        failure: None,
    };
    if write!(
        writer,
        "{:?}|{:?}|{}|{:?}",
        waiting.patterns, waiting.continuation, waiting.persist, waiting.peeks
    )
    .is_err()
    {
        return Err(writer.failure.unwrap_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation identity formatting failed".to_owned(),
            )
        }));
    }
    Ok(writer.value)
}

impl<K, V> ShardedMap<K, V>
where
    K: Clone + Hash + Eq + CloneBacking,
    V: Clone + CloneBacking,
{
    fn native_with<R>(
        &self,
        key: &K,
        meter: &dyn SourceMeter,
        action: impl FnOnce(Option<&V>) -> Result<R, RSpaceError>,
    ) -> Result<R, RSpaceError> {
        native_backing::inspect(key, meter)?;
        let guard = self.shards[shard_of(key)].read().expect("shard read lock");
        lookup(&guard, key, meter)?;
        action(guard.get(key))
    }

    /// An O(1) snapshot of the persistent shard that holds `key`
    /// (C2, DR-82).
    fn native_snapshot(
        &self,
        key: &K,
        meter: &dyn SourceMeter,
    ) -> Result<imbl::HashMap<K, V>, RSpaceError> {
        native_backing::inspect(key, meter)?;
        let guard = self.shards[shard_of(key)].read().expect("shard read lock");
        lookup(&guard, key, meter)?;
        let bytes = size_of::<imbl::HashMap<K, V>>();
        meter.reserve(1, bytes, bytes)?;
        Ok(guard.clone())
    }

    fn native_get(&self, key: &K, meter: &dyn SourceMeter) -> Result<Option<V>, RSpaceError> {
        self.native_with(key, meter, |value| {
            value
                .map(|value| {
                    native_backing::reserve_copy_and_cleanup(value, meter)?;
                    Ok(value.clone())
                })
                .transpose()
        })
    }

    fn native_insert_new(
        &self,
        key: &K,
        value: V,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        self.native_insert_new_with(key, value, false, meter)
            .map(drop)
    }

    /// C2 (DR-82): inserts a new entry and returns an O(1) snapshot of its
    /// shard, taken under the same write lock. Every charge, including the
    /// snapshot, is reserved before the insert.
    fn native_insert_new_snapshot(
        &self,
        key: &K,
        value: V,
        meter: &dyn SourceMeter,
    ) -> Result<imbl::HashMap<K, V>, RSpaceError> {
        self.native_insert_new_with(key, value, true, meter)?
            .ok_or(RSpaceError::HostWorkRejected)
    }

    fn native_insert_new_with(
        &self,
        key: &K,
        value: V,
        snapshot: bool,
        meter: &dyn SourceMeter,
    ) -> Result<Option<imbl::HashMap<K, V>>, RSpaceError> {
        native_backing::inspect(key, meter)?;
        let mut guard = self.shards[shard_of(key)]
            .write()
            .expect("shard write lock");
        lookup(&guard, key, meter)?;
        if guard.contains_key(key) {
            return Err(RSpaceError::HostWorkRejected);
        }
        let (operations, bytes) =
            persistent_insert_backing::<K, V>(guard.len()).ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(operations, bytes, bytes)?;
        let count = guard
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let repetitions = count
            .min(33)
            .checked_mul(3)
            .ok_or(RSpaceError::HostWorkRejected)?;
        inspect_key(key, count.checked_mul(3).ok_or(RSpaceError::HostWorkRejected)?, meter)?;
        native_backing::reserve_copy_and_cleanup(key, meter)?;
        for (existing, value) in guard.iter() {
            inspect_key(existing, repetitions, meter)?;
            native_backing::reserve_copy_and_cleanup(existing, meter)?;
            native_backing::reserve_copy_and_cleanup(value, meter)?;
        }
        if snapshot {
            let bytes = size_of::<imbl::HashMap<K, V>>();
            meter.reserve(1, bytes, bytes)?;
        }
        guard.insert(key.clone(), value);
        Ok(snapshot.then(|| guard.clone()))
    }

    fn native_insert_replace(
        &self,
        key: &K,
        value: V,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        native_backing::inspect(key, meter)?;
        let mut guard = self.shards[shard_of(key)]
            .write()
            .expect("shard write lock");
        reserve_replace(&guard, key, meter)?;
        guard.insert(key.clone(), value);
        Ok(())
    }
}

fn buffer<T>(length: usize, meter: &dyn SourceMeter) -> Result<Vec<T>, RSpaceError> {
    let bytes = length
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(length.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(length)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(result)
}

fn merge<T>(left: Vec<T>, right: Vec<T>, meter: &dyn SourceMeter) -> Result<Vec<T>, RSpaceError> {
    let count = left
        .len()
        .checked_add(right.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut result = buffer(count, meter)?;
    result.extend(left);
    result.extend(right);
    Ok(result)
}

struct PreparedDataRetirement<'a, C, A: Clone> {
    shards: Vec<(usize, std::sync::RwLockWriteGuard<'a, imbl::HashMap<C, Vec<Datum<A>>>>)>,
    updates: Vec<(usize, C, Vec<Datum<A>>)>,
}

impl<C: Clone + Hash + Eq, A: Clone> PreparedDataRetirement<'_, C, A> {
    fn publish(&mut self) {
        for (index, channel, values) in std::mem::take(&mut self.updates) {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested data shard");
            shard.insert(channel, values);
        }
    }
}

struct PreparedJoinRetirement<'a, C> {
    shards: Vec<(usize, std::sync::RwLockWriteGuard<'a, imbl::HashMap<C, Vec<Vec<C>>>>)>,
    updates: Vec<(usize, C, Vec<Vec<C>>)>,
}

impl<C: Clone + Hash + Eq> PreparedJoinRetirement<'_, C> {
    fn publish(mut self) {
        for (index, channel, values) in self.updates {
            let (_, shard) = self
                .shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            shard.insert(channel, values);
        }
    }
}

impl<C, P, A, K> InMemHotStore<C, P, A, K>
where
    C: Clone + Debug + Hash + Eq + Send + Sync,
    P: Clone + Debug + Send + Sync,
    A: Clone + Debug + Send + Sync,
    K: Clone + Debug + Send + Sync,
{
    pub(super) fn native_retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError>
    where
        C: CloneBacking,
        A: CloneBacking,
    {
        self.prepare_native_retire_data(data, retirement, meter)?
            .publish();
        Ok(())
    }

    fn prepare_native_retire_data(
        &self,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<PreparedDataRetirement<'_, C, A>, RSpaceError>
    where
        C: CloneBacking,
        A: CloneBacking,
    {
        if retirement.is_empty() {
            return Ok(PreparedDataRetirement {
                shards: Vec::new(),
                updates: Vec::new(),
            });
        }
        let mut requested = [false; NUM_SHARDS];
        for (position, _) in retirement {
            let channel = &data
                .get(*position)
                .ok_or(RSpaceError::HostWorkRejected)?
                .channel;
            native_backing::inspect(channel, meter)?;
            meter.reserve(64, 0, 0)?;
            requested[shard_of(channel)] = true;
        }
        meter.reserve(NUM_SHARDS, 0, 0)?;
        let shard_count = requested.iter().filter(|needed| **needed).count();
        let guard_bytes = shard_count
            .checked_mul(size_of::<(
                usize,
                std::sync::RwLockWriteGuard<'_, imbl::HashMap<C, Vec<Datum<A>>>>,
            )>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(shard_count, guard_bytes, guard_bytes)?;
        let mut shards = Vec::new();
        shards
            .try_reserve_exact(shard_count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for (index, needed) in requested.into_iter().enumerate() {
            if needed {
                shards.push((index, self.data.shards[index].write().expect("shard write lock")));
            }
        }
        let update_bytes = retirement
            .len()
            .checked_mul(size_of::<(usize, C, Vec<Datum<A>>)>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(retirement.len(), update_bytes, update_bytes)?;
        let mut updates = Vec::new();
        updates
            .try_reserve_exact(retirement.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for (position, datum_index) in retirement {
            let channel = &data[*position].channel;
            let mut staged = None;
            for (index, (_, existing, _)) in updates.iter().enumerate() {
                native_backing::inspect(existing, meter)?;
                native_backing::inspect(channel, meter)?;
                meter.reserve(1, 0, 0)?;
                if existing == channel {
                    staged = Some(index);
                    break;
                }
            }
            let staged = if let Some(index) = staged {
                index
            } else {
                native_backing::inspect(channel, meter)?;
                let shard_index = shard_of(channel);
                meter.reserve(shards.len(), 0, 0)?;
                let (_, shard) = shards
                    .iter()
                    .find(|(index, _)| *index == shard_index)
                    .expect("requested data shard");
                lookup(shard, channel, meter)?;
                let values = shard.get(channel).ok_or_else(|| {
                    RSpaceError::InterpreterError(
                        "native datum cache is absent at retirement".to_owned(),
                    )
                })?;
                reserve_replace(shard, channel, meter)?;
                native_backing::reserve_copy_and_cleanup(values, meter)?;
                native_backing::reserve_copy_and_cleanup(channel, meter)?;
                updates.push((shard_index, channel.clone(), values.clone()));
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
        Ok(PreparedDataRetirement { shards, updates })
    }

    pub(super) fn native_retire_produce_match(
        &self,
        channels: &[C],
        continuation_index: i32,
        continuation_persistent: bool,
        data: &[RSpaceResult<C, A>],
        retirement: &[(usize, i32)],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        A: CloneBacking,
        K: CloneBacking,
    {
        let mut data_update = self.prepare_native_retire_data(data, retirement, meter)?;
        native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
        let key = channels.to_vec();
        let installed = self
            .installed_continuations
            .native_with(&key, meter, |value| Ok(value.is_some()))?;
        native_backing::inspect(&key, meter)?;
        let mut continuation_shard = self.continuations.shards[shard_of(&key)]
            .write()
            .expect("shard write lock");
        lookup(&continuation_shard, &key, meter)?;
        let existing = continuation_shard.get(&key).ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at match retirement".to_owned(),
            )
        })?;
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
            // Changed by C5 (DR-83): the continuation shard holds store-owned
            // shared pointers whose payload releases were prepaid at entry.
            // reserve_replace(&continuation_shard, &key, meter)?;
            // native_backing::reserve_copy_and_cleanup(existing, meter)?;
            reserve_replace_shared(&continuation_shard, &key, meter)?;
            native_backing::reserve_shared_copy_and_cleanup(existing, meter)?;
            let mut values = existing.clone();
            let index = index as usize;
            // native_backing::reserve_cleanup(&values[index], meter)?;
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
            self.prepare_native_retire_joins(data, channels, meter)?
        } else {
            PreparedJoinRetirement {
                shards: Vec::new(),
                updates: Vec::new(),
            }
        };
        data_update.publish();
        if let Some(values) = continuation_update {
            continuation_shard.insert(key, values);
        }
        join_update.publish();
        drop(data_update);
        Ok(())
    }

    fn prepare_native_retire_joins(
        &self,
        data: &[RSpaceResult<C, A>],
        join: &[C],
        meter: &dyn SourceMeter,
    ) -> Result<PreparedJoinRetirement<'_, C>, RSpaceError>
    where
        C: CloneBacking,
    {
        let mut requested = [false; NUM_SHARDS];
        for datum in data {
            native_backing::inspect(&datum.channel, meter)?;
            meter.reserve(64, 0, 0)?;
            requested[shard_of(&datum.channel)] = true;
        }
        meter.reserve(NUM_SHARDS, 0, 0)?;
        let shard_count = requested.iter().filter(|needed| **needed).count();
        let guard_bytes = shard_count
            .checked_mul(size_of::<(
                usize,
                std::sync::RwLockWriteGuard<'_, imbl::HashMap<C, Vec<Vec<C>>>>,
            )>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(shard_count, guard_bytes, guard_bytes)?;
        let mut shards = Vec::new();
        shards
            .try_reserve_exact(shard_count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for (index, needed) in requested.into_iter().enumerate() {
            if needed {
                shards.push((index, self.joins.shards[index].write().expect("shard write lock")));
            }
        }
        let update_bytes = data
            .len()
            .checked_mul(size_of::<(usize, C, Vec<Vec<C>>)>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(data.len(), update_bytes, update_bytes)?;
        let mut updates = Vec::new();
        updates
            .try_reserve_exact(data.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for datum in data {
            let channel = &datum.channel;
            let mut repeated = false;
            for (_, previous, _) in &updates {
                native_backing::inspect(previous, meter)?;
                native_backing::inspect(channel, meter)?;
                meter.reserve(1, 0, 0)?;
                if previous == channel {
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            native_backing::inspect(channel, meter)?;
            let index = shard_of(channel);
            meter.reserve(shards.len(), 0, 0)?;
            let (_, shard) = shards
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            lookup(shard, channel, meter)?;
            let existing = shard.get(channel).ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at match retirement".to_owned(),
                )
            })?;
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
                reserve_replace(shard, channel, meter)?;
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
                native_backing::reserve_copy_and_cleanup(channel, meter)?;
                updates.push((index, channel.clone(), values));
            }
        }
        Ok(PreparedJoinRetirement { shards, updates })
    }

    pub(super) fn native_store_consume(
        &self,
        channels: &[C],
        waiting: WaitingContinuation<P, K>,
        meter: &dyn SourceMeter,
    ) -> Result<(bool, usize), RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        K: CloneBacking,
    {
        native_backing::reserve_cleanup(&waiting, meter)?;
        native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
        let key = channels.to_vec();
        // Changed by D-S4 (DR-90): the new identity is built only when a stored
        // continuation has the same source hash (see the loop below).
        // let identity = continuation_identity_metered(&waiting, meter)?;
        let mut identity: Option<String> = None;
        let installed = self
            .installed_continuations
            .native_with(&key, meter, |value| Ok(usize::from(value.is_some())))?;
        let mut continuation_shard = self.continuations.shards[shard_of(&key)]
            .write()
            .expect("shard write lock");
        lookup(&continuation_shard, &key, meter)?;
        let existing = continuation_shard.get(&key).ok_or_else(|| {
            RSpaceError::InterpreterError(
                "native continuation cache is absent at publication".to_owned(),
            )
        })?;
        let mut duplicate = false;
        // Changed by D-S4 (DR-90): equal identities imply equal source hashes
        // (the hash covers the sorted pattern encodings, the body and the
        // flag), so a stored continuation whose source hash differs is not a
        // duplicate. Identities are built and compared only on equal hashes.
        // for value in existing {
        //     let prior = continuation_identity_metered(value, meter)?;
        //     meter.reserve(
        //         1,
        //         identity
        //             .len()
        //             .checked_add(prior.len())
        //             .ok_or(RSpaceError::HostWorkRejected)?,
        //         0,
        //     )?;
        //     if prior == identity {
        //         duplicate = true;
        //         break;
        //     }
        // }
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
            // Changed by C5 (DR-83): the continuation shard holds store-owned
            // shared pointers whose payload releases were prepaid at entry.
            // reserve_replace(&continuation_shard, &key, meter)?;
            // native_backing::reserve_copy_and_cleanup(existing, meter)?;
            reserve_replace_shared(&continuation_shard, &key, meter)?;
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
            let arc_bytes =
                shared::rust::clone_backing::arc_allocation_bytes::<WaitingContinuation<P, K>>()
                    .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(2, arc_bytes, arc_bytes)?;
            values.insert(0, Arc::new(waiting));
            Some(values)
        };

        native_backing::inspect_slice(channels, meter)?;
        meter.reserve(
            channels
                .len()
                .checked_mul(64)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
            0,
        )?;
        let mut requested = [false; NUM_SHARDS];
        for channel in channels {
            requested[shard_of(channel)] = true;
        }
        meter.reserve(NUM_SHARDS, 0, 0)?;
        let shard_count = requested.iter().filter(|needed| **needed).count();
        let guard_bytes = shard_count
            .checked_mul(size_of::<(
                usize,
                std::sync::RwLockWriteGuard<'_, imbl::HashMap<C, Vec<Vec<C>>>>,
            )>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(shard_count, guard_bytes, guard_bytes)?;
        let mut join_shards = Vec::new();
        join_shards
            .try_reserve_exact(shard_count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for (index, needed) in requested.into_iter().enumerate() {
            if needed {
                join_shards
                    .push((index, self.joins.shards[index].write().expect("shard write lock")));
            }
        }
        let update_bytes = channels
            .len()
            .checked_mul(size_of::<(usize, C, Vec<Vec<C>>)>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(channels.len(), update_bytes, update_bytes)?;
        let mut join_updates = Vec::new();
        join_updates
            .try_reserve_exact(channels.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for channel in channels {
            let mut repeated = false;
            for (_, prior, _) in &join_updates {
                native_backing::inspect(prior, meter)?;
                native_backing::inspect(channel, meter)?;
                meter.reserve(1, 0, 0)?;
                if prior == channel {
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
            native_backing::inspect(channel, meter)?;
            let index = shard_of(channel);
            meter.reserve(join_shards.len(), 0, 0)?;
            let (_, shard) = join_shards
                .iter()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            lookup(shard, channel, meter)?;
            let values = shard.get(channel).ok_or_else(|| {
                RSpaceError::InterpreterError(
                    "native join cache is absent at publication".to_owned(),
                )
            })?;
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
            reserve_replace(shard, channel, meter)?;
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
            native_backing::reserve_copy_and_cleanup(channel, meter)?;
            join_updates.push((index, channel.clone(), updated));
        }
        if let Some(values) = continuation_update {
            continuation_shard.insert(key, values);
        }
        for (index, channel, values) in join_updates {
            let (_, shard) = join_shards
                .iter_mut()
                .find(|(candidate, _)| *candidate == index)
                .expect("requested join shard");
            shard.insert(channel, values);
        }
        Ok((!duplicate, depth))
    }

    pub(super) fn native_put_datum(
        &self,
        channel: &C,
        datum: Datum<A>,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError>
    where
        C: CloneBacking,
        A: CloneBacking,
    {
        native_backing::inspect(channel, meter)?;
        let mut shard = self.data.shards[shard_of(channel)]
            .write()
            .expect("shard write lock");
        lookup(&shard, channel, meter)?;
        let values = shard.get(channel).ok_or_else(|| {
            RSpaceError::InterpreterError("native datum cache is absent at publication".to_owned())
        })?;
        let count = values
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let vector_bytes = count
            .checked_mul(size_of::<Datum<A>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(count, vector_bytes, vector_bytes)?;
        native_backing::reserve_cleanup(&datum, meter)?;
        reserve_replace(&shard, channel, meter)?;
        shard
            .get_mut(channel)
            .expect("native datum cache was checked")
            .insert(0, datum);
        Ok(())
    }

    pub(super) fn native_install_continuation(
        &self,
        channels: &[C],
        value: WaitingContinuation<P, K>,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        K: CloneBacking,
    {
        native_backing::reserve_cleanup(&value, meter)?;
        native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
        let key = channels.to_vec();
        self.installed_continuations
            .native_insert_replace(&key, value, meter)
    }

    pub(super) fn native_install_join(
        &self,
        channel: &C,
        join: &[C],
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError>
    where
        C: CloneBacking,
    {
        let mut values = self
            .installed_joins
            .native_get(channel, meter)?
            .unwrap_or_default();
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
        self.installed_joins
            .native_insert_replace(channel, values, meter)
    }

    pub(super) fn native_changes(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<HotStoreAction<C, P, A, K>>, RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        A: CloneBacking,
        K: CloneBacking,
    {
        let _checkpoint = self.checkpoint_lock.lock().expect("checkpoint lock");
        meter.reserve(NUM_SHARDS * 3, 0, 0)?;
        let mut count = 0usize;
        for shard in self.continuations.shards.iter() {
            count = count
                .checked_add(shard.read().expect("shard read lock").len())
                .ok_or(RSpaceError::HostWorkRejected)?;
        }
        for shard in self.data.shards.iter() {
            count = count
                .checked_add(shard.read().expect("shard read lock").len())
                .ok_or(RSpaceError::HostWorkRejected)?;
        }
        for shard in self.joins.shards.iter() {
            count = count
                .checked_add(shard.read().expect("shard read lock").len())
                .ok_or(RSpaceError::HostWorkRejected)?;
        }
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
        for shard in self.continuations.shards.iter() {
            let guard = shard.read().expect("shard read lock");
            for (channels, values) in guard.iter() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(channels, meter)?;
                let channels = channels.clone();
                if values.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteContinuations(
                        DeleteContinuations { channels },
                    )));
                } else {
                    let mut continuations = buffer(values.len(), meter)?;
                    for value in values {
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
        for shard in self.data.shards.iter() {
            let guard = shard.read().expect("shard read lock");
            for (channel, values) in guard.iter() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(channel, meter)?;
                let channel = channel.clone();
                if values.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
                        channel,
                    })));
                } else {
                    native_backing::reserve_copy_and_cleanup(values, meter)?;
                    actions.push(HotStoreAction::Insert(InsertAction::InsertData(InsertData {
                        channel,
                        data: values.clone(),
                    })));
                }
            }
        }
        for shard in self.joins.shards.iter() {
            let guard = shard.read().expect("shard read lock");
            for (channel, values) in guard.iter() {
                meter.reserve(1, 0, 0)?;
                native_backing::reserve_copy_and_cleanup(channel, meter)?;
                let channel = channel.clone();
                if values.is_empty() {
                    actions.push(HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
                        channel,
                    })));
                } else {
                    native_backing::reserve_copy_and_cleanup(values, meter)?;
                    actions.push(HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
                        channel,
                        joins: values.clone(),
                    })));
                }
            }
        }
        Ok(actions)
    }

    pub(super) fn native_data(
        &self,
        channel: &C,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<Datum<A>>, RSpaceError>
    where
        C: CloneBacking,
        A: CloneBacking,
    {
        if let Some(values) = self.data.native_get(channel, meter)? {
            return Ok(values);
        }
        let values = read()?;
        native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let cached = values.clone();
        self.data.native_insert_new(channel, cached, meter)?;
        Ok(values)
    }

    /// C2 (DR-82): a copy-free view of the cached data of `channel`. A warm
    /// read takes an O(1) snapshot of the shard. A cold read prepays the
    /// release of the decoded data, moves them into the cache, and takes the
    /// snapshot under the insert's write lock, after every reservation.
    pub(super) fn native_data_view(
        &self,
        channel: &C,
        read: &dyn Fn() -> Result<Vec<Datum<A>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<super::NativeDataView<C, A>, RSpaceError>
    where
        C: CloneBacking,
        A: CloneBacking,
    {
        let shard = self.data.native_snapshot(channel, meter)?;
        let shard = if shard.contains_key(channel) {
            native_backing::reserve_copy_and_cleanup(channel, meter)?;
            shard
        } else {
            let values = read()?;
            native_backing::reserve_cleanup(&values, meter)?;
            native_backing::reserve_copy_and_cleanup(channel, meter)?;
            self.data
                .native_insert_new_snapshot(channel, values, meter)?
        };
        Ok(super::NativeDataView {
            shard,
            channel: channel.clone(),
        })
    }

    pub(super) fn native_continuations(
        &self,
        channels: &[C],
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        K: CloneBacking,
    {
        native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
        let key = channels.to_vec();
        let installed = self.installed_continuations.native_get(&key, meter)?;
        let mut prefix = buffer(usize::from(installed.is_some()), meter)?;
        if let Some(installed) = installed {
            prefix.push(installed);
        }
        let warm = self.continuations.native_with(&key, meter, |values| {
            values
                .map(|values| {
                    let mut result = buffer(values.len(), meter)?;
                    for value in values {
                        native_backing::reserve_copy_and_cleanup(value.as_ref(), meter)?;
                        result.push(value.as_ref().clone());
                    }
                    Ok(result)
                })
                .transpose()
        })?;
        if let Some(values) = warm {
            return merge(prefix, values, meter);
        }
        let values = read()?;
        let mut cached = buffer(values.len(), meter)?;
        for value in &values {
            native_backing::reserve_copy_and_cleanup(value, meter)?;
            let bytes =
                shared::rust::clone_backing::arc_allocation_bytes::<WaitingContinuation<P, K>>()
                    .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(2, bytes, bytes)?;
            cached.push(Arc::new(value.clone()));
        }
        let result = merge(prefix, values, meter)?;
        self.continuations.native_insert_new(&key, cached, meter)?;
        Ok(result)
    }

    /// C1 (DR-81): the continuations of `channels` as shared views. A warm
    /// read clones only the `Arc` pointers. A cold read moves each decoded
    /// continuation into its `Arc` and prepays its release once, when it
    /// enters the cache. A caller copies only the continuation it selects.
    pub(super) fn native_continuation_views(
        &self,
        channels: &[C],
        read: &dyn Fn() -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        K: CloneBacking,
    {
        native_backing::reserve_slice_copy_and_cleanup(channels, meter)?;
        let key = channels.to_vec();
        let pointer = size_of::<Arc<WaitingContinuation<P, K>>>();
        let allocation =
            shared::rust::clone_backing::arc_allocation_bytes::<WaitingContinuation<P, K>>()
                .ok_or(RSpaceError::HostWorkRejected)?;
        let installed = self.installed_continuations.native_get(&key, meter)?;
        let mut prefix = buffer(usize::from(installed.is_some()), meter)?;
        if let Some(installed) = installed {
            meter.reserve(2, allocation, allocation)?;
            prefix.push(Arc::new(installed));
        }
        let warm = self.continuations.native_with(&key, meter, |values| {
            values
                .map(|values| {
                    let mut result = buffer(values.len(), meter)?;
                    for value in values {
                        meter.reserve(1, pointer, 0)?;
                        result.push(Arc::clone(value));
                    }
                    Ok(result)
                })
                .transpose()
        })?;
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
        self.continuations.native_insert_new(&key, cached, meter)?;
        Ok(result)
    }

    pub(super) fn native_joins(
        &self,
        channel: &C,
        read: &dyn Fn() -> Result<Vec<Vec<C>>, RSpaceError>,
        meter: &dyn SourceMeter,
    ) -> Result<Vec<Vec<C>>, RSpaceError>
    where
        C: CloneBacking,
    {
        let installed = self
            .installed_joins
            .native_get(channel, meter)?
            .unwrap_or_default();
        if let Some(values) = self.joins.native_get(channel, meter)? {
            return merge(installed, values, meter);
        }
        let values = read()?;
        native_backing::reserve_copy_and_cleanup(&values, meter)?;
        let cached = values.clone();
        let result = merge(installed, values, meter)?;
        self.joins.native_insert_new(channel, cached, meter)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
