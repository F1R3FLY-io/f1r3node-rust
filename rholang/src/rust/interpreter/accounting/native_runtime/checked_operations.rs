use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use thiserror::Error;

use super::index::{IndexKey, NativeIndex};
use super::operation_sources::allocate;
use super::operations::sort;
use super::recording::work;
use super::{
    HostWorkBudget, InterpreterError, NativeBudgetRecording, NativeObservationLink,
    NativeOperationRecord, NativeOperationSource,
};
use crate::rust::interpreter::accounting::native_phlo_rules::{
    observation_comparison_bytes, CheckedNativeBudgetEvidence, NativeAttemptStage,
    NativeBudgetTraceError, NativeBudgetTraceLimits, NativePhloExecutionContract,
};

mod trace;
pub use trace::{
    CheckedNativeOperationTrace, NativeOperationReplay, NativeOperationTraceError,
    NativeOperationTraceLimits, NativeReplayAccountingSnapshot, NativeReplayBoundary,
    NativeReplayCheckpoint, NativeReplayError, NativeReplayOutcome, NativeReplayPublication,
    NativeReplayReservation, NativeReplayRestore, NativeRuntimeReplayCheckpoint,
    NativeRuntimeReplaySession,
};

#[derive(Clone, Copy, Debug)]
pub struct NativeOperationJournalLimits {
    pub budget: NativeBudgetTraceLimits,
    pub operations: usize,
    pub total_path_segments: usize,
    pub source_entries: usize,
    pub footprint_entries: usize,
    pub footprint_bytes: usize,
    pub predecessor_edges: usize,
}

#[derive(Debug, Error)]
pub enum NativeOperationJournalError {
    #[error(transparent)]
    Budget(#[from] NativeBudgetTraceError),
    #[error(transparent)]
    Host(#[from] InterpreterError),
    #[error("native operation journal exceeds its structural limits")]
    Limit,
    #[error("native operation journal contains a different execution session")]
    Session,
    #[error("native operation journal repeats an occurrence")]
    Duplicate,
    #[error("native operation journal usage differs from the checked budget")]
    Usage,
    #[error("native operation journal does not own each publication exactly once")]
    Ownership,
    #[error("native operation journal contains an invalid retry reference")]
    Retry,
    #[error("native operation journal contains an invalid budget interval")]
    Interval,
    #[error("native operation journal contains an invalid stage or stage order")]
    Stage,
    #[error("native operation journal completion differs from its budget decisions")]
    Lifecycle,
    #[error("native operation journal footprint is not sorted and unique")]
    Footprint,
    #[error("native consume peek metadata is absent or invalid")]
    Peeks,
    #[error("native operation journal predecessors differ from its declared conflicts")]
    Dependency,
}

pub struct CheckedNativeOperationJournal {
    recording: NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    budget: CheckedNativeBudgetEvidence,
}

impl CheckedNativeOperationJournal {
    pub fn total(&self) -> u64 { self.budget.total() }
    pub fn operation_count(&self) -> usize { self.operations.len() }
    pub fn attempt_count(&self) -> usize { self.recording.attempts.len() }
    pub fn retry_count(&self) -> usize { self.recording.retries.len() }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct JournalKey<'a> {
    session: &'a [u8; 32],
    path: &'a [(u64, u64)],
    stage: Option<NativeAttemptStage>,
}

impl IndexKey for JournalKey<'_> {
    fn comparison_work(&self) -> (usize, usize) {
        (
            self.path.len().saturating_mul(2).saturating_add(4),
            self.path.len().saturating_mul(16).saturating_add(34),
        )
    }
}

fn add_count(
    total: &mut usize,
    additional: usize,
    limit: usize,
) -> Result<(), NativeOperationJournalError> {
    *total = total
        .checked_add(additional)
        .filter(|next| *next <= limit)
        .ok_or(NativeOperationJournalError::Limit)?;
    Ok(())
}

fn check_path(
    session: &[u8; 32],
    expected: &[u8; 32],
    path: &[(u64, u64)],
    total: &mut usize,
    limits: NativeOperationJournalLimits,
) -> Result<(), NativeOperationJournalError> {
    if session != expected {
        return Err(NativeOperationJournalError::Session);
    }
    if path.len() > limits.budget.path_segments {
        return Err(NativeOperationJournalError::Limit);
    }
    add_count(total, path.len(), limits.total_path_segments)
}

fn check_sizes(
    session: &[u8; 32],
    recording: &NativeBudgetRecording,
    operations: &[NativeOperationRecord],
    limits: NativeOperationJournalLimits,
    host: &HostWorkBudget,
) -> Result<usize, NativeOperationJournalError> {
    let mut publications = recording.attempts.len();
    add_count(
        &mut publications,
        recording.retries.len(),
        limits.budget.attempts,
    )?;
    if operations.len() > limits.operations {
        return Err(NativeOperationJournalError::Limit);
    }
    if recording.session != *session {
        return Err(NativeOperationJournalError::Session);
    }
    let count = publications
        .checked_add(operations.len())
        .ok_or(NativeOperationJournalError::Limit)?;
    work(
        host,
        HostWorkDimension::VerificationOperations,
        count.saturating_mul(32).saturating_add(1),
    )?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        count.saturating_mul(32),
    )?;
    let mut paths = 0;
    for occurrence in recording
        .attempts
        .iter()
        .map(|row| &row.occurrence)
        .chain(recording.retries.iter().map(|row| &row.occurrence))
    {
        check_path(
            &occurrence.session,
            session,
            &occurrence.path,
            &mut paths,
            limits,
        )?;
    }
    let mut sources = 0;
    let mut footprints = 0;
    let mut bytes = 0;
    let mut edges = 0;
    for row in operations {
        check_path(
            &row.occurrence.session,
            session,
            &row.occurrence.path,
            &mut paths,
            limits,
        )?;
        add_count(&mut sources, 1, limits.source_entries)?;
        if let NativeOperationSource::Consume(source) = &row.source {
            add_count(&mut sources, source.channels.len(), limits.source_entries)?;
        }
        if let Some(peeks) = &row.consume_peeks {
            add_count(&mut sources, peeks.len(), limits.source_entries)?;
            work(
                host,
                HostWorkDimension::VerificationOperations,
                peeks.len().saturating_mul(4).saturating_add(2),
            )?;
            work(
                host,
                HostWorkDimension::VerificationBytes,
                peeks.len().saturating_mul(8),
            )?;
        }
        if let Some(comm) = &row.comm {
            for count in [
                1,
                comm.source.consume.channels.len(),
                comm.source.produces.len(),
                comm.source.peeks.len(),
                comm.source.repetitions.len(),
            ] {
                add_count(&mut sources, count, limits.source_entries)?;
            }
        }
        add_count(&mut edges, row.predecessors.len(), limits.predecessor_edges)?;
        add_count(
            &mut footprints,
            row.footprint.len(),
            limits.footprint_entries,
        )?;
        work(
            host,
            HostWorkDimension::VerificationOperations,
            row.footprint.len(),
        )?;
        for channel in row.footprint.iter() {
            add_count(&mut bytes, channel.len(), limits.footprint_bytes)?;
        }
    }
    Ok(publications)
}

fn insert_unique<'a>(
    index: &mut NativeIndex<JournalKey<'a>, ()>,
    key: JournalKey<'a>,
    host: &HostWorkBudget,
) -> Result<(), NativeOperationJournalError> {
    if index.get(&key, host)?.is_some() {
        return Err(NativeOperationJournalError::Duplicate);
    }
    let prepared = index.prepare_insert(key, (), host)?;
    index.commit(prepared);
    Ok(())
}

struct LinkDecision {
    start: usize,
    end: usize,
    granted: bool,
}

fn check_link(
    row: &NativeOperationRecord,
    link: NativeObservationLink,
    stage: NativeAttemptStage,
    recording: &NativeBudgetRecording,
    owned: &mut [bool],
    host: &HostWorkBudget,
) -> Result<LinkDecision, NativeOperationJournalError> {
    let (occurrence, start, end, granted, slot) = match link {
        NativeObservationLink::Attempt(index) => {
            let attempt = recording
                .attempts
                .get(index)
                .ok_or(NativeOperationJournalError::Ownership)?;
            (
                &attempt.occurrence,
                index,
                index + 1,
                attempt.granted,
                index,
            )
        }
        NativeObservationLink::Retry(index) => {
            let retry = recording
                .retries
                .get(index)
                .ok_or(NativeOperationJournalError::Ownership)?;
            (
                &retry.occurrence,
                retry.fresh_before,
                retry.fresh_before,
                true,
                recording.attempts.len() + index,
            )
        }
    };
    work(
        host,
        HostWorkDimension::VerificationOperations,
        occurrence.path.len().saturating_mul(2).saturating_add(10),
    )?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        occurrence.path.len().saturating_mul(16).saturating_add(32),
    )?;
    if occurrence.session != row.occurrence.session
        || occurrence.path.as_slice() != row.occurrence.path.as_ref()
    {
        return Err(NativeOperationJournalError::Ownership);
    }
    if occurrence.stage != stage {
        return Err(NativeOperationJournalError::Stage);
    }
    if start < row.budget_start || end > row.budget_end {
        return Err(NativeOperationJournalError::Interval);
    }
    if std::mem::replace(&mut owned[slot], true) {
        return Err(NativeOperationJournalError::Ownership);
    }
    Ok(LinkDecision {
        start,
        end,
        granted,
    })
}

fn check_dependencies(
    position: usize,
    rows: &[NativeOperationRecord],
    last: &mut NativeIndex<Arc<[u8]>, usize>,
    host: &HostWorkBudget,
) -> Result<(), NativeOperationJournalError> {
    let row = &rows[position];
    work(
        host,
        HostWorkDimension::VerificationOperations,
        row.footprint
            .len()
            .saturating_add(row.predecessors.len())
            .saturating_add(1),
    )?;
    for pair in row.footprint.windows(2) {
        work(
            host,
            HostWorkDimension::VerificationBytes,
            pair[0].len().min(pair[1].len()).saturating_add(1),
        )?;
        if pair[0] >= pair[1] {
            return Err(NativeOperationJournalError::Footprint);
        }
    }
    if row.predecessors.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(NativeOperationJournalError::Dependency);
    }
    let mut expected = allocate(row.footprint.len(), host)?;
    for channel in row.footprint.iter() {
        if let Some(prior) = last.get(channel, host)? {
            if rows[*prior].budget_end > row.budget_start {
                return Err(NativeOperationJournalError::Dependency);
            }
            expected.push(*prior);
        }
    }
    sort(&mut expected, |a, b| {
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        Ok(a.cmp(b))
    })?;
    work(
        host,
        HostWorkDimension::VerificationOperations,
        expected.len().saturating_mul(2),
    )?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        expected.len().saturating_mul(size_of::<usize>()),
    )?;
    expected.dedup();
    if expected.as_slice() != row.predecessors.as_ref() {
        return Err(NativeOperationJournalError::Dependency);
    }
    let mut updates = allocate(row.footprint.len(), host)?;
    updates.extend(
        row.footprint
            .iter()
            .map(|channel| (Arc::clone(channel), position)),
    );
    let prepared = last.prepare_batch(updates, host)?;
    last.commit_batch(prepared);
    Ok(())
}

impl NativePhloExecutionContract<'_> {
    pub fn check_operation_journal(
        &self,
        session: [u8; 32],
        recording: &NativeBudgetRecording,
        operations: Arc<[NativeOperationRecord]>,
        limits: NativeOperationJournalLimits,
        host: &HostWorkBudget,
    ) -> Result<CheckedNativeOperationJournal, NativeOperationJournalError> {
        let publication_count = check_sizes(&session, recording, &operations, limits, host)?;
        let budget = self.check_budget_evidence(
            session,
            Arc::clone(&recording.attempts),
            limits.budget,
            host,
        )?;
        if recording.used != budget.total() {
            return Err(NativeOperationJournalError::Usage);
        }
        let mut occurrences = NativeIndex::default();
        for occurrence in recording
            .attempts
            .iter()
            .map(|row| &row.occurrence)
            .chain(recording.retries.iter().map(|row| &row.occurrence))
        {
            insert_unique(
                &mut occurrences,
                JournalKey {
                    session: &occurrence.session,
                    path: &occurrence.path,
                    stage: Some(occurrence.stage),
                },
                host,
            )?;
        }
        for retry in recording.retries.iter() {
            work(host, HostWorkDimension::VerificationOperations, 6)?;
            let accepted = recording
                .attempts
                .get(retry.accepted_attempt)
                .ok_or(NativeOperationJournalError::Retry)?;
            if !accepted.granted
                || retry.accepted_attempt >= retry.fresh_before
                || retry.fresh_before > recording.attempts.len()
                || retry.occurrence.stage != accepted.occurrence.stage
                || retry.occurrence.stage != NativeAttemptStage::from(retry.observation.kind)
            {
                return Err(NativeOperationJournalError::Retry);
            }
            self.prepare(Arc::clone(&retry.observation), limits.budget.regions, host)
                .map_err(NativeBudgetTraceError::from)?;
            let bytes = observation_comparison_bytes(&retry.observation, host)?
                .checked_add(observation_comparison_bytes(&accepted.observation, host)?)
                .ok_or(NativeOperationJournalError::Limit)?;
            work(host, HostWorkDimension::VerificationBytes, bytes)?;
            if retry.observation != accepted.observation {
                return Err(NativeOperationJournalError::Retry);
            }
        }
        let mut owned = allocate(publication_count, host)?;
        owned.resize(publication_count, false);
        let mut operation_ids = NativeIndex::default();
        let mut last = NativeIndex::default();
        let mut previous_start = 0;
        for (position, row) in operations.iter().enumerate() {
            match (&row.source, &row.consume_peeks) {
                (NativeOperationSource::Produce(_), None) => {}
                (NativeOperationSource::Consume(source), Some(peeks)) => {
                    if peeks.windows(2).any(|pair| pair[0] >= pair[1])
                        || peeks.iter().any(|index| {
                            usize::try_from(*index)
                                .map_or(true, |index| index >= source.channels.len())
                        })
                        || row
                            .comm
                            .as_ref()
                            .is_some_and(|comm| comm.source.peeks != *peeks)
                    {
                        return Err(NativeOperationJournalError::Peeks);
                    }
                }
                _ => return Err(NativeOperationJournalError::Peeks),
            }
            insert_unique(
                &mut operation_ids,
                JournalKey {
                    session: &row.occurrence.session,
                    path: &row.occurrence.path,
                    stage: None,
                },
                host,
            )?;
            if row.budget_start < previous_start
                || row.budget_start > row.budget_end
                || row.budget_end > recording.attempts.len()
            {
                return Err(NativeOperationJournalError::Interval);
            }
            previous_start = row.budget_start;
            let stage = match row.source {
                NativeOperationSource::Produce(_) => NativeAttemptStage::ProduceIntroduction,
                NativeOperationSource::Consume(_) => NativeAttemptStage::ConsumeIntroduction,
            };
            let intro = check_link(row, row.introduction, stage, recording, &mut owned, host)?;
            let comm = row
                .comm
                .as_ref()
                .map(|comm| {
                    check_link(
                        row,
                        comm.observation,
                        NativeAttemptStage::Comm,
                        recording,
                        &mut owned,
                        host,
                    )
                })
                .transpose()?;
            if comm.as_ref().is_some_and(|comm| intro.end > comm.start) {
                return Err(NativeOperationJournalError::Stage);
            }
            let valid = matches!(
                (
                    intro.granted,
                    comm.as_ref().map(|comm| comm.granted),
                    row.completion
                ),
                (true, None, RSpaceOperationCompletion::Stored)
                    | (true, Some(true), RSpaceOperationCompletion::Matched)
                    | (false, None, RSpaceOperationCompletion::Rejected)
                    | (true, Some(false), RSpaceOperationCompletion::Rejected)
            );
            if !valid {
                return Err(NativeOperationJournalError::Lifecycle);
            }
            check_dependencies(position, &operations, &mut last, host)?;
        }
        work(
            host,
            HostWorkDimension::VerificationOperations,
            publication_count,
        )?;
        if owned.iter().any(|owned| !owned) {
            return Err(NativeOperationJournalError::Ownership);
        }
        drop(operation_ids);
        Ok(CheckedNativeOperationJournal {
            recording: recording.clone(),
            operations,
            budget,
        })
    }
}

#[cfg(test)]
#[path = "tests/checked_operations.rs"]
mod tests;
