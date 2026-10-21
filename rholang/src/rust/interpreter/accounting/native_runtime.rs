use std::sync::Arc;

use rspace_plus_plus::rspace::operation_context;

use super::byte_accounting::{ByteCharge, BYTE_COST_SCHEDULE_V1};
use super::byte_receipts::ByteObservation;
use super::native_phlo_rules::{
    NativeBoundSource, NativeBudgetTraceLimits, NativePhloExecutionContract,
    NativePhloExecutionError, NativePhloReservation, PreparedNativePhloCharge,
};
use super::{AuthorityRuntimeState, BillableKind, RuntimeBudget, Token};
use crate::rust::interpreter::errors::InterpreterError;

mod recording;
mod operation_sources;
mod operations;
pub(super) mod index;
mod path_trie;
mod checked_operations;
mod replay_authority;
mod wire;
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
pub use path_trie::{NativePathTrie, PathId};
use recording::{recording_error, NativeBudgetRecorder, NativeObservationPreparation};
pub use recording::{NativeBudgetRecording, NativeBudgetRetry};
pub(crate) use replay_authority::NativeAuthorityCheckpoint;
pub use wire::{
    decode_native_budget_recording, decode_native_operation_journal,
    encode_native_budget_recording, encode_native_operation_journal, NativeRecordingWireLimits,
};

pub struct NativeRuntimeConfig {
    replay_bound: bool,
    reservation: NativePhloReservation,
    limits: NativeBudgetTraceLimits,
    session: [u8; 32],
    generation: Arc<()>,
    recording: NativeBudgetRecorder,
    operations: operations::NativeOperationRecorder,
    /// Added by DR-114: the bytes of external-service replies that play may
    /// record. The default is 0, so a configuration without a room records
    /// no successful reply (fail closed).
    record_room: u64,
    /// Added by DR-114: the reply bytes recorded so far.
    record_used: u64,
    replay_transfers: ReplayStackTransfers,
}

pub(in crate::rust::interpreter::accounting) type ReplayStackTransfers =
    Arc<[([u8; 32], models::rhoapi::CostAuthority, u64, bool)]>;

impl NativeRuntimeConfig {
    pub fn new(contract: NativePhloExecutionContract<'_>, limits: NativeBudgetTraceLimits) -> Self {
        Self {
            replay_bound: false,
            reservation: contract.reservation(),
            limits,
            session: [0; 32],
            generation: Arc::new(()),
            recording: NativeBudgetRecorder::default(),
            operations: operations::NativeOperationRecorder::default(),
            record_room: 0,
            record_used: 0,
            replay_transfers: Arc::from([]),
        }
    }

    /// Added by DR-114: the room for recorded external-service replies.
    pub fn with_record_room(mut self, room: u64) -> Self {
        self.record_room = room;
        self
    }
}

impl RuntimeBudget {
    pub fn native_execution_active(&self) -> bool {
        self.authority_state
            .lock()
            .expect("authority state")
            .native
            .is_some()
    }

    /// Added by DR-114: takes `bytes` of the record room for one recorded
    /// reply in play. Replay and an exhausted room take nothing.
    pub(crate) fn take_native_record_room(&self, bytes: u64) -> bool {
        let mut state = self.authority_state.lock().expect("authority state");
        let Some(native) = state.native.as_mut().filter(|native| !native.replay_bound) else {
            return false;
        };
        match native.record_used.checked_add(bytes) {
            Some(used) if used <= native.record_room => {
                native.record_used = used;
                true
            }
            _ => false,
        }
    }

    pub fn reset_for_native_execution(
        &self,
        mut config: NativeRuntimeConfig,
    ) -> Result<(), InterpreterError> {
        if self.has_comm_accounting_scope() || self.is_unmetered() {
            return Err(InterpreterError::ReduceError(
                "native execution requires an idle metered budget".to_owned(),
            ));
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
                        Arc::clone(&native.generation),
                        native.session,
                        native.recording.prepare(native.limits.attempts)?,
                    ))
                })
                .transpose()?
        };
        let Some((preparer, limits, generation, session, ticket)) = captured else {
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
            // Changed by C7b (DR-86): the producer records a copied path; the
            // captured evidence names its trie node.
            // let occurrence = NativeBudgetOccurrence {
            let occurrence = recording::RecordedOccurrence {
                session,
                path: order.path.to_vec(),
                stage: observation.kind.into(),
            };
            let occurrence_key = index::OccurrenceKey {
                session,
                digest: order.path.digest(),
                depth: order.path.len(),
                last: order.path.last_segment(),
                stage: observation.kind.into(),
            };
            let charge = match preparer.prepare(Arc::clone(observation), limits.regions) {
                Ok(charge) => Some(charge),
                Err(NativePhloExecutionError::Overflow) => None,
                Err(error) => return Err(native_error(error, preparer.bound_source())),
            };
            Ok(Some(NativeObservationPreparation {
                generation: Arc::clone(&generation),
                occurrence,
                occurrence_key,
                observation: Arc::clone(observation),
                charge,
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

    pub(super) fn record_native_allocation_denial(
        &self,
        state: &mut AuthorityRuntimeState,
        prepared: Option<&NativeObservationPreparation>,
    ) -> Result<(), InterpreterError> {
        match (state.native.as_mut(), prepared) {
            (Some(native), Some(prepared)) => native.record_allocation_denial(prepared),
            (None, None) => Ok(()),
            _ => Err(recording_error(
                "native allocation context changed before acceptance",
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

// Changed by DR-113: exhaustion takes the class of the bound source.
// fn native_error(error: NativePhloExecutionError) -> InterpreterError {
//     match error {
//         NativePhloExecutionError::BoundExceeded | NativePhloExecutionError::Overflow => {
//             InterpreterError::OutOfPhlogistonsError
//         }
//         other => InterpreterError::ReduceError(other.to_string()),
//     }
// }
fn native_error(error: NativePhloExecutionError, source: NativeBoundSource) -> InterpreterError {
    match error {
        NativePhloExecutionError::BoundExceeded | NativePhloExecutionError::Overflow => {
            exhaustion_error(source)
        }
        other => InterpreterError::ReduceError(other.to_string()),
    }
}

/// Added by DR-113: the error of an exhausted bound. A signed limit gives a
/// classified user failure, and a certified bound gives a certificate failure.
pub(crate) fn exhaustion_error(source: NativeBoundSource) -> InterpreterError {
    match source {
        NativeBoundSource::SignedLimit => InterpreterError::SignedLimitExhausted,
        NativeBoundSource::Certificate => InterpreterError::OutOfPhlogistonsError,
    }
}

#[cfg(test)]
mod tests;
