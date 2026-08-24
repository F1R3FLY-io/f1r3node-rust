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
}

impl CompactTrieError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompactTrieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for CompactTrieError {}

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

    fn byte_with_source(&self, bytes: &[u8], index: usize) -> (u8, usize) {
        match self {
            Self::Sparse(range) => {
                let offset = range.start + index;
                (bytes[offset], offset)
            }
            Self::Dense { range, .. } => {
                let mut remaining = index;
                for (word, mask) in bytes[range.clone()].iter().copied().enumerate() {
                    for bit in 0..8u8 {
                        if mask & (1 << bit) == 0 {
                            continue;
                        }
                        if remaining == 0 {
                            return ((word * 8 + usize::from(bit)) as u8, range.start + word);
                        }
                        remaining -= 1;
                    }
                }
                unreachable!("validated dense ACT branch index")
            }
        }
    }

    fn byte(&self, bytes: &[u8], index: usize) -> u8 { self.byte_with_source(bytes, index).0 }
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

pub struct CanonicalCompactTrieTopologyCursor<B> {
    bytes: B,
    root_id: usize,
    empty: bool,
    stack: Vec<DirectTopologyFrame>,
}

impl<'a> CanonicalCompactTrieTopologyCursor<&'a [u8]> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, CompactTrieError> { Self::from_backing(bytes) }
}

impl<B> CanonicalCompactTrieTopologyCursor<B>
where B: AsRef<[u8]>
{
    pub fn from_backing(bytes: B) -> Result<Self, CompactTrieError> {
        let (root_id, empty) = validate_direct_canonical_act(bytes.as_ref())?;
        let mut cursor = Self {
            bytes,
            root_id,
            empty,
            stack: Vec::new(),
        };
        cursor.rewind();
        Ok(cursor)
    }

    pub const fn is_empty(&self) -> bool { self.empty }

    pub fn rewind(&mut self) {
        self.stack.clear();
        let root = direct_semantic_node(self.bytes.as_ref(), self.root_id)
            .expect("canonical ACT root remains valid while borrowed");
        self.stack.push(DirectTopologyFrame {
            next_child_id: root.children.first_child,
            node: root,
            next_child: 0,
            line_index: 0,
            leave_index: 0,
            stage: DirectTopologyStage::Value,
        });
    }

    pub fn next_event(&mut self) -> Option<CanonicalCompactTrieTopologyEvent> {
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
                let frame = self.stack.last_mut()?;
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
                        let byte = self.bytes.as_ref()[source_offset];
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
                            let child = parse_direct_act_node(self.bytes.as_ref(), child_id)
                                .expect("canonical ACT child remains valid while borrowed");
                            let (byte, source_offset) = frame
                                .node
                                .children
                                .child_bytes
                                .byte_with_source(self.bytes.as_ref(), frame.next_child);
                            frame.next_child += 1;
                            frame.next_child_id = (frame.next_child < frame.node.children.len())
                                .then(|| child_id + child.encoded_len);
                            let child = direct_semantic_node(self.bytes.as_ref(), child_id)
                                .expect("canonical ACT child remains valid while borrowed");
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
                Action::Event(event) => return Some(event),
                Action::Descend { event, child } => {
                    self.stack.push(DirectTopologyFrame {
                        next_child_id: child.children.first_child,
                        node: child,
                        next_child: 0,
                        line_index: 0,
                        leave_index: 0,
                        stage: DirectTopologyStage::Value,
                    });
                    return Some(event);
                }
                Action::Pop(event) => {
                    self.stack.pop();
                    return Some(event);
                }
                Action::Continue => {}
            }
        }
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
        let (root_id, empty) = validate_direct_canonical_act(bytes.as_ref())?;
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
        let root = direct_semantic_node(self.bytes.as_ref(), self.root_id)
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
                    let child = parse_direct_act_node(self.bytes.as_ref(), child_id)
                        .expect("canonical ACT child remains valid while borrowed");
                    let edge = frame
                        .node
                        .children
                        .child_bytes
                        .byte(self.bytes.as_ref(), frame.next_child);
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
                    let child = direct_semantic_node(self.bytes.as_ref(), child_id)
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

fn parse_direct_act_node(bytes: &[u8], node_id: usize) -> Result<DirectActNode, CompactTrieError> {
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
            let start = cursor;
            let raw = take(bytes, &mut cursor, encoded_count, "ACTree03 child bytes")?;
            if raw.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(CompactTrieError::new(
                    "ACTree03 branch bytes are not strictly ascending",
                ));
            }
            DirectBranchBytes::Sparse(start..cursor)
        } else {
            let start = cursor;
            let mask = take(bytes, &mut cursor, 32, "ACTree03 child mask")?;
            let len = mask.iter().map(|byte| byte.count_ones() as usize).sum();
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
        let mut line_cursor = line_id;
        let line_len = read_act_varint(bytes, &mut line_cursor, "ACTree03 line length")?;
        let line_len = usize::try_from(line_len)
            .map_err(|_| CompactTrieError::new("ACTree03 line length overflows usize"))?;
        if line_len == 0 {
            return Err(CompactTrieError::new("ACTree03 line suffix is empty"));
        }
        let suffix_start = line_cursor;
        take(bytes, &mut line_cursor, line_len, "ACTree03 line suffix")?;
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

fn direct_semantic_node(bytes: &[u8], id: usize) -> Result<DirectSemanticNode, CompactTrieError> {
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

fn validate_direct_canonical_act(bytes: &[u8]) -> Result<(usize, bool), CompactTrieError> {
    if bytes.len() < ACT_HEADER_LEN + 9 {
        return Err(CompactTrieError::new(
            "ACTree03 payload is shorter than header, root, and trailer",
        ));
    }
    if bytes[..COMPACT_TREE_MAGIC.len()] != COMPACT_TREE_MAGIC {
        return Err(CompactTrieError::new("invalid ACTree03 magic"));
    }
    let root_id = usize::try_from(u64::from_le_bytes(
        bytes[8..16]
            .try_into()
            .expect("the ACT header root slot is eight bytes"),
    ))
    .map_err(|_| CompactTrieError::new("ACTree03 root offset overflows usize"))?;
    if !(ACT_HEADER_LEN..bytes.len()).contains(&root_id) {
        return Err(CompactTrieError::new("ACTree03 root offset is outside the arena"));
    }
    let trailer_start = bytes.len() - 8;
    if bytes[trailer_start..] != [0; 8] {
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
    let mut lines = CanonicalLineReplay::new(bytes);

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
