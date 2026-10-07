use std::ops::Range;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::rspace_interface::{
    RSpaceOperationCompletion, RSpaceOperationSource,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce, COMM};
use thiserror::Error;

use super::super::operation_sources::allocate;
use super::super::operations::sort;
use super::super::recording::work;
use super::super::{
    HostWorkBudget, InterpreterError, NativeOperationRecord, NativeOperationSource,
};
use super::CheckedNativeOperationJournal;

mod replay;
pub use replay::{
    NativeOperationReplay, NativeReplayAccountingSnapshot, NativeReplayBoundary,
    NativeReplayCheckpoint, NativeReplayError, NativeReplayOutcome, NativeReplayPublication,
    NativeReplayReservation, NativeReplayRestore, NativeRuntimeReplayCheckpoint,
    NativeRuntimeReplaySession,
};

#[derive(Clone, Copy, Debug)]
pub struct NativeOperationTraceLimits {
    pub events: usize,
    pub source_entries: usize,
    pub source_bytes: usize,
    pub telemetry_items: usize,
    pub telemetry_bytes: usize,
}

#[derive(Debug, Error)]
pub enum NativeOperationTraceError {
    #[error(transparent)]
    Host(#[from] InterpreterError),
    #[error("native operation trace exceeds its structural limits")]
    Limit,
    #[error("native operation journal does not cover the committed trace exactly")]
    Coverage,
    #[error("native operation trace source differs from its journal occurrence")]
    Source,
    #[error("native COMM producer copies have inconsistent telemetry")]
    Telemetry,
}

struct TraceSlot {
    operation: usize,
    events: Range<usize>,
}

pub struct CheckedNativeOperationTrace {
    journal: CheckedNativeOperationJournal,
    trace: Arc<[Event]>,
    slots: Vec<TraceSlot>,
}

impl CheckedNativeOperationTrace {
    pub fn journal(&self) -> &CheckedNativeOperationJournal { &self.journal }

    pub fn operation_count(&self) -> usize { self.slots.len() }

    pub fn event_count(&self) -> usize { self.trace.len() }

    pub fn operation(&self, slot: usize) -> Option<&NativeOperationRecord> {
        self.slots
            .get(slot)
            .map(|slot| &self.journal.operations[slot.operation])
    }

    pub fn journal_index(&self, slot: usize) -> Option<usize> {
        self.slots.get(slot).map(|slot| slot.operation)
    }

    pub fn events(&self, slot: usize) -> Option<&[Event]> {
        self.slots
            .get(slot)
            .map(|slot| &self.trace[slot.events.clone()])
    }

    pub fn introduction(&self, slot: usize) -> Option<&IOEvent> {
        match self.events(slot)?.first()? {
            Event::IoEvent(source) => Some(source),
            Event::Comm(_) => None,
        }
    }

    pub fn comm(&self, slot: usize) -> Option<&COMM> {
        match self.events(slot)?.get(1)? {
            Event::Comm(source) => Some(source),
            Event::IoEvent(_) => None,
        }
    }
}

fn width(completion: RSpaceOperationCompletion) -> usize {
    match completion {
        RSpaceOperationCompletion::Stored => 1,
        RSpaceOperationCompletion::Matched => 2,
        RSpaceOperationCompletion::Rejected => 0,
    }
}

struct TraceSize<'a> {
    limits: NativeOperationTraceLimits,
    host: &'a HostWorkBudget,
    entries: usize,
    bytes: usize,
    items: usize,
    outputs: usize,
}

fn count(total: &mut usize, amount: usize, limit: usize) -> Result<(), NativeOperationTraceError> {
    *total = total
        .checked_add(amount)
        .filter(|sum| *sum <= limit)
        .ok_or(NativeOperationTraceError::Limit)?;
    Ok(())
}

fn work_count(count: usize, scale: usize, extra: usize) -> Result<usize, InterpreterError> {
    count
        .checked_mul(scale)
        .and_then(|total| total.checked_add(extra))
        .ok_or(InterpreterError::HostWorkRejected)
}

impl TraceSize<'_> {
    fn entries(&mut self, amount: usize) -> Result<(), NativeOperationTraceError> {
        count(&mut self.entries, amount, self.limits.source_entries)?;
        work(
            self.host,
            HostWorkDimension::VerificationOperations,
            work_count(amount, 1, 1)?,
        )?;
        Ok(())
    }

    fn bytes(&mut self, amount: usize) -> Result<(), NativeOperationTraceError> {
        count(&mut self.bytes, amount, self.limits.source_bytes)?;
        work(self.host, HostWorkDimension::VerificationBytes, amount)?;
        Ok(())
    }

    fn consume(&mut self, source: &Consume) -> Result<(), NativeOperationTraceError> {
        self.entries(work_count(source.channel_hashes.len(), 1, 1)?)?;
        self.bytes(work_count(source.hash.0.len(), 1, 1)?)?;
        for channel in &source.channel_hashes {
            self.bytes(channel.0.len())?;
        }
        Ok(())
    }

    fn produce(&mut self, source: &Produce) -> Result<(), NativeOperationTraceError> {
        self.entries(1)?;
        self.bytes(
            source
                .hash
                .0
                .len()
                .checked_add(source.channel_hash.0.len())
                .and_then(|count| count.checked_add(3))
                .ok_or(NativeOperationTraceError::Limit)?,
        )?;
        count(
            &mut self.items,
            source.output_value.len(),
            self.limits.telemetry_items,
        )?;
        work(
            self.host,
            HostWorkDimension::VerificationOperations,
            source.output_value.len(),
        )?;
        for item in &source.output_value {
            count(&mut self.outputs, item.len(), self.limits.telemetry_bytes)?;
            work(self.host, HostWorkDimension::VerificationBytes, item.len())?;
        }
        Ok(())
    }

    fn comm(&mut self, source: &COMM) -> Result<(), NativeOperationTraceError> {
        self.consume(&source.consume)?;
        self.entries(
            source
                .produces
                .len()
                .checked_add(source.times_repeated.len())
                .and_then(|count| count.checked_add(source.peeks.len()))
                .ok_or(NativeOperationTraceError::Limit)?,
        )?;
        self.bytes(
            source
                .peeks
                .len()
                .checked_add(source.times_repeated.len())
                .and_then(|count| count.checked_mul(4))
                .ok_or(NativeOperationTraceError::Limit)?,
        )?;
        for produce in &source.produces {
            self.produce(produce)?;
        }
        for produce in source.times_repeated.keys() {
            self.produce(produce)?;
        }
        Ok(())
    }
}

fn same_producer(a: &Produce, b: &Produce) -> bool {
    a.hash == b.hash && a.channel_hash == b.channel_hash && a.persistent == b.persistent
}

fn comm_copies(comm: &COMM, host: &HostWorkBudget) -> Result<(), NativeOperationTraceError> {
    let entries = comm
        .produces
        .len()
        .checked_add(comm.times_repeated.len())
        .ok_or(NativeOperationTraceError::Limit)?;
    work(host, HostWorkDimension::VerificationOperations, entries)?;
    let produce_hash_bytes = comm
        .produces
        .iter()
        .map(|produce| produce.hash.0.len())
        .max()
        .unwrap_or(0);
    let key_hash_bytes = comm
        .times_repeated
        .keys()
        .map(|produce| produce.hash.0.len())
        .max()
        .unwrap_or(0);
    let compared_bytes = produce_hash_bytes
        .checked_add(key_hash_bytes)
        .ok_or(NativeOperationTraceError::Limit)?;
    let comparisons = work_count(
        work_count(
            comm.times_repeated.len().checked_ilog2().unwrap_or(0) as usize,
            1,
            1,
        )?,
        16,
        0,
    )?
    .checked_mul(comm.produces.len())
    .ok_or(NativeOperationTraceError::Limit)?;
    work(host, HostWorkDimension::VerificationOperations, comparisons)?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        comparisons
            .checked_mul(compared_bytes)
            .ok_or(NativeOperationTraceError::Limit)?,
    )?;
    let mut producers = allocate(comm.produces.len(), host)?;
    producers.extend(comm.produces.iter());
    sort(&mut producers, |a, b| {
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            a.hash
                .0
                .len()
                .checked_add(b.hash.0.len())
                .ok_or(InterpreterError::HostWorkRejected)?,
        )?;
        Ok(a.hash.cmp(&b.hash))
    })?;
    let mut distinct = usize::from(!producers.is_empty());
    for pair in producers.windows(2) {
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        work(
            host,
            HostWorkDimension::VerificationBytes,
            pair[0]
                .hash
                .0
                .len()
                .checked_add(pair[1].hash.0.len())
                .ok_or(NativeOperationTraceError::Limit)?,
        )?;
        if pair[0].hash != pair[1].hash {
            distinct = distinct
                .checked_add(1)
                .ok_or(NativeOperationTraceError::Limit)?;
        }
    }
    if distinct != comm.times_repeated.len() {
        return Err(NativeOperationTraceError::Source);
    }
    for produce in &comm.produces {
        let (copy, _) = comm
            .times_repeated
            .get_key_value(produce)
            .ok_or(NativeOperationTraceError::Source)?;
        work(host, HostWorkDimension::VerificationOperations, 3)?;
        let source_bytes = [
            &produce.hash.0,
            &produce.channel_hash.0,
            &copy.hash.0,
            &copy.channel_hash.0,
        ]
        .into_iter()
        .try_fold(0usize, |total, bytes| total.checked_add(bytes.len()))
        .ok_or(NativeOperationTraceError::Limit)?;
        work(host, HostWorkDimension::VerificationBytes, source_bytes)?;
        if !same_producer(produce, copy) {
            return Err(NativeOperationTraceError::Source);
        }
        work(
            host,
            HostWorkDimension::VerificationOperations,
            produce
                .output_value
                .len()
                .checked_add(copy.output_value.len())
                .and_then(|count| count.checked_add(1))
                .ok_or(NativeOperationTraceError::Limit)?,
        )?;
        for item in produce.output_value.iter().chain(copy.output_value.iter()) {
            work(host, HostWorkDimension::VerificationBytes, item.len())?;
        }
        if produce.is_deterministic != copy.is_deterministic
            || produce.failed != copy.failed
            || produce.output_value != copy.output_value
        {
            return Err(NativeOperationTraceError::Telemetry);
        }
    }
    Ok(())
}

impl CheckedNativeOperationJournal {
    pub fn bind_trace(
        self,
        trace: Arc<[Event]>,
        limits: NativeOperationTraceLimits,
        host: &HostWorkBudget,
    ) -> Result<CheckedNativeOperationTrace, NativeOperationTraceError> {
        work(
            host,
            HostWorkDimension::VerificationOperations,
            work_count(self.operations.len(), 1, 1)?,
        )?;
        let mut expected = 0;
        for row in self.operations.iter() {
            count(&mut expected, width(row.completion), limits.events)?;
        }
        if trace.len() != expected {
            return Err(NativeOperationTraceError::Coverage);
        }
        let mut size = TraceSize {
            limits,
            host,
            entries: 0,
            bytes: 0,
            items: 0,
            outputs: 0,
        };
        for event in trace.iter() {
            match event {
                Event::IoEvent(IOEvent::Produce(source)) => size.produce(source)?,
                Event::IoEvent(IOEvent::Consume(source)) => size.consume(source)?,
                Event::Comm(source) => size.comm(source)?,
            }
        }
        let mut slots = allocate(self.operations.len(), host)?;
        let meter = |operations, scanned, backing| -> Result<(), RSpaceError> {
            work(host, HostWorkDimension::VerificationOperations, operations)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            work(host, HostWorkDimension::VerificationBytes, scanned)
                .map_err(|_| RSpaceError::HostWorkRejected)?;
            work(host, HostWorkDimension::SearchStateBytes, backing)
                .map_err(|_| RSpaceError::HostWorkRejected)
        };
        // Changed by C7b (DR-86): the slots follow a depth-first walk of the
        // path trie that visits the children of each node by increasing
        // segment. The walk lists the paths in sorted vector order
        // (`NativePathTrie.trie_dfs_is_lex_sort`), which is the order of the
        // replaced sort, because all rows share one session (check_sizes) and
        // their paths are distinct (check_operation_journal).
        // for operation in 0..self.operations.len() {
        //     slots.push(TraceSlot {
        //         operation,
        //         events: 0..0,
        //     });
        // }
        // sort(&mut slots, |left, right| {
        //     let a = &self.operations[left.operation].occurrence;
        //     let b = &self.operations[right.operation].occurrence;
        //     let segments = a
        //         .path
        //         .len()
        //         .checked_add(b.path.len())
        //         .ok_or(InterpreterError::HostWorkRejected)?;
        //     work(
        //         host,
        //         HostWorkDimension::VerificationOperations,
        //         work_count(segments, 1, 1)?,
        //     )?;
        //     work(
        //         host,
        //         HostWorkDimension::VerificationBytes,
        //         work_count(segments, 16, 64)?,
        //     )?;
        //     Ok(a.session.cmp(&b.session).then_with(|| a.path.cmp(&b.path)))
        // })?;
        let order = self.recording.paths.preorder(host)?;
        let nodes = order
            .len()
            .checked_add(1)
            .ok_or(NativeOperationTraceError::Limit)?;
        let mut node_operation = allocate::<Option<usize>>(nodes, host)?;
        node_operation.resize(nodes, None);
        work(
            host,
            HostWorkDimension::VerificationOperations,
            work_count(self.operations.len(), 1, order.len())?,
        )?;
        for (operation, row) in self.operations.iter().enumerate() {
            *node_operation
                .get_mut(row.occurrence.path.index())
                .ok_or(NativeOperationTraceError::Coverage)? = Some(operation);
        }
        // The empty path is the root, which the walk omits. It sorts before
        // every other path.
        if let Some(operation) = node_operation[0] {
            slots.push(TraceSlot {
                operation,
                events: 0..0,
            });
        }
        for id in order {
            if let Some(operation) = node_operation[id.index()] {
                slots.push(TraceSlot {
                    operation,
                    events: 0..0,
                });
            }
        }
        if slots.len() != self.operations.len() {
            return Err(NativeOperationTraceError::Coverage);
        }
        let mut cursor = 0usize;
        for slot in &mut slots {
            let row = &self.operations[slot.operation];
            let end = cursor
                .checked_add(width(row.completion))
                .ok_or(NativeOperationTraceError::Limit)?;
            slot.events = cursor..end;
            let events = &trace[slot.events.clone()];
            if !events.is_empty() {
                let source = match &events[0] {
                    Event::IoEvent(IOEvent::Produce(source)) => {
                        RSpaceOperationSource::Produce(source)
                    }
                    Event::IoEvent(IOEvent::Consume(source)) => {
                        RSpaceOperationSource::Consume(source)
                    }
                    Event::Comm(_) => return Err(NativeOperationTraceError::Coverage),
                };
                if !row
                    .source
                    .metered_matches(source, &meter)
                    .map_err(|_| InterpreterError::HostWorkRejected)?
                {
                    return Err(NativeOperationTraceError::Source);
                }
            }
            if row.completion == RSpaceOperationCompletion::Matched {
                let Event::Comm(comm) = &events[1] else {
                    return Err(NativeOperationTraceError::Coverage);
                };
                let recorded = row
                    .comm
                    .as_ref()
                    .ok_or(NativeOperationTraceError::Coverage)?;
                if !recorded
                    .source
                    .metered_matches(comm, &meter)
                    .map_err(|_| InterpreterError::HostWorkRejected)?
                {
                    return Err(NativeOperationTraceError::Source);
                }
                if matches!(&row.source, NativeOperationSource::Consume(_)) {
                    work(
                        host,
                        HostWorkDimension::VerificationOperations,
                        work_count(comm.consume.channel_hashes.len(), 1, 1)?,
                    )?;
                    work(
                        host,
                        HostWorkDimension::VerificationBytes,
                        work_count(comm.consume.channel_hashes.len(), 32, 33)?,
                    )?;
                    if !row
                        .source
                        .metered_matches(RSpaceOperationSource::Consume(&comm.consume), &meter)
                        .map_err(|_| InterpreterError::HostWorkRejected)?
                    {
                        return Err(NativeOperationTraceError::Source);
                    }
                }
                comm_copies(comm, host)?;
            }
            cursor = end;
        }
        Ok(CheckedNativeOperationTrace {
            journal: self,
            trace,
            slots,
        })
    }
}
