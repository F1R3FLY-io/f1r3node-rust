//! Owning durable authority for the PathMap-backed RSpace journal.
//!
//! This is deliberately separate from the read-only recovery façade.  The
//! authority owns the admitted writer and its generation lease, and replaces
//! both only after the WAL publication has returned a freshly re-admitted
//! successor.  Consequently a failed publication leaves the prior authority
//! usable and no caller can accidentally continue from an unauthenticated
//! pathname or a stale catalog image.

use super::admission::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableTransactionPreparationError,
    SemanticallyAdmittedDurableWriterV1,
};
use super::publication::IncrementalPublicationErrorV1;
use super::{DurableAuthorityViewV1, DurableObjectReadErrorV1, recover_read_authority};
use crate::rspace::hot_store::{PendingDeltas, stage_pending_continuation_sources};
use crate::rspace::persistence::mutation::{MutationError, MutationBatchViewV1};
use crate::rspace::persistence::{
    CanonicalEncode, DagTransactionError, DagTransactionLimits, GenerationLeaseError,
    GenerationLeaseKeyV1, GenerationLeaseV1, LogPackDecodeLimits, LogPackFormatError, StateVersionV1,
};
use crate::rspace::persistence::{DurableLimits, SemanticProfile};
use std::path::PathBuf;

/// Limits and recovery policy used by one durable RSpace publication.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DurableRSpacePublicationLimitsV1 {
    pub transaction: DagTransactionLimits,
    pub recovery: DurableLimits,
    pub wal: LogPackDecodeLimits,
    pub admission: DurableAdmissionLimitsV1,
}

/// Errors retain the phase at which publication failed.  In particular, no
/// variant represents an empty or silently reset state.
#[derive(Debug)]
pub(crate) enum DurableRSpaceTransactionError {
    Mutation(MutationError),
    Canonical(crate::rspace::persistence::CanonicalFormatError),
    Transaction(DagTransactionError<DurableObjectReadErrorV1>),
    Preparation(DurableTransactionPreparationError),
    Publication(IncrementalPublicationErrorV1),
    Lease(GenerationLeaseError),
    Sequence(LogPackFormatError),
    RecoveryRequired(PathBuf),
    PublicationRecovery {
        publication: Box<Self>,
        recovery: DurableAdmissionErrorV1,
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

/// The sole owner of mutable durable RSpace publication authority.
pub(crate) struct DurableRSpaceAuthorityV1<P: SemanticProfile> {
    state: DurableRSpaceAuthorityState<P>,
}

enum DurableRSpaceAuthorityState<P: SemanticProfile> {
    Ready {
        writer: SemanticallyAdmittedDurableWriterV1<P>,
        generation_lease: GenerationLeaseV1,
    },
    /// The previous consuming publication API has consumed its writer and
    /// reported an error whose phase may be before or after catalog CAS.  The
    /// pathname is only a recovery input; it is never used as an authority
    /// without running the full authenticated restart admission again.
    RecoveryRequired { directory: PathBuf },
}

impl<P: SemanticProfile> DurableRSpaceAuthorityV1<P> {
    pub(crate) fn from_view(view: DurableAuthorityViewV1<P>) -> Self {
        let DurableAuthorityViewV1 { writer, _generation_lease } = view;
        Self {
            state: DurableRSpaceAuthorityState::Ready {
                writer,
                generation_lease: _generation_lease,
            },
        }
    }

    pub(crate) fn state(&self) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        match &self.state {
            DurableRSpaceAuthorityState::Ready { writer, .. } => Ok(writer.catalog().state()),
            DurableRSpaceAuthorityState::RecoveryRequired { directory } => {
                Err(DurableRSpaceTransactionError::RecoveryRequired(directory.clone()))
            }
        }
    }

    fn recover_ready(
        &mut self,
        directory: PathBuf,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<(), DurableAdmissionErrorV1> {
        let view = recover_read_authority::<P>(
            directory,
            limits.recovery,
            limits.wal,
            limits.admission,
        )?;
        let DurableAuthorityViewV1 { writer, _generation_lease } = view;
        self.state = DurableRSpaceAuthorityState::Ready {
            writer,
            generation_lease: _generation_lease,
        };
        Ok(())
    }

    /// Publish one exact mutation and return the newly authenticated state.
    ///
    /// The live hot-store journal is intentionally not acknowledged here: the
    /// caller must invoke `commit_pending_deltas` only after this method has
    /// succeeded.  This preserves the publication-before-acknowledgement
    /// crash invariant.
    pub(crate) fn publish_mutation<'mutation>(
        &mut self,
        mutation: &'mutation crate::rspace::persistence::RSpaceMutationBatchV1,
        pending: &PendingDeltas,
        limits: DurableRSpacePublicationLimitsV1,
    ) -> Result<StateVersionV1, DurableRSpaceTransactionError> {
        let (directory, writer) = match &self.state {
            DurableRSpaceAuthorityState::Ready { writer, .. } => {
                (writer.directory_path(), writer)
            }
            DurableRSpaceAuthorityState::RecoveryRequired { directory } => {
                return Err(DurableRSpaceTransactionError::RecoveryRequired(directory.clone()));
            }
        };
        let plan = mutation.plan()?;
        let mutation_bytes = plan.try_canonical_bytes()?;
        let view = MutationBatchViewV1::parse(&mutation_bytes)?;
        let sequence = writer.authority().slot().commit_sequence().successor()?;
        let mut transaction = writer.open_transaction(view, limits.transaction)?;
        stage_pending_continuation_sources(&mut transaction, pending)?;
        let sealed = transaction.seal()?;
        let sealed = writer.attach_target_retention_manifest(sealed)?;
        let prepared = writer.prepare_transaction(&sealed, sequence, limits.wal)?;
        let prior = std::mem::replace(
            &mut self.state,
            DurableRSpaceAuthorityState::RecoveryRequired {
                directory: directory.clone(),
            },
        );
        let DurableRSpaceAuthorityState::Ready {
            writer,
            generation_lease,
        } = prior
        else {
            unreachable!("authority state was checked ready above")
        };
        drop(generation_lease);
        let successor = match writer.publish_incremental_transaction_v1(
            &prepared,
            limits.recovery,
            limits.wal,
            limits.admission,
        ) {
            Ok(successor) => successor,
            Err(error) => {
                let original = DurableRSpaceTransactionError::Publication(error);
                return match self.recover_ready(directory, limits) {
                    Ok(()) => Err(original),
                    Err(recovery) => Err(DurableRSpaceTransactionError::PublicationRecovery {
                        publication: Box::new(original),
                        recovery,
                    }),
                };
            }
        };
        let successor_lease = successor
            .generation_leases()
            .acquire(GenerationLeaseKeyV1::from_catalog(successor.catalog()));
        let successor_lease = match successor_lease {
            Ok(lease) => lease,
            Err(error) => {
                let original = DurableRSpaceTransactionError::Lease(error);
                return match self.recover_ready(directory, limits) {
                    Ok(()) => Err(original),
                    Err(recovery) => Err(DurableRSpaceTransactionError::PublicationRecovery {
                        publication: Box::new(original),
                        recovery,
                    }),
                };
            }
        };
        self.state = DurableRSpaceAuthorityState::Ready {
            writer: successor,
            generation_lease: successor_lease,
        };
        Ok(self.state()?)
    }
}
