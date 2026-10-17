#[cfg(test)]
use std::cmp::Ordering;
// Changed by DR-132 (D2-B): the change helper takes the state guard, and the
// drop paths tolerate a poisoned lock.
// use std::sync::{Arc, Mutex};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(test)]
use rspace_plus_plus::rspace::operation_context::CausalPath;
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::COMM;
use serde::Serialize;
use thiserror::Error;

use super::{CheckedNativeOperationTrace, InterpreterError, NativeOperationRecord};
use crate::rust::interpreter::accounting::byte_receipts::ByteObservation;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBudgetReplayDecision, NativeBudgetTraceError,
};
use crate::rust::interpreter::accounting::native_runtime::operation_sources::NativeOperationSource;
use crate::rust::interpreter::accounting::native_runtime::operations::{locked_footprint, sort};
use crate::rust::interpreter::accounting::native_runtime::replay_authority::{
    ReplayAuthorityBinding, ReplayAuthorityObservation, ReplayAuthorityPublication,
};
use crate::rust::interpreter::accounting::native_runtime::NativeObservationLink;
use crate::rust::interpreter::accounting::RuntimeBudget;

mod state;
/// Added by DR-132 (D2-B): the atomics of the progress word. The Loom crate
/// includes `progress.rs` with Loom atomics instead.
mod atomic {
    pub(super) use std::sync::atomic::{AtomicU64, Ordering};
}
mod progress;
#[cfg(test)]
mod progress_tests;
mod session;
mod observations;
mod evidence;
pub use evidence::NativeReplayAccountingSnapshot;
use progress::{Registration, ReplayProgress, WaitDecision};
pub use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeReplayOutcome;
pub use session::{NativeRuntimeReplayCheckpoint, NativeRuntimeReplaySession};
use state::{LedgerError, ReplayState, SlotState};

#[derive(Debug, Error)]
pub enum NativeReplayError {
    #[error(transparent)]
    Budget(#[from] NativeBudgetTraceError),
    #[error(transparent)]
    Interpreter(#[from] InterpreterError),
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
    /// Added by DR-132 (D2-B).
    #[error("native replay stalled: every live task waits on a recorded operation")]
    Stalled,
    /// Added by DR-132 (D2-B).
    #[error("native replay operation runs outside the counted tasks of its evaluation")]
    Uncounted,
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
            LedgerError::Stalled => Self::Stalled,
        }
    }
}

struct ReplayInner {
    authority: Option<ReplayAuthorityBinding>,
    trace: CheckedNativeOperationTrace,
    identity: Arc<()>,
    // Changed by DR-132 (D1-A): replay finds each row by its recorded source,
    // so the path limit and the path index have no reader.
    // max_path: usize,
    state: Mutex<ReplayState>,
    journal_slots: Vec<usize>,
    // /// The digest key of each slot's path with the slot, sorted by key
    // /// (C7b, DR-86).
    // slot_index: Vec<(PathKey, u32)>,
    /// Added by DR-132 (D1-A): each slot with its recorded source and its
    /// journal position, sorted by source and then by journal position.
    source_index: Vec<(NativeOperationSource, u32, u32)>,
    retry_predecessors: Vec<[Option<usize>; 2]>,
    changed: tokio::sync::Notify,
    /// Added by DR-132 (D2-B): the live tasks and the parked waiters of the
    /// current evaluation.
    progress: ReplayProgress,
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

/// Added by DR-132 (D2-B): the registration of one `wait_ready` call. A wait
/// that is dropped while it holds a registration, in a cancelled or
/// panicking task, leaves it, so the parked count stays exact. The drop
/// tolerates a poisoned lock and never panics.
struct Wait<'a> {
    replay: &'a NativeOperationReplay,
    registration: Registration,
}

impl Drop for Wait<'_> {
    fn drop(&mut self) {
        if self.registration.held() {
            let state = self
                .replay
                .inner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.registration
                .leave(&self.replay.inner.progress, state.version());
        }
    }
}

// Changed by C7b (DR-86): current_slot looks the live path up by its digest
// key. The path comparison stays as the test oracle of that lookup.
#[cfg(test)]
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

fn accepted_owner(
    link: NativeObservationLink,
    trace: &CheckedNativeOperationTrace,
    attempt_owners: &[Option<usize>],
    journal_slots: &[usize],
) -> Result<Option<usize>, NativeReplayError> {
    let NativeObservationLink::Retry(index) = link else {
        return Ok(None);
    };
    let retry = trace
        .journal
        .recording
        .retries
        .get(index)
        .ok_or(NativeReplayError::Stage)?;
    let journal = attempt_owners
        .get(retry.accepted_attempt)
        .copied()
        .flatten()
        .ok_or(NativeReplayError::Stage)?;
    Ok(Some(
        *journal_slots.get(journal).ok_or(NativeReplayError::Stage)?,
    ))
}

impl CheckedNativeOperationTrace {
    pub fn into_replay(self) -> Result<NativeOperationReplay, NativeReplayError> {
        let count = self.operation_count();
        let slots = vec![SlotState::Available; count];
        let undo = Vec::with_capacity(count);
        let mut journal_slots = vec![0; count];
        for slot in 0..count {
            journal_slots[self.journal_index(slot).expect("checked journal index")] = slot;
        }
        let attempts = self.journal.recording.attempts.len();
        let mut attempt_owners = vec![None; attempts];
        for (journal, row) in self.journal.operations.iter().enumerate() {
            for link in [
                Some(row.introduction),
                row.comm.as_ref().map(|comm| comm.observation),
            ]
            .into_iter()
            .flatten()
            {
                if let NativeObservationLink::Attempt(index) = link {
                    *attempt_owners
                        .get_mut(index)
                        .ok_or(NativeReplayError::Stage)? = Some(journal);
                }
            }
        }
        let mut retry_predecessors = Vec::with_capacity(count);
        for slot in 0..count {
            let row = self.operation(slot).ok_or(NativeReplayError::Stage)?;
            let position = self.journal_index(slot).ok_or(NativeReplayError::Stage)?;
            let introduction =
                accepted_owner(row.introduction, &self, &attempt_owners, &journal_slots)?;
            let comm = row
                .comm
                .as_ref()
                .map(|comm| {
                    accepted_owner(comm.observation, &self, &attempt_owners, &journal_slots)
                })
                .transpose()?
                .flatten();
            if [introduction, comm].into_iter().flatten().any(|owner| {
                self.journal_index(owner)
                    .is_none_or(|journal| journal >= position)
            }) {
                return Err(NativeReplayError::Stage);
            }
            retry_predecessors.push([introduction, comm]);
        }
        // Changed by DR-132 (D1-A): replay finds each row by its recorded
        // source, so the path index and the path limit are not built. The
        // checker keeps the paths unique (`check_operation_journal`).
        // let paths = &self.journal.recording.paths;
        // let mut slot_index = Vec::<(PathKey, u32)>::with_capacity(count);
        // for slot in 0..count {
        //     let row = self.operation(slot).ok_or(NativeReplayError::Stage)?;
        //     slot_index.push((
        //         paths.key(row.occurrence.path),
        //         u32::try_from(slot).map_err(|_| NativeReplayError::Stage)?,
        //     ));
        // }
        // sort(&mut slot_index, |left, right| Ok(left.0.cmp(&right.0)))?;
        // if slot_index.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        //     return Err(NativeReplayError::Context);
        // }
        // let max_path = self
        //     .journal
        //     .operations
        //     .iter()
        //     .map(|row| row.occurrence.path.depth())
        //     .max()
        //     .unwrap_or(0);
        let mut source_index = Vec::with_capacity(count);
        for slot in 0..count {
            let row = self.operation(slot).ok_or(NativeReplayError::Stage)?;
            let journal = self.journal_index(slot).ok_or(NativeReplayError::Stage)?;
            source_index.push((
                row.source.clone(),
                u32::try_from(journal).map_err(|_| NativeReplayError::Stage)?,
                u32::try_from(slot).map_err(|_| NativeReplayError::Stage)?,
            ));
        }
        sort(&mut source_index, |left, right| {
            Ok(left.0.cmp(&right.0).then(left.1.cmp(&right.1)))
        })?;
        let total = self.journal.total();
        Ok(NativeOperationReplay {
            inner: Arc::new(ReplayInner {
                authority: None,
                trace: self,
                identity: Arc::new(()),
                state: Mutex::new(ReplayState::new(slots, undo, total)),
                journal_slots,
                source_index,
                retry_predecessors,
                changed: tokio::sync::Notify::new(),
                progress: ReplayProgress::new(),
            }),
        })
    }
}

impl NativeOperationReplay {
    /// Added by DR-113: the bound source of the replayed journal.
    pub fn bound_source(
        &self,
    ) -> crate::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource {
        self.inner.trace.journal().bound_source()
    }

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
            .ready(slot, self.predecessors(slot))
            .map_err(Into::into)
    }

    fn predecessors(&self, slot: usize) -> impl Iterator<Item = usize> + '_ {
        let predecessors = &self
            .inner
            .trace
            .operation(slot)
            .expect("checked replay slot")
            .predecessors;
        predecessors
            .iter()
            .map(|index| self.inner.journal_slots[*index])
            .chain(self.inner.retry_predecessors[slot].into_iter().flatten())
    }

    // Changed by DR-132 (D1-A): the slot is selected by the recorded source,
    // again at each check under the state lock. Two live operations with one
    // source then take consecutive rows and never meet `Unavailable`.
    // let slot = self.current_slot(source)?;
    // loop {
    //     ...
    //         let state = self.inner.state.lock().expect("native replay state");
    //         if self.ready(&state, slot)? {
    // Changed by DR-132 (D2-B): the wait counts its park for the stall rule
    // (`progress.rs`). When every live task of the evaluation waits on a
    // recorded operation, replay declares a stall and every waiter fails, so
    // forged evidence cannot hang replay.
    // async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), NativeReplayError> {
    //     let live = Self::live_source(source)?;
    //     loop {
    //         let notification = self.inner.changed.notified();
    //         tokio::pin!(notification);
    //         notification.as_mut().enable();
    //         {
    //             let state = self.inner.state.lock().expect("native replay state");
    //             let slot = self.select_slot(&state, &live)?;
    //             if self.ready(&state, slot)? {
    //                 return Ok(());
    //             }
    //         }
    //         notification.await;
    //     }
    // }
    async fn wait_ready(&self, source: RSpaceOperationSource<'_>) -> Result<(), NativeReplayError> {
        let live = Self::live_source(source)?;
        let counted = self.counted()?;
        let mut wait = Wait {
            replay: self,
            registration: Registration::default(),
        };
        loop {
            let notification = self.inner.changed.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            {
                let mut state = self.inner.state.lock().expect("native replay state");
                let ready = state
                    .open()
                    .map_err(NativeReplayError::from)
                    .and_then(|()| self.select_slot(&state, &live))
                    .and_then(|slot| self.ready(&state, slot));
                match progress::check_and_park(
                    &mut state,
                    &self.inner.progress,
                    &mut wait.registration,
                    ready,
                    counted,
                )? {
                    WaitDecision::Ready => return Ok(()),
                    WaitDecision::Park => {}
                    WaitDecision::Stall => {
                        drop(state);
                        self.inner.changed.notify_waiters();
                        return Err(NativeReplayError::Stalled);
                    }
                }
            }
            notification.await;
        }
    }

    /// Added by DR-132 (D2-B): whether the current task is counted by the
    /// evaluation of this replay. While that evaluation is armed, an
    /// operation from outside it is refused, so no task changes the state
    /// behind the stall rule.
    fn counted(&self) -> Result<bool, NativeReplayError> {
        let counted = crate::rust::interpreter::reduce::counted_task(self.identity());
        if !counted && self.inner.progress.armed() {
            return Err(NativeReplayError::Uncounted);
        }
        Ok(counted)
    }

    /// Added by DR-132 (D2-B): the identity of this replay.
    fn identity(&self) -> usize { Arc::as_ptr(&self.inner) as usize }

    /// Added by DR-132 (D2-B): ends a change that can make a waiter ready.
    /// The state already bumped its version. The parked count clears inside
    /// the critical section, and the waiters wake after the unlock.
    fn changed(&self, mut state: MutexGuard<'_, ReplayState>) {
        progress::after_change(&mut state, &self.inner.progress);
        drop(state);
        self.inner.changed.notify_waiters();
    }

    /// Added by DR-132 (D2-B): no task is live, no waiter is parked and no
    /// flag is set.
    #[cfg(test)]
    pub(crate) fn progress_idle(&self) -> bool { self.inner.progress.idle() }

    /// Added by DR-132 (D2-B): replay declared a stall.
    #[cfg(test)]
    pub(crate) fn progress_stalled(&self) -> bool { self.inner.progress.stalled() }

    /// Added by DR-132 (D2-B): the live tasks and the parked waiters.
    #[cfg(test)]
    pub(crate) fn progress_counts(&self) -> (u64, u64) { self.inner.progress.counts() }

    /// Added by DR-132 (D2-B): poisons the state lock, as a task that panics
    /// while it holds the lock does.
    #[cfg(test)]
    pub(crate) fn poison_state(&self) {
        let inner = Arc::clone(&self.inner);
        let _ = std::thread::spawn(move || {
            let _state = inner.state.lock().expect("native replay state");
            panic!("a task panics while it holds the native replay state");
        })
        .join();
        assert!(self.inner.state.is_poisoned());
    }

    // Changed by DR-132 (D1-A): the slot is selected by the recorded source
    // under the same lock as the reservation.
    // let slot = self.current_slot(source)?;
    // let mut state = self.inner.state.lock().expect("native replay state");
    fn reserve_ready(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<NativeReplayReservation>, NativeReplayError> {
        let live = Self::live_source(source)?;
        // Added by DR-132 (D2-B): an uncounted operation is refused while an
        // evaluation is armed.
        self.counted()?;
        let mut state = self.inner.state.lock().expect("native replay state");
        let slot = self.select_slot(&state, &live)?;
        Ok(state
            .reserve_ready(slot, self.predecessors(slot))?
            .map(|serial| self.reservation(slot, serial)))
    }

    /// Added by DR-132 (D1-A): the recorded form of a live source.
    fn live_source(
        source: RSpaceOperationSource<'_>,
    ) -> Result<NativeOperationSource, NativeReplayError> {
        NativeOperationSource::capture(source).map_err(|_| NativeReplayError::Source)
    }

    /// Added by DR-132 (D1-A): the slot of a live operation. It is the first
    /// Available row with the same recorded source, in journal order. The
    /// completed rows of one source form a prefix, because each such row lists
    /// the one before it as a channel predecessor, and replay completes a row
    /// only after its predecessors. A binary search finds the end of that
    /// prefix. At most the first open row can be reserved, so the scan after
    /// the prefix takes at most two steps. A source that no row records is
    /// `Source`. A recorded source whose rows are all taken is `Unavailable`,
    /// as a taken slot was under the name lookup.
    fn select_slot(
        &self,
        state: &ReplayState,
        live: &NativeOperationSource,
    ) -> Result<usize, NativeReplayError> {
        let index = &self.inner.source_index;
        let start = index.partition_point(|(source, _, _)| source < live);
        let end = start + index[start..].partition_point(|(source, _, _)| source == live);
        let candidates = &index[start..end];
        if candidates.is_empty() {
            return Err(NativeReplayError::Source);
        }
        let open = candidates.partition_point(|(_, _, slot)| {
            state.slot(*slot as usize) == Some(SlotState::Completed)
        });
        candidates[open..]
            .iter()
            .map(|(_, _, slot)| *slot as usize)
            .find(|slot| state.slot(*slot) == Some(SlotState::Available))
            .ok_or(NativeReplayError::Unavailable)
    }

    // Changed by DR-132 (D2-B): the invalidation ends as a change.
    // fn invalidate(&self) {
    //     self.inner
    //         .state
    //         .lock()
    //         .unwrap_or_else(|poisoned| poisoned.into_inner())
    //         .invalidate();
    //     self.inner.changed.notify_waiters();
    // }
    fn invalidate(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.invalidate();
        self.changed(state);
    }

    // Changed by DR-132 (D1-A): replay selects each slot by its recorded source
    // (`select_slot`). Racing evaluation sets no operation name, so the name
    // lookups below have no caller.
    // fn current_slot(&self, source: RSpaceOperationSource<'_>) -> Result<usize, NativeReplayError> {
    //     let current = operation_context::current().ok_or(NativeReplayError::Context)?;
    //     if current.session != self.inner.trace.journal.recording.session
    //         || current.path.len() > self.inner.max_path
    //     {
    //         return Err(NativeReplayError::Context);
    //     }
    //     // Changed by C7b (DR-86): the slot of the live path is found by its
    //     // digest key in the sorted index. A binary search over n keys makes at
    //     // most bit_length(n) comparisons of fixed-size keys. Equal keys mean
    //     // equal paths (`NativePathTrie.occurrence_key_correct`).
    //     // let mut low = 0;
    //     // let mut high = self.inner.trace.operation_count();
    //     // let slot = loop {
    //     //     if low == high {
    //     //         return Err(NativeReplayError::Context);
    //     //     }
    //     //     let mid = low + (high - low) / 2;
    //     //     let row = self
    //     //         .inner
    //     //         .trace
    //     //         .operation(mid)
    //     //         .expect("checked native replay slot");
    //     //     let segments = checked_add(current.path.len(), row.occurrence.path.len())?;
    //     //     work(
    //     //         &self.inner.host,
    //     //         HostWorkDimension::VerificationOperations,
    //     //         checked_add(segments, 1)?,
    //     //     )?;
    //     //     work(
    //     //         &self.inner.host,
    //     //         HostWorkDimension::VerificationBytes,
    //     //         checked_mul(segments, 16)?,
    //     //     )?;
    //     //     match compare_path(&current.path, &row.occurrence.path) {
    //     //         Ordering::Less => high = mid,
    //     //         Ordering::Greater => low = mid + 1,
    //     //         Ordering::Equal => break mid,
    //     //     }
    //     // };
    //     let slot = self.lookup_slot(&current.path)?;
    //     let expected = &self
    //         .inner
    //         .trace
    //         .operation(slot)
    //         .expect("checked native replay slot")
    //         .source;
    //     if !expected.matches(source) {
    //         return Err(NativeReplayError::Source);
    //     }
    //     Ok(slot)
    // }
    //
    // /// The slot whose path is `path`, by the sorted digest index.
    // pub(in crate::rust::interpreter::accounting::native_runtime) fn lookup_slot(
    //     &self,
    //     path: &CausalPath,
    // ) -> Result<usize, NativeReplayError> {
    //     let key = PathKey::of(path);
    //     let position = self
    //         .inner
    //         .slot_index
    //         .binary_search_by(|(candidate, _)| candidate.cmp(&key))
    //         .map_err(|_| NativeReplayError::Context)?;
    //     Ok(self.inner.slot_index[position].1 as usize)
    // }

    pub fn begin_boundary(&self) -> Result<NativeReplayBoundary, NativeReplayError> {
        let mut state = self.inner.state.lock().expect("native replay state");
        // Added by DR-132 (D2-B): no boundary opens while an evaluation is
        // armed, so a boundary cannot hide readiness from the stall rule.
        if self.inner.progress.armed() {
            return Err(NativeReplayError::Busy);
        }
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
        let actual = locked_footprint(channels, joins)?;
        let expected = &self.operation().footprint;
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
        let decision = journal.budget.authenticate(index, observation)?;
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
            // Changed by DR-132 (D2-B): the cancel ends as a change.
            // drop(state);
            // self.replay.inner.changed.notify_waiters();
            self.replay.changed(state);
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
        // Changed by DR-132 (D2-B): the publish ends as a change.
        // drop(state);
        // ticket.replay.inner.changed.notify_waiters();
        ticket.replay.changed(state);
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
    // Changed by DR-132 (D2-B): the end of a boundary ends as a change.
    // fn drop(&mut self) {
    //     self.replay
    //         .inner
    //         .state
    //         .lock()
    //         .expect("native replay state")
    //         .end_boundary();
    //     self.replay.inner.changed.notify_waiters();
    // }
    fn drop(&mut self) {
        let mut state = self.replay.inner.state.lock().expect("native replay state");
        state.end_boundary();
        self.replay.changed(state);
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
