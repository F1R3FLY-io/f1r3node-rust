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

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroUsize;

#[cfg(feature = "protobuf-projection-experiment")]
#[path = "../../tests/support/substitution_cost.rs"]
pub mod substitution_cost;

use pathmap::zipper::{ZipperIteration, ZipperMoving, ZipperReadOnlyIteration};
use pathmap::PathMap;
use prost::encoding::{encoded_len_varint, key_len};
use rspace_plus_plus::rspace::bag::{Bag, BagError, UniqueBagError};
use rspace_plus_plus::rspace::pathmap_mutation::BarrierProvider;

use super::canonical_path::tag as path_tag;
use super::epathmap_trie_codec::{self, EPathMapMode, EPathMapRepr, EpmReprOwner};
#[cfg(test)]
use super::par_bag::ParBag;
use super::par_bag::{
    canonical_ezipper_region_parts, CanonicalKeyMetadata, CanonicalParBagAccumulator,
    CanonicalParBagKey, Multiplicity, OwnedCanonicalEzipper, ParBagCodecError, ParBagComponent,
    ParComponentTag, PAR_BAG_KEY_DOMAIN,
};
use super::rholang::canonical_par_identity::insert_semantic_par_identity;
use super::rholang::canonical_semantics::{
    self as semantics, CanonicalScoreAlgebra, PAR_SCORE_TAGS,
};
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
    Bundle, Connective, EPathMap, EZipper, Expr, GUnforgeable, If, Match, New, Par, Receive,
    ReceiveBind, Send,
};

/// An index into a [`ParConstruction`]'s planned-Par arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedPar(usize);

#[derive(Default)]
pub(crate) struct PlannedEpmBarrier(Option<PlannedPar>);

impl BarrierProvider<PlannedPar> for PlannedEpmBarrier {
    fn make_barrier(&self) -> PlannedPar {
        self.0
            .expect("an owned planned map retains a valid arena barrier")
    }

    fn discard_barrier(&self, _: PlannedPar) {}
}

struct ArenaEpmBarrier<'a>(RefCell<&'a mut ParConstruction>);

impl BarrierProvider<PlannedPar> for ArenaEpmBarrier<'_> {
    fn make_barrier(&self) -> PlannedPar { self.0.borrow_mut().epm_barrier() }

    fn discard_barrier(&self, _: PlannedPar) {}
}

pub(crate) type OwnedPlannedEPathMap = EpmReprOwner<PlannedPar, PlannedEpmBarrier>;

#[derive(Default)]
pub(crate) struct ProtobufParBuilder {
    occurrence_root: Option<usize>,
    locally_free: Vec<u8>,
    connective_used: bool,
}

pub(crate) struct ProtobufRootStream {
    entries: Bag,
    occurrences: Vec<u8>,
    overflow_counts: BTreeMap<usize, Multiplicity>,
    key: Vec<u8>,
    max_key: Vec<u8>,
    sorted_unique: bool,
    overlay: BTreeMap<Vec<u8>, (usize, bool)>,
    overlay_has_suffix: bool,
    score_len: usize,
    protobuf_len: usize,
    protobuf_zero_count: usize,
    singleton_stable: bool,
}

impl Default for ProtobufRootStream {
    fn default() -> Self {
        Self {
            entries: Bag::new(),
            occurrences: Vec::new(),
            overflow_counts: BTreeMap::new(),
            key: Vec::new(),
            max_key: Vec::new(),
            sorted_unique: true,
            overlay: BTreeMap::new(),
            overlay_has_suffix: false,
            score_len: 0,
            protobuf_len: 0,
            protobuf_zero_count: 0,
            singleton_stable: false,
        }
    }
}

impl ProtobufRootStream {
    const OVERLAY_LIMIT: usize = 64;

    fn convert_to_ordinals(&mut self) {
        if !self.sorted_unique {
            return;
        }
        let mut ordinal = 0u64;
        self.entries = std::mem::take(&mut self.entries)
            .try_reweight(|key, multiplicity| {
                debug_assert_eq!(multiplicity, Multiplicity::ONE);
                let index = match self.overlay.get(key) {
                    Some(&(index, true)) => index,
                    entry => {
                        let index = ordinal as usize;
                        if let Some(&(recorded, false)) = entry {
                            debug_assert_eq!(recorded, index);
                        }
                        ordinal = ordinal.checked_add(1).expect("addressable root ordinal");
                        index
                    }
                };
                Ok::<_, BagError>(
                    Multiplicity::new(u64::try_from(index + 1).expect("addressable root ordinal"))
                        .expect("a positive ordinal is a multiplicity"),
                )
            })
            .expect("ordinal cardinality fits the exact Bag counter");
        debug_assert_eq!(
            ordinal as usize + self.overlay.values().filter(|(_, suffix)| *suffix).count(),
            self.occurrences.len()
        );
        self.overlay.clear();
        self.overlay_has_suffix = false;
        self.sorted_unique = false;
    }

    fn multiplicity_at(&self, index: usize) -> Multiplicity {
        self.overflow_counts
            .get(&index)
            .copied()
            .unwrap_or_else(|| {
                Multiplicity::new(u64::from(self.occurrences[index]))
                    .expect("a streamed root endpoint remains positive")
            })
    }

    fn sorted_index(&self) -> Option<usize> {
        if let Some(&(index, _)) = self.overlay.get(&self.key) {
            return Some(index);
        }
        if !self.entries.contains(&self.key) {
            return None;
        }
        let mut ordinal = 0usize;
        let mut found = None;
        let _ = self.entries.try_visit_relative_prefix(&[], |path, _| {
            if self.overlay.get(path).is_some_and(|(_, suffix)| *suffix) {
                return Ok::<_, ()>(());
            }
            if path == self.key {
                found = Some(ordinal);
                return Err(());
            }
            ordinal += 1;
            Ok(())
        });
        Some(found.expect("a present sorted root endpoint has a prefix ordinal"))
    }

    fn insert_key(
        &mut self,
        own_score_len: usize,
        own_wire_len: usize,
        own_zero_count: usize,
        canonical_path_stable: bool,
        limit: usize,
    ) -> bool {
        let Some(score_len) = self
            .score_len
            .checked_add(own_score_len)
            .filter(|&value| value <= limit)
        else {
            return false;
        };
        let Some(protobuf_len) = self
            .protobuf_len
            .checked_add(own_wire_len)
            .filter(|&value| value <= limit)
        else {
            return false;
        };
        let Some(protobuf_zero_count) = self
            .protobuf_zero_count
            .checked_add(own_zero_count)
            .filter(|&value| value <= limit)
        else {
            return false;
        };
        if self.key.len() > limit || self.key.len() > isize::MAX as usize {
            return false;
        }
        let Some(proposed) = self
            .occurrences
            .len()
            .checked_add(1)
            .and_then(|value| u64::try_from(value).ok())
            .and_then(|value| Multiplicity::new(value).ok())
        else {
            return false;
        };
        let beyond_max = self.occurrences.is_empty() || self.key > self.max_key;
        let mut append_prefix = false;
        let mut sorted_result = None;
        if self.sorted_unique {
            if !self.overlay_has_suffix && beyond_max {
                let count = self
                    .entries
                    .insert_many(&self.key, Multiplicity::ONE)
                    .expect("a fresh root endpoint fits exact Bag cardinality");
                debug_assert_eq!(count, Multiplicity::ONE);
                append_prefix = true;
                sorted_result = Some((proposed, true));
            } else {
                let index = self.sorted_index();
                if self.overlay.len() == Self::OVERLAY_LIMIT
                    && !self.overlay.contains_key(&self.key)
                {
                    self.convert_to_ordinals();
                } else if let Some(index) = index {
                    self.overlay
                        .entry(self.key.clone())
                        .or_insert((index, false));
                    sorted_result = Some((
                        Multiplicity::new(u64::try_from(index + 1).expect("addressable ordinal"))
                            .expect("positive ordinal"),
                        false,
                    ));
                } else {
                    let count = self
                        .entries
                        .insert_many(&self.key, Multiplicity::ONE)
                        .expect("a fresh root endpoint fits exact Bag cardinality");
                    debug_assert_eq!(count, Multiplicity::ONE);
                    self.overlay
                        .insert(self.key.clone(), (self.occurrences.len(), true));
                    self.overlay_has_suffix = true;
                    sorted_result = Some((proposed, true));
                }
            }
        }
        let (id, inserted) = sorted_result.unwrap_or_else(|| {
            if beyond_max {
                self.entries.get_or_insert_from_cursor(&self.key, proposed)
            } else {
                self.entries.get_or_insert(&self.key, proposed)
            }
            .expect("addressable unique root ordinals fit exact Bag cardinality")
        });
        if inserted {
            if append_prefix || (!self.sorted_unique && beyond_max) {
                if self.max_key.is_empty() {
                    std::mem::swap(&mut self.max_key, &mut self.key);
                } else {
                    self.max_key.clear();
                    self.max_key.extend_from_slice(&self.key);
                }
            }
            self.occurrences.push(1);
            self.singleton_stable = self.occurrences.len() == 1 && canonical_path_stable;
        } else {
            let index = id.get() as usize - 1;
            let Ok(multiplicity) = self.multiplicity_at(index).checked_add(Multiplicity::ONE)
            else {
                return false;
            };
            if let Ok(small) = u8::try_from(multiplicity.get()) {
                self.occurrences[index] = small;
            } else {
                self.overflow_counts.insert(index, multiplicity);
            }
            self.singleton_stable = false;
        }
        self.score_len = score_len;
        self.protobuf_len = protobuf_len;
        self.protobuf_zero_count = protobuf_zero_count;
        true
    }

    pub(crate) fn try_insert_fixed_expr(&mut self, expr: &Expr, limit: usize) -> bool {
        if !matches!(
            expr.expr_instance.as_ref(),
            Some(ExprInstance::GBool(_) | ExprInstance::GInt(_) | ExprInstance::GDouble(_))
        ) {
            return false;
        }
        self.key.clear();
        self.key.extend_from_slice(&PAR_BAG_KEY_DOMAIN);
        self.key.push(ParComponentTag::Expr.byte());
        let header = PAR_BAG_KEY_DOMAIN.len() + 1;
        let score_len = Expr::append_key_suffix(expr, &mut self.key);
        let Ok(score_trailer) = u64::try_from(score_len) else {
            return false;
        };
        let Some(body_start) = header
            .checked_add(score_len)
            .filter(|&value| value <= self.key.len())
        else {
            return false;
        };
        let body = &self.key[body_start..];
        let prefix = InlineWireBytes::length_delimited_prefix(
            ParComponentTag::Expr.byte().into(),
            body.len(),
        );
        let Some(wire_len) = key_len(ParComponentTag::Expr.byte().into())
            .checked_add(encoded_len_varint(body.len() as u64))
            .and_then(|value| value.checked_add(body.len()))
        else {
            return false;
        };
        let Some(zero_count) = prefix
            .zero_count()
            .checked_add(body.iter().filter(|&&byte| byte == 0).count())
        else {
            return false;
        };
        self.key.extend_from_slice(&score_trailer.to_be_bytes());
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::EmitRootKey(self.key.len()));
        self.insert_key(score_len, wire_len, zero_count, true, limit)
    }

    pub(crate) fn try_insert(
        &mut self,
        arena: &ParConstruction,
        component: PlannedComponent,
        limit: usize,
    ) -> bool {
        let Ok(summary) = arena.make_occurrence_node(
            Occurrence {
                component,
                multiplicity: Multiplicity::ONE,
            },
            None,
            None,
        ) else {
            return false;
        };
        let node = &arena.components[component.0];
        let body_len = arena.protobuf_nodes[node.protobuf].encoded_len;
        let Some(capacity) = PAR_BAG_KEY_DOMAIN
            .len()
            .checked_add(1)
            .and_then(|value| value.checked_add(summary.score_len))
            .and_then(|value| value.checked_add(body_len))
            .and_then(|value| value.checked_add(std::mem::size_of::<u64>()))
            .filter(|&value| value <= isize::MAX as usize && value <= limit)
        else {
            return false;
        };
        if u64::try_from(summary.score_len).is_err() {
            return false;
        }
        arena
            .component_key_into(component, &mut self.key)
            .expect("preflighted root component key");
        debug_assert_eq!(self.key.len(), capacity);
        self.insert_key(
            summary.score_len,
            summary.protobuf_len,
            summary.protobuf_zero_count,
            node.canonical_path_stable,
            limit,
        )
    }

    pub(crate) fn replay(
        mut self,
        arena: &mut ParConstruction,
        parent: &mut ProtobufParBuilder,
    ) -> Result<(), ParConstructionError> {
        self.convert_to_ordinals();
        let mut ordered = vec![Vec::new(); self.occurrences.len()];
        self.entries
            .visit_prefix(&[], |key, id| ordered[id.get() as usize - 1] = key.to_vec());
        for (index, key) in ordered.into_iter().enumerate() {
            let multiplicity = self.multiplicity_at(index);
            let header = PAR_BAG_KEY_DOMAIN.len() + 1;
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len =
                u64::from_be_bytes(key[trailer..].try_into().expect("score trailer")) as usize;
            let tag = ParComponentTag::from_byte(key[PAR_BAG_KEY_DOMAIN.len()])
                .expect("streamed canonical component tag");
            let score = arena.score_from_encoded(&key[header..header + score_len])?;
            let body = &key[header + score_len..trailer];
            let protobuf = arena.protobuf_nodes.len();
            arena.protobuf_nodes.push(ProtoNode {
                pieces: if body.is_empty() {
                    ProtoProgram::default()
                } else {
                    ProtoProgram::One(ProtoPiece::Bytes(body.to_vec().into_boxed_slice()))
                },
                encoded_len: body.len(),
                zero_count: body.iter().filter(|&&byte| byte == 0).count(),
            });
            let component = PlannedComponent(arena.components.len());
            arena.components.push(ComponentNode {
                tag,
                score,
                protobuf,
                canonical_path_stable: self.singleton_stable,
                canonical_segment: None,
                semantic_shape: ComponentSemanticShape::Opaque,
            });
            parent.occurrence_root =
                arena.insert_occurrence(parent.occurrence_root, Occurrence {
                    component,
                    multiplicity,
                })?;
        }
        Ok(())
    }

    fn validate_root_summary(
        &self,
        parent: &ProtobufParBuilder,
    ) -> Result<(), ParConstructionError> {
        1usize
            .checked_add(10)
            .and_then(|length| length.checked_add(self.score_len))
            .and_then(|length| length.checked_add(10))
            .and_then(|length| length.checked_add(1))
            .ok_or(ParConstructionError::LengthOverflow("canonical score"))?;
        let mut protobuf_len = self.protobuf_len;
        if !parent.locally_free.is_empty() {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bytes::encoded_len(9, &parent.locally_free))
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par locally-free field",
                ))?;
            let prefix = InlineWireBytes::length_delimited_prefix(9, parent.locally_free.len());
            self.protobuf_zero_count
                .checked_add(prefix.zero_count())
                .and_then(|count| {
                    count.checked_add(
                        parent
                            .locally_free
                            .iter()
                            .filter(|&&byte| byte == 0)
                            .count(),
                    )
                })
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par protobuf zero-byte count",
                ))?;
        }
        if parent.connective_used {
            protobuf_len
                .checked_add(prost::encoding::bool::encoded_len(10, &true))
                .ok_or(ParConstructionError::LengthOverflow("Par connective flag"))?;
        }
        Ok(())
    }

    pub(crate) fn finish(
        self,
        _arena: &mut ParConstruction,
        parent: ProtobufParBuilder,
    ) -> Result<Par, ParConstructionError> {
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::BeginFinish);
        self.validate_root_summary(&parent)?;
        let singleton = self.occurrences.len() == 1 && self.multiplicity_at(0) == Multiplicity::ONE;
        let mut accumulator = CanonicalParBagAccumulator::new();
        let counts = &self.occurrences;
        let overflow_counts = &self.overflow_counts;
        let stable = self.singleton_stable;
        let mut record = |key: &[u8], multiplicity| {
            let trailer = key.len() - std::mem::size_of::<u64>();
            let score_len =
                u64::from_be_bytes(key[trailer..].try_into().expect("score trailer")) as usize;
            accumulator
                .record_prevalidated(multiplicity, &CanonicalKeyMetadata {
                    tag: ParComponentTag::from_byte(key[PAR_BAG_KEY_DOMAIN.len()])
                        .expect("streamed component tag"),
                    body_len: trailer - PAR_BAG_KEY_DOMAIN.len() - 1 - score_len,
                    canonical_path_stable: stable,
                })
                .map_err(ParConstructionError::FamilyMetrics)?;
            Ok::<_, ParConstructionError>(())
        };
        let entries = if self.sorted_unique {
            let mut entries = self.entries;
            for (key, &(index, _)) in &self.overlay {
                let multiplicity = overflow_counts.get(&index).copied().unwrap_or_else(|| {
                    Multiplicity::new(u64::from(counts[index]))
                        .expect("a streamed root endpoint remains positive")
                });
                if multiplicity > Multiplicity::ONE {
                    entries.insert_many(
                        key,
                        Multiplicity::new(multiplicity.get() - 1)
                            .expect("a repeated root endpoint adds positively"),
                    )?;
                }
            }
            if singleton {
                record(&self.max_key, Multiplicity::ONE)?;
            } else {
                entries.try_visit_relative_prefix(&[], |key, multiplicity| {
                    record(key, multiplicity)
                })?;
            }
            entries
        } else {
            self.entries.try_reweight(|key, id| {
                let index = id.get() as usize - 1;
                let multiplicity = overflow_counts.get(&index).copied().unwrap_or_else(|| {
                    Multiplicity::new(u64::from(counts[index]))
                        .expect("a streamed root endpoint remains positive")
                });
                record(key, multiplicity)?;
                Ok::<_, ParConstructionError>(multiplicity)
            })?
        };
        let mut components = accumulator.finish(entries);
        if singleton {
            let tag = ParComponentTag::from_byte(self.max_key[PAR_BAG_KEY_DOMAIN.len()])
                .expect("singleton component tag");
            components.seed_singleton_derived_caches(tag, &self.max_key, None);
        }
        let result = Par::from_parts(components, parent.locally_free, parent.connective_used);
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::FinishOk);
        Ok(result)
    }
}

impl ProtobufParBuilder {
    pub(crate) fn merge_protobuf_locally_free<B: prost::bytes::Buf>(
        &mut self,
        wire_type: prost::encoding::WireType,
        buf: &mut B,
        ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError> {
        prost::encoding::bytes::merge(wire_type, &mut self.locally_free, buf, ctx)
    }

    pub(crate) fn merge_protobuf_connective_used<B: prost::bytes::Buf>(
        &mut self,
        wire_type: prost::encoding::WireType,
        buf: &mut B,
        ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError> {
        prost::encoding::bool::merge(wire_type, &mut self.connective_used, buf, ctx)
    }
}

/// An index into the canonical-score fragment arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedScore(usize);

/// An index into the planned-component arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlannedComponent(usize);

/// Arena handle for the canonical order key of a receive bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedReceiveBindOrder {
    score: PlannedScore,
    protobuf: usize,
}

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
    Publication(UniqueBagError),
    FamilyMetrics(ParBagCodecError),
    InvalidCanonicalEzipperRegion,
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
            Self::Publication(error) => error.fmt(formatter),
            Self::FamilyMetrics(error) => error.fmt(formatter),
            Self::InvalidCanonicalEzipperRegion => {
                formatter.write_str("invalid canonical EZipper protobuf region")
            }
        }
    }
}

impl std::error::Error for ParConstructionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Multiplicity(error) => Some(error),
            Self::Publication(error) => Some(error),
            Self::FamilyMetrics(error) => Some(error),
            Self::LengthOverflow(_)
            | Self::MissingParSlot { .. }
            | Self::ExtraParSlots { .. }
            | Self::ChildArity { .. }
            | Self::InvalidCanonicalEzipperRegion => None,
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
    OccurrenceScores {
        root: Option<usize>,
        encoded_len: usize,
    },
    EscapedWire {
        root: WireRoot,
        encoded_len: usize,
    },
}

enum ScoreCursorOp<'a> {
    Score(PlannedScore),
    Bytes { bytes: &'a [u8], offset: usize },
    Repeat { child: PlannedScore, remaining: u64 },
    OccurrenceScores { root: Option<usize>, family: usize },
    OccurrenceFamilyNode { node: usize, family: usize },
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
                    let pieces = match &self.arena.scores[score.0].pieces {
                        ScoreProgram::Literal(fragment) => {
                            self.work.push(ScoreCursorOp::Bytes {
                                bytes: &self.arena.byte_fragments[*fragment],
                                offset: 0,
                            });
                            continue;
                        }
                        ScoreProgram::Pooled(span) => {
                            self.work.push(ScoreCursorOp::Bytes {
                                bytes: span.view(&self.arena.byte_pool),
                                offset: 0,
                            });
                            continue;
                        }
                        ScoreProgram::Composite(pieces) => pieces,
                    };
                    for piece in pieces.iter().rev() {
                        self.work.push(match *piece {
                            ScorePiece::Bytes(fragment) => ScoreCursorOp::Bytes {
                                bytes: &self.arena.byte_fragments[fragment],
                                offset: 0,
                            },
                            ScorePiece::Child(child) => ScoreCursorOp::Score(child),
                            ScorePiece::OccurrenceScores { root, .. } => {
                                ScoreCursorOp::OccurrenceScores { root, family: 0 }
                            }
                            ScorePiece::EscapedWire { root, .. } => {
                                ScoreCursorOp::EscapedWire(WireByteCursor::new(self.arena, root))
                            }
                        });
                    }
                }
                ScoreCursorOp::Bytes { bytes, offset } => {
                    let Some(&byte) = bytes.get(offset) else {
                        continue;
                    };
                    if offset + 1 < bytes.len() {
                        self.work.push(ScoreCursorOp::Bytes {
                            bytes,
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
                ScoreCursorOp::OccurrenceScores { root, family } => {
                    if family < PAR_SCORE_TAGS.len() {
                        self.work.push(ScoreCursorOp::OccurrenceScores {
                            root,
                            family: family + 1,
                        });
                        if let Some(node) = root {
                            self.work
                                .push(ScoreCursorOp::OccurrenceFamilyNode { node, family });
                        }
                    }
                }
                ScoreCursorOp::OccurrenceFamilyNode { node, family } => {
                    let occurrence_node = &self.arena.occurrence_nodes[node];
                    let component = &self.arena.components[occurrence_node.occurrence.component.0];
                    match component.tag.cmp(&PAR_SCORE_TAGS[family]) {
                        Ordering::Less => {
                            if let Some(right) = occurrence_node.right() {
                                self.work.push(ScoreCursorOp::OccurrenceFamilyNode {
                                    node: right,
                                    family,
                                });
                            }
                        }
                        Ordering::Greater => {
                            if let Some(left) = occurrence_node.left() {
                                self.work.push(ScoreCursorOp::OccurrenceFamilyNode {
                                    node: left,
                                    family,
                                });
                            }
                        }
                        Ordering::Equal => {
                            if let Some(right) = occurrence_node.right() {
                                self.work.push(ScoreCursorOp::OccurrenceFamilyNode {
                                    node: right,
                                    family,
                                });
                            }
                            self.work.push(ScoreCursorOp::Repeat {
                                child: component.score,
                                remaining: occurrence_node.occurrence.multiplicity.get(),
                            });
                            if let Some(left) = occurrence_node.left() {
                                self.work.push(ScoreCursorOp::OccurrenceFamilyNode {
                                    node: left,
                                    family,
                                });
                            }
                        }
                    }
                }
                ScoreCursorOp::EscapedWire(mut cursor) => {
                    let Some(byte) = cursor.next() else {
                        continue;
                    };
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
    pieces: ScoreProgram,
    encoded_len: usize,
}

enum ScoreProgram {
    Literal(usize),
    Pooled(ByteSpan),
    Composite(Box<[ScorePiece]>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ByteSpan {
    start: usize,
    end: usize,
}

impl ByteSpan {
    fn view(self, pool: &[u8]) -> &[u8] { &pool[self.start..self.end] }
}

impl From<Vec<ScorePiece>> for ScoreProgram {
    fn from(pieces: Vec<ScorePiece>) -> Self {
        if let [ScorePiece::Bytes(fragment)] = pieces.as_slice() {
            Self::Literal(*fragment)
        } else {
            Self::Composite(pieces.into_boxed_slice())
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/incomplete_component_preservation.rs"]
mod incomplete_preservation_tests;

#[cfg(test)]
#[path = "../../tests/support/protobuf_par_accumulation.rs"]
mod protobuf_par_accumulation_tests;

#[cfg(test)]
#[path = "../../tests/support/collection_collision_elision.rs"]
mod collection_collision_elision;

#[cfg(test)]
#[path = "../../tests/support/planned_epm_ownership.rs"]
mod planned_epm_ownership_tests;

#[cfg(test)]
#[path = "../../tests/support/protobuf_construction.rs"]
mod protobuf_construction_tests;

#[cfg(test)]
#[path = "../../tests/support/planned_fragment_comparison.rs"]
mod planned_fragment_comparison_tests;

#[cfg(test)]
#[path = "../../tests/support/protobuf_construction_resources.rs"]
mod protobuf_construction_resource_tests;

#[cfg(test)]
#[path = "../../tests/support/planned_ground_leaves.rs"]
mod planned_ground_leaf_tests;

#[cfg(test)]
#[path = "../../tests/support/planned_fragment_publication.rs"]
mod planned_fragment_publication_tests;

#[cfg(test)]
#[path = "../../tests/support/planned_pooled_storage.rs"]
mod planned_pooled_storage_tests;

enum ProtoPiece {
    Bytes(Box<[u8]>),
    PooledBytes(ByteSpan),
    SharedBytes(Box<prost::bytes::Bytes>),
    Node { tag: u32, node: usize },
    Par { tag: u32, par: PlannedPar },
    EPathMapSnapshot { tag: u32, map: PlannedEPathMap },
}

struct ProtoNode {
    pieces: ProtoProgram,
    encoded_len: usize,
    zero_count: usize,
}

enum ProtoProgram {
    One(ProtoPiece),
    Many(Vec<ProtoPiece>),
}

impl Default for ProtoProgram {
    fn default() -> Self { Self::Many(Vec::new()) }
}

impl From<Vec<ProtoPiece>> for ProtoProgram {
    fn from(mut pieces: Vec<ProtoPiece>) -> Self {
        if pieces.len() == 1 {
            Self::One(pieces.pop().unwrap())
        } else {
            Self::Many(pieces)
        }
    }
}

impl ProtoProgram {
    fn as_slice(&self) -> &[ProtoPiece] {
        match self {
            Self::One(piece) => std::slice::from_ref(piece),
            Self::Many(pieces) => pieces,
        }
    }
    fn get(&self, index: usize) -> Option<&ProtoPiece> { self.as_slice().get(index) }
    fn push(&mut self, piece: ProtoPiece) {
        match self {
            Self::Many(pieces) if !pieces.is_empty() => pieces.push(piece),
            Self::Many(_) => *self = Self::One(piece),
            Self::One(_) => {
                let Self::One(previous) = std::mem::take(self) else {
                    unreachable!()
                };
                *self = Self::Many(vec![previous, piece]);
            }
        }
    }
    fn into_vec(self) -> Vec<ProtoPiece> {
        match self {
            Self::One(piece) => vec![piece],
            Self::Many(pieces) => pieces,
        }
    }
}

impl<'a> IntoIterator for &'a ProtoProgram {
    type Item = &'a ProtoPiece;
    type IntoIter = std::slice::Iter<'a, ProtoPiece>;
    fn into_iter(self) -> Self::IntoIter { self.as_slice().iter() }
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
    ParSemanticIdentity(PlannedPar),
    KeyValuePair(PlannedPar, PlannedPar),
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
        root: Option<usize>,
        after_metadata: bool,
    },
    OccurrenceNode {
        node: usize,
        after_metadata: bool,
    },
    ComponentOccurrence {
        occurrence: Occurrence,
        remaining: u64,
    },
    RootMetadata(PlannedPar),
    RootConnective(PlannedPar),
    ParSemanticIdentity(PlannedPar),
    KeyValuePair {
        key: PlannedPar,
        value: PlannedPar,
    },
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
        #[cfg(test)]
        if matches!(root, WireRoot::ParSemanticIdentity(_)) {
            collection_collision_elision::record_identity_cursor();
        }
        let root = match root {
            WireRoot::Par(par) => WireCursorOp::Par(par),
            WireRoot::ParSemanticIdentity(par) => WireCursorOp::ParSemanticIdentity(par),
            WireRoot::KeyValuePair(key, value) => WireCursorOp::KeyValuePair { key, value },
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
                            ProtoPiece::PooledBytes(span) => WireCursorOp::Bytes {
                                bytes: span.view(&self.arena.byte_pool),
                                offset: 0,
                            },
                            ProtoPiece::SharedBytes(bytes) => {
                                WireCursorOp::Bytes { bytes, offset: 0 }
                            }
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
                        root: node.occurrence_root,
                        after_metadata: true,
                    });
                    self.work.push(WireCursorOp::RootMetadata(par));
                    self.work.push(WireCursorOp::Components {
                        root: node.occurrence_root,
                        after_metadata: false,
                    });
                }
                WireCursorOp::ParSemanticIdentity(par) => {
                    let node = &self.arena.pars[par.0];
                    self.work.push(WireCursorOp::Components {
                        root: node.occurrence_root,
                        after_metadata: true,
                    });
                    self.work.push(WireCursorOp::RootConnective(par));
                    self.work.push(WireCursorOp::Components {
                        root: node.occurrence_root,
                        after_metadata: false,
                    });
                }
                WireCursorOp::Components {
                    root,
                    after_metadata,
                } => {
                    if let Some(node) = root {
                        self.work.push(WireCursorOp::OccurrenceNode {
                            node,
                            after_metadata,
                        });
                    }
                }
                WireCursorOp::OccurrenceNode {
                    node,
                    after_metadata,
                } => {
                    let occurrence_node = &self.arena.occurrence_nodes[node];
                    let occurrence = occurrence_node.occurrence;
                    match is_after_metadata(self.arena.components[occurrence.component.0].tag)
                        .cmp(&after_metadata)
                    {
                        Ordering::Less => {
                            if let Some(right) = occurrence_node.right() {
                                self.work.push(WireCursorOp::OccurrenceNode {
                                    node: right,
                                    after_metadata,
                                });
                            }
                        }
                        Ordering::Greater => {
                            if let Some(left) = occurrence_node.left() {
                                self.work.push(WireCursorOp::OccurrenceNode {
                                    node: left,
                                    after_metadata,
                                });
                            }
                        }
                        Ordering::Equal => {
                            if let Some(right) = occurrence_node.right() {
                                self.work.push(WireCursorOp::OccurrenceNode {
                                    node: right,
                                    after_metadata,
                                });
                            }
                            self.work.push(WireCursorOp::ComponentOccurrence {
                                occurrence,
                                remaining: occurrence.multiplicity.get(),
                            });
                            if let Some(left) = occurrence_node.left() {
                                self.work.push(WireCursorOp::OccurrenceNode {
                                    node: left,
                                    after_metadata,
                                });
                            }
                        }
                    }
                }
                WireCursorOp::ComponentOccurrence {
                    occurrence,
                    remaining,
                } => {
                    if remaining != 0 {
                        let component = &self.arena.components[occurrence.component.0];
                        self.work.push(WireCursorOp::ComponentOccurrence {
                            occurrence,
                            remaining: remaining - 1,
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
                WireCursorOp::RootConnective(par) => {
                    if self.arena.pars[par.0].connective_used {
                        self.work
                            .push(WireCursorOp::Inline(InlineWireBytes::protobuf_varints(&[
                                10 << 3,
                                1,
                            ])));
                    }
                }
                WireCursorOp::KeyValuePair { key, value } => {
                    // `KeyValuePair { key: 1, value: 2 }` in exact protobuf
                    // field order. `ChildPar` contributes each tag, length,
                    // and planned Par body without allocating a pair node.
                    self.work
                        .push(WireCursorOp::ChildPar { tag: 2, par: value });
                    self.work.push(WireCursorOp::ChildPar { tag: 1, par: key });
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
                    let Some(&byte) = bytes.get(offset) else {
                        continue;
                    };
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
    semantic_shape: ComponentSemanticShape,
}

enum ComponentSemanticShape {
    Opaque,
    Expr(PlannedExprSemantic),
    Bundle {
        body: PlannedPar,
        write_flag: bool,
        read_flag: bool,
    },
    ConnectiveAnd(Box<[PlannedPar]>),
    ConnectiveOr(Box<[PlannedPar]>),
    ConnectiveOther,
}

enum PlannedExprSemantic {
    Inert,
    Unary(PlannedPar),
    Binary(PlannedPar, PlannedPar),
    MatchesTarget(PlannedPar),
    Unsupported(Box<UnsupportedExpr>),
}

struct UnsupportedExpr {
    kind: &'static str,
    detail: Option<String>,
}

/// Borrowed evaluator-relevant shape of one planned expression endpoint.
pub enum PlannedExprShape<'a> {
    Inert,
    Unary(PlannedPar),
    Binary(PlannedPar, PlannedPar),
    MatchesTarget(PlannedPar),
    Unsupported {
        kind: &'static str,
        detail: Option<&'a str>,
    },
    /// A component imported as already-canonical bytes. Normalizer-created
    /// guards never contain this shape; callers must not guess its semantics.
    Opaque,
}

/// Borrowed semantic view of the one top-level connective in a planned Par.
///
/// The view is maintained while the component is planned; it never decodes
/// protobuf bytes or materializes a resident `Par` merely to inspect shape.
pub enum PlannedConnective<'a> {
    And(&'a [PlannedPar]),
    Or(&'a [PlannedPar]),
    Other,
}

enum CanonicalSegment {
    Bytes(Box<[u8]>),
    PooledBytes(ByteSpan),
    List(Box<[PlannedPar]>),
    Tuple(Box<[PlannedPar]>),
    EPathMap(PlannedEPathMap),
}

#[derive(Clone, Copy)]
struct Occurrence {
    component: PlannedComponent,
    multiplicity: Multiplicity,
}

#[derive(Clone, Copy)]
struct OccurrenceTreeNode {
    owner: usize,
    occurrence: Occurrence,
    left: Option<NonZeroUsize>,
    right: Option<NonZeroUsize>,
    height: usize,
    distinct_len: usize,
    score_len: usize,
    protobuf_len: usize,
    protobuf_zero_count: usize,
}

impl OccurrenceTreeNode {
    fn left(&self) -> Option<usize> { self.left.map(|index| index.get() - 1) }
    fn right(&self) -> Option<usize> { self.right.map(|index| index.get() - 1) }
    fn encode_link(index: Option<usize>) -> Result<Option<NonZeroUsize>, ParConstructionError> {
        index
            .map(|index| {
                index.checked_add(1).and_then(NonZeroUsize::new).ok_or(
                    ParConstructionError::LengthOverflow("occurrence child index"),
                )
            })
            .transpose()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConstructionCheckpoint {
    bytes: usize,
    pool: usize,
    scores: usize,
    protobuf: usize,
    epathmaps: usize,
    components: usize,
    occurrences: usize,
    pars: usize,
}

pub(crate) fn retire_draining_capacity<T>(
    values: &mut Vec<T>,
    threshold: &mut Option<usize>,
) -> bool {
    let next = threshold.get_or_insert(values.capacity() / 2);
    if values.is_empty() {
        let released = values.capacity() != 0;
        *values = Vec::new();
        *next = 0;
        released
    } else if *next != 0 && values.len() <= *next {
        values.shrink_to_fit();
        *next = values.len() / 2;
        true
    } else {
        false
    }
}

pub(crate) struct OccurrenceSearch {
    path: Vec<(usize, bool)>,
    found: Option<usize>,
}

struct OccurrenceEdit<'a> {
    arena: &'a mut ParConstruction,
    owner: usize,
    path: Vec<(usize, bool)>,
    undo: Vec<(usize, OccurrenceTreeNode)>,
    committed: bool,
}

impl OccurrenceEdit<'_> {
    fn store(
        &mut self,
        preferred: Option<usize>,
        occurrence: Occurrence,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<usize, ParConstructionError> {
        let mut value = self.arena.make_occurrence_node(occurrence, left, right)?;
        value.owner = self.owner;
        if let Some(index) =
            preferred.filter(|&i| self.arena.occurrence_nodes[i].owner == self.owner)
        {
            self.undo.push((index, self.arena.occurrence_nodes[index]));
            self.arena.occurrence_nodes[index] = value;
            Ok(index)
        } else {
            let index = self.arena.occurrence_nodes.len();
            self.arena.occurrence_nodes.push(value);
            Ok(index)
        }
    }

    fn rebuild(
        &mut self,
        original: usize,
        occurrence: Occurrence,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<usize, ParConstructionError> {
        let lh = self.arena.occurrence_height(left);
        let rh = self.arena.occurrence_height(right);
        if lh > rh && lh - rh > 1 {
            let pivot_index = left.expect("left-heavy occurrence node");
            let pivot = self.arena.occurrence_nodes[pivot_index];
            if self.arena.occurrence_height(pivot.left())
                >= self.arena.occurrence_height(pivot.right())
            {
                let demoted = self.store(Some(original), occurrence, pivot.right(), right)?;
                return self.store(
                    Some(pivot_index),
                    pivot.occurrence,
                    pivot.left(),
                    Some(demoted),
                );
            }
            let middle_index = pivot.right().expect("left-right occurrence rotation");
            let middle = self.arena.occurrence_nodes[middle_index];
            let new_left = self.store(
                Some(pivot_index),
                pivot.occurrence,
                pivot.left(),
                middle.left(),
            )?;
            let new_right = self.store(Some(original), occurrence, middle.right(), right)?;
            return self.store(
                Some(middle_index),
                middle.occurrence,
                Some(new_left),
                Some(new_right),
            );
        }
        if rh > lh && rh - lh > 1 {
            let pivot_index = right.expect("right-heavy occurrence node");
            let pivot = self.arena.occurrence_nodes[pivot_index];
            if self.arena.occurrence_height(pivot.right())
                >= self.arena.occurrence_height(pivot.left())
            {
                let demoted = self.store(Some(original), occurrence, left, pivot.left())?;
                return self.store(
                    Some(pivot_index),
                    pivot.occurrence,
                    Some(demoted),
                    pivot.right(),
                );
            }
            let middle_index = pivot.left().expect("right-left occurrence rotation");
            let middle = self.arena.occurrence_nodes[middle_index];
            let new_left = self.store(Some(original), occurrence, left, middle.left())?;
            let new_right = self.store(
                Some(pivot_index),
                pivot.occurrence,
                middle.right(),
                pivot.right(),
            )?;
            return self.store(
                Some(middle_index),
                middle.occurrence,
                Some(new_left),
                Some(new_right),
            );
        }
        self.store(Some(original), occurrence, left, right)
    }
}

impl Drop for OccurrenceEdit<'_> {
    fn drop(&mut self) {
        if !self.committed {
            for (index, old) in self.undo.drain(..).rev() {
                self.arena.occurrence_nodes[index] = old;
            }
        }
        self.undo.clear();
        self.path.clear();
        self.arena.occurrence_undo = std::mem::take(&mut self.undo);
        self.arena.occurrence_path = std::mem::take(&mut self.path);
    }
}

struct OccurrenceIter<'a> {
    nodes: &'a [OccurrenceTreeNode],
    stack: Vec<usize>,
    cursor: Option<usize>,
}

impl<'a> OccurrenceIter<'a> {
    fn new(nodes: &'a [OccurrenceTreeNode], root: Option<usize>) -> Self {
        Self {
            nodes,
            stack: Vec::new(),
            cursor: root,
        }
    }
}

impl Iterator for OccurrenceIter<'_> {
    type Item = Occurrence;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.cursor {
            self.stack.push(node);
            self.cursor = self.nodes[node].left();
        }
        let node = self.stack.pop()?;
        self.cursor = self.nodes[node].right();
        Some(self.nodes[node].occurrence)
    }
}

struct ParNode {
    occurrence_root: Option<usize>,
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
    repr: OwnedPlannedEPathMap,
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
    byte_pool: Vec<u8>,
    scores: Vec<ScoreNode>,
    protobuf_nodes: Vec<ProtoNode>,
    epathmaps: Vec<PlannedEPathMapNode>,
    components: Vec<ComponentNode>,
    occurrence_nodes: Vec<OccurrenceTreeNode>,
    pars: Vec<ParNode>,
    occurrence_path: Vec<(usize, bool)>,
    occurrence_undo: Vec<(usize, OccurrenceTreeNode)>,
    occurrence_seal: Vec<(usize, bool)>,
}

impl CanonicalScoreAlgebra for ParConstruction {
    type Score = PlannedScore;
    type Error = ParConstructionError;

    fn integer(&mut self, value: i64) -> Result<Self::Score, Self::Error> { self.score_i64(value) }

    fn string(&mut self, value: &str) -> Result<Self::Score, Self::Error> {
        self.score_string(value)
    }

    fn bytes(&mut self, value: &[u8]) -> Result<Self::Score, Self::Error> {
        self.score_bytes(value)
    }

    fn node(&mut self, children: Vec<Self::Score>) -> Result<Self::Score, Self::Error> {
        self.score_node(children)
    }

    fn constructor(
        &mut self,
        constructor: i32,
        children: Vec<Self::Score>,
    ) -> Result<Self::Score, Self::Error> {
        self.score_constructor(constructor, children)
    }
}

impl Default for ParConstruction {
    fn default() -> Self { Self::new() }
}

impl ParConstruction {
    pub fn new() -> Self {
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::CreateArena);
        Self {
            byte_fragments: Vec::new(),
            byte_pool: Vec::new(),
            scores: Vec::new(),
            protobuf_nodes: Vec::new(),
            epathmaps: Vec::new(),
            components: Vec::new(),
            occurrence_nodes: Vec::new(),
            pars: Vec::new(),
            occurrence_path: Vec::new(),
            occurrence_undo: Vec::new(),
            occurrence_seal: Vec::new(),
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
                ScorePiece::OccurrenceScores { encoded_len, .. } => encoded_len,
                ScorePiece::EscapedWire { encoded_len, .. } => encoded_len,
            };
            encoded_len = encoded_len
                .checked_add(add)
                .ok_or(ParConstructionError::LengthOverflow("canonical score"))?;
        }
        let id = PlannedScore(self.scores.len());
        self.scores.push(ScoreNode {
            pieces: pieces.into(),
            encoded_len,
        });
        Ok(id)
    }

    fn score_from_encoded(
        &mut self,
        canonical_score: &[u8],
    ) -> Result<PlannedScore, ParConstructionError> {
        self.score_from_owned(canonical_score.to_vec())
    }

    fn score_from_owned(
        &mut self,
        canonical_score: impl Into<Box<[u8]>>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let fragment = self.bytes(canonical_score);
        let id = PlannedScore(self.scores.len());
        self.scores.push(ScoreNode {
            pieces: ScoreProgram::Literal(fragment),
            encoded_len: self.byte_fragments[fragment].len(),
        });
        Ok(id)
    }

    fn pooled_score(&mut self, span: ByteSpan) -> PlannedScore {
        let id = PlannedScore(self.scores.len());
        self.scores.push(ScoreNode {
            pieces: ScoreProgram::Pooled(span),
            encoded_len: span.end - span.start,
        });
        id
    }

    /// Import one already-computed canonical score tree into the fragment
    /// arena. This is used only for bounded non-`Par` leaves such as `Var` and
    /// unforgeables; recursive child scores remain arena edges.
    pub fn score_tree(
        &mut self,
        canonical_score: &Tree<ScoreAtom>,
    ) -> Result<PlannedScore, ParConstructionError> {
        let start = self.byte_pool.len();
        encode_order_key(canonical_score, &mut self.byte_pool);
        Ok(self.pooled_score(ByteSpan {
            start,
            end: self.byte_pool.len(),
        }))
    }

    /// One canonical integer score leaf.
    pub fn score_i64(&mut self, value: i64) -> Result<PlannedScore, ParConstructionError> {
        let start = self.byte_pool.len();
        self.byte_pool.extend_from_slice(&[1, 1]);
        self.byte_pool
            .extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
        Ok(self.pooled_score(ByteSpan {
            start,
            end: self.byte_pool.len(),
        }))
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
        self.byte_pool
            .len()
            .checked_add(capacity)
            .ok_or(ParConstructionError::LengthOverflow("canonical score pool"))?;
        let start = self.byte_pool.len();
        let bytes = &mut self.byte_pool;
        bytes.reserve(capacity);
        bytes.extend_from_slice(&[1, kind]);
        for &byte in value {
            if byte == 0 {
                bytes.extend_from_slice(&[0, 0xff]);
            } else {
                bytes.push(byte);
            }
        }
        bytes.extend_from_slice(&[0, 0]);
        Ok(self.pooled_score(ByteSpan {
            start,
            end: self.byte_pool.len(),
        }))
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

    /// Whether a planned value is semantically equal to the canonical empty
    /// `Par`. Root `locally_free` is deliberately ignored by `Par` equality.
    ///
    /// Optional receive conditions and match guards collapse an empty value to
    /// absence in the production sorter.  Planned construction must therefore
    /// decide presence from the planned child, not from the schema shell's
    /// deliberately empty placeholder.
    pub fn is_semantic_default(&self, par: PlannedPar) -> bool {
        let node = &self.pars[par.0];
        semantics::par_is_semantic_default(node.occurrence_root.is_some(), node.connective_used)
    }

    /// Multiplicity of one top-level constructor family without decoding any
    /// component body. The public projection matches `Par::family_cardinality`
    /// by saturating at `u64::MAX`.
    pub fn family_cardinality<T: ParBagComponent>(&self, par: PlannedPar) -> u64 {
        OccurrenceIter::new(&self.occurrence_nodes, self.pars[par.0].occurrence_root)
            .filter(|occurrence| self.components[occurrence.component.0].tag == T::TAG)
            .fold(0u64, |sum, occurrence| {
                sum.saturating_add(occurrence.multiplicity.get())
            })
    }

    /// The one planned Bundle when this Par contains exactly that occurrence.
    pub fn single_bundle(&self, par: PlannedPar) -> Option<(PlannedPar, bool, bool)> {
        let node = &self.pars[par.0];
        if self.occurrence_distinct_len(node.occurrence_root) != 1 {
            return None;
        }
        let occurrence = self.occurrence_nodes[node.occurrence_root?].occurrence;
        if occurrence.multiplicity != Multiplicity::ONE {
            return None;
        }
        match &self.components[occurrence.component.0].semantic_shape {
            ComponentSemanticShape::Bundle {
                body,
                write_flag,
                read_flag,
            } => Some((*body, *write_flag, *read_flag)),
            _ => None,
        }
    }

    /// The one planned Connective when this Par contains exactly that
    /// occurrence. The returned children are borrowed arena handles.
    pub fn single_connective(&self, par: PlannedPar) -> Option<PlannedConnective<'_>> {
        let node = &self.pars[par.0];
        if self.occurrence_distinct_len(node.occurrence_root) != 1 {
            return None;
        }
        let occurrence = self.occurrence_nodes[node.occurrence_root?].occurrence;
        if occurrence.multiplicity != Multiplicity::ONE {
            return None;
        }
        match &self.components[occurrence.component.0].semantic_shape {
            ComponentSemanticShape::ConnectiveAnd(children) => {
                Some(PlannedConnective::And(children))
            }
            ComponentSemanticShape::ConnectiveOr(children) => Some(PlannedConnective::Or(children)),
            ComponentSemanticShape::ConnectiveOther => Some(PlannedConnective::Other),
            _ => None,
        }
    }

    /// Visit the distinct top-level expression endpoints of a planned Par in
    /// canonical component order. Multiplicity is intentionally quotiented:
    /// pure decidability classification of one occurrence classifies every
    /// identical occurrence.
    pub fn visit_expr_shapes(&self, par: PlannedPar, mut visit: impl FnMut(PlannedExprShape<'_>)) {
        for occurrence in
            OccurrenceIter::new(&self.occurrence_nodes, self.pars[par.0].occurrence_root)
        {
            let component = &self.components[occurrence.component.0];
            if component.tag != ParComponentTag::Expr {
                continue;
            }
            visit(match &component.semantic_shape {
                ComponentSemanticShape::Expr(PlannedExprSemantic::Inert) => PlannedExprShape::Inert,
                ComponentSemanticShape::Expr(PlannedExprSemantic::Unary(child)) => {
                    PlannedExprShape::Unary(*child)
                }
                ComponentSemanticShape::Expr(PlannedExprSemantic::Binary(left, right)) => {
                    PlannedExprShape::Binary(*left, *right)
                }
                ComponentSemanticShape::Expr(PlannedExprSemantic::MatchesTarget(target)) => {
                    PlannedExprShape::MatchesTarget(*target)
                }
                ComponentSemanticShape::Expr(PlannedExprSemantic::Unsupported(value)) => {
                    PlannedExprShape::Unsupported {
                        kind: value.kind,
                        detail: value.detail.as_deref(),
                    }
                }
                _ => PlannedExprShape::Opaque,
            });
        }
    }

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

    /// Plan the exact sorter score and protobuf tie-break for one receive bind.
    /// Child order is patterns followed by source, matching the generated
    /// schema walk.
    pub fn receive_bind_order(
        &mut self,
        shell: &ReceiveBind,
        patterns: &[PlannedPar],
        source: PlannedPar,
    ) -> Result<PlannedReceiveBindOrder, ParConstructionError> {
        let mut children = Vec::with_capacity(patterns.len() + 1);
        children.extend_from_slice(patterns);
        children.push(source);
        let protobuf = self.plan_protobuf(shell, &children)?;
        let remainder = match &shell.remainder {
            Some(var) => self.score_tree(&sort_var(var).score)?,
            None => self.score_i64(Score::ABSENT as i64)?,
        };
        let score_children: Vec<_> = std::iter::once(self.par_score(source))
            .chain(patterns.iter().map(|pattern| self.par_score(*pattern)))
            .chain(std::iter::once(remainder))
            .collect();
        let score = self.score_node(score_children)?;
        Ok(PlannedReceiveBindOrder { score, protobuf })
    }

    /// Exact total ordering used by `ScoredTerm::sort_vec` for receive binds.
    pub fn compare_receive_bind_orders(
        &self,
        left: PlannedReceiveBindOrder,
        right: PlannedReceiveBindOrder,
    ) -> Ordering {
        ScoreByteCursor::new(self, left.score)
            .cmp(ScoreByteCursor::new(self, right.score))
            .then_with(|| {
                WireByteCursor::new(self, WireRoot::Proto(left.protobuf))
                    .cmp(WireByteCursor::new(self, WireRoot::Proto(right.protobuf)))
            })
    }

    /// Emit the exact canonical PathMap key of a planned Par directly from the
    /// construction graph. Stable collection segments traverse child handles;
    /// unstable positions stream the already-planned protobuf body into the
    /// escape arm. No nested resident `Par` is constructed.
    pub fn canonical_path_bytes(&self, root: PlannedPar) -> Result<Vec<u8>, ParConstructionError> {
        let mut output = Vec::new();
        self.emit_canonical_path_into(root, &mut output)?;
        Ok(output)
    }

    /// Emit one canonical trie segment for a planned `Par`.
    ///
    /// Unlike [`Self::canonical_path_bytes`], this never applies the top-level
    /// split-list rule. It is the required operation when a substituted `Par`
    /// remains one element of an enclosing split PathMap key.
    pub fn canonical_segment_bytes(
        &self,
        root: PlannedPar,
    ) -> Result<Vec<u8>, ParConstructionError> {
        let mut output = Vec::new();
        self.emit_canonical_segment_into(root, &mut output)?;
        Ok(output)
    }

    /// Append one complete canonical PathMap key directly to an existing byte
    /// sink, applying the top-level split-list rule.
    pub fn emit_canonical_path_into(
        &self,
        root: PlannedPar,
        output: &mut Vec<u8>,
    ) -> Result<(), ParConstructionError> {
        self.emit_canonical_key_into(root, true, output)
    }

    /// Append exactly one canonical trie segment directly to an existing byte
    /// sink, without applying the top-level split-list rule.
    pub fn emit_canonical_segment_into(
        &self,
        root: PlannedPar,
        output: &mut Vec<u8>,
    ) -> Result<(), ParConstructionError> {
        self.emit_canonical_key_into(root, false, output)
    }

    fn emit_canonical_key_into(
        &self,
        root: PlannedPar,
        apply_top_level_split: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), ParConstructionError> {
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

        let mut work = vec![if apply_top_level_split {
            Op::Path(root)
        } else {
            Op::Segment(root)
        }];
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
                        push_uv(out, node.protobuf_len as u64);
                        self.emit_par(par, out);
                        continue;
                    }
                    let occurrence = self.occurrence_nodes[node
                        .occurrence_root
                        .expect("a stable Par has one component occurrence")]
                    .occurrence;
                    let component = &self.components[occurrence.component.0];
                    match component
                        .canonical_segment
                        .as_ref()
                        .expect("a newly planned stable component has a segment program")
                    {
                        CanonicalSegment::Bytes(bytes) => out.extend_from_slice(bytes),
                        CanonicalSegment::PooledBytes(span) => {
                            out.extend_from_slice(span.view(&self.byte_pool))
                        }
                        CanonicalSegment::List(children) => {
                            out.push(path_tag::ELIST);
                            push_uv(out, children.len() as u64);
                            for &child in children.iter().rev() {
                                work.push(Op::Segment(child));
                            }
                        }
                        CanonicalSegment::Tuple(children) => {
                            out.push(path_tag::ETUPLE);
                            push_uv(out, children.len() as u64);
                            for &child in children.iter().rev() {
                                work.push(Op::Segment(child));
                            }
                        }
                        CanonicalSegment::EPathMap(map) => {
                            let map_node = &self.epathmaps[map.0];
                            match map_node.repr.as_repr() {
                                EPathMapRepr::Empty => {
                                    out.push(path_tag::EPATHMAP);
                                    push_uv(out, 0);
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
                                    push_uv(out, region_len as u64);
                                    let mut zipper = entries.read_zipper();
                                    while zipper.to_next_val() {
                                        let key = zipper.path();
                                        push_uv(out, key.len() as u64);
                                        out.extend_from_slice(key);
                                    }
                                }
                                EPathMapRepr::Map(_) => {
                                    out.push(path_tag::EPATHMAP_MAP);
                                    push_uv(out, map_node.snapshot_len as u64);
                                    self.emit_wire(WireRoot::EPathMapSnapshot(*map), out);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn push_proto_node(
        &mut self,
        pieces: impl Into<ProtoProgram>,
    ) -> Result<usize, ParConstructionError> {
        let pieces = pieces.into();
        let mut encoded_len = 0usize;
        let mut zero_count = 0usize;
        for piece in pieces.as_slice() {
            let opaque_bytes: Option<&[u8]> = match piece {
                ProtoPiece::Bytes(bytes) => Some(bytes),
                ProtoPiece::PooledBytes(span) => Some(span.view(&self.byte_pool)),
                ProtoPiece::SharedBytes(bytes) => Some(bytes),
                _ => None,
            };
            if let Some(bytes) = opaque_bytes {
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
            let (child_len, child_zeroes, prefix) = match piece {
                ProtoPiece::Bytes(_) | ProtoPiece::PooledBytes(_) | ProtoPiece::SharedBytes(_) => {
                    unreachable!()
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
        self.push_proto_node(ProtoProgram::One(ProtoPiece::Node { tag, node: child }))
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
            semantic_shape: ComponentSemanticShape::Opaque,
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

        let repr = self.build_epm_entries(shell.mode(), par_children)?;

        self.planned_epathmap_repr(
            repr,
            &shell.locally_free,
            shell.connective_used,
            shell.remainder.as_ref(),
        )
    }

    /// Finalize a homogeneous planned EPathMap representation into its EPM1
    /// snapshot cursor and protobuf node. Both decoded-shell imports and
    /// normalizer-native set construction share this path.
    fn planned_epathmap_repr(
        &mut self,
        repr: OwnedPlannedEPathMap,
        locally_free: &[u8],
        connective_used: bool,
        remainder: Option<&crate::rhoapi::Var>,
    ) -> Result<(PlannedEPathMap, usize), ParConstructionError> {
        let entries_stable = match repr.as_repr() {
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

        let layout = epathmap_trie_codec::layout(repr.as_repr());
        let prefix = layout.prefix().to_vec().into_boxed_slice();
        let mut value_order = Vec::with_capacity(layout.value_count());
        if let EPathMapRepr::Map(entries) = repr.as_repr() {
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
        if !locally_free.is_empty() {
            prost::encoding::bytes::encode(3, &locally_free.to_vec(), &mut bounded);
        }
        if connective_used {
            prost::encoding::bool::encode(4, &true, &mut bounded);
        }
        if !bounded.is_empty() {
            pieces.push(ProtoPiece::Bytes(bounded.into_boxed_slice()));
        }
        if let Some(remainder) = remainder {
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

    pub fn epathmap_expr_component(
        &mut self,
        mode: EPathMapMode,
        children: &[PlannedPar],
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<crate::rhoapi::Var>,
    ) -> Result<PlannedComponent, ParConstructionError> {
        match mode {
            EPathMapMode::Empty => {
                Self::require_arity("empty EPathMap", 0, children.len())?;
            }
            EPathMapMode::Set => {}
            EPathMapMode::Map => {
                if !children.len().is_multiple_of(2) {
                    return Err(ParConstructionError::ChildArity {
                        context: "map EPathMap",
                        expected: children.len().saturating_add(1),
                        supplied: children.len(),
                    });
                }
            }
        }
        let repr = self.build_epm_entries(mode, children)?;
        self.owned_epathmap_repr_expr_component(repr, locally_free, connective_used, remainder)
    }

    /// Finish an already prefix-compressed set/map construction as one Expr.
    /// Map values remain arena handles until final emission.
    pub fn epathmap_repr_expr_component(
        &mut self,
        repr: EPathMapRepr<PlannedPar>,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<crate::rhoapi::Var>,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let repr = self.own_epm_repr(repr);
        self.owned_epathmap_repr_expr_component(repr, locally_free, connective_used, remainder)
    }

    pub(crate) fn owned_epathmap_repr_expr_component(
        &mut self,
        repr: OwnedPlannedEPathMap,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<crate::rhoapi::Var>,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let (planned_map, map_protobuf) =
            self.planned_epathmap_repr(repr, &locally_free, connective_used, remainder.as_ref())?;
        let map_node = &self.epathmaps[planned_map.0];
        let snapshot_score = self.score_wire_bytes(
            WireRoot::EPathMapSnapshot(planned_map),
            map_node.snapshot_len,
            map_node.snapshot_zero_count,
        )?;
        let score = semantics::rpb3_pathmap_score(self, false, snapshot_score, connective_used)?;
        let expr_protobuf = self.wrap_proto_node(32, map_protobuf)?;
        let stable = remainder.is_none()
            && locally_free.is_empty()
            && !connective_used
            && self.epathmaps[planned_map.0].entries_stable;
        let component = self.component_from_proto(
            ParComponentTag::Expr,
            score,
            expr_protobuf,
            stable,
            stable.then_some(CanonicalSegment::EPathMap(planned_map)),
        );
        self.components[component.0].semantic_shape = ComponentSemanticShape::Expr(
            PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                kind: "EPathmapBody",
                detail: None,
            })),
        );
        Ok(component)
    }

    pub(crate) fn owned_ezipper_repr_expr_component(
        &mut self,
        shell: &EZipper,
        repr: OwnedPlannedEPathMap,
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<crate::rhoapi::Var>,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let (planned_map, map_protobuf) =
            self.planned_epathmap_repr(repr, &locally_free, connective_used, remainder.as_ref())?;
        self.ezipper_component(shell, planned_map, map_protobuf)
    }

    fn ezipper_component(
        &mut self,
        zipper: &EZipper,
        planned_map: PlannedEPathMap,
        map_protobuf: usize,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let map_node = &self.epathmaps[planned_map.0];
        let snapshot_score = self.score_wire_bytes(
            WireRoot::EPathMapSnapshot(planned_map),
            map_node.snapshot_len,
            map_node.snapshot_zero_count,
        )?;
        let score =
            semantics::rpb3_pathmap_score(self, true, snapshot_score, zipper.connective_used)?;
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
        pieces.extend(std::mem::take(&mut self.protobuf_nodes[tail].pieces).into_vec());
        let zipper_protobuf = self.push_proto_node(pieces)?;
        let expr_protobuf = self.wrap_proto_node(33, zipper_protobuf)?;
        Ok(self.component_from_proto(ParComponentTag::Expr, score, expr_protobuf, false, None))
    }

    /// Plan an EPathMap set expression directly from canonical planned keys.
    ///
    /// This is the normalizer's `{| p1, …, pn |}` path: each key is emitted by
    /// the canonical-path PDA straight into `PathMap<()>`; no child is finished
    /// to a resident `Par`, and no decoded entry vector becomes authority.
    pub fn epathmap_set_expr_component(
        &mut self,
        keys: &[PlannedPar],
        locally_free: Vec<u8>,
        connective_used: bool,
        remainder: Option<crate::rhoapi::Var>,
    ) -> Result<PlannedComponent, ParConstructionError> {
        self.epathmap_expr_component(
            EPathMapMode::Set,
            keys,
            locally_free,
            connective_used,
            remainder,
        )
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
            semantic_shape: ComponentSemanticShape::Opaque,
        });
        Ok(id)
    }

    /// Import one already-canonical component as an opaque construction leaf.
    /// Its RPB3 score and protobuf slices are copied once from the compiled key;
    /// no recursively nested `Par` is decoded or made resident in the arena.
    pub fn import_component<T: ParBagComponent>(
        &mut self,
        value: &T,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let key = CanonicalParBagKey::compile(value);
        let (encoded_score, encoded_protobuf) = key.parts();
        let score = self.score_from_encoded(encoded_score)?;
        let protobuf = self.push_proto_node(if encoded_protobuf.is_empty() {
            ProtoProgram::default()
        } else {
            ProtoProgram::One(ProtoPiece::Bytes(encoded_protobuf.into()))
        })?;
        Ok(self.component_from_proto(T::TAG, score, protobuf, key.canonical_path_stable(), None))
    }

    /// Import an authoritative resident runtime zipper without decoding its
    /// protobuf body or traversing its embedded PathMap.  The opaque token is
    /// constructible only by `ParBag`'s typed RPB3 visitor, so its score and
    /// body cannot be mixed with bytes from a different endpoint.
    pub fn import_canonical_ezipper(
        &mut self,
        canonical: OwnedCanonicalEzipper,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let (encoded_score, encoded_protobuf) = canonical.into_parts();
        let score = self.score_from_owned(encoded_score)?;
        let protobuf = self.push_proto_node(if encoded_protobuf.is_empty() {
            ProtoProgram::default()
        } else {
            ProtoProgram::One(ProtoPiece::Bytes(encoded_protobuf))
        })?;
        Ok(self.component_from_proto(ParComponentTag::Expr, score, protobuf, false, None))
    }

    /// Import a trusted canonical EZipper protobuf region that is nested below
    /// another component and therefore has no enclosing RPB3 score prefix.
    /// The score is derived directly from the borrowed EPM1 snapshot and the
    /// zipper connective bit; PathMap values and cursor paths are skipped by
    /// length and the original protobuf body is retained byte-for-byte.
    ///
    /// `encoded_protobuf` must descend from a component key admitted by RPB3's
    /// full typed-key validation. This method is public only because the
    /// substitution PDA lives in the sibling `rholang` crate; it is not an
    /// untrusted protobuf entry point. Untrusted bytes must first pass the
    /// ordinary generated decoder and canonical key recompilation.
    pub fn import_canonical_ezipper_region(
        &mut self,
        encoded_protobuf: &[u8],
    ) -> Result<(PlannedComponent, Vec<u8>, bool), ParConstructionError> {
        let parts = canonical_ezipper_region_parts(encoded_protobuf)
            .ok_or(ParConstructionError::InvalidCanonicalEzipperRegion)?;
        let locally_free = parts.locally_free.to_vec();
        let connective_used = parts.connective_used;
        let snapshot_score = self.score_bytes(parts.snapshot)?;
        let score = semantics::rpb3_pathmap_score(self, true, snapshot_score, connective_used)?;
        let protobuf = self.push_proto_node(if encoded_protobuf.is_empty() {
            ProtoProgram::default()
        } else {
            ProtoProgram::One(ProtoPiece::Bytes(encoded_protobuf.into()))
        })?;
        Ok((
            self.component_from_proto(ParComponentTag::Expr, score, protobuf, false, None),
            locally_free,
            connective_used,
        ))
    }

    pub fn incomplete_component<T: ParBagComponent>(
        &mut self,
        shell: &T,
        wire_children: &[PlannedPar],
    ) -> Result<Option<PlannedComponent>, ParConstructionError> {
        if T::semantic_shape_is_complete(shell) {
            return Ok(None);
        }
        let score = semantics::nonsemantic_score(self)?;
        self.component(shell, wire_children, score, false).map(Some)
    }

    pub fn import_admitted_incomplete_component_region<T: ParBagComponent>(
        &mut self,
        shallow_shell: &T,
        region: super::rholang::protobuf_cursor::ProtobufRegion,
    ) -> Result<Option<PlannedComponent>, ParConstructionError> {
        if T::semantic_shape_is_complete(shallow_shell) {
            return Ok(None);
        }
        let score = semantics::nonsemantic_score(self)?;
        let encoded_protobuf = region.into_bytes();
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::ImportShared);
        let protobuf = self.push_proto_node(if encoded_protobuf.is_empty() {
            ProtoProgram::default()
        } else {
            ProtoProgram::One(ProtoPiece::SharedBytes(Box::new(encoded_protobuf)))
        })?;
        Ok(Some(self.component_from_proto(
            T::TAG,
            score,
            protobuf,
            false,
            None,
        )))
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
        let channel = self.par_score(children[0]);
        let data = children[1..]
            .iter()
            .map(|child| self.par_score(*child))
            .collect();
        let region_score = shell
            .cost_region
            .as_ref()
            .map(|region| semantics::cost_region_score(self, region))
            .transpose()?;
        let score = semantics::send_score(
            self,
            shell.persistent,
            channel,
            data,
            shell.connective_used,
            region_score,
        )?;
        self.component(shell, children, score, false)
    }

    /// Plan a canonical Bundle from its single body.
    pub fn bundle_component(
        &mut self,
        shell: &Bundle,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("Bundle", 1, children.len())?;
        let body = self.par_score(children[0]);
        let score = semantics::bundle_score(self, shell.read_flag, shell.write_flag, body)?;
        let component = self.component(shell, children, score, false)?;
        self.components[component.0].semantic_shape = ComponentSemanticShape::Bundle {
            body: children[0],
            write_flag: shell.write_flag,
            read_flag: shell.read_flag,
        };
        Ok(component)
    }

    /// Plan a canonical conditional in condition/true/false order.
    pub fn if_component(
        &mut self,
        shell: &If,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("If", 3, children.len())?;
        let condition = self.par_score(children[0]);
        let if_true = self.par_score(children[1]);
        let if_false = self.par_score(children[2]);
        let score = semantics::if_score(self, condition, if_true, if_false, shell.connective_used)?;
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

    pub fn cost_token_stack_component(
        &mut self,
        shell: &crate::rhoapi::CostTokenStack,
        children: &[PlannedPar],
    ) -> Result<PlannedComponent, ParConstructionError> {
        Self::require_arity("CostTokenStack", 0, children.len())?;
        let score = semantics::cost_token_stack_score(self, shell)?;
        self.component(shell, children, score, false)
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
                semantics::ConnectiveScore::And(
                    children
                        .iter()
                        .map(|child| self.par_score(*child))
                        .collect(),
                )
            }
            Some(ConnectiveInstance::ConnOrBody(or)) => {
                Self::require_arity("Connective::Or", or.ps.len(), children.len())?;
                semantics::ConnectiveScore::Or(
                    children
                        .iter()
                        .map(|child| self.par_score(*child))
                        .collect(),
                )
            }
            Some(ConnectiveInstance::ConnNotBody(_)) => {
                Self::require_arity("Connective::Not", 1, children.len())?;
                semantics::ConnectiveScore::Not(self.par_score(children[0]))
            }
            Some(ConnectiveInstance::VarRefBody(var)) => {
                Self::require_arity("Connective::VarRef", 0, children.len())?;
                semantics::ConnectiveScore::VarRef {
                    index: var.index,
                    depth: var.depth,
                }
            }
            Some(ConnectiveInstance::ConnBool(value)) => {
                Self::require_arity("Connective::Bool", 0, children.len())?;
                semantics::ConnectiveScore::Bool(*value)
            }
            Some(ConnectiveInstance::ConnInt(value)) => {
                Self::require_arity("Connective::Int", 0, children.len())?;
                semantics::ConnectiveScore::Int(*value)
            }
            Some(ConnectiveInstance::ConnString(value)) => {
                Self::require_arity("Connective::String", 0, children.len())?;
                semantics::ConnectiveScore::String(*value)
            }
            Some(ConnectiveInstance::ConnUri(value)) => {
                Self::require_arity("Connective::Uri", 0, children.len())?;
                semantics::ConnectiveScore::Uri(*value)
            }
            Some(ConnectiveInstance::ConnByteArray(value)) => {
                Self::require_arity("Connective::ByteArray", 0, children.len())?;
                semantics::ConnectiveScore::ByteArray(*value)
            }
            None => {
                Self::require_arity("absent Connective", 0, children.len())?;
                semantics::ConnectiveScore::Absent
            }
        };
        let score = semantics::connective_score(self, score)?;
        let component = self.component(shell, children, score, false)?;
        self.components[component.0].semantic_shape = match shell.connective_instance.as_ref() {
            Some(ConnectiveInstance::ConnAndBody(_)) => {
                ComponentSemanticShape::ConnectiveAnd(children.into())
            }
            Some(ConnectiveInstance::ConnOrBody(_)) => {
                ComponentSemanticShape::ConnectiveOr(children.into())
            }
            _ => ComponentSemanticShape::ConnectiveOther,
        };
        Ok(component)
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

        let mut uri_scores = Vec::new();
        if canonical_shell.uri.is_empty() {
            uri_scores.push(self.score_i64(Score::ABSENT as i64)?);
        } else {
            for uri in &canonical_shell.uri {
                uri_scores.push(self.score_string(uri)?);
            }
        }
        let mut injection_scores = Vec::new();
        if shell.injections.is_empty() {
            injection_scores.push(self.score_i64(Score::ABSENT as i64)?);
        } else {
            for ((key, _), value) in shell.injections.iter().zip(&children[1..]) {
                let value = self.par_score(*value);
                injection_scores.push(semantics::injection_score(self, key, value)?);
            }
        }
        let body = self.par_score(children[0]);
        let score =
            semantics::new_score(self, shell.bind_count, uri_scores, injection_scores, body)?;
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
        let target_score = self.par_score(target);
        let mut case_scores = Vec::with_capacity(shell.cases.len());
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
            let pattern_score = self.par_score(pattern);
            let source_score = self.par_score(source);
            let guard_score = self.par_score(guard);
            case_scores.push(semantics::match_case_score(
                self,
                pattern_score,
                source_score,
                source_case.free_count,
                guard_score,
            )?);
            wire_children.extend([pattern, source]);
            let guard_present = source_case.guard.is_some() && !self.is_semantic_default(guard);
            if guard_present {
                wire_children.push(guard);
            } else {
                canonical_case.guard = None;
            }
        }
        let score = semantics::match_score(self, target_score, case_scores, shell.connective_used)?;
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

        let mut bind_scores = Vec::with_capacity(shell.binds.len());
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
            let source_score = self.par_score(source);
            let pattern_scores = patterns
                .iter()
                .map(|pattern| self.par_score(*pattern))
                .collect();
            let region_score = bind
                .cost_region
                .as_ref()
                .map(|region| semantics::cost_region_score(self, region))
                .transpose()?;
            bind_scores.push(semantics::receive_bind_score(
                self,
                source_score,
                pattern_scores,
                remainder,
                region_score,
            )?);
            wire_children.extend(patterns.iter().copied());
            wire_children.push(source);
        }

        let body = children[cursor];
        let condition = children[cursor + 1];
        wire_children.push(body);
        let condition_present = shell.condition.is_some() && !self.is_semantic_default(condition);
        let mut canonical_shell = shell.clone();
        if condition_present {
            wire_children.push(condition);
        } else {
            canonical_shell.condition = None;
        }
        let body_score = self.par_score(body);
        let condition_score = self.par_score(condition);
        let region_score = shell
            .cost_region
            .as_ref()
            .map(|region| semantics::cost_region_score(self, region))
            .transpose()?;
        let score = semantics::receive_score(
            self,
            shell.persistent,
            shell.peek,
            bind_scores,
            body_score,
            shell.bind_count,
            shell.connective_used,
            condition_score,
            region_score,
        )?;
        self.component(&canonical_shell, &wire_children, score, false)
    }

    fn ground_expr_component(
        &mut self,
        shell: &Expr,
    ) -> Result<PlannedComponent, ParConstructionError> {
        let score_start = self.byte_pool.len();
        assert!(super::par_bag::append_leaf_expr_score(
            shell,
            &mut self.byte_pool
        ));
        let score_span = ByteSpan {
            start: score_start,
            end: self.byte_pool.len(),
        };
        let protobuf_start = self.byte_pool.len();
        assert!(super::par_bag::append_shallow_protobuf(
            shell,
            &mut self.byte_pool
        ));
        let protobuf_span = ByteSpan {
            start: protobuf_start,
            end: self.byte_pool.len(),
        };
        let segment_start = self.byte_pool.len();
        let stable = Self::append_ground_segment(shell, &mut self.byte_pool);
        let canonical_segment = stable.then_some(CanonicalSegment::PooledBytes(ByteSpan {
            start: segment_start,
            end: self.byte_pool.len(),
        }));
        let score = self.pooled_score(score_span);
        let protobuf = self.push_proto_node(if protobuf_span.start == protobuf_span.end {
            ProtoProgram::default()
        } else {
            ProtoProgram::One(ProtoPiece::PooledBytes(protobuf_span))
        })?;
        let component = self.component_from_proto(
            ParComponentTag::Expr,
            score,
            protobuf,
            stable,
            canonical_segment,
        );
        self.components[component.0].semantic_shape =
            ComponentSemanticShape::Expr(PlannedExprSemantic::Inert);
        Ok(component)
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
        self.components[component.0].semantic_shape =
            ComponentSemanticShape::Expr(match shell.expr_instance.as_ref() {
                Some(ExprInstance::ENotBody(_)) | Some(ExprInstance::ENegBody(_)) => {
                    PlannedExprSemantic::Unary(par_children[0])
                }
                Some(ExprInstance::EMultBody(_))
                | Some(ExprInstance::EDivBody(_))
                | Some(ExprInstance::EModBody(_))
                | Some(ExprInstance::EPlusBody(_))
                | Some(ExprInstance::EMinusBody(_))
                | Some(ExprInstance::ELtBody(_))
                | Some(ExprInstance::ELteBody(_))
                | Some(ExprInstance::EGtBody(_))
                | Some(ExprInstance::EGteBody(_))
                | Some(ExprInstance::EEqBody(_))
                | Some(ExprInstance::ENeqBody(_))
                | Some(ExprInstance::EAndBody(_))
                | Some(ExprInstance::EOrBody(_)) => {
                    PlannedExprSemantic::Binary(par_children[0], par_children[1])
                }
                Some(ExprInstance::EMatchesBody(_)) => {
                    PlannedExprSemantic::MatchesTarget(par_children[0])
                }
                Some(ExprInstance::EMethodBody(method)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EMethodBody",
                        detail: Some(format!("`{}`", method.method_name)),
                    }))
                }
                Some(ExprInstance::EPercentPercentBody(_)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EPercentPercentBody",
                        detail: None,
                    }))
                }
                Some(ExprInstance::EPlusPlusBody(_)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EPlusPlusBody",
                        detail: None,
                    }))
                }
                Some(ExprInstance::EMinusMinusBody(_)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EMinusMinusBody",
                        detail: None,
                    }))
                }
                Some(ExprInstance::EPathmapBody(_)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EPathmapBody",
                        detail: None,
                    }))
                }
                Some(ExprInstance::EZipperBody(_)) => {
                    PlannedExprSemantic::Unsupported(Box::new(UnsupportedExpr {
                        kind: "EZipperBody",
                        detail: None,
                    }))
                }
                Some(ExprInstance::GBool(_))
                | Some(ExprInstance::GInt(_))
                | Some(ExprInstance::GString(_))
                | Some(ExprInstance::GUri(_))
                | Some(ExprInstance::GByteArray(_))
                | Some(ExprInstance::GDouble(_))
                | Some(ExprInstance::GBigInt(_))
                | Some(ExprInstance::GBigRat(_))
                | Some(ExprInstance::GFixedPoint(_))
                | Some(ExprInstance::EListBody(_))
                | Some(ExprInstance::ETupleBody(_))
                | Some(ExprInstance::ESetBody(_))
                | Some(ExprInstance::EMapBody(_))
                | Some(ExprInstance::EVarBody(_))
                | None => PlannedExprSemantic::Inert,
            });
        Ok(component)
    }

    fn append_ground_segment(shell: &Expr, bytes: &mut Vec<u8>) -> bool {
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
            return false;
        };
        match instance {
            ExprInstance::GBool(value) => {
                bytes.push(if *value {
                    path_tag::GBOOL_TRUE
                } else {
                    path_tag::GBOOL_FALSE
                });
            }
            ExprInstance::GInt(value) => {
                bytes.push(path_tag::GINT);
                push_uv(bytes, ((*value << 1) ^ (*value >> 63)) as u64);
            }
            ExprInstance::GString(value) => {
                bytes.push(path_tag::GSTRING);
                push_uv(bytes, value.len() as u64);
                bytes.extend_from_slice(value.as_bytes());
            }
            ExprInstance::GUri(value) => {
                bytes.push(path_tag::GURI);
                push_uv(bytes, value.len() as u64);
                bytes.extend_from_slice(value.as_bytes());
            }
            ExprInstance::GByteArray(value) => {
                bytes.push(path_tag::GBYTE_ARRAY);
                push_uv(bytes, value.len() as u64);
                bytes.extend_from_slice(value);
            }
            ExprInstance::GDouble(bits) => {
                bytes.push(path_tag::GDOUBLE);
                bytes.extend_from_slice(&bits.to_be_bytes());
            }
            ExprInstance::GBigInt(value) => {
                bytes.push(path_tag::GBIG_INT);
                push_uv(bytes, value.len() as u64);
                bytes.extend_from_slice(value);
            }
            ExprInstance::GBigRat(value) => {
                bytes.push(path_tag::GBIG_RAT);
                push_uv(bytes, value.numerator.len() as u64);
                bytes.extend_from_slice(&value.numerator);
                push_uv(bytes, value.denominator.len() as u64);
                bytes.extend_from_slice(&value.denominator);
            }
            ExprInstance::GFixedPoint(value) => {
                bytes.push(path_tag::GFIXED_POINT);
                push_uv(bytes, value.unscaled.len() as u64);
                bytes.extend_from_slice(&value.unscaled);
                push_uv(bytes, u64::from(value.scale));
            }
            _ => return false,
        }
        true
    }

    fn expr_canonical_segment(
        &self,
        shell: &Expr,
        par_children: &[PlannedPar],
    ) -> Result<(Option<CanonicalSegment>, bool), ParConstructionError> {
        let mut bytes = Vec::new();
        if Self::append_ground_segment(shell, &mut bytes) {
            return Ok((
                Some(CanonicalSegment::Bytes(bytes.into_boxed_slice())),
                true,
            ));
        }
        let Some(instance) = shell.expr_instance.as_ref() else {
            return Ok((None, false));
        };
        match instance {
            ExprInstance::EListBody(list)
                if list.remainder.is_none()
                    && list.locally_free.is_empty()
                    && !list.connective_used =>
            {
                Ok((
                    Some(CanonicalSegment::List(
                        par_children.to_vec().into_boxed_slice(),
                    )),
                    par_children
                        .iter()
                        .all(|child| self.pars[child.0].canonical_path_stable),
                ))
            }
            ExprInstance::ETupleBody(tuple)
                if tuple.locally_free.is_empty() && !tuple.connective_used =>
            {
                Ok((
                    Some(CanonicalSegment::Tuple(
                        par_children.to_vec().into_boxed_slice(),
                    )),
                    par_children
                        .iter()
                        .all(|child| self.pars[child.0].canonical_path_stable),
                ))
            }
            _ => Ok((None, false)),
        }
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
            return self.ground_expr_component(shell);
        };

        if matches!(
            instance,
            ExprInstance::ENegBody(_) | ExprInstance::ENotBody(_)
        ) {
            exact("unary Expr", 1, par_children.len())?;
            let child = self.par_score(par_children[0]);
            let score = semantics::unary_score(self, semantics::expr_constructor(instance), child)?;
            return self.schema_expr_component(shell, par_children, score);
        }

        if matches!(
            instance,
            ExprInstance::EMultBody(_)
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
                | ExprInstance::EMinusMinusBody(_)
        ) {
            exact("binary Expr", 2, par_children.len())?;
            let first = self.par_score(par_children[0]);
            let second = self.par_score(par_children[1]);
            let score = semantics::binary_score(
                self,
                semantics::expr_constructor(instance),
                first,
                second,
            )?;
            return self.schema_expr_component(shell, par_children, score);
        }

        match instance {
            ExprInstance::EListBody(list) => {
                exact("EList", list.ps.len(), par_children.len())?;
                let remainder = match &list.remainder {
                    Some(var) => self.score_tree(&sort_var(var).score)?,
                    None => self.score_i64(-1)?,
                };
                let elements = par_children
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                let score = semantics::ordered_collection_score(
                    self,
                    Score::ELIST,
                    Some(remainder),
                    elements,
                    list.connective_used,
                )?;
                self.schema_expr_component(shell, par_children, score)
            }
            ExprInstance::ETupleBody(tuple) => {
                exact("ETuple", tuple.ps.len(), par_children.len())?;
                let elements = par_children
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                let score = semantics::ordered_collection_score(
                    self,
                    Score::ETUPLE,
                    None,
                    elements,
                    tuple.connective_used,
                )?;
                self.schema_expr_component(shell, par_children, score)
            }
            ExprInstance::EMethodBody(method) => {
                exact("EMethod", method.arguments.len() + 1, par_children.len())?;
                let (&target, arguments) = par_children
                    .split_last()
                    .expect("EMethod arity proved one target");
                let target_score = self.par_score(target);
                let argument_scores = arguments
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                let score = semantics::method_score(
                    self,
                    &method.method_name,
                    target_score,
                    argument_scores,
                    method.connective_used,
                )?;
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
            | ExprInstance::GFixedPoint(_) => {
                exact("leaf Expr", 0, par_children.len())?;
                self.ground_expr_component(shell)
            }
            ExprInstance::EVarBody(_) => {
                exact("leaf Expr", 0, par_children.len())?;
                let score = ExprSortMatcher::sort_match(shell).score;
                let score = self.score_tree(&score)?;
                self.schema_expr_component(shell, &[], score)
            }
            ExprInstance::ESetBody(set) => {
                exact("ESet", set.ps.len(), par_children.len())?;
                let canonical = if par_children.len() <= 1 {
                    par_children.to_vec()
                } else {
                    let mut identities = PathMap::new();
                    let mut canonical: Vec<_> = par_children
                        .iter()
                        .copied()
                        .filter(|par| {
                            insert_semantic_par_identity(
                                &mut identities,
                                WireByteCursor::new(self, WireRoot::ParSemanticIdentity(*par)),
                                (),
                            )
                            .is_none()
                        })
                        .collect();
                    canonical.sort_by(|left, right| self.compare_pars(*left, *right));
                    canonical
                };

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
                let elements = canonical
                    .iter()
                    .map(|child| self.par_score(*child))
                    .collect();
                let score = semantics::ordered_collection_score(
                    self,
                    Score::ESET,
                    Some(remainder),
                    elements,
                    set.connective_used,
                )?;
                self.schema_expr_component(&canonical_shell, &canonical, score)
            }
            ExprInstance::EMapBody(map) => {
                exact("EMap", map.kvs.len() * 2, par_children.len())?;

                struct Pair {
                    key: PlannedPar,
                    value: PlannedPar,
                }
                let pair_shell = crate::rhoapi::KeyValuePair {
                    key: Some(Par::default()),
                    value: Some(Par::default()),
                };
                let mut pairs: Vec<Pair> = Vec::with_capacity(map.kvs.len());
                if map.kvs.len() <= 1 {
                    if let [key, value] = par_children {
                        pairs.push(Pair {
                            key: *key,
                            value: *value,
                        });
                    }
                } else {
                    let mut identities = PathMap::new();
                    for children in par_children.chunks_exact(2) {
                        let key = children[0];
                        let value = children[1];
                        if let Some(resident) = insert_semantic_par_identity(
                            &mut identities,
                            WireByteCursor::new(self, WireRoot::ParSemanticIdentity(key)),
                            pairs.len(),
                        ) {
                            if WireByteCursor::new(self, WireRoot::Par(value)).cmp(
                                WireByteCursor::new(self, WireRoot::Par(pairs[resident].value)),
                            ) == Ordering::Greater
                            {
                                pairs[resident].value = value;
                            }
                        } else {
                            pairs.push(Pair { key, value });
                        }
                    }
                    pairs.sort_by(|left, right| {
                        ScoreByteCursor::new(self, self.par_score(left.key))
                            .cmp(ScoreByteCursor::new(self, self.par_score(right.key)))
                            .then_with(|| {
                                WireByteCursor::new(
                                    self,
                                    WireRoot::KeyValuePair(left.key, left.value),
                                )
                                .cmp(WireByteCursor::new(
                                    self,
                                    WireRoot::KeyValuePair(right.key, right.value),
                                ))
                            })
                    });
                }
                let canonical = pairs;

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
                let key_scores = canonical
                    .iter()
                    .map(|pair| self.par_score(pair.key))
                    .collect();
                let score = semantics::ordered_collection_score(
                    self,
                    Score::EMAP,
                    Some(remainder),
                    key_scores,
                    map.connective_used,
                )?;
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
                let score = semantics::rpb3_pathmap_score(
                    self,
                    false,
                    snapshot_score,
                    pathmap.connective_used,
                )?;
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
                self.ezipper_component(zipper, planned_map, map_protobuf)
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

    fn occurrence_height(&self, root: Option<usize>) -> usize {
        root.map_or(0, |node| self.occurrence_nodes[node].height)
    }

    fn occurrence_distinct_len(&self, root: Option<usize>) -> usize {
        root.map_or(0, |node| self.occurrence_nodes[node].distinct_len)
    }

    fn push_occurrence_node(
        &mut self,
        occurrence: Occurrence,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<usize, ParConstructionError> {
        let value = self.make_occurrence_node(occurrence, left, right)?;
        let index = self.occurrence_nodes.len();
        self.occurrence_nodes.push(value);
        Ok(index)
    }

    fn make_occurrence_node(
        &self,
        occurrence: Occurrence,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<OccurrenceTreeNode, ParConstructionError> {
        let left_node = left.map(|node| &self.occurrence_nodes[node]);
        let right_node = right.map(|node| &self.occurrence_nodes[node]);
        let component = &self.components[occurrence.component.0];
        let multiplicity = usize::try_from(occurrence.multiplicity.get())
            .map_err(|_| ParConstructionError::LengthOverflow("Par component multiplicity"))?;
        let body_len = self.protobuf_nodes[component.protobuf].encoded_len;
        let prefix =
            InlineWireBytes::length_delimited_prefix(component.tag.byte().into(), body_len);
        let one_wire_len = key_len(component.tag.byte().into())
            .checked_add(encoded_len_varint(body_len as u64))
            .and_then(|len| len.checked_add(body_len))
            .ok_or(ParConstructionError::LengthOverflow("Par component"))?;
        let own_wire_len =
            one_wire_len
                .checked_mul(multiplicity)
                .ok_or(ParConstructionError::LengthOverflow(
                    "Par component multiplicity",
                ))?;
        let one_zero_count = prefix
            .zero_count()
            .checked_add(self.protobuf_nodes[component.protobuf].zero_count)
            .ok_or(ParConstructionError::LengthOverflow(
                "Par protobuf zero-byte count",
            ))?;
        let own_zero_count = one_zero_count.checked_mul(multiplicity).ok_or(
            ParConstructionError::LengthOverflow("Par protobuf zero-byte count"),
        )?;
        let own_score_len = self.scores[component.score.0]
            .encoded_len
            .checked_mul(multiplicity)
            .ok_or(ParConstructionError::LengthOverflow(
                "Par component score multiplicity",
            ))?;
        let add_aggregates = |left: usize, own: usize, right: usize, what| {
            left.checked_add(own)
                .and_then(|sum| sum.checked_add(right))
                .ok_or(ParConstructionError::LengthOverflow(what))
        };
        let height = self
            .occurrence_height(left)
            .max(self.occurrence_height(right))
            .checked_add(1)
            .ok_or(ParConstructionError::LengthOverflow(
                "planned occurrence-tree height",
            ))?;
        let distinct_len = add_aggregates(
            left_node.map_or(0, |node| node.distinct_len),
            1,
            right_node.map_or(0, |node| node.distinct_len),
            "planned occurrence count",
        )?;
        let score_len = add_aggregates(
            left_node.map_or(0, |node| node.score_len),
            own_score_len,
            right_node.map_or(0, |node| node.score_len),
            "Par score body",
        )?;
        let protobuf_len = add_aggregates(
            left_node.map_or(0, |node| node.protobuf_len),
            own_wire_len,
            right_node.map_or(0, |node| node.protobuf_len),
            "Par protobuf body",
        )?;
        let protobuf_zero_count = add_aggregates(
            left_node.map_or(0, |node| node.protobuf_zero_count),
            own_zero_count,
            right_node.map_or(0, |node| node.protobuf_zero_count),
            "Par protobuf zero-byte count",
        )?;
        Ok(OccurrenceTreeNode {
            owner: 0,
            occurrence,
            left: OccurrenceTreeNode::encode_link(left)?,
            right: OccurrenceTreeNode::encode_link(right)?,
            height,
            distinct_len,
            score_len,
            protobuf_len,
            protobuf_zero_count,
        })
    }

    fn rebuild_occurrence_node(
        &mut self,
        occurrence: Occurrence,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<usize, ParConstructionError> {
        let left_height = self.occurrence_height(left);
        let right_height = self.occurrence_height(right);
        if left_height > right_height && left_height - right_height > 1 {
            let pivot = self.occurrence_nodes[left.expect("left-heavy AVL node has a left child")];
            if self.occurrence_height(pivot.left()) >= self.occurrence_height(pivot.right()) {
                let demoted = self.push_occurrence_node(occurrence, pivot.right(), right)?;
                return self.push_occurrence_node(pivot.occurrence, pivot.left(), Some(demoted));
            }
            let middle = self.occurrence_nodes[pivot
                .right()
                .expect("left-right AVL rotation has an inner child")];
            let new_left =
                self.push_occurrence_node(pivot.occurrence, pivot.left(), middle.left())?;
            let new_right = self.push_occurrence_node(occurrence, middle.right(), right)?;
            return self.push_occurrence_node(middle.occurrence, Some(new_left), Some(new_right));
        }
        if right_height > left_height && right_height - left_height > 1 {
            let pivot =
                self.occurrence_nodes[right.expect("right-heavy AVL node has a right child")];
            if self.occurrence_height(pivot.right()) >= self.occurrence_height(pivot.left()) {
                let demoted = self.push_occurrence_node(occurrence, left, pivot.left())?;
                return self.push_occurrence_node(pivot.occurrence, Some(demoted), pivot.right());
            }
            let middle = self.occurrence_nodes[pivot
                .left()
                .expect("right-left AVL rotation has an inner child")];
            let new_left = self.push_occurrence_node(occurrence, left, middle.left())?;
            let new_right =
                self.push_occurrence_node(pivot.occurrence, middle.right(), pivot.right())?;
            return self.push_occurrence_node(middle.occurrence, Some(new_left), Some(new_right));
        }
        self.push_occurrence_node(occurrence, left, right)
    }

    fn insert_occurrence(
        &mut self,
        root: Option<usize>,
        occurrence: Occurrence,
    ) -> Result<Option<usize>, ParConstructionError> {
        enum Direction {
            Left,
            Right,
        }
        let mut path = Vec::new();
        let mut cursor = root;
        let rebuilt = loop {
            let Some(node) = cursor else {
                break self.push_occurrence_node(occurrence, None, None)?;
            };
            let current = self.occurrence_nodes[node];
            match self.compare_components(occurrence.component, current.occurrence.component) {
                Ordering::Less => {
                    path.push((current.occurrence, current.right(), Direction::Left));
                    cursor = current.left();
                }
                Ordering::Greater => {
                    path.push((current.occurrence, current.left(), Direction::Right));
                    cursor = current.right();
                }
                Ordering::Equal => {
                    let merged = Occurrence {
                        component: current.occurrence.component,
                        multiplicity: current
                            .occurrence
                            .multiplicity
                            .checked_add(occurrence.multiplicity)?,
                    };
                    break self.push_occurrence_node(merged, current.left(), current.right())?;
                }
            }
        };
        let mut rebuilt = Some(rebuilt);
        while let Some((parent, sibling, direction)) = path.pop() {
            rebuilt = Some(match direction {
                Direction::Left => self.rebuild_occurrence_node(parent, rebuilt, sibling)?,
                Direction::Right => self.rebuild_occurrence_node(parent, sibling, rebuilt)?,
            });
        }
        Ok(rebuilt)
    }

    fn union_occurrences(
        &mut self,
        left: Option<usize>,
        right: Option<usize>,
    ) -> Result<Option<usize>, ParConstructionError> {
        let (mut root, smaller) =
            if self.occurrence_distinct_len(left) >= self.occurrence_distinct_len(right) {
                (left, right)
            } else {
                (right, left)
            };
        let mut stack = Vec::new();
        let mut cursor = smaller;
        loop {
            while let Some(node) = cursor {
                stack.push(node);
                cursor = self.occurrence_nodes[node].left();
            }
            let Some(node) = stack.pop() else { break };
            let source = self.occurrence_nodes[node];
            cursor = source.right();
            root = self.insert_occurrence(root, source.occurrence)?;
        }
        Ok(root)
    }

    /// Plan a canonical `Par` from top-level component occurrences.
    ///
    /// Occurrences enter an immutable balanced dictionary keyed by their exact
    /// eventual RPB3 identity. Duplicate keys coalesce with checked Bag
    /// addition. No accumulated occurrence prefix is recopied.
    pub fn par(
        &mut self,
        occurrences: impl IntoIterator<Item = (PlannedComponent, Multiplicity)>,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        let mut root = None;
        for (component, multiplicity) in occurrences {
            root = self.insert_occurrence(root, Occurrence {
                component,
                multiplicity,
            })?;
        }
        self.par_from_occurrence_root(root, locally_free, connective_used)
    }

    #[cfg(test)]
    pub(crate) fn insert_protobuf_component(
        &mut self,
        parent: &mut ProtobufParBuilder,
        component: PlannedComponent,
    ) -> Result<(), ParConstructionError> {
        let search = self.search_protobuf_component(parent, component);
        self.apply_protobuf_component(parent, component, search)
    }

    pub(crate) fn search_protobuf_component(
        &mut self,
        parent: &ProtobufParBuilder,
        component: PlannedComponent,
    ) -> OccurrenceSearch {
        let mut path = std::mem::take(&mut self.occurrence_path);
        let mut cursor = parent.occurrence_root;
        while let Some(index) = cursor {
            let current = self.occurrence_nodes[index];
            match self.compare_components(component, current.occurrence.component) {
                Ordering::Equal => break,
                Ordering::Less => {
                    path.push((index, true));
                    cursor = current.left();
                }
                Ordering::Greater => {
                    path.push((index, false));
                    cursor = current.right();
                }
            }
        }
        OccurrenceSearch {
            path,
            found: cursor,
        }
    }

    pub(crate) fn existing_protobuf_component(
        &self,
        search: &OccurrenceSearch,
    ) -> Option<PlannedComponent> {
        search
            .found
            .map(|index| self.occurrence_nodes[index].occurrence.component)
    }

    pub(crate) fn apply_protobuf_component(
        &mut self,
        parent: &mut ProtobufParBuilder,
        component: PlannedComponent,
        search: OccurrenceSearch,
    ) -> Result<(), ParConstructionError> {
        let current_owner = parent
            .occurrence_root
            .map_or(0, |index| self.occurrence_nodes[index].owner);
        let owner = if current_owner != 0 {
            current_owner
        } else {
            self.occurrence_nodes
                .len()
                .checked_add(1)
                .ok_or(ParConstructionError::LengthOverflow("occurrence owner"))?
        };
        let undo = std::mem::take(&mut self.occurrence_undo);
        let mut edit = OccurrenceEdit {
            arena: self,
            owner,
            path: search.path,
            undo,
            committed: false,
        };
        let mut root = if let Some(index) = search.found {
            let current = edit.arena.occurrence_nodes[index];
            let multiplicity = current
                .occurrence
                .multiplicity
                .checked_add(Multiplicity::ONE)?;
            edit.store(
                Some(index),
                Occurrence {
                    component: current.occurrence.component,
                    multiplicity,
                },
                current.left(),
                current.right(),
            )?
        } else {
            edit.store(
                None,
                Occurrence {
                    component,
                    multiplicity: Multiplicity::ONE,
                },
                None,
                None,
            )?
        };
        while let Some((index, went_left)) = edit.path.pop() {
            let current = edit.arena.occurrence_nodes[index];
            let (left, right) = if went_left {
                (Some(root), current.right())
            } else {
                (current.left(), Some(root))
            };
            root = edit.rebuild(index, current.occurrence, left, right)?;
        }
        parent.occurrence_root = Some(root);
        edit.committed = true;
        Ok(())
    }

    pub(crate) fn construction_checkpoint(&self) -> ConstructionCheckpoint {
        ConstructionCheckpoint {
            bytes: self.byte_fragments.len(),
            pool: self.byte_pool.len(),
            scores: self.scores.len(),
            protobuf: self.protobuf_nodes.len(),
            epathmaps: self.epathmaps.len(),
            components: self.components.len(),
            occurrences: self.occurrence_nodes.len(),
            pars: self.pars.len(),
        }
    }

    pub(crate) fn rewind_construction(&mut self, checkpoint: ConstructionCheckpoint) {
        self.epathmaps.truncate(checkpoint.epathmaps);
        self.components.truncate(checkpoint.components);
        self.pars.truncate(checkpoint.pars);
        self.occurrence_nodes.truncate(checkpoint.occurrences);
        self.protobuf_nodes.truncate(checkpoint.protobuf);
        self.scores.truncate(checkpoint.scores);
        self.byte_fragments.truncate(checkpoint.bytes);
        self.byte_pool.truncate(checkpoint.pool);
    }

    fn seal_occurrences(&mut self, root: Option<usize>) {
        let Some(root) = root else { return };
        if self.occurrence_nodes[root].owner == 0 {
            return;
        }
        if self.occurrence_nodes[root].left().is_none()
            && self.occurrence_nodes[root].right().is_none()
        {
            self.occurrence_nodes[root].owner = 0;
            return;
        }
        let mut work = std::mem::take(&mut self.occurrence_seal);
        work.push((root, false));
        while let Some((index, expanded)) = work.pop() {
            let node = self.occurrence_nodes[index];
            if node.owner == 0 {
                continue;
            }
            if expanded {
                self.occurrence_nodes[index].owner = 0;
                continue;
            }
            work.push((index, true));
            if let Some(right) = node.right() {
                work.push((right, false));
            }
            if let Some(left) = node.left() {
                work.push((left, false));
            }
        }
        self.occurrence_seal = work;
    }

    pub(crate) fn freeze_protobuf_par(
        &mut self,
        value: ProtobufParBuilder,
    ) -> Result<PlannedPar, ParConstructionError> {
        self.par_from_occurrence_root(
            value.occurrence_root,
            value.locally_free,
            value.connective_used,
        )
    }

    fn par_summaries(
        &mut self,
        occurrence_root: Option<usize>,
        score_len: usize,
        mut protobuf_len: usize,
        mut protobuf_zero_count: usize,
        locally_free: &Vec<u8>,
        connective_used: bool,
    ) -> Result<(PlannedScore, usize, usize), ParConstructionError> {
        let occurrence_scores = self.score_from_pieces(vec![ScorePiece::OccurrenceScores {
            root: occurrence_root,
            encoded_len: score_len,
        }])?;
        let connective_score = self.score_i64(i64::from(connective_used))?;
        let score = self.score_node_with_pieces(Score::PAR, vec![
            ScorePiece::Child(occurrence_scores),
            ScorePiece::Child(connective_score),
        ])?;

        if !locally_free.is_empty() {
            protobuf_len = protobuf_len
                .checked_add(prost::encoding::bytes::encoded_len(9, locally_free))
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
        Ok((score, protobuf_len, protobuf_zero_count))
    }

    fn par_from_occurrence_root(
        &mut self,
        occurrence_root: Option<usize>,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        self.seal_occurrences(occurrence_root);
        let distinct_len = self.occurrence_distinct_len(occurrence_root);
        let aggregate = occurrence_root.map(|root| self.occurrence_nodes[root]);
        let (score, protobuf_len, protobuf_zero_count) = self.par_summaries(
            occurrence_root,
            aggregate.map_or(0, |node| node.score_len),
            aggregate.map_or(0, |node| node.protobuf_len),
            aggregate.map_or(0, |node| node.protobuf_zero_count),
            &locally_free,
            connective_used,
        )?;

        let singleton_occurrence = (distinct_len == 1)
            .then(|| {
                self.occurrence_nodes[occurrence_root.expect("singleton root exists")].occurrence
            })
            .filter(|occurrence| occurrence.multiplicity == Multiplicity::ONE);
        let singleton =
            singleton_occurrence.map(|occurrence| &self.components[occurrence.component.0]);
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
            occurrence_root,
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

    pub(crate) fn epm_barrier(&mut self) -> PlannedPar {
        if self.pars.is_empty() {
            self.empty_par()
                .expect("neutral Par construction has bounded constant summaries")
        } else {
            PlannedPar(0)
        }
    }

    fn build_epm_entries(
        &mut self,
        mode: EPathMapMode,
        children: &[PlannedPar],
    ) -> Result<OwnedPlannedEPathMap, ParConstructionError> {
        let (repr, barrier) = match mode {
            EPathMapMode::Empty => (EPathMapRepr::Empty, None),
            EPathMapMode::Set => (EPathMapRepr::Set(PathMap::new()), None),
            EPathMapMode::Map => (
                EPathMapRepr::Map(PathMap::new()),
                (!children.is_empty()).then(|| self.epm_barrier()),
            ),
        };
        let mut owner = EpmReprOwner::new(repr, PlannedEpmBarrier(barrier));
        match owner.as_repr_mut() {
            EPathMapRepr::Empty => {}
            EPathMapRepr::Set(entries) => {
                for &key in children {
                    entries.insert(&self.canonical_path_bytes(key)?, ());
                }
            }
            EPathMapRepr::Map(entries) => {
                for pair in children.chunks_exact(2) {
                    entries.insert(&self.canonical_path_bytes(pair[0])?, pair[1]);
                }
            }
        }
        owner.normalize_empty();
        Ok(owner)
    }

    fn own_epm_repr(&mut self, repr: EPathMapRepr<PlannedPar>) -> OwnedPlannedEPathMap {
        let mut owner = EpmReprOwner::new(repr, ArenaEpmBarrier(RefCell::new(self)));
        owner.normalize_empty();
        let barrier = if matches!(owner.as_repr(), EPathMapRepr::Map(_)) {
            Some(owner.barrier.make_barrier())
        } else {
            None
        };
        owner.with_barrier(PlannedEpmBarrier(barrier))
    }

    pub(crate) fn own_decoded_epm_repr(
        &mut self,
        repr: EPathMapRepr<PlannedPar>,
    ) -> OwnedPlannedEPathMap {
        let owner = EpmReprOwner::new(repr, ArenaEpmBarrier(RefCell::new(self)));
        let barrier = if matches!(owner.as_repr(), EPathMapRepr::Map(_)) {
            Some(owner.barrier.make_barrier())
        } else {
            None
        };
        owner.with_barrier(PlannedEpmBarrier(barrier))
    }

    /// Construct one top-level occurrence with its already-derived root
    /// summaries.
    ///
    /// Normalizer, reducer, and matcher PDAs use this as the common boundary
    /// between a schema-specific component constructor and parallel
    /// composition. Keeping it here prevents those callers from rebuilding a
    /// temporary resident [`Par`] merely to insert one occurrence.
    pub fn singleton_component(
        &mut self,
        component: PlannedComponent,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        self.par(
            [(component, Multiplicity::ONE)],
            locally_free,
            connective_used,
        )
    }

    /// Append one planned component to an existing planned parallel
    /// composition without finishing either side.
    ///
    /// The component summaries describe the component itself; [`parallel`]
    /// performs the exact Bag addition and summary union with `base`.
    pub fn append_component(
        &mut self,
        base: PlannedPar,
        component: PlannedComponent,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        let singleton = self.singleton_component(component, locally_free, connective_used)?;
        self.parallel(base, singleton)
    }

    /// Bag union of two planned parallel compositions. Component paths remain
    /// arena references; only the bounded occurrence descriptors and root
    /// summaries are merged here.
    pub fn parallel(
        &mut self,
        left: PlannedPar,
        right: PlannedPar,
    ) -> Result<PlannedPar, ParConstructionError> {
        let is_identity = |node: &ParNode| {
            node.occurrence_root.is_none() && node.locally_free.is_empty() && !node.connective_used
        };
        if is_identity(&self.pars[left.0]) {
            return Ok(right);
        }
        if is_identity(&self.pars[right.0]) {
            return Ok(left);
        }
        let left_root = self.pars[left.0].occurrence_root;
        let right_root = self.pars[right.0].occurrence_root;
        let occurrence_root = self.union_occurrences(left_root, right_root)?;
        let locally_free = super::utils::union(
            self.pars[left.0].locally_free.clone(),
            self.pars[right.0].locally_free.clone(),
        );
        self.par_from_occurrence_root(
            occurrence_root,
            locally_free,
            self.pars[left.0].connective_used || self.pars[right.0].connective_used,
        )
    }

    /// Multiply every occurrence in a planned parallel composition by one
    /// checked Bag multiplicity.
    ///
    /// Substitution visits one canonical PathMap endpoint once even when that
    /// endpoint represents many equal occurrences. If the endpoint is a
    /// variable, its replacement can itself contain several distinct
    /// components; scaling the immutable occurrence dictionary preserves the
    /// product algebraically instead of expanding either multiplicity into a
    /// work-item list. Root summaries are idempotent under positive Bag
    /// scaling and are therefore retained exactly.
    pub fn scale_multiplicity(
        &mut self,
        par: PlannedPar,
        factor: Multiplicity,
    ) -> Result<PlannedPar, ParConstructionError> {
        if factor == Multiplicity::ONE {
            return Ok(par);
        }

        let planned = &self.pars[par.0];
        let locally_free = planned.locally_free.clone();
        let connective_used = planned.connective_used;
        let Some(root) = planned.occurrence_root else {
            return self.par_from_occurrence_root(None, locally_free, connective_used);
        };

        // Clone the immutable AVL topology in one iterative post-order pass.
        // Re-inserting its already-sorted endpoints would cost O(n log n) and
        // allocate copied search spines; the ordering and balance are unchanged
        // by pointwise scaling, so each reachable node needs exactly one clone.
        enum ScaleFrame {
            Visit(usize),
            Finish(OccurrenceTreeNode),
        }

        let distinct_len = self.occurrence_distinct_len(Some(root));
        let mut work = Vec::with_capacity(distinct_len);
        let mut rebuilt = Vec::with_capacity(distinct_len);
        work.push(ScaleFrame::Visit(root));
        while let Some(frame) = work.pop() {
            match frame {
                ScaleFrame::Visit(index) => {
                    let node = self.occurrence_nodes[index];
                    work.push(ScaleFrame::Finish(node));
                    if let Some(right) = node.right() {
                        work.push(ScaleFrame::Visit(right));
                    }
                    if let Some(left) = node.left() {
                        work.push(ScaleFrame::Visit(left));
                    }
                }
                ScaleFrame::Finish(node) => {
                    let right = node.right().map(|_| {
                        rebuilt
                            .pop()
                            .expect("post-order scaling produced the right child")
                    });
                    let left = node.left().map(|_| {
                        rebuilt
                            .pop()
                            .expect("post-order scaling produced the left child")
                    });
                    let multiplicity = node
                        .occurrence
                        .multiplicity
                        .checked_mul(factor.get())?
                        .expect("a positive Bag factor preserves a positive multiplicity");
                    let scaled = self.push_occurrence_node(
                        Occurrence {
                            component: node.occurrence.component,
                            multiplicity,
                        },
                        left,
                        right,
                    )?;
                    rebuilt.push(scaled);
                }
            }
        }
        let scaled_root = rebuilt
            .pop()
            .expect("a non-empty occurrence tree produces one scaled root");
        debug_assert!(rebuilt.is_empty());
        self.par_from_occurrence_root(Some(scaled_root), locally_free, connective_used)
    }

    /// Replace a planned `Par`'s root locally-free summary while sharing its
    /// immutable occurrence tree.
    ///
    /// Reducer collection evaluation historically recomputes this summary from
    /// evaluated top-level components before embedding a child. Rebuilding the
    /// resident `Par` merely to change that metadata would copy the complete
    /// recursive protobuf body; this creates only a new root plan and retains
    /// all component, score, and wire nodes.
    pub fn with_locally_free(
        &mut self,
        par: PlannedPar,
        locally_free: Vec<u8>,
    ) -> Result<PlannedPar, ParConstructionError> {
        let occurrence_root = self.pars[par.0].occurrence_root;
        let connective_used = self.pars[par.0].connective_used;
        self.par_from_occurrence_root(occurrence_root, locally_free, connective_used)
    }

    /// Replace both root summaries while sharing the immutable occurrence
    /// tree. Canonical protobuf-region identity import uses the summaries from
    /// the indexed source message verbatim rather than re-inferring them from
    /// component contributions.
    pub fn with_metadata(
        &mut self,
        par: PlannedPar,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<PlannedPar, ParConstructionError> {
        let occurrence_root = self.pars[par.0].occurrence_root;
        self.par_from_occurrence_root(occurrence_root, locally_free, connective_used)
    }

    /// Import an already-authoritative resident `Par` without decoding any
    /// component or traversing any nested AST.
    ///
    /// The PathMap zipper lends each RPB3 score/body pair exactly once. Those
    /// canonical fragments become leaves in this transient arena, so a later
    /// planned parent can reference the imported value without rebuilding it.
    fn import_par_opaque(&mut self, par: &Par) -> Result<PlannedPar, ParConstructionError> {
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
                    ProtoProgram::default()
                } else {
                    ProtoProgram::One(ProtoPiece::Bytes(protobuf))
                },
                encoded_len,
                zero_count,
            });
            let component = PlannedComponent(self.components.len());
            let component_segment = (singleton_stable && imported_is_singleton)
                .then(|| CanonicalSegment::Bytes(canonical_segment.clone().into_boxed_slice()));
            self.components.push(ComponentNode {
                tag,
                score,
                protobuf: protobuf_id,
                canonical_path_stable: singleton_stable && imported_is_singleton,
                canonical_segment: component_segment,
                semantic_shape: ComponentSemanticShape::Opaque,
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

    /// Import an authoritative resident `Par`, retaining the immediate
    /// singleton shape required by normalizer and substitution algebra.
    ///
    /// The complete component bag still enters through one byte-native zipper
    /// pass. Only an exactly singleton Bundle or Connective is decoded, because
    /// those constructors participate in outer Bundle flag merging and
    /// conjunction/disjunction flattening. Their direct child Pars are imported
    /// as opaque canonical fragments, so this boundary never recursively walks
    /// the resident AST.
    pub fn import_par(&mut self, par: &Par) -> Result<PlannedPar, ParConstructionError> {
        let planned = self.import_par_opaque(par)?;
        if par.cardinality() != 1 {
            return Ok(planned);
        }

        let component = self.occurrence_nodes[self.pars[planned.0]
            .occurrence_root
            .expect("one authoritative occurrence has an arena root")]
        .occurrence
        .component;

        let semantic_shape = if par.family_cardinality::<Bundle>() == 1 {
            let bundle = par
                .first::<Bundle>()
                .expect("Par invariant: stored Bundle keys are canonical")
                .expect("the cardinality check proved one Bundle")
                .0;
            let body = match bundle.body.as_ref() {
                Some(body) => self.import_par_opaque(body)?,
                None => self.empty_par()?,
            };
            ComponentSemanticShape::Bundle {
                body,
                write_flag: bundle.write_flag,
                read_flag: bundle.read_flag,
            }
        } else if par.family_cardinality::<Connective>() == 1 {
            let connective = par
                .first::<Connective>()
                .expect("Par invariant: stored Connective keys are canonical")
                .expect("the cardinality check proved one Connective")
                .0;
            match connective.connective_instance {
                Some(ConnectiveInstance::ConnAndBody(body)) => {
                    let children = body
                        .ps
                        .iter()
                        .map(|child| self.import_par_opaque(child))
                        .collect::<Result<Vec<_>, _>>()?;
                    ComponentSemanticShape::ConnectiveAnd(children.into_boxed_slice())
                }
                Some(ConnectiveInstance::ConnOrBody(body)) => {
                    let children = body
                        .ps
                        .iter()
                        .map(|child| self.import_par_opaque(child))
                        .collect::<Result<Vec<_>, _>>()?;
                    ComponentSemanticShape::ConnectiveOr(children.into_boxed_slice())
                }
                _ => ComponentSemanticShape::ConnectiveOther,
            }
        } else {
            return Ok(planned);
        };
        self.components[component.0].semantic_shape = semantic_shape;
        Ok(planned)
    }

    pub(crate) fn finish_epm_repr(
        &self,
        repr: OwnedPlannedEPathMap,
    ) -> Result<epathmap_trie_codec::OwnedResidentEPathMap, ParConstructionError> {
        repr.try_map_resident(|value| self.finish(*value))
    }

    /// Emit the root planned `Par` into its sole resident representation.
    pub fn finish(&self, root: PlannedPar) -> Result<Par, ParConstructionError> {
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::BeginFinish);
        let planned = &self.pars[root.0];
        let mut accumulator = CanonicalParBagAccumulator::new();
        let mut key = Vec::new();
        let mut singleton_tag = None;
        let components = Bag::try_from_unique(|writer| {
            for occurrence in OccurrenceIter::new(&self.occurrence_nodes, planned.occurrence_root) {
                let component = &self.components[occurrence.component.0];
                self.component_key_into(occurrence.component, &mut key)?;
                writer
                    .push(&key, occurrence.multiplicity)
                    .map_err(ParConstructionError::Publication)?;
                accumulator
                    .record_prevalidated(occurrence.multiplicity, &CanonicalKeyMetadata {
                        tag: component.tag,
                        body_len: self.protobuf_nodes[component.protobuf].encoded_len,
                        canonical_path_stable: component.canonical_path_stable,
                    })
                    .map_err(ParConstructionError::FamilyMetrics)?;
                if self.occurrence_distinct_len(planned.occurrence_root) == 1
                    && occurrence.multiplicity == Multiplicity::ONE
                {
                    singleton_tag = Some(component.tag);
                }
            }
            Ok::<(), ParConstructionError>(())
        })?;
        let mut components = accumulator.finish(components);
        if let Some(tag) = singleton_tag {
            components.seed_singleton_derived_caches(tag, &key, None);
        }
        let result = Par::from_parts(
            components,
            planned.locally_free.clone(),
            planned.connective_used,
        );
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::FinishOk);
        Ok(result)
    }

    /// Emit the canonical protobuf body of a planned `Par` without first
    /// constructing any nested resident `Par`.
    pub fn protobuf_bytes(&self, root: PlannedPar) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pars[root.0].protobuf_len);
        self.emit_par(root, &mut out);
        debug_assert_eq!(out.len(), self.pars[root.0].protobuf_len);
        out
    }

    #[cfg(test)]
    fn component_key(&self, component: PlannedComponent) -> Result<Vec<u8>, ParConstructionError> {
        let mut out = Vec::new();
        self.component_key_into(component, &mut out)?;
        Ok(out)
    }

    fn component_key_into(
        &self,
        component: PlannedComponent,
        out: &mut Vec<u8>,
    ) -> Result<(), ParConstructionError> {
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
        let trailer = u64::try_from(score_len)
            .map_err(|_| ParConstructionError::LengthOverflow("RPB3 score trailer"))?;
        out.clear();
        out.reserve(capacity);
        out.extend_from_slice(&PAR_BAG_KEY_DOMAIN);
        out.push(component.tag.byte());
        match &self.scores[component.score.0].pieces {
            ScoreProgram::Pooled(span) => out.extend_from_slice(span.view(&self.byte_pool)),
            _ => self.emit_score(component.score, out),
        }
        match &self.protobuf_nodes[component.protobuf].pieces {
            ProtoProgram::One(ProtoPiece::PooledBytes(span)) => {
                out.extend_from_slice(span.view(&self.byte_pool))
            }
            _ => self.emit_proto(component.protobuf, out),
        }
        out.extend_from_slice(&trailer.to_be_bytes());
        debug_assert_eq!(out.len(), capacity);
        #[cfg(feature = "protobuf-projection-experiment")]
        substitution_cost::record(substitution_cost::Event::EmitRootKey(out.len()));
        Ok(())
    }

    fn compare_components(&self, left: PlannedComponent, right: PlannedComponent) -> Ordering {
        let left = &self.components[left.0];
        let right = &self.components[right.0];
        left.tag
            .cmp(&right.tag)
            .then_with(|| {
                match (
                    self.score_fragment(left.score),
                    self.score_fragment(right.score),
                ) {
                    (Some(left), Some(right)) => left.cmp(right),
                    _ => ScoreByteCursor::new(self, left.score)
                        .cmp(ScoreByteCursor::new(self, right.score)),
                }
            })
            .then_with(|| {
                match (
                    self.protobuf_fragment(left.protobuf),
                    self.protobuf_fragment(right.protobuf),
                ) {
                    (Some(left), Some(right)) => left.cmp(right),
                    _ => WireByteCursor::new(self, WireRoot::Proto(left.protobuf))
                        .cmp(WireByteCursor::new(self, WireRoot::Proto(right.protobuf))),
                }
            })
    }

    fn score_fragment(&self, root: PlannedScore) -> Option<&[u8]> {
        match &self.scores[root.0].pieces {
            ScoreProgram::Literal(fragment) => Some(&self.byte_fragments[*fragment]),
            ScoreProgram::Pooled(span) => Some(span.view(&self.byte_pool)),
            ScoreProgram::Composite(pieces) if pieces.is_empty() => Some(&[]),
            _ => None,
        }
    }

    fn protobuf_fragment(&self, root: usize) -> Option<&[u8]> {
        match self.protobuf_nodes[root].pieces.as_slice() {
            [] => Some(&[]),
            [ProtoPiece::Bytes(bytes)] => Some(bytes),
            [ProtoPiece::PooledBytes(span)] => Some(span.view(&self.byte_pool)),
            [ProtoPiece::SharedBytes(bytes)] => Some(bytes.as_ref().as_ref()),
            _ => None,
        }
    }

    fn emit_score(&self, root: PlannedScore, out: &mut Vec<u8>) {
        if let Some(bytes) = self.score_fragment(root) {
            out.extend_from_slice(bytes);
        } else {
            out.extend(ScoreByteCursor::new(self, root));
        }
    }

    fn emit_par(&self, root: PlannedPar, out: &mut Vec<u8>) {
        self.emit_wire(WireRoot::Par(root), out);
    }

    fn emit_proto(&self, root: usize, out: &mut Vec<u8>) {
        if let Some(bytes) = self.protobuf_fragment(root) {
            out.extend_from_slice(bytes);
        } else {
            self.emit_wire(WireRoot::Proto(root), out);
        }
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
            pieces: ProtoProgram::default(),
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
                    for key in map.keys() {
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
                        if self.pars[par.0].occurrence_root.is_some()
                            || self.pars[par.0].connective_used
                        {
                            pieces.push(ProtoPiece::Par { tag: 2, par });
                        }
                        self.protobuf_nodes.push(ProtoNode {
                            pieces: pieces.into(),
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
            for piece in self.protobuf_nodes[id].pieces.as_slice() {
                let (add, add_zeroes) = match piece {
                    ProtoPiece::Bytes(bytes) => {
                        (bytes.len(), bytes.iter().filter(|&&byte| byte == 0).count())
                    }
                    ProtoPiece::PooledBytes(span) => {
                        let bytes = span.view(&self.byte_pool);
                        (bytes.len(), bytes.iter().filter(|&&byte| byte == 0).count())
                    }
                    ProtoPiece::SharedBytes(bytes) => {
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
                    ProtoProgram::default()
                } else {
                    ProtoProgram::One(ProtoPiece::Bytes(bytes.into_boxed_slice()))
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
            pieces: ProtoProgram::default(),
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
        self.epathmaps.clear();
        self.components.clear();
        self.pars.clear();
        self.protobuf_nodes.clear();
        self.scores.clear();
        self.byte_fragments.clear();
        self.byte_pool.clear();
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
        Bundle, Connective, ConnectiveBody, EList, EMap, EMethod, ENot, EPathMap, EPlus, ESet,
        ETuple, EZipper, Expr, GPrivate, GUnforgeable, If, KeyValuePair, Match, MatchCase, New,
        Receive, ReceiveBind, Send,
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

    fn planned_semantic_identity_bytes(arena: &ParConstruction, par: PlannedPar) -> Vec<u8> {
        WireByteCursor::new(arena, WireRoot::ParSemanticIdentity(par)).collect()
    }

    #[test]
    fn schema_plan_is_byte_identical_to_repeated_enot_construction() {
        for depth in [0usize, 1, 4, 32, 128] {
            let mut arena = ParConstruction::new();
            let mut planned = planned_gint(&mut arena, 7).expect("planned integer");
            for _ in 0..depth {
                planned = planned_enot(&mut arena, planned).expect("planned ENot");
            }

            let terminal = resident_gint(7);
            let terminal_bytes = terminal.protobuf_bytes().unwrap();
            let mut expected = terminal;
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
            if depth != 0 {
                actual.visit_encoded::<Expr>(|entry| {
                    let (observed_depth, observed_terminal) = entry
                        .canonical_unary_enot_parts()
                        .expect("planned ENot spine");
                    assert_eq!(observed_depth, depth);
                    assert_eq!(observed_terminal, terminal_bytes);
                });
            }
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
    fn semantic_default_and_outer_identity_match_resident_par_semantics() {
        let lf_only = Par::default().with_locally_free(vec![7, 0, 9]);
        let stable = resident_gint(3);
        let nested_lf = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![stable.clone().with_locally_free(vec![5])],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);
        let corpus = [
            lf_only.clone(),
            Par::default().with_connective_used(true),
            stable,
            nested_lf,
            Par::default().with_bundles(vec![Bundle {
                body: Some(resident_gint(8)),
                write_flag: true,
                read_flag: false,
            }]),
            Par::default().with_conditionals(vec![If {
                condition: Some(resident_gint(1)),
                if_true: Some(resident_gint(2)),
                if_false: Some(resident_gint(3)),
                locally_free: Vec::new(),
                connective_used: false,
            }]),
        ];

        let mut arena = ParConstruction::new();
        for resident in corpus {
            let planned = arena.import_par(&resident).unwrap();
            let expected = resident.clone().with_locally_free(Vec::new());
            assert_eq!(
                planned_semantic_identity_bytes(&arena, planned),
                expected.protobuf_bytes().unwrap(),
                "outer identity must omit only root locally_free"
            );
            assert_eq!(
                arena.is_semantic_default(planned),
                resident == Par::default()
            );
        }

        let empty_lf = arena.import_par(&lf_only).unwrap();
        assert!(arena.is_semantic_default(empty_lf));
        let nonempty = planned_gint(&mut arena, 11).unwrap();
        assert!(!arena.is_semantic_default(nonempty));
    }

    #[test]
    fn lf_only_receive_condition_and_match_guard_collapse_like_the_resident_sorter() {
        let body_r = resident_gint(10);
        let pattern_r = resident_gint(11);
        let source_r = resident_gint(12);
        let target_r = resident_gint(13);
        let lf_only_r = Par::default().with_locally_free(vec![1, 0, 1]);

        let mut arena = ParConstruction::new();
        let body = arena.import_par(&body_r).unwrap();
        let pattern = arena.import_par(&pattern_r).unwrap();
        let source = arena.import_par(&source_r).unwrap();
        let target = arena.import_par(&target_r).unwrap();
        let lf_only = arena.import_par(&lf_only_r).unwrap();

        let receive = Receive {
            binds: Vec::new(),
            body: Some(body_r),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: Vec::new(),
            connective_used: false,
            condition: Some(lf_only_r.clone()),
            cost_region: None,
        };
        let receive_component = arena.receive_component(&receive, &[body, lf_only]).unwrap();
        let receive_plan = arena
            .par([(receive_component, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let receive_expected = Par::default().with_receives(vec![receive]);
        assert_eq!(
            arena.protobuf_bytes(receive_plan),
            receive_expected.protobuf_bytes().unwrap()
        );

        let match_term = Match {
            target: Some(target_r),
            cases: vec![MatchCase {
                pattern: Some(pattern_r),
                source: Some(source_r),
                free_count: 0,
                guard: Some(lf_only_r),
            }],
            locally_free: Vec::new(),
            connective_used: false,
        };
        let match_component = arena
            .match_component(&match_term, &[target, pattern, source, lf_only])
            .unwrap();
        let match_plan = arena
            .par([(match_component, Multiplicity::ONE)], Vec::new(), false)
            .unwrap();
        let match_expected = Par::default().with_matches(vec![match_term]);
        assert_eq!(
            arena.protobuf_bytes(match_plan),
            match_expected.protobuf_bytes().unwrap()
        );
    }

    #[test]
    fn ac_identity_is_global_and_root_lf_quotiented_for_every_permutation() {
        const PERMUTATIONS: [[usize; 3]; 6] =
            [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
                2, 1, 0,
            ]];
        let base = resident_gint(7);
        let aliases = [
            base.clone(),
            base.clone().with_locally_free(vec![1]),
            base.with_locally_free(vec![2]),
        ];
        let values = [resident_gint(20), resident_gint(10), resident_gint(30)];

        for permutation in PERMUTATIONS {
            let mut arena = ParConstruction::new();
            let planned_aliases: Vec<_> = aliases
                .iter()
                .map(|par| arena.import_par(par).unwrap())
                .collect();
            let planned_values: Vec<_> = values
                .iter()
                .map(|par| arena.import_par(par).unwrap())
                .collect();

            let set_expr = Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: permutation
                        .iter()
                        .map(|&index| aliases[index].clone())
                        .collect(),
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            };
            let set_children: Vec<_> = permutation
                .iter()
                .map(|&index| planned_aliases[index])
                .collect();
            let set_component = arena
                .expr_component(&set_expr, &set_children, false)
                .unwrap();
            let set_plan = arena
                .par([(set_component, Multiplicity::ONE)], Vec::new(), false)
                .unwrap();
            let set_expected =
                Par::default().with_exprs(vec![ExprSortMatcher::sort_match(&set_expr).term]);
            assert_eq!(
                arena.protobuf_bytes(set_plan),
                set_expected.protobuf_bytes().unwrap(),
                "set permutation {permutation:?}"
            );

            let map_expr = Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: permutation
                        .iter()
                        .map(|&index| KeyValuePair {
                            key: Some(aliases[index].clone()),
                            value: Some(values[index].clone()),
                        })
                        .collect(),
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            };
            let map_children: Vec<_> = permutation
                .iter()
                .flat_map(|&index| [planned_aliases[index], planned_values[index]])
                .collect();
            let map_component = arena
                .expr_component(&map_expr, &map_children, false)
                .unwrap();
            let map_plan = arena
                .par([(map_component, Multiplicity::ONE)], Vec::new(), false)
                .unwrap();
            let map_expected =
                Par::default().with_exprs(vec![ExprSortMatcher::sort_match(&map_expr).term]);
            assert_eq!(
                arena.protobuf_bytes(map_plan),
                map_expected.protobuf_bytes().unwrap(),
                "map permutation {permutation:?}"
            );
        }

        let nested = |lf| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![resident_gint(9).with_locally_free(lf)],
                    locally_free: Vec::new(),
                    connective_used: false,
                    remainder: None,
                })),
            }])
        };
        let distinct = [nested(vec![1]), nested(vec![2])];
        let set_expr = Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: distinct.to_vec(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let canonical = ExprSortMatcher::sort_match(&set_expr).term;
        let Some(ExprInstance::ESetBody(set)) = canonical.expr_instance else {
            panic!("set sorter retains its constructor")
        };
        assert_eq!(set.ps.len(), 2, "nested locally_free remains semantic here");
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
            cost_region: None,
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
                cost_region: None,
            }],
            body: Some(Par::default()),
            persistent: true,
            peek: true,
            bind_count: 2,
            locally_free: vec![5],
            connective_used: true,
            condition: Some(Par::default()),
            cost_region: None,
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
                    cost_region: None,
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
    fn resident_ezipper_import_retains_authoritative_score_and_protobuf() {
        let zipper = EZipper {
            pathmap: Some(EPathMap::from_set_iter(
                [resident_gint(1), resident_gint(2)],
                Vec::new(),
                false,
                None,
            )),
            current_path: vec![vec![path_tag::GINT, 2]],
            is_write_zipper: true,
            locally_free: vec![1, 0, 1],
            connective_used: true,
            cursor_kind: 1,
        };
        let original = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EZipperBody(zipper)),
        }]);
        let original_bytes = original.protobuf_bytes().unwrap();
        let original_snapshot = original.compact_snapshot();

        let mut retained = None;
        original.visit_encoded::<Expr>(|entry| {
            assert!(retained.is_none(), "fixture has one distinct Expr endpoint");
            let protobuf = entry.payload().to_vec();
            let canonical = entry
                .to_owned_canonical_ezipper()
                .expect("typed resident EZipper proof");
            assert_eq!(canonical.locally_free(), [1, 0, 1]);
            assert!(canonical.connective_used());
            retained = Some((canonical, protobuf, entry.multiplicity()));
        });
        let (canonical, protobuf, multiplicity) = retained.expect("resident EZipper endpoint");

        let mut arena = ParConstruction::new();
        let component = arena.import_canonical_ezipper(canonical).unwrap();
        let planned = arena
            .par(
                [(component, multiplicity)],
                original.locally_free().to_vec(),
                original.connective_used(),
            )
            .unwrap();
        let imported = arena.finish(planned).unwrap();
        assert_eq!(imported.protobuf_bytes().unwrap(), original_bytes);
        assert_eq!(imported.compact_snapshot(), original_snapshot);
        assert_eq!(imported, original);

        let mut arena = ParConstruction::new();
        let (component, locally_free, connective_used) = arena
            .import_canonical_ezipper_region(&protobuf)
            .expect("nested canonical EZipper import");
        assert_eq!(locally_free, [1, 0, 1]);
        assert!(connective_used);
        let planned = arena
            .par(
                [(component, multiplicity)],
                original.locally_free().to_vec(),
                original.connective_used(),
            )
            .unwrap();
        let imported = arena.finish(planned).unwrap();
        assert_eq!(imported.protobuf_bytes().unwrap(), original_bytes);
        assert_eq!(imported.compact_snapshot(), original_snapshot);
        assert_eq!(imported, original);
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
                                    cost_region: None,
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
                                    cost_region: None,
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
    fn planned_bag_scaling_multiplies_endpoints_without_expansion() {
        let seven = Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        };
        let eleven = Expr {
            expr_instance: Some(ExprInstance::GInt(11)),
        };
        let source = Par::default()
            .with_exprs(vec![seven.clone(), seven.clone(), eleven.clone()])
            .with_locally_free(vec![1, 0, 1])
            .with_connective_used(true);

        let mut arena = ParConstruction::new();
        let planned = arena.import_par(&source).expect("import resident Bag");
        let identity = arena
            .scale_multiplicity(planned, Multiplicity::ONE)
            .expect("scale by the Bag identity");
        assert_eq!(identity, planned, "multiplicity one must share the plan");

        let occurrence_nodes_before = arena.occurrence_nodes.len();
        let scaled = arena
            .scale_multiplicity(planned, Multiplicity::new(3).unwrap())
            .expect("scale every Bag endpoint");
        assert_eq!(
            arena.occurrence_nodes.len() - occurrence_nodes_before,
            source.distinct_len(),
            "pointwise scaling must clone each immutable AVL node exactly once"
        );
        let actual = arena.finish(scaled).expect("finish scaled Bag");

        assert_eq!(actual.distinct_len(), 2);
        assert_eq!(actual.count(&seven), 6);
        assert_eq!(actual.count(&eleven), 3);
        assert_eq!(actual.locally_free(), &[1, 0, 1]);
        assert!(actual.connective_used());
    }

    #[test]
    fn planned_canonical_segment_keeps_a_list_nested_while_path_splits_it() {
        let mut arena = ParConstruction::new();
        let leaf = planned_gint(&mut arena, 7).expect("plan list leaf");
        let shell = Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![Par::default()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let component = arena
            .expr_component(&shell, &[leaf], false)
            .expect("plan list component");
        let list = arena
            .singleton_component(component, Vec::new(), false)
            .expect("plan list carrier");
        let resident = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![resident_gint(7)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);

        let path = arena.canonical_path_bytes(list).expect("planned path");
        let segment = arena
            .canonical_segment_bytes(list)
            .expect("planned segment");
        assert_eq!(
            path,
            super::super::canonical_path::encode_trie_path(&resident)
        );
        assert_eq!(
            segment,
            super::super::canonical_path::encode_trie_segment(&resident)
        );
        assert_ne!(
            path, segment,
            "the top-level list rule must remain explicit"
        );
        assert_eq!(path.last(), Some(&super::super::canonical_path::tag::TERM));
        assert_eq!(
            segment.first(),
            Some(&super::super::canonical_path::tag::ELIST)
        );
    }

    #[test]
    fn stable_resident_import_remains_navigable_after_parallel_identity() {
        let source = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![resident_gint(7)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }]);

        let mut arena = ParConstruction::new();
        let empty = arena.empty_par().expect("plan parallel identity");
        let imported = arena.import_par(&source).expect("import stable singleton");
        let recomposed = arena
            .parallel(empty, imported)
            .expect("apply planned Bag identity");

        assert_eq!(arena.finish(recomposed).unwrap(), source);
        assert_eq!(
            arena.canonical_path_bytes(recomposed).unwrap(),
            crate::rust::canonical_path::encode_trie_path(&source),
            "component-level segment metadata must survive a new planned root",
        );
    }

    #[test]
    fn locally_free_replacement_shares_the_planned_occurrence_tree() {
        let source = Par::default()
            .with_exprs(vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GInt(2)),
                },
            ])
            .with_locally_free(vec![1, 1]);
        let expected = source.with_locally_free(vec![0, 1, 0]);

        let mut arena = ParConstruction::new();
        let original = arena.import_par(&source).expect("import resident Par");
        let occurrence_count = arena.occurrence_nodes.len();
        let occurrence_root = arena.pars[original.0].occurrence_root;
        let replaced = arena
            .with_locally_free(original, vec![0, 1, 0])
            .expect("replace planned root metadata");

        assert_eq!(arena.occurrence_nodes.len(), occurrence_count);
        assert_eq!(arena.pars[replaced.0].occurrence_root, occurrence_root);
        assert_eq!(arena.finish(replaced).unwrap(), expected);
        assert_eq!(
            arena.protobuf_bytes(replaced),
            expected.protobuf_bytes().unwrap()
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
    fn planned_parallel_persistently_balances_occurrences_without_prefix_copying() {
        const DISTINCT: usize = 4_096;

        let mut arena = ParConstruction::new();
        let mut planned = arena.empty_par().unwrap();
        let mut expected_exprs = Vec::with_capacity(DISTINCT + 1);

        // Monotone keys are the adversarial insertion order for an unbalanced
        // search tree. Each append may allocate only a bounded AVL spine, not
        // a copy of every occurrence accumulated so far.
        for value in 0..DISTINCT {
            let expr = Expr {
                expr_instance: Some(ExprInstance::GInt(value as i64)),
            };
            expected_exprs.push(expr.clone());
            let before = arena.occurrence_nodes.len();
            let component = arena.expr_component(&expr, &[], true).unwrap();
            let singleton = arena
                .singleton_component(component, Vec::new(), false)
                .unwrap();
            planned = arena.parallel(planned, singleton).unwrap();
            let allocated = arena.occurrence_nodes.len() - before;
            let logarithmic_height = (usize::BITS - (value + 1).max(1).leading_zeros()) as usize;
            assert!(
                allocated <= 4 * (logarithmic_height + 2),
                "append allocated {allocated} occurrence nodes at cardinality {}",
                value + 1
            );
        }

        // Equal canonical keys must retain Bag addition rather than becoming
        // a second dictionary entry.
        let duplicate = Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        };
        expected_exprs.push(duplicate.clone());
        let component = arena.expr_component(&duplicate, &[], true).unwrap();
        let singleton = arena
            .singleton_component(component, Vec::new(), false)
            .unwrap();
        planned = arena.parallel(planned, singleton).unwrap();

        let root = arena.pars[planned.0]
            .occurrence_root
            .expect("nonempty occurrence dictionary");
        let mut reachable = 0usize;
        let mut work = vec![root];
        while let Some(index) = work.pop() {
            reachable += 1;
            let node = arena.occurrence_nodes[index];
            let left_height = arena.occurrence_height(node.left());
            let right_height = arena.occurrence_height(node.right());
            assert!(left_height.abs_diff(right_height) <= 1);
            assert_eq!(node.height, left_height.max(right_height) + 1);
            assert_eq!(
                node.distinct_len,
                arena.occurrence_distinct_len(node.left())
                    + 1
                    + arena.occurrence_distinct_len(node.right())
            );
            if let Some(left) = node.left() {
                assert_eq!(
                    arena.compare_components(
                        arena.occurrence_nodes[left].occurrence.component,
                        node.occurrence.component,
                    ),
                    Ordering::Less
                );
                work.push(left);
            }
            if let Some(right) = node.right() {
                assert_eq!(
                    arena.compare_components(
                        arena.occurrence_nodes[right].occurrence.component,
                        node.occurrence.component,
                    ),
                    Ordering::Greater
                );
                work.push(right);
            }
        }
        assert_eq!(reachable, DISTINCT);
        assert_eq!(arena.occurrence_distinct_len(Some(root)), DISTINCT);

        let expected = Par::default().with_exprs(expected_exprs);
        let actual = arena.finish(planned).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            arena.protobuf_bytes(planned),
            expected.protobuf_bytes().unwrap()
        );
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
