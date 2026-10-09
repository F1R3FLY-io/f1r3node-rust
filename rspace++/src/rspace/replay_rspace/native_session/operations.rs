use std::borrow::Borrow;

use shared::rust::clone_backing::CloneBacking;

use super::*;
use crate::rspace::hashing::native_source::{GroupKeys, OperationKeys, StoreKey};
// Changed by D-D5 (DR-107): the produce selection owns the incoming datum.
// use crate::rspace::replay_rspace::native_candidate::metered::CandidateReader;
use crate::rspace::replay_rspace::native_candidate::metered::{CandidateReader, ProduceSelection};
use crate::rspace::replay_rspace::native_epoch::{
    NativeOperationEpoch, NativeOperationPublication, NativeOperationTicket, NativeReplayDecision,
    NativeReplayOutcome,
};

type Authority<C, P, A, K, E> =
    <<E as NativeOperationEpoch<C, P, A, K>>::Ticket as NativeOperationTicket<C, P, A, K>>::Authority;

fn mismatch() -> RSpaceError {
    RSpaceError::InterpreterError("native session outcome differs from actual execution".to_owned())
}

fn require(
    decision: NativeReplayDecision,
    expected: NativeReplayDecision,
) -> Result<(), RSpaceError> {
    if decision == expected {
        Ok(())
    } else {
        Err(mismatch())
    }
}

/// D-E3 (DR-110): the keys of the joins that a produce reads, with one
/// digest per channel, and the traversals of the join channels that the keys
/// read. The key writer reserves only the bytes that it writes, and the join
/// copy prepays only its clone and its release (DR-108). So each join gets
/// one block inspection before its keys (DR-108, decision 6).
pub(crate) fn join_keys<C: Serialize + CloneBacking>(
    joins: &[Vec<C>],
    reserve: &impl crate::rspace::hashing::native_source::SourceMeter,
) -> Result<OperationKeys, RSpaceError> {
    for join in joins {
        crate::rspace::native_backing::inspect_blocks_slice(join, reserve)?;
    }
    OperationKeys::build(joins, reserve)
}

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K>,
{
    fn publish_denial(
        &self,
        ticket: E::Ticket,
        outcome: NativeReplayOutcome,
    ) -> Result<(), RSpaceError> {
        let mut completion = ticket.prepare(outcome)?;
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        completion.publish();
        publication.complete();
        // Changed by DR-113: the epoch names the error of its denial.
        // Err(RSpaceError::OutOfPhlogistons)
        Err(self.epoch.denial())
    }

    pub async fn consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
        authority: &Authority<C, P, A, K, E>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        self.consume_with_authority(channels, patterns, continuation, persist, peeks, |_| {
            Ok(authority)
        })
        .await
    }

    pub async fn consume_with_authority<F, R>(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
        resolve: F,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError>
    where
        F: FnOnce(&Consume) -> Result<R, RSpaceError>,
        R: Borrow<Authority<C, P, A, K, E>>,
    {
        if channels.is_empty() || channels.len() != patterns.len() {
            return Err(mismatch());
        }
        // Changed by D-C2c (D-S1, DR-96): the source also returns the keys of
        // the channels.
        // let source = self.consume_source(&channels, &patterns, &continuation,
        // persist)?;
        let (source, channel_keys) =
            self.consume_source(&channels, &patterns, &continuation, persist)?;
        let authority = resolve(&source)?;
        let authority = authority.borrow();
        let hashes = self.channel_hashes(&channels, channels.len())?;
        let (_shared, _channels, mut ticket) = loop {
            self.ensure_open()?;
            self.epoch
                .wait_ready(RSpaceOperationSource::Consume(&source))
                .await?;
            let shared = self.gate.read().await;
            self.ensure_open()?;
            let locked = self.consume_lock(&hashes).await?;
            self.ensure_open()?;
            if let Some(ticket) = self
                .epoch
                .begin_operation(RSpaceOperationSource::Consume(&source))?
            {
                break (shared, locked, ticket);
            }
        };
        ticket.authenticate_footprint(&channels, &[])?;
        let outcome = ticket.outcome();
        let decision = ticket.observe_consume(
            &source,
            &channels,
            &patterns,
            &continuation,
            &peeks,
            authority,
        )?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        // D-C2c (D-S1, DR-96): the group key of the consume's channels.
        let keys = GroupKeys::from_channel_keys(channel_keys, &reserve)?;
        // Changed by C1 (DR-81): the prefetch fills the cache and copies no
        // continuation.
        // self.read_continuations(&channels)?;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the keys.
        // self.prefetch_continuations(&channels)?;
        // for channel in &channels {
        //     self.read_joins(channel)?;
        // }
        self.prefetch_continuations(&channels, &keys)?;
        for (channel, channel_key) in channels.iter().zip(&keys.channels) {
            self.read_joins(channel, *channel_key)?;
        }
        // let reserve =
        //     |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        // Changed by C2 (DR-82): candidates read copy-free data views.
        // let read_data = |channel: &C| self.read_data_with(channel, &reserve);
        // Changed by D-C2c (D-S1, DR-96): the readers receive the keys.
        // let read_data = |channel: &C| self.read_data_view_with(channel, &reserve);
        let read_data = |channel: &C, channel_key: StoreKey| {
            self.read_data_view_with(channel, channel_key, &reserve)
        };
        // Changed by C1 (DR-81): candidates read shared views.
        // let read_continuations = |channels: &[C]| self.read_continuations(channels);
        // let read_continuations = |channels: &[C]|
        // self.read_continuation_views(channels);
        let read_continuations =
            |channels: &[C], group: &GroupKeys| self.read_continuation_views(channels, group);
        let reader = CandidateReader {
            meter: &reserve,
            data: &read_data,
            continuations: &read_continuations,
        };
        let prepared = self.space.prepare_metered_consume_candidate(
            &channels,
            &keys.channels,
            &patterns,
            &continuation,
            &source,
            &peeks,
            ticket.candidate_identity(),
            &reader,
        )?;
        if outcome == NativeReplayOutcome::Stored {
            if prepared.is_some() {
                return Err(mismatch());
            }
            let waiting = WaitingContinuation {
                patterns,
                continuation,
                persist,
                peeks,
                source,
            };
            let mut completion = ticket.prepare(outcome)?;
            let publication =
                PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
            // Changed by D-C2c (D-S1, DR-96): the native session uses its
            // digest-keyed store.
            // let store = self.space.get_store();
            // let (inserted, depth) = store.store_consume_metered(&channels, waiting,
            // &reserve)?;
            let (inserted, depth) = self
                .store
                .store_consume(&channels, &keys, waiting, &reserve)?;
            if inserted {
                self.space.inc_replay_waiting_continuations_depth(depth);
            }
            completion.publish();
            publication.complete();
            return Ok(None);
        }
        let prepared = prepared.ok_or_else(mismatch)?;
        let decision =
            ticket.observe_comm(&prepared.comm, &continuation, persist, &prepared.data)?;
        if outcome == NativeReplayOutcome::DeniedComm {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let result = result::prepare(prepared.data, |operations, bytes| {
            self.epoch.reserve_work(operations, bytes)
        })?;
        let continuation = ContResult {
            patterns,
            continuation,
            persistent: persist,
            peek: !peeks.is_empty(),
            channels,
        };
        let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        // Changed by D-C2c (D-S1, DR-96): the native session uses its
        // digest-keyed store.
        // let store = self.space.get_store();
        // store.retire_data_metered(&result.data, &result.retirement, &reserve)?;
        self.store
            .retire_data(&result.data, &keys.channels, &result.retirement, &reserve)?;
        completion.publish();
        publication.complete();
        Ok(Some((continuation, result.data)))
    }

    pub async fn produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
        authority: &Authority<C, P, A, K, E>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        self.produce_with_authority(channel, data, persist, |_| Ok(authority))
            .await
    }

    pub async fn produce_with_authority<F, R>(
        &self,
        channel: C,
        data: A,
        persist: bool,
        resolve: F,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError>
    where
        F: FnOnce(&Produce) -> Result<R, RSpaceError>,
        R: Borrow<Authority<C, P, A, K, E>>,
    {
        let source = self.produce_source(&channel, &data, persist)?;
        // D-C2c (D-S1, DR-96): the store key of the produced channel is its
        // source digest.
        let key = StoreKey::from_digest(&source.channel_hash, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        let authority = resolve(&source)?;
        let authority = authority.borrow();
        let (_shared, _channels, mut ticket) = loop {
            self.ensure_open()?;
            self.epoch
                .wait_ready(RSpaceOperationSource::Produce(&source))
                .await?;
            let shared = self.gate.read().await;
            self.ensure_open()?;
            // Changed by D-C2c (D-S1, DR-96): the lock reads the joins by the
            // channel key.
            // let locked = self.produce_lock(&channel).await?;
            let locked = self.produce_lock(&channel, key).await?;
            self.ensure_open()?;
            if let Some(ticket) = self
                .epoch
                .begin_operation(RSpaceOperationSource::Produce(&source))?
            {
                break (shared, locked, ticket);
            }
        };
        // Changed by D-C2c (D-S1, DR-96): the store reads by the channel key.
        // let joins = self.read_joins(&channel)?;
        let joins = self.read_joins(&channel, key)?;
        ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        let outcome = ticket.outcome();
        let decision = ticket.observe_produce(&source, &channel, &data, authority)?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        // D-C2c (D-S1, DR-96): the keys of the joins, one digest per channel,
        // computed once for the operation.
        // Changed by D-E3 (DR-110): the keys also reserve the traversals of the
        // join channels that they read.
        // let join_keys = OperationKeys::build(&joins, &reserve)?;
        let join_keys = join_keys(&joins, &reserve)?;
        // Changed by C2 (DR-82): the prefetch copies no datum.
        // self.prepare_data(&channel)?;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the keys.
        // self.prefetch_data(&channel)?;
        // for channels in &joins {
        //     for channel in channels {
        //         self.read_joins(channel)?;
        //     }
        // }
        self.prefetch_data(&channel, key)?;
        for (channels, group) in joins.iter().zip(&join_keys.groups) {
            for (joined, joined_key) in channels.iter().zip(&group.channels) {
                self.read_joins(joined, *joined_key)?;
            }
        }
        // let reserve =
        //     |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        // Changed by C2 (DR-82): candidates read copy-free data views.
        // let read_data = |channel: &C| self.read_data_with(channel, &reserve);
        // Changed by D-C2c (D-S1, DR-96): the readers receive the keys.
        // let read_data = |channel: &C| self.read_data_view_with(channel, &reserve);
        let read_data = |channel: &C, channel_key: StoreKey| {
            self.read_data_view_with(channel, channel_key, &reserve)
        };
        // Changed by C1 (DR-81): candidates read shared views.
        // let read_continuations = |channels: &[C]| self.read_continuations(channels);
        // let read_continuations = |channels: &[C]|
        // self.read_continuation_views(channels);
        let read_continuations =
            |channels: &[C], group: &GroupKeys| self.read_continuation_views(channels, group);
        let reader = CandidateReader {
            meter: &reserve,
            data: &read_data,
            continuations: &read_continuations,
        };
        // Changed by D-D5 (DR-107): the selection owns the incoming datum.
        // It gives the datum back when nothing matches, and the source back
        // when a continuation matches.
        // let prepared = self.space.prepare_metered_produce_candidate(
        //     &channel,
        //     &data,
        //     persist,
        //     &source,
        //     joins,
        //     &join_keys,
        //     ticket.candidate_identity(),
        //     &reader,
        // )?;
        let selection = self.space.prepare_metered_produce_candidate(
            &channel,
            Datum {
                a: data,
                persist,
                source,
            },
            joins,
            &join_keys,
            ticket.candidate_identity(),
            &reader,
        )?;
        if outcome == NativeReplayOutcome::Stored {
            // if prepared.is_some() {
            //     return Err(mismatch());
            // }
            let ProduceSelection::Unmatched(incoming) = selection else {
                return Err(mismatch());
            };
            // let counter = self
            //     .space
            //     .prepare_metered_produce_counter(&source, persist, &reserve)?;
            let counter = self.space.prepare_metered_produce_counter(
                &incoming.source,
                incoming.persist,
                &reserve,
            )?;
            let mut completion = ticket.prepare(outcome)?;
            let publication =
                PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
            // Changed by D-C2c (D-S1, DR-96): the native session uses its
            // digest-keyed store.
            // self.space.get_store().put_datum_metered(
            //     &channel,
            //     Datum {
            //         a: data,
            //         persist,
            //         source,
            //     },
            //     &reserve,
            // )?;
            // Changed by D-C2e (D-S1, DR-96): the publication passes the
            // channel for the collision check.
            // self.store.put_datum(
            //     key,
            //     Datum {
            //         a: data,
            //         persist,
            //         source,
            //     },
            //     &reserve,
            // )?;
            // Changed by D-D5 (DR-107): the store receives the datum that
            // the selection gave back.
            // self.store.put_datum(
            //     &channel,
            //     key,
            //     Datum {
            //         a: data,
            //         persist,
            //         source,
            //     },
            //     &reserve,
            // )?;
            self.store.put_datum(&channel, key, incoming, &reserve)?;
            counter.publish();
            completion.publish();
            publication.complete();
            return Ok(None);
        }
        // Changed by D-C2c (D-S1, DR-96): the candidate names its join group.
        // let prepared = prepared.ok_or_else(mismatch)?;
        // Changed by D-D5 (DR-107): the selection also gives the source back.
        // let (prepared, group) = prepared.ok_or_else(mismatch)?;
        let ProduceSelection::Matched {
            prepared,
            group,
            source,
        } = selection
        else {
            return Err(mismatch());
        };
        let group_keys = join_keys.groups.get(group).ok_or_else(mismatch)?;
        let candidate = prepared.candidate;
        let decision = ticket.observe_comm(
            &prepared.comm,
            &candidate.continuation.continuation,
            candidate.continuation.persist,
            &candidate.data_candidates,
        )?;
        if outcome == NativeReplayOutcome::DeniedComm {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let returned = ticket.returned_produce(&source)?;
        let result = result::prepare(candidate.data_candidates, |operations, bytes| {
            self.epoch.reserve_work(operations, bytes)
        })?;
        let continuation = ContResult {
            continuation: candidate.continuation.continuation,
            persistent: candidate.continuation.persist,
            channels: candidate.channels,
            patterns: candidate.continuation.patterns,
            peek: !candidate.continuation.peeks.is_empty(),
        };
        let counter = self
            .space
            .prepare_metered_produce_counter(&source, persist, &reserve)?;
        let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        // Changed by D-C2c (D-S1, DR-96): the native session uses its
        // digest-keyed store.
        // let store = self.space.get_store();
        // store.retire_produce_match_metered(
        //     &continuation.channels,
        //     candidate.continuation_index,
        //     continuation.persistent,
        //     &result.data,
        //     &result.retirement,
        //     &reserve,
        // )?;
        self.store.retire_produce_match(
            &continuation.channels,
            group_keys,
            candidate.continuation_index,
            continuation.persistent,
            &result.data,
            &result.retirement,
            &reserve,
        )?;
        self.space.mark_replay_waiting_continuation_match();
        counter.publish();
        completion.publish();
        publication.complete();
        Ok(Some((continuation, result.data, returned)))
    }
}
