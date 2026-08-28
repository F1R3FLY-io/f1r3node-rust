//! Crash-durable physical publication formats and state machines.
//!
//! The semantic authority remains the canonical Merkle DAG. These records
//! identify which verified root is current and where its immutable objects are
//! stored; they never redefine RSpace semantics.

mod admission;
mod catalog;
mod error;
mod index;
mod io;
mod layout;
mod limits;
mod pack;
mod publication;
mod recovery;
mod startup;

#[cfg(test)]
pub(crate) use admission::copy_verified_canonical_with_scratch;
pub(crate) use admission::{
    AdmittedCheckpointSourceV1, CheckpointPublicationAuthorityV1, DurableAdmissionErrorV1,
    DurableAdmissionLimitKindV1, DurableAdmissionLimitsV1, DurableAdmissionPreflightErrorV1,
    DurableAdmissionReportV1, DurableIndexedObjectStoreV1, DurableObjectCopyErrorV1,
    DurableObjectReadErrorV1, InventoryMismatchKindV1, SemanticallyAdmittedDurableWriterV1,
    copy_verified_canonical_with,
};
pub use catalog::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogAuthorityTokenV1, CatalogPackStateV1,
    CatalogSelectionV1, CatalogSlotV1,
};
pub use error::DurableFormatError;
pub(crate) use index::{
    CanonicalIndexStreamV1, CommittedObjectIndexV1, ObjectIndexPrefixVerifierV1,
    PreparedGenerationCheckpointIndexV1, PreparedGenerationCheckpointIndexVisitErrorV1,
    PreparedIndexAppendV1,
};
pub use index::{
    IndexFormatError, IndexGenerationV1, ObjectIndexFileViewV1, ObjectIndexHeaderV1,
    ObjectIndexRecordIter, ObjectIndexRecordV1, ObjectLocationV1,
};
#[cfg(test)]
pub(crate) use index::{committed_index_constructions, reset_committed_index_constructions};
pub(crate) use io::{
    CATALOG_FILE_NAME_V1, CatalogSlotIndexV1, DurableDirectoryV1, DurableFileV1, DurableIoError,
    DurableIoOperation, DurableWriterLeaseV1, FileLengthExpectation, WRITER_LOCK_FILE_NAME_V1,
    index_file_name, pack_file_name,
};
#[cfg(test)]
pub(crate) use io::{
    DurableIoFailpointV1, arm_catalog_slot_write_prefix, arm_durable_io_failpoint,
};
pub use layout::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1};
pub use limits::{DurableLimitConfig, DurableLimitError, DurableLimitKind, DurableLimits};
pub use pack::{PackFormatError, PackHeaderV1, PackTerminalIdentityV1};
pub(crate) use publication::{
    GenerationCheckpointPublicationErrorV1, GenerationCheckpointPublicationOutcomeV1,
};
#[cfg(test)]
pub(crate) use publication::{
    GenerationCheckpointPublicationHookPointV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
pub(crate) use recovery::{
    CatalogPackProviderV1, DurableReadAt, RecoveryError, replay_catalog_authorized_pack_chain,
    scan_catalog_authorized_pack_chain,
};
#[cfg(test)]
pub(crate) use startup::{
    DurableStartupErrorV1, DurableStartupWorkV1, IndexBindingDispositionV1,
    IndexBoundPhysicalRecoveryV1, PhysicallyRecoveredDurableWriterV1,
    recover_physical_with_lease_v1,
};
#[cfg(test)]
pub(crate) use startup::{DurableStartupHookPointV1, arm_durable_startup_hook};
