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
//! and final emission is a heap worklist.  EPathMap remains an opaque EPM1
//! endpoint, so planning never projects a trie into entries.
//!
//! Canonical score keys use the same prefix-free byte language as the sorter,
//! represented as another flat arena.  Parent scores reference child score
//! nodes by index; no recursive `Tree` is cloned while a construction grows.

use std::cmp::Ordering;
use std::fmt;

use prost::encoding::{encoded_len_varint, key_len};

use super::par_bag::{Multiplicity, ParBag, ParBagComponent, ParComponentTag, PAR_BAG_KEY_DOMAIN};
use super::rholang::protobuf_encoder;
use super::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode, ProtobufSeq, NO_RESUME};
use super::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use super::rholang::sorter::score_tree::{encode_order_key, Score, ScoreAtom, Tree};
use super::rholang::sorter::sort_combine::sort_var;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{Expr, Par};

/// An index into a [`ParConstruction`]'s planned-Par arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedPar(usize);

/// An index into the canonical-score fragment arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedScore(usize);

/// An index into the planned-component arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedComponent(usize);

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
    UnsupportedPlannedShape(&'static str),
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
            Self::UnsupportedPlannedShape(shape) => {
                write!(formatter, "planned construction does not yet support {shape}")
            }
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
}

enum ScoreCursorOp {
    Score(PlannedScore),
    Bytes { fragment: usize, offset: usize },
    Repeat { child: PlannedScore, remaining: u64 },
}

/// A zero-copy cursor over the prefix-free canonical score language.
///
/// Canonicalization compares these cursors directly.  It does not flatten a
/// planned score graph, even when that graph represents a deep recursive term
/// or a high multiplicity.
struct ScoreByteCursor<'a> {
    arena: &'a ParConstruction,
    work: Vec<ScoreCursorOp>,
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
}

struct ProtoNode {
    pieces: Vec<ProtoPiece>,
    encoded_len: usize,
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
    Par(PlannedPar),
    Components {
        occurrences: &'a [Occurrence],
        index: usize,
        remaining: u64,
        after_metadata: bool,
    },
    RootMetadata(PlannedPar),
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
        });
        Ok(id)
    }

    /// Plan an order-preserving `Expr` constructor directly from normalized
    /// child handles.
    ///
    /// The supplied child order is the sorter's evaluation order. Where that
    /// differs from protobuf field order (`EMethod`), this method performs the
    /// permutation once before invoking the generated schema walk. Recursive
    /// scores remain arena references rather than owned score trees.
    pub fn ordered_expr_component(
        &mut self,
        shell: &Expr,
        par_children: &[PlannedPar],
        canonical_path_stable: bool,
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
            return self.component(shell, &[], score, canonical_path_stable);
        };

        let unary_tag = match instance {
            ExprInstance::ENegBody(_) => Some(Score::ENEG),
            ExprInstance::ENotBody(_) => Some(Score::ENOT),
            _ => None,
        };
        if let Some(tag) = unary_tag {
            exact("unary Expr", 1, par_children.len())?;
            let score = self.score_constructor(tag, [self.par_score(par_children[0])])?;
            return self.component(shell, par_children, score, canonical_path_stable);
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
            return self.component(shell, par_children, score, canonical_path_stable);
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
                self.component(shell, par_children, score, canonical_path_stable)
            }
            ExprInstance::ETupleBody(tuple) => {
                exact("ETuple", tuple.ps.len(), par_children.len())?;
                let connective = self.score_i64(i64::from(tuple.connective_used))?;
                let mut children = Vec::with_capacity(par_children.len() + 1);
                children.extend(par_children.iter().map(|child| self.par_score(*child)));
                children.push(connective);
                let score = self.score_constructor(Score::ETUPLE, children)?;
                self.component(shell, par_children, score, canonical_path_stable)
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
                self.component(shell, &wire_children, score, canonical_path_stable)
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
                self.component(shell, &[], score, canonical_path_stable)
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
                self.component(&canonical_shell, &canonical, score, canonical_path_stable)
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
                self.component(
                    &canonical_shell,
                    &wire_children,
                    score,
                    canonical_path_stable,
                )
            }
            ExprInstance::EPathmapBody(_) => Err(ParConstructionError::UnsupportedPlannedShape(
                "EPathMap expression",
            )),
            ExprInstance::EZipperBody(_) => {
                Err(ParConstructionError::UnsupportedPlannedShape("EZipper"))
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
        for occurrence in &occurrences {
            let component = &self.components[occurrence.component.0];
            let body_len = self.protobuf_nodes[component.protobuf].encoded_len;
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
        }
        if !locally_free.is_empty() {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bytes::encoded_len(9, &locally_free))
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par locally-free field",
                ))?;
        }
        if connective_used {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bool::encoded_len(10, &true))
                .ok_or(ParConstructionError::LengthOverflow("Par connective flag"))?;
        }

        let id = PlannedPar(self.pars.len());
        self.pars.push(ParNode {
            occurrences,
            locally_free,
            connective_used,
            score,
            protobuf_len,
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
        let singleton_stable = if par.locally_free().is_empty() && !par.connective_used() {
            par.components()
                .cached_canonical_path_stability()
                .unwrap_or(false)
        } else {
            false
        };
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
            self.protobuf_nodes.push(ProtoNode {
                pieces: if protobuf.is_empty() {
                    Vec::new()
                } else {
                    vec![ProtoPiece::Bytes(protobuf)]
                },
                encoded_len,
            });
            let component = PlannedComponent(self.components.len());
            self.components.push(ComponentNode {
                tag,
                score,
                protobuf: protobuf_id,
                canonical_path_stable: singleton_stable && imported_is_singleton,
            });
            occurrences.push((component, multiplicity));
        }
        self.par(
            occurrences,
            par.locally_free().to_vec(),
            par.connective_used(),
        )
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
            for piece in &self.protobuf_nodes[id].pieces {
                let add = match piece {
                    ProtoPiece::Bytes(bytes) => bytes.len(),
                    ProtoPiece::Node { tag, node } => {
                        let child = self.protobuf_nodes[*node].encoded_len;
                        key_len(*tag)
                            .checked_add(encoded_len_varint(child as u64))
                            .and_then(|n| n.checked_add(child))
                            .ok_or(ParConstructionError::LengthOverflow(
                                "nested protobuf message",
                            ))?
                    }
                    ProtoPiece::Par { tag, par } => {
                        let child = self.pars[par.0].protobuf_len;
                        key_len(*tag)
                            .checked_add(encoded_len_varint(child as u64))
                            .and_then(|n| n.checked_add(child))
                            .ok_or(ParConstructionError::LengthOverflow("nested planned Par"))?
                    }
                };
                len = len
                    .checked_add(add)
                    .ok_or(ParConstructionError::LengthOverflow("protobuf node"))?;
            }
            self.protobuf_nodes[id].encoded_len = len;
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
            self.protobuf_nodes.push(ProtoNode {
                encoded_len: bytes.len(),
                pieces: if bytes.is_empty() {
                    Vec::new()
                } else {
                    vec![ProtoPiece::Bytes(bytes.into_boxed_slice())]
                },
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
        self.protobuf_nodes.clear();
        self.scores.clear();
        self.byte_fragments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{EMap, EMethod, ENot, EPlus, ESet, ETuple, Expr, KeyValuePair};
    use crate::rust::par_bag::with_component_key;

    fn planned_gint(
        arena: &mut ParConstruction,
        value: i64,
    ) -> Result<PlannedPar, ParConstructionError> {
        let expr = Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        };
        let component = arena.ordered_expr_component(&expr, &[], true)?;
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
        let component = arena.ordered_expr_component(&expr, &[child], false)?;
        arena.par([(component, Multiplicity::ONE)], Vec::new(), false)
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
            .ordered_expr_component(&plus_shell, &[one, two], false)
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
            .ordered_expr_component(&tuple_shell, &[plus, two], false)
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
        // order is target first; `ordered_expr_component` owns that permutation.
        let method = arena
            .ordered_expr_component(&method_shell, &[one, tuple, plus], false)
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
            .ordered_expr_component(&set_shell, &[two, one, two], false)
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
            .ordered_expr_component(&map_shell, &[one, two, one, three, two, set], false)
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
