use std::ops::Range;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;
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

impl TraceSize<'_> {
    fn entries(&mut self, amount: usize) -> Result<(), NativeOperationTraceError> {
        count(&mut self.entries, amount, self.limits.source_entries)?;
        work(
            self.host,
            HostWorkDimension::VerificationOperations,
            amount.saturating_add(1),
        )?;
        Ok(())
    }

    fn bytes(&mut self, amount: usize) -> Result<(), NativeOperationTraceError> {
        count(&mut self.bytes, amount, self.limits.source_bytes)?;
        work(self.host, HostWorkDimension::VerificationBytes, amount)?;
        Ok(())
    }

    fn consume(&mut self, source: &Consume) -> Result<(), NativeOperationTraceError> {
        self.entries(source.channel_hashes.len().saturating_add(1))?;
        self.bytes(source.hash.0.len().saturating_add(1))?;
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
                .saturating_add(source.channel_hash.0.len())
                .saturating_add(3),
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
                .saturating_add(source.times_repeated.len())
                .saturating_add(source.peeks.len()),
        )?;
        self.bytes(
            source
                .peeks
                .len()
                .saturating_add(source.times_repeated.len())
                .saturating_mul(4),
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
    let comparisons = (comm.times_repeated.len().checked_ilog2().unwrap_or(0) as usize)
        .saturating_add(1)
        .saturating_mul(16)
        .saturating_mul(comm.produces.len());
    work(host, HostWorkDimension::VerificationOperations, comparisons)?;
    work(
        host,
        HostWorkDimension::VerificationBytes,
        comparisons.saturating_mul(32),
    )?;
    let mut producers = allocate(comm.produces.len(), host)?;
    producers.extend(comm.produces.iter());
    sort(&mut producers, |a, b| {
        work(host, HostWorkDimension::VerificationOperations, 1)?;
        work(host, HostWorkDimension::VerificationBytes, 64)?;
        Ok(a.hash.cmp(&b.hash))
    })?;
    let distinct = producers
        .iter()
        .enumerate()
        .filter(|(index, produce)| *index == 0 || produce.hash != producers[index - 1].hash)
        .count();
    if distinct != comm.times_repeated.len() {
        return Err(NativeOperationTraceError::Source);
    }
    for produce in &comm.produces {
        let (copy, _) = comm
            .times_repeated
            .get_key_value(produce)
            .ok_or(NativeOperationTraceError::Source)?;
        if !same_producer(produce, copy) {
            return Err(NativeOperationTraceError::Source);
        }
        work(
            host,
            HostWorkDimension::VerificationOperations,
            produce
                .output_value
                .len()
                .saturating_add(copy.output_value.len())
                .saturating_add(1),
        )?;
        for item in &produce.output_value {
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
            self.operations.len().saturating_add(1),
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
        for operation in 0..self.operations.len() {
            slots.push(TraceSlot {
                operation,
                events: 0..0,
            });
        }
        sort(&mut slots, |left, right| {
            let a = &self.operations[left.operation].occurrence;
            let b = &self.operations[right.operation].occurrence;
            let segments = a.path.len().saturating_add(b.path.len());
            work(
                host,
                HostWorkDimension::VerificationOperations,
                segments.saturating_add(1),
            )?;
            work(
                host,
                HostWorkDimension::VerificationBytes,
                segments.saturating_mul(16).saturating_add(64),
            )?;
            Ok(a.session.cmp(&b.session).then_with(|| a.path.cmp(&b.path)))
        })?;
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
                if !row.source.matches(source) {
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
                if !recorded.source.matches(comm) {
                    return Err(NativeOperationTraceError::Source);
                }
                if matches!(&row.source, NativeOperationSource::Consume(_)) {
                    work(
                        host,
                        HostWorkDimension::VerificationOperations,
                        comm.consume.channel_hashes.len().saturating_add(1),
                    )?;
                    work(
                        host,
                        HostWorkDimension::VerificationBytes,
                        comm.consume
                            .channel_hashes
                            .len()
                            .saturating_mul(32)
                            .saturating_add(33),
                    )?;
                    if !row
                        .source
                        .matches(RSpaceOperationSource::Consume(&comm.consume))
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
