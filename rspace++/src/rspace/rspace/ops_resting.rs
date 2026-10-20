use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::{EventCodecEpoch, StableHashSerialize};
use crate::rspace::hot_store::RestingRetirementAuthorization;
use crate::rspace::internal::{Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceOperationCompletion, RSpaceOperationSource, RSpaceResult,
};
use crate::rspace::striped_locks;
use crate::rspace::trace::event::{Consume, Event, Produce, RestingCOMM};

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
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        if self.epoch != EventCodecEpoch::CostV2 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let _checkpoint_guard = self.resting_checkpoint_barrier.read().await;
        let count = candidate.channels.len();
        if count == 0 || candidate.data_candidates.len() != count {
            return Ok(None);
        }
        let mut channel_hashes = Vec::<u64>::with_capacity(count);
        for channel in &candidate.channels {
            channel_hashes.push(striped_locks::channel_hash(channel));
        }
        let (_phase_a, _phase_b) = self.consume_lock(&channel_hashes).await;
        let read = self.read_resting_hot(candidate)?;
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
        let authenticated_waiter = Consume::create_for_epoch(
            self.epoch,
            &candidate.channels,
            waiter.patterns.as_ref(),
            waiter.continuation.as_ref(),
            waiter.persist,
        )?;
        if authenticated_waiter != waiter.source {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }

        let mut rows = Vec::<RSpaceResult<C, A>>::with_capacity(count);
        let mut ordered_produces = Vec::with_capacity(count);
        let mut ordered_data_indices = Vec::with_capacity(count);
        let mut retirement = Vec::<(usize, i32)>::with_capacity(count);
        for (position, selection) in candidate.data_candidates.iter().enumerate() {
            let channel = &candidate.channels[position];

            if selection.channel != *channel {
                return Ok(None);
            }
            let Some(index) = usize::try_from(selection.datum_index).ok() else {
                return Ok(None);
            };
            let Some(datum) = read.data[position].get(index) else {
                return Ok(None);
            };

            if datum.source.hash != selection.datum.source.hash ||
                datum.source.channel_hash != selection.datum.source.channel_hash ||
                datum.source.persistent != selection.datum.source.persistent ||
                datum.persist != selection.datum.persist
            {
                return Ok(None);
            }
            let authenticated_datum =
                Produce::create_for_epoch(self.epoch, channel, datum.a.as_ref(), datum.persist)?;
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
                    if candidate.channels[previous] == *channel &&
                        candidate.data_candidates[previous].datum_index == selection.datum_index
                    {
                        return Ok(None);
                    }
                }
            }
            let Some(matched) = self
                .matcher
                .get(&waiter.patterns[position], datum.a.as_ref())
            else {
                return Ok(None);
            };

            rows.push(RSpaceResult {
                channel: channel.clone(),
                matched_datum: matched,
                removed_datum: datum.a.as_ref().clone(),
                persistent: datum.persist,
            });

            ordered_produces.push(datum.source.clone());
            ordered_data_indices.push(selection.datum_index);
            if !datum.persist {
                retirement.push((position, selection.datum_index));
            }
        }
        // Native retirement applies each removal to a staged copy, so repeated
        // channels must be retired at descending store indices.
        retirement
            .sort_unstable_by(|(_, left_index), (_, right_index)| right_index.cmp(left_index));

        let mut matched_refs = Vec::<&A>::with_capacity(count);
        let mut observed_data = Vec::<(&A, bool)>::with_capacity(count);
        for (position, row) in rows.iter().enumerate() {
            matched_refs.push(&row.matched_datum);
            let datum = &read.data[position][ordered_data_indices[position] as usize];
            observed_data.push((datum.a.as_ref(), datum.persist));
        }
        let Some(prepared) = self
            .matcher
            .prepare_commit(waiter.continuation.as_ref(), &matched_refs)
        else {
            return Err(RSpaceError::ProduceCommitDenied);
        };

        let continuation = ContResult {
            continuation: waiter.continuation.as_ref().clone(),
            persistent: waiter.persist,
            channels: candidate.channels.clone(),
            patterns: waiter.patterns.as_ref().clone(),
            peek: !waiter.peeks.is_empty(),
        };

        let event = RestingCOMM {
            consume: waiter.source.clone(),
            continuation_index: candidate.continuation_index,
            ordered_produces,
            ordered_data_indices,
            peeks: waiter.peeks.clone(),
        };
        let identity = event.cost_v2_identity()?;
        let event = Arc::new(event);
        // Hold a destination-specific slot through retirement. Concurrent
        // ordinary writers preserve leased capacity, including in an ordered
        // causal-path log, so the post-retirement append cannot allocate.
        let event_slot = self.reserve_resting_event_slot()?;

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
        let outcome = store.retire_resting_match(
            &candidate.channels,
            candidate.continuation_index,
            waiter.persist,
            &rows,
            &retirement,
            &mut authorization,
        );
        // Changed by the merge of feature/cost-accounted-rho (clippy gate):
        // `AuthorizeRestingOnce` has no drop glue, so the `drop` did nothing.
        // drop(authorization);
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

    // Added by the merge of feature/cost-accounted-rho (finding R2-08): HEAD's
    // doc, restored. HEAD said "the unmetered history decoder". DR-131 removed
    // the meter, so the word "unmetered" no longer applies.
    /// Read only resident rows for a resting commit. A miss refuses instead
    /// of entering the history decoder or filling the hot cache.
    pub(super) fn read_resting_hot(
        &self,
        candidate: &ProduceCandidate<C, P, A, K>,
    ) -> Result<RestingHotRead<P, A, K>, RSpaceError> {
        let store = self.get_store();
        let installed = if candidate.continuation_index == 0 {
            store.get_installed_continuation_view(&candidate.channels)?
        } else {
            None
        };
        let is_installed = installed.is_some();
        let waiters = if let Some(installed) = installed {
            let mut values = Vec::with_capacity(1);
            values.push(installed);
            values
        } else {
            store.get_continuation_views_with_reader(&candidate.channels, &|| {
                Err(RSpaceError::ProduceCommitProtocolViolation)
            })?
        };
        let count = candidate.channels.len();
        let mut data = Vec::with_capacity(count);
        for channel in &candidate.channels {
            data.push(store.get_data_uncached_with_reader(channel, &|| {
                Err(RSpaceError::ProduceCommitProtocolViolation)
            })?);
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
                    store.get_joins_with_reader(channel, &|| {
                        Err(RSpaceError::ProduceCommitProtocolViolation)
                    })?;
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
