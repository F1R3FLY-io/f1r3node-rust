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

use std::fmt;

use prost::encoding::{encode_key, encode_varint, encoded_len_varint, key_len, WireType};

use super::par_bag::{Multiplicity, ParBag, ParBagComponent, ParComponentTag, PAR_BAG_KEY_DOMAIN};
use super::rholang::protobuf_encoder;
use super::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode, ProtobufSeq, NO_RESUME};
use super::rholang::sorter::score_tree::Score;
use crate::rhoapi::Par;

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
            let mut keyed = Vec::with_capacity(occurrences.len());
            for occurrence in occurrences {
                keyed.push((self.component_key(occurrence.component)?, occurrence));
            }
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            let mut coalesced: Vec<(Vec<u8>, Occurrence)> = Vec::with_capacity(keyed.len());
            for (key, occurrence) in keyed {
                if let Some((previous_key, previous)) = coalesced.last_mut() {
                    if previous_key == &key {
                        previous.multiplicity =
                            previous.multiplicity.checked_add(occurrence.multiplicity)?;
                        continue;
                    }
                }
                coalesced.push((key, occurrence));
            }
            occurrences = coalesced
                .into_iter()
                .map(|(_, occurrence)| occurrence)
                .collect();
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
        enum Op<'a> {
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
            Bytes(&'a [u8]),
        }

        fn is_after_metadata(tag: ParComponentTag) -> bool {
            matches!(tag, ParComponentTag::Bundle | ParComponentTag::If)
        }

        let mut work = vec![match root {
            WireRoot::Par(par) => Op::Par(par),
            WireRoot::Proto(node) => Op::Node { node, index: 0 },
        }];
        while let Some(op) = work.pop() {
            match op {
                Op::Node { node, index } => {
                    if let Some(piece) = self.protobuf_nodes[node].pieces.get(index) {
                        work.push(Op::Node {
                            node,
                            index: index + 1,
                        });
                        work.push(match piece {
                            ProtoPiece::Bytes(bytes) => Op::Bytes(bytes),
                            ProtoPiece::Node { tag, node } => Op::Nested {
                                tag: *tag,
                                node: *node,
                            },
                            ProtoPiece::Par { tag, par } => Op::ChildPar {
                                tag: *tag,
                                par: *par,
                            },
                        });
                    }
                }
                Op::Nested { tag, node } => {
                    encode_key(tag, WireType::LengthDelimited, out);
                    encode_varint(self.protobuf_nodes[node].encoded_len as u64, out);
                    work.push(Op::Node { node, index: 0 });
                }
                Op::ChildPar { tag, par } => {
                    encode_key(tag, WireType::LengthDelimited, out);
                    encode_varint(self.pars[par.0].protobuf_len as u64, out);
                    work.push(Op::Par(par));
                }
                Op::Par(par) => {
                    let node = &self.pars[par.0];
                    work.push(Op::Components {
                        occurrences: &node.occurrences,
                        index: 0,
                        remaining: 0,
                        after_metadata: true,
                    });
                    work.push(Op::RootMetadata(par));
                    work.push(Op::Components {
                        occurrences: &node.occurrences,
                        index: 0,
                        remaining: 0,
                        after_metadata: false,
                    });
                }
                Op::Components {
                    occurrences,
                    mut index,
                    mut remaining,
                    after_metadata,
                } => {
                    while remaining == 0 && index < occurrences.len() {
                        let occurrence = occurrences[index];
                        index += 1;
                        if is_after_metadata(self.components[occurrence.component.0].tag)
                            == after_metadata
                        {
                            remaining = occurrence.multiplicity.get();
                            break;
                        }
                    }
                    if remaining != 0 {
                        let occurrence = occurrences[index - 1];
                        let component = &self.components[occurrence.component.0];
                        work.push(Op::Components {
                            occurrences,
                            index,
                            remaining: remaining - 1,
                            after_metadata,
                        });
                        encode_key(component.tag.byte().into(), WireType::LengthDelimited, out);
                        encode_varint(
                            self.protobuf_nodes[component.protobuf].encoded_len as u64,
                            out,
                        );
                        work.push(Op::Node {
                            node: component.protobuf,
                            index: 0,
                        });
                    }
                }
                Op::RootMetadata(par) => {
                    let node = &self.pars[par.0];
                    if !node.locally_free.is_empty() {
                        prost::encoding::bytes::encode(9, &node.locally_free, out);
                    }
                    if node.connective_used {
                        prost::encoding::bool::encode(10, &true, out);
                    }
                }
                Op::Bytes(bytes) => out.extend_from_slice(bytes),
            }
        }
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
    use crate::rhoapi::{ENot, Expr};
    use crate::rust::par_bag::with_component_key;

    fn planned_gint(
        arena: &mut ParConstruction,
        value: i64,
    ) -> Result<PlannedPar, ParConstructionError> {
        let scalar = arena.score_i64(value)?;
        let score = arena.score_constructor(Score::INT, [scalar])?;
        let expr = Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        };
        let component = arena.component(&expr, &[], score, true)?;
        arena.par([(component, Multiplicity::ONE)], Vec::new(), false)
    }

    fn planned_enot(
        arena: &mut ParConstruction,
        child: PlannedPar,
    ) -> Result<PlannedPar, ParConstructionError> {
        let score = arena.score_constructor(Score::ENOT, [arena.par_score(child)])?;
        let expr = Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(Par::default()),
            })),
        };
        let component = arena.component(&expr, &[child], score, false)?;
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
