use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use thiserror::Error;

use super::index::{IndexKey, NativeIndex};
use super::operation_sources::allocate;
use super::operations::sort;
use super::path_trie::{NativePathTrie, PathId};
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

    /// Added by DR-113: the bound source of the checked budget evidence.
    pub fn bound_source(
        &self,
    ) -> crate::rust::interpreter::accounting::native_phlo_rules::NativeBoundSource {
        self.budget.bound_source()
    }
    pub fn operation_count(&self) -> usize { self.operations.len() }
    pub fn attempt_count(&self) -> usize { self.recording.attempts.len() }
    pub fn retry_count(&self) -> usize { self.recording.retries.len() }
}

// Changed by C7b (DR-86): a journal key names the trie node of its path, so
// a comparison reads the session, the node id and the stage. Equal ids mean
// equal paths (`NativePathTrie.node_identity_is_path_equality`).
// #[derive(PartialEq, Eq, PartialOrd, Ord)]
// struct JournalKey<'a> {
//     session: &'a [u8; 32],
//     path: &'a [(u64, u64)],
//     stage: Option<NativeAttemptStage>,
// }
//
// impl IndexKey for JournalKey<'_> {
//     fn comparison_work(&self) -> Result<(usize, usize), InterpreterError> {
//         let operations = self
//             .path
//             .len()
//             .checked_mul(2)
//             .and_then(|n| n.checked_add(4));
//         let bytes = self
//             .path
//             .len()
//             .checked_mul(16)
//             .and_then(|n| n.checked_add(34));
//         Ok((
//             operations.ok_or(InterpreterError::HostWorkRejected)?,
//             bytes.ok_or(InterpreterError::HostWorkRejected)?,
//         ))
//     }
// }
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct JournalKey<'a> {
    session: &'a [u8; 32],
    path: PathId,
    stage: Option<NativeAttemptStage>,
}

const JOURNAL_KEY_COMPARISON_BYTES: usize = 32 + size_of::<PathId>() + 2;

impl IndexKey for JournalKey<'_> {
    fn comparison_work(&self) -> Result<(usize, usize), InterpreterError> {
        Ok((4, JOURNAL_KEY_COMPARISON_BYTES))
    }
}

fn work_count(count: usize, scale: usize, extra: usize) -> Result<usize, InterpreterError> {
    count
        .checked_mul(scale)
        .and_then(|total| total.checked_add(extra))
        .ok_or(InterpreterError::HostWorkRejected)
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

// Changed by C7b (DR-86): an occurrence names a node of the recording's path
// trie. check_sizes limits the node count, which is at most the number of new
// suffix segments (`NativePathTrie.trie_nodes_bounded_by_suffixes`), instead
// of the sum of the path depths.
// fn check_path(
//     session: &[u8; 32],
//     expected: &[u8; 32],
//     path: &[(u64, u64)],
//     total: &mut usize,
//     limits: NativeOperationJournalLimits,
// ) -> Result<(), NativeOperationJournalError> {
//     if session != expected {
//         return Err(NativeOperationJournalError::Session);
//     }
//     if path.len() > limits.budget.path_segments {
//         return Err(NativeOperationJournalError::Limit);
//     }
//     add_count(total, path.len(), limits.total_path_segments)
// }
fn check_path(
    session: &[u8; 32],
    expected: &[u8; 32],
    path: PathId,
    paths: &NativePathTrie,
    limits: NativeOperationJournalLimits,
) -> Result<(), NativeOperationJournalError> {
    if session != expected {
        return Err(NativeOperationJournalError::Session);
    }
    if !paths.contains(path) || path.depth() > limits.budget.path_segments {
        return Err(NativeOperationJournalError::Limit);
    }
    Ok(())
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
        work_count(count, 32, 1)?,
    )?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        work_count(count, 32, 0)?,
    )?;
    if recording.paths.node_count() > limits.total_path_segments {
        return Err(NativeOperationJournalError::Limit);
    }
    for occurrence in recording
        .attempts
        .iter()
        .map(|row| &row.occurrence)
        .chain(recording.retries.iter().map(|row| &row.occurrence))
    {
        check_path(
            &occurrence.session,
            session,
            occurrence.path,
            &recording.paths,
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
            row.occurrence.path,
            &recording.paths,
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
                work_count(peeks.len(), 4, 2)?,
            )?;
            work(
                host,
                HostWorkDimension::VerificationBytes,
                work_count(peeks.len(), 8, 0)?,
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
    // Changed by C7b (DR-86): the link compares the session and the path ids.
    // work(
    //     host,
    //     HostWorkDimension::VerificationOperations,
    //     work_count(occurrence.path.len(), 2, 10)?,
    // )?;
    // work(
    //     host,
    //     HostWorkDimension::VerificationBytes,
    //     work_count(occurrence.path.len(), 16, 32)?,
    // )?;
    work(host, HostWorkDimension::VerificationOperations, 10)?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        32 + size_of::<PathId>(),
    )?;
    if occurrence.session != row.occurrence.session || occurrence.path != row.occurrence.path {
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
            .checked_add(row.predecessors.len())
            .and_then(|count| count.checked_add(1))
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    for pair in row.footprint.windows(2) {
        work(
            host,
            HostWorkDimension::VerificationBytes,
            pair[0]
                .len()
                .min(pair[1].len())
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
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
        work_count(expected.len(), 2, 0)?,
    )?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        work_count(expected.len(), size_of::<usize>(), 0)?,
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
                    path: occurrence.path,
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
                    path: row.occurrence.path,
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
