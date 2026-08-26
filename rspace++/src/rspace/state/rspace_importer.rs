use shared::rust::ByteVector;

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::state::import_session::{StateImportError, StateImportSession, StateImportSpec};

/// Authority boundary for importing an untrusted RSpace snapshot.
///
/// Import is intentionally not expressed as independent setters. A caller
/// must obtain one affine session, feed complete authenticated pages through
/// that session, consume it to seal the complete radix/cold-leaf closure, and
/// finally consume the sealed capability to publish verified roots. There is
/// therefore no API state in which peer bytes can directly appoint a root.
pub trait RSpaceImporter: Send + Sync {
    fn begin_import(
        &self,
        spec: StateImportSpec,
    ) -> Result<Box<dyn StateImportSession>, StateImportError>;

    /// Read-only diagnostic access retained for state-transfer observability.
    /// It confers no staging or root-publication authority.
    fn get_history_item(
        &self,
        hash: Blake2b256Hash,
    ) -> Result<Option<ByteVector>, StateImportError>;
}
