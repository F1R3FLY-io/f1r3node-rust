//! Indexed, non-flattening byte views and canonical protobuf framing.
//!
//! A view is either a constant-size owned window into [`ProtobufRegion`] or a
//! constant-size borrowed window into [`CanonicalBytesArena`]. Persistent
//! windows retain the original handle plus a range, so cloning and forming a
//! subview never walks a parent chain. Cursors expand the persistent DAG once,
//! in forward order, and framing consumes every input byte at most once.

use std::cmp::Ordering;
use std::fmt;

use verified_score_store::BorrowedIndexedWindow;

#[cfg(test)]
use super::canonical_bytes_arena::{
    CanonicalByteCursor, CanonicalBytesArena, CanonicalBytesHandle, CanonicalLocatedByteCursor,
};
use super::canonical_bytes_arena::{
    CanonicalByteProvenance, CanonicalLocatedByte, FrozenCanonicalByteCursor, FrozenCanonicalBytes,
    FrozenCanonicalLocatedByteCursor,
};
use super::canonical_source_tree::{
    SourceTreeByteCursor, SourceTreeLocatedByteCursor, SourceTreeWindow,
};
#[cfg(test)]
use super::protobuf_cursor::ProtobufRegion;
use super::protobuf_cursor::{CanonicalByteRange, CanonicalProtobufError, CanonicalWireType};

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
    Borrowed(BorrowedIndexedWindow<'a>),
    #[cfg(test)]
    Contiguous {
        bytes: prost::bytes::Bytes,
        source: u32,
        absolute_start: usize,
        end: CanonicalByteProvenance,
    },
    #[cfg(test)]
    Arena {
        arena: &'a CanonicalBytesArena<'a>,
        root: CanonicalBytesHandle,
        start: usize,
        len: usize,
        root_end: CanonicalByteProvenance,
    },
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "production completions build source-backed Composed regions (67035c57b), so frozen backings originate only in the test-only from_frozen control and are otherwise only re-windowed or joined"
        )
    )]
    Frozen {
        root: FrozenCanonicalBytes,
        start: usize,
        len: usize,
        root_end: CanonicalByteProvenance,
    },
    Composed {
        root: SourceTreeWindow<'a>,
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
            IndexedByteBacking::Borrowed(_) => "borrowed",
            #[cfg(test)]
            IndexedByteBacking::Contiguous { .. } => "contiguous",
            #[cfg(test)]
            IndexedByteBacking::Arena { .. } => "arena",
            IndexedByteBacking::Frozen { .. } => "frozen",
            IndexedByteBacking::Composed { .. } => "composed",
        };
        formatter
            .debug_struct("IndexedByteRegion")
            .field("backing", &backing)
            .field("len", &self.len())
            .finish()
    }
}

impl<'a> IndexedByteRegion<'a> {
    /// Borrow one contiguous input without copying it. `absolute_start`
    /// identifies the first byte in the caller's physical source, so nested
    /// framing errors retain whole-source rather than submessage offsets.
    pub(crate) fn from_borrowed(bytes: &'a [u8], source: u32, absolute_start: usize) -> Self {
        Self {
            backing: IndexedByteBacking::Borrowed(BorrowedIndexedWindow::new(
                bytes,
                source,
                absolute_start,
            )),
        }
    }

    pub(crate) fn borrowed_window(&self) -> Option<BorrowedIndexedWindow<'a>> {
        match &self.backing {
            IndexedByteBacking::Borrowed(window) => Some(*window),
            _ => None,
        }
    }

    pub(crate) fn same_borrowed_window(&self, other: &BorrowedIndexedWindow<'a>) -> bool {
        let Some(left) = self.borrowed_window() else {
            return false;
        };
        std::ptr::eq(left.root_slice(), other.root_slice())
            && left.start() == other.start()
            && left.len() == other.len()
            && left.source() == other.source()
            && left.absolute_start() == other.absolute_start()
            && left.end_source() == other.end_source()
            && left.end_offset() == other.end_offset()
    }

    pub(crate) fn from_source(
        root: SourceTreeWindow<'a>,
        root_end: CanonicalByteProvenance,
    ) -> Self {
        let len = root.len();
        Self {
            backing: IndexedByteBacking::Composed {
                root,
                start: 0,
                len,
                root_end,
            },
        }
    }

    pub(crate) fn source_window(&self) -> Option<SourceTreeWindow<'a>> {
        match &self.backing {
            IndexedByteBacking::Borrowed(window) => SourceTreeWindow::from_borrowed_range(
                window.root_slice(),
                window.start()..window.start().checked_add(window.len())?,
                window.source(),
                window.absolute_start(),
            ),
            IndexedByteBacking::Frozen {
                root, start, len, ..
            } => Some(SourceTreeWindow::from_frozen(
                root.subview(*start..start.checked_add(*len)?)?,
            )),
            IndexedByteBacking::Composed {
                root, start, len, ..
            } => root.subview(*start..start.checked_add(*len)?),
            #[cfg(test)]
            IndexedByteBacking::Contiguous { .. } | IndexedByteBacking::Arena { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_protobuf_region(region: ProtobufRegion, source: u32) -> Self {
        let range = region.absolute_range();
        let end = CanonicalByteProvenance::new(source, range.end);
        Self {
            backing: IndexedByteBacking::Contiguous {
                bytes: region.into_bytes(),
                source,
                absolute_start: range.start,
                end,
            },
        }
    }

    /// Construct a persistent view. `root_end` is the exact physical boundary
    /// immediately after `root`; it is required even for an empty root because
    /// no logical byte exists from which that boundary could be inferred.
    #[cfg(test)]
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
            IndexedByteBacking::Borrowed(window) => window.len(),
            #[cfg(test)]
            IndexedByteBacking::Contiguous { bytes, .. } => bytes.len(),
            #[cfg(test)]
            IndexedByteBacking::Arena { len, .. } => *len,
            IndexedByteBacking::Frozen { len, .. } => *len,
            IndexedByteBacking::Composed { len, .. } => *len,
        }
    }

    pub(crate) fn is_empty(&self) -> bool { self.len() == 0 }

    /// Constant-time identity for two views of the same bytes and provenance.
    /// This compares the backing handle and range, without reading payloads.
    pub(crate) fn same_view(&self, other: &Self) -> bool {
        match (&self.backing, &other.backing) {
            (IndexedByteBacking::Borrowed(left), IndexedByteBacking::Borrowed(right)) => {
                std::ptr::eq(left.root_slice(), right.root_slice())
                    && left.start() == right.start()
                    && left.len() == right.len()
                    && left.source() == right.source()
                    && left.absolute_start() == right.absolute_start()
                    && left.end_source() == right.end_source()
                    && left.end_offset() == right.end_offset()
            }
            #[cfg(test)]
            (
                IndexedByteBacking::Contiguous {
                    bytes,
                    source,
                    absolute_start,
                    end,
                },
                IndexedByteBacking::Contiguous {
                    bytes: other_bytes,
                    source: other_source,
                    absolute_start: other_absolute_start,
                    end: other_end,
                },
            ) => {
                std::ptr::eq(bytes.as_ptr(), other_bytes.as_ptr())
                    && bytes.len() == other_bytes.len()
                    && source == other_source
                    && absolute_start == other_absolute_start
                    && end == other_end
            }
            #[cfg(test)]
            (
                IndexedByteBacking::Arena {
                    arena,
                    root,
                    start,
                    len,
                    root_end,
                },
                IndexedByteBacking::Arena {
                    arena: other_arena,
                    root: other_root,
                    start: other_start,
                    len: other_len,
                    root_end: other_root_end,
                },
            ) => {
                std::ptr::eq(*arena, *other_arena)
                    && root == other_root
                    && start == other_start
                    && len == other_len
                    && root_end == other_root_end
            }
            (
                IndexedByteBacking::Frozen {
                    root,
                    start,
                    len,
                    root_end,
                },
                IndexedByteBacking::Frozen {
                    root: other_root,
                    start: other_start,
                    len: other_len,
                    root_end: other_root_end,
                },
            ) => {
                root.same_view(other_root)
                    && start == other_start
                    && len == other_len
                    && root_end == other_root_end
            }
            (
                IndexedByteBacking::Composed {
                    root,
                    start,
                    len,
                    root_end,
                },
                IndexedByteBacking::Composed {
                    root: other_root,
                    start: other_start,
                    len: other_len,
                    root_end: other_root_end,
                },
            ) => {
                root.same_view(other_root)
                    && start == other_start
                    && len == other_len
                    && root_end == other_root_end
            }
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn try_coalesce(&self, right: &Self) -> Option<Self> {
        match (&self.backing, &right.backing) {
            (IndexedByteBacking::Borrowed(left), IndexedByteBacking::Borrowed(right)) => {
                if !std::ptr::eq(left.root_slice(), right.root_slice())
                    || left.source() != right.source()
                    || left.absolute_start() != right.absolute_start()
                    || left.start().checked_add(left.len())? != right.start()
                {
                    return None;
                }
                let len = left.len().checked_add(right.len())?;
                let end = left.start().checked_add(len)?;
                if end > left.root_slice().len() {
                    return None;
                }
                left.absolute_start().checked_add(left.start())?;
                if len != 0 {
                    left.absolute_start().checked_add(end - 1)?;
                }
                right.position_at(right.len())?;
                Some(Self {
                    backing: IndexedByteBacking::Borrowed(left.rewindow(
                        end,
                        right.end_source(),
                        right.end_offset(),
                    )?),
                })
            }
            (
                IndexedByteBacking::Frozen {
                    root, start, len, ..
                },
                IndexedByteBacking::Frozen {
                    root: right_root,
                    start: right_start,
                    len: right_len,
                    ..
                },
            ) => {
                let left = root.subview(*start..start.checked_add(*len)?)?;
                let next =
                    right_root.subview(*right_start..right_start.checked_add(*right_len)?)?;
                let root = left.try_coalesce(&next)?;
                let root_end = right.position_at(*right_len)?.physical;
                let len = root.len();
                Some(Self {
                    backing: IndexedByteBacking::Frozen {
                        root,
                        start: 0,
                        len,
                        root_end,
                    },
                })
            }
            (IndexedByteBacking::Composed { .. }, _) | (_, IndexedByteBacking::Composed { .. }) => {
                let root = self
                    .source_window()?
                    .try_coalesce(&right.source_window()?)?;
                Some(Self::from_source(
                    root,
                    right.position_at(right.len())?.physical,
                ))
            }
            _ => None,
        }
    }

    /// Form a constant-size subview without traversing or copying its bytes.
    pub(crate) fn subview(&self, range: CanonicalByteRange) -> Option<Self> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        match &self.backing {
            IndexedByteBacking::Borrowed(window) => {
                let start = window.start().checked_add(range.start)?;
                window.absolute_start().checked_add(start)?;
                Some(Self {
                    backing: IndexedByteBacking::Borrowed(window.subview(range.start, range.end)?),
                })
            }
            #[cfg(test)]
            IndexedByteBacking::Contiguous {
                bytes,
                source,
                absolute_start,
                end,
            } => Some(Self {
                backing: IndexedByteBacking::Contiguous {
                    bytes: bytes.slice(range.start..range.end),
                    source: *source,
                    absolute_start: absolute_start.checked_add(range.start)?,
                    end: if range.end == bytes.len() {
                        *end
                    } else {
                        CanonicalByteProvenance::new(
                            *source,
                            absolute_start.checked_add(range.end)?,
                        )
                    },
                },
            }),
            #[cfg(test)]
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
            IndexedByteBacking::Frozen {
                root,
                start,
                root_end,
                ..
            } => Some(Self {
                backing: IndexedByteBacking::Frozen {
                    root: root.clone(),
                    start: start.checked_add(range.start)?,
                    len: range.end - range.start,
                    root_end: *root_end,
                },
            }),
            IndexedByteBacking::Composed {
                root,
                start,
                root_end,
                ..
            } => Some(Self {
                backing: IndexedByteBacking::Composed {
                    root: root.clone(),
                    start: start.checked_add(range.start)?,
                    len: range.end - range.start,
                    root_end: *root_end,
                },
            }),
        }
    }

    pub(crate) fn subview_with_end(
        &self,
        range: CanonicalByteRange,
        end: CanonicalByteProvenance,
    ) -> Option<Self> {
        let mut view = self.subview(range)?;
        match &mut view.backing {
            IndexedByteBacking::Borrowed(window) => {
                *window = window.with_end(end.source, end.offset);
            }
            #[cfg(test)]
            IndexedByteBacking::Contiguous { end: boundary, .. } => *boundary = end,
            #[cfg(test)]
            IndexedByteBacking::Arena {
                arena,
                root,
                start,
                len,
                root_end,
            } => {
                *root = arena.slice(*root, *start..start.checked_add(*len)?).ok()?;
                *start = 0;
                *root_end = end;
            }
            IndexedByteBacking::Frozen {
                root,
                start,
                len,
                root_end,
            } => {
                *root = root.subview(*start..start.checked_add(*len)?)?;
                *start = 0;
                *root_end = end;
            }
            IndexedByteBacking::Composed {
                root,
                start,
                len,
                root_end,
            } => {
                *root = root.subview(*start..start.checked_add(*len)?)?;
                *start = 0;
                *root_end = end;
            }
        }
        Some(view)
    }

    pub(crate) fn position_at(&self, logical_offset: usize) -> Option<IndexedBytePosition> {
        let len = self.len();
        if logical_offset > len {
            return None;
        }
        let physical = match &self.backing {
            IndexedByteBacking::Composed {
                root,
                start,
                root_end,
                ..
            } => {
                let offset = start.checked_add(logical_offset)?;
                if offset < root.len() {
                    root.provenance_at(offset)?
                } else if offset == root.len() {
                    *root_end
                } else {
                    return None;
                }
            }
            IndexedByteBacking::Borrowed(window) => {
                let (source, offset) = window.position_at(logical_offset)?;
                CanonicalByteProvenance::new(source, offset)
            }
            #[cfg(test)]
            IndexedByteBacking::Contiguous {
                absolute_start,
                source,
                end,
                ..
            } => {
                if logical_offset == len {
                    *end
                } else {
                    let absolute = absolute_start.checked_add(logical_offset)?;
                    CanonicalByteProvenance::new(*source, absolute)
                }
            }
            #[cfg(test)]
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
            IndexedByteBacking::Frozen {
                root,
                start,
                root_end,
                ..
            } => {
                let root_offset = start.checked_add(logical_offset)?;
                if root_offset < root.len() {
                    root.provenance_at(root_offset)?
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
            IndexedByteBacking::Composed {
                root, start, len, ..
            } => {
                let view = root
                    .subview(*start..start.checked_add(*len).expect("checked composed view end"))
                    .expect("checked composed source view");
                IndexedByteCursorInner::Composed(view.bytes())
            }
            IndexedByteBacking::Borrowed(window) => IndexedByteCursorInner::Borrowed {
                bytes: window.as_slice(),
                at: 0,
            },
            #[cfg(test)]
            IndexedByteBacking::Contiguous { bytes, .. } => IndexedByteCursorInner::Contiguous {
                region: ProtobufRegion::new(bytes.clone()),
                at: 0,
            },
            #[cfg(test)]
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
            IndexedByteBacking::Frozen {
                root, start, len, ..
            } => {
                let end = start
                    .checked_add(*len)
                    .expect("a validated frozen view range cannot overflow");
                let view = root
                    .subview(*start..end)
                    .expect("an indexed frozen view stays inside its root");
                IndexedByteCursorInner::Frozen(view.bytes())
            }
        };
        IndexedByteCursor { inner }
    }

    pub(crate) fn located_bytes(&self) -> IndexedLocatedByteCursor<'a> {
        let inner = match &self.backing {
            IndexedByteBacking::Composed {
                root, start, len, ..
            } => {
                let view = root
                    .subview(*start..start.checked_add(*len).expect("checked composed view end"))
                    .expect("checked composed source view");
                IndexedLocatedByteCursorInner::Composed(view.located_bytes())
            }
            IndexedByteBacking::Borrowed(window) => IndexedLocatedByteCursorInner::Borrowed {
                bytes: window.as_slice(),
                source: window.source(),
                absolute_start: window
                    .absolute_start()
                    .checked_add(window.start())
                    .expect("checked borrowed origin"),
                at: 0,
            },
            #[cfg(test)]
            IndexedByteBacking::Contiguous {
                bytes,
                source,
                absolute_start,
                ..
            } => IndexedLocatedByteCursorInner::Contiguous {
                region: ProtobufRegion::new(bytes.clone()),
                source: *source,
                absolute_start: *absolute_start,
                at: 0,
            },
            #[cfg(test)]
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
                IndexedLocatedByteCursorInner::Arena {
                    cursor: arena.located_bytes(handle),
                    remaining: *len,
                }
            }
            IndexedByteBacking::Frozen {
                root, start, len, ..
            } => {
                let end = start
                    .checked_add(*len)
                    .expect("a validated frozen view range cannot overflow");
                let view = root
                    .subview(*start..end)
                    .expect("an indexed frozen view stays inside its root");
                IndexedLocatedByteCursorInner::Frozen(view.located_bytes())
            }
        };
        IndexedLocatedByteCursor {
            inner,
            yielded: 0,
            traversed: 0,
        }
    }

    pub(crate) fn reader(&self) -> IndexedByteReader<'a> { IndexedByteReader::new(self.clone()) }

    /// Random access for schema-neutral indexed consumers. Frozen and
    /// contiguous views are bounded O(1); the borrowed arena compatibility
    /// backing may walk its persistent parent structure.
    pub(crate) fn byte_at(&self, logical_offset: usize) -> Option<u8> {
        if logical_offset >= self.len() {
            return None;
        }
        match &self.backing {
            IndexedByteBacking::Borrowed(window) => window.byte_at(logical_offset),
            #[cfg(test)]
            IndexedByteBacking::Contiguous { bytes, .. } => bytes.get(logical_offset).copied(),
            #[cfg(test)]
            IndexedByteBacking::Arena {
                arena, root, start, ..
            } => arena.byte_at(*root, start.checked_add(logical_offset)?),
            IndexedByteBacking::Frozen { root, start, .. } => {
                root.byte_at(start.checked_add(logical_offset)?)
            }
            IndexedByteBacking::Composed { root, start, .. } => {
                root.byte_at(start.checked_add(logical_offset)?)
            }
        }
    }

    pub(crate) fn content_eq(&self, other: &IndexedByteRegion<'_>) -> bool {
        if self.len() != other.len() {
            return false;
        }
        match (&self.backing, &other.backing) {
            (IndexedByteBacking::Composed { .. }, IndexedByteBacking::Composed { .. }) => self
                .source_window()
                .expect("checked composed source window")
                .content_eq(
                    &other
                        .source_window()
                        .expect("checked composed source window"),
                ),
            (
                IndexedByteBacking::Frozen {
                    root: left,
                    start: left_start,
                    len,
                    ..
                },
                IndexedByteBacking::Frozen {
                    root: right,
                    start: right_start,
                    ..
                },
            ) => {
                let left_end = left_start
                    .checked_add(*len)
                    .expect("a validated frozen region cannot overflow");
                let right_end = right_start
                    .checked_add(*len)
                    .expect("a validated frozen region cannot overflow");
                let left = left
                    .subview(*left_start..left_end)
                    .expect("an indexed frozen region stays inside its root");
                let right = right
                    .subview(*right_start..right_end)
                    .expect("an indexed frozen region stays inside its root");
                left.content_eq(&right)
            }
            _ => self.bytes().content_eq(other.bytes()),
        }
    }

    pub(crate) fn compare(&self, other: &IndexedByteRegion<'_>) -> Ordering {
        self.bytes().cmp(other.bytes())
    }

    pub(crate) fn validate_utf8(&self) -> Result<(), IndexedByteReadError> {
        // A valid contiguous window needs no indexed reader or returned view.
        // Keep the indexed scanner for malformed input so its exact physical
        // source positions and error classification remain authoritative.
        if let Some(bytes) =
            rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource::contiguous_range(
                self,
                0..self.len(),
            )
        {
            if std::str::from_utf8(bytes).is_ok() {
                return Ok(());
            }
        }
        let mut reader = self.reader();
        reader.scan_utf8(self.len()).map(|_| ())
    }
}

impl rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource for IndexedByteRegion<'_> {
    fn len(&self) -> usize { IndexedByteRegion::len(self) }

    fn byte_at(&self, index: usize) -> Option<u8> { IndexedByteRegion::byte_at(self, index) }

    fn contiguous_range(&self, range: std::ops::Range<usize>) -> Option<&[u8]> {
        match &self.backing {
            IndexedByteBacking::Composed {
                root, start, len, ..
            } => {
                if range.start > range.end || range.end > *len {
                    return None;
                }
                root.contiguous_bytes(
                    start.checked_add(range.start)?..start.checked_add(range.end)?,
                )
            }
            IndexedByteBacking::Borrowed(window) => {
                if range.start > range.end || range.end > window.len() {
                    return None;
                }
                window.as_slice().get(range)
            }
            #[cfg(test)]
            IndexedByteBacking::Contiguous { bytes, .. } => bytes.get(range),
            #[cfg(test)]
            IndexedByteBacking::Arena { .. } => None,
            IndexedByteBacking::Frozen {
                root, start, len, ..
            } => {
                if range.start > range.end || range.end > *len {
                    return None;
                }
                let absolute_start = start.checked_add(range.start)?;
                let absolute_end = start.checked_add(range.end)?;
                root.contiguous_bytes(absolute_start..absolute_end)
            }
        }
    }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        let Some(end) = start.checked_add(output.len()) else {
            return false;
        };
        if end > self.len() {
            return false;
        }
        match &self.backing {
            IndexedByteBacking::Composed {
                root,
                start: root_start,
                ..
            } => {
                let Some(absolute) = root_start.checked_add(start) else {
                    return false;
                };
                root.copy_exact(absolute, output)
            }
            IndexedByteBacking::Borrowed(window) => {
                let Some(source) = window.as_slice().get(start..end) else {
                    return false;
                };
                output.copy_from_slice(source);
                true
            }
            #[cfg(test)]
            IndexedByteBacking::Contiguous { bytes, .. } => {
                let Some(source) = bytes.get(start..end) else {
                    return false;
                };
                output.copy_from_slice(source);
                true
            }
            IndexedByteBacking::Frozen {
                root,
                start: root_start,
                ..
            } => {
                let Some(absolute_start) = root_start.checked_add(start) else {
                    return false;
                };
                root.copy_exact(absolute_start, output)
            }
            #[cfg(test)]
            IndexedByteBacking::Arena { .. } => {
                let Some(view) = self.subview(CanonicalByteRange { start, end }) else {
                    return false;
                };
                for (destination, source) in output.iter_mut().zip(view.bytes()) {
                    *destination = source;
                }
                true
            }
        }
    }

    fn ranges_equal(&self, left: std::ops::Range<usize>, right: std::ops::Range<usize>) -> bool {
        let Some(left) = self.subview(CanonicalByteRange {
            start: left.start,
            end: left.end,
        }) else {
            return false;
        };
        let Some(right) = self.subview(CanonicalByteRange {
            start: right.start,
            end: right.end,
        }) else {
            return false;
        };
        left.content_eq(&right)
    }
}

/// An indexed region whose storage has no borrow from its originating arena.
#[cfg(test)]
pub(crate) type OwnedIndexedByteRegion = IndexedByteRegion<'static>;

#[cfg(test)]
impl IndexedByteRegion<'static> {
    pub(crate) fn from_frozen(
        root: FrozenCanonicalBytes,
        root_end: CanonicalByteProvenance,
    ) -> Self {
        let len = root.len();
        Self {
            backing: IndexedByteBacking::Frozen {
                root,
                start: 0,
                len,
                root_end,
            },
        }
    }
}

enum IndexedByteCursorInner<'a> {
    Borrowed {
        bytes: &'a [u8],
        at: usize,
    },
    #[cfg(test)]
    Contiguous {
        region: ProtobufRegion,
        at: usize,
    },
    #[cfg(test)]
    Arena(CanonicalByteCursor<'a>),
    Frozen(FrozenCanonicalByteCursor),
    Composed(SourceTreeByteCursor<'a>),
}

/// A single forward traversal. Persistent parents are expanded by the arena's
/// heap worklist rather than revisited through logical byte indexing.
pub(crate) struct IndexedByteCursor<'a> {
    inner: IndexedByteCursorInner<'a>,
}

impl IndexedByteCursor<'_> {
    fn has_chunks(&self) -> bool {
        #[cfg(test)]
        if matches!(self.inner, IndexedByteCursorInner::Arena(_)) {
            return false;
        }
        true
    }

    fn chunk(&mut self) -> Option<&[u8]> {
        match &mut self.inner {
            IndexedByteCursorInner::Borrowed { bytes, at } => {
                (*at < bytes.len()).then(|| &bytes[*at..])
            }
            #[cfg(test)]
            IndexedByteCursorInner::Contiguous { region, at } => {
                (*at < region.len()).then(|| &region.as_bytes()[*at..])
            }
            IndexedByteCursorInner::Frozen(cursor) => cursor.chunk(),
            IndexedByteCursorInner::Composed(cursor) => cursor.chunk(),
            #[cfg(test)]
            IndexedByteCursorInner::Arena(_) => None,
        }
    }

    fn skip_chunk_prefix(&mut self, width: usize) -> bool {
        match &mut self.inner {
            IndexedByteCursorInner::Borrowed { bytes, at } => {
                if width > bytes.len() - *at {
                    return false;
                }
                *at += width;
                true
            }
            #[cfg(test)]
            IndexedByteCursorInner::Contiguous { region, at } => {
                if width > region.len() - *at {
                    return false;
                }
                *at += width;
                true
            }
            IndexedByteCursorInner::Frozen(cursor) => {
                if width > cursor.len() {
                    return false;
                }
                cursor.fast_forward(width);
                true
            }
            IndexedByteCursorInner::Composed(cursor) => cursor.skip_exact(width),
            #[cfg(test)]
            IndexedByteCursorInner::Arena(_) => false,
        }
    }

    fn content_eq(mut self, mut other: IndexedByteCursor<'_>) -> bool {
        if !self.has_chunks() || !other.has_chunks() {
            return self.eq(other);
        }
        loop {
            let width = match (self.chunk(), other.chunk()) {
                (Some(left), Some(right)) => {
                    let width = left.len().min(right.len());
                    if left[..width] != right[..width] {
                        return false;
                    }
                    width
                }
                (None, None) => return true,
                _ => return false,
            };
            assert!(self.skip_chunk_prefix(width));
            assert!(other.skip_chunk_prefix(width));
        }
    }
}

impl Iterator for IndexedByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = match &mut self.inner {
            IndexedByteCursorInner::Borrowed { bytes, at } => {
                let byte = bytes.get(*at).copied();
                *at += usize::from(byte.is_some());
                byte
            }
            #[cfg(test)]
            IndexedByteCursorInner::Contiguous { region, at } => {
                let byte = region.as_bytes().get(*at).copied();
                *at += usize::from(byte.is_some());
                byte
            }
            #[cfg(test)]
            IndexedByteCursorInner::Arena(cursor) => cursor.next(),
            IndexedByteCursorInner::Frozen(cursor) => cursor.next(),
            IndexedByteCursorInner::Composed(cursor) => cursor.next(),
        };
        byte
    }
}

enum IndexedLocatedByteCursorInner<'a> {
    Borrowed {
        bytes: &'a [u8],
        source: u32,
        absolute_start: usize,
        at: usize,
    },
    #[cfg(test)]
    Contiguous {
        region: ProtobufRegion,
        source: u32,
        absolute_start: usize,
        at: usize,
    },
    #[cfg(test)]
    Arena {
        cursor: CanonicalLocatedByteCursor<'a>,
        remaining: usize,
    },
    Frozen(FrozenCanonicalLocatedByteCursor),
    Composed(SourceTreeLocatedByteCursor<'a>),
}

pub(crate) struct IndexedLocatedByteCursor<'a> {
    inner: IndexedLocatedByteCursorInner<'a>,
    yielded: usize,
    traversed: usize,
}

impl IndexedLocatedByteCursor<'_> {
    pub(crate) const fn bytes_yielded(&self) -> usize { self.yielded }

    pub(crate) const fn bytes_traversed(&self) -> usize { self.traversed }

    pub(crate) fn skip_exact(&mut self, width: usize) -> bool {
        let remaining = match &self.inner {
            IndexedLocatedByteCursorInner::Borrowed { bytes, at, .. } => bytes.len() - at,
            #[cfg(test)]
            IndexedLocatedByteCursorInner::Contiguous { region, at, .. } => region.len() - at,
            #[cfg(test)]
            IndexedLocatedByteCursorInner::Arena { remaining, .. } => *remaining,
            IndexedLocatedByteCursorInner::Frozen(cursor) => cursor.len(),
            IndexedLocatedByteCursorInner::Composed(cursor) => cursor.len(),
        };
        if width > remaining {
            return false;
        }
        if !self.fast_forward(width) {
            for _ in 0..width {
                self.next()
                    .expect("a checked cursor skip stays inside its input");
            }
        }
        true
    }

    fn fast_forward(&mut self, width: usize) -> bool {
        let advanced = match &mut self.inner {
            IndexedLocatedByteCursorInner::Borrowed { bytes, at, .. } => {
                debug_assert!(*at + width <= bytes.len());
                *at += width;
                true
            }
            #[cfg(test)]
            IndexedLocatedByteCursorInner::Contiguous { region, at, .. } => {
                debug_assert!(*at + width <= region.len());
                *at += width;
                true
            }
            #[cfg(test)]
            IndexedLocatedByteCursorInner::Arena { .. } => false,
            IndexedLocatedByteCursorInner::Frozen(cursor) => {
                cursor.fast_forward(width);
                true
            }
            IndexedLocatedByteCursorInner::Composed(cursor) => cursor.skip_exact(width),
        };
        if advanced {
            self.yielded += width;
        }
        advanced
    }
}

impl Iterator for IndexedLocatedByteCursor<'_> {
    type Item = CanonicalLocatedByte;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = match &mut self.inner {
            IndexedLocatedByteCursorInner::Borrowed {
                bytes,
                source,
                absolute_start,
                at,
            } => {
                let logical = *at;
                let byte = bytes.get(logical).copied();
                *at += usize::from(byte.is_some());
                byte.map(|byte| {
                    CanonicalLocatedByte::new(
                        byte,
                        CanonicalByteProvenance::new(*source, *absolute_start + logical),
                    )
                })
            }
            #[cfg(test)]
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
            #[cfg(test)]
            IndexedLocatedByteCursorInner::Arena { cursor, remaining } => {
                let byte = cursor.next();
                *remaining -= usize::from(byte.is_some());
                byte
            }
            IndexedLocatedByteCursorInner::Frozen(cursor) => cursor.next(),
            IndexedLocatedByteCursorInner::Composed(cursor) => cursor.next(),
        };
        self.yielded += usize::from(byte.is_some());
        self.traversed += usize::from(byte.is_some());
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

    pub(crate) const fn bytes_traversed(&self) -> usize { self.cursor.bytes_traversed() }

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
        assert!(
            self.cursor.skip_exact(width),
            "reader and cursor share one checked input"
        );
        self.at = end;
        Ok(())
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

    /// Validate exactly `length` UTF-8 bytes without constructing a view.
    /// The admission hot path needs only this check, including exact source
    /// positions on failure.
    #[cfg_attr(debug_assertions, inline(never))]
    fn scan_utf8(&mut self, length: usize) -> Result<CanonicalByteRange, IndexedByteReadError> {
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
        Ok(CanonicalByteRange { start, end })
    }

    /// Validate and return a non-copying view of exactly `length` UTF-8 bytes.
    #[cfg(test)]
    pub(crate) fn read_utf8(
        &mut self,
        length: usize,
    ) -> Result<IndexedByteRegion<'a>, IndexedByteReadError> {
        let range = self.scan_utf8(length)?;
        Ok(self
            .view
            .subview(range)
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
    #[cfg(test)]
    pub(crate) payload_range: CanonicalByteRange,
    pub(crate) field_start: IndexedBytePosition,
}

impl<'a> IndexedCanonicalField<'a> {
    #[cfg(test)]
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

    #[cfg(test)]
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
    pub(crate) bytes_traversed: usize,
    pub(crate) bytes_fast_forwarded: usize,
}

/// Scalar state of a Message cursor immediately before `next_field`. A
/// successful capture also certifies that its message and reader retain the
/// supplied indexed view; no payload bytes are copied or traversed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexedMessagePreReadSource {
    pub(crate) reader_at: usize,
    pub(crate) cursor_yielded: usize,
    pub(crate) last_field: u32,
    pub(crate) fields_read: usize,
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

    /// Bind both the cursor's retained message and its reader to one indexed view.
    pub(crate) fn matches_message_view(&self, message: &IndexedByteRegion<'a>) -> bool {
        self.message.same_view(message) && self.reader.view.same_view(message)
    }

    /// Message frames use generated Prost slot order, so numeric wire order
    /// is deliberately disabled. The reader offset and yielded count must
    /// coincide for the concrete wire-cursor source projection.
    pub(crate) fn message_pre_read_source(
        &self,
        message: &IndexedByteRegion<'a>,
    ) -> Option<IndexedMessagePreReadSource> {
        let source = IndexedMessagePreReadSource {
            reader_at: self.reader.offset(),
            cursor_yielded: self.reader.bytes_read(),
            last_field: self.last_field,
            fields_read: self.fields_read,
        };
        (self.matches_message_view(message)
            && !self.enforce_numeric_order
            && self.terminal_error.is_none()
            && source.reader_at == source.cursor_yielded
            && source.reader_at <= message.len())
        .then_some(source)
    }

    pub(crate) fn matches_initial_view(
        &self,
        message: &IndexedByteRegion<'a>,
        enforce_numeric_order: bool,
    ) -> bool {
        self.matches_message_view(message)
            && self.enforce_numeric_order == enforce_numeric_order
            && self.last_field == 0
            && self.fields_read == 0
            && self.terminal_error.is_none()
            && self.reader.offset() == 0
            && self.reader.bytes_read() == 0
            && self.reader.bytes_traversed() == 0
            && self.reader.is_empty() == message.is_empty()
    }

    /// Observe the ordered cursor immediately after a successful field read.
    /// All checks use retained view identity and scalar cursor state; neither
    /// the message nor the field payload is scanned.
    pub(crate) fn matches_successful_ordered_field(
        &self,
        message: &IndexedByteRegion<'a>,
        field: &IndexedCanonicalField<'a>,
        last_field_before: u32,
        fields_read_before: usize,
    ) -> bool {
        self.matches_message_view(message)
            && self.enforce_numeric_order
            && self.terminal_error.is_none()
            && last_field_before <= field.number
            && self.last_field == field.number
            && self.reader.offset() == field.encoded_range.end
            && fields_read_before.checked_add(1) == Some(self.fields_read)
    }

    pub(crate) fn last_field_number(&self) -> u32 { self.last_field }

    /// Logical offset of the next byte, independent of traversal counters.
    pub(crate) fn logical_offset(&self) -> usize { self.reader.offset() }

    pub(crate) fn is_empty(&self) -> bool { self.reader.is_empty() }

    pub(crate) fn is_terminal_error_free(&self) -> bool { self.terminal_error.is_none() }

    pub(crate) fn stats(&self) -> IndexedCanonicalFieldCursorStats {
        let bytes_read = self.reader.bytes_read();
        let bytes_traversed = self.reader.bytes_traversed();
        IndexedCanonicalFieldCursorStats {
            fields_read: self.fields_read,
            bytes_read,
            bytes_traversed,
            bytes_fast_forwarded: bytes_read - bytes_traversed,
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
        let (value, _payload_range) = match wire_type {
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
            #[cfg(test)]
            payload_range: _payload_range,
            field_start,
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/support/indexed_span_transport.rs"]
mod indexed_span_transport;

#[cfg(test)]
#[path = "../../../tests/support/normalized_source_spans.rs"]
mod normalized_source_spans;

#[cfg(test)]
#[path = "indexed_source_projection_properties.rs"]
mod indexed_source_projection_properties;

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use prost::bytes::Bytes;

    use super::super::protobuf_cursor::{CanonicalRegionFieldCursor, CanonicalRegionFieldValue};
    use super::*;

    const SOURCE: u32 = 71;
    const BASE: usize = 10_000;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn equal_borrowed_images_from_distinct_backings_are_not_the_same_view(
            bytes in prop::collection::vec(any::<u8>(), 1..32),
            source in 1u32..1000,
            base in 0usize..100_000,
        ) {
            let shadow = bytes.clone();
            let selected = IndexedByteRegion::from_borrowed(&bytes, source, base);
            let other = IndexedByteRegion::from_borrowed(&shadow, source, base);
            prop_assert!(selected.content_eq(&other));
            prop_assert!(!selected.same_view(&other));
            prop_assert!(selected.same_view(&selected.clone()));
        }

        #[test]
        fn initial_field_cursor_requires_selected_view_and_exact_ordering_mode(
            bytes in prop::collection::vec(any::<u8>(), 0..32),
            source in 1u32..1000,
            base in 0usize..100_000,
        ) {
            let shadow = bytes.clone();
            let selected = IndexedByteRegion::from_borrowed(&bytes, source, base);
            let other = IndexedByteRegion::from_borrowed(&shadow, source + 1, base);
            prop_assert!(!selected.same_view(&other));
            for enforce_numeric_order in [false, true] {
                let fresh = || {
                    if enforce_numeric_order {
                        IndexedCanonicalFieldCursor::new(selected.clone())
                    } else {
                        IndexedCanonicalFieldCursor::in_prost_slot_order(selected.clone())
                    }
                };
                prop_assert!(fresh().matches_initial_view(&selected, enforce_numeric_order));
                prop_assert!(!fresh().matches_initial_view(&selected, !enforce_numeric_order));
                prop_assert!(!fresh().matches_initial_view(&other, enforce_numeric_order));

                let mut wrong_message = fresh();
                wrong_message.message = other.clone();
                prop_assert!(!wrong_message.matches_initial_view(&selected, enforce_numeric_order));

                let mut wrong_reader = fresh();
                wrong_reader.reader.view = other.clone();
                prop_assert!(!wrong_reader.matches_initial_view(&selected, enforce_numeric_order));

                let mut old_field = fresh();
                old_field.last_field = 1;
                prop_assert!(!old_field.matches_initial_view(&selected, enforce_numeric_order));

                let mut terminated = fresh();
                let anchor = selected
                    .position_at(0)
                    .expect("indexed view owns its start");
                terminated.terminal_error = Some(LocatedCanonicalProtobufError {
                    error: CanonicalProtobufError::TruncatedVarint { offset: 0 },
                    reported: anchor,
                    failure: anchor,
                });
                prop_assert!(!terminated.matches_initial_view(&selected, enforce_numeric_order));

                let mut counted = fresh();
                counted.fields_read = 1;
                prop_assert!(!counted.matches_initial_view(&selected, enforce_numeric_order));

                if !bytes.is_empty() {
                    let mut advanced = fresh();
                    advanced
                        .reader
                        .read_byte()
                        .expect("nonempty view has one byte");
                    prop_assert!(!advanced.matches_initial_view(&selected, enforce_numeric_order));
                }
            }
        }
    }

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

    fn frozen_view(
        arena: &CanonicalBytesArena,
        handle: CanonicalBytesHandle,
        end: usize,
    ) -> OwnedIndexedByteRegion {
        IndexedByteRegion::from_frozen(
            arena.freeze(handle),
            CanonicalByteProvenance::new(SOURCE, end),
        )
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

    fn generated_fields(inputs: &[(u32, u8, u64, Vec<u8>)]) -> (Vec<u8>, Vec<FieldSummary>) {
        let mut bytes = Vec::new();
        let mut fields = Vec::new();
        for (number, kind, scalar, payload) in inputs {
            let start = bytes.len();
            let wire = match kind {
                0 => 0u8,
                1 => 1,
                2 => 2,
                3 => 5,
                _ => unreachable!(),
            };
            bytes.extend(encode_varint((u64::from(*number) << 3) | u64::from(wire)));
            let value = match kind {
                0 => {
                    let encoded = encode_varint(*scalar);
                    let payload_start = bytes.len();
                    bytes.extend(encoded);
                    let value = FieldValueSummary::Varint(*scalar);
                    fields.push(FieldSummary {
                        number: *number,
                        wire_type: CanonicalWireType::Varint,
                        value: value.clone(),
                        encoded_range: CanonicalByteRange {
                            start,
                            end: bytes.len(),
                        },
                        payload_range: CanonicalByteRange {
                            start: payload_start,
                            end: bytes.len(),
                        },
                    });
                    value
                }
                1 => FieldValueSummary::Fixed64(scalar.to_le_bytes().to_vec()),
                2 => FieldValueSummary::LengthDelimited(payload.clone()),
                3 => FieldValueSummary::Fixed32((*scalar as u32).to_le_bytes().to_vec()),
                _ => unreachable!(),
            };
            if *kind != 0 {
                if *kind == 2 {
                    bytes.extend(encode_varint(payload.len() as u64));
                }
                let payload_start = bytes.len();
                match &value {
                    FieldValueSummary::Fixed64(value)
                    | FieldValueSummary::LengthDelimited(value)
                    | FieldValueSummary::Fixed32(value) => bytes.extend_from_slice(value),
                    FieldValueSummary::Varint(_) => unreachable!(),
                }
                fields.push(FieldSummary {
                    number: *number,
                    wire_type: match kind {
                        1 => CanonicalWireType::Fixed64,
                        2 => CanonicalWireType::LengthDelimited,
                        3 => CanonicalWireType::Fixed32,
                        _ => unreachable!(),
                    },
                    value,
                    encoded_range: CanonicalByteRange {
                        start,
                        end: bytes.len(),
                    },
                    payload_range: CanonicalByteRange {
                        start: payload_start,
                        end: bytes.len(),
                    },
                });
            }
        }
        (bytes, fields)
    }

    fn assert_generated_field_projection(
        view: IndexedByteRegion<'_>,
        expected: &[FieldSummary],
        byte_len: usize,
        slot_order: bool,
    ) {
        let mut cursor = if slot_order {
            IndexedCanonicalFieldCursor::in_prost_slot_order(view)
        } else {
            IndexedCanonicalFieldCursor::new(view)
        };
        for expected_field in expected {
            let field = cursor.next_field().unwrap().unwrap();
            assert_eq!(
                field.field_start.logical_offset,
                expected_field.encoded_range.start
            );
            assert_eq!(
                field.field_start.physical,
                CanonicalByteProvenance::new(SOURCE, BASE + expected_field.encoded_range.start)
            );
            let value = match field.value {
                IndexedCanonicalFieldValue::Varint(value) => FieldValueSummary::Varint(value),
                IndexedCanonicalFieldValue::Fixed64(region) => {
                    FieldValueSummary::Fixed64(bytes_of(&region))
                }
                IndexedCanonicalFieldValue::LengthDelimited(region) => {
                    FieldValueSummary::LengthDelimited(bytes_of(&region))
                }
                IndexedCanonicalFieldValue::Fixed32(region) => {
                    FieldValueSummary::Fixed32(bytes_of(&region))
                }
            };
            assert_eq!(
                FieldSummary {
                    number: field.number,
                    wire_type: field.wire_type,
                    value,
                    encoded_range: field.encoded_range,
                    payload_range: field.payload_range,
                },
                expected_field.clone()
            );
        }
        assert!(cursor.next_field().unwrap().is_none());
        assert!(cursor.next_field().unwrap().is_none());
        let stats = cursor.stats();
        assert_eq!(stats.fields_read, expected.len());
        assert_eq!(stats.bytes_read, byte_len);
        assert_eq!(
            stats.bytes_read,
            stats.bytes_traversed + stats.bytes_fast_forwarded
        );
    }

    fn assert_generated_varint_error(
        view: IndexedByteRegion<'_>,
        kind: IndexedByteReadErrorKind,
        failure: usize,
        input_len: usize,
    ) {
        let read_error = view.reader().read_canonical_varint().unwrap_err();
        assert_eq!(read_error.kind, kind);
        assert_eq!(read_error.start.logical_offset, 0);
        assert_eq!(
            read_error.start.physical,
            CanonicalByteProvenance::new(SOURCE, BASE)
        );
        assert_eq!(read_error.failure.logical_offset, failure);
        assert_eq!(
            read_error.failure.physical,
            CanonicalByteProvenance::new(SOURCE, BASE + failure)
        );
        assert_eq!(
            read_error.failure.kind == IndexedBytePositionKind::End,
            matches!(&kind, IndexedByteReadErrorKind::TruncatedVarint)
        );
        let mut cursor = IndexedCanonicalFieldCursor::in_prost_slot_order(view);
        let first = cursor.next_field().unwrap_err();
        let expected = match kind {
            IndexedByteReadErrorKind::TruncatedVarint => {
                CanonicalProtobufError::TruncatedVarint { offset: 0 }
            }
            IndexedByteReadErrorKind::NonMinimalVarint => {
                CanonicalProtobufError::NonMinimalVarint { offset: 0 }
            }
            IndexedByteReadErrorKind::VarintOverflow => {
                CanonicalProtobufError::VarintOverflow { offset: 0 }
            }
            _ => unreachable!(),
        };
        assert_eq!(first.error, expected);
        assert_eq!(first.reported, read_error.start);
        assert_eq!(first.failure, read_error.failure);
        assert_eq!(cursor.next_field().unwrap_err(), first);
        assert_eq!(cursor.stats().bytes_read, input_len);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_wire_fields_preserve_progress_payloads_and_physical_positions(
            inputs in prop::collection::vec(
                (1u32..4096, 0u8..4, any::<u64>(),
                    prop::collection::vec(any::<u8>(), 0..24)),
                0..16,
            ),
        ) {
            for slot_order in [true, false] {
                let mut ordered = inputs.clone();
                if !slot_order {
                    ordered.sort_by_key(|(number, _, _, _)| *number);
                }
                let (bytes, expected) = generated_fields(&ordered);
                assert_generated_field_projection(
                    IndexedByteRegion::from_borrowed(&bytes, SOURCE, BASE),
                    &expected, bytes.len(), slot_order,
                );
                let mut arena = CanonicalBytesArena::new();
                let handle = append_bytes(&mut arena, &bytes, BASE);
                assert_generated_field_projection(
                    persistent_view(&arena, handle, BASE + bytes.len()),
                    &expected, bytes.len(), slot_order,
                );
                assert_generated_field_projection(
                    frozen_view(&arena, handle, BASE + bytes.len()),
                    &expected, bytes.len(), slot_order,
                );
            }
        }

        #[test]
        fn empty_child_payload_uses_the_backing_end_anchor(
            end_source in 72u32..128,
            end_offset in 0usize..100_000,
        ) {
            let bytes = [0x0a, 0x00];
            let mut arena = CanonicalBytesArena::new();
            let handle = append_bytes(&mut arena, &bytes, BASE);
            let end = CanonicalByteProvenance::new(end_source, end_offset);
            let view = IndexedByteRegion::from_frozen(arena.freeze(handle), end);
            let mut cursor = IndexedCanonicalFieldCursor::in_prost_slot_order(view);
            let field = cursor.next_field().unwrap().unwrap();
            let child = field.expect_length_delimited().unwrap();
            assert_eq!(child.len(), 0);
            assert_eq!(child.position_at(0).unwrap().physical, end);
            assert_eq!(child.end_position().physical, end);
            assert_eq!(field.payload_range, CanonicalByteRange { start: 2, end: 2 });
            assert!(cursor.next_field().unwrap().is_none());
            assert_eq!(cursor.stats().bytes_read, bytes.len());
        }

        #[test]
        fn generated_child_subviews_preserve_discontinuous_source_anchors(
            payload in prop::collection::vec((any::<u8>(), 1u32..500, 0usize..100_000), 0..24),
            suffix in any::<bool>(),
            end_source in 500u32..1_000,
            end_offset in 0usize..100_000,
            first_cut in any::<u8>(),
            second_cut in any::<u8>(),
        ) {
            let mut located_bytes = vec![
                CanonicalLocatedByte::new(0x0a, CanonicalByteProvenance::new(1_000, 50)),
                CanonicalLocatedByte::new(
                    payload.len() as u8,
                    CanonicalByteProvenance::new(1_001, 7),
                ),
            ];
            for &(byte, source, offset) in &payload {
                located_bytes.push(CanonicalLocatedByte::new(
                    byte,
                    CanonicalByteProvenance::new(source, offset),
                ));
            }
            if suffix {
                located_bytes.extend([
                    CanonicalLocatedByte::new(0x10, CanonicalByteProvenance::new(2_000, 2)),
                    CanonicalLocatedByte::new(0x01, CanonicalByteProvenance::new(2_001, 9)),
                ]);
            }
            let root_end = CanonicalByteProvenance::new(end_source, end_offset);
            let child_end = located_bytes
                .get(2 + payload.len())
                .map(|byte| byte.provenance)
                .unwrap_or(root_end);
            let mut arena = CanonicalBytesArena::new();
            let mut handle = arena.empty();
            for located in &located_bytes {
                handle = arena.append(handle, *located).unwrap();
            }
            let frozen = arena.freeze(handle);
            for root in [
                IndexedByteRegion::from_arena(&arena, handle, root_end),
                IndexedByteRegion::from_frozen(frozen, root_end),
            ] {
                let mut cursor = IndexedCanonicalFieldCursor::in_prost_slot_order(root);
                let field = cursor.next_field().unwrap().unwrap();
                assert_eq!(field.number, 1);
                assert_eq!(field.payload_range, CanonicalByteRange {
                    start: 2,
                    end: 2 + payload.len(),
                });
                let child = field.expect_length_delimited().unwrap();
                for (index, &(_, source, offset)) in payload.iter().enumerate() {
                    assert_eq!(child.position_at(index).unwrap().physical,
                        CanonicalByteProvenance::new(source, offset));
                }
                assert_eq!(child.end_position().physical, child_end);

                let left = usize::from(first_cut) % (payload.len() + 1);
                let right = usize::from(second_cut) % (payload.len() + 1);
                let range = CanonicalByteRange {
                    start: left.min(right),
                    end: left.max(right),
                };
                let nested = child.subview(range).unwrap();
                for index in 0..nested.len() {
                    assert_eq!(
                        nested.position_at(index).unwrap().physical,
                        child.position_at(range.start + index).unwrap().physical,
                    );
                }
                assert_eq!(nested.end_position().physical,
                    child.position_at(range.end).unwrap().physical);
                if suffix {
                    let next = cursor.next_field().unwrap().unwrap();
                    assert_eq!(next.number, 2);
                    assert_eq!(next.field_start.physical, child_end);
                }
                assert!(cursor.next_field().unwrap().is_none());
                assert_eq!(cursor.stats().bytes_read, located_bytes.len());
            }
        }

        #[test]
        fn generated_nonminimal_truncated_and_overflow_varints_keep_exact_failure_positions(
            prefix in prop::collection::vec(any::<u8>(), 1..10),
        ) {
            let continued: Vec<_> = prefix.iter().map(|byte| *byte | 0x80).collect();
            let mut nonminimal = continued.clone();
            nonminimal.push(0);
            let mut overflow: Vec<_> = continued.iter().copied().cycle().take(9).collect();
            overflow.push(2);
            for (bytes, kind, failure) in [
                (continued, IndexedByteReadErrorKind::TruncatedVarint, prefix.len()),
                (nonminimal, IndexedByteReadErrorKind::NonMinimalVarint, prefix.len()),
                (overflow, IndexedByteReadErrorKind::VarintOverflow, 9),
            ] {
                assert_generated_varint_error(
                    IndexedByteRegion::from_borrowed(&bytes, SOURCE, BASE),
                    kind.clone(), failure, bytes.len(),
                );
                let mut arena = CanonicalBytesArena::new();
                let handle = append_bytes(&mut arena, &bytes, BASE);
                assert_generated_varint_error(
                    persistent_view(&arena, handle, BASE + bytes.len()),
                    kind.clone(), failure, bytes.len(),
                );
                assert_generated_varint_error(
                    frozen_view(&arena, handle, BASE + bytes.len()),
                    kind, failure, bytes.len(),
                );
            }
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

                let indexed_frozen = frozen_view(&arena, message, BASE + 7 + bytes.len());
                let actual_frozen =
                    indexed_summary(indexed_frozen.clone(), slot_order).map(|(fields, _)| fields);
                match (&expected, actual_frozen) {
                    (Ok(expected), Ok(actual)) => {
                        assert_eq!(&actual, expected, "frozen bytes: {bytes:02x?}")
                    }
                    (Err(expected), Err(actual)) => {
                        assert_eq!(&actual.error, expected, "frozen bytes: {bytes:02x?}");
                        assert_eq!(actual.reported.physical.source, SOURCE);
                        assert_eq!(
                            actual.reported.physical.offset,
                            BASE + 7 + actual.reported.logical_offset
                        );
                        assert_eq!(actual.failure.physical.source, SOURCE);
                        assert_eq!(
                            actual.failure.physical.offset,
                            BASE + 7 + actual.failure.logical_offset
                        );
                    }
                    (expected, actual) => panic!(
                        "frozen framing disagreement for {bytes:02x?}: {expected:?} vs {actual:?}"
                    ),
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn generated_utf8_fast_path_and_indexed_fallback_agree(
            arbitrary in prop::collection::vec(any::<u8>(), 0..64),
            chars in prop::collection::vec(any::<char>(), 0..24),
        ) {
            let valid: String = chars.into_iter().collect();
            let mut late_invalid = valid.as_bytes().to_vec();
            late_invalid.push(0x80);
            for bytes in [arbitrary, valid.into_bytes(), late_invalid] {
                let borrowed = IndexedByteRegion::from_borrowed(&bytes, SOURCE, BASE);
                let mut arena = CanonicalBytesArena::new();
                let handle = append_bytes(&mut arena, &bytes, BASE);
                let indexed = persistent_view(&arena, handle, BASE + bytes.len());
                let frozen = frozen_view(&arena, handle, BASE + bytes.len());
                let contiguous_result = borrowed.validate_utf8();
                let indexed_result = indexed.validate_utf8();
                let frozen_result = frozen.validate_utf8();
                prop_assert_eq!(&contiguous_result, &indexed_result);
                prop_assert_eq!(&contiguous_result, &frozen_result);
                prop_assert_eq!(contiguous_result.is_ok(), std::str::from_utf8(&bytes).is_ok());
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
        assert!(nested.same_view(&cloned));
        assert!(!first.same_view(&nested));
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
        assert!(!nested.same_view(&contiguous));
        assert!(nested.content_eq(&contiguous));
        assert_eq!(nested.compare(&contiguous), Ordering::Equal);
    }

    #[test]
    fn frozen_indexed_views_survive_arena_mutation_and_keep_discontinuous_locations() {
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        let expected: Vec<_> = (0..193usize)
            .map(|offset| {
                CanonicalLocatedByte::new(
                    (offset % 251) as u8,
                    CanonicalByteProvenance::new((offset % 7) as u32, 50_000 + offset * 13),
                )
            })
            .collect();
        for located in &expected {
            let singleton = arena.append(arena.empty(), *located).unwrap();
            handle = arena.concat(handle, singleton).unwrap();
        }
        let frozen = arena.freeze(handle);
        let root_end = CanonicalByteProvenance::new(99, 777_777);
        let root = IndexedByteRegion::from_frozen(frozen, root_end);
        assert!(root.same_view(&root.clone()));
        assert!(!root.same_view(
            &root
                .subview(CanonicalByteRange { start: 0, end: 1 })
                .unwrap()
        ));

        for offset in 0..4_096usize {
            let _ = arena
                .append(
                    handle,
                    CanonicalLocatedByte::new(
                        (offset % 239) as u8,
                        CanonicalByteProvenance::new(55, offset),
                    ),
                )
                .unwrap();
        }
        drop(arena);

        let nested = root
            .subview(CanonicalByteRange {
                start: 17,
                end: 181,
            })
            .unwrap()
            .subview(CanonicalByteRange { start: 9, end: 151 })
            .unwrap();
        assert_eq!(nested.len(), 142);
        for (logical, located) in expected[26..168].iter().enumerate() {
            assert_eq!(nested.byte_at(logical), Some(located.byte));
            assert_eq!(
                nested.position_at(logical).unwrap().physical,
                located.provenance
            );
        }
        assert_eq!(
            bytes_of(&nested),
            expected[26..168]
                .iter()
                .map(|byte| byte.byte)
                .collect::<Vec<_>>()
        );
        assert_eq!(nested.end_position().physical, expected[168].provenance);
        assert_eq!(root.end_position().physical, root_end);
    }

    #[test]
    fn frozen_indexed_source_borrows_and_copies_page_chunks_without_flattening() {
        use rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource;

        let bytes: Vec<_> = (0..193).map(|offset| (offset % 97) as u8).collect();
        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let root = frozen_view(&arena, handle, BASE + bytes.len());

        assert_eq!(
            IndexedByteSource::contiguous_range(&root, 3..17),
            Some(&bytes[3..17])
        );
        assert_eq!(
            IndexedByteSource::contiguous_range(&root, 0..bytes.len()),
            None,
            "a multi-page range must not pretend to be contiguous"
        );
        let mut copied = [0u8; 101];
        assert!(IndexedByteSource::copy_exact(&root, 37, &mut copied));
        assert_eq!(copied, bytes[37..138]);
        assert!(!IndexedByteSource::copy_exact(&root, 100, &mut copied));

        let cloned = root.clone();
        assert!(root.content_eq(&cloned));
        assert!(IndexedByteSource::ranges_equal(&root, 1..65, 98..162));
        assert!(!IndexedByteSource::ranges_equal(&root, 0..64, 1..65));
    }

    #[test]
    fn validated_act_source_bytes_agree_across_borrowed_and_fragmented_regions() {
        use pathmap::arena_compact::ArenaCompactTree;
        use pathmap::PathMap;
        use rspace_plus_plus::rspace::pathmap_codec::{
            CanonicalCompactTrieIndexedTopologyCursor, IndexedByteSource,
        };

        let mut map = PathMap::new();
        map.set_val_at(b"alpha/first/long/compressed/line".to_vec(), 0u64);
        map.set_val_at(b"beta/second/long/compressed/line".to_vec(), 0u64);
        map.set_val_at(b"gamma/third/long/compressed/line".to_vec(), 0u64);
        let bytes = ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
            .get_data()
            .to_vec();
        assert!(bytes.len() > 64);
        let borrowed = IndexedByteRegion::from_borrowed(&bytes, SOURCE, BASE);
        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let fragmented = frozen_view(&arena, handle, BASE + bytes.len());
        assert!(IndexedByteSource::contiguous_range(&fragmented, 0..bytes.len()).is_none());

        for (index, byte) in bytes.iter().copied().enumerate() {
            assert_eq!(IndexedByteSource::byte_at(&borrowed, index), Some(byte));
            assert_eq!(IndexedByteSource::byte_at(&fragmented, index), Some(byte));
            assert_eq!(
                fragmented.position_at(index).unwrap().physical,
                CanonicalByteProvenance::new(SOURCE, BASE + index)
            );
            for end in [index + 1, (index + 17).min(bytes.len())] {
                if let Some(span) = IndexedByteSource::contiguous_range(&fragmented, index..end) {
                    assert_eq!(span, &bytes[index..end]);
                }
            }
        }
        assert_eq!(
            fragmented.end_position().physical,
            CanonicalByteProvenance::new(SOURCE, BASE + bytes.len())
        );

        let mut borrowed_cursor =
            CanonicalCompactTrieIndexedTopologyCursor::from_source(borrowed).unwrap();
        let mut fragmented_cursor =
            CanonicalCompactTrieIndexedTopologyCursor::from_source(fragmented).unwrap();
        let mut borrowed_events = Vec::new();
        let mut fragmented_events = Vec::new();
        while let Some(event) = borrowed_cursor.next_event().unwrap() {
            borrowed_events.push(event);
        }
        while let Some(event) = fragmented_cursor.next_event().unwrap() {
            fragmented_events.push(event);
        }
        assert_eq!(fragmented_events, borrowed_events);
        assert!(!fragmented_events.is_empty());
    }

    #[test]
    fn borrowed_contiguous_regions_preserve_offsets_and_all_fast_paths_without_copying() {
        use rspace_plus_plus::rspace::pathmap_codec::IndexedByteSource;

        let storage: Vec<_> = (0..257).map(|offset| (offset % 113) as u8).collect();
        let root = IndexedByteRegion::from_borrowed(&storage[17..241], 37, 9_001);
        assert!(root.same_view(&root.clone()));
        assert!(!root.same_view(&IndexedByteRegion::from_borrowed(
            &storage[17..241],
            38,
            9_001,
        )));
        let end = CanonicalByteProvenance::new(37, 9_001 + root.len());
        let composed = IndexedByteRegion::from_source(
            SourceTreeWindow::from_borrowed(&storage[17..241], 37, 9_001).unwrap(),
            end,
        );
        let independent_composed = IndexedByteRegion::from_source(
            SourceTreeWindow::from_borrowed(&storage[17..241], 37, 9_001).unwrap(),
            end,
        );
        assert!(composed.same_view(&composed.clone()));
        assert!(!composed.same_view(&independent_composed));
        assert!(composed.content_eq(&independent_composed));
        assert_eq!(root.len(), 224);
        assert_eq!(
            root.position_at(0).unwrap().physical,
            CanonicalByteProvenance::new(37, 9_001)
        );
        assert_eq!(
            root.end_position().physical,
            CanonicalByteProvenance::new(37, 9_225)
        );

        let nested = root
            .subview(CanonicalByteRange {
                start: 13,
                end: 199,
            })
            .unwrap();
        assert_eq!(nested.bytes().collect::<Vec<_>>(), storage[30..216]);
        assert_eq!(
            nested.position_at(0).unwrap().physical,
            CanonicalByteProvenance::new(37, 9_014)
        );
        assert_eq!(
            nested.end_position().physical,
            CanonicalByteProvenance::new(37, 9_200)
        );
        assert_eq!(
            IndexedByteSource::contiguous_range(&nested, 7..61),
            Some(&storage[37..91])
        );

        let mut copied = [0u8; 73];
        assert!(IndexedByteSource::copy_exact(&nested, 41, &mut copied));
        assert_eq!(copied, storage[71..144]);
        assert!(IndexedByteSource::ranges_equal(&nested, 0..32, 113..145));
        assert!(!IndexedByteSource::ranges_equal(&nested, 0..32, 1..33));

        let located = nested.located_bytes().collect::<Vec<_>>();
        assert_eq!(located.len(), nested.len());
        for (logical, byte) in located.iter().enumerate() {
            assert_eq!(byte.byte, storage[30 + logical]);
            assert_eq!(
                byte.provenance,
                CanonicalByteProvenance::new(37, 9_014 + logical)
            );
        }
    }

    #[test]
    fn contiguous_and_frozen_length_payloads_fast_forward_with_logical_accounting() {
        let mut bytes = vec![0x0a];
        bytes.extend(encode_varint(8_192));
        bytes.extend((0..8_192usize).map(|offset| (offset % 251) as u8));

        let contiguous = IndexedByteRegion::from_protobuf_region(
            ProtobufRegion::copy_from_slice(&bytes),
            SOURCE,
        );
        let mut contiguous_cursor = IndexedCanonicalFieldCursor::new(contiguous);
        assert!(contiguous_cursor.next_field().unwrap().is_some());
        let contiguous_stats = contiguous_cursor.stats();
        assert_eq!(contiguous_stats.bytes_read, bytes.len());
        assert_eq!(contiguous_stats.bytes_fast_forwarded, 8_192);
        assert_eq!(
            contiguous_stats.bytes_traversed,
            bytes.len() - contiguous_stats.bytes_fast_forwarded
        );

        let mut arena = CanonicalBytesArena::new();
        let handle = append_bytes(&mut arena, &bytes, BASE);
        let frozen = frozen_view(&arena, handle, BASE + bytes.len());
        let mut frozen_cursor = IndexedCanonicalFieldCursor::new(frozen);
        assert!(frozen_cursor.next_field().unwrap().is_some());
        assert_eq!(frozen_cursor.stats(), contiguous_stats);

        let borrowed = persistent_view(&arena, handle, BASE + bytes.len());
        let mut borrowed_cursor = IndexedCanonicalFieldCursor::new(borrowed);
        assert!(borrowed_cursor.next_field().unwrap().is_some());
        let borrowed_stats = borrowed_cursor.stats();
        assert_eq!(borrowed_stats.bytes_read, bytes.len());
        assert_eq!(borrowed_stats.bytes_traversed, bytes.len());
        assert_eq!(borrowed_stats.bytes_fast_forwarded, 0);
    }

    #[test]
    fn deeply_nested_frozen_messages_traverse_each_physical_byte_once() {
        let thread = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                const DEPTH: usize = 20_000;
                let mut lengths = Vec::with_capacity(DEPTH + 1);
                lengths.push(0usize);
                for depth in 1..=DEPTH {
                    let child = lengths[depth - 1];
                    lengths.push(1 + encode_varint(child as u64).len() + child);
                }
                let mut bytes = Vec::with_capacity(lengths[DEPTH]);
                for child_len in lengths[..DEPTH].iter().rev().copied() {
                    bytes.push(0x0a);
                    bytes.extend(encode_varint(child_len as u64));
                }
                assert_eq!(bytes.len(), lengths[DEPTH]);

                let mut arena = CanonicalBytesArena::new();
                let handle = append_bytes(&mut arena, &bytes, BASE);
                let mut body = frozen_view(&arena, handle, BASE + bytes.len());
                drop(arena);

                let mut levels = 0usize;
                let mut logical_read = 0usize;
                let mut physically_traversed = 0usize;
                let mut fast_forwarded = 0usize;
                while !body.is_empty() {
                    let mut cursor = IndexedCanonicalFieldCursor::new(body);
                    let field = cursor.next_field().unwrap().unwrap();
                    assert_eq!(field.number, 1);
                    let child = field.expect_length_delimited().unwrap().clone();
                    assert!(cursor.next_field().unwrap().is_none());
                    let stats = cursor.stats();
                    logical_read += stats.bytes_read;
                    physically_traversed += stats.bytes_traversed;
                    fast_forwarded += stats.bytes_fast_forwarded;
                    body = child;
                    levels += 1;
                }

                assert_eq!(levels, DEPTH);
                assert_eq!(physically_traversed, bytes.len());
                assert!(logical_read > bytes.len() * 1_000);
                assert_eq!(logical_read, physically_traversed + fast_forwarded);
            })
            .unwrap();
        thread.join().unwrap();
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
                    bytes_traversed: 20_000,
                    bytes_fast_forwarded: 0,
                });
            })
            .unwrap();
        thread.join().unwrap();
    }
}
