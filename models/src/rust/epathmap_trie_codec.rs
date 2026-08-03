//! Canonical, prefix-compressed byte snapshots for `EPathMap`.
//!
//! `EPM1` keeps the PathMap trie intact across every serialization boundary:
//! the key topology is PathMap's compact `ACTree03` arena and map values are a
//! separate ordinal table encoded by the generated stack-safe protobuf PDA.
//! No `Vec<Par>` entry projection participates in either direction.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use pathmap::arena_compact::{ArenaCompactTree, COMPACT_TREE_MAGIC};
use pathmap::zipper::ZipperReadOnlyIteration;
#[cfg(test)]
use pathmap::zipper::{ZipperIteration, ZipperMoving};
use pathmap::PathMap;
use prost::bytes::Bytes;

use crate::rhoapi::Par;
use crate::rust::rholang::{protobuf_decoder, protobuf_encoder};

const EPM_MAGIC: &[u8; 4] = b"EPM1";
const EPM_VERSION: u8 = 1;
const ACT_HEADER_LEN: usize = 16;
const ACT_VALUE_FLAG: u8 = 0x40;
const ACT_LINE_FLAG: u8 = 0x80;
const ACT_VARINT_BIAS: u8 = u8::MAX - 8;

/// The homogeneous storage modes admitted by an `EPathMap`.
///
/// `Empty` is deliberately mode-neutral. The first insertion selects `Set` or
/// `Map`. Deleting the last value returns to `Empty` only when no explicit
/// value-free topology remains; otherwise the selected specialization stays
/// observable. A value can never contain a mixture of set-only and
/// value-bearing entries.
pub enum EPathMapRepr<T: Clone + Send + Sync + Unpin + 'static> {
    Empty,
    Set(PathMap<()>),
    Map(PathMap<T>),
}

impl<T: Clone + Send + Sync + Unpin + 'static> Clone for EPathMapRepr<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Set(map) => Self::Set(map.clone()),
            Self::Map(map) => Self::Map(map.clone()),
        }
    }
}

impl<T: Clone + Send + Sync + Unpin + 'static> Default for EPathMapRepr<T> {
    fn default() -> Self { Self::Empty }
}

impl<T: Clone + Send + Sync + Unpin + 'static> EPathMapRepr<T> {
    #[inline]
    pub fn mode(&self) -> EPathMapMode {
        match self {
            Self::Empty => EPathMapMode::Empty,
            Self::Set(_) => EPathMapMode::Set,
            Self::Map(_) => EPathMapMode::Map,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Set(map) => map.is_empty(),
            Self::Map(map) => map.is_empty(),
        }
    }

    #[inline]
    pub fn as_set(&self) -> Option<&PathMap<()>> {
        match self {
            Self::Set(map) => Some(map),
            Self::Empty | Self::Map(_) => None,
        }
    }

    #[inline]
    pub fn as_map(&self) -> Option<&PathMap<T>> {
        match self {
            Self::Map(map) => Some(map),
            Self::Empty | Self::Set(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EPathMapMode {
    Empty = 0,
    Set = 1,
    Map = 2,
}

impl TryFrom<u8> for EPathMapMode {
    type Error = TrieCodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Empty),
            1 => Ok(Self::Set),
            2 => Ok(Self::Map),
            other => Err(TrieCodecError::new(format!(
                "unknown EPathMap mode {other}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrieCodecError {
    message: String,
}

impl TrieCodecError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TrieCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for TrieCodecError {}

#[allow(deprecated)]
impl From<TrieCodecError> for prost::DecodeError {
    fn from(error: TrieCodecError) -> Self {
        prost::DecodeError::new(format!("EPathMap trie_snapshot: {error}"))
    }
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
        suffix: Vec<u8>,
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

/// The PathMap-native, value-independent prefix of an `EPM1` snapshot.
///
/// It contains the mode, compact ACTree03 arena, and value count. Map values
/// follow this prefix as `varint(protobuf_len) || protobuf_body` records. Keeping
/// only this topology in the shared cache is important: a nested map chain has
/// linear total topology, whereas caching the complete snapshot at every nested
/// node would retain every suffix and therefore quadratic bytes.
pub(crate) struct EpmLayout {
    prefix: Vec<u8>,
    value_count: usize,
}

impl EpmLayout {
    #[inline]
    pub(crate) fn prefix(&self) -> &[u8] { &self.prefix }

    #[inline]
    pub(crate) fn value_count(&self) -> usize { self.value_count }
}

/// Build the final-byte topology directly from PathMap. No `Par` projection or
/// encoded-value table is retained: the only temporary index maps the stable
/// addresses supplied by PathMap to ACTree03 value ordinals.
pub(crate) fn layout(repr: &EPathMapRepr<Par>) -> EpmLayout {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(EPM_MAGIC);
    prefix.push(EPM_VERSION);
    prefix.push(repr.mode() as u8);

    let value_count = match repr {
        EPathMapRepr::Empty => {
            push_varint(&mut prefix, 0);
            push_varint(&mut prefix, 0);
            0
        }
        EPathMapRepr::Set(map) => {
            let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0);
            let bytes = arena.get_data();
            push_varint(&mut prefix, bytes.len() as u64);
            prefix.extend_from_slice(bytes);
            push_varint(&mut prefix, 0);
            0
        }
        EPathMapRepr::Map(map) => {
            let mut ordinal_by_address = HashMap::new();
            let mut zipper = map.read_zipper();
            let mut value_count = 0usize;
            while let Some(value) = zipper.to_next_get_val() {
                ordinal_by_address.insert(value as *const Par as usize, value_count as u64);
                value_count += 1;
            }
            let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |value| {
                ordinal_by_address[&(value as *const Par as usize)]
            });
            let bytes = arena.get_data();
            push_varint(&mut prefix, bytes.len() as u64);
            prefix.extend_from_slice(bytes);
            push_varint(&mut prefix, value_count as u64);
            value_count
        }
    };

    EpmLayout {
        prefix,
        value_count,
    }
}

/// Encode a specialized EPathMap representation directly as one `EPM1` byte
/// string. The compact arena is produced by PathMap's own jumping catamorphism;
/// values never leave the generated iterative protobuf codec.
pub fn encode(repr: &EPathMapRepr<Par>) -> Vec<u8> {
    let layout = layout(repr);
    encode_with_layout(repr, &layout)
}

pub(crate) fn encode_with_layout(repr: &EPathMapRepr<Par>, layout: &EpmLayout) -> Vec<u8> {
    let mut out = Vec::from(layout.prefix());
    if let EPathMapRepr::Map(map) = repr {
        let mut zipper = map.read_zipper();
        let mut emitted = 0usize;
        while let Some(value) = zipper.to_next_get_val() {
            protobuf_encoder::encode_length_delimited_body_into(value, &mut out);
            emitted += 1;
        }
        debug_assert_eq!(emitted, layout.value_count());
    }
    out
}

/// Pausable EPM1 reader used by the generated protobuf decoder. The snapshot
/// bytes remain shared, and every map value is handed to the same explicit PDA
/// as an independent protobuf body.
pub(crate) struct PendingEpmDecode<S: AsRef<[u8]>> {
    snapshot: S,
    mode: EPathMapMode,
    arena: Vec<u8>,
    next_value_at: usize,
    remaining_values: usize,
    expected_values: usize,
    values: Vec<Option<Par>>,
}

pub(crate) type OwnedPendingEpmDecode = PendingEpmDecode<Bytes>;

impl<S: AsRef<[u8]>> PendingEpmDecode<S> {
    pub(crate) fn new(snapshot: S) -> Result<Self, TrieCodecError> {
        let bytes = snapshot.as_ref();
        let mut cursor = 0usize;
        let magic = take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")?;
        if magic != EPM_MAGIC {
            return Err(TrieCodecError::new("invalid EPM1 magic"));
        }
        let version = *take(bytes, &mut cursor, 1, "EPM1 version")?
            .first()
            .expect("a one-byte slice has a first byte");
        if version != EPM_VERSION {
            return Err(TrieCodecError::new(format!(
                "unsupported EPM1 version {version}"
            )));
        }
        let mode = EPathMapMode::try_from(
            *take(bytes, &mut cursor, 1, "EPM1 mode")?
                .first()
                .expect("a one-byte slice has a first byte"),
        )?;
        let arena_len = read_len(bytes, &mut cursor, "ACTree03 length")?;
        let arena = take(bytes, &mut cursor, arena_len, "ACTree03 payload")?.to_vec();
        let value_count = read_len(bytes, &mut cursor, "map value count")?;
        let remaining = bytes.len() - cursor;
        if value_count > remaining {
            return Err(TrieCodecError::new(format!(
                "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
            )));
        }

        let next_value_at = cursor;
        for _ in 0..value_count {
            let len = read_len(bytes, &mut cursor, "map value length")?;
            take(bytes, &mut cursor, len, "map value payload")?;
        }
        if cursor != bytes.len() {
            return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
        }

        Ok(Self {
            snapshot,
            mode,
            arena,
            next_value_at,
            remaining_values: value_count,
            expected_values: value_count,
            values: Vec::with_capacity(value_count),
        })
    }

    fn next_value_range(&mut self) -> Result<Option<Range<usize>>, TrieCodecError> {
        if self.remaining_values == 0 {
            return Ok(None);
        }
        let bytes = self.snapshot.as_ref();
        let len = read_len(bytes, &mut self.next_value_at, "map value length")?;
        let start = self.next_value_at;
        take(bytes, &mut self.next_value_at, len, "map value payload")?;
        self.remaining_values -= 1;
        Ok(Some(start..start + len))
    }

    pub(crate) fn push_value(&mut self, value: Par) -> Result<(), TrieCodecError> {
        if self.values.len() == self.expected_values {
            return Err(TrieCodecError::new(
                "more map values were decoded than the EPM1 value count",
            ));
        }
        self.values.push(Some(value));
        Ok(())
    }

    pub(crate) fn finish(
        self,
        validate_canonical: bool,
    ) -> Result<EPathMapRepr<Par>, TrieCodecError> {
        if self.remaining_values != 0 || self.values.len() != self.expected_values {
            return Err(TrieCodecError::new(
                "fewer map values were decoded than the EPM1 value count",
            ));
        }
        let repr = build_repr(self.mode, &self.arena, self.values)?;
        if validate_canonical {
            let canonical_layout = layout(&repr);
            if encode_with_layout(&repr, &canonical_layout) != self.snapshot.as_ref() {
                return Err(TrieCodecError::new("EPM1 payload is not canonical"));
            }
        }
        Ok(repr)
    }
}

impl PendingEpmDecode<Bytes> {
    pub(crate) fn next_value_bytes(&mut self) -> Result<Option<Bytes>, TrieCodecError> {
        let Some(range) = self.next_value_range()? else {
            return Ok(None);
        };
        Ok(Some(self.snapshot.slice(range)))
    }
}

fn build_repr(
    mode: EPathMapMode,
    arena: &[u8],
    mut values: Vec<Option<Par>>,
) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    match mode {
        EPathMapMode::Empty => {
            if !arena.is_empty() || !values.is_empty() {
                return Err(TrieCodecError::new(
                    "neutral empty EPathMap carries trie or values",
                ));
            }
            Ok(EPathMapRepr::Empty)
        }
        EPathMapMode::Set => {
            if !values.is_empty() {
                return Err(TrieCodecError::new("set EPathMap carries map values"));
            }
            let mut map = PathMap::<()>::new();
            visit_act(arena, |path, ordinal| {
                if !path.is_empty() {
                    map.create_path(&path);
                }
                if let Some(ordinal) = ordinal {
                    if ordinal != 0 {
                        return Err(TrieCodecError::new("set ACTree value ordinal is not zero"));
                    }
                    map.insert(path, ());
                }
                Ok(())
            })?;
            if map.is_empty() {
                return Err(TrieCodecError::new(
                    "typed set mode is empty; use neutral empty mode",
                ));
            }
            Ok(EPathMapRepr::Set(map))
        }
        EPathMapMode::Map => {
            let mut map = PathMap::<Par>::new();
            visit_act(arena, |path, ordinal| {
                if !path.is_empty() {
                    map.create_path(&path);
                }
                let Some(ordinal) = ordinal else {
                    return Ok(());
                };
                let index = usize::try_from(ordinal)
                    .map_err(|_| TrieCodecError::new("map value ordinal overflows usize"))?;
                let slot = values.get_mut(index).ok_or_else(|| {
                    TrieCodecError::new(format!("map value ordinal {index} is out of range"))
                })?;
                let value = slot.take().ok_or_else(|| {
                    TrieCodecError::new(format!("map value ordinal {index} is reused"))
                })?;
                map.insert(path, value);
                Ok(())
            })?;
            if map.is_empty() {
                return Err(TrieCodecError::new(
                    "typed map mode is empty; use neutral empty mode",
                ));
            }
            if let Some(missing) = values.iter().position(Option::is_some) {
                return Err(TrieCodecError::new(format!(
                    "map value ordinal {missing} is unreferenced"
                )));
            }
            Ok(EPathMapRepr::Map(map))
        }
    }
}

/// Decode and validate `EPM1`, then reconstruct PathMap directly through its
/// insertion API. A canonicality fence rejects alternate encodings of the same
/// trie, malformed ACT offsets, duplicate/missing value ordinals, and trailing
/// bytes.
pub fn decode(bytes: &[u8]) -> Result<EPathMapRepr<Par>, TrieCodecError> {
    let mut cursor = 0usize;
    let magic = take(bytes, &mut cursor, EPM_MAGIC.len(), "EPM1 magic")?;
    if magic != EPM_MAGIC {
        return Err(TrieCodecError::new("invalid EPM1 magic"));
    }
    let version = *take(bytes, &mut cursor, 1, "EPM1 version")?
        .first()
        .expect("a one-byte slice has a first byte");
    if version != EPM_VERSION {
        return Err(TrieCodecError::new(format!(
            "unsupported EPM1 version {version}"
        )));
    }
    let mode = EPathMapMode::try_from(
        *take(bytes, &mut cursor, 1, "EPM1 mode")?
            .first()
            .expect("a one-byte slice has a first byte"),
    )?;
    let arena_len = read_len(bytes, &mut cursor, "ACTree03 length")?;
    let arena = take(bytes, &mut cursor, arena_len, "ACTree03 payload")?;
    let value_count = read_len(bytes, &mut cursor, "map value count")?;

    // Every value, including an empty protobuf `Par`, needs at least one byte
    // for its length varint. Prove that lower bound against the actual input
    // before reserving `Option<Par>` slots. This is not an artificial traversal
    // ceiling: it is the information-theoretic bound `value_count <= remaining
    // bytes`, and therefore accepts every physically representable EPM1 value
    // table. Without it, one mutated count varint can ask the allocator for
    // exabytes before the first missing length prefix is observed.
    let remaining = bytes.len() - cursor;
    if value_count > remaining {
        return Err(TrieCodecError::new(format!(
            "map value count {value_count} exceeds the {remaining}-byte payload lower bound"
        )));
    }

    let mut values = Vec::with_capacity(value_count);
    for ordinal in 0..value_count {
        let len = read_len(bytes, &mut cursor, "map value length")?;
        let payload = take(bytes, &mut cursor, len, "map value payload")?;
        let value = protobuf_decoder::decode_par(payload).map_err(|error| {
            TrieCodecError::new(format!("map value {ordinal} is not a Par: {error}"))
        })?;
        values.push(Some(value));
    }
    if cursor != bytes.len() {
        return Err(TrieCodecError::new("trailing bytes after EPM1 payload"));
    }

    let repr = build_repr(mode, arena, values)?;

    let canonical_layout = layout(&repr);
    if encode_with_layout(&repr, &canonical_layout) != bytes {
        return Err(TrieCodecError::new("EPM1 payload is not canonical"));
    }
    Ok(repr)
}

fn visit_act(
    bytes: &[u8],
    mut visit: impl FnMut(Vec<u8>, Option<u64>) -> Result<(), TrieCodecError>,
) -> Result<(), TrieCodecError> {
    if bytes.len() < ACT_HEADER_LEN {
        return Err(TrieCodecError::new(
            "ACTree03 payload is shorter than its header",
        ));
    }
    if bytes[..COMPACT_TREE_MAGIC.len()] != COMPACT_TREE_MAGIC {
        return Err(TrieCodecError::new("invalid ACTree03 magic"));
    }
    let root_id = u64::from_le_bytes(
        bytes[8..16]
            .try_into()
            .expect("the ACT header root slot is eight bytes"),
    );
    let root_id = usize::try_from(root_id)
        .map_err(|_| TrieCodecError::new("ACTree03 root offset overflows usize"))?;
    if !(ACT_HEADER_LEN..bytes.len()).contains(&root_id) {
        return Err(TrieCodecError::new(
            "ACTree03 root offset is outside the arena",
        ));
    }

    let root = parse_act_node(bytes, root_id)?;
    let mut path = Vec::new();
    // ACTree03 is a tree laid out from children toward parents: every child
    // offset must therefore move strictly backward in the arena, and a node
    // can have exactly one parent. These two structural invariants are checked
    // before each descent. Besides validating the format, they prove that the
    // loop visits at most `arena.len()` nodes even for arbitrary input; a
    // malformed sibling offset cannot re-enter an ancestor or alias a subtree.
    let mut seen_nodes = HashSet::with_capacity(bytes.len().min(1024));
    seen_nodes.insert(root_id);
    let mut stack = vec![ActFrame {
        node_id: root_id,
        next_child_id: first_child_id(&root),
        node: root,
        path_len: 0,
        next_child: 0,
        entered: false,
    }];

    while let Some(frame) = stack.last_mut() {
        let child_count = match &frame.node.edge {
            ActEdge::Branch { child_bytes, .. } => child_bytes.len(),
            ActEdge::Line { child, .. } => usize::from(child.is_some()),
        };
        if !frame.entered {
            frame.entered = true;
            if let ActEdge::Line { suffix, .. } = &frame.node.edge {
                path.extend_from_slice(suffix);
            }
            // ACTree nodes without values are often only the compact trie's
            // branching structure. Reifying every such node with create_path
            // changes the trie. A value position is semantically present, and
            // a childless node is the endpoint of an explicit dangling path;
            // those are the only endpoints required to reconstruct the same
            // PathMap topology.
            if frame.node.value.is_some() || child_count == 0 {
                visit(path.clone(), frame.node.value)?;
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
            .ok_or_else(|| TrieCodecError::new("ACTree03 child list ended early"))?;
        if child_id >= frame.node_id {
            return Err(TrieCodecError::new(
                "ACTree03 child does not precede its parent",
            ));
        }
        if !seen_nodes.insert(child_id) {
            return Err(TrieCodecError::new(
                "ACTree03 node is referenced by more than one edge",
            ));
        }
        let edge_byte = match &frame.node.edge {
            ActEdge::Branch { child_bytes, .. } => Some(child_bytes[frame.next_child]),
            ActEdge::Line { .. } => None,
        };
        let child = parse_act_node(bytes, child_id)?;
        let next_id = child_id
            .checked_add(child.encoded_len)
            .ok_or_else(|| TrieCodecError::new("ACTree03 sibling offset overflow"))?;
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

fn first_child_id(node: &ActNode) -> Option<usize> {
    match &node.edge {
        ActEdge::Branch { first_child, .. } => *first_child,
        ActEdge::Line { child, .. } => *child,
    }
}

fn parse_act_node(bytes: &[u8], node_id: usize) -> Result<ActNode, TrieCodecError> {
    let head = *bytes
        .get(node_id)
        .ok_or_else(|| TrieCodecError::new("ACTree03 node offset is outside the arena"))?;
    let mut cursor = node_id + 1;
    let value = if head & ACT_VALUE_FLAG != 0 {
        Some(read_act_varint(bytes, &mut cursor, "ACTree03 node value")?)
    } else {
        None
    };

    let edge = if head & ACT_LINE_FLAG == 0 {
        let encoded_count = (head & 0x3f) as usize;
        if encoded_count > 32 {
            return Err(TrieCodecError::new(
                "ACTree03 branch child count exceeds 32 marker",
            ));
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
                return Err(TrieCodecError::new(
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
                return Err(TrieCodecError::new(
                    "ACTree03 dense child mask contains fewer than 32 branches",
                ));
            }
            branches
        };
        if child_bytes.is_empty() != first_child.is_none() {
            return Err(TrieCodecError::new(
                "ACTree03 branch child metadata disagrees",
            ));
        }
        ActEdge::Branch {
            first_child,
            child_bytes,
        }
    } else {
        if head & 0x3e != 0 {
            return Err(TrieCodecError::new(
                "ACTree03 line node has reserved header bits set",
            ));
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
            .map_err(|_| TrieCodecError::new("ACTree03 line length overflows usize"))?;
        if line_len == 0 {
            return Err(TrieCodecError::new("ACTree03 line suffix is empty"));
        }
        let suffix = take(bytes, &mut line_cursor, line_len, "ACTree03 line suffix")?.to_vec();
        if line_cursor > node_id {
            return Err(TrieCodecError::new("ACTree03 line data overlaps its node"));
        }
        ActEdge::Line { child, suffix }
    };

    Ok(ActNode {
        encoded_len: cursor - node_id,
        value,
        edge,
    })
}

fn relative_target(node_id: usize, offset: u64, label: &str) -> Result<usize, TrieCodecError> {
    let offset = usize::try_from(offset)
        .map_err(|_| TrieCodecError::new(format!("{label} overflows usize")))?;
    if offset == 0 {
        return Err(TrieCodecError::new(format!("{label} is zero")));
    }
    let target = node_id
        .checked_sub(offset)
        .ok_or_else(|| TrieCodecError::new(format!("{label} points before the arena")))?;
    if target < ACT_HEADER_LEN {
        return Err(TrieCodecError::new(format!(
            "{label} points into the header"
        )));
    }
    Ok(target)
}

fn read_act_varint(bytes: &[u8], cursor: &mut usize, label: &str) -> Result<u64, TrieCodecError> {
    let first = *bytes
        .get(*cursor)
        .ok_or_else(|| TrieCodecError::new(format!("truncated {label}")))?;
    *cursor += 1;
    if first <= ACT_VARINT_BIAS {
        return Ok(first as u64);
    }
    let len = (first - ACT_VARINT_BIAS) as usize;
    if !(1..=8).contains(&len) {
        return Err(TrieCodecError::new(format!("invalid {label} length")));
    }
    let raw = take(bytes, cursor, len, label)?;
    if raw.last() == Some(&0) {
        return Err(TrieCodecError::new(format!("non-canonical {label}")));
    }
    let mut full = [0u8; 8];
    full[..len].copy_from_slice(raw);
    let value = u64::from_le_bytes(full);
    if value <= ACT_VARINT_BIAS as u64 {
        return Err(TrieCodecError::new(format!("non-canonical {label}")));
    }
    Ok(value)
}

fn push_varint(out: &mut Vec<u8>, value: u64) { prost::encoding::encode_varint(value, out); }

fn read_len(bytes: &[u8], cursor: &mut usize, label: &str) -> Result<usize, TrieCodecError> {
    let value = read_protobuf_varint(bytes, cursor, label)?;
    usize::try_from(value).map_err(|_| TrieCodecError::new(format!("{label} overflows usize")))
}

fn read_protobuf_varint(
    bytes: &[u8],
    cursor: &mut usize,
    label: &str,
) -> Result<u64, TrieCodecError> {
    let start = *cursor;
    let mut value = 0u64;
    for index in 0..10usize {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| TrieCodecError::new(format!("truncated {label}")))?;
        *cursor += 1;
        if index == 9 && byte > 1 {
            return Err(TrieCodecError::new(format!("overflowing {label}")));
        }
        value |= ((byte & 0x7f) as u64) << (index * 7);
        if byte & 0x80 == 0 {
            let canonical_len = prost::encoding::encoded_len_varint(value);
            if *cursor - start != canonical_len {
                return Err(TrieCodecError::new(format!("non-canonical {label}")));
            }
            return Ok(value);
        }
    }
    Err(TrieCodecError::new(format!("unterminated {label}")))
}

fn take<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
    label: &str,
) -> Result<&'a [u8], TrieCodecError> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| TrieCodecError::new(format!("{label} length overflow")))?;
    let region = bytes
        .get(*cursor..end)
        .ok_or_else(|| TrieCodecError::new(format!("truncated {label}")))?;
    *cursor = end;
    Ok(region)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{Expr, Par};

    fn int(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    fn paths<V: Clone + Send + Sync + Unpin + 'static>(map: &PathMap<V>) -> Vec<Vec<u8>> {
        let mut zipper = map.read_zipper();
        let mut paths = Vec::new();
        while zipper.to_next_val() {
            paths.push(zipper.path().to_vec());
        }
        paths
    }

    #[test]
    fn neutral_empty_round_trips_without_choosing_a_mode() {
        let bytes = encode(&EPathMapRepr::Empty);
        let decoded = decode(&bytes).expect("neutral EPM1 payload decodes");
        assert_eq!(decoded.mode(), EPathMapMode::Empty);
        assert!(decoded.is_empty());
    }

    #[test]
    fn impossible_value_count_is_rejected_before_allocation() {
        // Empty arena, map mode, then a canonical value count of 128 with no
        // bytes left. The decoder must reject from the input-derived lower
        // bound; attempting `Vec::with_capacity(128)` would be harmless here,
        // but the same path with a larger canonical varint is the malformed
        // bincode mutation that previously exhausted the host.
        let bytes = [b'E', b'P', b'M', b'1', EPM_VERSION, 2, 0, 0x80, 0x01];
        let error = match decode(&bytes) {
            Err(error) => error,
            Ok(_) => panic!("the impossible value count must be rejected"),
        };
        assert_eq!(
            error.to_string(),
            "map value count 128 exceeds the 0-byte payload lower bound"
        );
    }

    #[test]
    fn sibling_offset_cannot_reenter_its_parent() {
        // A two-child root whose first child occupies bytes 16..18. Advancing
        // to the alleged second sibling lands exactly on the root at byte 18.
        // Without the strict child-before-parent check, the walker repeatedly
        // re-entered that root instead of making progress through the arena.
        let mut arena = vec![0u8; 22];
        arena[..COMPACT_TREE_MAGIC.len()].copy_from_slice(&COMPACT_TREE_MAGIC);
        arena[8..16].copy_from_slice(&18u64.to_le_bytes());
        arena[16] = ACT_VALUE_FLAG;
        arena[17] = 0;
        arena[18] = 2;
        arena[19] = 2;
        arena[20] = 0;
        arena[21] = 1;

        let error = visit_act(&arena, |_path, _value| Ok(())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ACTree03 child does not precede its parent"
        );
    }

    #[test]
    fn set_round_trip_preserves_prefix_compressed_keys() {
        let mut map = PathMap::new();
        map.insert(b"prefix/a", ());
        map.insert(b"prefix/b", ());
        map.insert(b"prefix/deep/c", ());
        let expected = paths(&map);
        let bytes = encode(&EPathMapRepr::Set(map));
        let decoded = decode(&bytes).expect("set EPM1 payload decodes");
        let EPathMapRepr::Set(map) = decoded else {
            panic!("set EPM1 payload changed mode");
        };
        assert_eq!(paths(&map), expected);
        assert!(bytes
            .windows(COMPACT_TREE_MAGIC.len())
            .any(|w| w == COMPACT_TREE_MAGIC));
    }

    #[test]
    fn map_round_trip_uses_generated_stack_safe_value_codecs() {
        let mut map = PathMap::new();
        map.insert(b"same/prefix/a", int(1));
        map.insert(b"same/prefix/b", int(2));
        let bytes = encode(&EPathMapRepr::Map(map));
        let decoded = decode(&bytes).expect("map EPM1 payload decodes");
        let EPathMapRepr::Map(map) = decoded else {
            panic!("map EPM1 payload changed mode");
        };
        assert_eq!(
            protobuf_encoder::encode_to_vec(map.get(b"same/prefix/a").expect("a exists")),
            protobuf_encoder::encode_to_vec(&int(1))
        );
        assert_eq!(
            protobuf_encoder::encode_to_vec(map.get(b"same/prefix/b").expect("b exists")),
            protobuf_encoder::encode_to_vec(&int(2))
        );
    }

    #[test]
    fn decoder_rejects_noncanonical_and_malformed_snapshots() {
        let canonical = encode(&EPathMapRepr::Empty);
        let mut trailing = canonical.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());

        let mut typed_empty = canonical;
        typed_empty[5] = EPathMapMode::Set as u8;
        assert!(decode(&typed_empty).is_err());
    }
}
