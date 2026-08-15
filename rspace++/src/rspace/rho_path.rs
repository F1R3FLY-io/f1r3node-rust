use std::fmt;

use super::hashing::stable_hash_provider::StableHashSerialize;

const MAGIC: [u8; 4] = *b"RHP\x01";
const DATUM_KIND: u8 = 1;
const ESCAPE: u8 = 0;
const ESCAPED_ZERO: u8 = 0xFF;
const FRAME_END: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RhoPathError {
    InvalidMagic,
    InvalidKind(u8),
    TruncatedFrame,
    InvalidEscape(u8),
}

impl fmt::Display for RhoPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => f.write_str("invalid RhoPathV1 magic"),
            Self::InvalidKind(kind) => write!(f, "invalid RhoPathV1 kind {kind}"),
            Self::TruncatedFrame => f.write_str("truncated RhoPathV1 frame"),
            Self::InvalidEscape(byte) => write!(f, "invalid RhoPathV1 escape {byte:#04x}"),
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

impl_default_rho_path!(String, i32, u64, bool);

impl RhoPathEncode for Vec<u8> {
    fn rho_path_structure(&self) -> Vec<u8> { self.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.clone() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatumPathView<'a> {
    encoded_channel_structure: &'a [u8],
    encoded_channel_identity: &'a [u8],
    encoded_datum_structure: &'a [u8],
    datum_identity: &'a [u8],
}

impl<'a> DatumPathView<'a> {
    pub fn channel_structure(&self) -> Vec<u8> { decode_frame(self.encoded_channel_structure) }

    pub fn channel_identity(&self) -> Vec<u8> { decode_frame(self.encoded_channel_identity) }

    pub fn datum_structure(&self) -> Vec<u8> { decode_frame(self.encoded_datum_structure) }

    pub const fn datum_identity(&self) -> &'a [u8] { self.datum_identity }
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
        append_frame_prefix(&mut output, structure_prefix);
        output
    }

    pub fn parse_datum(path: &[u8]) -> Result<DatumPathView<'_>, RhoPathError> {
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
        let datum_structure = take_frame(path, &mut cursor)?;
        Ok(DatumPathView {
            encoded_channel_structure: channel_structure,
            encoded_channel_identity: channel_identity,
            encoded_datum_structure: datum_structure,
            datum_identity: &path[cursor..],
        })
    }
}

pub fn encode_structural_sequence<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let parts = parts.into_iter();
    let (lower, _) = parts.size_hint();
    let mut output = Vec::with_capacity(1 + lower * 2);
    output.push(1);
    for part in parts {
        append_frame(&mut output, part);
    }
    output
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
