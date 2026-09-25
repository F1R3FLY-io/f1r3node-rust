use super::*;
use crate::rust::interpreter::accounting::byte_receipts::ByteObservationSnapshot;
use crate::rust::interpreter::accounting::native_runtime::NativeBudgetRecording;

#[derive(Debug)]
pub struct NativeReplayAccountingSnapshot {
    recording: NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    observations: ByteObservationSnapshot,
}

impl NativeReplayAccountingSnapshot {
    pub(crate) fn into_parts(
        self,
    ) -> (
        NativeBudgetRecording,
        Arc<[NativeOperationRecord]>,
        ByteObservationSnapshot,
    ) {
        (self.recording, self.operations, self.observations)
    }

    pub fn session(&self) -> &[u8; 32] { &self.recording.session }
    pub fn usage(&self) -> u64 { self.recording.used }
    pub fn recording(&self) -> &NativeBudgetRecording { &self.recording }
    pub fn operations(&self) -> &[NativeOperationRecord] { &self.operations }
    pub fn observations(&self) -> &ByteObservationSnapshot { &self.observations }
}

fn granted_observations(
    recording: &NativeBudgetRecording,
    host: &HostWorkBudget,
) -> Result<ByteObservationSnapshot, NativeReplayError> {
    work(
        host,
        HostWorkDimension::VerificationOperations,
        recording.attempts.len(),
    )?;
    let mut rows = allocate(recording.attempts.len(), host)?;
    rows.extend(
        recording
            .attempts
            .iter()
            .filter(|attempt| attempt.granted)
            .map(|attempt| Arc::clone(&attempt.observation)),
    );
    Ok(ByteObservationSnapshot {
        rows,
        metered_context: true,
        history_lost: false,
    })
}

impl NativeReplayBoundary {
    pub fn completed_evidence(&self) -> Result<NativeReplayAccountingSnapshot, NativeReplayError> {
        let usage = self.completed_usage()?;
        let journal = self.replay.inner.trace.journal();
        let observations = granted_observations(&journal.recording, &self.replay.inner.host)?;
        if usage != journal.recording.used {
            return Err(NativeReplayError::Outcome);
        }
        Ok(NativeReplayAccountingSnapshot {
            recording: journal.recording.clone(),
            operations: Arc::clone(&journal.operations),
            observations,
        })
    }
}

#[cfg(test)]
mod tests;
