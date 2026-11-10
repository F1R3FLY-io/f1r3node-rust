//! Stack-safe inspection of PathMap's canonical `ACTree03` byte image.
//!
//! PathMap exposes a compact-arena writer but no in-memory reader constructor.
//! This module is the single checked reader used by RSpace Bags and model-layer
//! EPathMaps. It validates arbitrary bytes without recursion, preserves line
//! compression, and can expose a one-line path as a range into the input arena.

#[cfg(any(test, feature = "act-codec-instrumentation"))]
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
#[cfg(not(any(miri, target_arch = "riscv64")))]
use std::hash::Hasher;
#[cfg(any(test, feature = "act-codec-instrumentation"))]
use std::mem::size_of;
use std::ops::Range;

use pathmap::arena_compact::COMPACT_TREE_MAGIC;

const ACT_HEADER_LEN: usize = 16;
const ACT_VALUE_FLAG: u8 = 0x40;
const ACT_LINE_FLAG: u8 = 0x80;
const ACT_VARINT_BIAS: u8 = u8::MAX - 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactTrieError {
    message: String,
    source_offset: Option<usize>,
}

impl CompactTrieError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source_offset: None,
        }
    }

    fn at(source_offset: usize, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source_offset: Some(source_offset),
        }
    }

    pub const fn source_offset(&self) -> Option<usize> { self.source_offset }
}

impl fmt::Display for CompactTrieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for CompactTrieError {}

/// Random-access bytes used by canonical ACTree03 validation and traversal.
///
/// Implementations may span persistent chunks. All default operations remain
/// bounded-memory and never request a contiguous image of the source.
pub trait IndexedByteSource {
    /// The logical source length. Every index below this bound must return a
    /// byte from [`Self::byte_at`]; indexed ACT validation relies on that total
    /// in-range contract rather than probing a declared range twice.
    fn len(&self) -> usize;

    fn byte_at(&self, index: usize) -> Option<u8>;

    /// Borrow a range when this source happens to store it contiguously.
    /// Fragmented sources return `None`; the optimization never changes the
    /// accepted byte language.
    fn contiguous_range(&self, _range: Range<usize>) -> Option<&[u8]> { None }

    fn is_empty(&self) -> bool { self.len() == 0 }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        let Some(end) = start.checked_add(output.len()) else {
            return false;
        };
        if end > self.len() {
            return false;
        }
        for (index, byte) in output.iter_mut().enumerate() {
            let Some(source_byte) = self.byte_at(start + index) else {
                return false;
            };
            *byte = source_byte;
        }
        true
    }

    fn ranges_equal(&self, left: Range<usize>, right: Range<usize>) -> bool {
        if left.len() != right.len() || left.end > self.len() || right.end > self.len() {
            return false;
        }
        (0..left.len()).all(|index| {
            matches!(
                (
                    self.byte_at(left.start + index),
                    self.byte_at(right.start + index),
                ),
                (Some(left), Some(right)) if left == right
            )
        })
    }
}

/// An indexed view over an existing contiguous `AsRef<[u8]>` backing.
pub struct AsRefIndexedByteSource<'a, B: ?Sized> {
    backing: &'a B,
}

impl<'a, B: ?Sized> AsRefIndexedByteSource<'a, B> {
    pub const fn new(backing: &'a B) -> Self { Self { backing } }
}

impl<B> IndexedByteSource for AsRefIndexedByteSource<'_, B>
where B: AsRef<[u8]> + ?Sized
{
    fn len(&self) -> usize { self.backing.as_ref().len() }

    fn byte_at(&self, index: usize) -> Option<u8> { self.backing.as_ref().get(index).copied() }

    fn contiguous_range(&self, range: Range<usize>) -> Option<&[u8]> {
        self.backing.as_ref().get(range)
    }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        let Some(end) = start.checked_add(output.len()) else {
            return false;
        };
        let Some(region) = self.backing.as_ref().get(start..end) else {
            return false;
        };
        output.copy_from_slice(region);
        true
    }

    fn ranges_equal(&self, left: Range<usize>, right: Range<usize>) -> bool {
        let bytes = self.backing.as_ref();
        match (bytes.get(left), bytes.get(right)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }
}

#[cfg(any(test, feature = "act-codec-instrumentation"))]
#[derive(Clone, Copy, Default)]
struct IndexedSourceAccessTally {
    byte_reads: usize,
    copy_bytes: usize,
    contiguous_bytes: usize,
    equality_bytes: usize,
}

#[cfg(any(test, feature = "act-codec-instrumentation"))]
struct CountedIndexedByteSource<'a, S: ?Sized> {
    inner: &'a S,
    tally: Cell<IndexedSourceAccessTally>,
}

#[cfg(any(test, feature = "act-codec-instrumentation"))]
impl<'a, S: ?Sized> CountedIndexedByteSource<'a, S> {
    fn new(inner: &'a S) -> Self {
        Self {
            inner,
            tally: Cell::new(IndexedSourceAccessTally::default()),
        }
    }

    fn tally(&self) -> IndexedSourceAccessTally { self.tally.get() }

    fn update(&self, update: impl FnOnce(&mut IndexedSourceAccessTally)) {
        let mut tally = self.tally.get();
        update(&mut tally);
        self.tally.set(tally);
    }
}

#[cfg(any(test, feature = "act-codec-instrumentation"))]
impl<S> IndexedByteSource for CountedIndexedByteSource<'_, S>
where S: IndexedByteSource + ?Sized
{
    fn len(&self) -> usize { self.inner.len() }

    fn byte_at(&self, index: usize) -> Option<u8> {
        self.update(|tally| tally.byte_reads += 1);
        self.inner.byte_at(index)
    }

    fn contiguous_range(&self, range: Range<usize>) -> Option<&[u8]> {
        let region = self.inner.contiguous_range(range);
        if let Some(region) = region {
            self.update(|tally| tally.contiguous_bytes += region.len());
        }
        region
    }

    fn copy_exact(&self, start: usize, output: &mut [u8]) -> bool {
        self.update(|tally| tally.copy_bytes += output.len());
        self.inner.copy_exact(start, output)
    }

    fn ranges_equal(&self, left: Range<usize>, right: Range<usize>) -> bool {
        if left.len() == right.len() {
            self.update(|tally| tally.equality_bytes += left.len());
        }
        self.inner.ranges_equal(left, right)
    }
}

/// A canonical path that either aliases one compressed line in the input or
/// owns the bytes assembled across branch edges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompactPath {
    Relative(Range<usize>),
    Owned(Vec<u8>),
}

impl CompactPath {
    pub fn as_bytes<'a>(&'a self, arena: &'a [u8]) -> &'a [u8] {
        match self {
            Self::Relative(range) => &arena[range.clone()],
            Self::Owned(path) => path,
        }
    }
}

/// One event in the logical byte-trie represented by an ACTree03 arena.
///
/// `Value` describes the current logical node, `Edge` descends to one child,
/// and `Leave` returns to the parent. A complete walk starts with the root's
/// `Value` and ends with the root's `Leave`. Compressed ACT line nodes are
/// expanded as borrowed edge bytes, so consumers receive exactly the event
/// sequence produced by an in-memory PathMap zipper without reconstructing a
/// PathMap or copying complete keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactTrieTopologyEvent {
    Value(bool),
    Edge(u8),
    Leave,
}

#[derive(Clone)]
struct ActNode {
    encoded_len: usize,
    value: Option<u64>,
    edge: ActEdge,
}

#[derive(Clone)]
enum ActEdge {
    Branch {
        first_child: Option<usize>,
        child_bytes: Vec<u8>,
    },
    Line {
        child: Option<usize>,
        suffix: Range<usize>,
        encoded_range: Range<usize>,
    },
}

struct ActFrame {
    node_id: usize,
    node: ActNode,
    path_len: usize,
    next_child: usize,
    next_child_id: Option<usize>,
    entered: bool,
}

struct ParsedAct {
    root_id: usize,
    nodes: HashMap<usize, ActNode>,
    semantic_nodes: HashMap<usize, SemanticActNode>,
}

#[derive(Clone)]
struct ActLineData {
    suffix: Range<usize>,
    encoded: Range<usize>,
}

struct SemanticActNode {
    value: Option<u64>,
    children: Vec<(u8, usize)>,
    line: Option<ActLineData>,
    wrapper: Option<usize>,
}

/// A structurally checked compact trie borrowed directly from its byte image.
pub struct CompactTrie<'a> {
    bytes: &'a [u8],
    parsed: ParsedAct,
}

impl<'a> CompactTrie<'a> {
    /// Parse the reachable graph and prove it is a finite tree. This admits
    /// structurally valid producer variants; use [`Self::parse_canonical`] at
    /// a consensus or persistence boundary.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, CompactTrieError> {
        Ok(Self {
            bytes,
            parsed: parse_act_tree(bytes)?,
        })
    }

    /// Parse and prove that the bytes are exactly an image of PathMap 0.3.4's
    /// deterministic compact writer (`ArenaCompactTree::from_zipper`).
    pub fn parse_canonical(bytes: &'a [u8]) -> Result<Self, CompactTrieError> {
        let parsed = parse_act_tree(bytes)?;
        validate_canonical_act(bytes, &parsed)?;
        Ok(Self { bytes, parsed })
    }

    /// True only for the compact image of a trie with no terminating path.
    pub fn is_empty(&self) -> bool {
        let root = &self.parsed.semantic_nodes[&self.parsed.root_id];
        root.line.is_none() && root.value.is_none() && root.children.is_empty()
    }

    /// Visit the complete logical byte topology in depth-first zipper order.
    ///
    /// Time is linear in logical trie nodes, heap state is linear only in ACT
    /// branch depth, and native-stack use is constant. A compressed line is
    /// read directly from the arena and emits one logical edge/node pair per
    /// byte. Value ordinals are intentionally erased to `bool`; callers that
    /// need value-table validation must run the mode-specific EPM1 validator
    /// before consuming these events.
    pub fn visit_topology<E>(
        &self,
        mut visitor: impl FnMut(CompactTrieTopologyEvent) -> Result<(), E>,
    ) -> Result<(), E> {
        enum Work {
            Visit(usize),
            Edge(u8),
            Leave(usize),
        }

        let mut work = vec![Work::Visit(self.parsed.root_id)];
        while let Some(next) = work.pop() {
            match next {
                Work::Visit(id) => {
                    let node = &self.parsed.semantic_nodes[&id];
                    let line = node
                        .line
                        .as_ref()
                        .map_or(&[][..], |line| &self.bytes[line.suffix.clone()]);

                    // A semantic line stores the value and branches at the
                    // final suffix node. The current node and every earlier
                    // suffix node are necessarily value-free.
                    visitor(CompactTrieTopologyEvent::Value(
                        line.is_empty() && node.value.is_some(),
                    ))?;
                    for (index, byte) in line.iter().copied().enumerate() {
                        visitor(CompactTrieTopologyEvent::Edge(byte))?;
                        visitor(CompactTrieTopologyEvent::Value(
                            index + 1 == line.len() && node.value.is_some(),
                        ))?;
                    }

                    work.push(Work::Leave(line.len() + 1));
                    for &(byte, child) in node.children.iter().rev() {
                        work.push(Work::Visit(child));
                        work.push(Work::Edge(byte));
                    }
                }
                Work::Edge(byte) => visitor(CompactTrieTopologyEvent::Edge(byte))?,
                Work::Leave(count) => {
                    for _ in 0..count {
                        visitor(CompactTrieTopologyEvent::Leave)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Visit every value-bearing node and explicit childless endpoint with a
    /// borrowed, contiguous path assembled on one reusable heap work stack.
    pub fn visit<E>(
        &self,
        mut visitor: impl FnMut(&[u8], Option<u64>) -> Result<(), E>,
    ) -> Result<(), E> {
        let root = self.parsed.nodes[&self.parsed.root_id].clone();
        let mut path = Vec::new();
        let mut stack = vec![ActFrame {
            node_id: self.parsed.root_id,
            next_child_id: first_child_id(&root),
            node: root,
            path_len: 0,
            next_child: 0,
            entered: false,
        }];

        while let Some(frame) = stack.last_mut() {
            let child_count = child_count(&frame.node);
            if !frame.entered {
                frame.entered = true;
                if let ActEdge::Line { suffix, .. } = &frame.node.edge {
                    path.extend_from_slice(&self.bytes[suffix.clone()]);
                }
                // Value-free internal nodes encode compression structure, not
                // terminating paths. A childless node is an explicit endpoint.
                if frame.node.value.is_some() || child_count == 0 {
                    visitor(&path, frame.node.value)?;
                }
            }

            if frame.next_child == child_count {
                let path_len = frame.path_len;
                stack.pop();
                path.truncate(path_len);
                continue;
            }

            let child_id = frame
                .next_child_id
                .expect("ACT parser proved the complete child sequence");
            let edge_byte = match &frame.node.edge {
                ActEdge::Branch { child_bytes, .. } => Some(child_bytes[frame.next_child]),
                ActEdge::Line { .. } => None,
            };
            let child = self.parsed.nodes[&child_id].clone();
            let next_id = child_id
                .checked_add(child.encoded_len)
                .expect("ACT parser rejected sibling offset overflow");
            frame.next_child += 1;
            frame.next_child_id = (frame.next_child < child_count).then_some(next_id);
            let path_len = path.len();
            if let Some(byte) = edge_byte {
                path.push(byte);
            }
            stack.push(ActFrame {
                node_id: child_id,
                next_child_id: first_child_id(&child),
                node: child,
                path_len,
                next_child: 0,
                entered: false,
            });
        }
        Ok(())
    }

    /// Visit canonical logical endpoints while retaining a single compressed
    /// line as a range into the arena. Branched paths are assembled only at an
    /// endpoint, so a deep one-entry trie incurs no copied key during inspect.
    pub fn visit_compact<E>(
        &self,
        mut visitor: impl FnMut(CompactPath, Option<u64>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut pieces = Vec::new();
        let mut stack = vec![KeyWalkFrame {
            id: self.parsed.root_id,
            next_child: 0,
            piece_len: 0,
            entered: false,
        }];
        while let Some(frame) = stack.last_mut() {
            let semantic = &self.parsed.semantic_nodes[&frame.id];
            if !frame.entered {
                frame.entered = true;
                if let Some(line) = &semantic.line {
                    pieces.push(KeyPiece::Slice(line.suffix.clone()));
                }
                if semantic.value.is_some() || semantic.children.is_empty() {
                    visitor(compact_path(self.bytes, &pieces), semantic.value)?;
                }
            }

            if frame.next_child == semantic.children.len() {
                let piece_len = frame.piece_len;
                stack.pop();
                pieces.truncate(piece_len);
                continue;
            }
            let (byte, child) = semantic.children[frame.next_child];
            frame.next_child += 1;
            let piece_len = pieces.len();
            pieces.push(KeyPiece::Byte(byte));
            stack.push(KeyWalkFrame {
                id: child,
                next_child: 0,
                piece_len,
                entered: false,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct DirectBranchByte {
    byte: u8,
    source_offset: usize,
}

#[derive(Clone)]
struct DirectBranchBytes(Range<usize>);

impl Default for DirectBranchBytes {
    fn default() -> Self { Self(0..0) }
}

impl DirectBranchBytes {
    fn len(&self) -> usize { self.0.len() }
}

#[derive(Clone)]
enum DirectActEdge {
    Branch {
        first_child: Option<usize>,
        child_bytes: DirectBranchBytes,
    },
    Line {
        child: Option<usize>,
        suffix: Range<usize>,
        encoded_range: Range<usize>,
    },
}

#[derive(Clone)]
struct DirectActNode {
    encoded_len: usize,
    value: Option<u64>,
    edge: DirectActEdge,
}

#[derive(Clone)]
struct DirectChildren {
    first_child: Option<usize>,
    child_bytes: DirectBranchBytes,
    parent_id: usize,
}

impl DirectChildren {
    fn empty(parent_id: usize) -> Self {
        Self {
            first_child: None,
            child_bytes: DirectBranchBytes::default(),
            parent_id,
        }
    }

    fn len(&self) -> usize { self.child_bytes.len() }
}

#[derive(Clone)]
struct DirectSemanticNode {
    id: usize,
    encoded_len: usize,
    value: Option<u64>,
    children: DirectChildren,
    line: Option<ActLineData>,
    wrapper: Option<(usize, usize)>,
}

struct DirectPlanValidationFrame {
    node: DirectSemanticNode,
    next_child: usize,
    next_child_id: Option<usize>,
    direct_children_encoded_len: usize,
    branch_checkpoint: usize,
}

#[derive(Clone, Copy)]
enum DirectTopologyStage {
    Value,
    LineEdge,
    LineValue,
    Children,
    Leave,
}

#[derive(Clone)]
struct DirectTopologyFrame {
    record: usize,
    remaining_children: usize,
    line_index: usize,
    leave_index: usize,
    stage: DirectTopologyStage,
}

struct DirectWalkFrame {
    record: usize,
    path_len: usize,
    remaining_children: usize,
    entered: bool,
}

#[derive(Clone)]
struct ValidatedTopologyRecord {
    node_id: usize,
    value: u64,
    value_source_offset: usize,
    line_start: usize,
    line_len: usize,
    incoming_source_offset: usize,
    child_count: u16,
    incoming_byte: u8,
    flags: u8,
}

/// Read-only scalar image of one record produced by ACT admission. Offsets
/// refer to the indexed ACT view owned by the cursor, not to a copied arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalCompactTrieValidatedRecordSource {
    pub node_id: usize,
    pub value: u64,
    pub value_source_offset: usize,
    pub line_start: usize,
    pub line_len: usize,
    pub incoming_source_offset: usize,
    pub child_count: u16,
    pub incoming_byte: u8,
    pub flags: u8,
}

impl From<&ValidatedTopologyRecord> for CanonicalCompactTrieValidatedRecordSource {
    fn from(record: &ValidatedTopologyRecord) -> Self {
        Self {
            node_id: record.node_id,
            value: record.value,
            value_source_offset: record.value_source_offset,
            line_start: record.line_start,
            line_len: record.line_len,
            incoming_source_offset: record.incoming_source_offset,
            child_count: record.child_count,
            incoming_byte: record.incoming_byte,
            flags: record.flags,
        }
    }
}

impl CanonicalCompactTrieValidatedRecordSource {
    pub const fn has_value(self) -> bool { self.flags & ValidatedTopologyRecord::HAS_VALUE != 0 }

    pub const fn has_incoming(self) -> bool {
        self.flags & ValidatedTopologyRecord::HAS_INCOMING != 0
    }
}

impl ValidatedTopologyRecord {
    const HAS_VALUE: u8 = 1;
    const HAS_INCOMING: u8 = 2;

    fn value(&self) -> Option<u64> { (self.flags & Self::HAS_VALUE != 0).then_some(self.value) }

    fn line_suffix(&self) -> Option<Range<usize>> {
        (self.line_len != 0).then_some(self.line_start..self.line_start + self.line_len)
    }

    fn child_count(&self) -> usize { usize::from(self.child_count) }

    fn incoming(&self) -> Option<(u8, usize)> {
        (self.flags & Self::HAS_INCOMING != 0)
            .then_some((self.incoming_byte, self.incoming_source_offset))
    }
}

/// Value-order facts proved while the physical ACT image is admitted.
///
/// Consumers such as EPM1 can apply their mode-specific rules after structural
/// admission without replaying the logical topology merely to rediscover its
/// ordinals. Offsets remain relative to the ACT image and can therefore be
/// mapped to an indexed source's physical provenance by the enclosing codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalCompactTrieValueSummary {
    value_count: usize,
    first_nonzero: Option<(usize, u64)>,
    first_nonsequential: Option<(usize, u64, u64)>,
}

impl CanonicalCompactTrieValueSummary {
    pub const fn value_count(self) -> usize { self.value_count }

    pub const fn first_nonzero(self) -> Option<(usize, u64)> { self.first_nonzero }

    pub const fn first_nonsequential(self) -> Option<(usize, u64, u64)> { self.first_nonsequential }
}

/// Work accounting for canonical ACT admission and logical event emission.
///
/// `node_reparses` and `event_metadata_source_reads` are explicit regression
/// sentinels: both remain zero in the validated-plan implementation.
#[cfg(any(test, feature = "act-codec-instrumentation"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalCompactTrieWorkCounters {
    pub semantic_node_parses: usize,
    pub wrapper_node_parses: usize,
    pub node_reparses: usize,
    pub sparse_branch_bytes: usize,
    pub dense_mask_bytes: usize,
    pub line_hash_bytes: usize,
    pub line_equality_bytes: usize,
    pub line_hash_collisions: usize,
    pub validation_source_byte_reads: usize,
    pub validation_source_copy_bytes: usize,
    pub validation_source_contiguous_bytes: usize,
    pub validation_source_equality_bytes: usize,
    pub event_metadata_source_reads: usize,
    pub event_line_source_reads: usize,
    pub plan_records: usize,
    pub plan_bytes: usize,
    pub plan_record_bytes: usize,
    pub validated_topology_state_bytes: usize,
    pub topology_cursor_bytes: usize,
    pub validation_branch_entries_peak: usize,
    pub validation_branch_bytes_peak: usize,
    pub validation_branch_capacity_bytes_peak: usize,
    pub validation_branch_allocations: usize,
    pub validation_frame_bytes_peak: usize,
    pub validation_frame_allocations: usize,
    pub validation_record_capacity_bytes_peak: usize,
    pub validation_record_allocations: usize,
    pub validation_seen_bitmap_bytes: usize,
    pub max_validation_depth: usize,
    pub max_event_depth: usize,
}

#[cfg(any(test, feature = "act-codec-instrumentation"))]
type ActWorkCounters = CanonicalCompactTrieWorkCounters;

#[cfg(not(any(test, feature = "act-codec-instrumentation")))]
struct ActWorkCounters;

#[cfg(any(test, feature = "act-codec-instrumentation"))]
fn new_act_work_counters() -> ActWorkCounters { ActWorkCounters::default() }

#[cfg(not(any(test, feature = "act-codec-instrumentation")))]
const fn new_act_work_counters() -> ActWorkCounters { ActWorkCounters }

struct ValidatedActPlan {
    empty: bool,
    topology_event_count: usize,
    records: Box<[ValidatedTopologyRecord]>,
    value_summary: CanonicalCompactTrieValueSummary,
    #[cfg(test)]
    audit_finish_node_ids: Vec<usize>,
    #[cfg(test)]
    audit_line_hashes: HashMap<usize, u64>,
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    counters: CanonicalCompactTrieWorkCounters,
}

/// A borrowed image of the accepted ACT plan and the exact indexed view it
/// was validated against. Access reads existing records; it adds no retained
/// cursor state and does not parse the indexed bytes again.
pub struct CanonicalCompactTrieValidatedPlanSource<'a, S> {
    plan: &'a ValidatedActPlan,
    indexed: &'a S,
}

impl<S> CanonicalCompactTrieValidatedPlanSource<'_, S> {
    pub fn indexed_source(&self) -> &S { self.indexed }

    pub fn is_empty(&self) -> bool { self.plan.empty }

    pub fn record_count(&self) -> usize { self.plan.records.len() }

    /// Exact scalar event count, checked when this plan was validated.
    pub fn topology_event_count(&self) -> usize { self.plan.topology_event_count }

    pub fn value_summary(&self) -> CanonicalCompactTrieValueSummary { self.plan.value_summary }

    pub fn record(&self, index: usize) -> Option<CanonicalCompactTrieValidatedRecordSource> {
        self.plan.records.get(index).map(Into::into)
    }
}

/// Each record emits Value and Leave; each non-root record also has one
/// incoming Edge. Every compressed-line byte emits Edge, Value, and Leave.
#[inline]
fn checked_add_topology_record_events(
    count: usize,
    line_len: usize,
    has_incoming: bool,
) -> Option<usize> {
    let record_events = line_len
        .checked_mul(3)?
        .checked_add(if has_incoming { 3 } else { 2 })?;
    count.checked_add(record_events)
}

fn checked_topology_event_count(records: &[ValidatedTopologyRecord]) -> Option<usize> {
    let (root, descendants) = records.split_first()?;
    let mut count = checked_add_topology_record_events(0, root.line_len, false)?;
    for record in descendants {
        count = checked_add_topology_record_events(count, record.line_len, true)?;
    }
    Some(count)
}

pub struct CanonicalCompactTrieEndpoint<'a> {
    pub path: &'a [u8],
    pub value: Option<u64>,
    /// Exact source range when the complete path is one borrowed ACT line.
    /// Branched paths are assembled in the cursor's reusable path buffer and
    /// therefore have no single contiguous source range.
    pub contiguous_path_range: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalCompactTrieTopologyEvent {
    Value {
        value: Option<u64>,
        source_offset: usize,
    },
    Edge {
        byte: u8,
        source_offset: usize,
    },
    Leave {
        source_offset: usize,
    },
}

struct ValidatedCompactTrieTopologyState {
    plan: ValidatedActPlan,
    next_record: usize,
    stack: Vec<DirectTopologyFrame>,
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    event_counters: CanonicalCompactTrieWorkCounters,
}

// These are production-layout budgets, evaluated only when measurement fields
// are absent. They prevent a diagnostic counter or ownership wrapper from
// silently returning to the hot cursor state.
#[cfg(not(any(test, feature = "act-codec-instrumentation")))]
const _: () = assert!(std::mem::size_of::<ValidatedActPlan>() <= 96);
#[cfg(not(any(test, feature = "act-codec-instrumentation")))]
const _: () = assert!(std::mem::size_of::<ValidatedCompactTrieTopologyState>() <= 128);

impl ValidatedCompactTrieTopologyState {
    fn new<S>(source: &S) -> Result<Self, CompactTrieError>
    where S: IndexedByteSource + ?Sized {
        let plan = validate_direct_canonical_act(source)?;
        #[cfg(any(test, feature = "act-codec-instrumentation"))]
        debug_assert_eq!(plan.counters.node_reparses, 0);
        #[cfg(any(test, feature = "act-codec-instrumentation"))]
        debug_assert_eq!(plan.counters.event_metadata_source_reads, 0);
        let mut state = Self {
            plan,
            next_record: 0,
            stack: Vec::new(),
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            event_counters: CanonicalCompactTrieWorkCounters::default(),
        };
        state.rewind();
        Ok(state)
    }

    fn is_empty(&self) -> bool { self.plan.empty }

    fn rewind(&mut self) {
        self.stack.clear();
        self.next_record = 1;
        let root = self
            .plan
            .records
            .first()
            .expect("canonical ACT validation produced a root record");
        self.stack.push(DirectTopologyFrame {
            record: 0,
            remaining_children: root.child_count(),
            line_index: 0,
            leave_index: 0,
            stage: DirectTopologyStage::Value,
        });
        #[cfg(any(test, feature = "act-codec-instrumentation"))]
        {
            self.event_counters.max_event_depth = self.event_counters.max_event_depth.max(1);
        }
    }

    fn value_summary(&self) -> CanonicalCompactTrieValueSummary { self.plan.value_summary }

    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    fn work_counters(&self) -> CanonicalCompactTrieWorkCounters {
        let mut counters = self.plan.counters;
        counters.event_metadata_source_reads = self.event_counters.event_metadata_source_reads;
        counters.event_line_source_reads = self.event_counters.event_line_source_reads;
        counters.max_event_depth = self.event_counters.max_event_depth;
        counters
    }

    fn next_event<S>(
        &mut self,
        source: &S,
    ) -> Result<Option<CanonicalCompactTrieTopologyEvent>, CompactTrieError>
    where
        S: IndexedByteSource + ?Sized,
    {
        enum Action {
            Event(CanonicalCompactTrieTopologyEvent),
            LineEdge {
                source_offset: usize,
            },
            Descend {
                event: CanonicalCompactTrieTopologyEvent,
                record: usize,
            },
            Pop(CanonicalCompactTrieTopologyEvent),
            Continue,
        }

        loop {
            let action = {
                let Some(frame) = self.stack.last_mut() else {
                    #[cfg(any(test, feature = "act-codec-instrumentation"))]
                    debug_assert_eq!(self.event_counters.node_reparses, 0);
                    #[cfg(any(test, feature = "act-codec-instrumentation"))]
                    debug_assert_eq!(self.event_counters.event_metadata_source_reads, 0);
                    return Ok(None);
                };
                let record = &self.plan.records[frame.record];
                match frame.stage {
                    DirectTopologyStage::Value => {
                        frame.stage = if record.line_suffix().is_some() {
                            DirectTopologyStage::LineEdge
                        } else {
                            DirectTopologyStage::Children
                        };
                        Action::Event(CanonicalCompactTrieTopologyEvent::Value {
                            value: record
                                .line_suffix()
                                .is_none()
                                .then(|| record.value())
                                .flatten(),
                            source_offset: record.node_id,
                        })
                    }
                    DirectTopologyStage::LineEdge => {
                        let line = record
                            .line_suffix()
                            .expect("line stage has canonical line data");
                        let source_offset = line.start + frame.line_index;
                        frame.stage = DirectTopologyStage::LineValue;
                        Action::LineEdge { source_offset }
                    }
                    DirectTopologyStage::LineValue => {
                        let line = record
                            .line_suffix()
                            .expect("line stage has canonical line data");
                        let line_offset = line.start + frame.line_index;
                        let final_value = frame.line_index + 1 == line.len();
                        let value = final_value.then(|| record.value()).flatten();
                        let source_offset = if value.is_some() {
                            record.value_source_offset
                        } else {
                            line_offset
                        };
                        frame.line_index += 1;
                        frame.stage = if frame.line_index < line.len() {
                            DirectTopologyStage::LineEdge
                        } else {
                            DirectTopologyStage::Children
                        };
                        Action::Event(CanonicalCompactTrieTopologyEvent::Value {
                            value,
                            source_offset,
                        })
                    }
                    DirectTopologyStage::Children => {
                        if frame.remaining_children == 0 {
                            frame.stage = DirectTopologyStage::Leave;
                            Action::Continue
                        } else {
                            let record_index = self.next_record;
                            let child = self.plan.records.get(record_index).expect(
                                "canonical ACT preorder plan contains every declared child",
                            );
                            let incoming = child
                                .incoming()
                                .expect("a non-root ACT plan record retains its incoming edge");
                            self.next_record += 1;
                            frame.remaining_children -= 1;
                            Action::Descend {
                                event: CanonicalCompactTrieTopologyEvent::Edge {
                                    byte: incoming.0,
                                    source_offset: incoming.1,
                                },
                                record: record_index,
                            }
                        }
                    }
                    DirectTopologyStage::Leave => {
                        let line = record.line_suffix();
                        let line_len = line.as_ref().map_or(0, Range::len);
                        let source_offset = if frame.leave_index < line_len {
                            line.as_ref()
                                .expect("line leave has canonical line data")
                                .end -
                                1 -
                                frame.leave_index
                        } else {
                            record.node_id
                        };
                        frame.leave_index += 1;
                        let event = CanonicalCompactTrieTopologyEvent::Leave { source_offset };
                        if frame.leave_index == line_len + 1 {
                            Action::Pop(event)
                        } else {
                            Action::Event(event)
                        }
                    }
                }
            };

            match action {
                Action::Event(event) => return Ok(Some(event)),
                Action::LineEdge { source_offset } => {
                    let byte = source.byte_at(source_offset).ok_or_else(|| {
                        CompactTrieError::at(
                            source_offset,
                            "ACTree03 line suffix byte is unavailable",
                        )
                    })?;
                    #[cfg(any(test, feature = "act-codec-instrumentation"))]
                    {
                        self.event_counters.event_line_source_reads += 1;
                    }
                    return Ok(Some(CanonicalCompactTrieTopologyEvent::Edge {
                        byte,
                        source_offset,
                    }));
                }
                Action::Descend { event, record } => {
                    let child = &self.plan.records[record];
                    self.stack.push(DirectTopologyFrame {
                        record,
                        remaining_children: child.child_count(),
                        line_index: 0,
                        leave_index: 0,
                        stage: DirectTopologyStage::Value,
                    });
                    #[cfg(any(test, feature = "act-codec-instrumentation"))]
                    {
                        self.event_counters.max_event_depth =
                            self.event_counters.max_event_depth.max(self.stack.len());
                    }
                    return Ok(Some(event));
                }
                Action::Pop(event) => {
                    self.stack.pop();
                    return Ok(Some(event));
                }
                Action::Continue => {}
            }
        }
    }
}

/// Source-owning canonical topology cursor for fragmented or persistent views.
///
/// Owning the indexed view makes it impossible to substitute a same-length
/// source after admission. The inline source is private and exposed only by
/// shared reference for physical-offset/provenance mapping.
pub struct CanonicalCompactTrieIndexedTopologyCursor<S> {
    source: S,
    state: ValidatedCompactTrieTopologyState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalCompactTrieLineDirection {
    Forward,
    Unwind,
}

/// The active validated-plan position, without copying the plan or cursor
/// stack. A `None` position means the topology cursor has emitted its final
/// event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalCompactTrieCursorSourcePosition {
    pub record: usize,
    pub node_id: usize,
    pub next_record: usize,
    pub stack_depth: usize,
    pub remaining_children: usize,
    pub stage: CanonicalCompactTrieCursorSourceStage,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub line_index: usize,
    pub leave_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalCompactTrieCursorSourceStage {
    Value,
    LineEdge,
    LineValue,
    Children,
    Leave,
}

/// The active line's remaining plan-work prefix at the enclosing cursor's
/// event index. `event_index` comes from the enclosing event cursor; this ACT
/// cursor does not maintain a second global event counter. The source offsets
/// are relative to its owned indexed byte view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalCompactTrieActiveLineWorkSource {
    pub position: CanonicalCompactTrieCursorSourcePosition,
    pub event_index: usize,
    pub direction: CanonicalCompactTrieLineDirection,
    /// The full offered line suffix, including bytes beyond event headroom.
    pub source_range: Range<usize>,
    /// Full suffix event count, when representable in `usize`.
    pub remaining_work_events: Option<usize>,
    /// Largest prefix whose event count can be added to `event_index`.
    pub admissible_source_width: usize,
    pub admissible_work_events: usize,
}

impl CanonicalCompactTrieActiveLineWorkSource {
    pub fn first_source_offset(&self) -> usize {
        match self.direction {
            CanonicalCompactTrieLineDirection::Forward => self.source_range.start,
            CanonicalCompactTrieLineDirection::Unwind => self.source_range.end - 1,
        }
    }

    pub fn last_source_offset(&self) -> usize {
        match self.direction {
            CanonicalCompactTrieLineDirection::Forward => self.source_range.end - 1,
            CanonicalCompactTrieLineDirection::Unwind => self.source_range.start,
        }
    }
}

impl CanonicalCompactTrieCursorSourceStage {
    fn from_internal(stage: DirectTopologyStage) -> Self {
        match stage {
            DirectTopologyStage::Value => Self::Value,
            DirectTopologyStage::LineEdge => Self::LineEdge,
            DirectTopologyStage::LineValue => Self::LineValue,
            DirectTopologyStage::Children => Self::Children,
            DirectTopologyStage::Leave => Self::Leave,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CanonicalCompactTrieLineCommit {
    source_range: Range<usize>,
    direction: CanonicalCompactTrieLineDirection,
    expanded_events: usize,
}

impl CanonicalCompactTrieLineCommit {
    pub fn source_range(&self) -> Range<usize> { self.source_range.clone() }

    pub fn direction(&self) -> CanonicalCompactTrieLineDirection { self.direction }

    pub fn expanded_events(&self) -> usize { self.expanded_events }

    pub fn last_source_offset(&self) -> usize {
        match self.direction {
            CanonicalCompactTrieLineDirection::Forward => self.source_range.end - 1,
            CanonicalCompactTrieLineDirection::Unwind => self.source_range.start,
        }
    }

    /// Check the exact validated-plan positions covered by this publication.
    /// Forward emits Edge, Value(None) at each offset in ascending order;
    /// Unwind emits Leave at each offset in descending order. Both stop before
    /// the line's final Value or the record's final Leave, respectively.
    pub fn matches_scalar_schedule(
        &self,
        before: CanonicalCompactTrieCursorSourcePosition,
        after: CanonicalCompactTrieCursorSourcePosition,
    ) -> bool {
        let Some(line_start) = before.line_start else {
            return false;
        };
        let Some(line_end) = before.line_end else {
            return false;
        };
        let width = self.source_range.len();
        if width == 0 ||
            before.record != after.record ||
            before.node_id != after.node_id ||
            before.next_record != after.next_record ||
            before.stack_depth != after.stack_depth ||
            before.remaining_children != after.remaining_children ||
            before.line_start != after.line_start ||
            before.line_end != after.line_end ||
            self.source_range.start < line_start ||
            self.source_range.end > line_end
        {
            return false;
        }
        match self.direction {
            CanonicalCompactTrieLineDirection::Forward => {
                before.stage == CanonicalCompactTrieCursorSourceStage::LineEdge &&
                    after.stage == CanonicalCompactTrieCursorSourceStage::LineEdge &&
                    line_start.checked_add(before.line_index) == Some(self.source_range.start) &&
                    line_end
                        .checked_sub(1)
                        .is_some_and(|end| self.source_range.end <= end) &&
                    before.line_index.checked_add(width) == Some(after.line_index) &&
                    before.leave_index == after.leave_index &&
                    width.checked_mul(2) == Some(self.expanded_events)
            }
            CanonicalCompactTrieLineDirection::Unwind => {
                matches!(
                    before.stage,
                    CanonicalCompactTrieCursorSourceStage::Leave |
                        CanonicalCompactTrieCursorSourceStage::Children
                ) && (before.stage != CanonicalCompactTrieCursorSourceStage::Children ||
                    before.remaining_children == 0) &&
                    after.stage == CanonicalCompactTrieCursorSourceStage::Leave &&
                    line_end.checked_sub(before.leave_index) == Some(self.source_range.end) &&
                    before.leave_index.checked_add(width) == Some(after.leave_index) &&
                    before.line_index == after.line_index &&
                    self.expanded_events == width
            }
        }
    }
}

pub struct CanonicalCompactTrieLineSpan<'a, S> {
    cursor: &'a mut CanonicalCompactTrieIndexedTopologyCursor<S>,
    source_range: Range<usize>,
    direction: CanonicalCompactTrieLineDirection,
}

pub struct CanonicalCompactTriePreparedLineSpan<'a, S> {
    source: &'a S,
    frame: &'a mut DirectTopologyFrame,
    publication: CanonicalCompactTrieLineCommit,
    next_index: usize,
}

impl<'a, S> CanonicalCompactTrieLineSpan<'a, S> {
    pub fn source(&self) -> &S { &self.cursor.source }

    pub fn source_range(&self) -> Range<usize> { self.source_range.clone() }

    pub fn direction(&self) -> CanonicalCompactTrieLineDirection { self.direction }

    pub fn prepare(self, width: usize) -> Option<CanonicalCompactTriePreparedLineSpan<'a, S>> {
        if width == 0 || width > self.source_range.len() {
            return None;
        }
        let expanded_events = match self.direction {
            CanonicalCompactTrieLineDirection::Forward => width.checked_mul(2)?,
            CanonicalCompactTrieLineDirection::Unwind => width,
        };
        let frame = self.cursor.state.stack.last_mut()?;
        let (source_range, next_index) = match self.direction {
            CanonicalCompactTrieLineDirection::Forward => {
                let end = self.source_range.start.checked_add(width)?;
                let next_index = frame.line_index.checked_add(width)?;
                (self.source_range.start..end, next_index)
            }
            CanonicalCompactTrieLineDirection::Unwind => {
                let start = self.source_range.end.checked_sub(width)?;
                let next_index = frame.leave_index.checked_add(width)?;
                (start..self.source_range.end, next_index)
            }
        };
        Some(CanonicalCompactTriePreparedLineSpan {
            source: &self.cursor.source,
            frame,
            publication: CanonicalCompactTrieLineCommit {
                source_range,
                direction: self.direction,
                expanded_events,
            },
            next_index,
        })
    }

    pub fn commit(self, width: usize) -> Option<CanonicalCompactTrieLineCommit> {
        Some(self.prepare(width)?.commit())
    }
}

impl<S> CanonicalCompactTriePreparedLineSpan<'_, S> {
    pub fn source(&self) -> &S { self.source }

    pub fn source_range(&self) -> Range<usize> { self.publication.source_range() }

    pub fn direction(&self) -> CanonicalCompactTrieLineDirection { self.publication.direction() }

    pub fn expanded_events(&self) -> usize { self.publication.expanded_events() }

    pub fn last_source_offset(&self) -> usize { self.publication.last_source_offset() }

    pub fn commit(self) -> CanonicalCompactTrieLineCommit {
        match self.publication.direction {
            CanonicalCompactTrieLineDirection::Forward => {
                self.frame.line_index = self.next_index;
            }
            CanonicalCompactTrieLineDirection::Unwind => {
                self.frame.leave_index = self.next_index;
                self.frame.stage = DirectTopologyStage::Leave;
            }
        }
        self.publication
    }
}

impl<S> CanonicalCompactTrieIndexedTopologyCursor<S>
where S: IndexedByteSource
{
    pub fn from_source(source: S) -> Result<Self, CompactTrieError> {
        let state = ValidatedCompactTrieTopologyState::new(&source)?;
        Ok(Self { source, state })
    }

    pub fn is_empty(&self) -> bool { self.state.is_empty() }

    pub fn source(&self) -> &S { &self.source }

    pub fn validated_plan_source(&self) -> CanonicalCompactTrieValidatedPlanSource<'_, S> {
        CanonicalCompactTrieValidatedPlanSource {
            plan: &self.state.plan,
            indexed: &self.source,
        }
    }

    pub fn source_position(&self) -> Option<CanonicalCompactTrieCursorSourcePosition> {
        let frame = self.state.stack.last()?;
        let record = &self.state.plan.records[frame.record];
        let line = record.line_suffix();
        Some(CanonicalCompactTrieCursorSourcePosition {
            record: frame.record,
            node_id: record.node_id,
            next_record: self.state.next_record,
            stack_depth: self.state.stack.len(),
            remaining_children: frame.remaining_children,
            stage: CanonicalCompactTrieCursorSourceStage::from_internal(frame.stage),
            line_start: line.as_ref().map(|range| range.start),
            line_end: line.as_ref().map(|range| range.end),
            line_index: frame.line_index,
            leave_index: frame.leave_index,
        })
    }

    /// Observe the plan record and the emitted prefix still available for a
    /// line batch. This reads only the active frame and its validated record;
    /// it does not copy the plan, traverse source bytes, or advance the cursor.
    pub fn active_line_work_source(
        &self,
        event_index: usize,
    ) -> Option<CanonicalCompactTrieActiveLineWorkSource> {
        let position = self.source_position()?;
        let line_start = position.line_start?;
        let line_end = position.line_end?;
        let (source_range, direction) = match position.stage {
            CanonicalCompactTrieCursorSourceStage::LineEdge => (
                line_start.checked_add(position.line_index)?..line_end.checked_sub(1)?,
                CanonicalCompactTrieLineDirection::Forward,
            ),
            CanonicalCompactTrieCursorSourceStage::Leave => (
                line_start..line_end.checked_sub(position.leave_index)?,
                CanonicalCompactTrieLineDirection::Unwind,
            ),
            CanonicalCompactTrieCursorSourceStage::Children if position.remaining_children == 0 => {
                (
                    line_start..line_end.checked_sub(position.leave_index)?,
                    CanonicalCompactTrieLineDirection::Unwind,
                )
            }
            _ => return None,
        };
        if source_range.is_empty() {
            return None;
        }
        let (remaining_work_events, admissible_source_width, admissible_work_events) =
            match direction {
                CanonicalCompactTrieLineDirection::Forward => {
                    let width = source_range.len().min((usize::MAX - event_index) / 2);
                    (source_range.len().checked_mul(2), width, width * 2)
                }
                CanonicalCompactTrieLineDirection::Unwind => {
                    let width = source_range.len().min(usize::MAX - event_index);
                    (Some(source_range.len()), width, width)
                }
            };
        Some(CanonicalCompactTrieActiveLineWorkSource {
            position,
            event_index,
            direction,
            source_range,
            remaining_work_events,
            admissible_source_width,
            admissible_work_events,
        })
    }

    pub fn rewind(&mut self) { self.state.rewind(); }

    pub fn line_span(&mut self) -> Option<CanonicalCompactTrieLineSpan<'_, S>> {
        let frame = self.state.stack.last()?;
        let line = self.state.plan.records[frame.record].line_suffix()?;
        let (source_range, direction) = match frame.stage {
            DirectTopologyStage::LineEdge => (
                line.start.checked_add(frame.line_index)?..line.end.checked_sub(1)?,
                CanonicalCompactTrieLineDirection::Forward,
            ),
            DirectTopologyStage::Leave => (
                line.start..line.end.checked_sub(frame.leave_index)?,
                CanonicalCompactTrieLineDirection::Unwind,
            ),
            DirectTopologyStage::Children if frame.remaining_children == 0 => (
                line.start..line.end.checked_sub(frame.leave_index)?,
                CanonicalCompactTrieLineDirection::Unwind,
            ),
            _ => return None,
        };
        if source_range.is_empty() {
            return None;
        }
        Some(CanonicalCompactTrieLineSpan {
            cursor: self,
            source_range,
            direction,
        })
    }

    pub fn next_event(
        &mut self,
    ) -> Result<Option<CanonicalCompactTrieTopologyEvent>, CompactTrieError> {
        self.state.next_event(&self.source)
    }

    pub fn value_summary(&self) -> CanonicalCompactTrieValueSummary { self.state.value_summary() }

    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    pub fn work_counters(&self) -> CanonicalCompactTrieWorkCounters {
        let mut counters = self.state.work_counters();
        counters.topology_cursor_bytes = size_of::<Self>();
        counters
    }
}

pub struct CanonicalCompactTrieTopologyCursor<B> {
    bytes: B,
    state: ValidatedCompactTrieTopologyState,
}

impl<'a> CanonicalCompactTrieTopologyCursor<&'a [u8]> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, CompactTrieError> { Self::from_backing(bytes) }
}

impl<B> CanonicalCompactTrieTopologyCursor<B>
where B: AsRef<[u8]>
{
    pub fn from_backing(bytes: B) -> Result<Self, CompactTrieError> {
        let state = {
            let source = AsRefIndexedByteSource::new(&bytes);
            ValidatedCompactTrieTopologyState::new(&source)?
        };
        Ok(Self { bytes, state })
    }

    pub fn is_empty(&self) -> bool { self.state.is_empty() }

    pub fn rewind(&mut self) { self.state.rewind(); }

    pub fn next_event(&mut self) -> Option<CanonicalCompactTrieTopologyEvent> {
        let source = AsRefIndexedByteSource::new(&self.bytes);
        self.state
            .next_event(&source)
            .expect("canonical ACT remains valid while owned")
    }

    pub fn value_summary(&self) -> CanonicalCompactTrieValueSummary { self.state.value_summary() }

    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    pub fn work_counters(&self) -> CanonicalCompactTrieWorkCounters {
        let mut counters = self.state.work_counters();
        counters.topology_cursor_bytes = size_of::<Self>();
        counters
    }
}

pub struct CanonicalCompactTrieCursor<B> {
    bytes: B,
    plan: ValidatedActPlan,
    next_record: usize,
    stack: Vec<DirectWalkFrame>,
    path: Vec<u8>,
}

impl<'a> CanonicalCompactTrieCursor<&'a [u8]> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, CompactTrieError> { Self::from_backing(bytes) }
}

impl<B> CanonicalCompactTrieCursor<B>
where B: AsRef<[u8]>
{
    pub fn from_backing(bytes: B) -> Result<Self, CompactTrieError> {
        let plan = {
            let source = AsRefIndexedByteSource::new(&bytes);
            validate_direct_canonical_act(&source)?
        };
        let mut cursor = Self {
            bytes,
            plan,
            next_record: 0,
            stack: Vec::new(),
            path: Vec::new(),
        };
        cursor.rewind();
        Ok(cursor)
    }

    pub fn is_empty(&self) -> bool { self.plan.empty }

    pub fn value_summary(&self) -> CanonicalCompactTrieValueSummary { self.plan.value_summary }

    pub fn rewind(&mut self) {
        self.path.clear();
        self.stack.clear();
        self.next_record = 1;
        let root = self
            .plan
            .records
            .first()
            .expect("canonical ACT validation produced a root record");
        self.stack.push(DirectWalkFrame {
            record: 0,
            path_len: 0,
            remaining_children: root.child_count(),
            entered: false,
        });
    }

    pub fn next_endpoint(&mut self) -> Option<CanonicalCompactTrieEndpoint<'_>> {
        self.next_endpoint_matching(false)
    }

    pub fn next_value_endpoint(&mut self) -> Option<CanonicalCompactTrieEndpoint<'_>> {
        self.next_endpoint_matching(true)
    }

    fn next_endpoint_matching(
        &mut self,
        value_only: bool,
    ) -> Option<CanonicalCompactTrieEndpoint<'_>> {
        enum Action {
            Yield {
                value: Option<u64>,
                contiguous_path_range: Option<Range<usize>>,
            },
            Pop(usize),
            Push {
                record: usize,
                edge: u8,
                path_len: usize,
            },
            Continue,
        }

        loop {
            let is_root = self.stack.len() == 1;
            let action = {
                let frame = self.stack.last_mut()?;
                let record = &self.plan.records[frame.record];
                if !frame.entered {
                    frame.entered = true;
                    if let Some(line) = record.line_suffix() {
                        self.path.extend_from_slice(&self.bytes.as_ref()[line]);
                    }
                    if record.value().is_some() || record.child_count() == 0 {
                        if !value_only || record.value().is_some() {
                            Action::Yield {
                                value: record.value(),
                                contiguous_path_range: is_root
                                    .then(|| record.line_suffix())
                                    .flatten(),
                            }
                        } else {
                            Action::Continue
                        }
                    } else {
                        Action::Continue
                    }
                } else if frame.remaining_children == 0 {
                    Action::Pop(frame.path_len)
                } else {
                    let record_index = self.next_record;
                    let child = self
                        .plan
                        .records
                        .get(record_index)
                        .expect("canonical ACT preorder plan contains every declared child");
                    let edge = child
                        .incoming()
                        .expect("a non-root ACT plan record retains its incoming edge")
                        .0;
                    self.next_record += 1;
                    frame.remaining_children -= 1;
                    Action::Push {
                        record: record_index,
                        edge,
                        path_len: self.path.len(),
                    }
                }
            };

            match action {
                Action::Yield {
                    value,
                    contiguous_path_range,
                } => {
                    return Some(CanonicalCompactTrieEndpoint {
                        path: &self.path,
                        value,
                        contiguous_path_range,
                    });
                }
                Action::Pop(path_len) => {
                    self.stack.pop();
                    self.path.truncate(path_len);
                }
                Action::Push {
                    record,
                    edge,
                    path_len,
                } => {
                    self.path.push(edge);
                    let child = &self.plan.records[record];
                    self.stack.push(DirectWalkFrame {
                        record,
                        path_len,
                        remaining_children: child.child_count(),
                        entered: false,
                    });
                }
                Action::Continue => {}
            }
        }
    }
}

fn take_indexed<S>(
    bytes: &S,
    cursor: &mut usize,
    len: usize,
    label: &str,
) -> Result<Range<usize>, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let start = *cursor;
    let end = start
        .checked_add(len)
        .ok_or_else(|| CompactTrieError::at(start, format!("{label} length overflow")))?;
    if end > bytes.len() {
        return Err(CompactTrieError::at(start, format!("truncated {label}")));
    }
    *cursor = end;
    Ok(start..end)
}

fn read_indexed_act_varint<S>(
    bytes: &S,
    cursor: &mut usize,
    label: &str,
) -> Result<u64, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let first_offset = *cursor;
    let first = bytes
        .byte_at(first_offset)
        .ok_or_else(|| CompactTrieError::at(first_offset, format!("truncated {label}")))?;
    *cursor += 1;
    if first <= ACT_VARINT_BIAS {
        return Ok(first as u64);
    }
    let len = (first - ACT_VARINT_BIAS) as usize;
    if !(1..=8).contains(&len) {
        return Err(CompactTrieError::at(first_offset, format!("invalid {label} length")));
    }
    let raw = take_indexed(bytes, cursor, len, label)?;
    if bytes.byte_at(raw.end - 1) == Some(0) {
        return Err(CompactTrieError::at(raw.end - 1, format!("non-canonical {label}")));
    }
    let mut full = [0u8; 8];
    if !bytes.copy_exact(raw.start, &mut full[..len]) {
        return Err(CompactTrieError::at(raw.start, format!("truncated {label}")));
    }
    let value = u64::from_le_bytes(full);
    if value <= ACT_VARINT_BIAS as u64 {
        return Err(CompactTrieError::at(first_offset, format!("non-canonical {label}")));
    }
    Ok(value)
}

fn reserve_direct_branch_edges(
    edges: &mut Vec<DirectBranchByte>,
    additional: usize,
    _counters: &mut ActWorkCounters,
) {
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    let capacity = edges.capacity();
    edges.reserve(additional);
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    if edges.capacity() != capacity {
        _counters.validation_branch_allocations += 1;
        _counters.validation_branch_capacity_bytes_peak =
            _counters.validation_branch_capacity_bytes_peak.max(
                edges
                    .capacity()
                    .saturating_mul(size_of::<DirectBranchByte>()),
            );
    }
}

fn push_direct_branch_edge(
    edges: &mut Vec<DirectBranchByte>,
    edge: DirectBranchByte,
    _counters: &mut ActWorkCounters,
) {
    edges.push(edge);
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        _counters.validation_branch_entries_peak =
            _counters.validation_branch_entries_peak.max(edges.len());
        _counters.validation_branch_bytes_peak = _counters
            .validation_branch_bytes_peak
            .max(edges.len().saturating_mul(size_of::<DirectBranchByte>()));
    }
}

fn parse_direct_act_node<S>(
    bytes: &S,
    node_id: usize,
    branch_edges: &mut Vec<DirectBranchByte>,
    counters: &mut ActWorkCounters,
) -> Result<DirectActNode, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let head = bytes.byte_at(node_id).ok_or_else(|| {
        CompactTrieError::at(node_id, "ACTree03 node offset is outside the arena")
    })?;
    let mut cursor = node_id + 1;
    let value = if head & ACT_VALUE_FLAG != 0 {
        Some(read_indexed_act_varint(bytes, &mut cursor, "ACTree03 node value")?)
    } else {
        None
    };

    let edge = if head & ACT_LINE_FLAG == 0 {
        let encoded_count = (head & 0x3f) as usize;
        if encoded_count > 32 {
            return Err(CompactTrieError::new("ACTree03 branch child count exceeds 32 marker"));
        }
        let first_child = if encoded_count == 0 {
            None
        } else {
            Some(relative_target(
                node_id,
                read_indexed_act_varint(bytes, &mut cursor, "ACTree03 first-child offset")?,
                "ACTree03 first-child offset",
            )?)
        };
        let child_start = branch_edges.len();
        let child_bytes = if encoded_count < 32 {
            let range = take_indexed(bytes, &mut cursor, encoded_count, "ACTree03 child bytes")?;
            reserve_direct_branch_edges(branch_edges, encoded_count, counters);
            let mut previous = None;
            for offset in range {
                let current = bytes.byte_at(offset).ok_or_else(|| {
                    CompactTrieError::at(offset, "truncated ACTree03 child bytes")
                })?;
                #[cfg(any(test, feature = "act-codec-instrumentation"))]
                {
                    counters.sparse_branch_bytes += 1;
                }
                if previous.is_some_and(|previous| previous >= current) {
                    return Err(CompactTrieError::at(
                        offset,
                        "ACTree03 branch bytes are not strictly ascending",
                    ));
                }
                push_direct_branch_edge(
                    branch_edges,
                    DirectBranchByte {
                        byte: current,
                        source_offset: offset,
                    },
                    counters,
                );
                previous = Some(current);
            }
            DirectBranchBytes(child_start..branch_edges.len())
        } else {
            let mask = take_indexed(bytes, &mut cursor, 32, "ACTree03 child mask")?;
            reserve_direct_branch_edges(branch_edges, 256, counters);
            for (word, offset) in mask.enumerate() {
                let mask_byte = bytes
                    .byte_at(offset)
                    .ok_or_else(|| CompactTrieError::at(offset, "truncated ACTree03 child mask"))?;
                #[cfg(any(test, feature = "act-codec-instrumentation"))]
                {
                    counters.dense_mask_bytes += 1;
                }
                for bit in 0..8u8 {
                    if mask_byte & (1 << bit) != 0 {
                        push_direct_branch_edge(
                            branch_edges,
                            DirectBranchByte {
                                byte: (word * 8 + usize::from(bit)) as u8,
                                source_offset: offset,
                            },
                            counters,
                        );
                    }
                }
            }
            if branch_edges.len() - child_start < 32 {
                return Err(CompactTrieError::new(
                    "ACTree03 dense child mask contains fewer than 32 branches",
                ));
            }
            DirectBranchBytes(child_start..branch_edges.len())
        };
        if (child_bytes.len() == 0) != first_child.is_none() {
            return Err(CompactTrieError::new("ACTree03 branch child metadata disagrees"));
        }
        DirectActEdge::Branch {
            first_child,
            child_bytes,
        }
    } else {
        if head & 0x3e != 0 {
            return Err(CompactTrieError::new("ACTree03 line node has reserved header bits set"));
        }
        let child = if head & 1 != 0 {
            Some(relative_target(
                node_id,
                read_indexed_act_varint(bytes, &mut cursor, "ACTree03 line-child offset")?,
                "ACTree03 line-child offset",
            )?)
        } else {
            None
        };
        let line_id = relative_target(
            node_id,
            read_indexed_act_varint(bytes, &mut cursor, "ACTree03 line-data offset")?,
            "ACTree03 line-data offset",
        )?;
        let mut line_cursor = line_id;
        let line_len = read_indexed_act_varint(bytes, &mut line_cursor, "ACTree03 line length")?;
        let line_len = usize::try_from(line_len)
            .map_err(|_| CompactTrieError::new("ACTree03 line length overflows usize"))?;
        if line_len == 0 {
            return Err(CompactTrieError::new("ACTree03 line suffix is empty"));
        }
        let suffix_start = line_cursor;
        take_indexed(bytes, &mut line_cursor, line_len, "ACTree03 line suffix")?;
        if line_cursor > node_id {
            return Err(CompactTrieError::new("ACTree03 line data overlaps its node"));
        }
        DirectActEdge::Line {
            child,
            suffix: suffix_start..line_cursor,
            encoded_range: line_id..line_cursor,
        }
    };

    Ok(DirectActNode {
        encoded_len: cursor - node_id,
        value,
        edge,
    })
}

fn direct_semantic_node<S>(
    bytes: &S,
    id: usize,
    branch_edges: &mut Vec<DirectBranchByte>,
    counters: &mut ActWorkCounters,
) -> Result<DirectSemanticNode, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        counters.semantic_node_parses += 1;
    }
    let node = parse_direct_act_node(bytes, id, branch_edges, counters)?;
    let encoded_len = node.encoded_len;
    let semantic = match node.edge {
        DirectActEdge::Branch {
            first_child,
            child_bytes,
        } => DirectSemanticNode {
            id,
            encoded_len,
            value: node.value,
            children: DirectChildren {
                first_child,
                child_bytes,
                parent_id: id,
            },
            line: None,
            wrapper: None,
        },
        DirectActEdge::Line {
            child: None,
            suffix,
            encoded_range,
        } => DirectSemanticNode {
            id,
            encoded_len,
            value: node.value,
            children: DirectChildren::empty(id),
            line: Some(ActLineData {
                suffix,
                encoded: encoded_range,
            }),
            wrapper: None,
        },
        DirectActEdge::Line {
            child: Some(wrapper_id),
            suffix,
            encoded_range,
        } => {
            if node.value.is_some() {
                return Err(CompactTrieError::new(
                    "ACTree03 line node has both a value and a child",
                ));
            }
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            {
                counters.wrapper_node_parses += 1;
            }
            let wrapper = parse_direct_act_node(bytes, wrapper_id, branch_edges, counters)?;
            let wrapper_len = wrapper.encoded_len;
            let DirectActEdge::Branch {
                first_child,
                child_bytes,
            } = wrapper.edge
            else {
                return Err(CompactTrieError::new("ACTree03 line child is not a branch node"));
            };
            DirectSemanticNode {
                id,
                encoded_len,
                value: wrapper.value,
                children: DirectChildren {
                    first_child,
                    child_bytes,
                    parent_id: wrapper_id,
                },
                line: Some(ActLineData {
                    suffix,
                    encoded: encoded_range,
                }),
                wrapper: Some((wrapper_id, wrapper_len)),
            }
        }
    };

    if semantic.value.is_none() && semantic.children.len() == 1 {
        return Err(CompactTrieError::new(
            "ACTree03 contains an uncompressed value-free unary branch",
        ));
    }
    if semantic.wrapper.is_some() && semantic.children.len() == 0 {
        return Err(CompactTrieError::new("ACTree03 line branch wrapper has no children"));
    }
    Ok(semantic)
}

fn register_direct_semantic_node(
    seen: &mut [u8],
    node: &DirectSemanticNode,
) -> Result<(), CompactTrieError> {
    fn insert(seen: &mut [u8], offset: usize) -> bool {
        let byte = &mut seen[offset / 8];
        let mask = 1 << (offset % 8);
        let fresh = *byte & mask == 0;
        *byte |= mask;
        fresh
    }

    if !insert(seen, node.id) {
        return Err(CompactTrieError::new("ACTree03 node is referenced by more than one edge"));
    }
    if let Some((wrapper, _)) = node.wrapper {
        if !insert(seen, wrapper) {
            return Err(CompactTrieError::new("ACTree03 node is referenced by more than one edge"));
        }
    }
    Ok(())
}

fn validated_topology_record(
    node: &DirectSemanticNode,
    incoming: Option<DirectBranchByte>,
) -> ValidatedTopologyRecord {
    let line = node.line.as_ref().map(|line| line.suffix.clone());
    let mut flags = 0;
    if node.value.is_some() {
        flags |= ValidatedTopologyRecord::HAS_VALUE;
    }
    if incoming.is_some() {
        flags |= ValidatedTopologyRecord::HAS_INCOMING;
    }
    ValidatedTopologyRecord {
        node_id: node.id,
        value: node.value.unwrap_or_default(),
        value_source_offset: node.wrapper.map_or(node.id, |(wrapper, _)| wrapper),
        line_start: line.as_ref().map_or(0, |line| line.start),
        line_len: line.as_ref().map_or(0, Range::len),
        incoming_source_offset: incoming.map_or(0, |incoming| incoming.source_offset),
        child_count: u16::try_from(node.children.len())
            .expect("ACTree03 nodes have at most 256 children"),
        incoming_byte: incoming.map_or(0, |incoming| incoming.byte),
        flags,
    }
}

fn observe_topology_value(
    summary: &mut CanonicalCompactTrieValueSummary,
    node: &DirectSemanticNode,
) {
    let Some(value) = node.value else {
        return;
    };
    let source_offset = node.wrapper.map_or(node.id, |(wrapper, _)| wrapper);
    if value != 0 && summary.first_nonzero.is_none() {
        summary.first_nonzero = Some((source_offset, value));
    }
    let expected = summary.value_count as u64;
    if value != expected && summary.first_nonsequential.is_none() {
        summary.first_nonsequential = Some((source_offset, value, expected));
    }
    summary.value_count = summary
        .value_count
        .checked_add(1)
        .expect("an ACT image cannot contain more values than addressable bytes");
}

fn validate_direct_producer_node<S>(
    node: &DirectSemanticNode,
    direct_children_encoded_len: usize,
    position: &mut usize,
    lines: &mut DirectCanonicalLineReplay<'_, S>,
    counters: &mut ActWorkCounters,
) -> Result<(), CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    if node.children.len() != 0 {
        let first_child = node
            .children
            .first_child
            .ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
        if first_child != *position {
            return Err(CompactTrieError::new("ACTree03 child roots are not in producer order"));
        }
        *position = position
            .checked_add(direct_children_encoded_len)
            .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
    }
    if let Some(line) = &node.line {
        let expected = lines.expected_line_id(line, position, counters)?;
        if expected != line.encoded.start {
            return Err(CompactTrieError::new(
                "ACTree03 line reuse differs from the canonical writer",
            ));
        }
        if let Some((wrapper, wrapper_len)) = node.wrapper {
            if wrapper != *position {
                return Err(CompactTrieError::new(
                    "ACTree03 line branch wrapper is not in producer order",
                ));
            }
            *position = position
                .checked_add(wrapper_len)
                .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
        }
    }
    Ok(())
}

fn validate_direct_canonical_act<S>(bytes: &S) -> Result<ValidatedActPlan, CompactTrieError>
where S: IndexedByteSource + ?Sized {
    let source_len = bytes.len();
    if source_len < ACT_HEADER_LEN {
        return Err(CompactTrieError::new("ACTree03 payload is shorter than its header"));
    }
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    let counted = CountedIndexedByteSource::new(bytes);
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    let bytes = &counted;
    let mut counters = new_act_work_counters();
    let mut magic = [0u8; COMPACT_TREE_MAGIC.len()];
    if !bytes.copy_exact(0, &mut magic) || magic != COMPACT_TREE_MAGIC {
        return Err(CompactTrieError::new("invalid ACTree03 magic"));
    }
    let mut root_slot = [0u8; 8];
    if !bytes.copy_exact(8, &mut root_slot) {
        return Err(CompactTrieError::at(8, "truncated ACTree03 root slot"));
    }
    let root_id = usize::try_from(u64::from_le_bytes(root_slot))
        .map_err(|_| CompactTrieError::new("ACTree03 root offset overflows usize"))?;
    if !(ACT_HEADER_LEN..bytes.len()).contains(&root_id) {
        return Err(CompactTrieError::new("ACTree03 root offset is outside the arena"));
    }
    let mut branch_edges = Vec::new();
    let root_branch_checkpoint = branch_edges.len();
    let root = direct_semantic_node(bytes, root_id, &mut branch_edges, &mut counters)?;
    let plan = validate_direct_canonical_act_from_root(
        bytes,
        source_len,
        root_id,
        root,
        root_branch_checkpoint,
        branch_edges,
        counters,
    )?;
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    let mut plan = plan;
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        let access = counted.tally();
        plan.counters.validation_source_byte_reads = access.byte_reads;
        plan.counters.validation_source_copy_bytes = access.copy_bytes;
        plan.counters.validation_source_contiguous_bytes = access.contiguous_bytes;
        plan.counters.validation_source_equality_bytes = access.equality_bytes;
    }
    Ok(plan)
}

// Keep the root decoder off the traversal/producer replay frame. Debug builds
// otherwise reserve both frames at once on admission's 64 KiB worker stack.
fn validate_direct_canonical_act_from_root<S>(
    bytes: &S,
    source_len: usize,
    root_id: usize,
    root: DirectSemanticNode,
    root_branch_checkpoint: usize,
    mut branch_edges: Vec<DirectBranchByte>,
    mut counters: ActWorkCounters,
) -> Result<ValidatedActPlan, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let empty = root.line.is_none() && root.value.is_none() && root.children.len() == 0;
    let root_len = root.encoded_len;
    let mut topology_event_count = checked_add_topology_record_events(
        0,
        root.line.as_ref().map_or(0, |line| line.suffix.len()),
        false,
    )
    .ok_or_else(|| CompactTrieError::new("ACTree03 topology event count overflows usize"))?;
    let mut seen = vec![0u8; source_len.div_ceil(8)];
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        counters.validation_seen_bitmap_bytes = seen.len();
    }
    register_direct_semantic_node(&mut seen, &root)?;
    let mut value_summary = CanonicalCompactTrieValueSummary::default();
    observe_topology_value(&mut value_summary, &root);
    let mut records = vec![validated_topology_record(&root, None)];
    let mut frames = vec![DirectPlanValidationFrame {
        next_child_id: root.children.first_child,
        node: root,
        next_child: 0,
        direct_children_encoded_len: 0,
        branch_checkpoint: root_branch_checkpoint,
    }];
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        counters.max_validation_depth = 1;
        counters.validation_frame_bytes_peak = frames
            .capacity()
            .saturating_mul(size_of::<DirectPlanValidationFrame>());
        counters.validation_frame_allocations = usize::from(frames.capacity() != 0);
        counters.validation_record_capacity_bytes_peak = records
            .capacity()
            .saturating_mul(size_of::<ValidatedTopologyRecord>());
        counters.validation_record_allocations = usize::from(records.capacity() != 0);
    }
    let mut canonical_position = ACT_HEADER_LEN;
    let mut canonical_lines = DirectCanonicalLineReplay::new(bytes);
    let mut canonical_error = None;
    #[cfg(test)]
    let mut audit_finish_node_ids = Vec::new();

    while let Some(frame) = frames.last_mut() {
        if frame.next_child < frame.node.children.len() {
            let child_id = frame
                .next_child_id
                .ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
            if child_id >= frame.node.children.parent_id {
                return Err(CompactTrieError::new("ACTree03 child does not precede its parent"));
            }
            let incoming = branch_edges[frame.node.children.child_bytes.0.start + frame.next_child];
            let child_branch_checkpoint = branch_edges.len();
            let child = direct_semantic_node(bytes, child_id, &mut branch_edges, &mut counters)?;
            frame.next_child += 1;
            frame.next_child_id = (frame.next_child < frame.node.children.len())
                .then(|| {
                    child_id
                        .checked_add(child.encoded_len)
                        .ok_or_else(|| CompactTrieError::new("ACTree03 sibling offset overflow"))
                })
                .transpose()?;
            frame.direct_children_encoded_len = frame
                .direct_children_encoded_len
                .checked_add(child.encoded_len)
                .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
            topology_event_count = checked_add_topology_record_events(
                topology_event_count,
                child.line.as_ref().map_or(0, |line| line.suffix.len()),
                true,
            )
            .ok_or_else(|| {
                CompactTrieError::new("ACTree03 topology event count overflows usize")
            })?;
            register_direct_semantic_node(&mut seen, &child)?;
            observe_topology_value(&mut value_summary, &child);
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            let record_capacity = records.capacity();
            records.push(validated_topology_record(&child, Some(incoming)));
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            if records.capacity() != record_capacity {
                counters.validation_record_allocations += 1;
            }
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            {
                counters.validation_record_capacity_bytes_peak =
                    counters.validation_record_capacity_bytes_peak.max(
                        records
                            .capacity()
                            .saturating_mul(size_of::<ValidatedTopologyRecord>()),
                    );
            }
            let next_child_id = child.children.first_child;
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            let frame_capacity = frames.capacity();
            frames.push(DirectPlanValidationFrame {
                node: child,
                next_child: 0,
                next_child_id,
                direct_children_encoded_len: 0,
                branch_checkpoint: child_branch_checkpoint,
            });
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            if frames.capacity() != frame_capacity {
                counters.validation_frame_allocations += 1;
            }
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            {
                counters.max_validation_depth = counters.max_validation_depth.max(frames.len());
                counters.validation_frame_bytes_peak = counters.validation_frame_bytes_peak.max(
                    frames
                        .capacity()
                        .saturating_mul(size_of::<DirectPlanValidationFrame>()),
                );
            }
            continue;
        }

        let frame = frames.pop().expect("a checked replay frame exists");
        #[cfg(test)]
        audit_finish_node_ids.push(frame.node.id);
        branch_edges.truncate(frame.branch_checkpoint);
        if canonical_error.is_none() {
            canonical_error = validate_direct_producer_node(
                &frame.node,
                frame.direct_children_encoded_len,
                &mut canonical_position,
                &mut canonical_lines,
                &mut counters,
            )
            .err();
        }
    }

    // Legacy admission parses and semantically validates every reachable node
    // before reporting canonical trailer/producer errors. Producer validation
    // runs in the same finish order while frames pop, but its first error is
    // latched until the complete structural walk and trailer check finish.
    // This preserves error precedence without retaining a duplicate O(node)
    // finish-order vector of complete semantic-node records.
    if source_len < ACT_HEADER_LEN + 9 {
        return Err(CompactTrieError::new(
            "ACTree03 payload is shorter than header, root, and trailer",
        ));
    }
    let trailer_start = source_len - 8;
    let mut trailer = [0u8; 8];
    if !bytes.copy_exact(trailer_start, &mut trailer) || trailer != [0; 8] {
        return Err(CompactTrieError::new("ACTree03 canonical varint trailer is not zero-filled"));
    }

    if let Some(error) = canonical_error {
        return Err(error);
    }

    if root_id != canonical_position {
        return Err(CompactTrieError::new("ACTree03 root is not in producer order"));
    }
    canonical_position = canonical_position
        .checked_add(root_len)
        .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
    if canonical_position != trailer_start {
        return Err(CompactTrieError::new(
            "ACTree03 arena contains gaps, overlaps, or unreachable objects",
        ));
    }
    debug_assert!(branch_edges.is_empty());
    debug_assert_eq!(checked_topology_event_count(&records), Some(topology_event_count));
    drop(branch_edges);
    #[cfg(test)]
    let audit_line_hashes = std::mem::take(&mut canonical_lines.hash_by_line);
    drop(canonical_lines);
    #[cfg(any(test, feature = "act-codec-instrumentation"))]
    {
        counters.plan_records = records.len();
        counters.plan_record_bytes = size_of::<ValidatedTopologyRecord>();
        counters.validated_topology_state_bytes = size_of::<ValidatedCompactTrieTopologyState>();
        counters.plan_bytes = size_of::<ValidatedActPlan>().saturating_add(
            records
                .len()
                .saturating_mul(size_of::<ValidatedTopologyRecord>()),
        );
        debug_assert_eq!(counters.semantic_node_parses, records.len());
        debug_assert_eq!(counters.node_reparses, 0);
        debug_assert_eq!(counters.event_metadata_source_reads, 0);
    }

    Ok(ValidatedActPlan {
        empty,
        topology_event_count,
        records: records.into_boxed_slice(),
        value_summary,
        #[cfg(test)]
        audit_finish_node_ids,
        #[cfg(test)]
        audit_line_hashes,
        #[cfg(any(test, feature = "act-codec-instrumentation"))]
        counters,
    })
}

enum KeyPiece {
    Slice(Range<usize>),
    Byte(u8),
}

struct KeyWalkFrame {
    id: usize,
    next_child: usize,
    piece_len: usize,
    entered: bool,
}

fn compact_path(arena: &[u8], pieces: &[KeyPiece]) -> CompactPath {
    if let [KeyPiece::Slice(range)] = pieces {
        return CompactPath::Relative(range.clone());
    }
    let len = pieces
        .iter()
        .map(|piece| match piece {
            KeyPiece::Slice(range) => range.len(),
            KeyPiece::Byte(_) => 1,
        })
        .sum();
    let mut path = Vec::with_capacity(len);
    for piece in pieces {
        match piece {
            KeyPiece::Slice(range) => path.extend_from_slice(&arena[range.clone()]),
            KeyPiece::Byte(byte) => path.push(*byte),
        }
    }
    CompactPath::Owned(path)
}

/// Parse the reachable ACTree03 graph once and prove that it is a finite tree.
fn parse_act_tree(bytes: &[u8]) -> Result<ParsedAct, CompactTrieError> {
    if bytes.len() < ACT_HEADER_LEN {
        return Err(CompactTrieError::new("ACTree03 payload is shorter than its header"));
    }
    if bytes[..COMPACT_TREE_MAGIC.len()] != COMPACT_TREE_MAGIC {
        return Err(CompactTrieError::new("invalid ACTree03 magic"));
    }
    let root_id = u64::from_le_bytes(
        bytes[8..16]
            .try_into()
            .expect("the ACT header root slot is eight bytes"),
    );
    let root_id = usize::try_from(root_id)
        .map_err(|_| CompactTrieError::new("ACTree03 root offset overflows usize"))?;
    if !(ACT_HEADER_LEN..bytes.len()).contains(&root_id) {
        return Err(CompactTrieError::new("ACTree03 root offset is outside the arena"));
    }

    let root = parse_act_node(bytes, root_id)?;
    let mut seen_nodes = HashSet::with_capacity(bytes.len().min(1024));
    seen_nodes.insert(root_id);
    let mut nodes = HashMap::with_capacity(bytes.len().min(1024));
    nodes.insert(root_id, root.clone());
    let mut stack = vec![ActFrame {
        node_id: root_id,
        next_child_id: first_child_id(&root),
        node: root,
        path_len: 0,
        next_child: 0,
        entered: true,
    }];

    while let Some(frame) = stack.last_mut() {
        let child_count = child_count(&frame.node);
        if frame.next_child == child_count {
            stack.pop();
            continue;
        }
        let child_id = frame
            .next_child_id
            .ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
        if child_id >= frame.node_id {
            return Err(CompactTrieError::new("ACTree03 child does not precede its parent"));
        }
        if !seen_nodes.insert(child_id) {
            return Err(CompactTrieError::new("ACTree03 node is referenced by more than one edge"));
        }
        let child = parse_act_node(bytes, child_id)?;
        nodes.insert(child_id, child.clone());
        let next_id = child_id
            .checked_add(child.encoded_len)
            .ok_or_else(|| CompactTrieError::new("ACTree03 sibling offset overflow"))?;
        frame.next_child += 1;
        frame.next_child_id = (frame.next_child < child_count).then_some(next_id);
        stack.push(ActFrame {
            node_id: child_id,
            next_child_id: first_child_id(&child),
            node: child,
            path_len: 0,
            next_child: 0,
            entered: true,
        });
    }

    let mut semantic_nodes = HashMap::with_capacity(nodes.len());
    let mut semantic_work = vec![root_id];
    while let Some(id) = semantic_work.pop() {
        if semantic_nodes.contains_key(&id) {
            continue;
        }
        let semantic = semantic_act_node(id, &nodes)?;
        semantic_work.extend(semantic.children.iter().map(|(_, child)| *child));
        semantic_nodes.insert(id, semantic);
    }
    Ok(ParsedAct {
        root_id,
        nodes,
        semantic_nodes,
    })
}

fn child_count(node: &ActNode) -> usize {
    match &node.edge {
        ActEdge::Branch { child_bytes, .. } => child_bytes.len(),
        ActEdge::Line { child, .. } => usize::from(child.is_some()),
    }
}

fn first_child_id(node: &ActNode) -> Option<usize> {
    match &node.edge {
        ActEdge::Branch { first_child, .. } => *first_child,
        ActEdge::Line { child, .. } => *child,
    }
}

fn parse_act_node(bytes: &[u8], node_id: usize) -> Result<ActNode, CompactTrieError> {
    let head = *bytes
        .get(node_id)
        .ok_or_else(|| CompactTrieError::new("ACTree03 node offset is outside the arena"))?;
    let mut cursor = node_id + 1;
    let value = if head & ACT_VALUE_FLAG != 0 {
        Some(read_act_varint(bytes, &mut cursor, "ACTree03 node value")?)
    } else {
        None
    };

    let edge = if head & ACT_LINE_FLAG == 0 {
        let encoded_count = (head & 0x3f) as usize;
        if encoded_count > 32 {
            return Err(CompactTrieError::new("ACTree03 branch child count exceeds 32 marker"));
        }
        let first_child = if encoded_count == 0 {
            None
        } else {
            Some(relative_target(
                node_id,
                read_act_varint(bytes, &mut cursor, "ACTree03 first-child offset")?,
                "ACTree03 first-child offset",
            )?)
        };
        let child_bytes = if encoded_count < 32 {
            let raw = take(bytes, &mut cursor, encoded_count, "ACTree03 child bytes")?;
            if raw.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(CompactTrieError::new(
                    "ACTree03 branch bytes are not strictly ascending",
                ));
            }
            raw.to_vec()
        } else {
            let mask = take(bytes, &mut cursor, 32, "ACTree03 child mask")?;
            let mut branches =
                Vec::with_capacity(mask.iter().map(|byte| byte.count_ones() as usize).sum());
            for (word, byte) in mask.iter().copied().enumerate() {
                for bit in 0..8u8 {
                    if byte & (1 << bit) != 0 {
                        branches.push((word * 8 + bit as usize) as u8);
                    }
                }
            }
            if branches.len() < 32 {
                return Err(CompactTrieError::new(
                    "ACTree03 dense child mask contains fewer than 32 branches",
                ));
            }
            branches
        };
        if child_bytes.is_empty() != first_child.is_none() {
            return Err(CompactTrieError::new("ACTree03 branch child metadata disagrees"));
        }
        ActEdge::Branch {
            first_child,
            child_bytes,
        }
    } else {
        if head & 0x3e != 0 {
            return Err(CompactTrieError::new("ACTree03 line node has reserved header bits set"));
        }
        let child = if head & 1 != 0 {
            Some(relative_target(
                node_id,
                read_act_varint(bytes, &mut cursor, "ACTree03 line-child offset")?,
                "ACTree03 line-child offset",
            )?)
        } else {
            None
        };
        let line_id = relative_target(
            node_id,
            read_act_varint(bytes, &mut cursor, "ACTree03 line-data offset")?,
            "ACTree03 line-data offset",
        )?;
        let encoded_start = line_id;
        let mut line_cursor = line_id;
        let line_len = read_act_varint(bytes, &mut line_cursor, "ACTree03 line length")?;
        let line_len = usize::try_from(line_len)
            .map_err(|_| CompactTrieError::new("ACTree03 line length overflows usize"))?;
        if line_len == 0 {
            return Err(CompactTrieError::new("ACTree03 line suffix is empty"));
        }
        let suffix_start = line_cursor;
        take(bytes, &mut line_cursor, line_len, "ACTree03 line suffix")?;
        let suffix = suffix_start..line_cursor;
        if line_cursor > node_id {
            return Err(CompactTrieError::new("ACTree03 line data overlaps its node"));
        }
        ActEdge::Line {
            child,
            suffix,
            encoded_range: encoded_start..line_cursor,
        }
    };

    Ok(ActNode {
        encoded_len: cursor - node_id,
        value,
        edge,
    })
}

fn branch_children(
    node: &ActNode,
    nodes: &HashMap<usize, ActNode>,
) -> Result<Vec<(u8, usize)>, CompactTrieError> {
    let ActEdge::Branch {
        first_child,
        child_bytes,
    } = &node.edge
    else {
        return Err(CompactTrieError::new("ACTree03 line node used as a branch wrapper"));
    };
    let mut next = *first_child;
    let mut children = Vec::with_capacity(child_bytes.len());
    for byte in child_bytes {
        let id = next.ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
        let child = nodes.get(&id).ok_or_else(|| {
            CompactTrieError::new("ACTree03 child was not retained by the parser")
        })?;
        children.push((*byte, id));
        next = id.checked_add(child.encoded_len);
    }
    Ok(children)
}

fn semantic_act_node(
    id: usize,
    nodes: &HashMap<usize, ActNode>,
) -> Result<SemanticActNode, CompactTrieError> {
    let node = nodes
        .get(&id)
        .ok_or_else(|| CompactTrieError::new("ACTree03 semantic node is missing"))?;
    let semantic = match &node.edge {
        ActEdge::Branch { .. } => SemanticActNode {
            value: node.value,
            children: branch_children(node, nodes)?,
            line: None,
            wrapper: None,
        },
        ActEdge::Line {
            child,
            suffix,
            encoded_range,
        } => match child {
            None => SemanticActNode {
                value: node.value,
                children: Vec::new(),
                line: Some(ActLineData {
                    suffix: suffix.clone(),
                    encoded: encoded_range.clone(),
                }),
                wrapper: None,
            },
            Some(wrapper) => {
                if node.value.is_some() {
                    return Err(CompactTrieError::new(
                        "ACTree03 line node has both a value and a child",
                    ));
                }
                let wrapper_node = nodes.get(wrapper).ok_or_else(|| {
                    CompactTrieError::new("ACTree03 line branch wrapper is missing")
                })?;
                if !matches!(wrapper_node.edge, ActEdge::Branch { .. }) {
                    return Err(CompactTrieError::new("ACTree03 line child is not a branch node"));
                }
                SemanticActNode {
                    value: wrapper_node.value,
                    children: branch_children(wrapper_node, nodes)?,
                    line: Some(ActLineData {
                        suffix: suffix.clone(),
                        encoded: encoded_range.clone(),
                    }),
                    wrapper: Some(*wrapper),
                }
            }
        },
    };

    if semantic.value.is_none() && semantic.children.len() == 1 {
        return Err(CompactTrieError::new(
            "ACTree03 contains an uncompressed value-free unary branch",
        ));
    }
    // A value-free childless branch is not redundant: PathMap's canonical
    // writer uses it for an explicit path endpoint whose final edge branches
    // from another path. `CompactTrie::visit` exposes that endpoint, and the
    // decoder must recreate it with `create_path`. The value-free unary case
    // above remains non-canonical because the writer line-compresses it.
    if semantic.wrapper.is_some() && semantic.children.is_empty() {
        return Err(CompactTrieError::new("ACTree03 line branch wrapper has no children"));
    }
    Ok(semantic)
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn canonical_line_hash(bytes: &[u8]) -> u64 {
    let mut hasher = gxhash::GxHasher::default();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(any(miri, target_arch = "riscv64"))]
fn canonical_line_hash(bytes: &[u8]) -> u64 {
    let mut state_lo = 0u64;
    let mut state_hi = 0u64;
    for byte in bytes {
        state_lo = state_lo.wrapping_add(u64::from(*byte));
        state_hi ^= u64::from(*byte).rotate_left(11);
        state_lo = state_lo.rotate_left(3);
    }
    let _ = state_hi;
    state_lo
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
const GX_KEYS: [u32; 12] = [
    0xF2784542, 0xB09D3E21, 0x89C222E5, 0xFC3BC28E, 0x03FCE279, 0xCB6B2E9B, 0xB361DC58, 0x39132BD9,
    0xD0012E32, 0x689D2B7D, 0x5544B1B7, 0xC78B122B,
];

#[cfg(not(any(miri, target_arch = "riscv64")))]
const AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn gx_key(index: usize) -> [u8; 16] {
    let mut key = [0u8; 16];
    for word in 0..4 {
        key[word * 4..word * 4 + 4].copy_from_slice(&GX_KEYS[index * 4 + word].to_ne_bytes());
    }
    key
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn aes_sub_shift(state: [u8; 16]) -> [u8; 16] {
    let mut output = [0u8; 16];
    for column in 0..4 {
        for row in 0..4 {
            let source_column = (column + row) & 3;
            output[column * 4 + row] = AES_SBOX[state[source_column * 4 + row] as usize];
        }
    }
    output
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn aes_xtime(byte: u8) -> u8 { (byte << 1) ^ ((byte >> 7).wrapping_mul(0x1b)) }

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn aes_encrypt(state: [u8; 16], key: [u8; 16]) -> [u8; 16] {
    let mut output = aes_sub_shift(state);
    for column in output.chunks_exact_mut(4) {
        let first = column[0];
        let all = column[0] ^ column[1] ^ column[2] ^ column[3];
        column[0] ^= all ^ aes_xtime(column[0] ^ column[1]);
        column[1] ^= all ^ aes_xtime(column[1] ^ column[2]);
        column[2] ^= all ^ aes_xtime(column[2] ^ column[3]);
        column[3] ^= all ^ aes_xtime(column[3] ^ first);
    }
    for (byte, key_byte) in output.iter_mut().zip(key) {
        *byte ^= key_byte;
    }
    output
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn aes_encrypt_last(state: [u8; 16], key: [u8; 16]) -> [u8; 16] {
    let mut output = aes_sub_shift(state);
    for (byte, key_byte) in output.iter_mut().zip(key) {
        *byte ^= key_byte;
    }
    output
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn bytewise_add(left: [u8; 16], right: [u8; 16]) -> [u8; 16] {
    let mut output = [0u8; 16];
    for index in 0..16 {
        output[index] = left[index].wrapping_add(right[index]);
    }
    output
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn indexed_gx_block<S>(source: &S, start: usize) -> Result<[u8; 16], CompactTrieError>
where S: IndexedByteSource + ?Sized {
    let mut block = [0u8; 16];
    if !source.copy_exact(start, &mut block) {
        return Err(CompactTrieError::at(start, "truncated canonical ACTree03 line"));
    }
    Ok(block)
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn indexed_gx_partial<S>(
    source: &S,
    start: usize,
    len: usize,
) -> Result<[u8; 16], CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let mut block = [0u8; 16];
    if !source.copy_exact(start, &mut block[..len]) {
        return Err(CompactTrieError::at(start, "truncated canonical ACTree03 line"));
    }
    for byte in &mut block {
        *byte = byte.wrapping_add(len as u8);
    }
    Ok(block)
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn indexed_gx_compress_8<S>(
    source: &S,
    mut position: usize,
    end: usize,
    hash_vector: [u8; 16],
    len: usize,
) -> Result<[u8; 16], CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let mut t1 = [0u8; 16];
    let mut t2 = [0u8; 16];
    let mut lane1 = hash_vector;
    let mut lane2 = hash_vector;
    while position < end {
        let v0 = indexed_gx_block(source, position)?;
        let v1 = indexed_gx_block(source, position + 16)?;
        let v2 = indexed_gx_block(source, position + 32)?;
        let v3 = indexed_gx_block(source, position + 48)?;
        let v4 = indexed_gx_block(source, position + 64)?;
        let v5 = indexed_gx_block(source, position + 80)?;
        let v6 = indexed_gx_block(source, position + 96)?;
        let v7 = indexed_gx_block(source, position + 112)?;
        position += 128;

        let mut tmp1 = aes_encrypt(v0, v2);
        let mut tmp2 = aes_encrypt(v1, v3);
        tmp1 = aes_encrypt(tmp1, v4);
        tmp2 = aes_encrypt(tmp2, v5);
        tmp1 = aes_encrypt(tmp1, v6);
        tmp2 = aes_encrypt(tmp2, v7);
        t1 = bytewise_add(t1, gx_key(0));
        t2 = bytewise_add(t2, gx_key(1));
        lane1 = aes_encrypt_last(aes_encrypt(tmp1, t1), lane1);
        lane2 = aes_encrypt_last(aes_encrypt(tmp2, t2), lane2);
    }

    let mut len_vector = [0u8; 16];
    for word in len_vector.chunks_exact_mut(4) {
        word.copy_from_slice(&(len as u32).to_ne_bytes());
    }
    lane1 = bytewise_add(lane1, len_vector);
    lane2 = bytewise_add(lane2, len_vector);
    Ok(aes_encrypt(lane1, lane2))
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn indexed_gx_compress_all<S>(
    source: &S,
    range: Range<usize>,
) -> Result<[u8; 16], CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let len = range.len();
    if len == 0 {
        return Ok([0u8; 16]);
    }
    if len <= 16 {
        return indexed_gx_partial(source, range.start, len);
    }

    let extra = len % 16;
    let (mut hash_vector, mut position) = if extra == 0 {
        (indexed_gx_block(source, range.start)?, range.start + 16)
    } else {
        (indexed_gx_partial(source, range.start, extra)?, range.start + extra)
    };
    let mut v0 = indexed_gx_block(source, position)?;
    position += 16;
    if len > 32 {
        v0 = aes_encrypt(v0, indexed_gx_block(source, position)?);
        position += 16;
        if len > 48 {
            v0 = aes_encrypt(v0, indexed_gx_block(source, position)?);
            position += 16;
            if len > 64 {
                let remaining = range.end - position;
                let unrollable_blocks = (remaining / 128) * 8;
                let leading_blocks = remaining / 16 - unrollable_blocks;
                for _ in 0..leading_blocks {
                    hash_vector = aes_encrypt(hash_vector, indexed_gx_block(source, position)?);
                    position += 16;
                }
                hash_vector = indexed_gx_compress_8(source, position, range.end, hash_vector, len)?;
            }
        }
    }
    Ok(aes_encrypt_last(hash_vector, aes_encrypt(aes_encrypt(v0, gx_key(0)), gx_key(1))))
}

#[cfg(not(any(miri, target_arch = "riscv64")))]
fn indexed_canonical_line_hash<S>(
    source: &S,
    range: Range<usize>,
) -> Result<u64, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    if let Some(contiguous) = source.contiguous_range(range.clone()) {
        return Ok(canonical_line_hash(contiguous));
    }
    let compressed = indexed_gx_compress_all(source, range)?;
    let initial_key = aes_encrypt([0u8; 16], gx_key(0));
    let mut state = aes_encrypt_last(compressed, initial_key);
    state = aes_encrypt(state, gx_key(0));
    state = aes_encrypt(state, gx_key(1));
    state = aes_encrypt_last(state, gx_key(2));
    Ok(u64::from_ne_bytes(state[..8].try_into().expect("gxhash state has eight bytes")))
}

#[cfg(any(miri, target_arch = "riscv64"))]
fn indexed_canonical_line_hash<S>(
    source: &S,
    range: Range<usize>,
) -> Result<u64, CompactTrieError>
where
    S: IndexedByteSource + ?Sized,
{
    let mut state_lo = 0u64;
    let mut state_hi = 0u64;
    for offset in range {
        let byte = source
            .byte_at(offset)
            .ok_or_else(|| CompactTrieError::at(offset, "truncated canonical ACTree03 line"))?;
        state_lo = state_lo.wrapping_add(u64::from(byte));
        state_hi ^= u64::from(byte).rotate_left(11);
        state_lo = state_lo.rotate_left(3);
    }
    let _ = state_hi;
    Ok(state_lo)
}

struct DirectCanonicalLineReplay<'a, S: ?Sized> {
    source: &'a S,
    first: Option<ActLineData>,
    by_hash: Option<HashMap<u64, ActLineData>>,
    hash_by_line: HashMap<usize, u64>,
    #[cfg(test)]
    forced_hash: Option<u64>,
}

impl<'a, S> DirectCanonicalLineReplay<'a, S>
where S: IndexedByteSource + ?Sized
{
    fn new(source: &'a S) -> Self {
        Self {
            source,
            first: None,
            by_hash: None,
            hash_by_line: HashMap::new(),
            #[cfg(test)]
            forced_hash: None,
        }
    }

    #[cfg(test)]
    fn force_hash(&mut self, hash: u64) { self.forced_hash = Some(hash); }

    fn line_hash(
        &mut self,
        line: &ActLineData,
        _counters: &mut ActWorkCounters,
    ) -> Result<u64, CompactTrieError> {
        if let Some(hash) = self.hash_by_line.get(&line.encoded.start) {
            return Ok(*hash);
        }
        #[cfg(test)]
        let hash = if let Some(hash) = self.forced_hash {
            hash
        } else {
            indexed_canonical_line_hash(self.source, line.suffix.clone())?
        };
        #[cfg(not(test))]
        let hash = indexed_canonical_line_hash(self.source, line.suffix.clone())?;
        #[cfg(any(test, feature = "act-codec-instrumentation"))]
        {
            _counters.line_hash_bytes = _counters.line_hash_bytes.saturating_add(line.suffix.len());
        }
        self.hash_by_line.insert(line.encoded.start, hash);
        Ok(hash)
    }

    fn expected_line_id(
        &mut self,
        line: &ActLineData,
        position: &mut usize,
        counters: &mut ActWorkCounters,
    ) -> Result<usize, CompactTrieError> {
        if self.first.is_none() {
            if line.encoded.start != *position {
                return Err(CompactTrieError::new(
                    "ACTree03 first line data is not in producer order",
                ));
            }
            *position = line.encoded.end;
            self.first = Some(line.clone());
            return Ok(line.encoded.start);
        }

        if self.by_hash.is_none() {
            let first = self.first.clone().expect("first line exists");
            let hash = self.line_hash(&first, counters)?;
            let mut by_hash = HashMap::new();
            by_hash.insert(hash, first);
            self.by_hash = Some(by_hash);
        }

        let hash = self.line_hash(line, counters)?;
        if let Some(previous) = self
            .by_hash
            .as_ref()
            .and_then(|by_hash| by_hash.get(&hash))
            .cloned()
        {
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            {
                counters.line_equality_bytes = counters
                    .line_equality_bytes
                    .saturating_add(line.suffix.len());
            }
            if self
                .source
                .ranges_equal(previous.suffix, line.suffix.clone())
            {
                return Ok(previous.encoded.start);
            }
            #[cfg(any(test, feature = "act-codec-instrumentation"))]
            {
                counters.line_hash_collisions += 1;
            }
        }

        if line.encoded.start != *position {
            return Err(CompactTrieError::new("ACTree03 line data is not in producer order"));
        }
        *position = line.encoded.end;
        self.by_hash
            .as_mut()
            .expect("the second line initialized the hash map")
            .insert(hash, line.clone());
        Ok(line.encoded.start)
    }
}

struct CanonicalLineReplay<'a> {
    bytes: &'a [u8],
    first: Option<ActLineData>,
    by_hash: Option<HashMap<u64, ActLineData>>,
    hash_by_line: HashMap<usize, u64>,
}

impl<'a> CanonicalLineReplay<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            first: None,
            by_hash: None,
            hash_by_line: HashMap::new(),
        }
    }

    fn suffix(&self, line: &ActLineData) -> &[u8] { &self.bytes[line.suffix.clone()] }

    fn line_hash(&mut self, line: &ActLineData) -> u64 {
        if let Some(hash) = self.hash_by_line.get(&line.encoded.start) {
            return *hash;
        }
        let hash = canonical_line_hash(&self.bytes[line.suffix.clone()]);
        self.hash_by_line.insert(line.encoded.start, hash);
        hash
    }

    fn expected_line_id(
        &mut self,
        line: &ActLineData,
        position: &mut usize,
    ) -> Result<usize, CompactTrieError> {
        if self.first.is_none() {
            if line.encoded.start != *position {
                return Err(CompactTrieError::new(
                    "ACTree03 first line data is not in producer order",
                ));
            }
            *position = line.encoded.end;
            self.first = Some(line.clone());
            return Ok(line.encoded.start);
        }

        if self.by_hash.is_none() {
            let first = self.first.clone().expect("first line exists");
            let hash = self.line_hash(&first);
            let mut by_hash = HashMap::new();
            by_hash.insert(hash, first);
            self.by_hash = Some(by_hash);
        }

        let hash = self.line_hash(line);
        if let Some(previous) = self
            .by_hash
            .as_ref()
            .and_then(|by_hash| by_hash.get(&hash))
            .cloned()
        {
            if self.suffix(&previous) == self.suffix(line) {
                return Ok(previous.encoded.start);
            }
        }

        if line.encoded.start != *position {
            return Err(CompactTrieError::new("ACTree03 line data is not in producer order"));
        }
        *position = line.encoded.end;
        self.by_hash
            .as_mut()
            .expect("the second line initialized the hash map")
            .insert(hash, line.clone());
        Ok(line.encoded.start)
    }
}

enum ReplayTask {
    Enter(usize),
    Finish(usize),
}

fn validate_canonical_act(bytes: &[u8], parsed: &ParsedAct) -> Result<(), CompactTrieError> {
    if bytes.len() < ACT_HEADER_LEN + 9 {
        return Err(CompactTrieError::new(
            "ACTree03 payload is shorter than header, root, and trailer",
        ));
    }
    let trailer_start = bytes.len() - 8;
    if bytes[trailer_start..] != [0; 8] {
        return Err(CompactTrieError::new("ACTree03 canonical varint trailer is not zero-filled"));
    }

    let mut position = ACT_HEADER_LEN;
    let mut lines = CanonicalLineReplay::new(bytes);
    let mut tasks = vec![ReplayTask::Enter(parsed.root_id)];
    while let Some(task) = tasks.pop() {
        match task {
            ReplayTask::Enter(id) => {
                let semantic = &parsed.semantic_nodes[&id];
                tasks.push(ReplayTask::Finish(id));
                for (_, child) in semantic.children.iter().rev() {
                    tasks.push(ReplayTask::Enter(*child));
                }
            }
            ReplayTask::Finish(id) => {
                let semantic = &parsed.semantic_nodes[&id];
                for (_, child_id) in &semantic.children {
                    if *child_id != position {
                        return Err(CompactTrieError::new(
                            "ACTree03 child roots are not in producer order",
                        ));
                    }
                    position = position
                        .checked_add(parsed.nodes[child_id].encoded_len)
                        .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
                }
                if let Some(line) = &semantic.line {
                    let expected = lines.expected_line_id(line, &mut position)?;
                    if expected != line.encoded.start {
                        return Err(CompactTrieError::new(
                            "ACTree03 line reuse differs from the canonical writer",
                        ));
                    }
                    if let Some(wrapper) = semantic.wrapper {
                        if wrapper != position {
                            return Err(CompactTrieError::new(
                                "ACTree03 line branch wrapper is not in producer order",
                            ));
                        }
                        position = position
                            .checked_add(parsed.nodes[&wrapper].encoded_len)
                            .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
                    }
                }
            }
        }
    }

    if parsed.root_id != position {
        return Err(CompactTrieError::new("ACTree03 root is not in producer order"));
    }
    position = position
        .checked_add(parsed.nodes[&parsed.root_id].encoded_len)
        .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
    if position != trailer_start {
        return Err(CompactTrieError::new(
            "ACTree03 arena contains gaps, overlaps, or unreachable objects",
        ));
    }
    Ok(())
}

fn relative_target(node_id: usize, offset: u64, label: &str) -> Result<usize, CompactTrieError> {
    let offset = usize::try_from(offset)
        .map_err(|_| CompactTrieError::new(format!("{label} overflows usize")))?;
    if offset == 0 {
        return Err(CompactTrieError::new(format!("{label} is zero")));
    }
    let target = node_id
        .checked_sub(offset)
        .ok_or_else(|| CompactTrieError::new(format!("{label} points before the arena")))?;
    if target < ACT_HEADER_LEN {
        return Err(CompactTrieError::new(format!("{label} points into the header")));
    }
    Ok(target)
}

fn read_act_varint(bytes: &[u8], cursor: &mut usize, label: &str) -> Result<u64, CompactTrieError> {
    let first = *bytes
        .get(*cursor)
        .ok_or_else(|| CompactTrieError::new(format!("truncated {label}")))?;
    *cursor += 1;
    if first <= ACT_VARINT_BIAS {
        return Ok(first as u64);
    }
    let len = (first - ACT_VARINT_BIAS) as usize;
    if !(1..=8).contains(&len) {
        return Err(CompactTrieError::new(format!("invalid {label} length")));
    }
    let raw = take(bytes, cursor, len, label)?;
    if raw.last() == Some(&0) {
        return Err(CompactTrieError::new(format!("non-canonical {label}")));
    }
    let mut full = [0u8; 8];
    full[..len].copy_from_slice(raw);
    let value = u64::from_le_bytes(full);
    if value <= ACT_VARINT_BIAS as u64 {
        return Err(CompactTrieError::new(format!("non-canonical {label}")));
    }
    Ok(value)
}

fn take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    label: &str,
) -> Result<&'a [u8], CompactTrieError> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| CompactTrieError::new(format!("{label} length overflow")))?;
    let region = bytes
        .get(*cursor..end)
        .ok_or_else(|| CompactTrieError::new(format!("truncated {label}")))?;
    *cursor = end;
    Ok(region)
}

#[cfg(test)]
#[path = "../../tests/support/pathmap_codec_validated_plan.rs"]
mod validated_plan_tests;

#[cfg(test)]
mod indexed_direct_tests {
    use pathmap::PathMap;
    use pathmap::arena_compact::ArenaCompactTree;
    use proptest::prelude::*;

    use super::*;

    struct FragmentedSource {
        chunks: Vec<Vec<u8>>,
        starts: Vec<usize>,
        len: usize,
    }

    impl FragmentedSource {
        fn new(bytes: &[u8], mut state: u64) -> Self {
            let mut chunks = Vec::new();
            let mut starts = Vec::new();
            let mut position = 0;
            while position < bytes.len() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let chunk_len = ((state as usize) % 31 + 1).min(bytes.len() - position);
                starts.push(position);
                chunks.push(bytes[position..position + chunk_len].to_vec());
                position += chunk_len;
            }
            Self {
                chunks,
                starts,
                len: bytes.len(),
            }
        }

        // A test-only physical address with a gap between every chunk. The
        // codec emits logical offsets; its caller maps them to provenance.
        fn physical_anchor(&self, index: usize) -> Option<(usize, usize)> {
            if index >= self.len {
                return None;
            }
            let chunk = self.starts.partition_point(|start| *start <= index) - 1;
            Some((chunk, index - self.starts[chunk]))
        }
    }

    impl IndexedByteSource for FragmentedSource {
        fn len(&self) -> usize { self.len }

        fn byte_at(&self, index: usize) -> Option<u8> {
            if index >= self.len {
                return None;
            }
            let chunk = self.starts.partition_point(|start| *start <= index) - 1;
            self.chunks[chunk].get(index - self.starts[chunk]).copied()
        }
    }

    fn canonical_line_image(suffix: &[u8], value: u8) -> Vec<u8> {
        assert!(!suffix.is_empty());
        assert!(suffix.len() <= ACT_VARINT_BIAS as usize);
        assert!(value <= ACT_VARINT_BIAS);
        let line_id = ACT_HEADER_LEN;
        let root_id = line_id + 1 + suffix.len();
        let mut bytes = Vec::with_capacity(root_id + 3 + 8);
        bytes.extend_from_slice(&COMPACT_TREE_MAGIC);
        bytes.extend_from_slice(&(root_id as u64).to_le_bytes());
        bytes.push(suffix.len() as u8);
        bytes.extend_from_slice(suffix);
        bytes.push(ACT_LINE_FLAG | ACT_VALUE_FLAG);
        bytes.push(value);
        bytes.push((root_id - line_id) as u8);
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    #[test]
    fn indexed_batch_position_selects_exact_next_scalar_line_events() {
        let bytes = canonical_line_image(b"abcde", 7);
        let mut batched = CanonicalCompactTrieIndexedTopologyCursor::from_source(
            AsRefIndexedByteSource::new(bytes.as_slice()),
        )
        .unwrap();
        let mut scalar = CanonicalCompactTrieIndexedTopologyCursor::from_source(
            AsRefIndexedByteSource::new(bytes.as_slice()),
        )
        .unwrap();
        let mut forward = 0;
        let mut unwind = 0;
        loop {
            let before = batched.source_position();
            if let Some(span) = batched.line_span() {
                let width = span.source_range().len().min(2);
                let prepared = span.prepare(width).unwrap();
                let publication = prepared.commit();
                let after = batched.source_position().unwrap();
                let before = before.unwrap();
                assert!(publication.matches_scalar_schedule(before, after));
                let mut changed = after;
                match publication.direction() {
                    CanonicalCompactTrieLineDirection::Forward => {
                        forward += 1;
                        changed.line_index += 1;
                        assert!(!publication.matches_scalar_schedule(before, changed));
                        for offset in publication.source_range() {
                            assert_eq!(
                                scalar.next_event().unwrap(),
                                Some(CanonicalCompactTrieTopologyEvent::Edge {
                                    byte: bytes[offset],
                                    source_offset: offset,
                                }),
                            );
                            assert_eq!(
                                scalar.next_event().unwrap(),
                                Some(CanonicalCompactTrieTopologyEvent::Value {
                                    value: None,
                                    source_offset: offset,
                                }),
                            );
                        }
                    }
                    CanonicalCompactTrieLineDirection::Unwind => {
                        unwind += 1;
                        changed.leave_index += 1;
                        assert!(!publication.matches_scalar_schedule(before, changed));
                        for offset in publication.source_range().rev() {
                            assert_eq!(
                                scalar.next_event().unwrap(),
                                Some(CanonicalCompactTrieTopologyEvent::Leave {
                                    source_offset: offset,
                                }),
                            );
                        }
                    }
                }
                assert_eq!(batched.source_position(), scalar.source_position());
            } else {
                let next = batched.next_event().unwrap();
                assert_eq!(next, scalar.next_event().unwrap());
                assert_eq!(batched.source_position(), scalar.source_position());
                if next.is_none() {
                    break;
                }
            }
        }
        assert!(forward > 1);
        assert!(unwind > 1);
    }

    #[test]
    fn active_line_work_observes_partial_event_headroom_in_both_directions() {
        let bytes = canonical_line_image(b"abcdef", 7);
        let mut cursor = CanonicalCompactTrieIndexedTopologyCursor::from_source(
            AsRefIndexedByteSource::new(bytes.as_slice()),
        )
        .unwrap();
        assert!(matches!(
            cursor.next_event().unwrap(),
            Some(CanonicalCompactTrieTopologyEvent::Value { .. })
        ));

        let forward = cursor.active_line_work_source(usize::MAX - 3).unwrap();
        assert_eq!(forward.direction, CanonicalCompactTrieLineDirection::Forward);
        assert_eq!(forward.source_range.len(), 5);
        assert_eq!(forward.remaining_work_events, Some(10));
        assert_eq!(forward.admissible_source_width, 1);
        assert_eq!(forward.admissible_work_events, 2);
        assert_eq!(cursor.line_span().unwrap().source_range(), forward.source_range);
        assert_eq!(
            cursor
                .line_span()
                .unwrap()
                .prepare(1)
                .unwrap()
                .expanded_events(),
            2
        );
        assert!(
            cursor
                .active_line_work_source(usize::MAX)
                .is_some_and(|work| {
                    work.source_range == forward.source_range && work.admissible_source_width == 0
                })
        );
        let before_forward = cursor.source_position().unwrap();
        let committed_forward = cursor.line_span().unwrap().prepare(1).unwrap().commit();
        assert_eq!(
            (usize::MAX - 3).checked_add(committed_forward.expanded_events()),
            Some(usize::MAX - 1),
        );
        assert!((usize::MAX - 3).checked_add(4).is_none());
        assert!(
            committed_forward
                .matches_scalar_schedule(before_forward, cursor.source_position().unwrap())
        );

        loop {
            if cursor
                .active_line_work_source(0)
                .is_some_and(|work| work.direction == CanonicalCompactTrieLineDirection::Unwind)
            {
                break;
            }
            assert!(cursor.next_event().unwrap().is_some());
        }
        let unwind = cursor.active_line_work_source(usize::MAX - 2).unwrap();
        assert_eq!(unwind.direction, CanonicalCompactTrieLineDirection::Unwind);
        assert_eq!(unwind.source_range.len(), 6);
        assert_eq!(unwind.remaining_work_events, Some(6));
        assert_eq!(unwind.admissible_source_width, 2);
        assert_eq!(unwind.admissible_work_events, 2);
        assert_eq!(cursor.line_span().unwrap().source_range(), unwind.source_range);
        assert_eq!(
            cursor
                .line_span()
                .unwrap()
                .prepare(2)
                .unwrap()
                .expanded_events(),
            2
        );
        assert!(
            cursor
                .active_line_work_source(usize::MAX)
                .is_some_and(|work| {
                    work.source_range == unwind.source_range && work.admissible_source_width == 0
                })
        );
        let before_unwind = cursor.source_position().unwrap();
        let committed_unwind = cursor.line_span().unwrap().prepare(2).unwrap().commit();
        assert_eq!(
            (usize::MAX - 2).checked_add(committed_unwind.expanded_events()),
            Some(usize::MAX),
        );
        assert!((usize::MAX - 2).checked_add(3).is_none());
        assert!(
            committed_unwind
                .matches_scalar_schedule(before_unwind, cursor.source_position().unwrap())
        );
    }

    // Recover the offerable line-work prefix from a completed scalar trace.
    // Forward's final pair is excluded by the scalar transition out of
    // LineEdge; Unwind stops when the next Leave is outside the line suffix.
    fn scalar_trace_line_work(
        events: &[CanonicalCompactTrieTopologyEvent],
        boundaries: &[Option<CanonicalCompactTrieCursorSourcePosition>],
        event_index: usize,
    ) -> Option<(CanonicalCompactTrieLineDirection, Vec<usize>)> {
        let position = boundaries.get(event_index).copied().flatten()?;
        let line_start = position.line_start?;
        let line_end = position.line_end?;
        let mut offsets = Vec::new();
        let direction = match position.stage {
            CanonicalCompactTrieCursorSourceStage::LineEdge => {
                let mut at = event_index;
                while let (
                    Some(CanonicalCompactTrieTopologyEvent::Edge {
                        source_offset: edge,
                        ..
                    }),
                    Some(CanonicalCompactTrieTopologyEvent::Value {
                        value: None,
                        source_offset: value,
                    }),
                    Some(Some(after)),
                ) = (events.get(at), events.get(at + 1), boundaries.get(at + 2))
                {
                    if edge != value ||
                        *edge < line_start ||
                        *edge >= line_end ||
                        after.record != position.record ||
                        after.stage != CanonicalCompactTrieCursorSourceStage::LineEdge
                    {
                        break;
                    }
                    offsets.push(*edge);
                    at += 2;
                }
                CanonicalCompactTrieLineDirection::Forward
            }
            CanonicalCompactTrieCursorSourceStage::Leave |
            CanonicalCompactTrieCursorSourceStage::Children
                if position.stage != CanonicalCompactTrieCursorSourceStage::Children ||
                    position.remaining_children == 0 =>
            {
                let mut at = event_index;
                while let (
                    Some(CanonicalCompactTrieTopologyEvent::Leave { source_offset }),
                    Some(Some(after)),
                ) = (events.get(at), boundaries.get(at + 1))
                {
                    if *source_offset < line_start ||
                        *source_offset >= line_end ||
                        after.record != position.record ||
                        after.stage != CanonicalCompactTrieCursorSourceStage::Leave
                    {
                        break;
                    }
                    offsets.push(*source_offset);
                    at += 1;
                }
                CanonicalCompactTrieLineDirection::Unwind
            }
            _ => return None,
        };
        (!offsets.is_empty()).then_some((direction, offsets))
    }

    fn check_generated_line_publications(bytes: &[u8], fragment_seed: u64, schedule: u8) {
        let (plan_events, plan_boundaries) = {
            let mut cursor = CanonicalCompactTrieIndexedTopologyCursor::from_source(
                AsRefIndexedByteSource::new(bytes),
            )
            .unwrap();
            let mut events = Vec::new();
            let mut boundaries = Vec::new();
            loop {
                boundaries.push(cursor.source_position());
                let Some(event) = cursor.next_event().unwrap() else {
                    break;
                };
                events.push(event);
            }
            (events, boundaries)
        };
        let anchor_oracle = FragmentedSource::new(bytes, fragment_seed);
        let mut batched = CanonicalCompactTrieIndexedTopologyCursor::from_source(
            FragmentedSource::new(bytes, fragment_seed),
        )
        .unwrap();
        let mut scalar = CanonicalCompactTrieIndexedTopologyCursor::from_source(
            FragmentedSource::new(bytes, fragment_seed),
        )
        .unwrap();
        let mut event_index = 0usize;
        let mut forward = 0;
        let mut unwind = 0;
        let mut branched_line = false;
        let mut cross_chunk = false;
        loop {
            let before = batched.source_position();
            assert_eq!(before, plan_boundaries[event_index]);
            assert_eq!(before, scalar.source_position());
            if let Some(before) = before {
                let record = &batched.state.plan.records[before.record];
                assert_eq!(before.node_id, record.node_id);
                assert_eq!(before.line_start, record.line_suffix().map(|line| line.start));
                assert_eq!(before.line_end, record.line_suffix().map(|line| line.end));
            }
            let work_source = batched.active_line_work_source(event_index);
            let scalar_work = scalar_trace_line_work(&plan_events, &plan_boundaries, event_index);
            assert_eq!(work_source.is_some(), scalar_work.is_some());
            if let Some(work) = &work_source {
                let (oracle_direction, oracle_offsets) = scalar_work.as_ref().unwrap();
                assert_eq!(Some(work.position), before);
                assert_eq!(work.event_index, event_index);
                assert_eq!(&work.direction, oracle_direction);
                let remaining_work_events = work.remaining_work_events.unwrap();
                assert_eq!(work.admissible_source_width, work.source_range.len());
                assert_eq!(work.admissible_work_events, remaining_work_events);
                let expected_event_count = match *oracle_direction {
                    CanonicalCompactTrieLineDirection::Forward => oracle_offsets.len() * 2,
                    CanonicalCompactTrieLineDirection::Unwind => oracle_offsets.len(),
                };
                assert_eq!(remaining_work_events, expected_event_count);
                let first_offset = *oracle_offsets.first().unwrap();
                let last_offset = *oracle_offsets.last().unwrap();
                assert_eq!(work.first_source_offset(), first_offset);
                assert_eq!(work.last_source_offset(), last_offset);
                assert_eq!(
                    work.source_range,
                    *oracle_offsets.iter().min().unwrap()..
                        *oracle_offsets.iter().max().unwrap() + 1,
                );
                let record = &batched.state.plan.records[work.position.record];
                let line = record.line_suffix().unwrap();
                assert!(line.start <= work.source_range.start);
                assert!(work.source_range.end <= line.end);
                assert!(event_index + remaining_work_events < plan_events.len());
                assert_eq!(
                    batched.source().physical_anchor(work.first_source_offset()),
                    anchor_oracle.physical_anchor(first_offset),
                );
                assert_eq!(
                    batched.source().physical_anchor(work.last_source_offset()),
                    anchor_oracle.physical_anchor(last_offset),
                );
                for step in 0..remaining_work_events {
                    let offset = match *oracle_direction {
                        CanonicalCompactTrieLineDirection::Forward => oracle_offsets[step / 2],
                        CanonicalCompactTrieLineDirection::Unwind => oracle_offsets[step],
                    };
                    let expected = match work.direction {
                        CanonicalCompactTrieLineDirection::Forward if step % 2 == 0 => {
                            CanonicalCompactTrieTopologyEvent::Edge {
                                byte: bytes[offset],
                                source_offset: offset,
                            }
                        }
                        CanonicalCompactTrieLineDirection::Forward => {
                            CanonicalCompactTrieTopologyEvent::Value {
                                value: None,
                                source_offset: offset,
                            }
                        }
                        CanonicalCompactTrieLineDirection::Unwind => {
                            CanonicalCompactTrieTopologyEvent::Leave {
                                source_offset: offset,
                            }
                        }
                    };
                    assert_eq!(plan_events[event_index + step], expected);
                    assert_eq!(
                        batched.source().physical_anchor(offset),
                        anchor_oracle.physical_anchor(offset),
                    );
                }
            }
            if let Some(span) = batched.line_span() {
                let work = work_source.unwrap();
                let (_, oracle_offsets) = scalar_work.unwrap();
                assert_eq!(span.source_range(), work.source_range);
                assert_eq!(span.direction(), work.direction);
                let offered = span.source_range();
                let direction = span.direction();
                let width = match schedule {
                    0 => 1,
                    1 => offered.len(),
                    _ => {
                        1 + (event_index.wrapping_mul(17) ^ fragment_seed as usize) % offered.len()
                    }
                };
                let accepted_offsets = &oracle_offsets[..width];
                let expected_range = *accepted_offsets.iter().min().unwrap()..
                    *accepted_offsets.iter().max().unwrap() + 1;
                let expected_last_anchor = anchor_oracle
                    .physical_anchor(*accepted_offsets.last().unwrap())
                    .unwrap();
                let prepared = span.prepare(width).unwrap();
                assert_eq!(prepared.source_range(), expected_range);
                assert_eq!(
                    prepared
                        .source()
                        .physical_anchor(prepared.last_source_offset()),
                    Some(expected_last_anchor),
                );
                let publication = prepared.commit();
                let after = batched.source_position().unwrap();
                let next_event_index = event_index + publication.expanded_events();
                assert_eq!(Some(after), plan_boundaries[next_event_index]);
                assert!(publication.matches_scalar_schedule(before.unwrap(), after));
                assert_eq!(after.record, before.unwrap().record);
                assert_eq!(after.stack_depth, before.unwrap().stack_depth);
                branched_line |= before.unwrap().stack_depth > 1;
                let range = publication.source_range();
                assert_eq!(range, expected_range);
                assert_eq!(publication.last_source_offset(), *accepted_offsets.last().unwrap());
                cross_chunk |= batched.source().physical_anchor(range.start).unwrap().0 !=
                    anchor_oracle.physical_anchor(range.end - 1).unwrap().0;
                let mut final_scalar_anchor = None;
                match direction {
                    CanonicalCompactTrieLineDirection::Forward => {
                        forward += 1;
                        assert_eq!(publication.expanded_events(), width * 2);
                        for offset in range {
                            let edge = scalar.next_event().unwrap().unwrap();
                            assert_eq!(edge, CanonicalCompactTrieTopologyEvent::Edge {
                                byte: bytes[offset],
                                source_offset: offset,
                            });
                            let value = scalar.next_event().unwrap().unwrap();
                            assert_eq!(value, CanonicalCompactTrieTopologyEvent::Value {
                                value: None,
                                source_offset: offset,
                            });
                            final_scalar_anchor = scalar.source().physical_anchor(offset);
                        }
                    }
                    CanonicalCompactTrieLineDirection::Unwind => {
                        unwind += 1;
                        assert_eq!(publication.expanded_events(), width);
                        for offset in range.rev() {
                            let leave = scalar.next_event().unwrap().unwrap();
                            assert_eq!(leave, CanonicalCompactTrieTopologyEvent::Leave {
                                source_offset: offset,
                            });
                            final_scalar_anchor = scalar.source().physical_anchor(offset);
                        }
                    }
                }
                assert_eq!(Some(expected_last_anchor), final_scalar_anchor);
                assert_eq!(
                    batched
                        .source()
                        .physical_anchor(publication.last_source_offset()),
                    final_scalar_anchor,
                );
                event_index = next_event_index;
                assert_eq!(batched.source_position(), scalar.source_position());
            } else {
                assert!(work_source.is_none());
                let event = batched.next_event().unwrap();
                assert_eq!(event, plan_events.get(event_index).copied());
                assert_eq!(event, scalar.next_event().unwrap());
                if event.is_none() {
                    break;
                }
                event_index += 1;
                assert_eq!(batched.source_position(), plan_boundaries[event_index]);
                assert_eq!(batched.source_position(), scalar.source_position());
            }
        }
        assert!(forward > 0 && unwind > 0);
        assert!(branched_line);
        if schedule == 1 {
            assert!(cross_chunk);
        }
        assert_eq!(event_index, plan_events.len());
        assert!(event_index > forward + unwind);
    }

    // Decode only the fields retained by a validated record, directly from
    // the contiguous wire image. This oracle does not call the admission
    // parser or inspect its validation frames.
    fn independently_decode_record(
        bytes: &[u8],
        node_id: usize,
    ) -> (Option<u64>, usize, usize, usize, usize, Option<usize>) {
        fn varint(bytes: &[u8], at: &mut usize) -> u64 {
            let first = bytes[*at];
            *at += 1;
            if first <= 247 {
                return u64::from(first);
            }
            let width = usize::from(first - 247);
            assert!((1..=8).contains(&width));
            let mut raw = [0u8; 8];
            raw[..width].copy_from_slice(&bytes[*at..*at + width]);
            *at += width;
            u64::from_le_bytes(raw)
        }

        fn branch_children(bytes: &[u8], at: &mut usize, head: u8) -> usize {
            let marker = usize::from(head & 0x3f);
            if marker == 0 {
                return 0;
            }
            let _first_child_distance = varint(bytes, at);
            if marker < 32 {
                *at += marker;
                marker
            } else {
                let count = bytes[*at..*at + 32]
                    .iter()
                    .map(|byte| byte.count_ones() as usize)
                    .sum();
                *at += 32;
                count
            }
        }

        let head = bytes[node_id];
        let mut at = node_id + 1;
        let node_value = (head & 0x40 != 0).then(|| varint(bytes, &mut at));
        if head & 0x80 == 0 {
            let children = branch_children(bytes, &mut at, head);
            return (node_value, node_id, 0, 0, children, None);
        }
        let wrapper_id =
            (head & 1 != 0).then(|| node_id - usize::try_from(varint(bytes, &mut at)).unwrap());
        let line_id = node_id - usize::try_from(varint(bytes, &mut at)).unwrap();
        let mut line_at = line_id;
        let line_len = usize::try_from(varint(bytes, &mut line_at)).unwrap();
        let (value, value_source_offset, children) = match wrapper_id {
            None => (node_value, node_id, 0),
            Some(wrapper_id) => {
                let wrapper_head = bytes[wrapper_id];
                let mut wrapper_at = wrapper_id + 1;
                let value = (wrapper_head & 0x40 != 0).then(|| varint(bytes, &mut wrapper_at));
                let children = branch_children(bytes, &mut wrapper_at, wrapper_head);
                (value, wrapper_id, children)
            }
        };
        (value, value_source_offset, line_at, line_len, children, Some(line_id))
    }

    fn check_generated_validated_plan_source(bytes: &[u8], fragment_seed: u64) {
        let fragmented = FragmentedSource::new(bytes, fragment_seed);
        let mut cursor =
            CanonicalCompactTrieIndexedTopologyCursor::from_source(fragmented).unwrap();
        let source = cursor.validated_plan_source();
        let records = (0..source.record_count())
            .map(|index| source.record(index).unwrap())
            .collect::<Vec<_>>();
        assert!(!source.is_empty());
        assert!(!records.is_empty());
        assert_eq!(
            source.value_summary().value_count(),
            records.iter().filter(|record| record.has_value()).count()
        );
        let mut value_ordinal = 0usize;
        let mut first_nonzero = None;
        let mut first_nonsequential = None;
        for record in &records {
            if record.has_value() {
                if record.value != 0 {
                    first_nonzero.get_or_insert((record.value_source_offset, record.value));
                }
                if record.value != value_ordinal as u64 {
                    first_nonsequential.get_or_insert((
                        record.value_source_offset,
                        record.value,
                        value_ordinal as u64,
                    ));
                }
                value_ordinal += 1;
            }
        }
        assert_eq!(source.value_summary().value_count(), value_ordinal);
        assert_eq!(source.value_summary().first_nonzero(), first_nonzero);
        assert_eq!(source.value_summary().first_nonsequential(), first_nonsequential);
        assert_eq!(records[0].has_incoming(), false);
        assert!(source.record(records.len()).is_none());

        // Child counts and preorder alone determine the postorder used by the
        // producer replay. Compute it independently of the validator's frames.
        let mut open = Vec::<(usize, usize)>::new();
        let mut expected_finish = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            if index != 0 {
                while open.last().is_some_and(|(_, remaining)| *remaining == 0) {
                    let (finished, _) = open.pop().unwrap();
                    expected_finish.push(records[finished].node_id);
                }
                let (_, remaining) = open.last_mut().expect("every child has a preorder parent");
                *remaining -= 1;
                assert!(record.has_incoming());
            }
            open.push((index, usize::from(record.child_count)));
        }
        while let Some((finished, remaining)) = open.pop() {
            assert_eq!(remaining, 0);
            expected_finish.push(records[finished].node_id);
        }
        assert_eq!(expected_finish, cursor.state.plan.audit_finish_node_ids);
        assert_eq!(expected_finish.last(), Some(&records[0].node_id));

        // The validation-local GX hash is the hash of the bytes named by the
        // admitted record, also when its indexed image is fragmented.
        let mut lines = 0;
        let mut independently_counted_events = 0usize;
        for (index, record) in records.into_iter().enumerate() {
            assert!(record.node_id < bytes.len() - 8);
            assert_eq!(record.flags & !3, 0);
            let (value, value_source_offset, line_start, line_len, children, line_id) =
                independently_decode_record(bytes, record.node_id);
            assert_eq!(value.is_some(), record.has_value());
            assert_eq!(value.unwrap_or_default(), record.value);
            assert_eq!(value_source_offset, record.value_source_offset);
            assert_eq!(line_start, record.line_start);
            assert_eq!(line_len, record.line_len);
            assert_eq!(children, usize::from(record.child_count));
            independently_counted_events += 3 * line_len + if index == 0 { 2 } else { 3 };
            if let Some(line_id) = line_id {
                lines += 1;
                let suffix = line_start..line_start + line_len;
                let used = cursor.state.plan.audit_line_hashes[&line_id];
                assert_eq!(used, canonical_line_hash(&bytes[suffix.clone()]));
                assert_eq!(used, indexed_canonical_line_hash(cursor.source(), suffix).unwrap());
            }
        }
        assert!(lines >= 2);
        assert_eq!(source.topology_event_count(), independently_counted_events);
        let mut scalar_events = 0usize;
        while cursor.next_event().unwrap().is_some() {
            scalar_events += 1;
        }
        assert_eq!(scalar_events, independently_counted_events);
    }

    #[test]
    fn validated_event_count_rejects_checked_arithmetic_overflow() {
        assert_eq!(checked_add_topology_record_events(0, 0, false), Some(2));
        assert_eq!(checked_add_topology_record_events(0, 0, true), Some(3));
        assert_eq!(checked_add_topology_record_events(usize::MAX - 2, 0, false), Some(usize::MAX));
        assert_eq!(checked_add_topology_record_events(usize::MAX - 1, 0, false), None);
        assert_eq!(checked_add_topology_record_events(0, usize::MAX / 3 + 1, false), None);
    }

    #[test]
    fn validated_event_count_matches_empty_set_and_map_act_traces() {
        let empty = PathMap::<u64>::new();
        let mut set = PathMap::new();
        set.set_val_at(b"alpha/a".to_vec(), 0u64);
        set.set_val_at(b"alpha/b".to_vec(), 0u64);
        let mut map = PathMap::new();
        map.set_val_at(b"alpha/a".to_vec(), 0u64);
        map.set_val_at(b"alpha/b".to_vec(), 1u64);
        let encoded = [
            ArenaCompactTree::from_zipper(empty.read_zipper(), |value| *value)
                .get_data()
                .to_vec(),
            ArenaCompactTree::from_zipper(set.read_zipper(), |value| *value)
                .get_data()
                .to_vec(),
            ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
                .get_data()
                .to_vec(),
        ];
        for (index, bytes) in encoded.iter().enumerate() {
            let source = AsRefIndexedByteSource::new(bytes.as_slice());
            let mut cursor =
                CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
            assert_eq!(cursor.validated_plan_source().is_empty(), index == 0);
            let expected = cursor.validated_plan_source().topology_event_count();
            let mut actual = 0usize;
            while cursor.next_event().unwrap().is_some() {
                actual += 1;
            }
            assert_eq!(actual, expected);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn generated_branched_line_publications_match_scalar_events_and_anchors(
            operations in proptest::collection::vec((
                proptest::collection::vec(any::<u8>(), 1..25), any::<u64>(),
            ), 0..12),
            fragment_seed in any::<u64>(),
        ) {
            let mut map = PathMap::new();
            map.set_val_at(b"alpha/long/shared/first/leaf".to_vec(), 1u64);
            map.set_val_at(b"alpha/long/shared/second/leaf".to_vec(), 2u64);
            map.set_val_at(b"beta/another/long/branch/leaf".to_vec(), 3u64);
            map.set_val_at(
                b"gamma/a/very/long/unique/compressed/suffix/that/crosses/pages".to_vec(),
                4u64,
            );
            for (path, value) in operations {
                let mut generated_path = b"random/".to_vec();
                generated_path.extend(path);
                map.set_val_at(generated_path, value);
            }
            let bytes = ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
                .get_data()
                .to_vec();
            check_generated_validated_plan_source(&bytes, fragment_seed);
            for schedule in 0..3 {
                check_generated_line_publications(&bytes, fragment_seed, schedule);
            }
        }
    }

    #[test]
    fn indexed_line_hash_matches_contiguous_gxhash_at_boundaries_and_random_lengths() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut payload = vec![0u8; 4099];
        for byte in &mut payload {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        let source = FragmentedSource::new(&payload, 0x243f_6a88_85a3_08d3);
        let mut lengths = vec![
            0, 1, 2, 15, 16, 17, 31, 32, 33, 47, 48, 49, 63, 64, 65, 127, 128, 129, 143, 144, 145,
            255, 256, 257, 1023, 1024, 1025, 2048, 3072,
        ];
        for _ in 0..256 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            lengths.push((state as usize) % 3073);
        }
        for len in lengths {
            let start = 7;
            let end = start + len;
            assert_eq!(
                indexed_canonical_line_hash(&source, start..end).unwrap(),
                canonical_line_hash(&payload[start..end]),
                "gxhash mismatch at length {len}",
            );
        }
    }

    #[test]
    fn validated_plan_exposes_exact_finish_order_and_replay_hashes() {
        let mut map = PathMap::new();
        map.set_val_at(b"alpha/shared/a".to_vec(), 0u64);
        map.set_val_at(b"beta/shared/b".to_vec(), 0u64);
        map.set_val_at(b"gamma/shared/c".to_vec(), 0u64);
        let bytes = ArenaCompactTree::from_zipper(map.read_zipper(), |value| *value)
            .get_data()
            .to_vec();
        let contiguous = AsRefIndexedByteSource::new(bytes.as_slice());
        let fragmented = FragmentedSource::new(&bytes, 0xd131_0ba6_98df_b5ac);
        let contiguous_plan = validate_direct_canonical_act(&contiguous).unwrap();
        let fragmented_plan = validate_direct_canonical_act(&fragmented).unwrap();
        let record_projection = |plan: &ValidatedActPlan| {
            plan.records
                .iter()
                .map(|record| {
                    (
                        record.node_id,
                        record.value,
                        record.value_source_offset,
                        record.line_start,
                        record.line_len,
                        record.incoming_source_offset,
                        record.child_count,
                        record.incoming_byte,
                        record.flags,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(record_projection(&fragmented_plan), record_projection(&contiguous_plan));
        assert_eq!(fragmented_plan.audit_finish_node_ids, contiguous_plan.audit_finish_node_ids);
        assert_eq!(fragmented_plan.audit_line_hashes, contiguous_plan.audit_line_hashes);
        assert_eq!(fragmented_plan.audit_finish_node_ids.len(), fragmented_plan.records.len());
        assert!(fragmented_plan.records.len() >= 4);
        assert_eq!(
            fragmented_plan.audit_finish_node_ids.last(),
            Some(&fragmented_plan.records[0].node_id)
        );
        let record_ids = fragmented_plan
            .records
            .iter()
            .map(|record| record.node_id)
            .collect::<std::collections::HashSet<_>>();
        let finish_ids = fragmented_plan
            .audit_finish_node_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(record_ids, finish_ids);
        assert!(fragmented_plan.audit_line_hashes.len() >= 2);
        for record in fragmented_plan.records.iter() {
            let mut edges = Vec::new();
            let mut counters = new_act_work_counters();
            let semantic =
                direct_semantic_node(&fragmented, record.node_id, &mut edges, &mut counters)
                    .unwrap();
            if let Some(line) = semantic.line {
                let used = fragmented_plan.audit_line_hashes[&line.encoded.start];
                assert_eq!(used, canonical_line_hash(&bytes[line.suffix.clone()]));
                assert_eq!(used, indexed_canonical_line_hash(&fragmented, line.suffix).unwrap());
            }
        }
    }

    #[test]
    fn forced_line_hash_collision_preserves_range_equality_and_last_writer_semantics() {
        let source = AsRefIndexedByteSource::new(b"aba".as_slice());
        let mut replay = DirectCanonicalLineReplay::new(&source);
        replay.force_hash(7);
        let mut counters = CanonicalCompactTrieWorkCounters::default();
        let mut position = 0;
        let first = ActLineData {
            suffix: 0..1,
            encoded: 0..1,
        };
        let second = ActLineData {
            suffix: 1..2,
            encoded: 1..2,
        };
        let third = ActLineData {
            suffix: 2..3,
            encoded: 2..3,
        };

        assert_eq!(
            replay
                .expected_line_id(&first, &mut position, &mut counters)
                .unwrap(),
            0,
        );
        assert_eq!(counters.line_hash_collisions, 0);
        assert_eq!(
            replay
                .expected_line_id(&second, &mut position, &mut counters)
                .unwrap(),
            1,
        );
        assert_eq!(counters.line_hash_collisions, 1);
        assert_eq!(
            replay
                .expected_line_id(&third, &mut position, &mut counters)
                .unwrap(),
            2,
            "the unequal second line must replace the first in the hash bucket",
        );
        assert_eq!(counters.line_hash_collisions, 2);
        assert_eq!(
            replay
                .expected_line_id(&third, &mut position, &mut counters)
                .unwrap(),
            2,
            "equal ranges may reuse the latest physical line",
        );
        assert_eq!(counters.line_hash_collisions, 2);
        assert_eq!(position, 3);
        assert_eq!(counters.line_hash_bytes, 3);
        assert_eq!(counters.line_equality_bytes, 3);
    }

    #[test]
    fn indexed_topology_state_matches_contiguous_cursor_events() {
        let bytes = canonical_line_image(&[0x01, 0x80, 0xff, 0x42], 23);
        let expected = {
            let mut cursor = CanonicalCompactTrieTopologyCursor::new(bytes.as_slice()).unwrap();
            std::iter::from_fn(|| cursor.next_event()).collect::<Vec<_>>()
        };
        let source = FragmentedSource::new(&bytes, 0x1319_8a2e_0370_7344);
        let mut state = CanonicalCompactTrieIndexedTopologyCursor::from_source(source).unwrap();
        let mut actual = Vec::new();
        while let Some(event) = state.next_event().unwrap() {
            actual.push(event);
        }
        assert_eq!(actual, expected);

        state.rewind();
        let replayed = std::iter::from_fn(|| state.next_event().unwrap()).collect::<Vec<_>>();
        assert_eq!(replayed, expected);
    }

    #[test]
    fn indexed_and_contiguous_validation_report_identical_errors() {
        let valid = canonical_line_image(&[0x61, 0x62], 1);
        let mut cases = vec![Vec::new(), valid[..valid.len() - 1].to_vec()];
        let mut bad_magic = valid.clone();
        bad_magic[0] ^= 1;
        cases.push(bad_magic);
        let mut bad_trailer = valid.clone();
        *bad_trailer.last_mut().unwrap() = 1;
        cases.push(bad_trailer);
        let mut reserved_line_bit = valid.clone();
        let root = u64::from_le_bytes(reserved_line_bit[8..16].try_into().unwrap()) as usize;
        reserved_line_bit[root] |= 0x02;
        cases.push(reserved_line_bit);

        for (index, bytes) in cases.iter().enumerate() {
            let contiguous = CanonicalCompactTrieTopologyCursor::new(bytes.as_slice())
                .err()
                .unwrap();
            let source = FragmentedSource::new(bytes, index as u64 + 1);
            let indexed = CanonicalCompactTrieIndexedTopologyCursor::from_source(source)
                .err()
                .unwrap();
            assert_eq!(indexed.to_string(), contiguous.to_string(), "case {index}");
            assert_eq!(indexed.source_offset(), contiguous.source_offset(), "case {index}",);
        }
    }
}
