use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyIteration, ZipperValues};
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

#[cfg(test)]
#[path = "../../../../tests/persistence/journal_owner_hooks.rs"]
pub(crate) mod hooks;

use super::admission::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableAuthorityCoreV1,
    DurableIndexedObjectStoreV1, DurableTransactionPreparationError, TrustedSuccessorPolicyV1,
    admit_physical_source,
};
use super::io::DurableWriterLeaseV1;
use super::publication::{IncrementalPublicationErrorV1, publish_trusted_incremental_v1};
use super::startup::recover_physical_from_lease_v1;
use super::state_root::{
    DurableStateReadHandleErrorV1, DurableStateRootV1, ResolvedDurableStateV1,
};
use super::{
    CatalogSlotV1, DurableAuthorityViewV1, DurableObjectReadErrorV1, DurableViewReadHandleV1,
    require_shareable_read_handle,
};
use crate::rspace::errors::{RSpaceError, RootError};
use crate::rspace::hot_store::{
    CapturedHotJournalV1, HotStore, PendingDeltas, mutation_batch_from_observation,
    stage_pending_continuation_sources,
};
use crate::rspace::persistence::canonical::{
    CanonicalSink, OBJECT_DIGEST_LEN, ObjectDigest, ObjectRefV1, write_u64,
};
use crate::rspace::persistence::difference::{StateDifferenceLimits, visit_state_difference};
use crate::rspace::persistence::manifest::{LegacyHistoryRoot, SemanticNamespace};
use crate::rspace::persistence::mutation::{ContentTransition, MutationBatchViewV1, MutationError};
use crate::rspace::persistence::{
    CanonicalEncode, CanonicalFormatError, ClosureError, DagTransactionError, DagTransactionLimits,
    DurableLimits, GenerationLeaseError, GenerationLeaseKeyV1, GenerationLeaseRegistryV1,
    GenerationLeaseV1, LogPackDecodeLimits, LogPackFormatError, OpenDagTransaction,
    RSpaceMutationBatchV1, SemanticProfile, SemanticProfileId, SemanticStateManifestView,
    StateVersionV1,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DurableRSpacePublicationLimitsV1 {
    pub transaction: DagTransactionLimits,
    pub recovery: DurableLimits,
    pub wal: LogPackDecodeLimits,
    pub admission: DurableAdmissionLimitsV1,
    pub trusted: TrustedSuccessorPolicyV1,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewCheckpointLimitsV1 {
    pub publication: DurableRSpacePublicationLimitsV1,
    pub difference: StateDifferenceLimits,
}

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "most variant payloads are diagnostic context read only by the derived Debug impl"
)]
pub(crate) enum DurableRSpaceTransactionError {
    Mutation(MutationError),
    Canonical(crate::rspace::persistence::CanonicalFormatError),
    Transaction(DagTransactionError<DurableObjectReadErrorV1>),
    Preparation(DurableTransactionPreparationError),
    Publication(IncrementalPublicationErrorV1),
    Lease(GenerationLeaseError),
    Sequence(LogPackFormatError),
    Journal(RSpaceError),
    Recovery(DurableAdmissionErrorV1),
    Unresolved(JournalPublicationPhaseV1),
    UnexpectedTerminal {
        predecessor: CatalogSlotV1,
        expected: Option<CatalogSlotV1>,
        actual: CatalogSlotV1,
    },
    Closed,
    IncarnationExhausted(OwnerIncarnationExhaustedV1),
    ForeignPermit,
    Attachment(ViewAttachmentRefusalV1),
    Root(RootError),
    Inheritance {
        current: ManifestInheritanceV1,
        base: ManifestInheritanceV1,
    },
    Rebase(ClosureError<DurableObjectReadErrorV1>),
}

impl From<MutationError> for DurableRSpaceTransactionError {
    fn from(error: MutationError) -> Self { Self::Mutation(error) }
}

impl From<crate::rspace::persistence::CanonicalFormatError> for DurableRSpaceTransactionError {
    fn from(error: crate::rspace::persistence::CanonicalFormatError) -> Self {
        Self::Canonical(error)
    }
}

impl From<DagTransactionError<DurableObjectReadErrorV1>> for DurableRSpaceTransactionError {
    fn from(error: DagTransactionError<DurableObjectReadErrorV1>) -> Self {
        Self::Transaction(error)
    }
}

impl From<DurableTransactionPreparationError> for DurableRSpaceTransactionError {
    fn from(error: DurableTransactionPreparationError) -> Self { Self::Preparation(error) }
}

impl From<IncrementalPublicationErrorV1> for DurableRSpaceTransactionError {
    fn from(error: IncrementalPublicationErrorV1) -> Self { Self::Publication(error) }
}

impl From<GenerationLeaseError> for DurableRSpaceTransactionError {
    fn from(error: GenerationLeaseError) -> Self { Self::Lease(error) }
}

impl From<LogPackFormatError> for DurableRSpaceTransactionError {
    fn from(error: LogPackFormatError) -> Self { Self::Sequence(error) }
}

impl From<RSpaceError> for DurableRSpaceTransactionError {
    fn from(error: RSpaceError) -> Self { Self::Journal(error) }
}

impl From<OwnerIncarnationExhaustedV1> for DurableRSpaceTransactionError {
    fn from(error: OwnerIncarnationExhaustedV1) -> Self { Self::IncarnationExhausted(error) }
}

impl From<ViewAttachmentRefusalV1> for DurableRSpaceTransactionError {
    fn from(refusal: ViewAttachmentRefusalV1) -> Self {
        match refusal {
            ViewAttachmentRefusalV1::OwnerClosed => Self::Closed,
            refusal => Self::Attachment(refusal),
        }
    }
}

impl From<ClosureError<DurableObjectReadErrorV1>> for DurableRSpaceTransactionError {
    fn from(error: ClosureError<DurableObjectReadErrorV1>) -> Self { Self::Rebase(error) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalPublicationPhaseV1 {
    Ready,
    Invalidated,
    Captured,
    RecoveryRequired,
    AwaitingAck,
    Closed,
}

static OWNER_INCARNATIONS: OwnerIncarnationAllocatorV1 = OwnerIncarnationAllocatorV1::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OwnerIncarnationV1(u64);

impl OwnerIncarnationV1 {
    pub(crate) const fn get(self) -> u64 { self.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OwnerIncarnationExhaustedV1;

pub(crate) struct OwnerIncarnationAllocatorV1 {
    next: AtomicU64,
}

impl OwnerIncarnationAllocatorV1 {
    pub(crate) const fn new() -> Self { Self::starting_at(0) }

    pub(crate) const fn starting_at(next: u64) -> Self {
        Self {
            next: AtomicU64::new(next),
        }
    }

    pub(crate) fn allocate(&self) -> Result<OwnerIncarnationV1, OwnerIncarnationExhaustedV1> {
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
            .map(OwnerIncarnationV1)
            .map_err(|_| OwnerIncarnationExhaustedV1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewAttachmentV1 {
    incarnation: OwnerIncarnationV1,
    base: CatalogSlotV1,
}

impl ViewAttachmentV1 {
    pub(crate) const fn incarnation(&self) -> OwnerIncarnationV1 { self.incarnation }

    pub(crate) const fn base(&self) -> CatalogSlotV1 { self.base }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewBaseV1 {
    incarnation: OwnerIncarnationV1,
    state: ResolvedDurableStateV1,
}

impl ViewBaseV1 {
    pub(crate) const fn new(
        incarnation: OwnerIncarnationV1,
        state: ResolvedDurableStateV1,
    ) -> Self {
        Self { incarnation, state }
    }

    pub(crate) const fn incarnation(&self) -> OwnerIncarnationV1 { self.incarnation }

    pub(crate) const fn state(&self) -> ResolvedDurableStateV1 { self.state }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManifestInheritanceV1 {
    schema_epoch: u64,
    profile_id: SemanticProfileId,
    legacy_history_root: Option<LegacyHistoryRoot>,
}

impl ManifestInheritanceV1 {
    fn of(manifest: SemanticStateManifestView) -> Self {
        Self {
            schema_epoch: manifest.schema_epoch(),
            profile_id: manifest.profile_id(),
            legacy_history_root: manifest.legacy_history_root(),
        }
    }

    pub(crate) const fn schema_epoch(&self) -> u64 { self.schema_epoch }

    pub(crate) const fn profile_id(&self) -> SemanticProfileId { self.profile_id }

    pub(crate) const fn legacy_history_root(&self) -> Option<LegacyHistoryRoot> {
        self.legacy_history_root
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewAttachmentRefusalV1 {
    OwnerClosed,
    StaleIncarnation {
        attached: OwnerIncarnationV1,
        current: OwnerIncarnationV1,
    },
    StaleBase {
        attached: Box<CatalogSlotV1>,
        current: Box<CatalogSlotV1>,
    },
}

enum OwnerLifecycleV1 {
    Live {
        incarnation: OwnerIncarnationV1,
        lease: DurableWriterLeaseV1,
        generation_lease: GenerationLeaseV1,
    },
    Closed {
        generation: GenerationLeaseKeyV1,
    },
}

type OwnerHotStoreV1<P> = Arc<
    Box<
        dyn HotStore<
                <P as SemanticProfile>::Channel,
                <P as SemanticProfile>::Pattern,
                <P as SemanticProfile>::Datum,
                <P as SemanticProfile>::Continuation,
            >,
    >,
>;

pub(crate) struct DurablePublishedCoreV1<P: SemanticProfile> {
    core: Arc<std::sync::RwLock<Arc<DurableAuthorityCoreV1<P>>>>,
    registry: GenerationLeaseRegistryV1,
}

impl<P: SemanticProfile> Clone for DurablePublishedCoreV1<P> {
    fn clone(&self) -> Self {
        Self {
            core: Arc::clone(&self.core),
            registry: self.registry.clone(),
        }
    }
}

impl<P: SemanticProfile> DurablePublishedCoreV1<P> {
    fn new(core: Arc<DurableAuthorityCoreV1<P>>, registry: GenerationLeaseRegistryV1) -> Self {
        const { require_shareable_read_handle::<Self>() };
        Self {
            core: Arc::new(std::sync::RwLock::new(core)),
            registry,
        }
    }

    fn install(&self, successor: Arc<DurableAuthorityCoreV1<P>>) {
        let superseded = {
            let mut published = match self.core.write() {
                Ok(published) => published,
                Err(poisoned) => poisoned.into_inner(),
            };
            std::mem::replace(&mut *published, successor)
        };
        drop(superseded);
    }

    fn load(&self) -> Arc<DurableAuthorityCoreV1<P>> {
        let published = match self.core.read() {
            Ok(published) => published,
            Err(poisoned) => poisoned.into_inner(),
        };
        Arc::clone(&published)
    }

    fn pin(
        &self,
    ) -> Result<(Arc<DurableAuthorityCoreV1<P>>, GenerationLeaseV1), GenerationLeaseError> {
        let mut core = self.load();
        loop {
            #[cfg(test)]
            hooks::run(hooks::JournalOwnerHookPoint::AfterPublishedCoreLoad);
            match self
                .registry
                .acquire(GenerationLeaseKeyV1::from_catalog(core.authority().slot()))
            {
                Ok(lease) => return Ok((core, lease)),
                Err(GenerationLeaseError::Retired(refused)) => {
                    let reloaded = self.load();
                    if GenerationLeaseKeyV1::from_catalog(reloaded.authority().slot()) == refused {
                        return Err(GenerationLeaseError::Retired(refused));
                    }
                    core = reloaded;
                }
                Err(refusal) => return Err(refusal),
            }
        }
    }

    pub(crate) fn current_root(&self) -> DurableStateRootV1 {
        DurableStateRootV1::of_manifest(self.load().authority().slot().state().manifest())
    }

    pub(crate) fn read_handle(
        &self,
        target: Option<DurableStateRootV1>,
    ) -> Result<(ResolvedDurableStateV1, DurableViewReadHandleV1<P>), DurableStateReadHandleErrorV1>
    {
        let (core, lease) = self.pin()?;
        let root = target.unwrap_or_else(|| {
            DurableStateRootV1::of_manifest(core.authority().slot().state().manifest())
        });
        let resolved = core
            .resolve(root)
            .map_err(DurableStateReadHandleErrorV1::Root)?;
        Ok((resolved, DurableViewReadHandleV1 {
            core,
            manifest: resolved.manifest(),
            _lease: lease,
        }))
    }
}

#[cfg(test)]
impl<P: SemanticProfile> DurablePublishedCoreV1<P> {
    pub(crate) fn detached_for_test(
        core: Arc<DurableAuthorityCoreV1<P>>,
        registry: GenerationLeaseRegistryV1,
    ) -> Self {
        Self::new(core, registry)
    }

    pub(crate) fn install_for_test(&self, core: Arc<DurableAuthorityCoreV1<P>>) {
        self.install(core);
    }
}

const VIEW_TRANSITION_MEMO_CAPACITY: usize = 4_096;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ViewTransitionKeyV1 {
    base: ObjectRefV1,
    journal: ObjectDigest,
}

impl ViewTransitionKeyV1 {
    fn of(base: ObjectRefV1, captured: &CapturedHotJournalV1) -> Option<Self> {
        let observation = captured.observation();
        let continuations = observation.pending.continuation_content.as_ref()?;
        let mut digest = JournalAfterValuesDigestV1(Blake2b::<U32>::new());
        match write_journal_after_values(
            &mut digest,
            &observation.data_counts,
            continuations,
            &observation.join_counts,
        ) {
            Ok(()) => {
                let journal: [u8; OBJECT_DIGEST_LEN] = digest.0.finalize().into();
                Some(Self {
                    base,
                    journal: ObjectDigest::from_array(journal),
                })
            }
            Err(never) => match never {},
        }
    }
}

struct JournalAfterValuesDigestV1(Blake2b<U32>);

impl CanonicalSink for JournalAfterValuesDigestV1 {
    type Error = Infallible;

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.update(bytes);
        Ok(())
    }
}

fn write_journal_after_values<S: CanonicalSink>(
    sink: &mut S,
    data: &PathMap<u64>,
    continuations: &PathMap<ContentTransition>,
    joins: &PathMap<u64>,
) -> Result<(), S::Error> {
    write_after_values(sink, SemanticNamespace::DataBag, data, |sink, after| {
        write_u64(sink, *after)
    })?;
    write_after_values(
        sink,
        SemanticNamespace::ContinuationMap,
        continuations,
        |sink, transition| match transition.after() {
            None => sink.write_all(&[0]),
            Some(reference) => {
                sink.write_all(&[1])?;
                reference.write(sink)
            }
        },
    )?;
    write_after_values(sink, SemanticNamespace::JoinBag, joins, |sink, after| {
        write_u64(sink, *after)
    })
}

fn write_after_values<S, T>(
    sink: &mut S,
    namespace: SemanticNamespace,
    entries: &PathMap<T>,
    mut write_after: impl FnMut(&mut S, &T) -> Result<(), S::Error>,
) -> Result<(), S::Error>
where
    S: CanonicalSink,
    T: Clone + Send + Sync + Unpin,
{
    write_u64(sink, namespace as u64)?;
    write_u64(sink, entries.val_count() as u64)?;
    let mut write_entry = |path: &[u8], after: &T| {
        write_u64(sink, path.len() as u64)?;
        sink.write_all(path)?;
        write_after(sink, after)
    };
    let mut cursor = entries.read_zipper_at_borrowed_path(&[]);
    if let Some(after) = cursor.val() {
        write_entry(cursor.path(), after)?;
    }
    while let Some(after) = cursor.to_next_get_val() {
        write_entry(cursor.path(), after)?;
    }
    Ok(())
}

struct ViewTransitionMemoV1 {
    capacity: usize,
    order: VecDeque<ViewTransitionKeyV1>,
    successors: HashMap<ViewTransitionKeyV1, ResolvedDurableStateV1>,
}

impl ViewTransitionMemoV1 {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            successors: HashMap::with_capacity(capacity),
        }
    }

    fn record(&mut self, key: Option<ViewTransitionKeyV1>, successor: ResolvedDurableStateV1) {
        let Some(key) = key.filter(|_| self.capacity > 0) else {
            return;
        };
        match self.successors.get_mut(&key) {
            Some(memoised) => *memoised = successor,
            None => {
                if self.order.len() == self.capacity {
                    let oldest = self
                        .order
                        .pop_front()
                        .expect("a full memo of positive capacity holds its oldest key");
                    self.successors.remove(&oldest);
                }
                self.order.push_back(key);
                self.successors.insert(key, successor);
            }
        }
    }
}

pub(crate) struct DurableRSpaceAuthorityV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    lifecycle: OwnerLifecycleV1,
    registry: GenerationLeaseRegistryV1,
    source: Arc<DurableAuthorityCoreV1<P>>,
    published: DurablePublishedCoreV1<P>,
    pending: Option<Box<PendingJournalPublicationV1<P>>>,
    transitions: ViewTransitionMemoV1,
}

struct PendingJournalPublicationV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    permit: OwnedRwLockWriteGuard<()>,
    hot: OwnerHotStoreV1<P>,
    captured: CapturedHotJournalV1,
    predecessor: CatalogSlotV1,
    expected: Option<CatalogSlotV1>,
    anchor: PendingJournalAnchorV1,
    phase: PendingJournalPhaseV1<P>,
}

#[derive(Clone, Copy)]
enum PendingJournalAnchorV1 {
    CurrentSlot,
    RetainedBase,
}

enum PendingJournalPhaseV1<P: SemanticProfile> {
    Captured,
    RecoveryRequired,
    AwaitingAck {
        successor: Arc<DurableAuthorityCoreV1<P>>,
        generation_lease: Option<GenerationLeaseV1>,
    },
}

impl<P: SemanticProfile> PendingJournalPhaseV1<P> {
    fn observed(&self) -> JournalPublicationPhaseV1 {
        match self {
            Self::Captured => JournalPublicationPhaseV1::Captured,
            Self::RecoveryRequired => JournalPublicationPhaseV1::RecoveryRequired,
            Self::AwaitingAck { .. } => JournalPublicationPhaseV1::AwaitingAck,
        }
    }
}

impl<P: SemanticProfile> DurableRSpaceAuthorityV1<P>
where P::Channel: Clone + Hash + Eq
{
    pub(crate) fn from_view(
        view: DurableAuthorityViewV1<P>,
        hot: &OwnerHotStoreV1<P>,
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, DurableRSpaceTransactionError)>> {
        Self::from_view_with(view, hot, &OWNER_INCARNATIONS)
    }

    pub(crate) fn from_view_with(
        view: DurableAuthorityViewV1<P>,
        hot: &OwnerHotStoreV1<P>,
        incarnations: &OwnerIncarnationAllocatorV1,
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, DurableRSpaceTransactionError)>> {
        Self::open(view, hot, incarnations, VIEW_TRANSITION_MEMO_CAPACITY)
    }

    fn open(
        view: DurableAuthorityViewV1<P>,
        hot: &OwnerHotStoreV1<P>,
        incarnations: &OwnerIncarnationAllocatorV1,
        memo_capacity: usize,
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, DurableRSpaceTransactionError)>> {
        if let Err(error) =
            hot.validate_journal_authority(view.catalog(), &view.generation_leases())
        {
            return Err(Box::new((view, error.into())));
        }
        let incarnation = match incarnations.allocate() {
            Ok(incarnation) => incarnation,
            Err(exhausted) => return Err(Box::new((view, exhausted.into()))),
        };
        let DurableAuthorityViewV1 {
            writer,
            _generation_lease,
        } = view;
        let (lease, source) = writer.into_parts();
        let registry = lease.generation_leases();
        let source = source.into_core();
        let published = DurablePublishedCoreV1::new(Arc::clone(&source), registry.clone());
        Ok(Self {
            lifecycle: OwnerLifecycleV1::Live {
                incarnation,
                lease,
                generation_lease: _generation_lease,
            },
            registry,
            source,
            published,
            pending: None,
            transitions: ViewTransitionMemoV1::with_capacity(memo_capacity),
        })
    }

    fn memoised_successor(
        &self,
        key: Option<ViewTransitionKeyV1>,
    ) -> Option<ResolvedDurableStateV1> {
        key.and_then(|key| self.transitions.successors.get(&key).copied())
            .filter(|successor| self.source.require_retained(successor).is_ok())
    }

    fn pending_phase(&self) -> Option<JournalPublicationPhaseV1> {
        self.pending
            .as_ref()
            .map(|pending| pending.phase.observed())
    }

    fn quiescent_state(&self) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live { .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        match self.pending_phase() {
            Some(phase) => Err(DurableRSpaceTransactionError::Unresolved(phase)),
            None => Ok(self.source.authority().slot().state()),
        }
    }

    pub(crate) fn phase(&self, hot: &OwnerHotStoreV1<P>) -> JournalPublicationPhaseV1 {
        match (&self.lifecycle, self.pending_phase()) {
            (OwnerLifecycleV1::Closed { .. }, _) => JournalPublicationPhaseV1::Closed,
            (_, Some(phase)) => phase,
            (_, None) => match self.validate_base(hot) {
                Ok(()) => JournalPublicationPhaseV1::Ready,
                Err(_) => JournalPublicationPhaseV1::Invalidated,
            },
        }
    }

    pub(crate) fn state(
        &self,
        hot: &OwnerHotStoreV1<P>,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let state = self.quiescent_state()?;
        self.validate_base(hot)?;
        Ok(state)
    }

    fn validate_base(&self, hot: &OwnerHotStoreV1<P>) -> Result<(), RSpaceError> {
        hot.validate_journal_authority(self.source.authority().slot(), &self.registry)
    }

    fn validate_unpublished(&self, hot: &OwnerHotStoreV1<P>) -> Result<(), RSpaceError> {
        match self.pending.as_deref() {
            Some(PendingJournalPublicationV1 {
                anchor: PendingJournalAnchorV1::RetainedBase,
                captured,
                ..
            }) => hot.validate_pending_journal(captured),
            Some(_) | None => self.validate_base(hot),
        }
    }

    fn resolve_state(
        &self,
        state: StateVersionV1,
    ) -> Result<ResolvedDurableStateV1, DurableRSpaceTransactionError> {
        self.source
            .resolve(DurableStateRootV1::of_manifest(state.manifest()))
            .map_err(DurableRSpaceTransactionError::Root)
    }

    fn inheritable_current_manifest(
        &self,
        base: &ResolvedDurableStateV1,
    ) -> Result<SemanticStateManifestView, DurableRSpaceTransactionError> {
        let verified = self.source.verified_state();
        let current = verified.semantic_manifest()?;
        let retained =
            SemanticStateManifestView::parse(verified.load_object(base.manifest())?.as_ref())
                .map_err(ClosureError::<DurableObjectReadErrorV1>::Format)?;
        match (ManifestInheritanceV1::of(current), ManifestInheritanceV1::of(retained)) {
            (inherited, required) if inherited == required => Ok(current),
            (inherited, required) => Err(DurableRSpaceTransactionError::Inheritance {
                current: inherited,
                base: required,
            }),
        }
    }

    pub(crate) fn pending_terminal(&self) -> Option<CatalogSlotV1> {
        self.pending.as_ref().and_then(|pending| pending.expected)
    }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 { self.registry.clone() }

    pub(crate) fn generation_key(&self) -> GenerationLeaseKeyV1 {
        match &self.lifecycle {
            OwnerLifecycleV1::Live {
                generation_lease, ..
            } => generation_lease.key(),
            OwnerLifecycleV1::Closed { generation } => *generation,
        }
    }

    pub(crate) fn current_root(&self) -> DurableStateRootV1 {
        DurableStateRootV1::of_manifest(self.source.authority().slot().state().manifest())
    }

    pub(crate) fn published_core(&self) -> DurablePublishedCoreV1<P> { self.published.clone() }

    pub(crate) fn attach(
        &self,
        hot: &OwnerHotStoreV1<P>,
    ) -> Result<ViewAttachmentV1, DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live { incarnation, .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        self.state(hot)?;
        Ok(ViewAttachmentV1 {
            incarnation,
            base: self.source.authority().slot(),
        })
    }

    pub(crate) fn validate_attachment(
        &self,
        attachment: ViewAttachmentV1,
    ) -> Result<(), ViewAttachmentRefusalV1> {
        let OwnerLifecycleV1::Live { incarnation, .. } = self.lifecycle else {
            return Err(ViewAttachmentRefusalV1::OwnerClosed);
        };
        if attachment.incarnation != incarnation {
            return Err(ViewAttachmentRefusalV1::StaleIncarnation {
                attached: attachment.incarnation,
                current: incarnation,
            });
        }
        let current = self.source.authority().slot();
        if attachment.base != current {
            return Err(ViewAttachmentRefusalV1::StaleBase {
                attached: Box::new(attachment.base),
                current: Box::new(current),
            });
        }
        Ok(())
    }

    pub(crate) fn close(&mut self) -> Result<(), DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live {
            generation_lease, ..
        } = &self.lifecycle
        else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        let generation = generation_lease.key();
        match self.pending_phase() {
            Some(phase) => Err(DurableRSpaceTransactionError::Unresolved(phase)),
            None => {
                self.lifecycle = OwnerLifecycleV1::Closed { generation };
                self.transitions = ViewTransitionMemoV1::with_capacity(0);
                Ok(())
            }
        }
    }

    pub(crate) async fn checkpoint_view(
        &mut self,
        permit: OwnedRwLockWriteGuard<()>,
        attachment: ViewAttachmentV1,
        gate: &Arc<RwLock<()>>,
        hot: &OwnerHotStoreV1<P>,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let (predecessor, captured) = self.capture_bound_journal(&permit, attachment, gate, hot)?;
        self.publish_bound_journal(permit, hot, predecessor, captured, limits, None)
            .map(|(reached, _)| reached)
    }

    fn capture_bound_journal(
        &self,
        permit: &OwnedRwLockWriteGuard<()>,
        attachment: ViewAttachmentV1,
        gate: &Arc<RwLock<()>>,
        hot: &OwnerHotStoreV1<P>,
    ) -> Result<(CatalogSlotV1, CapturedHotJournalV1), DurableRSpaceTransactionError> {
        if !Arc::ptr_eq(OwnedRwLockWriteGuard::rwlock(permit), gate) {
            return Err(DurableRSpaceTransactionError::ForeignPermit);
        }
        self.validate_attachment(attachment)?;
        if let Some(phase) = self.pending_phase() {
            return Err(DurableRSpaceTransactionError::Unresolved(phase));
        }
        self.validate_base(hot)?;
        let predecessor = self.source.authority().slot();
        let captured = hot.capture_pending_journal(predecessor)?;
        Ok((predecessor, captured))
    }

    fn publish_bound_journal(
        &mut self,
        permit: OwnedRwLockWriteGuard<()>,
        hot: &OwnerHotStoreV1<P>,
        predecessor: CatalogSlotV1,
        captured: CapturedHotJournalV1,
        limits: DurableRSpacePublicationLimitsV1,
        prebuilt: Option<RSpaceMutationBatchV1>,
    ) -> Result<(StateVersionV1, OwnedRwLockWriteGuard<()>), DurableRSpaceTransactionError> {
        self.pending = Some(Box::new(PendingJournalPublicationV1 {
            permit,
            hot: Arc::clone(hot),
            captured,
            predecessor,
            expected: None,
            anchor: PendingJournalAnchorV1::CurrentSlot,
            phase: PendingJournalPhaseV1::Captured,
        }));
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterCapture);
        let publication = self.publish_captured(hot, limits, prebuilt);
        if let (Err(_), Some(JournalPublicationPhaseV1::Captured)) =
            (&publication, self.pending_phase())
        {
            self.validate_base(hot)?;
            self.pending = None;
        }
        match publication? {
            Some(permit) => Ok((self.quiescent_state()?, permit)),
            None => self.acknowledge_successor(),
        }
    }

    pub(crate) async fn checkpoint_view_from_base(
        &mut self,
        permit: OwnedRwLockWriteGuard<()>,
        view: ViewBaseV1,
        gate: &Arc<RwLock<()>>,
        hot: &OwnerHotStoreV1<P>,
        limits: ViewCheckpointLimitsV1,
    ) -> Result<(ResolvedDurableStateV1, OwnedRwLockWriteGuard<()>), DurableRSpaceTransactionError>
    {
        if !Arc::ptr_eq(OwnedRwLockWriteGuard::rwlock(&permit), gate) {
            return Err(DurableRSpaceTransactionError::ForeignPermit);
        }
        let OwnerLifecycleV1::Live { incarnation, .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        if view.incarnation != incarnation {
            return Err(ViewAttachmentRefusalV1::StaleIncarnation {
                attached: view.incarnation,
                current: incarnation,
            }
            .into());
        }
        if let Some(phase) = self.pending_phase() {
            return Err(DurableRSpaceTransactionError::Unresolved(phase));
        }
        self.source
            .require_retained(&view.state)
            .map_err(DurableRSpaceTransactionError::Root)?;
        let predecessor = self.source.authority().slot();
        if view.state.manifest() == predecessor.state().manifest() {
            hot.bind_journal_authority(predecessor)?;
            let attachment = self.attach(hot)?;
            let (predecessor, captured) =
                self.capture_bound_journal(&permit, attachment, gate, hot)?;
            let (journalled, key) = match mutation_batch_from_observation(
                predecessor.state(),
                captured.observation(),
            ) {
                Ok(journalled) => {
                    (Some(journalled), ViewTransitionKeyV1::of(view.state.manifest(), &captured))
                }
                Err(_) => (None, None),
            };
            if let Some(memoised) = self.memoised_successor(key) {
                return Ok((memoised, permit));
            }
            let (reached, permit) = self.publish_bound_journal(
                permit,
                hot,
                predecessor,
                captured,
                limits.publication,
                journalled,
            )?;
            let reached = self.resolve_state(reached)?;
            self.transitions.record(key, reached);
            return Ok((reached, permit));
        }
        let current = self.inheritable_current_manifest(&view.state)?;
        let captured =
            hot.capture_cross_base_journal(view.state.manifest(), predecessor, &self.registry)?;
        let mut mutation =
            match mutation_batch_from_observation(predecessor.state(), captured.observation()) {
                Ok(mutation) => mutation,
                Err(MutationError::EmptyBatch) => return Ok((view.state, permit)),
                Err(error) => return Err(error.into()),
            };
        let key = ViewTransitionKeyV1::of(view.state.manifest(), &captured);
        if let Some(memoised) = self.memoised_successor(key) {
            return Ok((memoised, permit));
        }
        visit_state_difference(
            self.source.verified_state().object_store(),
            current.roots(),
            view.state.roots(),
            self.source.shape().maximum_logical_path_bytes()?,
            limits.difference,
            |namespace, path, current_value, base_value| {
                mutation
                    .precede_endpoint(namespace, path, current_value, base_value)
                    .map_err(rebase_refusal)
            },
        )?;
        if mutation.is_empty() {
            let reached = self.resolve_state(predecessor.state())?;
            self.transitions.record(key, reached);
            return Ok((reached, permit));
        }
        self.pending = Some(Box::new(PendingJournalPublicationV1 {
            permit,
            hot: Arc::clone(hot),
            captured,
            predecessor,
            expected: None,
            anchor: PendingJournalAnchorV1::RetainedBase,
            phase: PendingJournalPhaseV1::Captured,
        }));
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterCapture);
        let publication = self.publish_captured(hot, limits.publication, Some(mutation));
        if let (Err(_), Some(JournalPublicationPhaseV1::Captured)) =
            (&publication, self.pending_phase())
        {
            self.validate_unpublished(hot)?;
            self.pending = None;
        }
        let (reached, permit) = match publication? {
            Some(permit) => (self.quiescent_state()?, permit),
            None => self.acknowledge_successor()?,
        };
        let reached = self.resolve_state(reached)?;
        self.transitions.record(key, reached);
        Ok((reached, permit))
    }

    fn publish_captured(
        &mut self,
        hot: &OwnerHotStoreV1<P>,
        limits: DurableRSpacePublicationLimitsV1,
        prebuilt: Option<RSpaceMutationBatchV1>,
    ) -> Result<Option<OwnedRwLockWriteGuard<()>>, DurableRSpaceTransactionError> {
        let pending = self.pending.as_mut().expect("checkpoint owns its capture");
        hot.validate_pending_journal(&pending.captured)?;
        let observation = pending.captured.observation();
        let mutation = match prebuilt.map_or_else(
            || mutation_batch_from_observation(pending.predecessor.state(), observation),
            Ok,
        ) {
            Ok(mutation) => mutation,
            Err(MutationError::EmptyBatch) => {
                self.validate_base(hot)?;
                let PendingJournalPublicationV1 { permit, .. } =
                    *self.pending.take().expect("checkpoint owns its capture");
                return Ok(Some(permit));
            }
            Err(error) => return Err(error.into()),
        };
        let plan = mutation.plan()?;
        let bytes = plan.try_canonical_bytes()?;
        let view = MutationBatchViewV1::parse(&bytes)?;
        let mut transaction =
            OpenDagTransaction::new(self.source.verified_state(), view, limits.transaction)?;
        match pending.anchor {
            PendingJournalAnchorV1::CurrentSlot => {
                stage_pending_continuation_sources(&mut transaction, &observation.pending)?;
            }
            PendingJournalAnchorV1::RetainedBase => stage_rebased_continuation_sources(
                &mut transaction,
                view,
                &observation.pending,
                &self.source,
            )?,
        }
        let sealed = transaction.seal()?;
        let OwnerLifecycleV1::Live { lease, .. } = &mut self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        let sealed = self
            .source
            .bind_trusted_transaction(lease, sealed, limits.trusted)?;
        let sequence = pending.predecessor.commit_sequence().successor()?;
        let prepared = self
            .source
            .prepare_transaction(lease, &sealed, sequence, limits.wal)?;
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterPreparation);
        hot.validate_pending_journal(&pending.captured)?;
        pending.phase = PendingJournalPhaseV1::RecoveryRequired;
        let successor = publish_trusted_incremental_v1(
            lease,
            &self.source,
            &prepared,
            limits.trusted,
            |terminal| pending.expected = Some(terminal),
        )?;
        if pending.expected != Some(successor.authority().slot()) {
            return Err(DurableRSpaceTransactionError::UnexpectedTerminal {
                predecessor: pending.predecessor,
                expected: pending.expected,
                actual: successor.authority().slot(),
            });
        }
        pending.phase = PendingJournalPhaseV1::AwaitingAck {
            successor: Arc::new(successor),
            generation_lease: None,
        };
        Ok(None)
    }

    pub(crate) fn reconcile(
        &mut self,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let Some(pending) = self.pending.as_ref() else {
            return self.quiescent_state();
        };
        let hot = Arc::clone(&pending.hot);
        if let PendingJournalPhaseV1::AwaitingAck { .. } = pending.phase {
            return self.acknowledge_successor().map(|(reached, _)| reached);
        }
        self.validate_unpublished(&hot)?;
        let OwnerLifecycleV1::Live { lease, .. } = &mut self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        let physical = recover_physical_from_lease_v1(lease, limits.recovery, limits.wal).map_err(
            |error| {
                DurableRSpaceTransactionError::Recovery(DurableAdmissionErrorV1::Startup(error))
            },
        )?;
        let successor = admit_physical_source::<P>(lease, physical, limits.admission)
            .map_err(DurableRSpaceTransactionError::Recovery)?
            .into_core();
        let actual = successor.authority().slot();
        let pending = self
            .pending
            .as_mut()
            .expect("reconciliation retains its capture");
        if actual == pending.predecessor {
            let anchor = pending.anchor;
            match anchor {
                PendingJournalAnchorV1::CurrentSlot => {
                    hot.validate_journal_authority(actual, &self.registry)?;
                }
                PendingJournalAnchorV1::RetainedBase => {
                    hot.validate_pending_journal(&pending.captured)?;
                }
            }
            self.published.install(Arc::clone(&successor));
            self.source = successor;
            self.pending = None;
            return match anchor {
                PendingJournalAnchorV1::CurrentSlot => self.state(&hot),
                PendingJournalAnchorV1::RetainedBase => self.quiescent_state(),
            };
        }
        if pending.expected != Some(actual) {
            return Err(DurableRSpaceTransactionError::UnexpectedTerminal {
                predecessor: pending.predecessor,
                expected: pending.expected,
                actual,
            });
        }
        pending.phase = PendingJournalPhaseV1::AwaitingAck {
            successor,
            generation_lease: None,
        };
        self.acknowledge_successor().map(|(reached, _)| reached)
    }

    fn acknowledge_successor(
        &mut self,
    ) -> Result<(StateVersionV1, OwnedRwLockWriteGuard<()>), DurableRSpaceTransactionError> {
        let pending = self
            .pending
            .as_mut()
            .expect("acknowledgement follows an admitted publication");
        let hot = Arc::clone(&pending.hot);
        let observed = pending.phase.observed();
        let PendingJournalPhaseV1::AwaitingAck {
            successor,
            generation_lease,
        } = &mut pending.phase
        else {
            return Err(DurableRSpaceTransactionError::Unresolved(observed));
        };
        if generation_lease.is_none() {
            #[cfg(test)]
            hooks::run(hooks::JournalOwnerHookPoint::BeforeSuccessorPin);
            *generation_lease = Some(
                self.registry
                    .acquire(GenerationLeaseKeyV1::from_catalog(successor.authority().slot()))?,
            );
        }
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterSuccessorPin);
        hot.acknowledge_pending_journal(
            &pending.captured,
            pending
                .expected
                .expect("an admitted successor has its exact terminal"),
            successor.authority().slot(),
        )?;
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterAcknowledgement);
        hot.validate_journal_acknowledgement(
            &pending.captured,
            successor.authority().slot(),
            &self.registry,
        )?;
        let OwnerLifecycleV1::Live {
            generation_lease: current_pin,
            ..
        } = &mut self.lifecycle
        else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        let PendingJournalPublicationV1 { permit, phase, .. } = *self
            .pending
            .take()
            .expect("acknowledgement holds its pending publication");
        let PendingJournalPhaseV1::AwaitingAck {
            successor,
            generation_lease,
        } = phase
        else {
            unreachable!("acknowledgement preserves its admitted successor");
        };
        self.published.install(Arc::clone(&successor));
        self.source = successor;
        *current_pin = generation_lease.expect("successor pin precedes acknowledgement");
        self.state(&hot).map(|reached| (reached, permit))
    }
}

#[cfg(test)]
impl<P: SemanticProfile> DurableRSpaceAuthorityV1<P>
where P::Channel: Clone + Hash + Eq
{
    pub(crate) fn from_view_with_transition_memo_for_test(
        view: DurableAuthorityViewV1<P>,
        hot: &OwnerHotStoreV1<P>,
        capacity: usize,
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, DurableRSpaceTransactionError)>> {
        Self::open(view, hot, &OWNER_INCARNATIONS, capacity)
    }

    pub(crate) fn disable_transition_memo_for_test(&mut self) {
        self.transitions = ViewTransitionMemoV1::with_capacity(0);
    }

    pub(crate) fn transition_memo_lengths_for_test(&self) -> (usize, usize) {
        (self.transitions.order.len(), self.transitions.successors.len())
    }

    pub(crate) fn transition_memo_capacity_for_test(&self) -> usize { self.transitions.capacity }

    pub(crate) fn transition_memo_reserved_for_test(&self) -> usize {
        self.transitions
            .order
            .capacity()
            .min(self.transitions.successors.capacity())
    }

    pub(crate) fn replace_memoised_successors_for_test(
        &mut self,
        successor: ResolvedDurableStateV1,
    ) {
        self.transitions
            .successors
            .values_mut()
            .for_each(|memoised| *memoised = successor);
    }
}

fn rebase_refusal(refusal: MutationError) -> ClosureError<DurableObjectReadErrorV1> {
    ClosureError::Format(match refusal {
        MutationError::Canonical(format) => format,
        refusal => CanonicalFormatError::SemanticCoherence {
            detail: format!(
                "a state-difference endpoint cannot precede the view's journal: {refusal}"
            ),
        },
    })
}

fn stage_rebased_continuation_sources<P: SemanticProfile>(
    transaction: &mut OpenDagTransaction<'_, '_, DurableIndexedObjectStoreV1, P>,
    mutation: MutationBatchViewV1<'_>,
    journal: &PendingDeltas,
    current: &DurableAuthorityCoreV1<P>,
) -> Result<(), DurableRSpaceTransactionError> {
    let verified = current.verified_state();
    for entry in mutation.continuations() {
        let Some(expected) = entry.transition.after() else {
            continue;
        };
        let journalled = match (&journal.continuation_content, &journal.continuation_source_bytes) {
            (Some(transitions), Some(sources)) => transitions
                .get(entry.key)
                .filter(|transition| transition.after() == Some(expected))
                .and_then(|_| sources.get(entry.key).cloned()),
            (Some(_), None) | (None, Some(_)) | (None, None) => None,
        };
        let canonical = match journalled {
            Some(canonical) => canonical,
            None => Arc::from(verified.load_object(expected)?),
        };
        let (staged, _) = transaction.stage_continuation_source(canonical)?;
        if staged != expected {
            return Err(DagTransactionError::from(CanonicalFormatError::DigestMismatch {
                expected: expected.digest(),
                actual: staged.digest(),
            })
            .into());
        }
    }
    Ok(())
}
