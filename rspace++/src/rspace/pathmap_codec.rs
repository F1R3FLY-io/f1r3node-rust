//! Stack-safe inspection of PathMap's canonical `ACTree03` byte image.
//!
//! PathMap exposes a compact-arena writer but no in-memory reader constructor.
//! This module is the single checked reader used by RSpace Bags and model-layer
//! EPathMaps. It validates arbitrary bytes without recursion, preserves line
//! compression, and can expose a one-line path as a range into the input arena.

use std::collections::{HashMap, HashSet};
use std::fmt;
#[cfg(not(any(miri, target_arch = "riscv64")))]
use std::hash::Hasher;
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

    /// Parse and prove that the bytes are exactly an image of PathMap 0.2.2's
    /// deterministic compact writer.
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

#[derive(Clone)]
enum DirectBranchBytes {
    Sparse(Range<usize>),
    Dense { range: Range<usize>, len: usize },
}

impl DirectBranchBytes {
    fn len(&self) -> usize {
        match self {
            Self::Sparse(range) => range.len(),
            Self::Dense { len, .. } => *len,
        }
    }

    fn byte_with_source<S>(
        &self,
        bytes: &S,
        index: usize,
    ) -> Result<(u8, usize), CompactTrieError>
    where
        S: IndexedByteSource + ?Sized,
    {
        match self {
            Self::Sparse(range) => {
                let offset = range.start + index;
                let byte = bytes.byte_at(offset).ok_or_else(|| {
                    CompactTrieError::at(offset, "ACTree03 sparse branch byte is unavailable")
                })?;
                Ok((byte, offset))
            }
            Self::Dense { range, .. } => {
                let mut remaining = index;
                for (word, offset) in (range.start..range.end).enumerate() {
                    let mask = bytes.byte_at(offset).ok_or_else(|| {
                        CompactTrieError::at(offset, "ACTree03 dense branch mask is unavailable")
                    })?;
                    for bit in 0..8u8 {
                        if mask & (1 << bit) == 0 {
                            continue;
                        }
                        if remaining == 0 {
                            return Ok(((word * 8 + usize::from(bit)) as u8, offset));
                        }
                        remaining -= 1;
                    }
                }
                Err(CompactTrieError::at(
                    range.start,
                    "ACTree03 dense branch index exceeds its mask",
                ))
            }
        }
    }

    fn byte<S>(&self, bytes: &S, index: usize) -> Result<u8, CompactTrieError>
    where S: IndexedByteSource + ?Sized {
        self.byte_with_source(bytes, index).map(|(byte, _)| byte)
    }
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
            child_bytes: DirectBranchBytes::Sparse(0..0),
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

#[derive(Clone)]
struct DirectReplayFrame {
    node: DirectSemanticNode,
    next_child: usize,
    next_child_id: Option<usize>,
}

struct DirectWalkFrame {
    node: DirectSemanticNode,
    path_len: usize,
    next_child: usize,
    next_child_id: Option<usize>,
    entered: bool,
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
    node: DirectSemanticNode,
    next_child: usize,
    next_child_id: Option<usize>,
    line_index: usize,
    leave_index: usize,
    stage: DirectTopologyStage,
}

pub struct CanonicalCompactTrieEndpoint<'a> {
    pub path: &'a [u8],
    pub value: Option<u64>,
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

#[derive(Clone)]
pub struct CanonicalCompactTrieTopologyState {
    root_id: usize,
    empty: bool,
    stack: Vec<DirectTopologyFrame>,
}

impl CanonicalCompactTrieTopologyState {
    pub fn new<S>(source: &S) -> Result<Self, CompactTrieError>
    where S: IndexedByteSource + ?Sized {
        let (root_id, empty) = validate_direct_canonical_act(source)?;
        let mut state = Self {
            root_id,
            empty,
            stack: Vec::new(),
        };
        state.rewind(source)?;
        Ok(state)
    }

    pub const fn is_empty(&self) -> bool { self.empty }

    pub fn rewind<S>(&mut self, source: &S) -> Result<(), CompactTrieError>
    where S: IndexedByteSource + ?Sized {
        self.stack.clear();
        let root = direct_semantic_node(source, self.root_id)?;
        self.stack.push(DirectTopologyFrame {
            next_child_id: root.children.first_child,
            node: root,
            next_child: 0,
            line_index: 0,
            leave_index: 0,
            stage: DirectTopologyStage::Value,
        });
        Ok(())
    }

    pub fn next_event<S>(
        &mut self,
        source: &S,
    ) -> Result<Option<CanonicalCompactTrieTopologyEvent>, CompactTrieError>
    where
        S: IndexedByteSource + ?Sized,
    {
        enum Action {
            Event(CanonicalCompactTrieTopologyEvent),
            Descend {
                event: CanonicalCompactTrieTopologyEvent,
                child: DirectSemanticNode,
            },
            Pop(CanonicalCompactTrieTopologyEvent),
            Continue,
        }

        loop {
            let action = {
                let Some(frame) = self.stack.last_mut() else {
                    return Ok(None);
                };
                match frame.stage {
                    DirectTopologyStage::Value => {
                        frame.stage = if frame.node.line.is_some() {
                            DirectTopologyStage::LineEdge
                        } else {
                            DirectTopologyStage::Children
                        };
                        Action::Event(CanonicalCompactTrieTopologyEvent::Value {
                            value: frame
                                .node
                                .line
                                .is_none()
                                .then_some(frame.node.value)
                                .flatten(),
                            source_offset: frame.node.id,
                        })
                    }
                    DirectTopologyStage::LineEdge => {
                        let line = frame
                            .node
                            .line
                            .as_ref()
                            .expect("line stage has canonical line data");
                        let source_offset = line.suffix.start + frame.line_index;
                        let byte = source.byte_at(source_offset).ok_or_else(|| {
                            CompactTrieError::at(
                                source_offset,
                                "ACTree03 line suffix byte is unavailable",
                            )
                        })?;
                        frame.stage = DirectTopologyStage::LineValue;
                        Action::Event(CanonicalCompactTrieTopologyEvent::Edge {
                            byte,
                            source_offset,
                        })
                    }
                    DirectTopologyStage::LineValue => {
                        let line = frame
                            .node
                            .line
                            .as_ref()
                            .expect("line stage has canonical line data");
                        let line_offset = line.suffix.start + frame.line_index;
                        let final_value = frame.line_index + 1 == line.suffix.len();
                        let value = final_value.then_some(frame.node.value).flatten();
                        let source_offset = if value.is_some() {
                            frame.node.wrapper.map_or(frame.node.id, |(id, _)| id)
                        } else {
                            line_offset
                        };
                        frame.line_index += 1;
                        frame.stage = if frame.line_index < line.suffix.len() {
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
                        if frame.next_child == frame.node.children.len() {
                            frame.stage = DirectTopologyStage::Leave;
                            Action::Continue
                        } else {
                            let child_id = frame
                                .next_child_id
                                .expect("canonical ACT child sequence remains complete");
                            let child = parse_direct_act_node(source, child_id)?;
                            let (byte, source_offset) = frame
                                .node
                                .children
                                .child_bytes
                                .byte_with_source(source, frame.next_child)?;
                            frame.next_child += 1;
                            frame.next_child_id = (frame.next_child < frame.node.children.len())
                                .then(|| child_id + child.encoded_len);
                            let child = direct_semantic_node(source, child_id)?;
                            Action::Descend {
                                event: CanonicalCompactTrieTopologyEvent::Edge {
                                    byte,
                                    source_offset,
                                },
                                child,
                            }
                        }
                    }
                    DirectTopologyStage::Leave => {
                        let line_len = frame.node.line.as_ref().map_or(0, |line| line.suffix.len());
                        let source_offset = if frame.leave_index < line_len {
                            frame
                                .node
                                .line
                                .as_ref()
                                .expect("line leave has canonical line data")
                                .suffix
                                .end -
                                1 -
                                frame.leave_index
                        } else {
                            frame.node.id
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
                Action::Descend { event, child } => {
                    self.stack.push(DirectTopologyFrame {
                        next_child_id: child.children.first_child,
                        node: child,
                        next_child: 0,
                        line_index: 0,
                        leave_index: 0,
                        stage: DirectTopologyStage::Value,
                    });
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

pub struct CanonicalCompactTrieTopologyCursor<B> {
    bytes: B,
    state: CanonicalCompactTrieTopologyState,
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
            CanonicalCompactTrieTopologyState::new(&source)?
        };
        Ok(Self { bytes, state })
    }

    pub const fn is_empty(&self) -> bool { self.state.is_empty() }

    pub fn rewind(&mut self) {
        let source = AsRefIndexedByteSource::new(&self.bytes);
        self.state
            .rewind(&source)
            .expect("canonical ACT root remains valid while owned");
    }

    pub fn next_event(&mut self) -> Option<CanonicalCompactTrieTopologyEvent> {
        let source = AsRefIndexedByteSource::new(&self.bytes);
        self.state
            .next_event(&source)
            .expect("canonical ACT remains valid while owned")
    }
}

pub struct CanonicalCompactTrieCursor<B> {
    bytes: B,
    root_id: usize,
    empty: bool,
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
        let (root_id, empty) = {
            let source = AsRefIndexedByteSource::new(&bytes);
            validate_direct_canonical_act(&source)?
        };
        let mut cursor = Self {
            bytes,
            root_id,
            empty,
            stack: Vec::new(),
            path: Vec::new(),
        };
        cursor.rewind();
        Ok(cursor)
    }

    pub const fn is_empty(&self) -> bool { self.empty }

    pub fn rewind(&mut self) {
        self.path.clear();
        self.stack.clear();
        let source = AsRefIndexedByteSource::new(&self.bytes);
        let root = direct_semantic_node(&source, self.root_id)
            .expect("canonical ACT root remains valid while borrowed");
        let next_child_id = root.children.first_child;
        self.stack.push(DirectWalkFrame {
            node: root,
            path_len: 0,
            next_child: 0,
            next_child_id,
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
            Yield(Option<u64>),
            Pop(usize),
            Push {
                child_id: usize,
                edge: u8,
                path_len: usize,
            },
            Continue,
        }

        loop {
            let action = {
                let frame = self.stack.last_mut()?;
                if !frame.entered {
                    frame.entered = true;
                    if let Some(line) = &frame.node.line {
                        self.path
                            .extend_from_slice(&self.bytes.as_ref()[line.suffix.clone()]);
                    }
                    if frame.node.value.is_some() || frame.node.children.len() == 0 {
                        if !value_only || frame.node.value.is_some() {
                            Action::Yield(frame.node.value)
                        } else {
                            Action::Continue
                        }
                    } else {
                        Action::Continue
                    }
                } else if frame.next_child == frame.node.children.len() {
                    Action::Pop(frame.path_len)
                } else {
                    let child_id = frame
                        .next_child_id
                        .expect("canonical ACT child sequence remains complete");
                    let source = AsRefIndexedByteSource::new(&self.bytes);
                    let child = parse_direct_act_node(&source, child_id)
                        .expect("canonical ACT child remains valid while borrowed");
                    let edge = frame
                        .node
                        .children
                        .child_bytes
                        .byte(&source, frame.next_child)
                        .expect("canonical ACT branch remains valid while borrowed");
                    frame.next_child += 1;
                    frame.next_child_id = (frame.next_child < frame.node.children.len())
                        .then(|| child_id + child.encoded_len);
                    Action::Push {
                        child_id,
                        edge,
                        path_len: self.path.len(),
                    }
                }
            };

            match action {
                Action::Yield(value) => {
                    return Some(CanonicalCompactTrieEndpoint {
                        path: &self.path,
                        value,
                    });
                }
                Action::Pop(path_len) => {
                    self.stack.pop();
                    self.path.truncate(path_len);
                }
                Action::Push {
                    child_id,
                    edge,
                    path_len,
                } => {
                    self.path.push(edge);
                    let source = AsRefIndexedByteSource::new(&self.bytes);
                    let child = direct_semantic_node(&source, child_id)
                        .expect("canonical ACT child remains valid while borrowed");
                    let next_child_id = child.children.first_child;
                    self.stack.push(DirectWalkFrame {
                        node: child,
                        path_len,
                        next_child: 0,
                        next_child_id,
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

fn parse_direct_act_node<S>(bytes: &S, node_id: usize) -> Result<DirectActNode, CompactTrieError>
where S: IndexedByteSource + ?Sized {
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
        let child_bytes = if encoded_count < 32 {
            let start = cursor;
            let range = take_indexed(bytes, &mut cursor, encoded_count, "ACTree03 child bytes")?;
            for offset in range.start + 1..range.end {
                let previous = bytes.byte_at(offset - 1).ok_or_else(|| {
                    CompactTrieError::at(offset - 1, "truncated ACTree03 child bytes")
                })?;
                let current = bytes.byte_at(offset).ok_or_else(|| {
                    CompactTrieError::at(offset, "truncated ACTree03 child bytes")
                })?;
                if previous >= current {
                    return Err(CompactTrieError::at(
                        offset,
                        "ACTree03 branch bytes are not strictly ascending",
                    ));
                }
            }
            DirectBranchBytes::Sparse(start..cursor)
        } else {
            let start = cursor;
            let mask = take_indexed(bytes, &mut cursor, 32, "ACTree03 child mask")?;
            let mut len = 0;
            for offset in mask {
                let byte = bytes
                    .byte_at(offset)
                    .ok_or_else(|| CompactTrieError::at(offset, "truncated ACTree03 child mask"))?;
                len += byte.count_ones() as usize;
            }
            if len < 32 {
                return Err(CompactTrieError::new(
                    "ACTree03 dense child mask contains fewer than 32 branches",
                ));
            }
            DirectBranchBytes::Dense {
                range: start..cursor,
                len,
            }
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

fn direct_semantic_node<S>(bytes: &S, id: usize) -> Result<DirectSemanticNode, CompactTrieError>
where S: IndexedByteSource + ?Sized {
    let node = parse_direct_act_node(bytes, id)?;
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
            let wrapper = parse_direct_act_node(bytes, wrapper_id)?;
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
    seen: &mut HashSet<usize>,
    node: &DirectSemanticNode,
) -> Result<(), CompactTrieError> {
    if !seen.insert(node.id) {
        return Err(CompactTrieError::new("ACTree03 node is referenced by more than one edge"));
    }
    if let Some((wrapper, _)) = node.wrapper {
        if !seen.insert(wrapper) {
            return Err(CompactTrieError::new("ACTree03 node is referenced by more than one edge"));
        }
    }
    Ok(())
}

fn validate_direct_canonical_act<S>(bytes: &S) -> Result<(usize, bool), CompactTrieError>
where S: IndexedByteSource + ?Sized {
    if bytes.len() < ACT_HEADER_LEN + 9 {
        return Err(CompactTrieError::new(
            "ACTree03 payload is shorter than header, root, and trailer",
        ));
    }
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
    let trailer_start = bytes.len() - 8;
    let mut trailer = [0u8; 8];
    if !bytes.copy_exact(trailer_start, &mut trailer) || trailer != [0; 8] {
        return Err(CompactTrieError::new("ACTree03 canonical varint trailer is not zero-filled"));
    }

    let root = direct_semantic_node(bytes, root_id)?;
    let empty = root.line.is_none() && root.value.is_none() && root.children.len() == 0;
    let root_len = root.encoded_len;
    let mut seen = HashSet::with_capacity(bytes.len().min(1024));
    register_direct_semantic_node(&mut seen, &root)?;
    let mut frames = vec![DirectReplayFrame {
        next_child_id: root.children.first_child,
        node: root,
        next_child: 0,
    }];
    let mut position = ACT_HEADER_LEN;
    let mut lines = DirectCanonicalLineReplay::new(bytes);

    while let Some(frame) = frames.last_mut() {
        if frame.next_child < frame.node.children.len() {
            let child_id = frame
                .next_child_id
                .ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
            if child_id >= frame.node.children.parent_id {
                return Err(CompactTrieError::new("ACTree03 child does not precede its parent"));
            }
            let child = direct_semantic_node(bytes, child_id)?;
            frame.next_child += 1;
            frame.next_child_id = (frame.next_child < frame.node.children.len())
                .then(|| {
                    child_id
                        .checked_add(child.encoded_len)
                        .ok_or_else(|| CompactTrieError::new("ACTree03 sibling offset overflow"))
                })
                .transpose()?;
            register_direct_semantic_node(&mut seen, &child)?;
            let next_child_id = child.children.first_child;
            frames.push(DirectReplayFrame {
                node: child,
                next_child: 0,
                next_child_id,
            });
            continue;
        }

        let frame = frames.pop().expect("a checked replay frame exists");
        let mut child_id = frame.node.children.first_child;
        for index in 0..frame.node.children.len() {
            let id =
                child_id.ok_or_else(|| CompactTrieError::new("ACTree03 child list ended early"))?;
            if id != position {
                return Err(CompactTrieError::new(
                    "ACTree03 child roots are not in producer order",
                ));
            }
            let child = parse_direct_act_node(bytes, id)?;
            position = position
                .checked_add(child.encoded_len)
                .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
            child_id = (index + 1 < frame.node.children.len())
                .then(|| {
                    id.checked_add(child.encoded_len)
                        .ok_or_else(|| CompactTrieError::new("ACTree03 sibling offset overflow"))
                })
                .transpose()?;
        }
        if let Some(line) = &frame.node.line {
            let expected = lines.expected_line_id(line, &mut position)?;
            if expected != line.encoded.start {
                return Err(CompactTrieError::new(
                    "ACTree03 line reuse differs from the canonical writer",
                ));
            }
            if let Some((wrapper, wrapper_len)) = frame.node.wrapper {
                if wrapper != position {
                    return Err(CompactTrieError::new(
                        "ACTree03 line branch wrapper is not in producer order",
                    ));
                }
                position = position
                    .checked_add(wrapper_len)
                    .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
            }
        }
    }

    if root_id != position {
        return Err(CompactTrieError::new("ACTree03 root is not in producer order"));
    }
    position = position
        .checked_add(root_len)
        .ok_or_else(|| CompactTrieError::new("ACTree03 position overflow"))?;
    if position != trailer_start {
        return Err(CompactTrieError::new(
            "ACTree03 arena contains gaps, overlaps, or unreachable objects",
        ));
    }
    Ok((root_id, empty))
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
        }
    }

    fn line_hash(&mut self, line: &ActLineData) -> Result<u64, CompactTrieError> {
        if let Some(hash) = self.hash_by_line.get(&line.encoded.start) {
            return Ok(*hash);
        }
        let hash = indexed_canonical_line_hash(self.source, line.suffix.clone())?;
        self.hash_by_line.insert(line.encoded.start, hash);
        Ok(hash)
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
            let hash = self.line_hash(&first)?;
            let mut by_hash = HashMap::new();
            by_hash.insert(hash, first);
            self.by_hash = Some(by_hash);
        }

        let hash = self.line_hash(line)?;
        if let Some(previous) = self
            .by_hash
            .as_ref()
            .and_then(|by_hash| by_hash.get(&hash))
            .cloned()
        {
            if self
                .source
                .ranges_equal(previous.suffix, line.suffix.clone())
            {
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
mod indexed_direct_tests {
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
    fn indexed_topology_state_matches_contiguous_cursor_events() {
        let bytes = canonical_line_image(&[0x01, 0x80, 0xff, 0x42], 23);
        let expected = {
            let mut cursor = CanonicalCompactTrieTopologyCursor::new(bytes.as_slice()).unwrap();
            std::iter::from_fn(|| cursor.next_event()).collect::<Vec<_>>()
        };
        let source = FragmentedSource::new(&bytes, 0x1319_8a2e_0370_7344);
        let mut state = CanonicalCompactTrieTopologyState::new(&source).unwrap();
        let mut actual = Vec::new();
        while let Some(event) = state.next_event(&source).unwrap() {
            actual.push(event);
        }
        assert_eq!(actual, expected);

        state.rewind(&source).unwrap();
        let replayed =
            std::iter::from_fn(|| state.next_event(&source).unwrap()).collect::<Vec<_>>();
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
            let indexed = CanonicalCompactTrieTopologyState::new(&source)
                .err()
                .unwrap();
            assert_eq!(indexed.to_string(), contiguous.to_string(), "case {index}");
            assert_eq!(indexed.source_offset(), contiguous.source_offset(), "case {index}",);
        }
    }
}
