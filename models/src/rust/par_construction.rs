//! Schema-driven transient construction of authoritative [`Par`] values.
//!
//! RPB3 stores a complete canonical score and protobuf body in each top-level
//! component path.  That is ideal for lookup, ordering, hashing, and direct
//! serialization, but eagerly constructing every ancestor of a recursive term
//! copies the already-complete child body once per level.  [`ParConstruction`]
//! is the complementary construction representation: an arena of flat indices
//! and immutable byte fragments.  It is dismantled when [`finish`](ParConstruction::finish)
//! emits the root component paths once; it never becomes resident state.
//!
//! The protobuf half is driven by the generated [`ProtobufNode`] programs. A
//! caller supplies a canonical component shell containing one empty `Par`
//! placeholder per recursive slot and the corresponding planned children in
//! generated wire order.  The planner records bounded wire fragments and child
//! obligations without serializing a child.  Lengths are computed bottom-up,
//! and final emission is a heap worklist. EPathMap keeps canonical keys in its
//! homogeneous PathMap specialization and streams the PathMap-produced EPM1
//! topology plus planned map values; it is never projected into decoded Pars.
//!
//! Canonical score keys use the same prefix-free byte language as the sorter,
//! represented as another flat arena.  Parent scores reference child score
//! nodes by index; no recursive `Tree` is cloned while a construction grows.

use std::cmp::Ordering;
use std::fmt;

use pathmap::zipper::{ZipperIteration, ZipperMoving, ZipperReadOnlyIteration};
use pathmap::PathMap;
use prost::encoding::{encoded_len_varint, key_len};

use super::canonical_path::tag as path_tag;
use super::epathmap_trie_codec::{self, EPathMapMode, EPathMapRepr};
use super::par_bag::{Multiplicity, ParBag, ParBagComponent, ParComponentTag, PAR_BAG_KEY_DOMAIN};
use super::rholang::protobuf_encoder;
use super::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode, ProtobufSeq, NO_RESUME};
use super::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use super::rholang::sorter::score_tree::{encode_order_key, Score, ScoreAtom, Tree};
use super::rholang::sorter::sort_combine::{sort_unforgeable, sort_var};
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::connective::ConnectiveInstance;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::{
    Bundle, Connective, EPathMap, EZipper, Expr, GUnforgeable, If, Match, New, Par, Receive, Send,
};

/// An index into a [`ParConstruction`]'s planned-Par arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedPar(usize);

/// An index into the canonical-score fragment arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedScore(usize);

/// An index into the planned-component arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedComponent(usize);

/// An index into a transient homogeneous EPathMap arena. Keys remain in their
/// prefix-compressed PathMap and map values are lightweight planned-Par
/// handles; no decoded entry projection becomes resident state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedEPathMap(usize);

/// A construction failure. Every variant denotes an addressability or semantic
/// invariant failure; none is a policy depth/arity limit.
#[derive(Debug)]
pub enum ParConstructionError {
    LengthOverflow(&'static str),
    MissingParSlot {
        expected_at_least: usize,
        supplied: usize,
    },
    ExtraParSlots {
        consumed: usize,
        supplied: usize,
    },
    ChildArity {
        context: &'static str,
        expected: usize,
        supplied: usize,
    },
    Multiplicity(super::par_bag::BagError),
}

impl fmt::Display for ParConstructionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow(what) => write!(formatter, "{what} exceeds addressable memory"),
            Self::MissingParSlot {
                expected_at_least,
                supplied,
            } => write!(
                formatter,
                "component schema visited at least {expected_at_least} Par slots but only {supplied} were supplied"
            ),
            Self::ExtraParSlots { consumed, supplied } => write!(
                formatter,
                "component schema consumed {consumed} Par slots but {supplied} were supplied"
            ),
            Self::ChildArity {
                context,
                expected,
                supplied,
            } => write!(
                formatter,
                "{context} requires {expected} planned Par children but received {supplied}"
            ),
            Self::Multiplicity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParConstructionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Multiplicity(error) => Some(error),
            _ => None,
        }
    }
}

impl From<super::par_bag::BagError> for ParConstructionError {
    fn from(error: super::par_bag::BagError) -> Self { Self::Multiplicity(error) }
}

#[derive(Clone, Copy)]
enum ScorePiece {
    Bytes(usize),
    Child(PlannedScore),
    Repeat { child: PlannedScore, count: u64 },
    EscapedWire { root: WireRoot, encoded_len: usize },
}

enum ScoreCursorOp<'a> {
    Score(PlannedScore),
    Bytes { fragment: usize, offset: usize },
    Repeat { child: PlannedScore, remaining: u64 },
    EscapedWire(WireByteCursor<'a>),
    Byte(u8),
}

/// A zero-copy cursor over the prefix-free canonical score language.
///
/// Canonicalization compares these cursors directly.  It does not flatten a
/// planned score graph, even when that graph represents a deep recursive term
/// or a high multiplicity.
struct ScoreByteCursor<'a> {
    arena: &'a ParConstruction,
    work: Vec<ScoreCursorOp<'a>>,
}

impl<'a> ScoreByteCursor<'a> {
    fn new(arena: &'a ParConstruction, root: PlannedScore) -> Self {
        Self {
            arena,
            work: vec![ScoreCursorOp::Score(root)],
        }
    }
}

impl Iterator for ScoreByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.work.pop()? {
                ScoreCursorOp::Score(score) => {
                    for piece in self.arena.scores[score.0].pieces.iter().rev() {
                        self.work.push(match *piece {
                            ScorePiece::Bytes(fragment) => ScoreCursorOp::Bytes {
                                fragment,
                                offset: 0,
                            },
                            ScorePiece::Child(child) => ScoreCursorOp::Score(child),
                            ScorePiece::Repeat { child, count } => ScoreCursorOp::Repeat {
                                child,
                                remaining: count,
                            },
                            ScorePiece::EscapedWire { root, .. } => {
                                ScoreCursorOp::EscapedWire(WireByteCursor::new(self.arena, root))
                            }
                        });
                    }
                }
                ScoreCursorOp::Bytes { fragment, offset } => {
                    let bytes = &self.arena.byte_fragments[fragment];
                    let byte = *bytes.get(offset)?;
                    if offset + 1 < bytes.len() {
                        self.work.push(ScoreCursorOp::Bytes {
                            fragment,
                            offset: offset + 1,
                        });
                    }
                    return Some(byte);
                }
                ScoreCursorOp::Repeat { child, remaining } => {
                    if remaining != 0 {
                        if remaining != 1 {
                            self.work.push(ScoreCursorOp::Repeat {
                                child,
                                remaining: remaining - 1,
                            });
                        }
                        self.work.push(ScoreCursorOp::Score(child));
                    }
                }
                ScoreCursorOp::EscapedWire(mut cursor) => {
                    let byte = cursor.next()?;
                    self.work.push(ScoreCursorOp::EscapedWire(cursor));
                    if byte == 0 {
                        self.work.push(ScoreCursorOp::Byte(0xff));
                    }
                    return Some(byte);
                }
                ScoreCursorOp::Byte(byte) => return Some(byte),
            }
        }
    }
}

struct ScoreNode {
    pieces: Vec<ScorePiece>,
    encoded_len: usize,
}

enum ProtoPiece {
    Bytes(Box<[u8]>),
    Node { tag: u32, node: usize },
    Par { tag: u32, par: PlannedPar },
    EPathMapSnapshot { tag: u32, map: PlannedEPathMap },
}

struct ProtoNode {
    pieces: Vec<ProtoPiece>,
    encoded_len: usize,
    zero_count: usize,
}

/// One operation in the generated-schema construction walk.
///
/// This is module-level solely so [`ParConstruction::plan_child`] can append a
/// child continuation without resorting to a recursive callback or an erased
/// adapter.  Every borrow is of the caller's short-lived component shell; no
/// reference survives [`ParConstruction::component`].
enum ProtoBuildOp<'a> {
    Node {
        node: &'a dyn ProtobufNode,
        from: u16,
        target: usize,
    },
    Seq {
        seq: &'a dyn ProtobufSeq,
        index: usize,
        len: usize,
        tag: u32,
        target: usize,
    },
}

#[derive(Clone, Copy)]
enum WireRoot {
    Par(PlannedPar),
    Proto(usize),
    EPathMapSnapshot(PlannedEPathMap),
}

#[derive(Clone, Copy)]
struct InlineWireBytes {
    bytes: [u8; 20],
    len: u8,
    offset: u8,
}

impl InlineWireBytes {
    fn protobuf_varints(values: &[u64]) -> Self {
        let mut inline = Self {
            bytes: [0; 20],
            len: 0,
            offset: 0,
        };
        for &value in values {
            let mut value = value;
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                inline.bytes[usize::from(inline.len)] = byte;
                inline.len += 1;
                if value == 0 {
                    break;
                }
            }
        }
        inline
    }

    fn length_delimited_prefix(tag: u32, len: usize) -> Self {
        Self::protobuf_varints(&[(u64::from(tag) << 3) | 2, len as u64])
    }

    fn zero_count(&self) -> usize {
        self.bytes[..usize::from(self.len)]
            .iter()
            .filter(|&&byte| byte == 0)
            .count()
    }
}

enum WireCursorOp<'a> {
    Node {
        node: usize,
        index: usize,
    },
    Nested {
        tag: u32,
        node: usize,
    },
    ChildPar {
        tag: u32,
        par: PlannedPar,
    },
    NestedEPathMapSnapshot {
        tag: u32,
        map: PlannedEPathMap,
    },
    Par(PlannedPar),
    Components {
        occurrences: &'a [Occurrence],
        index: usize,
        remaining: u64,
        after_metadata: bool,
    },
    RootMetadata(PlannedPar),
    EPathMapSnapshot {
        map: PlannedEPathMap,
        prefix_emitted: bool,
        value_index: usize,
    },
    Bytes {
        bytes: &'a [u8],
        offset: usize,
    },
    Inline(InlineWireBytes),
}

/// A byte cursor over a planned protobuf graph.
///
/// The same cursor drives final emission and canonical tie-breaking. It is a
/// PDA over integer arena edges and borrowed byte fragments; neither operation
/// materializes a nested message or calls itself recursively.
struct WireByteCursor<'a> {
    arena: &'a ParConstruction,
    work: Vec<WireCursorOp<'a>>,
}

impl<'a> WireByteCursor<'a> {
    fn new(arena: &'a ParConstruction, root: WireRoot) -> Self {
        let root = match root {
            WireRoot::Par(par) => WireCursorOp::Par(par),
            WireRoot::Proto(node) => WireCursorOp::Node { node, index: 0 },
            WireRoot::EPathMapSnapshot(map) => WireCursorOp::EPathMapSnapshot {
                map,
                prefix_emitted: false,
                value_index: 0,
            },
        };
        Self {
            arena,
            work: vec![root],
        }
    }
}

impl Iterator for WireByteCursor<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        fn is_after_metadata(tag: ParComponentTag) -> bool {
            matches!(tag, ParComponentTag::Bundle | ParComponentTag::If)
        }

        loop {
            match self.work.pop()? {
                WireCursorOp::Node { node, index } => {
                    if let Some(piece) = self.arena.protobuf_nodes[node].pieces.get(index) {
                        self.work.push(WireCursorOp::Node {
                            node,
                            index: index + 1,
                        });
                        self.work.push(match piece {
                            ProtoPiece::Bytes(bytes) => WireCursorOp::Bytes { bytes, offset: 0 },
                            ProtoPiece::Node { tag, node } => WireCursorOp::Nested {
                                tag: *tag,
                                node: *node,
                            },
                            ProtoPiece::Par { tag, par } => WireCursorOp::ChildPar {
                                tag: *tag,
                                par: *par,
                            },
                            ProtoPiece::EPathMapSnapshot { tag, map } => {
                                WireCursorOp::NestedEPathMapSnapshot {
                                    tag: *tag,
                                    map: *map,
                                }
                            }
                        });
                    }
                }
                WireCursorOp::Nested { tag, node } => {
                    self.work.push(WireCursorOp::Node { node, index: 0 });
                    self.work.push(WireCursorOp::Inline(
                        InlineWireBytes::length_delimited_prefix(
                            tag,
                            self.arena.protobuf_nodes[node].encoded_len,
                        ),
                    ));
                }
                WireCursorOp::ChildPar { tag, par } => {
                    self.work.push(WireCursorOp::Par(par));
                    self.work.push(WireCursorOp::Inline(
                        InlineWireBytes::length_delimited_prefix(
                            tag,
                            self.arena.pars[par.0].protobuf_len,
                        ),
                    ));
                }
                WireCursorOp::NestedEPathMapSnapshot { tag, map } => {
                    self.work.push(WireCursorOp::EPathMapSnapshot {
                        map,
                        prefix_emitted: false,
                        value_index: 0,
                    });
                    self.work.push(WireCursorOp::Inline(
                        InlineWireBytes::length_delimited_prefix(
                            tag,
                            self.arena.epathmaps[map.0].snapshot_len,
                        ),
                    ));
                }
                WireCursorOp::Par(par) => {
                    let node = &self.arena.pars[par.0];
                    self.work.push(WireCursorOp::Components {
                        occurrences: &node.occurrences,
                        index: 0,
                        remaining: 0,
                        after_metadata: true,
                    });
                    self.work.push(WireCursorOp::RootMetadata(par));
                    self.work.push(WireCursorOp::Components {
                        occurrences: &node.occurrences,
                        index: 0,
                        remaining: 0,
                        after_metadata: false,
                    });
                }
                WireCursorOp::Components {
                    occurrences,
                    mut index,
                    mut remaining,
                    after_metadata,
                } => {
                    while remaining == 0 && index < occurrences.len() {
                        let occurrence = occurrences[index];
                        index += 1;
                        if is_after_metadata(self.arena.components[occurrence.component.0].tag)
                            == after_metadata
                        {
                            remaining = occurrence.multiplicity.get();
                            break;
                        }
                    }
                    if remaining != 0 {
                        let occurrence = occurrences[index - 1];
                        let component = &self.arena.components[occurrence.component.0];
                        self.work.push(WireCursorOp::Components {
                            occurrences,
                            index,
                            remaining: remaining - 1,
                            after_metadata,
                        });
                        self.work.push(WireCursorOp::Node {
                            node: component.protobuf,
                            index: 0,
                        });
                        self.work.push(WireCursorOp::Inline(
                            InlineWireBytes::length_delimited_prefix(
                                component.tag.byte().into(),
                                self.arena.protobuf_nodes[component.protobuf].encoded_len,
                            ),
                        ));
                    }
                }
                WireCursorOp::RootMetadata(par) => {
                    let node = &self.arena.pars[par.0];
                    if node.connective_used {
                        self.work
                            .push(WireCursorOp::Inline(InlineWireBytes::protobuf_varints(&[
                                10 << 3,
                                1,
                            ])));
                    }
                    if !node.locally_free.is_empty() {
                        self.work.push(WireCursorOp::Bytes {
                            bytes: &node.locally_free,
                            offset: 0,
                        });
                        self.work.push(WireCursorOp::Inline(
                            InlineWireBytes::length_delimited_prefix(9, node.locally_free.len()),
                        ));
                    }
                }
                WireCursorOp::EPathMapSnapshot {
                    map,
                    prefix_emitted,
                    value_index,
                } => {
                    let node = &self.arena.epathmaps[map.0];
                    if !prefix_emitted {
                        self.work.push(WireCursorOp::EPathMapSnapshot {
                            map,
                            prefix_emitted: true,
                            value_index: 0,
                        });
                        self.work.push(WireCursorOp::Bytes {
                            bytes: &node.prefix,
                            offset: 0,
                        });
                    } else if let Some(&value) = node.value_order.get(value_index) {
                        self.work.push(WireCursorOp::EPathMapSnapshot {
                            map,
                            prefix_emitted: true,
                            value_index: value_index + 1,
                        });
                        self.work.push(WireCursorOp::Par(value));
                        self.work
                            .push(WireCursorOp::Inline(InlineWireBytes::protobuf_varints(&[
                                self.arena.pars[value.0].protobuf_len as u64,
                            ])));
                    }
                }
                WireCursorOp::Bytes { bytes, offset } => {
                    let byte = *bytes.get(offset)?;
                    if offset + 1 < bytes.len() {
                        self.work.push(WireCursorOp::Bytes {
                            bytes,
                            offset: offset + 1,
                        });
                    }
                    return Some(byte);
                }
                WireCursorOp::Inline(mut inline) => {
                    let byte = inline.bytes[usize::from(inline.offset)];
                    inline.offset += 1;
                    if inline.offset < inline.len {
                        self.work.push(WireCursorOp::Inline(inline));
                    }
                    return Some(byte);
                }
            }
        }
    }
}

struct ComponentNode {
    tag: ParComponentTag,
    score: PlannedScore,
    protobuf: usize,
    canonical_path_stable: bool,
    canonical_segment: Option<CanonicalSegment>,
}

enum CanonicalSegment {
    Bytes(Box<[u8]>),
    List(Box<[PlannedPar]>),
    Tuple(Box<[PlannedPar]>),
    EPathMap(PlannedEPathMap),
}

#[derive(Clone, Copy)]
struct Occurrence {
    component: PlannedComponent,
    multiplicity: Multiplicity,
}

struct ParNode {
    occurrences: Vec<Occurrence>,
    locally_free: Vec<u8>,
    connective_used: bool,
    score: PlannedScore,
    protobuf_len: usize,
    protobuf_zero_count: usize,
    canonical_path_stable: bool,
    split_children: Option<Box<[PlannedPar]>>,
    canonical_path_override: Option<Box<[u8]>>,
    canonical_segment_override: Option<Box<[u8]>>,
}

struct PlannedEPathMapNode {
    repr: EPathMapRepr<PlannedPar>,
    /// EPM1 magic/version/mode + ACTree03 arena + value count.
    prefix: Box<[u8]>,
    /// Canonical trie-order map-value schedule. This is an ordinal schedule of
    /// integer arena handles, not a decoded `Vec<Par>` or semantic authority.
    value_order: Box<[PlannedPar]>,
    snapshot_len: usize,
    snapshot_zero_count: usize,
    entries_stable: bool,
}

/// Temporary schema construction arena.
///
/// Every internal edge is an integer index. Dropping an arbitrarily deep,
/// unfinished plan therefore drops flat vectors; it cannot recurse through the
/// planned term on the native stack.
pub struct ParConstruction {
    byte_fragments: Vec<Box<[u8]>>,
    scores: Vec<ScoreNode>,
    protobuf_nodes: Vec<ProtoNode>,
    epathmaps: Vec<PlannedEPathMapNode>,
    components: Vec<ComponentNode>,
    pars: Vec<ParNode>,
}

impl Default for ParConstruction {
    fn default() -> Self { Self::new() }
}

impl ParConstruction {
    pub fn new() -> Self {
        Self {
            byte_fragments: Vec::new(),
            scores: Vec::new(),
            protobuf_nodes: Vec::new(),
            epathmaps: Vec::new(),
            components: Vec::new(),
            pars: Vec::new(),
        }
    }

    fn bytes(&mut self, bytes: impl Into<Box<[u8]>>) -> usize {
        let id = self.byte_fragments.len();
        self.byte_fragments.push(bytes.into());
        id
    }

    fn score_from_pieces(
        &mut self,
        pieces: Vec<ScorePiece>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let mut encoded_len = 0usize;
        for piece in &pieces {
            let add = match *piece {
                ScorePiece::Bytes(id) => self.byte_fragments[id].len(),
                ScorePiece::Child(child) => self.scores[child.0].encoded_len,
                ScorePiece::Repeat { child, count } => self.scores[child.0]
                    .encoded_len
                    .checked_mul(usize::try_from(count).map_err(|_| {
                        ParConstructionError::LengthOverflow("canonical score multiplicity")
                    })?)
                    .ok_or(ParConstructionError::LengthOverflow(
                        "canonical score multiplicity",
                    ))?,
                ScorePiece::EscapedWire { encoded_len, .. } => encoded_len,
            };
            encoded_len = encoded_len
                .checked_add(add)
                .ok_or(ParConstructionError::LengthOverflow("canonical score"))?;
        }
        let id = PlannedScore(self.scores.len());
        self.scores.push(ScoreNode {
            pieces,
            encoded_len,
        });
        Ok(id)
    }

    fn score_from_encoded(
        &mut self,
        canonical_score: &[u8],
    ) -> Result<PlannedScore, ParConstructionError> {
        let fragment = self.bytes(canonical_score.to_vec());
        self.score_from_pieces(vec![ScorePiece::Bytes(fragment)])
    }

    /// Import one already-computed canonical score tree into the fragment
    /// arena. This is used only for bounded non-`Par` leaves such as `Var` and
    /// unforgeables; recursive child scores remain arena edges.
    pub fn score_tree(
        &mut self,
        canonical_score: &Tree<ScoreAtom>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let mut encoded = Vec::new();
        encode_order_key(canonical_score, &mut encoded);
        self.score_from_encoded(&encoded)
    }

    /// One canonical integer score leaf.
    pub fn score_i64(&mut self, value: i64) -> Result<PlannedScore, ParConstructionError> {
        let mut bytes = Vec::with_capacity(10);
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
        let bytes = self.bytes(bytes);
        self.score_from_pieces(vec![ScorePiece::Bytes(bytes)])
    }

    /// One canonical string score leaf.
    pub fn score_string(&mut self, value: &str) -> Result<PlannedScore, ParConstructionError> {
        self.score_terminated(2, value.as_bytes())
    }

    /// One canonical byte-array score leaf.
    pub fn score_bytes(&mut self, value: &[u8]) -> Result<PlannedScore, ParConstructionError> {
        self.score_terminated(3, value)
    }

    fn score_terminated(
        &mut self,
        kind: u8,
        value: &[u8],
    ) -> Result<PlannedScore, ParConstructionError> {
        let zeroes = value.iter().filter(|&&byte| byte == 0).count();
        let capacity = 4usize
            .checked_add(value.len())
            .and_then(|n| n.checked_add(zeroes))
            .ok_or(ParConstructionError::LengthOverflow("canonical score atom"))?;
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(&[1, kind]);
        for &byte in value {
            if byte == 0 {
                bytes.extend_from_slice(&[0, 0xff]);
            } else {
                bytes.push(byte);
            }
        }
        bytes.extend_from_slice(&[0, 0]);
        let bytes = self.bytes(bytes);
        self.score_from_pieces(vec![ScorePiece::Bytes(bytes)])
    }

    /// A canonical byte-array score atom whose payload remains a streaming
    /// wire graph. Zero escaping is represented as a cursor transform and its
    /// final length is derived from maintained byte/zero counts, so nested
    /// EPathMaps do not get flattened merely to become an ordering key.
    fn score_wire_bytes(
        &mut self,
        root: WireRoot,
        wire_len: usize,
        zero_count: usize,
    ) -> Result<PlannedScore, ParConstructionError> {
        let prefix = self.bytes(Box::<[u8]>::from([1, 3]));
        let suffix = self.bytes(Box::<[u8]>::from([0, 0]));
        let escaped_len =
            wire_len
                .checked_add(zero_count)
                .ok_or(ParConstructionError::LengthOverflow(
                    "escaped canonical byte score",
                ))?;
        self.score_from_pieces(vec![
            ScorePiece::Bytes(prefix),
            ScorePiece::EscapedWire {
                root,
                encoded_len: escaped_len,
            },
            ScorePiece::Bytes(suffix),
        ])
    }

    /// A canonical score node with no implicit constructor leaf.
    pub fn score_node(
        &mut self,
        children: impl IntoIterator<Item = PlannedScore>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let start = self.bytes(Box::<[u8]>::from([2]));
        let end = self.bytes(Box::<[u8]>::from([0]));
        let mut pieces = vec![ScorePiece::Bytes(start)];
        pieces.extend(children.into_iter().map(ScorePiece::Child));
        pieces.push(ScorePiece::Bytes(end));
        self.score_from_pieces(pieces)
    }

    /// A canonical score node whose first child is the constructor integer.
    pub fn score_constructor(
        &mut self,
        constructor: i32,
        children: impl IntoIterator<Item = PlannedScore>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let constructor = self.score_i64(i64::from(constructor))?;
        self.score_node(std::iter::once(constructor).chain(children))
    }

    fn score_node_with_pieces(
        &mut self,
        constructor: i32,
        children: Vec<ScorePiece>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let start = self.bytes(Box::<[u8]>::from([2]));
        let end = self.bytes(Box::<[u8]>::from([0]));
        let constructor = self.score_i64(i64::from(constructor))?;
        let mut pieces = Vec::with_capacity(children.len() + 3);
        pieces.push(ScorePiece::Bytes(start));
        pieces.push(ScorePiece::Child(constructor));
        pieces.extend(children);
        pieces.push(ScorePiece::Bytes(end));
        self.score_from_pieces(pieces)
    }

    pub fn par_score(&self, par: PlannedPar) -> PlannedScore { self.pars[par.0].score }

    pub fn locally_free(&self, par: PlannedPar) -> &[u8] { &self.pars[par.0].locally_free }

    pub fn connective_used(&self, par: PlannedPar) -> bool { self.pars[par.0].connective_used }

    /// Exact canonical term order for planned `Par` values: semantic score,
    /// then emitted protobuf bytes. Both halves are compared as cursors.
    pub fn compare_pars(&self, left: PlannedPar, right: PlannedPar) -> Ordering {
        ScoreByteCursor::new(self, self.par_score(left))
            .cmp(ScoreByteCursor::new(self, self.par_score(right)))
            .then_with(|| {
                WireByteCursor::new(self, WireRoot::Par(left))
                    .cmp(WireByteCursor::new(self, WireRoot::Par(right)))
            })
    }

    /// Emit the exact canonical PathMap key of a planned Par directly from the
    /// construction graph. Stable collection segments traverse child handles;
    /// unstable positions stream the already-planned protobuf body into the
    /// escape arm. No nested resident `Par` is constructed.
    pub fn canonical_path_bytes(&self, root: PlannedPar) -> Result<Vec<u8>, ParConstructionError> {
        enum Op {
            Path(PlannedPar),
            Segment(PlannedPar),
            Byte(u8),
        }

        fn push_uv(out: &mut Vec<u8>, mut value: u64) {
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                out.push(byte);
                if value == 0 {
                    break;
                }
            }
        }

        let mut out = Vec::new();
        let mut work = vec![Op::Path(root)];
        while let Some(op) = work.pop() {
            match op {
                Op::Byte(byte) => out.push(byte),
                Op::Path(par) => {
                    let node = &self.pars[par.0];
                    if let Some(path) = &node.canonical_path_override {
                        out.extend_from_slice(path);
                    } else if let Some(children) = &node.split_children {
                        work.push(Op::Byte(path_tag::TERM));
                        for &child in children.iter().rev() {
                            work.push(Op::Segment(child));
                        }
                    } else {
                        work.push(Op::Segment(par));
                    }
                }
                Op::Segment(par) => {
                    let node = &self.pars[par.0];
                    if let Some(segment) = &node.canonical_segment_override {
                        out.extend_from_slice(segment);
                        continue;
                    }
                    if !node.canonical_path_stable {
                        out.push(path_tag::ESCAPE);
                        push_uv(&mut out, node.protobuf_len as u64);
                        self.emit_par(par, &mut out);
                        continue;
                    }
                    let occurrence = node
                        .occurrences
                        .first()
                        .expect("a stable Par has one component occurrence");
                    let component = &self.components[occurrence.component.0];
                    match component
                        .canonical_segment
                        .as_ref()
                        .expect("a newly planned stable component has a segment program")
                    {
                        CanonicalSegment::Bytes(bytes) => out.extend_from_slice(bytes),
                        CanonicalSegment::List(children) => {
                            out.push(path_tag::ELIST);
                            push_uv(&mut out, children.len() as u64);
                            for &child in children.iter().rev() {
                                work.push(Op::Segment(child));
                            }
                        }
                        CanonicalSegment::Tuple(children) => {
                            out.push(path_tag::ETUPLE);
                            push_uv(&mut out, children.len() as u64);
                            for &child in children.iter().rev() {
                                work.push(Op::Segment(child));
                            }
                        }
                        CanonicalSegment::EPathMap(map) => {
                            let map_node = &self.epathmaps[map.0];
                            match &map_node.repr {
                                EPathMapRepr::Empty => {
                                    out.push(path_tag::EPATHMAP);
                                    push_uv(&mut out, 0);
                                }
                                EPathMapRepr::Set(entries) => {
                                    let mut region_len = 0usize;
                                    let mut zipper = entries.read_zipper();
                                    while zipper.to_next_val() {
                                        let key = zipper.path();
                                        region_len = region_len
                                            .checked_add(encoded_len_varint(key.len() as u64))
                                            .and_then(|n| n.checked_add(key.len()))
                                            .ok_or(ParConstructionError::LengthOverflow(
                                                "stable EPathMap region",
                                            ))?;
                                    }
                                    out.push(path_tag::EPATHMAP);
                                    push_uv(&mut out, region_len as u64);
                                    let mut zipper = entries.read_zipper();
                                    while zipper.to_next_val() {
                                        let key = zipper.path();
                                        push_uv(&mut out, key.len() as u64);
                                        out.extend_from_slice(key);
                                    }
                                }
                                EPathMapRepr::Map(_) => {
                                    out.push(path_tag::EPATHMAP_MAP);
                                    push_uv(&mut out, map_node.snapshot_len as u64);
                                    self.emit_wire(WireRoot::EPathMapSnapshot(*map), &mut out);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn push_proto_node(&mut self, pieces: Vec<ProtoPiece>) -> Result<usize, ParConstructionError> {
        let mut encoded_len = 0usize;
        let mut zero_count = 0usize;
        for piece in &pieces {
            let (child_len, child_zeroes, prefix) = match piece {
                ProtoPiece::Bytes(bytes) => {
                    encoded_len = encoded_len
                        .checked_add(bytes.len())
                        .ok_or(ParConstructionError::LengthOverflow("protobuf node"))?;
                    zero_count = zero_count
                        .checked_add(bytes.iter().filter(|&&byte| byte == 0).count())
                        .ok_or(ParConstructionError::LengthOverflow(
                            "protobuf zero-byte count",
                        ))?;
                    continue;
                }
                ProtoPiece::Node { tag, node } => (
                    self.protobuf_nodes[*node].encoded_len,
                    self.protobuf_nodes[*node].zero_count,
                    InlineWireBytes::length_delimited_prefix(
                        *tag,
                        self.protobuf_nodes[*node].encoded_len,
                    ),
                ),
                ProtoPiece::Par { tag, par } => (
                    self.pars[par.0].protobuf_len,
                    self.pars[par.0].protobuf_zero_count,
                    InlineWireBytes::length_delimited_prefix(*tag, self.pars[par.0].protobuf_len),
                ),
                ProtoPiece::EPathMapSnapshot { tag, map } => (
                    self.epathmaps[map.0].snapshot_len,
                    self.epathmaps[map.0].snapshot_zero_count,
                    InlineWireBytes::length_delimited_prefix(
                        *tag,
                        self.epathmaps[map.0].snapshot_len,
                    ),
                ),
            };
            encoded_len = encoded_len
                .checked_add(usize::from(prefix.len))
                .and_then(|n| n.checked_add(child_len))
                .ok_or(ParConstructionError::LengthOverflow("protobuf node"))?;
            zero_count = zero_count
                .checked_add(prefix.zero_count())
                .and_then(|n| n.checked_add(child_zeroes))
                .ok_or(ParConstructionError::LengthOverflow(
                    "protobuf zero-byte count",
                ))?;
        }
        let id = self.protobuf_nodes.len();
        self.protobuf_nodes.push(ProtoNode {
            pieces,
            encoded_len,
            zero_count,
        });
        Ok(id)
    }

    fn wrap_proto_node(&mut self, tag: u32, child: usize) -> Result<usize, ParConstructionError> {
        self.push_proto_node(vec![ProtoPiece::Node { tag, node: child }])
    }

    fn component_from_proto(
        &mut self,
        tag: ParComponentTag,
        score: PlannedScore,
        protobuf: usize,
        canonical_path_stable: bool,
        canonical_segment: Option<CanonicalSegment>,
    ) -> PlannedComponent {
        let component = PlannedComponent(self.components.len());
        self.components.push(ComponentNode {
            tag,
            score,
            protobuf,
            canonical_path_stable,
            canonical_segment,
        });
        component
    }

    fn planned_epathmap(
        &mut self,
        shell: &EPathMap,
        par_children: &[PlannedPar],
    ) -> Result<(PlannedEPathMap, usize), ParConstructionError> {
        let expected = match shell.mode() {
            EPathMapMode::Empty => 0,
            EPathMapMode::Set => shell.len(),
            EPathMapMode::Map => shell
                .len()
                .checked_mul(2)
                .ok_or(ParConstructionError::LengthOverflow("EPathMap child arity"))?,
        };
        if expected != par_children.len() {
            return Err(ParConstructionError::ChildArity {
                context: "EPathMap",
                expected,
                supplied: par_children.len(),
            });
        }

        let repr = match shell.mode() {
            EPathMapMode::Empty => EPathMapRepr::Empty,
            EPathMapMode::Set => {
                let mut entries = PathMap::new();
                for &entry in par_children {
                    let key = self.canonical_path_bytes(entry)?;
                    entries.insert(&key, ());
                }
                if entries.is_empty() {
                    EPathMapRepr::Empty
                } else {
                    EPathMapRepr::Set(entries)
                }
            }
            EPathMapMode::Map => {
                let mut entries = PathMap::new();
                for pair in par_children.chunks_exact(2) {
                    let key = pair[0];
                    let value = pair[1];
                    let encoded_key = self.canonical_path_bytes(key)?;
                    entries.insert(&encoded_key, value);
                }
                if entries.is_empty() {
                    EPathMapRepr::Empty
                } else {
                    EPathMapRepr::Map(entries)
                }
            }
        };

        let entries_stable = match &repr {
            EPathMapRepr::Empty => true,
            EPathMapRepr::Set(entries) => {
                let mut stable = true;
                let mut zipper = entries.read_zipper();
                while stable && zipper.to_next_val() {
                    stable = super::canonical_path::path_declares_stable(zipper.path())
                        .expect("planned keys are emitted by the canonical path PDA");
                }
                stable
            }
            EPathMapRepr::Map(entries) => {
                let mut stable = true;
                let mut zipper = entries.read_zipper();
                while stable {
                    let Some(value) = zipper.to_next_get_val() else {
                        break;
                    };
                    stable = super::canonical_path::path_declares_stable(zipper.path())
                        .expect("planned keys are emitted by the canonical path PDA")
                        && self.pars[value.0].canonical_path_stable;
                }
                stable
            }
        };

        let layout = epathmap_trie_codec::layout(&repr);
        let prefix = layout.prefix().to_vec().into_boxed_slice();
        let mut value_order = Vec::with_capacity(layout.value_count());
        if let EPathMapRepr::Map(entries) = &repr {
            let mut zipper = entries.read_zipper();
            while let Some(value) = zipper.to_next_get_val() {
                value_order.push(*value);
            }
        }
        debug_assert_eq!(value_order.len(), layout.value_count());

        let mut snapshot_len = prefix.len();
        let mut snapshot_zero_count = prefix.iter().filter(|&&byte| byte == 0).count();
        for &value in &value_order {
            let body_len = self.pars[value.0].protobuf_len;
            let len_prefix = InlineWireBytes::protobuf_varints(&[body_len as u64]);
            snapshot_len = snapshot_len
                .checked_add(usize::from(len_prefix.len))
                .and_then(|n| n.checked_add(body_len))
                .ok_or(ParConstructionError::LengthOverflow(
                    "planned EPathMap snapshot",
                ))?;
            snapshot_zero_count = snapshot_zero_count
                .checked_add(len_prefix.zero_count())
                .and_then(|n| n.checked_add(self.pars[value.0].protobuf_zero_count))
                .ok_or(ParConstructionError::LengthOverflow(
                    "planned EPathMap zero-byte count",
                ))?;
        }

        let map = PlannedEPathMap(self.epathmaps.len());
        self.epathmaps.push(PlannedEPathMapNode {
            repr,
            prefix,
            value_order: value_order.into_boxed_slice(),
            snapshot_len,
            snapshot_zero_count,
            entries_stable,
        });

        let mut pieces = Vec::new();
        let mut bounded = Vec::new();
        if !shell.locally_free.is_empty() {
            prost::encoding::bytes::encode(3, &shell.locally_free, &mut bounded);
        }
        if shell.connective_used {
            prost::encoding::bool::encode(4, &true, &mut bounded);
        }
        if !bounded.is_empty() {
            pieces.push(ProtoPiece::Bytes(bounded.into_boxed_slice()));
        }
        if let Some(remainder) = &shell.remainder {
            let remainder = self.plan_protobuf(remainder, &[])?;
            pieces.push(ProtoPiece::Node {
                tag: 5,
                node: remainder,
            });
        }
        pieces.push(ProtoPiece::EPathMapSnapshot { tag: 9, map });
        let protobuf = self.push_proto_node(pieces)?;
        Ok((map, protobuf))
    }

    /// Plan one canonical component shell.
    ///
    /// `par_children` is ordered by the generated protobuf walk, not by Rust
    /// declaration order. The method proves exact consumption in both
    /// directions, so schema drift fails at construction rather than emitting
    /// a shifted child into a later field.
    pub fn component<T: ParBagComponent>(
        &mut self,
        shell: &T,
        par_children: &[PlannedPar],
        score: PlannedScore,
        canonical_path_stable: bool,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let protobuf = self.plan_protobuf(shell, par_children)?;
        let id = PlannedComponent(self.components.len());
        self.components.push(ComponentNode {
            tag: T::TAG,
            score,
            protobuf,
            canonical_path_stable,
            canonical_segment: None,
        });
        Ok(id)
    }

    fn require_arity(
        context: &'static str,
        expected: usize,
        supplied: usize,
    ) -> Result<(), ParConstructionError> {
        if expected == supplied {
            Ok(())
        } else {
            Err(ParConstructionError::ChildArity {
                context,
                expected,
                supplied,
            })
        }
    }

    /// Plan a canonical Send. Child order is channel followed by data.
    pub fn send_component(
        &mut self,
        shell: &Send,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("Send", shell.data.len() + 1, children.len())?;
        let persistent = self.score_i64(i64::from(shell.persistent))?;
        let connective = self.score_i64(i64::from(shell.connective_used))?;
        let scores: Vec<_> = std::iter::once(persistent)
            .chain(std::iter::once(self.par_score(children[0])))
            .chain(children[1..].iter().map(|child| self.par_score(*child)))
            .chain(std::iter::once(connective))
            .collect();
        let score = self.score_constructor(Score::SEND, scores)?;
        self.component(shell, children, score, false)
    }

    /// Plan a canonical Bundle from its single body.
    pub fn bundle_component(
        &mut self,
        shell: &Bundle,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("Bundle", 1, children.len())?;
        let constructor = match (shell.write_flag, shell.read_flag) {
            (true, true) => Score::BUNDLE_READ_WRITE,
            (true, false) => Score::BUNDLE_WRITE,
            (false, true) => Score::BUNDLE_READ,
            (false, false) => Score::BUNDLE_EQUIV,
        };
        let score = self.score_constructor(constructor, [self.par_score(children[0])])?;
        self.component(shell, children, score, false)
    }

    /// Plan a canonical conditional in condition/true/false order.
    pub fn if_component(
        &mut self,
        shell: &If,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("If", 3, children.len())?;
        let connective = self.score_i64(i64::from(shell.connective_used))?;
        let score = self.score_constructor(Score::IF, [
            self.par_score(children[0]),
            self.par_score(children[1]),
            self.par_score(children[2]),
            connective,
        ])?;
        self.component(shell, children, score, false)
    }

    /// Plan a leaf unforgeable and retain the GPrivate canonical-path arm.
    pub fn unforgeable_component(
        &mut self,
        shell: &GUnforgeable,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let score = self.score_tree(&sort_unforgeable(shell).score)?;
        let (stable, segment) = match shell.unf_instance.as_ref() {
            Some(UnfInstance::GPrivateBody(private)) => {
                let mut bytes = vec![path_tag::GPRIVATE];
                let mut len = private.id.len() as u64;
                loop {
                    let mut byte = (len & 0x7f) as u8;
                    len >>= 7;
                    if len != 0 {
                        byte |= 0x80;
                    }
                    bytes.push(byte);
                    if len == 0 {
                        break;
                    }
                }
                bytes.extend_from_slice(&private.id);
                (
                    true,
                    Some(CanonicalSegment::Bytes(bytes.into_boxed_slice())),
                )
            }
            _ => (false, None),
        };
        let component = self.component(shell, &[], score, stable)?;
        self.components[component.0].canonical_segment = segment;
        Ok(component)
    }

    /// Plan a connective. And/Or children remain order-preserving, matching
    /// the historical sorter; no commutative reorder is invented here.
    pub fn connective_component(
        &mut self,
        shell: &Connective,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        let score = match shell.connective_instance.as_ref() {
            Some(ConnectiveInstance::ConnAndBody(and)) => {
                Self::require_arity("Connective::And", and.ps.len(), children.len())?;
                let scores: Vec<_> = children
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                self.score_constructor(Score::CONNECTIVE_AND, scores)?
            }
            Some(ConnectiveInstance::ConnOrBody(or)) => {
                Self::require_arity("Connective::Or", or.ps.len(), children.len())?;
                let scores: Vec<_> = children
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                self.score_constructor(Score::CONNECTIVE_OR, scores)?
            }
            Some(ConnectiveInstance::ConnNotBody(_)) => {
                Self::require_arity("Connective::Not", 1, children.len())?;
                self.score_constructor(Score::CONNECTIVE_NOT, [self.par_score(children[0])])?
            }
            Some(ConnectiveInstance::VarRefBody(var)) => {
                Self::require_arity("Connective::VarRef", 0, children.len())?;
                let index = self.score_i64(i64::from(var.index))?;
                let depth = self.score_i64(i64::from(var.depth))?;
                self.score_constructor(Score::CONNECTIVE_VARREF, [index, depth])?
            }
            Some(ConnectiveInstance::ConnBool(value)) => {
                Self::require_arity("Connective::Bool", 0, children.len())?;
                let value = self.score_i64(i64::from(*value))?;
                self.score_constructor(Score::CONNECTIVE_BOOL, [value])?
            }
            Some(ConnectiveInstance::ConnInt(value)) => {
                Self::require_arity("Connective::Int", 0, children.len())?;
                let value = self.score_i64(i64::from(*value))?;
                self.score_constructor(Score::CONNECTIVE_INT, [value])?
            }
            Some(ConnectiveInstance::ConnString(value)) => {
                Self::require_arity("Connective::String", 0, children.len())?;
                let value = self.score_i64(i64::from(*value))?;
                self.score_constructor(Score::CONNECTIVE_STRING, [value])?
            }
            Some(ConnectiveInstance::ConnUri(value)) => {
                Self::require_arity("Connective::Uri", 0, children.len())?;
                let value = self.score_i64(i64::from(*value))?;
                self.score_constructor(Score::CONNECTIVE_URI, [value])?
            }
            Some(ConnectiveInstance::ConnByteArray(value)) => {
                Self::require_arity("Connective::ByteArray", 0, children.len())?;
                let value = self.score_i64(i64::from(*value))?;
                self.score_constructor(Score::CONNECTIVE_BYTEARRAY, [value])?
            }
            None => {
                Self::require_arity("absent Connective", 0, children.len())?;
                self.score_i64(Score::ABSENT as i64)?
            }
        };
        self.component(shell, children, score, false)
    }

    /// Plan New. Children are body followed by injection values in BTreeMap
    /// key order, which is both the sorter and generated-map order.
    pub fn new_component(
        &mut self,
        shell: &New,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("New", shell.injections.len() + 1, children.len())?;
        let mut canonical_shell = shell.clone();
        canonical_shell.uri.sort();

        let bind_count = self.score_i64(i64::from(shell.bind_count))?;
        let mut scores = vec![bind_count];
        if canonical_shell.uri.is_empty() {
            scores.push(self.score_i64(Score::ABSENT as i64)?);
        } else {
            for uri in &canonical_shell.uri {
                scores.push(self.score_string(uri)?);
            }
        }
        if shell.injections.is_empty() {
            scores.push(self.score_i64(Score::ABSENT as i64)?);
        } else {
            for ((key, _), value) in shell.injections.iter().zip(&children[1..]) {
                let key = self.score_string(key)?;
                scores.push(self.score_node([key, self.par_score(*value)])?);
            }
        }
        scores.push(self.par_score(children[0]));
        let score = self.score_constructor(Score::NEW, scores)?;
        self.component(&canonical_shell, children, score, false)
    }

    /// Plan Match. The input includes one guard handle per case even when the
    /// source guard is absent; absent/empty guards contribute to the score but
    /// are omitted from canonical protobuf exactly like the production sorter.
    pub fn match_component(
        &mut self,
        shell: &Match,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        let expected = 1usize
            .checked_add(
                shell
                    .cases
                    .len()
                    .checked_mul(3)
                    .ok_or(ParConstructionError::LengthOverflow("Match child arity"))?,
            )
            .ok_or(ParConstructionError::LengthOverflow("Match child arity"))?;
        Self::require_arity("Match", expected, children.len())?;

        let target = children[0];
        let mut scores = Vec::with_capacity(shell.cases.len() + 2);
        scores.push(self.par_score(target));
        let mut wire_children = Vec::with_capacity(expected);
        wire_children.push(target);
        let mut canonical_shell = shell.clone();
        let mut cursor = 1usize;
        for (source_case, canonical_case) in
            shell.cases.iter().zip(canonical_shell.cases.iter_mut())
        {
            let pattern = children[cursor];
            let source = children[cursor + 1];
            let guard = children[cursor + 2];
            cursor += 3;
            let free_count = self.score_i64(i64::from(source_case.free_count))?;
            scores.push(self.score_node([
                self.par_score(pattern),
                self.par_score(source),
                free_count,
                self.par_score(guard),
            ])?);
            wire_children.extend([pattern, source]);
            let guard_present = source_case
                .guard
                .as_ref()
                .is_some_and(|guard| guard != &Par::default());
            if guard_present {
                wire_children.push(guard);
            } else {
                canonical_case.guard = None;
            }
        }
        scores.push(self.score_i64(i64::from(shell.connective_used))?);
        let score = self.score_constructor(Score::MATCH, scores)?;
        self.component(&canonical_shell, &wire_children, score, false)
    }

    /// Plan Receive. Children are every bind's patterns then source, followed
    /// by body and one condition handle (the empty planned Par when absent).
    pub fn receive_component(
        &mut self,
        shell: &Receive,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        let bind_children = shell.binds.iter().try_fold(0usize, |sum, bind| {
            sum.checked_add(bind.patterns.len() + 1)
                .ok_or(ParConstructionError::LengthOverflow("Receive child arity"))
        })?;
        let expected = bind_children
            .checked_add(2)
            .ok_or(ParConstructionError::LengthOverflow("Receive child arity"))?;
        Self::require_arity("Receive", expected, children.len())?;

        let persistent = self.score_i64(i64::from(shell.persistent))?;
        let peek = self.score_i64(i64::from(shell.peek))?;
        let mut scores = vec![persistent, peek];
        let mut wire_children = Vec::with_capacity(expected);
        let mut cursor = 0usize;
        for bind in &shell.binds {
            let pattern_end = cursor + bind.patterns.len();
            let patterns = &children[cursor..pattern_end];
            let source = children[pattern_end];
            cursor = pattern_end + 1;

            let remainder = match &bind.remainder {
                Some(var) => self.score_tree(&sort_var(var).score)?,
                None => self.score_i64(Score::ABSENT as i64)?,
            };
            let bind_scores: Vec<_> = std::iter::once(self.par_score(source))
                .chain(patterns.iter().map(|pattern| self.par_score(*pattern)))
                .chain(std::iter::once(remainder))
                .collect();
            scores.push(self.score_node(bind_scores)?);
            wire_children.extend(patterns.iter().copied());
            wire_children.push(source);
        }

        let body = children[cursor];
        let condition = children[cursor + 1];
        wire_children.push(body);
        let condition_present = shell
            .condition
            .as_ref()
            .is_some_and(|condition| condition != &Par::default());
        let mut canonical_shell = shell.clone();
        if condition_present {
            wire_children.push(condition);
        } else {
            canonical_shell.condition = None;
        }
        scores.extend([
            self.par_score(body),
            self.score_i64(i64::from(shell.bind_count))?,
            self.score_i64(i64::from(shell.connective_used))?,
            self.par_score(condition),
        ]);
        let score = self.score_constructor(Score::RECEIVE, scores)?;
        self.component(&canonical_shell, &wire_children, score, false)
    }

    fn schema_expr_component(
        &mut self,
        shell: &Expr,
        par_children: &[PlannedPar],
        score: PlannedScore,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let (canonical_segment, stable) = self.expr_canonical_segment(shell, par_children)?;
        let component = self.component(shell, par_children, score, stable)?;
        self.components[component.0].canonical_segment = canonical_segment;
        Ok(component)
    }

    fn expr_canonical_segment(
        &self,
        shell: &Expr,
        par_children: &[PlannedPar],
    ) -> Result<(Option<CanonicalSegment>, bool), ParConstructionError> {
        fn push_uv(out: &mut Vec<u8>, mut value: u64) {
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                out.push(byte);
                if value == 0 {
                    break;
                }
            }
        }

        let Some(instance) = shell.expr_instance.as_ref() else {
            return Ok((None, false));
        };
        let mut bytes = Vec::new();
        let segment = match instance {
            ExprInstance::GBool(value) => {
                bytes.push(if *value {
                    path_tag::GBOOL_TRUE
                } else {
                    path_tag::GBOOL_FALSE
                });
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GInt(value) => {
                bytes.push(path_tag::GINT);
                push_uv(&mut bytes, ((*value << 1) ^ (*value >> 63)) as u64);
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GString(value) => {
                bytes.push(path_tag::GSTRING);
                push_uv(&mut bytes, value.len() as u64);
                bytes.extend_from_slice(value.as_bytes());
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GUri(value) => {
                bytes.push(path_tag::GURI);
                push_uv(&mut bytes, value.len() as u64);
                bytes.extend_from_slice(value.as_bytes());
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GByteArray(value) => {
                bytes.push(path_tag::GBYTE_ARRAY);
                push_uv(&mut bytes, value.len() as u64);
                bytes.extend_from_slice(value);
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GDouble(bits) => {
                bytes.push(path_tag::GDOUBLE);
                bytes.extend_from_slice(&bits.to_be_bytes());
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GBigInt(value) => {
                bytes.push(path_tag::GBIG_INT);
                push_uv(&mut bytes, value.len() as u64);
                bytes.extend_from_slice(value);
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GBigRat(value) => {
                bytes.push(path_tag::GBIG_RAT);
                push_uv(&mut bytes, value.numerator.len() as u64);
                bytes.extend_from_slice(&value.numerator);
                push_uv(&mut bytes, value.denominator.len() as u64);
                bytes.extend_from_slice(&value.denominator);
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::GFixedPoint(value) => {
                bytes.push(path_tag::GFIXED_POINT);
                push_uv(&mut bytes, value.unscaled.len() as u64);
                bytes.extend_from_slice(&value.unscaled);
                push_uv(&mut bytes, u64::from(value.scale));
                CanonicalSegment::Bytes(bytes.into_boxed_slice())
            }
            ExprInstance::EListBody(list)
                if list.remainder.is_none()
                    && list.locally_free.is_empty()
                    && !list.connective_used =>
            {
                return Ok((
                    Some(CanonicalSegment::List(
                        par_children.to_vec().into_boxed_slice(),
                    )),
                    par_children
                        .iter()
                        .all(|child| self.pars[child.0].canonical_path_stable),
                ));
            }
            ExprInstance::ETupleBody(tuple)
                if tuple.locally_free.is_empty() && !tuple.connective_used =>
            {
                return Ok((
                    Some(CanonicalSegment::Tuple(
                        par_children.to_vec().into_boxed_slice(),
                    )),
                    par_children
                        .iter()
                        .all(|child| self.pars[child.0].canonical_path_stable),
                ));
            }
            _ => return Ok((None, false)),
        };
        Ok((Some(segment), true))
    }

    /// Plan an order-preserving `Expr` constructor directly from normalized
    /// child handles.
    ///
    /// The supplied child order is the sorter's evaluation order. Where that
    /// differs from protobuf field order (`EMethod`), this method performs the
    /// permutation once before invoking the generated schema walk. Recursive
    /// scores remain arena references rather than owned score trees.
    pub fn expr_component(
        &mut self,
        shell: &Expr,
        par_children: &[PlannedPar],
        _canonical_path_stable: bool,
    ) -> Result<PlannedComponent, ParConstructionError> {
        fn exact(
            context: &'static str,
            expected: usize,
            supplied: usize,
        ) -> Result<(), ParConstructionError> {
            if expected == supplied {
                Ok(())
            } else {
                Err(ParConstructionError::ChildArity {
                    context,
                    expected,
                    supplied,
                })
            }
        }

        let Some(instance) = shell.expr_instance.as_ref() else {
            exact("absent Expr", 0, par_children.len())?;
            let score = ExprSortMatcher::sort_match(shell).score;
            let score = self.score_tree(&score)?;
            return self.schema_expr_component(shell, &[], score);
        };

        let unary_tag = match instance {
            ExprInstance::ENegBody(_) => Some(Score::ENEG),
            ExprInstance::ENotBody(_) => Some(Score::ENOT),
            _ => None,
        };
        if let Some(tag) = unary_tag {
            exact("unary Expr", 1, par_children.len())?;
            let score = self.score_constructor(tag, [self.par_score(par_children[0])])?;
            return self.schema_expr_component(shell, par_children, score);
        }

        let binary_tag = match instance {
            ExprInstance::EMultBody(_) => Some(Score::EMULT),
            ExprInstance::EDivBody(_) => Some(Score::EDIV),
            ExprInstance::EModBody(_) => Some(Score::EMOD),
            ExprInstance::EPlusBody(_) => Some(Score::EPLUS),
            ExprInstance::EMinusBody(_) => Some(Score::EMINUS),
            ExprInstance::ELtBody(_) => Some(Score::ELT),
            ExprInstance::ELteBody(_) => Some(Score::ELTE),
            ExprInstance::EGtBody(_) => Some(Score::EGT),
            ExprInstance::EGteBody(_) => Some(Score::EGTE),
            ExprInstance::EEqBody(_) => Some(Score::EEQ),
            ExprInstance::ENeqBody(_) => Some(Score::ENEQ),
            ExprInstance::EAndBody(_) => Some(Score::EAND),
            ExprInstance::EOrBody(_) => Some(Score::EOR),
            ExprInstance::EMatchesBody(_) => Some(Score::EMATCHES),
            ExprInstance::EPercentPercentBody(_) => Some(Score::EPERCENT),
            ExprInstance::EPlusPlusBody(_) => Some(Score::EPLUSPLUS),
            ExprInstance::EMinusMinusBody(_) => Some(Score::EMINUSMINUS),
            _ => None,
        };
        if let Some(tag) = binary_tag {
            exact("binary Expr", 2, par_children.len())?;
            let score = self.score_constructor(tag, [
                self.par_score(par_children[0]),
                self.par_score(par_children[1]),
            ])?;
            return self.schema_expr_component(shell, par_children, score);
        }

        match instance {
            ExprInstance::EListBody(list) => {
                exact("EList", list.ps.len(), par_children.len())?;
                let remainder = match &list.remainder {
                    Some(var) => self.score_tree(&sort_var(var).score)?,
                    None => self.score_i64(-1)?,
                };
                let connective = self.score_i64(i64::from(list.connective_used))?;
                let mut children = Vec::with_capacity(par_children.len() + 2);
                children.push(remainder);
                children.extend(par_children.iter().map(|child| self.par_score(*child)));
                children.push(connective);
                let score = self.score_constructor(Score::ELIST, children)?;
                self.schema_expr_component(shell, par_children, score)
            }
            ExprInstance::ETupleBody(tuple) => {
                exact("ETuple", tuple.ps.len(), par_children.len())?;
                let connective = self.score_i64(i64::from(tuple.connective_used))?;
                let mut children = Vec::with_capacity(par_children.len() + 1);
                children.extend(par_children.iter().map(|child| self.par_score(*child)));
                children.push(connective);
                let score = self.score_constructor(Score::ETUPLE, children)?;
                self.schema_expr_component(shell, par_children, score)
            }
            ExprInstance::EMethodBody(method) => {
                exact("EMethod", method.arguments.len() + 1, par_children.len())?;
                let (&target, arguments) = par_children
                    .split_last()
                    .expect("EMethod arity proved one target");
                let method_name = self.score_string(&method.method_name)?;
                let connective = self.score_i64(i64::from(method.connective_used))?;
                let mut children = Vec::with_capacity(par_children.len() + 2);
                children.push(method_name);
                children.push(self.par_score(target));
                children.extend(arguments.iter().map(|child| self.par_score(*child)));
                children.push(connective);
                let score = self.score_constructor(Score::EMETHOD, children)?;
                let wire_children: Vec<_> = std::iter::once(target)
                    .chain(arguments.iter().copied())
                    .collect();
                self.schema_expr_component(shell, &wire_children, score)
            }
            ExprInstance::GBool(_)
            | ExprInstance::GInt(_)
            | ExprInstance::GString(_)
            | ExprInstance::GUri(_)
            | ExprInstance::GByteArray(_)
            | ExprInstance::GDouble(_)
            | ExprInstance::GBigInt(_)
            | ExprInstance::GBigRat(_)
            | ExprInstance::GFixedPoint(_)
            | ExprInstance::EVarBody(_) => {
                exact("leaf Expr", 0, par_children.len())?;
                let score = ExprSortMatcher::sort_match(shell).score;
                let score = self.score_tree(&score)?;
                self.schema_expr_component(shell, &[], score)
            }
            ExprInstance::ESetBody(set) => {
                exact("ESet", set.ps.len(), par_children.len())?;
                let mut canonical = par_children.to_vec();
                canonical.sort_by(|left, right| self.compare_pars(*left, *right));
                canonical
                    .dedup_by(|right, left| self.compare_pars(*left, *right) == Ordering::Equal);

                let mut canonical_shell = shell.clone();
                let ExprInstance::ESetBody(canonical_set) = canonical_shell
                    .expr_instance
                    .as_mut()
                    .expect("ESet shell retains its instance")
                else {
                    unreachable!("the exhaustive arm selected ESet")
                };
                canonical_set.ps = vec![Par::default(); canonical.len()];

                let remainder = match &set.remainder {
                    Some(var) => self.score_tree(&sort_var(var).score)?,
                    None => self.score_i64(-1)?,
                };
                let connective = self.score_i64(i64::from(set.connective_used))?;
                let mut children = Vec::with_capacity(canonical.len() + 2);
                children.push(remainder);
                children.extend(canonical.iter().map(|child| self.par_score(*child)));
                children.push(connective);
                let score = self.score_constructor(Score::ESET, children)?;
                self.schema_expr_component(&canonical_shell, &canonical, score)
            }
            ExprInstance::EMapBody(map) => {
                exact("EMap", map.kvs.len() * 2, par_children.len())?;

                struct Pair {
                    key: PlannedPar,
                    value: PlannedPar,
                    protobuf: usize,
                }
                let pair_shell = crate::rhoapi::KeyValuePair {
                    key: Some(Par::default()),
                    value: Some(Par::default()),
                };
                let mut pairs = Vec::with_capacity(map.kvs.len());
                for children in par_children.chunks_exact(2) {
                    pairs.push(Pair {
                        key: children[0],
                        value: children[1],
                        protobuf: self.plan_protobuf(&pair_shell, children)?,
                    });
                }
                pairs.sort_by(|left, right| {
                    ScoreByteCursor::new(self, self.par_score(left.key))
                        .cmp(ScoreByteCursor::new(self, self.par_score(right.key)))
                        .then_with(|| {
                            WireByteCursor::new(self, WireRoot::Proto(left.protobuf))
                                .cmp(WireByteCursor::new(self, WireRoot::Proto(right.protobuf)))
                        })
                });
                let mut canonical: Vec<Pair> = Vec::with_capacity(pairs.len());
                for pair in pairs {
                    if let Some(previous) = canonical.last_mut() {
                        if self.compare_pars(previous.key, pair.key) == Ordering::Equal {
                            *previous = pair;
                            continue;
                        }
                    }
                    canonical.push(pair);
                }

                let mut canonical_shell = shell.clone();
                let ExprInstance::EMapBody(canonical_map) = canonical_shell
                    .expr_instance
                    .as_mut()
                    .expect("EMap shell retains its instance")
                else {
                    unreachable!("the exhaustive arm selected EMap")
                };
                canonical_map.kvs = (0..canonical.len()).map(|_| pair_shell.clone()).collect();

                let remainder = match &map.remainder {
                    Some(var) => self.score_tree(&sort_var(var).score)?,
                    None => self.score_i64(-1)?,
                };
                let connective = self.score_i64(i64::from(map.connective_used))?;
                let mut scores = Vec::with_capacity(canonical.len() + 2);
                scores.push(remainder);
                scores.extend(canonical.iter().map(|pair| self.par_score(pair.key)));
                scores.push(connective);
                let score = self.score_constructor(Score::EMAP, scores)?;
                let wire_children: Vec<_> = canonical
                    .iter()
                    .flat_map(|pair| [pair.key, pair.value])
                    .collect();
                self.schema_expr_component(&canonical_shell, &wire_children, score)
            }
            ExprInstance::EPathmapBody(pathmap) => {
                let (planned_map, map_protobuf) = self.planned_epathmap(pathmap, par_children)?;
                let map_node = &self.epathmaps[planned_map.0];
                let snapshot_score = self.score_wire_bytes(
                    WireRoot::EPathMapSnapshot(planned_map),
                    map_node.snapshot_len,
                    map_node.snapshot_zero_count,
                )?;
                let connective = self.score_i64(i64::from(pathmap.connective_used))?;
                let score =
                    self.score_constructor(Score::EPATHMAP, [snapshot_score, connective])?;
                let expr_protobuf = self.wrap_proto_node(32, map_protobuf)?;
                let stable = pathmap.remainder.is_none()
                    && pathmap.locally_free.is_empty()
                    && !pathmap.connective_used
                    && self.epathmaps[planned_map.0].entries_stable;
                Ok(self.component_from_proto(
                    ParComponentTag::Expr,
                    score,
                    expr_protobuf,
                    stable,
                    stable.then_some(CanonicalSegment::EPathMap(planned_map)),
                ))
            }
            ExprInstance::EZipperBody(zipper) => {
                let Some(pathmap) = zipper.pathmap.as_ref() else {
                    exact("EZipper without PathMap", 0, par_children.len())?;
                    let score = ExprSortMatcher::sort_match(shell).score;
                    let score = self.score_tree(&score)?;
                    return self.schema_expr_component(shell, &[], score);
                };
                let (planned_map, map_protobuf) = self.planned_epathmap(pathmap, par_children)?;
                let map_node = &self.epathmaps[planned_map.0];
                let snapshot_score = self.score_wire_bytes(
                    WireRoot::EPathMapSnapshot(planned_map),
                    map_node.snapshot_len,
                    map_node.snapshot_zero_count,
                )?;
                let connective = self.score_i64(i64::from(zipper.connective_used))?;
                let score =
                    self.score_constructor(Score::EPATHMAP + 1, [snapshot_score, connective])?;

                let tail = EZipper {
                    pathmap: None,
                    current_path: zipper.current_path.clone(),
                    is_write_zipper: zipper.is_write_zipper,
                    locally_free: zipper.locally_free.clone(),
                    connective_used: zipper.connective_used,
                    cursor_kind: zipper.cursor_kind,
                };
                let tail = self.plan_protobuf(&tail, &[])?;
                let mut pieces = vec![ProtoPiece::Node {
                    tag: 1,
                    node: map_protobuf,
                }];
                pieces.append(&mut self.protobuf_nodes[tail].pieces);
                let zipper_protobuf = self.push_proto_node(pieces)?;
                let expr_protobuf = self.wrap_proto_node(33, zipper_protobuf)?;
                Ok(self.component_from_proto(
                    ParComponentTag::Expr,
                    score,
                    expr_protobuf,
                    false,
                    None,
                ))
            }
            ExprInstance::ENegBody(_)
            | ExprInstance::ENotBody(_)
            | ExprInstance::EMultBody(_)
            | ExprInstance::EDivBody(_)
            | ExprInstance::EModBody(_)
            | ExprInstance::EPlusBody(_)
            | ExprInstance::EMinusBody(_)
            | ExprInstance::ELtBody(_)
            | ExprInstance::ELteBody(_)
            | ExprInstance::EGtBody(_)
            | ExprInstance::EGteBody(_)
            | ExprInstance::EEqBody(_)
            | ExprInstance::ENeqBody(_)
            | ExprInstance::EAndBody(_)
            | ExprInstance::EOrBody(_)
            | ExprInstance::EMatchesBody(_)
            | ExprInstance::EPercentPercentBody(_)
            | ExprInstance::EPlusPlusBody(_)
            | ExprInstance::EMinusMinusBody(_) => unreachable!("handled by arity class above"),
        }
    }

    /// Plan a canonical `Par` from top-level component occurrences.
    ///
    /// A multi-component value is ordered and coalesced by its exact eventual
    /// RPB3 key. The singleton fast path performs no body materialization,
    /// which is the critical skewed-recursion case.
    pub fn par(
        &mut self,
        occurrences: impl IntoIterator<Item = (PlannedComponent, Multiplicity)>,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        let mut occurrences: Vec<Occurrence> = occurrences
            .into_iter()
            .map(|(component, multiplicity)| Occurrence {
                component,
                multiplicity,
            })
            .collect();

        if occurrences.len() > 1 {
            occurrences
                .sort_by(|left, right| self.compare_components(left.component, right.component));
            let mut coalesced: Vec<Occurrence> = Vec::with_capacity(occurrences.len());
            for occurrence in occurrences {
                if let Some(previous) = coalesced.last_mut() {
                    if self.compare_components(previous.component, occurrence.component)
                        == Ordering::Equal
                    {
                        previous.multiplicity =
                            previous.multiplicity.checked_add(occurrence.multiplicity)?;
                        continue;
                    }
                }
                coalesced.push(occurrence);
            }
            occurrences = coalesced;
        }

        let mut score_pieces = Vec::new();
        for tag in [
            ParComponentTag::Send,
            ParComponentTag::Receive,
            ParComponentTag::Expr,
            ParComponentTag::New,
            ParComponentTag::Match,
            ParComponentTag::Bundle,
            ParComponentTag::Connective,
            ParComponentTag::GUnforgeable,
            ParComponentTag::If,
        ] {
            for occurrence in &occurrences {
                let component = &self.components[occurrence.component.0];
                if component.tag == tag {
                    score_pieces.push(ScorePiece::Repeat {
                        child: component.score,
                        count: occurrence.multiplicity.get(),
                    });
                }
            }
        }
        let connective_score = self.score_i64(i64::from(connective_used))?;
        score_pieces.push(ScorePiece::Child(connective_score));
        let score = self.score_node_with_pieces(Score::PAR, score_pieces)?;

        let mut protobuf_len = 0usize;
        let mut protobuf_zero_count = 0usize;
        for occurrence in &occurrences {
            let component = &self.components[occurrence.component.0];
            let body_len = self.protobuf_nodes[component.protobuf].encoded_len;
            let prefix =
                InlineWireBytes::length_delimited_prefix(component.tag.byte().into(), body_len);
            let one = key_len(component.tag.byte().into())
                .checked_add(encoded_len_varint(body_len as u64))
                .and_then(|n| n.checked_add(body_len))
                .ok_or(ParConstructionError::LengthOverflow("Par component"))?;
            let family = one
                .checked_mul(usize::try_from(occurrence.multiplicity.get()).map_err(|_| {
                    ParConstructionError::LengthOverflow("Par component multiplicity")
                })?)
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par component multiplicity",
                ))?;
            protobuf_len = protobuf_len
                .checked_add(family)
                .ok_or(ParConstructionError::LengthOverflow("Par protobuf body"))?;
            let one_zeroes = prefix
                .zero_count()
                .checked_add(self.protobuf_nodes[component.protobuf].zero_count)
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par protobuf zero-byte count",
                ))?;
            protobuf_zero_count = protobuf_zero_count
                .checked_add(
                    one_zeroes
                        .checked_mul(usize::try_from(occurrence.multiplicity.get()).map_err(
                            |_| ParConstructionError::LengthOverflow("Par component multiplicity"),
                        )?)
                        .ok_or(ParConstructionError::LengthOverflow(
                            "Par protobuf zero-byte count",
                        ))?,
                )
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par protobuf zero-byte count",
                ))?;
        }
        if !locally_free.is_empty() {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bytes::encoded_len(9, &locally_free))
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par locally-free field",
                ))?;
            let prefix = InlineWireBytes::length_delimited_prefix(9, locally_free.len());
            protobuf_zero_count = protobuf_zero_count
                .checked_add(prefix.zero_count())
                .and_then(|n| n.checked_add(locally_free.iter().filter(|&&byte| byte == 0).count()))
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par protobuf zero-byte count",
                ))?;
        }
        if connective_used {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bool::encoded_len(10, &true))
                .ok_or(ParConstructionError::LengthOverflow("Par connective flag"))?;
        }

        let singleton = (occurrences.len() == 1
            && occurrences[0].multiplicity == Multiplicity::ONE)
            .then(|| &self.components[occurrences[0].component.0]);
        let canonical_path_stable = locally_free.is_empty()
            && !connective_used
            && singleton.is_some_and(|component| {
                matches!(
                    component.tag,
                    ParComponentTag::Expr | ParComponentTag::GUnforgeable
                ) && component.canonical_path_stable
            });
        let split_children = if locally_free.is_empty() && !connective_used {
            singleton.and_then(|component| match &component.canonical_segment {
                Some(CanonicalSegment::List(children)) => Some(children.clone()),
                _ => None,
            })
        } else {
            None
        };

        let id = PlannedPar(self.pars.len());
        self.pars.push(ParNode {
            occurrences,
            locally_free,
            connective_used,
            score,
            protobuf_len,
            protobuf_zero_count,
            canonical_path_stable,
            split_children,
            canonical_path_override: None,
            canonical_segment_override: None,
        });
        Ok(id)
    }

    pub fn empty_par(&mut self) -> Result<PlannedPar, ParConstructionError> {
        self.par(std::iter::empty(), Vec::new(), false)
    }

    /// Bag union of two planned parallel compositions. Component paths remain
    /// arena references; only the bounded occurrence descriptors and root
    /// summaries are merged here.
    pub fn parallel(
        &mut self,
        left: PlannedPar,
        right: PlannedPar,
    ) -> Result<PlannedPar, ParConstructionError> {
        let mut occurrences = Vec::with_capacity(
            self.pars[left.0].occurrences.len() + self.pars[right.0].occurrences.len(),
        );
        occurrences.extend(
            self.pars[left.0]
                .occurrences
                .iter()
                .map(|occurrence| (occurrence.component, occurrence.multiplicity)),
        );
        occurrences.extend(
            self.pars[right.0]
                .occurrences
                .iter()
                .map(|occurrence| (occurrence.component, occurrence.multiplicity)),
        );
        let locally_free = super::utils::union(
            self.pars[left.0].locally_free.clone(),
            self.pars[right.0].locally_free.clone(),
        );
        self.par(
            occurrences,
            locally_free,
            self.pars[left.0].connective_used || self.pars[right.0].connective_used,
        )
    }

    /// Import an already-authoritative resident `Par` without decoding any
    /// component or traversing any nested AST.
    ///
    /// The PathMap zipper lends each RPB3 score/body pair exactly once. Those
    /// canonical fragments become leaves in this transient arena, so a later
    /// planned parent can reference the imported value without rebuilding it.
    pub fn import_par(&mut self, par: &Par) -> Result<PlannedPar, ParConstructionError> {
        let (canonical_path, singleton_stable) =
            super::canonical_path::encode_trie_path_with_stability(par);
        let canonical_segment = super::canonical_path::encode_trie_segment(par);
        let mut imported = Vec::with_capacity(par.components().distinct_len());
        par.components()
            .visit_canonical_key_parts(|tag, score, protobuf, multiplicity| {
                imported.push((
                    tag,
                    score.to_vec().into_boxed_slice(),
                    protobuf.to_vec().into_boxed_slice(),
                    multiplicity,
                ));
            });

        let imported_is_singleton = imported.len() == 1;
        let mut occurrences = Vec::with_capacity(imported.len());
        for (tag, score, protobuf, multiplicity) in imported {
            let score = self.score_from_encoded(&score)?;
            let protobuf_id = self.protobuf_nodes.len();
            let encoded_len = protobuf.len();
            let zero_count = protobuf.iter().filter(|&&byte| byte == 0).count();
            self.protobuf_nodes.push(ProtoNode {
                pieces: if protobuf.is_empty() {
                    Vec::new()
                } else {
                    vec![ProtoPiece::Bytes(protobuf)]
                },
                encoded_len,
                zero_count,
            });
            let component = PlannedComponent(self.components.len());
            self.components.push(ComponentNode {
                tag,
                score,
                protobuf: protobuf_id,
                canonical_path_stable: singleton_stable && imported_is_singleton,
                canonical_segment: None,
            });
            occurrences.push((component, multiplicity));
        }
        let planned = self.par(
            occurrences,
            par.locally_free().to_vec(),
            par.connective_used(),
        )?;
        self.pars[planned.0].canonical_path_stable = singleton_stable;
        self.pars[planned.0].canonical_path_override = Some(canonical_path.into_boxed_slice());
        self.pars[planned.0].canonical_segment_override =
            Some(canonical_segment.into_boxed_slice());
        Ok(planned)
    }

    /// Emit the root planned `Par` into its sole resident representation.
    pub fn finish(&self, root: PlannedPar) -> Result<Par, ParConstructionError> {
        let planned = &self.pars[root.0];
        let mut components = ParBag::new();
        for occurrence in &planned.occurrences {
            let component = &self.components[occurrence.component.0];
            let key = self.component_key(occurrence.component)?;
            components.insert_planned_key(
                component.tag,
                &key,
                occurrence.multiplicity,
                component.canonical_path_stable,
            )?;
        }
        Ok(Par::from_parts(
            components,
            planned.locally_free.clone(),
            planned.connective_used,
        ))
    }

    /// Emit the canonical protobuf body of a planned `Par` without first
    /// constructing any nested resident `Par`.
    pub fn protobuf_bytes(&self, root: PlannedPar) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pars[root.0].protobuf_len);
        self.emit_par(root, &mut out);
        debug_assert_eq!(out.len(), self.pars[root.0].protobuf_len);
        out
    }

    fn component_key(&self, component: PlannedComponent) -> Result<Vec<u8>, ParConstructionError> {
        let component = &self.components[component.0];
        let score_len = self.scores[component.score.0].encoded_len;
        let body_len = self.protobuf_nodes[component.protobuf].encoded_len;
        let capacity = PAR_BAG_KEY_DOMAIN
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_add(score_len))
            .and_then(|n| n.checked_add(body_len))
            .and_then(|n| n.checked_add(std::mem::size_of::<u64>()))
            .ok_or(ParConstructionError::LengthOverflow("RPB3 component key"))?;
        let mut out = Vec::with_capacity(capacity);
        out.extend_from_slice(&PAR_BAG_KEY_DOMAIN);
        out.push(component.tag.byte());
        self.emit_score(component.score, &mut out);
        self.emit_proto(component.protobuf, &mut out);
        let score_len = u64::try_from(score_len)
            .map_err(|_| ParConstructionError::LengthOverflow("RPB3 score trailer"))?;
        out.extend_from_slice(&score_len.to_be_bytes());
        debug_assert_eq!(out.len(), capacity);
        Ok(out)
    }

    /// Compare exact eventual RPB3 keys without constructing either key.
    /// Both the prefix-free score and canonical protobuf tie-break are lazy
    /// cursors over arena fragments.
    fn compare_components(&self, left: PlannedComponent, right: PlannedComponent) -> Ordering {
        let left = &self.components[left.0];
        let right = &self.components[right.0];
        left.tag
            .cmp(&right.tag)
            .then_with(|| {
                ScoreByteCursor::new(self, left.score).cmp(ScoreByteCursor::new(self, right.score))
            })
            .then_with(|| {
                WireByteCursor::new(self, WireRoot::Proto(left.protobuf))
                    .cmp(WireByteCursor::new(self, WireRoot::Proto(right.protobuf)))
            })
    }

    fn emit_score(&self, root: PlannedScore, out: &mut Vec<u8>) {
        enum Op {
            Score(PlannedScore),
            Bytes(usize),
            Repeat { child: PlannedScore, remaining: u64 },
            EscapedWire(WireRoot),
        }
        let mut work = vec![Op::Score(root)];
        while let Some(op) = work.pop() {
            match op {
                Op::Score(score) => {
                    for piece in self.scores[score.0].pieces.iter().rev() {
                        work.push(match *piece {
                            ScorePiece::Bytes(bytes) => Op::Bytes(bytes),
                            ScorePiece::Child(child) => Op::Score(child),
                            ScorePiece::Repeat { child, count } => Op::Repeat {
                                child,
                                remaining: count,
                            },
                            ScorePiece::EscapedWire { root, .. } => Op::EscapedWire(root),
                        });
                    }
                }
                Op::Bytes(bytes) => out.extend_from_slice(&self.byte_fragments[bytes]),
                Op::Repeat { child, remaining } => {
                    if remaining != 0 {
                        if remaining != 1 {
                            work.push(Op::Repeat {
                                child,
                                remaining: remaining - 1,
                            });
                        }
                        work.push(Op::Score(child));
                    }
                }
                Op::EscapedWire(root) => {
                    for byte in WireByteCursor::new(self, root) {
                        out.push(byte);
                        if byte == 0 {
                            out.push(0xff);
                        }
                    }
                }
            }
        }
    }

    fn emit_par(&self, root: PlannedPar, out: &mut Vec<u8>) {
        self.emit_wire(WireRoot::Par(root), out);
    }

    fn emit_proto(&self, root: usize, out: &mut Vec<u8>) {
        self.emit_wire(WireRoot::Proto(root), out);
    }

    /// The one wire-emission PDA shared by root `Par` serialization and by the
    /// component-body suffix of an RPB3 key.  A nested planned `Par` pushes the
    /// same `Par` operation; it never calls `protobuf_bytes` recursively and
    /// never allocates an intermediate child body.
    fn emit_wire(&self, root: WireRoot, out: &mut Vec<u8>) {
        out.extend(WireByteCursor::new(self, root));
    }

    fn plan_protobuf<T: ProtobufNode>(
        &mut self,
        shell: &T,
        par_children: &[PlannedPar],
    ) -> Result<usize, ParConstructionError> {
        let start = self.protobuf_nodes.len();
        let root = start;
        self.protobuf_nodes.push(ProtoNode {
            pieces: Vec::new(),
            encoded_len: 0,
            zero_count: 0,
        });
        let mut work = vec![ProtoBuildOp::Node {
            node: shell,
            from: 0,
            target: root,
        }];
        let mut child_cursor = 0usize;

        while let Some(op) = work.pop() {
            let (node, from, target) = match op {
                ProtoBuildOp::Node { node, from, target } => (node, from, target),
                ProtoBuildOp::Seq {
                    seq,
                    index,
                    len,
                    tag,
                    target,
                } => {
                    if index >= len {
                        continue;
                    }
                    if index + 1 < len {
                        work.push(ProtoBuildOp::Seq {
                            seq,
                            index: index + 1,
                            len,
                            tag,
                            target,
                        });
                    }
                    let child = seq.protobuf_get(index);
                    self.plan_child(
                        child,
                        tag,
                        target,
                        par_children,
                        &mut child_cursor,
                        &mut work,
                    )?;
                    continue;
                }
            };

            let mut bounded = Vec::new();
            let descent = node.protobuf_emit(from as usize, &mut bounded);
            if !bounded.is_empty() {
                self.protobuf_nodes[target]
                    .pieces
                    .push(ProtoPiece::Bytes(bounded.into_boxed_slice()));
            }
            match descent {
                ProtobufDescent::Done => {}
                ProtobufDescent::Node {
                    resume,
                    tag,
                    node: child,
                } => {
                    if resume != NO_RESUME {
                        work.push(ProtoBuildOp::Node {
                            node,
                            from: resume,
                            target,
                        });
                    }
                    self.plan_child(
                        child,
                        tag,
                        target,
                        par_children,
                        &mut child_cursor,
                        &mut work,
                    )?;
                }
                ProtobufDescent::Seq {
                    resume,
                    tag,
                    len,
                    seq,
                } => {
                    if resume != NO_RESUME {
                        work.push(ProtoBuildOp::Node {
                            node,
                            from: resume,
                            target,
                        });
                    }
                    work.push(ProtoBuildOp::Seq {
                        seq,
                        index: 0,
                        len,
                        tag,
                        target,
                    });
                }
                ProtobufDescent::Map { resume, tag, map } => {
                    if resume != NO_RESUME {
                        work.push(ProtoBuildOp::Node {
                            node,
                            from: resume,
                            target,
                        });
                    }
                    for (key, _placeholder) in map {
                        let par = *par_children.get(child_cursor).ok_or(
                            ParConstructionError::MissingParSlot {
                                expected_at_least: child_cursor + 1,
                                supplied: par_children.len(),
                            },
                        )?;
                        child_cursor += 1;
                        let entry = self.protobuf_nodes.len();
                        let mut pieces = Vec::new();
                        if !key.is_empty() {
                            let mut bytes = Vec::new();
                            prost::encoding::string::encode(1, key, &mut bytes);
                            pieces.push(ProtoPiece::Bytes(bytes.into_boxed_slice()));
                        }
                        if !self.pars[par.0].occurrences.is_empty()
                            || self.pars[par.0].connective_used
                        {
                            pieces.push(ProtoPiece::Par { tag: 2, par });
                        }
                        self.protobuf_nodes.push(ProtoNode {
                            pieces,
                            encoded_len: 0,
                            zero_count: 0,
                        });
                        self.protobuf_nodes[target]
                            .pieces
                            .push(ProtoPiece::Node { tag, node: entry });
                    }
                }
                ProtobufDescent::EPathMapSnapshot { .. } => unreachable!(
                    "ParConstruction: EPathMap is intercepted as one opaque generated-PDA body"
                ),
            }
        }

        if child_cursor != par_children.len() {
            return Err(ParConstructionError::ExtraParSlots {
                consumed: child_cursor,
                supplied: par_children.len(),
            });
        }

        for id in (start..self.protobuf_nodes.len()).rev() {
            let mut len = 0usize;
            let mut zero_count = 0usize;
            for piece in &self.protobuf_nodes[id].pieces {
                let (add, add_zeroes) = match piece {
                    ProtoPiece::Bytes(bytes) => {
                        (bytes.len(), bytes.iter().filter(|&&byte| byte == 0).count())
                    }
                    ProtoPiece::Node { tag, node } => {
                        let child = self.protobuf_nodes[*node].encoded_len;
                        let add = key_len(*tag)
                            .checked_add(encoded_len_varint(child as u64))
                            .and_then(|n| n.checked_add(child))
                            .ok_or(ParConstructionError::LengthOverflow(
                                "nested protobuf message",
                            ))?;
                        let prefix = InlineWireBytes::length_delimited_prefix(*tag, child);
                        (
                            add,
                            prefix.zero_count() + self.protobuf_nodes[*node].zero_count,
                        )
                    }
                    ProtoPiece::Par { tag, par } => {
                        let child = self.pars[par.0].protobuf_len;
                        let add = key_len(*tag)
                            .checked_add(encoded_len_varint(child as u64))
                            .and_then(|n| n.checked_add(child))
                            .ok_or(ParConstructionError::LengthOverflow("nested planned Par"))?;
                        let prefix = InlineWireBytes::length_delimited_prefix(*tag, child);
                        (
                            add,
                            prefix.zero_count() + self.pars[par.0].protobuf_zero_count,
                        )
                    }
                    ProtoPiece::EPathMapSnapshot { tag, map } => {
                        let child = self.epathmaps[map.0].snapshot_len;
                        let add = key_len(*tag)
                            .checked_add(encoded_len_varint(child as u64))
                            .and_then(|n| n.checked_add(child))
                            .ok_or(ParConstructionError::LengthOverflow(
                                "planned EPathMap snapshot",
                            ))?;
                        let prefix = InlineWireBytes::length_delimited_prefix(*tag, child);
                        (
                            add,
                            prefix.zero_count() + self.epathmaps[map.0].snapshot_zero_count,
                        )
                    }
                };
                len = len
                    .checked_add(add)
                    .ok_or(ParConstructionError::LengthOverflow("protobuf node"))?;
                zero_count = zero_count.checked_add(add_zeroes).ok_or(
                    ParConstructionError::LengthOverflow("protobuf zero-byte count"),
                )?;
            }
            self.protobuf_nodes[id].encoded_len = len;
            self.protobuf_nodes[id].zero_count = zero_count;
        }
        Ok(root)
    }

    fn plan_child<'a>(
        &mut self,
        child: &'a dyn ProtobufNode,
        tag: u32,
        target: usize,
        par_children: &[PlannedPar],
        child_cursor: &mut usize,
        work: &mut Vec<ProtoBuildOp<'a>>,
    ) -> Result<(), ParConstructionError> {
        if child.as_par().is_some() {
            let par =
                *par_children
                    .get(*child_cursor)
                    .ok_or(ParConstructionError::MissingParSlot {
                        expected_at_least: *child_cursor + 1,
                        supplied: par_children.len(),
                    })?;
            *child_cursor += 1;
            self.protobuf_nodes[target]
                .pieces
                .push(ProtoPiece::Par { tag, par });
            return Ok(());
        }
        if let Some(map) = child.as_epathmap() {
            let bytes = protobuf_encoder::encode_to_vec(map);
            let node = self.protobuf_nodes.len();
            let zero_count = bytes.iter().filter(|&&byte| byte == 0).count();
            self.protobuf_nodes.push(ProtoNode {
                encoded_len: bytes.len(),
                pieces: if bytes.is_empty() {
                    Vec::new()
                } else {
                    vec![ProtoPiece::Bytes(bytes.into_boxed_slice())]
                },
                zero_count,
            });
            self.protobuf_nodes[target]
                .pieces
                .push(ProtoPiece::Node { tag, node });
            return Ok(());
        }
        let node = self.protobuf_nodes.len();
        self.protobuf_nodes.push(ProtoNode {
            pieces: Vec::new(),
            encoded_len: 0,
            zero_count: 0,
        });
        self.protobuf_nodes[target]
            .pieces
            .push(ProtoPiece::Node { tag, node });
        work.push(ProtoBuildOp::Node {
            node: child,
            from: 0,
            target: node,
        });
        Ok(())
    }
}

impl Drop for ParConstruction {
    fn drop(&mut self) {
        // All edges are integer indices. Explicitly clearing the largest
        // fragment owners first keeps teardown order deterministic and makes
        // the non-recursive nature evident to stack-slope instrumentation.
        self.components.clear();
        self.pars.clear();
        self.epathmaps.clear();
        self.protobuf_nodes.clear();
        self.scores.clear();
        self.byte_fragments.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rhoapi::connective::ConnectiveInstance;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, EMap, EMethod, ENot, EPathMap, EPlus, ESet, ETuple,
        EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, Match, MatchCase, New, Receive,
        ReceiveBind, Send,
    };
    use crate::rust::par_bag::with_component_key;

    fn planned_gint(
        arena: &mut ParConstruction,
        value: i64,
    ) -> Result<PlannedPar, ParConstructionError> {
        let expr = Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        };
        let component = arena.expr_component(&expr, &[], true)?;
        arena.par([(component, Multiplicity::ONE)], Vec::new(), false)
    }

    fn planned_enot(
        arena: &mut ParConstruction,
        child: PlannedPar,
    ) -> Result<PlannedPar, ParConstructionError> {
        let expr = Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(Par::default()),
            })),
        };
        let component = arena.expr_component(&expr, &[child], false)?;
        arena.par([(component, Multiplicity::ONE)], Vec::new(), false)
    }

    fn resident_gint(value: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])
    }

    #[test]
    fn schema_plan_is_byte_identical_to_repeated_enot_construction() {
        for depth in [0usize, 1, 4, 32, 128] {
            let mut arena = ParConstruction::new();
            let mut planned = planned_gint(&mut arena, 7).expect("planned integer");
            for _ in 0..depth {
                planned = planned_enot(&mut arena, planned).expect("planned ENot");
            }

            let mut expected = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            }]);
            for _ in 0..depth {
                expected = Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(expected) })),
                }]);
            }

            let actual = arena.finish(planned).expect("finish plan");
            assert_eq!(actual, expected);
            assert_eq!(
                arena.protobuf_bytes(planned),
                expected.protobuf_bytes().unwrap()
            );
            let expected_expr = expected.first::<Expr>().unwrap().unwrap().0;
            let actual_expr = actual.first::<Expr>().unwrap().unwrap().0;
            assert_eq!(
                with_component_key(&actual_expr, <[u8]>::to_vec),
                with_component_key(&expected_expr, <[u8]>::to_vec)
            );
        }
    }

    #[test]
    fn ordered_expr_score_and_wire_permutations_match_the_production_sorter() {
        let mut arena = ParConstruction::new();
        let one = planned_gint(&mut arena, 1).unwrap();
        let two = planned_gint(&mut arena, 2).unwrap();

        let plus_shell = Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(Par::default()),
                p2: Some(Par::default()),
            })),
        };
        let plus = arena
            .expr_component(&plus_shell, &[one, two], false)
            .unwrap();
        let plus = arena
            .par([(plus, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();

        let tuple_shell = Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![Par::default(), Par::default()],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        };
        let tuple = arena
            .expr_component(&tuple_shell, &[plus, two], false)
            .unwrap();
        let tuple = arena
            .par([(tuple, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();

        let method_shell = Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "choose".to_owned(),
                target: Some(Par::default()),
                arguments: vec![Par::default(), Par::default()],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        };
        // Sorter evaluation order is arguments first, target last. Protobuf
        // order is target first; `expr_component` owns that permutation.
        let method = arena
            .expr_component(&method_shell, &[one, tuple, plus], false)
            .unwrap();
        let root = arena
            .par([(method, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let actual = arena.finish(root).unwrap();

        let one = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }]);
        let two = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        }]);
        let plus = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(one.clone()),
                p2: Some(two.clone()),
            })),
        }]);
        let tuple = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![plus.clone(), two],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let expected = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                method_name: "choose".to_owned(),
                target: Some(plus),
                arguments: vec![one, tuple],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);

        assert_eq!(actual, expected);
        assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    }

    #[test]
    fn ac_expr_plans_match_set_deduplication_and_map_collision_winners() {
        let mut arena = ParConstruction::new();
        let one = planned_gint(&mut arena, 1).unwrap();
        let two = planned_gint(&mut arena, 2).unwrap();
        let three = planned_gint(&mut arena, 3).unwrap();

        let set_shell = Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![Par::default(); 3],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let set = arena
            .expr_component(&set_shell, &[two, one, two], false)
            .unwrap();
        let set = arena
            .par([(set, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();

        let map_shell = Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: vec![KeyValuePair::default(); 3],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let map = arena
            .expr_component(&map_shell, &[one, two, one, three, two, set], false)
            .unwrap();
        let root = arena
            .par([(map, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let actual = arena.finish(root).unwrap();

        let one = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }]);
        let two = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(2)),
        }]);
        let three = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(3)),
        }]);
        let set = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![two.clone(), one.clone(), two.clone()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);
        let expected = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(one.clone()),
                        value: Some(two.clone()),
                    },
                    KeyValuePair {
                        key: Some(one),
                        value: Some(three),
                    },
                    KeyValuePair {
                        key: Some(two),
                        value: Some(set),
                    },
                ],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);

        assert_eq!(actual, expected);
        assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    }

    #[test]
    fn every_top_level_component_plan_matches_production_canonicalization() {
        fn assert_exact(arena: &ParConstruction, planned: PlannedPar, expected: &Par) {
            let actual = arena.finish(planned).unwrap();
            assert_eq!(&actual, expected);
            assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
            assert_eq!(
                arena.protobuf_bytes(planned),
                expected.protobuf_bytes().unwrap()
            );
        }

        let one_r = resident_gint(1);
        let two_r = resident_gint(2);
        let three_r = resident_gint(3);
        let empty_r = Par::default();
        let mut arena = ParConstruction::new();
        let one = planned_gint(&mut arena, 1).unwrap();
        let two = planned_gint(&mut arena, 2).unwrap();
        let three = planned_gint(&mut arena, 3).unwrap();
        let empty = arena.empty_par().unwrap();

        let send_shell = Send {
            chan: Some(Par::default()),
            data: vec![Par::default(), Par::default()],
            persistent: true,
            locally_free: vec![1],
            connective_used: true,
        };
        let send = arena
            .send_component(&send_shell, &[one, two, three])
            .unwrap();
        let send = arena
            .par([(send, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            send,
            &Par::default().with_sends(vec![Send {
                chan: Some(one_r.clone()),
                data: vec![two_r.clone(), three_r.clone()],
                ..send_shell
            }]),
        );

        let bundle_shell = Bundle {
            body: Some(Par::default()),
            write_flag: true,
            read_flag: false,
        };
        let bundle = arena.bundle_component(&bundle_shell, &[one]).unwrap();
        let bundle = arena
            .par([(bundle, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            bundle,
            &Par::default().with_bundles(vec![Bundle {
                body: Some(one_r.clone()),
                ..bundle_shell
            }]),
        );

        let if_shell = If {
            condition: Some(Par::default()),
            if_true: Some(Par::default()),
            if_false: Some(Par::default()),
            locally_free: vec![2],
            connective_used: true,
        };
        let conditional = arena.if_component(&if_shell, &[one, two, three]).unwrap();
        let conditional = arena
            .par([(conditional, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            conditional,
            &Par::default().with_conditionals(vec![If {
                condition: Some(one_r.clone()),
                if_true: Some(two_r.clone()),
                if_false: Some(three_r.clone()),
                ..if_shell
            }]),
        );

        let unforgeable_shell = GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: vec![0, 1, 2, 0xff],
            })),
        };
        let unforgeable = arena.unforgeable_component(&unforgeable_shell).unwrap();
        let unforgeable = arena
            .par([(unforgeable, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let expected_unforgeable =
            Par::default().with_unforgeables(vec![unforgeable_shell.clone()]);
        assert_exact(&arena, unforgeable, &expected_unforgeable);
        assert_eq!(
            arena.canonical_path_bytes(unforgeable).unwrap(),
            super::super::canonical_path::encode_trie_path(&expected_unforgeable)
        );

        let connective_shell = Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![Par::default(), Par::default()],
            })),
        };
        let connective = arena
            .connective_component(&connective_shell, &[one, two])
            .unwrap();
        let connective = arena
            .par([(connective, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            connective,
            &Par::default().with_connectives(vec![Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![one_r.clone(), two_r.clone()],
                })),
            }]),
        );

        let new_shell = New {
            bind_count: 2,
            p: Some(Par::default()),
            uri: vec!["z".to_owned(), "a".to_owned()],
            injections: BTreeMap::from([
                ("left".to_owned(), Par::default()),
                ("right".to_owned(), Par::default()),
            ]),
            locally_free: vec![3],
        };
        let new = arena.new_component(&new_shell, &[one, two, three]).unwrap();
        let new = arena
            .par([(new, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            new,
            &Par::default().with_news(vec![New {
                p: Some(one_r.clone()),
                injections: BTreeMap::from([
                    ("left".to_owned(), two_r.clone()),
                    ("right".to_owned(), three_r.clone()),
                ]),
                ..new_shell
            }]),
        );

        let match_shell = Match {
            target: Some(Par::default()),
            cases: vec![MatchCase {
                pattern: Some(Par::default()),
                source: Some(Par::default()),
                free_count: 1,
                guard: Some(Par::default()),
            }],
            locally_free: vec![4],
            connective_used: true,
        };
        let matched = arena
            .match_component(&match_shell, &[one, two, three, empty])
            .unwrap();
        let matched = arena
            .par([(matched, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            matched,
            &Par::default().with_matches(vec![Match {
                target: Some(one_r.clone()),
                cases: vec![MatchCase {
                    pattern: Some(two_r.clone()),
                    source: Some(three_r.clone()),
                    free_count: 1,
                    guard: None,
                }],
                ..match_shell
            }]),
        );

        let receive_shell = Receive {
            binds: vec![ReceiveBind {
                patterns: vec![Par::default(), Par::default()],
                source: Some(Par::default()),
                remainder: None,
                free_count: 2,
            }],
            body: Some(Par::default()),
            persistent: true,
            peek: true,
            bind_count: 2,
            locally_free: vec![5],
            connective_used: true,
            condition: Some(Par::default()),
        };
        let receive = arena
            .receive_component(&receive_shell, &[one, two, three, one, empty])
            .unwrap();
        let receive = arena
            .par([(receive, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        assert_exact(
            &arena,
            receive,
            &Par::default().with_receives(vec![Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![one_r.clone(), two_r.clone()],
                    source: Some(three_r),
                    remainder: None,
                    free_count: 2,
                }],
                body: Some(one_r),
                condition: None,
                ..receive_shell
            }]),
        );
        assert_eq!(empty_r, Par::default());
    }

    #[test]
    fn planned_epathmap_and_zipper_stream_exact_pathmap_snapshots() {
        let one_resident = resident_gint(1);
        let two_resident = resident_gint(2);
        let three_resident = resident_gint(3);

        let set_shell = EPathMap::from_set_iter(
            [two_resident.clone(), one_resident.clone()],
            Vec::new(),
            false,
            None,
        );
        let map_shell = EPathMap::new_map(
            [
                (one_resident.clone(), Par::default()),
                (two_resident.clone(), Par::default()),
            ],
            Vec::new(),
            false,
            None,
        );

        let mut arena = ParConstruction::new();
        let one = planned_gint(&mut arena, 1).unwrap();
        let two = planned_gint(&mut arena, 2).unwrap();
        let three = planned_gint(&mut arena, 3).unwrap();

        let set_expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(set_shell.clone())),
        };
        let set_component = arena.expr_component(&set_expr, &[one, two], true).unwrap();
        let set = arena
            .par([(set_component, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();

        let map_expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map_shell.clone())),
        };
        let map_component = arena
            .expr_component(&map_expr, &[one, set, two, three], true)
            .unwrap();
        let map = arena
            .par([(map_component, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();

        let expected_set = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::from_set_iter(
                [one_resident.clone(), two_resident.clone()],
                Vec::new(),
                false,
                None,
            ))),
        }]);
        let expected_map_value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [
                    (one_resident.clone(), expected_set.clone()),
                    (two_resident.clone(), three_resident.clone()),
                ],
                Vec::new(),
                false,
                None,
            ))),
        }]);
        let actual_map_value = arena.finish(map).unwrap();
        assert_eq!(actual_map_value, expected_map_value);
        assert_eq!(
            actual_map_value.compact_snapshot(),
            expected_map_value.compact_snapshot()
        );
        assert_eq!(
            arena.canonical_path_bytes(map).unwrap(),
            super::super::canonical_path::encode_trie_path(&expected_map_value)
        );

        let zipper = EZipper {
            pathmap: Some(map_shell),
            current_path: vec![vec![path_tag::GINT, 2]],
            is_write_zipper: true,
            locally_free: vec![1, 0, 1],
            connective_used: true,
            cursor_kind: 1,
        };
        let zipper_expr = Expr {
            expr_instance: Some(ExprInstance::EZipperBody(zipper.clone())),
        };
        let zipper_component = arena
            .expr_component(&zipper_expr, &[one, set, two, three], false)
            .unwrap();
        let zipper_par = arena
            .par([(zipper_component, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let actual_zipper = arena.finish(zipper_par).unwrap();
        let expected_zipper = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EZipperBody(EZipper {
                pathmap: Some(EPathMap::new_map(
                    [(one_resident, expected_set), (two_resident, three_resident)],
                    Vec::new(),
                    false,
                    None,
                )),
                ..zipper
            })),
        }]);
        assert_eq!(actual_zipper, expected_zipper);
        assert_eq!(
            actual_zipper.compact_snapshot(),
            expected_zipper.compact_snapshot()
        );
    }

    #[test]
    fn planned_epathmap_value_chain_is_stack_safe_and_linear_to_emit() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                const DEPTH: usize = 4_096;
                let key_resident = resident_gint(1);
                let shell =
                    EPathMap::new_map([(key_resident, Par::default())], Vec::new(), false, None);
                let expr_shell = Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(shell)),
                };

                let mut arena = ParConstruction::new();
                let key = planned_gint(&mut arena, 1).unwrap();
                let mut value = arena.empty_par().unwrap();
                for _ in 0..DEPTH {
                    let component = arena
                        .expr_component(&expr_shell, &[key, value], true)
                        .unwrap();
                    value = arena
                        .par([(component, Multiplicity::ONE)], Vec::new(), false)
                        .unwrap();
                }

                let expected_len = arena.pars[value.0].protobuf_len;
                let bytes = arena.protobuf_bytes(value);
                assert_eq!(bytes.len(), expected_len);
                let actual = arena.finish(value).unwrap();
                assert_eq!(actual.protobuf_bytes().unwrap(), bytes);
            })
            .expect("spawn fixed-stack EPathMap construction gate")
            .join()
            .expect("fixed-stack EPathMap construction gate");
    }

    #[test]
    fn all_recursive_component_families_share_the_fixed_stack_construction_pda() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                const DEPTH: usize = 8_192;
                let mut arena = ParConstruction::new();
                let empty = arena.empty_par().unwrap();
                let mut value = planned_gint(&mut arena, 0).unwrap();

                for level in 0..DEPTH {
                    let component = match level % 8 {
                        0 => arena
                            .send_component(
                                &Send {
                                    chan: Some(Par::default()),
                                    data: Vec::new(),
                                    persistent: false,
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                },
                                &[value],
                            )
                            .unwrap(),
                        1 => arena
                            .bundle_component(
                                &Bundle {
                                    body: Some(Par::default()),
                                    write_flag: true,
                                    read_flag: true,
                                },
                                &[value],
                            )
                            .unwrap(),
                        2 => arena
                            .new_component(
                                &New {
                                    bind_count: 0,
                                    p: Some(Par::default()),
                                    uri: Vec::new(),
                                    injections: BTreeMap::new(),
                                    locally_free: Vec::new(),
                                },
                                &[value],
                            )
                            .unwrap(),
                        3 => arena
                            .match_component(
                                &Match {
                                    target: Some(Par::default()),
                                    cases: Vec::new(),
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                },
                                &[value],
                            )
                            .unwrap(),
                        4 => arena
                            .connective_component(
                                &Connective {
                                    connective_instance: Some(ConnectiveInstance::ConnNotBody(
                                        Par::default(),
                                    )),
                                },
                                &[value],
                            )
                            .unwrap(),
                        5 => arena
                            .if_component(
                                &If {
                                    condition: Some(Par::default()),
                                    if_true: Some(Par::default()),
                                    if_false: Some(Par::default()),
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                },
                                &[value, empty, empty],
                            )
                            .unwrap(),
                        6 => arena
                            .receive_component(
                                &Receive {
                                    binds: Vec::new(),
                                    body: Some(Par::default()),
                                    persistent: false,
                                    peek: false,
                                    bind_count: 0,
                                    locally_free: Vec::new(),
                                    connective_used: false,
                                    condition: None,
                                },
                                &[value, empty],
                            )
                            .unwrap(),
                        _ => {
                            let expr = Expr {
                                expr_instance: Some(ExprInstance::ENotBody(ENot {
                                    p: Some(Par::default()),
                                })),
                            };
                            arena.expr_component(&expr, &[value], false).unwrap()
                        }
                    };
                    value = arena
                        .par([(component, Multiplicity::ONE)], Vec::new(), false)
                        .unwrap();
                }

                let bytes = arena.protobuf_bytes(value);
                let actual = arena.finish(value).unwrap();
                assert_eq!(actual.protobuf_bytes().unwrap(), bytes);
            })
            .expect("spawn fixed-stack all-family construction gate")
            .join()
            .expect("fixed-stack all-family construction gate");
    }

    #[test]
    fn resident_import_preserves_multiplicity_metadata_and_exact_trie_image() {
        let source = Par::default()
            .with_exprs(vec![
                Expr {
                    expr_instance: Some(ExprInstance::GString("alpha".to_owned())),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(7)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(7)),
                },
            ])
            .with_locally_free(vec![1, 0, 1])
            .with_connective_used(true);

        let mut arena = ParConstruction::new();
        let planned = arena.import_par(&source).expect("import resident Par");
        let actual = arena.finish(planned).expect("finish imported Par");

        assert_eq!(actual, source);
        assert_eq!(actual.compact_snapshot(), source.compact_snapshot());
        assert_eq!(
            arena.protobuf_bytes(planned),
            source.protobuf_bytes().unwrap()
        );
    }

    #[test]
    fn planned_parallel_uses_bag_addition_without_flattening_component_keys() {
        let left_value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let right_value = Par::default().with_exprs(vec![
            Expr {
                expr_instance: Some(ExprInstance::GString("alpha".to_owned())),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            },
        ]);
        let expected_order = left_value.cmp(&right_value);
        let expected = left_value.clone().parallel(right_value.clone()).unwrap();

        let mut arena = ParConstruction::new();
        let left = arena.import_par(&left_value).unwrap();
        let right = arena.import_par(&right_value).unwrap();
        assert_eq!(arena.compare_pars(left, right), expected_order);
        let root = arena.parallel(left, right).unwrap();
        let actual = arena.finish(root).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    }

    #[test]
    fn schema_plan_constructs_depth_16384_on_a_256_kib_stack() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                const DEPTH: usize = 16_384;
                let mut arena = ParConstruction::new();
                let mut planned = arena.empty_par().expect("planned empty Par");
                for _ in 0..DEPTH {
                    planned = planned_enot(&mut arena, planned).expect("planned ENot");
                }

                let planned_bytes = arena.protobuf_bytes(planned);
                let actual = arena.finish(planned).expect("finish deep plan");
                assert_eq!(actual.protobuf_bytes().unwrap(), planned_bytes);

                let mut observed_depth = None;
                actual.visit_encoded::<Expr>(|entry| {
                    observed_depth = entry.canonical_unary_enot_depth();
                });
                assert_eq!(observed_depth, Some(DEPTH));
            })
            .expect("spawn fixed-stack construction gate")
            .join()
            .expect("fixed-stack construction gate");
    }
}
