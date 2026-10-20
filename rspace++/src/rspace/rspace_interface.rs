// See rspace/src/main/scala/coop/rchain/rspace/ISpace.scala

use std::collections::{BTreeSet, HashMap};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::checkpoint::Checkpoint;
use super::errors::RSpaceError;
use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, ProduceCandidate, Row, WaitingContinuation};
// Disabled by the merge of feature/cost-accounted-rho (DR-131): only the
// removed `get_data_cold_decode_metered` default named this trait.
// use super::serializers::cold_store_decode::ColdStoreDecode;
use super::trace::Log;
use super::trace::event::{COMM, Consume, Produce, RestingCOMM};
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

pub trait ProduceCommitGuard: Send + Sync {
    fn with_commit(&self, commit: Box<dyn FnOnce() + '_>) -> Result<(), RSpaceError>;
}

pub fn commit_produce<T>(
    guard: Option<&dyn ProduceCommitGuard>,
    commit: impl FnOnce() -> T,
) -> Result<T, RSpaceError> {
    match guard {
        None => Ok(commit()),
        Some(guard) => {
            let mut result = None;
            let decision = guard.with_commit(Box::new(|| result = Some(commit())));
            match (result, decision) {
                (Some(result), Ok(())) => Ok(result),
                (None, Err(error)) => Err(error),
                (Some(_), Err(_)) => Err(RSpaceError::ProduceCommitProtocolViolation),
                (None, Ok(())) => Err(RSpaceError::ProduceCommitProtocolViolation),
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RSpaceOperationSource<'a> {
    Produce(&'a Produce),
    Consume(&'a Consume),
    /// Protocol-7 resting commit; it is not a new IO introduction.
    Resting(&'a RestingCOMM),
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

    fn observe_operation_start(
        &self,
        _source: RSpaceOperationSource<'_>,
        _channels: &[C],
        _joins: &[Vec<C>],
    ) -> Result<(), RSpaceError> {
        Ok(())
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

    /// A pre-existing waiter matches pre-existing data. The default refuses
    /// so older observers cannot accidentally charge it as an ordinary COMM
    /// attached to fabricated Produce/Consume IO.
    fn observe_resting_comm(
        &self,
        _identity: &Blake2b256Hash,
        _event: &RestingCOMM,
        _continuation: &K,
        _continuation_persistent: bool,
        _data: &[(&A, bool)],
    ) -> Result<(), RSpaceError> {
        Err(RSpaceError::ProduceCommitDenied)
    }
}

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
    fn codec_epoch(&self) -> super::hashing::stable_hash_provider::EventCodecEpoch;

    fn set_accounting_observer(
        &self,
        observer: Option<std::sync::Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>,
    );

    /** Creates a checkpoint.
     *
     * @return A [[Checkpoint]]
     */
    async fn create_checkpoint(&self) -> Result<Checkpoint, RSpaceError>;

    async fn get_data(&self, channel: &C) -> Result<Vec<Datum<A>>, RSpaceError>;

    async fn get_waiting_continuations(
        &self,
        channels: Vec<C>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>;

    async fn get_joins(&self, channel: C) -> Result<Vec<Vec<C>>, RSpaceError>;

    async fn remove_all_data(&self, channel: &C) -> Result<(), RSpaceError>;

    async fn remove_data_at(&self, channel: &C, index: i32) -> Result<(), RSpaceError>;

    async fn remove_data_at_recorded(
        &self,
        channel: &C,
        index: i32,
        operation_id: &[u8],
    ) -> Result<(), RSpaceError>;

    async fn remove_all_continuations(&self, channels: Vec<C>) -> Result<(), RSpaceError>;

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

    /// The MeTTaIL reactive single-stepper's back-pressure gate, if a step
    /// session is active on this space. Default `None` (production and
    /// replay never pause). `RSpace` overrides this to forward to its
    /// installed [`super::logging::StepCommObserver`], letting the reducer
    /// pause after each committed COMM with no construction-path threading.
    /// Synchronous and cheap — a `None` return is one observer `is_none`
    /// check.
    fn step_gate(&self) -> Option<std::sync::Arc<super::logging::StepGate>> { None }

    /// Forward a non-COMM structural reduction (dereference, method,
    /// match/if/new/bundle body, or a produce reaching quiescence) to the
    /// installed [`super::logging::StepCommObserver`], if any.
    /// The symmetric twin of [`Self::step_gate`]: default `None`-op (production
    /// and replay never observe), `RSpace` overrides it to forward to its
    /// observer, and the reducer reaches it through `self.space` with no
    /// construction-path threading. Synchronous and cheap — the default is a
    /// no-op, the `RSpace` override is one observer `is_none` check.
    fn observe_reduction(&self, _redex: &C, _kind: super::logging::ReductionKind) {}

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

    async fn produce_guarded(
        &self,
        channel: C,
        data: A,
        persist: bool,
        guard: &dyn ProduceCommitGuard,
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

    // Changed by the merge of feature/cost-accounted-rho (finding R2-08):
    // DR-131 removed the host-work meter, so these entrypoints take none.
    // /// The authenticated protocol-7 replay entrypoints require a finite
    // /// caller-owned host-work meter. Defaults refuse; a legacy `ISpace`
    // /// implementation must never infer CostV2 authority from an event body.
    /// The authenticated protocol-7 replay entrypoints. Defaults refuse; a
    /// legacy `ISpace` implementation must never infer CostV2 authority from
    /// an event body.
    async fn rig_cost_v2(&self, _start_root: Blake2b256Hash, _log: Log) -> Result<(), RSpaceError> {
        Err(RSpaceError::ProduceCommitProtocolViolation)
    }

    async fn replay_produce_cost_v2(
        &self,
        _channel: C,
        _data: A,
        _persist: bool,
        _guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError>
    where
        C: 'static,
        A: 'static,
    {
        Err(RSpaceError::ProduceCommitProtocolViolation)
    }

    async fn replay_consume_cost_v2(
        &self,
        _channels: Vec<C>,
        _patterns: Vec<P>,
        _continuation: K,
        _persist: bool,
        _peeks: BTreeSet<i32>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError>
    where
        C: 'static,
        P: 'static,
        K: 'static,
    {
        Err(RSpaceError::ProduceCommitProtocolViolation)
    }

    async fn replay_resting_cost_v2(
        &self,
        _candidate: &ProduceCandidate<C, P, A, K>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        Err(RSpaceError::ProduceCommitProtocolViolation)
    }

    async fn check_replay_data(&self) -> Result<(), RSpaceError>;

    async fn is_replay(&self) -> bool;

    async fn update_produce(&self, produce: Produce) -> ();

    /// Mark the reporting phase that the next flushed segment belongs to
    async fn set_report_phase(&self, _phase: super::reporting_rspace::ReportPhase) {}
}
