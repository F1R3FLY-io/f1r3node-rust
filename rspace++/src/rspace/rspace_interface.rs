// See rspace/src/main/scala/coop/rchain/rspace/ISpace.scala

use std::collections::{BTreeSet, HashMap};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::checkpoint::Checkpoint;
use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, ProduceCandidate, Row, WaitingContinuation};
use super::operation_context::OperationOrder;
use super::trace::Log;
use super::trace::event::{COMM, Consume, Produce};
use crate::rspace::checkpoint::SoftCheckpoint;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct RSpaceResult<C, A> {
    pub channel: C,
    pub matched_datum: A,
    pub removed_datum: A,
    pub persistent: bool,
}

// NOTE: On Scala side, they are defaulting "peek" to false
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct ContResult<C, P, K> {
    pub continuation: K,
    pub persistent: bool,
    pub channels: Vec<C>,
    pub patterns: Vec<P>,
    pub peek: bool,
}

pub type MaybeProduceCandidate<C, P, A, K> = Option<ProduceCandidate<C, P, A, K>>;
pub type MaybeConsumeResult<C, P, A, K> = Option<(ContResult<C, P, K>, Vec<RSpaceResult<C, A>>)>;
pub type MaybeProduceResult<C, P, A, K> =
    Option<(ContResult<C, P, K>, Vec<RSpaceResult<C, A>>, Produce)>;

#[derive(Clone, Copy, Debug)]
pub enum RSpaceOperationSource<'a> {
    Produce(&'a Produce),
    Consume(&'a Consume),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RSpaceOperationCompletion {
    Stored,
    Matched,
    Rejected,
}

#[derive(Clone, Debug)]
pub enum ReplayOperationDirective {
    Store,
    RejectedIntroduction,
    AcceptedComm(std::sync::Arc<COMM>),
    RejectedComm(std::sync::Arc<COMM>),
}

impl RSpaceOperationCompletion {
    pub fn from_result<T>(result: &Result<Option<T>, RSpaceError>) -> Self {
        match result {
            Ok(Some(_)) => Self::Matched,
            Ok(None) => Self::Stored,
            Err(_) => Self::Rejected,
        }
    }
}

pub trait RSpaceAccountingObserver<C, P, A, K>: Send + Sync {
    fn replay_operation_directive(
        &self,
        _source: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayOperationDirective>, RSpaceError> {
        Ok(None)
    }

    // Changed by DR-132: the start hook returns the name of the recorded
    // operation, and RSpace runs the rest of the locked operation under it.
    // Racing evaluation sets no outer name.
    // ) -> Result<(), RSpaceError> {
    //     Ok(())
    // }
    fn observe_operation_start(
        &self,
        _source: RSpaceOperationSource<'_>,
        _channels: &[C],
        _joins: &[Vec<C>],
    ) -> Result<Option<OperationOrder>, RSpaceError> {
        Ok(None)
    }

    fn observe_operation_finish(
        &self,
        _source: RSpaceOperationSource<'_>,
        _completion: RSpaceOperationCompletion,
    ) {
    }

    fn observe_produce(
        &self,
        source: &Produce,
        channel: &C,
        data: &A,
        persistent: bool,
    ) -> Result<(), RSpaceError>;

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError>;

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &K,
        continuation_persistent: bool,
        data: &[(&A, bool)],
    ) -> Result<(), RSpaceError>;

    /// Added by G3 (DR-135, S5): whether the observer checks owners in the
    /// running operation (R5, R6). Without owner funding it is false, and
    /// RSpace matches as on dev.
    fn owner_checks_active(&self) -> bool { false }

    /// Added by G3 (DR-135, S5): whether a continuation may take `datum` as
    /// one item of a match, before the other items are known (R5). A false
    /// result makes the datum a miss of its pattern.
    fn admit_item(
        &self,
        _continuation: &K,
        _continuation_persistent: bool,
        _channel_count: usize,
        _datum: &A,
        _datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        Ok(true)
    }

    /// Added by G3 (DR-135, S5): `observe_comm` with the channel and the
    /// stored value of each datum (R9). The default observes the COMM as
    /// `observe_comm` does.
    fn observe_comm_participants(
        &self,
        comm: &COMM,
        continuation: &K,
        continuation_persistent: bool,
        participants: &[CommParticipant<'_, C, A>],
    ) -> Result<(), RSpaceError> {
        let mut data = Vec::with_capacity(participants.len());
        data.extend(
            participants
                .iter()
                .map(|participant| (participant.matched, participant.persistent)),
        );
        self.observe_comm(comm, continuation, continuation_persistent, &data)
    }
}

/// Added by G3 (DR-135, S5): one datum of a COMM: its channel, the value that
/// the continuation binds, the value that the store held, and whether the
/// datum is persistent. A peek restores the stored value (R9).
pub struct CommParticipant<'a, C, A> {
    pub channel: &'a C,
    pub matched: &'a A,
    pub stored: &'a A,
    pub persistent: bool,
}

/// Added by DR-132: the accounting observer of a locked operation and the
/// name of its recorded row. The rest of the operation runs under the name.
pub type AccountingStart<C, P, A, K> =
    (Option<std::sync::Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>, Option<OperationOrder>);

/** The interface for RSpace
 *
 * @tparam C a type representing a channel
 * @tparam P a type representing a pattern
 * @tparam A a type representing an arbitrary piece of data and match result
 * @tparam K a type representing a continuation
 *
 * The traits 'Tuplespace' and 'IReplayRSpace' have been combined into this
 * trait
 *
 */
#[async_trait]
pub trait ISpace<
    C: Eq + std::hash::Hash + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
>: Send + Sync
{
    /// Install (or clear) the per-ISpace accounting observer.  Called
    /// at runtime boot after the ReductionCoordinator mints the
    /// observer; `None` disables accounting (default, triage-mode).
    ///
    /// Default no-op so existing ISpace impls on triage don't
    /// require changes; the `RSpace` / `ReplayRSpace` /
    /// `ReportingRspace` impls will override once the production
    /// consumer lands.
    fn set_accounting_observer(
        &self,
        _observer: Option<std::sync::Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>,
    ) {
    }

    /** Creates a checkpoint.
     *
     * @return A [[Checkpoint]]
     */
    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError>;

    async fn get_data(&self, channel: &C) -> Vec<Datum<A>>;

    async fn get_waiting_continuations(&self, channels: Vec<C>) -> Vec<WaitingContinuation<P, K>>;

    async fn get_joins(&self, channel: C) -> Vec<Vec<C>>;

    /// Remove every datum stored on `channel`.  Default: returns
    /// `BugFoundError` — triage hasn't ported the removal path yet.
    /// Needed by `deterministic_reduction`'s hot-store compaction;
    /// implementors (RSpace / ReplayRSpace / ReportingRspace) will
    /// override once that slice lands.
    async fn remove_all_data(&self, _channel: &C) -> Result<(), RSpaceError> {
        Err(RSpaceError::BugFoundError(
            "ISpace::remove_all_data not yet ported to triage".to_string(),
        ))
    }

    /// Remove the datum at `index` on `channel`.  Default: see
    /// `remove_all_data`.
    async fn remove_data_at(&self, _channel: &C, _index: i32) -> Result<(), RSpaceError> {
        Err(RSpaceError::BugFoundError(
            "ISpace::remove_data_at not yet ported to triage".to_string(),
        ))
    }

    /// Record-oriented variant: remove the datum at `index` and
    /// log an operation with `operation_id`.  Default: see
    /// `remove_all_data`.
    async fn remove_data_at_recorded(
        &self,
        _channel: &C,
        _index: i32,
        _operation_id: &[u8],
    ) -> Result<(), RSpaceError> {
        Err(RSpaceError::BugFoundError(
            "ISpace::remove_data_at_recorded not yet ported to triage".to_string(),
        ))
    }

    /// Remove every continuation registered on `channels`.
    /// Default: see `remove_all_data`.
    async fn remove_all_continuations(&self, _channels: Vec<C>) -> Result<(), RSpaceError>
    where C: 'async_trait {
        Err(RSpaceError::BugFoundError(
            "ISpace::remove_all_continuations not yet ported to triage".to_string(),
        ))
    }

    /** Clears the store.  Does not affect the history trie.
     */
    async fn clear(&self) -> Result<(), RSpaceError>;

    /// Return current history root hash without creating a checkpoint.
    async fn get_root(&self) -> Blake2b256Hash;

    /** Resets the store to the given root.
     *
     * @param root A BLAKE2b256 Hash representing the checkpoint
     */
    async fn reset(&self, root: &Blake2b256Hash) -> Result<(), RSpaceError>;

    async fn consume_result(
        &self,
        channel: Vec<C>,
        pattern: Vec<P>,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError>;

    // TODO: this should not be exposed - OLD
    async fn to_map(&self) -> HashMap<Vec<C>, Row<P, A, K>>;

    /**
    Allows to create a "soft" checkpoint which doesn't persist the checkpointed data into history.
    This operation is significantly faster than {@link #createCheckpoint()} because the computationally
    expensive operation of creating the history trie is avoided.
    */
    async fn create_soft_checkpoint(&self) -> SoftCheckpoint<C, P, A, K>;

    /// Drain and return the in-memory event log without cloning the hot-store
    /// snapshot. This is a lightweight alternative when only logs are
    /// needed.
    async fn take_event_log(&self) -> Log;

    /**
    Reverts the ISpace to the state checkpointed using {@link #createSoftCheckpoint()}
    */
    async fn revert_to_soft_checkpoint(
        &self,
        checkpoint: SoftCheckpoint<C, P, A, K>,
    ) -> Result<(), RSpaceError>;

    /* TUPLESPACE */

    /** Searches the store for data matching all the given patterns at the
     * given channels.
     *
     * If no match is found, then the continuation and patterns are put in
     * the store at the given channels.
     *
     * If a match is found, then the continuation is returned along with the
     * matching data.
     *
     * Matching data stored with the `persist` flag set to `true` will not
     * be removed when it is retrieved. See below for more information
     * about using the `persist` flag.
     *
     * '''NOTE''':
     *
     * A call to [[consume]] that is made with the persist flag set to
     * `true` only persists when there is no matching data.
     *
     * This means that in order to make a continuation "stick" in the store,
     * the user will have to continue to call [[consume]] until a `None`
     * is received.
     *
     * @param channels A Seq of channels on which to search for matching
     * data @param patterns A Seq of patterns with which to search for
     * matching data @param continuation A continuation
     * @param persist Whether or not to attempt to persist the data
     */
    async fn consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError>;

    /** Searches the store for a continuation that has patterns that match
     * the given data at the given channel.
     *
     * If no match is found, then the data is put in the store at the given
     * channel.
     *
     * If a match is found, then the continuation is returned along with the
     * matching data.
     *
     * Matching data or continuations stored with the `persist` flag set to
     * `true` will not be removed when they are retrieved. See below for
     * more information about using the `persist` flag.
     *
     * '''NOTE''':
     *
     * A call to [[produce]] that is made with the persist flag set to
     * `true` only persists when there are no matching continuations.
     *
     * This means that in order to make a piece of data "stick" in the
     * store, the user will have to continue to call [[produce]] until a
     * `None` is received.
     *
     * @param channel A channel on which to search for matching
     * continuations and/or store data @param data A piece of data
     * @param persist Whether or not to attempt to persist the data
     */
    async fn produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError>;

    async fn install(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    ) -> Result<Option<(K, Vec<A>)>, RSpaceError>;

    /* REPLAY */

    async fn rig_and_reset(&self, start_root: Blake2b256Hash, log: Log) -> Result<(), RSpaceError>;

    async fn rig(&self, log: Log) -> Result<(), RSpaceError>;

    async fn check_replay_data(&self) -> Result<(), RSpaceError>;

    async fn is_replay(&self) -> bool;

    async fn update_produce(&self, produce: Produce) -> ();

    /// Mark the reporting phase that the next flushed segment belongs to
    async fn set_report_phase(&self, _phase: super::reporting_rspace::ReportPhase) {}
}
