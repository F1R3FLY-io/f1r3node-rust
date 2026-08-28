//! Kind-neutral authority carried by a fully parsed terminal COMMIT.
//!
//! The enum is constructed only from a kind-specific parser. It does not
//! perform unauthenticated routing and therefore cannot blur the incremental
//! and generation-checkpoint wire contracts.

use super::{
    CommitPublicationV1, GenerationCheckpointCommitViewV1, LogPackAddress, LogPackSequence,
    TransactionCommitViewV1,
};
use crate::rspace::persistence::{PhysicalDigest, SemanticProfileId, StateVersionV1, StoreId};

#[derive(Clone, Copy, Debug)]
pub(crate) enum TerminalCommitViewV1 {
    Incremental(TransactionCommitViewV1),
    GenerationCheckpoint(GenerationCheckpointCommitViewV1),
}

impl From<TransactionCommitViewV1> for TerminalCommitViewV1 {
    fn from(commit: TransactionCommitViewV1) -> Self { Self::Incremental(commit) }
}

impl From<GenerationCheckpointCommitViewV1> for TerminalCommitViewV1 {
    fn from(commit: GenerationCheckpointCommitViewV1) -> Self { Self::GenerationCheckpoint(commit) }
}

impl TerminalCommitViewV1 {
    pub(crate) const fn sequence(self) -> LogPackSequence {
        match self {
            Self::Incremental(commit) => commit.sequence(),
            Self::GenerationCheckpoint(commit) => commit.sequence(),
        }
    }

    pub(crate) const fn store_id(self) -> StoreId {
        match self {
            Self::Incremental(commit) => commit.store_id(),
            Self::GenerationCheckpoint(commit) => commit.store_id(),
        }
    }

    pub(crate) const fn previous_commit(self) -> PhysicalDigest {
        match self {
            Self::Incremental(commit) => commit.previous_commit(),
            Self::GenerationCheckpoint(commit) => commit.previous_commit(),
        }
    }

    pub(crate) const fn prepare_address(self) -> LogPackAddress {
        match self {
            Self::Incremental(commit) => commit.prepare_address(),
            Self::GenerationCheckpoint(commit) => commit.prepare_address(),
        }
    }

    pub(crate) const fn state(self) -> StateVersionV1 {
        match self {
            Self::Incremental(commit) => commit.receipt().target(),
            Self::GenerationCheckpoint(commit) => commit.descriptor().state(),
        }
    }

    pub(crate) const fn profile_id(self) -> SemanticProfileId {
        match self {
            Self::Incremental(commit) => commit.receipt().profile_id(),
            Self::GenerationCheckpoint(commit) => commit.descriptor().profile_id(),
        }
    }

    pub(crate) const fn publication(self) -> CommitPublicationV1 {
        match self {
            Self::Incremental(commit) => commit.publication(),
            Self::GenerationCheckpoint(commit) => commit.publication(),
        }
    }

    pub(crate) const fn durable_index_tail(self) -> u64 { self.publication().durable_index_tail() }

    pub(crate) const fn digest(self) -> PhysicalDigest {
        match self {
            Self::Incremental(commit) => commit.digest(),
            Self::GenerationCheckpoint(commit) => commit.digest(),
        }
    }
}
