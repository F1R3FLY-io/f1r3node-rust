// Added by G3 (DR-135, S4): the journal check maps peek COMMs to their
// restorations.
// Changed by G3 (DR-135, S4 review): the restorations come from the COMM
// attempts.
// use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;
use thiserror::Error;

use super::index::NativeIndex;
use super::operations::sort;
use super::path_trie::{NativePathTrie, PathId};
use super::{
    InterpreterError, NativeBudgetRecording, NativeObservationLink, NativeOperationRecord,
    NativeOperationSource,
};
// Added by G3 (DR-135, S4): the owner table and the restorations of a peek.
use crate::rust::interpreter::accounting::byte_accounting::produce_introduction_identity_of_hash;
// Changed by G3 (DR-135, S4): the fold takes the owner terms.
// use crate::rust::interpreter::accounting::native_phlo_rules::{
//     CheckedNativeBudgetEvidence, NativeAttemptStage, NativeBudgetTraceError,
//     NativeBudgetTraceLimits, NativePhloExecutionContract,
// };
use crate::rust::interpreter::accounting::native_phlo_rules::{
    CheckedNativeBudgetEvidence, NativeAttemptStage, NativeBudgetTraceError,
    NativeBudgetTraceLimits, NativePhloExecutionContract, OwnerFoldInput,
};
// Added by G3 (DR-135, S4).
// Changed by G3 (DR-135, S7): the table of a recording comes from
// `recorded_owner_table`, and the journal reads the owner totals.
// use crate::rust::interpreter::accounting::owner_funding::{
//     OwnerFundingBinding, OwnerPurseSet, OwnerTable, OwnerTableEntry,
// };
use crate::rust::interpreter::accounting::owner_funding::{
    recorded_owner_table, OwnerFundingBinding, OwnerTable, OwnerTableEntry, OwnerTotals,
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
    Interpreter(#[from] InterpreterError),
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
    /// Added by G3 (DR-135, S4).
    #[error("native owner table differs from the owner funding of the member")]
    OwnerTable,
    /// Added by G3 (DR-135, S4).
    #[error("native operation journal contains an invalid COMM refusal")]
    Refusal,
    /// Added by G3 (DR-135, S4 review).
    #[error("native COMM attempt records restorations that differ from its peek")]
    Restoration,
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
    /// Added by G3 (DR-135, S4 review): the checked budget evidence, with the
    /// owner fold of a member with owner funding.
    pub(in crate::rust::interpreter::accounting) fn budget_evidence(
        &self,
    ) -> &CheckedNativeBudgetEvidence {
        &self.budget
    }
    pub fn attempt_count(&self) -> usize { self.recording.attempts.len() }
    pub fn retry_count(&self) -> usize { self.recording.retries.len() }

    /// Added by G3 (DR-135, S7): the billed owner sums that the fold of a
    /// member with owner funding rebuilt (G-4), or `None` for a journal
    /// checked without owner funding. Play and replay read them from the same
    /// fold of the recording.
    pub fn owner_totals(&self) -> Option<OwnerTotals> {
        let ledger = self.budget.owner()?.ledger();
        Some(OwnerTotals {
            debits: ledger.debits().0.clone(),
            usage: ledger.usage(),
            rev: ledger.rev(),
            table: ledger.table().clone(),
        })
    }
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
        }
        // Changed by G3 (DR-135, S4): each refused COMM counts like a COMM
        // (R7).
        // if let Some(comm) = &row.comm {
        //     for count in [
        //         1,
        //         comm.source.consume.channels.len(),
        //         comm.source.produces.len(),
        //         comm.source.peeks.len(),
        //         comm.source.repetitions.len(),
        //     ] {
        //         add_count(&mut sources, count, limits.source_entries)?;
        //     }
        // }
        for source in row
            .comm
            .iter()
            .map(|comm| comm.source.as_ref())
            .chain(row.refusals.iter().map(|refusal| refusal.source.as_ref()))
        {
            for count in [
                1,
                source.consume.channels.len(),
                source.produces.len(),
                source.peeks.len(),
                source.repetitions.len(),
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
        for channel in row.footprint.iter() {
            add_count(&mut bytes, channel.len(), limits.footprint_bytes)?;
        }
    }
    Ok(publications)
}

fn insert_unique<'a>(
    index: &mut NativeIndex<JournalKey<'a>, ()>,
    key: JournalKey<'a>,
) -> Result<(), NativeOperationJournalError> {
    if index.get(&key).is_some() {
        return Err(NativeOperationJournalError::Duplicate);
    }
    let prepared = index.prepare_insert(key, ())?;
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
) -> Result<(), NativeOperationJournalError> {
    let row = &rows[position];
    for pair in row.footprint.windows(2) {
        if pair[0] >= pair[1] {
            return Err(NativeOperationJournalError::Footprint);
        }
    }
    if row.predecessors.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(NativeOperationJournalError::Dependency);
    }
    let mut expected = Vec::with_capacity(row.footprint.len());
    for channel in row.footprint.iter() {
        if let Some(prior) = last.get(channel) {
            if rows[*prior].budget_end > row.budget_start {
                return Err(NativeOperationJournalError::Dependency);
            }
            expected.push(*prior);
        }
    }
    sort(&mut expected, |a, b| Ok(a.cmp(b)))?;
    expected.dedup();
    if expected.as_slice() != row.predecessors.as_ref() {
        return Err(NativeOperationJournalError::Dependency);
    }
    let mut updates = Vec::with_capacity(row.footprint.len());
    updates.extend(
        row.footprint
            .iter()
            .map(|channel| (Arc::clone(channel), position)),
    );
    let prepared = last.prepare_batch(updates)?;
    last.commit_batch(prepared);
    Ok(())
}

/// Added by G3 (DR-135, S4): the owner table of a recording, validated
/// against the binding of the member (R3): every entry locates a purse with
/// its signature, the custody keys ascend strictly, and the table fits the
/// room of the binding.
// Changed by G3 (DR-135, S7): the shared `recorded_owner_table` builds the
// table, so Casper builds the binding of a replay in the same way.
// fn checked_owner_table(
//     binding: &OwnerFundingBinding,
//     entries: &[OwnerTableEntry],
// ) -> Result<OwnerTable, NativeOperationJournalError> {
//     let mut keyed = Vec::with_capacity(entries.len());
//     for entry in entries {
//         let custody = match binding.locate(&entry.signature) {
//             Some(OwnerPurseSet::Single(purse)) if purse.signature == entry.signature => {
//                 purse.custody
//             }
//             Some(OwnerPurseSet::Compound { vault, .. }) if vault.signature == entry.signature => {
//                 vault.custody
//             }
//             _ => return Err(NativeOperationJournalError::OwnerTable),
//         };
//         keyed.push((custody, entry.clone()));
//     }
//     OwnerTable::new(keyed, binding.table().room())
//         .map_err(|_| NativeOperationJournalError::OwnerTable)
// }
fn checked_owner_table(
    binding: &OwnerFundingBinding,
    entries: &[OwnerTableEntry],
) -> Result<OwnerTable, NativeOperationJournalError> {
    recorded_owner_table(
        entries,
        &|signature| binding.locate(signature),
        binding.table().room(),
    )
    .map_err(|_| NativeOperationJournalError::OwnerTable)
}

// Changed by G3 (DR-135, S4 review): the restorations of a peek COMM come
// from its attempt, so the journal no longer maps a COMM to later
// introductions. A restoration that never runs then stays paid at its COMM
// in play and in the fold (§6.3 item 4). The replaced function follows.
// /// Added by G3 (DR-135, S4): the restoration attempts of each granted peek
// /// COMM attempt (R9). Each non-persistent datum of the COMM maps to the
// /// first unclaimed produce introduction after the COMM with the identity of
// /// its produce hash. Play reserves a restoration at the COMM and takes the
// /// reservation at the next introduction with that identity, so both claim
// /// the same attempts.
// fn peek_restorations(
//     recording: &NativeBudgetRecording,
//     operations: &[NativeOperationRecord],
// ) -> BTreeMap<usize, Vec<usize>> {
//     let mut candidates = BTreeMap::<[u8; 32], BTreeSet<usize>>::new();
//     for (index, attempt) in recording.attempts.iter().enumerate() {
//         if attempt.occurrence.stage == NativeAttemptStage::ProduceIntroduction {
//             candidates
//                 .entry(attempt.observation.event_id)
//                 .or_default()
//                 .insert(index);
//         }
//     }
//     let mut comms = Vec::with_capacity(operations.len());
//     for row in operations {
//         if let Some(comm) = row.comm.as_ref() {
//             if let NativeObservationLink::Attempt(index) = comm.observation {
//                 if !comm.source.peeks.is_empty()
//                     && recording
//                         .attempts
//                         .get(index)
//                         .is_some_and(|attempt| attempt.granted)
//                 {
//                     comms.push((index, comm.source.as_ref()));
//                 }
//             }
//         }
//     }
//     comms.sort_unstable_by_key(|(index, _)| *index);
//     let mut restorations = BTreeMap::new();
//     for (index, source) in comms {
//         let mut claimed = Vec::with_capacity(source.produces.len());
//         for produce in source.produces.iter().filter(|produce| !produce.persistent) {
//             let identity = produce_introduction_identity_of_hash(&produce.hash);
//             let next = candidates
//                 .get(&identity)
//                 .and_then(|attempts| attempts.range(index + 1..).next().copied());
//             if let (Some(next), Some(attempts)) = (next, candidates.get_mut(&identity)) {
//                 attempts.remove(&next);
//                 claimed.push(next);
//             }
//         }
//         if !claimed.is_empty() {
//             restorations.insert(index, claimed);
//         }
//     }
//     restorations
// }

/// Added by G3 (DR-135, S4 review): the restorations that a COMM attempt
/// records (R9). With owner funding, the attempt of a peek COMM records the
/// introduction of each non-persistent datum of the COMM, in COMM order,
/// with the identity of its produce hash. Every other COMM attempt records
/// none. A peek is all or nothing, because the reducer peeks every channel
/// of a peek receive (`consume_inner`), so each non-persistent datum is
/// restored. Replay checks the full observations against the live data
/// (S6).
fn check_restorations(
    row: &NativeOperationRecord,
    recording: &NativeBudgetRecording,
    owned: bool,
) -> Result<(), NativeOperationJournalError> {
    let Some(comm) = row.comm.as_ref() else {
        return Ok(());
    };
    let NativeObservationLink::Attempt(index) = comm.observation else {
        return Ok(());
    };
    let attempt = recording
        .attempts
        .get(index)
        .ok_or(NativeOperationJournalError::Ownership)?;
    let restored = comm
        .source
        .produces
        .iter()
        .filter(|produce| owned && !comm.source.peeks.is_empty() && !produce.persistent)
        .map(|produce| produce_introduction_identity_of_hash(&produce.hash));
    if !attempt
        .restorations
        .iter()
        .map(|restoration| restoration.event_id)
        .eq(restored)
    {
        return Err(NativeOperationJournalError::Restoration);
    }
    Ok(())
}

/// Added by G3 (DR-135, S4): the checks of the refused COMMs of a row (R6,
/// R7). A refusal lies in the budget interval of the row, before its COMM,
/// and in order. Each consume source is refused once. A consume row is
/// refused at most once and stays stored. Each refusal of a produce row
/// holds the produce of the row.
fn check_refusals(
    row: &NativeOperationRecord,
    comm: Option<&LinkDecision>,
) -> Result<(), NativeOperationJournalError> {
    if row.refusals.is_empty() {
        return Ok(());
    }
    match (&row.source, row.completion) {
        (NativeOperationSource::Consume(_), RSpaceOperationCompletion::Stored) => {
            if row.refusals.len() > 1 {
                return Err(NativeOperationJournalError::Refusal);
            }
        }
        (
            NativeOperationSource::Produce(produce),
            RSpaceOperationCompletion::Stored | RSpaceOperationCompletion::Matched,
        ) => {
            if row
                .refusals
                .iter()
                .any(|refusal| !refusal.source.produces.contains(produce))
            {
                return Err(NativeOperationJournalError::Refusal);
            }
        }
        _ => return Err(NativeOperationJournalError::Lifecycle),
    }
    let mut previous = row.budget_start;
    let mut consumes = Vec::with_capacity(row.refusals.len());
    for refusal in row.refusals.iter() {
        if refusal.position < previous
            || refusal.position > row.budget_end
            || comm.is_some_and(|comm| refusal.position > comm.start)
        {
            return Err(NativeOperationJournalError::Refusal);
        }
        previous = refusal.position;
        consumes.push(&refusal.source.consume);
    }
    consumes.sort_unstable();
    if consumes.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(NativeOperationJournalError::Refusal);
    }
    Ok(())
}

impl NativePhloExecutionContract<'_> {
    pub fn check_operation_journal(
        &self,
        session: [u8; 32],
        recording: &NativeBudgetRecording,
        operations: Arc<[NativeOperationRecord]>,
        limits: NativeOperationJournalLimits,
    ) -> Result<CheckedNativeOperationJournal, NativeOperationJournalError> {
        // Changed by G3 (DR-135, S4): the check of a member without owner
        // funding.
        self.check_operation_journal_with_owners(session, recording, operations, limits, None)
    }

    /// Added by G3 (DR-135, S4): `check_operation_journal` for a member with
    /// the owner funding `binding` (R3, R7, R8, R9). Without a binding the
    /// recording must hold no owner evidence.
    pub fn check_operation_journal_with_owners(
        &self,
        session: [u8; 32],
        recording: &NativeBudgetRecording,
        operations: Arc<[NativeOperationRecord]>,
        limits: NativeOperationJournalLimits,
        binding: Option<&OwnerFundingBinding>,
    ) -> Result<CheckedNativeOperationJournal, NativeOperationJournalError> {
        let publication_count = check_sizes(&session, recording, &operations, limits)?;
        let owners = match binding {
            Some(binding) => Some(OwnerFoldInput {
                binding,
                table: checked_owner_table(binding, &recording.owner_table)?,
            }),
            None if recording.owner_table.is_empty()
                && operations.iter().all(|row| row.refusals.is_empty()) =>
            {
                None
            }
            None => return Err(NativeOperationJournalError::OwnerTable),
        };
        let budget = self.check_budget_evidence(
            session,
            Arc::clone(&recording.attempts),
            Arc::clone(&recording.transfers),
            limits.budget,
            owners,
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
            insert_unique(&mut occurrences, JournalKey {
                session: &occurrence.session,
                path: occurrence.path,
                stage: Some(occurrence.stage),
            })?;
        }
        for retry in recording.retries.iter() {
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
            self.prepare(Arc::clone(&retry.observation), limits.budget.regions)
                .map_err(NativeBudgetTraceError::from)?;
            if retry.observation != accepted.observation {
                return Err(NativeOperationJournalError::Retry);
            }
        }
        let mut owned = vec![false; publication_count];
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
            insert_unique(&mut operation_ids, JournalKey {
                session: &row.occurrence.session,
                path: row.occurrence.path,
                stage: None,
            })?;
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
            let intro = check_link(row, row.introduction, stage, recording, &mut owned)?;
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
            // Added by G3 (DR-135, S4): the refused COMMs of the row (R7).
            check_refusals(row, comm.as_ref())?;
            // Added by G3 (DR-135, S4 review): the restorations of its COMM.
            check_restorations(row, recording, binding.is_some())?;
            check_dependencies(position, &operations, &mut last)?;
        }
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
