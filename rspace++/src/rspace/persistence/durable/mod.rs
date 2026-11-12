//! Crash-durable physical publication formats and state machines.
//!
//! The semantic authority remains the canonical Merkle DAG. These records
//! identify which verified root is current and where its immutable objects are
//! stored; they never redefine RSpace semantics.

use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::persistence::semantic::SemanticProfile;
use crate::rspace::persistence::{
    ClosureError, EndpointValue, GenerationLeaseError, GenerationLeaseKeyV1,
    GenerationLeaseRegistryV1, GenerationLeaseV1, LogPackDecodeLimits, SemanticRoots,
    VerifiedDagState,
};
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;

mod admission;
mod catalog;
mod error;
mod index;
mod io;
mod layout;
mod limits;
mod pack;
mod publication;
mod reclaim;
mod recovery;
mod rspace_authority;
mod startup;
mod state_root;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only unit tests export a durable state until task \
                  rspace-dag-transfer-trait-binding serves transfer pages through it"
    )
)]
mod transfer_export;
mod view_reader;

pub(crate) use admission::{
    AdmittedCheckpointSourceV1, DurableAdmissionErrorV1, DurableAdmissionLimitsV1,
    DurableAdmissionReportV1, DurableIndexedObjectStoreV1, DurableObjectCopyErrorV1,
    DurableObjectReadErrorV1, SemanticallyAdmittedDurableWriterV1,
};
#[cfg(test)]
pub(crate) use admission::{
    DurableAdmissionLimitKindV1, DurableTransactionPreparationError, InventoryMismatchKindV1,
    copy_verified_canonical_with,
};
#[cfg(test)]
pub(crate) use admission::{
    DurableAuthorityCoreV1, PreparedTrustedSuccessorV1, SealedDurableTransactionV1,
    SuccessorBindingV1, TrustedCeilingAxisV1, TrustedStateCeilingsV1, TrustedSuccessorPolicyV1,
    TrustedTrialWorkV1,
};
#[cfg(test)]
pub(crate) use admission::{
    acquire_cold_checked, admit_physical_source, admit_physical_writer, cold_admissions,
    copy_verified_canonical_with_scratch, reset_cold_admissions,
};
pub use catalog::{CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CatalogAuthorityTokenV1, CatalogSlotV1};
#[cfg(test)]
pub(crate) use catalog::{CatalogPackStateV1, CatalogSelectionV1};
pub use error::DurableFormatError;
#[cfg(test)]
pub(crate) use expansion_count::expansion_total;
pub(crate) use index::CommittedObjectIndexV1;
#[cfg(test)]
pub(crate) use index::{
    CanonicalIndexStreamV1, ObjectIndexPrefixVerifierV1, PreparedGenerationCheckpointIndexV1,
    PreparedGenerationCheckpointIndexVisitErrorV1, PreparedIndexAppendV1,
};
pub use index::{IndexFormatError, IndexGenerationV1, ObjectIndexHeaderV1, ObjectIndexRecordV1};
#[cfg(test)]
pub(crate) use index::{ObjectIndexFileViewV1, ObjectLocationV1};
#[cfg(test)]
pub(crate) use index::{committed_index_constructions, reset_committed_index_constructions};
#[cfg(all(test, windows))]
pub(crate) use io::{CATALOG_FILE_NAME_V1, DurableIoOperation, WRITER_LOCK_FILE_NAME_V1};
#[cfg(test)]
pub(crate) use io::{
    DurableDirectoryV1, DurableIoError, DurableWriterLeaseV1, FileLengthExpectation,
    index_file_name, pack_file_name,
};
#[cfg(test)]
pub(crate) use io::{
    DurableIoFailpointV1, arm_catalog_slot_write_prefix, arm_durable_io_failpoint,
    arm_reclamation_intent_write_prefix,
};
pub use layout::{INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, PACK_HEADER_LEN_V1};
pub use limits::{DurableLimitConfig, DurableLimitError, DurableLimitKind, DurableLimits};
pub use pack::{PackFormatError, PackHeaderV1, PackTerminalIdentityV1};
#[cfg(test)]
pub(crate) use publication::require_incremental_retention_manifest;
#[cfg(test)]
pub(crate) use publication::{
    GenerationCheckpointPublicationErrorV1, GenerationCheckpointPublicationOutcomeV1,
    IncrementalPublicationErrorV1, publish_incremental_from_lease_v1,
    publish_incremental_with_terminal_v1, publish_trusted_incremental_v1,
};
#[cfg(test)]
pub(crate) use publication::{
    GenerationCheckpointPublicationHookPointV1, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
#[cfg(test)]
pub(crate) use reclaim::RECLAMATION_RECORD_LEN_V1;
pub(crate) use reclaim::ReclamationCandidateV1;
#[cfg(test)]
pub(crate) use recovery::{
    CatalogPackProviderV1, DurableReadAt, IncrementalLayoutTranscriptV1, RecoveryError,
    replay_catalog_authorized_pack_chain, scan_catalog_authorized_pack_chain,
};
pub(crate) use rspace_authority::DurableRSpaceAuthorityV1;
#[cfg(test)]
pub(crate) use rspace_authority::hooks as journal_owner_hooks;
#[cfg(test)]
pub(crate) use rspace_authority::{
    DurableRSpacePublicationLimitsV1, DurableRSpaceTransactionError, JournalPublicationPhaseV1,
    OwnerIncarnationAllocatorV1, OwnerIncarnationExhaustedV1, ViewAttachmentRefusalV1,
};
pub(crate) use startup::EmptyStateCheckpointSourceV1;
#[cfg(test)]
pub(crate) use startup::{
    DurableStartupErrorV1, IndexBindingDispositionV1, PhysicallyRecoveredDurableWriterV1,
    recover_physical_from_lease_v1, recover_physical_with_lease_v1,
};
#[cfg(test)]
pub(crate) use startup::{DurableStartupHookPointV1, arm_durable_startup_hook};
#[cfg(test)]
pub(crate) use startup::{
    DurableStoreGenesisErrorV1, DurableStoreGenesisStepV1, arm_durable_store_genesis_failpoint,
    create_empty_store_v1, start_durable_store_genesis_trace, take_durable_store_genesis_trace,
};
pub(crate) use state_root::{DurableStateReadHandleErrorV1, DurableStateRootV1};
#[cfg(test)]
pub(crate) use state_root::{ResolvedDurableStateV1, resolve_retained_state};
#[cfg(test)]
pub(crate) use transfer_export::{
    DurableExportSessionV1, TransferBindingV1, TransferExportErrorV1, TransferPageFormatErrorV1,
    TransferPageLimitsV1, TransferPageViewV1, TransferTokenV1,
};
pub(crate) use view_reader::DurableViewReaderV1;

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
    pub(crate) fn into_rspace_authority(
        self,
        hot: &std::sync::Arc<
            Box<
                dyn crate::rspace::hot_store::HotStore<
                        P::Channel,
                        P::Pattern,
                        P::Datum,
                        P::Continuation,
                    >,
            >,
        >,
    ) -> Result<
        DurableRSpaceAuthorityV1<P>,
        Box<(Self, rspace_authority::DurableRSpaceTransactionError)>,
    >
    where
        P::Channel: Clone + std::hash::Hash + Eq,
    {
        DurableRSpaceAuthorityV1::from_view(self, hot)
    }
    /// Return the exact catalog image selected during restart recovery.
    ///
    /// A caller that keeps this read view alive can use the immutable image to
    /// identify the pack/index generation it is reading.  Compaction must use
    /// that identity when deciding whether an obsolete generation is eligible
    /// for reclamation; a freshly re-read pathname is not a sufficient lease.
    pub(crate) fn catalog(&self) -> CatalogSlotV1 { self.writer.catalog() }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 {
        self.writer.generation_leases()
    }

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

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only unit tests take a read handle until task 7931 reads durable views \
                      through it"
        )
    )]
    pub(crate) fn read_handle(&self) -> Result<DurableViewReadHandleV1<P>, GenerationLeaseError> {
        const { require_shareable_read_handle::<DurableViewReadHandleV1<P>>() };
        let lease = self.acquire_generation_lease()?;
        let core = self.writer.shared_core();
        Ok(DurableViewReadHandleV1 {
            manifest: core.verified_state().version().manifest(),
            core,
            _lease: lease,
        })
    }

    pub(crate) fn read_handle_at(
        &self,
        resolved: &state_root::ResolvedDurableStateV1,
    ) -> Result<DurableViewReadHandleV1<P>, state_root::DurableStateReadHandleErrorV1> {
        const { require_shareable_read_handle::<DurableViewReadHandleV1<P>>() };
        let core = self.writer.shared_core();
        core.require_retained(resolved)
            .map_err(state_root::DurableStateReadHandleErrorV1::Root)?;
        let lease = self.acquire_generation_lease()?;
        Ok(DurableViewReadHandleV1 {
            core,
            manifest: resolved.manifest(),
            _lease: lease,
        })
    }

    pub(crate) fn resolve(
        &self,
        root: state_root::DurableStateRootV1,
    ) -> Result<state_root::ResolvedDurableStateV1, crate::rspace::errors::RootError> {
        self.writer.shared_core().resolve(root)
    }

    pub(crate) fn state(&self) -> crate::rspace::persistence::StateVersionV1 {
        self.catalog().state()
    }

    /// Borrow the exact restart-admitted semantic state for read-only
    /// projections.  The returned token is tied to this authority's pinned
    /// object index and catalog generation; callers cannot substitute a
    /// reopened path or a caller-selected root.  This is intentionally a
    /// read-only seam: mutation and publication remain on the writer path.
    pub(crate) fn verified_state(&self) -> VerifiedDagState<'_, DurableIndexedObjectStoreV1, P> {
        self.writer.verified_state()
    }

    /// Return the namespace roots authenticated by this authority's exact
    /// semantic manifest.
    pub(crate) fn semantic_roots(
        &self,
    ) -> Result<SemanticRoots, ClosureError<DurableObjectReadErrorV1>> {
        self.writer
            .verified_state()
            .semantic_manifest()
            .map(|manifest| manifest.roots())
    }

    /// Project one canonical namespace prefix through the pinned durable
    /// object index. The result contains encoded paths and endpoint states;
    /// decoding into application values remains the caller's typed boundary.
    pub(crate) fn project_prefix(
        &self,
        root: crate::rspace::persistence::ObjectRefV1,
        prefix: &[u8],
        max_steps: u64,
        max_endpoints: u64,
    ) -> Result<Vec<(Vec<u8>, EndpointValue)>, ClosureError<DurableObjectReadErrorV1>> {
        self.writer.verified_state().collect_endpoints_under_prefix(
            root,
            prefix,
            max_steps,
            max_endpoints,
        )
    }

    /// Decode the authenticated DataBag projection for one canonical channel
    /// directly from trie endpoints.  The channel path is the query key; no
    /// channel hash inversion or flattened legacy leaf is involved.
    pub(crate) fn data_for_channel(
        &self,
        channel: &P::Channel,
        max_steps: u64,
        max_endpoints: u64,
        max_values: u64,
    ) -> Result<Vec<crate::rspace::internal::Datum<P::Datum>>, ClosureError<DurableObjectReadErrorV1>>
    where
        P::Datum: Clone + ColdStoreDecode + RhoPathEncode + serde::Serialize,
    {
        data_for_channel_at(
            self.verified_state(),
            &self.semantic_roots()?,
            channel,
            max_steps,
            max_endpoints,
            max_values,
        )
    }

    /// Decode authenticated continuation records for one exact ordered
    /// channel group.  Source payloads are loaded through the pinned object
    /// index and validated by the same semantic round-trip machine used at
    /// admission.
    pub(crate) fn continuations_for_channels(
        &self,
        channels: &[P::Channel],
        max_steps: u64,
        max_endpoints: u64,
        max_payload_bytes: u64,
    ) -> Result<
        Vec<crate::rspace::internal::WaitingContinuation<P::Pattern, P::Continuation>>,
        ClosureError<DurableObjectReadErrorV1>,
    >
    where
        P::Channel: Clone + RhoPathEncode,
        P::Pattern: Clone + ColdStoreDecode + StableHashSerialize,
        P::Continuation: Clone + ColdStoreDecode + StableHashSerialize,
    {
        continuations_for_channels_at(
            self.verified_state(),
            &self.semantic_roots()?,
            channels,
            max_steps,
            max_endpoints,
            max_payload_bytes,
        )
    }

    /// Decode authenticated JoinBag records for one indexed channel.  Each
    /// endpoint multiplicity is expanded only at this legacy typed boundary;
    /// the trie remains the sole authority.
    pub(crate) fn joins_for_channel(
        &self,
        channel: &P::Channel,
        max_steps: u64,
        max_endpoints: u64,
        max_values: u64,
    ) -> Result<Vec<Vec<P::Channel>>, ClosureError<DurableObjectReadErrorV1>>
    where
        P::Channel: Clone + ColdStoreDecode,
    {
        joins_for_channel_at(
            self.verified_state(),
            &self.semantic_roots()?,
            channel,
            max_steps,
            max_endpoints,
            max_values,
        )
    }

    /// Enumerate the authenticated DataBag without converting it through a
    /// legacy channel projection. This is intended for a bounded startup
    /// compatibility bridge; each endpoint retains its checked multiplicity
    /// so callers can build a PathMap-backed state without expanding counts.
    pub(crate) fn data_entries(
        &self,
        max_steps: u64,
        max_endpoints: u64,
    ) -> Result<
        Vec<(P::Channel, crate::rspace::internal::Datum<P::Datum>, std::num::NonZeroU64)>,
        ClosureError<DurableObjectReadErrorV1>,
    >
    where
        P::Channel: Clone + ColdStoreDecode + RhoPathEncode,
        P::Datum: Clone + ColdStoreDecode + RhoPathEncode + serde::Serialize,
    {
        let roots = self.semantic_roots()?;
        let state = self.verified_state();
        let mut result = Vec::new();
        state.visit_endpoints_under_prefix(
            roots.data_bag,
            &[],
            max_steps,
            max_endpoints,
            |path, endpoint| {
                let entry =
                    crate::rspace::persistence::semantic::decode_data_endpoint_with_channel::<
                        P::Channel,
                        P::Datum,
                    >(path, endpoint)
                    .map_err(ClosureError::Format)?;
                result.push(entry);
                Ok(())
            },
        )?;
        Ok(result)
    }

    /// Enumerate authenticated ContinuationMap records while preserving each
    /// ordered channel group. Source payloads are validated through the same
    /// pinned generation and bounded decoder as keyed reads.
    pub(crate) fn continuation_entries(
        &self,
        max_steps: u64,
        max_endpoints: u64,
        max_payload_bytes: u64,
    ) -> Result<
        Vec<(
            Vec<P::Channel>,
            crate::rspace::internal::WaitingContinuation<P::Pattern, P::Continuation>,
        )>,
        ClosureError<DurableObjectReadErrorV1>,
    >
    where
        P::Channel: Clone + RhoPathEncode,
        P::Pattern: Clone + ColdStoreDecode + StableHashSerialize,
        P::Continuation: Clone + ColdStoreDecode + StableHashSerialize,
    {
        let roots = self.semantic_roots()?;
        let state = self.verified_state();
        let mut result = Vec::new();
        state.visit_endpoints_under_prefix(
            roots.continuation_map,
            &[],
            max_steps,
            max_endpoints,
            |path, endpoint| {
                let entry = crate::rspace::persistence::semantic::decode_continuation_endpoint_with_channels::<
                    P::Channel,
                    P::Pattern,
                    P::Continuation,
                    _,
                >(state.object_store(), path, endpoint, max_payload_bytes)?;
                result.push(entry);
                Ok(())
            },
        )?;
        Ok(result)
    }

    /// Enumerate authenticated JoinBag records while preserving endpoint
    /// multiplicity and the indexed channel identity.
    pub(crate) fn join_entries(
        &self,
        max_steps: u64,
        max_endpoints: u64,
    ) -> Result<
        Vec<(P::Channel, Vec<P::Channel>, std::num::NonZeroU64)>,
        ClosureError<DurableObjectReadErrorV1>,
    >
    where
        P::Channel: Clone + ColdStoreDecode + RhoPathEncode,
    {
        let roots = self.semantic_roots()?;
        let state = self.verified_state();
        let mut result = Vec::new();
        state.visit_endpoints_under_prefix(
            roots.join_bag,
            &[],
            max_steps,
            max_endpoints,
            |path, endpoint| {
                let entry =
                    crate::rspace::persistence::semantic::decode_join_endpoint_with_channel::<
                        P::Channel,
                    >(path, endpoint)
                    .map_err(ClosureError::Format)?;
                result.push(entry);
                Ok(())
            },
        )?;
        Ok(result)
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

pub(crate) struct DurableViewReadHandleV1<P: SemanticProfile> {
    core: std::sync::Arc<admission::DurableAuthorityCoreV1<P>>,
    manifest: crate::rspace::persistence::ObjectRefV1,
    _lease: GenerationLeaseV1,
}

impl<P: SemanticProfile> DurableViewReadHandleV1<P> {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only unit tests read through a handle until task 7931 reads durable views \
                      through it"
        )
    )]
    pub(crate) fn semantic_roots(
        &self,
    ) -> Result<SemanticRoots, ClosureError<DurableObjectReadErrorV1>> {
        let manifest = self.core.verified_state().load_object(self.manifest)?;
        crate::rspace::persistence::SemanticStateManifestView::parse(&manifest)
            .map(crate::rspace::persistence::SemanticStateManifestView::roots)
            .map_err(ClosureError::Format)
    }
}

fn data_for_channel_at<P: SemanticProfile, E: Copy>(
    state: VerifiedDagState<'_, DurableIndexedObjectStoreV1, P, E>,
    roots: &SemanticRoots,
    channel: &P::Channel,
    max_steps: u64,
    max_endpoints: u64,
    max_values: u64,
) -> Result<Vec<crate::rspace::internal::Datum<P::Datum>>, ClosureError<DurableObjectReadErrorV1>>
where
    P::Datum: Clone + ColdStoreDecode + RhoPathEncode + serde::Serialize,
{
    let prefix = RhoPathV1::datum_prefix(channel);
    let mut result = Vec::new();
    state.visit_endpoints_under_prefix(
        roots.data_bag,
        &prefix,
        max_steps,
        max_endpoints,
        |path, endpoint| {
            let (datum, multiplicity) =
                crate::rspace::persistence::semantic::decode_data_endpoint::<P::Datum>(
                    path, endpoint,
                )
                .map_err(ClosureError::Format)?;
            let count = multiplicity.get();
            expansion_count::expansion_total(
                result.len(),
                count,
                max_values,
                "durable data materialization",
            )
            .map_err(ClosureError::Format)?;
            let count = usize::try_from(count).map_err(|_| {
                ClosureError::Format(
                    crate::rspace::persistence::CanonicalFormatError::LengthOutOfRange {
                        field: "durable data multiplicity",
                        value: multiplicity.get(),
                    },
                )
            })?;
            result.try_reserve(count).map_err(|_| {
                ClosureError::Format(
                    crate::rspace::persistence::CanonicalFormatError::AllocationFailure {
                        requested: count as u64,
                    },
                )
            })?;
            result.extend(std::iter::repeat_n(datum, count));
            Ok(())
        },
    )?;
    Ok(result)
}

fn continuations_for_channels_at<P: SemanticProfile, E: Copy>(
    state: VerifiedDagState<'_, DurableIndexedObjectStoreV1, P, E>,
    roots: &SemanticRoots,
    channels: &[P::Channel],
    max_steps: u64,
    max_endpoints: u64,
    max_payload_bytes: u64,
) -> Result<
    Vec<crate::rspace::internal::WaitingContinuation<P::Pattern, P::Continuation>>,
    ClosureError<DurableObjectReadErrorV1>,
>
where
    P::Channel: Clone + RhoPathEncode,
    P::Pattern: Clone + ColdStoreDecode + StableHashSerialize,
    P::Continuation: Clone + ColdStoreDecode + StableHashSerialize,
{
    let prefix = RhoPathV1::continuation_prefix(channels);
    let mut result = Vec::new();
    state.visit_endpoints_under_prefix(
        roots.continuation_map,
        &prefix,
        max_steps,
        max_endpoints,
        |path, endpoint| {
            result.push(crate::rspace::persistence::semantic::decode_continuation_endpoint::<
                P::Channel,
                P::Pattern,
                P::Continuation,
                _,
            >(state.object_store(), path, endpoint, max_payload_bytes)?);
            Ok(())
        },
    )?;
    Ok(result)
}

fn joins_for_channel_at<P: SemanticProfile, E: Copy>(
    state: VerifiedDagState<'_, DurableIndexedObjectStoreV1, P, E>,
    roots: &SemanticRoots,
    channel: &P::Channel,
    max_steps: u64,
    max_endpoints: u64,
    max_values: u64,
) -> Result<Vec<Vec<P::Channel>>, ClosureError<DurableObjectReadErrorV1>>
where
    P::Channel: Clone + ColdStoreDecode,
{
    let prefix = RhoPathV1::join_prefix(channel);
    let mut result = Vec::new();
    state.visit_endpoints_under_prefix(
        roots.join_bag,
        &prefix,
        max_steps,
        max_endpoints,
        |path, endpoint| {
            let (join, multiplicity) =
                crate::rspace::persistence::semantic::decode_join_endpoint::<P::Channel>(
                    path, endpoint,
                )
                .map_err(ClosureError::Format)?;
            let count = multiplicity.get();
            expansion_count::expansion_total(
                result.len(),
                count,
                max_values,
                "durable join materialization",
            )
            .map_err(ClosureError::Format)?;
            let count = usize::try_from(count).map_err(|_| {
                ClosureError::Format(
                    crate::rspace::persistence::CanonicalFormatError::LengthOutOfRange {
                        field: "durable join multiplicity",
                        value: multiplicity.get(),
                    },
                )
            })?;
            result.try_reserve(count).map_err(|_| {
                ClosureError::Format(
                    crate::rspace::persistence::CanonicalFormatError::AllocationFailure {
                        requested: count as u64,
                    },
                )
            })?;
            result.extend(std::iter::repeat_with(|| join.clone()).take(count));
            Ok(())
        },
    )?;
    Ok(result)
}

mod expansion_count {
    use crate::rspace::persistence::CanonicalFormatError;

    pub(crate) fn expansion_total(
        expanded: usize,
        count: u64,
        limit: u64,
        resource: &'static str,
    ) -> Result<u64, CanonicalFormatError> {
        match u64::try_from(expanded)
            .ok()
            .and_then(|expanded| expanded.checked_add(count))
        {
            Some(total) if total <= limit => Ok(total),
            Some(total) => Err(CanonicalFormatError::ResourceLimit {
                resource,
                limit,
                attempted: total,
            }),
            None => Err(CanonicalFormatError::ResourceLimit {
                resource,
                limit,
                attempted: u64::MAX,
            }),
        }
    }
}

const fn require_shareable_read_handle<T: Send + Sync + 'static>() {}

pub(crate) fn recover_read_authority<P: SemanticProfile>(
    path: impl Into<std::path::PathBuf>,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<DurableAuthorityViewV1<P>, DurableAdmissionErrorV1> {
    let directory = io::DurableDirectoryV1::open(path).map_err(|error| {
        DurableAdmissionErrorV1::Startup(startup::DurableStartupErrorV1::Io(error))
    })?;
    recover_read_authority_from_directory(directory, recovery_limits, wal_limits, admission_limits)
}

/// Recover and admit the read authority of an opened directory, under the
/// retention reader and writer that the directory carries.
pub(crate) fn recover_read_authority_from_directory<P: SemanticProfile>(
    directory: io::DurableDirectoryV1,
    recovery_limits: DurableLimits,
    wal_limits: LogPackDecodeLimits,
    admission_limits: DurableAdmissionLimitsV1,
) -> Result<DurableAuthorityViewV1<P>, DurableAdmissionErrorV1> {
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
