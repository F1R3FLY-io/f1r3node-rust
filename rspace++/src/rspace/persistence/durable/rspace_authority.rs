use std::hash::Hash;
use std::sync::Arc;

use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

#[cfg(test)]
#[path = "../../../../tests/persistence/journal_owner_hooks.rs"]
pub(crate) mod hooks;

use super::admission::{
    AdmittedCheckpointSourceV1, DurableAdmissionErrorV1, DurableAdmissionLimitsV1,
    DurableTransactionPreparationError, admit_physical_source,
};
use super::io::DurableWriterLeaseV1;
use super::publication::{IncrementalPublicationErrorV1, publish_incremental_with_terminal_v1};
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
    LogPackDecodeLimits, LogPackFormatError, SemanticProfile, StateVersionV1,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DurableRSpacePublicationLimitsV1 {
    pub transaction: DagTransactionLimits,
    pub recovery: DurableLimits,
    pub wal: LogPackDecodeLimits,
    pub admission: DurableAdmissionLimitsV1,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JournalPublicationPhaseV1 {
    Ready,
    Invalidated,
    Captured,
    RecoveryRequired,
    AwaitingAck,
}

pub(crate) struct DurableRSpaceAuthorityV1<P: SemanticProfile>
where P::Channel: Clone + Hash + Eq
{
    lease: DurableWriterLeaseV1,
    source: AdmittedCheckpointSourceV1<P>,
    generation_lease: GenerationLeaseV1,
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
        successor: AdmittedCheckpointSourceV1<P>,
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
    ) -> Result<Self, Box<(DurableAuthorityViewV1<P>, RSpaceError)>> {
        if let Err(error) =
            hot.validate_journal_authority(view.catalog(), &view.generation_leases())
        {
            return Err(Box::new((view, error)));
        }
        let DurableAuthorityViewV1 {
            writer,
            _generation_lease,
        } = view;
        let (lease, source) = writer.into_parts();
        Ok(Self {
            lease,
            source,
            generation_lease: _generation_lease,
            hot,
            operation_gate,
            pending: None,
        })
    }

    pub(crate) fn phase(&self) -> JournalPublicationPhaseV1 {
        match self.pending.as_ref().map(|pending| &pending.phase) {
            None if self.validate_base().is_ok() => JournalPublicationPhaseV1::Ready,
            None => JournalPublicationPhaseV1::Invalidated,
            Some(PendingJournalPhaseV1::Captured) => JournalPublicationPhaseV1::Captured,
            Some(PendingJournalPhaseV1::RecoveryRequired) => {
                JournalPublicationPhaseV1::RecoveryRequired
            }
            Some(PendingJournalPhaseV1::AwaitingAck { .. }) => {
                JournalPublicationPhaseV1::AwaitingAck
            }
        }
    }

    pub(crate) fn state(&self) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        if self.pending.is_some() {
            return Err(DurableRSpaceTransactionError::Unresolved(self.phase()));
        }
        self.validate_base()?;
        Ok(self.source.authority().slot().state())
    }

    fn validate_base(&self) -> Result<(), RSpaceError> {
        self.hot.validate_journal_authority(
            self.source.authority().slot(),
            &self.lease.generation_leases(),
        )
    }

    pub(crate) fn pending_terminal(&self) -> Option<CatalogSlotV1> {
        self.pending.as_ref().and_then(|pending| pending.expected)
    }

    pub(crate) fn generation_leases(&self) -> GenerationLeaseRegistryV1 {
        self.lease.generation_leases()
    }

    pub(crate) fn generation_key(&self) -> GenerationLeaseKeyV1 { self.generation_lease.key() }

    pub(crate) async fn checkpoint(
        &mut self,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
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
        let mut transaction = self.source.open_transaction(view, limits.transaction)?;
        stage_pending_continuation_sources(&mut transaction, &observation.pending)?;
        let sealed = transaction.seal()?;
        let sealed = self
            .source
            .bind_durable_transaction(&self.lease, sealed, limits.admission)?;
        let sequence = pending.predecessor.commit_sequence().successor()?;
        let prepared =
            self.source
                .prepare_transaction(&self.lease, &sealed, sequence, limits.wal)?;
        #[cfg(test)]
        hooks::run(hooks::JournalOwnerHookPoint::AfterPreparation);
        self.hot.validate_pending_journal(&pending.captured)?;
        pending.phase = PendingJournalPhaseV1::RecoveryRequired;
        let successor = publish_incremental_with_terminal_v1(
            &mut self.lease,
            &self.source,
            &prepared,
            limits.recovery,
            limits.wal,
            limits.admission,
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
            successor,
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
        let physical = recover_physical_from_lease_v1(&mut self.lease, limits.recovery, limits.wal)
            .map_err(|error| {
                DurableRSpaceTransactionError::Recovery(DurableAdmissionErrorV1::Startup(error))
            })?;
        let successor = admit_physical_source::<P>(&self.lease, physical, limits.admission)
            .map_err(DurableRSpaceTransactionError::Recovery)?;
        let actual = successor.authority().slot();
        let pending = self
            .pending
            .as_mut()
            .expect("reconciliation retains its capture");
        if actual == pending.predecessor {
            self.hot
                .validate_journal_authority(actual, &self.lease.generation_leases())?;
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
                self.lease
                    .generation_leases()
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
            &self.lease.generation_leases(),
        )?;
        let PendingJournalPublicationV1 { permit, phase, .. } = *self.pending.take().unwrap();
        let PendingJournalPhaseV1::AwaitingAck {
            successor,
            generation_lease,
        } = phase
        else {
            unreachable!("acknowledgement preserves its admitted successor");
        };
        self.source = successor;
        self.generation_lease = generation_lease.expect("successor pin precedes acknowledgement");
        drop(permit);
        self.state()
    }
}
