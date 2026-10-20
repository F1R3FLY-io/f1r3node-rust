use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use models::rhoapi::Par;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::COMM;
use serde::Serialize;

use super::index::NativeIndex;
use super::operation_sources::{channel_bytes, NativeCommSource, NativeOperationSource};
use super::path_trie::{NativePathTrie, PathId};
use super::recording::{recording_error, RecordedOccurrence};
use super::{InterpreterError, NativeRuntimeConfig, RuntimeBudget};
use crate::rust::interpreter::accounting::native_phlo_rules::NativeAttemptStage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeOperationOccurrence {
    pub session: [u8; 32],
    // Changed by C7b (DR-86): a journal row names the node of its path in the
    // recording's path trie instead of holding a copy of the path.
    // pub path: Arc<[(u64, u64)]>,
    pub path: PathId,
}

/// The occurrence of an operation that the producer is recording: the
/// session and a copy of the causal path (C7b, DR-86).
struct PendingOccurrence {
    session: [u8; 32],
    path: Arc<[(u64, u64)]>,
}

/// The path trie of captured evidence and the node of each recorded path, in
/// the wire order of the evidence (C7b, DR-86).
pub(super) struct CapturedPaths {
    pub(super) trie: NativePathTrie,
    pub(super) attempts: Vec<PathId>,
    pub(super) retries: Vec<PathId>,
    pub(super) rows: Vec<PathId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeObservationLink {
    Attempt(usize),
    Retry(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCommRecord {
    pub source: Arc<NativeCommSource>,
    pub observation: NativeObservationLink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeOperationRecord {
    pub occurrence: NativeOperationOccurrence,
    pub source: NativeOperationSource,
    pub consume_peeks: Option<Arc<[i32]>>,
    pub footprint: Arc<[Arc<[u8]>]>,
    pub predecessors: Arc<[usize]>,
    pub introduction: NativeObservationLink,
    pub comm: Option<NativeCommRecord>,
    pub completion: RSpaceOperationCompletion,
    pub budget_start: usize,
    pub budget_end: usize,
}

#[derive(Clone)]
struct OperationKey(OperationOrder);

impl PartialEq for OperationKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.session == other.0.session
            && self.0.path.len() == other.0.path.len()
            && self.0.path.segments_rev().eq(other.0.path.segments_rev())
    }
}

impl Eq for OperationKey {}

impl PartialOrd for OperationKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for OperationKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .session
            .cmp(&other.0.session)
            .then_with(|| self.0.path.len().cmp(&other.0.path.len()))
            .then_with(|| self.0.path.segments_rev().cmp(other.0.path.segments_rev()))
    }
}

#[derive(Clone, Copy)]
struct PublishedObservation {
    link: NativeObservationLink,
    granted: bool,
}

struct PendingOperation {
    occurrence: PendingOccurrence,
    source: NativeOperationSource,
    consume_peeks: Option<Arc<[i32]>>,
    footprint: Arc<[Arc<[u8]>]>,
    predecessors: Arc<[usize]>,
    introduction: Option<PublishedObservation>,
    comm_source: Option<Arc<NativeCommSource>>,
    comm: Option<PublishedObservation>,
    completion: Option<RSpaceOperationCompletion>,
    budget_start: usize,
    budget_end: usize,
}

#[derive(Default)]
pub(super) struct NativeOperationRecorder {
    rows: Vec<PendingOperation>,
    index: NativeIndex<OperationKey, usize>,
    last_channel: NativeIndex<Arc<[u8]>, usize>,
}

pub(super) fn sort<T>(
    values: &mut [T],
    compare: impl Fn(&T, &T) -> Result<std::cmp::Ordering, InterpreterError>,
) -> Result<(), InterpreterError> {
    shared::rust::fallible_sort::sort(values, compare)
}

pub(super) fn locked_footprint<C: Serialize>(
    channels: &[C],
    joins: &[Vec<C>],
) -> Result<Vec<Arc<[u8]>>, InterpreterError> {
    let count = joins
        .iter()
        .try_fold(channels.len(), |n, group| n.checked_add(group.len()))
        .ok_or_else(|| recording_error("native operation footprint size overflows"))?;
    let mut footprint = Vec::with_capacity(count);
    for channel in channels.iter().chain(joins.iter().flatten()) {
        footprint.push(channel_bytes(channel)?);
    }
    sort(&mut footprint, |a, b| Ok(a.cmp(b)))?;
    footprint.dedup();
    Ok(footprint)
}

impl NativeRuntimeConfig {
    fn operation_key(&self) -> Result<OperationKey, InterpreterError> {
        let order = operation_context::current()
            .ok_or_else(|| recording_error("native operation has no context"))?;
        if order.session != self.session || order.path.len() > self.limits.path_segments {
            return Err(recording_error("native operation context is invalid"));
        }
        Ok(OperationKey(order))
    }

    // Changed by DR-132: the start returns the name of the recorded row.
    // ) -> Result<(), InterpreterError> {
    fn start_operation(
        &mut self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<OperationOrder, InterpreterError> {
        if self.recording.invalid.load(Ordering::Acquire)
            || self.operations.rows.len() >= self.limits.attempts
        {
            return Err(recording_error(
                "native operation recording is invalid or full",
            ));
        }
        // Changed by DR-132 (D1-A): the recorder names each row by its journal
        // index. Racing evaluation sets no outer name. The row index is the
        // start order of play, which the record keeps, and replay finds each
        // row by its source.
        // let key = self.operation_key()?;
        let row_index = u64::try_from(self.operations.rows.len())
            .map_err(|_| recording_error("native operation index overflows"))?;
        let key = OperationKey(OperationOrder {
            session: self.session,
            path: vec![(row_index, 0)].into(),
        });
        let name = key.0.clone();
        if self.operations.index.get(&key).is_some() {
            return Err(recording_error("native operation occurrence is duplicated"));
        }
        let source = NativeOperationSource::capture(source)?;
        let footprint = locked_footprint(channels, joins)?;
        let mut predecessors = Vec::with_capacity(footprint.len());
        for channel in &footprint {
            if let Some(index) = self.operations.last_channel.get(channel) {
                if self.operations.rows[*index].completion.is_none() {
                    return Err(recording_error(
                        "native conflicting operations overlap outside channel guards",
                    ));
                }
                predecessors.push(*index);
            }
        }
        sort(&mut predecessors, |a, b| Ok(a.cmp(b)))?;
        predecessors.dedup();
        let mut path = Vec::with_capacity(key.0.path.len());
        path.extend(key.0.path.segments_rev());
        path.reverse();
        let index = self.operations.rows.len();
        let row = PendingOperation {
            occurrence: PendingOccurrence {
                session: key.0.session,
                path: path.into(),
            },
            source,
            consume_peeks: None,
            footprint: footprint.into(),
            predecessors: predecessors.into(),
            introduction: None,
            comm_source: None,
            comm: None,
            completion: None,
            budget_start: self.recording.attempts.len(),
            budget_end: self.recording.attempts.len(),
        };
        let mut updates = Vec::with_capacity(row.footprint.len());
        updates.extend(
            row.footprint
                .iter()
                .map(|channel| (Arc::clone(channel), index)),
        );
        let channel_updates = self.operations.last_channel.prepare_batch(updates)?;
        let operation = self.operations.index.prepare_insert(key, index)?;
        self.operations.last_channel.commit_batch(channel_updates);
        self.operations.index.commit(operation);
        self.operations.rows.push(row);
        Ok(name)
    }

    fn observe_consume_peeks(&mut self, peeks: &BTreeSet<i32>) -> Result<(), InterpreterError> {
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key)
            .copied()
            .ok_or_else(|| recording_error("native consume has no started operation"))?;
        let row = &self.operations.rows[index];
        let NativeOperationSource::Consume(source) = &row.source else {
            return Err(recording_error("native peek metadata belongs to a consume"));
        };
        if row.completion.is_some() || row.introduction.is_some() || row.consume_peeks.is_some() {
            return Err(recording_error("native peek metadata is out of order"));
        }
        if peeks.iter().any(|index| {
            usize::try_from(*index).map_or(true, |index| index >= source.channels.len())
        }) {
            return Err(recording_error(
                "native peek index is outside the consume channels",
            ));
        }
        let mut captured = Vec::with_capacity(peeks.len());
        captured.extend(peeks.iter().copied());
        self.operations.rows[index].consume_peeks = Some(captured.into());
        Ok(())
    }

    fn observe_comm_source(&mut self, source: &COMM) -> Result<(), InterpreterError> {
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key)
            .copied()
            .ok_or_else(|| recording_error("native COMM has no started operation"))?;
        let row = &self.operations.rows[index];
        if row.completion.is_some()
            || row.comm_source.is_some()
            || !row.introduction.is_some_and(|intro| intro.granted)
        {
            return Err(recording_error("native COMM observation is out of order"));
        }
        let source = Arc::new(NativeCommSource::capture(source)?);
        self.operations.rows[index].comm_source = Some(source);
        Ok(())
    }

    pub(super) fn operation_stage_slot(
        &self,
        occurrence: &RecordedOccurrence,
    ) -> Result<Option<usize>, InterpreterError> {
        if self.operations.rows.is_empty() {
            return Ok(None);
        }
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key)
            .copied()
            .ok_or_else(|| recording_error("native charge has no started operation"))?;
        let row = &self.operations.rows[index];
        if row.completion.is_some()
            || row.occurrence.session != occurrence.session
            || row.occurrence.path.as_ref() != occurrence.path.as_slice()
        {
            return Err(recording_error(
                "native charge differs from its active operation",
            ));
        }
        let valid = match occurrence.stage {
            NativeAttemptStage::ProduceIntroduction => {
                matches!(row.source, NativeOperationSource::Produce(_))
                    && row.introduction.is_none()
            }
            NativeAttemptStage::ConsumeIntroduction => {
                matches!(row.source, NativeOperationSource::Consume(_))
                    && row.introduction.is_none()
                    && row.consume_peeks.is_some()
            }
            NativeAttemptStage::Comm => {
                row.comm_source.is_some()
                    && row.comm.is_none()
                    && row.introduction.is_some_and(|intro| intro.granted)
            }
        };
        if !valid {
            return Err(recording_error("native charge stage is out of order"));
        }
        Ok(Some(index))
    }

    pub(super) fn publish_operation_link(
        &mut self,
        slot: Option<usize>,
        stage: NativeAttemptStage,
        link: NativeObservationLink,
        granted: bool,
    ) {
        if let Some(index) = slot {
            let row = &mut self.operations.rows[index];
            let target = if stage == NativeAttemptStage::Comm {
                &mut row.comm
            } else {
                &mut row.introduction
            };
            *target = Some(PublishedObservation { link, granted });
        }
    }

    fn finish_operation(
        &mut self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) -> bool {
        let Some(order) = operation_context::current() else {
            return false;
        };
        if order.session != self.session || order.path.len() > self.limits.path_segments {
            return false;
        }
        let Some(index) = self.operations.index.get(&OperationKey(order)).copied() else {
            return false;
        };
        let row = &mut self.operations.rows[index];
        if row.completion.is_some() || !row.source.matches(source) {
            return false;
        }
        let Some(intro) = row.introduction else {
            return false;
        };
        let valid = match completion {
            RSpaceOperationCompletion::Stored => intro.granted && row.comm_source.is_none(),
            RSpaceOperationCompletion::Matched => {
                intro.granted && row.comm.is_some_and(|comm| comm.granted)
            }
            RSpaceOperationCompletion::Rejected => {
                !intro.granted || row.comm.is_some_and(|comm| !comm.granted)
            }
        };
        if !valid || row.comm_source.is_some() != row.comm.is_some() {
            return false;
        }
        row.completion = Some(completion);
        row.budget_end = self.recording.attempts.len();
        true
    }

    /// Builds the path trie of the captured evidence in its wire order: the
    /// attempts, then the retries from the last attempt, then the journal
    /// rows from the empty path. The decoder interns the same paths in the
    /// same order, so it builds the same nodes with the same ids.
    pub(super) fn capture_paths(&self) -> Result<CapturedPaths, InterpreterError> {
        let mut trie = NativePathTrie::new(self.limits.path_segments);
        let mut previous: (&[(u64, u64)], PathId) = (&[], PathId::ROOT);
        let mut attempts = Vec::with_capacity(self.recording.attempts.len());
        for row in &self.recording.attempts {
            let id = trie.intern_delta(&row.occurrence.path, previous.0, previous.1)?;
            attempts.push(id);
            previous = (&row.occurrence.path, id);
        }
        let mut retries = Vec::with_capacity(self.recording.retries.len());
        for row in &self.recording.retries {
            let id = trie.intern_delta(&row.occurrence.path, previous.0, previous.1)?;
            retries.push(id);
            previous = (&row.occurrence.path, id);
        }
        previous = (&[], PathId::ROOT);
        let mut rows = Vec::with_capacity(self.operations.rows.len());
        for row in &self.operations.rows {
            let id = trie.intern_delta(&row.occurrence.path, previous.0, previous.1)?;
            rows.push(id);
            previous = (&row.occurrence.path, id);
        }
        Ok(CapturedPaths {
            trie,
            attempts,
            retries,
            rows,
        })
    }

    fn capture_operations(&self) -> Result<Arc<[NativeOperationRecord]>, InterpreterError> {
        self.ensure_recording_complete()?;
        let paths = self.capture_paths()?;
        let mut result = Vec::with_capacity(self.operations.rows.len());
        let mut linked = 0usize;
        for (row, path) in self.operations.rows.iter().zip(&paths.rows) {
            let introduction = row
                .introduction
                .ok_or_else(|| recording_error("native operation lacks introduction evidence"))?;
            let completion = row
                .completion
                .ok_or_else(|| recording_error("native operation did not complete"))?;
            linked = linked
                .checked_add(1 + usize::from(row.comm.is_some()))
                .ok_or_else(|| recording_error("native operation evidence count overflows"))?;
            let comm = match (&row.comm_source, row.comm) {
                (Some(source), Some(observation)) => Some(NativeCommRecord {
                    source: Arc::clone(source),
                    observation: observation.link,
                }),
                (None, None) => None,
                _ => return Err(recording_error("native COMM source and observation differ")),
            };
            result.push(NativeOperationRecord {
                occurrence: NativeOperationOccurrence {
                    session: row.occurrence.session,
                    path: *path,
                },
                source: row.source.clone(),
                consume_peeks: row.consume_peeks.clone(),
                footprint: Arc::clone(&row.footprint),
                predecessors: Arc::clone(&row.predecessors),
                introduction: introduction.link,
                comm,
                completion,
                budget_start: row.budget_start,
                budget_end: row.budget_end,
            });
        }
        let publications = self
            .recording
            .attempts
            .len()
            .checked_add(self.recording.retries.len())
            .ok_or_else(|| recording_error("native operation evidence count overflows"))?;
        if linked != publications {
            return Err(recording_error(
                "native operation evidence does not cover all budget publications",
            ));
        }
        Ok(result.into())
    }
}

impl RuntimeBudget {
    // Changed by DR-132: the update returns a value, so that the start can
    // return the name of the recorded row. Without native accounting it
    // returns the default value.
    // update: impl FnOnce(&mut NativeRuntimeConfig) -> Result<(), InterpreterError>,
    // ) -> Result<(), InterpreterError> {
    //     ...
    //     return Ok(());
    fn update_native_operations<T: Default>(
        &self,
        update: impl FnOnce(&mut NativeRuntimeConfig) -> Result<T, InterpreterError>,
    ) -> Result<T, InterpreterError> {
        if !self.has_comm_accounting_scope() || self.is_unmetered() {
            return Ok(T::default());
        }
        let mut state = self.authority_state.lock().expect("authority state");
        let Some(native) = state.native.as_mut() else {
            return Ok(T::default());
        };
        let result = update(native);
        if result.is_err() {
            native.recording.invalid.store(true, Ordering::Release);
        }
        result
    }

    // Changed by DR-132: the start returns the name of the recorded row, or
    // `None` without native accounting.
    // ) -> Result<(), InterpreterError> {
    //     self.update_native_operations(|native| native.start_operation(source, channels, joins))
    pub(crate) fn start_native_operation(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<Option<OperationOrder>, InterpreterError> {
        self.update_native_operations(|native| {
            native.start_operation(source, channels, joins).map(Some)
        })
    }

    pub(crate) fn observe_native_comm_source(&self, source: &COMM) -> Result<(), InterpreterError> {
        self.update_native_operations(|native| native.observe_comm_source(source))
    }

    pub(crate) fn observe_native_consume_peeks(
        &self,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), InterpreterError> {
        self.update_native_operations(|native| native.observe_consume_peeks(peeks))
    }

    pub(crate) fn finish_native_operation(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        if !self.has_comm_accounting_scope() || self.is_unmetered() {
            return;
        }
        let mut state = self.authority_state.lock().expect("authority state");
        if let Some(native) = state.native.as_mut() {
            if !native.finish_operation(source, completion) {
                native.recording.invalid.store(true, Ordering::Release);
            }
        }
    }

    pub fn native_operation_recording(
        &self,
    ) -> Result<Option<Arc<[NativeOperationRecord]>>, InterpreterError> {
        let state = self.authority_state.lock().expect("authority state");
        state
            .native
            .as_ref()
            .map(NativeRuntimeConfig::capture_operations)
            .transpose()
    }
}

#[cfg(test)]
mod operation_index_tests {
    use super::*;

    fn operation_key(path: &[(u64, u64)]) -> OperationKey {
        OperationKey(OperationOrder {
            session: [7; 32],
            path: path.to_vec().into(),
        })
    }

    fn branching_paths() -> Vec<Vec<(u64, u64)>> {
        vec![
            vec![(0, 1), (0, 0)],
            vec![(0, 2)],
            vec![(0, 1)],
            vec![(0, 1), (1, 0)],
            vec![(1, 0)],
            vec![(0, 0), (5, 5)],
            vec![(1, 1)],
            vec![(1, 0), (0, 0)],
            vec![(0, 0), (1, 0)],
            vec![(0, 1), (0, 0), (2, 2)],
            Vec::new(),
        ]
    }

    #[test]
    fn operation_index_answers_lookups_for_branching_paths() {
        let paths = branching_paths();
        let mut index = NativeIndex::<OperationKey, usize>::default();
        for (position, path) in paths.iter().enumerate() {
            let prepared = index
                .prepare_insert(operation_key(path), position)
                .expect("insert fits");
            index.commit(prepared);
        }
        for (position, path) in paths.iter().enumerate() {
            assert_eq!(
                index.get(&operation_key(path)),
                Some(&position),
                "lookup misses {path:?}"
            );
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

        /// An index built by inserts answers every lookup as the list of
        /// inserted keys does, for arbitrary key sets and probes.
        #[test]
        fn operation_index_answers_lookups_for_arbitrary_key_sets(
            inserted in proptest::collection::btree_set(
                proptest::collection::vec((0u64..2, 0u64..2), 0..20),
                0..24,
            ),
            probes in proptest::collection::vec(
                proptest::collection::vec((0u64..2, 0u64..2), 0..20),
                0..24,
            ),
        ) {
            let inserted: Vec<Vec<(u64, u64)>> = inserted.into_iter().collect();
            let mut index = NativeIndex::<OperationKey, usize>::default();
            for (position, path) in inserted.iter().enumerate() {
                let prepared = index
                    .prepare_insert(operation_key(path), position)
                    .expect("insert fits");
                index.commit(prepared);
            }
            for probe in inserted.iter().chain(probes.iter()) {
                let expected = inserted.iter().position(|path| path == probe);
                proptest::prop_assert_eq!(index.get(&operation_key(probe)).copied(), expected);
            }
        }
    }
}
