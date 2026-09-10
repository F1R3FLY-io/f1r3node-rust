//! Persistent canonical bytes with per-byte physical provenance.
//!
//! Obligation bytes arrive while an ACT trie is being walked. A branch shares
//! every byte above it, and `Leave` must restore that prefix without copying or
//! replaying an endpoint. Constant-size [`CanonicalBytesHandle`] spans point
//! into an append-log rope. Descriptor nodes are arena-owned while byte and
//! provenance payloads occupy separate, fixed-capacity `Arc` pages.
//!
//! A linear append mutates a uniquely owned page only after every byte visible
//! to existing handles, so checkpoints remain immutable without copying a
//! partial page. A divergent append, concat continuation, full page, or frozen
//! page starts one new segment. Slices are O(1) spans, concat adds one descriptor,
//! and page allocation is amortized across many appends. Freezing resolves the
//! descriptor DAG iteratively and retains each intersected page pair once; it
//! never copies byte or provenance payloads.
//!
//! Exact comparison walks chunks and never accepts a digest as equality
//! evidence. Frozen single-leaf ranges expose their underlying byte slice so
//! ACT hashing can keep its contiguous hardware path without flattening.
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
/// Initial production page-capacity hypothesis. Focused probes instantiate the
/// private const-generic backend at 32, 64, 128, and 256 without adding a
/// runtime branch to production construction.
const DEFAULT_PAGE_BYTES: usize = 64;

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
struct SequenceNodeId(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SequenceSpan {
    root: SequenceNodeId,
    start: usize,
    len: usize,
}

/// An immutable O(1)-size checkpoint into the append-log rope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalBytesHandle {
    sequence: Option<SequenceSpan>,
}

impl CanonicalBytesHandle {
    pub(crate) const fn len(self) -> usize {
        match self.sequence {
            Some(span) => span.len,
            None => 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn is_empty(self) -> bool { self.sequence.is_none() }
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

// Sequence spans bound visibility: extending a page only appends after every
// previously returned span, so old handles remain immutable without tail copy.

#[derive(Debug)]
struct CanonicalBytePage {
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct CanonicalProvenancePage {
    values: Vec<CanonicalByteProvenance>,
}

#[derive(Clone, Copy)]
enum SequenceNodeKind {
    Segment {
        previous: Option<SequenceSpan>,
        page: usize,
        page_start: usize,
        used: usize,
    },
    #[cfg(test)]
    Concat {
        left: SequenceSpan,
        right: SequenceSpan,
    },
}

struct SequenceNode {
    len: usize,
    kind: SequenceNodeKind,
}

#[derive(Clone, Copy)]
enum SequenceCursorTask {
    Span(SequenceSpan),
    Page { page: usize, at: usize, end: usize },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PageBackedArenaStats {
    segments: usize,
    concats: usize,
    byte_pages: usize,
    provenance_pages: usize,
    payload_slots: usize,
    divergent_segments: usize,
    page_rollovers: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PageLayoutHypothesisStats {
    full_page_pairs: usize,
    unique_full_byte_pages: usize,
    unique_full_provenance_pages: usize,
    unique_full_page_pairs: usize,
    provenance_runs: usize,
}

/// Append-log backend. Descriptor nodes are arena-owned; only immutable
/// payload pages cross the ownership boundary when a value is frozen.
struct PageBackedArena<const CAPACITY: usize> {
    nodes: Vec<SequenceNode>,
    byte_pages: Vec<Arc<CanonicalBytePage>>,
    provenance_pages: Vec<Arc<CanonicalProvenancePage>>,
    #[cfg(test)]
    stats: PageBackedArenaStats,
}

impl<const CAPACITY: usize> PageBackedArena<CAPACITY> {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            byte_pages: Vec::new(),
            provenance_pages: Vec::new(),
            #[cfg(test)]
            stats: PageBackedArenaStats::default(),
        }
    }

    fn empty(&self) -> CanonicalBytesHandle { CanonicalBytesHandle { sequence: None } }

    fn span(value: CanonicalBytesHandle) -> Option<SequenceSpan> { value.sequence }

    fn handle(span: SequenceSpan) -> CanonicalBytesHandle {
        CanonicalBytesHandle {
            sequence: Some(span),
        }
    }

    fn append(
        &mut self,
        prefix: CanonicalBytesHandle,
        byte: CanonicalLocatedByte,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        prefix
            .len()
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        let Some(prefix_span) = Self::span(prefix) else {
            return self.push_segment(None, byte, false);
        };
        if let Some(span) = self.try_extend(prefix_span, byte)? {
            return Ok(Self::handle(span));
        }
        self.push_segment(Some(prefix_span), byte, true)
    }

    fn try_extend(
        &mut self,
        span: SequenceSpan,
        byte: CanonicalLocatedByte,
    ) -> Result<Option<SequenceSpan>, CanonicalBytesArenaError> {
        let node = &self.nodes[span.root.0];
        let (page, page_start, used) = match node.kind {
            SequenceNodeKind::Segment {
                page,
                page_start,
                used,
                ..
            } => (page, page_start, used),
            #[cfg(test)]
            SequenceNodeKind::Concat { .. } => return Ok(None),
        };
        if span.start != 0 || span.len != node.len || used == CAPACITY {
            return Ok(None);
        }
        let page_end = page_start
            .checked_add(used)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        if page_end >= CAPACITY
            || self.byte_pages[page].bytes.len() != page_end
            || self.provenance_pages[page].values.len() != page_end
        {
            return Ok(None);
        }
        let (Some(byte_page), Some(provenance_page)) = (
            Arc::get_mut(&mut self.byte_pages[page]),
            Arc::get_mut(&mut self.provenance_pages[page]),
        ) else {
            return Ok(None);
        };
        byte_page.bytes.push(byte.byte);
        provenance_page.values.push(byte.provenance);
        let len = span
            .len
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        match &mut self.nodes[span.root.0].kind {
            SequenceNodeKind::Segment { used, .. } => *used += 1,
            #[cfg(test)]
            SequenceNodeKind::Concat { .. } => {
                unreachable!("the inspected sequence node remains a segment")
            }
        }
        self.nodes[span.root.0].len = len;
        #[cfg(test)]
        {
            self.stats.payload_slots += 1;
        }
        Ok(Some(SequenceSpan {
            root: span.root,
            start: 0,
            len,
        }))
    }

    fn push_segment(
        &mut self,
        previous: Option<SequenceSpan>,
        byte: CanonicalLocatedByte,
        _divergent: bool,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        let len = previous
            .map_or(0, |span| span.len)
            .checked_add(1)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        let (page, page_start) = self.push_payload(byte)?;
        let root = SequenceNodeId(self.nodes.len());
        self.nodes.push(SequenceNode {
            len,
            kind: SequenceNodeKind::Segment {
                previous,
                page,
                page_start,
                used: 1,
            },
        });
        #[cfg(test)]
        {
            self.stats.segments += 1;
            self.stats.payload_slots += 1;
            self.stats.divergent_segments += usize::from(_divergent);
        }
        Ok(Self::handle(SequenceSpan {
            root,
            start: 0,
            len,
        }))
    }

    fn push_payload(
        &mut self,
        byte: CanonicalLocatedByte,
    ) -> Result<(usize, usize), CanonicalBytesArenaError> {
        if let Some(page) = self.byte_pages.len().checked_sub(1) {
            if self.byte_pages[page].bytes.len() < CAPACITY
                && self.byte_pages[page].bytes.len() == self.provenance_pages[page].values.len()
            {
                let page_start = self.byte_pages[page].bytes.len();
                if let (Some(byte_page), Some(provenance_page)) = (
                    Arc::get_mut(&mut self.byte_pages[page]),
                    Arc::get_mut(&mut self.provenance_pages[page]),
                ) {
                    byte_page.bytes.push(byte.byte);
                    provenance_page.values.push(byte.provenance);
                    return Ok((page, page_start));
                }
            }
        }
        #[cfg(test)]
        let had_page = !self.byte_pages.is_empty();
        self.byte_pages
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        self.provenance_pages
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(CAPACITY)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        bytes.push(byte.byte);
        let mut values = Vec::new();
        values
            .try_reserve_exact(CAPACITY)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        values.push(byte.provenance);
        self.byte_pages.push(Arc::new(CanonicalBytePage { bytes }));
        self.provenance_pages
            .push(Arc::new(CanonicalProvenancePage { values }));
        #[cfg(test)]
        {
            self.stats.byte_pages = self.byte_pages.len();
            self.stats.provenance_pages = self.provenance_pages.len();
            self.stats.page_rollovers += usize::from(had_page);
        }
        Ok((self.byte_pages.len() - 1, 0))
    }

    fn slice(
        &self,
        value: CanonicalBytesHandle,
        range: Range<usize>,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        if range.start > range.end || range.end > value.len() {
            return Err(CanonicalBytesArenaError::SliceOutsideSequence {
                start: range.start,
                end: range.end,
                len: value.len(),
            });
        }
        if range.is_empty() {
            return Ok(self.empty());
        }
        let span = Self::span(value).expect("a non-empty value has a sequence span");
        let start = span
            .start
            .checked_add(range.start)
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        Ok(Self::handle(SequenceSpan {
            root: span.root,
            start,
            len: range.end - range.start,
        }))
    }

    #[cfg(test)]
    fn concat(
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
            .len()
            .checked_add(right.len())
            .ok_or(CanonicalBytesArenaError::LengthOverflow)?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        let root = SequenceNodeId(self.nodes.len());
        self.nodes.push(SequenceNode {
            len,
            kind: SequenceNodeKind::Concat {
                left: Self::span(left).expect("a non-empty left value has a span"),
                right: Self::span(right).expect("a non-empty right value has a span"),
            },
        });
        self.stats.concats += 1;
        Ok(Self::handle(SequenceSpan {
            root,
            start: 0,
            len,
        }))
    }

    fn expand(&self, work: &mut Vec<SequenceCursorTask>, span: SequenceSpan) {
        let node = &self.nodes[span.root.0];
        let end = span.start + span.len;
        match node.kind {
            SequenceNodeKind::Segment {
                previous,
                page,
                page_start,
                used,
            } => {
                let previous_len = previous.map_or(0, |value| value.len);
                if end > previous_len {
                    work.push(SequenceCursorTask::Page {
                        page,
                        at: page_start + span.start.max(previous_len) - previous_len,
                        end: page_start + (end - previous_len).min(used),
                    });
                }
                if span.start < previous_len {
                    let previous = previous.expect("a positive previous range has a predecessor");
                    work.push(SequenceCursorTask::Span(SequenceSpan {
                        root: previous.root,
                        start: previous.start + span.start,
                        len: end.min(previous_len) - span.start,
                    }));
                }
            }
            #[cfg(test)]
            SequenceNodeKind::Concat { left, right } => {
                if end > left.len {
                    work.push(SequenceCursorTask::Span(SequenceSpan {
                        root: right.root,
                        start: right.start + span.start.saturating_sub(left.len),
                        len: end.min(left.len + right.len) - span.start.max(left.len),
                    }));
                }
                if span.start < left.len {
                    work.push(SequenceCursorTask::Span(SequenceSpan {
                        root: left.root,
                        start: left.start + span.start,
                        len: end.min(left.len) - span.start,
                    }));
                }
            }
        }
    }

    fn leaves(&self, value: CanonicalBytesHandle) -> Vec<PageBackedLeaf> {
        let mut work: Vec<_> = Self::span(value)
            .into_iter()
            .map(SequenceCursorTask::Span)
            .collect();
        let mut leaves = Vec::new();
        let mut logical_start = 0usize;
        while let Some(task) = work.pop() {
            match task {
                SequenceCursorTask::Span(span) => self.expand(&mut work, span),
                SequenceCursorTask::Page { page, at, end } => {
                    let len = end - at;
                    leaves.push(PageBackedLeaf {
                        page,
                        page_start: at,
                        len,
                        logical_start,
                    });
                    logical_start += len;
                }
            }
        }
        leaves
    }

    /// Count exact deduplication and provenance-run opportunities without
    /// changing the measured representation. Hash-set equality is the final
    /// evidence, so a hash collision cannot fabricate an interning win.
    #[cfg(test)]
    fn layout_hypothesis_stats(&self) -> PageLayoutHypothesisStats {
        use std::collections::HashSet;

        let mut unique_full_byte_pages = HashSet::new();
        let mut unique_full_provenance_pages = HashSet::new();
        let mut unique_full_page_pairs = HashSet::new();
        let mut full_page_pairs = 0usize;
        let mut provenance_runs = 0usize;
        for (bytes, provenance) in self.byte_pages.iter().zip(&self.provenance_pages) {
            let bytes = bytes.bytes.as_slice();
            let provenance = provenance.values.as_slice();
            provenance_runs += usize::from(!provenance.is_empty());
            provenance_runs += provenance
                .windows(2)
                .filter(|pair| {
                    pair[0].source != pair[1].source
                        || pair[0].offset.checked_add(1) != Some(pair[1].offset)
                })
                .count();
            if bytes.len() == CAPACITY && provenance.len() == CAPACITY {
                full_page_pairs += 1;
                unique_full_byte_pages.insert(bytes);
                unique_full_provenance_pages.insert(provenance);
                unique_full_page_pairs.insert((bytes, provenance));
            }
        }
        PageLayoutHypothesisStats {
            full_page_pairs,
            unique_full_byte_pages: unique_full_byte_pages.len(),
            unique_full_provenance_pages: unique_full_provenance_pages.len(),
            unique_full_page_pairs: unique_full_page_pairs.len(),
            provenance_runs,
        }
    }
}

#[derive(Clone, Copy)]
struct PageBackedLeaf {
    page: usize,
    page_start: usize,
    len: usize,
    logical_start: usize,
}

struct PageBackedByteCursor<'a> {
    arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>,
    work: Vec<SequenceCursorTask>,
}

impl<'a> PageBackedByteCursor<'a> {
    fn new(arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>, value: CanonicalBytesHandle) -> Self {
        Self {
            arena,
            work: PageBackedArena::<DEFAULT_PAGE_BYTES>::span(value)
                .into_iter()
                .map(SequenceCursorTask::Span)
                .collect(),
        }
    }
}

impl Iterator for PageBackedByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                SequenceCursorTask::Span(span) => self.arena.expand(&mut self.work, span),
                SequenceCursorTask::Page { page, at, end } => {
                    let byte = self.arena.byte_pages[page].bytes[at];
                    if at + 1 < end {
                        self.work.push(SequenceCursorTask::Page {
                            page,
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

struct PageBackedProvenanceCursor<'a> {
    arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>,
    work: Vec<SequenceCursorTask>,
}

impl<'a> PageBackedProvenanceCursor<'a> {
    fn new(arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>, value: CanonicalBytesHandle) -> Self {
        Self {
            arena,
            work: PageBackedArena::<DEFAULT_PAGE_BYTES>::span(value)
                .into_iter()
                .map(SequenceCursorTask::Span)
                .collect(),
        }
    }
}

impl Iterator for PageBackedProvenanceCursor<'_> {
    type Item = CanonicalByteProvenance;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                SequenceCursorTask::Span(span) => self.arena.expand(&mut self.work, span),
                SequenceCursorTask::Page { page, at, end } => {
                    let provenance = self.arena.provenance_pages[page].values[at];
                    if at + 1 < end {
                        self.work.push(SequenceCursorTask::Page {
                            page,
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

#[derive(Clone)]
struct FrozenPagePair {
    bytes: Arc<CanonicalBytePage>,
    provenance: Arc<CanonicalProvenancePage>,
}

#[derive(Clone, Copy)]
struct FrozenPageLeaf {
    page: usize,
    page_start: usize,
    len: usize,
    logical_start: usize,
}

impl FrozenPageLeaf {
    fn logical_end(self) -> usize { self.logical_start + self.len }
}

struct PageFrozenData {
    len: usize,
    pages: Box<[FrozenPagePair]>,
    leaves: Box<[FrozenPageLeaf]>,
    logical_pages: Box<[usize]>,
}

#[derive(Clone)]
struct PageFrozenBytes {
    data: Arc<PageFrozenData>,
    start: usize,
    len: usize,
}

impl<const CAPACITY: usize> PageBackedArena<CAPACITY> {
    fn freeze(&self, value: CanonicalBytesHandle) -> PageFrozenBytes {
        let source_leaves = self.leaves(value);
        let mut page_ordinals = HashMap::new();
        let mut pages = Vec::new();
        let mut leaves = Vec::with_capacity(source_leaves.len());
        for leaf in source_leaves {
            let page = *page_ordinals.entry(leaf.page).or_insert_with(|| {
                let ordinal = pages.len();
                pages.push(FrozenPagePair {
                    bytes: Arc::clone(&self.byte_pages[leaf.page]),
                    provenance: Arc::clone(&self.provenance_pages[leaf.page]),
                });
                ordinal
            });
            leaves.push(FrozenPageLeaf {
                page,
                page_start: leaf.page_start,
                len: leaf.len,
                logical_start: leaf.logical_start,
            });
        }
        let value_len = value.len();
        let page_count = value_len / BLOCK_BYTES + usize::from(value_len % BLOCK_BYTES != 0);
        let mut logical_pages = Vec::with_capacity(page_count);
        let mut leaf = 0usize;
        for logical_page in 0..page_count {
            let logical_start = logical_page * BLOCK_BYTES;
            while leaves[leaf].logical_end() <= logical_start {
                leaf += 1;
            }
            logical_pages.push(leaf);
        }
        PageFrozenBytes {
            data: Arc::new(PageFrozenData {
                len: value_len,
                pages: pages.into_boxed_slice(),
                leaves: leaves.into_boxed_slice(),
                logical_pages: logical_pages.into_boxed_slice(),
            }),
            start: 0,
            len: value_len,
        }
    }
}

impl PageFrozenBytes {
    fn len(&self) -> usize { self.len }

    fn is_empty(&self) -> bool { self.len == 0 }

    fn subview(&self, range: Range<usize>) -> Option<Self> {
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

    fn leaf_at(&self, absolute: usize) -> Option<usize> {
        let mut leaf = *self.data.logical_pages.get(absolute / BLOCK_BYTES)?;
        while self.data.leaves.get(leaf)?.logical_end() <= absolute {
            leaf += 1;
        }
        Some(leaf)
    }

    fn byte_at(&self, logical_offset: usize) -> Option<u8> {
        if logical_offset >= self.len {
            return None;
        }
        let absolute = self.start.checked_add(logical_offset)?;
        let leaf = self.data.leaves[self.leaf_at(absolute)?];
        let at = leaf.page_start + absolute - leaf.logical_start;
        self.data.pages[leaf.page].bytes.bytes.get(at).copied()
    }

    fn provenance_at(&self, logical_offset: usize) -> Option<CanonicalByteProvenance> {
        if logical_offset >= self.len {
            return None;
        }
        let absolute = self.start.checked_add(logical_offset)?;
        let leaf = self.data.leaves[self.leaf_at(absolute)?];
        let at = leaf.page_start + absolute - leaf.logical_start;
        self.data.pages[leaf.page]
            .provenance
            .values
            .get(at)
            .copied()
    }

    /// Borrow bytes directly when the complete logical range is held by one
    /// page leaf. Crossing a rope boundary deliberately returns `None`.
    fn contiguous_bytes(&self, range: Range<usize>) -> Option<&[u8]> {
        if range.start > range.end || range.end > self.len {
            return None;
        }
        if range.is_empty() {
            return Some(&[]);
        }
        let absolute_start = self.start.checked_add(range.start)?;
        let absolute_end = self.start.checked_add(range.end)?;
        let leaf = self.data.leaves[self.leaf_at(absolute_start)?];
        if absolute_end > leaf.logical_end() {
            return None;
        }
        let start = leaf.page_start + absolute_start - leaf.logical_start;
        let end = start.checked_add(range.end - range.start)?;
        self.data.pages[leaf.page].bytes.bytes.get(start..end)
    }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        let Some(end) = start.checked_add(output.len()) else {
            return false;
        };
        if end > self.len {
            return false;
        }
        let mut destination = 0usize;
        let mut absolute = self.start + start;
        let absolute_end = self.start + end;
        while absolute < absolute_end {
            let Some(leaf_index) = self.leaf_at(absolute) else {
                return false;
            };
            let leaf = self.data.leaves[leaf_index];
            let width = (absolute_end - absolute).min(leaf.logical_end() - absolute);
            let page_start = leaf.page_start + absolute - leaf.logical_start;
            let page_end = page_start + width;
            output[destination..destination + width]
                .copy_from_slice(&self.data.pages[leaf.page].bytes.bytes[page_start..page_end]);
            destination += width;
            absolute += width;
        }
        true
    }

    fn content_eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut left_absolute = self.start;
        let mut right_absolute = other.start;
        let mut remaining = self.len;
        while remaining != 0 {
            let Some(left_leaf) = self.leaf_at(left_absolute) else {
                return false;
            };
            let Some(right_leaf) = other.leaf_at(right_absolute) else {
                return false;
            };
            let left_leaf = self.data.leaves[left_leaf];
            let right_leaf = other.data.leaves[right_leaf];
            let width = remaining
                .min(left_leaf.logical_end() - left_absolute)
                .min(right_leaf.logical_end() - right_absolute);
            let left_start = left_leaf.page_start + left_absolute - left_leaf.logical_start;
            let right_start = right_leaf.page_start + right_absolute - right_leaf.logical_start;
            if self.data.pages[left_leaf.page].bytes.bytes[left_start..left_start + width]
                != other.data.pages[right_leaf.page].bytes.bytes[right_start..right_start + width]
            {
                return false;
            }
            left_absolute += width;
            right_absolute += width;
            remaining -= width;
        }
        true
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalBytesArenaStats {
    pub(crate) content_nodes: usize,
    pub(crate) provenance_nodes: usize,
    pub(crate) segments: usize,
    pub(crate) concats: usize,
    pub(crate) byte_pages: usize,
    pub(crate) provenance_pages: usize,
    pub(crate) payload_slots: usize,
    pub(crate) divergent_segments: usize,
    pub(crate) page_rollovers: usize,
}

/// Immutable, ownership-independent bytes frozen from one persistent handle.
///
/// Freezing copies only leaf/index metadata. Byte and provenance payload arrays
/// remain in the immutable arena nodes retained by `Arc`, so the originating
/// arena may be appended to or dropped after this value is created. A view and
/// each of its subviews are constant-size clones.
#[derive(Clone)]
pub(crate) struct FrozenCanonicalBytes {
    inner: PageFrozenBytes,
}

impl fmt::Debug for FrozenCanonicalBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrozenCanonicalBytes")
            .field("len", &self.inner.len)
            .finish()
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrozenCanonicalBytesStats {
    pub(crate) content_leaves: usize,
    pub(crate) provenance_leaves: usize,
    pub(crate) page_entries: usize,
}

impl FrozenCanonicalBytes {
    #[cfg(test)]
    pub(crate) fn shares_backing_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner.data, &other.inner.data)
    }

    pub(crate) fn len(&self) -> usize { self.inner.len() }

    pub(crate) fn is_empty(&self) -> bool { self.inner.is_empty() }

    /// Form an O(1) owned subview without copying payload or index metadata.
    pub(crate) fn subview(&self, range: Range<usize>) -> Option<Self> {
        self.inner.subview(range).map(|inner| Self { inner })
    }

    pub(crate) fn try_coalesce(&self, right: &Self) -> Option<Self> {
        if !Arc::ptr_eq(&self.inner.data, &right.inner.data)
            || self.inner.start.checked_add(self.inner.len)? != right.inner.start
        {
            return None;
        }
        let len = self.inner.len.checked_add(right.inner.len)?;
        if self.inner.start.checked_add(len)? > self.inner.data.len {
            return None;
        }
        Some(Self {
            inner: PageFrozenBytes {
                data: Arc::clone(&self.inner.data),
                start: self.inner.start,
                len,
            },
        })
    }

    /// Random byte lookup. At most `BLOCK_BYTES` leaf records are inspected.
    pub(crate) fn byte_at(&self, logical_offset: usize) -> Option<u8> {
        self.inner.byte_at(logical_offset)
    }

    /// Random physical-provenance lookup with the same bounded index walk.
    pub(crate) fn provenance_at(&self, logical_offset: usize) -> Option<CanonicalByteProvenance> {
        self.inner.provenance_at(logical_offset)
    }

    /// Return a borrowed contiguous range when it lies within one page leaf.
    pub(crate) fn contiguous_bytes(&self, range: Range<usize>) -> Option<&[u8]> {
        self.inner.contiguous_bytes(range)
    }

    /// Copy directly from page chunks without flattening an intermediate value.
    pub(crate) fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        self.inner.copy_exact(start, output)
    }

    /// Exact chunk-wise equality across independently fragmented frozen values.
    pub(crate) fn content_eq(&self, other: &Self) -> bool { self.inner.content_eq(&other.inner) }

    pub(crate) fn bytes(&self) -> FrozenCanonicalByteCursor {
        FrozenCanonicalByteCursor::new(self.clone())
    }

    pub(crate) fn located_bytes(&self) -> FrozenCanonicalLocatedByteCursor {
        FrozenCanonicalLocatedByteCursor {
            bytes: FrozenCanonicalByteCursor::new(self.clone()),
            provenance: FrozenCanonicalProvenanceCursor::new(self.clone()),
        }
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> FrozenCanonicalBytesStats {
        FrozenCanonicalBytesStats {
            content_leaves: self.inner.data.leaves.len(),
            provenance_leaves: self.inner.data.leaves.len(),
            page_entries: self.inner.data.logical_pages.len() * 2,
        }
    }
}

/// Append-only ownership arena for persistent canonical byte strings.
pub(crate) struct CanonicalBytesArena {
    backend: PageBackedArena<DEFAULT_PAGE_BYTES>,
    #[cfg(test)]
    stats: CanonicalBytesArenaStats,
}

impl Default for CanonicalBytesArena {
    fn default() -> Self { Self::new() }
}

impl CanonicalBytesArena {
    pub(crate) fn new() -> Self {
        Self {
            backend: PageBackedArena::<DEFAULT_PAGE_BYTES>::new(),
            #[cfg(test)]
            stats: CanonicalBytesArenaStats::default(),
        }
    }

    pub(crate) const fn empty(&self) -> CanonicalBytesHandle {
        CanonicalBytesHandle { sequence: None }
    }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalBytesArenaStats { self.stats }

    /// Append one located byte. The returned handle does not mutate or
    /// invalidate `prefix`, so retaining `prefix` is an O(1) fork checkpoint.
    pub(crate) fn append(
        &mut self,
        prefix: CanonicalBytesHandle,
        byte: CanonicalLocatedByte,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        let value = self.backend.append(prefix, byte)?;
        #[cfg(test)]
        self.sync_page_stats();
        Ok(value)
    }

    /// O(1) persistent slice. No byte or provenance entry is copied.
    pub(crate) fn slice(
        &self,
        value: CanonicalBytesHandle,
        range: Range<usize>,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        self.backend.slice(value, range)
    }

    /// O(1) structural concatenation. Both operands remain valid checkpoints.
    #[cfg(test)]
    pub(crate) fn concat(
        &mut self,
        left: CanonicalBytesHandle,
        right: CanonicalBytesHandle,
    ) -> Result<CanonicalBytesHandle, CanonicalBytesArenaError> {
        let value = self.backend.concat(left, right)?;
        self.sync_page_stats();
        Ok(value)
    }

    /// Exact byte equality. Hashes are never equality evidence.
    #[cfg(test)]
    pub(crate) fn content_eq(
        &self,
        left: CanonicalBytesHandle,
        right: CanonicalBytesHandle,
    ) -> bool {
        if left.len() != right.len() {
            return false;
        }
        if left.sequence == right.sequence {
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
        CanonicalByteCursor::new(&self.backend, value)
    }

    /// Compatibility random lookup for a live borrowed arena. Frozen views
    /// provide the bounded indexed implementation used by production parsers.
    #[cfg(test)]
    pub(crate) fn byte_at(&self, value: CanonicalBytesHandle, logical_offset: usize) -> Option<u8> {
        if logical_offset >= value.len() {
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
            bytes: CanonicalByteCursor::new(&self.backend, value),
            provenance: CanonicalProvenanceCursor::new(&self.backend, value),
        }
    }

    /// Freeze one handle into ownership-independent indexed metadata.
    ///
    /// Retained immutable pages own the original payload vectors. Freezing
    /// allocates only leaf/index metadata and clones each intersected page
    /// pair once; no byte or provenance payload is copied.
    pub(crate) fn freeze(&self, value: CanonicalBytesHandle) -> FrozenCanonicalBytes {
        FrozenCanonicalBytes {
            inner: self.backend.freeze(value),
        }
    }

    pub(crate) fn provenance_at(
        &self,
        value: CanonicalBytesHandle,
        logical_offset: usize,
    ) -> Option<CanonicalByteProvenance> {
        if logical_offset >= value.len() {
            return None;
        }
        let one = self.slice(value, logical_offset..logical_offset + 1).ok()?;
        self.located_bytes(one).next().map(|byte| byte.provenance)
    }

    #[cfg(test)]
    fn sync_page_stats(&mut self) {
        self.stats.content_nodes = self.backend.nodes.len();
        self.stats.provenance_nodes = self.backend.nodes.len();
        self.stats.segments = self.backend.stats.segments;
        self.stats.concats = self.backend.stats.concats;
        self.stats.byte_pages = self.backend.stats.byte_pages;
        self.stats.provenance_pages = self.backend.stats.provenance_pages;
        self.stats.payload_slots = self.backend.stats.payload_slots;
        self.stats.divergent_segments = self.backend.stats.divergent_segments;
        self.stats.page_rollovers = self.backend.stats.page_rollovers;
    }
}

pub(crate) struct CanonicalByteCursor<'a> {
    inner: PageBackedByteCursor<'a>,
}

impl<'a> CanonicalByteCursor<'a> {
    fn new(arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>, value: CanonicalBytesHandle) -> Self {
        Self {
            inner: PageBackedByteCursor::new(arena, value),
        }
    }
}

impl Iterator for CanonicalByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> { self.inner.next() }
}

struct CanonicalProvenanceCursor<'a> {
    inner: PageBackedProvenanceCursor<'a>,
}

impl<'a> CanonicalProvenanceCursor<'a> {
    fn new(arena: &'a PageBackedArena<DEFAULT_PAGE_BYTES>, value: CanonicalBytesHandle) -> Self {
        Self {
            inner: PageBackedProvenanceCursor::new(arena, value),
        }
    }
}

impl Iterator for CanonicalProvenanceCursor<'_> {
    type Item = CanonicalByteProvenance;

    fn next(&mut self) -> Option<Self::Item> { self.inner.next() }
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
        let absolute = value.inner.start;
        let end = absolute + value.inner.len;
        let leaf = if value.is_empty() {
            value.inner.data.leaves.len()
        } else {
            value
                .inner
                .leaf_at(absolute)
                .expect("a non-empty frozen view starts in one content leaf")
        };
        Self {
            value,
            leaf,
            absolute,
            end,
        }
    }

    pub(crate) fn fast_forward(&mut self, width: usize) {
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
            self.value.inner.data.leaves.len()
        } else {
            self.value
                .inner
                .leaf_at(absolute)
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
        while self.value.inner.data.leaves[self.leaf].logical_end() <= self.absolute {
            self.leaf += 1;
        }
        let leaf = self.value.inner.data.leaves[self.leaf];
        let at = leaf.page_start + self.absolute - leaf.logical_start;
        let byte = self.value.inner.data.pages[leaf.page].bytes.bytes[at];
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
        let absolute = value.inner.start;
        let end = absolute + value.inner.len;
        let leaf = if value.is_empty() {
            value.inner.data.leaves.len()
        } else {
            value
                .inner
                .leaf_at(absolute)
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
            self.value.inner.data.leaves.len()
        } else {
            self.value
                .inner
                .leaf_at(absolute)
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
        while self.value.inner.data.leaves[self.leaf].logical_end() <= self.absolute {
            self.leaf += 1;
        }
        let leaf = self.value.inner.data.leaves[self.leaf];
        let at = leaf.page_start + self.absolute - leaf.logical_start;
        let provenance = self.value.inner.data.pages[leaf.page].provenance.values[at];
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

        let after = arena.stats();
        let suffix_bytes = branches.len() * 8;
        assert!(after.content_nodes - before.content_nodes <= branches.len());
        assert_eq!(after.content_nodes, after.provenance_nodes);
        assert_eq!(after.payload_slots - before.payload_slots, suffix_bytes);
    }

    #[test]
    fn equal_content_with_different_provenance_compares_exactly_and_keeps_sources() {
        let mut arena = CanonicalBytesArena::new();
        let first = append_bytes(&mut arena, b"canonical", 7, 100);
        let second = append_bytes(&mut arena, b"canonical", 9, 900);

        assert!(arena.content_eq(first, second));
        assert_eq!(arena.compare(first, second), Ordering::Equal);
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
                const LEN: usize = 100_000;
                let mut arena = CanonicalBytesArena::new();
                let mut value = arena.empty();
                for offset in 0..LEN {
                    let singleton = arena
                        .append(arena.empty(), located((offset % 251) as u8, 4, offset))
                        .unwrap();
                    value = arena.concat(value, singleton).unwrap();
                }
                assert_eq!(arena.bytes(value).count(), LEN);
                assert_eq!(arena.located_bytes(value).count(), LEN);
                assert_eq!(arena.provenance_at(value, LEN - 1).unwrap().offset, LEN - 1);
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
        assert!(small.content_nodes <= 4_000_usize.div_ceil(DEFAULT_PAGE_BYTES) + 64 + 1);
        assert!(small.provenance_nodes <= 4_000_usize.div_ceil(DEFAULT_PAGE_BYTES) + 64 + 1);
        assert!(large.content_nodes <= 8_000_usize.div_ceil(DEFAULT_PAGE_BYTES) + 64 + 1);
        assert!(large.provenance_nodes <= 8_000_usize.div_ceil(DEFAULT_PAGE_BYTES) + 64 + 1);
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
    fn page_capacity_probe_exposes_static_32_64_128_256_policies() {
        use std::hint::black_box;
        use std::time::Instant;

        #[derive(Clone, Copy)]
        enum Workload {
            Linear,
            Forked,
            FreezeThenExtend,
        }

        impl Workload {
            fn name(self) -> &'static str {
                match self {
                    Self::Linear => "linear",
                    Self::Forked => "forked",
                    Self::FreezeThenExtend => "freeze_then_extend",
                }
            }
        }

        fn probe<const CAPACITY: usize>(len: usize, workload: Workload, report: bool) {
            let mut arena = PageBackedArena::<CAPACITY>::new();
            let mut value = arena.empty();
            let started = Instant::now();
            for offset in 0..len {
                value = arena
                    .append(value, located((offset % 251) as u8, 31, offset))
                    .unwrap();
            }
            match workload {
                Workload::Linear => {}
                Workload::Forked => {
                    let checkpoint = value;
                    for branch in 0..len / 16 {
                        let mut suffix = checkpoint;
                        for offset in 0..16 {
                            suffix = arena
                                .append(
                                    suffix,
                                    located(
                                        (branch.wrapping_add(offset) % 251) as u8,
                                        32,
                                        branch * 16 + offset,
                                    ),
                                )
                                .unwrap();
                        }
                        black_box(suffix);
                    }
                }
                Workload::FreezeThenExtend => {
                    let frozen = arena.freeze(value);
                    let mut extension = value;
                    for offset in 0..len {
                        extension = arena
                            .append(extension, located((offset % 239) as u8, 33, len + offset))
                            .unwrap();
                    }
                    assert_eq!(frozen.len(), len);
                    assert_eq!(frozen.byte_at(len - 1), Some(((len - 1) % 251) as u8));
                    black_box(extension);
                }
            }
            let build_elapsed = started.elapsed();
            let frozen = FrozenCanonicalBytes {
                inner: arena.freeze(value),
            };
            let scan_started = Instant::now();
            let checksum = frozen
                .bytes()
                .fold(0u64, |sum, byte| sum.wrapping_add(u64::from(byte)));
            let scan_elapsed = scan_started.elapsed();
            black_box(checksum);

            assert_eq!(frozen.len(), len);
            for offset in (0..len).step_by(37) {
                assert_eq!(frozen.byte_at(offset), Some((offset % 251) as u8));
                assert_eq!(frozen.provenance_at(offset).unwrap().offset, offset);
            }
            if matches!(workload, Workload::Linear) {
                assert_eq!(arena.stats.payload_slots, len);
                assert_eq!(arena.stats.byte_pages, len.div_ceil(CAPACITY));
                assert_eq!(arena.stats.provenance_pages, len.div_ceil(CAPACITY));
                assert_eq!(arena.stats.segments, len.div_ceil(CAPACITY));
            }
            if report {
                println!(
                    "canonical_bytes_arena,{},{},{},{},{},{},{},{},{},{}",
                    CAPACITY,
                    workload.name(),
                    len,
                    build_elapsed.as_nanos(),
                    scan_elapsed.as_nanos(),
                    arena.stats.segments,
                    arena.stats.byte_pages,
                    arena.stats.provenance_pages,
                    arena.stats.payload_slots,
                    frozen.inner.data.leaves.len(),
                );
                if std::env::var_os("CANONICAL_BYTES_ARENA_LAYOUT_ANALYSIS").is_some() {
                    let layout = arena.layout_hypothesis_stats();
                    println!(
                        "canonical_bytes_arena_layout,{},{},{},{},{},{},{},{},{}",
                        CAPACITY,
                        workload.name(),
                        len,
                        layout.full_page_pairs,
                        layout.unique_full_byte_pages,
                        layout.unique_full_provenance_pages,
                        layout.unique_full_page_pairs,
                        arena.stats.payload_slots,
                        layout.provenance_runs,
                    );
                }
            }
        }

        let report = std::env::var_os("CANONICAL_BYTES_ARENA_EXPERIMENT").is_some();
        let len = if report {
            std::env::var("CANONICAL_BYTES_ARENA_LEN")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(65_536)
                .max(1)
        } else {
            1_025
        };
        if report {
            println!(
                "subject,page_capacity,workload,base_length,build_ns,scan_ns,segments,byte_pages,provenance_pages,payload_slots,frozen_leaves"
            );
            if std::env::var_os("CANONICAL_BYTES_ARENA_LAYOUT_ANALYSIS").is_some() {
                println!(
                    "layout_subject,page_capacity,workload,base_length,full_page_pairs,unique_full_byte_pages,unique_full_provenance_pages,unique_full_page_pairs,payload_slots,provenance_runs"
                );
            }
        }
        let selected_capacity = std::env::var("CANONICAL_BYTES_ARENA_CAPACITY")
            .ok()
            .map(|value| {
                value
                    .parse::<usize>()
                    .expect("CANONICAL_BYTES_ARENA_CAPACITY must be 32, 64, 128, or 256")
            });
        let selected_workload = std::env::var("CANONICAL_BYTES_ARENA_WORKLOAD").ok();
        for workload in [
            Workload::Linear,
            Workload::Forked,
            Workload::FreezeThenExtend,
        ] {
            if selected_workload
                .as_deref()
                .is_some_and(|selected| selected != workload.name())
            {
                continue;
            }
            if selected_capacity.is_none_or(|selected| selected == 32) {
                probe::<32>(len, workload, report);
            }
            if selected_capacity.is_none_or(|selected| selected == 64) {
                probe::<64>(len, workload, report);
            }
            if selected_capacity.is_none_or(|selected| selected == 128) {
                probe::<128>(len, workload, report);
            }
            if selected_capacity.is_none_or(|selected| selected == 256) {
                probe::<256>(len, workload, report);
            }
        }
        assert!(
            selected_capacity.is_none_or(|selected| [32, 64, 128, 256].contains(&selected)),
            "CANONICAL_BYTES_ARENA_CAPACITY must be 32, 64, 128, or 256"
        );
        assert!(
            selected_workload.as_deref().is_none_or(|selected| [
                Workload::Linear.name(),
                Workload::Forked.name(),
                Workload::FreezeThenExtend.name(),
            ]
            .contains(&selected)),
            "CANONICAL_BYTES_ARENA_WORKLOAD must be linear, forked, or freeze_then_extend"
        );
    }

    #[test]
    fn randomized_persistent_operations_match_a_flat_located_byte_oracle() {
        #[derive(Clone)]
        struct OracleValue {
            handle: CanonicalBytesHandle,
            bytes: Vec<CanonicalLocatedByte>,
        }

        fn next_random(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }

        fn check(arena: &CanonicalBytesArena, value: &OracleValue) {
            assert_eq!(
                arena.located_bytes(value.handle).collect::<Vec<_>>(),
                value.bytes
            );
            assert_eq!(
                arena.bytes(value.handle).collect::<Vec<_>>(),
                value.bytes.iter().map(|byte| byte.byte).collect::<Vec<_>>()
            );
        }

        let mut arena = CanonicalBytesArena::new();
        let mut values = vec![OracleValue {
            handle: arena.empty(),
            bytes: Vec::new(),
        }];
        let mut frozen = Vec::new();
        let mut random = 0x9e37_79b9_7f4a_7c15u64;

        for step in 0..20_000usize {
            let choice = next_random(&mut random) as usize;
            let left = choice % values.len();
            let operation = choice % 4;
            let next = match operation {
                0 => {
                    let byte = CanonicalLocatedByte::new(
                        (next_random(&mut random) % 251) as u8,
                        CanonicalByteProvenance::new(
                            (next_random(&mut random) % 17) as u32,
                            step * 31 + choice % 29,
                        ),
                    );
                    let mut bytes = values[left].bytes.clone();
                    bytes.push(byte);
                    OracleValue {
                        handle: arena.append(values[left].handle, byte).unwrap(),
                        bytes,
                    }
                }
                1 if !values[left].bytes.is_empty() => {
                    let len = values[left].bytes.len();
                    let first = next_random(&mut random) as usize % (len + 1);
                    let second = next_random(&mut random) as usize % (len + 1);
                    let range = first.min(second)..first.max(second);
                    OracleValue {
                        handle: arena.slice(values[left].handle, range.clone()).unwrap(),
                        bytes: values[left].bytes[range].to_vec(),
                    }
                }
                2 if values[left].bytes.len() < 4_096 => {
                    let right = next_random(&mut random) as usize % values.len();
                    if values[left].bytes.len() + values[right].bytes.len() > 4_096 {
                        values[left].clone()
                    } else {
                        let mut bytes = values[left].bytes.clone();
                        bytes.extend_from_slice(&values[right].bytes);
                        OracleValue {
                            handle: arena
                                .concat(values[left].handle, values[right].handle)
                                .unwrap(),
                            bytes,
                        }
                    }
                }
                _ => values[left].clone(),
            };
            check(&arena, &next);
            if step % 97 == 0 {
                let snapshot = arena.freeze(next.handle);
                assert_eq!(snapshot.located_bytes().collect::<Vec<_>>(), next.bytes);
                frozen.push((snapshot, next.bytes.clone()));
            }
            values.push(next);
            if values.len() > 512 {
                values.swap_remove(next_random(&mut random) as usize % values.len());
            }
        }

        drop(arena);
        for (snapshot, expected) in frozen {
            assert_eq!(snapshot.located_bytes().collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn frozen_partial_page_aliasing_is_safe_under_miri() {
        let mut arena = CanonicalBytesArena::new();
        let prefix_bytes: Vec<_> = (0..63).map(|offset| (offset % 251) as u8).collect();
        let prefix = append_bytes(&mut arena, &prefix_bytes, 71, 1_000);
        let frozen_prefix = arena.freeze(prefix);

        let left = arena.append(prefix, located(0xaa, 72, 2_000)).unwrap();
        let right = arena.append(prefix, located(0xbb, 73, 3_000)).unwrap();
        let frozen_left = arena.freeze(left);
        let frozen_right = arena.freeze(right);

        assert_eq!(frozen_prefix.bytes().collect::<Vec<_>>(), prefix_bytes);
        assert_eq!(frozen_left.byte_at(63), Some(0xaa));
        assert_eq!(frozen_right.byte_at(63), Some(0xbb));
        assert_eq!(
            frozen_left.provenance_at(63),
            Some(CanonicalByteProvenance::new(72, 2_000))
        );
        assert_eq!(
            frozen_right.provenance_at(63),
            Some(CanonicalByteProvenance::new(73, 3_000))
        );

        drop(arena);
        assert_eq!(frozen_prefix.bytes().collect::<Vec<_>>(), prefix_bytes);
        assert_eq!(frozen_left.bytes().last(), Some(0xaa));
        assert_eq!(frozen_right.bytes().last(), Some(0xbb));
    }

    #[test]
    fn handle_retains_one_span_without_parallel_legacy_metadata() {
        assert_eq!(
            std::mem::size_of::<CanonicalBytesHandle>(),
            std::mem::size_of::<Option<SequenceSpan>>()
        );
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<CanonicalBytesHandle>(), 32);
        // The removed representation held two 32-byte optional spans plus an
        // eight-byte duplicate length on 64-bit targets: 72 bytes per handle.
    }

    #[test]
    fn frozen_chunks_support_contiguous_borrow_copy_and_fragment_independent_equality() {
        let bytes: Vec<_> = (0..130).map(|offset| (offset % 251) as u8).collect();
        let mut linear_arena = CanonicalBytesArena::new();
        let linear = append_bytes(&mut linear_arena, &bytes, 41, 1_000);
        let linear = linear_arena.freeze(linear);

        assert_eq!(linear.contiguous_bytes(3..20), Some(&bytes[3..20]));
        assert_eq!(linear.contiguous_bytes(60..68), None);
        let mut copied = [0u8; 72];
        assert!(linear.copy_exact(37, &mut copied));
        assert_eq!(copied, bytes[37..109]);
        assert!(!linear.copy_exact(100, &mut copied));

        let mut fragmented_arena = CanonicalBytesArena::new();
        let mut fragmented = fragmented_arena.empty();
        for (offset, byte) in bytes.iter().copied().enumerate() {
            let singleton = fragmented_arena
                .append(fragmented_arena.empty(), located(byte, 42, 8_000 + offset))
                .unwrap();
            fragmented = fragmented_arena.concat(fragmented, singleton).unwrap();
        }
        let fragmented = fragmented_arena.freeze(fragmented);
        assert!(linear.content_eq(&fragmented));
        assert!(fragmented.content_eq(&linear));
    }

    #[test]
    fn freezing_seals_pages_without_invalidating_prefix_or_copying_payload() {
        let mut arena = CanonicalBytesArena::new();
        let prefix = append_bytes(&mut arena, b"persistent-prefix", 51, 100);
        let before = arena.stats();
        let frozen = arena.freeze(prefix);
        assert_eq!(arena.stats(), before);

        let extended = arena.append(prefix, located(b'!', 52, 900)).unwrap();
        assert_eq!(to_vec(&arena, prefix), b"persistent-prefix");
        assert_eq!(to_vec(&arena, extended), b"persistent-prefix!");
        assert_eq!(frozen.bytes().collect::<Vec<_>>(), b"persistent-prefix");
        assert_eq!(arena.stats().payload_slots, before.payload_slots + 1);
        assert_eq!(arena.stats().segments, before.segments + 1);
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
