// See rspace/src/main/scala/coop/rchain/rspace/merger/StateChange.scala

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dashmap::DashMap;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::channel_change::ChannelChange;
use super::event_log_index::EventLogIndex;
use super::merging_logic::{consumes_affected, produces_affected};
use crate::rspace::errors::HistoryError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider;
use crate::rspace::history::history_reader::HistoryReader;
use crate::rspace::history::instances::rspace_history_reader_impl::RSpaceHistoryReaderImpl;

#[derive(Debug, Clone)]
pub struct FrozenMap<K, V>(Arc<BTreeMap<K, V>>);

impl<K, V> Default for FrozenMap<K, V> {
    fn default() -> Self { Self(Arc::new(BTreeMap::new())) }
}

impl<K: Ord, V> FrozenMap<K, V> {
    pub fn from_map<M>(map: M) -> Self
    where M: IntoIterator<Item = (K, V)> {
        Self(Arc::new(map.into_iter().collect()))
    }

    fn from_dash_map(map: DashMap<K, V>) -> Self
    where K: Eq + Hash {
        Self::from_map(map.into_iter())
    }

    pub fn len(&self) -> usize { self.0.len() }

    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    pub fn get(&self, key: &K) -> Option<&V> { self.0.get(key) }

    pub fn iter(&self) -> FrozenMapIter<'_, K, V> { FrozenMapIter(self.0.iter()) }
}

impl<K: Clone + Ord, V: Clone> FrozenMap<K, V> {
    fn into_owned(self) -> BTreeMap<K, V> {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone())
    }
}

impl<K: Ord, V: PartialEq> PartialEq for FrozenMap<K, V> {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl<K: Ord, V: Eq> Eq for FrozenMap<K, V> {}

impl<K: Ord, V: Ord> Ord for FrozenMap<K, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.iter().cmp(other.0.iter()) }
}

impl<K: Ord, V: Ord> PartialOrd for FrozenMap<K, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl<K: Ord + Hash, V: Hash> Hash for FrozenMap<K, V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for (key, value) in self.0.iter() {
            key.hash(state);
            value.hash(state);
        }
    }
}

pub struct FrozenMapEntry<'a, K, V> {
    key: &'a K,
    value: &'a V,
}

impl<'a, K, V> FrozenMapEntry<'a, K, V> {
    pub fn key(&self) -> &'a K { self.key }

    pub fn value(&self) -> &'a V { self.value }
}

pub struct FrozenMapIter<'a, K, V>(std::collections::btree_map::Iter<'a, K, V>);

impl<'a, K, V> Iterator for FrozenMapIter<'a, K, V> {
    type Item = FrozenMapEntry<'a, K, V>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|(key, value)| FrozenMapEntry { key, value })
    }
}

/**
 * Datum changes are referenced by channel, continuation changes are
 * references by consume. In addition, map from consume channels to binary
 * representation of a join in trie have to be maintained. This is because
 * only hashes of channels are available in log event, and computing a join
 * binary to be inserted or removed on merge requires channels before
 * hashing.
 */
#[derive(Debug, Clone)]
pub struct StateChange {
    pub datums_changes: FrozenMap<Blake2b256Hash, ChannelChange<Vec<u8>>>,
    pub cont_changes: FrozenMap<Vec<Blake2b256Hash>, ChannelChange<Vec<u8>>>,
    pub consume_channels_to_join_serialized_map: FrozenMap<Vec<Blake2b256Hash>, Vec<u8>>,
}

impl PartialEq for StateChange {
    fn eq(&self, other: &Self) -> bool {
        self.datums_changes == other.datums_changes &&
            self.cont_changes == other.cont_changes &&
            self.consume_channels_to_join_serialized_map ==
                other.consume_channels_to_join_serialized_map
    }
}

impl PartialOrd for StateChange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Hash for StateChange {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.datums_changes.hash(state);
        self.cont_changes.hash(state);
        self.consume_channels_to_join_serialized_map.hash(state);
    }
}

impl Eq for StateChange {}

impl Ord for StateChange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.datums_changes
            .cmp(&other.datums_changes)
            .then_with(|| self.cont_changes.cmp(&other.cont_changes))
            .then_with(|| {
                self.consume_channels_to_join_serialized_map
                    .cmp(&other.consume_channels_to_join_serialized_map)
            })
    }
}

impl StateChange {
    pub fn new<C, P, A, K>(
        pre_state_reader: RSpaceHistoryReaderImpl<C, P, A, K>,
        post_state_reader: RSpaceHistoryReaderImpl<C, P, A, K>,
        event_log_index: &EventLogIndex,
    ) -> Result<Self, HistoryError>
    where
        C: Clone + for<'a> Deserialize<'a> + Serialize + 'static + Sync + Send,
        P: Clone + for<'a> Deserialize<'a> + 'static + Sync + Send,
        A: Clone + for<'a> Deserialize<'a> + 'static + Sync + Send,
        K: Clone + for<'a> Deserialize<'a> + 'static + Sync + Send,
    {
        let datums_diff = DashMap::new();
        let cont_diff = DashMap::new();
        // Since event log only contains hashes of channels, so to know which join
        // stored corresponds to channels, this index have to be maintained
        let joins_map = DashMap::new();

        let produces_affected = produces_affected(event_log_index);

        // Deduplicate by channel hash before processing
        // Without this, if multiple Produces have the same channel_hash, we'd compute
        // changes multiple times
        let unique_produce_channels: HashSet<Blake2b256Hash> = produces_affected
            .0
            .iter()
            .map(|produce| produce.channel_hash.clone())
            .collect();

        let channels_of_consumes_affected = consumes_affected(event_log_index)
            .0
            .into_iter()
            .map(|consume| consume.channel_hashes)
            .collect::<Vec<_>>();

        // Deduplicate consume channels as well
        let unique_consume_channels: HashSet<Vec<Blake2b256Hash>> =
            channels_of_consumes_affected.into_iter().collect();
        let channels_of_consumes_affected: Vec<Vec<Blake2b256Hash>> =
            unique_consume_channels.into_iter().collect();

        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "StateChange::new.ENTER",
            unique_produce_channels = unique_produce_channels.len(),
            unique_consume_channels = channels_of_consumes_affected.len(),
            "computing state change diff from event log index",
        );

        // Process produces in parallel - each unique channel is processed exactly once
        unique_produce_channels
            .par_iter()
            .try_for_each(|history_pointer| {
                let change = Self::compute_value_change(
                    history_pointer,
                    |h| pre_state_reader.get_data_proj_binary(h),
                    |h| post_state_reader.get_data_proj_binary(h),
                )?;

                // Since each channel is processed exactly once, we can just insert directly
                datums_diff.insert(history_pointer.clone(), change);

                Ok::<(), HistoryError>(())
            })?;

        // Process consumes in parallel - each unique consume channels set is processed
        // exactly once
        channels_of_consumes_affected
            .par_iter()
            .try_for_each(|consume_channels| {
                let consume_channels = consume_channels.clone();
                let history_pointer = stable_hash_provider::hash_from_hashes(&consume_channels);

                let change = Self::compute_value_change(
                    &history_pointer,
                    |h| pre_state_reader.get_continuations_proj_binary(h),
                    |h| post_state_reader.get_continuations_proj_binary(h),
                )?;

                // Since each consume channels set is processed exactly once, we can just insert
                // directly
                cont_diff.insert(consume_channels, change);

                Ok::<(), HistoryError>(())
            })?;

        // Process joins in parallel
        channels_of_consumes_affected
            .par_iter()
            .try_for_each(|consume_channels| {
                let mut consume_channels = consume_channels.clone();
                let history_pointer = consume_channels[0].clone();
                let pre = pre_state_reader.get_joins(&history_pointer)?;
                let post = post_state_reader.get_joins(&history_pointer)?;

                // find join which match channels
                let join_opt = pre.into_iter().chain(post).find(|join| {
                    let mut join_channels = join
                        .iter()
                        .map(|item| stable_hash_provider::hash(item))
                        .collect::<Vec<_>>();
                    // sorting is required because channels of a consume in event log and
                    // channels of a join in history might not be
                    // ordered the same way
                    consume_channels.sort();
                    join_channels.sort();
                    *consume_channels == join_channels
                });

                // A consume can be "affected" by the event-log set algebra yet have ZERO net
                // tuple-space effect (created-and-resolved within the block, or a peek /
                // immediate match leaving no residue), in which case neither pre nor post
                // state holds a join for it. Such a no-op consume carries no join change:
                // the `cont_diff.retain(...)` below drops it from the continuation diff, and
                // `state_change_merger` consults this join map ONLY for consumes that have a
                // (non-no-op) continuation change. So skip it rather than `.expect()`-
                // panicking — the panic was a craftable validator-crash on attacker-
                // influenceable input, and it fired during eager per-branch indexing.
                // A genuine surviving/destroyed consume always has its join written in
                // lockstep with its continuation, so it cannot reach pre==post==empty; were
                // one ever to, the downstream merger degrades to a deterministic
                // `HistoryError::MergeError` rather than a silent wrong state.
                match join_opt {
                    Some(join) => {
                        let raw_join = bincode::serialize(&join).expect("Unable to serialize join");
                        joins_map.insert(consume_channels, raw_join);
                    }
                    None => {
                        tracing::warn!(
                            target: "rspace.merger",
                            "state_change: affected consume has no join record in pre or post \
                             state (net-zero-effect / no-op consume); skipping its join entry. \
                             This is expected for created-and-resolved or peek consumes and is \
                             NOT a tuple-space inconsistency. channels={:?}",
                            consume_channels
                        );
                    }
                }
                Ok::<(), HistoryError>(())
            })?;

        // Drop no-op channel changes. In practice these can appear in complex
        // COMM/peek scenarios where a channel is touched in event log but the
        // net pre/post tuple-space value is unchanged.
        datums_diff.retain(|_, change| !(change.added.is_empty() && change.removed.is_empty()));
        cont_diff.retain(|_, change| !(change.added.is_empty() && change.removed.is_empty()));

        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "StateChange::new.EXIT",
            datums_changes = datums_diff.len(),
            cont_changes = cont_diff.len(),
            joins = joins_map.len(),
            "state change diff computed (no-op channel changes dropped)",
        );

        Ok(Self {
            datums_changes: FrozenMap::from_dash_map(datums_diff),
            cont_changes: FrozenMap::from_dash_map(cont_diff),
            consume_channels_to_join_serialized_map: FrozenMap::from_dash_map(joins_map),
        })
    }

    /// Compute multiset difference using O(n+m) HashMap-based algorithm.
    /// Removes each element of `to_remove` from `from` exactly once.
    ///
    /// This is an improvement over Scala's `Seq.diff` which has O(n*m)
    /// complexity.
    pub fn multiset_diff(from: &[Vec<u8>], to_remove: &[Vec<u8>]) -> Vec<Vec<u8>> {
        use std::collections::HashMap;

        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "multiset_diff.ENTER",
            from_items = from.len(),
            to_remove_items = to_remove.len(),
            "multiset difference (remove each to_remove item once from from)",
        );

        // Build occurrence count map - O(m)
        let mut remove_counts: HashMap<&Vec<u8>, usize> = HashMap::new();
        for item in to_remove {
            *remove_counts.entry(item).or_insert(0) += 1;
        }

        // Single pass filter - O(n)
        let mut result = Vec::with_capacity(from.len());
        for item in from {
            if let Some(count) = remove_counts.get_mut(&item) {
                if *count > 0 {
                    *count -= 1;
                    continue;
                }
            }
            result.push(item.clone());
        }

        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "multiset_diff.EXIT",
            from_items = from.len(),
            removed = from.len() - result.len(),
            result_items = result.len(),
            "multiset difference result",
        );

        result
    }

    fn compute_value_change(
        history_pointer: &Blake2b256Hash,
        start_value: impl Fn(&Blake2b256Hash) -> Result<Vec<Vec<u8>>, HistoryError>,
        end_value: impl Fn(&Blake2b256Hash) -> Result<Vec<Vec<u8>>, HistoryError>,
    ) -> Result<ChannelChange<Vec<u8>>, HistoryError> {
        let start = start_value(history_pointer)?;
        let end = end_value(history_pointer)?;

        // Use multiset diff for correct merge semantics
        // added = endValue diff startValue (items in end that aren't in start,
        // multiset)
        let added = Self::multiset_diff(&end, &start);

        // deleted = startValue diff endValue (items in start that aren't in end,
        // multiset)
        let deleted = Self::multiset_diff(&start, &end);

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_value_change.EXIT",
                channel = %hex::encode(history_pointer.clone().bytes()),
                start_items = start.len(),
                end_items = end.len(),
                added = added.len(),
                removed = deleted.len(),
                "computed per-channel pre/post diff",
            );
        }

        Ok(ChannelChange {
            added,
            removed: deleted,
        })
    }

    pub fn empty() -> Self {
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "StateChange::empty",
            "creating empty StateChange",
        );
        Self {
            datums_changes: FrozenMap::default(),
            cont_changes: FrozenMap::default(),
            consume_channels_to_join_serialized_map: FrozenMap::default(),
        }
    }

    pub fn from_parts(
        datums_changes: HashMap<Blake2b256Hash, ChannelChange<Vec<u8>>>,
        cont_changes: HashMap<Vec<Blake2b256Hash>, ChannelChange<Vec<u8>>>,
        consume_channels_to_join_serialized_map: HashMap<Vec<Blake2b256Hash>, Vec<u8>>,
    ) -> Self {
        Self {
            datums_changes: FrozenMap::from_map(datums_changes),
            cont_changes: FrozenMap::from_map(cont_changes),
            consume_channels_to_join_serialized_map: FrozenMap::from_map(
                consume_channels_to_join_serialized_map,
            ),
        }
    }

    pub fn combine(self, other: Self) -> Self {
        self.merge(other, true, false)
            .expect("legacy StateChange merge cannot fail")
    }

    pub fn join(self, other: Self) -> Self {
        self.merge(other, false, false)
            .expect("legacy StateChange join cannot fail")
    }

    pub fn additive_join(self, other: Self) -> Result<Self, HistoryError> {
        self.merge(other, false, true)
    }

    fn merge(
        self,
        other: Self,
        normalize_channels: bool,
        additive_channels: bool,
    ) -> Result<Self, HistoryError> {
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "StateChange::combine.ENTER",
            self_datums = self.datums_changes.len(),
            self_conts = self.cont_changes.len(),
            self_joins = self.consume_channels_to_join_serialized_map.len(),
            other_datums = other.datums_changes.len(),
            other_conts = other.cont_changes.len(),
            other_joins = other.consume_channels_to_join_serialized_map.len(),
            normalize_channels,
            "combining two StateChanges (multiset union per channel)",
        );

        let mut datums_changes = self.datums_changes.into_owned();
        let mut cont_changes = self.cont_changes.into_owned();
        let mut consume_channels_to_join_serialized_map =
            self.consume_channels_to_join_serialized_map.into_owned();

        // Combine datum changes via ChannelChange::combine (multiset union)
        for (key, value) in other.datums_changes.into_owned() {
            match datums_changes.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let current = std::mem::replace(entry.get_mut(), ChannelChange::empty());
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.DATUM_ACCUM",
                            channel = %hex::encode(entry.key().clone().bytes()),
                            existing_added = current.added.len(),
                            existing_removed = current.removed.len(),
                            incoming_added = value.added.len(),
                            incoming_removed = value.removed.len(),
                            "datum channel present on both sides -> accumulating",
                        );
                    }
                    let merged = if additive_channels {
                        current.additive_join(value)
                    } else if normalize_channels {
                        current.combine(value)
                    } else {
                        current.join(value)
                    };
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.DATUM_ACCUM_RESULT",
                            channel = %hex::encode(entry.key().clone().bytes()),
                            merged_added = merged.added.len(),
                            merged_removed = merged.removed.len(),
                            "datum channel accumulated total",
                        );
                    }
                    *entry.get_mut() = merged;
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.DATUM_NEW",
                            channel = %hex::encode(entry.key().clone().bytes()),
                            added = value.added.len(),
                            removed = value.removed.len(),
                            "datum channel only on other side -> inserted as-is",
                        );
                    }
                    entry.insert(value);
                }
            }
        }

        // Combine continuation changes via ChannelChange::combine (multiset union)
        for (key, value) in other.cont_changes.into_owned() {
            match cont_changes.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let current = std::mem::replace(entry.get_mut(), ChannelChange::empty());
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.CONT_ACCUM",
                            consume_pointer =
                                %hex::encode(stable_hash_provider::hash_from_hashes(entry.key()).bytes()),
                            n_channels = entry.key().len(),
                            existing_added = current.added.len(),
                            existing_removed = current.removed.len(),
                            incoming_added = value.added.len(),
                            incoming_removed = value.removed.len(),
                            "cont consume present on both sides -> accumulating",
                        );
                    }
                    let merged = if additive_channels {
                        current.additive_join(value)
                    } else if normalize_channels {
                        current.combine(value)
                    } else {
                        current.join(value)
                    };
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.CONT_ACCUM_RESULT",
                            consume_pointer =
                                %hex::encode(stable_hash_provider::hash_from_hashes(entry.key()).bytes()),
                            merged_added = merged.added.len(),
                            merged_removed = merged.removed.len(),
                            "cont consume accumulated total",
                        );
                    }
                    *entry.get_mut() = merged;
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(
                            target: "f1r3fly.merge.step",
                            step = "StateChange::combine.CONT_NEW",
                            consume_pointer =
                                %hex::encode(stable_hash_provider::hash_from_hashes(entry.key()).bytes()),
                            n_channels = entry.key().len(),
                            added = value.added.len(),
                            removed = value.removed.len(),
                            "cont consume only on other side -> inserted as-is",
                        );
                    }
                    entry.insert(value);
                }
            }
        }

        for (key, value) in other.consume_channels_to_join_serialized_map.into_owned() {
            if additive_channels {
                match consume_channels_to_join_serialized_map.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        if entry.get() != &value {
                            return Err(HistoryError::MergeError(format!(
                                "inconsistent serialized join for consume channels {:?}",
                                entry.key()
                            )));
                        }
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            } else {
                consume_channels_to_join_serialized_map.insert(key, value);
            }
        }

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            for (channel, value) in &datums_changes {
                let added_bytes: usize = value.added.iter().map(|d| d.len()).sum();
                let removed_bytes: usize = value.removed.iter().map(|d| d.len()).sum();
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "StateChange::combine.EXIT_DATUM",
                    channel = %hex::encode(channel.clone().bytes()),
                    added = value.added.len(),
                    added_bytes,
                    removed = value.removed.len(),
                    removed_bytes,
                    "combined datum channel summary",
                );
            }
        }

        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "StateChange::combine.EXIT",
            datums_changes = datums_changes.len(),
            cont_changes = cont_changes.len(),
            joins = consume_channels_to_join_serialized_map.len(),
            "combined StateChange",
        );

        Ok(Self {
            datums_changes: FrozenMap::from_map(datums_changes),
            cont_changes: FrozenMap::from_map(cont_changes),
            consume_channels_to_join_serialized_map: FrozenMap::from_map(
                consume_channels_to_join_serialized_map,
            ),
        })
    }

    pub fn normalized(self) -> Self {
        let mut datums_changes = self.datums_changes.into_owned();
        let mut cont_changes = self.cont_changes.into_owned();
        let joins = self.consume_channels_to_join_serialized_map.into_owned();
        for change in datums_changes.values_mut() {
            let current = std::mem::replace(change, ChannelChange::empty());
            *change = current.canonicalized();
        }
        for change in cont_changes.values_mut() {
            let current = std::mem::replace(change, ChannelChange::empty());
            *change = current.canonicalized();
        }
        datums_changes.retain(|_, change| !(change.added.is_empty() && change.removed.is_empty()));
        cont_changes.retain(|_, change| !(change.added.is_empty() && change.removed.is_empty()));
        Self {
            datums_changes: FrozenMap::from_map(datums_changes),
            cont_changes: FrozenMap::from_map(cont_changes),
            consume_channels_to_join_serialized_map: FrozenMap::from_map(joins),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for entry in self.datums_changes.iter() {
            total = total
                .saturating_add(std::mem::size_of::<Blake2b256Hash>())
                .saturating_add(channel_change_retained_bytes(entry.value()));
        }
        for entry in self.cont_changes.iter() {
            total = total
                .saturating_add(
                    entry
                        .key()
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Blake2b256Hash>()),
                )
                .saturating_add(channel_change_retained_bytes(entry.value()));
        }
        for entry in self.consume_channels_to_join_serialized_map.iter() {
            total = total
                .saturating_add(
                    entry
                        .key()
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Blake2b256Hash>()),
                )
                .saturating_add(entry.value().capacity());
        }
        total
    }
}

fn channel_change_retained_bytes(change: &ChannelChange<Vec<u8>>) -> usize {
    change
        .added
        .iter()
        .chain(change.removed.iter())
        .fold(std::mem::size_of::<ChannelChange<Vec<u8>>>(), |total, item| {
            total.saturating_add(item.capacity())
        })
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::collections::hash_map::DefaultHasher;

    use proptest::prelude::*;

    use super::*;

    fn datum_change(channel: u8, datum: u8) -> StateChange {
        StateChange::from_parts(
            HashMap::from([(Blake2b256Hash::from_bytes(vec![channel; 32]), ChannelChange {
                added: vec![vec![datum]],
                removed: Vec::new(),
            })]),
            HashMap::new(),
            HashMap::new(),
        )
    }

    #[test]
    fn combining_a_shared_snapshot_does_not_mutate_the_original() {
        let original = datum_change(1, 10);
        let combined = original.clone().combine(datum_change(2, 20));

        assert_eq!(original.datums_changes.len(), 1);
        assert_eq!(combined.datums_changes.len(), 2);
    }

    #[test]
    fn ordering_distinguishes_equal_sized_state_changes() {
        let left = datum_change(1, 10);
        let right = datum_change(2, 10);

        assert_ne!(left, right);
        assert_ne!(left.cmp(&right), Ordering::Equal);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn frozen_map_equality_order_and_hash_ignore_insertion_order(
            entries in prop::collection::btree_map(any::<u8>(), any::<u16>(), 0..64)
        ) {
            let forward = entries.iter().map(|(key, value)| (*key, *value)).collect::<Vec<_>>();
            let reverse = forward.iter().rev().copied().collect::<Vec<_>>();
            let left = FrozenMap::from_map(forward);
            let right = FrozenMap::from_map(reverse);
            let mut left_hash = DefaultHasher::new();
            let mut right_hash = DefaultHasher::new();

            left.hash(&mut left_hash);
            right.hash(&mut right_hash);

            prop_assert_eq!(&left, &right);
            prop_assert_eq!(left.cmp(&right), Ordering::Equal);
            prop_assert_eq!(left_hash.finish(), right_hash.finish());
        }
    }
}

#[cfg(test)]
mod behavior_tests {
    use std::collections::HashMap;
    use std::collections::hash_map::DefaultHasher;

    use super::*;

    fn mk_hash(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

    fn bytes(byte: u8) -> Vec<u8> { vec![byte; 4] }

    fn change(added: &[u8], removed: &[u8]) -> ChannelChange<Vec<u8>> {
        ChannelChange {
            added: added.iter().map(|b| bytes(*b)).collect(),
            removed: removed.iter().map(|b| bytes(*b)).collect(),
        }
    }

    fn state_change(
        datums: Vec<(u8, ChannelChange<Vec<u8>>)>,
        conts: Vec<(u8, ChannelChange<Vec<u8>>)>,
        joins: Vec<(u8, u8)>,
    ) -> StateChange {
        StateChange::from_parts(
            datums
                .into_iter()
                .map(|(ch, c)| (mk_hash(ch), c))
                .collect::<HashMap<_, _>>(),
            conts
                .into_iter()
                .map(|(ch, c)| (vec![mk_hash(ch)], c))
                .collect::<HashMap<_, _>>(),
            joins
                .into_iter()
                .map(|(ch, join)| (vec![mk_hash(ch)], bytes(join)))
                .collect::<HashMap<_, _>>(),
        )
    }

    fn hash_of(sc: &StateChange) -> u64 {
        let mut hasher = DefaultHasher::new();
        sc.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn multiset_diff_removes_each_item_exactly_once() {
        let x = bytes(1);
        let y = bytes(2);
        let from = vec![x.clone(), x.clone(), y.clone()];
        let result = StateChange::multiset_diff(&from, &[x.clone()]);
        assert_eq!(result, vec![x, y]);
    }

    #[test]
    fn multiset_diff_ignores_absent_removals_and_empty_inputs() {
        let x = bytes(1);
        assert!(StateChange::multiset_diff(&[], &[x.clone()]).is_empty());
        assert_eq!(StateChange::multiset_diff(&[x.clone()], &[]), vec![x.clone()]);
        assert_eq!(StateChange::multiset_diff(&[x.clone()], &[bytes(9)]), vec![x]);
    }

    #[test]
    fn empty_state_change_has_no_entries() {
        let sc = StateChange::empty();
        assert!(sc.datums_changes.is_empty());
        assert!(sc.cont_changes.is_empty());
        assert!(sc.consume_channels_to_join_serialized_map.is_empty());
    }

    #[test]
    fn combine_inserts_entries_present_only_on_one_side() {
        let left = state_change(vec![(1, change(&[10], &[]))], vec![], vec![(3, 30)]);
        let right = state_change(vec![], vec![(2, change(&[20], &[]))], vec![(4, 40)]);
        let combined = left.combine(right);

        assert_eq!(combined.datums_changes.get(&mk_hash(1)).unwrap().added, vec![bytes(10)]);
        assert_eq!(combined.cont_changes.get(&vec![mk_hash(2)]).unwrap().added, vec![bytes(20)]);
        assert_eq!(
            *combined
                .consume_channels_to_join_serialized_map
                .get(&vec![mk_hash(3)])
                .unwrap(),
            bytes(30)
        );
        assert_eq!(
            *combined
                .consume_channels_to_join_serialized_map
                .get(&vec![mk_hash(4)])
                .unwrap(),
            bytes(40)
        );
    }

    #[test]
    fn combine_accumulates_distinct_values_on_shared_channels() {
        let left =
            state_change(vec![(1, change(&[10], &[]))], vec![(2, change(&[20], &[]))], vec![]);
        let right =
            state_change(vec![(1, change(&[11], &[]))], vec![(2, change(&[21], &[]))], vec![]);
        let combined = left.combine(right);

        let datum = combined.datums_changes.get(&mk_hash(1)).unwrap();
        assert_eq!(datum.added.len(), 2);
        assert!(datum.added.contains(&bytes(10)) && datum.added.contains(&bytes(11)));
        assert!(datum.removed.is_empty());

        let cont = combined.cont_changes.get(&vec![mk_hash(2)]).unwrap();
        assert_eq!(cont.added.len(), 2);
        assert!(cont.added.contains(&bytes(20)) && cont.added.contains(&bytes(21)));
    }

    #[test]
    fn combine_nets_producer_consumer_pair_on_shared_channel() {
        let left = state_change(vec![(1, change(&[10], &[]))], vec![], vec![]);
        let right = state_change(vec![(1, change(&[], &[10]))], vec![], vec![]);
        let combined = left.combine(right);

        let datum = combined.datums_changes.get(&mk_hash(1)).unwrap();
        assert!(datum.added.is_empty());
        assert!(datum.removed.is_empty());
    }

    #[test]
    fn combine_join_map_takes_other_sides_value_on_shared_key() {
        let left = state_change(vec![], vec![], vec![(1, 10)]);
        let right = state_change(vec![], vec![], vec![(1, 11)]);
        let combined = left.combine(right);
        assert_eq!(
            *combined
                .consume_channels_to_join_serialized_map
                .get(&vec![mk_hash(1)])
                .unwrap(),
            bytes(11)
        );
    }

    #[test]
    fn eq_holds_for_identical_state_changes() {
        let a =
            state_change(vec![(1, change(&[10], &[11]))], vec![(2, change(&[20], &[]))], vec![(
                3, 30,
            )]);
        let b =
            state_change(vec![(1, change(&[10], &[11]))], vec![(2, change(&[20], &[]))], vec![(
                3, 30,
            )]);
        assert_eq!(a, b);
    }

    #[test]
    fn eq_detects_size_and_value_mismatches() {
        let base = state_change(vec![(1, change(&[10], &[]))], vec![], vec![]);

        let extra_entry =
            state_change(vec![(1, change(&[10], &[])), (2, change(&[12], &[]))], vec![], vec![]);
        assert_ne!(base, extra_entry);

        let different_value = state_change(vec![(1, change(&[11], &[]))], vec![], vec![]);
        assert_ne!(base, different_value);

        let different_key = state_change(vec![(2, change(&[10], &[]))], vec![], vec![]);
        assert_ne!(base, different_key);
    }

    #[test]
    fn eq_detects_cont_and_join_mismatches() {
        let a = state_change(vec![], vec![(1, change(&[10], &[]))], vec![(2, 20)]);
        let cont_differs = state_change(vec![], vec![(1, change(&[11], &[]))], vec![(2, 20)]);
        let join_differs = state_change(vec![], vec![(1, change(&[10], &[]))], vec![(2, 21)]);
        assert_ne!(a, cont_differs);
        assert_ne!(a, join_differs);
    }

    #[test]
    fn hash_is_deterministic_for_equal_state_changes() {
        let build = |reversed: bool| {
            let mut datums = vec![(1, change(&[10, 12], &[11])), (2, change(&[13], &[]))];
            if reversed {
                datums.reverse();
            }
            state_change(datums, vec![(3, change(&[20], &[21]))], vec![(4, 40)])
        };
        let a = build(false);
        let b = build(true);
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    #[test]
    fn ordering_compares_by_total_entry_count() {
        let small = state_change(vec![(1, change(&[10], &[]))], vec![], vec![]);
        let large =
            state_change(vec![(1, change(&[10], &[]))], vec![(2, change(&[20], &[]))], vec![]);
        assert_eq!(small.cmp(&large), std::cmp::Ordering::Less);
        assert_eq!(large.cmp(&small), std::cmp::Ordering::Greater);
        assert_eq!(small.partial_cmp(&large), Some(std::cmp::Ordering::Less));
        assert_eq!(small.cmp(&small.clone()), std::cmp::Ordering::Equal);
    }
}
