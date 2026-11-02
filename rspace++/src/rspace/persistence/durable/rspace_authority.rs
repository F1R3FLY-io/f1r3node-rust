use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

#[cfg(test)]
#[path = "../../../../tests/persistence/journal_owner_hooks.rs"]
pub(crate) mod hooks;

use super::admission::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableAuthorityCoreV1,
    DurableTransactionPreparationError, TrustedSuccessorPolicyV1, admit_physical_source,
};
use super::io::DurableWriterLeaseV1;
use super::publication::{IncrementalPublicationErrorV1, publish_trusted_incremental_v1};
use super::startup::recover_physical_from_lease_v1;
use super::{CatalogSlotV1, DurableAuthorityViewV1, DurableObjectReadErrorV1};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::{
    CapturedHotJournalV1, HotStore, mutation_batch_from_observation,
    stage_pending_continuation_sources,
};
use crate::rspace::persistence::mutation::{MutationBatchViewV1, MutationError};
use crate::rspace::persistence::{
    CanonicalEncode, DagTransactionError, DagTransactionLimits, DurableLimits,
    GenerationLeaseError, GenerationLeaseKeyV1, GenerationLeaseRegistryV1, GenerationLeaseV1,
    LogPackDecodeLimits, LogPackFormatError, OpenDagTransaction, SemanticProfile, StateVersionV1,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DurableRSpacePublicationLimitsV1 {
    pub transaction: DagTransactionLimits,
    pub recovery: DurableLimits,
    pub wal: LogPackDecodeLimits,
    pub admission: DurableAdmissionLimitsV1,
    pub trusted: TrustedSuccessorPolicyV1,
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

pub(crate) struct DurableRSpaceAuthorityV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    lifecycle: OwnerLifecycleV1,
    registry: GenerationLeaseRegistryV1,
    source: Arc<DurableAuthorityCoreV1<P>>,
    hot: Arc<Box<dyn HotStore<P::Channel, P::Pattern, P::Datum, P::Continuation>>>,
    operation_gate: Arc<RwLock<()>>,
    pending: Option<Box<PendingJournalPublicationV1<P>>>,
}

struct PendingJournalPublicationV1<P: SemanticProfile> {
    permit: OwnedRwLockWriteGuard<()>,
    captured: CapturedHotJournalV1,
    predecessor: CatalogSlotV1,
    expected: Option<CatalogSlotV1>,
    phase: PendingJournalPhaseV1<P>,
}

enum PendingJournalPhaseV1<P: SemanticProfile> {
    Captured,
    RecoveryRequired,
    AwaitingAck {
        successor: Arc<DurableAuthorityCoreV1<P>>,
        generation_lease: Option<GenerationLeaseV1>,
    },
}

impl<P: SemanticProfile> DurableRSpaceAuthorityV1<P>
where P::Channel: Clone + Hash + Eq
{
    pub(crate) fn from_view(
        view: DurableAuthorityViewV1<P>,
        hot: Arc<Box<dyn HotStore<P::Channel, P::Pattern, P::Datum, P::Continuation>>>,
        operation_gate: Arc<RwLock<()>>,
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, DurableRSpaceTransactionError)>> {
        Self::from_view_with(view, hot, operation_gate, &OWNER_INCARNATIONS)
    }

    pub(crate) fn from_view_with(
        view: DurableAuthorityViewV1<P>,
        hot: Arc<Box<dyn HotStore<P::Channel, P::Pattern, P::Datum, P::Continuation>>>,
        operation_gate: Arc<RwLock<()>>,
        incarnations: &OwnerIncarnationAllocatorV1,
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
        Ok(Self {
            lifecycle: OwnerLifecycleV1::Live {
                incarnation,
                lease,
                generation_lease: _generation_lease,
            },
            registry,
            source: source.into_core(),
            hot,
            operation_gate,
            pending: None,
        })
    }

    pub(crate) fn phase(&self) -> JournalPublicationPhaseV1 {
        match (&self.lifecycle, self.pending.as_ref().map(|pending| &pending.phase)) {
            (OwnerLifecycleV1::Closed { .. }, _) => JournalPublicationPhaseV1::Closed,
            (_, None) if self.validate_base().is_ok() => JournalPublicationPhaseV1::Ready,
            (_, None) => JournalPublicationPhaseV1::Invalidated,
            (_, Some(PendingJournalPhaseV1::Captured)) => JournalPublicationPhaseV1::Captured,
            (_, Some(PendingJournalPhaseV1::RecoveryRequired)) => {
                JournalPublicationPhaseV1::RecoveryRequired
            }
            (_, Some(PendingJournalPhaseV1::AwaitingAck { .. })) => {
                JournalPublicationPhaseV1::AwaitingAck
            }
        }
    }

    pub(crate) fn state(&self) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live { .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        if self.pending.is_some() {
            return Err(DurableRSpaceTransactionError::Unresolved(self.phase()));
        }
        self.validate_base()?;
        Ok(self.source.authority().slot().state())
    }

    fn validate_base(&self) -> Result<(), RSpaceError> {
        self.hot
            .validate_journal_authority(self.source.authority().slot(), &self.registry)
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

    pub(crate) fn attach(&self) -> Result<ViewAttachmentV1, DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live { incarnation, .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        self.state()?;
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
        if self.pending.is_some() {
            return Err(DurableRSpaceTransactionError::Unresolved(self.phase()));
        }
        self.lifecycle = OwnerLifecycleV1::Closed { generation };
        Ok(())
    }

    pub(crate) async fn checkpoint(
        &mut self,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let OwnerLifecycleV1::Live { .. } = self.lifecycle else {
            return Err(DurableRSpaceTransactionError::Closed);
        };
        if self.pending.is_some() {
            let old = self.source.authority().slot().state();
            let recovered = self.reconcile(limits)?;
            if recovered != old {
                return Ok(recovered);
            }
        }
        let permit = Arc::clone(&self.operation_gate).write_owned().await;
        self.validate_base()?;
        let predecessor = self.source.authority().slot();
        let captured = self.hot.capture_pending_journal(predecessor)?;
        self.pending = Some(Box::new(PendingJournalPublicationV1 {
            permit,
            captured,
            predecessor,
            expected: None,
            phase: PendingJournalPhaseV1::Captured,
        }));
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterCapture);
        let publication = self.publish_captured(limits);
        if publication.is_err() && self.phase() == JournalPublicationPhaseV1::Captured {
            self.validate_base()?;
            self.pending = None;
        }
        publication?;
        self.acknowledge_successor()
    }

    fn publish_captured(
        &mut self,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<(), DurableRSpaceTransactionError> {
        let pending = self.pending.as_mut().expect("checkpoint owns its capture");
        self.hot.validate_pending_journal(&pending.captured)?;
        let observation = pending.captured.observation();
        let mutation =
            match mutation_batch_from_observation(pending.predecessor.state(), observation) {
                Ok(mutation) => mutation,
                Err(MutationError::EmptyBatch) => {
                    self.validate_base()?;
                    self.pending = None;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            };
        let plan = mutation.plan()?;
        let bytes = plan.try_canonical_bytes()?;
        let view = MutationBatchViewV1::parse(&bytes)?;
        let mut transaction =
            OpenDagTransaction::new(self.source.verified_state(), view, limits.transaction)?;
        stage_pending_continuation_sources(&mut transaction, &observation.pending)?;
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
        self.hot.validate_pending_journal(&pending.captured)?;
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
        Ok(())
    }

    pub(crate) fn reconcile(
        &mut self,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        if self.pending.is_none() {
            return self.state();
        }
        if self.phase() == JournalPublicationPhaseV1::AwaitingAck {
            return self.acknowledge_successor();
        }
        self.validate_base()?;
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
            self.hot
                .validate_journal_authority(actual, &self.registry)?;
            self.source = successor;
            self.pending = None;
            return self.state();
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
        self.acknowledge_successor()
    }

    fn acknowledge_successor(&mut self) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let Some(pending) = self.pending.as_mut() else {
            return self.state();
        };
        let PendingJournalPhaseV1::AwaitingAck {
            successor,
            generation_lease,
        } = &mut pending.phase
        else {
            return Err(DurableRSpaceTransactionError::Unresolved(self.phase()));
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
        self.hot.acknowledge_pending_journal(
            &pending.captured,
            pending
                .expected
                .expect("an admitted successor has its exact terminal"),
            successor.authority().slot(),
        )?;
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterAcknowledgement);
        self.hot.validate_journal_acknowledgement(
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
        self.source = successor;
        *current_pin = generation_lease.expect("successor pin precedes acknowledgement");
        drop(permit);
        self.state()
    }
}
