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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeReplayDecision {
    Granted,
    Denied,
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
}

#[async_trait::async_trait]
pub trait NativeOperationEpoch<C, P, A: Clone, K>: NativeReplayEpoch {
    type Ticket: NativeOperationTicket<C, P, A, K>;

    fn prepare_produce_source(&self, channel: &C, data: &A) -> Result<(), RSpaceError>;
    fn prepare_consume_source(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
    ) -> Result<(), RSpaceError>;

    async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), RSpaceError>;

    fn begin_operation(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<Self::Ticket>, RSpaceError>;
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
    fn reserve_work(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError>;
    fn reserve_comparison(&self, operations: usize, bytes: usize) -> Result<(), RSpaceError>;
    fn invalidate(&self);
}
