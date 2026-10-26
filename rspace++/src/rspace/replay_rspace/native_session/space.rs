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
// A transaction reads and changes the digest-keyed store of the session
// (DR-96) through `NativeSpaceStore`, so a space operation and a host
// operation of native replay share one store, as they share the hot store in
// play. The effects are the store steps of native replay of a host operation
// (`operations.rs`): `put_datum`, `store_consume`, `retire_produce_match` and
// `retire_data`. The oracle tests of `native_store.rs` show that these steps
// change the store as the steps of play (`store_data` and the others of
// `space_store_handle.rs`) change the hot store. So the export of the session
// holds the effects of the space operations, and native replay reaches the
// root of play.
//
// The session logs no event. The backend gives the events of play to the
// event correlation check of `commit_operation` only. A non-persistent
// produce advances the produce counter of the session, which `host_counter`
// reads, as `log_produce` advances the counter of play. A space operation
// meters only what its host operation meters, through the observations of
// the ticket: its COMMs and the bytes that it stores (DR-131).

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{NativeReplaySession, PublicationGuard, result};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{GroupKeys, StoreKey};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::replay_rspace::native_epoch::{
    NativeOperationEpoch, NativeOperationPublication, NativeOperationTicket, NativeReplayDecision,
    NativeReplayOutcome,
};
use crate::rspace::rspace_interface::RSpaceOperationSource;
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::space_store_handle::{
    BeforeObserve, BoxedSpaceTxn, HostTxn, LockCoverage, ReadyConsume, ReadyConsumeFire, ReadyFire,
    ReadyProduce, SpaceMode, SpaceOpReceipt, SpaceTxnError, SpaceTxnHost, StagedConsume,
    StagedConsumeFire, StagedFire, StagedOperation, StagedProduce, SweepReport, TxnBackend,
    TxnRegistration, TxnStore, check_swept, fired_receipt, plan_sweep, ready_consume,
    ready_produce, root_leaf_records, run_before_observe, sweep_channels, swept_join_count,
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

/// The digest-keyed store of a native replay session (DR-96) as the store of
/// a space transaction. A read takes the value from the store, and a value
/// that the store does not hold from the history at the root of the session,
/// with the cold fills of the session. The session sits inside its owner by
/// value, so the store keeps the owner and a projection to the session.
pub(crate) struct NativeSpaceStore<O, C, P, A, K, E>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    E: NativeOperationEpoch<C, P, A, K>,
{
    owner: Arc<O>,
    session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
}

impl<O, C, P, A, K, E> NativeSpaceStore<O, C, P, A, K, E>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    E: NativeOperationEpoch<C, P, A, K>,
{
    fn session(&self) -> &NativeReplaySession<C, P, A, K, E> { (self.session)(&self.owner) }
}

/// The store keys of a group from the store identities of its channels, in
/// group order. A store identity is the digest that keys the channel in the
/// session store, so no channel is hashed again.
fn group_keys(keys: &[Blake2b256Hash]) -> Result<GroupKeys, RSpaceError> {
    let mut channels = Vec::with_capacity(keys.len());
    for key in keys {
        channels.push(StoreKey::from_digest(key)?);
    }
    Ok(GroupKeys::from_channel_keys(channels))
}

impl<O, C, P, A, K, E> TxnStore<C, P, A, K> for NativeSpaceStore<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
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
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    fn data(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        Ok(session.read_data_with(channel, StoreKey::from_digest(key)?)?)
    }

    fn continuations(
        &self,
        channels: &[C],
        keys: &[Blake2b256Hash],
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        Ok(session.read_continuation_views(channels, &group_keys(keys)?)?)
    }

    fn joins(&self, channel: &C, key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        Ok(session.read_joins(channel, StoreKey::from_digest(key)?)?)
    }
}

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
    store: Arc<NativeSpaceStore<O, C, P, A, K, E>>,
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

impl<O, C, P, A, K, E> NativeSpaceHost<O, C, P, A, K, E>
where
    O: Send + Sync + 'static,
    C: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
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
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    /// The host of the session that `session` projects out of `owner`.
    /// `installed` holds the keys of the channels of the system processes
    /// that the session installs (R2). The caller takes them from the
    /// installations of the session (`installed_channel_keys`), as play
    /// takes them from its installs. Every transaction and every sweep of
    /// the host shares one store, so the store identity of a commit is
    /// stable.
    pub fn new(
        owner: Arc<O>,
        session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
        authority: AuthorityResolver<C, P, A, K, E>,
        installed: BTreeSet<Blake2b256Hash>,
    ) -> Self {
        let store = Arc::new(NativeSpaceStore {
            owner: Arc::clone(&owner),
            session,
        });
        Self {
            owner,
            session,
            authority,
            installed,
            store,
        }
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
        + DeserializeOwned
        + ColdStoreDecode
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
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    fn mode(&self) -> SpaceMode { SpaceMode::NativeReplay }

    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { Arc::clone(&self.session().space.matcher) }

    // The stripes are taken as play takes them. Changed by decision 89
    // (DR-131): R3 refused a transaction without the metered reads, and the
    // metered reads are removed with host-work metering, so a native replay
    // transaction reads as a transaction of play reads.
    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        if lock_keys.is_empty() {
            return Err(SpaceTxnError::InvalidOperation("lock keys must not be empty"));
        }
        let hashes: Vec<u64> = lock_keys.iter().map(striped_locks::channel_hash).collect();
        let coverage = LockCoverage::new(
            &hashes,
            session.space.phase_a_locks.len(),
            session.space.phase_b_locks.len(),
        );
        let locks = session.consume_lock(&hashes).await?;
        let backend = NativeBackend {
            owner: Arc::clone(&self.owner),
            session: self.session,
            authority: Arc::clone(&self.authority),
            store: Arc::clone(&self.store),
            denied: AtomicBool::new(false),
        };
        Ok(Box::new(HostTxn::new(
            SpaceMode::NativeReplay,
            backend,
            Arc::clone(&self.store),
            self.installed.clone(),
            coverage,
            locks,
            registration,
        )))
    }

    // R7.
    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        let repository = self.session().space.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        root_leaf_records(reader.as_ref(), channel)
    }

    // R6: the sweep of play over the session store (DR-96), with the
    // installed channels of R2. The validation of `plan_sweep` runs first.
    // Then each continuation of a swept group leaves with
    // `retire_continuation`, the highest index first, and the last one of a
    // group takes the join of the group with it. Then each swept channel
    // loses its data. The residue check of play closes the sweep.
    async fn sweep_volatile(
        &self,
        channels: &[C],
        _registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        let channels = sweep_channels(channels)?;
        let hashes: Vec<u64> = channels.values().map(striped_locks::channel_hash).collect();
        let _guards = session.consume_lock(&hashes).await?;
        let _gate = session
            .gate
            .try_read()
            .map_err(|_| mismatch("the session is at a boundary"))?;
        let repository = session.space.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        let store = &*self.store;
        let groups = plan_sweep(store, reader.as_ref(), &self.installed, &channels)?;
        let mut report = SweepReport {
            channels: channels.len(),
            ..SweepReport::default()
        };
        let joins_before = swept_join_count(store, &channels)?;
        for (group_key, group) in &groups {
            let keys = group_keys(group_key)?;
            let waiting = store.continuations(group, group_key)?.len();
            for index in (0..waiting).rev() {
                let index = i32::try_from(index)
                    .map_err(|_| SpaceTxnError::Inconsistent("a continuation index exceeds i32"))?;
                session.store.retire_continuation(group, &keys, index)?;
                report.continuations += 1;
                if session
                    .space
                    .replay_waiting_continuations_estimate
                    .load(Ordering::Relaxed) >
                    0
                {
                    session.space.dec_replay_waiting_continuations();
                }
            }
        }
        report.joins = joins_before
            .checked_sub(swept_join_count(store, &channels)?)
            .ok_or(SpaceTxnError::Inconsistent("the volatile sweep added a join"))?;
        for (key, channel) in &channels {
            let key = StoreKey::from_digest(key)?;
            session.prefetch_data(channel, key)?;
            report.data += session.store.retire_channel_data(channel, key)?;
        }
        check_swept(store, &channels, &groups)?;
        Ok(report)
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
    store: Arc<NativeSpaceStore<O, C, P, A, K, E>>,
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
        + DeserializeOwned
        + ColdStoreDecode
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
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeOperationEpoch<C, P, A, K> + 'static,
{
    type Store = NativeSpaceStore<O, C, P, A, K, E>;

    fn store(&self) -> Arc<Self::Store> { Arc::clone(&self.store) }

    // A3: the produce counter of the session.
    fn host_counter(&self, produce: &Produce) -> i32 {
        self.session().space.get_produce_count(produce)
    }

    fn recorded(&self, _event: &IOEvent) -> Vec<(COMM, usize)> { Vec::new() }

    fn recorded_binding(&self, _event: &IOEvent, _comm: &COMM) -> Option<(COMM, usize)> { None }

    fn apply_op(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
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
                self.apply_produce(store, produce, before_observe, events)
            }
            StagedOperation::Consume(consume) => {
                self.apply_consume(store, consume, before_observe, events)
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
        + DeserializeOwned
        + ColdStoreDecode
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
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
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
    // operation fails as play fails it, with the error that the epoch names
    // for its denial (DR-113).
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
        // Changed by decision 89 (DR-113): the epoch names the error of its
        // denial, because the class of an exhaustion depends on the bound
        // source, as `operations.rs` returns it for a host operation.
        // Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons))
        Err(SpaceTxnError::Host(session.epoch.denial()))
    }

    // R4 step 9 (first part): the staged COMM is the COMM of the recorded
    // candidate. `observe_comm` checks it again.
    fn check_candidate(&self, ticket: &E::Ticket, comm: &COMM) -> Result<(), SpaceTxnError> {
        let identity = ticket
            .candidate_identity()
            .ok_or_else(|| mismatch("a recorded fire has no candidate identity"))?;
        match identity.matches_comm(comm) {
            true => Ok(()),
            false => Err(mismatch("the staged COMM differs from the recorded candidate")),
        }
    }

    fn apply_produce(
        &self,
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        produce: StagedProduce<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // Step 2: the commit checks of play, over the session store.
        let host_counter = self.host_counter(&produce.source);
        let (produce, joins) = ready_produce(store, produce, host_counter, |produces| {
            session.space.produce_counters(produces)
        })?;
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
                // Steps 10 to 12, with the store step of a stored host
                // produce (`operations.rs`): the data of the channel are in
                // the store before the publication.
                let key = StoreKey::from_digest(&produce_ref.channel_hash)?;
                session.prefetch_data(&channel, key)?;
                let counter = session
                    .space
                    .prepare_produce_counter(&produce_ref, persist)?;
                let mut completion = ticket.prepare(NativeReplayOutcome::Stored)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                // Changed by decision 89 (DR-96): the session store takes the
                // datum, so the export of the session holds it.
                // store_data(store, channel, data, persist, produce_ref.clone());
                session.store.put_datum(&channel, key, Datum {
                    a: data,
                    persist,
                    source: produce_ref.clone(),
                })?;
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
                            group,
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
                // Steps 10 to 12, with the store step of a host produce match
                // (`operations.rs`). The incoming datum is never stored, as in
                // play (FR-10, decision 30). The continuations, the data and
                // the joins of the group are in the store before the
                // publication. A persistent continuation stays, so its index
                // is not read: `continuation_index` is `None` exactly for a
                // persistent continuation.
                let keys = group_keys(&group)?;
                session.prefetch_continuations(&channels, &keys)?;
                for (joined, joined_key) in channels.iter().zip(&keys.channels) {
                    session.prefetch_data(joined, *joined_key)?;
                    session.read_joins(joined, *joined_key)?;
                }
                let retired = result::prepare(committed);
                let counter = session
                    .space
                    .prepare_produce_counter(&produce_ref, persist)?;
                let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                // Changed by decision 89 (DR-96): the session store retires the
                // continuation, the data and the joins of the match, so the
                // export of the session holds the removals.
                // if let Some(index) = continuation_index {
                //     store.remove_continuation(&channels, index);
                // }
                // session.space.mark_replay_waiting_continuation_match();
                // remove_matched_datum_and_join(store, &channels, &committed);
                session.store.retire_produce_match(
                    &channels,
                    &keys,
                    continuation_index.unwrap_or(0),
                    continuation.persist,
                    &retired.data,
                    &retired.retirement,
                )?;
                session.space.mark_replay_waiting_continuation_match();
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
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        consume: StagedConsume<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // Step 2.
        let consume =
            ready_consume(store, consume, |produces| session.space.produce_counters(produces))?;
        let ReadyConsume {
            source: consume_ref,
            channels,
            group,
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
        let keys = group_keys(&group)?;
        match fire {
            None => {
                // Steps 10 to 12, with the store step of a parked host
                // consume (`operations.rs`): the continuations of the group and
                // the joins of its channels are in the store before the
                // publication.
                session.prefetch_continuations(&channels, &keys)?;
                for (joined, joined_key) in channels.iter().zip(&keys.channels) {
                    session.read_joins(joined, *joined_key)?;
                }
                let mut completion = ticket.prepare(NativeReplayOutcome::Stored)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                // Changed by decision 89 (DR-96): the session store takes the
                // continuation and its joins, and it gives the depth of the
                // group for the waiting-continuation estimate.
                // if store_waiting_continuation(store, channels.clone(), wk) {
                //     session.space.inc_replay_waiting_continuations(&channels);
                // }
                let (inserted, depth) = session.store.store_consume(&channels, &keys, wk)?;
                if inserted {
                    session.space.inc_replay_waiting_continuations_depth(depth);
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
                // Steps 10 to 12, with the store step of a host consume match
                // (`operations.rs`): the data of the channels are in the store
                // before the publication.
                for (joined, joined_key) in channels.iter().zip(&keys.channels) {
                    session.prefetch_data(joined, *joined_key)?;
                }
                let retired = result::prepare(committed);
                let mut completion = ticket.prepare(NativeReplayOutcome::Matched)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                // Changed by decision 89 (DR-96): the session store retires the
                // data of the match, so the export of the session holds the
                // removals.
                // store_persistent_data(store, &committed);
                session
                    .store
                    .retire_data(&retired.data, &keys.channels, &retired.retirement)?;
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
    use std::collections::{BTreeSet, HashMap, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::rspace::hot_store_action::{HotStoreAction, InsertAction};
    use crate::rspace::internal::{ConsumeCandidate, Datum, Install};
    use crate::rspace::replay_rspace::native_epoch::{
        NativeCandidateIdentity, NativeReplayBoundary, NativeReplayEpoch, NativeReplayRestore,
    };
    use crate::rspace::rspace::RSpace;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use crate::rspace::space_store_handle::{
        ConsumeOutcome, ProduceOutcome, SharedStore, SpaceConsume, SpaceProduce, SpaceStoreHandle,
        SpaceTxnReceipt, bind_candidate, incoming_candidate, installed_channel_keys,
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
        // False for a row whose predecessors are not complete: begin_operation
        // consumes it and returns no ticket.
        ready: bool,
    }

    impl Row {
        fn stored() -> Self {
            Self {
                outcome: NativeReplayOutcome::Stored,
                introduction: NativeReplayDecision::Granted,
                comm: NativeReplayDecision::Granted,
                candidate: None,
                ready: true,
            }
        }

        fn not_ready() -> Self {
            Self {
                ready: false,
                ..Self::stored()
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

        fn invalidate(&self) { self.note("invalidate"); }

        // DR-113: the epoch names the error of its denial. The script names
        // the error of an exhausted signed limit, so a test sees that a space
        // operation fails with the error of its epoch.
        fn denial(&self) -> RSpaceError { RSpaceError::SignedLimitExhausted }
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
            if !row.ready {
                return Ok(None);
            }
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

    fn handle(session: &Arc<Session>) -> Handle { handle_with(session, BTreeSet::new()) }

    // The handle of a session whose system processes install the channels of
    // `installed` (R2).
    fn handle_with(session: &Arc<Session>, installed: BTreeSet<Blake2b256Hash>) -> Handle {
        let host = NativeSpaceHost::new(
            Arc::clone(session),
            |session: &Session| session,
            Arc::new(|_source: RSpaceOperationSource<'_>| Ok(())),
            installed,
        );
        SpaceStoreHandle::new(host)
    }

    // The data, continuations and joins of a store, as maps.
    #[derive(Debug, PartialEq)]
    struct StoreState {
        data: HashMap<String, Vec<Datum<String>>>,
        continuations: HashMap<Vec<String>, Vec<WaitingContinuation<String, String>>>,
        joins: HashMap<String, Vec<Vec<String>>>,
    }

    impl StoreState {
        // The state of the hot store of play.
        fn of_play(store: SharedStore<String, String, String, String>) -> Self {
            let snapshot = store.snapshot();
            Self {
                data: snapshot.data_flat(),
                continuations: snapshot.continuations_flat(),
                joins: snapshot.joins_flat(),
            }
        }

        // The state of the session store, from the export of every entry. An
        // empty list exports a deletion, so the maps do not hold it.
        fn of_session(session: &Session) -> Self {
            let mut state = Self {
                data: HashMap::new(),
                continuations: HashMap::new(),
                joins: HashMap::new(),
            };
            for action in session.store.changes() {
                match action {
                    HotStoreAction::Insert(InsertAction::InsertData(insert)) => {
                        state.data.insert(insert.channel, insert.data);
                    }
                    HotStoreAction::Insert(InsertAction::InsertContinuations(insert)) => {
                        state
                            .continuations
                            .insert(insert.channels, insert.continuations);
                    }
                    HotStoreAction::Insert(InsertAction::InsertJoins(insert)) => {
                        state.joins.insert(insert.channel, insert.joins);
                    }
                    HotStoreAction::Delete(_) => {}
                }
            }
            state
        }

        // The state without empty lists. A view load caches the empty history
        // value of a channel that it reads, so a run that loaded an empty
        // channel holds an empty list where another run holds none. An empty
        // list stores nothing and reaches no checkpoint.
        fn nonempty(mut self) -> Self {
            self.data.retain(|_, records| !records.is_empty());
            self.continuations.retain(|_, waiters| !waiters.is_empty());
            self.joins.retain(|_, groups| !groups.is_empty());
            self
        }
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
        let mut txn = handle.begin(&[s("b")]).await.expect("begin");
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
        let mut txn = handle.begin(&[s("a"), s("b")]).await.expect("begin");
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
        let state = StoreState::of_session(&session);
        assert_eq!(state.data.get(&s("a")).map(Vec::len), Some(1));
        assert_eq!(state.continuations.get(&vec![s("b")]).map(Vec::len), Some(1));
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
        let mut txn = handle.begin(&[s("b")]).await.expect("begin");
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
        let state = StoreState::of_session(&session);
        assert!(
            state
                .continuations
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
        let mut txn = handle.begin(&[s("b")]).await.expect("begin");
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
        let state = StoreState::of_session(&session);
        assert_eq!(state.continuations.get(&vec![s("b")]).map(Vec::len), Some(1));
    }

    // Negative control: a recorded outcome that does not fit the staged kind fails
    // closed before any observation and any effect.
    #[tokio::test]
    async fn native_space_replay_rejects_an_outcome_that_does_not_fit() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::matched(None));
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
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
        // The view load caches the empty history value of `a`. No record is stored.
        assert!(
            StoreState::of_session(&session)
                .data
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
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        let error = txn
            .commit()
            .await
            .expect_err("the denial fails the operation");
        assert_eq!(error, SpaceTxnError::Host(RSpaceError::SignedLimitExhausted));
        let log = script.log();
        assert_eq!(&log[5..], &[
            "begin produce",
            "footprint [\"a\"] []",
            "observe_produce a",
            "prepare DeniedIntroduction",
            "publish",
        ]);
        let state = StoreState::of_session(&session);
        assert_eq!(
            state.data.get(&s("a")).map(|records| records
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
        let mut txn = handle.begin(&[s("b")]).await.expect("begin");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let staged = txn
            .produce(fire_op("b", "z", &waiting))
            .expect("stage the fire");
        script.push(Row::denied_comm(staged.comm.clone().expect("a fire has a COMM")));
        let error = txn.commit().await.expect_err("the denial fails the fire");
        assert_eq!(error, SpaceTxnError::Host(RSpaceError::SignedLimitExhausted));
        assert_eq!(script.log(), vec![
            "begin produce",
            "footprint [\"b\"] [[\"b\"]]",
            "observe_produce b",
            "observe_comm",
            "prepare DeniedComm",
            "publish",
        ]);
        let state = StoreState::of_session(&session);
        assert_eq!(state.continuations.get(&vec![s("b")]).map(Vec::len), Some(1));
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
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let error = txn.commit().await.expect_err("an unready row fails");
        assert_eq!(error, mismatch("the journal row of the operation is not ready"));
        // The failure closes the epoch, so an operation that waits for the row
        // wakes and fails.
        assert_eq!(script.log(), vec!["begin produce", "invalidate"]);
        // The view load caches the empty history value of `a`. No record is stored.
        assert!(
            StoreState::of_session(&session)
                .data
                .get(&s("a"))
                .is_none_or(Vec::is_empty)
        );
    }

    // R2 (A4): the installed channels of the session are guarded, as in play.
    // The keys come from the installations of the session, as
    // `create_native_replay_env` takes them.
    #[tokio::test]
    async fn native_space_replay_keeps_the_installed_guard() {
        let script = Script::default();
        let install = Install {
            patterns: vec![s("_")],
            continuation: s("system"),
        };
        let installs = vec![(vec![s("sys")], install)];
        let installed = installed_channel_keys(installs.iter().map(|(channels, _)| channels))
            .expect("the keys of the installed channels");
        let session = session(&script, installs).await;
        let handle = handle_with(&session, installed);
        let mut txn = handle.begin(&[s("sys")]).await.expect("begin");
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
        let mut txn = handle.begin(&[s("b")]).await.expect("begin");
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
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
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

    // Runs one plan: store x and y on a and park k on b, then take x from a and
    // fire k with z. `script` is the native replay script, or `None` for play.
    // Native replay records the row of each operation as play commits it.
    async fn run_plan(
        handle: &Handle,
        script: Option<&Script>,
    ) -> Vec<SpaceTxnReceipt<String, String, String, String>> {
        let first = run_first(handle, script).await;
        let second = run_second(handle, script).await;
        vec![first, second]
    }

    // The first transaction of the plan: store x and y on a and park k on b.
    async fn run_first(
        handle: &Handle,
        script: Option<&Script>,
    ) -> SpaceTxnReceipt<String, String, String, String> {
        if let Some(script) = script {
            for _ in 0..3 {
                script.push(Row::stored());
            }
        }
        let mut txn = handle.begin(&[s("a"), s("b")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        txn.consume(park_op("b", "k")).expect("stage the park");
        txn.commit().await.expect("commit the stores and the park")
    }

    // The second transaction of the plan: take x from a and fire k with z.
    async fn run_second(
        handle: &Handle,
        script: Option<&Script>,
    ) -> SpaceTxnReceipt<String, String, String, String> {
        let keys = [s("a"), s("b")];
        let mut txn = handle.begin(&keys).await.expect("begin");
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

    // H24 (S2): native replay commits the receipts, the store state and the
    // produce counters of play, and its export reaches the root of the
    // checkpoint of play. Both runs start at the empty history.
    #[tokio::test]
    async fn native_space_replay_effects_equal_play() {
        let mut stores = InMemoryStoreManager::new();
        let play = RSpace::<String, String, String, String>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("the play space");
        let empty = play.get_history_repository().root();
        let play_handle = SpaceStoreHandle::new(play.clone());
        let played = run_plan(&play_handle, None).await;

        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let native_handle = handle(&session);
        let replayed = run_plan(&native_handle, Some(&script)).await;

        assert_eq!(replayed, played);
        assert_eq!(
            StoreState::of_session(&session).nonempty(),
            StoreState::of_play(play.get_store()).nonempty()
        );
        let produces: Vec<Produce> = [("a", "x"), ("a", "y"), ("b", "z")]
            .into_iter()
            .map(|(channel, data)| Produce::create(&s(channel), &s(data), false))
            .collect();
        let mut counters = Vec::with_capacity(2);
        for handle in [&play_handle, &native_handle] {
            let mut txn = handle.begin(&[s("a"), s("b")]).await.expect("begin");
            counters.push(txn.produce_counters(&produces).expect("read the counters"));
        }
        assert_eq!(counters[0], counters[1]);
        assert_eq!(counters[1].values().copied().collect::<Vec<_>>(), vec![1, 1, 1]);
        assert!(!script.log().contains(&"wait_ready".to_owned()));
        // The export of the session holds the effects of the space operations,
        // so it reaches the root of play (DR-96).
        let exported = session.export().await.expect("the export");
        let checkpoint = play.create_checkpoint().await.expect("the checkpoint");
        assert_ne!(
            checkpoint.root, empty,
            "the plan changes the store, so the check is not vacuous"
        );
        assert_eq!(exported.root(), &checkpoint.root);
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
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        txn.commit().await.expect("commit the stores");
        script.clear();

        // The COMM of the take of y, from a transaction that never commits.
        let other = {
            let mut txn = handle.begin(&[s("a")]).await.expect("begin");
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
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
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
        let state = StoreState::of_session(&session);
        assert_eq!(state.data.get(&s("a")).map(Vec::len), Some(2));
    }

    // ─── Properties of decision 47 (H24-H26, T8, NoDeadlock)
    // ──────────────────────
    //
    // A plan is a list of transactions of one to three steps. A step is a send of
    // a value or a receive on a channel, and the agent's rule decides its staged
    // operation: a send fires the first waiter of its channel or stores, and a
    // receive takes the first record of its channel or parks. Play runs the plan on
    // a play space, and native replay runs it on a scripted epoch that records the
    // row of each operation as play staged it. Runs are deterministic (a fixed
    // seed) and keep no failure file. H27 compared the charges of metered reads,
    // and the metered reads are removed with host-work metering (DR-131).

    const PROP_SEED: u64 = 0x0D47_5EED_2026_1007;

    fn property_config() -> proptest::test_runner::Config {
        proptest::test_runner::Config {
            cases: 64,
            rng_seed: proptest::test_runner::RngSeed::Fixed(PROP_SEED),
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        }
    }

    fn property_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a test runtime")
    }

    const PROP_CHANNELS: [&str; 3] = ["a", "b", "c"];
    const PROP_VALUES: [&str; 3] = ["x", "y", "z"];

    #[derive(Clone, Copy, Debug)]
    enum Step {
        Send(usize, usize),
        Receive(usize),
    }

    // The change that native replay meets at one operation: a recorded denial of
    // the introduction or of the COMM, an outcome that does not fit the staged
    // kind, another candidate, or a row that is not ready.
    #[derive(Clone, Copy, Debug)]
    enum Edit {
        Deny { comm: bool },
        Misfit,
        OtherCandidate,
        NotReady,
    }

    fn plan_strategy() -> impl proptest::strategy::Strategy<Value = Vec<Vec<Step>>> {
        use proptest::prelude::*;
        let step = prop_oneof![
            (0..3usize, 0..3usize).prop_map(|(channel, value)| Step::Send(channel, value)),
            (0..3usize).prop_map(Step::Receive),
        ];
        proptest::collection::vec(proptest::collection::vec(step, 1..=3), 1..=4)
    }

    // Stages `step` and returns the row that native play records for it. `kont`
    // names the continuation of a park, unique in the plan.
    fn stage_step(
        txn: &mut BoxedSpaceTxn<String, String, String, String>,
        step: Step,
        kont: &str,
    ) -> Row {
        match step {
            Step::Send(channel, value) => {
                let channel = PROP_CHANNELS[channel];
                let waiting = txn
                    .continuations(&[s(channel)])
                    .expect("read the waiters")
                    .first()
                    .cloned();
                match waiting {
                    Some(waiting) => {
                        let staged = txn
                            .produce(fire_op(channel, PROP_VALUES[value], &waiting))
                            .expect("stage a fire");
                        Row::matched(staged.comm.clone())
                    }
                    None => {
                        txn.produce(store_op(channel, PROP_VALUES[value]))
                            .expect("stage a store");
                        Row::stored()
                    }
                }
            }
            Step::Receive(channel) => {
                let channel = PROP_CHANNELS[channel];
                let stored = txn
                    .data(&s(channel))
                    .expect("read the data")
                    .first()
                    .cloned();
                match stored {
                    Some(datum) => {
                        let staged = txn
                            .consume(take_op(channel, &datum, 0))
                            .expect("stage a take");
                        Row::matched(staged.comm.clone())
                    }
                    None => {
                        txn.consume(park_op(channel, kont)).expect("stage a park");
                        Row::stored()
                    }
                }
            }
        }
    }

    // The row that native replay meets instead of `row`.
    fn edited(row: &Row, edit: Edit) -> Row {
        match (edit, &row.candidate) {
            (Edit::Deny { comm: true }, Some(candidate)) => Row::denied_comm(candidate.clone()),
            (Edit::Deny { .. }, _) => Row::denied_introduction(),
            (Edit::OtherCandidate, Some(candidate)) => {
                let mut other = candidate.clone();
                other
                    .times_repeated
                    .insert(Produce::create(&s("q"), &s("q"), false), 7);
                Row::matched(Some(other))
            }
            (Edit::Misfit | Edit::OtherCandidate, _) => match row.outcome {
                NativeReplayOutcome::Stored => Row::matched(None),
                _ => Row::stored(),
            },
            (Edit::NotReady, _) => Row::not_ready(),
        }
    }

    type PlanResults = Vec<Result<SpaceTxnReceipt<String, String, String, String>, SpaceTxnError>>;

    // Runs `plan` on `handle` and stops at the first failed commit. With a script
    // (native replay), each transaction records the rows of its operations before
    // it commits, and `edit` changes the row of one operation.
    async fn run_prop_plan(
        handle: &Handle,
        script: Option<&Script>,
        plan: &[Vec<Step>],
        edit: Option<(usize, usize, Edit)>,
    ) -> PlanResults {
        let keys: Vec<String> = PROP_CHANNELS.iter().map(|channel| s(channel)).collect();
        let mut results = Vec::with_capacity(plan.len());
        for (index, steps) in plan.iter().enumerate() {
            let mut txn = handle.begin(&keys).await.expect("begin");
            let mut rows = Vec::with_capacity(steps.len());
            for (position, step) in steps.iter().enumerate() {
                rows.push(stage_step(&mut txn, *step, &format!("k{index}.{position}")));
            }
            if let Some(script) = script {
                for (position, row) in rows.iter().enumerate() {
                    match edit {
                        Some((at, op, change)) if at == index && op == position => {
                            script.push(edited(row, change))
                        }
                        _ => script.push(row.clone()),
                    }
                }
            }
            let result = txn.commit().await;
            let failed = result.is_err();
            results.push(result);
            if failed {
                break;
            }
        }
        results
    }

    // The plan up to operation `op` of transaction `at`, without that operation.
    fn prefix_plan(plan: &[Vec<Step>], at: usize, op: usize) -> Vec<Vec<Step>> {
        let mut prefix: Vec<Vec<Step>> = plan[..at].to_vec();
        if op > 0 {
            prefix.push(plan[at][..op].to_vec());
        }
        prefix
    }

    // The transaction and the operation of the flat operation index `pick`.
    fn locate(plan: &[Vec<Step>], pick: usize) -> (usize, usize) {
        let mut rest = pick % plan.iter().map(Vec::len).sum::<usize>();
        for (index, steps) in plan.iter().enumerate() {
            if rest < steps.len() {
                return (index, rest);
            }
            rest -= steps.len();
        }
        unreachable!("the pick is within the plan")
    }

    async fn play_space() -> RSpace<String, String, String, String> {
        let mut stores = InMemoryStoreManager::new();
        RSpace::<String, String, String, String>::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("the play space")
    }

    // The produce counters of every send of the plan's values.
    async fn plan_counters(handle: &Handle) -> std::collections::BTreeMap<Produce, i32> {
        let mut produces = Vec::with_capacity(PROP_CHANNELS.len() * PROP_VALUES.len());
        for channel in PROP_CHANNELS {
            for value in PROP_VALUES {
                produces.push(Produce::create(&s(channel), &s(value), false));
            }
        }
        let keys: Vec<String> = PROP_CHANNELS.iter().map(|channel| s(channel)).collect();
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.produce_counters(&produces)
            .expect("read the counters")
            .into_iter()
            .collect()
    }

    proptest::proptest! {
        #![proptest_config(property_config())]

        // H24 and T8: native replay over the rows of play commits the receipts, the
        // store and the produce counters of play, consumes one row for each
        // operation, and keeps the epoch open. Its export reaches the root of the
        // checkpoint of play (DR-96).
        #[test]
        fn prop_native_space_replay_reproduces_play(plan in plan_strategy()) {
            property_runtime().block_on(async {
                let play = play_space().await;
                let play_handle = SpaceStoreHandle::new(play.clone());
                let played = run_prop_plan(&play_handle, None, &plan, None).await;
                proptest::prop_assert!(played.iter().all(Result::is_ok), "{:?}", played);

                let script = Script::default();
                let session = session(&script, Vec::new()).await;
                let native_handle = handle(&session);
                let replayed = run_prop_plan(&native_handle, Some(&script), &plan, None).await;
                proptest::prop_assert_eq!(&replayed, &played);
                proptest::prop_assert_eq!(
                    StoreState::of_session(&session).nonempty(),
                    StoreState::of_play(play.get_store()).nonempty()
                );
                proptest::prop_assert_eq!(
                    plan_counters(&native_handle).await,
                    plan_counters(&play_handle).await
                );
                proptest::prop_assert!(script.rows.lock().expect("rows lock").is_empty());
                proptest::prop_assert!(!script.log().contains(&"invalidate".to_owned()));
                let exported = session.export().await.expect("the export");
                let checkpoint = play.create_checkpoint().await.expect("the checkpoint");
                proptest::prop_assert_eq!(exported.root(), &checkpoint.root);
                Ok::<(), proptest::test_runner::TestCaseError>(())
            })?;
        }

        // H25 and NoDeadlock: a recorded denial at one operation fails the commit
        // with the denial error of the epoch (DR-113), keeps the operations before
        // it applied (D6), as play of the prefix applies them, and keeps the epoch
        // open.
        #[test]
        fn prop_native_space_replay_denial_keeps_prefix(
            plan in plan_strategy(),
            pick in 0usize..64,
            comm in proptest::bool::ANY,
        ) {
            property_runtime().block_on(async {
                let (at, op) = locate(&plan, pick);
                let prefix = play_space().await;
                run_prop_plan(
                    &SpaceStoreHandle::new(prefix.clone()),
                    None,
                    &prefix_plan(&plan, at, op),
                    None,
                )
                .await;

                let script = Script::default();
                let session = session(&script, Vec::new()).await;
                let replayed = run_prop_plan(
                    &handle(&session),
                    Some(&script),
                    &plan,
                    Some((at, op, Edit::Deny { comm })),
                )
                .await;
                proptest::prop_assert_eq!(replayed.len(), at + 1);
                proptest::prop_assert!(replayed[..at].iter().all(Result::is_ok));
                proptest::prop_assert_eq!(
                    replayed[at].as_ref().err(),
                    Some(&SpaceTxnError::Host(RSpaceError::SignedLimitExhausted))
                );
                proptest::prop_assert_eq!(
                    StoreState::of_session(&session).nonempty(),
                    StoreState::of_play(prefix.get_store()).nonempty()
                );
                proptest::prop_assert!(!script.log().contains(&"invalidate".to_owned()));
                Ok::<(), proptest::test_runner::TestCaseError>(())
            })?;
        }

        // H26, T8 and NoDeadlock: an outcome that does not fit, another candidate,
        // and a row that is not ready fail the commit closed. The operations before
        // it stay applied, the failed operation has no effect, and the failure
        // closes the epoch, so no operation waits for its row.
        #[test]
        fn prop_native_space_replay_fails_closed(
            plan in plan_strategy(),
            pick in 0usize..64,
            kind in 0usize..3,
        ) {
            property_runtime().block_on(async {
                let (at, op) = locate(&plan, pick);
                let edit = [Edit::Misfit, Edit::OtherCandidate, Edit::NotReady][kind];
                let prefix = play_space().await;
                run_prop_plan(
                    &SpaceStoreHandle::new(prefix.clone()),
                    None,
                    &prefix_plan(&plan, at, op),
                    None,
                )
                .await;

                let script = Script::default();
                let session = session(&script, Vec::new()).await;
                let replayed =
                    run_prop_plan(&handle(&session), Some(&script), &plan, Some((at, op, edit)))
                        .await;
                proptest::prop_assert_eq!(replayed.len(), at + 1);
                proptest::prop_assert!(replayed[..at].iter().all(Result::is_ok));
                match &replayed[at] {
                    Err(SpaceTxnError::Host(RSpaceError::InterpreterError(message))) => {
                        proptest::prop_assert!(message.starts_with("native space replay: "), "{}", message)
                    }
                    other => proptest::prop_assert!(false, "a mismatch, not {:?}", other),
                }
                proptest::prop_assert_eq!(
                    StoreState::of_session(&session).nonempty(),
                    StoreState::of_play(prefix.get_store()).nonempty()
                );
                proptest::prop_assert!(script.log().contains(&"invalidate".to_owned()));
                Ok::<(), proptest::test_runner::TestCaseError>(())
            })?;
        }
    }
}
