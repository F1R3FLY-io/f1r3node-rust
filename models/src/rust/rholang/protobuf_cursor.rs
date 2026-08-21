//! Borrowed, restartable cursors over canonical protobuf messages.
//!
//! The generated protobuf decoder is the authority when a resident Rust value
//! is required.  Some analyses need only selected child-message regions,
//! however, and materialising every recursively shorter suffix turns an
//! otherwise linear walk into a quadratic one.  This module supplies the
//! complementary byte view: it validates protobuf framing and canonical field
//! order while returning slices borrowed from the already-canonical source.
//!
//! It deliberately does not interpret a schema beyond [`CanonicalParProcessIndex`].
//! Typed consumers remain responsible for checking the fields and wire types of
//! their own component messages.

use std::fmt;

/// A half-open range inside the message passed to a cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalByteRange {
    pub start: usize,
    pub end: usize,
}

impl CanonicalByteRange {
    #[inline]
    pub const fn is_empty(self) -> bool { self.start == self.end }

    #[inline]
    pub fn slice(self, message: &[u8]) -> Option<&[u8]> { message.get(self.start..self.end) }
}

/// The non-group protobuf wire types admitted by the Rholang schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CanonicalWireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    Fixed32 = 5,
}

/// A framing or canonicality failure in a borrowed protobuf cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalProtobufError {
    TruncatedVarint {
        offset: usize,
    },
    NonMinimalVarint {
        offset: usize,
    },
    VarintOverflow {
        offset: usize,
    },
    InvalidFieldNumber {
        offset: usize,
        field: u64,
    },
    UnsupportedWireType {
        offset: usize,
        wire: u8,
    },
    DecreasingFieldOrder {
        offset: usize,
        previous: u32,
        current: u32,
    },
    LengthOverflow {
        offset: usize,
        length: u64,
    },
    TruncatedField {
        offset: usize,
        required_end: usize,
        message_len: usize,
    },
    UnexpectedField {
        offset: usize,
        field: u32,
    },
    UnexpectedWireType {
        offset: usize,
        field: u32,
        expected: CanonicalWireType,
        actual: CanonicalWireType,
    },
    NonCanonicalDefault {
        offset: usize,
        field: u32,
    },
    DuplicateSingularField {
        offset: usize,
        field: u32,
    },
}

impl fmt::Display for CanonicalProtobufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedVarint { offset } => {
                write!(f, "truncated protobuf varint at byte {offset}")
            }
            Self::NonMinimalVarint { offset } => {
                write!(f, "non-minimal protobuf varint at byte {offset}")
            }
            Self::VarintOverflow { offset } => {
                write!(f, "protobuf varint overflows u64 at byte {offset}")
            }
            Self::InvalidFieldNumber { offset, field } => {
                write!(f, "invalid protobuf field {field} at byte {offset}")
            }
            Self::UnsupportedWireType { offset, wire } => {
                write!(f, "unsupported protobuf wire type {wire} at byte {offset}")
            }
            Self::DecreasingFieldOrder {
                offset,
                previous,
                current,
            } => write!(
                f,
                "protobuf field order decreases from {previous} to {current} at byte {offset}"
            ),
            Self::LengthOverflow { offset, length } => write!(
                f,
                "protobuf length {length} cannot be represented at byte {offset}"
            ),
            Self::TruncatedField {
                offset,
                required_end,
                message_len,
            } => write!(
                f,
                "protobuf field at byte {offset} ends at {required_end}, beyond message length {message_len}"
            ),
            Self::UnexpectedField { offset, field } => {
                write!(f, "unexpected protobuf field {field} at byte {offset}")
            }
            Self::UnexpectedWireType {
                offset,
                field,
                expected,
                actual,
            } => write!(
                f,
                "protobuf field {field} at byte {offset} uses {actual:?}, expected {expected:?}"
            ),
            Self::NonCanonicalDefault { offset, field } => write!(
                f,
                "protobuf field {field} explicitly encodes its default at byte {offset}"
            ),
            Self::DuplicateSingularField { offset, field } => write!(
                f,
                "protobuf singular field {field} is duplicated at byte {offset}"
            ),
        }
    }
}

impl std::error::Error for CanonicalProtobufError {}

/// The borrowed value portion of one framed field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalFieldValue<'a> {
    Varint(u64),
    Fixed64(&'a [u8]),
    LengthDelimited(&'a [u8]),
    Fixed32(&'a [u8]),
}

/// One canonical protobuf field borrowed from its enclosing message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalField<'a> {
    pub number: u32,
    pub wire_type: CanonicalWireType,
    pub value: CanonicalFieldValue<'a>,
    pub encoded_range: CanonicalByteRange,
    pub payload_range: CanonicalByteRange,
}

impl<'a> CanonicalField<'a> {
    pub fn expect_length_delimited(self) -> Result<&'a [u8], CanonicalProtobufError> {
        match self.value {
            CanonicalFieldValue::LengthDelimited(payload) => Ok(payload),
            _ => Err(CanonicalProtobufError::UnexpectedWireType {
                offset: self.encoded_range.start,
                field: self.number,
                expected: CanonicalWireType::LengthDelimited,
                actual: self.wire_type,
            }),
        }
    }

    pub fn expect_varint(self) -> Result<u64, CanonicalProtobufError> {
        match self.value {
            CanonicalFieldValue::Varint(value) => Ok(value),
            _ => Err(CanonicalProtobufError::UnexpectedWireType {
                offset: self.encoded_range.start,
                field: self.number,
                expected: CanonicalWireType::Varint,
                actual: self.wire_type,
            }),
        }
    }
}

fn read_canonical_varint(bytes: &[u8], at: &mut usize) -> Result<u64, CanonicalProtobufError> {
    let start = *at;
    let mut value = 0u64;
    for shift_group in 0..10usize {
        let Some(&byte) = bytes.get(*at) else {
            return Err(CanonicalProtobufError::TruncatedVarint { offset: start });
        };
        *at += 1;
        if shift_group == 9 && byte > 1 {
            return Err(CanonicalProtobufError::VarintOverflow { offset: start });
        }
        value |= u64::from(byte & 0x7f) << (shift_group * 7);
        if byte & 0x80 == 0 {
            if shift_group != 0 && byte == 0 {
                return Err(CanonicalProtobufError::NonMinimalVarint { offset: start });
            }
            return Ok(value);
        }
    }
    Err(CanonicalProtobufError::VarintOverflow { offset: start })
}

/// A single forward pass over a canonical protobuf message.
#[derive(Clone, Copy, Debug)]
pub struct CanonicalFieldCursor<'a> {
    message: &'a [u8],
    at: usize,
    last_field: u32,
}

impl<'a> CanonicalFieldCursor<'a> {
    pub const fn new(message: &'a [u8]) -> Self {
        Self {
            message,
            at: 0,
            last_field: 0,
        }
    }

    #[inline]
    pub const fn is_empty(&self) -> bool { self.at == self.message.len() }

    pub fn next_field(&mut self) -> Result<Option<CanonicalField<'a>>, CanonicalProtobufError> {
        if self.is_empty() {
            return Ok(None);
        }
        let field_start = self.at;
        let key = read_canonical_varint(self.message, &mut self.at)?;
        let field_u64 = key >> 3;
        if field_u64 == 0 || field_u64 > 0x1fff_ffff {
            return Err(CanonicalProtobufError::InvalidFieldNumber {
                offset: field_start,
                field: field_u64,
            });
        }
        let number = field_u64 as u32;
        if number < self.last_field {
            return Err(CanonicalProtobufError::DecreasingFieldOrder {
                offset: field_start,
                previous: self.last_field,
                current: number,
            });
        }
        self.last_field = number;

        let wire = (key & 0x07) as u8;
        let wire_type = match wire {
            0 => CanonicalWireType::Varint,
            1 => CanonicalWireType::Fixed64,
            2 => CanonicalWireType::LengthDelimited,
            5 => CanonicalWireType::Fixed32,
            _ => {
                return Err(CanonicalProtobufError::UnsupportedWireType {
                    offset: field_start,
                    wire,
                });
            }
        };

        let (value, payload_range) = match wire_type {
            CanonicalWireType::Varint => {
                let payload_start = self.at;
                let value = read_canonical_varint(self.message, &mut self.at)?;
                (CanonicalFieldValue::Varint(value), CanonicalByteRange {
                    start: payload_start,
                    end: self.at,
                })
            }
            CanonicalWireType::Fixed64 => {
                let payload_start = self.at;
                let end = self
                    .at
                    .checked_add(8)
                    .ok_or(CanonicalProtobufError::TruncatedField {
                        offset: field_start,
                        required_end: usize::MAX,
                        message_len: self.message.len(),
                    })?;
                let payload = self.message.get(self.at..end).ok_or(
                    CanonicalProtobufError::TruncatedField {
                        offset: field_start,
                        required_end: end,
                        message_len: self.message.len(),
                    },
                )?;
                self.at = end;
                (CanonicalFieldValue::Fixed64(payload), CanonicalByteRange {
                    start: payload_start,
                    end,
                })
            }
            CanonicalWireType::LengthDelimited => {
                let length_offset = self.at;
                let length = read_canonical_varint(self.message, &mut self.at)?;
                let length = usize::try_from(length).map_err(|_| {
                    CanonicalProtobufError::LengthOverflow {
                        offset: length_offset,
                        length,
                    }
                })?;
                let payload_start = self.at;
                let end =
                    self.at
                        .checked_add(length)
                        .ok_or(CanonicalProtobufError::LengthOverflow {
                            offset: length_offset,
                            length: length as u64,
                        })?;
                let payload = self.message.get(self.at..end).ok_or(
                    CanonicalProtobufError::TruncatedField {
                        offset: field_start,
                        required_end: end,
                        message_len: self.message.len(),
                    },
                )?;
                self.at = end;
                (
                    CanonicalFieldValue::LengthDelimited(payload),
                    CanonicalByteRange {
                        start: payload_start,
                        end,
                    },
                )
            }
            CanonicalWireType::Fixed32 => {
                let payload_start = self.at;
                let end = self
                    .at
                    .checked_add(4)
                    .ok_or(CanonicalProtobufError::TruncatedField {
                        offset: field_start,
                        required_end: usize::MAX,
                        message_len: self.message.len(),
                    })?;
                let payload = self.message.get(self.at..end).ok_or(
                    CanonicalProtobufError::TruncatedField {
                        offset: field_start,
                        required_end: end,
                        message_len: self.message.len(),
                    },
                )?;
                self.at = end;
                (CanonicalFieldValue::Fixed32(payload), CanonicalByteRange {
                    start: payload_start,
                    end,
                })
            }
        };

        Ok(Some(CanonicalField {
            number,
            wire_type,
            value,
            encoded_range: CanonicalByteRange {
                start: field_start,
                end: self.at,
            },
            payload_range,
        }))
    }
}

/// One contiguous run of repeated length-delimited fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalDelimitedRun<'a> {
    encoded: &'a [u8],
    field: u32,
}

impl<'a> CanonicalDelimitedRun<'a> {
    pub const fn new(encoded: &'a [u8], field: u32) -> Self { Self { encoded, field } }

    pub const fn cursor(self) -> CanonicalDelimitedCursor<'a> {
        CanonicalDelimitedCursor {
            fields: CanonicalFieldCursor::new(self.encoded),
            expected_field: self.field,
        }
    }
}

/// Restartable iterator over the payloads in a repeated message-field run.
#[derive(Clone, Copy, Debug)]
pub struct CanonicalDelimitedCursor<'a> {
    fields: CanonicalFieldCursor<'a>,
    expected_field: u32,
}

impl<'a> CanonicalDelimitedCursor<'a> {
    #[inline]
    pub const fn is_empty(&self) -> bool { self.fields.is_empty() }

    pub fn next_payload(&mut self) -> Result<Option<&'a [u8]>, CanonicalProtobufError> {
        let Some(field) = self.fields.next_field()? else {
            return Ok(None);
        };
        if field.number != self.expected_field {
            return Err(CanonicalProtobufError::UnexpectedField {
                offset: field.encoded_range.start,
                field: field.number,
            });
        }
        field.expect_length_delimited().map(Some)
    }
}

fn extend_run<'a>(
    message: &'a [u8],
    slot: &mut Option<CanonicalDelimitedRun<'a>>,
    field: CanonicalField<'a>,
) -> Result<(), CanonicalProtobufError> {
    field.expect_length_delimited()?;
    match slot {
        Some(run) => {
            let base = message.as_ptr() as usize;
            let start = run.encoded.as_ptr() as usize - base;
            let end = start + run.encoded.len();
            debug_assert_eq!(end, field.encoded_range.start);
            run.encoded = &message[start..field.encoded_range.end];
        }
        None => {
            *slot = Some(CanonicalDelimitedRun::new(
                &message[field.encoded_range.start..field.encoded_range.end],
                field.number,
            ));
        }
    }
    Ok(())
}

/// Constant-size index of the process-relevant constructor runs in a canonical
/// protobuf `Par` body.
///
/// One scan records the byte ranges. Consumers may then visit them in semantic
/// reducer order rather than protobuf tag order without re-scanning the suffix.
#[derive(Clone, Copy, Debug, Default)]
pub struct CanonicalParProcessIndex<'a> {
    pub sends: Option<CanonicalDelimitedRun<'a>>,
    pub receives: Option<CanonicalDelimitedRun<'a>>,
    pub news: Option<CanonicalDelimitedRun<'a>>,
    pub exprs: Option<CanonicalDelimitedRun<'a>>,
    pub matches: Option<CanonicalDelimitedRun<'a>>,
    pub bundles: Option<CanonicalDelimitedRun<'a>>,
    pub conditionals: Option<CanonicalDelimitedRun<'a>>,
}

impl<'a> CanonicalParProcessIndex<'a> {
    pub fn parse(message: &'a [u8]) -> Result<Self, CanonicalProtobufError> {
        let mut index = Self::default();
        let mut fields = CanonicalFieldCursor::new(message);
        let mut saw_locally_free = false;
        let mut saw_connective_used = false;
        while let Some(field) = fields.next_field()? {
            match field.number {
                1 => extend_run(message, &mut index.sends, field)?,
                2 => extend_run(message, &mut index.receives, field)?,
                4 => extend_run(message, &mut index.news, field)?,
                5 => extend_run(message, &mut index.exprs, field)?,
                6 => extend_run(message, &mut index.matches, field)?,
                7 | 8 => {
                    field.expect_length_delimited()?;
                }
                9 if !saw_locally_free => {
                    let payload = field.expect_length_delimited()?;
                    if payload.is_empty() {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset: field.encoded_range.start,
                            field: 9,
                        });
                    }
                    saw_locally_free = true;
                }
                9 => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: 9,
                    });
                }
                10 if !saw_connective_used => {
                    if field.expect_varint()? != 1 {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset: field.encoded_range.start,
                            field: 10,
                        });
                    }
                    saw_connective_used = true;
                }
                10 => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: 10,
                    });
                }
                11 => extend_run(message, &mut index.bundles, field)?,
                12 => extend_run(message, &mut index.conditionals, field)?,
                number => {
                    return Err(CanonicalProtobufError::UnexpectedField {
                        offset: field.encoded_range.start,
                        field: number,
                    });
                }
            }
        }
        Ok(index)
    }
}
