use std::borrow::Cow;
use std::cmp::Ordering;
use std::mem::size_of;

use shared::rust::clone_backing::CloneBacking;
use shared::rust::collection_backing::{tree_backing, tree_growth, tree_search_bound};

use super::*;
use crate::rspace::candidate_order::{CandidateSource, OrderWork, canonical_order};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{
    self, GroupKeys, OperationKeys, SourceMeter, StoreKey,
};
use crate::rspace::hot_store::NativeDataView;
use crate::rspace::native_backing;

type Result<T> = std::result::Result<T, RSpaceError>;
// Changed by C2 (DR-82): the reader returns a copy-free view.
// type DataReader<'a, C, A> = &'a dyn Fn(&C) -> Result<Vec<Datum<A>>>;
// Changed by D-C2c (D-S1, DR-96): the reader receives the channel key.
// type DataReader<'a, C, A> = &'a dyn Fn(&C) -> Result<NativeDataView<C, A>>;
type DataReader<'a, C, A> = &'a dyn Fn(&C, StoreKey) -> Result<NativeDataView<C, A>>;
// Changed by C1 (DR-81): the reader returns shared views.
// type ContinuationReader<'a, C, P, K> = &'a dyn Fn(&[C]) ->
// Result<Vec<WaitingContinuation<P, K>>>;
// Changed by D-C2c (D-S1, DR-96): the reader receives the group keys.
// type ContinuationReader<'a, C, P, K> =
//     &'a dyn Fn(&[C]) -> Result<Vec<Arc<WaitingContinuation<P, K>>>>;
type ContinuationReader<'a, C, P, K> =
    &'a dyn Fn(&[C], &GroupKeys) -> Result<Vec<Arc<WaitingContinuation<P, K>>>>;

pub(in crate::rspace::replay_rspace) struct CandidateReader<'a, C, P: Clone, A: Clone, K: Clone> {
    pub(in crate::rspace::replay_rspace) meter: &'a (dyn SourceMeter + Send + Sync),
    pub(in crate::rspace::replay_rspace) data: DataReader<'a, C, A>,
    pub(in crate::rspace::replay_rspace) continuations: ContinuationReader<'a, C, P, K>,
}

fn buffer<T>(length: usize, meter: &dyn SourceMeter) -> Result<Vec<T>> {
    let bytes = length
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(length.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

/// D-D4 (DR-106): the bytes charged for one recorded channel position. The
/// constant keeps the charge independent of the platform's layout, and the
/// assertion keeps the charge at or above the allocation.
const POSITION_BYTES: usize = 8;
const _: () = assert!(size_of::<usize>() <= POSITION_BYTES);

/// D-D4 (DR-106): a buffer for `length` channel positions. It charges one
/// operation for each slot plus one, and `POSITION_BYTES` for each slot.
fn position_buffer(length: usize, meter: &dyn SourceMeter) -> Result<Vec<usize>> {
    let bytes = length
        .checked_mul(POSITION_BYTES)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(length.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, bytes, bytes)?;
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(length)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(positions)
}

/// D-D5 (DR-107): the datum index of the incoming datum of a produce
/// (`native_candidate.rs`). The retirement skips it, because it retires only
/// stored data (`native_session/result.rs`).
const INCOMING_INDEX: i32 = -1;

/// D-D5 (DR-107): the bytes charged to read the datum index of one
/// candidate in the fill. The constant keeps the charge independent of the
/// platform's layout, and the assertion keeps it at or above the read.
const INDEX_BYTES: usize = 4;
const _: () = assert!(size_of::<i32>() <= INDEX_BYTES);

/// D-D5 (DR-107): the selection of a produce owns the incoming datum. A
/// produce that matches nothing gets its datum back to store it, and a
/// matched produce gets its source back for the counter and the trace.
pub(in crate::rspace::replay_rspace) enum ProduceSelection<C, P: Clone, A: Clone, K: Clone> {
    Unmatched(Datum<A>),
    Matched {
        prepared: PreparedProduceCandidate<C, P, A, K>,
        group: usize,
        source: Produce,
    },
}

/// D-D5 (DR-107): gives the incoming value to the incoming candidates of
/// the selected match. The first one receives the value itself, and each
/// further one (a persistent datum matched through a repeated channel)
/// receives a copy. The pass reserves its reads and the move first, and
/// each copy reserves its copy and cleanup before the copy.
fn fill_incoming<C, A: Clone + CloneBacking>(
    candidates: &mut [ConsumeCandidate<C, A>],
    value: A,
    meter: &dyn SourceMeter,
) -> Result<()> {
    let count = candidates.len();
    let moved = size_of::<A>()
        .checked_mul(2)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let scanned = count
        .checked_mul(INDEX_BYTES)
        .and_then(|bytes| bytes.checked_add(moved))
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, scanned, 0)?;
    let mut first = None;
    for candidate in candidates.iter_mut() {
        if candidate.datum_index != INCOMING_INDEX {
            continue;
        }
        if first.is_none() {
            first = Some(candidate);
            continue;
        }
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(&value, meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(&value, meter)?;
        candidate.removed_datum = value.clone();
    }
    if let Some(first) = first {
        first.removed_datum = value;
    }
    Ok(())
}

// Legacy metered (digest, index) order: it inspected and hashed every candidate
// on every operation. Disabled by I1 (DR-75); the canonical order below
// digests only tie runs and matches play's
// `candidate_order::canonical_candidates`. fn sorted<D: Serialize +
// CloneBacking>(     values: Vec<D>,
//     meter: &dyn SourceMeter,
// ) -> Result<Vec<(D, i32)>> {
//     if values.len() > i32::MAX as usize {
//         return Err(RSpaceError::HostWorkRejected);
//     }
//     let mut indexed = buffer(values.len(), meter)?;
//     for (index, value) in values.into_iter().enumerate() {
//         native_backing::inspect(&value, meter)?;
//         let digest = native_source::hash(&value, &|operations, scanned,
// backing| {             meter.reserve(operations, scanned, backing)
//         })?;
//         indexed.push((value, index as i32, digest));
//     }
//     sort(
//         &mut indexed,
//         meter,
//         |a, b| a.2.cmp(&b.2).then_with(|| a.1.cmp(&b.1)),
//         |value| Ok(value.2.0.len()),
//     )?;
//     let mut result = buffer(indexed.len(), meter)?;
//     result.extend(indexed.into_iter().map(|(value, index, _)| (value,
// index)));     Ok(result)
// }

/// One `BTreeMap` lookup among `entries` produce keys: at most
/// `tree_search_bound(entries)` comparisons, each reading two hashes of
/// `hash_bytes` bytes (C3, DR-78;
/// `OrderedLookupBound.search_within_size_bound`).
fn reserve_ordered_lookup(
    entries: usize,
    hash_bytes: usize,
    meter: &dyn SourceMeter,
) -> Result<()> {
    let comparisons = tree_search_bound(entries);
    meter.reserve(
        comparisons,
        hash_bytes
            .checked_mul(2)
            .and_then(|pair| pair.checked_mul(comparisons))
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )
}

struct MeteredOrder<'a>(&'a dyn SourceMeter);

impl<D: Serialize + CloneBacking> OrderWork<D> for MeteredOrder<'_> {
    type Error = RSpaceError;

    fn index_bound(&self, length: usize) -> Result<i32> {
        i32::try_from(length).map_err(|_| RSpaceError::HostWorkRejected)
    }

    fn buffer<T>(&self, length: usize) -> Result<Vec<T>> { buffer(length, self.0) }

    fn sort<T>(
        &self,
        values: &mut [T],
        compare: impl Fn(&T, &T) -> Ordering,
        scanned: impl Fn(&T) -> usize,
    ) -> Result<()> {
        sort(values, self.0, compare, |value| Ok(scanned(value)))
    }

    fn scan(&self, left: &Blake2b256Hash, right: &Blake2b256Hash) -> Result<()> {
        self.0.reserve(
            1,
            left.0
                .len()
                .checked_add(right.0.len())
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )
    }

    fn digest(&self, value: &D) -> Result<Blake2b256Hash> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(value, self.0)?;
        native_backing::inspect_blocks(value, self.0)?;
        native_source::hash(value, &|operations, scanned, backing| {
            self.0.reserve(operations, scanned, backing)
        })
    }
}

fn sorted<D: CandidateSource + Serialize + CloneBacking>(
    values: Vec<D>,
    meter: &dyn SourceMeter,
) -> Result<Vec<(D, i32)>> {
    canonical_order(values, &MeteredOrder(meter))
}

fn sort<T>(
    values: &mut [T],
    meter: &dyn SourceMeter,
    compare: impl Fn(&T, &T) -> Ordering,
    scanned: impl Fn(&T) -> Result<usize>,
) -> Result<()> {
    let moves = size_of::<T>()
        .checked_mul(2)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(
        values.len(),
        values
            .len()
            .checked_mul(moves)
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )?;
    shared::rust::fallible_sort::sort(values, |a, b| {
        let left = scanned(a)?;
        let right = scanned(b)?;
        meter.reserve(
            1,
            moves
                .checked_add(left)
                .and_then(|bytes| bytes.checked_add(right))
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        Ok(compare(a, b))
    })
}

// Changed by C2 (DR-82): the values borrow the cached data; only the
// incoming datum is owned.
// struct ChannelData<C, A: Clone> {
//     channel: C,
//     values: Vec<(Datum<A>, i32)>,
// }
// Changed by D-D4 (DR-106): the entry borrows its channel, which is only
// compared, so the channel is not copied.
// struct ChannelData<'v, C, A: Clone> {
//     channel: C,
//     values: Vec<(Cow<'v, Datum<A>>, i32)>,
// }
struct ChannelData<'v, C, A: Clone> {
    channel: &'v C,
    values: Vec<(Cow<'v, Datum<A>>, i32)>,
}

pub(in crate::rspace::replay_rspace) struct PreparedProduceCounter<'a> {
    counters: Option<std::sync::MutexGuard<'a, BTreeMap<Produce, i32>>>,
    entry: Option<(Produce, i32)>,
}

impl PreparedProduceCounter<'_> {
    pub(in crate::rspace::replay_rspace) fn publish(mut self) {
        if let (Some(counters), Some((source, count))) = (&mut self.counters, self.entry.take()) {
            counters.insert(source, count);
        }
    }
}

fn channel_position<C: Eq + CloneBacking, A: Clone>(
    channels: &[ChannelData<'_, C, A>],
    channel: &C,
    meter: &dyn SourceMeter,
) -> Result<Option<usize>> {
    for (position, existing) in channels.iter().enumerate() {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(channel, meter)?;
        native_backing::inspect_blocks(channel, meter)?;
        // Changed by D-D4 (DR-106): the entry borrows its channel.
        // native_backing::inspect(&existing.channel, meter)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(existing.channel, meter)?;
        native_backing::inspect_blocks(existing.channel, meter)?;
        meter.reserve(1, 0, 0)?;
        // if channel == &existing.channel {
        if channel == existing.channel {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + CloneBacking + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + CloneBacking + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + CloneBacking + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + CloneBacking + 'static + Sync + Send,
{
    pub(in crate::rspace::replay_rspace) fn prepare_metered_produce_counter(
        &'_ self,
        source: &Produce,
        persist: bool,
        meter: &dyn SourceMeter,
    ) -> Result<PreparedProduceCounter<'_>> {
        if persist {
            return Ok(PreparedProduceCounter {
                counters: None,
                entry: None,
            });
        }
        let counters = self.produce_counter.lock().expect("produce counter lock");
        // Disabled by C3 (DR-78): this charged two comparisons per entry and
        // walked every key only to charge it, while the lookup and the insert
        // below are two B-tree searches.
        // let entries = counters
        //     .len()
        //     .checked_add(1)
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        // let comparisons = entries
        //     .checked_mul(2)
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        // meter.reserve(
        //     comparisons
        //         .checked_mul(32)
        //         .ok_or(RSpaceError::HostWorkRejected)?,
        //     source
        //         .hash
        //         .0
        //         .len()
        //         .checked_mul(comparisons)
        //         .ok_or(RSpaceError::HostWorkRejected)?,
        //     0,
        // )?;
        // for existing in counters.keys() {
        //     meter.reserve(
        //         2,
        //         existing
        //             .hash
        //             .0
        //             .len()
        //             .checked_mul(2)
        //             .ok_or(RSpaceError::HostWorkRejected)?,
        //         0,
        //     )?;
        // }
        reserve_ordered_lookup(counters.len(), source.hash.0.len(), meter)?;
        reserve_ordered_lookup(counters.len(), source.hash.0.len(), meter)?;
        let next = counters
            .get(source)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(source, meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(source, meter)?;
        // Disabled by C13 (DR-77): the produce-counter map lives for the whole
        // replay, so charging its whole backing on every produce charged a
        // quadratic total.
        // let (operations, bytes) = tree_backing::<Produce, i32>(entries)
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        let (operations, bytes) =
            tree_growth::<Produce, i32>(counters.len(), 1).ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(operations, bytes, bytes)?;
        Ok(PreparedProduceCounter {
            counters: Some(counters),
            entry: Some((source.clone(), next)),
        })
    }

    fn metered_produce_count(&self, source: &Produce, meter: &dyn SourceMeter) -> Result<i32> {
        let counters = self.produce_counter.lock().expect("produce counter lock");
        // Disabled by C3 (DR-78): this charged one comparison per entry and
        // walked every key only to charge it, while the lookup below is one
        // B-tree search.
        // let steps = counters
        //     .len()
        //     .checked_add(1)
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        // meter.reserve(
        //     steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
        //     source
        //         .hash
        //         .0
        //         .len()
        //         .checked_mul(steps)
        //         .ok_or(RSpaceError::HostWorkRejected)?,
        //     0,
        // )?;
        // for key in counters.keys() {
        //     meter.reserve(1, key.hash.0.len(), 0)?;
        // }
        reserve_ordered_lookup(counters.len(), source.hash.0.len(), meter)?;
        Ok(*counters.get(source).unwrap_or(&0))
    }

    fn metered_candidate_matches(
        &self,
        datum: &Datum<A>,
        expected: Option<&dyn NativeCandidateIdentity>,
        incoming: Option<&Produce>,
        meter: &dyn SourceMeter,
    ) -> Result<bool> {
        let Some(expected) = expected else {
            return Ok(true);
        };
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(&datum.source, meter)?;
        native_backing::inspect_blocks(&datum.source, meter)?;
        if !expected.metered_matches_produce(&datum.source, meter)? {
            return Ok(false);
        }
        if datum.persist {
            return Ok(true);
        }
        let count = self.metered_produce_count(&datum.source, meter)?;
        let increment = if let Some(incoming) = incoming {
            meter.reserve(
                1,
                incoming
                    .hash
                    .0
                    .len()
                    .checked_add(datum.source.hash.0.len())
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            incoming == &datum.source
        } else {
            false
        };
        let count = if increment {
            count.checked_add(1)
        } else {
            Some(count)
        };
        Ok(match count {
            Some(count) => expected.metered_repetition(&datum.source, meter)? == Some(count),
            None => false,
        })
    }

    /// One copy-free view of the cached data of each channel (C2, DR-82),
    /// read by the channel's key (D-C2c, DR-96).
    fn metered_data_views(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Vec<NativeDataView<C, A>>> {
        if keys.len() != channels.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        let mut views = buffer(channels.len(), reader.meter)?;
        for (channel, key) in channels.iter().zip(keys) {
            views.push((reader.data)(channel, *key)?);
        }
        Ok(views)
    }

    /// One entry for each distinct channel, and for each channel of
    /// `channels` the position of its entry (D-D4, DR-106). The entries
    /// borrow the channels, and the incoming datum of a produce (D-D5,
    /// DR-107).
    fn metered_channel_data<'v>(
        &self,
        channels: &'v [C],
        views: &'v [NativeDataView<C, A>],
        // Changed by D-D5 (DR-107): the entries borrow the incoming datum.
        // incoming: Option<(&C, &A, bool, &Produce)>,
        incoming: Option<(&C, &'v Datum<A>)>,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<(Vec<ChannelData<'v, C, A>>, Vec<usize>)> {
        let mut result = buffer(channels.len(), reader.meter)?;
        let mut positions = position_buffer(channels.len(), reader.meter)?;
        for (channel, view) in channels.iter().zip(views) {
            // Changed by C2 (DR-82): the candidates borrow the cached data.
            // let mut values = sorted((reader.data)(channel)?, reader.meter)?;
            let mut borrowed = buffer(view.values().len(), reader.meter)?;
            for datum in view.values() {
                reader.meter.reserve(1, size_of::<Cow<'v, Datum<A>>>(), 0)?;
                borrowed.push(Cow::Borrowed(datum));
            }
            let mut values = sorted(borrowed, reader.meter)?;
            // Changed by D-D5 (DR-107): the incoming datum is borrowed.
            // if let Some((trigger, data, persist, source)) = incoming {
            if let Some((trigger, datum)) = incoming {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect(channel, reader.meter)?;
                // native_backing::inspect(trigger, reader.meter)?;
                native_backing::inspect_blocks(channel, reader.meter)?;
                native_backing::inspect_blocks(trigger, reader.meter)?;
                if channel == trigger {
                    let count = values
                        .len()
                        .checked_add(1)
                        .ok_or(RSpaceError::HostWorkRejected)?;
                    let mut all = buffer(count, reader.meter)?;
                    // Changed by D-D5 (DR-107): the entry borrows the
                    // incoming datum, as it borrows the cached data (DR-82),
                    // so the value and the source are not copied.
                    // native_backing::reserve_copy_and_cleanup(data, reader.meter)?;
                    // native_backing::reserve_copy_and_cleanup(source, reader.meter)?;
                    // all.push((
                    //     Cow::Owned(Datum {
                    //         a: data.clone(),
                    //         persist,
                    //         source: source.clone(),
                    //     }),
                    //     -1,
                    // ));
                    reader.meter.reserve(1, size_of::<Cow<'v, Datum<A>>>(), 0)?;
                    all.push((Cow::Borrowed(datum), INCOMING_INDEX));
                    all.extend(values);
                    values = all;
                }
            }
            let mut position = 0;
            while position < values.len() {
                // Changed by D-D5 (DR-107): the source of the borrowed datum.
                // let source = incoming.and_then(|(_, _, persist, source)| {
                //     (!persist).then_some(source)
                // });
                let source =
                    incoming.and_then(|(_, datum)| (!datum.persist).then_some(&datum.source));
                if self.metered_candidate_matches(
                    &values[position].0,
                    expected,
                    source,
                    reader.meter,
                )? {
                    position += 1;
                } else {
                    reader.meter.reserve(
                        values.len(),
                        values
                            .len()
                            .checked_mul(size_of::<(Cow<'v, Datum<A>>, i32)>())
                            .ok_or(RSpaceError::HostWorkRejected)?,
                        0,
                    )?;
                    values.remove(position);
                }
            }
            // Changed by D-D4 (DR-106): the entry borrows its channel, which
            // is only compared, and the position of the channel's entry is
            // recorded for the match.
            // if let Some(position) = channel_position(&result, channel, reader.meter)? {
            //     result[position].values = values;
            // } else {
            //     native_backing::reserve_copy_and_cleanup(channel, reader.meter)?;
            //     result.push(ChannelData {
            //         channel: channel.clone(),
            //         values,
            //     });
            // }
            match channel_position(&result, channel, reader.meter)? {
                Some(position) => {
                    result[position].values = values;
                    positions.push(position);
                }
                None => {
                    positions.push(result.len());
                    result.push(ChannelData { channel, values });
                }
            }
        }
        // Ok(result)
        Ok((result, positions))
    }

    /// Selects one datum for each pattern. `positions[i]` is the position in
    /// `data` of the entry of `channels[i]`, as `metered_channel_data`
    /// recorded it (D-D4, DR-106).
    fn metered_match_data(
        &self,
        channels: &[C],
        positions: &[usize],
        patterns: &[P],
        continuation: &K,
        data: &[ChannelData<'_, C, A>],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>> {
        if positions.len() != channels.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        let count = channels.len().min(patterns.len());
        let mut chosen = buffer::<(usize, usize)>(count, meter)?;
        let mut candidates = buffer(count, meter)?;
        let mut complete = true;
        // Changed by D-D4 (DR-106): the position of each channel's entry is
        // read, one word for each pattern, instead of searched.
        // for (channel, pattern) in channels.iter().zip(patterns) {
        //     let Some(channel_index) = channel_position(data, channel, meter)? else {
        //         complete = false;
        //         continue;
        //     };
        for ((channel, pattern), recorded) in channels.iter().zip(patterns).zip(positions) {
            meter.reserve(1, POSITION_BYTES, 0)?;
            let channel_index = *recorded;
            let entry = data
                .get(channel_index)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let mut found = false;
            // for (position, (datum, index)) in
            //     data[channel_index].values.iter().enumerate()
            // {
            for (position, (datum, index)) in entry.values.iter().enumerate() {
                meter.reserve(
                    chosen
                        .len()
                        .checked_add(1)
                        .ok_or(RSpaceError::HostWorkRejected)?,
                    chosen
                        .len()
                        .checked_mul(size_of::<(usize, usize)>())
                        .ok_or(RSpaceError::HostWorkRejected)?,
                    0,
                )?;
                if !datum.persist && chosen.contains(&(channel_index, position)) {
                    continue;
                }
                // Disabled by D-M1 (DR-88): get_metered reserves every read of
                // the pattern and the datum; this walk read nothing.
                // native_backing::inspect(pattern, meter)?;
                // native_backing::inspect(&datum.a, meter)?;
                let Some(matched) = self.matcher.get_metered(pattern, &datum.a, meter)? else {
                    continue;
                };
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(channel, meter)?;
                // native_backing::reserve_copy_and_cleanup(&datum.source, meter)?;
                native_backing::reserve_blocks_copy_and_cleanup(channel, meter)?;
                native_backing::reserve_blocks_copy_and_cleanup(&datum.source, meter)?;
                // Changed by D-D5 (DR-107): the removed value of an incoming
                // candidate is the produce's own value, which the produce
                // preparation moves in after the selection (`fill_incoming`).
                // The default value owns no allocation.
                // native_backing::reserve_copy_and_cleanup(&datum.a, meter)?;
                let removed_datum = if *index == INCOMING_INDEX {
                    A::default()
                } else {
                    // Changed by D-O1 (DR-108): block accounting charges inline bytes
                    // once per enclosing block.
                    // native_backing::reserve_copy_and_cleanup(&datum.a, meter)?;
                    native_backing::reserve_blocks_copy_and_cleanup(&datum.a, meter)?;
                    datum.a.clone()
                };
                candidates.push(ConsumeCandidate {
                    channel: channel.clone(),
                    datum: Datum {
                        a: matched,
                        persist: datum.persist,
                        source: datum.source.clone(),
                    },
                    // removed_datum: datum.a.clone(),
                    removed_datum,
                    datum_index: *index,
                });
                if !datum.persist {
                    chosen.push((channel_index, position));
                }
                found = true;
                break;
            }
            if !found {
                complete = false;
            }
        }
        if !complete {
            return Ok(None);
        }
        // Changed by D-M6 (DR-88): the commit check reads the matched data by
        // reference, so they are not copied.
        // let mut matched = buffer(candidates.len(), meter)?;
        // for candidate in &candidates {
        //     native_backing::reserve_copy_and_cleanup(&candidate.datum.a, meter)?;
        //     matched.push(candidate.datum.a.clone());
        // }
        let mut matched = buffer::<&A>(candidates.len(), meter)?;
        for candidate in &candidates {
            matched.push(&candidate.datum.a);
        }
        // Disabled by D-M1 (DR-88): check_commit_metered reserves every read
        // of the continuation (its guard); this walk read nothing.
        // native_backing::inspect(continuation, meter)?;
        if !self
            .matcher
            .check_commit_metered(continuation, &matched, meter)?
        {
            return Ok(None);
        }
        Ok(Some(candidates))
    }

    fn metered_comm(
        &self,
        data: &[ConsumeCandidate<C, A>],
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        incoming: Option<(&Produce, i32)>,
        meter: &dyn SourceMeter,
    ) -> Result<COMM> {
        let mut indexed = buffer(data.len(), meter)?;
        for (index, candidate) in data.iter().enumerate() {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(&candidate.datum.source, meter)?;
            native_backing::reserve_blocks_copy_and_cleanup(&candidate.datum.source, meter)?;
            indexed.push((candidate.datum.source.clone(), index));
        }
        sort(
            &mut indexed,
            meter,
            |a, b| {
                a.0.channel_hash
                    .cmp(&b.0.channel_hash)
                    .then_with(|| a.0.hash.cmp(&b.0.hash))
                    .then_with(|| a.0.persistent.cmp(&b.0.persistent))
                    .then_with(|| a.1.cmp(&b.1))
            },
            |(source, _)| {
                source
                    .channel_hash
                    .0
                    .len()
                    .checked_add(source.hash.0.len())
                    .ok_or(RSpaceError::HostWorkRejected)
            },
        )?;
        let mut produces = buffer(indexed.len(), meter)?;
        produces.extend(indexed.into_iter().map(|(source, _)| source));
        let (operations, bytes) =
            tree_backing::<Produce, i32>(produces.len()).ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(operations, bytes, bytes)?;
        let mut times_repeated: BTreeMap<Produce, i32> = BTreeMap::new();
        for source in produces.iter().rev() {
            let count = self.metered_produce_count(source, meter)?;
            let count = if let Some((incoming, next)) = incoming {
                meter.reserve(
                    1,
                    source
                        .hash
                        .0
                        .len()
                        .checked_add(incoming.hash.0.len())
                        .ok_or(RSpaceError::HostWorkRejected)?,
                    0,
                )?;
                if source == incoming { next } else { count }
            } else {
                count
            };
            // Disabled by C3 (DR-78): these charges walked every key of the
            // map twice only to charge it, for one lookup and one insert.
            // let steps = times_repeated
            //     .len()
            //     .checked_add(1)
            //     .ok_or(RSpaceError::HostWorkRejected)?;
            // meter.reserve(
            //     steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
            //     source
            //         .hash
            //         .0
            //         .len()
            //         .checked_mul(steps)
            //         .ok_or(RSpaceError::HostWorkRejected)?,
            //     0,
            // )?;
            // for key in times_repeated.keys() {
            //     meter.reserve(1, key.hash.0.len(), 0)?;
            // }
            reserve_ordered_lookup(times_repeated.len(), source.hash.0.len(), meter)?;
            if times_repeated.contains_key(source) {
                continue;
            }
            // meter.reserve(
            //     steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
            //     source
            //         .hash
            //         .0
            //         .len()
            //         .checked_mul(steps)
            //         .ok_or(RSpaceError::HostWorkRejected)?,
            //     0,
            // )?;
            // for key in times_repeated.keys() {
            //     meter.reserve(1, key.hash.0.len(), 0)?;
            // }
            reserve_ordered_lookup(times_repeated.len(), source.hash.0.len(), meter)?;
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(source, meter)?;
            native_backing::reserve_blocks_copy_and_cleanup(source, meter)?;
            times_repeated.insert(source.clone(), count);
        }
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(consume, meter)?;
        // native_backing::reserve_copy_and_cleanup(peeks, meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(consume, meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(peeks, meter)?;
        Ok(COMM {
            consume: consume.clone(),
            produces,
            peeks: peeks.clone(),
            times_repeated,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::rspace::replay_rspace) fn prepare_metered_consume_candidate(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(consume, reader.meter)?;
        native_backing::inspect_blocks(consume, reader.meter)?;
        if let Some(identity) = expected {
            if !identity.metered_matches_consume(consume, reader.meter)? {
                return Ok(None);
            }
        }
        // Changed by C2 (DR-82): the channel data borrow copy-free views.
        // let data = self.metered_channel_data(channels, None, expected, reader)?;
        let views = self.metered_data_views(channels, keys, reader)?;
        // Changed by D-D4 (DR-106): the match reads the recorded positions.
        // let data =
        //     self.metered_channel_data(channels, &views, None, expected, reader)?;
        // let Some(data) = self.metered_match_data(
        //     channels,
        //     patterns,
        //     continuation,
        //     &data,
        //     reader.meter,
        // )?
        // else {
        //     return Ok(None);
        // };
        let (data, positions) =
            self.metered_channel_data(channels, &views, None, expected, reader)?;
        let Some(data) = self.metered_match_data(
            channels,
            &positions,
            patterns,
            continuation,
            &data,
            reader.meter,
        )?
        else {
            return Ok(None);
        };
        let comm = self.metered_comm(&data, consume, peeks, None, reader.meter)?;
        if let Some(identity) = expected {
            if !identity.metered_matches_comm(&comm, reader.meter)? {
                return Ok(None);
            }
        }
        Ok(Some(PreparedConsumeCandidate { data, comm }))
    }

    /// The selection of a produce: the produce candidate, the index of its
    /// join group in `grouped_channels` (D-C2c, DR-96: the retirement uses
    /// that group's keys) and the produce's source, or the incoming datum
    /// when nothing matches (D-D5, DR-107).
    // Changed by D-D5 (DR-107): the selection owns the incoming datum, so
    // the selection borrows it and the match moves its value.
    // #[allow(clippy::too_many_arguments)]
    // pub(in crate::rspace::replay_rspace) fn prepare_metered_produce_candidate(
    //     &self,
    //     channel: &C,
    //     value: &A,
    //     persist: bool,
    //     source: &Produce,
    //     grouped_channels: Vec<Vec<C>>,
    //     keys: &OperationKeys,
    //     expected: Option<&dyn NativeCandidateIdentity>,
    //     reader: &CandidateReader<'_, C, P, A, K>,
    // ) -> Result<Option<(PreparedProduceCandidate<C, P, A, K>, usize)>> {
    pub(in crate::rspace::replay_rspace) fn prepare_metered_produce_candidate(
        &self,
        channel: &C,
        incoming: Datum<A>,
        grouped_channels: Vec<Vec<C>>,
        keys: &OperationKeys,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<ProduceSelection<C, P, A, K>> {
        if keys.groups.len() != grouped_channels.len() {
            return Err(RSpaceError::HostWorkRejected);
        }
        // Changed by D-D5 (DR-107): the fields of the incoming datum.
        // let next = if persist {
        let next = if incoming.persist {
            None
        } else {
            // let count = self.metered_produce_count(source, reader.meter)?;
            let count = self.metered_produce_count(&incoming.source, reader.meter)?;
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                reader.meter.reserve(1, message.len(), message.len())?;
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for (group, (channels, group_keys)) in
            grouped_channels.into_iter().zip(&keys.groups).enumerate()
        {
            let continuations =
                sorted((reader.continuations)(&channels, group_keys)?, reader.meter)?;
            // Changed by C2 (DR-82): the channel data borrow copy-free views.
            let views = self.metered_data_views(&channels, &group_keys.channels, reader)?;
            // Changed by D-D4 (DR-106): the match reads the recorded positions.
            // let data = self.metered_channel_data(
            let (data, positions) = self.metered_channel_data(
                &channels,
                &views,
                // Changed by D-D5 (DR-107): the entries borrow the datum.
                // Some((channel, value, persist, source)),
                Some((channel, &incoming)),
                expected,
                reader,
            )?;
            for (continuation, index) in continuations {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect(&continuation.source, reader.meter)?;
                native_backing::inspect_blocks(&continuation.source, reader.meter)?;
                if let Some(identity) = expected {
                    if !identity.metered_matches_consume(&continuation.source, reader.meter)? {
                        continue;
                    }
                }
                let Some(data_candidates) = self.metered_match_data(
                    &channels,
                    &positions,
                    &continuation.patterns,
                    &continuation.continuation,
                    &data,
                    reader.meter,
                )?
                else {
                    continue;
                };
                let comm = self.metered_comm(
                    &data_candidates,
                    &continuation.source,
                    &continuation.peeks,
                    // Changed by D-D5 (DR-107): the source of the datum.
                    // next.map(|next| (source, next)),
                    next.map(|next| (&incoming.source, next)),
                    reader.meter,
                )?;
                if let Some(identity) = expected {
                    if !identity.metered_matches_comm(&comm, reader.meter)? {
                        // Changed by D-D5 (DR-107): the datum goes back.
                        // return Ok(None);
                        return Ok(ProduceSelection::Unmatched(incoming));
                    }
                }
                // C1 (DR-81): only the selected continuation is copied.
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(
                //     continuation.as_ref(),
                //     reader.meter,
                // )?;
                native_backing::reserve_blocks_copy_and_cleanup(
                    continuation.as_ref(),
                    reader.meter,
                )?;
                let continuation = continuation.as_ref().clone();
                // Changed by D-D5 (DR-107): the incoming candidates receive
                // the produce's value, and the source goes back.
                // let candidate = ProduceCandidate {
                //     channels,
                //     continuation,
                //     continuation_index: index,
                //     data_candidates,
                // };
                // return Ok(Some((PreparedProduceCandidate { candidate, comm }, group)));
                let mut data_candidates = data_candidates;
                let Datum {
                    a: value, source, ..
                } = incoming;
                fill_incoming(&mut data_candidates, value, reader.meter)?;
                let candidate = ProduceCandidate {
                    channels,
                    continuation,
                    continuation_index: index,
                    data_candidates,
                };
                return Ok(ProduceSelection::Matched {
                    prepared: PreparedProduceCandidate { candidate, comm },
                    group,
                    source,
                });
            }
        }
        // Changed by D-D5 (DR-107): the datum goes back.
        // Ok(None)
        Ok(ProduceSelection::Unmatched(incoming))
    }
}

#[cfg(test)]
mod tests;
