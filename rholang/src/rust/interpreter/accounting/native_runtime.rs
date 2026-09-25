use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::operation_context;

use super::byte_accounting::{ByteCharge, BYTE_COST_SCHEDULE_V1};
use super::byte_receipts::ByteObservation;
use super::native_phlo_rules::{
    NativeBudgetOccurrence, NativeBudgetTraceLimits, NativePhloExecutionContract,
    NativePhloExecutionError, NativePhloReservation, PreparedNativePhloCharge,
};
use super::{AuthorityRuntimeState, BillableKind, RuntimeBudget, Token};
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::host_work::HostWorkBudget;

mod recording;
mod operation_sources;
mod operations;
pub(super) mod index;
mod checked_operations;
mod replay_authority;
pub(crate) mod clone_backing;
pub use checked_operations::{
    CheckedNativeOperationJournal, CheckedNativeOperationTrace, NativeOperationJournalError,
    NativeOperationJournalLimits, NativeOperationReplay, NativeOperationTraceError,
    NativeOperationTraceLimits, NativeReplayAccountingSnapshot, NativeReplayBoundary,
    NativeReplayCheckpoint, NativeReplayError, NativeReplayOutcome, NativeReplayPublication,
    NativeReplayReservation, NativeReplayRestore, NativeRuntimeReplayCheckpoint,
    NativeRuntimeReplaySession,
};
pub use operation_sources::{
    NativeCommSource, NativeConsumeSource, NativeOperationSource, NativeProduceSource,
};
pub use operations::{
    NativeCommRecord, NativeObservationLink, NativeOperationOccurrence, NativeOperationRecord,
};
use recording::{recording_error, work, NativeBudgetRecorder, NativeObservationPreparation};
pub use recording::{NativeBudgetRecording, NativeBudgetRetry};
pub(crate) use replay_authority::NativeAuthorityCheckpoint;

pub struct NativeRuntimeConfig {
    replay_bound: bool,
    reservation: NativePhloReservation,
    limits: NativeBudgetTraceLimits,
    host_work: HostWorkBudget,
    session: [u8; 32],
    generation: Arc<()>,
    recording: NativeBudgetRecorder,
    operations: operations::NativeOperationRecorder,
}

impl NativeRuntimeConfig {
    pub(crate) fn host_work(&self) -> HostWorkBudget { self.host_work.clone() }

    pub fn new(
        contract: NativePhloExecutionContract<'_>,
        limits: NativeBudgetTraceLimits,
        host_work: HostWorkBudget,
    ) -> Self {
        Self {
            replay_bound: false,
            reservation: contract.reservation(),
            limits,
            host_work,
            session: [0; 32],
            generation: Arc::new(()),
            recording: NativeBudgetRecorder::default(),
            operations: operations::NativeOperationRecorder::default(),
        }
    }
}

impl RuntimeBudget {
    pub fn reset_for_native_execution(
        &self,
        mut config: NativeRuntimeConfig,
    ) -> Result<(), InterpreterError> {
        if self.has_comm_accounting_scope() || self.is_unmetered() {
            return Err(InterpreterError::ReduceError(
                "native execution requires an idle metered budget".to_owned(),
            ));
        }
        if config.host_work.is_rejected() {
            return Err(InterpreterError::HostWorkRejected);
        }
        self.reset_from_token(&Token::coalesced(self.signature(), 0));
        config.session = self.deploy_id();
        self.authority_state.lock().expect("authority state").native = Some(config);
        Ok(())
    }

    pub fn native_phlo_usage(&self) -> Option<u64> {
        self.authority_state
            .lock()
            .expect("authority state")
            .native
            .as_ref()
            .filter(|native| !native.replay_bound)
            .map(|native| native.reservation.used())
    }

    pub fn native_budget_recording(
        &self,
    ) -> Result<Option<NativeBudgetRecording>, InterpreterError> {
        let state = self.authority_state.lock().expect("authority state");
        if state.native.is_some() && !state.byte_observations.has_complete_history() {
            return Err(recording_error(
                "native execution lost accepted measurement history",
            ));
        }
        state
            .native
            .as_ref()
            .map(NativeRuntimeConfig::capture_recording)
            .transpose()
    }

    pub(super) fn measured_legacy_cost(
        &self,
        measurement: ByteCharge,
    ) -> Result<u64, InterpreterError> {
        if self
            .authority_state
            .lock()
            .expect("authority state")
            .native
            .is_some()
        {
            return Ok(0);
        }
        measurement
            .cost(BYTE_COST_SCHEDULE_V1)
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))
    }

    pub(super) fn prepare_native_observation(
        &self,
        observation: &Arc<ByteObservation>,
    ) -> Result<Option<NativeObservationPreparation>, InterpreterError> {
        let captured = {
            let state = self.authority_state.lock().expect("authority state");
            state
                .native
                .as_ref()
                .map(|native| {
                    Ok::<_, InterpreterError>((
                        native.reservation.preparer(),
                        native.limits,
                        native.host_work.clone(),
                        Arc::clone(&native.generation),
                        native.session,
                        native.recording.prepare(native.limits.attempts)?,
                    ))
                })
                .transpose()?
        };
        let Some((preparer, limits, host_work, generation, session, ticket)) = captured else {
            return Ok(None);
        };
        let prepare = || {
            let order = operation_context::current()
                .ok_or_else(|| recording_error("native charge has no operation context"))?;
            if order.session != session || order.path.len() > limits.path_segments {
                return Err(recording_error(
                    "native charge operation session or path is invalid",
                ));
            }
            work(
                &host_work,
                HostWorkDimension::VerificationOperations,
                order.path.len().saturating_add(34),
            )?;
            work(
                &host_work,
                HostWorkDimension::SearchStateBytes,
                order
                    .path
                    .len()
                    .saturating_mul(std::mem::size_of::<(u64, u64)>()),
            )?;
            let occurrence = NativeBudgetOccurrence {
                session,
                path: order.path.to_vec(),
                stage: observation.kind.into(),
            };
            let comparison_bytes =
                super::native_phlo_rules::observation_comparison_bytes(observation, &host_work)
                    .map_err(|_| InterpreterError::HostWorkRejected)?;
            let charge = match preparer.prepare(Arc::clone(observation), limits.regions, &host_work)
            {
                Ok(charge) => Some(charge),
                Err(NativePhloExecutionError::Overflow) if !host_work.is_rejected() => None,
                Err(error) => {
                    return Err(if host_work.is_rejected() {
                        InterpreterError::HostWorkRejected
                    } else {
                        native_error(error)
                    })
                }
            };
            Ok(Some(NativeObservationPreparation {
                generation: Arc::clone(&generation),
                occurrence,
                observation: Arc::clone(observation),
                charge,
                comparison_bytes,
                ticket,
            }))
        };
        prepare()
    }

    pub(super) fn reserve_observation_usage(
        &self,
        state: &mut AuthorityRuntimeState,
        prepared: Option<&NativeObservationPreparation>,
        identity: [u8; 32],
        kind: BillableKind,
        legacy_weight: u64,
        description: &'static str,
    ) -> Result<(), InterpreterError> {
        match (state.native.as_mut(), prepared) {
            (Some(native), Some(prepared)) => native.record_charge(prepared),
            (None, None) => {
                self.reserve_consensus_identity(identity, kind, legacy_weight, description)
            }
            _ => Err(InterpreterError::ReduceError(
                "native measurement context changed before acceptance".to_owned(),
            )),
        }
    }

    pub(super) fn record_native_retry(
        &self,
        state: &mut AuthorityRuntimeState,
        prepared: Option<&NativeObservationPreparation>,
    ) -> Result<(), InterpreterError> {
        match (state.native.as_mut(), prepared) {
            (Some(native), Some(prepared)) => native.record_retry(prepared),
            (None, None) => Ok(()),
            _ => Err(recording_error(
                "native retry context changed before acceptance",
            )),
        }
    }
}

fn native_error(error: NativePhloExecutionError) -> InterpreterError {
    match error {
        NativePhloExecutionError::BoundExceeded | NativePhloExecutionError::Overflow => {
            InterpreterError::OutOfPhlogistonsError
        }
        other => InterpreterError::ReduceError(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
