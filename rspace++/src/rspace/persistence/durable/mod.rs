//! Crash-durable physical publication formats and state machines.
//!
//! The semantic authority remains the canonical Merkle DAG. These records
//! identify which verified root is current and where its immutable objects are
//! stored; they never redefine RSpace semantics.

use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::{
    GenerationLeaseError, GenerationLeaseKeyV1, GenerationLeaseV1, LogPackDecodeLimits,
};

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
    DurableObjectReadErrorV1, DurableTransactionPreparationError, InventoryMismatchKindV1,
    SemanticallyAdmittedDurableWriterV1, copy_verified_canonical_with,
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
    IncrementalAppendErrorV1, IncrementalPublicationErrorV1, append_incremental_prepare_and_index,
};
#[cfg(test)]
pub(crate) use publication::{
    GenerationCheckpointPublicationHookPointV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
pub(crate) use recovery::{
    CatalogPackProviderV1, DurableReadAt, IncrementalLayoutTranscriptV1, RecoveryError,
    replay_catalog_authorized_pack_chain, scan_catalog_authorized_pack_chain,
    stage_catalog_verified_data_in_source_order,
};
#[cfg(test)]
pub(crate) use startup::{
    DurableStartupErrorV1, DurableStartupWorkV1, IndexBindingDispositionV1,
    IndexBoundPhysicalRecoveryV1, PhysicallyRecoveredDurableWriterV1,
    recover_physical_with_lease_v1,
};
#[cfg(test)]
pub(crate) use startup::{DurableStartupHookPointV1, arm_durable_startup_hook};

/// Restart-recovered semantic authority exposed as a read-only view.
///
/// The underlying admitted writer retains its affine lease and therefore
/// prevents a second process from mutating the same durable directory. This
/// façade deliberately exposes no publication or filesystem mutation method;
/// it is the first production-safe seam for wiring recovery into RSpace while
/// differential migration work is still in progress.
pub(crate) struct DurableAuthorityViewV1<P: SemanticProfile> {
    writer: SemanticallyAdmittedDurableWriterV1<P>,
    _generation_lease: GenerationLeaseV1,
}

impl<P: SemanticProfile> DurableAuthorityViewV1<P> {
    /// Return the exact catalog image selected during restart recovery.
    ///
    /// A caller that keeps this read view alive can use the immutable image to
    /// identify the pack/index generation it is reading.  Compaction must use
    /// that identity when deciding whether an obsolete generation is eligible
    /// for reclamation; a freshly re-read pathname is not a sufficient lease.
    pub(crate) const fn catalog(&self) -> CatalogSlotV1 { self.writer.catalog() }

    /// Acquire a lease for this exact recovered physical generation. The
    /// caller must retain the returned token for as long as any pack/index
    /// handle or decoded view may still be used.
    pub(crate) fn acquire_generation_lease(
        &self,
    ) -> Result<GenerationLeaseV1, GenerationLeaseError> {
        self.writer
            .generation_leases()
            .acquire(GenerationLeaseKeyV1::from_catalog(self.catalog()))
    }

    pub(crate) fn state(&self) -> crate::rspace::persistence::StateVersionV1 {
        self.catalog().state()
    }

    pub(crate) fn manifest(&self) -> crate::rspace::persistence::ObjectRefV1 {
        self.state().manifest()
    }

    pub(crate) fn get_object_bounded(
        &self,
        digest: &crate::rspace::persistence::ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Box<[u8]>>, DurableObjectReadErrorV1> {
        self.writer.get_object_bounded(digest, max_encoded_len)
    }

    pub(crate) const fn report(&self) -> DurableAdmissionReportV1 { self.writer.report() }
}

pub(crate) fn recover_read_authority<P: SemanticProfile>(
    path: impl Into<std::path::PathBuf>,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<DurableAuthorityViewV1<P>, DurableAdmissionErrorV1> {
    let directory = io::DurableDirectoryV1::open(path).map_err(|error| {
        DurableAdmissionErrorV1::Startup(startup::DurableStartupErrorV1::Io(error))
    })?;
    let writer =
        directory.recover_and_admit_v1::<P>(recovery_limits, wal_limits, admission_limits)?;
    let generation_lease = writer
        .generation_leases()
        .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))?;
    Ok(DurableAuthorityViewV1 {
        writer,
        _generation_lease: generation_lease,
    })
}
