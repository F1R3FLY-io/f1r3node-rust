use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use models::rhoapi::Par;
use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::operation_context::{self, OperationOrder};
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::COMM;
use serde::Serialize;

use super::index::{reserve_lookup, reserve_vector, IndexKey, NativeIndex};
use super::operation_sources::{allocate, channel_bytes, NativeCommSource, NativeOperationSource};
use super::recording::{recording_error, work};
use super::{InterpreterError, NativeRuntimeConfig, RuntimeBudget};
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetOccurrence,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeOperationOccurrence {
    pub session: [u8; 32],
    pub path: Arc<[(u64, u64)]>,
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

impl IndexKey for OperationKey {
    fn comparison_work(&self) -> (usize, usize) {
        (
            self.0.path.len().saturating_mul(2).saturating_add(4),
            self.0.path.len().saturating_mul(16).saturating_add(32),
        )
    }
}

#[derive(Clone, Copy)]
struct PublishedObservation {
    link: NativeObservationLink,
    granted: bool,
}

struct PendingOperation {
    occurrence: NativeOperationOccurrence,
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
    row_capacity: usize,
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
    host: &super::HostWorkBudget,
) -> Result<Vec<Arc<[u8]>>, InterpreterError> {
    work(host, HostWorkDimension::VerificationOperations, joins.len())?;
    let count = joins
        .iter()
        .try_fold(channels.len(), |n, group| n.checked_add(group.len()))
        .ok_or(InterpreterError::HostWorkRejected)?;
    let mut footprint = allocate(count, host)?;
    for channel in channels.iter().chain(joins.iter().flatten()) {
        footprint.push(channel_bytes(channel, host)?);
    }
    sort(&mut footprint, |a, b| {
        work(
            host,
            HostWorkDimension::VerificationBytes,
            a.len().min(b.len()).saturating_add(1),
        )?;
        Ok(a.cmp(b))
    })?;
    let bytes = footprint
        .iter()
        .try_fold(0usize, |n, channel| n.checked_add(channel.len()))
        .ok_or(InterpreterError::HostWorkRejected)?;
    work(host, HostWorkDimension::VerificationBytes, bytes)?;
    work(
        host,
        HostWorkDimension::VerificationOperations,
        footprint.len(),
    )?;
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
        work(
            &self.host_work,
            HostWorkDimension::VerificationOperations,
            order.path.len().saturating_mul(4).saturating_add(68),
        )?;
        Ok(OperationKey(order))
    }

    fn start_operation(
        &mut self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<(), InterpreterError> {
        if self.recording.invalid.load(Ordering::Acquire)
            || self.operations.rows.len() >= self.limits.attempts
        {
            return Err(recording_error(
                "native operation recording is invalid or full",
            ));
        }
        let key = self.operation_key()?;
        if self.operations.index.get(&key, &self.host_work)?.is_some() {
            return Err(recording_error("native operation occurrence is duplicated"));
        }
        let source = NativeOperationSource::capture(source, &self.host_work)?;
        let footprint = locked_footprint(channels, joins, &self.host_work)?;
        let mut predecessors = allocate(footprint.len(), &self.host_work)?;
        for channel in &footprint {
            if let Some(index) = self.operations.last_channel.get(channel, &self.host_work)? {
                if self.operations.rows[*index].completion.is_none() {
                    return Err(recording_error(
                        "native conflicting operations overlap outside channel guards",
                    ));
                }
                predecessors.push(*index);
            }
        }
        sort(&mut predecessors, |a, b| {
            work(
                &self.host_work,
                HostWorkDimension::VerificationOperations,
                1,
            )?;
            Ok(a.cmp(b))
        })?;
        work(
            &self.host_work,
            HostWorkDimension::VerificationOperations,
            predecessors.len(),
        )?;
        predecessors.dedup();
        reserve_vector(
            &mut self.operations.rows,
            &mut self.operations.row_capacity,
            1,
            &self.host_work,
        )?;
        reserve_lookup(
            (
                self.limits
                    .path_segments
                    .saturating_mul(2)
                    .saturating_add(4),
                self.limits
                    .path_segments
                    .saturating_mul(16)
                    .saturating_add(32),
            ),
            self.limits.attempts,
            &self.host_work,
        )?;
        let mut path = allocate(key.0.path.len(), &self.host_work)?;
        path.extend(key.0.path.segments_rev());
        path.reverse();
        let index = self.operations.rows.len();
        let row = PendingOperation {
            occurrence: NativeOperationOccurrence {
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
        let mut updates = allocate(row.footprint.len(), &self.host_work)?;
        updates.extend(
            row.footprint
                .iter()
                .map(|channel| (Arc::clone(channel), index)),
        );
        let channel_updates = self
            .operations
            .last_channel
            .prepare_batch(updates, &self.host_work)?;
        let operation = self
            .operations
            .index
            .prepare_insert(key, index, &self.host_work)?;
        self.operations.last_channel.commit_batch(channel_updates);
        self.operations.index.commit(operation);
        self.operations.rows.push(row);
        Ok(())
    }

    fn observe_consume_peeks(&mut self, peeks: &BTreeSet<i32>) -> Result<(), InterpreterError> {
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key, &self.host_work)?
            .copied()
            .ok_or_else(|| recording_error("native consume has no started operation"))?;
        let row = &self.operations.rows[index];
        let NativeOperationSource::Consume(source) = &row.source else {
            return Err(recording_error("native peek metadata belongs to a consume"));
        };
        if row.completion.is_some() || row.introduction.is_some() || row.consume_peeks.is_some() {
            return Err(recording_error("native peek metadata is out of order"));
        }
        work(
            &self.host_work,
            HostWorkDimension::VerificationOperations,
            peeks.len(),
        )?;
        if peeks.iter().any(|index| {
            usize::try_from(*index).map_or(true, |index| index >= source.channels.len())
        }) {
            return Err(recording_error(
                "native peek index is outside the consume channels",
            ));
        }
        let mut captured = allocate(peeks.len(), &self.host_work)?;
        captured.extend(peeks.iter().copied());
        self.operations.rows[index].consume_peeks = Some(captured.into());
        Ok(())
    }

    fn observe_comm_source(&mut self, source: &COMM) -> Result<(), InterpreterError> {
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key, &self.host_work)?
            .copied()
            .ok_or_else(|| recording_error("native COMM has no started operation"))?;
        let row = &self.operations.rows[index];
        if row.completion.is_some()
            || row.comm_source.is_some()
            || !row.introduction.is_some_and(|intro| intro.granted)
        {
            return Err(recording_error("native COMM observation is out of order"));
        }
        work(
            &self.host_work,
            HostWorkDimension::SearchStateBytes,
            std::mem::size_of::<NativeCommSource>(),
        )?;
        let source = Arc::new(NativeCommSource::capture(source, &self.host_work)?);
        self.operations.rows[index].comm_source = Some(source);
        Ok(())
    }

    pub(super) fn operation_stage_slot(
        &self,
        occurrence: &NativeBudgetOccurrence,
    ) -> Result<Option<usize>, InterpreterError> {
        if self.operations.rows.is_empty() {
            return Ok(None);
        }
        let key = self.operation_key()?;
        let index = self
            .operations
            .index
            .get(&key, &self.host_work)?
            .copied()
            .ok_or_else(|| recording_error("native charge has no started operation"))?;
        let row = &self.operations.rows[index];
        if row.completion.is_some()
            || row.occurrence.session != occurrence.session
            || row.occurrence.path.as_ref() != occurrence.path
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
        let Some(index) = self
            .operations
            .index
            .get_prepaid(&OperationKey(order))
            .copied()
        else {
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

    fn capture_operations(&self) -> Result<Arc<[NativeOperationRecord]>, InterpreterError> {
        self.ensure_recording_complete()?;
        let mut result = allocate(self.operations.rows.len(), &self.host_work)?;
        let mut linked = 0usize;
        for row in &self.operations.rows {
            let introduction = row
                .introduction
                .ok_or_else(|| recording_error("native operation lacks introduction evidence"))?;
            let completion = row
                .completion
                .ok_or_else(|| recording_error("native operation did not complete"))?;
            linked = linked.saturating_add(1 + usize::from(row.comm.is_some()));
            let comm = match (&row.comm_source, row.comm) {
                (Some(source), Some(observation)) => Some(NativeCommRecord {
                    source: Arc::clone(source),
                    observation: observation.link,
                }),
                (None, None) => None,
                _ => return Err(recording_error("native COMM source and observation differ")),
            };
            result.push(NativeOperationRecord {
                occurrence: row.occurrence.clone(),
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
        if linked
            != self
                .recording
                .attempts
                .len()
                .saturating_add(self.recording.retries.len())
        {
            return Err(recording_error(
                "native operation evidence does not cover all budget publications",
            ));
        }
        Ok(result.into())
    }
}

impl RuntimeBudget {
    fn update_native_operations(
        &self,
        update: impl FnOnce(&mut NativeRuntimeConfig) -> Result<(), InterpreterError>,
    ) -> Result<(), InterpreterError> {
        if !self.has_comm_accounting_scope() || self.is_unmetered() {
            return Ok(());
        }
        let mut state = self.authority_state.lock().expect("authority state");
        let Some(native) = state.native.as_mut() else {
            return Ok(());
        };
        let result = update(native);
        if result.is_err() {
            native.recording.invalid.store(true, Ordering::Release);
        }
        result
    }

    pub(crate) fn start_native_operation(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<(), InterpreterError> {
        self.update_native_operations(|native| native.start_operation(source, channels, joins))
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
