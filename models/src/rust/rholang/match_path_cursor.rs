//! Validated, stack-safe navigation of `RMP1` structural matching paths.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use super::match_path::{
    ABSENT, BOOL, BYTES, BYTES_SEQUENCE, FIELD_END, FIELD_START, I32, I64, MAP_END, MAP_KEY,
    MAP_START, NODE_END, NODE_START, ONEOF_END, ONEOF_START, PATHMAP, RHO_MATCH_PATH_VERSION,
    SEQUENCE_END, SEQUENCE_START, STRING, STRING_SEQUENCE, U32, U64,
};
use super::match_schema::{
    RhoMatchField, RhoMatchFieldKind, RhoMatchOneofVariant, RhoMatchPayloadKind, RhoMatchProgram,
};
use super::match_schema_tables::RHO_MATCH_REGISTRY;
use crate::rust::epathmap_trie_codec::EPathMapMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RhoMatchPathErrorKind {
    InvalidVersion,
    Truncated,
    LengthOverflow(u64),
    InvalidUtf8,
    InvalidBool(u8),
    InvalidPathMapMode(u8),
    NeutralPathMapHasEntries(u64),
    UnknownToken(u8),
    UnknownTypeId(u16),
    UnexpectedTypeId {
        expected: u16,
        actual: u16,
    },
    UnknownOneofTag {
        field: &'static str,
        tag: u32,
    },
    UnexpectedToken {
        expected: &'static str,
        actual: &'static str,
    },
    UnexpectedField {
        type_name: &'static str,
        expected: Option<u32>,
        actual: u32,
    },
    MissingFields {
        type_name: &'static str,
        next_tag: u32,
    },
    ZeroOneofTag,
    Unclosed(&'static str),
    NotATokenBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RhoMatchPathError {
    pub offset: usize,
    pub kind: RhoMatchPathErrorKind,
}

impl fmt::Display for RhoMatchPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid RMP1 path at byte {}: ", self.offset)?;
        match &self.kind {
            RhoMatchPathErrorKind::InvalidVersion => f.write_str("invalid version"),
            RhoMatchPathErrorKind::Truncated => f.write_str("truncated token"),
            RhoMatchPathErrorKind::LengthOverflow(len) => {
                write!(f, "length {len} does not fit this platform")
            }
            RhoMatchPathErrorKind::InvalidUtf8 => f.write_str("invalid UTF-8 string"),
            RhoMatchPathErrorKind::InvalidBool(value) => {
                write!(f, "invalid boolean byte {value}")
            }
            RhoMatchPathErrorKind::InvalidPathMapMode(mode) => {
                write!(f, "invalid EPathMap mode {mode}")
            }
            RhoMatchPathErrorKind::NeutralPathMapHasEntries(len) => {
                write!(f, "neutral EPathMap carries {len} entries")
            }
            RhoMatchPathErrorKind::UnknownToken(token) => {
                write!(f, "unknown token {token:#04x}")
            }
            RhoMatchPathErrorKind::UnknownTypeId(type_id) => {
                write!(f, "unknown message type identifier {type_id}")
            }
            RhoMatchPathErrorKind::UnexpectedTypeId { expected, actual } => {
                write!(
                    f,
                    "expected message type identifier {expected}, found {actual}"
                )
            }
            RhoMatchPathErrorKind::UnknownOneofTag { field, tag } => {
                write!(f, "oneof field {field} has no variant with tag {tag}")
            }
            RhoMatchPathErrorKind::UnexpectedToken { expected, actual } => {
                write!(f, "expected {expected}, found {actual}")
            }
            RhoMatchPathErrorKind::UnexpectedField {
                type_name,
                expected,
                actual,
            } => match expected {
                Some(expected) => write!(
                    f,
                    "message {type_name} expected field tag {expected}, found {actual}"
                ),
                None => write!(f, "message {type_name} has no field tag {actual}"),
            },
            RhoMatchPathErrorKind::MissingFields {
                type_name,
                next_tag,
            } => write!(f, "message {type_name} is missing field tag {next_tag}"),
            RhoMatchPathErrorKind::ZeroOneofTag => f.write_str("oneof tag zero is reserved"),
            RhoMatchPathErrorKind::Unclosed(expected) => {
                write!(f, "end of input before {expected}")
            }
            RhoMatchPathErrorKind::NotATokenBoundary => {
                f.write_str("requested offset is not a token boundary")
            }
        }
    }
}

impl std::error::Error for RhoMatchPathError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RhoMatchPathScalar<'a> {
    Bool(bool),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Bytes(&'a [u8]),
    String(&'a str),
    StringSequence { count: u64, encoded: &'a [u8] },
    BytesSequence { count: u64, encoded: &'a [u8] },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RhoMatchPathToken<'a> {
    NodeStart {
        program: &'static RhoMatchProgram,
    },
    NodeEnd,
    FieldStart(u32),
    FieldEnd,
    Scalar(RhoMatchPathScalar<'a>),
    Absent,
    SequenceStart(u64),
    SequenceEnd,
    MapStart(u64),
    MapKey(&'a str),
    MapEnd,
    OneofStart(u32),
    OneofEnd,
    PathMap {
        mode: EPathMapMode,
        len: u64,
        digest: &'a [u8; 32],
    },
}

impl RhoMatchPathToken<'_> {
    pub const fn name(self) -> &'static str {
        match self {
            Self::NodeStart { .. } => "node",
            Self::NodeEnd => "node end",
            Self::FieldStart(_) => "field",
            Self::FieldEnd => "field end",
            Self::Scalar(_) => "scalar",
            Self::Absent => "absent value",
            Self::SequenceStart(_) => "sequence",
            Self::SequenceEnd => "sequence end",
            Self::MapStart(_) => "map",
            Self::MapKey(_) => "map key",
            Self::MapEnd => "map end",
            Self::OneofStart(_) => "oneof",
            Self::OneofEnd => "oneof end",
            Self::PathMap { .. } => "PathMap",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpannedRhoMatchPathToken<'a> {
    pub start: usize,
    pub end: usize,
    pub token: RhoMatchPathToken<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EncodedRhoMatchPathCursor<'a> {
    input: &'a [u8],
    offset: usize,
    failed: bool,
}

impl<'a> EncodedRhoMatchPathCursor<'a> {
    pub fn new(input: &'a [u8]) -> Result<Self, RhoMatchPathError> {
        if !input.starts_with(&RHO_MATCH_PATH_VERSION) {
            return Err(RhoMatchPathError {
                offset: 0,
                kind: RhoMatchPathErrorKind::InvalidVersion,
            });
        }
        Ok(Self {
            input,
            offset: RHO_MATCH_PATH_VERSION.len(),
            failed: false,
        })
    }

    fn at(input: &'a [u8], offset: usize) -> Self {
        Self {
            input,
            offset,
            failed: false,
        }
    }

    pub const fn offset(&self) -> usize { self.offset }

    fn parse_next(&mut self) -> Result<SpannedRhoMatchPathToken<'a>, RhoMatchPathError> {
        let start = self.offset;
        let tag = self.take_byte()?;
        let token = match tag {
            NODE_START => {
                let type_id = self.take_u16()?;
                let program = type_id
                    .checked_sub(1)
                    .and_then(|index| RHO_MATCH_REGISTRY.get(usize::from(index)))
                    .map(|(_, program)| *program)
                    .filter(|program| program.type_id == type_id)
                    .ok_or_else(|| {
                        self.error_at(start, RhoMatchPathErrorKind::UnknownTypeId(type_id))
                    })?;
                RhoMatchPathToken::NodeStart { program }
            }
            NODE_END => RhoMatchPathToken::NodeEnd,
            FIELD_START => RhoMatchPathToken::FieldStart(self.take_u32()?),
            FIELD_END => RhoMatchPathToken::FieldEnd,
            BOOL => {
                let value = self.take_byte()?;
                let value = match value {
                    0 => false,
                    1 => true,
                    value => {
                        return Err(self
                            .error_at(self.offset - 1, RhoMatchPathErrorKind::InvalidBool(value)));
                    }
                };
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::Bool(value))
            }
            I32 => RhoMatchPathToken::Scalar(RhoMatchPathScalar::I32(i32::from_be_bytes(
                *self.take_array()?,
            ))),
            U32 => RhoMatchPathToken::Scalar(RhoMatchPathScalar::U32(u32::from_be_bytes(
                *self.take_array()?,
            ))),
            I64 => RhoMatchPathToken::Scalar(RhoMatchPathScalar::I64(i64::from_be_bytes(
                *self.take_array()?,
            ))),
            U64 => RhoMatchPathToken::Scalar(RhoMatchPathScalar::U64(u64::from_be_bytes(
                *self.take_array()?,
            ))),
            BYTES => {
                let value = self.take_length_prefixed()?;
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::Bytes(value))
            }
            STRING => {
                let value = self.take_string()?;
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::String(value))
            }
            STRING_SEQUENCE => {
                let count = self.take_u64()?;
                let payload_start = self.offset;
                for _ in 0..count {
                    self.take_string()?;
                }
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::StringSequence {
                    count,
                    encoded: &self.input[payload_start..self.offset],
                })
            }
            BYTES_SEQUENCE => {
                let count = self.take_u64()?;
                let payload_start = self.offset;
                for _ in 0..count {
                    self.take_length_prefixed()?;
                }
                RhoMatchPathToken::Scalar(RhoMatchPathScalar::BytesSequence {
                    count,
                    encoded: &self.input[payload_start..self.offset],
                })
            }
            ABSENT => RhoMatchPathToken::Absent,
            SEQUENCE_START => RhoMatchPathToken::SequenceStart(self.take_u64()?),
            SEQUENCE_END => RhoMatchPathToken::SequenceEnd,
            MAP_START => RhoMatchPathToken::MapStart(self.take_u64()?),
            MAP_KEY => RhoMatchPathToken::MapKey(self.take_string()?),
            MAP_END => RhoMatchPathToken::MapEnd,
            ONEOF_START => {
                let tag = self.take_u32()?;
                if tag == 0 {
                    return Err(self.error_at(start, RhoMatchPathErrorKind::ZeroOneofTag));
                }
                RhoMatchPathToken::OneofStart(tag)
            }
            ONEOF_END => RhoMatchPathToken::OneofEnd,
            PATHMAP => {
                let raw_mode = self.take_byte()?;
                let mode = EPathMapMode::try_from(raw_mode).map_err(|_| {
                    self.error_at(start, RhoMatchPathErrorKind::InvalidPathMapMode(raw_mode))
                })?;
                let len = self.take_u64()?;
                if mode == EPathMapMode::Empty && len != 0 {
                    return Err(
                        self.error_at(start, RhoMatchPathErrorKind::NeutralPathMapHasEntries(len))
                    );
                }
                let digest = self.take_array()?;
                RhoMatchPathToken::PathMap { mode, len, digest }
            }
            token => {
                return Err(self.error_at(start, RhoMatchPathErrorKind::UnknownToken(token)));
            }
        };
        Ok(SpannedRhoMatchPathToken {
            start,
            end: self.offset,
            token,
        })
    }

    fn take_byte(&mut self) -> Result<u8, RhoMatchPathError> {
        let value = self
            .input
            .get(self.offset)
            .copied()
            .ok_or_else(|| self.error_at(self.offset, RhoMatchPathErrorKind::Truncated))?;
        self.offset += 1;
        Ok(value)
    }

    fn take_u16(&mut self) -> Result<u16, RhoMatchPathError> {
        Ok(u16::from_be_bytes(*self.take_array()?))
    }

    fn take_u32(&mut self) -> Result<u32, RhoMatchPathError> {
        Ok(u32::from_be_bytes(*self.take_array()?))
    }

    fn take_u64(&mut self) -> Result<u64, RhoMatchPathError> {
        Ok(u64::from_be_bytes(*self.take_array()?))
    }

    fn take_array<const N: usize>(&mut self) -> Result<&'a [u8; N], RhoMatchPathError> {
        let bytes = self.take(N)?;
        Ok(bytes.try_into().expect("take returned the requested width"))
    }

    fn take_length_prefixed(&mut self) -> Result<&'a [u8], RhoMatchPathError> {
        let len_offset = self.offset;
        let encoded_len = self.take_u64()?;
        let len = usize::try_from(encoded_len).map_err(|_| {
            self.error_at(
                len_offset,
                RhoMatchPathErrorKind::LengthOverflow(encoded_len),
            )
        })?;
        self.take(len)
    }

    fn take_string(&mut self) -> Result<&'a str, RhoMatchPathError> {
        let start = self.offset;
        let bytes = self.take_length_prefixed()?;
        std::str::from_utf8(bytes)
            .map_err(|_| self.error_at(start, RhoMatchPathErrorKind::InvalidUtf8))
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], RhoMatchPathError> {
        let end = self.offset.checked_add(len).ok_or_else(|| {
            self.error_at(self.offset, RhoMatchPathErrorKind::LengthOverflow(u64::MAX))
        })?;
        let bytes = self
            .input
            .get(self.offset..end)
            .ok_or_else(|| self.error_at(self.offset, RhoMatchPathErrorKind::Truncated))?;
        self.offset = end;
        Ok(bytes)
    }

    fn error_at(&self, offset: usize, kind: RhoMatchPathErrorKind) -> RhoMatchPathError {
        RhoMatchPathError { offset, kind }
    }
}

impl<'a> Iterator for EncodedRhoMatchPathCursor<'a> {
    type Item = Result<SpannedRhoMatchPathToken<'a>, RhoMatchPathError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.offset == self.input.len() {
            return None;
        }
        let result = self.parse_next();
        if result.is_err() {
            self.failed = true;
        }
        Some(result)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ValidatedRhoMatchPath<'a> {
    bytes: &'a [u8],
    root_program: &'static RhoMatchProgram,
}

impl<'a> ValidatedRhoMatchPath<'a> {
    pub const fn bytes(self) -> &'a [u8] { self.bytes }

    pub const fn root_program(self) -> &'static RhoMatchProgram { self.root_program }

    /// Re-emit the canonical bytes represented by this validated token stream.
    ///
    /// `RMP1` is intentionally a structural search image rather than a second
    /// `Par` serialization: an `EPathMap` token carries a key-topology digest,
    /// so reconstructing the original term would be lossy by design. This is
    /// therefore the codec's lossless round trip. It decodes every token and
    /// re-encodes its typed value with fixed-width integers, without allocating
    /// a recursive rhoapi object graph.
    pub fn to_canonical_bytes(self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.bytes.len());
        output.extend_from_slice(&RHO_MATCH_PATH_VERSION);
        let cursor = EncodedRhoMatchPathCursor::new(self.bytes)
            .expect("validated RMP1 bytes retain their version");
        for token in cursor {
            let token = token.expect("validated RMP1 bytes retain their token grammar");
            append_canonical_token(&mut output, token.token);
        }
        output
    }

    /// Return the encoded extent of a value beginning at a token boundary.
    pub fn value(self, start: usize) -> Result<&'a [u8], RhoMatchPathError> {
        let mut boundary_cursor = EncodedRhoMatchPathCursor::new(self.bytes)?;
        let first = loop {
            let Some(next) = boundary_cursor.next() else {
                return Err(RhoMatchPathError {
                    offset: start,
                    kind: RhoMatchPathErrorKind::NotATokenBoundary,
                });
            };
            let span = next?;
            if span.start == start {
                break span;
            }
            if span.start > start {
                return Err(RhoMatchPathError {
                    offset: start,
                    kind: RhoMatchPathErrorKind::NotATokenBoundary,
                });
            }
        };
        let end = value_end(self.bytes, first)?;
        Ok(&self.bytes[start..end])
    }
}

impl PartialEq for ValidatedRhoMatchPath<'_> {
    fn eq(&self, other: &Self) -> bool { self.bytes == other.bytes }
}

impl Eq for ValidatedRhoMatchPath<'_> {}

impl PartialOrd for ValidatedRhoMatchPath<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for ValidatedRhoMatchPath<'_> {
    fn cmp(&self, other: &Self) -> Ordering { self.bytes.cmp(other.bytes) }
}

impl Hash for ValidatedRhoMatchPath<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) { self.bytes.hash(state); }
}

fn append_canonical_token(output: &mut Vec<u8>, token: RhoMatchPathToken<'_>) {
    match token {
        RhoMatchPathToken::NodeStart { program } => {
            output.push(NODE_START);
            output.extend_from_slice(&program.type_id.to_be_bytes());
        }
        RhoMatchPathToken::NodeEnd => output.push(NODE_END),
        RhoMatchPathToken::FieldStart(tag) => {
            output.push(FIELD_START);
            output.extend_from_slice(&tag.to_be_bytes());
        }
        RhoMatchPathToken::FieldEnd => output.push(FIELD_END),
        RhoMatchPathToken::Scalar(value) => append_canonical_scalar(output, value),
        RhoMatchPathToken::Absent => output.push(ABSENT),
        RhoMatchPathToken::SequenceStart(len) => {
            output.push(SEQUENCE_START);
            output.extend_from_slice(&len.to_be_bytes());
        }
        RhoMatchPathToken::SequenceEnd => output.push(SEQUENCE_END),
        RhoMatchPathToken::MapStart(len) => {
            output.push(MAP_START);
            output.extend_from_slice(&len.to_be_bytes());
        }
        RhoMatchPathToken::MapKey(key) => {
            output.push(MAP_KEY);
            append_canonical_bytes(output, key.as_bytes());
        }
        RhoMatchPathToken::MapEnd => output.push(MAP_END),
        RhoMatchPathToken::OneofStart(tag) => {
            output.push(ONEOF_START);
            output.extend_from_slice(&tag.to_be_bytes());
        }
        RhoMatchPathToken::OneofEnd => output.push(ONEOF_END),
        RhoMatchPathToken::PathMap { mode, len, digest } => {
            output.push(PATHMAP);
            output.push(mode as u8);
            output.extend_from_slice(&len.to_be_bytes());
            output.extend_from_slice(digest);
        }
    }
}

fn append_canonical_scalar(output: &mut Vec<u8>, value: RhoMatchPathScalar<'_>) {
    match value {
        RhoMatchPathScalar::Bool(value) => output.extend_from_slice(&[BOOL, u8::from(value)]),
        RhoMatchPathScalar::I32(value) => {
            output.push(I32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoMatchPathScalar::U32(value) => {
            output.push(U32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoMatchPathScalar::I64(value) => {
            output.push(I64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoMatchPathScalar::U64(value) => {
            output.push(U64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoMatchPathScalar::Bytes(value) => {
            output.push(BYTES);
            append_canonical_bytes(output, value);
        }
        RhoMatchPathScalar::String(value) => {
            output.push(STRING);
            append_canonical_bytes(output, value.as_bytes());
        }
        RhoMatchPathScalar::StringSequence { count, encoded } => {
            output.push(STRING_SEQUENCE);
            output.extend_from_slice(&count.to_be_bytes());
            output.extend_from_slice(encoded);
        }
        RhoMatchPathScalar::BytesSequence { count, encoded } => {
            output.push(BYTES_SEQUENCE);
            output.extend_from_slice(&count.to_be_bytes());
            output.extend_from_slice(encoded);
        }
    }
}

fn append_canonical_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let len = u64::try_from(value.len()).expect("an allocated RMP1 field length fits in u64");
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(value);
}

pub fn validate_match_path(bytes: &[u8]) -> Result<ValidatedRhoMatchPath<'_>, RhoMatchPathError> {
    let mut cursor = EncodedRhoMatchPathCursor::new(bytes)?;
    let mut frames = vec![ValidationFrame::Root(ValueState::Need)];
    let mut root_program = None;
    for next in cursor.by_ref() {
        let span = next?;
        match span.token {
            RhoMatchPathToken::NodeStart { program } => {
                start_value(&mut frames, ValueKind::Node(program.type_id), span.start)?;
                if root_program.is_none() {
                    root_program = Some(program);
                }
                frames.push(ValidationFrame::Node {
                    program,
                    next_field: 0,
                });
            }
            RhoMatchPathToken::NodeEnd => {
                let frame = pop_expected(&mut frames, "node", span.start)?;
                let ValidationFrame::Node {
                    program,
                    next_field,
                } = frame
                else {
                    return Err(unexpected(span.start, "matching frame end", "node end"));
                };
                if let Some(field) = program.fields.get(next_field) {
                    return Err(RhoMatchPathError {
                        offset: span.start,
                        kind: RhoMatchPathErrorKind::MissingFields {
                            type_name: program.type_name,
                            next_tag: field.tag,
                        },
                    });
                }
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::FieldStart(tag) => {
                let Some(ValidationFrame::Node {
                    program,
                    next_field,
                }) = frames.last_mut()
                else {
                    return Err(unexpected(span.start, "node field or node end", "field"));
                };
                let Some(field) = program.fields.get(*next_field) else {
                    return Err(RhoMatchPathError {
                        offset: span.start,
                        kind: RhoMatchPathErrorKind::UnexpectedField {
                            type_name: program.type_name,
                            expected: None,
                            actual: tag,
                        },
                    });
                };
                if field.tag != tag {
                    return Err(RhoMatchPathError {
                        offset: span.start,
                        kind: RhoMatchPathErrorKind::UnexpectedField {
                            type_name: program.type_name,
                            expected: Some(field.tag),
                            actual: tag,
                        },
                    });
                }
                *next_field += 1;
                frames.push(ValidationFrame::Field {
                    field,
                    state: ValueState::Need,
                });
            }
            RhoMatchPathToken::FieldEnd => {
                let frame = pop_expected(&mut frames, "field", span.start)?;
                if !matches!(frame, ValidationFrame::Field {
                    state: ValueState::Complete,
                    ..
                }) {
                    return Err(unexpected(span.start, "complete field value", "field end"));
                }
            }
            RhoMatchPathToken::Scalar(value) => {
                let kind = scalar_kind(value);
                start_value(&mut frames, kind, span.start)?;
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::Absent => {
                start_value(&mut frames, ValueKind::Absent, span.start)?;
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::SequenceStart(remaining) => {
                let element_type_id = match frames.last() {
                    Some(ValidationFrame::Field {
                        field:
                            RhoMatchField {
                                kind: RhoMatchFieldKind::MessageSequence(type_id),
                                ..
                            },
                        ..
                    }) => *type_id,
                    _ => 0,
                };
                start_value(&mut frames, ValueKind::Sequence, span.start)?;
                frames.push(ValidationFrame::Sequence {
                    remaining,
                    state: ValueState::Need,
                    element_type_id,
                });
            }
            RhoMatchPathToken::SequenceEnd => {
                let frame = pop_expected(&mut frames, "sequence", span.start)?;
                if !matches!(frame, ValidationFrame::Sequence {
                    remaining: 0,
                    state: ValueState::Need,
                    ..
                }) {
                    return Err(unexpected(
                        span.start,
                        "all declared sequence elements",
                        "sequence end",
                    ));
                }
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::MapStart(remaining) => {
                let value_type_id = match frames.last() {
                    Some(ValidationFrame::Field {
                        field:
                            RhoMatchField {
                                kind: RhoMatchFieldKind::StringMessageMap(type_id),
                                ..
                            },
                        ..
                    }) => *type_id,
                    _ => 0,
                };
                start_value(&mut frames, ValueKind::Map, span.start)?;
                frames.push(ValidationFrame::Map {
                    remaining,
                    state: MapState::NeedKey,
                    value_type_id,
                });
            }
            RhoMatchPathToken::MapKey(_) => {
                let Some(ValidationFrame::Map {
                    remaining, state, ..
                }) = frames.last_mut()
                else {
                    return Err(unexpected(span.start, "map key or map end", "map key"));
                };
                if *remaining == 0 || *state != MapState::NeedKey {
                    return Err(unexpected(span.start, "map value or map end", "map key"));
                }
                *state = MapState::NeedValue;
            }
            RhoMatchPathToken::MapEnd => {
                let frame = pop_expected(&mut frames, "map", span.start)?;
                if !matches!(frame, ValidationFrame::Map {
                    remaining: 0,
                    state: MapState::NeedKey,
                    ..
                }) {
                    return Err(unexpected(
                        span.start,
                        "all declared map entries",
                        "map end",
                    ));
                }
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::OneofStart(tag) => {
                let (field_name, variants) = match frames.last() {
                    Some(ValidationFrame::Field {
                        field:
                            RhoMatchField {
                                name,
                                kind: RhoMatchFieldKind::Oneof(variants),
                                ..
                            },
                        ..
                    }) => (*name, *variants),
                    _ => ("<non-oneof>", &[][..]),
                };
                let variant = variants.iter().find(|variant| variant.tag == tag).ok_or(
                    RhoMatchPathError {
                        offset: span.start,
                        kind: RhoMatchPathErrorKind::UnknownOneofTag {
                            field: field_name,
                            tag,
                        },
                    },
                )?;
                start_value(&mut frames, ValueKind::Oneof, span.start)?;
                frames.push(ValidationFrame::Oneof {
                    variant,
                    state: ValueState::Need,
                });
            }
            RhoMatchPathToken::OneofEnd => {
                let frame = pop_expected(&mut frames, "oneof", span.start)?;
                if !matches!(frame, ValidationFrame::Oneof {
                    state: ValueState::Complete,
                    ..
                }) {
                    return Err(unexpected(
                        span.start,
                        "complete oneof payload",
                        "oneof end",
                    ));
                }
                complete_value(&mut frames, span.start)?;
            }
            RhoMatchPathToken::PathMap { .. } => {
                start_value(&mut frames, ValueKind::PathMap, span.start)?;
                complete_value(&mut frames, span.start)?;
            }
        }
    }

    if frames != [ValidationFrame::Root(ValueState::Complete)] {
        return Err(RhoMatchPathError {
            offset: cursor.offset(),
            kind: RhoMatchPathErrorKind::Unclosed(frame_name(
                frames.last().expect("root frame always exists"),
            )),
        });
    }
    Ok(ValidatedRhoMatchPath {
        bytes,
        root_program: root_program.expect("a complete root value is a node"),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueState {
    Need,
    Active,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapState {
    NeedKey,
    NeedValue,
    ActiveValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidationFrame {
    Root(ValueState),
    Node {
        program: &'static RhoMatchProgram,
        next_field: usize,
    },
    Field {
        field: &'static RhoMatchField,
        state: ValueState,
    },
    Sequence {
        remaining: u64,
        state: ValueState,
        element_type_id: u16,
    },
    Map {
        remaining: u64,
        state: MapState,
        value_type_id: u16,
    },
    Oneof {
        variant: &'static RhoMatchOneofVariant,
        state: ValueState,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueKind {
    Bool,
    I32,
    U32,
    I64,
    U64,
    Bytes,
    String,
    StringSequence,
    BytesSequence,
    Absent,
    Node(u16),
    Sequence,
    Map,
    Oneof,
    PathMap,
}

fn scalar_kind(value: RhoMatchPathScalar<'_>) -> ValueKind {
    match value {
        RhoMatchPathScalar::Bool(_) => ValueKind::Bool,
        RhoMatchPathScalar::I32(_) => ValueKind::I32,
        RhoMatchPathScalar::U32(_) => ValueKind::U32,
        RhoMatchPathScalar::I64(_) => ValueKind::I64,
        RhoMatchPathScalar::U64(_) => ValueKind::U64,
        RhoMatchPathScalar::Bytes(_) => ValueKind::Bytes,
        RhoMatchPathScalar::String(_) => ValueKind::String,
        RhoMatchPathScalar::StringSequence { .. } => ValueKind::StringSequence,
        RhoMatchPathScalar::BytesSequence { .. } => ValueKind::BytesSequence,
    }
}

fn start_value(
    frames: &mut [ValidationFrame],
    kind: ValueKind,
    offset: usize,
) -> Result<(), RhoMatchPathError> {
    let frame = frames.last_mut().expect("root frame always exists");
    let allowed = match frame {
        ValidationFrame::Root(state) => {
            let allowed = *state == ValueState::Need && matches!(kind, ValueKind::Node(_));
            if allowed {
                *state = ValueState::Active;
            }
            allowed
        }
        ValidationFrame::Field { field, state } => {
            let allowed = *state == ValueState::Need && field_accepts(field.kind, kind);
            if allowed {
                *state = ValueState::Active;
            }
            allowed
        }
        ValidationFrame::Sequence {
            remaining,
            state,
            element_type_id,
        } => {
            let allowed = *remaining != 0
                && *state == ValueState::Need
                && kind == ValueKind::Node(*element_type_id);
            if allowed {
                *state = ValueState::Active;
            }
            allowed
        }
        ValidationFrame::Map {
            state,
            value_type_id,
            ..
        } => {
            let allowed = *state == MapState::NeedValue && kind == ValueKind::Node(*value_type_id);
            if allowed {
                *state = MapState::ActiveValue;
            }
            allowed
        }
        ValidationFrame::Oneof { variant, state } => {
            let allowed = *state == ValueState::Need && payload_accepts(variant.payload, kind);
            if allowed {
                *state = ValueState::Active;
            }
            allowed
        }
        ValidationFrame::Node { .. } => false,
    };
    if allowed {
        Ok(())
    } else {
        if let (Some(expected), ValueKind::Node(actual)) = (expected_node_type(frame), kind) {
            return Err(RhoMatchPathError {
                offset,
                kind: RhoMatchPathErrorKind::UnexpectedTypeId { expected, actual },
            });
        }
        Err(unexpected(
            offset,
            frame_value_expectation(frame),
            value_kind_name(kind),
        ))
    }
}

fn expected_node_type(frame: &ValidationFrame) -> Option<u16> {
    match frame {
        ValidationFrame::Field {
            field:
                RhoMatchField {
                    kind: RhoMatchFieldKind::OptionalMessage(type_id),
                    ..
                },
            ..
        }
        | ValidationFrame::Sequence {
            element_type_id: type_id,
            ..
        }
        | ValidationFrame::Map {
            value_type_id: type_id,
            ..
        } => Some(*type_id),
        ValidationFrame::Oneof {
            variant:
                RhoMatchOneofVariant {
                    payload: RhoMatchPayloadKind::Message(type_id),
                    ..
                },
            ..
        } => Some(*type_id),
        _ => None,
    }
}

fn complete_value(frames: &mut [ValidationFrame], offset: usize) -> Result<(), RhoMatchPathError> {
    let frame = frames.last_mut().expect("root frame always exists");
    let completed = match frame {
        ValidationFrame::Root(state)
        | ValidationFrame::Field { state, .. }
        | ValidationFrame::Oneof { state, .. } => {
            if *state == ValueState::Active {
                *state = ValueState::Complete;
                true
            } else {
                false
            }
        }
        ValidationFrame::Sequence {
            remaining, state, ..
        } => {
            if *state == ValueState::Active {
                *remaining -= 1;
                *state = ValueState::Need;
                true
            } else {
                false
            }
        }
        ValidationFrame::Map {
            remaining, state, ..
        } => {
            if *state == MapState::ActiveValue {
                *remaining -= 1;
                *state = MapState::NeedKey;
                true
            } else {
                false
            }
        }
        ValidationFrame::Node { .. } => false,
    };
    if completed {
        Ok(())
    } else {
        Err(unexpected(offset, "active parent value", frame_name(frame)))
    }
}

fn field_accepts(expected: RhoMatchFieldKind, actual: ValueKind) -> bool {
    match expected {
        RhoMatchFieldKind::Bool => actual == ValueKind::Bool,
        RhoMatchFieldKind::I32 => actual == ValueKind::I32,
        RhoMatchFieldKind::U32 => actual == ValueKind::U32,
        RhoMatchFieldKind::I64 => actual == ValueKind::I64,
        RhoMatchFieldKind::U64 => actual == ValueKind::U64,
        RhoMatchFieldKind::Bytes => actual == ValueKind::Bytes,
        RhoMatchFieldKind::String => actual == ValueKind::String,
        RhoMatchFieldKind::StringSequence => actual == ValueKind::StringSequence,
        RhoMatchFieldKind::BytesSequence => actual == ValueKind::BytesSequence,
        RhoMatchFieldKind::OptionalMessage(type_id) => {
            matches!(actual, ValueKind::Absent) || actual == ValueKind::Node(type_id)
        }
        RhoMatchFieldKind::MessageSequence(_) => actual == ValueKind::Sequence,
        RhoMatchFieldKind::StringMessageMap(_) => actual == ValueKind::Map,
        RhoMatchFieldKind::Oneof(_) => matches!(actual, ValueKind::Absent | ValueKind::Oneof),
        RhoMatchFieldKind::PathMap => actual == ValueKind::PathMap,
    }
}

fn payload_accepts(expected: RhoMatchPayloadKind, actual: ValueKind) -> bool {
    match expected {
        RhoMatchPayloadKind::Bool => actual == ValueKind::Bool,
        RhoMatchPayloadKind::I32 => actual == ValueKind::I32,
        RhoMatchPayloadKind::U32 => actual == ValueKind::U32,
        RhoMatchPayloadKind::I64 => actual == ValueKind::I64,
        RhoMatchPayloadKind::U64 => actual == ValueKind::U64,
        RhoMatchPayloadKind::Bytes => actual == ValueKind::Bytes,
        RhoMatchPayloadKind::String => actual == ValueKind::String,
        RhoMatchPayloadKind::Message(type_id) => actual == ValueKind::Node(type_id),
    }
}

fn frame_value_expectation(frame: &ValidationFrame) -> &'static str {
    match frame {
        ValidationFrame::Root(_) => "one root node",
        ValidationFrame::Node { .. } => "field or node end",
        ValidationFrame::Field { field, .. } => match field.kind {
            RhoMatchFieldKind::Bool => "boolean",
            RhoMatchFieldKind::I32 => "i32",
            RhoMatchFieldKind::U32 => "u32",
            RhoMatchFieldKind::I64 => "i64",
            RhoMatchFieldKind::U64 => "u64",
            RhoMatchFieldKind::Bytes => "bytes",
            RhoMatchFieldKind::String => "string",
            RhoMatchFieldKind::StringSequence => "string sequence",
            RhoMatchFieldKind::BytesSequence => "bytes sequence",
            RhoMatchFieldKind::OptionalMessage(_) => "message or absent value",
            RhoMatchFieldKind::MessageSequence(_) => "message sequence",
            RhoMatchFieldKind::StringMessageMap(_) => "string/message map",
            RhoMatchFieldKind::Oneof(_) => "oneof or absent value",
            RhoMatchFieldKind::PathMap => "PathMap",
        },
        ValidationFrame::Sequence { .. } => "sequence element or sequence end",
        ValidationFrame::Map { state, .. } => match state {
            MapState::NeedKey => "map key or map end",
            MapState::NeedValue | MapState::ActiveValue => "map value",
        },
        ValidationFrame::Oneof { .. } => "declared oneof payload",
    }
}

fn frame_name(frame: &ValidationFrame) -> &'static str {
    match frame {
        ValidationFrame::Root(_) => "root value",
        ValidationFrame::Node { .. } => "node end",
        ValidationFrame::Field { .. } => "field end",
        ValidationFrame::Sequence { .. } => "sequence end",
        ValidationFrame::Map { .. } => "map end",
        ValidationFrame::Oneof { .. } => "oneof end",
    }
}

fn value_kind_name(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Bool => "boolean",
        ValueKind::I32 => "i32",
        ValueKind::U32 => "u32",
        ValueKind::I64 => "i64",
        ValueKind::U64 => "u64",
        ValueKind::Bytes => "bytes",
        ValueKind::String => "string",
        ValueKind::StringSequence => "string sequence",
        ValueKind::BytesSequence => "bytes sequence",
        ValueKind::Absent => "absent value",
        ValueKind::Node(_) => "node",
        ValueKind::Sequence => "sequence",
        ValueKind::Map => "map",
        ValueKind::Oneof => "oneof",
        ValueKind::PathMap => "PathMap",
    }
}

fn pop_expected(
    frames: &mut Vec<ValidationFrame>,
    expected: &'static str,
    offset: usize,
) -> Result<ValidationFrame, RhoMatchPathError> {
    if frames.len() == 1 {
        return Err(unexpected(offset, expected, "root"));
    }
    Ok(frames.pop().expect("length checked"))
}

fn unexpected(offset: usize, expected: &'static str, actual: &'static str) -> RhoMatchPathError {
    RhoMatchPathError {
        offset,
        kind: RhoMatchPathErrorKind::UnexpectedToken { expected, actual },
    }
}

fn value_end(
    bytes: &[u8],
    first: SpannedRhoMatchPathToken<'_>,
) -> Result<usize, RhoMatchPathError> {
    let expected = match first.token {
        RhoMatchPathToken::NodeStart { .. } => Some(NODE_END),
        RhoMatchPathToken::SequenceStart(_) => Some(SEQUENCE_END),
        RhoMatchPathToken::MapStart(_) => Some(MAP_END),
        RhoMatchPathToken::OneofStart(_) => Some(ONEOF_END),
        RhoMatchPathToken::Scalar(_)
        | RhoMatchPathToken::Absent
        | RhoMatchPathToken::PathMap { .. } => return Ok(first.end),
        _ => return Err(unexpected(first.start, "value", first.token.name())),
    };
    let mut closers = vec![expected.expect("compound value has a closer")];
    let mut cursor = EncodedRhoMatchPathCursor::at(bytes, first.end);
    for next in cursor.by_ref() {
        let span = next?;
        match span.token {
            RhoMatchPathToken::NodeStart { .. } => closers.push(NODE_END),
            RhoMatchPathToken::FieldStart(_) => closers.push(FIELD_END),
            RhoMatchPathToken::SequenceStart(_) => closers.push(SEQUENCE_END),
            RhoMatchPathToken::MapStart(_) => closers.push(MAP_END),
            RhoMatchPathToken::OneofStart(_) => closers.push(ONEOF_END),
            RhoMatchPathToken::NodeEnd => close_extent(&mut closers, NODE_END, span.start)?,
            RhoMatchPathToken::FieldEnd => close_extent(&mut closers, FIELD_END, span.start)?,
            RhoMatchPathToken::SequenceEnd => close_extent(&mut closers, SEQUENCE_END, span.start)?,
            RhoMatchPathToken::MapEnd => close_extent(&mut closers, MAP_END, span.start)?,
            RhoMatchPathToken::OneofEnd => close_extent(&mut closers, ONEOF_END, span.start)?,
            _ => {}
        }
        if closers.is_empty() {
            return Ok(span.end);
        }
    }
    Err(RhoMatchPathError {
        offset: cursor.offset(),
        kind: RhoMatchPathErrorKind::Unclosed("value end"),
    })
}

fn close_extent(closers: &mut Vec<u8>, actual: u8, offset: usize) -> Result<(), RhoMatchPathError> {
    let expected = closers
        .last()
        .copied()
        .ok_or_else(|| unexpected(offset, "value token", token_name(actual)))?;
    if expected != actual {
        return Err(unexpected(offset, token_name(expected), token_name(actual)));
    }
    closers.pop();
    Ok(())
}

fn token_name(token: u8) -> &'static str {
    match token {
        NODE_END => "node end",
        FIELD_END => "field end",
        SEQUENCE_END => "sequence end",
        MAP_END => "map end",
        ONEOF_END => "oneof end",
        _ => "token",
    }
}
