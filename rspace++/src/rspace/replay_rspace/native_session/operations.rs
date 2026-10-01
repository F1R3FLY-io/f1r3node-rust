use std::borrow::Borrow;

use shared::rust::clone_backing::CloneBacking;

use super::*;
use crate::rspace::replay_rspace::native_candidate::metered::CandidateReader;
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

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
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
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
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
        Err(RSpaceError::OutOfPhlogistons)
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
        let source = self.consume_source(&channels, &patterns, &continuation, persist)?;
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
        self.read_continuations(&channels)?;
        for channel in &channels {
            self.read_joins(channel)?;
        }
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        let read_data = |channel: &C| self.read_data_with(channel, &reserve);
        let read_continuations = |channels: &[C]| self.read_continuations(channels);
        let reader = CandidateReader {
            meter: &reserve,
            data: &read_data,
            continuations: &read_continuations,
        };
        let prepared = self.space.prepare_metered_consume_candidate(
            &channels,
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
            let store = self.space.get_store();
            let (inserted, depth) = store.store_consume_metered(&channels, waiting, &reserve)?;
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
        let store = self.space.get_store();
        store.retire_data_metered(&result.data, &result.retirement, &reserve)?;
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
        let authority = resolve(&source)?;
        let authority = authority.borrow();
        let (_shared, _channels, mut ticket) = loop {
            self.ensure_open()?;
            self.epoch
                .wait_ready(RSpaceOperationSource::Produce(&source))
                .await?;
            let shared = self.gate.read().await;
            self.ensure_open()?;
            let locked = self.produce_lock(&channel).await?;
            self.ensure_open()?;
            if let Some(ticket) = self
                .epoch
                .begin_operation(RSpaceOperationSource::Produce(&source))?
            {
                break (shared, locked, ticket);
            }
        };
        let joins = self.read_joins(&channel)?;
        ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        let outcome = ticket.outcome();
        let decision = ticket.observe_produce(&source, &channel, &data, authority)?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        self.prepare_data(&channel)?;
        for channels in &joins {
            for channel in channels {
                self.read_joins(channel)?;
            }
        }
        let reserve =
            |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
        let read_data = |channel: &C| self.read_data_with(channel, &reserve);
        let read_continuations = |channels: &[C]| self.read_continuations(channels);
        let reader = CandidateReader {
            meter: &reserve,
            data: &read_data,
            continuations: &read_continuations,
        };
        let prepared = self.space.prepare_metered_produce_candidate(
            &channel,
            &data,
            persist,
            &source,
            joins,
            ticket.candidate_identity(),
            &reader,
        )?;
        if outcome == NativeReplayOutcome::Stored {
            if prepared.is_some() {
                return Err(mismatch());
            }
            let counter = self
                .space
                .prepare_metered_produce_counter(&source, persist, &reserve)?;
            let mut completion = ticket.prepare(outcome)?;
            let publication =
                PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
            self.space.get_store().put_datum_metered(
                &channel,
                Datum {
                    a: data,
                    persist,
                    source,
                },
                &reserve,
            )?;
            counter.publish();
            completion.publish();
            publication.complete();
            return Ok(None);
        }
        let prepared = prepared.ok_or_else(mismatch)?;
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
        let store = self.space.get_store();
        store.retire_produce_match_metered(
            &continuation.channels,
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
