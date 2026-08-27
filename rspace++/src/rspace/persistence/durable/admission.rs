//! Profile-typed semantic admission of physically recovered durable state.
//!
//! Physical WAL, catalog, and index recovery is deliberately an internal
//! phase. Production callers receive authority only after the catalog-named
//! retention policy, every retained structural closure, the current RSpace
//! semantics, and both authenticated byte inventories have been recomputed
//! through the recovered digest index.

use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use super::CommittedObjectIndexV1;
use super::io::{DurableDirectoryV1, DurableFileV1, DurableIoError, DurableWriterLeaseV1};
use super::startup::{
    DurableStartupErrorV1, DurableStartupWorkV1, IndexBindingDispositionV1,
    PhysicallyRecoveredDurableWriterV1, read_catalog_token,
};
use crate::rspace::persistence::canonical::{
    CanonicalFormatError, ObjectDigest, ObjectRefV1, checked_len,
};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1,
};
use crate::rspace::persistence::closure::{
    ClosureError, RetainedUnionClosureLimitsV1, RetainedUnionClosureReportV1,
    StructuralClosureLedgerV1, StructuralValidationLimits,
    verify_structural_closure_in_ledger_with_limits,
};
use crate::rspace::persistence::retention::RetentionManifestView;
use crate::rspace::persistence::semantic::{
    SemanticProfile, SemanticValidationLimits, SemanticValidationReport,
    SemanticallyAdmittedStateV1, verify_rspace_semantics_after_structural,
};
use crate::rspace::persistence::store::ImmutableObjectStore;
use crate::rspace::persistence::{
    CatalogAuthorityTokenV1, DurableLimits, LogPackDecodeLimits, ObjectIndexRecordV1, PackId,
    SemanticProfileId, StateVersionV1, VerifiedDagState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionLimitsV1 {
    pub(crate) current_semantics: SemanticValidationLimits,
    pub(crate) per_state_structural: StructuralValidationLimits,
    pub(crate) max_retained_states: u64,
    pub(crate) max_union_objects: u64,
    pub(crate) max_union_canonical_bytes: u64,
    pub(crate) max_aggregate_root_visits: u64,
    pub(crate) max_aggregate_references: u64,
    pub(crate) max_object_reads: u64,
    pub(crate) max_object_read_bytes: u64,
}

impl DurableAdmissionLimitsV1 {
    pub(crate) const UNBOUNDED: Self = Self {
        current_semantics: SemanticValidationLimits::unbounded(),
        per_state_structural: StructuralValidationLimits::unbounded(),
        max_retained_states: u64::MAX,
        max_union_objects: u64::MAX,
        max_union_canonical_bytes: u64::MAX,
        max_aggregate_root_visits: u64::MAX,
        max_aggregate_references: u64::MAX,
        max_object_reads: u64::MAX,
        max_object_read_bytes: u64::MAX,
    };
}

impl Default for DurableAdmissionLimitsV1 {
    fn default() -> Self { Self::UNBOUNDED }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InventoryMismatchKindV1 {
    CurrentState,
    RetainedUnion,
    RetainedState { generation: u64 },
}

#[derive(Debug)]
pub(crate) enum DurableObjectReadErrorV1 {
    Io(DurableIoError),
    MaterializationLimit {
        digest: ObjectDigest,
        limit: u64,
        attempted: u64,
    },
    MaterializationAddressSpace {
        digest: ObjectDigest,
        attempted: u64,
    },
    MaterializationAllocation {
        digest: ObjectDigest,
        attempted: u64,
    },
    PackOutsideAuthority {
        digest: ObjectDigest,
        pack: PackId,
        first: PackId,
        active: PackId,
    },
    ObjectExtentOverflow {
        digest: ObjectDigest,
        offset: u64,
        length: u64,
    },
    ObjectBeyondAuthorizedTail {
        digest: ObjectDigest,
        pack: PackId,
        end: u64,
        tail: u64,
    },
    AdmissionReadLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
}

impl Display for DurableObjectReadErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::MaterializationLimit {
                digest,
                limit,
                attempted,
            } => write!(
                formatter,
                "durable object {digest:?} advertises {attempted} bytes above the caller's \
                 {limit}-byte materialization bound"
            ),
            Self::MaterializationAddressSpace { digest, attempted } => write!(
                formatter,
                "durable object {digest:?} length {attempted} does not fit this platform's \
                 address space"
            ),
            Self::MaterializationAllocation { digest, attempted } => {
                write!(formatter, "cannot reserve {attempted} bytes for durable object {digest:?}")
            }
            Self::PackOutsideAuthority {
                digest,
                pack,
                first,
                active,
            } => write!(
                formatter,
                "durable object {digest:?} points to pack {}, outside catalog range {}..={}",
                pack.get(),
                first.get(),
                active.get()
            ),
            Self::ObjectExtentOverflow {
                digest,
                offset,
                length,
            } => write!(formatter, "durable object {digest:?} extent {offset}+{length} overflows"),
            Self::ObjectBeyondAuthorizedTail {
                digest,
                pack,
                end,
                tail,
            } => write!(
                formatter,
                "durable object {digest:?} ends at {end} in pack {}, beyond authorized tail {tail}",
                pack.get()
            ),
            Self::AdmissionReadLimit {
                resource,
                limit,
                attempted,
            } => write!(
                formatter,
                "durable semantic admission {resource} limit {limit} exceeded by {attempted}"
            ),
        }
    }
}

impl std::error::Error for DurableObjectReadErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DurableIoError> for DurableObjectReadErrorV1 {
    fn from(error: DurableIoError) -> Self { Self::Io(error) }
}

#[derive(Debug)]
pub(crate) enum DurableAdmissionErrorV1 {
    Startup(DurableStartupErrorV1),
    ObjectStore(DurableObjectReadErrorV1),
    Canonical(CanonicalFormatError),
    Closure(ClosureError<DurableObjectReadErrorV1>),
    ProfileMismatch {
        expected: SemanticProfileId,
        actual: SemanticProfileId,
    },
    RetentionCurrentMismatch {
        catalog: StateVersionV1,
        manifest: StateVersionV1,
    },
    NonHistoricalState {
        current_generation: u64,
        retained_generation: u64,
    },
    InventoryMismatch {
        kind: InventoryMismatchKindV1,
        declared: u64,
        computed: u64,
    },
    AuthorityChanged,
    PackTail {
        pack: PackId,
        authorized: u64,
        available: u64,
    },
}

impl Display for DurableAdmissionErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Startup(error) => Display::fmt(error, formatter),
            Self::ObjectStore(error) => Display::fmt(error, formatter),
            Self::Canonical(error) => Display::fmt(error, formatter),
            Self::Closure(error) => Display::fmt(error, formatter),
            Self::ProfileMismatch { expected, actual } => write!(
                formatter,
                "durable catalog semantic profile {:?} does not equal requested profile {:?}",
                actual.as_bytes(),
                expected.as_bytes()
            ),
            Self::RetentionCurrentMismatch { catalog, manifest } => write!(
                formatter,
                "retention current state {manifest:?} does not equal catalog state {catalog:?}"
            ),
            Self::NonHistoricalState {
                current_generation,
                retained_generation,
            } => write!(
                formatter,
                "retained generation {retained_generation} does not precede current generation \
                 {current_generation}"
            ),
            Self::InventoryMismatch {
                kind,
                declared,
                computed,
            } => write!(
                formatter,
                "authenticated {kind:?} inventory declares {declared} canonical bytes but \
                 admission computed {computed}"
            ),
            Self::AuthorityChanged => {
                formatter.write_str("durable catalog authority changed during semantic admission")
            }
            Self::PackTail {
                pack,
                authorized,
                available,
            } => write!(
                formatter,
                "catalog authorizes {authorized} bytes in pack {}, but only {available} exist",
                pack.get()
            ),
        }
    }
}

impl std::error::Error for DurableAdmissionErrorV1 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Startup(error) => Some(error),
            Self::ObjectStore(error) => Some(error),
            Self::Canonical(error) => Some(error),
            Self::Closure(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DurableStartupErrorV1> for DurableAdmissionErrorV1 {
    fn from(error: DurableStartupErrorV1) -> Self { Self::Startup(error) }
}

impl From<DurableObjectReadErrorV1> for DurableAdmissionErrorV1 {
    fn from(error: DurableObjectReadErrorV1) -> Self { Self::ObjectStore(error) }
}

impl From<CanonicalFormatError> for DurableAdmissionErrorV1 {
    fn from(error: CanonicalFormatError) -> Self { Self::Canonical(error) }
}

impl From<ClosureError<DurableObjectReadErrorV1>> for DurableAdmissionErrorV1 {
    fn from(error: ClosureError<DurableObjectReadErrorV1>) -> Self { Self::Closure(error) }
}

struct PinnedPackV1 {
    id: PackId,
    file: DurableFileV1,
    authorized_tail: u64,
}

/// Immutable object reads bound to the one physically recovered index and to
/// already-open handles for every catalog-named pack. The pack vector is
/// addressed by `pack_id - first_live_pack`; no directory enumeration or
/// second location map participates in lookup.
pub(crate) struct DurableIndexedObjectStoreV1 {
    index: CommittedObjectIndexV1,
    first_live_pack: PackId,
    active_pack: PackId,
    packs: Box<[PinnedPackV1]>,
}

impl DurableIndexedObjectStoreV1 {
    fn pin(
        directory: &DurableDirectoryV1,
        authority: &CatalogAuthorityTokenV1,
        index: CommittedObjectIndexV1,
    ) -> Result<Self, DurableAdmissionErrorV1> {
        let pack_state = authority.slot().pack();
        let first_live_pack = PackId::new(pack_state.first_live_pack()).map_err(|error| {
            CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            }
        })?;
        let active_pack = PackId::new(pack_state.active_pack()).map_err(|error| {
            CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            }
        })?;
        let count = active_pack
            .get()
            .checked_sub(first_live_pack.get())
            .and_then(|distance| distance.checked_add(1))
            .ok_or(CanonicalFormatError::CounterOverflow("live pack handles"))?;
        let count_usize =
            usize::try_from(count).map_err(|_| CanonicalFormatError::LengthOutOfRange {
                field: "live pack handles",
                value: count,
            })?;
        let mut packs = Vec::new();
        packs
            .try_reserve_exact(count_usize)
            .map_err(|_| CanonicalFormatError::AllocationFailure { requested: count })?;
        let mut raw = first_live_pack.get();
        loop {
            let id = PackId::new(raw).map_err(|error| CanonicalFormatError::SemanticCoherence {
                detail: error.to_string(),
            })?;
            let file = directory
                .open_pack(id)
                .map_err(DurableStartupErrorV1::from)?;
            let available = file.len().map_err(DurableStartupErrorV1::from)?;
            let authorized_tail = if id == active_pack {
                pack_state.durable_pack_tail()
            } else {
                available
            };
            if available < authorized_tail {
                return Err(DurableAdmissionErrorV1::PackTail {
                    pack: id,
                    authorized: authorized_tail,
                    available,
                });
            }
            packs.push(PinnedPackV1 {
                id,
                file,
                authorized_tail,
            });
            if id == active_pack {
                break;
            }
            raw = raw
                .checked_add(1)
                .ok_or(CanonicalFormatError::CounterOverflow("live pack identifier"))?;
        }
        Ok(Self {
            index,
            first_live_pack,
            active_pack,
            packs: packs.into_boxed_slice(),
        })
    }

    fn lookup_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<ObjectIndexRecordV1>, DurableObjectReadErrorV1> {
        let Some(record) = self.index.get(*digest) else {
            return Ok(None);
        };
        let attempted = record.reference().encoded_len();
        if attempted > max_encoded_len {
            return Err(DurableObjectReadErrorV1::MaterializationLimit {
                digest: *digest,
                limit: max_encoded_len,
                attempted,
            });
        }
        Ok(Some(record))
    }

    fn read_record(
        &self,
        record: ObjectIndexRecordV1,
    ) -> Result<Box<[u8]>, DurableObjectReadErrorV1> {
        let reference = record.reference();
        let location = record.location();
        let pack = location.pack_id();
        let relative = pack.get().checked_sub(self.first_live_pack.get()).ok_or(
            DurableObjectReadErrorV1::PackOutsideAuthority {
                digest: reference.digest(),
                pack,
                first: self.first_live_pack,
                active: self.active_pack,
            },
        )?;
        let index = usize::try_from(relative).map_err(|_| {
            DurableObjectReadErrorV1::PackOutsideAuthority {
                digest: reference.digest(),
                pack,
                first: self.first_live_pack,
                active: self.active_pack,
            }
        })?;
        let pinned =
            self.packs
                .get(index)
                .ok_or(DurableObjectReadErrorV1::PackOutsideAuthority {
                    digest: reference.digest(),
                    pack,
                    first: self.first_live_pack,
                    active: self.active_pack,
                })?;
        debug_assert_eq!(pinned.id, pack);
        let length = reference.encoded_len();
        let end = location.canonical_offset().checked_add(length).ok_or(
            DurableObjectReadErrorV1::ObjectExtentOverflow {
                digest: reference.digest(),
                offset: location.canonical_offset(),
                length,
            },
        )?;
        if end > pinned.authorized_tail {
            return Err(DurableObjectReadErrorV1::ObjectBeyondAuthorizedTail {
                digest: reference.digest(),
                pack,
                end,
                tail: pinned.authorized_tail,
            });
        }
        let length_usize = usize::try_from(length).map_err(|_| {
            DurableObjectReadErrorV1::MaterializationAddressSpace {
                digest: reference.digest(),
                attempted: length,
            }
        })?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length_usize).map_err(|_| {
            DurableObjectReadErrorV1::MaterializationAllocation {
                digest: reference.digest(),
                attempted: length,
            }
        })?;
        bytes.resize(length_usize, 0);
        pinned
            .file
            .read_exact_at(location.canonical_offset(), &mut bytes)?;
        Ok(bytes.into_boxed_slice())
    }
}

impl ImmutableObjectStore for DurableIndexedObjectStoreV1 {
    type Error = DurableObjectReadErrorV1;
    type Object = Box<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        self.lookup_bounded(digest, max_encoded_len)?
            .map(|record| self.read_record(record))
            .transpose()
    }
}

struct AdmissionReadBudgetV1 {
    reads: AtomicU64,
    bytes: AtomicU64,
    max_reads: u64,
    max_bytes: u64,
}

impl AdmissionReadBudgetV1 {
    fn new(max_reads: u64, max_bytes: u64) -> Self {
        Self {
            reads: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            max_reads,
            max_bytes,
        }
    }

    fn charge(&self, bytes: u64) -> Result<(), DurableObjectReadErrorV1> {
        charge_atomic(&self.bytes, bytes, self.max_bytes, "object-read bytes")?;
        if let Err(error) = charge_atomic(&self.reads, 1, self.max_reads, "object reads") {
            self.bytes.fetch_sub(bytes, Ordering::Relaxed);
            return Err(error);
        }
        Ok(())
    }

    fn reads(&self) -> u64 { self.reads.load(Ordering::Relaxed) }

    fn bytes(&self) -> u64 { self.bytes.load(Ordering::Relaxed) }
}

fn charge_atomic(
    counter: &AtomicU64,
    amount: u64,
    limit: u64,
    resource: &'static str,
) -> Result<(), DurableObjectReadErrorV1> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let attempted =
            current
                .checked_add(amount)
                .ok_or(DurableObjectReadErrorV1::AdmissionReadLimit {
                    resource,
                    limit,
                    attempted: u64::MAX,
                })?;
        if attempted > limit {
            return Err(DurableObjectReadErrorV1::AdmissionReadLimit {
                resource,
                limit,
                attempted,
            });
        }
        match counter.compare_exchange_weak(
            current,
            attempted,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

struct BoundedAdmissionStoreV1<'store> {
    base: &'store DurableIndexedObjectStoreV1,
    budget: AdmissionReadBudgetV1,
}

impl<'store> BoundedAdmissionStoreV1<'store> {
    fn new(base: &'store DurableIndexedObjectStoreV1, limits: DurableAdmissionLimitsV1) -> Self {
        Self {
            base,
            budget: AdmissionReadBudgetV1::new(
                limits.max_object_reads,
                limits.max_object_read_bytes,
            ),
        }
    }
}

impl ImmutableObjectStore for BoundedAdmissionStoreV1<'_> {
    type Error = DurableObjectReadErrorV1;
    type Object = Box<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        let Some(record) = self.base.lookup_bounded(digest, max_encoded_len)? else {
            return Ok(None);
        };
        self.budget.charge(record.reference().encoded_len())?;
        self.base.read_record(record).map(Some)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DurableAdmissionReportV1 {
    pub(crate) startup_disposition: IndexBindingDispositionV1,
    pub(crate) startup_work: DurableStartupWorkV1,
    pub(crate) current_semantics: SemanticValidationReport,
    pub(crate) retained_states: u64,
    pub(crate) aggregate_root_visits: u64,
    pub(crate) aggregate_references: u64,
    pub(crate) retained_union: RetainedUnionClosureReportV1,
    pub(crate) retention_manifest_canonical_bytes: u64,
    pub(crate) object_reads: u64,
    pub(crate) object_read_bytes: u64,
}

pub(crate) struct SemanticallyAdmittedDurableWriterV1<P: SemanticProfile> {
    lease: DurableWriterLeaseV1,
    store: DurableIndexedObjectStoreV1,
    authority: CatalogAuthorityTokenV1,
    version: StateVersionV1,
    retention_manifest_bytes: Box<[u8]>,
    semantic: SemanticallyAdmittedStateV1,
    retained_closure: StructuralClosureLedgerV1,
    report: DurableAdmissionReportV1,
    profile: PhantomData<fn() -> P>,
}

impl<P: SemanticProfile> SemanticallyAdmittedDurableWriterV1<P> {
    pub(crate) fn verified_state(&self) -> VerifiedDagState<'_, DurableIndexedObjectStoreV1, P> {
        VerifiedDagState::from_completed_admission(&self.store, self.version, self.semantic)
            .expect("durable admission token is bound to its exact catalog state")
    }

    pub(crate) const fn report(&self) -> DurableAdmissionReportV1 { self.report }

    pub(crate) const fn authority(&self) -> &CatalogAuthorityTokenV1 { &self.authority }

    pub(crate) const fn retention_manifest_bytes(&self) -> &[u8] { &self.retention_manifest_bytes }

    pub(crate) const fn retained_closure(&self) -> &StructuralClosureLedgerV1 {
        &self.retained_closure
    }

    /// Derive fresh-generation checkpoint authority from the exact closure
    /// and byte inventories admitted under this still-current catalog token.
    pub(crate) fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1> {
        let pack = self.authority.slot().pack();
        let retention_manifest = pack.retention_manifest();
        let inventory = CheckpointObjectInventoryV1::from_retained_union(
            &self.retained_closure,
            retention_manifest,
        )?;
        GenerationCheckpointDescriptorV1::new(
            P::ID,
            self.version,
            retention_manifest,
            pack.current_state_canonical_bytes(),
            inventory,
        )
    }

    /// Publication is an affine transition: later durable mutation code must
    /// consume this complete authority rather than borrowing the raw lease.
    pub(super) fn into_publication_parts(
        self,
    ) -> (DurableWriterLeaseV1, DurableIndexedObjectStoreV1, CatalogAuthorityTokenV1, StateVersionV1)
    {
        (self.lease, self.store, self.authority, self.version)
    }
}

impl DurableDirectoryV1 {
    pub(crate) fn recover_and_admit_v1<P: SemanticProfile>(
        self,
        recovery_limits: DurableLimits,
        wal_limits: LogPackDecodeLimits,
        admission_limits: DurableAdmissionLimitsV1,
    ) -> Result<SemanticallyAdmittedDurableWriterV1<P>, DurableAdmissionErrorV1> {
        let physical = self.recover_physical_writer_v1(recovery_limits, wal_limits)?;
        admit_physical_writer::<P>(physical, admission_limits)
    }
}

fn admit_physical_writer<P: SemanticProfile>(
    physical: PhysicallyRecoveredDurableWriterV1,
    limits: DurableAdmissionLimitsV1,
) -> Result<SemanticallyAdmittedDurableWriterV1<P>, DurableAdmissionErrorV1> {
    let (lease, recovered) = physical.into_admission_parts();
    let (authority, index, startup_disposition, startup_work) = recovered.into_admission_parts();
    let catalog = authority.slot();
    if catalog.profile_id() != P::ID {
        return Err(DurableAdmissionErrorV1::ProfileMismatch {
            expected: P::ID,
            actual: catalog.profile_id(),
        });
    }

    let store = DurableIndexedObjectStoreV1::pin(lease.directory(), &authority, index)?;
    require_same_authority(lease.directory(), &authority)?;
    let bounded = BoundedAdmissionStoreV1::new(&store, limits);
    let retention_ref = catalog.pack().retention_manifest();
    let retention_manifest_bytes = load_exact(&bounded, retention_ref)?;
    let retention = RetentionManifestView::parse(&retention_manifest_bytes)?;
    let historical_count = checked_len(retention.historical().len(), "retained historical states")?;
    let retained_states = historical_count
        .checked_add(1)
        .ok_or(CanonicalFormatError::CounterOverflow("retained states"))?;
    require_limit("retained states", limits.max_retained_states, retained_states)?;

    let retained_current = retention.current();
    if retained_current.state() != catalog.state() {
        return Err(DurableAdmissionErrorV1::RetentionCurrentMismatch {
            catalog: catalog.state(),
            manifest: retained_current.state(),
        });
    }

    let union_limits = RetainedUnionClosureLimitsV1 {
        max_distinct_objects: limits.max_union_objects,
        max_canonical_bytes: limits.max_union_canonical_bytes,
    };
    let mut ledger = StructuralClosureLedgerV1::new();
    let current_structural = verify_structural_closure_in_ledger_with_limits(
        &bounded,
        catalog.state().manifest(),
        limits.per_state_structural,
        &mut ledger,
        union_limits,
    )?;
    compare_inventory(
        InventoryMismatchKindV1::CurrentState,
        retained_current.canonical_bytes(),
        current_structural.report().canonical_bytes,
    )?;
    compare_inventory(
        InventoryMismatchKindV1::CurrentState,
        catalog.pack().current_state_canonical_bytes(),
        current_structural.report().canonical_bytes,
    )?;

    let mut aggregate_root_visits = current_structural.report().distinct_objects;
    let mut aggregate_references = current_structural.report().references;
    require_limit(
        "aggregate retained-root visits",
        limits.max_aggregate_root_visits,
        aggregate_root_visits,
    )?;
    require_limit(
        "aggregate retained references",
        limits.max_aggregate_references,
        aggregate_references,
    )?;

    for retained in retention.historical() {
        let retained = retained?;
        if retained.state().generation() >= catalog.state().generation() {
            return Err(DurableAdmissionErrorV1::NonHistoricalState {
                current_generation: catalog.state().generation(),
                retained_generation: retained.state().generation(),
            });
        }
        let admitted = verify_structural_closure_in_ledger_with_limits(
            &bounded,
            retained.state().manifest(),
            limits.per_state_structural,
            &mut ledger,
            union_limits,
        )?;
        compare_inventory(
            InventoryMismatchKindV1::RetainedState {
                generation: retained.state().generation(),
            },
            retained.canonical_bytes(),
            admitted.report().canonical_bytes,
        )?;
        aggregate_root_visits = aggregate_root_visits
            .checked_add(admitted.report().distinct_objects)
            .ok_or(CanonicalFormatError::CounterOverflow("aggregate retained-root visits"))?;
        aggregate_references = aggregate_references
            .checked_add(admitted.report().references)
            .ok_or(CanonicalFormatError::CounterOverflow("aggregate retained references"))?;
        require_limit(
            "aggregate retained-root visits",
            limits.max_aggregate_root_visits,
            aggregate_root_visits,
        )?;
        require_limit(
            "aggregate retained references",
            limits.max_aggregate_references,
            aggregate_references,
        )?;
    }

    let retained_union = ledger.retained_union_report();
    compare_inventory(
        InventoryMismatchKindV1::RetainedUnion,
        catalog.pack().retained_canonical_bytes(),
        retained_union.canonical_bytes,
    )?;
    let semantic = verify_rspace_semantics_after_structural::<P, _>(
        &bounded,
        current_structural,
        limits.current_semantics,
    )?;
    require_same_authority(lease.directory(), &authority)?;

    let report = DurableAdmissionReportV1 {
        startup_disposition,
        startup_work,
        current_semantics: semantic.report(),
        retained_states,
        aggregate_root_visits,
        aggregate_references,
        retained_union,
        retention_manifest_canonical_bytes: retention_ref.encoded_len(),
        object_reads: bounded.budget.reads(),
        object_read_bytes: bounded.budget.bytes(),
    };
    drop(bounded);
    Ok(SemanticallyAdmittedDurableWriterV1 {
        lease,
        store,
        authority,
        version: catalog.state(),
        retention_manifest_bytes,
        semantic,
        retained_closure: ledger,
        report,
        profile: PhantomData,
    })
}

fn require_same_authority(
    directory: &DurableDirectoryV1,
    authority: &CatalogAuthorityTokenV1,
) -> Result<(), DurableAdmissionErrorV1> {
    let current = read_catalog_token(directory)?;
    if current.matches_exactly(authority.image()) {
        Ok(())
    } else {
        Err(DurableAdmissionErrorV1::AuthorityChanged)
    }
}

fn load_exact<S: ImmutableObjectStore>(
    store: &S,
    reference: ObjectRefV1,
) -> Result<Box<[u8]>, DurableAdmissionErrorV1>
where
    S::Error: Into<DurableObjectReadErrorV1>,
    S::Object: Into<Box<[u8]>>,
{
    let bytes = store
        .get_bounded(&reference.digest(), reference.encoded_len())
        .map_err(Into::into)?
        .ok_or(CanonicalFormatError::MissingObject(reference.digest()))?
        .into();
    let actual_len = checked_len(bytes.len(), "retention manifest object")?;
    if actual_len != reference.encoded_len() {
        return Err(CanonicalFormatError::ObjectLengthMismatch {
            expected: reference.encoded_len(),
            actual: actual_len,
        }
        .into());
    }
    let actual = ObjectDigest::of_canonical_bytes(&bytes);
    if actual != reference.digest() {
        return Err(CanonicalFormatError::DigestMismatch {
            expected: reference.digest(),
            actual,
        }
        .into());
    }
    Ok(bytes)
}

fn compare_inventory(
    kind: InventoryMismatchKindV1,
    declared: u64,
    computed: u64,
) -> Result<(), DurableAdmissionErrorV1> {
    if declared == computed {
        Ok(())
    } else {
        Err(DurableAdmissionErrorV1::InventoryMismatch {
            kind,
            declared,
            computed,
        })
    }
}

fn require_limit(
    resource: &'static str,
    limit: u64,
    attempted: u64,
) -> Result<(), DurableAdmissionErrorV1> {
    if attempted <= limit {
        Ok(())
    } else {
        Err(CanonicalFormatError::ResourceLimit {
            resource,
            limit,
            attempted,
        }
        .into())
    }
}
