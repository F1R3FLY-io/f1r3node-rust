use super::RSpaceError;
use crate::rspace::internal::ConsumeCandidate;
use crate::rspace::rspace_interface::RSpaceOperationSource;
use crate::rspace::trace::event::{COMM, Consume, Produce};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReplayOutcome {
    Stored,
    Matched,
    DeniedIntroduction,
    DeniedComm,
    /// Added by G3 (DR-135, S6): a `Rejected` row whose introduction its
    /// owner could not pay (R8). Replay publishes the row, stores nothing,
    /// and returns `RSpaceError::IntroductionRefused`, which the backend maps
    /// to "nothing stored", as play does.
    SkippedIntroduction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReplayDecision {
    Granted,
    Denied,
    /// Added by G3 (DR-135, S6): the owner of the introduction could not pay
    /// (R8).
    OwnerRefused,
}

/// Added by G3 (DR-135, S6): the owner checks of the replay matcher for an
/// operation of a member with owner funding (R5, R6, R7).
pub trait NativeOwnerFilter<C, P, A, K> {
    /// The item test of the Stored rule (R5, SA8): whether the owners of a
    /// COMM of `continuation` with `datum` alone can pay their shares from
    /// the budgets S_r of the operation, where an unlisted purse passes when
    /// the owner table has room.
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> Result<bool, RSpaceError>;

    /// Whether the row refused a COMM of the continuation with the source
    /// `consume` (R7). The Stored rule skips such a continuation, because
    /// the refusal is verified on its own.
    fn names_refusal_for(&self, consume: &Consume) -> bool;

    /// Whether a produce keeps its own datum (R6). It holds under owner
    /// funding.
    fn requires_own_datum(&self) -> bool;
}

pub trait NativeOperationPublication: Send {
    fn publish(&mut self);
}

pub trait NativeOperationTicket<C, P, A: Clone, K>: Send {
    type Authority: Sync;
    type Publication: NativeOperationPublication;

    fn outcome(&self) -> NativeReplayOutcome;
    fn candidate_identity(&self) -> Option<&dyn NativeCandidateIdentity>;
    fn authenticate_footprint(
        &mut self,
        channels: &[C],
        joins: &[Vec<C>],
    ) -> Result<(), RSpaceError>;
    fn observe_produce(
        &mut self,
        source: &Produce,
        channel: &C,
        data: &A,
        authority: &Self::Authority,
    ) -> Result<NativeReplayDecision, RSpaceError>;
    fn observe_consume(
        &mut self,
        source: &Consume,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        peeks: &std::collections::BTreeSet<i32>,
        authority: &Self::Authority,
    ) -> Result<NativeReplayDecision, RSpaceError>;
    fn observe_comm(
        &mut self,
        source: &COMM,
        continuation: &K,
        persistent: bool,
        data: &[ConsumeCandidate<C, A>],
    ) -> Result<NativeReplayDecision, RSpaceError>;
    fn returned_produce(&self, source: &Produce) -> Result<Produce, RSpaceError>;
    fn prepare(self, outcome: NativeReplayOutcome) -> Result<Self::Publication, RSpaceError>;

    /// Added by G3 (DR-135, S6): the owner checks of the operation, when the
    /// replayed member has owner funding.
    fn owner_filter(&self) -> Option<&dyn NativeOwnerFilter<C, P, A, K>> { None }

    /// Added by G3 (DR-135, S6): the number of COMMs that the row refused
    /// (R7).
    fn refusal_count(&self) -> usize { 0 }

    /// Added by G3 (DR-135, S6): the COMM source of the refusal `index`,
    /// which finds the refused tuple.
    fn refusal_identity(&self, _index: usize) -> Option<&dyn NativeCandidateIdentity> { None }

    /// Added by G3 (DR-135, S6): verifies the refusal `index` with the tuple
    /// that its source names: the COMM `comm` of `continuation` with `data`.
    /// `incoming` is the stored value of the datum of a produce, which the
    /// tuple holds at the index -1. A ticket without owner funding has no
    /// refusal to verify.
    fn verify_refusal(
        &mut self,
        _index: usize,
        _comm: &COMM,
        _continuation: &K,
        _persistent: bool,
        _data: &[ConsumeCandidate<C, A>],
        _incoming: Option<&A>,
    ) -> Result<(), RSpaceError> {
        Err(RSpaceError::InterpreterError(
            "native replay has a refusal without owner funding".to_owned(),
        ))
    }
}

/// Added by decision 95: the readiness of the first row of a space
/// transaction of Reified RSpaces in native replay (decision 92).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceTransactionReadiness {
    /// The row is open, and its predecessors are complete.
    Ready,
    /// The row is not ready at the replay state version `seen`.
    NotReady { seen: u64 },
}

#[async_trait::async_trait]
pub trait NativeOperationEpoch<C, P, A: Clone, K>: NativeReplayEpoch {
    type Ticket: NativeOperationTicket<C, P, A, K>;

    async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), RSpaceError>;

    fn begin_operation(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<Self::Ticket>, RSpaceError>;

    /// Added by decision 95: the readiness of the recorded row of `first`,
    /// the first operation of a space transaction (decision 92). The
    /// transaction holds its stripes, so the check reserves nothing and never
    /// waits. The default fails closed, because an epoch without space
    /// transactions has no such row.
    fn space_transaction_ready(
        &self,
        first: RSpaceOperationSource<'_>,
    ) -> Result<SpaceTransactionReadiness, RSpaceError> {
        let _ = first;
        Err(RSpaceError::InterpreterError(
            "native replay has no space transaction readiness".to_owned(),
        ))
    }

    /// Added by decision 95: the counted wait of a job whose space
    /// transaction was not ready at the version `seen`, after the job
    /// released its stripes. The default fails closed.
    async fn wait_space_change(&self, seen: u64) -> Result<(), RSpaceError> {
        let _ = seen;
        Err(RSpaceError::InterpreterError("native replay has no space transaction wait".to_owned()))
    }

    /// Added by decision 102 (decision 119 S2): the recorded row of `source`
    /// in a space transaction. With `previous` None it is the first open row
    /// of the source. Otherwise it is the row of the source whose S(sid)
    /// predecessor is the journal row `previous`. `footprint_key` is S(sid).
    /// Every index is a journal index. The answer holds the row's COMM. It
    /// reserves nothing and never waits. The default fails closed.
    fn space_recorded_row(
        &self,
        source: RSpaceOperationSource<'_>,
        previous: Option<usize>,
        footprint_key: &C,
    ) -> Result<Option<(usize, NativeReplayOutcome, Option<COMM>)>, RSpaceError> {
        let _ = (source, previous, footprint_key);
        Err(RSpaceError::InterpreterError("native replay has no space recorded row".to_owned()))
    }
}

pub trait NativeCandidateIdentity {
    fn matches_consume(&self, source: &Consume) -> bool;
    fn matches_produce(&self, source: &Produce) -> bool;
    fn repetition(&self, source: &Produce) -> Option<i32>;
    fn matches_comm(&self, source: &COMM) -> bool;
}

pub trait NativeReplayRestore: Send {
    fn publish(self);
}

pub trait NativeReplayBoundary: Send {
    type Checkpoint: Send + Sync;
    type Restore: NativeReplayRestore;
    type Evidence;

    fn checkpoint(&self) -> Self::Checkpoint;
    fn prepare_restore(self, checkpoint: &Self::Checkpoint) -> Result<Self::Restore, RSpaceError>;
    fn check_complete(&self) -> Result<(), RSpaceError>;
    fn completed_usage(&self) -> Result<u64, RSpaceError>;
    fn completed_evidence(&self) -> Result<Self::Evidence, RSpaceError>;
    fn close(self);
}

pub trait NativeReplayEpoch: Send + Sync {
    type Boundary: NativeReplayBoundary;

    fn begin_boundary(&self) -> Result<Self::Boundary, RSpaceError>;
    fn invalidate(&self);
    /// Added by DR-113: the error of a published denial. The epoch names it,
    /// because the class of an exhaustion depends on the bound source.
    fn denial(&self) -> RSpaceError;
}
