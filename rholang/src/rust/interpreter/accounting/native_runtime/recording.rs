use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

// Added by G3 (DR-135, S4): the owner part of an attempt.
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::index::{NativeIndex, OccurrenceKey, PreparedInsert};
use super::*;
// Added by G3 (DR-135, S4).
// Changed by G3 (DR-135, G-4): the owner part keeps the draws of each part.
// use crate::rust::interpreter::accounting::authority::ResourceMultiset;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
};
// Added by G3 (DR-135, S4).
// Changed by G3 (DR-135, G-4): the owner part keeps the draws of each part.
// use crate::rust::interpreter::accounting::owner_funding::{
//     OwnerCharge, OwnerShortfall, OwnerTableEntry, UnlistedRule,
// };
use crate::rust::interpreter::accounting::owner_funding::{
    OwnerCharge, OwnerDraws, OwnerShortfall, OwnerTableEntry, UnlistedRule,
};

#[derive(Clone, Debug)]
pub struct NativeBudgetRetry {
    pub occurrence: NativeBudgetOccurrence,
    pub observation: Arc<ByteObservation>,
    pub accepted_attempt: usize,
    pub fresh_before: usize,
}

#[derive(Clone, Debug)]
pub struct NativeBudgetRecording {
    pub session: [u8; 32],
    pub attempts: Arc<[NativeBudgetAttempt]>,
    pub retries: Arc<[NativeBudgetRetry]>,
    pub used: u64,
    /// The causal paths of the attempts, the retries and the operation
    /// journal rows, interned once each (C7b, DR-86). Occurrences and journal
    /// rows name their nodes.
    pub paths: Arc<NativePathTrie>,
    /// Added by G3 (DR-135, S4): the owner table T of the run: each listed
    /// purse, by the signature that locates it, with its budget, in
    /// ascending custody order (R3). Empty without owner funding.
    pub owner_table: Arc<[OwnerTableEntry]>,
}

/// An occurrence as the producer records it: the session, a copy of the
/// causal path and the stage. Captured evidence names the path by its trie
/// node instead (C7b, DR-86).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct RecordedOccurrence {
    pub(super) session: [u8; 32],
    pub(super) path: Vec<(u64, u64)>,
    pub(super) stage: NativeAttemptStage,
}

#[derive(Clone, Debug)]
pub(super) struct RecordedAttempt {
    pub(super) occurrence: RecordedOccurrence,
    pub(super) observation: Arc<ByteObservation>,
    pub(super) granted: bool,
    /// Added by G3 (DR-135, S4): the owner of the introduction could not pay
    /// (R8).
    pub(super) owner_refused: bool,
    /// Added by G3 (DR-135, S4 review): the restorations that a peek COMM
    /// paid for (R9).
    pub(super) restorations: Arc<[Arc<ByteObservation>]>,
}

#[derive(Clone, Debug)]
pub(super) struct RecordedRetry {
    pub(super) occurrence: RecordedOccurrence,
    pub(super) observation: Arc<ByteObservation>,
    pub(super) accepted_attempt: usize,
    pub(super) fresh_before: usize,
}

#[derive(Default)]
pub(super) struct NativeBudgetRecorder {
    // Changed by C7b (DR-86): the recorder keeps copied paths; the captured
    // evidence names trie nodes.
    // pub(super) attempts: Vec<NativeBudgetAttempt>,
    // pub(super) retries: Vec<NativeBudgetRetry>,
    pub(super) attempts: Vec<RecordedAttempt>,
    pub(super) retries: Vec<RecordedRetry>,
    // Changed by C7a (DR-85): the index is keyed by the chained path digest,
    // so a comparison no longer scans the path.
    // occurrences: NativeIndex<NativeBudgetOccurrence, ()>,
    occurrences: NativeIndex<OccurrenceKey, ()>,
    accepted: NativeIndex<(NativeAttemptStage, [u8; 32]), usize>,
    pub(super) invalid: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

pub(in crate::rust::interpreter::accounting) struct NativeObservationPreparation {
    pub(super) generation: Arc<()>,
    pub(super) occurrence: RecordedOccurrence,
    pub(super) occurrence_key: OccurrenceKey,
    pub(super) observation: Arc<ByteObservation>,
    pub(super) charge: Option<PreparedNativePhloCharge>,
    /// Added by G3 (DR-135, S4): the owner charge of the observation, when
    /// the run has owner funding and the meter charge did not overflow (R4).
    pub(super) owner: Option<OwnerCharge>,
    /// Added by G3 (DR-135, S4 review): the introductions of the data that a
    /// peek COMM restores (R9). The COMM attempt records them, so the fold
    /// charges them at the COMM, as play does.
    pub(super) restorations: Arc<[Arc<ByteObservation>]>,
    pub(super) ticket: NativePreparationTicket,
}

pub(super) struct NativePreparationTicket {
    invalid: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
    published: Cell<bool>,
}

impl NativeBudgetRecorder {
    pub(super) fn prepare(
        &self,
        limit: usize,
    ) -> Result<NativePreparationTicket, InterpreterError> {
        if self
            .pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1).filter(|next| *next <= limit)
            })
            .is_err()
        {
            self.invalid.store(true, Ordering::Release);
            return Err(recording_error("native budget preparation limit exceeded"));
        }
        Ok(NativePreparationTicket {
            invalid: Arc::clone(&self.invalid),
            pending: Arc::clone(&self.pending),
            published: Cell::new(false),
        })
    }
}

impl Drop for NativePreparationTicket {
    fn drop(&mut self) {
        if !self.published.get() {
            self.invalid.store(true, Ordering::Release);
        }
        self.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

impl NativePreparationTicket {
    fn reject_unpublished_failure(
        &self,
        result: Result<(), InterpreterError>,
    ) -> Result<(), InterpreterError> {
        if result.is_err() && !self.published.get() {
            self.invalid.store(true, Ordering::Release);
        }
        result
    }
}

pub(super) fn recording_error(message: &str) -> InterpreterError {
    InterpreterError::ReduceError(message.to_owned())
}

/// Added by G3 (DR-135, G-4): the owner part of an attempt, which play
/// decides before the bound (R4, R9).
enum OwnerPart {
    /// No owner funding, or no owner charge.
    None,
    /// The draws of the charge, by part. A grant draws the total, bills the
    /// main part and reserves the restorations.
    Draws(OwnerDraws),
    /// The introduction of a reserved restoration: it draws nothing, and a
    /// grant bills the reservation.
    Reserved,
}

impl NativeRuntimeConfig {
    fn reserve_record(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(PreparedInsert<OccurrenceKey, ()>, RecordedOccurrence), InterpreterError> {
        if self.replay_bound
            || !Arc::ptr_eq(&self.generation, &prepared.generation)
            || self.session != prepared.occurrence.session
        {
            return Err(recording_error(
                "native charge belongs to another execution generation",
            ));
        }
        if self.recording.invalid.load(Ordering::Acquire) {
            return Err(recording_error("native budget recording is incomplete"));
        }
        if self.recording.occurrences.len() >= self.limits.attempts {
            return Err(recording_error(
                "native budget recording exceeds the attempt limit",
            ));
        }
        // Changed by C7a (DR-85): the index key is a fixed-size copy, so only
        // the attempt's copy of the path remains.
        // work(
        //     &self.host_work,
        //     HostWorkDimension::VerificationOperations,
        //     prepared
        //         .occurrence
        //         .path
        //         .len()
        //         .checked_mul(2)
        //         .and_then(|count| count.checked_add(34))
        //         .ok_or(InterpreterError::HostWorkRejected)?,
        // )?;
        if self
            .recording
            .occurrences
            .get(&prepared.occurrence_key)
            .is_some()
        {
            return Err(recording_error(
                "native budget occurrence was already recorded",
            ));
        }
        // Changed by C7a (DR-85): the key no longer copies the path.
        // let bytes = prepared
        //     .occurrence
        //     .path
        //     .len()
        //     .checked_mul(size_of::<(u64, u64)>())
        //     .and_then(|bytes| bytes.checked_mul(2))
        //     .ok_or(InterpreterError::HostWorkRejected)?;
        // Changed by C7a (DR-85): the key is the fixed-size occurrence key.
        // let key = copy_occurrence(&prepared.occurrence)?;
        let key = prepared.occurrence_key;
        let insertion = self.recording.occurrences.prepare_insert(key, ())?;
        Ok((insertion, prepared.occurrence.clone()))
    }

    pub(super) fn record_charge(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        prepared
            .ticket
            .reject_unpublished_failure(self.try_record_charge(prepared))
    }

    fn try_record_charge(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        let (key, occurrence) = self.reserve_record(prepared)?;
        let slot = self.operation_stage_slot(&prepared.occurrence)?;
        let index = self.recording.attempts.len();
        let accepted = self.recording.accepted.prepare_insert(
            (prepared.occurrence.stage, prepared.observation.event_id),
            index,
        )?;
        // Added by G3 (DR-135, S4): the owner part is decided first, in the
        // same critical section as the bound (R4, §6.1). A shortfall refuses
        // the attempt before the bound decides.
        // Changed by G3 (DR-135, G-4): the owner part keeps the draws of each
        // part, or names a reserved restoration.
        // let owner_draws = match self.owner_draws(prepared) {
        //     Ok(draws) => draws,
        //     Err(OwnerShortfall) => {
        //         return self.record_owner_refusal(prepared, key, occurrence, slot, index);
        //     }
        // };
        let owner_part = match self.owner_part(prepared) {
            Ok(part) => part,
            Err(OwnerShortfall) => {
                return self.record_owner_refusal(prepared, key, occurrence, slot, index);
            }
        };
        // Changed by G3 (DR-135, S11): the bound counts only the client part
        // of the charge. The owner shares at a price of 1 or more of a granted
        // owner part leave it: the main shares of a charge with draws, or the
        // shares of a reserved restoration (item 13). The replaced lines
        // follow.
        // let decision = match &prepared.charge {
        //     Some(charge) => self.reservation.reserve(charge),
        //     None => Err(NativePhloExecutionError::Overflow),
        // };
        let owner_usage = match (&owner_part, prepared.owner.as_ref()) {
            (OwnerPart::Draws(_) | OwnerPart::Reserved, Some(owner)) => owner.priced_usage().ok(),
            _ => Some(0),
        };
        let decision = match (&prepared.charge, owner_usage) {
            (Some(charge), Some(owner_usage)) => self
                .reservation
                .reserve_with_owner_usage(charge, owner_usage),
            _ => Err(NativePhloExecutionError::Overflow),
        };
        // Added by DR-113: the class of an exhaustion follows the bound source.
        let source = self.reservation.bound_source();
        if decision.as_ref().is_err_and(|error| {
            !matches!(
                error,
                NativePhloExecutionError::Overflow | NativePhloExecutionError::BoundExceeded
            )
        }) {
            // Changed by DR-113: exhaustion takes the class of the bound source.
            // return decision.map_err(native_error);
            return decision.map_err(|error| native_error(error, source));
        }
        // Added by G3 (DR-135, S4): a grant deducts the owner draws and
        // reserves the restorations of a peek COMM (R9). The deduction is
        // checked before any commit.
        // Changed by G3 (DR-135, G-4): a grant draws the total, bills the main
        // part and reserves the restorations with their draws. The grant of a
        // reserved restoration bills its reservation. A denial bills nothing
        // and leaves every reservation as it is.
        // if let (true, Some(draws), Some(charge), Some(owners)) = (
        //     decision.is_ok(),
        //     owner_draws.as_ref(),
        //     prepared.owner.as_ref(),
        //     self.reservation.owners_mut(),
        // ) {
        //     owners
        //         .apply(draws, charge)
        //         .map_err(|error| recording_error(&error.to_string()))?;
        //     owners.reserve_restorations(charge);
        // }
        if let (true, Some(charge), Some(owners)) = (
            decision.is_ok(),
            prepared.owner.as_ref(),
            self.reservation.owners_mut(),
        ) {
            match &owner_part {
                OwnerPart::None => {}
                OwnerPart::Draws(draws) => owners
                    .grant(draws, charge)
                    .map_err(|error| recording_error(&error.to_string()))?,
                OwnerPart::Reserved => {
                    let billed = owners
                        .bill_reservation(&prepared.observation.event_id, charge)
                        .map_err(|error| recording_error(&error.to_string()))?;
                    if !billed {
                        return Err(recording_error(
                            "a reserved restoration has no reservation to bill",
                        ));
                    }
                }
            }
        }
        self.recording.occurrences.commit(key);
        // Changed by G3 (DR-135, S4): the attempt records its owner decision.
        // self.recording.attempts.push(RecordedAttempt {
        //     occurrence,
        //     observation: Arc::clone(&prepared.observation),
        //     granted: decision.is_ok(),
        // });
        // Changed by G3 (DR-135, S4 review): a COMM attempt records the
        // restorations that its owner charge paid for.
        // self.recording.attempts.push(RecordedAttempt {
        //     occurrence,
        //     observation: Arc::clone(&prepared.observation),
        //     granted: decision.is_ok(),
        //     owner_refused: false,
        // });
        self.recording.attempts.push(RecordedAttempt {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            granted: decision.is_ok(),
            owner_refused: false,
            restorations: Arc::clone(&prepared.restorations),
        });
        if decision.is_ok() {
            self.recording.accepted.commit(accepted);
        }
        self.publish_operation_link(
            slot,
            prepared.occurrence.stage,
            NativeObservationLink::Attempt(index),
            decision.is_ok(),
        );
        prepared.ticket.published.set(true);
        // Changed by DR-113: exhaustion takes the class of the bound source.
        // decision.map_err(native_error)
        decision.map_err(|error| native_error(error, source))
    }

    // Changed by G3 (DR-135, G-4): `owner_part` keeps the draws of each part
    // and takes no reservation before the bound decides.
    // /// Added by G3 (DR-135, S4): the owner draws of an attempt, or none
    // /// without owner funding, for the introduction of a restoration that a
    // /// granted peek COMM paid for (R9), or without an owner charge. A
    // /// restoration takes its reservation also when its charge overflows, as
    // /// the fold skips every restoration attempt. Play gives an unlisted purse
    // /// 0 and notes it (SA8).
    // fn owner_draws(
    //     &mut self,
    //     prepared: &NativeObservationPreparation,
    // ) -> Result<Option<ResourceMultiset<[u8; 32]>>, OwnerShortfall> {
    //     let (Some(binding), Some(owners)) = (self.owner.as_ref(), self.reservation.owners_mut())
    //     else {
    //         return Ok(None);
    //     };
    //     if prepared.occurrence.stage == NativeAttemptStage::ProduceIntroduction
    //         && owners.take_reservation(&prepared.observation.event_id)
    //     {
    //         return Ok(None);
    //     }
    //     let Some(charge) = prepared.owner.as_ref() else {
    //         return Ok(None);
    //     };
    //     owners
    //         .allocate(charge, UnlistedRule::Play, Some(binding.notes()))
    //         .map(Some)
    // }

    /// Added by G3 (DR-135, G-4): the owner part of an attempt: none without
    /// owner funding or without an owner charge (an overflowing charge has
    /// none, and the bound denies it). The introduction of a restoration that
    /// a granted peek COMM reserved with an equal charge draws nothing (R9).
    /// Otherwise the draws of each part, under the Play rule: an unlisted
    /// purse gets 0 and is noted (SA8). No reservation is taken before the
    /// bound decides.
    fn owner_part(
        &self,
        prepared: &NativeObservationPreparation,
    ) -> Result<OwnerPart, OwnerShortfall> {
        let (Some(binding), Some(owners)) = (self.owner.as_ref(), self.reservation.owners()) else {
            return Ok(OwnerPart::None);
        };
        let Some(charge) = prepared.owner.as_ref() else {
            return Ok(OwnerPart::None);
        };
        if prepared.occurrence.stage == NativeAttemptStage::ProduceIntroduction
            && owners.is_reserved_for(&prepared.observation.event_id, charge)
        {
            return Ok(OwnerPart::Reserved);
        }
        owners
            .allocate_parts(charge, UnlistedRule::Play, Some(binding.notes()))
            .map(OwnerPart::Draws)
    }

    /// Added by G3 (DR-135, S4): an owner that cannot pay refuses the
    /// attempt. A COMM refusal records its source at the position of the
    /// next attempt and leaves the COMM stage open, so the operation may try
    /// another tuple (R7). An introduction refusal is an attempt that is not
    /// granted, with nothing stored (R8).
    fn record_owner_refusal(
        &mut self,
        prepared: &NativeObservationPreparation,
        key: PreparedInsert<OccurrenceKey, ()>,
        occurrence: RecordedOccurrence,
        slot: Option<usize>,
        index: usize,
    ) -> Result<(), InterpreterError> {
        if prepared.occurrence.stage == NativeAttemptStage::Comm {
            self.record_comm_refusal(slot, index);
            prepared.ticket.published.set(true);
            return Err(InterpreterError::RSpaceError(RSpaceError::CommRefused));
        }
        self.recording.occurrences.commit(key);
        self.recording.attempts.push(RecordedAttempt {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            granted: false,
            owner_refused: true,
            // Added by G3 (DR-135, S4 review): an introduction restores
            // nothing.
            restorations: Arc::from([]),
        });
        self.publish_operation_link(
            slot,
            prepared.occurrence.stage,
            NativeObservationLink::Attempt(index),
            false,
        );
        if let Some(owners) = self.reservation.owners_mut() {
            owners.refuse(prepared.observation.event_id);
        }
        prepared.ticket.published.set(true);
        Err(InterpreterError::RSpaceError(
            RSpaceError::IntroductionRefused,
        ))
    }

    pub(super) fn record_retry(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        prepared
            .ticket
            .reject_unpublished_failure(self.try_record_retry(prepared))
    }

    fn try_record_retry(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        let (key, occurrence) = self.reserve_record(prepared)?;
        let slot = self.operation_stage_slot(&prepared.occurrence)?;
        let index = self
            .recording
            .accepted
            .get(&(prepared.occurrence.stage, prepared.observation.event_id))
            .copied()
            .ok_or_else(|| recording_error("native retry has no accepted charge"))?;
        let original = &self.recording.attempts[index];
        if !Arc::ptr_eq(&original.observation, &prepared.observation)
            && original.observation != prepared.observation
        {
            return Err(recording_error(
                "native retry differs from the accepted charge",
            ));
        }
        self.recording.occurrences.commit(key);
        self.recording.retries.push(RecordedRetry {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            accepted_attempt: index,
            fresh_before: self.recording.attempts.len(),
        });
        self.publish_operation_link(
            slot,
            prepared.occurrence.stage,
            NativeObservationLink::Retry(self.recording.retries.len() - 1),
            true,
        );
        prepared.ticket.published.set(true);
        Ok(())
    }

    pub(super) fn ensure_recording_complete(&self) -> Result<(), InterpreterError> {
        if self.replay_bound
            || self.recording.pending.load(Ordering::Acquire) != 0
            || self.recording.invalid.load(Ordering::Acquire)
        {
            return Err(recording_error("native budget recording is incomplete"));
        }
        Ok(())
    }

    pub(super) fn capture_recording(&self) -> Result<NativeBudgetRecording, InterpreterError> {
        self.ensure_recording_complete()?;
        // Changed by C7b (DR-86): the captured occurrences name trie nodes, so
        // no path is copied. capture_paths charges the trie it builds.
        // let path_bytes = self
        //     .recording
        //     .occurrences
        //     .keys()
        //     .try_fold(0usize, |total, row| {
        //         total.checked_add(row.depth.checked_mul(size_of::<(u64, u64)>())?)
        //     })
        //     .ok_or(InterpreterError::HostWorkRejected)?;
        // let bytes = size_of::<NativeBudgetAttempt>()
        //     .checked_add(size_of::<NativeBudgetRetry>())
        //     .and_then(|record_bytes| count.checked_mul(record_bytes))
        //     .and_then(|bytes| bytes.checked_mul(2))
        //     .and_then(|bytes| bytes.checked_add(path_bytes))
        //     .ok_or(InterpreterError::HostWorkRejected)?;
        // work(&self.host_work, HostWorkDimension::SearchStateBytes, bytes)?;
        // work(
        //     &self.host_work,
        //     HostWorkDimension::VerificationBytes,
        //     path_bytes,
        // )?;
        let paths = self.capture_paths()?;
        let mut attempts = Vec::with_capacity(self.recording.attempts.len());
        for (row, path) in self.recording.attempts.iter().zip(&paths.attempts) {
            attempts.push(NativeBudgetAttempt {
                occurrence: NativeBudgetOccurrence {
                    session: row.occurrence.session,
                    path: *path,
                    stage: row.occurrence.stage,
                },
                observation: Arc::clone(&row.observation),
                granted: row.granted,
                // Added by G3 (DR-135, S4).
                owner_refused: row.owner_refused,
                // Added by G3 (DR-135, S4 review).
                restorations: Arc::clone(&row.restorations),
            });
        }
        let mut retries = Vec::with_capacity(self.recording.retries.len());
        for (row, path) in self.recording.retries.iter().zip(&paths.retries) {
            retries.push(NativeBudgetRetry {
                occurrence: NativeBudgetOccurrence {
                    session: row.occurrence.session,
                    path: *path,
                    stage: row.occurrence.stage,
                },
                observation: Arc::clone(&row.observation),
                accepted_attempt: row.accepted_attempt,
                fresh_before: row.fresh_before,
            });
        }
        // Added by G3 (DR-135, S4): the owner table of the run (R3).
        let owner_table = match self.reservation.owners() {
            Some(owners) => {
                let entries = owners.table().entries();
                let mut table = Vec::with_capacity(entries.len());
                table.extend(entries.iter().map(|(_, entry)| entry.clone()));
                Arc::from(table)
            }
            None => Arc::from([]),
        };
        Ok(NativeBudgetRecording {
            session: self.session,
            attempts: attempts.into(),
            retries: retries.into(),
            used: self.reservation.used(),
            paths: Arc::new(paths.trie),
            // Added by G3 (DR-135, S4).
            owner_table,
        })
    }
}
