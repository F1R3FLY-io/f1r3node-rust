//! Borrowed and owned restartable cursors over canonical protobuf messages.
//!
//! The generated protobuf decoder is the authority when a resident Rust value
//! is required.  Some analyses need only selected child-message regions,
//! however, and materialising every recursively shorter suffix turns an
//! otherwise linear walk into a quadratic one.  This module supplies the
//! complementary byte view: it validates protobuf framing and canonical field
//! order while returning borrowed slices or constant-size owned regions into
//! the already-canonical source.
//!
//! It deliberately does not interpret a schema beyond
//! [`CanonicalParProcessIndex`] and [`CanonicalParRegionIndex`]. Typed consumers
//! remain responsible for checking the fields and wire types of their own
//! component messages.

use std::fmt;
use std::ops::Range;

use prost::bytes::Bytes;

use super::match_schema_tables::PAR_PROTOBUF_REGION_PROGRAM;
use super::protobuf_schema::{ProtobufField, ProtobufWireType};

#[cfg(test)]
#[path = "../../../tests/support/par_count_header_work.rs"]
pub(crate) mod par_count_header_work;

#[cfg(test)]
#[path = "../../../tests/support/protobuf_varint_access.rs"]
pub(crate) mod varint_test_access;

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

#[derive(Clone)]
pub struct ProtobufRegion {
    backing: Bytes,
    range: Range<usize>,
}

impl ProtobufRegion {
    #[inline]
    pub fn new(backing: Bytes) -> Self {
        let end = backing.len();
        Self {
            backing,
            range: 0..end,
        }
    }

    #[inline]
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        #[cfg(feature = "protobuf-projection-experiment")]
        crate::rust::par_construction::substitution_cost::record(
            crate::rust::par_construction::substitution_cost::Event::CopyRoot(bytes.len()),
        );
        Self::new(Bytes::copy_from_slice(bytes))
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] { &self.backing[self.range.clone()] }

    #[inline]
    pub fn len(&self) -> usize { self.range.end - self.range.start }

    #[inline]
    pub fn is_empty(&self) -> bool { self.range.is_empty() }

    #[inline]
    pub fn absolute_range(&self) -> Range<usize> { self.range.clone() }

    pub fn subregion(&self, relative: CanonicalByteRange) -> Option<Self> {
        if relative.start > relative.end || relative.end > self.len() {
            return None;
        }
        let start = self.range.start.checked_add(relative.start)?;
        let end = self.range.start.checked_add(relative.end)?;
        #[cfg(feature = "protobuf-projection-experiment")]
        crate::rust::par_construction::substitution_cost::record(
            crate::rust::par_construction::substitution_cost::Event::BorrowRegion,
        );
        Some(Self {
            backing: self.backing.clone(),
            range: start..end,
        })
    }

    #[inline]
    pub fn into_bytes(self) -> Bytes { self.backing.slice(self.range) }

    #[inline]
    #[cfg(test)]
    pub(crate) fn backing_bytes(&self) -> Bytes { self.backing.clone() }

    #[inline]
    #[cfg(test)]
    pub(crate) fn shares_backing_with(&self, backing: &Bytes) -> bool {
        self.backing.as_ptr() == backing.as_ptr() && self.backing.len() == backing.len()
    }
}

impl From<Bytes> for ProtobufRegion {
    #[inline]
    fn from(bytes: Bytes) -> Self { Self::new(bytes) }
}

impl From<Vec<u8>> for ProtobufRegion {
    #[inline]
    fn from(bytes: Vec<u8>) -> Self { Self::new(Bytes::from(bytes)) }
}

impl AsRef<[u8]> for ProtobufRegion {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.as_bytes() }
}

impl fmt::Debug for ProtobufRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtobufRegion")
            .field("range", &self.range)
            .field("len", &self.len())
            .finish()
    }
}

impl PartialEq for ProtobufRegion {
    #[inline]
    fn eq(&self, other: &Self) -> bool { self.as_bytes() == other.as_bytes() }
}

impl Eq for ProtobufRegion {}

/// The non-group protobuf wire types admitted by the Rholang schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CanonicalWireType {
    Varint = 0,
    Fixed64 = 1,
    LengthDelimited = 2,
    Fixed32 = 5,
}

/// A framing or canonicality failure in a protobuf cursor.
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

#[inline]
const fn canonical_wire_type(wire: ProtobufWireType) -> CanonicalWireType {
    match wire {
        ProtobufWireType::Varint => CanonicalWireType::Varint,
        ProtobufWireType::Fixed64 => CanonicalWireType::Fixed64,
        ProtobufWireType::LengthDelimited => CanonicalWireType::LengthDelimited,
        ProtobufWireType::Fixed32 => CanonicalWireType::Fixed32,
    }
}

fn par_protobuf_field(
    number: u32,
    actual: CanonicalWireType,
    offset: usize,
) -> Result<&'static ProtobufField, CanonicalProtobufError> {
    let field = PAR_PROTOBUF_REGION_PROGRAM
        .protobuf_fields
        .iter()
        .find(|field| field.tag == number)
        .ok_or(CanonicalProtobufError::UnexpectedField {
            offset,
            field: number,
        })?;
    let expected = field
        .kind
        .wire_type()
        .map(canonical_wire_type)
        .expect("the generated raw Par program contains no oneof field");
    if actual != expected {
        return Err(CanonicalProtobufError::UnexpectedWireType {
            offset,
            field: number,
            expected,
            actual,
        });
    }
    Ok(field)
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalRegionFieldValue {
    Varint(u64),
    Fixed64(ProtobufRegion),
    LengthDelimited(ProtobufRegion),
    Fixed32(ProtobufRegion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRegionField {
    pub number: u32,
    pub wire_type: CanonicalWireType,
    pub value: CanonicalRegionFieldValue,
    pub encoded_range: CanonicalByteRange,
    pub payload_range: CanonicalByteRange,
}

impl CanonicalRegionField {
    pub fn expect_length_delimited(&self) -> Result<&ProtobufRegion, CanonicalProtobufError> {
        match &self.value {
            CanonicalRegionFieldValue::LengthDelimited(payload) => Ok(payload),
            _ => Err(CanonicalProtobufError::UnexpectedWireType {
                offset: self.encoded_range.start,
                field: self.number,
                expected: CanonicalWireType::LengthDelimited,
                actual: self.wire_type,
            }),
        }
    }

    pub fn into_length_delimited(self) -> Result<ProtobufRegion, CanonicalProtobufError> {
        match self.value {
            CanonicalRegionFieldValue::LengthDelimited(payload) => Ok(payload),
            _ => Err(CanonicalProtobufError::UnexpectedWireType {
                offset: self.encoded_range.start,
                field: self.number,
                expected: CanonicalWireType::LengthDelimited,
                actual: self.wire_type,
            }),
        }
    }

    pub fn expect_varint(&self) -> Result<u64, CanonicalProtobufError> {
        match self.value {
            CanonicalRegionFieldValue::Varint(value) => Ok(value),
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
        #[cfg(test)]
        par_count_header_work::record_byte(bytes.as_ptr() as usize + *at);
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
    enforce_numeric_order: bool,
}

impl<'a> CanonicalFieldCursor<'a> {
    pub const fn new(message: &'a [u8]) -> Self {
        Self {
            message,
            at: 0,
            last_field: 0,
            enforce_numeric_order: true,
        }
    }

    /// Frame fields in producer order without assuming that their actual tag
    /// numbers are monotone.
    ///
    /// Prost orders a oneof *slot* by its minimum declared tag, but the
    /// selected arm writes its own tag. A later ordinary field may therefore
    /// have a smaller numeric tag even though the message is exactly the
    /// canonical output of the generated encoder. Schema-aware consumers must
    /// validate the resolved generated slot order instead.
    pub const fn in_prost_slot_order(message: &'a [u8]) -> Self {
        Self {
            message,
            at: 0,
            last_field: 0,
            enforce_numeric_order: false,
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
        if self.enforce_numeric_order && number < self.last_field {
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

#[derive(Clone, Debug)]
pub struct CanonicalRegionFieldCursor {
    message: ProtobufRegion,
    at: usize,
    last_field: u32,
    enforce_numeric_order: bool,
}

impl CanonicalRegionFieldCursor {
    pub fn new(message: ProtobufRegion) -> Self {
        Self {
            message,
            at: 0,
            last_field: 0,
            enforce_numeric_order: true,
        }
    }

    /// Frame owned regions in generated Prost slot order.
    ///
    /// This is the owned-region counterpart of
    /// [`CanonicalFieldCursor::in_prost_slot_order`]. Schema-aware drivers
    /// resolve each actual tag to its generated slot and enforce slot order;
    /// the framing cursor must therefore permit a oneof arm whose actual tag
    /// is followed by a numerically smaller ordinary-field tag.
    pub fn in_prost_slot_order(message: ProtobufRegion) -> Self {
        Self {
            message,
            at: 0,
            last_field: 0,
            enforce_numeric_order: false,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool { self.at == self.message.len() }

    pub fn next_field(&mut self) -> Result<Option<CanonicalRegionField>, CanonicalProtobufError> {
        let Some((number, wire_type, varint, encoded_range, payload_range, at, last_field)) =
            (|| {
                let mut borrowed = CanonicalFieldCursor {
                    message: self.message.as_bytes(),
                    at: self.at,
                    last_field: self.last_field,
                    enforce_numeric_order: self.enforce_numeric_order,
                };
                let field = borrowed.next_field()?;
                Ok::<_, CanonicalProtobufError>(field.map(|field| {
                    let varint = match field.value {
                        CanonicalFieldValue::Varint(value) => Some(value),
                        _ => None,
                    };
                    (
                        field.number,
                        field.wire_type,
                        varint,
                        field.encoded_range,
                        field.payload_range,
                        borrowed.at,
                        borrowed.last_field,
                    )
                }))
            })()?
        else {
            return Ok(None);
        };

        self.at = at;
        self.last_field = last_field;
        let payload = || {
            self.message
                .subregion(payload_range)
                .expect("a field payload parsed from the same protobuf region")
        };
        let value = match wire_type {
            CanonicalWireType::Varint => CanonicalRegionFieldValue::Varint(
                varint.expect("the borrowed parser supplies a varint value"),
            ),
            CanonicalWireType::Fixed64 => CanonicalRegionFieldValue::Fixed64(payload()),
            CanonicalWireType::LengthDelimited => {
                CanonicalRegionFieldValue::LengthDelimited(payload())
            }
            CanonicalWireType::Fixed32 => CanonicalRegionFieldValue::Fixed32(payload()),
        };
        Ok(Some(CanonicalRegionField {
            number,
            wire_type,
            value,
            encoded_range,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRegionDelimitedRun {
    encoded: ProtobufRegion,
    field: u32,
}

impl CanonicalRegionDelimitedRun {
    pub fn new(encoded: ProtobufRegion, field: u32) -> Self { Self { encoded, field } }

    pub fn cursor(&self) -> CanonicalRegionDelimitedCursor {
        CanonicalRegionDelimitedCursor {
            fields: CanonicalRegionFieldCursor::new(self.encoded.clone()),
            expected_field: self.field,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CanonicalRegionDelimitedCursor {
    fields: CanonicalRegionFieldCursor,
    expected_field: u32,
}

impl CanonicalRegionDelimitedCursor {
    #[inline]
    pub fn is_empty(&self) -> bool { self.fields.is_empty() }

    pub fn next_region(&mut self) -> Result<Option<ProtobufRegion>, CanonicalProtobufError> {
        let Some(field) = self.fields.next_field()? else {
            return Ok(None);
        };
        if field.number != self.expected_field {
            return Err(CanonicalProtobufError::UnexpectedField {
                offset: field.encoded_range.start,
                field: field.number,
            });
        }
        field.into_length_delimited().map(Some)
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
    pub cost_token_stacks: Option<CanonicalDelimitedRun<'a>>,
}

impl<'a> CanonicalParProcessIndex<'a> {
    pub fn parse(message: &'a [u8]) -> Result<Self, CanonicalProtobufError> {
        let mut index = Self::default();
        let mut fields = CanonicalFieldCursor::new(message);
        let mut saw_locally_free = false;
        let mut saw_connective_used = false;
        while let Some(field) = fields.next_field()? {
            let schema =
                par_protobuf_field(field.number, field.wire_type, field.encoded_range.start)?;
            match schema.name {
                "sends" => extend_run(message, &mut index.sends, field)?,
                "receives" => extend_run(message, &mut index.receives, field)?,
                "news" => extend_run(message, &mut index.news, field)?,
                "exprs" => extend_run(message, &mut index.exprs, field)?,
                "matches" => extend_run(message, &mut index.matches, field)?,
                "unforgeables" | "connectives" => {
                    field.expect_length_delimited()?;
                }
                "locally_free" if !saw_locally_free => {
                    let payload = field.expect_length_delimited()?;
                    if payload.is_empty() {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset: field.encoded_range.start,
                            field: field.number,
                        });
                    }
                    saw_locally_free = true;
                }
                "locally_free" => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: field.number,
                    });
                }
                "connective_used" if !saw_connective_used => {
                    if field.expect_varint()? != 1 {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset: field.encoded_range.start,
                            field: field.number,
                        });
                    }
                    saw_connective_used = true;
                }
                "connective_used" => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: field.number,
                    });
                }
                "bundles" => extend_run(message, &mut index.bundles, field)?,
                "conditionals" => extend_run(message, &mut index.conditionals, field)?,
                "cost_token_stacks" => extend_run(message, &mut index.cost_token_stacks, field)?,
                other => unreachable!("generated raw Par field `{other}` is not indexed"),
            }
        }
        Ok(index)
    }
}

fn extend_region_run(
    message: &ProtobufRegion,
    slot: &mut Option<CanonicalRegionDelimitedRun>,
    field: &CanonicalRegionField,
) -> Result<(), CanonicalProtobufError> {
    field.expect_length_delimited()?;
    let start = match slot {
        Some(run) => {
            let run_range = run.encoded.absolute_range();
            let message_range = message.absolute_range();
            debug_assert_eq!(
                run_range.end,
                message_range.start + field.encoded_range.start
            );
            run_range.start - message_range.start
        }
        None => field.encoded_range.start,
    };
    let encoded = message
        .subregion(CanonicalByteRange {
            start,
            end: field.encoded_range.end,
        })
        .expect("a repeated-field run parsed from the same protobuf region");
    *slot = Some(CanonicalRegionDelimitedRun::new(encoded, field.number));
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct CanonicalParRegionIndex {
    pub sends: Option<CanonicalRegionDelimitedRun>,
    pub receives: Option<CanonicalRegionDelimitedRun>,
    pub news: Option<CanonicalRegionDelimitedRun>,
    pub exprs: Option<CanonicalRegionDelimitedRun>,
    pub matches: Option<CanonicalRegionDelimitedRun>,
    pub unforgeables: Option<CanonicalRegionDelimitedRun>,
    pub connectives: Option<CanonicalRegionDelimitedRun>,
    pub bundles: Option<CanonicalRegionDelimitedRun>,
    pub conditionals: Option<CanonicalRegionDelimitedRun>,
    pub cost_token_stacks: Option<CanonicalRegionDelimitedRun>,
    pub locally_free: Option<ProtobufRegion>,
    pub connective_used: bool,
}

impl CanonicalParRegionIndex {
    pub fn parse(message: ProtobufRegion) -> Result<Self, CanonicalProtobufError> {
        let mut index = Self::default();
        let mut fields = CanonicalRegionFieldCursor::new(message.clone());
        while let Some(field) = fields.next_field()? {
            let schema =
                par_protobuf_field(field.number, field.wire_type, field.encoded_range.start)?;
            match schema.name {
                "sends" => extend_region_run(&message, &mut index.sends, &field)?,
                "receives" => extend_region_run(&message, &mut index.receives, &field)?,
                "news" => extend_region_run(&message, &mut index.news, &field)?,
                "exprs" => extend_region_run(&message, &mut index.exprs, &field)?,
                "matches" => extend_region_run(&message, &mut index.matches, &field)?,
                "unforgeables" => extend_region_run(&message, &mut index.unforgeables, &field)?,
                "connectives" => extend_region_run(&message, &mut index.connectives, &field)?,
                "locally_free" if index.locally_free.is_none() => {
                    let offset = field.encoded_range.start;
                    let number = field.number;
                    let payload = field.into_length_delimited()?;
                    if payload.is_empty() {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset,
                            field: number,
                        });
                    }
                    index.locally_free = Some(payload);
                }
                "locally_free" => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: field.number,
                    });
                }
                "connective_used" if !index.connective_used => {
                    if field.expect_varint()? != 1 {
                        return Err(CanonicalProtobufError::NonCanonicalDefault {
                            offset: field.encoded_range.start,
                            field: field.number,
                        });
                    }
                    index.connective_used = true;
                }
                "connective_used" => {
                    return Err(CanonicalProtobufError::DuplicateSingularField {
                        offset: field.encoded_range.start,
                        field: field.number,
                    });
                }
                "bundles" => extend_region_run(&message, &mut index.bundles, &field)?,
                "conditionals" => extend_region_run(&message, &mut index.conditionals, &field)?,
                "cost_token_stacks" => {
                    extend_region_run(&message, &mut index.cost_token_stacks, &field)?
                }
                other => unreachable!("generated raw Par field `{other}` is not indexed"),
            }
        }
        Ok(index)
    }
}
