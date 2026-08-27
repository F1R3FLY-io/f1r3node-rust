//! Append-only transactional log-pack.
//!
//! A prepare record streams the exact normalized mutation plus each new
//! canonical object once, in digest order. A separately synced fixed commit
//! record authenticates that prepare record. Thus recovery and object lookup
//! share one physical stream without a second copy of the transaction delta.

mod codec;
mod disposition;
mod error;
mod types;

pub use codec::{
    LogPackObjectIter, LogPackObjectView, PrepareRecordIdentityV1, PreparedLogPackTransactionV1,
    TRANSACTION_COMMIT_RECORD_LEN_V1, TRANSACTION_PREPARE_HEADER_LEN_V1, TransactionCommitRecordV1,
    TransactionCommitViewV1, TransactionPrepareRecordViewV1, preflight_prepare_record_len_v1,
    preflight_prepare_record_v1,
};
pub(crate) use codec::{PrepareRecordPreflightV1, PreparedObjectLayoutV1};
pub use disposition::{
    CommittedObjectComparison, CommittedObjectLookup, LogPackObjectDispositionV1,
    LogPackObjectPlanError, LogPackObjectPlanV1,
};
pub use error::{LogPackFormatError, LogPackWriteError};
pub use types::{
    CommitPublicationV1, LogPackAddress, LogPackDecodeLimits, LogPackSequence, PackId,
};
