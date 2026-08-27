//! Append-only transactional log-pack.
//!
//! A prepare record streams the exact normalized mutation plus each new
//! canonical object once, in digest order. A separately synced fixed commit
//! record authenticates that prepare record. Thus recovery and object lookup
//! share one physical stream without a second copy of the transaction delta.

mod codec;
mod error;
mod types;

pub use codec::{
    LogPackObjectIter, LogPackObjectView, PrepareRecordIdentityV1, PreparedLogPackTransactionV1,
    TransactionCommitRecordV1, TransactionCommitViewV1, TransactionPrepareRecordViewV1,
};
pub use error::{LogPackFormatError, LogPackWriteError};
pub use types::{LogPackAddress, LogPackDecodeLimits, LogPackSequence};
