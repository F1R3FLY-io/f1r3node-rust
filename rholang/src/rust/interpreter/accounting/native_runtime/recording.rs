use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use models::rhoapi::CostAuthority;

use super::index::{NativeIndex, OccurrenceKey, PreparedInsert};
use super::*;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence, NativeDenialCause,
    NativeStackTransferEvent, NativeStackTransferKind,
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
    pub transfers: Arc<[NativeStackTransferEvent]>,
    pub used: u64,
    /// The causal paths of the attempts, the retries and the operation
    /// journal rows, interned once each (C7b, DR-86). Occurrences and journal
    /// rows name their nodes.
    pub paths: Arc<NativePathTrie>,
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
    pub(super) cause: NativeDenialCause,
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
    pub(super) transfers: Vec<NativeStackTransferEvent>,
    transfer_reserves: BTreeMap<[u8; 32], usize>,
    pub(super) invalid: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

pub(in crate::rust::interpreter::accounting) struct NativeObservationPreparation {
    pub(super) generation: Arc<()>,
    pub(super) occurrence: RecordedOccurrence,
    pub(super) occurrence_key: OccurrenceKey,
    pub(super) observation: Arc<ByteObservation>,
    pub(super) charge: Option<PreparedNativePhloCharge>,
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
        let decision = match &prepared.charge {
            Some(charge) => self.reservation.reserve(charge),
            None => Err(NativePhloExecutionError::Overflow),
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
        self.recording.occurrences.commit(key);
        self.recording.attempts.push(RecordedAttempt {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            granted: decision.is_ok(),
            cause: NativeDenialCause::Budget,
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

    pub(super) fn record_allocation_denial(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        prepared
            .ticket
            .reject_unpublished_failure(self.try_record_allocation_denial(prepared))
    }

    fn try_record_allocation_denial(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<(), InterpreterError> {
        if prepared.occurrence.stage != NativeAttemptStage::Comm {
            return Err(recording_error(
                "native allocation denial belongs to a COMM",
            ));
        }
        let (key, occurrence) = self.reserve_record(prepared)?;
        let slot = self.operation_stage_slot(&prepared.occurrence)?;
        let index = self.recording.attempts.len();
        self.recording.occurrences.commit(key);
        self.recording.attempts.push(RecordedAttempt {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            granted: false,
            cause: NativeDenialCause::Allocation,
        });
        self.publish_operation_link(
            slot,
            prepared.occurrence.stage,
            NativeObservationLink::Attempt(index),
            false,
        );
        prepared.ticket.published.set(true);
        Ok(())
    }

    pub(in crate::rust::interpreter::accounting) fn record_stack_transfer(
        &mut self,
        produce_hash: [u8; 32],
        authority: &CostAuthority,
        cells: u64,
        granted: bool,
    ) {
        if self.replay_bound {
            return;
        }
        let index = self.recording.transfers.len();
        if index >= self.limits.attempts
            || self
                .recording
                .transfer_reserves
                .insert(produce_hash, index)
                .is_some()
        {
            self.recording.invalid.store(true, Ordering::Release);
            return;
        }
        self.recording.transfers.push(NativeStackTransferEvent {
            kind: NativeStackTransferKind::Reserve {
                produce_hash,
                authority: authority.clone(),
                cells,
                granted,
            },
            fresh_before: self.recording.attempts.len(),
        });
    }

    pub(in crate::rust::interpreter::accounting) fn record_stack_release(
        &mut self,
        produce_hash: [u8; 32],
    ) {
        if self.replay_bound {
            return;
        }
        let reserve = self.recording.transfer_reserves.get(&produce_hash).copied();
        match reserve {
            Some(reserve) if self.recording.transfers.len() < self.limits.attempts => {
                self.recording.transfers.push(NativeStackTransferEvent {
                    kind: NativeStackTransferKind::Release { reserve },
                    fresh_before: self.recording.attempts.len(),
                });
            }
            _ => self.recording.invalid.store(true, Ordering::Release),
        }
    }

    pub(in crate::rust::interpreter::accounting) fn replayed_stack_transfer(
        &self,
        produce_hash: &[u8; 32],
    ) -> Option<Option<(CostAuthority, u64, bool)>> {
        if !self.replay_bound {
            return None;
        }
        Some(
            self.replay_transfers
                .binary_search_by(|row| row.0.cmp(produce_hash))
                .ok()
                .map(|position| {
                    let row = &self.replay_transfers[position];
                    (row.1.clone(), row.2, row.3)
                }),
        )
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
                cause: row.cause,
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
        Ok(NativeBudgetRecording {
            session: self.session,
            attempts: attempts.into(),
            retries: retries.into(),
            transfers: self.recording.transfers.clone().into(),
            used: self.reservation.used(),
            paths: Arc::new(paths.trie),
        })
    }
}
