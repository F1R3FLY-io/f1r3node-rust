// Native replay mode of the Reified RSpaces substrate: `SpaceTxnHost` for the
// native replay session (decision 47, design v2, R1 to R7).
//
// The session has no rigged log. Each staged operation meets its recorded
// journal row at commit, under the sub-order of the operation (D3), through
// the ticket protocol of the session: the operation begins its row, which
// fails closed when the row is not ready (P1), authenticates the footprint,
// checks that the recorded outcome fits the staged kind, makes the
// observations of play, prepares the completion, and publishes it with the
// effects under one publication guard. A backend never waits for readiness,
// so it never waits while it holds stripes: the per-space key of I-11
// orders every transaction of one space before the next, so every
// predecessor of a space row has completed (design v2, section 4.5).
//
// The effects are the store steps of play (`store_data` and the others of
// `space_store_handle.rs`), so native replay changes the store as play does.
// The session logs no event. The backend gives the events of play to the
// event correlation check of `commit_operation` only. A non-persistent
// produce advances the produce counter of the session, which `host_counter`
// reads, as `log_produce` advances the counter of play.
//
// Native play charges no host work for the source, the locks and the
// produce counter of a space operation, so this backend charges none either.
// Its view and its commit checks read through the metered reads of the
// transaction (S4, S5). The ticket protocol charges its own verification.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;
use shared::rust::clone_backing::CloneBacking;

use super::{NativeReplaySession, PublicationGuard};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::native_epoch::{
    NativeOperationEpoch, NativeOperationPublication, NativeOperationTicket, NativeReplayDecision,
    NativeReplayOutcome,
};
use crate::rspace::rspace_interface::RSpaceOperationSource;
use crate::rspace::space_store_handle::{
    BeforeObserve, BoxedSpaceTxn, HostTxn, LockCoverage, Reads, ReadyConsume, ReadyConsumeFire,
    ReadyFire, ReadyProduce, SharedHistory, SharedStore, SpaceMode, SpaceOpReceipt, SpaceTxnError,
    SpaceTxnHost, StagedConsume, StagedConsumeFire, StagedFire, StagedOperation, StagedProduce,
    SweepReport, TxnBackend, TxnRegistration, channel_key, fired_receipt, ready_consume,
    ready_produce, remove_matched_datum_and_join, root_leaf_records, run_before_observe,
    store_data, store_persistent_data, store_waiting_continuation, sweep_channels,
    sweep_volatile_channels,
};
use crate::rspace::striped_locks;
use crate::rspace::trace::event::{COMM, Event, IOEvent, Produce};

type Authority<C, P, A, K, E> =
    <<E as NativeOperationEpoch<C, P, A, K>>::Ticket as NativeOperationTicket<C, P, A, K>>::Authority;

/// The introduction authority of a space operation in native replay. The
/// accessor of the accounting session supplies it, as `NativeExecution`
/// resolves the authority of a host operation (decision 47, W1).
pub type AuthorityResolver<C, P, A, K, E> = Arc<
    dyn for<'s> Fn(RSpaceOperationSource<'s>) -> Result<Authority<C, P, A, K, E>, RSpaceError>
        + Send
        + Sync,
>;

/// `SpaceTxnHost` over a native replay session that `owner` holds (decision
/// 47, R1). The session sits inside its owner by value, so the host keeps the
/// owner and a projection to the session.
pub struct NativeSpaceHost<O, C, P, A, K, E>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    E: NativeOperationEpoch<C, P, A, K>,
{
    owner: Arc<O>,
    session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
    authority: AuthorityResolver<C, P, A, K, E>,
    installed: BTreeSet<Blake2b256Hash>,
}

// The mismatch of native replay, in the class of the session's own mismatch
// (`native_session/operations.rs`, `mismatch`): a platform failure.
fn mismatch(reason: &str) -> SpaceTxnError {
    SpaceTxnError::Host(RSpaceError::InterpreterError(format!("native space replay: {reason}")))
}

fn require(
    decision: NativeReplayDecision,
    expected: NativeReplayDecision,
) -> Result<(), SpaceTxnError> {
    match decision == expected {
        true => Ok(()),
        false => Err(mismatch("the observed decision differs from the recorded outcome")),
    }
}

// The reservation of work that native play does not charge for a space
// operation: the locks and the produce counter (see the module comment).
fn free(_operations: usize, _scanned: usize, _backing: usize) -> Result<(), RSpaceError> { Ok(()) }

impl<O, C, P, A, K, E> NativeSpaceHost<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
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
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    /// The host of the session that `session` projects out of `owner`. The
    /// installed channels are taken once, from the installed continuations of
    /// the session store (R2), because the session installs its system
    /// processes into the store only.
    pub fn new(
        owner: Arc<O>,
        session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
        authority: AuthorityResolver<C, P, A, K, E>,
    ) -> Result<Self, SpaceTxnError> {
        let state = session(&owner).space.get_store().snapshot();
        let mut installed = BTreeSet::new();
        for channels in state.installed_continuations_flat().keys() {
            for channel in channels {
                installed.insert(channel_key(channel)?);
            }
        }
        Ok(Self {
            owner,
            session,
            authority,
            installed,
        })
    }

    fn session(&self) -> &NativeReplaySession<C, P, A, K, E> { (self.session)(&self.owner) }
}

#[async_trait]
impl<O, C, P, A, K, E> SpaceTxnHost<C, P, A, K> for NativeSpaceHost<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
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
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    fn mode(&self) -> SpaceMode { SpaceMode::NativeReplay }

    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { Arc::clone(&self.session().space.matcher) }

    // R3. A transaction of native replay reads through metered reads only:
    // native play meters the same reads, and a plain read would make the
    // charges of the two modes differ (I-12). The stripes are taken without a
    // charge, as play takes them.
    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        if !registration.is_metered() {
            return Err(SpaceTxnError::InvalidOperation(
                "a native replay transaction reads through metered reads",
            ));
        }
        if lock_keys.is_empty() {
            return Err(SpaceTxnError::InvalidOperation("lock keys must not be empty"));
        }
        let hashes: Vec<u64> = lock_keys.iter().map(striped_locks::channel_hash).collect();
        let coverage = LockCoverage::new(
            &hashes,
            session.space.phase_a_locks.len(),
            session.space.phase_b_locks.len(),
        );
        let locks = session
            .consume_lock_with(&hashes, |_operations, _bytes| Ok(()))
            .await?;
        let store = session.space.get_store();
        let backend = NativeBackend {
            owner: Arc::clone(&self.owner),
            session: self.session,
            authority: Arc::clone(&self.authority),
            denied: AtomicBool::new(false),
        };
        Ok(Box::new(HostTxn::new(
            SpaceMode::NativeReplay,
            backend,
            store,
            self.installed.clone(),
            coverage,
            locks,
            registration,
        )?))
    }

    // R7.
    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        let repository = self.session().space.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        root_leaf_records(reader.as_ref(), channel)
    }

    // R6: `sweep_replay` over the session, with the installed channels of R2.
    async fn sweep_volatile(
        &self,
        channels: &[C],
        _registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        let channels = sweep_channels(channels)?;
        let hashes: Vec<u64> = channels.values().map(striped_locks::channel_hash).collect();
        let _guards = session
            .consume_lock_with(&hashes, |_operations, _bytes| Ok(()))
            .await?;
        let _gate = session
            .gate
            .try_read()
            .map_err(|_| mismatch("the session is at a boundary"))?;
        let repository = session.space.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        let store = session.space.get_store();
        sweep_volatile_channels(&**store, reader.as_ref(), &self.installed, &channels, || {
            if session
                .space
                .replay_waiting_continuations_estimate
                .load(std::sync::atomic::Ordering::Relaxed) >
                0
            {
                session.space.dec_replay_waiting_continuations();
            }
        })
    }
}

// The backend of a native replay transaction.
struct NativeBackend<O, C, P, A, K, E>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    E: NativeOperationEpoch<C, P, A, K>,
{
    owner: Arc<O>,
    session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
    authority: AuthorityResolver<C, P, A, K, E>,
    // Set when the current operation published a recorded denial, so its row
    // is complete and its failure keeps the epoch open.
    denied: AtomicBool,
}

impl<O, C, P, A, K, E> TxnBackend<C, P, A, K> for NativeBackend<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
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
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    fn store(&self) -> SharedStore<C, P, A, K> { self.session().space.get_store() }

    // A3: the produce counter of the session.
    fn host_counter(&self, produce: &Produce) -> i32 {
        self.session().space.get_produce_count(produce)
    }

    fn recorded(&self, _event: &IOEvent) -> Vec<(COMM, usize)> { Vec::new() }

    fn recorded_binding(&self, _event: &IOEvent, _comm: &COMM) -> Option<(COMM, usize)> { None }

    // The session reads its history at its own root.
    fn history(&self) -> Result<(SharedHistory<C, P, A, K>, [u8; 32]), SpaceTxnError> {
        let session = self.session();
        Ok((session.space.get_history_repository(), session.root))
    }

    fn apply_op(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        reads: Reads<'_, C, P, A, K>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // R4 step 1 (A6): the session must be open and not at a boundary. A
        // transaction cannot own a guard of the gate, so each operation takes
        // it and fails closed on contention.
        session.ensure_open()?;
        let _gate = session
            .gate
            .try_read()
            .map_err(|_| mismatch("the session is at a boundary"))?;
        let result = match operation {
            StagedOperation::Produce(produce) => {
                self.apply_produce(store, produce, before_observe, reads, events)
            }
            StagedOperation::Consume(consume) => {
                self.apply_consume(store, consume, before_observe, reads, events)
            }
        };
        // A failed operation leaves its journal row incomplete, and an operation
        // that waits for the row would wait forever. So the failure closes the
        // epoch: every waiting operation wakes and fails (Closed), and the
        // certification fails closed. A recorded denial publishes its row first,
        // and play goes on after it, so it keeps the epoch open.
        if result.is_err() && !self.denied.swap(false, Ordering::AcqRel) {
            session.epoch.invalidate();
        }
        result
    }
}

impl<O, C, P, A, K, E> NativeBackend<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
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
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + DeserializeOwned
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    fn session(&self) -> &NativeReplaySession<C, P, A, K, E> { (self.session)(&self.owner) }

    // R4 steps 4 and 5: the authority of the operation, which step 3
    // registered, and its journal row. A row that is not ready fails closed
    // (P1): the backend holds stripes and never waits.
    fn begin_row(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<(Authority<C, P, A, K, E>, E::Ticket), SpaceTxnError> {
        let authority = (self.authority)(source)?;
        let ticket = self
            .session()
            .epoch
            .begin_operation(source)?
            .ok_or_else(|| mismatch("the journal row of the operation is not ready"))?;
        Ok((authority, ticket))
    }

    // R5 (A11): a recorded denial is published with its outcome, and the
    // operation fails as play fails it.
    fn publish_denial(
        &self,
        ticket: E::Ticket,
        outcome: NativeReplayOutcome,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        let mut completion = ticket.prepare(outcome)?;
        let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
            session.epoch.invalidate()
        });
        completion.publish();
        publication.complete();
        self.denied.store(true, Ordering::Release);
        Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons))
    }

    // R4 step 9 (first part): the staged COMM is the COMM of the recorded
    // candidate. `observe_comm` checks it again.
    fn check_candidate(&self, ticket: &E::Ticket, comm: &COMM) -> Result<(), SpaceTxnError> {
        let session = self.session();
        let identity = ticket
            .candidate_identity()
            .ok_or_else(|| mismatch("a recorded fire has no candidate identity"))?;
        let meter =
            |operations, scanned, backing| session.history_reserve(operations, scanned, backing);
        match identity.metered_matches_comm(comm, &meter)? {
            true => Ok(()),
            false => Err(mismatch("the staged COMM differs from the recorded candidate")),
        }
    }

    fn apply_produce(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: StagedProduce<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        reads: Reads<'_, C, P, A, K>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // Step 2: the commit checks of play, with the metered reads.
        let host_counter = self.host_counter(&produce.source);
        let (produce, joins) = ready_produce(
            store,
            produce,
            host_counter,
            |produces| session.space.produce_counters(produces),
            reads,
        )?;
        let ReadyProduce {
            source: produce_ref,
            channel,
            data,
            persist,
            fire,
        } = produce;
        let source = RSpaceOperationSource::Produce(&produce_ref);
        // Steps 3 to 6.
        run_before_observe(before_observe, source)?;
        let (authority, mut ticket) = self.begin_row(source)?;
        ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        // Step 7.
        let outcome = ticket.outcome();
        let fits = match (&fire, outcome) {
            (None, NativeReplayOutcome::Stored | NativeReplayOutcome::DeniedIntroduction) => true,
            (
                Some(_),
                NativeReplayOutcome::Matched |
                NativeReplayOutcome::DeniedIntroduction |
                NativeReplayOutcome::DeniedComm,
            ) => true,
            _ => false,
        };
        if !fits {
            return Err(mismatch("the recorded outcome does not fit the staged produce"));
        }
        // Step 8.
        let decision = ticket.observe_produce(&produce_ref, &channel, &data, &authority)?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome);
        }
        require(decision, NativeReplayDecision::Granted)?;
        match fire {
            None => {
                // Steps 10 to 12.
                let counter =
                    session
                        .space
                        .prepare_metered_produce_counter(&produce_ref, persist, &free)?;
                let mut completion = ticket.prepare(NativeReplayOutcome::Stored)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                store_data(store, channel, data, persist, produce_ref.clone());
                counter.publish();
                completion.publish();
                publication.complete();
                events.push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
                Ok(SpaceOpReceipt::Stored {
                    produce: produce_ref,
                })
            }
            Some(fire) => {
                let ReadyFire {
                    fire:
                        StagedFire {
                            channels,
                            continuation,
                            candidates,
                            ..
                        },
                    comm,
                    continuation_index,
                    committed,
                } = fire;
                // Step 9.
                self.check_candidate(&ticket, &comm)?;
                let decision = ticket.observe_comm(
                    &comm,
                    &continuation.continuation,
                    continuation.persist,
                    &candidates,
                )?;
                if outcome == NativeReplayOutcome::DeniedComm {
                    require(decision, NativeReplayDecision::Denied)?;
                    return self.publish_denial(ticket, outcome);
                }
                require(decision, NativeReplayDecision::Granted)?;
                let returned = ticket.returned_produce(&produce_ref)?;
                // Steps 10 to 12. The incoming datum is never stored, as in
                // play (FR-10, decision 30).
                let counter =
                    session
                        .space
                        .prepare_metered_produce_counter(&produce_ref, persist, &free)?;
                let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                if let Some(index) = continuation_index {
                    store.remove_continuation(&channels, index);
                }
                session.space.mark_replay_waiting_continuation_match();
                remove_matched_datum_and_join(store, &channels, &committed);
                counter.publish();
                completion.publish();
                publication.complete();
                events.push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
                events.push(Event::Comm(comm.clone()));
                let consume_ref = continuation.source.clone();
                let fired = candidates.len();
                let wrapped =
                    session
                        .space
                        .wrap_result(channels, continuation, consume_ref, candidates);
                fired_receipt(comm, wrapped, fired, Some(returned))
            }
        }
    }

    fn apply_consume(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume: StagedConsume<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        reads: Reads<'_, C, P, A, K>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // Step 2.
        let consume = ready_consume(
            store,
            consume,
            |produces| session.space.produce_counters(produces),
            reads,
        )?;
        let ReadyConsume {
            source: consume_ref,
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            fire,
        } = consume;
        let source = RSpaceOperationSource::Consume(&consume_ref);
        // Steps 3 to 6. A consume authenticates its channels and no joins.
        run_before_observe(before_observe, source)?;
        let (authority, mut ticket) = self.begin_row(source)?;
        ticket.authenticate_footprint(&channels, &[])?;
        // Step 7.
        let outcome = ticket.outcome();
        let fits = match (&fire, outcome) {
            (None, NativeReplayOutcome::Stored | NativeReplayOutcome::DeniedIntroduction) => true,
            (
                Some(_),
                NativeReplayOutcome::Matched |
                NativeReplayOutcome::DeniedIntroduction |
                NativeReplayOutcome::DeniedComm,
            ) => true,
            _ => false,
        };
        if !fits {
            return Err(mismatch("the recorded outcome does not fit the staged consume"));
        }
        // Step 8.
        let decision = ticket.observe_consume(
            &consume_ref,
            &channels,
            &patterns,
            &continuation,
            &peeks,
            &authority,
        )?;
        if outcome == NativeReplayOutcome::DeniedIntroduction {
            require(decision, NativeReplayDecision::Denied)?;
            return self.publish_denial(ticket, outcome);
        }
        require(decision, NativeReplayDecision::Granted)?;
        let wk = WaitingContinuation {
            patterns,
            continuation,
            persist,
            peeks,
            source: consume_ref.clone(),
        };
        match fire {
            None => {
                // Steps 10 to 12.
                let mut completion = ticket.prepare(NativeReplayOutcome::Stored)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                if store_waiting_continuation(store, channels.clone(), wk) {
                    session.space.inc_replay_waiting_continuations(&channels);
                }
                completion.publish();
                publication.complete();
                events.push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
                Ok(SpaceOpReceipt::Parked {
                    consume: consume_ref,
                })
            }
            Some(ReadyConsumeFire {
                fire: StagedConsumeFire { candidates, .. },
                comm,
                committed,
            }) => {
                // Step 9.
                self.check_candidate(&ticket, &comm)?;
                let decision =
                    ticket.observe_comm(&comm, &wk.continuation, wk.persist, &candidates)?;
                if outcome == NativeReplayOutcome::DeniedComm {
                    require(decision, NativeReplayDecision::Denied)?;
                    return self.publish_denial(ticket, outcome);
                }
                require(decision, NativeReplayDecision::Granted)?;
                // Steps 10 to 12.
                let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                store_persistent_data(store, &committed);
                completion.publish();
                publication.complete();
                events.push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
                events.push(Event::Comm(comm.clone()));
                let fired = candidates.len();
                let wrapped = session
                    .space
                    .wrap_result(channels, wk, consume_ref, candidates);
                fired_receipt(comm, wrapped, fired, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::rspace::hashing::native_source::SourceMeter;
    use crate::rspace::internal::{ConsumeCandidate, Datum, Install};
    use crate::rspace::replay_rspace::native_epoch::{
        NativeCandidateIdentity, NativeReplayBoundary, NativeReplayEpoch, NativeReplayRestore,
    };
    use crate::rspace::rspace::RSpace;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use crate::rspace::space_store_handle::{
        ConsumeOutcome, NativeViewReads, ProduceOutcome, SharedReads, SpaceConsume, SpaceProduce,
        SpaceStoreHandle, SpaceTxnReceipt, bind_candidate, incoming_candidate,
    };
    use crate::rspace::trace::event::Consume;

    // One recorded journal row: its outcome, the decisions of its introduction and
    // of its COMM, and the COMM of its candidate.
    #[derive(Clone)]
    struct Row {
        outcome: NativeReplayOutcome,
        introduction: NativeReplayDecision,
        comm: NativeReplayDecision,
        candidate: Option<COMM>,
    }

    impl Row {
        fn stored() -> Self {
            Self {
                outcome: NativeReplayOutcome::Stored,
                introduction: NativeReplayDecision::Granted,
                comm: NativeReplayDecision::Granted,
                candidate: None,
            }
        }

        fn matched(candidate: Option<COMM>) -> Self {
            Self {
                outcome: NativeReplayOutcome::Matched,
                candidate,
                ..Self::stored()
            }
        }

        fn denied_introduction() -> Self {
            Self {
                outcome: NativeReplayOutcome::DeniedIntroduction,
                introduction: NativeReplayDecision::Denied,
                ..Self::stored()
            }
        }

        fn denied_comm(candidate: COMM) -> Self {
            Self {
                outcome: NativeReplayOutcome::DeniedComm,
                comm: NativeReplayDecision::Denied,
                candidate: Some(candidate),
                ..Self::stored()
            }
        }
    }

    // A scripted epoch: each operation begins the next recorded row, and the script
    // logs every call of the protocol in order.
    #[derive(Clone, Default)]
    struct Script {
        rows: Arc<Mutex<VecDeque<Row>>>,
        log: Arc<Mutex<Vec<String>>>,
        not_ready: Arc<AtomicBool>,
        work: Arc<AtomicUsize>,
    }

    impl Script {
        fn push(&self, row: Row) { self.rows.lock().expect("rows lock").push_back(row); }

        fn note(&self, entry: impl Into<String>) {
            self.log.lock().expect("log lock").push(entry.into());
        }

        fn log(&self) -> Vec<String> { self.log.lock().expect("log lock").clone() }

        fn clear(&self) { self.log.lock().expect("log lock").clear(); }
    }

    struct Boundary;

    struct Restore;

    impl NativeReplayEpoch for Script {
        type Boundary = Boundary;

        fn begin_boundary(&self) -> Result<Boundary, RSpaceError> { Ok(Boundary) }

        fn reserve_work(&self, operations: usize, _bytes: usize) -> Result<(), RSpaceError> {
            self.work.fetch_add(operations, Ordering::Relaxed);
            Ok(())
        }

        fn reserve_comparison(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError> {
            self.reserve_work(operations, bytes)
        }

        fn invalidate(&self) { self.note("invalidate"); }
    }

    impl NativeReplayBoundary for Boundary {
        type Checkpoint = ();
        type Evidence = u64;
        type Restore = Restore;

        fn checkpoint(&self) -> Self::Checkpoint {}

        fn prepare_restore(self, _checkpoint: &()) -> Result<Restore, RSpaceError> { Ok(Restore) }

        fn check_complete(&self) -> Result<(), RSpaceError> { Ok(()) }

        fn completed_usage(&self) -> Result<u64, RSpaceError> { Ok(0) }

        fn completed_evidence(&self) -> Result<u64, RSpaceError> { Ok(0) }

        fn close(self) {}
    }

    impl NativeReplayRestore for Restore {
        fn publish(self) {}
    }

    struct Identity(Option<COMM>);

    impl NativeCandidateIdentity for Identity {
        fn matches_consume(&self, source: &Consume) -> bool {
            self.0.as_ref().is_some_and(|comm| comm.consume == *source)
        }

        fn matches_produce(&self, source: &Produce) -> bool {
            self.0
                .as_ref()
                .is_some_and(|comm| comm.produces.contains(source))
        }

        fn repetition(&self, source: &Produce) -> Option<i32> {
            self.0
                .as_ref()
                .and_then(|comm| comm.times_repeated.get(source).copied())
        }

        fn matches_comm(&self, source: &COMM) -> bool { self.0.as_ref() == Some(source) }

        fn metered_matches_consume(
            &self,
            source: &Consume,
            _meter: &dyn SourceMeter,
        ) -> Result<bool, RSpaceError> {
            Ok(self.matches_consume(source))
        }

        fn metered_matches_produce(
            &self,
            source: &Produce,
            _meter: &dyn SourceMeter,
        ) -> Result<bool, RSpaceError> {
            Ok(self.matches_produce(source))
        }

        fn metered_repetition(
            &self,
            source: &Produce,
            _meter: &dyn SourceMeter,
        ) -> Result<Option<i32>, RSpaceError> {
            Ok(self.repetition(source))
        }

        fn metered_matches_comm(
            &self,
            source: &COMM,
            meter: &dyn SourceMeter,
        ) -> Result<bool, RSpaceError> {
            meter.reserve(1, 0, 0)?;
            Ok(self.matches_comm(source))
        }
    }

    struct Ticket {
        row: Row,
        identity: Identity,
        script: Script,
    }

    struct Publication(Script);

    impl NativeOperationPublication for Publication {
        fn publish(&mut self) { self.0.note("publish"); }
    }

    impl NativeOperationTicket<String, String, String, String> for Ticket {
        type Authority = ();
        type Publication = Publication;

        fn outcome(&self) -> NativeReplayOutcome { self.row.outcome }

        fn candidate_identity(&self) -> Option<&dyn NativeCandidateIdentity> {
            self.identity
                .0
                .as_ref()
                .map(|_| &self.identity as &dyn NativeCandidateIdentity)
        }

        fn authenticate_footprint(
            &mut self,
            channels: &[String],
            joins: &[Vec<String>],
        ) -> Result<(), RSpaceError> {
            self.script
                .note(format!("footprint {channels:?} {joins:?}"));
            Ok(())
        }

        fn observe_produce(
            &mut self,
            _source: &Produce,
            channel: &String,
            _data: &String,
            _authority: &(),
        ) -> Result<NativeReplayDecision, RSpaceError> {
            self.script.note(format!("observe_produce {channel}"));
            Ok(self.row.introduction)
        }

        fn observe_consume(
            &mut self,
            _source: &Consume,
            channels: &[String],
            _patterns: &[String],
            _continuation: &String,
            _peeks: &BTreeSet<i32>,
            _authority: &(),
        ) -> Result<NativeReplayDecision, RSpaceError> {
            self.script.note(format!("observe_consume {channels:?}"));
            Ok(self.row.introduction)
        }

        fn observe_comm(
            &mut self,
            _source: &COMM,
            _continuation: &String,
            _persistent: bool,
            _data: &[ConsumeCandidate<String, String>],
        ) -> Result<NativeReplayDecision, RSpaceError> {
            self.script.note("observe_comm");
            Ok(self.row.comm)
        }

        fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError> {
            Ok(source.clone())
        }

        fn prepare(self, outcome: NativeReplayOutcome) -> Result<Publication, RSpaceError> {
            if outcome != self.row.outcome {
                return Err(RSpaceError::BugFoundError(format!(
                    "prepared {outcome:?}, recorded {:?}",
                    self.row.outcome
                )));
            }
            self.script.note(format!("prepare {outcome:?}"));
            Ok(Publication(self.script))
        }
    }

    #[async_trait]
    impl NativeOperationEpoch<String, String, String, String> for Script {
        type Ticket = Ticket;

        fn prepare_produce_source(
            &self,
            _channel: &String,
            _data: &String,
        ) -> Result<(), RSpaceError> {
            self.note("prepare_produce_source");
            Ok(())
        }

        fn prepare_consume_source(
            &self,
            _channels: &[String],
            _patterns: &[String],
            _continuation: &String,
        ) -> Result<(), RSpaceError> {
            self.note("prepare_consume_source");
            Ok(())
        }

        async fn wait_ready(&self, _source: RSpaceOperationSource<'_>) -> Result<(), RSpaceError> {
            self.note("wait_ready");
            Ok(())
        }

        fn begin_operation(
            &self,
            source: RSpaceOperationSource<'_>,
        ) -> Result<Option<Ticket>, RSpaceError> {
            let kind = match source {
                RSpaceOperationSource::Produce(_) => "produce",
                RSpaceOperationSource::Consume(_) => "consume",
            };
            self.note(format!("begin {kind}"));
            if self.not_ready.load(Ordering::Acquire) {
                return Ok(None);
            }
            let row = self
                .rows
                .lock()
                .expect("rows lock")
                .pop_front()
                .ok_or_else(|| RSpaceError::BugFoundError("no recorded row".to_owned()))?;
            Ok(Some(Ticket {
                identity: Identity(row.candidate.clone()),
                row,
                script: self.clone(),
            }))
        }
    }

    struct AnyMatch;

    impl Match<String, String, String> for AnyMatch {
        fn get(&self, _pattern: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
    }

    type Session = NativeReplaySession<String, String, String, String, Script>;
    type Handle = SpaceStoreHandle<String, String, String, String>;

    fn s(value: &str) -> String { value.to_string() }

    async fn session(
        script: &Script,
        installs: Vec<(Vec<String>, Install<String, String>)>,
    ) -> Arc<Session> {
        let mut stores = InMemoryStoreManager::new();
        let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("an in-memory play and replay space");
        Arc::new(
            Session::new_with_installs(
                play.get_history_repository(),
                Arc::new(Box::new(AnyMatch)),
                script.clone(),
                installs,
            )
            .expect("a native replay session"),
        )
    }

    fn handle(session: &Arc<Session>) -> Handle {
        let host = NativeSpaceHost::new(
            Arc::clone(session),
            |session: &Session| session,
            Arc::new(|_source: RSpaceOperationSource<'_>| Ok(())),
        )
        .expect("the native space host");
        SpaceStoreHandle::new(host)
    }

    fn reads() -> SharedReads<String, String, String, String> {
        Arc::new(NativeViewReads::new(|_operations: usize, _scanned: usize, _backing: usize| {
            Ok::<(), RSpaceError>(())
        }))
    }

    fn store_op(channel: &str, data: &str) -> SpaceProduce<String, String, String, String> {
        SpaceProduce {
            channel: s(channel),
            data: s(data),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
        }
    }

    fn park_op(channel: &str, continuation: &str) -> SpaceConsume<String, String, String, String> {
        SpaceConsume {
            channels: vec![s(channel)],
            patterns: vec![s("_")],
            continuation: s(continuation),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
        }
    }

    fn fire_op(
        channel: &str,
        data: &str,
        waiting: &WaitingContinuation<String, String>,
    ) -> SpaceProduce<String, String, String, String> {
        SpaceProduce {
            channel: s(channel),
            data: s(data),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: vec![s(channel)],
                continuation: waiting.clone(),
                data: vec![incoming_candidate(
                    s(channel),
                    Produce::create(&s(channel), &s(data), false),
                    s(data),
                    false,
                    s(data),
                )],
            },
            before_observe: None,
        }
    }

    fn take_op(
        channel: &str,
        stored: &Datum<String>,
        index: i32,
    ) -> SpaceConsume<String, String, String, String> {
        SpaceConsume {
            channels: vec![s(channel)],
            patterns: vec![s("_")],
            continuation: s("taker"),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Fire {
                data: vec![bind_candidate(s(channel), stored, stored.a.clone(), index)],
            },
            before_observe: None,
        }
    }

    // Parks `k` on `b` through one recorded row.
    async fn park_k(script: &Script, handle: &Handle) {
        script.push(Row::stored());
        let mut txn = handle
            .begin_metered(&[s("b")], reads())
            .await
            .expect("begin");
        txn.consume(park_op("b", "k")).expect("stage the park");
        txn.commit().await.expect("commit the park");
    }

    // R4: each staged operation meets one recorded row in the order of the
    // protocol, and the effects and the produce counter of play follow. The
    // backend never waits for readiness.
    #[tokio::test]
    async fn native_space_replay_stores_and_parks_through_the_journal() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        script.push(Row::stored());
        let mut txn = handle
            .begin_metered(&[s("a"), s("b")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.consume(park_op("b", "k")).expect("stage the park");
        let receipt = txn.commit().await.expect("commit");
        assert!(matches!(receipt.ops[0], SpaceOpReceipt::Stored { .. }));
        assert!(matches!(receipt.ops[1], SpaceOpReceipt::Parked { .. }));
        assert_eq!(script.log(), vec![
            "begin produce",
            "footprint [\"a\"] []",
            "observe_produce a",
            "prepare Stored",
            "publish",
            "begin consume",
            "footprint [\"b\"] []",
            "observe_consume [\"b\"]",
            "prepare Stored",
            "publish",
        ]);
        let state = session.space.get_store().snapshot();
        assert_eq!(state.data_flat().get(&s("a")).map(Vec::len), Some(1));
        assert_eq!(state.continuations_flat().get(&vec![s("b")]).map(Vec::len), Some(1));
        let x = Produce::create(&s("a"), &s("x"), false);
        assert_eq!(session.space.get_produce_count(&x), 1);
    }

    // R4 step 9: a fire checks its staged COMM against the recorded candidate, then
    // observes the COMM, and removes the fired continuation.
    #[tokio::test]
    async fn native_space_replay_fires_against_the_recorded_candidate() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        park_k(&script, &handle).await;
        script.clear();
        let mut txn = handle
            .begin_metered(&[s("b")], reads())
            .await
            .expect("begin");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let staged = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        script.push(Row::matched(staged.comm.clone()));
        let receipt = txn.commit().await.expect("commit the fire");
        assert!(matches!(receipt.ops[0], SpaceOpReceipt::Fired { .. }));
        assert_eq!(script.log(), vec![
            "begin produce",
            "footprint [\"b\"] [[\"b\"]]",
            "observe_produce b",
            "observe_comm",
            "prepare Matched",
            "publish",
        ]);
        let state = session.space.get_store().snapshot();
        assert!(
            state
                .continuations_flat()
                .get(&vec![s("b")])
                .is_none_or(Vec::is_empty)
        );
        let z = Produce::create(&s("b"), &s("z"), false);
        assert_eq!(session.space.get_produce_count(&z), 1);
    }

    // Negative control: a recorded candidate whose COMM differs from the staged
    // COMM fails the operation closed, before its COMM is observed and before
    // any effect.
    #[tokio::test]
    async fn native_space_replay_rejects_a_tampered_candidate() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        park_k(&script, &handle).await;
        script.clear();
        let mut txn = handle
            .begin_metered(&[s("b")], reads())
            .await
            .expect("begin");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let staged = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        let mut tampered = staged.comm.clone().expect("a fire has a COMM");
        tampered
            .times_repeated
            .insert(Produce::create(&s("q"), &s("q"), false), 7);
        script.push(Row::matched(Some(tampered)));
        let error = txn.commit().await.expect_err("a tampered candidate fails");
        assert_eq!(error, mismatch("the staged COMM differs from the recorded candidate"));
        assert!(!script.log().contains(&"observe_comm".to_owned()));
        assert!(!script.log().contains(&"publish".to_owned()));
        assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        let state = session.space.get_store().snapshot();
        assert_eq!(state.continuations_flat().get(&vec![s("b")]).map(Vec::len), Some(1));
    }

    // Negative control: a recorded outcome that does not fit the staged kind fails
    // closed before any observation and any effect.
    #[tokio::test]
    async fn native_space_replay_rejects_an_outcome_that_does_not_fit() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::matched(None));
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let error = txn.commit().await.expect_err("a misfit outcome fails");
        assert_eq!(error, mismatch("the recorded outcome does not fit the staged produce"));
        assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        assert!(
            !script
                .log()
                .iter()
                .any(|entry| entry.starts_with("observe"))
        );
        // The metered view load caches the empty history value of `a`. No record is
        // stored.
        assert!(
            session
                .space
                .get_store()
                .snapshot()
                .data_flat()
                .get(&s("a"))
                .is_none_or(Vec::is_empty)
        );
    }

    // R5: a recorded denial is published with its outcome, and the operation fails
    // with the error of play. The operations before it stay applied (decision
    // 19, D6).
    #[tokio::test]
    async fn native_space_replay_publishes_a_recorded_denial() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        script.push(Row::denied_introduction());
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        let error = txn
            .commit()
            .await
            .expect_err("the denial fails the operation");
        assert_eq!(error, SpaceTxnError::Host(RSpaceError::OutOfPhlogistons));
        let log = script.log();
        assert_eq!(&log[5..], &[
            "begin produce",
            "footprint [\"a\"] []",
            "observe_produce a",
            "prepare DeniedIntroduction",
            "publish",
        ]);
        let state = session.space.get_store().snapshot();
        assert_eq!(
            state.data_flat().get(&s("a")).map(|records| records
                .iter()
                .map(|datum| datum.a.clone())
                .collect::<Vec<_>>()),
            Some(vec![s("x")])
        );
        assert!(
            !script.log().contains(&"invalidate".to_owned()),
            "a recorded denial keeps the epoch open"
        );
    }

    // R5 for a COMM: a recorded COMM denial is published after the granted
    // introduction.
    #[tokio::test]
    async fn native_space_replay_publishes_a_recorded_comm_denial() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        park_k(&script, &handle).await;
        script.clear();
        let mut txn = handle
            .begin_metered(&[s("b")], reads())
            .await
            .expect("begin");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let staged = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        script.push(Row::denied_comm(staged.comm.clone().expect("a fire has a COMM")));
        let error = txn.commit().await.expect_err("the denial fails the fire");
        assert_eq!(error, SpaceTxnError::Host(RSpaceError::OutOfPhlogistons));
        assert_eq!(script.log(), vec![
            "begin produce",
            "footprint [\"b\"] [[\"b\"]]",
            "observe_produce b",
            "observe_comm",
            "prepare DeniedComm",
            "publish",
        ]);
        let state = session.space.get_store().snapshot();
        assert_eq!(state.continuations_flat().get(&vec![s("b")]).map(Vec::len), Some(1));
    }

    // P1: a row that is not ready fails the operation closed. The backend holds
    // stripes, so it never waits, and the test ends. The failure closes the
    // epoch.
    #[tokio::test]
    async fn native_space_replay_fails_closed_on_a_row_that_is_not_ready() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.not_ready.store(true, Ordering::Release);
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let error = txn.commit().await.expect_err("an unready row fails");
        assert_eq!(error, mismatch("the journal row of the operation is not ready"));
        // The failure closes the epoch, so an operation that waits for the row
        // wakes and fails.
        assert_eq!(script.log(), vec!["begin produce", "invalidate"]);
        // The metered view load caches the empty history value of `a`. No record is
        // stored.
        assert!(
            session
                .space
                .get_store()
                .snapshot()
                .data_flat()
                .get(&s("a"))
                .is_none_or(Vec::is_empty)
        );
    }

    // R3: native replay reads through metered reads only.
    #[tokio::test]
    async fn native_space_replay_requires_metered_reads() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        let error = handle
            .begin(&[s("a")])
            .await
            .err()
            .expect("a plain begin fails");
        assert_eq!(
            error,
            SpaceTxnError::InvalidOperation(
                "a native replay transaction reads through metered reads"
            )
        );
    }

    // R2 (A4): the installed channels of the session are guarded, as in play.
    #[tokio::test]
    async fn native_space_replay_keeps_the_installed_guard() {
        let script = Script::default();
        let install = Install {
            patterns: vec![s("_")],
            continuation: s("system"),
        };
        let session = session(&script, vec![(vec![s("sys")], install)]).await;
        let handle = handle(&session);
        let mut txn = handle
            .begin_metered(&[s("sys")], reads())
            .await
            .expect("begin");
        assert_eq!(txn.data(&s("sys")).err(), Some(SpaceTxnError::InstalledChannel));
    }

    // S1: native replay stages as play: it pairs nothing and admits every
    // candidate.
    #[tokio::test]
    async fn native_space_replay_stages_as_play() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        park_k(&script, &handle).await;
        let mut txn = handle
            .begin_metered(&[s("b")], reads())
            .await
            .expect("begin");
        assert_eq!(txn.mode(), SpaceMode::NativeReplay);
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let staged = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        let comm = staged.comm.expect("a fire has a COMM");
        let z = Produce::create(&s("b"), &s("z"), false);
        assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&z)), None);
        let incoming = Datum {
            a: s("z"),
            persist: false,
            source: z,
        };
        assert_eq!(txn.admits(&comm, &incoming, true), Ok(true));
    }

    // A6: at a boundary of the session an operation fails closed instead of
    // waiting.
    #[tokio::test]
    async fn native_space_replay_fails_closed_at_a_boundary() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let exclusive = session.gate.write().await;
        let error = txn
            .commit()
            .await
            .expect_err("a boundary fails the operation");
        drop(exclusive);
        assert_eq!(error, mismatch("the session is at a boundary"));
        assert!(script.log().is_empty());
    }

    // The committed state of a plan in play or in native replay.
    type Outcome = (
        Vec<SpaceTxnReceipt<String, String, String, String>>,
        std::collections::HashMap<String, Vec<Datum<String>>>,
        std::collections::HashMap<Vec<String>, Vec<WaitingContinuation<String, String>>>,
        std::collections::HashMap<String, Vec<Vec<String>>>,
    );

    // Runs one plan: store x and y on a and park k on b, then take x from a and
    // fire k with z. `script` is the native replay script, or `None` for play.
    // Native replay records the row of each operation as play commits it.
    async fn run_plan(
        handle: &Handle,
        script: Option<&Script>,
    ) -> Vec<SpaceTxnReceipt<String, String, String, String>> {
        let first = run_first(handle, script, reads()).await;
        let second = run_second(handle, script, reads()).await;
        vec![first, second]
    }

    // The first transaction of the plan, through `reads`: store x and y on a and
    // park k on b.
    async fn run_first(
        handle: &Handle,
        script: Option<&Script>,
        reads: SharedReads<String, String, String, String>,
    ) -> SpaceTxnReceipt<String, String, String, String> {
        if let Some(script) = script {
            for _ in 0..3 {
                script.push(Row::stored());
            }
        }
        let mut txn = handle
            .begin_metered(&[s("a"), s("b")], reads)
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        txn.consume(park_op("b", "k")).expect("stage the park");
        txn.commit().await.expect("commit the stores and the park")
    }

    // The second transaction of the plan, through `reads`: take x from a and fire k
    // with z.
    async fn run_second(
        handle: &Handle,
        script: Option<&Script>,
        reads: SharedReads<String, String, String, String>,
    ) -> SpaceTxnReceipt<String, String, String, String> {
        let keys = [s("a"), s("b")];
        let mut txn = handle.begin_metered(&keys, reads).await.expect("begin");
        let (index, stored) = txn
            .data(&s("a"))
            .expect("read a")
            .iter()
            .cloned()
            .enumerate()
            .find(|(_, datum)| datum.a == "x")
            .expect("x is stored");
        let take = txn
            .consume(take_op("a", &stored, i32::try_from(index).expect("a small index")))
            .expect("stage the take");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let fire = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        if let Some(script) = script {
            script.push(Row::matched(take.comm.clone()));
            script.push(Row::matched(fire.comm.clone()));
        }
        txn.commit().await.expect("commit the take and the fire")
    }

    fn state(
        store: SharedStore<String, String, String, String>,
    ) -> (
        std::collections::HashMap<String, Vec<Datum<String>>>,
        std::collections::HashMap<Vec<String>, Vec<WaitingContinuation<String, String>>>,
        std::collections::HashMap<String, Vec<Vec<String>>>,
    ) {
        let snapshot = store.snapshot();
        (snapshot.data_flat(), snapshot.continuations_flat(), snapshot.joins_flat())
    }

    // H24 by construction (S2): native replay commits the receipts, the store state
    // and the produce counters of play.
    #[tokio::test]
    async fn native_space_replay_effects_equal_play() {
        let mut stores = InMemoryStoreManager::new();
        let play = RSpace::<String, String, String, String>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("the play space");
        let play_handle = SpaceStoreHandle::new(play.clone());
        let played = run_plan(&play_handle, None).await;
        let play_state = state(play.get_store());

        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let native_handle = handle(&session);
        let replayed = run_plan(&native_handle, Some(&script)).await;
        let replay_state = state(session.space.get_store());

        let outcome: Outcome = (played, play_state.0, play_state.1, play_state.2);
        let native: Outcome = (replayed, replay_state.0, replay_state.1, replay_state.2);
        assert_eq!(outcome, native);
        let produces: Vec<Produce> = [("a", "x"), ("a", "y"), ("b", "z")]
            .into_iter()
            .map(|(channel, data)| Produce::create(&s(channel), &s(data), false))
            .collect();
        let mut counters = Vec::with_capacity(2);
        for handle in [&play_handle, &native_handle] {
            let mut txn = handle
                .begin_metered(&[s("a"), s("b")], reads())
                .await
                .expect("begin");
            counters.push(txn.produce_counters(&produces).expect("read the counters"));
        }
        assert_eq!(counters[0], counters[1]);
        assert_eq!(counters[1].values().copied().collect::<Vec<_>>(), vec![1, 1, 1]);
        assert!(!script.log().contains(&"wait_ready".to_owned()));
    }

    // The view reservations of metered reads, in order: (operations, scanned,
    // backing).
    #[derive(Clone, Default)]
    struct ChargeLog(Arc<Mutex<Vec<(usize, usize, usize)>>>);

    impl ChargeLog {
        fn reads(&self) -> SharedReads<String, String, String, String> {
            let log = self.clone();
            Arc::new(NativeViewReads::new(
                move |operations: usize, scanned: usize, backing: usize| {
                    log.0
                        .lock()
                        .expect("the charge log")
                        .push((operations, scanned, backing));
                    Ok::<(), RSpaceError>(())
                },
            ))
        }

        fn entries(&self) -> Vec<(usize, usize, usize)> {
            self.0.lock().expect("the charge log").clone()
        }
    }

    // The native replay session of `script` over `history`, at its current root.
    fn session_on(
        script: &Script,
        history: SharedHistory<String, String, String, String>,
    ) -> Arc<Session> {
        Arc::new(
            Session::new_with_installs(
                history,
                Arc::new(Box::new(AnyMatch)),
                script.clone(),
                Vec::new(),
            )
            .expect("a native replay session"),
        )
    }

    // P5 (decision 47, I-12): the metered reads of native replay reserve the view
    // loads of play, in the order of play, and a warm load reserves what a cold
    // load reserves. Play and native replay read the records of the first
    // transaction cold from the history. A second play reads them warm from its
    // hot store.
    #[tokio::test]
    async fn native_space_replay_view_charges_equal_play() {
        let mut stores = InMemoryStoreManager::new();
        let cold = RSpace::<String, String, String, String>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("the cold play space");
        let cold_handle = SpaceStoreHandle::new(cold.clone());
        run_first(&cold_handle, None, reads()).await;
        cold.create_checkpoint().await.expect("the checkpoint");
        let history = cold.get_history_repository();
        let cold_charges = ChargeLog::default();
        let cold_receipt = run_second(&cold_handle, None, cold_charges.reads()).await;

        let script = Script::default();
        let session = session_on(&script, history);
        let native_handle = handle(&session);
        let native_charges = ChargeLog::default();
        let native_receipt =
            run_second(&native_handle, Some(&script), native_charges.reads()).await;

        let mut stores = InMemoryStoreManager::new();
        let warm = RSpace::<String, String, String, String>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("the warm play space");
        let warm_handle = SpaceStoreHandle::new(warm);
        run_first(&warm_handle, None, reads()).await;
        let warm_charges = ChargeLog::default();
        run_second(&warm_handle, None, warm_charges.reads()).await;

        assert!(!cold_charges.entries().is_empty(), "the metered reads reserve the view loads");
        assert_eq!(native_charges.entries(), cold_charges.entries());
        assert_eq!(warm_charges.entries(), cold_charges.entries());
        assert_eq!(native_receipt, cold_receipt);
    }

    // Negative control for a backend that searches the candidates again (design
    // v2, section 6, the forced DR-75 search): x and y both fit the take, and the
    // recorded row names the COMM of the take of y. The staged take of x fails
    // closed, although a search would accept either candidate.
    #[tokio::test]
    async fn native_space_replay_rejects_another_valid_candidate() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        for _ in 0..2 {
            script.push(Row::stored());
        }
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        txn.commit().await.expect("commit the stores");
        script.clear();

        // The COMM of the take of y, from a transaction that never commits.
        let other = {
            let mut txn = handle
                .begin_metered(&[s("a")], reads())
                .await
                .expect("begin");
            let (index, stored) = txn
                .data(&s("a"))
                .expect("read a")
                .iter()
                .cloned()
                .enumerate()
                .find(|(_, datum)| datum.a == "y")
                .expect("y is stored");
            let take = take_op("a", &stored, i32::try_from(index).expect("a small index"));
            txn.consume(take)
                .expect("stage the take of y")
                .comm
                .expect("a take has a COMM")
        };
        let mut txn = handle
            .begin_metered(&[s("a")], reads())
            .await
            .expect("begin");
        let (index, stored) = txn
            .data(&s("a"))
            .expect("read a")
            .iter()
            .cloned()
            .enumerate()
            .find(|(_, datum)| datum.a == "x")
            .expect("x is stored");
        let take = take_op("a", &stored, i32::try_from(index).expect("a small index"));
        let staged = txn.consume(take).expect("stage the take of x");
        assert_ne!(staged.comm.as_ref(), Some(&other), "the takes have two COMMs");
        script.push(Row::matched(Some(other)));
        let error = txn.commit().await.expect_err("another candidate fails");
        assert_eq!(error, mismatch("the staged COMM differs from the recorded candidate"));
        assert!(!script.log().contains(&"observe_comm".to_owned()));
        assert!(!script.log().contains(&"publish".to_owned()));
        assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        let state = session.space.get_store().snapshot();
        assert_eq!(state.data_flat().get(&s("a")).map(Vec::len), Some(2));
    }
}
