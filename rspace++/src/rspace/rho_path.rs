use std::fmt;

use pathmap::PathMap;

use super::hashing::stable_hash_provider::StableHashSerialize;

const MAGIC: [u8; 4] = *b"RHP\x01";
const DATUM_KIND: u8 = 1;
const CONTINUATION_KIND: u8 = 2;
const JOIN_KIND: u8 = 3;
const ESCAPE: u8 = 0;
const ESCAPED_ZERO: u8 = 0xFF;
const FRAME_END: u8 = 0;
// An impossible pair between escaped frames. Frame data encode zero as
// `[0, 255]` and frame termination as `[0, 0]`, so `[0, 1]` can safely mark
// the end of an exact channel group before a record-identity suffix begins.
const GROUP_END: [u8; 2] = [ESCAPE, 1];

pub const RHO_MATCH_PATH_VERSION: [u8; 4] = *b"RMP\x02";
pub const RHO_MATCH_NODE_START: u8 = 0x01;
pub const RHO_MATCH_NODE_END: u8 = 0x02;
pub const RHO_MATCH_FIELD_START: u8 = 0x03;
pub const RHO_MATCH_FIELD_END: u8 = 0x04;
pub const RHO_MATCH_BOOL: u8 = 0x10;
pub const RHO_MATCH_I32: u8 = 0x11;
pub const RHO_MATCH_U32: u8 = 0x12;
pub const RHO_MATCH_I64: u8 = 0x13;
pub const RHO_MATCH_U64: u8 = 0x14;
pub const RHO_MATCH_BYTES: u8 = 0x15;
pub const RHO_MATCH_STRING: u8 = 0x16;
pub const RHO_MATCH_STRING_SEQUENCE: u8 = 0x17;
pub const RHO_MATCH_BYTES_SEQUENCE: u8 = 0x18;
pub const RHO_MATCH_ABSENT: u8 = 0x20;
pub const RHO_MATCH_SEQUENCE_START: u8 = 0x21;
pub const RHO_MATCH_SEQUENCE_END: u8 = 0x22;
pub const RHO_MATCH_MAP_START: u8 = 0x23;
pub const RHO_MATCH_MAP_KEY: u8 = 0x24;
pub const RHO_MATCH_MAP_END: u8 = 0x25;
pub const RHO_MATCH_ONEOF_START: u8 = 0x26;
pub const RHO_MATCH_ONEOF_END: u8 = 0x27;
pub const RHO_MATCH_PATHMAP: u8 = 0x28;
/// Repository-owned `ParBag` summary: distinct endpoints, total cardinality,
/// and the domain-separated digest of its canonical, multiplicity-compressed
/// component transcript. No ACT/PathMap storage bytes enter this token.
pub const RHO_MATCH_PAR_BAG: u8 = 0x29;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RhoPathCaptureId(u32);

impl RhoPathCaptureId {
    pub const fn new(id: u32) -> Self { Self(id) }

    pub const fn get(self) -> u32 { self.0 }

    pub(crate) const fn path(self) -> [u8; 4] { self.0.to_be_bytes() }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RhoPathQueryAtom {
    Exact(Vec<u8>),
    Capture(RhoPathCaptureId),
    Wildcard,
    Match(RhoMatchQuery),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CompiledRhoPathQueryAtom {
    Exact {
        inner_frame: Vec<u8>,
        stored_frame: Vec<u8>,
    },
    Capture {
        slot: u32,
    },
    Wildcard,
    Match(RhoMatchQuery),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RhoMatchQueryPart {
    Exact(Vec<u8>),
    Capture(RhoPathCaptureId),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CompiledRhoMatchQueryPart {
    Exact {
        inner_frame: Vec<u8>,
        stored_frame: Vec<u8>,
    },
    Capture(RhoPathCaptureId),
    Wildcard,
}

/// A stack-safe structural query over one complete `RMP2` value.
///
/// Exact runs are stored in the same escaped form used by the enclosing
/// RhoPathV1 datum-structure frame. Wildcards and captures consume exactly one
/// self-delimiting RMP2 value. Repeated capture identifiers compare their
/// canonical escaped byte slices without decoding a `Par`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RhoMatchQuery {
    parts: Vec<CompiledRhoMatchQueryPart>,
}

impl RhoMatchQuery {
    pub fn new(parts: impl IntoIterator<Item = RhoMatchQueryPart>) -> Self {
        let parts = parts.into_iter().collect::<Vec<_>>();
        let mut capture_counts = PathMap::new();
        for part in &parts {
            if let RhoMatchQueryPart::Capture(id) = part {
                let count = capture_counts.get(id.path()).copied().unwrap_or(0u32);
                capture_counts.insert(id.path(), count.saturating_add(1));
            }
        }
        let mut compiled = Vec::with_capacity(parts.len());
        for part in parts {
            let next = match part {
                RhoMatchQueryPart::Exact(bytes) => {
                    let mut inner_frame = Vec::with_capacity(bytes.len());
                    append_frame_prefix(&mut inner_frame, &bytes);
                    let mut stored_frame = Vec::with_capacity(inner_frame.len());
                    append_frame_prefix(&mut stored_frame, &inner_frame);
                    CompiledRhoMatchQueryPart::Exact {
                        inner_frame,
                        stored_frame,
                    }
                }
                RhoMatchQueryPart::Capture(id)
                    if capture_counts.get(id.path()).copied().unwrap_or(0) > 1 =>
                {
                    CompiledRhoMatchQueryPart::Capture(id)
                }
                RhoMatchQueryPart::Capture(_) | RhoMatchQueryPart::Wildcard => {
                    CompiledRhoMatchQueryPart::Wildcard
                }
            };
            match (compiled.last_mut(), next) {
                (
                    Some(CompiledRhoMatchQueryPart::Exact {
                        inner_frame: previous_inner,
                        stored_frame: previous_stored,
                    }),
                    CompiledRhoMatchQueryPart::Exact {
                        inner_frame: next_inner,
                        stored_frame: next_stored,
                    },
                ) => {
                    previous_inner.extend_from_slice(&next_inner);
                    previous_stored.extend_from_slice(&next_stored);
                }
                (_, next) => compiled.push(next),
            }
        }
        Self { parts: compiled }
    }

    pub fn matches_stored_frame(&self, frame: &[u8]) -> bool { self.matches_frame(frame, 1) }

    pub(crate) fn matches_product_frame(&self, frame: &[u8]) -> bool {
        self.matches_frame(frame, 2)
    }

    pub(crate) fn product_prefix(&self) -> &[u8] {
        match self.parts.first() {
            Some(CompiledRhoMatchQueryPart::Exact { stored_frame, .. }) => stored_frame,
            Some(CompiledRhoMatchQueryPart::Capture(_) | CompiledRhoMatchQueryPart::Wildcard) |
            None => &[],
        }
    }

    fn inner_prefix(&self) -> &[u8] {
        match self.parts.first() {
            Some(CompiledRhoMatchQueryPart::Exact { inner_frame, .. }) => inner_frame,
            Some(CompiledRhoMatchQueryPart::Capture(_) | CompiledRhoMatchQueryPart::Wildcard) |
            None => &[],
        }
    }

    fn is_exact(&self) -> bool {
        !self.parts.is_empty() &&
            self.parts
                .iter()
                .all(|part| matches!(part, CompiledRhoMatchQueryPart::Exact { .. }))
    }

    fn matches_frame(&self, frame: &[u8], escape_layers: u8) -> bool {
        let mut cursor = 0usize;
        let mut captures: PathMap<(usize, usize)> = PathMap::new();
        for part in &self.parts {
            match part {
                CompiledRhoMatchQueryPart::Exact {
                    inner_frame,
                    stored_frame,
                } => {
                    let bytes = if escape_layers == 1 {
                        inner_frame
                    } else {
                        stored_frame
                    };
                    let Some(end) = cursor.checked_add(bytes.len()) else {
                        return false;
                    };
                    if frame.get(cursor..end) != Some(bytes.as_slice()) {
                        return false;
                    }
                    cursor = end;
                }
                CompiledRhoMatchQueryPart::Capture(id) => {
                    let start = cursor;
                    if skip_rmp_value(frame, &mut cursor, escape_layers).is_err() {
                        return false;
                    }
                    let path = id.path();
                    match captures.get(path).copied() {
                        Some((previous_start, previous_end))
                            if frame[previous_start..previous_end] != frame[start..cursor] =>
                        {
                            return false;
                        }
                        Some(_) => {}
                        None => {
                            captures.insert(path, (start, cursor));
                        }
                    }
                }
                CompiledRhoMatchQueryPart::Wildcard => {
                    if skip_rmp_value(frame, &mut cursor, escape_layers).is_err() {
                        return false;
                    }
                }
            }
        }
        let end = if escape_layers == 1 {
            &[ESCAPE, FRAME_END][..]
        } else {
            &[ESCAPE, ESCAPED_ZERO, ESCAPE, ESCAPED_ZERO][..]
        };
        frame.get(cursor..cursor.saturating_add(end.len())) == Some(end) &&
            cursor + end.len() == frame.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RhoPathQuery {
    atoms: Vec<CompiledRhoPathQueryAtom>,
    capture_slot_count: u32,
    accepts_remainder: bool,
}

impl RhoPathQuery {
    pub fn structural_sequence(
        atoms: impl IntoIterator<Item = RhoPathQueryAtom>,
        accepts_remainder: bool,
    ) -> Self {
        let atoms = atoms.into_iter().collect::<Vec<_>>();
        let mut capture_counts = PathMap::new();
        for atom in &atoms {
            if let RhoPathQueryAtom::Capture(id) = atom {
                let path = id.path();
                let count = capture_counts.get(path).copied().unwrap_or(0u32);
                capture_counts.insert(path, count.saturating_add(1));
            }
        }
        let mut capture_slots = PathMap::new();
        let mut capture_slot_count = 0u32;
        let atoms = atoms
            .into_iter()
            .map(|atom| match atom {
                RhoPathQueryAtom::Exact(bytes) => {
                    let mut inner_frame = Vec::with_capacity(bytes.len() + 2);
                    append_frame(&mut inner_frame, &bytes);
                    let mut stored_frame = Vec::with_capacity(inner_frame.len());
                    append_frame_prefix(&mut stored_frame, &inner_frame);
                    CompiledRhoPathQueryAtom::Exact {
                        inner_frame,
                        stored_frame,
                    }
                }
                RhoPathQueryAtom::Capture(id)
                    if capture_counts.get(id.path()).copied().unwrap_or(0) > 1 =>
                {
                    let slot = match capture_slots.get(id.path()).copied() {
                        Some(slot) => slot,
                        None => {
                            let slot = capture_slot_count;
                            capture_slot_count = capture_slot_count
                                .checked_add(1)
                                .expect("unique u32 capture identifiers fit in u32 slots");
                            capture_slots.insert(id.path(), slot);
                            slot
                        }
                    };
                    CompiledRhoPathQueryAtom::Capture { slot }
                }
                RhoPathQueryAtom::Capture(_) | RhoPathQueryAtom::Wildcard => {
                    CompiledRhoPathQueryAtom::Wildcard
                }
                RhoPathQueryAtom::Match(query) => CompiledRhoPathQueryAtom::Match(query),
            })
            .collect();
        Self {
            atoms,
            capture_slot_count,
            accepts_remainder,
        }
    }

    pub fn is_selective(&self) -> bool { !(self.atoms.is_empty() && self.accepts_remainder) }

    pub fn matches_structure(&self, structure: &[u8]) -> bool {
        if structure.first() != Some(&1) {
            return false;
        }
        let mut cursor = 1usize;
        let mut captures: Vec<Option<Vec<u8>>> = vec![None; self.capture_slot_count as usize];
        for atom in &self.atoms {
            let frame = match take_frame(structure, &mut cursor) {
                Ok(frame) => frame,
                Err(_) => return false,
            };
            match atom {
                CompiledRhoPathQueryAtom::Exact { inner_frame, .. } => {
                    if frame != inner_frame {
                        return false;
                    }
                }
                CompiledRhoPathQueryAtom::Capture { slot } => {
                    let slot = *slot as usize;
                    match captures[slot].as_ref() {
                        Some(previous) if previous.as_slice() != frame => return false,
                        Some(_) => {}
                        None => {
                            captures[slot] = Some(frame.to_vec());
                        }
                    }
                }
                CompiledRhoPathQueryAtom::Wildcard => {}
                CompiledRhoPathQueryAtom::Match(query) => {
                    if !query.matches_stored_frame(frame) {
                        return false;
                    }
                }
            }
        }
        if !self.accepts_remainder {
            return cursor == structure.len();
        }
        while cursor < structure.len() {
            if take_frame(structure, &mut cursor).is_err() {
                return false;
            }
        }
        true
    }

    pub(crate) fn atoms(&self) -> &[CompiledRhoPathQueryAtom] { &self.atoms }

    pub(crate) const fn accepts_remainder(&self) -> bool { self.accepts_remainder }

    /// Longest exact byte prefix usable by a retained query trie.
    ///
    /// The returned bytes are in the unframed datum-structure representation
    /// consumed by [`Self::matches_structure`]. A nested match contributes its
    /// exact RMP2 prefix without closing the surrounding structural frame.
    pub fn retained_prefix(&self) -> Vec<u8> {
        let mut prefix = vec![1];
        for atom in &self.atoms {
            match atom {
                CompiledRhoPathQueryAtom::Exact { inner_frame, .. } => {
                    prefix.extend_from_slice(inner_frame);
                }
                CompiledRhoPathQueryAtom::Match(query) => {
                    prefix.extend_from_slice(query.inner_prefix());
                    if query.is_exact() {
                        prefix.extend_from_slice(&[ESCAPE, FRAME_END]);
                    } else {
                        break;
                    }
                }
                CompiledRhoPathQueryAtom::Capture { .. } | CompiledRhoPathQueryAtom::Wildcard => {
                    break;
                }
            }
        }
        prefix
    }

    pub(crate) const fn capture_slot_count(&self) -> u32 { self.capture_slot_count }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredSequenceFrameStep {
    NeedMore,
    Complete,
    Invalid,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StoredSequenceFrameScanner {
    outer_escape: bool,
    inner_escape: bool,
}

impl StoredSequenceFrameScanner {
    pub(crate) fn feed(&mut self, byte: u8) -> StoredSequenceFrameStep {
        if self.outer_escape {
            self.outer_escape = false;
            return match byte {
                ESCAPED_ZERO => self.feed_inner(ESCAPE),
                FRAME_END => StoredSequenceFrameStep::Invalid,
                _ => StoredSequenceFrameStep::Invalid,
            };
        }
        if byte == ESCAPE {
            self.outer_escape = true;
            StoredSequenceFrameStep::NeedMore
        } else {
            self.feed_inner(byte)
        }
    }

    fn feed_inner(&mut self, byte: u8) -> StoredSequenceFrameStep {
        if self.inner_escape {
            self.inner_escape = false;
            return match byte {
                ESCAPED_ZERO => StoredSequenceFrameStep::NeedMore,
                FRAME_END => StoredSequenceFrameStep::Complete,
                _ => StoredSequenceFrameStep::Invalid,
            };
        }
        if byte == ESCAPE {
            self.inner_escape = true;
        }
        StoredSequenceFrameStep::NeedMore
    }
}

fn skip_rmp_value(frame: &[u8], cursor: &mut usize, escape_layers: u8) -> Result<(), ()> {
    let mut closers = Vec::new();
    let mut root_started = false;
    loop {
        let token = take_logical_byte(frame, cursor, escape_layers)?;
        let primitive = match token {
            RHO_MATCH_NODE_START => {
                skip_logical_bytes(frame, cursor, 2, escape_layers)?;
                closers.push(RHO_MATCH_NODE_END);
                false
            }
            RHO_MATCH_FIELD_START => {
                if !root_started {
                    return Err(());
                }
                skip_logical_bytes(frame, cursor, 4, escape_layers)?;
                closers.push(RHO_MATCH_FIELD_END);
                false
            }
            RHO_MATCH_SEQUENCE_START => {
                skip_logical_bytes(frame, cursor, 8, escape_layers)?;
                closers.push(RHO_MATCH_SEQUENCE_END);
                false
            }
            RHO_MATCH_MAP_START => {
                skip_logical_bytes(frame, cursor, 8, escape_layers)?;
                closers.push(RHO_MATCH_MAP_END);
                false
            }
            RHO_MATCH_ONEOF_START => {
                skip_logical_bytes(frame, cursor, 4, escape_layers)?;
                closers.push(RHO_MATCH_ONEOF_END);
                false
            }
            RHO_MATCH_NODE_END |
            RHO_MATCH_FIELD_END |
            RHO_MATCH_SEQUENCE_END |
            RHO_MATCH_MAP_END |
            RHO_MATCH_ONEOF_END => {
                if closers.pop() != Some(token) {
                    return Err(());
                }
                if closers.is_empty() {
                    return Ok(());
                }
                false
            }
            RHO_MATCH_BOOL => {
                skip_logical_bytes(frame, cursor, 1, escape_layers)?;
                true
            }
            RHO_MATCH_I32 | RHO_MATCH_U32 => {
                skip_logical_bytes(frame, cursor, 4, escape_layers)?;
                true
            }
            RHO_MATCH_I64 | RHO_MATCH_U64 => {
                skip_logical_bytes(frame, cursor, 8, escape_layers)?;
                true
            }
            RHO_MATCH_BYTES | RHO_MATCH_STRING | RHO_MATCH_MAP_KEY => {
                if token == RHO_MATCH_MAP_KEY && !root_started {
                    return Err(());
                }
                skip_logical_length_prefixed(frame, cursor, escape_layers)?;
                true
            }
            RHO_MATCH_STRING_SEQUENCE | RHO_MATCH_BYTES_SEQUENCE => {
                let count = take_logical_u64(frame, cursor, escape_layers)?;
                for _ in 0..count {
                    skip_logical_length_prefixed(frame, cursor, escape_layers)?;
                }
                true
            }
            RHO_MATCH_ABSENT => true,
            RHO_MATCH_PATHMAP => {
                skip_logical_bytes(frame, cursor, 1 + 8 + 32, escape_layers)?;
                true
            }
            RHO_MATCH_PAR_BAG => {
                skip_logical_bytes(frame, cursor, 8 + 8 + 32, escape_layers)?;
                true
            }
            _ => return Err(()),
        };
        if !root_started {
            root_started = true;
            if primitive {
                return Ok(());
            }
        }
    }
}

fn skip_logical_length_prefixed(
    frame: &[u8],
    cursor: &mut usize,
    escape_layers: u8,
) -> Result<(), ()> {
    let len = take_logical_u64(frame, cursor, escape_layers)?;
    let len = usize::try_from(len).map_err(|_| ())?;
    skip_logical_bytes(frame, cursor, len, escape_layers)
}

fn take_logical_u64(frame: &[u8], cursor: &mut usize, escape_layers: u8) -> Result<u64, ()> {
    let mut bytes = [0; 8];
    for byte in &mut bytes {
        *byte = take_logical_byte(frame, cursor, escape_layers)?;
    }
    Ok(u64::from_be_bytes(bytes))
}

fn skip_logical_bytes(
    frame: &[u8],
    cursor: &mut usize,
    len: usize,
    escape_layers: u8,
) -> Result<(), ()> {
    for _ in 0..len {
        take_logical_byte(frame, cursor, escape_layers)?;
    }
    Ok(())
}

fn take_logical_byte(frame: &[u8], cursor: &mut usize, escape_layers: u8) -> Result<u8, ()> {
    let byte = take_escaped_byte(frame, cursor)?;
    if escape_layers == 1 || byte != ESCAPE {
        return Ok(byte);
    }
    match take_escaped_byte(frame, cursor)? {
        ESCAPED_ZERO => Ok(ESCAPE),
        FRAME_END => Err(()),
        _ => Err(()),
    }
}

fn take_escaped_byte(frame: &[u8], cursor: &mut usize) -> Result<u8, ()> {
    let byte = *frame.get(*cursor).ok_or(())?;
    *cursor += 1;
    if byte != ESCAPE {
        return Ok(byte);
    }
    let escaped = *frame.get(*cursor).ok_or(())?;
    *cursor += 1;
    match escaped {
        ESCAPED_ZERO => Ok(ESCAPE),
        FRAME_END => Err(()),
        _ => Err(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RhoPathError {
    InvalidMagic,
    InvalidKind(u8),
    InvalidCollectionVersion(u8),
    EmptyChannelGroup,
    TruncatedFrame,
    InvalidEscape(u8),
    PositionOverflow,
    TrailingBytes,
}

/// The three semantic namespaces whose keys use the `RhoPathV1` envelope.
///
/// This is deliberately independent of the persistence-layer namespace enum:
/// the chunk validator lives beside, and is refined against, the contiguous
/// `RhoPathV1` parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RhoPathNamespaceV1 {
    Datum,
    Continuation,
    Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RhoPathFrameStateV1 {
    Data,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RhoPathStreamStateV1 {
    Magic {
        matched: u8,
    },
    Kind,
    CollectionVersion,
    DatumFrame {
        index: u8,
        frame: RhoPathFrameStateV1,
    },
    DatumSuffix,
    ContinuationBoundary {
        channel_count: u64,
    },
    ContinuationBoundaryEscape {
        channel_count: u64,
    },
    ContinuationChannelFrame {
        channel_count: u64,
        frame: RhoPathFrameStateV1,
    },
    ContinuationIdentityFrame {
        frame: RhoPathFrameStateV1,
    },
    ContinuationDone,
    JoinIndexedChannelFrame {
        frame: RhoPathFrameStateV1,
    },
    JoinGroupFirst,
    JoinGroupSecond,
    JoinIdentityFirst,
    JoinIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RhoPathFrameStepV1 {
    Pending(RhoPathFrameStateV1),
    Complete,
}

fn rho_path_frame_step_v1(
    state: RhoPathFrameStateV1,
    byte: u8,
) -> Result<RhoPathFrameStepV1, RhoPathError> {
    match (state, byte) {
        (RhoPathFrameStateV1::Data, ESCAPE) => {
            Ok(RhoPathFrameStepV1::Pending(RhoPathFrameStateV1::Escape))
        }
        (RhoPathFrameStateV1::Data, _) => {
            Ok(RhoPathFrameStepV1::Pending(RhoPathFrameStateV1::Data))
        }
        (RhoPathFrameStateV1::Escape, FRAME_END) => Ok(RhoPathFrameStepV1::Complete),
        (RhoPathFrameStateV1::Escape, ESCAPED_ZERO) => {
            Ok(RhoPathFrameStepV1::Pending(RhoPathFrameStateV1::Data))
        }
        (RhoPathFrameStateV1::Escape, byte) => Err(RhoPathError::InvalidEscape(byte)),
    }
}

/// A constant-state, allocation-free validator for chunked `RhoPathV1` keys.
///
/// Chunk boundaries are semantically invisible, including boundaries between
/// the two bytes of an escape or group terminator. The accepted language and
/// first error are exactly those of `parse_datum`, `parse_continuation`, and
/// `parse_join`; those contiguous parsers remain the independent oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RhoPathStreamValidatorV1 {
    namespace: RhoPathNamespaceV1,
    state: RhoPathStreamStateV1,
    first_error: Option<RhoPathError>,
}

impl RhoPathStreamValidatorV1 {
    pub(crate) const fn new(namespace: RhoPathNamespaceV1) -> Self {
        Self {
            namespace,
            state: RhoPathStreamStateV1::Magic { matched: 0 },
            first_error: None,
        }
    }

    pub(crate) const fn datum() -> Self { Self::new(RhoPathNamespaceV1::Datum) }

    pub(crate) const fn continuation() -> Self { Self::new(RhoPathNamespaceV1::Continuation) }

    pub(crate) const fn join() -> Self { Self::new(RhoPathNamespaceV1::Join) }

    /// Consume one consecutive key chunk.
    ///
    /// Once an error is observed it remains authoritative: later calls and
    /// `finish` return the same first error without inspecting more bytes.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<(), RhoPathError> {
        if let Some(error) = self.first_error {
            return Err(error);
        }
        for &byte in chunk {
            if let Err(error) = self.step(byte) {
                self.first_error = Some(error);
                return Err(error);
            }
        }
        Ok(())
    }

    /// Finish the key at end-of-stream.
    pub(crate) fn finish(self) -> Result<(), RhoPathError> {
        if let Some(error) = self.first_error {
            return Err(error);
        }
        match self.state {
            RhoPathStreamStateV1::DatumSuffix |
            RhoPathStreamStateV1::ContinuationDone |
            RhoPathStreamStateV1::JoinIdentity => Ok(()),
            RhoPathStreamStateV1::Magic { .. } => Err(RhoPathError::InvalidMagic),
            _ => Err(RhoPathError::TruncatedFrame),
        }
    }

    fn step(&mut self, byte: u8) -> Result<(), RhoPathError> {
        self.state = match self.state {
            RhoPathStreamStateV1::Magic { matched } => {
                if byte != MAGIC[matched as usize] {
                    return Err(RhoPathError::InvalidMagic);
                }
                let matched = matched + 1;
                if matched as usize == MAGIC.len() {
                    RhoPathStreamStateV1::Kind
                } else {
                    RhoPathStreamStateV1::Magic { matched }
                }
            }
            RhoPathStreamStateV1::Kind => {
                let expected = match self.namespace {
                    RhoPathNamespaceV1::Datum => DATUM_KIND,
                    RhoPathNamespaceV1::Continuation => CONTINUATION_KIND,
                    RhoPathNamespaceV1::Join => JOIN_KIND,
                };
                if byte != expected {
                    return Err(RhoPathError::InvalidKind(byte));
                }
                match self.namespace {
                    RhoPathNamespaceV1::Datum => RhoPathStreamStateV1::DatumFrame {
                        index: 0,
                        frame: RhoPathFrameStateV1::Data,
                    },
                    RhoPathNamespaceV1::Continuation | RhoPathNamespaceV1::Join => {
                        RhoPathStreamStateV1::CollectionVersion
                    }
                }
            }
            RhoPathStreamStateV1::CollectionVersion => {
                if byte != 1 {
                    return Err(RhoPathError::InvalidCollectionVersion(byte));
                }
                match self.namespace {
                    RhoPathNamespaceV1::Continuation => {
                        RhoPathStreamStateV1::ContinuationBoundary { channel_count: 0 }
                    }
                    RhoPathNamespaceV1::Join => RhoPathStreamStateV1::JoinIndexedChannelFrame {
                        frame: RhoPathFrameStateV1::Data,
                    },
                    RhoPathNamespaceV1::Datum => {
                        unreachable!("datum keys do not contain a collection-version byte")
                    }
                }
            }
            RhoPathStreamStateV1::DatumFrame { index, frame } => {
                match rho_path_frame_step_v1(frame, byte)? {
                    RhoPathFrameStepV1::Pending(frame) => {
                        RhoPathStreamStateV1::DatumFrame { index, frame }
                    }
                    RhoPathFrameStepV1::Complete if index == 2 => RhoPathStreamStateV1::DatumSuffix,
                    RhoPathFrameStepV1::Complete => RhoPathStreamStateV1::DatumFrame {
                        index: index + 1,
                        frame: RhoPathFrameStateV1::Data,
                    },
                }
            }
            RhoPathStreamStateV1::DatumSuffix => RhoPathStreamStateV1::DatumSuffix,
            RhoPathStreamStateV1::ContinuationBoundary { channel_count } => {
                if byte == ESCAPE {
                    RhoPathStreamStateV1::ContinuationBoundaryEscape { channel_count }
                } else {
                    RhoPathStreamStateV1::ContinuationChannelFrame {
                        channel_count,
                        frame: RhoPathFrameStateV1::Data,
                    }
                }
            }
            RhoPathStreamStateV1::ContinuationBoundaryEscape { channel_count } => match byte {
                1 if channel_count == 0 => return Err(RhoPathError::EmptyChannelGroup),
                1 => RhoPathStreamStateV1::ContinuationIdentityFrame {
                    frame: RhoPathFrameStateV1::Data,
                },
                FRAME_END => RhoPathStreamStateV1::ContinuationBoundary {
                    channel_count: channel_count + 1,
                },
                ESCAPED_ZERO => RhoPathStreamStateV1::ContinuationChannelFrame {
                    channel_count,
                    frame: RhoPathFrameStateV1::Data,
                },
                byte => return Err(RhoPathError::InvalidEscape(byte)),
            },
            RhoPathStreamStateV1::ContinuationChannelFrame {
                channel_count,
                frame,
            } => match rho_path_frame_step_v1(frame, byte)? {
                RhoPathFrameStepV1::Pending(frame) => {
                    RhoPathStreamStateV1::ContinuationChannelFrame {
                        channel_count,
                        frame,
                    }
                }
                RhoPathFrameStepV1::Complete => RhoPathStreamStateV1::ContinuationBoundary {
                    channel_count: channel_count + 1,
                },
            },
            RhoPathStreamStateV1::ContinuationIdentityFrame { frame } => {
                match rho_path_frame_step_v1(frame, byte)? {
                    RhoPathFrameStepV1::Pending(frame) => {
                        RhoPathStreamStateV1::ContinuationIdentityFrame { frame }
                    }
                    RhoPathFrameStepV1::Complete => RhoPathStreamStateV1::ContinuationDone,
                }
            }
            RhoPathStreamStateV1::ContinuationDone => {
                return Err(RhoPathError::TrailingBytes);
            }
            RhoPathStreamStateV1::JoinIndexedChannelFrame { frame } => {
                match rho_path_frame_step_v1(frame, byte)? {
                    RhoPathFrameStepV1::Pending(frame) => {
                        RhoPathStreamStateV1::JoinIndexedChannelFrame { frame }
                    }
                    RhoPathFrameStepV1::Complete => RhoPathStreamStateV1::JoinGroupFirst,
                }
            }
            RhoPathStreamStateV1::JoinGroupFirst => {
                if byte != GROUP_END[0] {
                    return Err(RhoPathError::TruncatedFrame);
                }
                RhoPathStreamStateV1::JoinGroupSecond
            }
            RhoPathStreamStateV1::JoinGroupSecond => {
                if byte != GROUP_END[1] {
                    return Err(RhoPathError::TruncatedFrame);
                }
                RhoPathStreamStateV1::JoinIdentityFirst
            }
            RhoPathStreamStateV1::JoinIdentityFirst => RhoPathStreamStateV1::JoinIdentity,
            RhoPathStreamStateV1::JoinIdentity => RhoPathStreamStateV1::JoinIdentity,
        };
        Ok(())
    }
}

impl fmt::Display for RhoPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => f.write_str("invalid RhoPath magic"),
            Self::InvalidKind(kind) => write!(f, "invalid RhoPath kind {kind}"),
            Self::InvalidCollectionVersion(version) => {
                write!(f, "invalid RhoPath collection version {version}")
            }
            Self::EmptyChannelGroup => f.write_str("RhoPath channel group is empty"),
            Self::TruncatedFrame => f.write_str("truncated RhoPath frame"),
            Self::InvalidEscape(byte) => write!(f, "invalid RhoPath escape {byte:#04x}"),
            Self::PositionOverflow => f.write_str("RhoPath receive position exceeds u64"),
            Self::TrailingBytes => f.write_str("unexpected bytes after RhoPath prefix"),
        }
    }
}

impl std::error::Error for RhoPathError {}

pub trait RhoPathEncode: StableHashSerialize {
    fn rho_path_structure(&self) -> Vec<u8> { self.stable_hash_bytes() }

    fn rho_path_identity(&self) -> Vec<u8> { self.stable_hash_bytes() }
}

macro_rules! impl_default_rho_path {
    ($($type:ty),+ $(,)?) => {
        $(impl RhoPathEncode for $type {})+
    };
}

impl_default_rho_path!(i32, u64, bool);

impl RhoPathEncode for String {
    fn rho_path_structure(&self) -> Vec<u8> { self.as_bytes().to_vec() }
}

impl RhoPathEncode for Vec<u8> {
    fn rho_path_structure(&self) -> Vec<u8> { self.clone() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelPathView<'a> {
    encoded_channel_structure: &'a [u8],
    encoded_channel_identity: &'a [u8],
}

impl ChannelPathView<'_> {
    pub fn channel_structure(&self) -> Vec<u8> { decode_frame(self.encoded_channel_structure) }

    pub fn channel_identity(&self) -> Vec<u8> { decode_frame(self.encoded_channel_identity) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatumPathView<'a> {
    encoded_channel_structure: &'a [u8],
    encoded_channel_identity: &'a [u8],
    encoded_datum_structure: &'a [u8],
    datum_identity: &'a [u8],
    channel_prefix_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContinuationPathView<'a> {
    encoded_channels: &'a [u8],
    channel_count: usize,
    encoded_continuation_identity: &'a [u8],
}

impl<'a> ContinuationPathView<'a> {
    pub(crate) fn channel_identities(self) -> ContinuationChannelIdentities<'a> {
        ContinuationChannelIdentities {
            encoded: self.encoded_channels,
            cursor: 0,
            remaining: self.channel_count,
        }
    }

    pub(crate) fn continuation_identity(&self) -> Vec<u8> {
        decode_frame(self.encoded_continuation_identity)
    }

    #[cfg(test)]
    pub(crate) const fn encoded_identity_frame(&self) -> &'a [u8] {
        self.encoded_continuation_identity
    }

    #[cfg(test)]
    pub(crate) fn identity_matches(&self, identity: &[u8]) -> bool {
        frame_equals(self.encoded_continuation_identity, identity)
    }

    #[cfg(test)]
    pub(crate) fn channel_positions<'q>(
        &self,
        channel_identity: &'q [u8],
    ) -> RhoPathChannelPositions<'a, 'q> {
        RhoPathChannelPositions::new(self.encoded_channels, self.channel_count, channel_identity)
    }
}

pub(crate) struct ContinuationChannelIdentities<'a> {
    encoded: &'a [u8],
    cursor: usize,
    remaining: usize,
}

impl Iterator for ContinuationChannelIdentities<'_> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let frame = take_frame(self.encoded, &mut self.cursor)
            .expect("ContinuationPathView contains validated channel frames");
        self.remaining -= 1;
        Some(decode_frame(frame))
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (self.remaining, Some(self.remaining)) }
}

impl ExactSizeIterator for ContinuationChannelIdentities<'_> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JoinPathView<'a> {
    encoded_indexed_channel_identity: &'a [u8],
    join_identity: &'a [u8],
}

impl<'a> JoinPathView<'a> {
    pub(crate) fn indexed_channel_identity(self) -> Vec<u8> {
        decode_frame(self.encoded_indexed_channel_identity)
    }

    pub(crate) const fn join_identity(self) -> &'a [u8] { self.join_identity }
}

impl<'a> DatumPathView<'a> {
    pub fn channel_structure(&self) -> Vec<u8> { decode_frame(self.encoded_channel_structure) }

    pub fn channel_identity(&self) -> Vec<u8> { decode_frame(self.encoded_channel_identity) }

    pub fn datum_structure(&self) -> Vec<u8> { decode_frame(self.encoded_datum_structure) }

    pub const fn datum_identity(&self) -> &'a [u8] { self.datum_identity }

    pub const fn channel_prefix_len(&self) -> usize { self.channel_prefix_len }
}

pub struct RhoPathV1;

impl RhoPathV1 {
    pub fn datum_prefix<C: RhoPathEncode>(channel: &C) -> Vec<u8> {
        let structure = channel.rho_path_structure();
        let identity = channel.rho_path_identity();
        let mut output = Vec::with_capacity(MAGIC.len() + 1 + structure.len() + identity.len() + 4);
        output.extend_from_slice(&MAGIC);
        output.push(DATUM_KIND);
        append_frame(&mut output, &structure);
        append_frame(&mut output, &identity);
        output
    }

    /// Exact prefix for one ordered receive-channel group.
    ///
    /// Channel identities are individually escaped and framed, so common
    /// leading channels remain common trie prefixes while sequence boundaries
    /// are collision-free. No channel hash is used as semantic identity.
    pub(crate) fn continuation_prefix<C: RhoPathEncode>(channels: &[C]) -> Vec<u8> {
        let mut output = Vec::with_capacity(MAGIC.len() + 2 + channels.len().saturating_mul(2));
        output.extend_from_slice(&MAGIC);
        output.push(CONTINUATION_KIND);
        output.push(1);
        for channel in channels {
            append_frame(&mut output, &channel.rho_path_identity());
        }
        output.extend_from_slice(&GROUP_END);
        output
    }

    /// Exact prefix for the join memberships indexed by one channel.
    pub(crate) fn join_prefix<C: RhoPathEncode>(channel: &C) -> Vec<u8> {
        let mut output = Vec::with_capacity(MAGIC.len() + 4);
        output.extend_from_slice(&MAGIC);
        output.push(JOIN_KIND);
        output.push(1);
        append_frame(&mut output, &channel.rho_path_identity());
        output.extend_from_slice(&GROUP_END);
        output
    }

    /// Address one source-free continuation identity below its group prefix.
    ///
    /// The zero-byte escape is order preserving and the two-byte terminator
    /// makes every complete identity prefix-free. Consequently PathMap zipper
    /// order is exactly the lexicographic order of the unframed canonical
    /// identity bytes even when one identity is a prefix of another. This is
    /// the non-authoritative continuation deduplication lookup; canonical
    /// egress traverses a separate primary trie keyed by complete raw record
    /// bytes, so it imposes no prefix-code contract on generic
    /// [`StableHashSerialize`] implementations.
    pub(crate) fn collection_record_path(prefix: &[u8], identity: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(
            prefix
                .len()
                .saturating_add(identity.len())
                .saturating_add(2),
        );
        output.extend_from_slice(prefix);
        append_frame(&mut output, identity);
        output
    }

    /// Parse the identity-keyed resting continuation path used by the
    /// persistent semantic `ContinuationMap`.
    pub(crate) fn parse_continuation(
        path: &[u8],
    ) -> Result<ContinuationPathView<'_>, RhoPathError> {
        let (channels_start, channels_end, channel_count) = take_continuation_channels(path)?;
        if channel_count == 0 {
            return Err(RhoPathError::EmptyChannelGroup);
        }
        let mut cursor = channels_end + GROUP_END.len();
        let continuation_identity = take_frame(path, &mut cursor)?;
        if cursor != path.len() {
            return Err(RhoPathError::TrailingBytes);
        }
        Ok(ContinuationPathView {
            encoded_channels: &path[channels_start..channels_end],
            channel_count,
            encoded_continuation_identity: continuation_identity,
        })
    }

    pub(crate) fn parse_continuation_group(
        prefix: &[u8],
    ) -> Result<ReceiveChannelGroup<'_>, RhoPathError> {
        let (channels_start, channels_end, channel_count) = take_continuation_channels(prefix)?;
        if channels_end + GROUP_END.len() != prefix.len() {
            return Err(RhoPathError::TrailingBytes);
        }
        Ok(ReceiveChannelGroup {
            profile: ReceiveChannelProfile::LegacyV1,
            encoded_channels: &prefix[channels_start..channels_end],
            channel_count,
        })
    }

    /// Parse the resting join path indexed by one channel and ending in one
    /// complete, ordered join record.
    pub(crate) fn parse_join(path: &[u8]) -> Result<JoinPathView<'_>, RhoPathError> {
        let mut cursor = parse_collection_header(path, JOIN_KIND)?;
        let indexed_channel = take_frame(path, &mut cursor)?;
        if path.get(cursor..cursor.saturating_add(GROUP_END.len())) != Some(&GROUP_END) {
            return Err(RhoPathError::TruncatedFrame);
        }
        cursor += GROUP_END.len();
        if cursor == path.len() {
            return Err(RhoPathError::TruncatedFrame);
        }
        Ok(JoinPathView {
            encoded_indexed_channel_identity: indexed_channel,
            join_identity: &path[cursor..],
        })
    }

    pub fn datum_key<C: RhoPathEncode>(
        channel: &C,
        datum_structure: &[u8],
        datum_identity: &[u8],
    ) -> Vec<u8> {
        let mut output = Self::datum_prefix(channel);
        output.reserve(datum_structure.len() + datum_identity.len() + 2);
        append_frame(&mut output, datum_structure);
        output.extend_from_slice(datum_identity);
        output
    }

    pub fn datum_structure_prefix<C: RhoPathEncode>(
        channel: &C,
        structure_prefix: &[u8],
    ) -> Vec<u8> {
        let mut output = Self::datum_prefix(channel);
        output.extend_from_slice(&Self::relative_datum_structure_prefix(structure_prefix));
        output
    }

    /// Encode a datum-structure prefix relative to its channel subtrie.
    ///
    /// Candidate pools already hold the channel's PathMap subtree, so their
    /// keys begin at the datum-structure frame. Keeping this helper beside the
    /// complete-key encoder prevents the derived candidate index from growing
    /// a second, subtly different escaping implementation.
    pub fn relative_datum_structure_prefix(structure_prefix: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(structure_prefix.len());
        output.extend(Self::relative_datum_structure_prefix_bytes(structure_prefix));
        output
    }

    /// Stream the channel-relative structure prefix without allocating its
    /// escaped representation.
    ///
    /// Hot retained-automaton lookups use this iterator to descend their
    /// PathMap directly. The owned helper above remains the appropriate API
    /// when a prefix has to outlive the traversal.
    pub(crate) fn relative_datum_structure_prefix_bytes(
        structure_prefix: &[u8],
    ) -> impl Iterator<Item = u8> + '_ {
        structure_prefix.iter().flat_map(|byte| {
            std::iter::once(*byte).chain((*byte == ESCAPE).then_some(ESCAPED_ZERO))
        })
    }

    /// Encode one complete datum key relative to an already-selected channel
    /// subtree.
    ///
    /// The structural frame remains the leading trie dimension, so
    /// [`Self::relative_datum_structure_prefix`] can prune it without touching
    /// identity bytes. The unframed identity suffix then makes each complete
    /// path injective. This is the channel-relative counterpart of
    /// [`Self::datum_key`] and deliberately shares its framing implementation.
    pub fn relative_datum_key(datum_structure: &[u8], datum_identity: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(
            datum_structure
                .len()
                .saturating_add(datum_identity.len())
                .saturating_add(2),
        );
        append_frame(&mut output, datum_structure);
        output.extend_from_slice(datum_identity);
        output
    }

    pub fn parse_datum(path: &[u8]) -> Result<DatumPathView<'_>, RhoPathError> {
        let (channel, mut cursor) = parse_channel(path)?;
        let channel_prefix_len = cursor;
        let datum_structure = take_frame(path, &mut cursor)?;
        Ok(DatumPathView {
            encoded_channel_structure: channel.encoded_channel_structure,
            encoded_channel_identity: channel.encoded_channel_identity,
            encoded_datum_structure: datum_structure,
            datum_identity: &path[cursor..],
            channel_prefix_len,
        })
    }

    pub fn parse_datum_prefix(path: &[u8]) -> Result<ChannelPathView<'_>, RhoPathError> {
        let (channel, cursor) = parse_channel(path)?;
        if cursor == path.len() {
            Ok(channel)
        } else {
            Err(RhoPathError::TrailingBytes)
        }
    }

    /// Borrow the canonical datum identity from a channel-relative RhoPath.
    ///
    /// A `Bag` prefix zipper already selected and removed the complete channel
    /// prefix, so the remaining path is exactly `frame(datum_structure) ||
    /// datum_identity`. Parsing that suffix directly avoids rebuilding one
    /// absolute key per endpoint during checkpoint egress.
    pub(crate) fn relative_datum_identity(path: &[u8]) -> Result<&[u8], RhoPathError> {
        let mut cursor = 0;
        let _ = take_frame(path, &mut cursor)?;
        Ok(&path[cursor..])
    }
}

fn parse_channel(path: &[u8]) -> Result<(ChannelPathView<'_>, usize), RhoPathError> {
    if path.len() < MAGIC.len() || path[..MAGIC.len()] != MAGIC {
        return Err(RhoPathError::InvalidMagic);
    }
    let kind = *path.get(MAGIC.len()).ok_or(RhoPathError::TruncatedFrame)?;
    if kind != DATUM_KIND {
        return Err(RhoPathError::InvalidKind(kind));
    }
    let mut cursor = MAGIC.len() + 1;
    let channel_structure = take_frame(path, &mut cursor)?;
    let channel_identity = take_frame(path, &mut cursor)?;
    Ok((
        ChannelPathView {
            encoded_channel_structure: channel_structure,
            encoded_channel_identity: channel_identity,
        },
        cursor,
    ))
}

fn parse_collection_header(path: &[u8], expected_kind: u8) -> Result<usize, RhoPathError> {
    if path.get(..MAGIC.len()) != Some(&MAGIC) {
        return Err(RhoPathError::InvalidMagic);
    }
    let kind = *path.get(MAGIC.len()).ok_or(RhoPathError::TruncatedFrame)?;
    if kind != expected_kind {
        return Err(RhoPathError::InvalidKind(kind));
    }
    let version = *path
        .get(MAGIC.len() + 1)
        .ok_or(RhoPathError::TruncatedFrame)?;
    if version != 1 {
        return Err(RhoPathError::InvalidCollectionVersion(version));
    }
    Ok(MAGIC.len() + 2)
}

fn take_continuation_channels(path: &[u8]) -> Result<(usize, usize, usize), RhoPathError> {
    let mut cursor = parse_collection_header(path, CONTINUATION_KIND)?;
    let channels_start = cursor;
    let mut channel_count = 0usize;
    while path.get(cursor..cursor.saturating_add(GROUP_END.len())) != Some(&GROUP_END) {
        let _ = take_frame(path, &mut cursor)?;
        channel_count = channel_count
            .checked_add(1)
            .ok_or(RhoPathError::PositionOverflow)?;
    }
    u64::try_from(channel_count).map_err(|_| RhoPathError::PositionOverflow)?;
    Ok((channels_start, cursor, channel_count))
}

pub fn encode_structural_sequence<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let parts = parts.into_iter();
    let (lower, _) = parts.size_hint();
    let mut output = StructuralSequenceEncoder::with_capacity(lower);
    for part in parts {
        output.push(part);
    }
    output.finish()
}

/// Incremental structural-sequence framing for callers that produce one
/// encoded term at a time. This avoids retaining a `Vec<Vec<u8>>` merely to
/// borrow every completed element for [`encode_structural_sequence`].
pub struct StructuralSequenceEncoder {
    output: Vec<u8>,
}

impl StructuralSequenceEncoder {
    pub fn new() -> Self { Self::with_capacity(0) }

    pub fn with_capacity(part_count: usize) -> Self {
        let mut output = Vec::with_capacity(1 + part_count.saturating_mul(2));
        output.push(1);
        Self { output }
    }

    pub fn push(&mut self, part: &[u8]) { append_frame(&mut self.output, part); }

    pub fn finish(self) -> Vec<u8> { self.output }
}

impl Default for StructuralSequenceEncoder {
    fn default() -> Self { Self::new() }
}

/// RhoPathV2's schema-level envelope.  The codec accepts already-canonical
/// byte slices from the semantic layer; it never materializes Par collections
/// or sorts trie entries.  All parsing is cursor-based and iterative.
pub struct RhoPathV2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhoPathV2DatumView<'a> {
    channel_image: &'a [u8],
    datum_structure: &'a [u8],
    datum_identity: &'a [u8],
}

impl RhoPathV2DatumView<'_> {
    /// Borrow the complete escaped frame, including its terminator.
    pub const fn encoded_channel_image(&self) -> &[u8] { self.channel_image }
    pub const fn encoded_datum_structure(&self) -> &[u8] { self.datum_structure }
    pub const fn encoded_datum_identity(&self) -> &[u8] { self.datum_identity }
    pub fn channel_image(&self) -> Vec<u8> { decode_frame(self.channel_image) }
    pub fn datum_structure(&self) -> Vec<u8> { decode_frame(self.datum_structure) }
    pub fn datum_identity(&self) -> Vec<u8> { decode_frame(self.datum_identity) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RhoPathV2ContinuationView<'a> {
    encoded_channels: &'a [u8],
    channel_count: usize,
    identity_frame: &'a [u8],
}

impl<'a> RhoPathV2ContinuationView<'a> {
    pub const fn channel_count(&self) -> usize { self.channel_count }
    pub const fn encoded_identity_frame(&self) -> &'a [u8] { self.identity_frame }
    pub fn identity_matches(&self, identity: &[u8]) -> bool {
        frame_equals(self.identity_frame, identity)
    }
    pub fn channel_frames(&self) -> RhoPathV2ChannelFrames<'a> {
        RhoPathV2ChannelFrames {
            encoded: self.encoded_channels,
            cursor: 0,
            remaining: self.channel_count,
        }
    }

    pub fn channel_positions<'q>(
        &self,
        channel_image: &'q [u8],
    ) -> RhoPathChannelPositions<'a, 'q> {
        RhoPathChannelPositions::new(self.encoded_channels, self.channel_count, channel_image)
    }

    pub const fn channel_group(&self) -> ReceiveChannelGroup<'a> {
        ReceiveChannelGroup {
            profile: ReceiveChannelProfile::Rci2V2,
            encoded_channels: self.encoded_channels,
            channel_count: self.channel_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RhoPathReceivePosition(u64);

impl RhoPathReceivePosition {
    pub const fn get(self) -> u64 { self.0 }

    pub fn index(self) -> usize {
        usize::try_from(self.0).expect("a receive position addresses an in-memory channel group")
    }

    pub fn signed_peek_key(self) -> Option<i32> { i32::try_from(self.0).ok() }

    pub(crate) fn at_index(index: usize) -> Self {
        Self(u64::try_from(index).expect("an in-memory channel position fits u64"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReceiveChannelProfile {
    LegacyV1,
    Rci2V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReceiveChannelQuery<'q> {
    LegacyV1(&'q [u8]),
    Rci2V2(&'q [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveChannelGroup<'a> {
    profile: ReceiveChannelProfile,
    encoded_channels: &'a [u8],
    channel_count: usize,
}

impl<'a> ReceiveChannelGroup<'a> {
    pub const fn profile(&self) -> ReceiveChannelProfile { self.profile }

    pub const fn channel_count(&self) -> usize { self.channel_count }

    pub const fn frames(&self) -> RhoPathV2ChannelFrames<'a> {
        RhoPathV2ChannelFrames {
            encoded: self.encoded_channels,
            cursor: 0,
            remaining: self.channel_count,
        }
    }

    pub fn frame_at(&self, position: RhoPathReceivePosition) -> Option<&'a [u8]> {
        self.frames().nth(position.index())
    }

    pub fn channel_positions_for<'q>(
        &self,
        query: ReceiveChannelQuery<'q>,
    ) -> Option<RhoPathChannelPositions<'a, 'q>> {
        match (self.profile, query) {
            (ReceiveChannelProfile::LegacyV1, ReceiveChannelQuery::LegacyV1(raw)) |
            (ReceiveChannelProfile::Rci2V2, ReceiveChannelQuery::Rci2V2(raw)) => {
                Some(RhoPathChannelPositions::new(self.encoded_channels, self.channel_count, raw))
            }
            (ReceiveChannelProfile::LegacyV1, ReceiveChannelQuery::Rci2V2(_)) |
            (ReceiveChannelProfile::Rci2V2, ReceiveChannelQuery::LegacyV1(_)) => None,
        }
    }
}

pub struct RhoPathChannelPositions<'a, 'q> {
    frames: RhoPathV2ChannelFrames<'a>,
    query: &'q [u8],
    next_position: u64,
}

impl<'a, 'q> RhoPathChannelPositions<'a, 'q> {
    fn new(encoded: &'a [u8], count: usize, query: &'q [u8]) -> Self {
        Self {
            frames: RhoPathV2ChannelFrames {
                encoded,
                cursor: 0,
                remaining: count,
            },
            query,
            next_position: 0,
        }
    }
}

impl Iterator for RhoPathChannelPositions<'_, '_> {
    type Item = RhoPathReceivePosition;

    fn next(&mut self) -> Option<Self::Item> {
        for frame in self.frames.by_ref() {
            let position = self.next_position;
            self.next_position = self
                .next_position
                .checked_add(1)
                .expect("validated continuation positions fit u64");
            if frame_equals(frame, self.query) {
                return Some(RhoPathReceivePosition(position));
            }
        }
        None
    }
}

pub(crate) fn frame_equals(frame: &[u8], raw: &[u8]) -> bool {
    let mut cursor = 0;
    let mut offset = 0;
    while cursor < frame.len() {
        let byte = frame[cursor];
        if byte == ESCAPE {
            let Some(&escaped) = frame.get(cursor + 1) else {
                return false;
            };
            if escaped == FRAME_END {
                return cursor + 2 == frame.len() && offset == raw.len();
            }
            if escaped != ESCAPED_ZERO || raw.get(offset) != Some(&ESCAPE) {
                return false;
            }
            cursor += 2;
        } else {
            if raw.get(offset) != Some(&byte) {
                return false;
            }
            cursor += 1;
        }
        offset += 1;
    }
    false
}

pub struct RhoPathV2ChannelFrames<'a> {
    encoded: &'a [u8],
    cursor: usize,
    remaining: usize,
}

impl<'a> Iterator for RhoPathV2ChannelFrames<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let frame = take_frame(self.encoded, &mut self.cursor).ok()?;
        self.remaining -= 1;
        Some(frame)
    }
    fn size_hint(&self) -> (usize, Option<usize>) { (self.remaining, Some(self.remaining)) }
}

impl ExactSizeIterator for RhoPathV2ChannelFrames<'_> {}

impl std::fmt::Debug for RhoPathV2ChannelFrames<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RhoPathV2ChannelFrames")
            .field("remaining", &self.remaining)
            .finish()
    }
}

impl RhoPathV2 {
    pub const MAGIC: [u8; 4] = *b"RHP\x02";
    pub const DATUM_KIND: u8 = 1;
    pub const CONTINUATION_KIND: u8 = 2;
    pub const JOIN_KIND: u8 = 3;
    pub const COLLECTION_VERSION: u8 = 1;

    pub fn datum_key(
        channel_image: &[u8],
        datum_structure: &[u8],
        datum_identity: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            5 + channel_image.len() + datum_structure.len() + datum_identity.len() + 6,
        );
        out.extend_from_slice(&Self::MAGIC);
        out.push(Self::DATUM_KIND);
        append_frame(&mut out, channel_image);
        append_frame(&mut out, datum_structure);
        append_frame(&mut out, datum_identity);
        out
    }

    pub fn parse_datum(path: &[u8]) -> Result<RhoPathV2DatumView<'_>, RhoPathError> {
        if path.len() < 5 || path[..4] != Self::MAGIC {
            return Err(RhoPathError::InvalidMagic);
        }
        if path[4] != Self::DATUM_KIND {
            return Err(RhoPathError::InvalidKind(path[4]));
        }
        let mut cursor = 5;
        let channel_image = take_frame(path, &mut cursor)?;
        let datum_structure = take_frame(path, &mut cursor)?;
        let datum_identity = take_frame(path, &mut cursor)?;
        if cursor != path.len() {
            return Err(RhoPathError::TrailingBytes);
        }
        Ok(RhoPathV2DatumView {
            channel_image,
            datum_structure,
            datum_identity,
        })
    }

    /// Borrow the complete escaped channel-image frame from a V2 datum key.
    /// This is the read-side primitive for PathMap channel-subtree lookup;
    /// it performs no payload decoding or allocation.
    pub fn parse_datum_prefix(path: &[u8]) -> Result<&[u8], RhoPathError> {
        if path.len() < 5 || path[..4] != Self::MAGIC {
            return Err(RhoPathError::InvalidMagic);
        }
        if path[4] != Self::DATUM_KIND {
            return Err(RhoPathError::InvalidKind(path[4]));
        }
        let mut cursor = 5;
        take_frame(path, &mut cursor)
    }

    pub fn continuation_key(channels: &[&[u8]], identity: &[u8]) -> Result<Vec<u8>, RhoPathError> {
        let mut out = Self::continuation_prefix(channels)?;
        append_frame(&mut out, identity);
        Ok(out)
    }

    pub fn continuation_prefix(channels: &[&[u8]]) -> Result<Vec<u8>, RhoPathError> {
        if channels.is_empty() {
            return Err(RhoPathError::EmptyChannelGroup);
        }
        let count = u64::try_from(channels.len()).map_err(|_| RhoPathError::TrailingBytes)?;
        let mut out = Vec::new();
        out.extend_from_slice(&Self::MAGIC);
        out.extend_from_slice(&[Self::CONTINUATION_KIND, Self::COLLECTION_VERSION]);
        out.extend_from_slice(&count.to_be_bytes());
        for channel in channels {
            append_frame(&mut out, channel);
        }
        out.extend_from_slice(&GROUP_END);
        Ok(out)
    }

    pub fn join_key(channel: &[u8], identity: &[u8]) -> Vec<u8> {
        let mut out = Self::join_prefix(channel);
        append_frame(&mut out, identity);
        out
    }

    pub fn join_prefix(channel: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + channel.len());
        out.extend_from_slice(&Self::MAGIC);
        out.push(Self::JOIN_KIND);
        append_frame(&mut out, channel);
        out.extend_from_slice(&GROUP_END);
        out
    }

    pub fn parse_continuation(path: &[u8]) -> Result<RhoPathV2ContinuationView<'_>, RhoPathError> {
        if path.len() < 14 || path[..4] != Self::MAGIC {
            return Err(RhoPathError::InvalidMagic);
        }
        if path[4] != Self::CONTINUATION_KIND {
            return Err(RhoPathError::InvalidKind(path[4]));
        }
        if path[5] != Self::COLLECTION_VERSION {
            return Err(RhoPathError::InvalidCollectionVersion(path[5]));
        }
        let mut count_bytes = [0u8; 8];
        count_bytes.copy_from_slice(&path[6..14]);
        let count = usize::try_from(u64::from_be_bytes(count_bytes))
            .map_err(|_| RhoPathError::TrailingBytes)?;
        if count == 0 {
            return Err(RhoPathError::EmptyChannelGroup);
        }
        let mut cursor = 14;
        let channels_start = cursor;
        for _ in 0..count {
            let _ = take_frame(path, &mut cursor)?;
        }
        let encoded_channels = &path[channels_start..cursor];
        if path.get(cursor..cursor.saturating_add(GROUP_END.len())) != Some(&GROUP_END) {
            return Err(RhoPathError::TruncatedFrame);
        }
        cursor += GROUP_END.len();
        let identity_frame = take_frame(path, &mut cursor)?;
        if cursor != path.len() {
            return Err(RhoPathError::TrailingBytes);
        }
        Ok(RhoPathV2ContinuationView {
            encoded_channels,
            channel_count: count,
            identity_frame,
        })
    }

    pub fn parse_join(path: &[u8]) -> Result<(&[u8], &[u8]), RhoPathError> {
        if path.len() < 7 || path[..4] != Self::MAGIC {
            return Err(RhoPathError::InvalidMagic);
        }
        if path[4] != Self::JOIN_KIND {
            return Err(RhoPathError::InvalidKind(path[4]));
        }
        let mut cursor = 5;
        let channel = take_frame(path, &mut cursor)?;
        if path.get(cursor..cursor.saturating_add(GROUP_END.len())) != Some(&GROUP_END) {
            return Err(RhoPathError::TruncatedFrame);
        }
        cursor += GROUP_END.len();
        let identity = take_frame(path, &mut cursor)?;
        if cursor != path.len() {
            return Err(RhoPathError::TrailingBytes);
        }
        Ok((channel, identity))
    }
}

fn append_frame(output: &mut Vec<u8>, bytes: &[u8]) {
    append_frame_prefix(output, bytes);
    output.extend_from_slice(&[ESCAPE, FRAME_END]);
}

fn append_frame_prefix(output: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        if *byte == ESCAPE {
            output.extend_from_slice(&[ESCAPE, ESCAPED_ZERO]);
        } else {
            output.push(*byte);
        }
    }
}

fn take_frame<'a>(path: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], RhoPathError> {
    let start = *cursor;
    while *cursor < path.len() {
        if path[*cursor] != ESCAPE {
            *cursor += 1;
            continue;
        }
        let escaped = *path.get(*cursor + 1).ok_or(RhoPathError::TruncatedFrame)?;
        match escaped {
            FRAME_END => {
                let end = *cursor + 2;
                *cursor = end;
                return Ok(&path[start..end]);
            }
            ESCAPED_ZERO => *cursor += 2,
            byte => return Err(RhoPathError::InvalidEscape(byte)),
        }
    }
    Err(RhoPathError::TruncatedFrame)
}

fn decode_frame(frame: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(frame.len().saturating_sub(2));
    let mut cursor = 0;
    while cursor + 1 < frame.len() {
        if frame[cursor] == ESCAPE {
            if frame[cursor + 1] == FRAME_END {
                break;
            }
            output.push(ESCAPE);
            cursor += 2;
        } else {
            output.push(frame[cursor]);
            cursor += 1;
        }
    }
    output
}

#[cfg(test)]
mod receive_position_tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn v1_borrowed_positions_preserve_repeated_escaped_channels_and_identity() {
        let channels = vec![b"a\0".to_vec(), b"a".to_vec(), b"a\0".to_vec()];
        let prefix = RhoPathV1::continuation_prefix(&channels);
        let path = RhoPathV1::collection_record_path(&prefix, b"body\0peek");
        let view = RhoPathV1::parse_continuation(&path).unwrap();
        let escaped_identity = channels[0].rho_path_identity();
        let plain_identity = channels[1].rho_path_identity();
        assert_eq!(
            view.channel_positions(&escaped_identity)
                .map(|position| position.get())
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(
            view.channel_positions(&plain_identity)
                .map(|position| position.get())
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(view.channel_positions(b"a\0").count(), 0);
        assert!(view.identity_matches(b"body\0peek"));
        assert!(!view.identity_matches(b"body"));
        assert_eq!(view.encoded_identity_frame().last(), Some(&0));
    }

    proptest! {
        #[test]
        fn v1_borrowed_positions_refine_ordered_channel_occurrences(
            query in proptest::collection::vec(proptest::num::u8::ANY, 0..24),
            before in proptest::collection::vec(
                proptest::collection::vec(proptest::num::u8::ANY, 0..24), 0..32,
            ),
            after in proptest::collection::vec(
                proptest::collection::vec(proptest::num::u8::ANY, 0..24), 0..32,
            ),
        ) {
            let mut channels = before;
            channels.push(query.clone());
            channels.extend(after);
            channels.push(query.clone());
            let expected = channels.iter().enumerate()
                .filter_map(|(index, channel)| (channel == &query).then_some(index as u64))
                .collect::<Vec<_>>();

            let prefix = RhoPathV1::continuation_prefix(&channels);
            let path = RhoPathV1::collection_record_path(&prefix, b"id\0suffix");
            let view = RhoPathV1::parse_continuation(&path).unwrap();
            let actual = view.channel_positions(&query.rho_path_identity())
                .map(|position| position.get()).collect::<Vec<_>>();
            prop_assert_eq!(actual, expected);
            prop_assert!(view.identity_matches(b"id\0suffix"));
        }
    }
}
