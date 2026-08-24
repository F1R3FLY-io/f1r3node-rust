//! Persistent canonical bytes with per-byte physical provenance.
//!
//! Obligation bytes arrive while an ACT trie is being walked. A branch shares
//! every byte above it, and `Leave` must restore that prefix without copying or
//! replaying an endpoint. This arena stores immutable sequence DAG nodes and
//! hands callers small [`CanonicalBytesHandle`] values that are safe to copy as
//! checkpoints.
//!
//! Ordinary append-only construction uses normalized fixed-size tail blocks.
//! Extending a partial block copies at most [`BLOCK_BYTES`] entries and never
//! points at the superseded partial block; a complete block becomes the parent
//! of the next block. Thus append copies O(1) data and, including exact-key
//! interning table maintenance, is amortized O(1) for a full sequence. It
//! allocates at most one content node and one provenance node. Appending to an
//! arbitrary slice/concatenation allocates a singleton plus one concatenation,
//! also amortized O(1). Slices are O(1) views and concatenation is one immutable
//! DAG node per stream (with amortized O(1) content interning).
//!
//! Content nodes are interned by their complete structural key. `HashMap` is
//! only an index: Rust still compares the full key after a hash collision, and
//! [`CanonicalBytesArena::content_eq`] falls back to an exact byte cursor when
//! two equal strings have different DAG shapes. No digest is ever accepted as
//! evidence of byte equality.
//!
//! There is deliberately no production flattening API. Forward byte and
//! located-byte cursors use explicit heap worklists and have no recursion or
//! depth limit.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

const BLOCK_BYTES: usize = 32;

/// Ultimate physical origin of one logical obligation byte.
///
/// `source` distinguishes independent root byte images. `offset` is an offset
/// in that image. Nested EPM/ACT validation retains the same pair while its
/// logical offsets change, so an error can always be remapped to the byte that
/// actually stored the offending edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalByteProvenance {
    pub(crate) source: u32,
    pub(crate) offset: usize,
}

impl CanonicalByteProvenance {
    pub(crate) const fn new(source: u32, offset: usize) -> Self { Self { source, offset } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalLocatedByte {
    pub(crate) byte: u8,
    pub(crate) provenance: CanonicalByteProvenance,
}

impl CanonicalLocatedByte {
    pub(crate) const fn new(byte: u8, provenance: CanonicalByteProvenance) -> Self {
        Self { byte, provenance }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ContentNodeId(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ProvenanceNodeId(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ContentSpan {
    root: ContentNodeId,
    start: usize,
    len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ProvenanceSpan {
    root: ProvenanceNodeId,
    start: usize,
    len: usize,
}

/// An immutable O(1)-size checkpoint into the content/provenance DAG.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalBytesHandle {
    content: Option<ContentSpan>,
    provenance: Option<ProvenanceSpan>,
    len: usize,
}

impl CanonicalBytesHandle {
    pub(crate) const fn len(self) -> usize { self.len }

    pub(crate) const fn is_empty(self) -> bool { self.len == 0 }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalBytesArenaError {
    LengthOverflow,
    SliceOutsideSequence {
        start: usize,
        end: usize,
        len: usize,
    },
}

impl fmt::Display for CanonicalBytesArenaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow => formatter.write_str("canonical byte sequence length overflow"),
            Self::SliceOutsideSequence { start, end, len } => write!(
                formatter,
                "canonical byte slice {start}..{end} is outside sequence length {len}"
            ),
        }
    }
}

impl std::error::Error for CanonicalBytesArenaError {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ContentNodeKind {
    Block {
        previous: Option<ContentNodeId>,
        used: u8,
        bytes: [u8; BLOCK_BYTES],
    },
    Concat {
        left: ContentSpan,
        right: ContentSpan,
    },
}

struct ContentNode {
    len: usize,
    kind: ContentNodeKind,
}

#[derive(Clone)]
enum ProvenanceNodeKind {
    Block {
        previous: Option<ProvenanceNodeId>,
        used: u8,
        values: [CanonicalByteProvenance; BLOCK_BYTES],
    },
    Concat {
        left: ProvenanceSpan,
        right: ProvenanceSpan,
    },
}

struct ProvenanceNode {
    len: usize,
    kind: ProvenanceNodeKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalBytesArenaStats {
    pub(crate) content_nodes: usize,
    pub(crate) provenance_nodes: usize,
    pub(crate) content_blocks: usize,
    pub(crate) content_concats: usize,
    pub(crate) provenance_blocks: usize,
    pub(crate) provenance_concats: usize,
}

#[derive(Clone)]
struct FrozenContentLeaf {
    node: Arc<ContentNode>,
    block_start: usize,
    len: usize,
    logical_start: usize,
}

impl FrozenContentLeaf {
    fn logical_end(&self) -> usize { self.logical_start + self.len }

    fn byte_at(&self, logical_offset: usize) -> u8 {
        let ContentNodeKind::Block { bytes, .. } = &self.node.kind else {
            unreachable!("a frozen content leaf retains a block node")
        };
        bytes[self.block_start + logical_offset - self.logical_start]
    }
}

#[derive(Clone)]
struct FrozenProvenanceLeaf {
    node: Arc<ProvenanceNode>,
    block_start: usize,
    len: usize,
    logical_start: usize,
}

impl FrozenProvenanceLeaf {
    fn logical_end(&self) -> usize { self.logical_start + self.len }

    fn provenance_at(&self, logical_offset: usize) -> CanonicalByteProvenance {
        let ProvenanceNodeKind::Block { values, .. } = &self.node.kind else {
            unreachable!("a frozen provenance leaf retains a block node")
        };
        values[self.block_start + logical_offset - self.logical_start]
    }
}

struct FrozenCanonicalBytesData {
    len: usize,
    content_leaves: Box<[FrozenContentLeaf]>,
    provenance_leaves: Box<[FrozenProvenanceLeaf]>,
    content_pages: Box<[usize]>,
    provenance_pages: Box<[usize]>,
}

/// Immutable, ownership-independent bytes frozen from one persistent handle.
///
/// Freezing copies only leaf/index metadata. Byte and provenance payload arrays
/// remain in the immutable arena nodes retained by `Arc`, so the originating
/// arena may be appended to or dropped after this value is created. A view and
/// each of its subviews are constant-size clones.
#[derive(Clone)]
pub(crate) struct FrozenCanonicalBytes {
    data: Arc<FrozenCanonicalBytesData>,
    start: usize,
    len: usize,
}

impl fmt::Debug for FrozenCanonicalBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrozenCanonicalBytes")
            .field("len", &self.len)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrozenCanonicalBytesStats {
    pub(crate) content_leaves: usize,
    pub(crate) provenance_leaves: usize,
    pub(crate) page_entries: usize,
}

impl FrozenCanonicalBytes {
    pub(crate) const fn len(&self) -> usize { self.len }

    pub(crate) const fn is_empty(&self) -> bool { self.len == 0 }

    /// Form an O(1) owned subview without copying payload or index metadata.
    pub(crate) fn subview(&self, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end > self.len {
            return None;
        }
        let start = self.start.checked_add(range.start)?;
        let end = self.start.checked_add(range.end)?;
        if end > self.data.len {
            return None;
        }
        Some(Self {
            data: Arc::clone(&self.data),
            start,
            len: range.end - range.start,
        })
    }

    /// Random byte lookup. At most `BLOCK_BYTES` leaf records are inspected.
    pub(crate) fn byte_at(&self, logical_offset: usize) -> Option<u8> {
        if logical_offset >= self.len {
            return None;
        }
        let absolute = self.start.checked_add(logical_offset)?;
        let leaf = Self::leaf_at(
            absolute,
            &self.data.content_pages,
            &self.data.content_leaves,
            FrozenContentLeaf::logical_end,
        )?;
        Some(self.data.content_leaves[leaf].byte_at(absolute))
    }

    /// Random physical-provenance lookup with the same bounded index walk.
    pub(crate) fn provenance_at(&self, logical_offset: usize) -> Option<CanonicalByteProvenance> {
        if logical_offset >= self.len {
            return None;
        }
        let absolute = self.start.checked_add(logical_offset)?;
        let leaf = Self::leaf_at(
            absolute,
            &self.data.provenance_pages,
            &self.data.provenance_leaves,
            FrozenProvenanceLeaf::logical_end,
        )?;
        Some(self.data.provenance_leaves[leaf].provenance_at(absolute))
    }

    pub(crate) fn bytes(&self) -> FrozenCanonicalByteCursor {
        FrozenCanonicalByteCursor::new(self.clone())
    }

    pub(crate) fn located_bytes(&self) -> FrozenCanonicalLocatedByteCursor {
        FrozenCanonicalLocatedByteCursor {
            bytes: FrozenCanonicalByteCursor::new(self.clone()),
            provenance: FrozenCanonicalProvenanceCursor::new(self.clone()),
        }
    }

    pub(crate) fn stats(&self) -> FrozenCanonicalBytesStats {
        FrozenCanonicalBytesStats {
            content_leaves: self.data.content_leaves.len(),
            provenance_leaves: self.data.provenance_leaves.len(),
            page_entries: self.data.content_pages.len() + self.data.provenance_pages.len(),
        }
    }

    fn leaf_at<T>(
        logical_offset: usize,
        pages: &[usize],
        leaves: &[T],
        logical_end: fn(&T) -> usize,
    ) -> Option<usize> {
        let mut leaf = *pages.get(logical_offset / BLOCK_BYTES)?;
        while logical_end(leaves.get(leaf)?) <= logical_offset {
            leaf += 1;
        }
        Some(leaf)
    }
}

/// Append-only ownership arena for persistent canonical byte strings.
pub(crate) struct CanonicalBytesArena {
    content_nodes: Vec<Arc<ContentNode>>,
    provenance_nodes: Vec<Arc<ProvenanceNode>>,
    content_intern: HashMap<ContentNodeKind, ContentNodeId>,
    stats: CanonicalBytesArenaStats,
}

impl Default for CanonicalBytesArena {
    fn default() -> Self { Self::new() }
}

impl CanonicalBytesArena {
    pub(crate) fn new() -> Self {
        Self {
            content_nodes: Vec::new(),
            provenance_nodes: Vec::new(),
            content_intern: HashMap::new(),
            stats: CanonicalBytesArenaStats::default(),
        }
    }

    pub(crate) const fn empty(&self) -> CanonicalBytesHandle {
        CanonicalBytesHandle {
            content: None,
            provenance: None,
            len: 0,
        }
    }

    pub(crate) const fn stats(&self) -> CanonicalBytesArenaStats { self.stats }

    /// Append one located byte. The returned handle does not mutate or
    /// invalidate `prefix`, so retaining `prefix` is an O(1) fork checkpoint.
    pub(crate) fn append(
        &mut self,
        prefix: CanonicalBytesHandle,
        byte: CanonicalLocatedByte,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        let len = prefix
            .len
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        if prefix.is_empty() {
            return self.singleton(byte);
        }

        let content = prefix.content.expect("a non-empty handle has content");
        let provenance = prefix
            .provenance
            .expect("a non-empty handle has provenance");
        if self.extendable_content_block(content) && self.extendable_provenance_block(provenance) {
            let content = self.extend_content_block(content, byte.byte)?;
            let provenance = self.extend_provenance_block(provenance, byte.provenance)?;
            return Ok(CanonicalBytesHandle {
                content: Some(content),
                provenance: Some(provenance),
                len,
            });
        }

        let suffix = self.singleton(byte)?;
        self.concat(prefix, suffix)
    }

    /// O(1) persistent slice. No byte or provenance entry is copied.
    pub(crate) fn slice(
        &self,
        value: CanonicalBytesHandle,
        range: Range<usize>,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        if range.start > range.end || range.end > value.len {
            return Err(CanonicalBytesArenaError::SliceOutsideSequence {
                start: range.start,
                end: range.end,
                len: value.len,
            });
        }
        let len = range.end - range.start;
        if len == 0 {
            return Ok(self.empty());
        }
        let content = value.content.expect("a non-empty slice source has content");
        let provenance = value
            .provenance
            .expect("a non-empty slice source has provenance");
        let content_start = content
            .start
            .checked_add(range.start)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        let provenance_start = provenance
            .start
            .checked_add(range.start)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        Ok(CanonicalBytesHandle {
            content: Some(ContentSpan {
                root: content.root,
                start: content_start,
                len,
            }),
            provenance: Some(ProvenanceSpan {
                root: provenance.root,
                start: provenance_start,
                len,
            }),
            len,
        })
    }

    /// O(1) structural concatenation. Both operands remain valid checkpoints.
    pub(crate) fn concat(
        &mut self,
        left: CanonicalBytesHandle,
        right: CanonicalBytesHandle,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        if left.is_empty() {
            return Ok(right);
        }
        if right.is_empty() {
            return Ok(left);
        }
        let len = left
            .len
            .checked_add(right.len)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        let left_content = left.content.expect("a non-empty left value has content");
        let right_content = right.content.expect("a non-empty right value has content");
        let content = self.intern_content(
            ContentNodeKind::Concat {
                left: left_content,
                right: right_content,
            },
            len,
        )?;
        let left_provenance = left
            .provenance
            .expect("a non-empty left value has provenance");
        let right_provenance = right
            .provenance
            .expect("a non-empty right value has provenance");
        let provenance = self.push_provenance(
            ProvenanceNodeKind::Concat {
                left: left_provenance,
                right: right_provenance,
            },
            len,
        )?;
        Ok(CanonicalBytesHandle {
            content: Some(ContentSpan {
                root: content,
                start: 0,
                len,
            }),
            provenance: Some(ProvenanceSpan {
                root: provenance,
                start: 0,
                len,
            }),
            len,
        })
    }

    /// Exact byte equality. Hashes are never equality evidence.
    pub(crate) fn content_eq(
        &self,
        left: CanonicalBytesHandle,
        right: CanonicalBytesHandle,
    ) -> bool {
        if left.len != right.len {
            return false;
        }
        if left.content == right.content {
            return true;
        }
        self.bytes(left).eq(self.bytes(right))
    }

    pub(crate) fn compare(
        &self,
        left: CanonicalBytesHandle,
        right: CanonicalBytesHandle,
    ) -> Ordering {
        self.bytes(left).cmp(self.bytes(right))
    }

    pub(crate) fn bytes(&self, value: CanonicalBytesHandle) -> CanonicalByteCursor<'_> {
        CanonicalByteCursor::new(self, value.content)
    }

    /// Compatibility random lookup for a live borrowed arena. Frozen views
    /// provide the bounded indexed implementation used by production parsers.
    pub(crate) fn byte_at(&self, value: CanonicalBytesHandle, logical_offset: usize) -> Option<u8> {
        if logical_offset >= value.len {
            return None;
        }
        let one = self.slice(value, logical_offset..logical_offset + 1).ok()?;
        self.bytes(one).next()
    }

    pub(crate) fn located_bytes(
        &self,
        value: CanonicalBytesHandle,
    ) -> CanonicalLocatedByteCursor<'_> {
        CanonicalLocatedByteCursor {
            bytes: CanonicalByteCursor::new(self, value.content),
            provenance: CanonicalProvenanceCursor::new(self, value.provenance),
        }
    }

    /// Freeze one handle into ownership-independent indexed metadata.
    ///
    /// The retained block nodes own the original fixed-size payload arrays.
    /// Only one leaf record per intersected block fragment and one index entry
    /// per fixed-size logical page are allocated here.
    pub(crate) fn freeze(&self, value: CanonicalBytesHandle) -> FrozenCanonicalBytes {
        let content_leaves = self.freeze_content_leaves(value.content);
        let provenance_leaves = self.freeze_provenance_leaves(value.provenance);
        debug_assert_eq!(
            content_leaves
                .last()
                .map_or(0, FrozenContentLeaf::logical_end),
            value.len
        );
        debug_assert_eq!(
            provenance_leaves
                .last()
                .map_or(0, FrozenProvenanceLeaf::logical_end),
            value.len
        );
        let content_pages =
            Self::freeze_page_index(value.len, &content_leaves, FrozenContentLeaf::logical_end);
        let provenance_pages = Self::freeze_page_index(
            value.len,
            &provenance_leaves,
            FrozenProvenanceLeaf::logical_end,
        );
        FrozenCanonicalBytes {
            data: Arc::new(FrozenCanonicalBytesData {
                len: value.len,
                content_leaves: content_leaves.into_boxed_slice(),
                provenance_leaves: provenance_leaves.into_boxed_slice(),
                content_pages: content_pages.into_boxed_slice(),
                provenance_pages: provenance_pages.into_boxed_slice(),
            }),
            start: 0,
            len: value.len,
        }
    }

    fn freeze_content_leaves(&self, root: Option<ContentSpan>) -> Vec<FrozenContentLeaf> {
        let mut work: Vec<_> = root.into_iter().map(ContentCursorTask::Span).collect();
        let mut leaves = Vec::new();
        let mut logical_start = 0usize;
        while let Some(task) = work.pop() {
            match task {
                ContentCursorTask::Span(span) => {
                    expand_content_span(&self.content_nodes, &mut work, span)
                }
                ContentCursorTask::Block { root, at, end } => {
                    let len = end - at;
                    leaves.push(FrozenContentLeaf {
                        node: Arc::clone(&self.content_nodes[root.0]),
                        block_start: at,
                        len,
                        logical_start,
                    });
                    logical_start += len;
                }
            }
        }
        leaves
    }

    fn freeze_provenance_leaves(&self, root: Option<ProvenanceSpan>) -> Vec<FrozenProvenanceLeaf> {
        let mut work: Vec<_> = root.into_iter().map(ProvenanceCursorTask::Span).collect();
        let mut leaves = Vec::new();
        let mut logical_start = 0usize;
        while let Some(task) = work.pop() {
            match task {
                ProvenanceCursorTask::Span(span) => {
                    expand_provenance_span(&self.provenance_nodes, &mut work, span)
                }
                ProvenanceCursorTask::Block { root, at, end } => {
                    let len = end - at;
                    leaves.push(FrozenProvenanceLeaf {
                        node: Arc::clone(&self.provenance_nodes[root.0]),
                        block_start: at,
                        len,
                        logical_start,
                    });
                    logical_start += len;
                }
            }
        }
        leaves
    }

    fn freeze_page_index<T>(len: usize, leaves: &[T], logical_end: fn(&T) -> usize) -> Vec<usize> {
        let page_count = len / BLOCK_BYTES + usize::from(len % BLOCK_BYTES != 0);
        let mut pages = Vec::with_capacity(page_count);
        let mut leaf = 0usize;
        for page in 0..page_count {
            let logical_start = page * BLOCK_BYTES;
            while logical_end(&leaves[leaf]) <= logical_start {
                leaf += 1;
            }
            pages.push(leaf);
        }
        pages
    }

    pub(crate) fn provenance_at(
        &self,
        value: CanonicalBytesHandle,
        logical_offset: usize,
    ) -> Option<CanonicalByteProvenance> {
        if logical_offset >= value.len {
            return None;
        }
        let one = self.slice(value, logical_offset..logical_offset + 1).ok()?;
        self.located_bytes(one).next().map(|byte| byte.provenance)
    }

    fn singleton(
        &mut self,
        byte: CanonicalLocatedByte,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        let mut bytes = [0; BLOCK_BYTES];
        bytes[0] = byte.byte;
        let content = self.intern_content(
            ContentNodeKind::Block {
                previous: None,
                used: 1,
                bytes,
            },
            1,
        )?;
        let mut values = [CanonicalByteProvenance::default(); BLOCK_BYTES];
        values[0] = byte.provenance;
        let provenance = self.push_provenance(
            ProvenanceNodeKind::Block {
                previous: None,
                used: 1,
                values,
            },
            1,
        )?;
        Ok(CanonicalBytesHandle {
            content: Some(ContentSpan {
                root: content,
                start: 0,
                len: 1,
            }),
            provenance: Some(ProvenanceSpan {
                root: provenance,
                start: 0,
                len: 1,
            }),
            len: 1,
        })
    }

    fn extendable_content_block(&self, span: ContentSpan) -> bool {
        span.start == 0
            && span.len == self.content_nodes[span.root.0].len
            && matches!(
                self.content_nodes[span.root.0].kind,
                ContentNodeKind::Block { .. }
            )
    }

    fn extendable_provenance_block(&self, span: ProvenanceSpan) -> bool {
        span.start == 0
            && span.len == self.provenance_nodes[span.root.0].len
            && matches!(
                self.provenance_nodes[span.root.0].kind,
                ProvenanceNodeKind::Block { .. }
            )
    }

    fn extend_content_block(
        &mut self,
        span: ContentSpan,
        byte: u8,
    ) -> Result<ContentSpan, CanonicalBytesArenaError> {
        let (previous, used, mut bytes) = match &self.content_nodes[span.root.0].kind {
            ContentNodeKind::Block {
                previous,
                used,
                bytes,
            } => (*previous, usize::from(*used), *bytes),
            ContentNodeKind::Concat { .. } => unreachable!("extendability checks block kind"),
        };
        let (previous, used, bytes) = if used == BLOCK_BYTES {
            let mut next = [0; BLOCK_BYTES];
            next[0] = byte;
            (Some(span.root), 1usize, next)
        } else {
            bytes[used] = byte;
            (previous, used + 1, bytes)
        };
        let len = span
            .len
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        let root = self.intern_content(
            ContentNodeKind::Block {
                previous,
                used: used as u8,
                bytes,
            },
            len,
        )?;
        Ok(ContentSpan {
            root,
            start: 0,
            len,
        })
    }

    fn extend_provenance_block(
        &mut self,
        span: ProvenanceSpan,
        provenance: CanonicalByteProvenance,
    ) -> Result<ProvenanceSpan, CanonicalBytesArenaError> {
        let (previous, used, mut values) = match &self.provenance_nodes[span.root.0].kind {
            ProvenanceNodeKind::Block {
                previous,
                used,
                values,
            } => (*previous, usize::from(*used), *values),
            ProvenanceNodeKind::Concat { .. } => {
                unreachable!("extendability checks provenance block kind")
            }
        };
        let (previous, used, values) = if used == BLOCK_BYTES {
            let mut next = [CanonicalByteProvenance::default(); BLOCK_BYTES];
            next[0] = provenance;
            (Some(span.root), 1usize, next)
        } else {
            values[used] = provenance;
            (previous, used + 1, values)
        };
        let len = span
            .len
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        let root = self.push_provenance(
            ProvenanceNodeKind::Block {
                previous,
                used: used as u8,
                values,
            },
            len,
        )?;
        Ok(ProvenanceSpan {
            root,
            start: 0,
            len,
        })
    }

    fn intern_content(
        &mut self,
        kind: ContentNodeKind,
        len: usize,
    ) -> Result<ContentNodeId, CanonicalBytesArenaError> {
        if let Some(id) = self.content_intern.get(&kind) {
            debug_assert_eq!(self.content_nodes[id.0].len, len);
            return Ok(*id);
        }
        self.content_nodes
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        self.content_intern
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        let id = ContentNodeId(self.content_nodes.len());
        match &kind {
            ContentNodeKind::Block { .. } => self.stats.content_blocks += 1,
            ContentNodeKind::Concat { .. } => self.stats.content_concats += 1,
        }
        self.content_nodes.push(Arc::new(ContentNode {
            len,
            kind: kind.clone(),
        }));
        self.content_intern.insert(kind, id);
        self.stats.content_nodes = self.content_nodes.len();
        Ok(id)
    }

    fn push_provenance(
        &mut self,
        kind: ProvenanceNodeKind,
        len: usize,
    ) -> Result<ProvenanceNodeId, CanonicalBytesArenaError> {
        self.provenance_nodes
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        let id = ProvenanceNodeId(self.provenance_nodes.len());
        match &kind {
            ProvenanceNodeKind::Block { .. } => self.stats.provenance_blocks += 1,
            ProvenanceNodeKind::Concat { .. } => self.stats.provenance_concats += 1,
        }
        self.provenance_nodes
            .push(Arc::new(ProvenanceNode { len, kind }));
        self.stats.provenance_nodes = self.provenance_nodes.len();
        Ok(id)
    }
}

#[derive(Clone, Copy)]
enum ContentCursorTask {
    Span(ContentSpan),
    Block {
        root: ContentNodeId,
        at: usize,
        end: usize,
    },
}

fn expand_content_span(
    nodes: &[Arc<ContentNode>],
    work: &mut Vec<ContentCursorTask>,
    span: ContentSpan,
) {
    let node = &nodes[span.root.0];
    let end = span.start + span.len;
    match &node.kind {
        ContentNodeKind::Block { previous, used, .. } => {
            let previous_len = previous.map_or(0, |id| nodes[id.0].len);
            if end > previous_len {
                work.push(ContentCursorTask::Block {
                    root: span.root,
                    at: span.start.max(previous_len) - previous_len,
                    end: (end - previous_len).min(usize::from(*used)),
                });
            }
            if span.start < previous_len {
                work.push(ContentCursorTask::Span(ContentSpan {
                    root: previous.expect("a positive previous range has a previous block"),
                    start: span.start,
                    len: end.min(previous_len) - span.start,
                }));
            }
        }
        ContentNodeKind::Concat { left, right } => {
            if end > left.len {
                work.push(ContentCursorTask::Span(ContentSpan {
                    root: right.root,
                    start: right.start + span.start.saturating_sub(left.len),
                    len: end.min(left.len + right.len) - span.start.max(left.len),
                }));
            }
            if span.start < left.len {
                work.push(ContentCursorTask::Span(ContentSpan {
                    root: left.root,
                    start: left.start + span.start,
                    len: end.min(left.len) - span.start,
                }));
            }
        }
    }
}

pub(crate) struct CanonicalByteCursor<'a> {
    arena: &'a CanonicalBytesArena,
    work: Vec<ContentCursorTask>,
}

impl<'a> CanonicalByteCursor<'a> {
    fn new(arena: &'a CanonicalBytesArena, root: Option<ContentSpan>) -> Self {
        Self {
            arena,
            work: root.into_iter().map(ContentCursorTask::Span).collect(),
        }
    }

    fn expand(&mut self, span: ContentSpan) {
        expand_content_span(&self.arena.content_nodes, &mut self.work, span);
    }
}

impl Iterator for CanonicalByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                ContentCursorTask::Span(span) => self.expand(span),
                ContentCursorTask::Block { root, at, end } => {
                    let ContentNodeKind::Block { bytes, .. } =
                        &self.arena.content_nodes[root.0].kind
                    else {
                        unreachable!("a block cursor points at a block node")
                    };
                    let byte = bytes[at];
                    if at + 1 < end {
                        self.work.push(ContentCursorTask::Block {
                            root,
                            at: at + 1,
                            end,
                        });
                    }
                    return Some(byte);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ProvenanceCursorTask {
    Span(ProvenanceSpan),
    Block {
        root: ProvenanceNodeId,
        at: usize,
        end: usize,
    },
}

fn expand_provenance_span(
    nodes: &[Arc<ProvenanceNode>],
    work: &mut Vec<ProvenanceCursorTask>,
    span: ProvenanceSpan,
) {
    let node = &nodes[span.root.0];
    let end = span.start + span.len;
    match &node.kind {
        ProvenanceNodeKind::Block { previous, used, .. } => {
            let previous_len = previous.map_or(0, |id| nodes[id.0].len);
            if end > previous_len {
                work.push(ProvenanceCursorTask::Block {
                    root: span.root,
                    at: span.start.max(previous_len) - previous_len,
                    end: (end - previous_len).min(usize::from(*used)),
                });
            }
            if span.start < previous_len {
                work.push(ProvenanceCursorTask::Span(ProvenanceSpan {
                    root: previous.expect("a positive previous range has a previous block"),
                    start: span.start,
                    len: end.min(previous_len) - span.start,
                }));
            }
        }
        ProvenanceNodeKind::Concat { left, right } => {
            if end > left.len {
                work.push(ProvenanceCursorTask::Span(ProvenanceSpan {
                    root: right.root,
                    start: right.start + span.start.saturating_sub(left.len),
                    len: end.min(left.len + right.len) - span.start.max(left.len),
                }));
            }
            if span.start < left.len {
                work.push(ProvenanceCursorTask::Span(ProvenanceSpan {
                    root: left.root,
                    start: left.start + span.start,
                    len: end.min(left.len) - span.start,
                }));
            }
        }
    }
}

struct CanonicalProvenanceCursor<'a> {
    arena: &'a CanonicalBytesArena,
    work: Vec<ProvenanceCursorTask>,
}

impl<'a> CanonicalProvenanceCursor<'a> {
    fn new(arena: &'a CanonicalBytesArena, root: Option<ProvenanceSpan>) -> Self {
        Self {
            arena,
            work: root.into_iter().map(ProvenanceCursorTask::Span).collect(),
        }
    }

    fn expand(&mut self, span: ProvenanceSpan) {
        expand_provenance_span(&self.arena.provenance_nodes, &mut self.work, span);
    }
}

impl Iterator for CanonicalProvenanceCursor<'_> {
    type Item = CanonicalByteProvenance;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                ProvenanceCursorTask::Span(span) => self.expand(span),
                ProvenanceCursorTask::Block { root, at, end } => {
                    let ProvenanceNodeKind::Block { values, .. } =
                        &self.arena.provenance_nodes[root.0].kind
                    else {
                        unreachable!("a provenance block cursor points at a block node")
                    };
                    let provenance = values[at];
                    if at + 1 < end {
                        self.work.push(ProvenanceCursorTask::Block {
                            root,
                            at: at + 1,
                            end,
                        });
                    }
                    return Some(provenance);
                }
            }
        }
    }
}

pub(crate) struct CanonicalLocatedByteCursor<'a> {
    bytes: CanonicalByteCursor<'a>,
    provenance: CanonicalProvenanceCursor<'a>,
}

impl Iterator for CanonicalLocatedByteCursor<'_> {
    type Item = CanonicalLocatedByte;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = self.bytes.next();
        let provenance = self.provenance.next();
        debug_assert_eq!(byte.is_some(), provenance.is_some());
        byte.zip(provenance)
            .map(|(byte, provenance)| CanonicalLocatedByte { byte, provenance })
    }
}

/// Lifetime-free forward cursor over frozen byte payloads.
#[derive(Clone)]
pub(crate) struct FrozenCanonicalByteCursor {
    value: FrozenCanonicalBytes,
    leaf: usize,
    absolute: usize,
    end: usize,
}

impl FrozenCanonicalByteCursor {
    fn new(value: FrozenCanonicalBytes) -> Self {
        let absolute = value.start;
        let end = absolute + value.len;
        let leaf = if value.is_empty() {
            value.data.content_leaves.len()
        } else {
            FrozenCanonicalBytes::leaf_at(
                absolute,
                &value.data.content_pages,
                &value.data.content_leaves,
                FrozenContentLeaf::logical_end,
            )
            .expect("a non-empty frozen view starts in one content leaf")
        };
        Self {
            value,
            leaf,
            absolute,
            end,
        }
    }

    fn fast_forward(&mut self, width: usize) {
        let absolute = self
            .absolute
            .checked_add(width)
            .expect("a checked frozen cursor seek cannot overflow");
        assert!(
            absolute <= self.end,
            "a frozen cursor seek stays inside its view"
        );
        self.absolute = absolute;
        self.leaf = if absolute == self.end {
            self.value.data.content_leaves.len()
        } else {
            FrozenCanonicalBytes::leaf_at(
                absolute,
                &self.value.data.content_pages,
                &self.value.data.content_leaves,
                FrozenContentLeaf::logical_end,
            )
            .expect("a frozen cursor seek lands in one content leaf")
        };
    }
}

impl Iterator for FrozenCanonicalByteCursor {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.absolute == self.end {
            return None;
        }
        while self.value.data.content_leaves[self.leaf].logical_end() <= self.absolute {
            self.leaf += 1;
        }
        let byte = self.value.data.content_leaves[self.leaf].byte_at(self.absolute);
        self.absolute += 1;
        Some(byte)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end - self.absolute;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for FrozenCanonicalByteCursor {}

#[derive(Clone)]
struct FrozenCanonicalProvenanceCursor {
    value: FrozenCanonicalBytes,
    leaf: usize,
    absolute: usize,
    end: usize,
}

impl FrozenCanonicalProvenanceCursor {
    fn new(value: FrozenCanonicalBytes) -> Self {
        let absolute = value.start;
        let end = absolute + value.len;
        let leaf = if value.is_empty() {
            value.data.provenance_leaves.len()
        } else {
            FrozenCanonicalBytes::leaf_at(
                absolute,
                &value.data.provenance_pages,
                &value.data.provenance_leaves,
                FrozenProvenanceLeaf::logical_end,
            )
            .expect("a non-empty frozen view starts in one provenance leaf")
        };
        Self {
            value,
            leaf,
            absolute,
            end,
        }
    }

    fn fast_forward(&mut self, width: usize) {
        let absolute = self
            .absolute
            .checked_add(width)
            .expect("a checked frozen provenance seek cannot overflow");
        assert!(
            absolute <= self.end,
            "a frozen provenance seek stays inside its view"
        );
        self.absolute = absolute;
        self.leaf = if absolute == self.end {
            self.value.data.provenance_leaves.len()
        } else {
            FrozenCanonicalBytes::leaf_at(
                absolute,
                &self.value.data.provenance_pages,
                &self.value.data.provenance_leaves,
                FrozenProvenanceLeaf::logical_end,
            )
            .expect("a frozen provenance seek lands in one leaf")
        };
    }
}

impl Iterator for FrozenCanonicalProvenanceCursor {
    type Item = CanonicalByteProvenance;

    fn next(&mut self) -> Option<Self::Item> {
        if self.absolute == self.end {
            return None;
        }
        while self.value.data.provenance_leaves[self.leaf].logical_end() <= self.absolute {
            self.leaf += 1;
        }
        let provenance = self.value.data.provenance_leaves[self.leaf].provenance_at(self.absolute);
        self.absolute += 1;
        Some(provenance)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end - self.absolute;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for FrozenCanonicalProvenanceCursor {}

/// Lifetime-free forward cursor over frozen bytes and their exact provenance.
#[derive(Clone)]
pub(crate) struct FrozenCanonicalLocatedByteCursor {
    bytes: FrozenCanonicalByteCursor,
    provenance: FrozenCanonicalProvenanceCursor,
}

impl FrozenCanonicalLocatedByteCursor {
    pub(crate) fn fast_forward(&mut self, width: usize) {
        self.bytes.fast_forward(width);
        self.provenance.fast_forward(width);
    }
}

impl Iterator for FrozenCanonicalLocatedByteCursor {
    type Item = CanonicalLocatedByte;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = self.bytes.next();
        let provenance = self.provenance.next();
        debug_assert_eq!(byte.is_some(), provenance.is_some());
        byte.zip(provenance)
            .map(|(byte, provenance)| CanonicalLocatedByte { byte, provenance })
    }

    fn size_hint(&self) -> (usize, Option<usize>) { self.bytes.size_hint() }
}

impl ExactSizeIterator for FrozenCanonicalLocatedByteCursor {}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(byte: u8, source: u32, offset: usize) -> CanonicalLocatedByte {
        CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(source, offset))
    }

    fn append_bytes(
        arena: &mut CanonicalBytesArena,
        bytes: &[u8],
        source: u32,
        offset: usize,
    ) -> CanonicalBytesHandle {
        let mut value = arena.empty();
        for (index, byte) in bytes.iter().copied().enumerate() {
            value = arena
                .append(value, located(byte, source, offset + index))
                .unwrap();
        }
        value
    }

    fn to_vec(arena: &CanonicalBytesArena, value: CanonicalBytesHandle) -> Vec<u8> {
        arena.bytes(value).collect()
    }

    fn located_vec(
        arena: &CanonicalBytesArena,
        value: CanonicalBytesHandle,
    ) -> Vec<CanonicalLocatedByte> {
        arena.located_bytes(value).collect()
    }

    #[test]
    fn forked_suffixes_share_the_prefix_and_restore_by_handle() {
        let mut arena = CanonicalBytesArena::new();
        let prefix_bytes: Vec<u8> = (0..=255).cycle().take(4096).collect();
        let prefix = append_bytes(&mut arena, &prefix_bytes, 1, 10_000);
        let before = arena.stats();

        let mut branches = Vec::new();
        for branch in 0..128u32 {
            let checkpoint = prefix;
            let mut value = checkpoint;
            for suffix in 0..8usize {
                value = arena
                    .append(
                        value,
                        located(
                            (branch as u8).wrapping_add(suffix as u8),
                            2 + branch,
                            suffix,
                        ),
                    )
                    .unwrap();
            }
            assert_eq!(&to_vec(&arena, value)[..prefix.len()], prefix_bytes);
            branches.push(value);
            assert_eq!(to_vec(&arena, checkpoint), prefix_bytes);
        }

        let delta = CanonicalBytesArenaStats {
            content_nodes: arena.stats().content_nodes - before.content_nodes,
            provenance_nodes: arena.stats().provenance_nodes - before.provenance_nodes,
            content_blocks: arena.stats().content_blocks - before.content_blocks,
            content_concats: arena.stats().content_concats - before.content_concats,
            provenance_blocks: arena.stats().provenance_blocks - before.provenance_blocks,
            provenance_concats: arena.stats().provenance_concats - before.provenance_concats,
        };
        let suffix_bytes = branches.len() * 8;
        assert!(delta.content_nodes <= suffix_bytes);
        assert_eq!(delta.provenance_nodes, suffix_bytes);
    }

    #[test]
    fn equal_content_with_different_provenance_reuses_content_but_not_sources() {
        let mut arena = CanonicalBytesArena::new();
        let first = append_bytes(&mut arena, b"canonical", 7, 100);
        let content_after_first = arena.stats().content_nodes;
        let second = append_bytes(&mut arena, b"canonical", 9, 900);

        assert!(arena.content_eq(first, second));
        assert_eq!(arena.compare(first, second), Ordering::Equal);
        assert_eq!(arena.stats().content_nodes, content_after_first);
        assert_eq!(
            arena.provenance_at(first, 3),
            Some(CanonicalByteProvenance::new(7, 103))
        );
        assert_eq!(
            arena.provenance_at(second, 3),
            Some(CanonicalByteProvenance::new(9, 903))
        );
    }

    #[test]
    fn slicing_and_concat_preserve_bytes_and_each_leafs_provenance() {
        let mut arena = CanonicalBytesArena::new();
        let alphabet = append_bytes(&mut arena, b"abcdef", 3, 40);
        let middle = arena.slice(alphabet, 1..5).unwrap();
        assert_eq!(to_vec(&arena, middle), b"bcde");
        assert_eq!(arena.provenance_at(middle, 0).unwrap().offset, 41);
        assert_eq!(arena.provenance_at(middle, 3).unwrap().offset, 44);

        let left = arena.slice(alphabet, 0..2).unwrap();
        let right = arena.slice(alphabet, 4..6).unwrap();
        let joined = arena.concat(left, right).unwrap();
        assert_eq!(to_vec(&arena, joined), b"abef");
        let locations: Vec<_> = located_vec(&arena, joined)
            .into_iter()
            .map(|byte| byte.provenance.offset)
            .collect();
        assert_eq!(locations, [40, 41, 44, 45]);

        let independently_built = append_bytes(&mut arena, b"abef", 99, 0);
        assert!(arena.content_eq(joined, independently_built));
    }

    #[test]
    fn slices_cross_block_boundaries_and_remain_appendable() {
        let mut arena = CanonicalBytesArena::new();
        let original: Vec<u8> = (0..96).collect();
        let value = append_bytes(&mut arena, &original, 8, 1_000);
        let across_blocks = arena.slice(value, 17..81).unwrap();
        let nested = arena.slice(across_blocks, 11..50).unwrap();

        assert_eq!(to_vec(&arena, across_blocks), original[17..81]);
        assert_eq!(to_vec(&arena, nested), original[28..67]);
        assert_eq!(
            arena.provenance_at(nested, 0),
            Some(CanonicalByteProvenance::new(8, 1_028))
        );
        assert_eq!(
            arena.provenance_at(nested, nested.len() - 1),
            Some(CanonicalByteProvenance::new(8, 1_066))
        );

        let extended = arena.append(nested, located(0xff, 12, 77)).unwrap();
        assert_eq!(&to_vec(&arena, extended)[..nested.len()], &original[28..67]);
        assert_eq!(to_vec(&arena, extended)[nested.len()], 0xff);
        assert_eq!(
            arena.provenance_at(extended, nested.len()),
            Some(CanonicalByteProvenance::new(12, 77))
        );
        assert_eq!(to_vec(&arena, nested), original[28..67]);
    }

    #[test]
    fn lexicographic_compare_uses_bytes_not_provenance_or_shape() {
        let mut arena = CanonicalBytesArena::new();
        let ab = append_bytes(&mut arena, b"ab", 1, 0);
        let abc = append_bytes(&mut arena, b"abc", 2, 0);
        let abd = append_bytes(&mut arena, b"abd", 3, 0);
        let a = arena.slice(abc, 0..1).unwrap();
        let bc = arena.slice(abc, 1..3).unwrap();
        let joined = arena.concat(a, bc).unwrap();

        assert_eq!(arena.compare(ab, abc), Ordering::Less);
        assert_eq!(arena.compare(abd, abc), Ordering::Greater);
        assert_eq!(arena.compare(joined, abc), Ordering::Equal);
        assert!(arena.content_eq(joined, abc));
    }

    #[test]
    fn deeply_concatenated_cursor_is_heap_total_on_a_tiny_stack() {
        let thread = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut arena = CanonicalBytesArena::new();
                let mut value = arena.empty();
                for offset in 0..20_000usize {
                    let singleton = arena
                        .append(arena.empty(), located((offset % 251) as u8, 4, offset))
                        .unwrap();
                    value = arena.concat(value, singleton).unwrap();
                }
                assert_eq!(arena.bytes(value).count(), 20_000);
                assert_eq!(arena.located_bytes(value).count(), 20_000);
                assert_eq!(arena.provenance_at(value, 19_999).unwrap().offset, 19_999);
            })
            .unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn allocation_growth_is_linear_in_distinct_appends_and_concats() {
        fn measure(bytes: usize) -> CanonicalBytesArenaStats {
            let mut arena = CanonicalBytesArena::new();
            let mut appended = arena.empty();
            for offset in 0..bytes {
                appended = arena
                    .append(appended, located((offset % 251) as u8, 1, offset))
                    .unwrap();
            }
            let checkpoint = appended;
            for branch in 0..64usize {
                let _ = arena
                    .append(checkpoint, located(branch as u8, 2, branch))
                    .unwrap();
            }
            let left = arena.slice(appended, 0..bytes / 2).unwrap();
            let right = arena.slice(appended, bytes / 2..bytes).unwrap();
            let before_concat = arena.stats();
            let _ = arena.concat(left, right).unwrap();
            let after_concat = arena.stats();
            assert!(after_concat.content_nodes - before_concat.content_nodes <= 1);
            assert_eq!(
                after_concat.provenance_nodes - before_concat.provenance_nodes,
                1
            );
            after_concat
        }

        let small = measure(4_000);
        let large = measure(8_000);
        assert!(small.content_nodes <= 4_000 + 64 + 1);
        assert!(small.provenance_nodes <= 4_000 + 64 + 1);
        assert!(large.content_nodes <= 8_000 + 64 + 1);
        assert!(large.provenance_nodes <= 8_000 + 64 + 1);
        assert!(large.content_nodes <= small.content_nodes * 2 + 2);
        assert!(large.provenance_nodes <= small.provenance_nodes * 2 + 2);
    }

    #[test]
    fn frozen_bytes_survive_origin_mutation_and_drop_without_payload_copying() {
        let mut arena = CanonicalBytesArena::new();
        let bytes: Vec<u8> = (0..160).map(|offset| (offset % 251) as u8).collect();
        let value = append_bytes(&mut arena, &bytes, 11, 1_000);
        let before = arena.stats();
        let frozen = arena.freeze(value);
        assert_eq!(
            arena.stats(),
            before,
            "freezing does not add arena payload nodes"
        );

        let mut extended = value;
        for offset in 0..4_096usize {
            extended = arena
                .append(extended, located((offset % 239) as u8, 12, 9_000 + offset))
                .unwrap();
        }
        assert_eq!(frozen.bytes().collect::<Vec<_>>(), bytes);
        for offset in 0..bytes.len() {
            assert_eq!(frozen.byte_at(offset), Some(bytes[offset]));
            assert_eq!(
                frozen.provenance_at(offset),
                Some(CanonicalByteProvenance::new(11, 1_000 + offset))
            );
        }

        drop(arena);
        assert_eq!(frozen.bytes().collect::<Vec<_>>(), bytes);
        assert_eq!(frozen.byte_at(bytes.len()), None);
        assert_eq!(frozen.provenance_at(bytes.len()), None);
    }

    #[test]
    fn frozen_subviews_keep_discontinuous_provenance_and_bounded_random_lookup() {
        let mut arena = CanonicalBytesArena::new();
        let mut value = arena.empty();
        let mut expected = Vec::new();
        for offset in 0..257usize {
            let byte = (offset % 251) as u8;
            let provenance = CanonicalByteProvenance::new((offset % 5) as u32, offset * 17 + 3);
            let singleton = arena
                .append(arena.empty(), CanonicalLocatedByte::new(byte, provenance))
                .unwrap();
            value = arena.concat(value, singleton).unwrap();
            expected.push(CanonicalLocatedByte::new(byte, provenance));
        }
        let frozen = arena.freeze(value);
        let subview = frozen.subview(19..241).unwrap().subview(7..199).unwrap();
        let expected = &expected[26..218];

        assert_eq!(subview.len(), expected.len());
        for (offset, located) in expected.iter().enumerate() {
            assert_eq!(subview.byte_at(offset), Some(located.byte));
            assert_eq!(subview.provenance_at(offset), Some(located.provenance));
        }
        assert_eq!(subview.located_bytes().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn frozen_metadata_and_cursors_are_linear_and_tiny_stack_safe() {
        let thread = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                const LEN: usize = 20_000;
                let mut arena = CanonicalBytesArena::new();
                let mut value = arena.empty();
                for offset in 0..LEN {
                    let singleton = arena
                        .append(arena.empty(), located((offset % 251) as u8, 21, offset * 2))
                        .unwrap();
                    value = arena.concat(value, singleton).unwrap();
                }

                let frozen = arena.freeze(value);
                let stats = frozen.stats();
                assert!(stats.content_leaves <= LEN);
                assert!(stats.provenance_leaves <= LEN);
                assert_eq!(
                    stats.page_entries,
                    2 * (LEN / BLOCK_BYTES + usize::from(LEN % BLOCK_BYTES != 0))
                );
                assert!(
                    stats.content_leaves + stats.provenance_leaves + stats.page_entries
                        <= 2 * LEN + 2 * (LEN / BLOCK_BYTES + 1)
                );
                assert_eq!(frozen.bytes().count(), LEN);
                assert_eq!(frozen.located_bytes().count(), LEN);
                for offset in (0..LEN).step_by(97) {
                    assert_eq!(frozen.byte_at(offset), Some((offset % 251) as u8));
                    assert_eq!(frozen.provenance_at(offset).unwrap().offset, offset * 2);
                }
            })
            .unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn invalid_slice_is_reported_without_mutating_the_arena() {
        let mut arena = CanonicalBytesArena::new();
        let value = append_bytes(&mut arena, b"abc", 0, 0);
        let before = arena.stats();
        assert_eq!(
            arena.slice(value, 2..4).unwrap_err(),
            CanonicalBytesArenaError::SliceOutsideSequence {
                start: 2,
                end: 4,
                len: 3,
            }
        );
        assert_eq!(arena.stats(), before);
    }
}
