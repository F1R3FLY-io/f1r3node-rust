use std::fmt::Debug;
use std::hash::Hash;
use std::mem::size_of;
use std::sync::Arc;

use serde::Serialize;
use shared::rust::clone_backing::CloneBacking;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::hashing::stable_hash_provider::{EventCodecEpoch, StableHashSerialize};
use crate::rspace::hot_store::RestingRetirementAuthorization;
use crate::rspace::internal::{Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceOperationCompletion, RSpaceOperationSource, RSpaceResult,
};
use crate::rspace::trace::event::{Event, RestingCOMM};
use crate::rspace::{native_backing, striped_locks};

fn metered_vec<T>(count: usize, meter: &dyn SourceMeter) -> Result<Vec<T>, RSpaceError> {
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, 0, bytes)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

/// A prepared resting commit may authorize one staged retirement only.
struct AuthorizeRestingOnce<F>(Option<F>);

impl<F> RestingRetirementAuthorization for AuthorizeRestingOnce<F>
where F: FnOnce() -> Result<(), RSpaceError>
{
    fn authorize(&mut self) -> Result<(), RSpaceError> {
        self.0
            .take()
            .ok_or(RSpaceError::ProduceCommitProtocolViolation)?()
    }
}

pub(super) struct RestingHotRead<P: Clone, A: Clone, K: Clone> {
    pub(super) waiters: Vec<Arc<WaitingContinuation<P, K>>>,
    pub(super) data: Vec<Vec<Datum<A>>>,
    pub(super) installed: bool,
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + Hash
        + Ord
        + Eq
        + Send
        + Sync
        + 'static,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + Send + Sync + 'static,
    A: Clone + Debug + Default + Serialize + StableHashSerialize + Send + Sync + 'static,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + Send + Sync + 'static,
{
    /// Commit a previously enumerated, now revalidated resting rendezvous.
    /// Legacy/CostV1 and cold-missing inputs refuse before any store change.
    /// An anonymous candidate is never treated as a prevalidated capability.
    pub async fn commit_enabled_rendezvous(
        &self,
        candidate: &ProduceCandidate<C, P, A, K>,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        A: CloneBacking,
        K: CloneBacking,
    {
        if self.epoch != EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let _checkpoint_guard = self.resting_checkpoint_barrier.read().await;
        let count = candidate.channels.len();
        if count == 0 || candidate.data_candidates.len() != count {
            return Ok(None);
        }
        let mut channel_hashes = metered_vec::<u64>(count, meter)?;
        for channel in &candidate.channels {
            native_backing::inspect_blocks(channel, meter)?;
            meter.reserve(1, size_of::<u64>(), 0)?;
            channel_hashes.push(striped_locks::channel_hash(channel));
        }
        let (_phase_a, _phase_b) = self.consume_lock(&channel_hashes).await;
        let read = self.read_resting_hot(candidate, meter)?;
        let Some(waiter_index) = usize::try_from(candidate.continuation_index).ok() else {
            return Ok(None);
        };
        let Some(waiter) = read.waiters.get(waiter_index) else {
            return Ok(None);
        };
        if waiter.source != candidate.continuation.source ||
            waiter.patterns.len() != count ||
            (read.installed && !waiter.persist)
        {
            if read.installed && !waiter.persist {
                return Err(RSpaceError::ProduceCommitDenied);
            }
            return Ok(None);
        }
        let forward = |operations, scanned, backing| meter.reserve(operations, scanned, backing);
        let authenticated_waiter = native_source::consume_for_epoch(
            self.epoch,
            &candidate.channels,
            waiter.patterns.as_ref(),
            waiter.continuation.as_ref(),
            waiter.persist,
            &forward,
        )?;
        if authenticated_waiter != waiter.source {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }

        let mut rows = metered_vec::<RSpaceResult<C, A>>(count, meter)?;
        let mut ordered_produces = metered_vec(count, meter)?;
        let mut ordered_data_indices = metered_vec(count, meter)?;
        let mut retirement = metered_vec::<(usize, i32)>(count, meter)?;
        for (position, selection) in candidate.data_candidates.iter().enumerate() {
            let channel = &candidate.channels[position];
            native_backing::inspect_blocks(&selection.channel, meter)?;
            native_backing::inspect_blocks(channel, meter)?;
            if selection.channel != *channel {
                return Ok(None);
            }
            let Some(index) = usize::try_from(selection.datum_index).ok() else {
                return Ok(None);
            };
            let Some(datum) = read.data[position].get(index) else {
                return Ok(None);
            };
            native_backing::inspect_blocks(&datum.source, meter)?;
            native_backing::inspect_blocks(&selection.datum.source, meter)?;
            meter.reserve(1, 0, 0)?;
            if datum.source.hash != selection.datum.source.hash ||
                datum.source.channel_hash != selection.datum.source.channel_hash ||
                datum.source.persistent != selection.datum.source.persistent ||
                datum.persist != selection.datum.persist
            {
                return Ok(None);
            }
            let authenticated_datum = native_source::produce_for_epoch(
                self.epoch,
                channel,
                datum.a.as_ref(),
                datum.persist,
                &forward,
            )?;
            if authenticated_datum.hash != datum.source.hash ||
                authenticated_datum.channel_hash != datum.source.channel_hash ||
                authenticated_datum.persistent != datum.source.persistent
            {
                return Err(RSpaceError::ProduceCommitProtocolViolation);
            }
            // A linear datum can occupy only one bind, even when a channel
            // appears more than once. Persistent data may be reused.
            if !datum.persist {
                for previous in 0..position {
                    meter.reserve(1, size_of::<i32>() * 2, 0)?;
                    native_backing::inspect_blocks(&candidate.channels[previous], meter)?;
                    native_backing::inspect_blocks(channel, meter)?;
                    if candidate.channels[previous] == *channel &&
                        candidate.data_candidates[previous].datum_index == selection.datum_index
                    {
                        return Ok(None);
                    }
                }
            }
            let Some(matched) =
                self.matcher
                    .get_metered(&waiter.patterns[position], datum.a.as_ref(), meter)?
            else {
                return Ok(None);
            };
            native_backing::reserve_blocks_copy_and_cleanup(channel, meter)?;
            native_backing::reserve_blocks_copy_and_cleanup(datum.a.as_ref(), meter)?;
            rows.push(RSpaceResult {
                channel: channel.clone(),
                matched_datum: matched,
                removed_datum: datum.a.as_ref().clone(),
                persistent: datum.persist,
            });
            native_backing::reserve_blocks_copy_and_cleanup(&datum.source, meter)?;
            ordered_produces.push(datum.source.clone());
            ordered_data_indices.push(selection.datum_index);
            if !datum.persist {
                retirement.push((position, selection.datum_index));
            }
        }
        // Native retirement applies each removal to a staged copy, so repeated
        // channels must be retired at descending store indices.
        let comparisons = retirement
            .len()
            .checked_mul(retirement.len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            comparisons,
            comparisons
                .checked_mul(size_of::<(usize, i32)>())
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
        )?;
        retirement
            .sort_unstable_by(|(_, left_index), (_, right_index)| right_index.cmp(left_index));

        let mut matched_refs = metered_vec::<&A>(count, meter)?;
        let mut observed_data = metered_vec::<(&A, bool)>(count, meter)?;
        for (position, row) in rows.iter().enumerate() {
            matched_refs.push(&row.matched_datum);
            let datum = &read.data[position][ordered_data_indices[position] as usize];
            observed_data.push((datum.a.as_ref(), datum.persist));
        }
        let Some(prepared) = self.matcher.prepare_commit_metered(
            waiter.continuation.as_ref(),
            &matched_refs,
            meter,
        )?
        else {
            return Err(RSpaceError::ProduceCommitDenied);
        };
        native_backing::reserve_blocks_copy_and_cleanup(waiter.continuation.as_ref(), meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(waiter.patterns.as_ref(), meter)?;
        native_backing::reserve_blocks_slice_copy_and_cleanup(&candidate.channels, meter)?;
        let continuation = ContResult {
            continuation: waiter.continuation.as_ref().clone(),
            persistent: waiter.persist,
            channels: candidate.channels.clone(),
            patterns: waiter.patterns.as_ref().clone(),
            peek: !waiter.peeks.is_empty(),
        };

        native_backing::reserve_blocks_copy_and_cleanup(&waiter.source, meter)?;
        native_backing::reserve_blocks_copy_and_cleanup(&waiter.peeks, meter)?;
        let event = RestingCOMM {
            consume: waiter.source.clone(),
            continuation_index: candidate.continuation_index,
            ordered_produces,
            ordered_data_indices,
            peeks: waiter.peeks.clone(),
        };
        let identity = event.cost_v2_identity_metered(meter)?;
        let allocation = shared::rust::clone_backing::arc_allocation_bytes::<RestingCOMM>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(1, allocation, allocation)?;
        let event = Arc::new(event);
        // Hold a destination-specific slot through retirement. Concurrent
        // ordinary writers preserve leased capacity, including in an ordered
        // causal-path log, so the post-retirement append cannot allocate.
        let event_slot = self.reserve_resting_event_slot(meter)?;

        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
        let source = RSpaceOperationSource::Resting(event.as_ref());
        let store = self.get_store();
        let mut observer_started = false;
        let mut authorization = AuthorizeRestingOnce(Some(|| {
            prepared
                .commit(|| {
                    if let Some(observer) = observer.as_deref() {
                        observer.observe_operation_start(source, &candidate.channels, &[])?;
                        observer_started = true;
                        observer.observe_resting_comm(
                            &identity,
                            event.as_ref(),
                            waiter.continuation.as_ref(),
                            waiter.persist,
                            &observed_data,
                        )?;
                    }
                    Ok::<(), RSpaceError>(())
                })
                .and_then(std::convert::identity)
        }));
        let outcome = store.retire_resting_match_metered(
            &candidate.channels,
            candidate.continuation_index,
            waiter.persist,
            &rows,
            &retirement,
            meter,
            &mut authorization,
        );
        drop(authorization);
        if outcome.is_ok() {
            event_slot.publish(Event::RestingComm(Arc::clone(&event)));
        }
        if observer_started {
            let observer = observer.as_deref().expect("observer started");
            observer.observe_operation_finish(
                source,
                if outcome.is_ok() {
                    RSpaceOperationCompletion::Matched
                } else {
                    RSpaceOperationCompletion::Rejected
                },
            );
        }
        outcome?;
        Ok(Some((continuation, rows)))
    }

    /// Read only resident rows for a resting commit. A miss refuses instead
    /// of entering the unmetered history decoder or filling the hot cache.
    pub(super) fn read_resting_hot(
        &self,
        candidate: &ProduceCandidate<C, P, A, K>,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<RestingHotRead<P, A, K>, RSpaceError>
    where
        C: CloneBacking,
        P: CloneBacking,
        A: CloneBacking,
        K: CloneBacking,
    {
        let store = self.get_store();
        let installed = if candidate.continuation_index == 0 {
            store.get_installed_continuation_view_metered(&candidate.channels, meter)?
        } else {
            None
        };
        let is_installed = installed.is_some();
        let waiters = if let Some(installed) = installed {
            meter.reserve(1, 0, size_of::<Arc<WaitingContinuation<P, K>>>())?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(1)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            values.push(installed);
            values
        } else {
            store.get_continuation_views_with_reader(
                &candidate.channels,
                &|| Err(RSpaceError::HostWorkRejected),
                meter,
            )?
        };
        let count = candidate.channels.len();
        let bytes = count
            .checked_mul(size_of::<Vec<Datum<A>>>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(1, 0, bytes)?;
        let mut data = Vec::new();
        data.try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for channel in &candidate.channels {
            data.push(store.get_data_uncached_with_reader(
                channel,
                &|| Err(RSpaceError::HostWorkRejected),
                meter,
            )?);
        }
        if let Some(waiter) = usize::try_from(candidate.continuation_index)
            .ok()
            .and_then(|index| waiters.get(index))
        {
            // Persistent and installed waiters retain their join; only a
            // linear waiter needs a join removal preflight. Reading absent
            // ordinary joins for an installed-only waiter would otherwise
            // enter cold history despite the installed join being hot.
            if !waiter.persist {
                for channel in &candidate.channels {
                    store.get_joins_with_reader(
                        channel,
                        &|| Err(RSpaceError::HostWorkRejected),
                        meter,
                    )?;
                }
            }
        }
        Ok(RestingHotRead {
            waiters,
            data,
            installed: is_installed,
        })
    }
}
