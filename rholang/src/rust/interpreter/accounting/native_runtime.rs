use std::sync::Arc;

use rspace_plus_plus::rspace::operation_context;

use super::byte_accounting::{ByteCharge, BYTE_COST_SCHEDULE_V1};
use super::byte_receipts::ByteObservation;
// Changed by G3 (DR-135, S4): the meter keeps the owner ledger.
// use super::native_phlo_rules::{
//     NativeBoundSource, NativeBudgetTraceLimits, NativePhloExecutionContract,
//     NativePhloExecutionError, NativePhloReservation, PreparedNativePhloCharge,
// };
// Changed by G3 (DR-135, S6): the owner checks of replay prepare charges.
// use super::native_phlo_rules::{
//     NativeBoundSource, NativeBudgetTraceLimits, NativePhloExecutionContract,
//     NativePhloExecutionError, NativePhloReservation, OwnerReservation, PreparedNativePhloCharge,
// };
use super::native_phlo_rules::{
    NativeBoundSource, NativeBudgetTraceLimits, NativePhloChargePreparer,
    NativePhloExecutionContract, NativePhloExecutionError, NativePhloRegionLimits,
    NativePhloReservation, OwnerReservation, PreparedNativePhloCharge,
};
// Added by G3 (DR-135, S4): the owner charge of an observation.
// Changed by G3 (DR-135, S6): the charge of a COMM with its extras is shared.
// use super::owner_funding::{prepared_owner_charge, OwnerCommExtras};
use super::owner_funding::{
    prepared_owner_charge, OwnerCharge, OwnerCommExtras, OwnerFundingBinding, OwnerPurseSet,
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
// Changed by G3 (DR-135, S4): a journal row records its refused COMMs.
// pub use operations::{
//     NativeCommRecord, NativeObservationLink, NativeOperationOccurrence, NativeOperationRecord,
// };
pub use operations::{
    NativeCommRecord, NativeObservationLink, NativeOperationOccurrence, NativeOperationRecord,
    NativeRefusal,
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
    /// Added by G3 (DR-135): the checked actual price of the contract. The
    /// signed consent of the deployment already checked it against every
    /// required owner ceiling (R4). Each region that the run creates carries
    /// it.
    price: u64,
    /// Added by G3 (DR-135): the owner funding of the run (R11). None keeps
    /// the behavior of dev.
    owner: Option<Arc<super::owner_funding::OwnerFundingBinding>>,
}

pub(in crate::rust::interpreter::accounting) type ReplayStackTransfers =
    Arc<[([u8; 32], models::rhoapi::CostAuthority, u64, bool)]>;

impl NativeRuntimeConfig {
    pub fn new(contract: NativePhloExecutionContract<'_>, limits: NativeBudgetTraceLimits) -> Self {
        // Added by G3 (DR-135): the price is read before `reservation`
        // consumes the contract.
        let price = contract.controls().schedule().actual_price;
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
            // Added by G3 (DR-135).
            price,
            // Added by G3 (DR-135).
            owner: None,
        }
    }

    /// Added by DR-114: the room for recorded external-service replies.
    pub fn with_record_room(mut self, room: u64) -> Self {
        self.record_room = room;
        self
    }

    /// Added by G3 (DR-135): the owner funding of the run (R11). A fired
    /// owner body then runs for its owner, and a system body for its caller.
    /// Without it the run keeps the behavior of dev.
    pub fn with_owner_funding(
        mut self,
        binding: Arc<super::owner_funding::OwnerFundingBinding>,
    ) -> Self {
        // Added by G3 (DR-135, S4): the meter keeps the owner ledger of the
        // binding's table (R3).
        self.reservation
            .set_owners(OwnerReservation::new(binding.table().clone()));
        self.owner = Some(binding);
        self
    }

    /// Added by G3 (DR-135, S4, SA5): whether the owner of the introduction
    /// with `identity` could not pay in this run.
    fn introduction_refused(&self, identity: &[u8; 32]) -> bool {
        self.reservation
            .owners()
            .is_some_and(|owners| owners.is_refused(identity))
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
        // Added by G3 (DR-135): the regions of the run carry the price of its
        // contract (R4). The reset above cleared the price of an earlier run.
        self.region_price
            .store(config.price, std::sync::atomic::Ordering::Release);
        // Added by G3 (DR-135): the fired bodies of the run load the binding
        // (R11). The reset above cleared the binding of an earlier run.
        self.owner_binding.store(config.owner.clone());
        self.authority_state.lock().expect("authority state").native = Some(config);
        Ok(())
    }

    /// Added by G3 (DR-135, S4, SA5): whether the owner of the introduction
    /// with `identity` could not pay, so that its operation stored nothing.
    // Changed by G3 (DR-135, S6): replay records each skipped introduction
    // in its ledger when it publishes the row.
    // /// Play reads its owner ledger. Replay reads its journal (S6).
    /// Play records a refusal in its owner ledger when it decides it. Replay
    /// records it in the ledger of its configuration when it publishes the
    /// skipped introduction (`replay_authority.rs`).
    pub(crate) fn introduction_refused(&self, identity: &[u8; 32]) -> bool {
        self.authority_state
            .lock()
            .expect("authority state")
            .native
            .as_ref()
            .is_some_and(|native| native.introduction_refused(identity))
    }

    /// Added by G3 (DR-135, S6): the preparer and the region limits of the
    /// native meter. The owner checks of replay measure the items and the
    /// refused COMMs of a row with them, as the meter of play does.
    pub(super) fn native_owner_meter(
        &self,
    ) -> Option<(NativePhloChargePreparer, NativePhloRegionLimits)> {
        self.authority_state
            .lock()
            .expect("authority state")
            .native
            .as_ref()
            .map(|native| (native.reservation.preparer(), native.limits.regions))
    }

    /// Added by G3 (DR-135, S4): the owner ledger of the running native
    /// execution.
    #[cfg(test)]
    pub(crate) fn native_owner_ledger(&self) -> Option<OwnerReservation> {
        self.authority_state
            .lock()
            .expect("authority state")
            .native
            .as_ref()
            .and_then(|native| native.reservation.owners().cloned())
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
        // Changed by G3 (DR-135, S4): the preparation also computes the
        // owner charge. An introduction has no COMM extras.
        self.prepare_native_observation_with(observation, None)
    }

    /// Added by G3 (DR-135, S4): `prepare_native_observation` with the
    /// owner terms of a COMM (A5, R9). The binding is captured with the
    /// preparer, and the owner charge is computed outside the mutex.
    pub(super) fn prepare_native_observation_with(
        &self,
        observation: &Arc<ByteObservation>,
        extras: Option<OwnerCommExtras>,
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
                        // Added by G3 (DR-135, S4): the binding of the run.
                        native.owner.clone(),
                    ))
                })
                .transpose()?
        };
        // Changed by G3 (DR-135, S4): the capture also takes the binding.
        // let Some((preparer, limits, generation, session, ticket)) = captured else {
        let Some((preparer, limits, generation, session, ticket, binding)) = captured else {
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
            // Added by G3 (DR-135, S4): the owner charge (R4). An overflowing
            // charge has none, because the bound denies it.
            // Changed by G3 (DR-135, S4 review): the restorations of a peek
            // COMM are kept for its attempt, also when its charge overflows,
            // so the fold charges them at the COMM, as play does (R9). Only a
            // run with owner funding has COMM extras.
            // let owner = match (binding.as_deref(), charge.as_ref()) {
            //     (Some(binding), Some(prepared)) => {
            //         let mut owner = prepared_owner_charge(binding, observation, Some(prepared))
            //             .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
            //         if let Some(extras) = extras {
            //             owner.transferee = extras.transferee;
            //             owner.restorations.reserve_exact(extras.restorations.len());
            //             for restoration in extras.restorations {
            //                 let prepared =
            //                     match preparer.prepare(Arc::clone(&restoration), limits.regions) {
            //                         Ok(prepared) => Some(prepared),
            //                         Err(NativePhloExecutionError::Overflow) => None,
            //                         Err(error) => {
            //                             return Err(native_error(error, preparer.bound_source()))
            //                         }
            //                     };
            //                 let charge =
            //                     prepared_owner_charge(binding, &restoration, prepared.as_ref())
            //                         .map_err(|error| {
            //                             InterpreterError::ReduceError(error.to_string())
            //                         })?;
            //                 owner.restorations.push((restoration.event_id, charge));
            //             }
            //         }
            //         Some(owner)
            //     }
            //     _ => None,
            // };
            let (transferee, restorations): (_, Arc<[Arc<ByteObservation>]>) =
                match (binding.as_deref(), extras) {
                    (Some(_), Some(extras)) => (extras.transferee, extras.restorations.into()),
                    _ => (None, Arc::from([])),
                };
            // Changed by G3 (DR-135, S6): the refusal check of replay charges a
            // COMM in the same way (`owner_charge_with_extras`).
            // let owner = match (binding.as_deref(), charge.as_ref()) {
            //     (Some(binding), Some(prepared)) => {
            //         let mut owner = prepared_owner_charge(binding, observation, Some(prepared))
            //             .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
            //         owner.transferee = transferee;
            //         owner.restorations.reserve_exact(restorations.len());
            //         for restoration in restorations.iter() {
            //             let prepared = match preparer
            //                 .prepare(Arc::clone(restoration), limits.regions)
            //             {
            //                 Ok(prepared) => Some(prepared),
            //                 Err(NativePhloExecutionError::Overflow) => None,
            //                 Err(error) => return Err(native_error(error, preparer.bound_source())),
            //             };
            //             let charge = prepared_owner_charge(binding, restoration, prepared.as_ref())
            //                 .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
            //             owner.restorations.push((restoration.event_id, charge));
            //         }
            //         Some(owner)
            //     }
            //     _ => None,
            // };
            let owner = match (binding.as_deref(), charge.as_ref()) {
                (Some(binding), Some(prepared)) => Some(owner_charge_with_extras(
                    binding,
                    &preparer,
                    limits.regions,
                    observation,
                    prepared,
                    transferee,
                    &restorations,
                )?),
                _ => None,
            };
            Ok(Some(NativeObservationPreparation {
                generation: Arc::clone(&generation),
                occurrence,
                occurrence_key,
                observation: Arc::clone(observation),
                charge,
                // Added by G3 (DR-135, S4).
                owner,
                // Added by G3 (DR-135, S4 review).
                restorations,
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

/// Added by G3 (DR-135, S6): the owner charge of the prepared observation
/// `prepared` of a COMM, with its A5 transferee and the charges of its R9
/// restorations, each prepared by `preparer` (an overflowing restoration has
/// no share). The recorder of play and the refusal check of replay share it,
/// so both roles charge a COMM in the same way.
pub(super) fn owner_charge_with_extras(
    binding: &OwnerFundingBinding,
    preparer: &NativePhloChargePreparer,
    limits: NativePhloRegionLimits,
    observation: &ByteObservation,
    prepared: &PreparedNativePhloCharge,
    transferee: Option<Option<OwnerPurseSet>>,
    restorations: &[Arc<ByteObservation>],
) -> Result<OwnerCharge, InterpreterError> {
    let mut owner = prepared_owner_charge(binding, observation, Some(prepared))
        .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
    owner.transferee = transferee;
    owner.restorations.reserve_exact(restorations.len());
    for restoration in restorations {
        let prepared = match preparer.prepare(Arc::clone(restoration), limits) {
            Ok(prepared) => Some(prepared),
            Err(NativePhloExecutionError::Overflow) => None,
            Err(error) => return Err(native_error(error, preparer.bound_source())),
        };
        let charge = prepared_owner_charge(binding, restoration, prepared.as_ref())
            .map_err(|error| InterpreterError::ReduceError(error.to_string()))?;
        owner.restorations.push((restoration.event_id, charge));
    }
    Ok(owner)
}

#[cfg(test)]
mod tests;
