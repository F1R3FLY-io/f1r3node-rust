use std::sync::Arc;

use models::rhoapi::CostAuthority;
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};
use prost::Message;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationCompletion;

use super::path_trie::{NativePathTrie, PathId};
use super::recording::recording_error;
use super::{
    ByteObservation, InterpreterError, NativeBudgetRecording, NativeBudgetRetry, NativeCommRecord,
    NativeCommSource, NativeConsumeSource, NativeObservationLink, NativeOperationOccurrence,
    NativeOperationRecord, NativeOperationSource, NativeProduceSource,
};
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::ByteCharge;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetAttempt, NativeBudgetOccurrence, NativeBudgetTraceLimits,
    NativeDenialCause, NativeStackTransferEvent, NativeStackTransferKind,
};

const BUDGET_DOMAIN: &[u8] = b"f1r3node:native-budget-recording:v1";
const JOURNAL_DOMAIN: &[u8] = b"f1r3node:native-operation-journal:v1";
const BUDGET_DOMAIN_V2: &[u8] = b"f1r3node:native-budget-recording:v2";
const BUDGET_DOMAIN_V3: &[u8] = b"f1r3node:native-budget-recording:v3";
const JOURNAL_DOMAIN_V2: &[u8] = b"f1r3node:native-operation-journal:v2";

#[derive(Clone, Copy, Debug)]
pub struct NativeRecordingWireLimits {
    pub wire: PhloWireLimits,
    pub budget: NativeBudgetTraceLimits,
    pub operations: usize,
    pub source_entries: usize,
    pub footprint_entries: usize,
    pub footprint_bytes: usize,
    pub predecessor_edges: usize,
    /// The limit on the new path segments Σ s_i of a recording and its
    /// journal together, which bounds the nodes of their path trie (C7b,
    /// DR-86).
    pub total_path_segments: usize,
}

fn malformed() -> InterpreterError { recording_error("invalid native recording wire") }

fn length(value: usize) -> Result<u64, InterpreterError> {
    u64::try_from(value).map_err(|_| malformed())
}

fn bounded_count(value: u64, maximum: usize) -> Result<usize, InterpreterError> {
    usize::try_from(value)
        .ok()
        .filter(|count| *count <= maximum)
        .ok_or_else(malformed)
}

struct Writer {
    wire: PhloWireEncoder,
}

impl Writer {
    fn u8(&mut self, value: u8) -> Result<(), InterpreterError> {
        self.wire.u8(value).map_err(|_| malformed())
    }

    fn u64(&mut self, value: u64) -> Result<(), InterpreterError> {
        self.wire.u64(value).map_err(|_| malformed())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), InterpreterError> {
        self.wire.bytes(value).map_err(|_| malformed())
    }

    fn finish(self) -> Vec<u8> { self.wire.into_bytes() }
}

struct Reader<'a> {
    wire: PhloWireDecoder<'a>,
}

impl<'a> Reader<'a> {
    fn new(input: &'a [u8], limits: PhloWireLimits) -> Result<Self, InterpreterError> {
        if input.len() > limits.total_bytes {
            return Err(malformed());
        }
        Ok(Self {
            wire: PhloWireDecoder::new(input, limits).map_err(|_| malformed())?,
        })
    }

    fn u8(&mut self) -> Result<u8, InterpreterError> { self.wire.u8().map_err(|_| malformed()) }

    fn u64(&mut self) -> Result<u64, InterpreterError> { self.wire.u64().map_err(|_| malformed()) }

    fn bytes(&mut self) -> Result<&'a [u8], InterpreterError> {
        self.wire.bytes().map_err(|_| malformed())
    }

    fn fixed32(&mut self) -> Result<[u8; 32], InterpreterError> {
        let bytes = self.bytes()?;
        bytes.try_into().map_err(|_| malformed())
    }

    fn finish(self) -> Result<(), InterpreterError> { self.wire.finish().map_err(|_| malformed()) }
}

fn stage_tag(stage: NativeAttemptStage) -> u8 {
    match stage {
        NativeAttemptStage::ProduceIntroduction => 0,
        NativeAttemptStage::ConsumeIntroduction => 1,
        NativeAttemptStage::Comm => 2,
    }
}

fn stage(value: u8) -> Result<NativeAttemptStage, InterpreterError> {
    match value {
        0 => Ok(NativeAttemptStage::ProduceIntroduction),
        1 => Ok(NativeAttemptStage::ConsumeIntroduction),
        2 => Ok(NativeAttemptStage::Comm),
        _ => Err(malformed()),
    }
}

fn kind(value: NativeAttemptStage) -> AuthorityByteEventKind {
    match value {
        NativeAttemptStage::ProduceIntroduction => AuthorityByteEventKind::ProduceIntroduction,
        NativeAttemptStage::ConsumeIntroduction => AuthorityByteEventKind::ConsumeIntroduction,
        NativeAttemptStage::Comm => AuthorityByteEventKind::Comm,
    }
}

#[cfg(test)]
fn write_path(
    wire: &mut Writer,
    path: &[(u64, u64)],
    maximum: usize,
) -> Result<(), InterpreterError> {
    if path.len() > maximum {
        return Err(malformed());
    }
    wire.u64(length(path.len())?)?;
    for &(left, right) in path {
        wire.u64(left)?;
        wire.u64(right)?;
    }
    Ok(())
}

// Changed by C7b (DR-86): a path decodes into the trie. Its segments are
// admitted against the Σ s_i limit before any node is allocated.
// fn read_path(wire: &mut Reader<'_>, maximum: usize) -> Result<Vec<(u64, u64)>, InterpreterError> {
//     let count = bounded_count(wire.u64()?, maximum)?;
//     let mut path = reserve_vec(count, wire.host)?;
//     for _ in 0..count {
//         path.push((wire.u64()?, wire.u64()?));
//     }
//     Ok(path)
// }
fn read_path(
    wire: &mut Reader<'_>,
    paths: &mut NativePathTrie,
    maximum: usize,
    total: usize,
) -> Result<PathId, InterpreterError> {
    let count = bounded_count(wire.u64()?, maximum)?;
    if !paths.admit(count, total) {
        return Err(malformed());
    }
    let mut node = PathId::ROOT;
    for _ in 0..count {
        node = paths.child(node, (wire.u64()?, wire.u64()?))?;
    }
    Ok(node)
}

// Changed by C7b (DR-86): the delta encoder reads both paths from the trie.
// The longest common prefix comes from the jump table, so the bytes equal
// those of the segment encoder, which stays as the test oracle.
fn write_path_delta(
    wire: &mut Writer,
    paths: &NativePathTrie,
    path: PathId,
    previous: PathId,
    maximum: usize,
) -> Result<(), InterpreterError> {
    if path.depth() > maximum || !paths.contains(path) || !paths.contains(previous) {
        return Err(malformed());
    }
    let prefix = paths.lcp_depth(path, previous)?;
    let suffix = paths.suffix(path, prefix)?;
    wire.u64(length(prefix)?)?;
    wire.u64(length(suffix.len())?)?;
    for (left, right) in suffix {
        wire.u64(left)?;
        wire.u64(right)?;
    }
    Ok(())
}

#[cfg(test)]
fn write_path_delta_segments(
    wire: &mut Writer,
    path: &[(u64, u64)],
    previous: &[(u64, u64)],
    maximum: usize,
) -> Result<(), InterpreterError> {
    if path.len() > maximum {
        return Err(malformed());
    }
    let shared = path.len().min(previous.len());
    let mut prefix = 0;
    while prefix < shared {
        let end = prefix.saturating_add(16).min(shared);
        while prefix < end && path[prefix] == previous[prefix] {
            prefix += 1;
        }
        if prefix != end {
            break;
        }
    }
    wire.u64(length(prefix)?)?;
    wire.u64(length(path.len() - prefix)?)?;
    for &(left, right) in &path[prefix..] {
        wire.u64(left)?;
        wire.u64(right)?;
    }
    Ok(())
}

// Changed by C7b (DR-86): the delta decoder interns the suffix below the
// ancestor of the previous path at the prefix depth. The suffix length is
// admitted against the Σ s_i limit before any node is allocated, and the
// canonical-prefix check reads the first suffix segment before its node is
// created.
// fn read_path_delta(
//     wire: &mut Reader<'_>,
//     previous: &[(u64, u64)],
//     maximum: usize,
// ) -> Result<Vec<(u64, u64)>, InterpreterError> {
//     let prefix = bounded_count(wire.u64()?, previous.len().min(maximum))?;
//     let suffix = bounded_count(wire.u64()?, maximum - prefix)?;
//     let mut path = reserve_vec(prefix + suffix, wire.host)?;
//     work(wire.host, HostWorkDimension::VerificationOperations, prefix)?;
//     work(
//         wire.host,
//         HostWorkDimension::VerificationBytes,
//         prefix
//             .checked_mul(16)
//             .ok_or(InterpreterError::HostWorkRejected)?,
//     )?;
//     path.extend_from_slice(&previous[..prefix]);
//     for _ in 0..suffix {
//         path.push((wire.u64()?, wire.u64()?));
//     }
//     if suffix != 0 && prefix < previous.len() && path[prefix] == previous[prefix] {
//         return Err(malformed());
//     }
//     Ok(path)
// }
fn read_path_delta(
    wire: &mut Reader<'_>,
    paths: &mut NativePathTrie,
    previous: PathId,
    maximum: usize,
    total: usize,
) -> Result<PathId, InterpreterError> {
    let prefix = bounded_count(wire.u64()?, previous.depth().min(maximum))?;
    let suffix = bounded_count(wire.u64()?, maximum - prefix)?;
    if !paths.admit(suffix, total) {
        return Err(malformed());
    }
    let mut node = paths.ancestor(previous, prefix)?;
    for index in 0..suffix {
        let segment = (wire.u64()?, wire.u64()?);
        if index == 0 && prefix < previous.depth() {
            let next = paths.ancestor(previous, prefix + 1)?;
            if paths.last_segment(next) == Some(segment) {
                return Err(malformed());
            }
        }
        node = paths.child(node, segment)?;
    }
    Ok(node)
}

#[cfg(test)]
fn write_occurrence(
    wire: &mut Writer,
    paths: &NativePathTrie,
    occurrence: &NativeBudgetOccurrence,
    limits: NativeBudgetTraceLimits,
) -> Result<(), InterpreterError> {
    wire.bytes(&occurrence.session)?;
    write_path(wire, &paths.segments(occurrence.path), limits.path_segments)?;
    wire.u8(stage_tag(occurrence.stage))
}

fn read_occurrence(
    wire: &mut Reader<'_>,
    paths: &mut NativePathTrie,
    limits: NativeRecordingWireLimits,
) -> Result<NativeBudgetOccurrence, InterpreterError> {
    Ok(NativeBudgetOccurrence {
        session: wire.fixed32()?,
        path: read_path(
            wire,
            paths,
            limits.budget.path_segments,
            limits.total_path_segments,
        )?,
        stage: stage(wire.u8()?)?,
    })
}

fn write_occurrence_delta(
    wire: &mut Writer,
    paths: &NativePathTrie,
    occurrence: &NativeBudgetOccurrence,
    previous: PathId,
    limits: NativeBudgetTraceLimits,
) -> Result<(), InterpreterError> {
    wire.bytes(&occurrence.session)?;
    write_path_delta(wire, paths, occurrence.path, previous, limits.path_segments)?;
    wire.u8(stage_tag(occurrence.stage))
}

fn read_occurrence_delta(
    wire: &mut Reader<'_>,
    paths: &mut NativePathTrie,
    previous: PathId,
    limits: NativeRecordingWireLimits,
) -> Result<NativeBudgetOccurrence, InterpreterError> {
    Ok(NativeBudgetOccurrence {
        session: wire.fixed32()?,
        path: read_path_delta(
            wire,
            paths,
            previous,
            limits.budget.path_segments,
            limits.total_path_segments,
        )?,
        stage: stage(wire.u8()?)?,
    })
}

fn write_observation(
    wire: &mut Writer,
    observation: &ByteObservation,
) -> Result<(), InterpreterError> {
    wire.bytes(&observation.event_id)?;
    wire.u8(stage_tag(observation.kind.into()))?;
    wire.bytes(&observation.authority.encode_to_vec())?;
    match observation.measurement {
        Some(charge) => {
            wire.u8(1)?;
            wire.u64(charge.introduction_bytes)?;
            wire.u64(charge.transfer_bytes)?;
            wire.u64(charge.trace_bytes)?;
        }
        None => wire.u8(0)?,
    }
    match observation.legacy_amount {
        Some(amount) => {
            wire.u8(1)?;
            wire.u64(amount)?;
        }
        None => wire.u8(0)?,
    }
    Ok(())
}

fn read_observation(wire: &mut Reader<'_>) -> Result<Arc<ByteObservation>, InterpreterError> {
    let event_id = wire.fixed32()?;
    let kind = kind(stage(wire.u8()?)?);
    let authority_bytes = wire.bytes()?;
    let authority = CostAuthority::decode(authority_bytes).map_err(|_| malformed())?;
    if authority.encoded_len() != authority_bytes.len() {
        return Err(malformed());
    }
    if authority.encode_to_vec() != authority_bytes {
        return Err(malformed());
    }
    let measurement = match wire.u8()? {
        0 => None,
        1 => Some(ByteCharge {
            introduction_bytes: wire.u64()?,
            transfer_bytes: wire.u64()?,
            trace_bytes: wire.u64()?,
        }),
        _ => return Err(malformed()),
    };
    let legacy_amount = match wire.u8()? {
        0 => None,
        1 => Some(wire.u64()?),
        _ => return Err(malformed()),
    };
    Ok(Arc::new(ByteObservation {
        event_id,
        kind,
        authority,
        measurement,
        legacy_amount,
    }))
}

pub fn encode_native_budget_recording(
    recording: &NativeBudgetRecording,
    limits: NativeRecordingWireLimits,
) -> Result<Vec<u8>, InterpreterError> {
    if recording.transfers.len() > limits.budget.attempts
        || recording
            .attempts
            .len()
            .checked_add(recording.retries.len())
            .filter(|n| *n <= limits.budget.attempts)
            .is_none()
    {
        return Err(malformed());
    }
    let mut wire = Writer {
        wire: PhloWireEncoder::new(limits.wire),
    };
    let extended = !recording.transfers.is_empty()
        || recording
            .attempts
            .iter()
            .any(|attempt| attempt.cause != NativeDenialCause::Budget);
    wire.bytes(if extended {
        BUDGET_DOMAIN_V3
    } else {
        BUDGET_DOMAIN_V2
    })?;
    wire.bytes(&recording.session)?;
    wire.u64(recording.used)?;
    wire.u64(length(recording.attempts.len())?)?;
    let paths = recording.paths.as_ref();
    let mut previous = PathId::ROOT;
    for attempt in recording.attempts.iter() {
        write_occurrence_delta(
            &mut wire,
            paths,
            &attempt.occurrence,
            previous,
            limits.budget,
        )?;
        previous = attempt.occurrence.path;
        if attempt.occurrence.session != recording.session
            || NativeAttemptStage::from(attempt.observation.kind) != attempt.occurrence.stage
        {
            return Err(malformed());
        }
        write_observation(&mut wire, &attempt.observation)?;
        wire.u8(attempt_tag(attempt)?)?;
    }
    wire.u64(length(recording.retries.len())?)?;
    for retry in recording.retries.iter() {
        write_occurrence_delta(&mut wire, paths, &retry.occurrence, previous, limits.budget)?;
        previous = retry.occurrence.path;
        if retry.occurrence.session != recording.session
            || NativeAttemptStage::from(retry.observation.kind) != retry.occurrence.stage
        {
            return Err(malformed());
        }
        write_observation(&mut wire, &retry.observation)?;
        wire.u64(length(retry.accepted_attempt)?)?;
        wire.u64(length(retry.fresh_before)?)?;
    }
    if extended {
        write_transfers(&mut wire, recording)?;
    }
    Ok(wire.finish())
}

pub fn decode_native_budget_recording(
    input: &[u8],
    limits: NativeRecordingWireLimits,
) -> Result<NativeBudgetRecording, InterpreterError> {
    let mut wire = Reader::new(input, limits.wire)?;
    let (delta, extended) = match wire.bytes()? {
        BUDGET_DOMAIN => (false, false),
        BUDGET_DOMAIN_V2 => (true, false),
        BUDGET_DOMAIN_V3 => (true, true),
        _ => return Err(malformed()),
    };
    let session = wire.fixed32()?;
    let used = wire.u64()?;
    let attempts_count = bounded_count(wire.u64()?, limits.budget.attempts)?;
    let mut paths = NativePathTrie::new(limits.budget.path_segments);
    let mut attempts = Vec::<NativeBudgetAttempt>::with_capacity(attempts_count);
    for _ in 0..attempts_count {
        let previous = attempts
            .last()
            .map_or(PathId::ROOT, |attempt| attempt.occurrence.path);
        let occurrence = if delta {
            read_occurrence_delta(&mut wire, &mut paths, previous, limits)?
        } else {
            read_occurrence(&mut wire, &mut paths, limits)?
        };
        let observation = read_observation(&mut wire)?;
        let (granted, cause) = match (wire.u8()?, extended) {
            (0, _) => (false, NativeDenialCause::Budget),
            (1, _) => (true, NativeDenialCause::Budget),
            (2, true) => (false, NativeDenialCause::Allocation),
            _ => return Err(malformed()),
        };
        if occurrence.session != session || occurrence.stage != observation.kind.into() {
            return Err(malformed());
        }
        attempts.push(NativeBudgetAttempt {
            occurrence,
            observation,
            granted,
            cause,
        });
    }
    let retries_count = bounded_count(wire.u64()?, limits.budget.attempts - attempts_count)?;
    let mut retries = Vec::<NativeBudgetRetry>::with_capacity(retries_count);
    for _ in 0..retries_count {
        let previous = retries
            .last()
            .map(|retry| retry.occurrence.path)
            .or_else(|| attempts.last().map(|attempt| attempt.occurrence.path))
            .unwrap_or(PathId::ROOT);
        let occurrence = if delta {
            read_occurrence_delta(&mut wire, &mut paths, previous, limits)?
        } else {
            read_occurrence(&mut wire, &mut paths, limits)?
        };
        let observation = read_observation(&mut wire)?;
        let accepted_attempt = bounded_count(wire.u64()?, attempts_count.saturating_sub(1))?;
        let fresh_before = bounded_count(wire.u64()?, attempts_count)?;
        if occurrence.session != session
            || occurrence.stage != observation.kind.into()
            || accepted_attempt >= fresh_before
        {
            return Err(malformed());
        }
        retries.push(NativeBudgetRetry {
            occurrence,
            observation,
            accepted_attempt,
            fresh_before,
        });
    }
    let transfers = if extended {
        read_transfers(&mut wire, attempts.len(), limits)?
    } else {
        Vec::new()
    };
    if extended
        && transfers.is_empty()
        && attempts
            .iter()
            .all(|row| row.cause == NativeDenialCause::Budget)
    {
        return Err(malformed());
    }
    wire.finish()?;
    Ok(NativeBudgetRecording {
        session,
        attempts: attempts.into(),
        retries: retries.into(),
        used,
        transfers: transfers.into(),
        paths: Arc::new(paths),
    })
}

fn attempt_tag(attempt: &NativeBudgetAttempt) -> Result<u8, InterpreterError> {
    match (attempt.granted, attempt.cause) {
        (false, NativeDenialCause::Budget) => Ok(0),
        (true, NativeDenialCause::Budget) => Ok(1),
        (false, NativeDenialCause::Allocation) => Ok(2),
        (true, NativeDenialCause::Allocation) => Err(malformed()),
    }
}

fn write_transfers(
    wire: &mut Writer,
    recording: &NativeBudgetRecording,
) -> Result<(), InterpreterError> {
    wire.u64(length(recording.transfers.len())?)?;
    let mut previous = 0;
    for (index, event) in recording.transfers.iter().enumerate() {
        if event.fresh_before < previous || event.fresh_before > recording.attempts.len() {
            return Err(malformed());
        }
        previous = event.fresh_before;
        match &event.kind {
            NativeStackTransferKind::Reserve {
                produce_hash,
                authority,
                cells,
                granted,
            } => {
                wire.u8(u8::from(*granted))?;
                wire.bytes(produce_hash)?;
                wire.bytes(&authority.encode_to_vec())?;
                wire.u64(*cells)?;
            }
            NativeStackTransferKind::Release { reserve } => {
                if *reserve >= index {
                    return Err(malformed());
                }
                wire.u8(2)?;
                wire.u64(length(*reserve)?)?;
            }
        }
        wire.u64(length(event.fresh_before)?)?;
    }
    Ok(())
}

fn read_transfers(
    wire: &mut Reader<'_>,
    attempts: usize,
    limits: NativeRecordingWireLimits,
) -> Result<Vec<NativeStackTransferEvent>, InterpreterError> {
    let count = bounded_count(wire.u64()?, limits.budget.attempts)?;
    let mut transfers = Vec::with_capacity(count);
    let mut previous = 0;
    for index in 0..count {
        let kind = match wire.u8()? {
            tag @ (0 | 1) => {
                let produce_hash = wire.fixed32()?;
                let authority_bytes = wire.bytes()?;
                let authority = CostAuthority::decode(authority_bytes).map_err(|_| malformed())?;
                if authority.encode_to_vec() != authority_bytes {
                    return Err(malformed());
                }
                NativeStackTransferKind::Reserve {
                    produce_hash,
                    authority,
                    cells: wire.u64()?,
                    granted: tag == 1,
                }
            }
            2 => NativeStackTransferKind::Release {
                reserve: bounded_count(wire.u64()?, index.saturating_sub(1))?,
            },
            _ => return Err(malformed()),
        };
        if matches!(kind, NativeStackTransferKind::Release { .. }) && index == 0 {
            return Err(malformed());
        }
        let fresh_before = bounded_count(wire.u64()?, attempts)?;
        if fresh_before < previous {
            return Err(malformed());
        }
        previous = fresh_before;
        transfers.push(NativeStackTransferEvent { kind, fresh_before });
    }
    Ok(transfers)
}

fn add_count(total: &mut usize, extra: usize, limit: usize) -> Result<(), InterpreterError> {
    *total = total
        .checked_add(extra)
        .filter(|next| *next <= limit)
        .ok_or_else(malformed)?;
    Ok(())
}

fn write_produce(wire: &mut Writer, source: &NativeProduceSource) -> Result<(), InterpreterError> {
    wire.bytes(&source.channel)?;
    wire.bytes(&source.hash)?;
    wire.u8(u8::from(source.persistent))
}

fn read_produce(wire: &mut Reader<'_>) -> Result<NativeProduceSource, InterpreterError> {
    let channel = wire.fixed32()?;
    let hash = wire.fixed32()?;
    let persistent = match wire.u8()? {
        0 => false,
        1 => true,
        _ => return Err(malformed()),
    };
    Ok(NativeProduceSource {
        channel,
        hash,
        persistent,
    })
}

fn write_consume(
    wire: &mut Writer,
    source: &NativeConsumeSource,
    sources: &mut usize,
    limits: NativeRecordingWireLimits,
) -> Result<(), InterpreterError> {
    add_count(sources, source.channels.len(), limits.source_entries)?;
    wire.u64(length(source.channels.len())?)?;
    for channel in source.channels.iter() {
        wire.bytes(channel)?;
    }
    wire.bytes(&source.hash)?;
    wire.u8(u8::from(source.persistent))
}

fn read_consume(
    wire: &mut Reader<'_>,
    sources: &mut usize,
    limits: NativeRecordingWireLimits,
) -> Result<NativeConsumeSource, InterpreterError> {
    let count = bounded_count(wire.u64()?, limits.source_entries.saturating_sub(*sources))?;
    add_count(sources, count, limits.source_entries)?;
    let mut channels = Vec::with_capacity(count);
    for _ in 0..count {
        channels.push(wire.fixed32()?);
    }
    let hash = wire.fixed32()?;
    let persistent = match wire.u8()? {
        0 => false,
        1 => true,
        _ => return Err(malformed()),
    };
    Ok(NativeConsumeSource {
        channels: channels.into(),
        hash,
        persistent,
    })
}

fn write_link(wire: &mut Writer, link: NativeObservationLink) -> Result<(), InterpreterError> {
    match link {
        NativeObservationLink::Attempt(index) => {
            wire.u8(0)?;
            wire.u64(length(index)?)
        }
        NativeObservationLink::Retry(index) => {
            wire.u8(1)?;
            wire.u64(length(index)?)
        }
    }
}

fn read_link(
    wire: &mut Reader<'_>,
    limits: NativeBudgetTraceLimits,
) -> Result<NativeObservationLink, InterpreterError> {
    let tag = wire.u8()?;
    let index = bounded_count(wire.u64()?, limits.attempts)?;
    match tag {
        0 => Ok(NativeObservationLink::Attempt(index)),
        1 => Ok(NativeObservationLink::Retry(index)),
        _ => Err(malformed()),
    }
}

fn write_i32(wire: &mut Writer, value: i32) -> Result<(), InterpreterError> {
    wire.u64(u64::from(value as u32))
}

fn read_i32(wire: &mut Reader<'_>) -> Result<i32, InterpreterError> {
    let value = u32::try_from(wire.u64()?).map_err(|_| malformed())?;
    Ok(value as i32)
}

fn write_comm(
    wire: &mut Writer,
    comm: &NativeCommRecord,
    sources: &mut usize,
    limits: NativeRecordingWireLimits,
) -> Result<(), InterpreterError> {
    add_count(sources, 1, limits.source_entries)?;
    write_consume(wire, &comm.source.consume, sources, limits)?;
    add_count(sources, comm.source.produces.len(), limits.source_entries)?;
    wire.u64(length(comm.source.produces.len())?)?;
    for source in comm.source.produces.iter() {
        write_produce(wire, source)?;
    }
    add_count(sources, comm.source.peeks.len(), limits.source_entries)?;
    wire.u64(length(comm.source.peeks.len())?)?;
    for &index in comm.source.peeks.iter() {
        write_i32(wire, index)?;
    }
    add_count(
        sources,
        comm.source.repetitions.len(),
        limits.source_entries,
    )?;
    wire.u64(length(comm.source.repetitions.len())?)?;
    for (source, count) in comm.source.repetitions.iter() {
        write_produce(wire, source)?;
        write_i32(wire, *count)?;
    }
    write_link(wire, comm.observation)
}

fn read_comm(
    wire: &mut Reader<'_>,
    sources: &mut usize,
    limits: NativeRecordingWireLimits,
) -> Result<NativeCommRecord, InterpreterError> {
    add_count(sources, 1, limits.source_entries)?;
    let consume = read_consume(wire, sources, limits)?;
    let count = bounded_count(wire.u64()?, limits.source_entries.saturating_sub(*sources))?;
    add_count(sources, count, limits.source_entries)?;
    let mut produces = Vec::with_capacity(count);
    for _ in 0..count {
        produces.push(read_produce(wire)?);
    }
    let count = bounded_count(wire.u64()?, limits.source_entries.saturating_sub(*sources))?;
    add_count(sources, count, limits.source_entries)?;
    let mut peeks = Vec::with_capacity(count);
    for _ in 0..count {
        peeks.push(read_i32(wire)?);
    }
    let count = bounded_count(wire.u64()?, limits.source_entries.saturating_sub(*sources))?;
    add_count(sources, count, limits.source_entries)?;
    let mut repetitions = Vec::with_capacity(count);
    for _ in 0..count {
        repetitions.push((read_produce(wire)?, read_i32(wire)?));
    }
    let observation = read_link(wire, limits.budget)?;
    Ok(NativeCommRecord {
        source: Arc::new(NativeCommSource {
            consume,
            produces: produces.into(),
            peeks: peeks.into(),
            repetitions: repetitions.into(),
        }),
        observation,
    })
}

fn completion_tag(completion: RSpaceOperationCompletion) -> u8 {
    match completion {
        RSpaceOperationCompletion::Stored => 0,
        RSpaceOperationCompletion::Matched => 1,
        RSpaceOperationCompletion::Rejected => 2,
    }
}

fn completion(value: u8) -> Result<RSpaceOperationCompletion, InterpreterError> {
    match value {
        0 => Ok(RSpaceOperationCompletion::Stored),
        1 => Ok(RSpaceOperationCompletion::Matched),
        2 => Ok(RSpaceOperationCompletion::Rejected),
        _ => Err(malformed()),
    }
}

/// Encodes the journal rows. Their path ids name nodes of `paths`, the path
/// trie of the recording that the journal belongs to (C7b, DR-86).
pub fn encode_native_operation_journal(
    operations: &[NativeOperationRecord],
    paths: &NativePathTrie,
    limits: NativeRecordingWireLimits,
) -> Result<Vec<u8>, InterpreterError> {
    if operations.len() > limits.operations {
        return Err(malformed());
    }
    let mut wire = Writer {
        wire: PhloWireEncoder::new(limits.wire),
    };
    wire.bytes(JOURNAL_DOMAIN_V2)?;
    wire.u64(length(operations.len())?)?;
    let (mut sources, mut footprints, mut footprint_bytes, mut edges) = (0, 0, 0, 0);
    let mut previous = PathId::ROOT;
    for row in operations {
        wire.bytes(&row.occurrence.session)?;
        write_path_delta(
            &mut wire,
            paths,
            row.occurrence.path,
            previous,
            limits.budget.path_segments,
        )?;
        previous = row.occurrence.path;
        add_count(&mut sources, 1, limits.source_entries)?;
        match &row.source {
            NativeOperationSource::Produce(source) => {
                wire.u8(0)?;
                write_produce(&mut wire, source)?;
            }
            NativeOperationSource::Consume(source) => {
                wire.u8(1)?;
                write_consume(&mut wire, source, &mut sources, limits)?;
            }
        }
        match &row.consume_peeks {
            None => wire.u8(0)?,
            Some(peeks) => {
                wire.u8(1)?;
                add_count(&mut sources, peeks.len(), limits.source_entries)?;
                wire.u64(length(peeks.len())?)?;
                for &index in peeks.iter() {
                    write_i32(&mut wire, index)?;
                }
            }
        }
        add_count(
            &mut footprints,
            row.footprint.len(),
            limits.footprint_entries,
        )?;
        wire.u64(length(row.footprint.len())?)?;
        for channel in row.footprint.iter() {
            add_count(&mut footprint_bytes, channel.len(), limits.footprint_bytes)?;
            wire.bytes(channel)?;
        }
        add_count(&mut edges, row.predecessors.len(), limits.predecessor_edges)?;
        wire.u64(length(row.predecessors.len())?)?;
        for &index in row.predecessors.iter() {
            wire.u64(length(index)?)?;
        }
        write_link(&mut wire, row.introduction)?;
        match &row.comm {
            None => wire.u8(0)?,
            Some(comm) => {
                wire.u8(1)?;
                write_comm(&mut wire, comm, &mut sources, limits)?;
            }
        }
        wire.u8(completion_tag(row.completion))?;
        wire.u64(length(row.budget_start)?)?;
        wire.u64(length(row.budget_end)?)?;
    }
    Ok(wire.finish())
}

/// Decodes the journal rows into the path trie of `recording`, which must not
/// be shared. The Σ s_i limit counts the recording and the journal together
/// (C7b, DR-86).
pub fn decode_native_operation_journal(
    input: &[u8],
    recording: &mut NativeBudgetRecording,
    limits: NativeRecordingWireLimits,
) -> Result<Arc<[NativeOperationRecord]>, InterpreterError> {
    let paths = Arc::get_mut(&mut recording.paths).ok_or_else(|| {
        recording_error("native operation journal needs the recording's unshared path trie")
    })?;
    let mut wire = Reader::new(input, limits.wire)?;
    let delta = match wire.bytes()? {
        JOURNAL_DOMAIN => false,
        JOURNAL_DOMAIN_V2 => true,
        _ => return Err(malformed()),
    };
    let count = bounded_count(wire.u64()?, limits.operations)?;
    let mut operations = Vec::<NativeOperationRecord>::with_capacity(count);
    let (mut sources, mut footprints, mut footprint_bytes, mut edges) = (0, 0, 0, 0);
    for _ in 0..count {
        let previous = operations
            .last()
            .map_or(PathId::ROOT, |row| row.occurrence.path);
        let occurrence = NativeOperationOccurrence {
            session: wire.fixed32()?,
            path: if delta {
                read_path_delta(
                    &mut wire,
                    paths,
                    previous,
                    limits.budget.path_segments,
                    limits.total_path_segments,
                )?
            } else {
                read_path(
                    &mut wire,
                    paths,
                    limits.budget.path_segments,
                    limits.total_path_segments,
                )?
            },
        };
        add_count(&mut sources, 1, limits.source_entries)?;
        let source = match wire.u8()? {
            0 => NativeOperationSource::Produce(read_produce(&mut wire)?),
            1 => NativeOperationSource::Consume(read_consume(&mut wire, &mut sources, limits)?),
            _ => return Err(malformed()),
        };
        let consume_peeks = match wire.u8()? {
            0 => None,
            1 => {
                let count =
                    bounded_count(wire.u64()?, limits.source_entries.saturating_sub(sources))?;
                add_count(&mut sources, count, limits.source_entries)?;
                let mut peeks = Vec::with_capacity(count);
                for _ in 0..count {
                    peeks.push(read_i32(&mut wire)?);
                }
                Some(Arc::from(peeks))
            }
            _ => return Err(malformed()),
        };
        let count = bounded_count(
            wire.u64()?,
            limits.footprint_entries.saturating_sub(footprints),
        )?;
        add_count(&mut footprints, count, limits.footprint_entries)?;
        let mut footprint = Vec::with_capacity(count);
        for _ in 0..count {
            let bytes = wire.bytes()?;
            add_count(&mut footprint_bytes, bytes.len(), limits.footprint_bytes)?;
            footprint.push(Arc::from(bytes));
        }
        let count = bounded_count(wire.u64()?, limits.predecessor_edges.saturating_sub(edges))?;
        add_count(&mut edges, count, limits.predecessor_edges)?;
        let mut predecessors = Vec::with_capacity(count);
        for _ in 0..count {
            predecessors.push(bounded_count(wire.u64()?, limits.operations)?);
        }
        let introduction = read_link(&mut wire, limits.budget)?;
        let comm = match wire.u8()? {
            0 => None,
            1 => Some(read_comm(&mut wire, &mut sources, limits)?),
            _ => return Err(malformed()),
        };
        let completion = completion(wire.u8()?)?;
        let budget_start = bounded_count(wire.u64()?, limits.budget.attempts)?;
        let budget_end = bounded_count(wire.u64()?, limits.budget.attempts)?;
        operations.push(NativeOperationRecord {
            occurrence,
            source,
            consume_peeks,
            footprint: footprint.into(),
            predecessors: predecessors.into(),
            introduction,
            comm,
            completion,
            budget_start,
            budget_end,
        });
    }
    wire.finish()?;
    Ok(operations.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::accounting::native_phlo_rules::NativePhloRegionLimits;

    fn limits() -> NativeRecordingWireLimits {
        NativeRecordingWireLimits {
            wire: PhloWireLimits {
                total_bytes: 16_384,
                field_bytes: 4_096,
            },
            budget: NativeBudgetTraceLimits {
                attempts: 8,
                path_segments: 4,
                regions: NativePhloRegionLimits {
                    regions: 8,
                    encoded_authority_bytes: 4_096,
                },
            },
            operations: 8,
            source_entries: 32,
            footprint_entries: 16,
            footprint_bytes: 256,
            predecessor_edges: 16,
            total_path_segments: 64,
        }
    }

    /// A recording whose trie holds the paths [(7, 8)] and [(7, 9)] in wire
    /// order: the attempt, then the retry.
    fn recording() -> NativeBudgetRecording {
        let mut paths = NativePathTrie::new(limits().budget.path_segments);
        let first = paths.intern(PathId::ROOT, &[(7, 8)]).expect("path fits");
        let second = paths.intern(PathId::ROOT, &[(7, 9)]).expect("path fits");
        let observation = Arc::new(ByteObservation {
            event_id: [2; 32],
            kind: AuthorityByteEventKind::ProduceIntroduction,
            authority: CostAuthority::default(),
            measurement: Some(ByteCharge {
                introduction_bytes: 3,
                transfer_bytes: 4,
                trace_bytes: 5,
            }),
            legacy_amount: None,
        });
        NativeBudgetRecording {
            session: [1; 32],
            attempts: vec![NativeBudgetAttempt {
                occurrence: NativeBudgetOccurrence {
                    session: [1; 32],
                    path: first,
                    stage: NativeAttemptStage::ProduceIntroduction,
                },
                observation: Arc::clone(&observation),
                granted: true,
                cause: NativeDenialCause::Budget,
            }]
            .into(),
            retries: vec![NativeBudgetRetry {
                occurrence: NativeBudgetOccurrence {
                    session: [1; 32],
                    path: second,
                    stage: NativeAttemptStage::ProduceIntroduction,
                },
                observation,
                accepted_attempt: 0,
                fresh_before: 1,
            }]
            .into(),
            transfers: Arc::from([]),
            used: 12,
            paths: Arc::new(paths),
        }
    }

    /// Journal rows whose paths are nodes of the trie of [`recording`].
    fn journal(recording: &NativeBudgetRecording) -> Arc<[NativeOperationRecord]> {
        let node = |path: &[(u64, u64)]| recording.paths.find(path).expect("fixture path");
        let first = NativeOperationRecord {
            occurrence: NativeOperationOccurrence {
                session: [1; 32],
                path: node(&[(7, 8)]),
            },
            source: NativeOperationSource::Produce(NativeProduceSource {
                channel: [3; 32],
                hash: [4; 32],
                persistent: true,
            }),
            consume_peeks: None,
            footprint: vec![Arc::from(&b"channel"[..])].into(),
            predecessors: Arc::from([]),
            introduction: NativeObservationLink::Attempt(0),
            comm: None,
            completion: RSpaceOperationCompletion::Stored,
            budget_start: 0,
            budget_end: 1,
        };
        let produce = NativeProduceSource {
            channel: [5; 32],
            hash: [6; 32],
            persistent: false,
        };
        let consume = NativeConsumeSource {
            channels: vec![[3; 32]].into(),
            hash: [7; 32],
            persistent: false,
        };
        let second = NativeOperationRecord {
            occurrence: NativeOperationOccurrence {
                session: [1; 32],
                path: node(&[(7, 9)]),
            },
            source: NativeOperationSource::Consume(consume.clone()),
            consume_peeks: Some(vec![0].into()),
            footprint: vec![Arc::from(&b"channel-2"[..])].into(),
            predecessors: vec![0].into(),
            introduction: NativeObservationLink::Retry(0),
            comm: Some(NativeCommRecord {
                source: Arc::new(NativeCommSource {
                    consume,
                    produces: vec![produce.clone()].into(),
                    peeks: vec![0].into(),
                    repetitions: vec![(produce, -2)].into(),
                }),
                observation: NativeObservationLink::Attempt(0),
            }),
            completion: RSpaceOperationCompletion::Matched,
            budget_start: 1,
            budget_end: 1,
        };
        vec![first, second].into()
    }

    #[test]
    fn canonical_recording_and_journal_round_trip() {
        let original = recording();
        let bytes = encode_native_budget_recording(&original, limits()).unwrap();
        let mut decoded = decode_native_budget_recording(&bytes, limits()).unwrap();
        assert_eq!(decoded.session, original.session);
        assert_eq!(decoded.used, original.used);
        assert_eq!(decoded.attempts.as_ref(), original.attempts.as_ref());
        assert_eq!(
            decoded.retries[0].occurrence,
            original.retries[0].occurrence
        );
        assert_eq!(
            decoded.retries[0].observation,
            original.retries[0].observation
        );
        assert_eq!(decoded.paths, original.paths);
        assert_eq!(
            encode_native_budget_recording(&decoded, limits()).unwrap(),
            bytes
        );
        let rows = journal(&original);
        let bytes = encode_native_operation_journal(&rows, &original.paths, limits()).unwrap();
        let decoded_rows = decode_native_operation_journal(&bytes, &mut decoded, limits()).unwrap();
        assert_eq!(decoded_rows.as_ref(), rows.as_ref());
        assert_eq!(decoded.paths, original.paths);
        assert_eq!(
            encode_native_operation_journal(&decoded_rows, &decoded.paths, limits()).unwrap(),
            bytes
        );
    }

    #[test]
    fn historical_recording_decodes_and_noncanonical_delta_is_rejected() {
        let original = recording();
        let mut writer = Writer {
            wire: PhloWireEncoder::new(limits().wire),
        };
        writer.bytes(BUDGET_DOMAIN).unwrap();
        writer.bytes(&original.session).unwrap();
        writer.u64(original.used).unwrap();
        writer.u64(1).unwrap();
        let attempt = &original.attempts[0];
        write_occurrence(
            &mut writer,
            &original.paths,
            &attempt.occurrence,
            limits().budget,
        )
        .unwrap();
        write_observation(&mut writer, &attempt.observation).unwrap();
        writer.u8(u8::from(attempt.granted)).unwrap();
        writer.u64(1).unwrap();
        let retry = &original.retries[0];
        write_occurrence(
            &mut writer,
            &original.paths,
            &retry.occurrence,
            limits().budget,
        )
        .unwrap();
        write_observation(&mut writer, &retry.observation).unwrap();
        writer.u64(retry.accepted_attempt as u64).unwrap();
        writer.u64(retry.fresh_before as u64).unwrap();
        let decoded = decode_native_budget_recording(&writer.finish(), limits()).unwrap();
        assert_eq!(decoded.session, original.session);
        assert_eq!(decoded.used, original.used);
        assert_eq!(decoded.attempts.as_ref(), original.attempts.as_ref());
        assert_eq!(
            decoded.retries[0].occurrence,
            original.retries[0].occurrence
        );
        assert_eq!(
            decoded.retries[0].observation,
            original.retries[0].observation
        );
        assert_eq!(decoded.paths, original.paths);

        let mut writer = Writer {
            wire: PhloWireEncoder::new(limits().wire),
        };
        writer.u64(0).unwrap();
        writer.u64(1).unwrap();
        writer.u64(7).unwrap();
        writer.u64(8).unwrap();
        let bytes = writer.finish();
        let mut reader = Reader::new(&bytes, limits().wire).unwrap();
        let mut paths = NativePathTrie::new(limits().budget.path_segments);
        let previous = paths.intern(PathId::ROOT, &[(7, 8)]).unwrap();
        assert!(read_path_delta(
            &mut reader,
            &mut paths,
            previous,
            limits().budget.path_segments,
            limits().total_path_segments,
        )
        .is_err());
    }

    #[test]
    fn historical_operation_journal_decodes() {
        let mut recording = recording();
        let rows = journal(&recording);
        let row = &rows[0];
        let mut writer = Writer {
            wire: PhloWireEncoder::new(limits().wire),
        };
        writer.bytes(JOURNAL_DOMAIN).unwrap();
        writer.u64(1).unwrap();
        writer.bytes(&row.occurrence.session).unwrap();
        write_path(
            &mut writer,
            &recording.paths.segments(row.occurrence.path),
            limits().budget.path_segments,
        )
        .unwrap();
        writer.u8(0).unwrap();
        let NativeOperationSource::Produce(source) = &row.source else {
            unreachable!()
        };
        write_produce(&mut writer, source).unwrap();
        writer.u8(0).unwrap();
        writer.u64(1).unwrap();
        writer.bytes(&row.footprint[0]).unwrap();
        writer.u64(0).unwrap();
        write_link(&mut writer, row.introduction).unwrap();
        writer.u8(0).unwrap();
        writer.u8(completion_tag(row.completion)).unwrap();
        writer.u64(row.budget_start as u64).unwrap();
        writer.u64(row.budget_end as u64).unwrap();
        let decoded =
            decode_native_operation_journal(&writer.finish(), &mut recording, limits()).unwrap();
        assert_eq!(decoded.as_ref(), &rows[..1]);
    }

    #[test]
    fn malformed_or_over_limit_records_fail_before_publication() {
        let original = recording();
        let bytes = encode_native_budget_recording(&original, limits()).unwrap();
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_native_budget_recording(&trailing, limits()).is_err());
        let mut short = limits();
        short.wire.total_bytes = bytes.len() - 1;
        assert!(decode_native_budget_recording(&bytes, short).is_err());
        let mut short = limits();
        short.budget.attempts = 1;
        assert!(decode_native_budget_recording(&bytes, short).is_err());
        let rows = journal(&original);
        let bytes = encode_native_operation_journal(&rows, &original.paths, limits()).unwrap();
        let mut short = limits();
        short.footprint_bytes = 1;
        assert!(decode_native_operation_journal(&bytes, &mut recording(), short).is_err());
    }

    /// C7b (DR-86): the Σ s_i limit is checked from the wire header before
    /// any node is allocated, and it counts a recording and its journal
    /// together.
    #[test]
    fn delta_limit_checked_before_allocation() {
        let mut writer = Writer {
            wire: PhloWireEncoder::new(limits().wire),
        };
        writer.u64(0).unwrap();
        writer.u64(3).unwrap();
        for segment in 0..3 {
            writer.u64(segment).unwrap();
            writer.u64(0).unwrap();
        }
        let bytes = writer.finish();
        for total in [0, 1, 2] {
            let mut paths = NativePathTrie::new(4);
            let mut reader = Reader::new(&bytes, limits().wire).unwrap();
            assert!(read_path_delta(&mut reader, &mut paths, PathId::ROOT, 4, total).is_err());
            assert_eq!(paths.node_count(), 0);
            let mut reader = Reader::new(&bytes[8..], limits().wire).unwrap();
            assert!(read_path(&mut reader, &mut paths, 4, total).is_err());
            assert_eq!(paths.node_count(), 0);
        }
        let mut paths = NativePathTrie::new(4);
        let mut reader = Reader::new(&bytes, limits().wire).unwrap();
        let node = read_path_delta(&mut reader, &mut paths, PathId::ROOT, 4, 3).unwrap();
        assert_eq!(paths.segments(node), vec![(0, 0), (1, 0), (2, 0)]);

        let original = recording();
        let recording_bytes = encode_native_budget_recording(&original, limits()).unwrap();
        let journal_bytes =
            encode_native_operation_journal(&journal(&original), &original.paths, limits())
                .unwrap();
        let mut tight = limits();
        tight.total_path_segments = 2;
        let mut decoded = decode_native_budget_recording(&recording_bytes, tight).unwrap();
        assert!(decode_native_operation_journal(&journal_bytes, &mut decoded, tight).is_err());
        tight.total_path_segments = 4;
        let mut decoded = decode_native_budget_recording(&recording_bytes, tight).unwrap();
        assert!(decode_native_operation_journal(&journal_bytes, &mut decoded, tight).is_ok());
    }

    fn shared_prefix(path: &[(u64, u64)], previous: &[(u64, u64)]) -> usize {
        path.iter()
            .zip(previous)
            .take_while(|(left, right)| left == right)
            .count()
    }

    fn path_chain() -> impl proptest::strategy::Strategy<Value = Vec<Vec<(u64, u64)>>> {
        proptest::collection::vec(proptest::collection::vec((0u64..3, 0u64..3), 0..7), 0..8)
    }

    /// Encodes `paths` as one delta chain with the segment encoder.
    fn segment_chain_bytes(paths: &[Vec<(u64, u64)>], maximum: usize) -> Vec<u8> {
        let mut writer = Writer {
            wire: PhloWireEncoder::new(limits().wire),
        };
        let mut previous: &[(u64, u64)] = &[];
        for path in paths {
            write_path_delta_segments(&mut writer, path, previous, maximum)
                .expect("bounded path encodes");
            previous = path;
        }
        writer.finish()
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

        /// Extracted from `NativePathDeltaCodec.v`: `delta_codec_round_trip`,
        /// `delta_codec_canonical`, and `delta_encoding_unique`. A random path chain
        /// round-trips exactly, consumes all bytes, and every non-maximal prefix
        /// encoding of every path is rejected.
        #[test]
        fn path_delta_chain_round_trips_and_rejects_every_noncanonical_prefix(
            paths in path_chain(),
        ) {
            let maximum = 6;
            let bytes = segment_chain_bytes(&paths, maximum);
            let mut reader = Reader::new(&bytes, limits().wire).expect("reader opens");
            let mut trie = NativePathTrie::new(maximum);
            let mut ids = Vec::new();
            let mut decoded_previous = PathId::ROOT;
            for path in &paths {
                let decoded = read_path_delta(&mut reader, &mut trie, decoded_previous, maximum, usize::MAX)
                    .expect("canonical encoding decodes");
                proptest::prop_assert_eq!(&trie.segments(decoded), path);
                ids.push(decoded);
                decoded_previous = decoded;
            }
            proptest::prop_assert!(reader.finish().is_ok());

            let mut previous: Vec<(u64, u64)> = Vec::new();
            let mut previous_id = PathId::ROOT;
            for (path, id) in paths.iter().zip(&ids) {
                for prefix in 0..shared_prefix(path, &previous) {
                    let mut writer = Writer {
                        wire: PhloWireEncoder::new(limits().wire),
                    };
                    writer.u64(prefix as u64).expect("prefix encodes");
                    writer.u64((path.len() - prefix) as u64).expect("length encodes");
                    for &(left, right) in &path[prefix..] {
                        writer.u64(left).expect("segment encodes");
                        writer.u64(right).expect("segment encodes");
                    }
                    let bytes = writer.finish();
                    let mut reader = Reader::new(&bytes, limits().wire)
                        .expect("reader opens");
                    let mut attempt = trie.clone();
                    proptest::prop_assert!(
                        read_path_delta(&mut reader, &mut attempt, previous_id, maximum, usize::MAX).is_err(),
                        "non-maximal prefix {} accepted for {:?} after {:?}",
                        prefix,
                        path,
                        previous
                    );
                }
                previous = path.clone();
                previous_id = *id;
            }
        }

        /// C7b (DR-86; `NativePathTrie.trie_decode_denotes_paths`): the trie
        /// encoder writes the bytes of the segment encoder. The v2 decoder and
        /// the v1 decoder both rebuild the encoder's trie with the same ids,
        /// for a recording chain followed by a journal chain that restarts at
        /// the empty path, and re-encoding the decoded ids gives the same
        /// bytes.
        #[test]
        fn trie_decode_round_trips_v1_v2(
            recording_paths in path_chain(),
            journal_paths in path_chain(),
        ) {
            let maximum = 6;
            let mut trie = NativePathTrie::new(maximum);
            let mut chains = Vec::new();
            for chain in [&recording_paths, &journal_paths] {
                let mut previous: (&[(u64, u64)], PathId) = (&[], PathId::ROOT);
                let mut ids = Vec::new();
                for path in chain.iter() {
                    let id = trie.intern_delta(path, previous.0, previous.1).expect("path fits");
                    ids.push(id);
                    previous = (path, id);
                }
                chains.push(ids);
            }
            let mut v2 = Vec::new();
            for (ids, paths) in chains.iter().zip([&recording_paths, &journal_paths]) {
                let mut writer = Writer {
                    wire: PhloWireEncoder::new(limits().wire),
                };
                let mut previous = PathId::ROOT;
                for id in ids {
                    write_path_delta(&mut writer, &trie, *id, previous, maximum).expect("path encodes");
                    previous = *id;
                }
                let bytes = writer.finish();
                proptest::prop_assert_eq!(&bytes, &segment_chain_bytes(paths, maximum));
                v2.push(bytes);
            }

            let mut decoded = NativePathTrie::new(maximum);
            for (bytes, ids) in v2.iter().zip(&chains) {
                let mut reader = Reader::new(bytes, limits().wire).expect("reader opens");
                let mut previous = PathId::ROOT;
                for id in ids {
                    let node = read_path_delta(&mut reader, &mut decoded, previous, maximum, usize::MAX)
                        .expect("canonical encoding decodes");
                    proptest::prop_assert_eq!(node, *id);
                    previous = node;
                }
                proptest::prop_assert!(reader.finish().is_ok());
            }
            proptest::prop_assert_eq!(&decoded, &trie);

            let mut historical = NativePathTrie::new(maximum);
            for (chain, ids) in [&recording_paths, &journal_paths].into_iter().zip(&chains) {
                let mut writer = Writer {
                    wire: PhloWireEncoder::new(limits().wire),
                };
                for path in chain.iter() {
                    write_path(&mut writer, path, maximum).expect("path encodes");
                }
                let bytes = writer.finish();
                let mut reader = Reader::new(&bytes, limits().wire).expect("reader opens");
                for id in ids {
                    let node = read_path(&mut reader, &mut historical, maximum, usize::MAX)
                        .expect("historical path decodes");
                    proptest::prop_assert_eq!(node, *id);
                }
            }
            proptest::prop_assert_eq!(&historical, &trie);
        }
    }
}
