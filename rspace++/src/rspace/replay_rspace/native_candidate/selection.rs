use std::borrow::Cow;
use std::convert::Infallible;

use super::*;
use crate::rspace::hashing::native_source::{GroupKeys, OperationKeys, StoreKey};
use crate::rspace::hot_store::NativeDataView;
// Added by G3 (DR-135, S6): the owner checks of the replay matcher.
use crate::rspace::replay_rspace::native_epoch::NativeOwnerFilter;

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

pub(in crate::rspace::replay_rspace) struct CandidateReader<
    'a,
    C,
    P: Clone + PartialEq,
    A: Clone,
    K: Clone + PartialEq,
> {
    pub(in crate::rspace::replay_rspace) data: DataReader<'a, C, A>,
    pub(in crate::rspace::replay_rspace) continuations: ContinuationReader<'a, C, P, K>,
}

/// D-D5 (DR-107): the datum index of the incoming datum of a produce
/// (`native_candidate.rs`). The retirement skips it, because it retires only
/// stored data (`native_session/result.rs`).
const INCOMING_INDEX: i32 = -1;

/// D-D5 (DR-107): the selection of a produce owns the incoming datum. A
/// produce that matches nothing gets its datum back to store it, and a
/// matched produce gets its source back for the counter and the trace.
pub(in crate::rspace::replay_rspace) enum ProduceSelection<
    C,
    P: Clone + PartialEq,
    A: Clone,
    K: Clone + PartialEq,
> {
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
/// receives a copy.
fn fill_incoming<C, A: Clone>(candidates: &mut [ConsumeCandidate<C, A>], value: A) {
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
        candidate.removed_datum = value.clone();
    }
    if let Some(first) = first {
        first.removed_datum = value;
    }
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

fn channel_position<C: Eq, A: Clone>(
    channels: &[ChannelData<'_, C, A>],
    channel: &C,
) -> Option<usize> {
    for (position, existing) in channels.iter().enumerate() {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(channel, meter)?;
        // Changed by D-D4 (DR-106): the entry borrows its channel.
        // native_backing::inspect(&existing.channel, meter)?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(existing.channel, meter)?;
        // if channel == &existing.channel {
        if channel == existing.channel {
            return Some(position);
        }
    }
    None
}

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(in crate::rspace::replay_rspace) fn prepare_produce_counter(
        &'_ self,
        source: &Produce,
        persist: bool,
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
        let next = counters
            .get(source)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                RSpaceError::InterpreterError("Native replay produce counter overflow".to_owned())
            })?;
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(source, meter)?;
        // Disabled by C13 (DR-77): the produce-counter map lives for the whole
        // replay, so charging its whole backing on every produce charged a
        // quadratic total.
        // let (operations, bytes) = tree_backing::<Produce, i32>(entries)
        //     .ok_or(RSpaceError::HostWorkRejected)?;
        Ok(PreparedProduceCounter {
            counters: Some(counters),
            entry: Some((source.clone(), next)),
        })
    }

    fn matches_identity(
        &self,
        datum: &Datum<A>,
        expected: Option<&dyn NativeCandidateIdentity>,
        incoming: Option<&Produce>,
    ) -> bool {
        let Some(expected) = expected else {
            return true;
        };
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(&datum.source, meter)?;
        if !expected.matches_produce(&datum.source) {
            return false;
        }
        if datum.persist {
            return true;
        }
        let count = self.get_produce_count(&datum.source);
        let increment = if let Some(incoming) = incoming {
            incoming == &datum.source
        } else {
            false
        };
        let count = if increment {
            count.checked_add(1)
        } else {
            Some(count)
        };
        match count {
            Some(count) => expected.repetition(&datum.source) == Some(count),
            None => false,
        }
    }

    /// One copy-free view of the cached data of each channel (C2, DR-82),
    /// read by the channel's key (D-C2c, DR-96).
    fn data_views(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Vec<NativeDataView<C, A>>> {
        if keys.len() != channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay channel keys do not match the channels".to_owned(),
            ));
        }
        let mut views = Vec::with_capacity(channels.len());
        for (channel, key) in channels.iter().zip(keys) {
            views.push((reader.data)(channel, *key)?);
        }
        Ok(views)
    }

    /// One entry for each distinct channel, and for each channel of
    /// `channels` the position of its entry (D-D4, DR-106). The entries
    /// borrow the channels, and the incoming datum of a produce (D-D5,
    /// DR-107).
    fn channel_data<'v>(
        &self,
        channels: &'v [C],
        views: &'v [NativeDataView<C, A>],
        // Changed by D-D5 (DR-107): the entries borrow the incoming datum.
        // incoming: Option<(&C, &A, bool, &Produce)>,
        incoming: Option<(&C, &'v Datum<A>)>,
        expected: Option<&dyn NativeCandidateIdentity>,
    ) -> (Vec<ChannelData<'v, C, A>>, Vec<usize>) {
        let mut result = Vec::with_capacity(channels.len());
        let mut positions = Vec::with_capacity(channels.len());
        for (channel, view) in channels.iter().zip(views) {
            // Changed by C2 (DR-82): the candidates borrow the cached data.
            // let mut values = sorted((reader.data)(channel)?, reader.meter)?;
            let mut borrowed = Vec::with_capacity(view.values().len());
            for datum in view.values() {
                borrowed.push(Cow::Borrowed(datum));
            }
            let mut values = canonical_candidates(borrowed);
            // Changed by D-D5 (DR-107): the incoming datum is borrowed.
            // if let Some((trigger, data, persist, source)) = incoming {
            if let Some((trigger, datum)) = incoming {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect(channel, reader.meter)?;
                // native_backing::inspect(trigger, reader.meter)?;
                if channel == trigger {
                    let mut all = Vec::with_capacity(values.len() + 1);
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
                if self.matches_identity(&values[position].0, expected, source) {
                    position += 1;
                } else {
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
            match channel_position(&result, channel) {
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
        (result, positions)
    }

    /// Selects one datum for each pattern. `positions[i]` is the position in
    /// `data` of the entry of `channels[i]`, as `channel_data`
    /// recorded it (D-D4, DR-106).
    ///
    /// Changed by G3 (DR-135, S6): with `owner`, the owner checks and the
    /// persistence of `continuation`, a datum whose item test fails is a
    /// miss, as in play (R5).
    fn match_data(
        &self,
        channels: &[C],
        positions: &[usize],
        patterns: &[P],
        continuation: &K,
        data: &[ChannelData<'_, C, A>],
        owner: Option<(&dyn NativeOwnerFilter<C, P, A, K>, bool)>,
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>> {
        if positions.len() != channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay channel positions do not match the channels".to_owned(),
            ));
        }
        let count = channels.len().min(patterns.len());
        let mut chosen: Vec<(usize, usize)> = Vec::with_capacity(count);
        let mut candidates = Vec::with_capacity(count);
        let mut complete = true;
        // Changed by D-D4 (DR-106): the position of each channel's entry is
        // read, one word for each pattern, instead of searched.
        // for (channel, pattern) in channels.iter().zip(patterns) {
        //     let Some(channel_index) = channel_position(data, channel, meter)? else {
        //         complete = false;
        //         continue;
        //     };
        for ((channel, pattern), recorded) in channels.iter().zip(patterns).zip(positions) {
            let channel_index = *recorded;
            let entry = data.get(channel_index).ok_or_else(|| {
                RSpaceError::BugFoundError(
                    "a recorded channel position is outside the channel data".to_owned(),
                )
            })?;
            let mut found = false;
            // for (position, (datum, index)) in
            //     data[channel_index].values.iter().enumerate()
            // {
            for (position, (datum, index)) in entry.values.iter().enumerate() {
                if !datum.persist && chosen.contains(&(channel_index, position)) {
                    continue;
                }
                // Disabled by D-M1 (DR-88): get_metered reserves every read of
                // the pattern and the datum; this walk read nothing.
                // native_backing::inspect(pattern, meter)?;
                // native_backing::inspect(&datum.a, meter)?;
                let Some(matched) = self.matcher.get(pattern, &datum.a) else {
                    continue;
                };
                // Added by G3 (DR-135, S6): a datum whose owners cannot pay
                // its share of a COMM with this continuation is a miss (R5).
                if let Some((filter, persistent)) = owner {
                    if !filter.admit_item(
                        continuation,
                        persistent,
                        channels.len(),
                        &matched,
                        datum.persist,
                    )? {
                        continue;
                    }
                }
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(channel, meter)?;
                // native_backing::reserve_copy_and_cleanup(&datum.source, meter)?;
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
        let mut matched: Vec<&A> = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            matched.push(&candidate.datum.a);
        }
        // Disabled by D-M1 (DR-88): check_commit_metered reserves every read
        // of the continuation (its guard); this walk read nothing.
        // native_backing::inspect(continuation, meter)?;
        if !self
            .matcher
            .check_commit_by_reference(continuation, &matched)
        {
            return Ok(None);
        }
        Ok(Some(candidates))
    }

    fn build_comm(
        &self,
        data: &[ConsumeCandidate<C, A>],
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        incoming: Option<(&Produce, i32)>,
    ) -> COMM {
        let mut indexed = Vec::with_capacity(data.len());
        for (index, candidate) in data.iter().enumerate() {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(&candidate.datum.source, meter)?;
            indexed.push((candidate.datum.source.clone(), index));
        }
        // Changed by DR-131 S6: the heap sort of `fallible_sort` (D-F) with a
        // comparator that cannot fail. The index makes the order total.
        let Ok(()) = shared::rust::fallible_sort::sort(&mut indexed, |a, b| {
            Ok::<_, Infallible>(
                a.0.channel_hash
                    .cmp(&b.0.channel_hash)
                    .then_with(|| a.0.hash.cmp(&b.0.hash))
                    .then_with(|| a.0.persistent.cmp(&b.0.persistent))
                    .then_with(|| a.1.cmp(&b.1)),
            )
        });
        let mut produces = Vec::with_capacity(indexed.len());
        produces.extend(indexed.into_iter().map(|(source, _)| source));
        let mut times_repeated: BTreeMap<Produce, i32> = BTreeMap::new();
        for source in produces.iter().rev() {
            let count = self.get_produce_count(source);
            let count = if let Some((incoming, next)) = incoming {
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
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block.
            // native_backing::reserve_copy_and_cleanup(source, meter)?;
            times_repeated.insert(source.clone(), count);
        }
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::reserve_copy_and_cleanup(consume, meter)?;
        // native_backing::reserve_copy_and_cleanup(peeks, meter)?;
        COMM {
            consume: consume.clone(),
            produces,
            peeks: peeks.clone(),
            times_repeated,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::rspace::replay_rspace) fn prepare_consume_candidate(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        expected: Option<&dyn NativeCandidateIdentity>,
        // Added by G3 (DR-135, S6): the owner checks of the operation and the
        // persistence of the consume (R5, R7).
        owner: Option<(&dyn NativeOwnerFilter<C, P, A, K>, bool)>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(consume, reader.meter)?;
        if let Some(identity) = expected {
            if !identity.matches_consume(consume) {
                return Ok(None);
            }
        }
        // Added by DR-129 (E3-a): with no recorded identity this call is the
        // Stored check (`native_session/operations.rs`). It skips a
        // continuation whose commit can veto, because play, in dev's random
        // order, may have tried another tuple and stored.
        if expected.is_none() && self.matcher.commit_may_veto(continuation) {
            return Ok(None);
        }
        // Added by G3 (DR-135, S6): the Stored check also skips a consume
        // whose COMM the row refused. The refusal is verified on its own
        // (R7, SA4).
        if expected.is_none() && owner.is_some_and(|(filter, _)| filter.names_refusal_for(consume))
        {
            return Ok(None);
        }
        // Changed by C2 (DR-82): the channel data borrow copy-free views.
        // let data = self.metered_channel_data(channels, None, expected, reader)?;
        let views = self.data_views(channels, keys, reader)?;
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
        let (data, positions) = self.channel_data(channels, &views, None, expected);
        // Changed by G3 (DR-135, S6): with the item test of the owner checks.
        // let Some(data) =
        //     self.match_data(channels, &positions, patterns, continuation, &data)?
        // else {
        //     return Ok(None);
        // };
        let Some(data) =
            self.match_data(channels, &positions, patterns, continuation, &data, owner)?
        else {
            return Ok(None);
        };
        let comm = self.build_comm(&data, consume, peeks, None);
        if let Some(identity) = expected {
            if !identity.matches_comm(&comm) {
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
    pub(in crate::rspace::replay_rspace) fn prepare_produce_candidate(
        &self,
        channel: &C,
        incoming: Datum<A>,
        grouped_channels: Vec<Vec<C>>,
        keys: &OperationKeys,
        expected: Option<&dyn NativeCandidateIdentity>,
        // Added by G3 (DR-135, S6): the owner checks of the operation (R5,
        // R6, R7).
        owner: Option<&dyn NativeOwnerFilter<C, P, A, K>>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<ProduceSelection<C, P, A, K>> {
        // Added by G3 (DR-135, S6): a produce keeps its own datum under owner
        // funding (R6).
        let own_datum = owner.is_some_and(|filter| filter.requires_own_datum());
        if keys.groups.len() != grouped_channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay join keys do not match the joins".to_owned(),
            ));
        }
        // Changed by D-D5 (DR-107): the fields of the incoming datum.
        // let next = if persist {
        let next = if incoming.persist {
            None
        } else {
            // let count = self.metered_produce_count(source, reader.meter)?;
            let count = self.get_produce_count(&incoming.source);
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for (group, (channels, group_keys)) in
            grouped_channels.into_iter().zip(&keys.groups).enumerate()
        {
            let continuations =
                canonical_candidates((reader.continuations)(&channels, group_keys)?);
            // Changed by C2 (DR-82): the channel data borrow copy-free views.
            let views = self.data_views(&channels, &group_keys.channels, reader)?;
            // Changed by D-D4 (DR-106): the match reads the recorded positions.
            // let data = self.metered_channel_data(
            let (data, positions) = self.channel_data(
                &channels,
                &views,
                // Changed by D-D5 (DR-107): the entries borrow the datum.
                // Some((channel, value, persist, source)),
                Some((channel, &incoming)),
                expected,
            );
            // Added by G3 (DR-135, S6): on a channel that the join names
            // once, the patterns read only the incoming datum (R6).
            let mut data = data;
            if own_datum {
                keep_only_incoming(&mut data, &channels, &positions, channel);
            }
            for (continuation, index) in continuations {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect(&continuation.source, reader.meter)?;
                if let Some(identity) = expected {
                    if !identity.matches_consume(&continuation.source) {
                        continue;
                    }
                }
                // Added by DR-129 (E3-a): the Stored check (no recorded
                // identity) skips a continuation whose commit can veto.
                if expected.is_none() && self.matcher.commit_may_veto(&continuation.continuation) {
                    continue;
                }
                // Added by G3 (DR-135, S6): the Stored check also skips a
                // continuation whose COMM the row refused. The refusal is
                // verified on its own (R7, SA4).
                if expected.is_none() &&
                    owner.is_some_and(|filter| filter.names_refusal_for(&continuation.source))
                {
                    continue;
                }
                let Some(data_candidates) = self.match_data(
                    &channels,
                    &positions,
                    &continuation.patterns,
                    &continuation.continuation,
                    &data,
                    // Added by G3 (DR-135, S6): the item test of the owner
                    // checks (R5).
                    owner.map(|filter| (filter, continuation.persist)),
                )?
                else {
                    continue;
                };
                // Added by G3 (DR-135, S6): on a repeated channel the tuple
                // must hold the incoming datum (R6).
                if own_datum && !holds_incoming(&data_candidates) {
                    continue;
                }
                let comm = self.build_comm(
                    &data_candidates,
                    &continuation.source,
                    &continuation.peeks,
                    // Changed by D-D5 (DR-107): the source of the datum.
                    // next.map(|next| (source, next)),
                    next.map(|next| (&incoming.source, next)),
                );
                if let Some(identity) = expected {
                    if !identity.matches_comm(&comm) {
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
                fill_incoming(&mut data_candidates, value);
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

    /// Added by G3 (DR-135, S6): the tuple of a consume's refused COMM
    /// (R7): the data that the refusal source `identity` names, as the
    /// Matched branch finds its data. No item test runs here, because the
    /// refusal check tests each item strictly (SA4).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::rspace::replay_rspace) fn find_refused_consume_tuple(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        identity: &dyn NativeCandidateIdentity,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        self.prepare_consume_candidate(
            channels,
            keys,
            patterns,
            continuation,
            consume,
            peeks,
            Some(identity),
            None,
            reader,
        )
    }

    /// Added by G3 (DR-135, S6): the tuple of a produce's refused COMM (R7):
    /// the continuation that the refusal source `identity` names, with the
    /// data that the source names. With `own_datum` the tuple keeps the
    /// produce's own datum, as in play (R6). The search borrows the incoming
    /// datum, so its candidate keeps the default removed value, and the
    /// selection of the produce can take the datum afterwards. No item test
    /// runs here, because the refusal check tests each item strictly (SA4).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::rspace::replay_rspace) fn find_refused_produce_tuple(
        &self,
        channel: &C,
        incoming: &Datum<A>,
        grouped_channels: &[Vec<C>],
        keys: &OperationKeys,
        identity: &dyn NativeCandidateIdentity,
        own_datum: bool,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<RefusedProduceTuple<C, P, A, K>>> {
        if keys.groups.len() != grouped_channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay join keys do not match the joins".to_owned(),
            ));
        }
        let next = if incoming.persist {
            None
        } else {
            let count = self.get_produce_count(&incoming.source);
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for (channels, group_keys) in grouped_channels.iter().zip(&keys.groups) {
            let continuations = canonical_candidates((reader.continuations)(channels, group_keys)?);
            let views = self.data_views(channels, &group_keys.channels, reader)?;
            let (mut data, positions) =
                self.channel_data(channels, &views, Some((channel, incoming)), Some(identity));
            if own_datum {
                keep_only_incoming(&mut data, channels, &positions, channel);
            }
            for (continuation, _) in continuations {
                if !identity.matches_consume(&continuation.source) {
                    continue;
                }
                let Some(data_candidates) = self.match_data(
                    channels,
                    &positions,
                    &continuation.patterns,
                    &continuation.continuation,
                    &data,
                    None,
                )?
                else {
                    continue;
                };
                if own_datum && !holds_incoming(&data_candidates) {
                    continue;
                }
                let comm = self.build_comm(
                    &data_candidates,
                    &continuation.source,
                    &continuation.peeks,
                    next.map(|next| (&incoming.source, next)),
                );
                if !identity.matches_comm(&comm) {
                    return Ok(None);
                }
                return Ok(Some(RefusedProduceTuple {
                    continuation: continuation.as_ref().clone(),
                    data: data_candidates,
                    comm,
                }));
            }
        }
        Ok(None)
    }
}

/// Added by G3 (DR-135, S6): the tuple of a refused COMM of a produce (R7):
/// its waiting continuation, its data and the COMM.
pub(in crate::rspace::replay_rspace) struct RefusedProduceTuple<
    C,
    P: Clone + PartialEq,
    A: Clone,
    K: Clone + PartialEq,
> {
    pub(in crate::rspace::replay_rspace) continuation: WaitingContinuation<P, K>,
    pub(in crate::rspace::replay_rspace) data: Vec<ConsumeCandidate<C, A>>,
    pub(in crate::rspace::replay_rspace) comm: COMM,
}

/// Added by G3 (DR-135, S6): the restriction of R6. On a channel that
/// `channels` names once, the entry of the produce's channel `trigger` keeps
/// only the incoming datum. A channel that `channels` names more than once
/// keeps every datum, and the tuple must hold the incoming datum instead
/// (`holds_incoming`). Play restricts its lists in the same way
/// (`space_matcher.rs`, `extract_first_match`).
fn keep_only_incoming<C: Eq, A: Clone>(
    data: &mut [ChannelData<'_, C, A>],
    channels: &[C],
    positions: &[usize],
    trigger: &C,
) {
    let mut named = channels
        .iter()
        .zip(positions)
        .filter(|(channel, _)| *channel == trigger);
    if let (Some((_, position)), None) = (named.next(), named.next()) {
        if let Some(entry) = data.get_mut(*position) {
            entry.values.retain(|(_, index)| *index == INCOMING_INDEX);
        }
    }
}

/// Added by G3 (DR-135, S6): whether a tuple holds the incoming datum of its
/// produce (R6).
fn holds_incoming<C, A: Clone>(candidates: &[ConsumeCandidate<C, A>]) -> bool {
    candidates
        .iter()
        .any(|candidate| candidate.datum_index == INCOMING_INDEX)
}

#[cfg(test)]
mod tests;
