use std::cmp::Ordering;
use std::mem::size_of;

use shared::rust::clone_backing::CloneBacking;
use shared::rust::collection_backing::tree_backing;

use super::*;
use crate::rspace::candidate_order::{CandidateSource, OrderWork, canonical_order};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{self, SourceMeter};
use crate::rspace::native_backing;

type Result<T> = std::result::Result<T, RSpaceError>;
type DataReader<'a, C, A> = &'a dyn Fn(&C) -> Result<Vec<Datum<A>>>;
type ContinuationReader<'a, C, P, K> = &'a dyn Fn(&[C]) -> Result<Vec<WaitingContinuation<P, K>>>;

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
        native_backing::inspect(value, self.0)?;
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

struct ChannelData<C, A: Clone> {
    channel: C,
    values: Vec<(Datum<A>, i32)>,
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
    channels: &[ChannelData<C, A>],
    channel: &C,
    meter: &dyn SourceMeter,
) -> Result<Option<usize>> {
    for (position, existing) in channels.iter().enumerate() {
        native_backing::inspect(channel, meter)?;
        native_backing::inspect(&existing.channel, meter)?;
        meter.reserve(1, 0, 0)?;
        if channel == &existing.channel {
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
        let entries = counters
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let comparisons = entries
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            comparisons
                .checked_mul(32)
                .ok_or(RSpaceError::HostWorkRejected)?,
            source
                .hash
                .0
                .len()
                .checked_mul(comparisons)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        for existing in counters.keys() {
            meter.reserve(
                2,
                existing
                    .hash
                    .0
                    .len()
                    .checked_mul(2)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
        }
        let next = counters
            .get(source)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        native_backing::reserve_copy_and_cleanup(source, meter)?;
        let (operations, bytes) =
            tree_backing::<Produce, i32>(entries).ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(operations, bytes, bytes)?;
        Ok(PreparedProduceCounter {
            counters: Some(counters),
            entry: Some((source.clone(), next)),
        })
    }

    fn metered_produce_count(&self, source: &Produce, meter: &dyn SourceMeter) -> Result<i32> {
        let counters = self.produce_counter.lock().expect("produce counter lock");
        let steps = counters
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
            source
                .hash
                .0
                .len()
                .checked_mul(steps)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        for key in counters.keys() {
            meter.reserve(1, key.hash.0.len(), 0)?;
        }
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
        native_backing::inspect(&datum.source, meter)?;
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

    fn metered_channel_data(
        &self,
        channels: &[C],
        incoming: Option<(&C, &A, bool, &Produce)>,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Vec<ChannelData<C, A>>> {
        let mut result = buffer(channels.len(), reader.meter)?;
        for channel in channels {
            let mut values = sorted((reader.data)(channel)?, reader.meter)?;
            if let Some((trigger, data, persist, source)) = incoming {
                native_backing::inspect(channel, reader.meter)?;
                native_backing::inspect(trigger, reader.meter)?;
                if channel == trigger {
                    let count = values
                        .len()
                        .checked_add(1)
                        .ok_or(RSpaceError::HostWorkRejected)?;
                    let mut all = buffer(count, reader.meter)?;
                    native_backing::reserve_copy_and_cleanup(data, reader.meter)?;
                    native_backing::reserve_copy_and_cleanup(source, reader.meter)?;
                    all.push((
                        Datum {
                            a: data.clone(),
                            persist,
                            source: source.clone(),
                        },
                        -1,
                    ));
                    all.extend(values);
                    values = all;
                }
            }
            let mut position = 0;
            while position < values.len() {
                let source =
                    incoming.and_then(|(_, _, persist, source)| (!persist).then_some(source));
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
                            .checked_mul(size_of::<(Datum<A>, i32)>())
                            .ok_or(RSpaceError::HostWorkRejected)?,
                        0,
                    )?;
                    values.remove(position);
                }
            }
            if let Some(position) = channel_position(&result, channel, reader.meter)? {
                result[position].values = values;
            } else {
                native_backing::reserve_copy_and_cleanup(channel, reader.meter)?;
                result.push(ChannelData {
                    channel: channel.clone(),
                    values,
                });
            }
        }
        Ok(result)
    }

    fn metered_match_data(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        data: &[ChannelData<C, A>],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>> {
        let count = channels.len().min(patterns.len());
        let mut chosen = buffer::<(usize, usize)>(count, meter)?;
        let mut candidates = buffer(count, meter)?;
        let mut complete = true;
        for (channel, pattern) in channels.iter().zip(patterns) {
            let Some(channel_index) = channel_position(data, channel, meter)? else {
                complete = false;
                continue;
            };
            let mut found = false;
            for (position, (datum, index)) in data[channel_index].values.iter().enumerate() {
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
                native_backing::inspect(pattern, meter)?;
                native_backing::inspect(&datum.a, meter)?;
                let Some(matched) = self.matcher.get_metered(pattern, &datum.a, meter)? else {
                    continue;
                };
                native_backing::reserve_copy_and_cleanup(channel, meter)?;
                native_backing::reserve_copy_and_cleanup(&datum.source, meter)?;
                native_backing::reserve_copy_and_cleanup(&datum.a, meter)?;
                candidates.push(ConsumeCandidate {
                    channel: channel.clone(),
                    datum: Datum {
                        a: matched,
                        persist: datum.persist,
                        source: datum.source.clone(),
                    },
                    removed_datum: datum.a.clone(),
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
        let mut matched = buffer(candidates.len(), meter)?;
        for candidate in &candidates {
            native_backing::reserve_copy_and_cleanup(&candidate.datum.a, meter)?;
            matched.push(candidate.datum.a.clone());
        }
        native_backing::inspect(continuation, meter)?;
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
            native_backing::reserve_copy_and_cleanup(&candidate.datum.source, meter)?;
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
            let steps = times_repeated
                .len()
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(
                steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
                source
                    .hash
                    .0
                    .len()
                    .checked_mul(steps)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            for key in times_repeated.keys() {
                meter.reserve(1, key.hash.0.len(), 0)?;
            }
            if times_repeated.contains_key(source) {
                continue;
            }
            meter.reserve(
                steps.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
                source
                    .hash
                    .0
                    .len()
                    .checked_mul(steps)
                    .ok_or(RSpaceError::HostWorkRejected)?,
                0,
            )?;
            for key in times_repeated.keys() {
                meter.reserve(1, key.hash.0.len(), 0)?;
            }
            native_backing::reserve_copy_and_cleanup(source, meter)?;
            times_repeated.insert(source.clone(), count);
        }
        native_backing::reserve_copy_and_cleanup(consume, meter)?;
        native_backing::reserve_copy_and_cleanup(peeks, meter)?;
        Ok(COMM {
            consume: consume.clone(),
            produces,
            peeks: peeks.clone(),
            times_repeated,
        })
    }

    pub(in crate::rspace::replay_rspace) fn prepare_metered_consume_candidate(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        native_backing::inspect(consume, reader.meter)?;
        if let Some(identity) = expected {
            if !identity.metered_matches_consume(consume, reader.meter)? {
                return Ok(None);
            }
        }
        let data = self.metered_channel_data(channels, None, expected, reader)?;
        let Some(data) =
            self.metered_match_data(channels, patterns, continuation, &data, reader.meter)?
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

    pub(in crate::rspace::replay_rspace) fn prepare_metered_produce_candidate(
        &self,
        channel: &C,
        value: &A,
        persist: bool,
        source: &Produce,
        grouped_channels: Vec<Vec<C>>,
        expected: Option<&dyn NativeCandidateIdentity>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedProduceCandidate<C, P, A, K>>> {
        let next = if persist {
            None
        } else {
            let count = self.metered_produce_count(source, reader.meter)?;
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                reader.meter.reserve(1, message.len(), message.len())?;
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for channels in grouped_channels {
            let continuations = sorted((reader.continuations)(&channels)?, reader.meter)?;
            let data = self.metered_channel_data(
                &channels,
                Some((channel, value, persist, source)),
                expected,
                reader,
            )?;
            for (continuation, index) in continuations {
                native_backing::inspect(&continuation.source, reader.meter)?;
                if let Some(identity) = expected {
                    if !identity.metered_matches_consume(&continuation.source, reader.meter)? {
                        continue;
                    }
                }
                let Some(data_candidates) = self.metered_match_data(
                    &channels,
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
                    next.map(|next| (source, next)),
                    reader.meter,
                )?;
                if let Some(identity) = expected {
                    if !identity.metered_matches_comm(&comm, reader.meter)? {
                        return Ok(None);
                    }
                }
                let candidate = ProduceCandidate {
                    channels,
                    continuation,
                    continuation_index: index,
                    data_candidates,
                };
                return Ok(Some(PreparedProduceCandidate { candidate, comm }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
