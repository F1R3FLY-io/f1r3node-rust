use std::borrow::Borrow;

use super::*;
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
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
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
        let prepared = self.space.prepare_native_consume_candidate(
            &channels,
            &patterns,
            &continuation,
            &source,
            &peeks,
            ticket.candidate_identity(),
        );
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
            if store.put_continuation(&channels, waiting).unwrap_or(false) {
                self.space.inc_replay_waiting_continuations(&channels);
            }
            for channel in &channels {
                store.put_join(channel, &channels);
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
        for (position, index) in result.retirement {
            store.remove_datum(&result.data[position].channel, index)?;
        }
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
        let joins = self.space.get_store().get_joins(&channel);
        ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        let outcome = ticket.outcome();
        let decision = ticket.observe_produce(&source, &channel, &data, authority)?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome).map(|()| None);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let prepared = self.space.prepare_native_produce_candidate(
            &channel,
            &data,
            persist,
            &source,
            joins,
            ticket.candidate_identity(),
        )?;
        if outcome == NativeReplayOutcome::Stored {
            if prepared.is_some() {
                return Err(mismatch());
            }
            let mut completion = ticket.prepare(outcome)?;
            let publication =
                PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
            self.space.increment_produce_counter(&source, persist);
            let result = self.space.store_data(channel, data, persist, source);
            completion.publish();
            publication.complete();
            return Ok(result);
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
        let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
        let publication =
            PublicationGuard::with_invalidation(&self.unavailable, || self.epoch.invalidate());
        self.space.increment_produce_counter(&source, persist);
        let store = self.space.get_store();
        if !continuation.persistent {
            store
                .remove_continuation(&continuation.channels, candidate.continuation_index)
                .ok_or_else(mismatch)?;
        }
        self.space.mark_replay_waiting_continuation_match();
        for (position, index) in result.retirement {
            store.remove_datum(&result.data[position].channel, index)?;
        }
        for datum in &result.data {
            store.remove_join(&datum.channel, &continuation.channels);
        }
        completion.publish();
        publication.complete();
        Ok(Some((continuation, result.data, returned)))
    }
}
