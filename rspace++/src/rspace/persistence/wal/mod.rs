//! Append-only transactional log-pack.
//!
//! A prepare record streams the exact normalized mutation plus each new
//! canonical object once, in digest order. A separately synced fixed commit
//! record authenticates that prepare record. Thus recovery and object lookup
//! share one physical stream without a second copy of the transaction delta.

mod codec;
mod checkpoint;
mod checkpoint_commit;
mod disposition;
mod error;
mod terminal;
mod types;

#[cfg(test)]
pub(crate) use checkpoint::GenerationCheckpointSourceTestSealV1;
pub(crate) use checkpoint::{
    GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1, GenerationCheckpointLayoutVisitErrorV1,
    GenerationCheckpointObjectLayoutV1, GenerationCheckpointPrepareIdentityV1,
    GenerationCheckpointPreparePreflightV1, GenerationCheckpointPrepareWriteErrorV1,
    GenerationCheckpointSourceCopyErrorV1, GenerationCheckpointSourceV1,
    PreparedGenerationCheckpointV1, designated_retention_object,
    preflight_generation_checkpoint_prepare_for,
};
#[cfg(test)]
pub(crate) use checkpoint::{
    GenerationCheckpointPrepareRecordViewV1, preflight_generation_checkpoint_prepare_v1,
};
pub(crate) use checkpoint_commit::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    GenerationCheckpointCommitViewV1,
};
pub use codec::{
    COMMIT_DISCRIMINATOR_LEN_V1, CommitRecordKindV1, LogPackObjectView,
    PREPARE_DISCRIMINATOR_LEN_V1, PrepareRecordIdentityV1, PrepareRecordKindV1,
    PreparedLogPackTransactionV1, TRANSACTION_COMMIT_RECORD_LEN_V1,
    TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitRecordV1, TransactionCommitViewV1,
    decode_commit_kind_v1, decode_prepare_kind_v1, preflight_prepare_record_v1,
};
pub(crate) use codec::{
    LogPackObjectHeaderV1, OBJECT_RECORD_HEADER_LEN_V1, PrepareRecordPreflightV1,
    PreparedObjectLayoutV1, validate_streamed_mutation_receipt_v1,
};
#[cfg(test)]
pub(crate) use codec::{
    TransactionPrepareRecordViewV1, preflight_prepare_record_len_v1, validate_object_for,
};
pub use disposition::{
    CommittedObjectComparison, CommittedObjectLookup, LogPackObjectDispositionV1,
    LogPackObjectPlanError, LogPackObjectPlanV1,
};
pub use error::{LogPackFormatError, LogPackWriteError};
pub(crate) use terminal::TerminalCommitViewV1;
pub use types::{
    CommitPublicationV1, LogPackAddress, LogPackDecodeLimits, LogPackSequence, PackId,
};
