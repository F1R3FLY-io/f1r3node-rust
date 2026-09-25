use std::cmp::Ordering;
use std::mem::size_of;
use std::sync::{Arc, Mutex};

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::operation_context::{self, CausalPath};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::COMM;
use serde::Serialize;
use thiserror::Error;

use super::{
    allocate, work, CheckedNativeOperationTrace, HostWorkBudget, InterpreterError,
    NativeOperationRecord, NativeOperationSource,
};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetReplayDecision, NativeBudgetTraceError,
};
use crate::rust::interpreter::accounting::native_runtime::operations::locked_footprint;
use crate::rust::interpreter::accounting::native_runtime::replay_authority::{
    ReplayAuthorityBinding, ReplayAuthorityObservation, ReplayAuthorityPublication,
};
use crate::rust::interpreter::accounting::native_runtime::NativeObservationLink;
use crate::rust::interpreter::accounting::RuntimeBudget;

mod state;
mod session;
mod observations;
mod evidence;
pub use evidence::NativeReplayAccountingSnapshot;
pub use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeReplayOutcome;
pub use session::{NativeRuntimeReplayCheckpoint, NativeRuntimeReplaySession};
use state::{LedgerError, ReplayState, SlotState};

#[derive(Debug, Error)]
pub enum NativeReplayError {
    #[error(transparent)]
    Budget(#[from] NativeBudgetTraceError),
    #[error(transparent)]
    Host(#[from] InterpreterError),
    #[error("native replay operation context is absent or does not occur in this trace")]
    Context,
    #[error("native replay introduction source differs from its occurrence")]
    Source,
    #[error("native replay locked footprint differs from its checked occurrence")]
    Footprint,
    #[error("native replay COMM source differs from its checked occurrence")]
    CommSource,
    #[error("native replay occurrence is reserved or completed")]
    Unavailable,
    #[error("native replay channel predecessors have not completed")]
    Dependency,
    #[error("native replay boundary requires quiescence and exclusive ownership")]
    Busy,
    #[error("native replay checkpoint is foreign or no longer an ancestor")]
    Checkpoint,
    #[error("native replay epoch is closed")]
    Closed,
    #[error("native replay publication serial is exhausted")]
    Serial,
    #[error("native replay outcome differs from the exact recorded outcome")]
    Outcome,
    #[error("native replay has incomplete operations")]
    Incomplete,
    #[error("native replay budget observation is absent, repeated, or out of order")]
    Stage,
    #[error("native replay observation construction failed: {0}")]
    Construction(String),
}

impl From<LedgerError> for NativeReplayError {
    fn from(error: LedgerError) -> Self {
        match error {
            LedgerError::Closed => Self::Closed,
            LedgerError::Busy => Self::Busy,
            LedgerError::Unavailable => Self::Unavailable,
            LedgerError::Serial => Self::Serial,
            LedgerError::Checkpoint => Self::Checkpoint,
            LedgerError::Incomplete => Self::Incomplete,
        }
    }
}

struct ReplayInner {
    authority: Option<ReplayAuthorityBinding>,
    trace: CheckedNativeOperationTrace,
    host: HostWorkBudget,
    identity: Arc<()>,
    max_path: usize,
    state: Mutex<ReplayState>,
    journal_slots: Vec<usize>,
    changed: tokio::sync::Notify,
}

#[derive(Clone)]
pub struct NativeOperationReplay {
    inner: Arc<ReplayInner>,
}

#[derive(Clone)]
pub struct NativeReplayCheckpoint {
    identity: Arc<()>,
    cursor: usize,
    serial: u64,
}

pub struct NativeReplayReservation {
    authority_observations: [Option<ReplayAuthorityObservation>; 2],
    replay: NativeOperationReplay,
    slot: usize,
    serial: u64,
    active: bool,
    stage: ObservationStage,
    usage: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObservationStage {
    Footprint,
    Introduction,
    Comm,
    Complete,
}

pub struct NativeReplayPublication {
    authority: Option<ReplayAuthorityPublication>,
    reservation: NativeReplayReservation,
}

pub struct NativeReplayBoundary {
    replay: NativeOperationReplay,
}

pub struct NativeReplayRestore {
    boundary: NativeReplayBoundary,
    cursor: usize,
}

fn compare_path(actual: &CausalPath, expected: &[(u64, u64)]) -> Ordering {
    let common = actual.len().min(expected.len());
    let mut order = actual.len().cmp(&expected.len());
    for (a, b) in actual
        .segments_rev()
        .skip(actual.len() - common)
        .zip(expected[..common].iter().rev())
    {
        let comparison = a.cmp(b);
        if comparison != Ordering::Equal {
            order = comparison;
        }
    }
    order
}

impl CheckedNativeOperationTrace {
    pub fn into_replay(
        self,
        host: HostWorkBudget,
    ) -> Result<NativeOperationReplay, NativeReplayError> {
        let count = self.operation_count();
        work(
            &host,
            HostWorkDimension::VerificationOperations,
            count.saturating_add(1),
        )?;
        work(
            &host,
            HostWorkDimension::SearchStateBytes,
            size_of::<ReplayInner>()
                .saturating_add(128)
                .saturating_mul(2),
        )?;
        let mut slots = allocate(count, &host)?;
        slots.resize(count, SlotState::Available);
        let undo = allocate(count, &host)?;
        let mut journal_slots = allocate(count, &host)?;
        journal_slots.resize(count, 0);
        for slot in 0..count {
            journal_slots[self.journal_index(slot).expect("checked journal index")] = slot;
        }
        let total = self.journal.total();
        let max_path = self
            .journal
            .operations
            .iter()
            .map(|row| row.occurrence.path.len())
            .max()
            .unwrap_or(0);
        Ok(NativeOperationReplay {
            inner: Arc::new(ReplayInner {
                authority: None,
                trace: self,
                host,
                identity: Arc::new(()),
                max_path,
                state: Mutex::new(ReplayState::new(slots, undo, total)),
                journal_slots,
                changed: tokio::sync::Notify::new(),
            }),
        })
    }
}

impl NativeOperationReplay {
    fn bind_runtime(mut self, budget: RuntimeBudget) -> Result<Self, NativeReplayError> {
        let binding =
            ReplayAuthorityBinding::new(budget, self.inner.trace.journal.recording.session)?;
        Arc::get_mut(&mut self.inner)
            .ok_or(NativeReplayError::Busy)?
            .authority = Some(binding);
        Ok(self)
    }
    #[cfg(test)]
    pub fn completed_usage(&self) -> u64 {
        self.inner
            .state
            .lock()
            .expect("native replay state")
            .completed_usage()
    }

    pub fn trace(&self) -> &CheckedNativeOperationTrace { &self.inner.trace }

    pub fn reserve_current(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<NativeReplayReservation, NativeReplayError> {
        self.reserve_ready(source)?
            .ok_or(NativeReplayError::Dependency)
    }

    fn reservation(&self, slot: usize, serial: u64) -> NativeReplayReservation {
        NativeReplayReservation {
            authority_observations: [None, None],
            replay: self.clone(),
            slot,
            serial,
            active: true,
            stage: ObservationStage::Footprint,
            usage: 0,
        }
    }

    fn ready(&self, state: &ReplayState, slot: usize) -> Result<bool, NativeReplayError> {
        state
            .ready(slot, self.predecessors(slot)?)
            .map_err(Into::into)
    }

    fn predecessors(
        &self,
        slot: usize,
    ) -> Result<impl Iterator<Item = usize> + '_, NativeReplayError> {
        let predecessors = &self
            .inner
            .trace
            .operation(slot)
            .expect("checked replay slot")
            .predecessors;
        work(
            &self.inner.host,
            HostWorkDimension::VerificationOperations,
            predecessors.len().saturating_add(1),
        )?;
        Ok(predecessors
            .iter()
            .map(|index| self.inner.journal_slots[*index]))
    }

    async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), NativeReplayError> {
        let slot = self.current_slot(source)?;
        loop {
            let notification = self.inner.changed.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            {
                let state = self.inner.state.lock().expect("native replay state");
                if self.ready(&state, slot)? {
                    return Ok(());
                }
            }
            notification.await;
        }
    }

    fn reserve_ready(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<NativeReplayReservation>, NativeReplayError> {
        let slot = self.current_slot(source)?;
        let mut state = self.inner.state.lock().expect("native replay state");
        Ok(state
            .reserve_ready(slot, self.predecessors(slot)?)?
            .map(|serial| self.reservation(slot, serial)))
    }

    fn invalidate(&self) {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate();
        self.inner.changed.notify_waiters();
    }

    fn current_slot(&self, source: RSpaceOperationSource<'_>) -> Result<usize, NativeReplayError> {
        work(
            &self.inner.host,
            HostWorkDimension::VerificationOperations,
            1,
        )?;
        work(&self.inner.host, HostWorkDimension::VerificationBytes, 64)?;
        let current = operation_context::current().ok_or(NativeReplayError::Context)?;
        if current.session != self.inner.trace.journal.recording.session
            || current.path.len() > self.inner.max_path
        {
            return Err(NativeReplayError::Context);
        }
        let mut low = 0;
        let mut high = self.inner.trace.operation_count();
        let slot = loop {
            if low == high {
                return Err(NativeReplayError::Context);
            }
            let mid = low + (high - low) / 2;
            let row = self
                .inner
                .trace
                .operation(mid)
                .expect("checked native replay slot");
            let segments = current.path.len().saturating_add(row.occurrence.path.len());
            work(
                &self.inner.host,
                HostWorkDimension::VerificationOperations,
                segments.saturating_add(1),
            )?;
            work(
                &self.inner.host,
                HostWorkDimension::VerificationBytes,
                segments.saturating_mul(16),
            )?;
            match compare_path(&current.path, &row.occurrence.path) {
                Ordering::Less => high = mid,
                Ordering::Greater => low = mid + 1,
                Ordering::Equal => break mid,
            }
        };
        let expected = &self
            .inner
            .trace
            .operation(slot)
            .expect("checked native replay slot")
            .source;
        let (operations, bytes) = match expected {
            NativeOperationSource::Produce(_) => (1, 65),
            NativeOperationSource::Consume(source) => (
                source.channels.len().saturating_add(1),
                source.channels.len().saturating_mul(32).saturating_add(33),
            ),
        };
        work(
            &self.inner.host,
            HostWorkDimension::VerificationOperations,
            operations.saturating_add(8),
        )?;
        work(
            &self.inner.host,
            HostWorkDimension::VerificationBytes,
            bytes,
        )?;
        if !expected.matches(source) {
            return Err(NativeReplayError::Source);
        }
        Ok(slot)
    }

    pub fn begin_boundary(&self) -> Result<NativeReplayBoundary, NativeReplayError> {
        let mut state = self.inner.state.lock().expect("native replay state");
        state.begin_boundary()?;
        Ok(NativeReplayBoundary {
            replay: self.clone(),
        })
    }

    pub fn checkpoint(&self) -> Result<NativeReplayCheckpoint, NativeReplayError> {
        Ok(self.begin_boundary()?.checkpoint())
    }

    pub fn restore(&self, token: &NativeReplayCheckpoint) -> Result<(), NativeReplayError> {
        self.begin_boundary()?.prepare_restore(token)?.publish();
        Ok(())
    }

    pub fn close(&self) -> Result<(), NativeReplayError> {
        self.begin_boundary()?.close();
        Ok(())
    }

    pub fn check_complete(&self) -> Result<(), NativeReplayError> {
        let state = self.inner.state.lock().expect("native replay state");
        state.check_complete().map_err(Into::into)
    }
}

impl NativeReplayReservation {
    pub fn authenticate_footprint<C: Serialize>(
        &mut self,
        channels: &[C],
        joins: &[Vec<C>],
    ) -> Result<(), NativeReplayError> {
        if self.stage != ObservationStage::Footprint {
            return Err(NativeReplayError::Stage);
        }
        let actual = locked_footprint(channels, joins, &self.replay.inner.host)?;
        let expected = &self.operation().footprint;
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationOperations,
            actual.len().saturating_add(1),
        )?;
        for channel in &actual {
            work(
                &self.replay.inner.host,
                HostWorkDimension::VerificationBytes,
                channel.len(),
            )?;
        }
        if actual.as_slice() != expected.as_ref() {
            return Err(NativeReplayError::Footprint);
        }
        self.stage = ObservationStage::Introduction;
        Ok(())
    }

    pub fn observe_introduction(
        &mut self,
        observation: &ByteObservation,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        if self.stage != ObservationStage::Introduction {
            return Err(NativeReplayError::Stage);
        }
        let link = self.operation().introduction;
        let next = if self.operation().comm.is_some() {
            ObservationStage::Comm
        } else {
            ObservationStage::Complete
        };
        self.observe(link, observation, next)
    }

    pub fn observe_comm(
        &mut self,
        source: &COMM,
        observation: &ByteObservation,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        self.authenticate_comm_source(source)?;
        let link = self
            .operation()
            .comm
            .as_ref()
            .ok_or(NativeReplayError::Stage)?
            .observation;
        self.observe(link, observation, ObservationStage::Complete)
    }

    fn authenticate_comm_source(&self, source: &COMM) -> Result<(), NativeReplayError> {
        if self.stage != ObservationStage::Comm {
            return Err(NativeReplayError::Stage);
        }
        let expected = self
            .operation()
            .comm
            .as_ref()
            .ok_or(NativeReplayError::Stage)?;
        expected
            .source
            .reserve_comparison(&self.replay.inner.host)?;
        if !expected.source.matches(source) {
            return Err(NativeReplayError::CommSource);
        }
        Ok(())
    }

    fn observe(
        &mut self,
        link: NativeObservationLink,
        observation: &ByteObservation,
        next: ObservationStage,
    ) -> Result<NativeBudgetReplayDecision, NativeReplayError> {
        let journal = &self.replay.inner.trace.journal;
        let (index, retry) = match link {
            NativeObservationLink::Attempt(index) => (index, false),
            NativeObservationLink::Retry(index) => {
                (journal.recording.retries[index].accepted_attempt, true)
            }
        };
        let decision = journal
            .budget
            .authenticate(index, observation, &self.replay.inner.host)?;
        let decision = match (decision, retry) {
            (NativeBudgetReplayDecision::Accepted { .. }, true) => {
                NativeBudgetReplayDecision::Accepted { usage: 0 }
            }
            (NativeBudgetReplayDecision::Denied, true) => {
                return Err(NativeBudgetTraceError::Decision.into())
            }
            (decision, false) => decision,
        };
        if self.replay.inner.authority.is_some() {
            let stage = match self.stage {
                ObservationStage::Introduction => 0,
                ObservationStage::Comm => 1,
                _ => return Err(NativeReplayError::Stage),
            };
            self.authority_observations[stage] = Some(ReplayAuthorityObservation {
                observation: Arc::clone(&journal.recording.attempts[index].observation),
                granted: matches!(decision, NativeBudgetReplayDecision::Accepted { .. }),
                retry,
            });
        }
        let usage = match decision {
            NativeBudgetReplayDecision::Accepted { usage } => usage,
            NativeBudgetReplayDecision::Denied => 0,
        };
        self.usage = self
            .usage
            .checked_add(usage)
            .filter(|usage| *usage <= journal.total())
            .ok_or(NativeBudgetTraceError::Usage)?;
        self.stage = next;
        Ok(decision)
    }

    pub fn slot(&self) -> usize { self.slot }

    pub fn operation(&self) -> &NativeOperationRecord {
        self.replay
            .inner
            .trace
            .operation(self.slot)
            .expect("reserved native replay slot")
    }

    pub fn expected_outcome(&self) -> NativeReplayOutcome {
        let row = self.operation();
        match row.completion {
            RSpaceOperationCompletion::Stored => NativeReplayOutcome::Stored,
            RSpaceOperationCompletion::Matched => NativeReplayOutcome::Matched,
            RSpaceOperationCompletion::Rejected if row.comm.is_some() => {
                NativeReplayOutcome::DeniedComm
            }
            RSpaceOperationCompletion::Rejected => NativeReplayOutcome::DeniedIntroduction,
        }
    }

    pub fn prepare_completion(
        mut self,
        outcome: NativeReplayOutcome,
    ) -> Result<NativeReplayPublication, NativeReplayError> {
        if outcome != self.expected_outcome() {
            return Err(NativeReplayError::Outcome);
        }
        if self.stage != ObservationStage::Complete {
            return Err(NativeReplayError::Stage);
        }
        work(
            &self.replay.inner.host,
            HostWorkDimension::VerificationOperations,
            24,
        )?;
        let authority = self
            .replay
            .inner
            .authority
            .as_ref()
            .map(|binding| binding.prepare(std::mem::take(&mut self.authority_observations)))
            .transpose()?;
        Ok(NativeReplayPublication {
            reservation: self,
            authority,
        })
    }
}

impl Drop for NativeReplayReservation {
    fn drop(&mut self) {
        if self.active {
            let mut state = self
                .replay
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.cancel(self.slot, self.serial);
            drop(state);
            self.replay.inner.changed.notify_waiters();
        }
    }
}

impl NativeReplayPublication {
    pub fn publish(mut self) { self.publish_in_place(); }

    fn publish_in_place(&mut self) {
        let ticket = &mut self.reservation;
        assert!(ticket.active, "native replay publication already completed");
        if let Some(authority) = &mut self.authority {
            authority.publish();
        }
        let mut state = ticket
            .replay
            .inner
            .state
            .lock()
            .expect("native replay state");
        state.publish(ticket.slot, ticket.serial, ticket.usage);
        ticket.active = false;
        drop(state);
        ticket.replay.inner.changed.notify_waiters();
    }
}

impl NativeReplayBoundary {
    pub fn completed_usage(&self) -> Result<u64, NativeReplayError> {
        self.replay
            .inner
            .state
            .lock()
            .expect("native replay state")
            .completed_usage_at_boundary()
            .map_err(Into::into)
    }

    pub fn check_complete(&self) -> Result<(), NativeReplayError> {
        self.replay
            .inner
            .state
            .lock()
            .expect("native replay state")
            .check_complete_at_boundary()
            .map_err(Into::into)
    }

    pub fn checkpoint(&self) -> NativeReplayCheckpoint {
        let state = self.replay.inner.state.lock().expect("native replay state");
        let (cursor, serial) = state.checkpoint();
        NativeReplayCheckpoint {
            identity: Arc::clone(&self.replay.inner.identity),
            cursor,
            serial,
        }
    }

    pub fn prepare_restore(
        self,
        token: &NativeReplayCheckpoint,
    ) -> Result<NativeReplayRestore, NativeReplayError> {
        {
            let state = self.replay.inner.state.lock().expect("native replay state");
            if !Arc::ptr_eq(&token.identity, &self.replay.inner.identity) {
                return Err(NativeReplayError::Checkpoint);
            }
            state.validate_restore(token.cursor, token.serial)?;
        }
        Ok(NativeReplayRestore {
            boundary: self,
            cursor: token.cursor,
        })
    }

    pub fn close(self) {
        self.replay
            .inner
            .state
            .lock()
            .expect("native replay state")
            .close();
    }
}

impl Drop for NativeReplayBoundary {
    fn drop(&mut self) {
        self.replay
            .inner
            .state
            .lock()
            .expect("native replay state")
            .end_boundary();
        self.replay.inner.changed.notify_waiters();
    }
}

impl NativeReplayRestore {
    pub fn publish(self) {
        let mut state = self
            .boundary
            .replay
            .inner
            .state
            .lock()
            .expect("native replay state");
        state.restore(self.cursor);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn reverse_path_walk_preserves_forward_lexicographic_order(
            actual in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
            expected in prop::collection::vec((any::<u64>(), any::<u64>()), 0..128),
        ) {
            prop_assert_eq!(compare_path(&CausalPath::from(actual.clone()), &expected), actual.cmp(&expected));
        }
    }
}
