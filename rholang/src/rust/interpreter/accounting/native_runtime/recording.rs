use std::cell::Cell;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;

use super::index::{reserve_vector, NativeIndex, PreparedInsert};
use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::reserve_work;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence,
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
}

#[derive(Default)]
pub(super) struct NativeBudgetRecorder {
    pub(super) attempts: Vec<NativeBudgetAttempt>,
    pub(super) retries: Vec<NativeBudgetRetry>,
    occurrences: NativeIndex<NativeBudgetOccurrence, ()>,
    accepted: NativeIndex<(NativeAttemptStage, [u8; 32]), usize>,
    pub(super) invalid: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
    attempt_capacity: usize,
    retry_capacity: usize,
    #[cfg(test)]
    pub(super) fail_allocation_after: Option<usize>,
}

pub(in crate::rust::interpreter::accounting) struct NativeObservationPreparation {
    pub(super) generation: Arc<()>,
    pub(super) occurrence: NativeBudgetOccurrence,
    pub(super) observation: Arc<ByteObservation>,
    pub(super) charge: Option<PreparedNativePhloCharge>,
    pub(super) comparison_bytes: usize,
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

pub(super) fn work(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), InterpreterError> {
    reserve_work(budget, dimension, amount).map_err(|_| InterpreterError::HostWorkRejected)
}

fn copy_occurrence(
    value: &NativeBudgetOccurrence,
) -> Result<NativeBudgetOccurrence, InterpreterError> {
    let mut path = Vec::new();
    path.try_reserve_exact(value.path.len())
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    path.extend_from_slice(&value.path);
    Ok(NativeBudgetOccurrence {
        session: value.session,
        path,
        stage: value.stage,
    })
}

impl NativeRuntimeConfig {
    fn reserve_record(
        &mut self,
        prepared: &NativeObservationPreparation,
    ) -> Result<
        (
            PreparedInsert<NativeBudgetOccurrence, ()>,
            NativeBudgetOccurrence,
        ),
        InterpreterError,
    > {
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
        work(
            &self.host_work,
            HostWorkDimension::VerificationOperations,
            prepared
                .occurrence
                .path
                .len()
                .saturating_mul(2)
                .saturating_add(34),
        )?;
        if self
            .recording
            .occurrences
            .get(&prepared.occurrence, &self.host_work)?
            .is_some()
        {
            return Err(recording_error(
                "native budget occurrence was already recorded",
            ));
        }
        let bytes = prepared
            .occurrence
            .path
            .len()
            .saturating_mul(size_of::<(u64, u64)>())
            .saturating_mul(2);
        work(&self.host_work, HostWorkDimension::SearchStateBytes, bytes)?;
        #[cfg(test)]
        if self.recording.fail_allocation_after == Some(self.recording.occurrences.len()) {
            return Err(InterpreterError::HostWorkRejected);
        }
        reserve_vector(
            &mut self.recording.attempts,
            &mut self.recording.attempt_capacity,
            1,
            &self.host_work,
        )?;
        reserve_vector(
            &mut self.recording.retries,
            &mut self.recording.retry_capacity,
            1,
            &self.host_work,
        )?;
        let key = copy_occurrence(&prepared.occurrence)?;
        let insertion = self
            .recording
            .occurrences
            .prepare_insert(key, (), &self.host_work)?;
        Ok((insertion, copy_occurrence(&prepared.occurrence)?))
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
            &self.host_work,
        )?;
        let decision = match &prepared.charge {
            Some(charge) => self.reservation.reserve(charge),
            None => Err(NativePhloExecutionError::Overflow),
        };
        if decision.as_ref().is_err_and(|error| {
            !matches!(
                error,
                NativePhloExecutionError::Overflow | NativePhloExecutionError::BoundExceeded
            )
        }) {
            return decision.map_err(native_error);
        }
        self.recording.occurrences.commit(key);
        self.recording.attempts.push(NativeBudgetAttempt {
            occurrence,
            observation: Arc::clone(&prepared.observation),
            granted: decision.is_ok(),
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
        decision.map_err(native_error)
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
            .get(
                &(prepared.occurrence.stage, prepared.observation.event_id),
                &self.host_work,
            )?
            .copied()
            .ok_or_else(|| recording_error("native retry has no accepted charge"))?;
        let original = &self.recording.attempts[index];
        work(
            &self.host_work,
            HostWorkDimension::VerificationBytes,
            prepared.comparison_bytes,
        )?;
        if !Arc::ptr_eq(&original.observation, &prepared.observation)
            && original.observation != prepared.observation
        {
            return Err(recording_error(
                "native retry differs from the accepted charge",
            ));
        }
        self.recording.occurrences.commit(key);
        self.recording.retries.push(NativeBudgetRetry {
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
            || self.host_work.is_rejected()
        {
            return Err(recording_error("native budget recording is incomplete"));
        }
        Ok(())
    }

    pub(super) fn capture_recording(&self) -> Result<NativeBudgetRecording, InterpreterError> {
        self.ensure_recording_complete()?;
        let count = self.recording.occurrences.len();
        work(
            &self.host_work,
            HostWorkDimension::VerificationOperations,
            count.saturating_add(1),
        )?;
        let path_bytes = self
            .recording
            .occurrences
            .keys()
            .try_fold(0usize, |total, row| {
                total.checked_add(row.path.len().checked_mul(size_of::<(u64, u64)>())?)
            })
            .ok_or(InterpreterError::HostWorkRejected)?;
        let bytes = count
            .saturating_mul(size_of::<NativeBudgetAttempt>() + size_of::<NativeBudgetRetry>())
            .saturating_mul(2)
            .saturating_add(path_bytes);
        work(&self.host_work, HostWorkDimension::SearchStateBytes, bytes)?;
        work(
            &self.host_work,
            HostWorkDimension::VerificationBytes,
            path_bytes,
        )?;
        let mut attempts = Vec::new();
        attempts
            .try_reserve_exact(self.recording.attempts.len())
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        for row in &self.recording.attempts {
            attempts.push(NativeBudgetAttempt {
                occurrence: copy_occurrence(&row.occurrence)?,
                observation: Arc::clone(&row.observation),
                granted: row.granted,
            });
        }
        let mut retries = Vec::new();
        retries
            .try_reserve_exact(self.recording.retries.len())
            .map_err(|_| InterpreterError::HostWorkRejected)?;
        for row in &self.recording.retries {
            retries.push(NativeBudgetRetry {
                occurrence: copy_occurrence(&row.occurrence)?,
                observation: Arc::clone(&row.observation),
                accepted_attempt: row.accepted_attempt,
                fresh_before: row.fresh_before,
            });
        }
        Ok(NativeBudgetRecording {
            session: self.session,
            attempts: attempts.into(),
            retries: retries.into(),
            used: self.reservation.used(),
        })
    }
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};

    use super::*;
    use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::native_runtime::tests::{
        authority, config, with_operation,
    };

    #[test]
    fn record_growth_reserves_storage_before_debit() {
        let budget = RuntimeBudget::new(Cost::unsafe_max());
        budget
            .reset_for_native_execution(config(100, [0, 0, 1, 0]))
            .unwrap();
        loop {
            let observation = Arc::new(ByteObservation {
                event_id: [9; 32],
                kind: AuthorityByteEventKind::ProduceIntroduction,
                authority: authority(1),
                measurement: Some(ByteCharge {
                    transfer_bytes: 1,
                    ..ByteCharge::default()
                }),
                legacy_amount: None,
            });
            let prepared =
                with_operation(&budget, || budget.prepare_native_observation(&observation))
                    .unwrap()
                    .unwrap();
            let mut state = budget.authority_state.lock().unwrap();
            let native = state.native.as_mut().unwrap();
            let recorder = &native.recording;
            let count = recorder.occurrences.len();
            let capacity = recorder.occurrences.capacity();
            if count != 0 && count == capacity {
                let incoming = prepared.occurrence.path.len() * 2 * size_of::<(u64, u64)>();
                let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
                limits.set(
                    HostWorkDimension::SearchStateBytes,
                    HostWorkLimit::new(incoming as u64),
                );
                native.host_work = HostWorkBudget::new(limits);
                assert!(matches!(
                    native.record_charge(&prepared),
                    Err(InterpreterError::HostWorkRejected)
                ));
                assert!(native.host_work.is_rejected());
                assert_eq!(native.recording.occurrences.capacity(), capacity);
                assert_eq!(native.recording.occurrences.len(), count);
                assert_eq!(native.recording.attempts.len(), count);
                assert_eq!(native.reservation.used(), count as u64);
                break;
            }
            native.record_charge(&prepared).unwrap();
        }
    }
}
