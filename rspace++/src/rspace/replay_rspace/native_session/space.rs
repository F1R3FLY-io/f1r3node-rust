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
// Changed by decision 90 (DR-132): racing evaluation removed the scheduler
// and its per-space order, so a transaction can reach its rows before their
// predecessors complete. Before its first operation, under its stripes, the
// transaction checks all its rows (`check_ready`, `SpaceTxnReadiness`). A
// transaction that is not ready applies nothing and fails with `NotReady`.
// The job then releases its stripes, waits for a change as a counted waiter
// (`wait_changed`), and stages again (`NativeReplayStall.tla`). A backend
// still never waits while it holds stripes, and a row that is not ready
// after a passed check fails closed (P1).
//
// Changed by decision 92: every row of a space transaction names the
// footprint key `S(sid)` as one more join group. The first row of a
// committed transaction is unique to its job. So the check reads the first
// staged row only (`SpaceReplayOrder.tla`), and a later row that is not
// ready or does not fit fails closed at its commit. Changed by decision 95:
// the check and the wait go through the epoch of the session
// (`NativeOperationEpoch::space_transaction_ready` and
// `NativeOperationEpoch::wait_space_change`).
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
// Changed by decision 123 (plan v4, V3-8, V3-16, R9, R20) and decision 124
// (L10): play changes the hot store through the free store steps of
// ops_produce.rs and ops_consume.rs. `space_play.rs` calls them in place of
// the retired copies of `space_store_handle.rs`. A recorded denial publishes
// its row through the one base `publish_denial` of `operations.rs`. No
// failure closes the epoch.
//
// The session logs no event. The backend gives the events of play to the
// event correlation check of `commit_operation` only. A non-persistent
// produce advances the produce counter of the session, which `host_counter`
// reads, as `log_produce` advances the counter of play. A space operation
// meters only what its host operation meters, through the observations of
// the ticket: its COMMs and the bytes that it stores (DR-131).
//
// Changed by decision 90 (C6): a session for a block report also gives each
// applied space row to the reporting logger of its space, where the replay
// backend of `space_replay.rs` logs the row. This is how `operations.rs`
// logs a host row (DR-134, DR-70). Validation and play build no logger, so
// they skip every logging step.
//
// Added by decision 124 (plan v4 C7.16, C7.18, C7.20, rules CR-18, CR-22 and
// F12): under owner funding a row with a search runs the base replay walk at
// its commit (`apply_row`). It verifies the recorded refusals, then it selects
// the recorded tuple over the search. Each tuple but the staged one runs the
// guard once, then the theory (F12). A recorded store of a staged fire stores
// the datum. A store above the bound of its channel is the host outcome, and
// the readers accept a side above its bound (CR-22).

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
// Changed by decision 124 (L10): the `denied` flag retires.
// use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;

// Added by decision 124 (L15, L16, plan v4 C7.16, errata V4-6): the refusal
// loop of the base replay, over the row search.
use super::operations::verify_row_refusals;
use super::{NativeReplaySession, PublicationGuard, result};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{GroupKeys, StoreKey};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
// Changed by decision 95: the readiness of a space transaction comes from
// the epoch.
// use crate::rspace::replay_rspace::native_epoch::{
//     NativeOperationEpoch, NativeOperationPublication, NativeOperationTicket,
// NativeReplayDecision,     NativeReplayOutcome,
// };
use crate::rspace::replay_rspace::native_epoch::{
    NativeOperationEpoch, NativeOperationPublication, NativeOperationTicket, NativeReplayDecision,
    NativeReplayOutcome, SpaceTransactionReadiness,
};
use crate::rspace::rspace_interface::RSpaceOperationSource;
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
// Changed by decisions 92 and 94: the transaction covers the exact keys of
// phase B, and the footprint names the footprint key.
// use crate::rspace::space_store_handle::{
//     BeforeObserve, BoxedSpaceTxn, HostTxn, LockCoverage, ReadyConsume, ReadyConsumeFire,
// ReadyFire,     ReadyProduce, SpaceMode, SpaceOpReceipt, SpaceTxnError, SpaceTxnHost,
// StagedConsume,     StagedConsumeFire, StagedFire, StagedOperation, StagedProduce,
// SweepReport, TxnBackend,     TxnRegistration, TxnStore, check_swept, fired_receipt,
// plan_sweep, ready_consume,     ready_produce, root_leaf_records, run_before_observe,
// sweep_channels, swept_join_count, };
// use crate::rspace::striped_locks;
// Changed by decision 102 (decision 119 S2): the backend reads the recorded
// rows of a transaction.
// Changed by decision 124 (L10): the arms name the source of a skipped row.
use crate::rspace::space_store_handle::{
    AppliedRow, BeforeObserve, BoxedSpaceTxn, HostTxn, OpSource, ReadyConsume, ReadyConsumeFire,
    ReadyFire, ReadyProduce, RecordedRow, ReplayItemRule, RowCandidates, RowLists, RowParts,
    RowReads, RowSearch, RowSearchWithReads, RowTupleRequest, SpaceMode, SpaceOpReceipt,
    SpaceTxnError, SpaceTxnHost, StagedConsume, StagedConsumeFire, StagedFire, StagedOperation,
    StagedProduce, StoreEffect, SweepReport, TailRead, TxnBackend, TxnRegistration, TxnStore,
    actual_park_effects, check_swept, fired_receipt, footprint_joins, plan_sweep,
    ready_actual_consume_fire, ready_actual_produce_fire, ready_consume, ready_produce,
    root_leaf_records, row_enumeration, run_before_observe, skipped_row, sweep_channels,
    swept_join_count,
};
use crate::rspace::striped_locks::{self, ChannelLockGuard};
// Changed by decision 90 (C6, step D): the rigged replay paths retire, so only
// the test module of this file names a COMM. The text before:
// use crate::rspace::trace::event::{COMM, Event, IOEvent, Produce};
use crate::rspace::trace::event::{Event, IOEvent, Produce};

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
    // Added by decision 90 (DR-132): the readiness wait of the transactions of
    // the host.
    // Disabled by decision 95: the epoch of the session checks and waits
    // (`NativeOperationEpoch::space_transaction_ready`).
    // readiness: Arc<dyn SpaceTxnReadiness>,
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

/// Added by decision 90 (DR-132): whether the recorded outcome `outcome` fits a
/// staged operation, a fire when `fire` holds (R4 step 7). The commit of each
/// operation and the readiness check of the whole transaction
/// (`SpaceTxnReadiness::check`) share this rule.
///
/// Changed by decision 92: only the commit of each operation reads the rule,
/// because the readiness check reads no outcome. A skipped introduction (G3,
/// DR-135, S6) fits a store and a fire, as the base host operation replays it
/// (`operations.rs`).
// pub fn outcome_fits(fire: bool, outcome: NativeReplayOutcome) -> bool {
//     match (fire, outcome) {
//         (false, NativeReplayOutcome::Stored |
// NativeReplayOutcome::DeniedIntroduction) => true,         (
//             true,
//             NativeReplayOutcome::Matched |
//             NativeReplayOutcome::DeniedIntroduction |
//             NativeReplayOutcome::DeniedComm,
//         ) => true,
//         _ => false,
//     }
// }
pub fn outcome_fits(fire: bool, outcome: NativeReplayOutcome) -> bool {
    match (fire, outcome) {
        (
            false,
            NativeReplayOutcome::Stored |
            NativeReplayOutcome::DeniedIntroduction |
            NativeReplayOutcome::SkippedIntroduction,
        ) => true,
        (
            true,
            NativeReplayOutcome::Matched |
            NativeReplayOutcome::DeniedIntroduction |
            NativeReplayOutcome::SkippedIntroduction |
            NativeReplayOutcome::DeniedComm,
        ) => true,
        _ => false,
    }
}

// Disabled by decision 92: the readiness check reads the first staged row
// only, so no check reads an outcome that ends the transaction, and no check
// reads the rows of every operation. Disabled by decision 95: the epoch of the
// session checks and waits (`NativeOperationEpoch::space_transaction_ready`
// and `NativeOperationEpoch::wait_space_change`), so the host takes no
// readiness wait.

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
    ///
    /// Changed by decision 90 (DR-132): `readiness` is the readiness wait of
    /// the transactions of the host, over the journal of the session.
    ///
    /// Changed by decision 95: the epoch of the session checks and waits, so
    /// the host takes no readiness wait.
    // pub fn new(
    //     owner: Arc<O>,
    //     session: fn(&O) -> &NativeReplaySession<C, P, A, K, E>,
    //     authority: AuthorityResolver<C, P, A, K, E>,
    //     installed: BTreeSet<Blake2b256Hash>,
    //     readiness: Arc<dyn SpaceTxnReadiness>,
    // ) -> Self {
    //     let store = Arc::new(NativeSpaceStore {
    //         owner: Arc::clone(&owner),
    //         session,
    //     });
    //     Self {
    //         owner,
    //         session,
    //         authority,
    //         installed,
    //         store,
    //         readiness,
    //     }
    // }
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
    // Changed by decision 94: the host takes phase A at the begin, and the
    // transaction takes phase B ([`SpaceTxn::lock_phase_b`]). The default
    // `begin` takes both phases over the lock keys, as `consume_lock` does.
    // async fn begin(
    //     &self,
    //     lock_keys: &[C],
    //     registration: TxnRegistration,
    // ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
    //     let session = self.session();
    //     session.ensure_open()?;
    //     if lock_keys.is_empty() {
    //         return Err(SpaceTxnError::InvalidOperation("lock keys must not be
    // empty"));     }
    //     let hashes: Vec<u64> =
    // lock_keys.iter().map(striped_locks::channel_hash).collect();
    //     let coverage = LockCoverage::new(
    //         &hashes,
    //         session.space.phase_a_locks.len(),
    //         session.space.phase_b_locks.len(),
    //     );
    //     let locks = session.consume_lock(&hashes).await?;
    //     let backend = NativeBackend {
    //         owner: Arc::clone(&self.owner),
    //         session: self.session,
    //         authority: Arc::clone(&self.authority),
    //         store: Arc::clone(&self.store),
    //         denied: AtomicBool::new(false),
    //     };
    //     Ok(Box::new(HostTxn::new(
    //         SpaceMode::NativeReplay,
    //         backend,
    //         Arc::clone(&self.store),
    //         self.installed.clone(),
    //         coverage,
    //         locks,
    //         registration,
    //     )))
    // }
    async fn begin_phase_a(
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
        let phase_a = striped_locks::native::prepare(&session.space.phase_a_locks, &hashes)?
            .acquire()
            .await;
        let backend = NativeBackend {
            owner: Arc::clone(&self.owner),
            session: self.session,
            authority: Arc::clone(&self.authority),
            store: Arc::clone(&self.store),
            // Retired by decision 124 (L10): no flag.
            // denied: AtomicBool::new(false),
        };
        Ok(Box::new(HostTxn::new(
            SpaceMode::NativeReplay,
            backend,
            Arc::clone(&self.store),
            self.installed.clone(),
            lock_keys.to_vec(),
            phase_a,
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

    // Added by decision 90 (DR-132): the counted wait of a job whose transaction
    // was not ready (`TxnPark`).
    // Changed by decision 95: the epoch of the session waits.
    async fn wait_changed(&self, seen: u64) -> Result<(), SpaceTxnError> {
        self.session()
            .epoch
            .wait_space_change(seen)
            .await
            .map_err(SpaceTxnError::Host)
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
    // Retired by decision 124 (L10): no failure closes the epoch, so no flag
    // keeps it open. The text before:
    // // Set when the current operation published a recorded denial, so its row
    // // is complete and its failure keeps the epoch open.
    // denied: AtomicBool,
    // Added by decision 90 (DR-132): the readiness wait of the transaction.
    // Disabled by decision 95: the epoch of the session checks the readiness.
    // readiness: Arc<dyn SpaceTxnReadiness>,
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

    // Retired by decision 90 (C6, step D): the trait no longer asks for the
    // rigged log. The text before:
    // ```
    // fn recorded(&self, _event: &IOEvent) -> Vec<(COMM, usize)> { Vec::new() }
    //
    // fn recorded_binding(&self, _event: &IOEvent, _comm: &COMM) -> Option<(COMM, usize)> { None }
    // ```

    // Added by decision 90 (DR-132): the readiness check of the whole transaction
    // (`TxnCheck`), under its stripes and before its first operation. While the
    // stripes are held, no other operation can reserve or publish a row of the
    // footprints of the transaction, so the rows that the check selects are the
    // rows that the operations then reserve, and a passed check stays valid
    // through the commit. A transaction that is not ready applies nothing
    // and keeps the epoch open: the job waits and stages again.
    // Changed by decision 92: the check reads the first staged operation
    // `first` only. Changed by decision 95: the epoch of the session checks it.
    fn check_ready(&self, first: &StagedOperation<C, P, A, K>) -> Result<(), SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        let source = match first {
            StagedOperation::Produce(produce) => RSpaceOperationSource::Produce(&produce.source),
            StagedOperation::Consume(consume) => RSpaceOperationSource::Consume(&consume.source),
        };
        match session
            .epoch
            .space_transaction_ready(source)
            .map_err(SpaceTxnError::Host)?
        {
            SpaceTransactionReadiness::Ready => Ok(()),
            SpaceTransactionReadiness::NotReady { seen } => Err(SpaceTxnError::NotReady { seen }),
        }
    }

    // Added by decision 102 (decision 119 S2): the recorded row of a staged
    // operation comes from the epoch of the session
    // (`NativeOperationEpoch::space_recorded_row`).
    fn recorded_row(
        &self,
        source: RSpaceOperationSource<'_>,
        previous: Option<usize>,
        footprint_key: &C,
    ) -> Result<Option<RecordedRow>, SpaceTxnError> {
        let session = self.session();
        session.ensure_open()?;
        session
            .epoch
            .space_recorded_row(source, previous, footprint_key)
            .map_err(SpaceTxnError::Host)
    }

    // Changed by decision 92: each operation authenticates the footprint key
    // of its transaction in its footprint.
    // fn apply_op(
    //     &self,
    //     store: &Self::Store,
    //     operation: StagedOperation<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    fn apply_op(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        // Changed by decision 124 (L10, plan v4 C7.16): the session gate and
        // the dispatch moved to `apply_row`, which takes the row search. The
        // text before:
        // ```
        // let session = self.session();
        // // R4 step 1 (A6): the session must be open and not at a boundary. A
        // // transaction cannot own a guard of the gate, so each operation takes
        // // it and fails closed on contention.
        // session.ensure_open()?;
        // let _gate = session
        //     .gate
        //     .try_read()
        //     .map_err(|_| mismatch("the session is at a boundary"))?;
        // ```
        // let result = match operation {
        //     StagedOperation::Produce(produce) => {
        //         self.apply_produce(store, produce, before_observe, events)
        //     }
        //     StagedOperation::Consume(consume) => {
        //         self.apply_consume(store, consume, before_observe, events)
        //     }
        // };
        // Changed by decision 124 (L10): a failed operation leaves the epoch
        // open, as a failed host operation does in the base. Its ticket
        // cancels its row on drop. A waiter of that row parks counted, so the
        // stall rule of the base replay ends the wait (DR-132). The match is
        // the tail of `apply_row` now. The text before:
        // ```
        // let result = match operation {
        //     StagedOperation::Produce(produce) => {
        //         self.apply_produce(store, produce, before_observe, events)
        //     }
        //     StagedOperation::Consume(consume) => {
        //         self.apply_consume(store, consume, before_observe, events)
        //     }
        // };
        // // A failed operation leaves its journal row incomplete, and an operation
        // // that waits for the row would wait forever. So the failure closes the
        // // epoch: every waiting operation wakes and fails (Closed), and the
        // // certification fails closed. A recorded denial publishes its row first,
        // // and play goes on after it, so it keeps the epoch open.
        // if result.is_err() && !self.denied.swap(false, Ordering::AcqRel) {
        //     session.epoch.invalidate();
        // }
        // result
        // ```
        self.apply_row(store, operation, before_observe, footprint_key, None, events)
            .map(|applied| applied.receipt)
    }

    // Added by decision 124 (L10, plan v4 C7.16, LP-6): the row of native
    // replay. The session gate holds for the row (R4 step 1). The dispatch
    // takes the row search. An introduction that its owner cannot pay skips
    // the row (R8). Its row stores nothing, and it can stop a unit (CR-18).
    fn apply_row(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let session = self.session();
        // R4 step 1 (A6): the session must be open and not at a boundary. A
        // transaction cannot own a guard of the gate, so each operation takes
        // it and fails closed on contention.
        session.ensure_open()?;
        let _gate = session
            .gate
            .try_read()
            .map_err(|_| mismatch("the session is at a boundary"))?;
        let source = match &operation {
            StagedOperation::Produce(produce) => OpSource::Produce(produce.source.clone()),
            StagedOperation::Consume(consume) => OpSource::Consume(consume.source.clone()),
        };
        let applied = match operation {
            StagedOperation::Produce(produce) => {
                self.apply_produce(store, produce, before_observe, footprint_key, search, events)
            }
            StagedOperation::Consume(consume) => {
                self.apply_consume(store, consume, before_observe, footprint_key, search, events)
            }
        };
        match applied {
            Err(SpaceTxnError::Host(RSpaceError::IntroductionRefused)) => {
                Ok(skipped_row(source, true))
            }
            other => other,
        }
    }

    // Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD 1.2): the tail
    // read of a deviating unit, over the lists of the replay search of the row
    // (`produce_found`) on the store after the retire group of the unit.
    fn tail_read(
        &self,
        store: &Self::Store,
        search: &mut dyn RowSearch<C, P, A, K>,
        reads: RowReads<'_>,
    ) -> Result<TailRead, SpaceTxnError> {
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**self.session().space.matcher;
        let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
        search.tail_read(&lists, matcher)
    }

    // Added by decision 94: phase B as the session takes it for a host
    // operation, with the fallible preparation of `striped_locks::native`.
    async fn lock_phase_b(&self, hashes: &[u64]) -> Result<ChannelLockGuard, SpaceTxnError> {
        let prepared = striped_locks::native::prepare(&self.session().space.phase_b_locks, hashes)?;
        Ok(prepared.acquire().await)
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
    // Changed by decision 90 (DR-132): the readiness check of the transaction
    // (`check_ready`) passed under the same stripes, so a row that is not
    // ready here shows a defect or forged evidence, and it still fails closed.
    // Changed by decision 92: the check reads the first row only. A later row
    // follows the first row on the chain of `S(sid)`, so a later row that is
    // not ready here also shows a defect or forged evidence.
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

    // Added by decision 124 (L15, L16, plan v4 C7.16, errata V4-6, V4-17,
    // V4-18): the replay search of a produce row, before the row commits. It
    // verifies the recorded refusals of the row over the search (R7). A staged
    // fire selects the recorded tuple over the search (X7). The staged tuple
    // c0 keeps the staged fire. Another tuple gives the actual fire, readied
    // against the store, and a recorded store drops the fire. A store above
    // the bound of its channel is the host outcome (CR-22). Each searched row
    // enumerates its pairs for CanStop(d) and CanContinue(d) (CR-18, S1). A
    // Matched row books none of them (CR-20, F19).
    fn produce_found(
        &self,
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        ticket: &mut E::Ticket,
        incoming: &Datum<A>,
        fire: Option<ReadyFire<C, P, A, K>>,
        search: RowSearchWithReads<'_, C, P, A, K>,
    ) -> Result<(Option<ReadyFire<C, P, A, K>>, RowParts<C, P, A, K>), SpaceTxnError> {
        let session = self.session();
        let (mut search, reads) = search;
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**session.space.matcher;
        let own_datum = ticket
            .owner_filter()
            .is_some_and(|filter| filter.requires_own_datum());
        let mut refused = Vec::new();
        verify_row_refusals(
            ticket,
            |identity| {
                let lists = RowLists::new(
                    &read_data,
                    &read_continuations,
                    &read_joins,
                    Some(identity),
                    &counter,
                );
                let mut candidates = RowCandidates::new(&mut *search, lists, None);
                let groups = candidates.groups();
                session.space.find_refused_produce_tuple_with(
                    &mut candidates,
                    incoming,
                    &groups,
                    identity,
                    own_datum,
                )
            },
            |ticket, index, found| {
                ticket
                    .verify_refusal(
                        index,
                        &found.comm,
                        &found.continuation.continuation,
                        found.continuation.persist,
                        &found.data,
                        Some(&incoming.a),
                    )
                    .map_err(SpaceTxnError::Host)?;
                let consume = &found.continuation.source.hash;
                refused.extend(
                    found
                        .data
                        .iter()
                        .map(|candidate| (candidate.datum.source.hash.clone(), consume.clone())),
                );
                Ok(())
            },
            || mismatch("a recorded refusal names no tuple of the row search"),
        )?;
        let Some(filter) = ticket.owner_filter() else {
            return Err(SpaceTxnError::Inconsistent("a searched replay row has no owner filter"));
        };
        let rule = ReplayItemRule(filter);
        let fired = fire.is_some();
        let selected = match &fire {
            Some(_) => {
                let lists =
                    RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
                let mut candidates = RowCandidates::new(&mut *search, lists, Some(&rule));
                let groups = candidates.groups();
                let selected = session.space.prepare_produce_candidate_with(
                    &mut candidates,
                    incoming,
                    &groups,
                    ticket.candidate_identity(),
                    Some(filter),
                )?;
                match selected {
                    Some(selected) => {
                        let channels = groups.get(selected.group).cloned().ok_or(
                            SpaceTxnError::Inconsistent("a selected tuple has no run of the row"),
                        )?;
                        Some((channels, selected))
                    }
                    None => None,
                }
            }
            None => None,
        };
        let (fire, actual, effects, deviated) = match (selected, fire, ticket.outcome()) {
            (None, None, _) => (None, None, None, false),
            (None, Some(_), NativeReplayOutcome::Stored) => {
                let effects = match search.keeps_staged_record() {
                    true => vec![StoreEffect::PutDatum(incoming.source.clone())],
                    false => Vec::new(),
                };
                (None, None, Some(effects), true)
            }
            (Some(_), _, NativeReplayOutcome::Stored) | (None, Some(_), _) => {
                return Err(mismatch("the row search does not give the recorded outcome"));
            }
            // Added by decision 124 (P2, R-end, REPLAY-END): the row fires the staged
            // tuple, which is the actual fire of play. The search keeps that fire, as
            // the arm below does, so a cut after a record end reads the unit of a
            // fire. The staged fire already holds the retires of the tuple.
            (Some((channels, selected)), Some(staged), _) if selected.comm == staged.comm => {
                let request = RowTupleRequest {
                    matcher,
                    group: &channels,
                    patterns: &selected.continuation.patterns,
                    continuation: &selected.continuation.continuation,
                    persistent: selected.continuation.persist,
                    source: &selected.continuation.source,
                    own: None,
                    item: None,
                };
                search.actual_fire(&request, &selected.data);
                (Some(staged), None, None, false)
            }
            (Some((channels, selected)), _, _) => {
                let request = RowTupleRequest {
                    matcher,
                    group: &channels,
                    patterns: &selected.continuation.patterns,
                    continuation: &selected.continuation.continuation,
                    persistent: selected.continuation.persist,
                    source: &selected.continuation.source,
                    own: None,
                    item: None,
                };
                let actual = search.actual_fire(&request, &selected.data);
                let next = match incoming.persist {
                    true => None,
                    false => Some(&incoming.source),
                };
                let fire = (channels, selected.continuation, selected.data);
                let (fire, effects) = ready_actual_produce_fire(self, store, reads, next, fire)?;
                (Some(fire), Some(actual), Some(effects), true)
            }
        };
        let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
        let (item_failed, vetoing) = row_enumeration(&mut *search, &lists, matcher, &rule, None)?;
        let stored = fire.is_none();
        let can_stop = !refused.is_empty() || !item_failed.is_empty() || (stored && fired);
        let can_continue = refused.is_empty() && !deviated;
        // A Matched row books no item pair (F19), and only a Stored or Parked
        // row is vetoing (CR-20, item 2). A head row lists its head only
        // (CR-2), so it deviates only to a store or a park.
        let (item_failed, vetoing) = match stored {
            true => (item_failed, vetoing),
            false => (Vec::new(), false),
        };
        let parts = RowParts {
            effects,
            deviated,
            refused,
            item_failed,
            actual,
            can_stop,
            can_continue,
            vetoing,
            claims: match deviated {
                false => None,
                true => search.actual_claims(),
            },
            search: Some(search),
        };
        Ok((fire, parts))
    }

    // Added by decision 124 (L15, L16, plan v4 C7.16, errata V4-6, V4-17,
    // V4-18): the replay search of a consume row, before the row commits. It
    // verifies the recorded refusals of the row over the search (R7). A staged
    // fire selects the recorded tuple over the search (X7). The staged tuple
    // c0 keeps the staged fire. Another tuple gives the actual fire, readied
    // against the store, and a recorded park drops the fire. Each searched row
    // enumerates its pairs for CanStop(d) and CanContinue(d) (CR-18, S1). A
    // Matched row books none of them (CR-20, F19).
    fn consume_found(
        &self,
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        ticket: &mut E::Ticket,
        waiting: (&[C], &WaitingContinuation<P, K>),
        fire: Option<ReadyConsumeFire<C, A>>,
        search: RowSearchWithReads<'_, C, P, A, K>,
    ) -> Result<(Option<ReadyConsumeFire<C, A>>, RowParts<C, P, A, K>), SpaceTxnError> {
        let session = self.session();
        let (channels, wk) = waiting;
        let (mut search, reads) = search;
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**session.space.matcher;
        let mut refused = Vec::new();
        verify_row_refusals(
            ticket,
            |identity| {
                let lists = RowLists::new(
                    &read_data,
                    &read_continuations,
                    &read_joins,
                    Some(identity),
                    &counter,
                );
                let mut candidates = RowCandidates::new(&mut *search, lists, None);
                session.space.find_refused_consume_tuple_with(
                    &mut candidates,
                    channels,
                    &wk.patterns,
                    &wk.continuation,
                    &wk.source,
                    &wk.peeks,
                    identity,
                )
            },
            |ticket, index, found| {
                ticket
                    .verify_refusal(
                        index,
                        &found.comm,
                        &wk.continuation,
                        wk.persist,
                        &found.data,
                        None,
                    )
                    .map_err(SpaceTxnError::Host)?;
                let consume = &wk.source.hash;
                refused.extend(
                    found
                        .data
                        .iter()
                        .map(|candidate| (candidate.datum.source.hash.clone(), consume.clone())),
                );
                Ok(())
            },
            || mismatch("a recorded refusal names no tuple of the row search"),
        )?;
        let Some(filter) = ticket.owner_filter() else {
            return Err(SpaceTxnError::Inconsistent("a searched replay row has no owner filter"));
        };
        let rule = ReplayItemRule(filter);
        let fired = fire.is_some();
        let selected = match &fire {
            Some(_) => {
                let lists =
                    RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
                let mut candidates = RowCandidates::new(&mut *search, lists, Some(&rule));
                session.space.prepare_consume_candidate_with(
                    &mut candidates,
                    channels,
                    &wk.patterns,
                    &wk.continuation,
                    &wk.source,
                    &wk.peeks,
                    ticket.candidate_identity(),
                    Some((filter, wk.persist)),
                )?
            }
            None => None,
        };
        let (fire, actual, effects, deviated) = match (selected, fire, ticket.outcome()) {
            (None, None, _) => (None, None, None, false),
            (None, Some(_), NativeReplayOutcome::Stored) => {
                let effects = actual_park_effects(store, reads, channels, wk.clone())?;
                (None, None, Some(effects), true)
            }
            (Some(_), _, NativeReplayOutcome::Stored) | (None, Some(_), _) => {
                return Err(mismatch("the row search does not give the recorded outcome"));
            }
            // Added by decision 124 (P2, R-end, REPLAY-END): the row fires the staged
            // tuple, which is the actual fire of play. The search keeps that fire, as
            // the arm below does, so a cut after a record end reads the unit of a
            // fire. The staged fire already holds the retires of the tuple.
            (Some(selected), Some(staged), _) if selected.comm == staged.comm => {
                let request = RowTupleRequest {
                    matcher,
                    group: channels,
                    patterns: &wk.patterns,
                    continuation: &wk.continuation,
                    persistent: wk.persist,
                    source: &wk.source,
                    own: None,
                    item: None,
                };
                search.actual_fire(&request, &selected.data);
                (Some(staged), None, None, false)
            }
            (Some(selected), _, _) => {
                let request = RowTupleRequest {
                    matcher,
                    group: channels,
                    patterns: &wk.patterns,
                    continuation: &wk.continuation,
                    persistent: wk.persist,
                    source: &wk.source,
                    own: None,
                    item: None,
                };
                let actual = search.actual_fire(&request, &selected.data);
                let source = (&wk.source, &wk.peeks);
                let (fire, effects) =
                    ready_actual_consume_fire(self, store, reads, source, channels, selected.data)?;
                (Some(fire), Some(actual), Some(effects), true)
            }
        };
        let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
        let (item_failed, vetoing) =
            row_enumeration(&mut *search, &lists, matcher, &rule, Some(&wk.continuation))?;
        let stored = fire.is_none();
        let can_stop = !refused.is_empty() || !item_failed.is_empty() || (stored && fired);
        let can_continue = refused.is_empty() && !deviated;
        // A Matched row books no item pair (F19), and only a Stored or Parked
        // row is vetoing (CR-20, item 2).
        let (item_failed, vetoing) = match stored {
            true => (item_failed, vetoing),
            false => (Vec::new(), false),
        };
        let parts = RowParts {
            effects,
            deviated,
            refused,
            item_failed,
            actual,
            can_stop,
            can_continue,
            vetoing,
            claims: match deviated {
                false => None,
                true => search.actual_claims(),
            },
            search: Some(search),
        };
        Ok((fire, parts))
    }

    // Retired by decision 124 (L10): the arms call the base `publish_denial`
    // of `operations.rs`, as the base arms do. The flag is gone, so the
    // wrapper has no step of its own. The text before:
    // ```
    // // R5 (A11): a recorded denial is published with its outcome, and the
    // // operation fails as play fails it, with the error that the epoch names
    // // for its denial (DR-113).
    // fn publish_denial(
    //     &self,
    //     ticket: E::Ticket,
    //     outcome: NativeReplayOutcome,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    //     let session = self.session();
    //     let mut completion = ticket.prepare(outcome)?;
    //     let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
    //         session.epoch.invalidate()
    //     });
    //     completion.publish();
    //     publication.complete();
    //     self.denied.store(true, Ordering::Release);
    //     // Changed by decision 89 (DR-113): the epoch names the error of its
    //     // denial, because the class of an exhaustion depends on the bound
    //     // source, as `operations.rs` returns it for a host operation.
    //     // Err(SpaceTxnError::Host(RSpaceError::OutOfPhlogistons))
    //     Err(SpaceTxnError::Host(session.epoch.denial()))
    // }
    // ```

    // Retired by decision 123 (plan v4 C3g) and decision 124 (review N, M3):
    // base `observe_comm` checks the COMM source before it observes
    // (`authenticate_comm_source` of the rholang replay). The text before:
    // ```
    // // R4 step 9 (first part): the staged COMM is the COMM of the recorded
    // // candidate. `observe_comm` checks it again.
    // fn check_candidate(&self, ticket: &E::Ticket, comm: &COMM) -> Result<(), SpaceTxnError> {
    //     let identity = ticket
    //         .candidate_identity()
    //         .ok_or_else(|| mismatch("a recorded fire has no candidate identity"))?;
    //     match identity.matches_comm(comm) {
    //         true => Ok(()),
    //         false => Err(mismatch("the staged COMM differs from the recorded candidate")),
    //     }
    // }
    // ```

    // Changed by decision 92: the footprint names the footprint key
    // `footprint_key` as one more join group, as play records it.
    // fn apply_produce(
    //     &self,
    //     store: &NativeSpaceStore<O, C, P, A, K, E>,
    //     produce: StagedProduce<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    fn apply_produce(
        &self,
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        produce: StagedProduce<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
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
        // ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        let joins = footprint_joins(&joins, footprint_key);
        ticket.authenticate_footprint(std::slice::from_ref(&channel), &joins)?;
        // Step 7.
        let outcome = ticket.outcome();
        // Changed by decision 90 (DR-132): the readiness check shares the rule
        // (`outcome_fits`).
        // let fits = match (&fire, outcome) {
        //     (None, NativeReplayOutcome::Stored |
        // NativeReplayOutcome::DeniedIntroduction) => true,     (
        //         Some(_),
        //         NativeReplayOutcome::Matched |
        //         NativeReplayOutcome::DeniedIntroduction |
        //         NativeReplayOutcome::DeniedComm,
        //     ) => true,
        //     _ => false,
        // };
        let fits = outcome_fits(fire.is_some(), outcome) ||
            matches!((&fire, &search, outcome), (Some(_), Some(_), NativeReplayOutcome::Stored));
        if !fits {
            return Err(mismatch("the recorded outcome does not fit the staged produce"));
        }
        // Step 8.
        let decision = ticket.observe_produce(&produce_ref, &channel, &data, &authority)?;
        // Changed by decision 92: an owner-refused introduction replays as a
        // skip (G3, DR-135, S6), as `operations.rs` replays it.
        // if outcome == NativeReplayOutcome::DeniedIntroduction {
        //     require(decision, NativeReplayDecision::Denied)?;
        //     return self.publish_denial(ticket, outcome);
        // }
        match outcome {
            NativeReplayOutcome::DeniedIntroduction => {
                require(decision, NativeReplayDecision::Denied)?;
                // Changed by decision 124 (L10): the arm calls the base
                // `publish_denial`, as the base arm does. The text before:
                // return self.publish_denial(ticket, outcome);
                return session
                    .publish_denial(ticket, outcome)
                    .map(|()| skipped_row(OpSource::Produce(produce_ref.clone()), true))
                    .map_err(SpaceTxnError::Host);
            }
            NativeReplayOutcome::SkippedIntroduction => {
                require(decision, NativeReplayDecision::OwnerRefused)?;
                // Changed by decision 124 (L10): the arm calls the base
                // `publish_denial`, as the base arm does. The text before:
                // return self.publish_denial(ticket, outcome);
                return session
                    .publish_denial(ticket, outcome)
                    .map(|()| skipped_row(OpSource::Produce(produce_ref.clone()), true))
                    .map_err(SpaceTxnError::Host);
            }
            NativeReplayOutcome::Stored |
            NativeReplayOutcome::Matched |
            NativeReplayOutcome::DeniedComm => {}
        }
        require(decision, NativeReplayDecision::Granted)?;
        // Added by decision 124 (L15, L16, plan v4 C7.16, errata V4-17): under
        // owner funding a row with a search runs its replay search before it
        // commits (`produce_found`). The search may replace the staged fire by
        // the actual fire of the row, or drop it for a recorded store.
        let (fire, parts) = match (search, ticket.owner_filter().is_some()) {
            (Some(search), true) => {
                let incoming = Datum {
                    a: data.clone(),
                    persist,
                    source: produce_ref.clone(),
                };
                self.produce_found(store, &mut ticket, &incoming, fire, search)?
            }
            (_, _) => (fire, RowParts::staged()),
        };
        match fire {
            None => {
                // Steps 10 to 12, with the store step of a stored host
                // produce (`operations.rs`): the data of the channel are in
                // the store before the publication.
                let key = StoreKey::from_digest(&produce_ref.channel_hash)?;
                session.prefetch_data(&channel, key)?;
                // Added by decision 124 (ruling RADD2 1.2, J-STORE, rule CR-19
                // case 3, F22): a row that deviates to a store writes nothing
                // when its search keeps no staged record, or when the store
                // holds the persistent record already, as play does.
                let keeps = match (parts.deviated, parts.search.as_deref()) {
                    (true, Some(search)) => search.keeps_staged_record(),
                    (_, _) => true,
                };
                let writes = match (parts.deviated, keeps, persist) {
                    (true, false, _) => false,
                    (true, true, true) => !store
                        .data(&channel, &produce_ref.channel_hash)?
                        .iter()
                        .any(|datum| datum.source == produce_ref),
                    (true, true, false) | (false, _, _) => true,
                };
                let counter = session
                    .space
                    .prepare_produce_counter(&produce_ref, persist)?;
                let mut completion = ticket.prepare(NativeReplayOutcome::Stored)?;
                let publication = PublicationGuard::with_invalidation(&session.unavailable, || {
                    session.epoch.invalidate()
                });
                // Added by decision 90 (C6): a reporting session logs the
                // stored produce, so it keeps a copy of the datum that the
                // store takes, as `operations.rs` does for a host produce.
                let logged = session.logging.then(|| data.clone());
                // Changed by decision 89 (DR-96): the session store takes the
                // datum, so the export of the session holds it.
                // store_data(store, channel, data, persist, produce_ref.clone());
                // Changed by decision 124 (ruling RADD2 1.2, J-STORE, rule CR-19
                // case 3, F22): a row that `writes` denies writes no datum. The
                // text before:
                // ```
                // session.store.put_datum(&channel, key, Datum {
                //     a: data,
                //     persist,
                //     source: produce_ref.clone(),
                // })?;
                // ```
                match writes {
                    // Changed by CD-2 (clippy -D warnings, the final
                    // verification of merge 2): the empty arm comes first
                    // (`clippy::single_match`). The text before:
                    // true => {
                    //     session.store.put_datum(&channel, key, Datum {
                    //         a: data,
                    //         persist,
                    //         source: produce_ref.clone(),
                    //     })?;
                    // }
                    // false => {}
                    false => {}
                    true => {
                        session.store.put_datum(&channel, key, Datum {
                            a: data,
                            persist,
                            source: produce_ref.clone(),
                        })?;
                    }
                }
                counter.publish();
                completion.publish();
                publication.complete();
                events.push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
                // Added by decision 90 (C6): a reporting session logs the
                // stored produce, as `space_replay.rs` logs it.
                if let Some(data) = logged {
                    session
                        .space
                        .log_produce(produce_ref.clone(), &channel, &data, persist);
                }
                // Changed by decision 124 (L15, L16, plan v4 C7.16): the row
                // gives its applied row, with the parts of its search. The
                // text before:
                // ```
                // Ok(SpaceOpReceipt::Stored {
                //     produce: produce_ref,
                // })
                // ```
                let receipt = SpaceOpReceipt::Stored {
                    produce: produce_ref,
                };
                Ok(AppliedRow { receipt, parts })
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
                // Retired by decision 123 (plan v4 C3g) and decision 124
                // (review N, M3): base `observe_comm` checks the COMM source.
                // self.check_candidate(&ticket, &comm)?;
                let decision = ticket.observe_comm(
                    &comm,
                    &continuation.continuation,
                    continuation.persist,
                    &candidates,
                )?;
                if outcome == NativeReplayOutcome::DeniedComm {
                    require(decision, NativeReplayDecision::Denied)?;
                    // Changed by decision 124 (L10): the arm calls the base
                    // `publish_denial`, as the base arm does. The text before:
                    // return self.publish_denial(ticket, outcome);
                    return session
                        .publish_denial(ticket, outcome)
                        .map(|()| skipped_row(OpSource::Produce(produce_ref.clone()), true))
                        .map_err(SpaceTxnError::Host);
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
                // Added by decision 90 (C6): a reporting session logs the
                // matched produce and its COMM, as `space_replay.rs` logs them
                // and as `operations.rs` logs a matched host produce.
                if session.logging {
                    session
                        .space
                        .log_produce(produce_ref.clone(), &channel, &data, persist);
                    session.space.log_comm(
                        &candidates,
                        &channels,
                        continuation.clone(),
                        comm.clone(),
                        "comm.produce",
                    );
                }
                let consume_ref = continuation.source.clone();
                let fired = candidates.len();
                let wrapped =
                    session
                        .space
                        .wrap_result(channels, continuation, consume_ref, candidates);
                // Changed by decision 124 (L15, L16, plan v4 C7.16): the row
                // gives its applied row, with the parts of its search. The
                // text before:
                // fired_receipt(comm, wrapped, fired, Some(returned))
                fired_receipt(comm, wrapped, fired, Some(returned))
                    .map(|receipt| AppliedRow { receipt, parts })
            }
        }
    }

    // Changed by decision 92: the footprint names the footprint key
    // `footprint_key` as its one join group, as play records it.
    // fn apply_consume(
    //     &self,
    //     store: &NativeSpaceStore<O, C, P, A, K, E>,
    //     consume: StagedConsume<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    fn apply_consume(
        &self,
        store: &NativeSpaceStore<O, C, P, A, K, E>,
        consume: StagedConsume<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
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
        // Changed by decision 92: a consume authenticates its channels and the
        // footprint key as its one join group.
        run_before_observe(before_observe, source)?;
        let (authority, mut ticket) = self.begin_row(source)?;
        // ticket.authenticate_footprint(&channels, &[])?;
        let joins = footprint_joins(&[], footprint_key);
        ticket.authenticate_footprint(&channels, &joins)?;
        // Step 7.
        let outcome = ticket.outcome();
        // Changed by decision 90 (DR-132): the readiness check shares the rule
        // (`outcome_fits`).
        // let fits = match (&fire, outcome) {
        //     (None, NativeReplayOutcome::Stored |
        // NativeReplayOutcome::DeniedIntroduction) => true,     (
        //         Some(_),
        //         NativeReplayOutcome::Matched |
        //         NativeReplayOutcome::DeniedIntroduction |
        //         NativeReplayOutcome::DeniedComm,
        //     ) => true,
        //     _ => false,
        // };
        let fits = outcome_fits(fire.is_some(), outcome) ||
            matches!((&fire, &search, outcome), (Some(_), Some(_), NativeReplayOutcome::Stored));
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
        // Changed by decision 92: an owner-refused introduction replays as a
        // skip (G3, DR-135, S6), as `operations.rs` replays it.
        // if outcome == NativeReplayOutcome::DeniedIntroduction {
        //     require(decision, NativeReplayDecision::Denied)?;
        //     return self.publish_denial(ticket, outcome);
        // }
        match outcome {
            NativeReplayOutcome::DeniedIntroduction => {
                require(decision, NativeReplayDecision::Denied)?;
                // Changed by decision 124 (L10): the arm calls the base
                // `publish_denial`, as the base arm does. The text before:
                // return self.publish_denial(ticket, outcome);
                return session
                    .publish_denial(ticket, outcome)
                    .map(|()| skipped_row(OpSource::Consume(consume_ref.clone()), true))
                    .map_err(SpaceTxnError::Host);
            }
            NativeReplayOutcome::SkippedIntroduction => {
                require(decision, NativeReplayDecision::OwnerRefused)?;
                // Changed by decision 124 (L10): the arm calls the base
                // `publish_denial`, as the base arm does. The text before:
                // return self.publish_denial(ticket, outcome);
                return session
                    .publish_denial(ticket, outcome)
                    .map(|()| skipped_row(OpSource::Consume(consume_ref.clone()), true))
                    .map_err(SpaceTxnError::Host);
            }
            NativeReplayOutcome::Stored |
            NativeReplayOutcome::Matched |
            NativeReplayOutcome::DeniedComm => {}
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
        // Added by decision 124 (L15, L16, plan v4 C7.16, errata V4-17): under
        // owner funding a row with a search runs its replay search before it
        // commits (`consume_found`). The search may replace the staged fire by
        // the actual fire of the row, or drop it for a recorded park.
        let (fire, parts) = match (search, ticket.owner_filter().is_some()) {
            (Some(search), true) => {
                let waiting = (channels.as_slice(), &wk);
                self.consume_found(store, &mut ticket, waiting, fire, search)?
            }
            (_, _) => (fire, RowParts::staged()),
        };
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
                // Added by decision 90 (C6): a reporting session logs the
                // parked consume, so it keeps a copy of the parts of the
                // continuation that the store takes, as `operations.rs` does
                // for a host consume.
                let logged = session
                    .logging
                    .then(|| (wk.patterns.clone(), wk.continuation.clone(), wk.peeks.clone()));
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
                // Added by decision 90 (C6): a reporting session logs the
                // parked consume, as `space_replay.rs` logs it.
                if let Some((patterns, continuation, peeks)) = logged {
                    session.space.log_consume(
                        consume_ref.clone(),
                        &channels,
                        &patterns,
                        &continuation,
                        persist,
                        &peeks,
                    );
                }
                // Changed by decision 124 (L15, L16, plan v4 C7.16): the row
                // gives its applied row, with the parts of its search. The
                // text before:
                // ```
                // Ok(SpaceOpReceipt::Parked {
                //     consume: consume_ref,
                // })
                // ```
                let receipt = SpaceOpReceipt::Parked {
                    consume: consume_ref,
                };
                Ok(AppliedRow { receipt, parts })
            }
            Some(ReadyConsumeFire {
                fire: StagedConsumeFire { candidates, .. },
                comm,
                committed,
            }) => {
                // Step 9.
                // Retired by decision 123 (plan v4 C3g) and decision 124
                // (review N, M3): base `observe_comm` checks the COMM source.
                // self.check_candidate(&ticket, &comm)?;
                let decision =
                    ticket.observe_comm(&comm, &wk.continuation, wk.persist, &candidates)?;
                if outcome == NativeReplayOutcome::DeniedComm {
                    require(decision, NativeReplayDecision::Denied)?;
                    // Changed by decision 124 (L10): the arm calls the base
                    // `publish_denial`, as the base arm does. The text before:
                    // return self.publish_denial(ticket, outcome);
                    return session
                        .publish_denial(ticket, outcome)
                        .map(|()| skipped_row(OpSource::Consume(consume_ref.clone()), true))
                        .map_err(SpaceTxnError::Host);
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
                // Added by decision 90 (C6): a reporting session logs the
                // matched consume and its COMM, as `space_replay.rs` logs them
                // and as `operations.rs` logs a matched host consume.
                if session.logging {
                    session.space.log_consume(
                        consume_ref.clone(),
                        &channels,
                        &wk.patterns,
                        &wk.continuation,
                        wk.persist,
                        &wk.peeks,
                    );
                    session.space.log_comm(
                        &candidates,
                        &channels,
                        wk.clone(),
                        comm.clone(),
                        "comm.consume",
                    );
                }
                let fired = candidates.len();
                let wrapped = session
                    .space
                    .wrap_result(channels, wk, consume_ref, candidates);
                // Changed by decision 124 (L15, L16, plan v4 C7.16): the row
                // gives its applied row, with the parts of its search. The
                // text before:
                // fired_receipt(comm, wrapped, fired, None)
                fired_receipt(comm, wrapped, fired, None)
                    .map(|receipt| AppliedRow { receipt, parts })
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
    // Changed by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
    // 8.7): a funded script names the owner checks of replay. The text before:
    // use crate::rspace::replay_rspace::native_epoch::{
    //     NativeCandidateIdentity, NativeReplayBoundary, NativeReplayEpoch,
    //     NativeReplayRestore,
    // };
    use crate::rspace::replay_rspace::native_epoch::{
        NativeCandidateIdentity, NativeOwnerFilter, NativeReplayBoundary, NativeReplayEpoch,
        NativeReplayRestore,
    };
    use crate::rspace::rspace::RSpace;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    // Changed by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
    // 8.7): the test of the cut names the parts of its scripted search. The text
    // before:
    // use crate::rspace::space_store_handle::{
    //     ConsumeOutcome, ProduceOutcome, SharedStore, SpaceConsume,
    //     SpaceProduce, SpaceStoreHandle, SpaceTxnReceipt, bind_candidate,
    //     incoming_candidate, installed_channel_keys,
    // };
    use crate::rspace::space_store_handle::{
        ActualFire, ConsumeOutcome, FinishRow, ProduceOutcome, RowItemRule, RowReissue, RowRun,
        SharedStore, SpaceConsume, SpaceProduce, SpaceStoreHandle, SpaceTxnReceipt, bind_candidate,
        incoming_candidate, installed_channel_keys,
    };
    // Changed by decision 90 (C6, step D): the file no longer imports `COMM` at
    // its top, and these tests name it. The text before:
    // use crate::rspace::trace::event::Consume;
    use crate::rspace::trace::event::{COMM, Consume};

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

        // Added by decision 92: a skipped introduction (G3, DR-135, S6). Its owner
        // could not pay, so the decision of the introduction is `OwnerRefused`.
        fn skipped_introduction() -> Self {
            Self {
                outcome: NativeReplayOutcome::SkippedIntroduction,
                introduction: NativeReplayDecision::OwnerRefused,
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
        // Added by decision 90 (DR-132): the readiness checks that still report
        // `NotReady`, and the log of the checks and waits of the transactions.
        unready_checks: Arc<std::sync::atomic::AtomicUsize>,
        readiness_log: Arc<Mutex<Vec<String>>>,
        // Added by decision 102 (decision 119 S2): the scripted answers of the
        // lookup of the recorded rows, and the log of the lookups.
        recorded_rows: Arc<Mutex<VecDeque<Option<RecordedRow>>>>,
        lookup_log: Arc<Mutex<Vec<String>>>,
        // Added by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
        // 8.7): whether the tickets of the script have owner funding.
        funded: Arc<AtomicBool>,
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

        // Changed by decision 123 (plan v4 C3g) and decision 124 (review N, M3):
        // the scripted ticket checks the COMM source before it observes, as
        // base `observe_comm` does (`authenticate_comm_source`), because
        // `check_candidate` retires. The failure keeps the text of the retired
        // check, so the negative controls keep their error. The text before:
        // ```
        // fn observe_comm(
        //     &mut self,
        //     _source: &COMM,
        //     _continuation: &String,
        //     _persistent: bool,
        //     _data: &[ConsumeCandidate<String, String>],
        // ) -> Result<NativeReplayDecision, RSpaceError> {
        //     self.script.note("observe_comm");
        //     Ok(self.row.comm)
        // }
        // ```
        fn observe_comm(
            &mut self,
            source: &COMM,
            _continuation: &String,
            _persistent: bool,
            _data: &[ConsumeCandidate<String, String>],
        ) -> Result<NativeReplayDecision, RSpaceError> {
            match self.identity.matches_comm(source) {
                true => {
                    self.script.note("observe_comm");
                    Ok(self.row.comm)
                }
                false => Err(RSpaceError::InterpreterError(
                    "native space replay: the staged COMM differs from the recorded candidate"
                        .to_owned(),
                )),
            }
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

        // Added by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
        // 8.7): a ticket of a funded script has the owner checks of `Funding`.
        fn owner_filter(&self) -> Option<&dyn NativeOwnerFilter<String, String, String, String>> {
            match self.script.funded.load(Ordering::Acquire) {
                false => None,
                true => Some(&Funding),
            }
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

        // Added by decision 95: the scripted readiness of the first row. A
        // check logs the kind of the first operation ("produce" or "consume")
        // and passes, or it reports `NotReady` at `SCRIPTED_VERSION` while the
        // script holds unready checks.
        fn space_transaction_ready(
            &self,
            first: RSpaceOperationSource<'_>,
        ) -> Result<SpaceTransactionReadiness, RSpaceError> {
            let kind = match first {
                RSpaceOperationSource::Produce(_) => "produce",
                RSpaceOperationSource::Consume(_) => "consume",
            };
            self.readiness_log
                .lock()
                .expect("readiness log lock")
                .push(format!("check {kind}"));
            let unready =
                self.unready_checks
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                        count.checked_sub(1)
                    });
            match unready {
                Ok(_) => Ok(SpaceTransactionReadiness::NotReady {
                    seen: SCRIPTED_VERSION,
                }),
                Err(_) => Ok(SpaceTransactionReadiness::Ready),
            }
        }

        // Added by decision 95: a scripted wait logs its version and returns.
        async fn wait_space_change(&self, seen: u64) -> Result<(), RSpaceError> {
            self.readiness_log
                .lock()
                .expect("readiness log lock")
                .push(format!("wait {seen}"));
            Ok(())
        }

        // Added by decision 102 (decision 119 S2): a scripted lookup logs the
        // kind of its operation and its `previous` row, and it takes the next
        // scripted answer. Without a scripted answer it gives None, which
        // stages the transaction in DR-75 order (rule CR-5).
        fn space_recorded_row(
            &self,
            source: RSpaceOperationSource<'_>,
            previous: Option<usize>,
            _footprint_key: &String,
        ) -> Result<Option<RecordedRow>, RSpaceError> {
            let kind = match source {
                RSpaceOperationSource::Produce(_) => "produce",
                RSpaceOperationSource::Consume(_) => "consume",
            };
            self.lookup_log
                .lock()
                .expect("lookup log lock")
                .push(format!("recorded {kind} {previous:?}"));
            Ok(self
                .recorded_rows
                .lock()
                .expect("recorded rows lock")
                .pop_front()
                .flatten())
        }
    }

    // Added by decision 90 (DR-132): the version that a scripted `NotReady`
    // reports.
    const SCRIPTED_VERSION: u64 = 7;

    // Disabled by decision 95: the epoch of the session checks and waits, so
    // the scripted epoch (`space_transaction_ready` and `wait_space_change`)
    // replaces the scripted readiness wait of the host.

    impl Script {
        // Added by decision 90 (DR-132).
        fn readiness_log(&self) -> Vec<String> {
            self.readiness_log
                .lock()
                .expect("readiness log lock")
                .clone()
        }

        // Added by decision 102 (plan v4 C3b): scripts the answers of the next
        // lookups of the recorded rows, in order.
        fn script_rows(&self, rows: impl IntoIterator<Item = Option<RecordedRow>>) {
            self.recorded_rows
                .lock()
                .expect("recorded rows lock")
                .extend(rows);
        }

        // Added by decision 102 (plan v4 C3b): the log of the lookups so far.
        fn lookups(&self) -> Vec<String> {
            self.lookup_log.lock().expect("lookup log lock").clone()
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
    // Changed by decision 90 (DR-132): the host takes the scripted readiness wait
    // of the epoch of the session.
    // Changed by decision 95: the scripted epoch of the session checks and
    // waits, so the host takes no readiness wait.
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
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
        // Changed by decision 124 (L10): no failure closes the epoch. The text
        // before:
        // ```
        // assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        // ```
        assert!(
            !script.log().contains(&"invalidate".to_owned()),
            "a failure leaves the epoch open (decision 124, L10)"
        );
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
        // Changed by decision 124 (L10): no failure closes the epoch. The text
        // before:
        // ```
        // assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        // ```
        assert!(
            !script.log().contains(&"invalidate".to_owned()),
            "a failure leaves the epoch open (decision 124, L10)"
        );
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
            // Changed by decision 124 (L10): no failure closes the epoch now, a
            // denial included. The text before:
            // "a recorded denial keeps the epoch open"
            "a recorded denial keeps the epoch open, as every failure does (L10)"
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
    // stripes, so it never waits, and the test ends. Changed by decision 124
    // (L10): the failure leaves the epoch open. The text before: "The failure
    // closes the epoch."
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
        // Changed by decision 124 (L10): the failure leaves the epoch open. A
        // waiter of the row parks counted, and the stall rule ends its wait
        // (DR-132). The text before:
        // // The failure closes the epoch, so an operation that waits for the row
        // // wakes and fails.
        // assert_eq!(script.log(), vec!["begin produce", "invalidate"]);
        assert_eq!(script.log(), vec!["begin produce"]);
        // The view load caches the empty history value of `a`. No record is stored.
        assert!(
            StoreState::of_session(&session)
                .data
                .get(&s("a"))
                .is_none_or(Vec::is_empty)
        );
    }

    // Added by decision 90 (DR-132): a transaction whose rows are not ready fails
    // with `NotReady` before its first operation. It begins no row, makes no
    // observation, stores nothing and keeps the epoch open (review amendment
    // (e)). Its stripes are free after the commit, so the job can wait and run
    // again, and the second run commits through its row.
    #[tokio::test]
    async fn native_space_replay_waits_when_the_transaction_is_not_ready() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.unready_checks.store(1, Ordering::Release);
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let error = txn
            .commit()
            .await
            .expect_err("an unready transaction waits");
        assert_eq!(error, SpaceTxnError::NotReady {
            seen: SCRIPTED_VERSION
        });
        assert!(script.log().is_empty(), "nothing began, observed or invalidated");
        assert!(
            StoreState::of_session(&session)
                .data
                .get(&s("a"))
                .is_none_or(Vec::is_empty)
        );
        drop(txn);
        handle
            .wait_changed(SCRIPTED_VERSION)
            .await
            .expect("the job waits without a transaction");
        script.push(Row::stored());
        let mut txn = handle.begin(&[s("a")]).await.expect("begin again");
        txn.produce(store_op("a", "x")).expect("stage x again");
        txn.commit().await.expect("the second run commits");
        // Changed by decision 95: the scripted epoch logs the kind of the first
        // operation.
        assert_eq!(script.readiness_log(), vec![
            "check produce".to_owned(),
            format!("wait {SCRIPTED_VERSION}"),
            "check produce".to_owned(),
        ]);
        assert_eq!(
            StoreState::of_session(&session)
                .data
                .get(&s("a"))
                .map(Vec::len),
            Some(1)
        );
    }

    // Added by decision 90 (DR-132): the check reads every staged operation of the
    // transaction, in staging order and before the first one applies, with the
    // staged COMM of a fire.
    // Disabled by decision 92: the check reads the first staged operation only.
    // `native_space_replay_checks_the_first_staged_row_only` checks that rule.

    // Added by decision 92: the check reads the first staged operation only,
    // before it applies (`SpaceReplayOrder.tla`, `FirstReady`). The park of k
    // checks its consume. The next transaction stages a consume and then a
    // fire, and its check reads the consume only. A transaction that is not
    // ready begins no row.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_space_replay_checks_the_first_staged_row_only() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        park_k(&script, &handle).await;
        script.clear();
        script.unready_checks.store(1, Ordering::Release);
        let mut txn = handle.begin(&[s("a"), s("b")]).await.expect("begin");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        txn.consume(park_op("a", "j")).expect("stage the park of j");
        txn.produce(fire_op("b", "y", &waiting))
            .expect("stage the fire");
        let error = txn
            .commit()
            .await
            .expect_err("an unready transaction waits");
        assert_eq!(error, SpaceTxnError::NotReady {
            seen: SCRIPTED_VERSION
        });
        assert!(script.log().is_empty(), "no operation began");
        assert_eq!(script.readiness_log(), vec![
            "check consume".to_owned(),
            "check consume".to_owned(),
        ]);
    }

    // Added by decision 92: every row of a transaction with a footprint key
    // authenticates the key as one more join group of its footprint, as play
    // records it. A produce adds the group after its joins, and a consume
    // has the key as its one group.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_space_replay_authenticates_the_footprint_key() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        script.push(Row::stored());
        let mut txn = handle
            .begin(&[s("a"), s("b"), s("S")])
            .await
            .expect("begin");
        txn.set_footprint_key(s("S")).expect("S is a lock key");
        let mut first = store_op("a", "x");
        first.own = true;
        txn.produce(first).expect("stage x");
        txn.consume(park_op("b", "k")).expect("stage the park");
        txn.commit().await.expect("commit");
        assert_eq!(script.log(), vec![
            "begin produce",
            "footprint [\"a\"] [[\"S\"]]",
            "observe_produce a",
            "prepare Stored",
            "publish",
            "begin consume",
            "footprint [\"b\"] [[\"S\"]]",
            "observe_consume [\"b\"]",
            "prepare Stored",
            "publish",
        ]);
        script.clear();
        let mut txn = handle.begin(&[s("b"), s("S")]).await.expect("begin");
        txn.set_footprint_key(s("S")).expect("S is a lock key");
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let mut fire = fire_op("b", "z", &waiting);
        fire.own = true;
        let staged = txn.produce(fire).expect("stage the fire of k");
        script.push(Row::matched(staged.comm.clone()));
        txn.commit().await.expect("commit the fire");
        assert_eq!(&script.log()[..2], &["begin produce", "footprint [\"b\"] [[\"b\"], [\"S\"]]",]);
    }

    // Added by decision 92: a skipped introduction (G3, DR-135, S6) publishes its
    // row with its outcome. The row commits as `Skipped`, as play skips it (R8,
    // decision 124, L10), and it stops the commit (CR-18). The operations before
    // it stay applied, and the epoch stays open.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_space_replay_publishes_a_skipped_introduction() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        script.push(Row::skipped_introduction());
        let mut txn = handle.begin(&[s("a"), s("b")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        txn.consume(park_op("b", "k")).expect("stage the park");
        let receipt = txn.commit().await.expect("the refusal skips its row");
        assert_eq!(receipt.ops.len(), 2);
        assert!(matches!(receipt.ops[1], SpaceOpReceipt::Skipped { .. }));
        assert_eq!(receipt.deviation.map(|deviation| deviation.index), Some(1));
        assert_eq!(&script.log()[5..], &[
            "begin consume",
            "footprint [\"b\"] []",
            "observe_consume [\"b\"]",
            "prepare SkippedIntroduction",
            "publish",
        ]);
        assert!(
            !script.log().contains(&"invalidate".to_owned()),
            // Changed by decision 124 (L10): no failure closes the epoch now, a
            // skipped introduction included.
            "a skipped introduction keeps the epoch open, as every failure does (L10)"
        );
        let state = StoreState::of_session(&session);
        assert_eq!(state.data.get(&s("a")).map(Vec::len), Some(1));
        assert!(
            state
                .continuations
                .get(&vec![s("b")])
                .is_none_or(Vec::is_empty),
            "the refused park stores nothing"
        );
    }

    // Added by decision 90 (item 3.10, and decision 91 SQ-4): the hold of a commit
    // keeps its stripes, so a transaction on its channels begins only after the
    // hold drops. While the hold lives, its task holds an open transaction of
    // the handle, so it cannot begin another one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_space_replay_hold_keeps_the_stripes_of_a_commit() {
        use std::time::Duration;

        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.push(Row::stored());
        let mut txn = handle.begin(&[s("a")]).await.expect("begin");
        txn.produce(store_op("a", "x")).expect("stage x");
        let (receipt, hold) = txn.commit_holding().await.expect("commit and hold");
        assert_eq!(receipt.ops.len(), 1);
        assert!(matches!(handle.begin(&[s("b")]).await, Err(SpaceTxnError::NestedTransaction)));
        let other = handle.clone();
        let mut waiting = tokio::spawn(async move { other.begin(&[s("a")]).await.map(drop) });
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut waiting)
                .await
                .is_err(),
            "the stripe of a is held"
        );
        drop(hold);
        tokio::time::timeout(Duration::from_secs(10), waiting)
            .await
            .expect("the begin proceeds after the hold drops")
            .expect("the other task joins")
            .expect("the other transaction begins");
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
    // Changed by decision 102 (rules CR-4, CR-5 and R13): native replay still
    // pairs nothing, but it names a record by the COMM of its recorded row, as
    // replay does. A transaction without a footprint key has no chain, so it
    // follows no row, and `recorded_comms` gives None without a lookup. The
    // staged view counts the staged produce, so the record is named with
    // `incoming` false and not with `incoming` true. The text before: "S1:
    // native replay stages as play: it pairs nothing and admits every
    // candidate."
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
        // Changed by decision 102 (rules CR-4 and R13): the named-record test of
        // replay.
        // assert_eq!(txn.admits(&comm, &incoming, true), Ok(true));
        assert_eq!(txn.admits(&comm, &incoming, false), Ok(true));
        assert_eq!(txn.admits(&comm, &incoming, true), Ok(false));
        assert!(script.lookups().is_empty(), "a transaction without a chain looks up no row");
    }

    // Added by decision 102 (plan v4 C3b): a transaction over the channels
    // `channels` and the footprint key `S`, whose rows form one chain.
    async fn begin_chained(
        handle: &Handle,
        channels: &[&str],
    ) -> BoxedSpaceTxn<String, String, String, String> {
        let mut keys = Vec::with_capacity(channels.len() + 1);
        keys.extend(channels.iter().map(|channel| s(channel)));
        keys.push(s("S"));
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.set_footprint_key(s("S")).expect("S is a lock key");
        txn
    }

    // Added by decision 102 (plan v4 C3b): the own store of `data` on
    // `channel`, which can be the first operation of a chained transaction.
    fn own_store_op(channel: &str, data: &str) -> SpaceProduce<String, String, String, String> {
        let mut op = store_op(channel, data);
        op.own = true;
        op
    }

    // Added by decision 102 (plan v4 C3b): a recorded row with the journal
    // index `journal`, the outcome `outcome` and no COMM.
    fn row_at(journal: usize, outcome: NativeReplayOutcome) -> Option<RecordedRow> {
        Some((journal, outcome, None))
    }

    // Added by decision 102 (decision 119 S1, plan v4 C3b, rules CR-4 and
    // CR-5): operation 0 takes the first open row of its source, and each
    // later operation takes the successor of the row of the operation before
    // it. The lookup of `recorded_comms` waits for the stage call of its
    // source, so each operation looks up once. A Stored row names no COMM.
    #[tokio::test]
    async fn native_space_replay_follows_the_chain_of_the_footprint_key() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([
            row_at(4, NativeReplayOutcome::Stored),
            row_at(9, NativeReplayOutcome::Stored),
        ]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        let x = Produce::create(&s("a"), &s("x"), false);
        assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), Some(Vec::new()));
        assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&x)), Some(Vec::new()));
        txn.produce(own_store_op("a", "x")).expect("stage x");
        assert_eq!(script.lookups(), vec!["recorded produce None"]);
        txn.consume(park_op("b", "k")).expect("stage the park");
        assert_eq!(script.lookups(), vec!["recorded produce None", "recorded consume Some(4)"]);
        script.push(Row::stored());
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("commit");
        assert_eq!(receipt.ops.len(), 2);
        // Added by decision 124 (L11, L12): no cut, no deviation and no refused
        // pair (V4-17).
        assert_eq!(receipt.deviation, None);
        assert!(receipt.refused.is_empty());
    }

    // Added by decision 102 (decision 119 S1, plan v4 C3b, rule CR-6): a stage
    // call whose source has no successor row of the followed row stages
    // nothing and fails with `RecordEnded`.
    // Changed by decision 124 (L11): the commit needs CanStop of the last row
    // of the cut. The record stays ended.
    #[tokio::test]
    async fn native_space_replay_ends_the_record_at_a_successor_miss() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([row_at(4, NativeReplayOutcome::Stored), None]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "x")).expect("stage x");
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        assert_eq!(script.lookups(), vec!["recorded produce None", "recorded consume Some(4)"]);
        assert!(!txn.end_record(), "an ended record does not end again");
        script.push(Row::stored());
        // Changed by decision 124 (L11): a cut needs CanStop of its last row.
        // A Stored row whose staging had no allowed candidate has none, so the
        // commit fails with the session mismatch after the row applied.
        let error = txn.commit().await.expect_err("a cut without CanStop fails");
        assert_eq!(error, SpaceTxnError::Host(super::super::mismatch()));
        let state = StoreState::of_session(&session);
        assert!(
            state
                .continuations
                .get(&vec![s("b")])
                .is_none_or(Vec::is_empty),
            "the call after the record end stages nothing"
        );
    }

    // Added by plan v4 errata (V4-12, rule CR-5): the row of a leading anchor
    // is not a followed row. The error rule: a staging error after the anchor
    // keeps the failure path, so `end_record` is false. The miss rule: a
    // stage call without a successor row of the anchor row frees the cursor
    // and stages its operation. The rest of the transaction stages in DR-75
    // mode and looks up no row.
    #[tokio::test]
    async fn native_space_replay_frees_the_cursor_at_a_miss_after_a_leading_anchor() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([row_at(4, NativeReplayOutcome::Stored), None]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "anchor"))
            .expect("stage the anchor");
        txn.mark_leading_anchor();
        assert!(!txn.end_record(), "the error rule keeps the failure path");
        txn.consume(park_op("b", "k"))
            .expect("the miss rule stages the park");
        assert!(!txn.end_record(), "a free cursor keeps the failure path");
        let y = Produce::create(&s("a"), &s("y"), false);
        assert_eq!(txn.recorded_comms(RSpaceOperationSource::Produce(&y)), None);
        txn.produce(store_op("a", "y"))
            .expect("stage y in DR-75 mode");
        assert_eq!(script.lookups(), vec!["recorded produce None", "recorded consume Some(4)"]);
        for _ in 0..3 {
            script.push(Row::stored());
        }
        let receipt = txn.commit().await.expect("commit");
        assert_eq!(receipt.ops.len(), 3);
    }

    // Added by plan v4 errata (V4-12, rule CR-32): after the transaction
    // followed a row after its leading anchor, a staging error ends the
    // staging. `end_record` ends the record once, and a later stage call
    // fails with `RecordEnded`. A mark after the first operation changes
    // nothing.
    #[tokio::test]
    async fn native_space_replay_ends_the_record_after_a_followed_row() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([
            row_at(4, NativeReplayOutcome::Stored),
            row_at(9, NativeReplayOutcome::Stored),
        ]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "anchor"))
            .expect("stage the anchor");
        txn.mark_leading_anchor();
        txn.consume(park_op("b", "k")).expect("stage the park");
        txn.mark_leading_anchor();
        assert!(txn.end_record(), "a staging error after a followed row ends the staging");
        assert!(!txn.end_record(), "the record ends once");
        assert_eq!(txn.produce(store_op("a", "y")).err(), Some(SpaceTxnError::RecordEnded));
        assert_eq!(script.lookups(), vec!["recorded produce None", "recorded consume Some(4)"]);
    }

    // Added by decision 124 (L11, CR-32): after a followed row, a staging
    // error ends the staging (`end_record`). The commit of the cut needs
    // CanStop of its last row. The park on `b` is Stored with no data, so it
    // has no CanStop, and the commit fails with the session mismatch.
    #[tokio::test]
    async fn native_space_replay_fails_a_cr32_cut_without_can_stop() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([
            row_at(4, NativeReplayOutcome::Stored),
            row_at(9, NativeReplayOutcome::Stored),
        ]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "anchor"))
            .expect("stage the anchor");
        txn.mark_leading_anchor();
        txn.consume(park_op("b", "k")).expect("stage the park");
        assert!(txn.end_record(), "a staging error after a followed row ends the staging");
        script.push(Row::stored());
        script.push(Row::stored());
        let error = txn
            .commit()
            .await
            .expect_err("a CR-32 cut without CanStop fails");
        assert_eq!(error, SpaceTxnError::Host(super::super::mismatch()));
        assert!(!script.log().contains(&"invalidate".to_owned()));
    }

    // Added by decision 124 (L11, V4-19): a followed row that publishes a
    // denial fails the commit with the denial error. The unit rule does not
    // run for that row, so a record end after it changes nothing. This is the
    // order of base `publish_denial`.
    #[tokio::test]
    async fn native_space_replay_fails_a_denied_cut_with_the_denial() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([row_at(4, NativeReplayOutcome::DeniedIntroduction), None]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "x")).expect("stage x");
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        script.push(Row::denied_introduction());
        let error = txn.commit().await.expect_err("the denial fails the commit");
        assert_eq!(error, SpaceTxnError::Host(RSpaceError::SignedLimitExhausted));
        assert!(script.log().contains(&"publish".to_owned()));
        assert!(!script.log().contains(&"invalidate".to_owned()));
    }

    // Added by decision 124 (plan of the batch B tests, B16, rulings RADD2v2
    // 6.6 (f) and 8.7, form (i)): the owner checks of a funded script. Every
    // item passes, the row refused no COMM, and a produce keeps its own datum
    // (R5, R6, R7).
    struct Funding;

    impl NativeOwnerFilter<String, String, String, String> for Funding {
        fn admit_item(
            &self,
            _continuation: &String,
            _continuation_persistent: bool,
            _channel_count: usize,
            _datum: &String,
            _datum_persistent: bool,
        ) -> Result<bool, RSpaceError> {
            Ok(true)
        }

        fn names_refusal_for(&self, _consume: &Consume) -> bool { false }

        fn requires_own_datum(&self) -> bool { true }
    }

    // Added by decision 124 (plan of the batch B tests, B16, rulings RADD2v2
    // 6.6 (f) and 8.7, form (i)): a scripted row search with no run. So the
    // replay search of its row finds no tuple: a store keeps its row, and a
    // fire that the record stores deviates (C2v3 6.18 (g)). The search gives
    // `failed` as the item failures of its row and `tail` as its tail read, and
    // it hands out `reissue` and `finish` once.
    struct CutSearch {
        failed: Vec<(Blake2b256Hash, Blake2b256Hash)>,
        tail: TailRead,
        reissue: Option<RowReissue>,
        finish: Vec<FinishRow<String, String, String, String>>,
    }

    impl RowSearch<String, String, String, String> for CutSearch {
        fn runs(&self) -> Vec<RowRun<String>> { Vec::new() }

        fn continuations(
            &mut self,
            _run: usize,
            _lists: &RowLists<'_, String, String, String, String>,
        ) -> Result<Vec<(Arc<WaitingContinuation<String, String>>, i32)>, SpaceTxnError> {
            Ok(Vec::new())
        }

        fn tuple(
            &mut self,
            _run: usize,
            _lists: &RowLists<'_, String, String, String, String>,
            _request: RowTupleRequest<'_, String, String, String, String>,
        ) -> Result<Option<Vec<ConsumeCandidate<String, String>>>, SpaceTxnError> {
            Ok(None)
        }

        fn restore(&mut self) {}

        fn commit_check(
            &mut self,
            _request: &RowTupleRequest<'_, String, String, String, String>,
            _tuple: &[ConsumeCandidate<String, String>],
        ) -> Result<bool, SpaceTxnError> {
            Ok(true)
        }

        fn may_veto(
            &self,
            _matcher: &dyn Match<String, String, String>,
            _continuation: &String,
        ) -> bool {
            false
        }

        fn staged_retires(&self) -> usize { 0 }

        fn actual_fire(
            &mut self,
            _request: &RowTupleRequest<'_, String, String, String, String>,
            _tuple: &[ConsumeCandidate<String, String>],
        ) -> ActualFire<String, String, String, String> {
            ActualFire {
                retires: Vec::new(),
                dispatched: None,
            }
        }

        fn item_failures(
            &self,
            _lists: &RowLists<'_, String, String, String, String>,
            _matcher: &dyn Match<String, String, String>,
            _rule: &dyn RowItemRule<String, String>,
        ) -> Result<Vec<(Blake2b256Hash, Blake2b256Hash)>, SpaceTxnError> {
            Ok(self.failed.clone())
        }

        fn tail_read(
            &mut self,
            _lists: &RowLists<'_, String, String, String, String>,
            _matcher: &dyn Match<String, String, String>,
        ) -> Result<TailRead, SpaceTxnError> {
            Ok(self.tail)
        }

        fn tail_reissue(&mut self) -> Option<RowReissue> { self.reissue.take() }

        fn finish_rows(&mut self) -> Vec<FinishRow<String, String, String, String>> {
            std::mem::take(&mut self.finish)
        }
    }

    // Added by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
    // 8.7): the boxed search of a row with `failed` item failures, the tail
    // read `tail`, the re-issue `reissue` and the finish rows `finish`.
    fn cut_search(
        failed: usize,
        tail: TailRead,
        reissue: Option<RowReissue>,
        finish: Vec<FinishRow<String, String, String, String>>,
    ) -> Box<dyn RowSearch<String, String, String, String>> {
        let pair = (Blake2b256Hash::new(b"record"), Blake2b256Hash::new(b"waiter"));
        Box::new(CutSearch {
            failed: vec![pair; failed],
            tail,
            reissue,
            finish,
        })
    }

    // Added by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
    // 8.7): a script whose tickets have owner funding, its session and its
    // handle.
    async fn funded_session() -> (Script, Arc<Session>, Handle) {
        let script = Script::default();
        script.funded.store(true, Ordering::Release);
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        (script, session, handle)
    }

    // Added by decision 124 (plan of the batch B tests, B16, ruling RADD2v2
    // 8.7): parks `k` on `b`, then stages in a chained transaction the own fire
    // of `k` with `z`, which carries the search `search`.
    async fn staged_fire_of_k(
        script: &Script,
        handle: &Handle,
        search: Box<dyn RowSearch<String, String, String, String>>,
    ) -> BoxedSpaceTxn<String, String, String, String> {
        park_k(script, handle).await;
        let mut txn = begin_chained(handle, &["b"]).await;
        let waiting = txn.continuations(&[s("b")]).expect("read [b]")[0].clone();
        let mut fire = fire_op("b", "z", &waiting);
        fire.own = true;
        fire.search = Some(search);
        txn.produce(fire).expect("stage the fire of k");
        txn
    }

    // Added by decision 124 (plan of the batch B tests, B16, rulings RADD2v2
    // 6.6 (f) and 8.7, form (i)): the one test of the cut. Native replay builds
    // the cut of each case in a run of its transaction, with a scripted search
    // and owner funding. Cases 1 to 3 are those of ADD A2.9, and cases 4 to 6
    // are U6: R-dev, R-end and the mismatch.
    #[tokio::test]
    async fn native_replay_cut_reads_the_search_of_its_row() {
        // Case 1: a Skipped row with no search, then a record end. The row
        // deviates, so the cut reads `Ends` with no re-issue (F-V4-11).
        let (script, _session, handle) = funded_session().await;
        script.script_rows([row_at(4, NativeReplayOutcome::SkippedIntroduction), None]);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        txn.produce(own_store_op("a", "x")).expect("stage x");
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        script.push(Row::skipped_introduction());
        let receipt = txn.commit().await.expect("case 1: a Skipped row can stop");
        let cut = receipt.deviation.expect("case 1 cuts");
        assert_eq!((cut.index, cut.tail_read), (0, TailRead::Ends));
        assert_eq!(cut.reissue, None);

        // Case 2: a Stored row with a search, which does not deviate, then a
        // record end. The cut takes the answer and the re-issue of the search,
        // and the item failure of the row gives CanStop.
        let (script, _session, handle) = funded_session().await;
        script.script_rows([row_at(4, NativeReplayOutcome::Stored), None]);
        let reissue = RowReissue::new(2_u8);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        let mut store = own_store_op("a", "x");
        store.search = Some(cut_search(1, TailRead::Reissues, Some(reissue.clone()), Vec::new()));
        txn.produce(store).expect("stage x");
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("case 2: a failure can stop");
        let cut = receipt.deviation.expect("case 2 cuts");
        assert_eq!((cut.index, cut.tail_read), (0, TailRead::Reissues));
        assert_eq!(cut.reissue, Some(reissue));
        assert_eq!(cut.item_failed.len(), 1);

        // Case 3: the fire of `k` with a search, which the record stores. The
        // row deviates as the last row, and the cut reads its search.
        let (script, _session, handle) = funded_session().await;
        let reissue = RowReissue::new(3_u8);
        let search = cut_search(0, TailRead::Stages, Some(reissue.clone()), Vec::new());
        let mut txn = staged_fire_of_k(&script, &handle, search).await;
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("case 3: a deviation can stop");
        assert!(matches!(receipt.ops.as_slice(), [SpaceOpReceipt::Stored { .. }]));
        let cut = receipt.deviation.expect("case 3 deviates");
        assert_eq!((cut.index, cut.tail_read), (0, TailRead::Stages));
        assert_eq!(cut.reissue, Some(reissue));

        // Case 4, U6 (a), R-dev: as case 3, with one finish row that the later
        // staged row follows. The finish row commits after the group, and the
        // unit is the last unit.
        let (script, _session, handle) = funded_session().await;
        let finish = vec![FinishRow::Store(store_op("b", "y"))];
        let search = cut_search(0, TailRead::Stages, None, finish);
        let mut txn = staged_fire_of_k(&script, &handle, search).await;
        txn.produce(store_op("b", "y")).expect("stage y");
        script.push(Row::stored());
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("case 4: y is a finish row");
        assert!(matches!(receipt.ops.as_slice(), [
            SpaceOpReceipt::Stored { .. },
            SpaceOpReceipt::Stored { .. }
        ]));
        assert_eq!(receipt.deviation.map(|cut| cut.index), Some(0));

        // Case 5, U6 (b), R-end: a Stored row with a search and one finish row,
        // a later row that follows it, then a record end. Row 0 is the last row
        // with a search, so the cut moves to row 0 with the scripted re-issue.
        let (script, _session, handle) = funded_session().await;
        script.script_rows([
            row_at(4, NativeReplayOutcome::Stored),
            row_at(9, NativeReplayOutcome::Stored),
            None,
        ]);
        let reissue = RowReissue::new(5_u8);
        let mut txn = begin_chained(&handle, &["a", "b"]).await;
        let mut store = own_store_op("a", "x");
        let finish = vec![FinishRow::Store(store_op("a", "y"))];
        store.search = Some(cut_search(1, TailRead::Reissues, Some(reissue.clone()), finish));
        txn.produce(store).expect("stage x");
        txn.produce(store_op("a", "y")).expect("stage y");
        assert_eq!(txn.consume(park_op("b", "k")).err(), Some(SpaceTxnError::RecordEnded));
        script.push(Row::stored());
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("case 5: y is a finish row");
        assert_eq!(receipt.ops.len(), 2);
        let cut = receipt.deviation.expect("case 5 cuts at row 0");
        assert_eq!((cut.index, cut.tail_read), (0, TailRead::Reissues));
        assert_eq!(cut.reissue, Some(reissue));

        // Case 6, U6 (c): as case 4, but the later row stores `w`, which no
        // finish row names. The unit before the last cannot continue, so the
        // commit fails with the session mismatch.
        let (script, _session, handle) = funded_session().await;
        let finish = vec![FinishRow::Store(store_op("b", "y"))];
        let search = cut_search(0, TailRead::Stages, None, finish);
        let mut txn = staged_fire_of_k(&script, &handle, search).await;
        txn.produce(store_op("b", "w")).expect("stage w");
        script.push(Row::stored());
        script.push(Row::stored());
        let error = txn.commit().await.expect_err("case 6: w is no finish row");
        assert_eq!(error, SpaceTxnError::Host(super::super::mismatch()));
    }

    // Added by decision 102 (plan v4 C3b, F6): the discard of the staging
    // starts the cursor again, so the next operation 0 takes the first open
    // row of its source again.
    #[tokio::test]
    async fn native_space_replay_discard_starts_the_cursor_again() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        script.script_rows([
            row_at(4, NativeReplayOutcome::Stored),
            row_at(4, NativeReplayOutcome::Stored),
        ]);
        let mut txn = begin_chained(&handle, &["a"]).await;
        txn.produce(own_store_op("a", "x")).expect("stage x");
        txn.discard_staged().expect("discard the staging");
        assert!(!txn.end_record(), "a discarded staging follows no row");
        txn.produce(own_store_op("a", "x")).expect("stage x again");
        assert_eq!(script.lookups(), vec!["recorded produce None", "recorded produce None"]);
        script.push(Row::stored());
        let receipt = txn.commit().await.expect("commit");
        assert_eq!(receipt.ops.len(), 1);
    }

    // Added by decision 102 (decision 119 S1, plan v4 C3b, rule CR-4): the
    // answer of `recorded_comms` for each outcome of a recorded row. A
    // Matched or DeniedComm row names its COMM. A Stored row names no COMM.
    // An introduction row, a Matched row without a COMM, or no row gives
    // None. Each source looks up once, and no stage call moves the cursor.
    #[tokio::test]
    async fn native_space_replay_names_the_comm_of_the_recorded_row() {
        let script = Script::default();
        let session = session(&script, Vec::new()).await;
        let handle = handle(&session);
        let comm = COMM {
            consume: Consume::default(),
            produces: Vec::new(),
            peeks: BTreeSet::new(),
            times_repeated: Default::default(),
        };
        script.script_rows([
            Some((1, NativeReplayOutcome::Matched, Some(comm.clone()))),
            Some((2, NativeReplayOutcome::DeniedComm, Some(comm.clone()))),
            row_at(3, NativeReplayOutcome::Stored),
            row_at(4, NativeReplayOutcome::SkippedIntroduction),
            row_at(5, NativeReplayOutcome::DeniedIntroduction),
            row_at(6, NativeReplayOutcome::Matched),
            None,
        ]);
        let mut txn = begin_chained(&handle, &["a"]).await;
        let sources: Vec<Produce> = (0..7)
            .map(|value| Produce::create(&s("a"), &value.to_string(), false))
            .collect();
        let answers: Vec<Option<Vec<COMM>>> = sources
            .iter()
            .map(|source| txn.recorded_comms(RSpaceOperationSource::Produce(source)))
            .collect();
        assert_eq!(answers, vec![
            Some(vec![comm.clone()]),
            Some(vec![comm]),
            Some(Vec::new()),
            None,
            None,
            None,
            None,
        ]);
        assert_eq!(script.lookups(), vec!["recorded produce None"; 7]);
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
        // Changed by decision 124 (L10): no failure closes the epoch. The text
        // before:
        // ```
        // assert!(script.log().contains(&"invalidate".to_owned()), "the failure closes the epoch");
        // ```
        assert!(
            !script.log().contains(&"invalidate".to_owned()),
            "a failure leaves the epoch open (decision 124, L10)"
        );
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
        // Changed by decision 124 (L10): no failure closes the epoch either.
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
        // Changed by decision 124 (L10): no failure closes the epoch either.
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
        // it stay applied, and the failed operation has no effect. Changed by
        // decision 124 (L10): the failure leaves the epoch open. A waiter of its
        // row parks counted, and the stall rule ends the wait. The text before:
        // "the failure closes the epoch, so no operation waits for its row."
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
                // Changed by decision 124 (L10): the failure leaves the epoch
                // open. The text before:
                // proptest::prop_assert!(script.log().contains(&"invalidate".to_owned()));
                proptest::prop_assert!(!script.log().contains(&"invalidate".to_owned()));
                Ok::<(), proptest::test_runner::TestCaseError>(())
            })?;
        }
    }
}
