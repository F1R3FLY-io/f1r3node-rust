//! Indexed, non-flattening byte views and canonical protobuf framing.
//!
//! A view is either a constant-size owned window into [`ProtobufRegion`] or a
//! constant-size borrowed window into [`CanonicalBytesArena`]. Persistent
//! windows retain the original handle plus a range, so cloning and forming a
//! subview never walks a parent chain. Cursors expand the persistent DAG once,
//! in forward order, and framing consumes every input byte at most once.

use std::cmp::Ordering;
use std::fmt;

use super::canonical_bytes_arena::{
    CanonicalByteCursor, CanonicalByteProvenance, CanonicalBytesArena, CanonicalBytesHandle,
    CanonicalLocatedByte, CanonicalLocatedByteCursor,
};
use super::protobuf_cursor::{
    CanonicalByteRange, CanonicalProtobufError, CanonicalWireType, ProtobufRegion,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexedBytePositionKind {
    Byte,
    End,
}

/// A logical byte position projected back into its ultimate physical source.
///
/// `End` positions are boundaries, not bytes. For an arena root, callers
/// provide the physical end boundary once; subview boundaries inside that root
/// inherit the provenance of the next physical byte without storing a copied
/// payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedBytePosition {
    pub(crate) logical_offset: usize,
    pub(crate) physical: CanonicalByteProvenance,
    pub(crate) kind: IndexedBytePositionKind,
}

#[derive(Clone)]
enum IndexedByteBacking<'a> {
    Contiguous {
        region: ProtobufRegion,
        source: u32,
    },
    Arena {
        arena: &'a CanonicalBytesArena,
        root: CanonicalBytesHandle,
        start: usize,
        len: usize,
        root_end: CanonicalByteProvenance,
    },
}

/// A cloneable, constant-size window over contiguous or persistent bytes.
///
/// This type deliberately has no `AsRef<[u8]>` or flattening operation.
#[derive(Clone)]
pub(crate) struct IndexedByteRegion<'a> {
    backing: IndexedByteBacking<'a>,
}

impl fmt::Debug for IndexedByteRegion<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backing = match self.backing {
            IndexedByteBacking::Contiguous { .. } => "contiguous",
            IndexedByteBacking::Arena { .. } => "arena",
        };
        formatter
            .debug_struct("IndexedByteRegion")
            .field("backing", &backing)
            .field("len", &self.len())
            .finish()
    }
}

impl<'a> IndexedByteRegion<'a> {
    pub(crate) fn from_protobuf_region(region: ProtobufRegion, source: u32) -> Self {
        Self {
            backing: IndexedByteBacking::Contiguous { region, source },
        }
    }

    /// Construct a persistent view. `root_end` is the exact physical boundary
    /// immediately after `root`; it is required even for an empty root because
    /// no logical byte exists from which that boundary could be inferred.
    pub(crate) fn from_arena(
        arena: &'a CanonicalBytesArena,
        root: CanonicalBytesHandle,
        root_end: CanonicalByteProvenance,
    ) -> Self {
        Self {
            backing: IndexedByteBacking::Arena {
                arena,
                root,
                start: 0,
                len: root.len(),
                root_end,
            },
        }
    }

    pub(crate) fn len(&self) -> usize {
        match &self.backing {
            IndexedByteBacking::Contiguous { region, .. } => region.len(),
            IndexedByteBacking::Arena { len, .. } => *len,
        }
    }

    pub(crate) fn is_empty(&self) -> bool { self.len() == 0 }

    /// Form a constant-size subview without traversing or copying its bytes.
    pub(crate) fn subview(&self, range: CanonicalByteRange) -> Option<Self> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        match &self.backing {
            IndexedByteBacking::Contiguous { region, source } => Some(Self {
                backing: IndexedByteBacking::Contiguous {
                    region: region.subregion(range)?,
                    source: *source,
                },
            }),
            IndexedByteBacking::Arena {
                arena,
                root,
                start,
                root_end,
                ..
            } => Some(Self {
                backing: IndexedByteBacking::Arena {
                    arena,
                    root: *root,
                    start: start.checked_add(range.start)?,
                    len: range.end - range.start,
                    root_end: *root_end,
                },
            }),
        }
    }

    pub(crate) fn position_at(&self, logical_offset: usize) -> Option<IndexedBytePosition> {
        let len = self.len();
        if logical_offset > len {
            return None;
        }
        let physical = match &self.backing {
            IndexedByteBacking::Contiguous { region, source } => {
                let absolute = region.absolute_range().start.checked_add(logical_offset)?;
                CanonicalByteProvenance::new(*source, absolute)
            }
            IndexedByteBacking::Arena {
                arena,
                root,
                start,
                root_end,
                ..
            } => {
                let root_offset = start.checked_add(logical_offset)?;
                if root_offset < root.len() {
                    arena.provenance_at(*root, root_offset)?
                } else if root_offset == root.len() {
                    *root_end
                } else {
                    return None;
                }
            }
        };
        Some(IndexedBytePosition {
            logical_offset,
            physical,
            kind: if logical_offset == len {
                IndexedBytePositionKind::End
            } else {
                IndexedBytePositionKind::Byte
            },
        })
    }

    pub(crate) fn end_position(&self) -> IndexedBytePosition {
        self.position_at(self.len())
            .expect("an indexed byte region always owns its end boundary")
    }

    pub(crate) fn bytes(&self) -> IndexedByteCursor<'a> {
        let inner = match &self.backing {
            IndexedByteBacking::Contiguous { region, .. } => IndexedByteCursorInner::Contiguous {
                region: region.clone(),
                at: 0,
            },
            IndexedByteBacking::Arena {
                arena,
                root,
                start,
                len,
                ..
            } => {
                let end = start
                    .checked_add(*len)
                    .expect("a validated arena view range cannot overflow");
                let handle = arena
                    .slice(*root, *start..end)
                    .expect("an indexed arena view stays inside its root");
                IndexedByteCursorInner::Arena(arena.bytes(handle))
            }
        };
        IndexedByteCursor { inner }
    }

    pub(crate) fn located_bytes(&self) -> IndexedLocatedByteCursor<'a> {
        let inner = match &self.backing {
            IndexedByteBacking::Contiguous { region, source } => {
                IndexedLocatedByteCursorInner::Contiguous {
                    region: region.clone(),
                    source: *source,
                    absolute_start: region.absolute_range().start,
                    at: 0,
                }
            }
            IndexedByteBacking::Arena {
                arena,
                root,
                start,
                len,
                ..
            } => {
                let end = start
                    .checked_add(*len)
                    .expect("a validated arena view range cannot overflow");
                let handle = arena
                    .slice(*root, *start..end)
                    .expect("an indexed arena view stays inside its root");
                IndexedLocatedByteCursorInner::Arena(arena.located_bytes(handle))
            }
        };
        IndexedLocatedByteCursor { inner, yielded: 0 }
    }

    pub(crate) fn reader(&self) -> IndexedByteReader<'a> { IndexedByteReader::new(self.clone()) }

    pub(crate) fn content_eq(&self, other: &IndexedByteRegion<'_>) -> bool {
        self.len() == other.len() && self.bytes().eq(other.bytes())
    }

    pub(crate) fn compare(&self, other: &IndexedByteRegion<'_>) -> Ordering {
        self.bytes().cmp(other.bytes())
    }

    pub(crate) fn validate_utf8(&self) -> Result<(), IndexedByteReadError> {
        let mut reader = self.reader();
        reader.read_utf8(self.len()).map(|_| ())
    }
}

enum IndexedByteCursorInner<'a> {
    Contiguous { region: ProtobufRegion, at: usize },
    Arena(CanonicalByteCursor<'a>),
}

/// A single forward traversal. Persistent parents are expanded by the arena's
/// heap worklist rather than revisited through logical byte indexing.
pub(crate) struct IndexedByteCursor<'a> {
    inner: IndexedByteCursorInner<'a>,
}

impl Iterator for IndexedByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = match &mut self.inner {
            IndexedByteCursorInner::Contiguous { region, at } => {
                let byte = region.as_bytes().get(*at).copied();
                *at += usize::from(byte.is_some());
                byte
            }
            IndexedByteCursorInner::Arena(cursor) => cursor.next(),
        };
        byte
    }
}

enum IndexedLocatedByteCursorInner<'a> {
    Contiguous {
        region: ProtobufRegion,
        source: u32,
        absolute_start: usize,
        at: usize,
    },
    Arena(CanonicalLocatedByteCursor<'a>),
}

pub(crate) struct IndexedLocatedByteCursor<'a> {
    inner: IndexedLocatedByteCursorInner<'a>,
    yielded: usize,
}

impl IndexedLocatedByteCursor<'_> {
    pub(crate) const fn bytes_yielded(&self) -> usize { self.yielded }
}

impl Iterator for IndexedLocatedByteCursor<'_> {
    type Item = CanonicalLocatedByte;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = match &mut self.inner {
            IndexedLocatedByteCursorInner::Contiguous {
                region,
                source,
                absolute_start,
                at,
            } => {
                let logical = *at;
                let byte = region.as_bytes().get(logical).copied();
                *at += usize::from(byte.is_some());
                byte.map(|byte| {
                    CanonicalLocatedByte::new(
                        byte,
                        CanonicalByteProvenance::new(*source, *absolute_start + logical),
                    )
                })
            }
            IndexedLocatedByteCursorInner::Arena(cursor) => cursor.next(),
        };
        self.yielded += usize::from(byte.is_some());
        byte
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndexedByteReadOperation {
    Byte,
    Fixed32,
    Fixed64,
    Utf8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IndexedByteReadErrorKind {
    UnexpectedEof {
        operation: IndexedByteReadOperation,
        required_end: usize,
        available_end: usize,
    },
    TruncatedVarint,
    NonMinimalVarint,
    VarintOverflow,
    InvalidUtf8,
    TruncatedUtf8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexedByteReadError {
    pub(crate) kind: IndexedByteReadErrorKind,
    pub(crate) start: IndexedBytePosition,
    pub(crate) failure: IndexedBytePosition,
}

impl fmt::Display for IndexedByteReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "indexed byte read {:?} from logical byte {} failed at logical byte {}",
            self.kind, self.start.logical_offset, self.failure.logical_offset
        )
    }
}

impl std::error::Error for IndexedByteReadError {}

pub(crate) struct IndexedByteReader<'a> {
    view: IndexedByteRegion<'a>,
    cursor: IndexedLocatedByteCursor<'a>,
    at: usize,
}

struct IndexedCanonicalVarint {
    value: u64,
    start: IndexedBytePosition,
    last: IndexedBytePosition,
}

impl<'a> IndexedByteReader<'a> {
    fn new(view: IndexedByteRegion<'a>) -> Self {
        let cursor = view.located_bytes();
        Self {
            view,
            cursor,
            at: 0,
        }
    }

    pub(crate) const fn offset(&self) -> usize { self.at }

    pub(crate) const fn bytes_read(&self) -> usize { self.cursor.bytes_yielded() }

    pub(crate) fn is_empty(&self) -> bool { self.at == self.view.len() }

    fn position(&self, offset: usize) -> IndexedBytePosition {
        self.view
            .position_at(offset)
            .expect("reader offsets stay within their indexed view")
    }

    fn eof_error(
        &self,
        start: usize,
        operation: IndexedByteReadOperation,
        required_end: usize,
    ) -> IndexedByteReadError {
        IndexedByteReadError {
            kind: IndexedByteReadErrorKind::UnexpectedEof {
                operation,
                required_end,
                available_end: self.view.len(),
            },
            start: self.position(start),
            failure: self.view.end_position(),
        }
    }

    fn byte_position(offset: usize, byte: CanonicalLocatedByte) -> IndexedBytePosition {
        IndexedBytePosition {
            logical_offset: offset,
            physical: byte.provenance,
            kind: IndexedBytePositionKind::Byte,
        }
    }

    fn next_raw(&mut self) -> Option<(usize, CanonicalLocatedByte)> {
        let offset = self.at;
        let byte = self.cursor.next()?;
        self.at += 1;
        Some((offset, byte))
    }

    pub(crate) fn read_byte(&mut self) -> Result<u8, IndexedByteReadError> {
        let start = self.at;
        self.next_raw().map(|(_, byte)| byte.byte).ok_or_else(|| {
            self.eof_error(
                start,
                IndexedByteReadOperation::Byte,
                start.saturating_add(1),
            )
        })
    }

    fn require(
        &self,
        width: usize,
        operation: IndexedByteReadOperation,
    ) -> Result<usize, IndexedByteReadError> {
        let start = self.at;
        let Some(end) = start.checked_add(width) else {
            return Err(self.eof_error(start, operation, usize::MAX));
        };
        if end > self.view.len() {
            return Err(self.eof_error(start, operation, end));
        }
        Ok(end)
    }

    pub(crate) fn skip_exact(
        &mut self,
        width: usize,
        operation: IndexedByteReadOperation,
    ) -> Result<(), IndexedByteReadError> {
        let end = self.require(width, operation)?;
        while self.at < end {
            self.next_raw()
                .expect("a range checked against the view has every requested byte");
        }
        Ok(())
    }

    pub(crate) fn read_fixed32(&mut self) -> Result<u32, IndexedByteReadError> {
        self.require(4, IndexedByteReadOperation::Fixed32)?;
        let mut bytes = [0u8; 4];
        for byte in &mut bytes {
            *byte = self
                .next_raw()
                .expect("a checked fixed32 has four bytes")
                .1
                .byte;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    pub(crate) fn read_fixed64(&mut self) -> Result<u64, IndexedByteReadError> {
        self.require(8, IndexedByteReadOperation::Fixed64)?;
        let mut bytes = [0u8; 8];
        for byte in &mut bytes {
            *byte = self
                .next_raw()
                .expect("a checked fixed64 has eight bytes")
                .1
                .byte;
        }
        Ok(u64::from_le_bytes(bytes))
    }

    pub(crate) fn read_canonical_varint(&mut self) -> Result<u64, IndexedByteReadError> {
        self.read_canonical_varint_located()
            .map(|varint| varint.value)
    }

    fn read_canonical_varint_located(
        &mut self,
    ) -> Result<IndexedCanonicalVarint, IndexedByteReadError> {
        let mut start = None;
        let mut last = None;
        let mut value = 0u64;
        for shift_group in 0..10usize {
            let Some((offset, byte)) = self.next_raw() else {
                return Err(IndexedByteReadError {
                    kind: IndexedByteReadErrorKind::TruncatedVarint,
                    start: start.unwrap_or_else(|| self.view.end_position()),
                    failure: self.view.end_position(),
                });
            };
            let position = Self::byte_position(offset, byte);
            let start = *start.get_or_insert(position);
            last = Some(position);
            if shift_group == 9 && byte.byte > 1 {
                return Err(IndexedByteReadError {
                    kind: IndexedByteReadErrorKind::VarintOverflow,
                    start,
                    failure: position,
                });
            }
            value |= u64::from(byte.byte & 0x7f) << (shift_group * 7);
            if byte.byte & 0x80 == 0 {
                if shift_group != 0 && byte.byte == 0 {
                    return Err(IndexedByteReadError {
                        kind: IndexedByteReadErrorKind::NonMinimalVarint,
                        start,
                        failure: position,
                    });
                }
                return Ok(IndexedCanonicalVarint {
                    value,
                    start,
                    last: position,
                });
            }
        }
        Err(IndexedByteReadError {
            kind: IndexedByteReadErrorKind::VarintOverflow,
            start: start.expect("an overflowing varint consumed ten bytes"),
            failure: last.expect("an overflowing varint consumed ten bytes"),
        })
    }

    fn utf8_error(
        kind: IndexedByteReadErrorKind,
        sequence_start: IndexedBytePosition,
        failure: IndexedBytePosition,
    ) -> IndexedByteReadError {
        IndexedByteReadError {
            kind,
            start: sequence_start,
            failure,
        }
    }

    fn read_utf8_range_byte(
        &mut self,
        end: usize,
        sequence_start: IndexedBytePosition,
    ) -> Result<(u8, IndexedBytePosition), IndexedByteReadError> {
        if self.at == end {
            let failure = self
                .view
                .position_at(end)
                .expect("a checked UTF-8 end belongs to its view");
            return Err(Self::utf8_error(
                IndexedByteReadErrorKind::TruncatedUtf8,
                sequence_start,
                failure,
            ));
        }
        let (offset, byte) = self
            .next_raw()
            .expect("a UTF-8 byte below the checked range end exists");
        Ok((byte.byte, Self::byte_position(offset, byte)))
    }

    fn require_utf8_byte(
        &mut self,
        end: usize,
        sequence_start: IndexedBytePosition,
        minimum: u8,
        maximum: u8,
    ) -> Result<(), IndexedByteReadError> {
        let (byte, failure) = self.read_utf8_range_byte(end, sequence_start)?;
        if !(minimum..=maximum).contains(&byte) {
            return Err(Self::utf8_error(
                IndexedByteReadErrorKind::InvalidUtf8,
                sequence_start,
                failure,
            ));
        }
        Ok(())
    }

    /// Validate and return a non-copying view of exactly `length` UTF-8 bytes.
    pub(crate) fn read_utf8(
        &mut self,
        length: usize,
    ) -> Result<IndexedByteRegion<'a>, IndexedByteReadError> {
        let start = self.at;
        let end = self.require(length, IndexedByteReadOperation::Utf8)?;
        while self.at < end {
            let (sequence_offset, first) = self
                .next_raw()
                .expect("a UTF-8 sequence start below the checked end exists");
            let sequence_start = Self::byte_position(sequence_offset, first);
            match first.byte {
                0x00..=0x7f => {}
                0xc2..=0xdf => {
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xe0 => {
                    self.require_utf8_byte(end, sequence_start, 0xa0, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xe1..=0xec | 0xee..=0xef => {
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xed => {
                    self.require_utf8_byte(end, sequence_start, 0x80, 0x9f)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xf0 => {
                    self.require_utf8_byte(end, sequence_start, 0x90, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xf1..=0xf3 => {
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                0xf4 => {
                    self.require_utf8_byte(end, sequence_start, 0x80, 0x8f)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                    self.require_utf8_byte(end, sequence_start, 0x80, 0xbf)?;
                }
                _ => {
                    return Err(Self::utf8_error(
                        IndexedByteReadErrorKind::InvalidUtf8,
                        sequence_start,
                        sequence_start,
                    ));
                }
            }
        }
        Ok(self
            .view
            .subview(CanonicalByteRange { start, end })
            .expect("a validated UTF-8 range lies inside its view"))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum IndexedCanonicalFieldValue<'a> {
    Varint(u64),
    Fixed64(IndexedByteRegion<'a>),
    LengthDelimited(IndexedByteRegion<'a>),
    Fixed32(IndexedByteRegion<'a>),
}

#[derive(Clone, Debug)]
pub(crate) struct IndexedCanonicalField<'a> {
    pub(crate) number: u32,
    pub(crate) wire_type: CanonicalWireType,
    pub(crate) value: IndexedCanonicalFieldValue<'a>,
    pub(crate) encoded_range: CanonicalByteRange,
    pub(crate) payload_range: CanonicalByteRange,
    pub(crate) field_start: IndexedBytePosition,
}

impl<'a> IndexedCanonicalField<'a> {
    pub(crate) fn expect_length_delimited(
        &self,
    ) -> Result<&IndexedByteRegion<'a>, LocatedCanonicalProtobufError> {
        match &self.value {
            IndexedCanonicalFieldValue::LengthDelimited(payload) => Ok(payload),
            _ => Err(LocatedCanonicalProtobufError {
                error: CanonicalProtobufError::UnexpectedWireType {
                    offset: self.encoded_range.start,
                    field: self.number,
                    expected: CanonicalWireType::LengthDelimited,
                    actual: self.wire_type,
                },
                reported: self.field_start,
                failure: self.field_start,
            }),
        }
    }

    pub(crate) fn expect_varint(&self) -> Result<u64, LocatedCanonicalProtobufError> {
        match self.value {
            IndexedCanonicalFieldValue::Varint(value) => Ok(value),
            _ => Err(LocatedCanonicalProtobufError {
                error: CanonicalProtobufError::UnexpectedWireType {
                    offset: self.encoded_range.start,
                    field: self.number,
                    expected: CanonicalWireType::Varint,
                    actual: self.wire_type,
                },
                reported: self.field_start,
                failure: self.field_start,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocatedCanonicalProtobufError {
    pub(crate) error: CanonicalProtobufError,
    pub(crate) reported: IndexedBytePosition,
    pub(crate) failure: IndexedBytePosition,
}

impl fmt::Display for LocatedCanonicalProtobufError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { self.error.fmt(formatter) }
}

impl std::error::Error for LocatedCanonicalProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.error) }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndexedCanonicalFieldCursorStats {
    pub(crate) fields_read: usize,
    pub(crate) bytes_read: usize,
}

/// Schema-neutral canonical protobuf framing over either indexed backing.
///
/// After an error the cursor is terminal and returns the same cached error.
/// This preserves exact diagnostics without replaying a persistent prefix.
pub(crate) struct IndexedCanonicalFieldCursor<'a> {
    message: IndexedByteRegion<'a>,
    reader: IndexedByteReader<'a>,
    last_field: u32,
    enforce_numeric_order: bool,
    fields_read: usize,
    terminal_error: Option<LocatedCanonicalProtobufError>,
}

impl<'a> IndexedCanonicalFieldCursor<'a> {
    pub(crate) fn new(message: IndexedByteRegion<'a>) -> Self { Self::with_order(message, true) }

    pub(crate) fn in_prost_slot_order(message: IndexedByteRegion<'a>) -> Self {
        Self::with_order(message, false)
    }

    fn with_order(message: IndexedByteRegion<'a>, enforce_numeric_order: bool) -> Self {
        let reader = message.reader();
        Self {
            message,
            reader,
            last_field: 0,
            enforce_numeric_order,
            fields_read: 0,
            terminal_error: None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool { self.reader.is_empty() }

    pub(crate) fn stats(&self) -> IndexedCanonicalFieldCursorStats {
        IndexedCanonicalFieldCursorStats {
            fields_read: self.fields_read,
            bytes_read: self.reader.bytes_read(),
        }
    }

    fn direct_error(
        error: CanonicalProtobufError,
        reported: IndexedBytePosition,
        failure: IndexedBytePosition,
    ) -> LocatedCanonicalProtobufError {
        LocatedCanonicalProtobufError {
            error,
            reported,
            failure,
        }
    }

    fn varint_error(&self, error: IndexedByteReadError) -> LocatedCanonicalProtobufError {
        let offset = error.start.logical_offset;
        let source = match error.kind {
            IndexedByteReadErrorKind::TruncatedVarint => {
                CanonicalProtobufError::TruncatedVarint { offset }
            }
            IndexedByteReadErrorKind::NonMinimalVarint => {
                CanonicalProtobufError::NonMinimalVarint { offset }
            }
            IndexedByteReadErrorKind::VarintOverflow => {
                CanonicalProtobufError::VarintOverflow { offset }
            }
            _ => unreachable!("canonical protobuf varint reads return only varint errors"),
        };
        LocatedCanonicalProtobufError {
            error: source,
            reported: error.start,
            failure: error.failure,
        }
    }

    pub(crate) fn next_field(
        &mut self,
    ) -> Result<Option<IndexedCanonicalField<'a>>, LocatedCanonicalProtobufError> {
        if let Some(error) = &self.terminal_error {
            return Err(error.clone());
        }
        if self.is_empty() {
            return Ok(None);
        }
        match self.next_field_inner() {
            Ok(field) => Ok(Some(field)),
            Err(error) => {
                self.terminal_error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn next_field_inner(
        &mut self,
    ) -> Result<IndexedCanonicalField<'a>, LocatedCanonicalProtobufError> {
        let key = self
            .reader
            .read_canonical_varint_located()
            .map_err(|error| self.varint_error(error))?;
        let field_start = key.start;
        let field_offset = field_start.logical_offset;
        let field_u64 = key.value >> 3;
        if field_u64 == 0 || field_u64 > 0x1fff_ffff {
            return Err(Self::direct_error(
                CanonicalProtobufError::InvalidFieldNumber {
                    offset: field_offset,
                    field: field_u64,
                },
                field_start,
                field_start,
            ));
        }
        let number = field_u64 as u32;
        if self.enforce_numeric_order && number < self.last_field {
            return Err(Self::direct_error(
                CanonicalProtobufError::DecreasingFieldOrder {
                    offset: field_offset,
                    previous: self.last_field,
                    current: number,
                },
                field_start,
                field_start,
            ));
        }
        self.last_field = number;

        let wire = (key.value & 0x07) as u8;
        let wire_type = match wire {
            0 => CanonicalWireType::Varint,
            1 => CanonicalWireType::Fixed64,
            2 => CanonicalWireType::LengthDelimited,
            5 => CanonicalWireType::Fixed32,
            _ => {
                return Err(Self::direct_error(
                    CanonicalProtobufError::UnsupportedWireType {
                        offset: field_offset,
                        wire,
                    },
                    field_start,
                    field_start,
                ));
            }
        };

        let payload_start = self.reader.offset();
        let (value, payload_range) = match wire_type {
            CanonicalWireType::Varint => {
                let value = self
                    .reader
                    .read_canonical_varint()
                    .map_err(|error| self.varint_error(error))?;
                let payload_range = CanonicalByteRange {
                    start: payload_start,
                    end: self.reader.offset(),
                };
                (IndexedCanonicalFieldValue::Varint(value), payload_range)
            }
            CanonicalWireType::Fixed64 => {
                let end = payload_start.checked_add(8).ok_or_else(|| {
                    Self::direct_error(
                        CanonicalProtobufError::TruncatedField {
                            offset: field_offset,
                            required_end: usize::MAX,
                            message_len: self.message.len(),
                        },
                        field_start,
                        self.message.end_position(),
                    )
                })?;
                if end > self.message.len() {
                    return Err(Self::direct_error(
                        CanonicalProtobufError::TruncatedField {
                            offset: field_offset,
                            required_end: end,
                            message_len: self.message.len(),
                        },
                        field_start,
                        self.message.end_position(),
                    ));
                }
                self.reader
                    .skip_exact(8, IndexedByteReadOperation::Fixed64)
                    .expect("a protobuf fixed64 range was checked against the message");
                let payload_range = CanonicalByteRange {
                    start: payload_start,
                    end,
                };
                let payload = self
                    .message
                    .subview(payload_range)
                    .expect("a checked protobuf fixed64 payload lies inside the message");
                (IndexedCanonicalFieldValue::Fixed64(payload), payload_range)
            }
            CanonicalWireType::LengthDelimited => {
                let length_offset = payload_start;
                let length_varint = self
                    .reader
                    .read_canonical_varint_located()
                    .map_err(|error| self.varint_error(error))?;
                let length_u64 = length_varint.value;
                let length_start = length_varint.start;
                let length_last = length_varint.last;
                let length = usize::try_from(length_u64).map_err(|_| {
                    Self::direct_error(
                        CanonicalProtobufError::LengthOverflow {
                            offset: length_offset,
                            length: length_u64,
                        },
                        length_start,
                        length_last,
                    )
                })?;
                let body_start = self.reader.offset();
                let end = body_start.checked_add(length).ok_or_else(|| {
                    Self::direct_error(
                        CanonicalProtobufError::LengthOverflow {
                            offset: length_offset,
                            length: length_u64,
                        },
                        length_start,
                        length_last,
                    )
                })?;
                if end > self.message.len() {
                    return Err(Self::direct_error(
                        CanonicalProtobufError::TruncatedField {
                            offset: field_offset,
                            required_end: end,
                            message_len: self.message.len(),
                        },
                        field_start,
                        self.message.end_position(),
                    ));
                }
                self.reader
                    .skip_exact(length, IndexedByteReadOperation::Byte)
                    .expect("a protobuf length-delimited range was checked against the message");
                let payload_range = CanonicalByteRange {
                    start: body_start,
                    end,
                };
                let payload = self
                    .message
                    .subview(payload_range)
                    .expect("a checked protobuf length-delimited payload lies inside the message");
                (
                    IndexedCanonicalFieldValue::LengthDelimited(payload),
                    payload_range,
                )
            }
            CanonicalWireType::Fixed32 => {
                let end = payload_start.checked_add(4).ok_or_else(|| {
                    Self::direct_error(
                        CanonicalProtobufError::TruncatedField {
                            offset: field_offset,
                            required_end: usize::MAX,
                            message_len: self.message.len(),
                        },
                        field_start,
                        self.message.end_position(),
                    )
                })?;
                if end > self.message.len() {
                    return Err(Self::direct_error(
                        CanonicalProtobufError::TruncatedField {
                            offset: field_offset,
                            required_end: end,
                            message_len: self.message.len(),
                        },
                        field_start,
                        self.message.end_position(),
                    ));
                }
                self.reader
                    .skip_exact(4, IndexedByteReadOperation::Fixed32)
                    .expect("a protobuf fixed32 range was checked against the message");
                let payload_range = CanonicalByteRange {
                    start: payload_start,
                    end,
                };
                let payload = self
                    .message
                    .subview(payload_range)
                    .expect("a checked protobuf fixed32 payload lies inside the message");
                (IndexedCanonicalFieldValue::Fixed32(payload), payload_range)
            }
        };

        self.fields_read += 1;
        Ok(IndexedCanonicalField {
            number,
            wire_type,
            value,
            encoded_range: CanonicalByteRange {
                start: field_offset,
                end: self.reader.offset(),
            },
            payload_range,
            field_start,
        })
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::super::protobuf_cursor::{CanonicalRegionFieldCursor, CanonicalRegionFieldValue};
    use super::*;

    const SOURCE: u32 = 71;
    const BASE: usize = 10_000;

    fn located(byte: u8, offset: usize) -> CanonicalLocatedByte {
        CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(SOURCE, offset))
    }

    fn append_bytes(
        arena: &mut CanonicalBytesArena,
        bytes: &[u8],
        base: usize,
    ) -> CanonicalBytesHandle {
        let mut value = arena.empty();
        for (index, byte) in bytes.iter().copied().enumerate() {
            value = arena.append(value, located(byte, base + index)).unwrap();
        }
        value
    }

    fn persistent_view<'a>(
        arena: &'a CanonicalBytesArena,
        handle: CanonicalBytesHandle,
        end: usize,
    ) -> IndexedByteRegion<'a> {
        IndexedByteRegion::from_arena(arena, handle, CanonicalByteProvenance::new(SOURCE, end))
    }

    fn bytes_of(view: &IndexedByteRegion<'_>) -> Vec<u8> { view.bytes().collect() }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum FieldValueSummary {
        Varint(u64),
        Fixed64(Vec<u8>),
        LengthDelimited(Vec<u8>),
        Fixed32(Vec<u8>),
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FieldSummary {
        number: u32,
        wire_type: CanonicalWireType,
        value: FieldValueSummary,
        encoded_range: CanonicalByteRange,
        payload_range: CanonicalByteRange,
    }

    fn contiguous_summary(
        bytes: &[u8],
        slot_order: bool,
    ) -> Result<Vec<FieldSummary>, CanonicalProtobufError> {
        let region = ProtobufRegion::copy_from_slice(bytes);
        let mut cursor = if slot_order {
            CanonicalRegionFieldCursor::in_prost_slot_order(region)
        } else {
            CanonicalRegionFieldCursor::new(region)
        };
        let mut fields = Vec::new();
        while let Some(field) = cursor.next_field()? {
            let value = match field.value {
                CanonicalRegionFieldValue::Varint(value) => FieldValueSummary::Varint(value),
                CanonicalRegionFieldValue::Fixed64(value) => {
                    FieldValueSummary::Fixed64(value.as_bytes().to_vec())
                }
                CanonicalRegionFieldValue::LengthDelimited(value) => {
                    FieldValueSummary::LengthDelimited(value.as_bytes().to_vec())
                }
                CanonicalRegionFieldValue::Fixed32(value) => {
                    FieldValueSummary::Fixed32(value.as_bytes().to_vec())
                }
            };
            fields.push(FieldSummary {
                number: field.number,
                wire_type: field.wire_type,
                value,
                encoded_range: field.encoded_range,
                payload_range: field.payload_range,
            });
        }
        Ok(fields)
    }

    fn indexed_summary(
        view: IndexedByteRegion<'_>,
        slot_order: bool,
    ) -> Result<(Vec<FieldSummary>, IndexedCanonicalFieldCursorStats), LocatedCanonicalProtobufError>
    {
        let mut cursor = if slot_order {
            IndexedCanonicalFieldCursor::in_prost_slot_order(view)
        } else {
            IndexedCanonicalFieldCursor::new(view)
        };
        let mut fields = Vec::new();
        while let Some(field) = cursor.next_field()? {
            let value = match field.value {
                IndexedCanonicalFieldValue::Varint(value) => FieldValueSummary::Varint(value),
                IndexedCanonicalFieldValue::Fixed64(value) => {
                    FieldValueSummary::Fixed64(bytes_of(&value))
                }
                IndexedCanonicalFieldValue::LengthDelimited(value) => {
                    FieldValueSummary::LengthDelimited(bytes_of(&value))
                }
                IndexedCanonicalFieldValue::Fixed32(value) => {
                    FieldValueSummary::Fixed32(bytes_of(&value))
                }
            };
            fields.push(FieldSummary {
                number: field.number,
                wire_type: field.wire_type,
                value,
                encoded_range: field.encoded_range,
                payload_range: field.payload_range,
            });
        }
        Ok((fields, cursor.stats()))
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                encoded.push(byte);
                return encoded;
            }
            encoded.push(byte | 0x80);
        }
    }

    fn protobuf_error_offset(error: &CanonicalProtobufError) -> usize {
        match error {
            CanonicalProtobufError::TruncatedVarint { offset }
            | CanonicalProtobufError::NonMinimalVarint { offset }
            | CanonicalProtobufError::VarintOverflow { offset }
            | CanonicalProtobufError::InvalidFieldNumber { offset, .. }
            | CanonicalProtobufError::UnsupportedWireType { offset, .. }
            | CanonicalProtobufError::DecreasingFieldOrder { offset, .. }
            | CanonicalProtobufError::LengthOverflow { offset, .. }
            | CanonicalProtobufError::TruncatedField { offset, .. }
            | CanonicalProtobufError::UnexpectedField { offset, .. }
            | CanonicalProtobufError::UnexpectedWireType { offset, .. }
            | CanonicalProtobufError::NonCanonicalDefault { offset, .. }
            | CanonicalProtobufError::DuplicateSingularField { offset, .. } => *offset,
        }
    }

    fn framing_cases() -> Vec<Vec<u8>> {
        let mut legal = vec![
            Vec::new(),
            vec![0x08, 0x00],
            vec![0x08, 0x7f],
            vec![0x08, 0x80, 0x01],
            vec![0x09, 1, 2, 3, 4, 5, 6, 7, 8],
            vec![0x0d, 1, 2, 3, 4],
            vec![0x12, 0],
            vec![0x12, 3, b'a', b'b', b'c'],
            vec![0x08, 1, 0x08, 2, 0x12, 1, 9],
        ];
        let mut maximum = vec![0x08];
        maximum.extend(encode_varint(u64::MAX));
        legal.push(maximum);
        let mut maximum_field = encode_varint(u64::from(0x1fff_ffffu32) << 3);
        maximum_field.push(1);
        legal.push(maximum_field);
        let mut long_payload = vec![0x12];
        long_payload.extend(encode_varint(128));
        long_payload.extend(0..=127);
        legal.push(long_payload);

        let mut cases = legal;
        cases.extend([
            vec![0x80],
            vec![0x88, 0x00, 1],
            vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
            vec![0x00],
            encode_varint((u64::from(0x1fff_ffffu32) + 1) << 3),
            vec![0x0b],
            vec![0x0c],
            vec![0x0e],
            vec![0x0f],
            vec![0x08],
            vec![0x08, 0x80],
            vec![0x08, 0x80, 0x00],
            vec![
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
            ],
            vec![0x12],
            vec![0x12, 0x80],
            vec![0x12, 0x80, 0x00],
            vec![0x12, 4, 1, 2, 3],
            {
                let mut bytes = vec![0x12];
                bytes.extend(encode_varint(u64::MAX));
                bytes
            },
            vec![0x10, 1, 0x08, 2],
        ]);
        for present in 0..8usize {
            let mut truncated = vec![0x09];
            truncated.extend((0..present).map(|byte| byte as u8));
            cases.push(truncated);
        }
        for present in 0..4usize {
            let mut truncated = vec![0x0d];
            truncated.extend((0..present).map(|byte| byte as u8));
            cases.push(truncated);
        }
        for present in 0..5usize {
            let mut truncated = vec![0x12, 5];
            truncated.extend((0..present).map(|byte| byte as u8));
            cases.push(truncated);
        }
        cases
    }

    #[test]
    fn every_framing_case_matches_the_existing_region_cursor_on_both_backings() {
        for slot_order in [false, true] {
            for bytes in framing_cases() {
                let expected = contiguous_summary(&bytes, slot_order);

                let region = ProtobufRegion::new(Bytes::from(bytes.clone()));
                let indexed_contiguous = IndexedByteRegion::from_protobuf_region(region, SOURCE);
                let actual_contiguous = indexed_summary(indexed_contiguous, slot_order)
                    .map(|(fields, _)| fields)
                    .map_err(|error| error.error);
                assert_eq!(
                    actual_contiguous, expected,
                    "contiguous bytes: {bytes:02x?}"
                );

                let mut arena = CanonicalBytesArena::new();
                let mut framed = vec![0xaa; 7];
                framed.extend_from_slice(&bytes);
                framed.extend_from_slice(&[0xbb; 5]);
                let all = append_bytes(&mut arena, &framed, BASE);
                let message = arena.slice(all, 7..7 + bytes.len()).unwrap();
                let indexed_arena = persistent_view(&arena, message, BASE + 7 + bytes.len());
                let actual_arena =
                    indexed_summary(indexed_arena.clone(), slot_order).map(|(fields, _)| fields);
                match (&expected, actual_arena) {
                    (Ok(expected), Ok(actual)) => {
                        assert_eq!(&actual, expected, "arena bytes: {bytes:02x?}")
                    }
                    (Err(expected), Err(actual)) => {
                        assert_eq!(&actual.error, expected, "arena bytes: {bytes:02x?}");
                        let offset = protobuf_error_offset(expected);
                        assert_eq!(actual.reported.logical_offset, offset);
                        assert_eq!(actual.reported.physical.source, SOURCE);
                        assert_eq!(actual.reported.physical.offset, BASE + 7 + offset);
                        assert!(actual.failure.logical_offset <= bytes.len());
                        assert_eq!(actual.failure.physical.source, SOURCE);
                        assert_eq!(
                            actual.failure.physical.offset,
                            BASE + 7 + actual.failure.logical_offset
                        );
                        let mut terminal = IndexedCanonicalFieldCursor::new(indexed_arena);
                        let first = loop {
                            match terminal.next_field() {
                                Ok(Some(_)) => {}
                                Ok(None) => panic!("expected malformed framing"),
                                Err(error) => break error,
                            }
                        };
                        assert_eq!(terminal.next_field().unwrap_err(), first);
                    }
                    (expected, actual) => {
                        panic!("framing disagreement for {bytes:02x?}: {expected:?} vs {actual:?}")
                    }
                }
            }
        }
    }

    #[test]
    fn primitive_reads_are_exact_without_flattening() {
        let mut bytes = encode_varint(u64::MAX);
        bytes.extend_from_slice(&0x7856_3412u32.to_le_bytes());
        bytes.extend_from_slice(&0xfedc_ba98_7654_3210u64.to_le_bytes());
        bytes.extend_from_slice("λ🦀".as_bytes());
        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let view = persistent_view(&arena, handle, BASE + bytes.len());
        let mut reader = view.reader();

        assert_eq!(reader.read_canonical_varint().unwrap(), u64::MAX);
        assert_eq!(reader.read_fixed32().unwrap(), 0x7856_3412);
        assert_eq!(reader.read_fixed64().unwrap(), 0xfedc_ba98_7654_3210);
        let utf8 = reader.read_utf8("λ🦀".len()).unwrap();
        assert_eq!(bytes_of(&utf8), "λ🦀".as_bytes());
        assert!(reader.is_empty());
        assert_eq!(reader.bytes_read(), bytes.len());

        let eof = reader.read_byte().unwrap_err();
        assert!(matches!(
            eof.kind,
            IndexedByteReadErrorKind::UnexpectedEof {
                operation: IndexedByteReadOperation::Byte,
                ..
            }
        ));
        assert_eq!(eof.failure.kind, IndexedBytePositionKind::End);
        assert_eq!(eof.failure.physical.offset, BASE + bytes.len());
    }

    #[test]
    fn primitive_fixed_and_utf8_eof_errors_project_the_exact_end_boundary() {
        for (bytes, operation, required_end) in [
            (vec![1, 2, 3], IndexedByteReadOperation::Fixed32, 4usize),
            (
                vec![1, 2, 3, 4, 5, 6, 7],
                IndexedByteReadOperation::Fixed64,
                8,
            ),
        ] {
            let mut arena = CanonicalBytesArena::new();
            let handle = append_bytes(&mut arena, &bytes, BASE);
            let view = persistent_view(&arena, handle, BASE + bytes.len());
            let error = if operation == IndexedByteReadOperation::Fixed32 {
                view.reader().read_fixed32().unwrap_err()
            } else {
                view.reader().read_fixed64().unwrap_err()
            };
            assert_eq!(error.kind, IndexedByteReadErrorKind::UnexpectedEof {
                operation,
                required_end,
                available_end: bytes.len(),
            });
            assert_eq!(
                error.start.physical,
                CanonicalByteProvenance::new(SOURCE, BASE)
            );
            assert_eq!(
                error.failure.physical,
                CanonicalByteProvenance::new(SOURCE, BASE + bytes.len())
            );
            assert_eq!(error.failure.kind, IndexedBytePositionKind::End);
        }

        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, b"abc", BASE);
        let view = persistent_view(&arena, handle, BASE + 3);
        let error = view.reader().read_utf8(4).unwrap_err();
        assert_eq!(error.kind, IndexedByteReadErrorKind::UnexpectedEof {
            operation: IndexedByteReadOperation::Utf8,
            required_end: 4,
            available_end: 3,
        });
        assert_eq!(error.failure.physical.offset, BASE + 3);
    }

    #[test]
    fn field_expectations_keep_message_level_failure_provenance() {
        let bytes = [0x0d, 1, 2, 3, 4, 0x12, 1, b'x'];
        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let view = persistent_view(&arena, handle, BASE + bytes.len());
        let mut cursor = IndexedCanonicalFieldCursor::new(view);

        let fixed = cursor.next_field().unwrap().unwrap();
        let error = fixed.expect_varint().unwrap_err();
        assert_eq!(error.error, CanonicalProtobufError::UnexpectedWireType {
            offset: 0,
            field: 1,
            expected: CanonicalWireType::Varint,
            actual: CanonicalWireType::Fixed32,
        });
        assert_eq!(error.reported.physical.offset, BASE);
        assert_eq!(error.failure.physical.offset, BASE);

        let string = cursor.next_field().unwrap().unwrap();
        assert_eq!(bytes_of(string.expect_length_delimited().unwrap()), b"x");
        let error = string.expect_varint().unwrap_err();
        assert_eq!(error.reported.logical_offset, 5);
        assert_eq!(error.reported.physical.offset, BASE + 5);
    }

    #[test]
    fn canonical_varint_errors_identify_start_and_detection_byte_or_eof() {
        for (bytes, kind, failure) in [
            (
                vec![0x80],
                IndexedByteReadErrorKind::TruncatedVarint,
                1usize,
            ),
            (
                vec![0x80, 0x00],
                IndexedByteReadErrorKind::NonMinimalVarint,
                1,
            ),
            (
                vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
                IndexedByteReadErrorKind::VarintOverflow,
                9,
            ),
        ] {
            let mut arena = CanonicalBytesArena::new();
            let handle = append_bytes(&mut arena, &bytes, BASE);
            let view = persistent_view(&arena, handle, BASE + bytes.len());
            let error = view.reader().read_canonical_varint().unwrap_err();
            assert_eq!(error.kind, kind);
            assert_eq!(error.start.logical_offset, 0);
            assert_eq!(error.start.physical.offset, BASE);
            assert_eq!(error.failure.logical_offset, failure);
            assert_eq!(error.failure.physical.offset, BASE + failure);
        }
    }

    #[test]
    fn utf8_acceptance_matches_std_and_projects_invalid_and_truncated_positions() {
        let cases: &[&[u8]] = &[
            b"",
            b"ascii",
            "λ🦀".as_bytes(),
            &[0xc2, 0xa2],
            &[0xe0, 0xa0, 0x80],
            &[0xed, 0x9f, 0xbf],
            &[0xf0, 0x90, 0x80, 0x80],
            &[0xf4, 0x8f, 0xbf, 0xbf],
            &[0x80],
            &[0xc0, 0x80],
            &[0xe0, 0x9f, 0x80],
            &[0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80],
            &[0xe2, 0x28, 0xa1],
            &[0xc2],
            &[0xe2, 0x82],
            &[0xf0, 0x90, 0x80],
        ];
        for bytes in cases {
            let mut arena = CanonicalBytesArena::new();
            let mut value = arena.empty();
            for (index, byte) in bytes.iter().copied().enumerate() {
                let singleton = arena
                    .append(arena.empty(), located(byte, BASE + index))
                    .unwrap();
                value = arena.concat(value, singleton).unwrap();
            }
            let view = persistent_view(&arena, value, BASE + bytes.len());
            let actual = view.validate_utf8();
            assert_eq!(
                actual.is_ok(),
                std::str::from_utf8(bytes).is_ok(),
                "{bytes:02x?}"
            );
            if let Err(error) = actual {
                assert_eq!(
                    error.start.physical.offset,
                    BASE + error.start.logical_offset
                );
                assert_eq!(
                    error.failure.physical.offset,
                    BASE + error.failure.logical_offset
                );
                if matches!(error.kind, IndexedByteReadErrorKind::TruncatedUtf8) {
                    assert_eq!(error.failure.kind, IndexedBytePositionKind::End);
                }
            }
        }
    }

    #[test]
    fn clone_subview_compare_and_provenance_preserve_persistent_storage() {
        let mut arena = CanonicalBytesArena::new();
        let bytes: Vec<u8> = (0..96).collect();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let before = arena.stats();
        let root = persistent_view(&arena, handle, BASE + bytes.len());
        let first = root
            .subview(CanonicalByteRange { start: 17, end: 81 })
            .unwrap();
        let nested = first
            .subview(CanonicalByteRange { start: 11, end: 50 })
            .unwrap();
        let cloned = nested.clone();
        assert_eq!(arena.stats(), before);
        assert!(nested.content_eq(&cloned));
        assert_eq!(nested.compare(&cloned), Ordering::Equal);
        assert_eq!(bytes_of(&nested), bytes[28..67]);
        let mut located = nested.located_bytes();
        let mapped: Vec<_> = located
            .by_ref()
            .map(|byte| byte.provenance.offset)
            .collect();
        assert_eq!(located.bytes_yielded(), nested.len());
        assert_eq!(mapped, (BASE + 28..BASE + 67).collect::<Vec<_>>());
        assert_eq!(nested.end_position().physical.offset, BASE + 67);

        let contiguous = IndexedByteRegion::from_protobuf_region(
            ProtobufRegion::copy_from_slice(&bytes[28..67]),
            99,
        );
        assert!(!contiguous.is_empty());
        assert!(nested.content_eq(&contiguous));
        assert_eq!(nested.compare(&contiguous), Ordering::Equal);
    }

    #[test]
    fn deeply_persistent_framing_is_stack_safe_and_single_pass_linear() {
        let thread = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut arena = CanonicalBytesArena::new();
                let mut message = arena.empty();
                for offset in 0..20_000usize {
                    let byte = if offset % 2 == 0 { 0x08 } else { 0x01 };
                    let singleton = arena
                        .append(arena.empty(), located(byte, BASE + offset))
                        .unwrap();
                    message = arena.concat(message, singleton).unwrap();
                }
                let storage = arena.stats();
                assert!(storage.content_nodes <= 40_000);
                assert!(storage.provenance_nodes <= 40_000);
                let view = persistent_view(&arena, message, BASE + 20_000);
                let mut cursor = IndexedCanonicalFieldCursor::new(view);
                let mut fields = 0usize;
                while cursor.next_field().unwrap().is_some() {
                    fields += 1;
                }
                assert_eq!(fields, 10_000);
                assert_eq!(cursor.stats(), IndexedCanonicalFieldCursorStats {
                    fields_read: 10_000,
                    bytes_read: 20_000,
                });
            })
            .unwrap();
        thread.join().unwrap();
    }
}
