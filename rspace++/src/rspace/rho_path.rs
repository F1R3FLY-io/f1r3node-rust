use std::fmt;

use pathmap::PathMap;

use super::hashing::stable_hash_provider::StableHashSerialize;

const MAGIC: [u8; 4] = *b"RHP\x01";
const DATUM_KIND: u8 = 1;
const ESCAPE: u8 = 0;
const ESCAPED_ZERO: u8 = 0xFF;
const FRAME_END: u8 = 0;

pub const RHO_MATCH_PATH_VERSION: [u8; 4] = *b"RMP\x01";
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RhoPathCaptureId(u32);

impl RhoPathCaptureId {
    pub const fn new(id: u32) -> Self { Self(id) }

    pub const fn get(self) -> u32 { self.0 }

    pub(crate) const fn path(self) -> [u8; 4] { self.0.to_be_bytes() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RhoPathQueryAtom {
    Exact(Vec<u8>),
    Capture(RhoPathCaptureId),
    Wildcard,
    Match(RhoMatchQuery),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompiledRhoPathQueryAtom {
    Exact {
        inner_frame: Vec<u8>,
        stored_frame: Vec<u8>,
    },
    Capture(RhoPathCaptureId),
    Wildcard,
    Match(RhoMatchQuery),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RhoMatchQueryPart {
    Exact(Vec<u8>),
    Capture(RhoPathCaptureId),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompiledRhoMatchQueryPart {
    Exact {
        inner_frame: Vec<u8>,
        stored_frame: Vec<u8>,
    },
    Capture(RhoPathCaptureId),
    Wildcard,
}

/// A stack-safe structural query over one complete `RMP1` value.
///
/// Exact runs are stored in the same escaped form used by the enclosing
/// RhoPathV1 datum-structure frame. Wildcards and captures consume exactly one
/// self-delimiting RMP1 value. Repeated capture identifiers compare their
/// canonical escaped byte slices without decoding a `Par`.
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RhoPathQuery {
    atoms: Vec<CompiledRhoPathQueryAtom>,
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
                    CompiledRhoPathQueryAtom::Capture(id)
                }
                RhoPathQueryAtom::Capture(_) | RhoPathQueryAtom::Wildcard => {
                    CompiledRhoPathQueryAtom::Wildcard
                }
                RhoPathQueryAtom::Match(query) => CompiledRhoPathQueryAtom::Match(query),
            })
            .collect();
        Self {
            atoms,
            accepts_remainder,
        }
    }

    pub fn is_selective(&self) -> bool { !(self.atoms.is_empty() && self.accepts_remainder) }

    pub fn matches_structure(&self, structure: &[u8]) -> bool {
        if structure.first() != Some(&1) {
            return false;
        }
        let mut cursor = 1usize;
        let mut captures: PathMap<Vec<u8>> = PathMap::new();
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
                CompiledRhoPathQueryAtom::Capture(id) => {
                    let path = id.path();
                    match captures.get(path) {
                        Some(previous) if previous.as_slice() != frame => return false,
                        Some(_) => {}
                        None => {
                            captures.insert(path, frame.to_vec());
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
    /// exact RMP1 prefix without closing the surrounding structural frame.
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
                CompiledRhoPathQueryAtom::Capture(_) | CompiledRhoPathQueryAtom::Wildcard => break,
            }
        }
        prefix
    }
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
    TruncatedFrame,
    InvalidEscape(u8),
    TrailingBytes,
}

impl fmt::Display for RhoPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => f.write_str("invalid RhoPathV1 magic"),
            Self::InvalidKind(kind) => write!(f, "invalid RhoPathV1 kind {kind}"),
            Self::TruncatedFrame => f.write_str("truncated RhoPathV1 frame"),
            Self::InvalidEscape(byte) => write!(f, "invalid RhoPathV1 escape {byte:#04x}"),
            Self::TrailingBytes => f.write_str("unexpected bytes after RhoPathV1 prefix"),
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
