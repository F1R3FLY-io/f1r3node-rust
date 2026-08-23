//! Authoritative prefix-compressed multiset of a `Par`'s top-level components.
//!
//! A Rholang parallel composition is unordered and may contain the same process
//! occurrence more than once. `ParBag` models exactly that product-of-bags as a
//! single `PathMap<Multiplicity>` (through `rspace++`'s invariant-preserving
//! [`Bag`]). It is not a list, hash map, or shadow index.
//!
//! Each complete trie key is:
//!
//! ```text
//!     "RPB"  0x03  family  order-key  protobuf body  u64-BE(|order-key|)
//!      └domain┘     └split┘ └──────── one complete occurrence ──────────┘
//! ```
//!
//! `order-key` is the prefix-free, lexicographically order-preserving image of
//! the component's score tree.  The protobuf body is consequently both the
//! exact tie-break used by canonical Rholang sorting and the bytes emitted on
//! the legacy protobuf surface.  Serialization walks the trie and copies those
//! bytes directly: it never decodes a component or constructs `Vec<Par>`.
//!
//! Canonical ACT import validates bodies with byte-native schema recognizers
//! where an exact proof is available and otherwise uses the generated
//! stack-safe protobuf PDA; it regenerates the complete key and requires byte
//! equality. Identical typed occurrences therefore share one prefix-compressed
//! path whose value is their checked `u64` multiplicity.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

use prost::bytes::BufMut;
use prost::encoding::{encode_key, encoded_len_varint, key_len, WireType};
use rspace_plus_plus::rspace::bag::{Bag, BagCodecError};
pub use rspace_plus_plus::rspace::bag::{BagError, Multiplicity};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use crate::rust::rholang::protobuf_encoder;
use crate::rust::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode, ProtobufWireType};
use crate::rust::rholang::protobuf_schema_tables::expr_instance_protobuf_kind_by_tag;
use crate::rust::rholang::sorter::bundle_sort_matcher::BundleSortMatcher;
use crate::rust::rholang::sorter::connective_sort_matcher::ConnectiveSortMatcher;
use crate::rust::rholang::sorter::expr_sort_matcher::ExprSortMatcher;
use crate::rust::rholang::sorter::if_sort_matcher::IfSortMatcher;
use crate::rust::rholang::sorter::match_sort_matcher::MatchSortMatcher;
use crate::rust::rholang::sorter::new_sort_matcher::NewSortMatcher;
use crate::rust::rholang::sorter::receive_sort_matcher::ReceiveSortMatcher;
use crate::rust::rholang::sorter::score_tree::{
    decode_order_key, encode_order_key, order_key_extent, Score, ScoreAtom, ScoredTerm, Tree,
};
use crate::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use crate::rust::rholang::sorter::sortable::Sortable;
use crate::rust::rholang::sorter::unforgeable_sort_matcher::UnforgeableSortMatcher;

/// Domain and version prefix for every authoritative ParBag component key.
///
/// The version byte is part of the trie path, so a future encoding cannot
/// silently collide with or reinterpret this key language.
pub const PAR_BAG_KEY_DOMAIN: [u8; 4] = *b"RPB\x03";
const PAR_BAG_FAMILY_PREFIX_LEN: usize = PAR_BAG_KEY_DOMAIN.len() + 1;
const PAR_BAG_SCORE_LEN_BYTES: usize = std::mem::size_of::<u64>();
const PAR_BAG_KEY_HEADER_LEN: usize = PAR_BAG_FAMILY_PREFIX_LEN;
const PAR_BAG_KEY_MIN_LEN: usize = PAR_BAG_KEY_HEADER_LEN + PAR_BAG_SCORE_LEN_BYTES;
const PAR_COMPONENT_FAMILY_COUNT: usize = 9;

const fn project_cardinality(exact: u128) -> u64 {
    if exact > u64::MAX as u128 {
        u64::MAX
    } else {
        exact as u64
    }
}

/// Constructor-family tags, chosen to match the corresponding `Par` protobuf
/// field numbers. Numeric trie order is consequently schema order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ParComponentTag {
    Send = 1,
    Receive = 2,
    New = 4,
    Expr = 5,
    Match = 6,
    GUnforgeable = 7,
    Connective = 8,
    Bundle = 11,
    If = 12,
}

impl ParComponentTag {
    pub const fn byte(self) -> u8 { self as u8 }

    const fn index(self) -> usize {
        match self {
            Self::Send => 0,
            Self::Receive => 1,
            Self::New => 2,
            Self::Expr => 3,
            Self::Match => 4,
            Self::GUnforgeable => 5,
            Self::Connective => 6,
            Self::Bundle => 7,
            Self::If => 8,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Send),
            2 => Some(Self::Receive),
            4 => Some(Self::New),
            5 => Some(Self::Expr),
            6 => Some(Self::Match),
            7 => Some(Self::GUnforgeable),
            8 => Some(Self::Connective),
            11 => Some(Self::Bundle),
            12 => Some(Self::If),
            _ => None,
        }
    }
}

mod sealed {
    pub trait Sealed {}
}

/// One of the nine types that can occur directly in a `Par`.
///
/// Sealing prevents downstream code from assigning an already-used tag to a
/// different byte language and breaking the one-key/one-type invariant.
pub trait ParBagComponent: sealed::Sealed + ProtobufNode + Sized + 'static {
    const TAG: ParComponentTag;

    fn canonicalize(value: &Self) -> ScoredTerm<Self>;

    /// Whether the legacy canonical sorter is defined for this protobuf
    /// value. Proto3 message fields are optional on the wire even where the
    /// Rholang sorter requires them semantically; an untrusted but encodable
    /// message must therefore remain representable without using a panic as
    /// control flow.
    fn semantic_shape_is_complete(_value: &Self) -> bool { true }

    /// Exact contribution to the canonical-path ground classifier when this
    /// value is the sole component of a metadata-free `Par`.
    ///
    /// This is a maintained fold, not a second representation.  It lets an
    /// enclosing `EPathMap<PathMap<Par>>` classify an already-canonical value
    /// without decoding its complete nested protobuf suffix.  Constructor
    /// families that can never be stable use the conservative default.
    fn canonical_path_stable(_value: &Self) -> bool { false }

    /// Append the score-order key and canonical protobuf body for one value.
    ///
    /// The default is the complete stack-safe canonical sorter. Components
    /// whose entire constructor family is already a canonical leaf override
    /// this to avoid allocating a traversal, score tree, and cloned term on
    /// every exact lookup.
    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) -> usize {
        let score_start = out.len();
        if !Self::semantic_shape_is_complete(value) {
            append_nonsemantic_order_key(out);
            let score_len = out.len() - score_start;
            protobuf_encoder::encode_into(value, out);
            return score_len;
        }
        let scored = Self::canonicalize(value);
        encode_order_key(&scored.score, out);
        let score_len = out.len() - score_start;
        protobuf_encoder::encode_into(&scored.term, out);
        score_len
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError>;
    fn decode_protobuf_deferred_epathmap_validation(
        payload: &[u8],
    ) -> Result<Self, prost::DecodeError>;
}

macro_rules! impl_par_bag_component {
    ($ty:ty, $tag:ident, $sorter:ty, $decoder:ident, $deferred_decoder:ident, $shape:ident) => {
        impl sealed::Sealed for $ty {}
        impl ParBagComponent for $ty {
            const TAG: ParComponentTag = ParComponentTag::$tag;

            fn canonicalize(value: &Self) -> ScoredTerm<Self> {
                <$sorter as Sortable<Self>>::sort_match(value)
            }

            fn semantic_shape_is_complete(value: &Self) -> bool { $shape(value) }

            fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
                crate::rust::rholang::protobuf_decoder::$decoder(payload)
            }

            fn decode_protobuf_deferred_epathmap_validation(
                payload: &[u8],
            ) -> Result<Self, prost::DecodeError> {
                crate::rust::rholang::protobuf_decoder::$deferred_decoder(payload)
            }
        }
    };
}

fn send_semantic_shape_is_complete(value: &Send) -> bool { value.chan.is_some() }

fn receive_semantic_shape_is_complete(value: &Receive) -> bool {
    value.body.is_some() && value.binds.iter().all(|bind| bind.source.is_some())
}

fn new_semantic_shape_is_complete(value: &New) -> bool { value.p.is_some() }

fn match_semantic_shape_is_complete(value: &Match) -> bool {
    value.target.is_some()
        && value
            .cases
            .iter()
            .all(|case| case.pattern.is_some() && case.source.is_some())
}

fn bundle_semantic_shape_is_complete(value: &Bundle) -> bool { value.body.is_some() }

fn if_semantic_shape_is_complete(value: &If) -> bool {
    value.condition.is_some() && value.if_true.is_some() && value.if_false.is_some()
}

fn connective_semantic_shape_is_complete(_value: &Connective) -> bool { true }

impl_par_bag_component!(
    Send,
    Send,
    SendSortMatcher,
    decode_send,
    decode_send_deferred_epathmap_validation,
    send_semantic_shape_is_complete
);
impl_par_bag_component!(
    Receive,
    Receive,
    ReceiveSortMatcher,
    decode_receive,
    decode_receive_deferred_epathmap_validation,
    receive_semantic_shape_is_complete
);
impl_par_bag_component!(
    New,
    New,
    NewSortMatcher,
    decode_new,
    decode_new_deferred_epathmap_validation,
    new_semantic_shape_is_complete
);
impl_par_bag_component!(
    Match,
    Match,
    MatchSortMatcher,
    decode_match,
    decode_match_deferred_epathmap_validation,
    match_semantic_shape_is_complete
);
impl_par_bag_component!(
    Connective,
    Connective,
    ConnectiveSortMatcher,
    decode_connective,
    decode_connective_deferred_epathmap_validation,
    connective_semantic_shape_is_complete
);
impl_par_bag_component!(
    Bundle,
    Bundle,
    BundleSortMatcher,
    decode_bundle,
    decode_bundle_deferred_epathmap_validation,
    bundle_semantic_shape_is_complete
);
impl_par_bag_component!(
    If,
    If,
    IfSortMatcher,
    decode_if,
    decode_if_deferred_epathmap_validation,
    if_semantic_shape_is_complete
);

/// Append one score atom without first materializing `Tree<ScoreAtom>`.
/// These are exactly the bytes produced by `score_tree::encode_order_key`.
#[inline]
fn append_int_score_atom(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&[1, 1]);
    out.extend_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
}

#[inline]
fn append_terminated_score_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    // Canonical text/channel identifiers overwhelmingly contain no NUL. Keep
    // that path one bulk copy; enter the escaping loop only when required.
    if !bytes.contains(&0) {
        out.extend_from_slice(bytes);
        out.extend_from_slice(&[0, 0]);
        return;
    }
    for &byte in bytes {
        if byte == 0 {
            out.extend_from_slice(&[0, 0xff]);
        } else {
            out.push(byte);
        }
    }
    out.extend_from_slice(&[0, 0]);
}

#[inline]
fn append_string_score_atom(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&[1, 2]);
    append_terminated_score_bytes(out, value.as_bytes());
}

#[inline]
fn append_bytes_score_atom(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&[1, 3]);
    append_terminated_score_bytes(out, value);
}

#[inline]
fn begin_score_node(out: &mut Vec<u8>, constructor: i32) {
    out.push(2);
    append_int_score_atom(out, i64::from(constructor));
}

#[inline]
fn end_score_node(out: &mut Vec<u8>) { out.push(0); }

/// Append the canonical score of a bounded `Var` without allocating a score
/// tree. This is the byte-exact image of `sort_combine::sort_var` followed by
/// `score_tree::encode_order_key`.
#[inline]
fn append_var_score(value: &crate::rhoapi::Var, out: &mut Vec<u8>) {
    match value.var_instance {
        Some(VarInstance::BoundVar(level)) => {
            begin_score_node(out, Score::BOUND_VAR);
            append_int_score_atom(out, i64::from(level));
            end_score_node(out);
        }
        Some(VarInstance::FreeVar(level)) => {
            begin_score_node(out, Score::FREE_VAR);
            append_int_score_atom(out, i64::from(level));
            end_score_node(out);
        }
        Some(VarInstance::Wildcard(_)) => {
            begin_score_node(out, Score::WILDCARD);
            end_score_node(out);
        }
        None => append_int_score_atom(out, i64::from(Score::ABSENT)),
    }
}

/// Ordered recursive collections do not need the general sorter. Their child
/// order is semantic, and every child `Par` already carries its canonical
/// score prefixes in the authoritative component trie. Stream those prefixes
/// directly instead of decoding them into recursive `Tree`s and encoding the
/// same bytes again.
fn append_ordered_collection_expr_score(value: &Expr, out: &mut Vec<u8>) -> bool {
    match value.expr_instance.as_ref() {
        Some(ExprInstance::EListBody(list)) => {
            begin_score_node(out, Score::ELIST);
            match list.remainder.as_ref() {
                Some(remainder) => append_var_score(remainder, out),
                None => append_int_score_atom(out, -1),
            }
            for child in &list.ps {
                child
                    .components()
                    .append_par_score_key(child.connective_used(), out);
            }
            append_int_score_atom(out, i64::from(list.connective_used));
            end_score_node(out);
            true
        }
        Some(ExprInstance::ETupleBody(tuple)) => {
            begin_score_node(out, Score::ETUPLE);
            for child in &tuple.ps {
                child
                    .components()
                    .append_par_score_key(child.connective_used(), out);
            }
            append_int_score_atom(out, i64::from(tuple.connective_used));
            end_score_node(out);
            true
        }
        _ => false,
    }
}

/// A reserved score domain for protobuf values outside the semantic sorter's
/// input language. The canonical protobuf body remains the exact tie-break in
/// the PathMap key, so malformed values still have a deterministic total order
/// without being normalized or decoded into a shadow collection.
fn append_nonsemantic_order_key(out: &mut Vec<u8>) {
    begin_score_node(out, i32::MAX);
    end_score_node(out);
}

fn expr_semantic_shape_is_complete(value: &Expr) -> bool {
    match value.expr_instance.as_ref() {
        None
        | Some(ExprInstance::GBool(_))
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
        | Some(ExprInstance::EPathmapBody(_)) => true,
        Some(ExprInstance::EVarBody(value)) => value.v.is_some(),
        Some(ExprInstance::ENotBody(value)) => value.p.is_some(),
        Some(ExprInstance::ENegBody(value)) => value.p.is_some(),
        Some(ExprInstance::EMultBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EDivBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EModBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EPlusBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EMinusBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::ELtBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::ELteBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EGtBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EGteBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EEqBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::ENeqBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EAndBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EOrBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EPercentPercentBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EPlusPlusBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EMinusMinusBody(value)) => value.p1.is_some() && value.p2.is_some(),
        Some(ExprInstance::EMatchesBody(value)) => {
            value.target.is_some() && value.pattern.is_some()
        }
        Some(ExprInstance::EMapBody(value)) => value
            .kvs
            .iter()
            .all(|pair| pair.key.is_some() && pair.value.is_some()),
        Some(ExprInstance::EMethodBody(value)) => value.target.is_some(),
        Some(ExprInstance::EZipperBody(value)) => value.pathmap.is_some(),
    }
}

/// Encode a schema node whose protobuf shape has at most one non-recursive
/// message edge. This is still the generated protobuf program: it merely
/// specializes the general PDA's zero/one-edge cases so a ground value does
/// not allocate the PDA's length table and two obligation stacks.
///
/// `false` is returned before writing anything when the node contains a
/// sequence, map, PathMap snapshot, or a message grandchild. Such nodes use the
/// full generated stack-safe machine.
fn append_shallow_protobuf(value: &dyn ProtobufNode, out: &mut Vec<u8>) -> bool {
    let (_, descent) = value.protobuf_len_step(0);
    match descent {
        ProtobufDescent::Done => {
            assert!(matches!(value.protobuf_emit(0, out), ProtobufDescent::Done));
        }
        ProtobufDescent::Node { resume, tag, node } => {
            let (child_len, child_descent) = node.protobuf_len_step(0);
            if !matches!(child_descent, ProtobufDescent::Done)
                || !matches!(
                    value.protobuf_len_step(usize::from(resume)).1,
                    ProtobufDescent::Done
                )
            {
                return false;
            }

            let emitted = value.protobuf_emit(0, out);
            assert!(matches!(
                emitted,
                ProtobufDescent::Node {
                    resume: emitted_resume,
                    tag: emitted_tag,
                    ..
                } if emitted_resume == resume && emitted_tag == tag
            ));
            encode_key(tag, WireType::LengthDelimited, out);
            prost::encoding::encode_varint(child_len, out);
            assert!(matches!(node.protobuf_emit(0, out), ProtobufDescent::Done));
            assert!(matches!(
                value.protobuf_emit(usize::from(resume), out),
                ProtobufDescent::Done
            ));
        }
        ProtobufDescent::Seq { .. }
        | ProtobufDescent::Map { .. }
        | ProtobufDescent::EPathMapSnapshot { .. } => return false,
    }
    true
}

/// Ground `Expr`s and absent `Expr` are canonical leaves. Encode their exact
/// score directly instead of starting the general mutually-recursive sorter.
/// Returns false for every arm that can contain or reorder child structure.
fn append_leaf_expr_score(value: &Expr, out: &mut Vec<u8>) -> bool {
    let constructor = match value.expr_instance.as_ref() {
        None => Score::ABSENT,
        Some(ExprInstance::GBool(_)) => Score::BOOL,
        Some(ExprInstance::GInt(_)) => Score::INT,
        Some(ExprInstance::GString(_)) => Score::STRING,
        Some(ExprInstance::GUri(_)) => Score::URI,
        Some(ExprInstance::GByteArray(_)) => Score::EBYTEARR,
        Some(ExprInstance::GDouble(_)) => Score::DOUBLE,
        Some(ExprInstance::GBigInt(_)) => Score::BIG_INT,
        Some(ExprInstance::GBigRat(_)) => Score::BIG_RAT,
        Some(ExprInstance::GFixedPoint(_)) => Score::FIXED_POINT,
        Some(_) => return false,
    };
    begin_score_node(out, constructor);

    match value.expr_instance.as_ref() {
        None => {}
        Some(ExprInstance::GBool(value)) => {
            // Historical canonical order deliberately places true before false.
            append_int_score_atom(out, i64::from(!*value));
        }
        Some(ExprInstance::GInt(value)) => append_int_score_atom(out, *value),
        Some(ExprInstance::GString(value)) | Some(ExprInstance::GUri(value)) => {
            append_string_score_atom(out, value);
        }
        Some(ExprInstance::GByteArray(value)) | Some(ExprInstance::GBigInt(value)) => {
            append_bytes_score_atom(out, value);
        }
        Some(ExprInstance::GDouble(bits)) => append_int_score_atom(out, *bits as i64),
        Some(ExprInstance::GBigRat(value)) => {
            append_bytes_score_atom(out, &value.numerator);
            append_bytes_score_atom(out, &value.denominator);
        }
        Some(ExprInstance::GFixedPoint(value)) => {
            append_bytes_score_atom(out, &value.unscaled);
            out.push(2);
            append_int_score_atom(out, i64::from(value.scale));
            end_score_node(out);
        }
        Some(_) => unreachable!("non-leaf Expr returned before score emission"),
    }
    end_score_node(out);
    true
}

/// Score the two PathMap-backed expression families from their authoritative
/// canonical byte image. Re-entering the general sorter would decode every
/// set key and rebuild the same trie; nested non-ground EPathMaps then form a
/// doubling recurrence because decoding each escape constructs and sorts its
/// complete suffix again. The snapshot is already canonical, prefix-compressed
/// PathMap state, so it is the natural total order key for these terminal
/// families.
fn append_pathmap_expr_score(value: &Expr, out: &mut Vec<u8>) -> bool {
    let (constructor, snapshot, connective_used) = match value.expr_instance.as_ref() {
        Some(ExprInstance::EPathmapBody(pathmap)) => (
            Score::EPATHMAP,
            pathmap.trie_snapshot(),
            pathmap.connective_used,
        ),
        Some(ExprInstance::EZipperBody(zipper)) => {
            let Some(pathmap) = zipper.pathmap.as_ref() else {
                return false;
            };
            (
                Score::EPATHMAP + 1,
                pathmap.trie_snapshot(),
                zipper.connective_used,
            )
        }
        _ => return false,
    };

    begin_score_node(out, constructor);
    append_bytes_score_atom(out, snapshot);
    append_int_score_atom(out, i64::from(connective_used));
    end_score_node(out);
    true
}

impl sealed::Sealed for Expr {}
impl ParBagComponent for Expr {
    const TAG: ParComponentTag = ParComponentTag::Expr;

    fn canonicalize(value: &Self) -> ScoredTerm<Self> {
        <ExprSortMatcher as Sortable<Expr>>::sort_match(value)
    }

    fn semantic_shape_is_complete(value: &Self) -> bool { expr_semantic_shape_is_complete(value) }

    fn canonical_path_stable(value: &Self) -> bool {
        match value.expr_instance.as_ref() {
            Some(
                ExprInstance::GBool(_)
                | ExprInstance::GInt(_)
                | ExprInstance::GString(_)
                | ExprInstance::GUri(_)
                | ExprInstance::GByteArray(_)
                | ExprInstance::GDouble(_)
                | ExprInstance::GBigInt(_)
                | ExprInstance::GBigRat(_)
                | ExprInstance::GFixedPoint(_),
            ) => true,
            Some(ExprInstance::EListBody(list)) => {
                list.remainder.is_none()
                    && list.locally_free.is_empty()
                    && !list.connective_used
                    && list
                        .ps
                        .iter()
                        .all(|child| crate::rust::pathmap_crate_type_mapper::eval_stable_par(child))
            }
            Some(ExprInstance::ETupleBody(tuple)) => {
                tuple.locally_free.is_empty()
                    && !tuple.connective_used
                    && tuple
                        .ps
                        .iter()
                        .all(|child| crate::rust::pathmap_crate_type_mapper::eval_stable_par(child))
            }
            Some(ExprInstance::EPathmapBody(pathmap)) => {
                pathmap.remainder.is_none()
                    && pathmap.locally_free.is_empty()
                    && !pathmap.connective_used
                    && pathmap.entry_trie().entries_stable()
            }
            _ => false,
        }
    }

    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) -> usize {
        let score_start = out.len();
        if !Self::semantic_shape_is_complete(value) {
            append_nonsemantic_order_key(out);
            let score_len = out.len() - score_start;
            protobuf_encoder::encode_into(value, out);
            return score_len;
        }
        if append_pathmap_expr_score(value, out) {
            let score_len = out.len() - score_start;
            protobuf_encoder::encode_into(value, out);
            return score_len;
        }
        if append_ordered_collection_expr_score(value, out) {
            let score_len = out.len() - score_start;
            protobuf_encoder::encode_into(value, out);
            return score_len;
        }
        if !append_leaf_expr_score(value, out) {
            let scored = Self::canonicalize(value);
            encode_order_key(&scored.score, out);
            let score_len = out.len() - score_start;
            protobuf_encoder::encode_into(&scored.term, out);
            return score_len;
        }
        let score_len = out.len() - score_start;
        assert!(
            append_shallow_protobuf(value, out),
            "ground Expr has at most one non-recursive protobuf message edge"
        );
        score_len
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_expr(payload)
    }

    fn decode_protobuf_deferred_epathmap_validation(
        payload: &[u8],
    ) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_expr_deferred_epathmap_validation(payload)
    }
}

impl sealed::Sealed for GUnforgeable {}
impl ParBagComponent for GUnforgeable {
    const TAG: ParComponentTag = ParComponentTag::GUnforgeable;

    fn canonicalize(value: &Self) -> ScoredTerm<Self> {
        <UnforgeableSortMatcher as Sortable<GUnforgeable>>::sort_match(value)
    }

    fn canonical_path_stable(value: &Self) -> bool {
        matches!(value.unf_instance, Some(UnfInstance::GPrivateBody(_)))
    }

    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) -> usize {
        let score_start = out.len();
        let (constructor, bytes) = match value.unf_instance.as_ref() {
            None => (Score::ABSENT, None),
            Some(UnfInstance::GPrivateBody(value)) => (Score::PRIVATE, Some(value.id.as_slice())),
            Some(UnfInstance::GDeployerIdBody(value)) => {
                (Score::DEPLOYER_AUTH, Some(value.public_key.as_slice()))
            }
            Some(UnfInstance::GDeployIdBody(value)) => {
                (Score::DEPLOY_ID, Some(value.sig.as_slice()))
            }
            Some(UnfInstance::GSysAuthTokenBody(_)) => (Score::SYS_AUTH_TOKEN, None),
        };
        begin_score_node(out, constructor);
        if let Some(bytes) = bytes {
            append_bytes_score_atom(out, bytes);
        }
        end_score_node(out);
        let score_len = out.len() - score_start;
        assert!(
            append_shallow_protobuf(value, out),
            "GUnforgeable has exactly one non-recursive protobuf message edge"
        );
        score_len
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_g_unforgeable(payload)
    }

    fn decode_protobuf_deferred_epathmap_validation(
        payload: &[u8],
    ) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_g_unforgeable_deferred_epathmap_validation(
            payload,
        )
    }
}

/// Failure to decode or validate the typed key language of a ParBag image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParBagCodecError {
    Bag(BagCodecError),
    KeyTooShort {
        length: usize,
    },
    InvalidDomain {
        found: [u8; 4],
    },
    InvalidTag(u8),
    InvalidOrderKey {
        tag: ParComponentTag,
    },
    ComponentDecode {
        tag: ParComponentTag,
        reason: String,
    },
    NonCanonicalComponent {
        tag: ParComponentTag,
    },
    FamilyCardinalityOverflow {
        tag: ParComponentTag,
    },
}

impl fmt::Display for ParBagCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bag(error) => write!(formatter, "invalid ParBag trie image: {error}"),
            Self::KeyTooShort { length } => write!(
                formatter,
                "ParBag key is {length} byte(s), shorter than its {PAR_BAG_KEY_MIN_LEN}-byte minimum"
            ),
            Self::InvalidDomain { found } => {
                write!(formatter, "invalid ParBag key domain/version: {found:02x?}")
            }
            Self::InvalidTag(tag) => write!(formatter, "invalid ParBag component tag {tag}"),
            Self::InvalidOrderKey { tag } => {
                write!(formatter, "invalid {tag:?} canonical order key")
            }
            Self::ComponentDecode { tag, reason } => {
                write!(formatter, "invalid {tag:?} protobuf component body: {reason}")
            }
            Self::NonCanonicalComponent { tag } => {
                write!(formatter, "non-canonical {tag:?} ParBag key")
            }
            Self::FamilyCardinalityOverflow { tag } => write!(
                formatter,
                "{tag:?} ParBag family cardinality exceeds u128::MAX"
            ),
        }
    }
}

impl std::error::Error for ParBagCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bag(error) => Some(error),
            _ => None,
        }
    }
}

impl From<BagCodecError> for ParBagCodecError {
    fn from(error: BagCodecError) -> Self { Self::Bag(error) }
}

/// Failure to expand the compact multiplicity representation onto the legacy
/// repeated-message protobuf surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParBagProtobufError {
    Codec(ParBagCodecError),
    EncodedLengthOverflow {
        tag: ParComponentTag,
        multiplicity: u64,
        one_occurrence_bytes: usize,
    },
}

impl fmt::Display for ParBagProtobufError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(formatter),
            Self::EncodedLengthOverflow {
                tag,
                multiplicity,
                one_occurrence_bytes,
            } => write!(
                formatter,
                "expanding {multiplicity} {tag:?} occurrence(s) of {one_occurrence_bytes} byte(s) overflows the protobuf output length"
            ),
        }
    }
}

impl std::error::Error for ParBagProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(error) => Some(error),
            Self::EncodedLengthOverflow { .. } => None,
        }
    }
}

impl From<ParBagCodecError> for ParBagProtobufError {
    fn from(error: ParBagCodecError) -> Self { Self::Codec(error) }
}

/// Error from a decoded trie traversal, preserving a caller's own error type.
#[derive(Debug)]
pub enum ParBagVisitError<E> {
    Codec(ParBagCodecError),
    Visitor(E),
}

impl<E: fmt::Display> fmt::Display for ParBagVisitError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(formatter),
            Self::Visitor(error) => error.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ParBagVisitError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(error) => Some(error),
            Self::Visitor(error) => Some(error),
        }
    }
}

impl<E> From<ParBagCodecError> for ParBagVisitError<E> {
    fn from(error: ParBagCodecError) -> Self { Self::Codec(error) }
}

const KEY_BUFFER_CAPACITY: usize = 256;
const KEY_BUFFER_SHRINK_THRESHOLD: usize = 1 << 20;

thread_local! {
    /// Reused complete-key buffer. The sorter and generated protobuf encoder
    /// append directly after the domain and tag, avoiding a second temporary
    /// byte vector in the steady state.
    static KEY_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(KEY_BUFFER_CAPACITY));
}

fn component_prefix(tag: ParComponentTag) -> [u8; PAR_BAG_FAMILY_PREFIX_LEN] {
    let mut prefix = [0u8; PAR_BAG_FAMILY_PREFIX_LEN];
    prefix[..PAR_BAG_KEY_DOMAIN.len()].copy_from_slice(&PAR_BAG_KEY_DOMAIN);
    prefix[PAR_BAG_KEY_DOMAIN.len()] = tag.byte();
    prefix
}

/// Encode one typed occurrence directly into a reusable complete PathMap key.
///
/// The callback must not retain `key`. Re-entrant encoding falls back to a
/// private buffer instead of panicking on a thread-local borrow.
pub fn with_component_key<T, R>(value: &T, callback: impl FnOnce(&[u8]) -> R) -> R
where T: ParBagComponent {
    fn append_key<T: ParBagComponent>(value: &T, key: &mut Vec<u8>) {
        key.extend_from_slice(&component_prefix(T::TAG));
        let score_len = T::append_key_suffix(value, key);
        let score_len =
            u64::try_from(score_len).expect("one allocated ParBag score key length fits in u64");
        key.extend_from_slice(&score_len.to_be_bytes());
    }

    KEY_BUFFER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut key) => {
            key.clear();
            append_key(value, &mut key);
            let result = callback(&key);
            if key.capacity() > KEY_BUFFER_SHRINK_THRESHOLD {
                key.shrink_to(KEY_BUFFER_CAPACITY);
            }
            result
        }
        Err(_) => {
            let mut key = Vec::with_capacity(KEY_BUFFER_CAPACITY);
            append_key(value, &mut key);
            callback(&key)
        }
    })
}

fn decode_exact<T: ParBagComponent>(payload: &[u8]) -> Result<T, ParBagCodecError> {
    T::decode_protobuf_deferred_epathmap_validation(payload).map_err(|source| {
        ParBagCodecError::ComponentDecode {
            tag: T::TAG,
            reason: source.to_string(),
        }
    })
}

fn split_key_parts(path: &[u8]) -> Result<(ParComponentTag, &[u8], &[u8]), ParBagCodecError> {
    if path.len() < PAR_BAG_KEY_MIN_LEN {
        return Err(ParBagCodecError::KeyTooShort { length: path.len() });
    }
    let mut found = [0u8; PAR_BAG_KEY_DOMAIN.len()];
    found.copy_from_slice(&path[..PAR_BAG_KEY_DOMAIN.len()]);
    if found != PAR_BAG_KEY_DOMAIN {
        return Err(ParBagCodecError::InvalidDomain { found });
    }
    let tag_byte = path[PAR_BAG_KEY_DOMAIN.len()];
    let tag = ParComponentTag::from_byte(tag_byte).ok_or(ParBagCodecError::InvalidTag(tag_byte))?;
    let (score, payload) = split_key_suffix(tag, &path[PAR_BAG_KEY_HEADER_LEN..])?;
    Ok((tag, score, payload))
}

/// Split the schema-known suffix of one RPB3 key without materializing its
/// domain-and-family prefix.
fn split_key_suffix(
    tag: ParComponentTag,
    suffix: &[u8],
) -> Result<(&[u8], &[u8]), ParBagCodecError> {
    if suffix.len() < PAR_BAG_SCORE_LEN_BYTES {
        return Err(ParBagCodecError::KeyTooShort {
            length: PAR_BAG_KEY_HEADER_LEN + suffix.len(),
        });
    }
    let payload_end = suffix.len() - PAR_BAG_SCORE_LEN_BYTES;
    let declared = u64::from_be_bytes(
        suffix[payload_end..]
            .try_into()
            .expect("the checked key trailer contains one u64 score length"),
    );
    let score_len =
        usize::try_from(declared).map_err(|_| ParBagCodecError::InvalidOrderKey { tag })?;
    let score_end = (score_len <= payload_end)
        .then_some(score_len)
        .ok_or(ParBagCodecError::InvalidOrderKey { tag })?;
    Ok((&suffix[..score_end], &suffix[score_end..payload_end]))
}

fn split_key(path: &[u8]) -> Result<(ParComponentTag, &[u8]), ParBagCodecError> {
    let (tag, _score, payload) = split_key_parts(path)?;
    Ok((tag, payload))
}

/// Read one minimally encoded protobuf varint. Returning `None` delegates the
/// input to the complete generated validator, which preserves its exact error
/// classification for malformed inputs.
fn read_canonical_varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let start = *at;
    let mut value = 0u64;
    for shift in (0u32..=63).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        let payload = u64::from(byte & 0x7f);
        if shift == 63 && payload > 1 {
            return None;
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            return (prost::encoding::encoded_len_varint(value) == *at - start).then_some(value);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CanonicalExprEnvelope<'a> {
    tag: u32,
    payload: &'a [u8],
}

/// Prove the exact outer framing of one known `Expr` oneof field under the
/// authoritative ParBag-byte provenance while deliberately leaving the arm's
/// payload uninterpreted.
///
/// This shallow proof is distinct from [`canonical_expr_parts`]: a proto3
/// message arm can have a valid outer envelope while omitting one of the
/// required child messages assumed by the Rholang AST. Matcher dispatch may
/// use the returned tag to reject a proven different variant, but must still
/// delegate a same-tag payload that the deeper cursor cannot prove.
fn canonical_expr_envelope(message: &[u8]) -> Option<CanonicalExprEnvelope<'_>> {
    let mut at = 0usize;
    let key = read_canonical_varint(message, &mut at)?;
    let tag = u32::try_from(key >> 3).ok()?;
    let wire = key & 0x07;
    let expected_wire = expr_instance_protobuf_kind_by_tag(tag)?.wire_type()?;
    (wire == expected_wire.code()).then_some(())?;
    let payload_start = at;
    let payload = match expected_wire {
        ProtobufWireType::Varint => {
            let _ = read_canonical_varint(message, &mut at)?;
            message.get(payload_start..at)?
        }
        ProtobufWireType::Fixed64 => {
            at = at.checked_add(8).filter(|end| *end <= message.len())?;
            message.get(payload_start..at)?
        }
        ProtobufWireType::LengthDelimited => {
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            let end = at.checked_add(len)?;
            let payload = message.get(at..end)?;
            at = end;
            payload
        }
        ProtobufWireType::Fixed32 => {
            at = at.checked_add(4).filter(|end| *end <= message.len())?;
            message.get(payload_start..at)?
        }
    };
    (at == message.len()).then_some(CanonicalExprEnvelope { tag, payload })
}

/// Return the sole canonically framed outer `Expr` oneof field number without
/// validating scalar payload semantics or recursive child-message invariants.
///
/// `None` means only that this shallow cursor could not prove the envelope. It
/// must never be interpreted as a semantic negative by a matcher.
pub fn canonical_expr_oneof_tag(message: &[u8]) -> Option<u32> {
    canonical_expr_envelope(message).map(|envelope| envelope.tag)
}

/// Borrowed score and metadata inputs of one trusted canonical EZipper region.
pub(crate) struct CanonicalEzipperRegionParts<'a> {
    pub(crate) snapshot: &'a [u8],
    pub(crate) locally_free: &'a [u8],
    pub(crate) connective_used: bool,
}

/// Borrow the score input and substitution summaries from an authoritative
/// canonical `Expr(EZipper)` body while skipping the cursor's PathMap values
/// and path segments by their protobuf extents.
///
/// This is deliberately a *resident-key* cursor, not a general protobuf
/// validator. Its caller must prove that `message` is either the payload of a
/// typed RPB3 endpoint or a subregion cut from such a payload. At the RPB3
/// boundary [`validate_typed_key`] decodes the component and recompiles its
/// complete key; consequently a missing required field, a non-canonical
/// collection order, or an invalid EPM1 snapshot cannot enter this lane. The
/// shallow scan below then preserves those already-authoritative bytes without
/// decoding the recursive `PathMap<Par>` value graph a second time.
pub(crate) fn canonical_ezipper_region_parts(
    message: &[u8],
) -> Option<CanonicalEzipperRegionParts<'_>> {
    let CanonicalExprEnvelope {
        tag: 33,
        payload: zipper,
    } = canonical_expr_envelope(message)?
    else {
        return None;
    };

    let mut at = 0usize;
    let mut last_field = 0u64;
    let mut saw_pathmap = false;
    let mut saw_is_write = false;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    let mut saw_cursor_kind = false;
    let mut pathmap = None;
    let mut locally_free = &[][..];
    while at < zipper.len() {
        let key = read_canonical_varint(zipper, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) if !saw_pathmap => {
                let len = usize::try_from(read_canonical_varint(zipper, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                pathmap = Some(zipper.get(at..end)?);
                at = end;
                saw_pathmap = true;
            }
            // `current_path` is the sole repeated field.
            (2, 2) => {
                let len = usize::try_from(read_canonical_varint(zipper, &mut at)?).ok()?;
                at = at.checked_add(len).filter(|end| *end <= zipper.len())?;
            }
            // Canonical proto3 omits false scalar defaults.
            (3, 0) if !saw_is_write => {
                (read_canonical_varint(zipper, &mut at)? == 1).then_some(())?;
                saw_is_write = true;
            }
            (4, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(zipper, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                let end = at.checked_add(len)?;
                locally_free = zipper.get(at..end)?;
                at = end;
                saw_locally_free = true;
            }
            (5, 0) if !saw_connective => {
                (read_canonical_varint(zipper, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            (6, 0) if !saw_cursor_kind => {
                matches!(read_canonical_varint(zipper, &mut at)?, 1 | 2).then_some(())?;
                saw_cursor_kind = true;
            }
            _ => return None,
        }
    }
    let pathmap = pathmap.filter(|_| saw_pathmap)?;
    let map = canonical_epathmap_parts(pathmap)?;
    Some(CanonicalEzipperRegionParts {
        snapshot: map.snapshot,
        locally_free,
        connective_used: saw_connective,
    })
}

/// Extract the level from an exact canonical `Expr(EVar(FreeVar(level)))`
/// without decoding any recursively nested expression alternative.
///
/// This cursor is intentionally exact: every enclosing message must contain
/// only its required oneof field, and the `sint32` payload must fit the schema
/// domain. `None` is lack of proof, so callers must delegate to their complete
/// semantic path rather than treating an unproved message as a different
/// expression.
pub fn canonical_exact_free_var_expr_level(message: &[u8]) -> Option<i32> {
    const EVAR_TAG: u32 = 19;
    const EVAR_VAR_KEY: u64 = (1 << 3) | 2;

    let envelope = canonical_expr_envelope(message)?;
    (envelope.tag == EVAR_TAG).then_some(())?;
    let variable = canonical_single_delimited_field(envelope.payload, EVAR_VAR_KEY)?;
    match canonical_var(variable)? {
        CanonicalVar::Free(level) => Some(level),
        _ => None,
    }
}

/// The two exact `EVar` forms that compile to an ordered-query hole.
///
/// This is a byte-native classification over an already canonical component
/// body. `None` is lack of proof and must retain the ordinary semantic path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalExactHole {
    FreeVar(i32),
    Wildcard,
}

/// Classify an exact canonical `Expr(EVar(FreeVar | Wildcard))` without
/// constructing `Expr` or `Var`.
pub fn canonical_exact_hole_expr(message: &[u8]) -> Option<CanonicalExactHole> {
    const EVAR_TAG: u32 = 19;
    const EVAR_VAR_KEY: u64 = (1 << 3) | 2;

    let envelope = canonical_expr_envelope(message)?;
    (envelope.tag == EVAR_TAG).then_some(())?;
    let variable = canonical_single_delimited_field(envelope.payload, EVAR_VAR_KEY)?;
    match canonical_var(variable)? {
        CanonicalVar::Free(level) => Some(CanonicalExactHole::FreeVar(level)),
        CanonicalVar::Wildcard => Some(CanonicalExactHole::Wildcard),
        CanonicalVar::Absent | CanonicalVar::Bound(_) => None,
    }
}

/// A canonical singleton-Expr `Par` whose expression is exactly one free
/// variable. Root matcher metadata is retained because `connective_used`
/// controls whether the variable denotes a capture or ordinary data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalExactFreeVarPar {
    level: i32,
    connective_used: bool,
}

impl CanonicalExactFreeVarPar {
    pub const fn level(self) -> i32 { self.level }

    pub const fn connective_used(self) -> bool { self.connective_used }
}

/// Match a canonical message containing exactly one length-delimited field and
/// return that field's body. This deliberately accepts no unknown, duplicate,
/// reordered, or trailing field: Prost's canonical encoder would emit none of
/// those shapes.
fn canonical_single_delimited_field(message: &[u8], expected_key: u64) -> Option<&[u8]> {
    let mut at = 0usize;
    (read_canonical_varint(message, &mut at)? == expected_key).then_some(())?;
    let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
    let end = at.checked_add(len)?;
    (end == message.len()).then(|| &message[at..end])
}

/// Match a canonical message containing exactly one varint field.
///
/// As with [`canonical_single_delimited_field`], accepting only a minimal key,
/// a minimal value, and no trailing field is deliberate: the direct importer
/// may bypass the generated semantic decoder only for bytes that the canonical
/// protobuf encoder itself emits.
fn canonical_single_varint_field(message: &[u8], expected_key: u64) -> Option<u64> {
    let mut at = 0usize;
    (read_canonical_varint(message, &mut at)? == expected_key).then_some(())?;
    let value = read_canonical_varint(message, &mut at)?;
    (at == message.len()).then_some(value)
}

/// The exact canonical protobuf image of `rhoapi.Var`.
///
/// `Var` is a oneof, so an explicitly present default scalar still emits its
/// oneof key and a zero payload. The empty byte string is the distinct
/// `Var::default()` value with no selected arm. Keeping this grammar in one
/// cursor prevents specialized collection paths from silently accepting a
/// different wire language than the generated decoder/canonical encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanonicalVar {
    Absent,
    Bound(i32),
    Free(i32),
    Wildcard,
}

#[inline]
fn decode_canonical_sint32(encoded: u64) -> Option<i32> {
    let encoded = u32::try_from(encoded).ok()?;
    Some(((encoded >> 1) as i32) ^ -((encoded & 1) as i32))
}

fn canonical_var(message: &[u8]) -> Option<CanonicalVar> {
    const BOUND_VAR_KEY: u64 = 1 << 3;
    const FREE_VAR_KEY: u64 = 2 << 3;
    const WILDCARD_KEY: u64 = (3 << 3) | 2;

    if message.is_empty() {
        return Some(CanonicalVar::Absent);
    }
    if let Some(encoded) = canonical_single_varint_field(message, BOUND_VAR_KEY) {
        return decode_canonical_sint32(encoded).map(CanonicalVar::Bound);
    }
    if let Some(encoded) = canonical_single_varint_field(message, FREE_VAR_KEY) {
        return decode_canonical_sint32(encoded).map(CanonicalVar::Free);
    }
    canonical_single_delimited_field(message, WILDCARD_KEY)
        .filter(|body| body.is_empty())
        .map(|_| CanonicalVar::Wildcard)
}

/// Extract the sole Connective component body from a canonical singleton
/// connective `Par`. Root locally-free/connective metadata may follow the
/// component, exactly as emitted by the canonical protobuf encoder.
pub fn canonical_single_connective_from_par(message: &[u8]) -> Option<&[u8]> {
    const CONNECTIVE_KEY: u64 = (8 << 3) | 2;
    const LOCALLY_FREE_KEY: u64 = (9 << 3) | 2;
    const CONNECTIVE_USED_KEY: u64 = 10 << 3;

    let mut at = 0usize;
    (read_canonical_varint(message, &mut at)? == CONNECTIVE_KEY).then_some(())?;
    let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
    let end = at.checked_add(len)?;
    let connective = message.get(at..end)?;
    at = end;
    if at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        if key == LOCALLY_FREE_KEY {
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
            if at < message.len() && read_canonical_varint(message, &mut at)? != CONNECTIVE_USED_KEY
            {
                return None;
            }
            if at < message.len() {
                let _ = read_canonical_varint(message, &mut at)?;
            }
        } else if key == CONNECTIVE_USED_KEY {
            let _ = read_canonical_varint(message, &mut at)?;
        } else {
            return None;
        }
    }
    (at == message.len()).then_some(connective)
}

/// A validated, allocation-free cursor over the repeated `Par` children of a
/// canonical [`ConnectiveBody`].
///
/// Construction validates the complete body once. Iteration can therefore be
/// infallible and lend child protobuf regions without decoding or collecting a
/// `Vec<&[u8]>`. The cursor is deliberately restartable by value so an
/// explicit PDA can suspend it beside a child obligation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalConnectiveChildren<'a> {
    message: &'a [u8],
    cursor: CanonicalConnectiveChildCursor,
}

/// Slice-independent continuation for a validated canonical
/// [`ConnectiveBody`] child sequence.
///
/// The byte slice remains with the caller (or with an owned encoded region in
/// a PDA frame), while this small value records only the next byte offset and
/// remaining arity. That separation lets borrowed compilation and owned
/// authoritative matching share one framing implementation without a
/// self-referential object or a projected `Vec<Par>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalConnectiveChildCursor {
    at: usize,
    remaining: usize,
}

impl<'a> CanonicalConnectiveChildren<'a> {
    fn new(message: &'a [u8]) -> Option<Self> {
        const CHILD_KEY: u64 = (1 << 3) | 2;

        let mut at = 0usize;
        let mut remaining = 0usize;
        while at < message.len() {
            (read_canonical_varint(message, &mut at)? == CHILD_KEY).then_some(())?;
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
            remaining = remaining.checked_add(1)?;
        }
        Some(Self {
            message,
            cursor: CanonicalConnectiveChildCursor { at: 0, remaining },
        })
    }

    pub const fn len(&self) -> usize { self.cursor.len() }

    pub const fn is_empty(&self) -> bool { self.cursor.is_empty() }

    /// Complete validated `ConnectiveBody` bytes driven by this cursor.
    pub const fn message(&self) -> &'a [u8] { self.message }

    /// Export the slice-independent continuation for an owned PDA frame.
    pub const fn cursor(&self) -> CanonicalConnectiveChildCursor { self.cursor }
}

impl CanonicalConnectiveChildCursor {
    pub const fn len(&self) -> usize { self.remaining }

    pub const fn is_empty(&self) -> bool { self.remaining == 0 }

    /// Lend the next child from the same body that was fully validated when
    /// this cursor was created.
    pub fn next_in<'a>(&mut self, message: &'a [u8]) -> Option<&'a [u8]> {
        const CHILD_KEY: u64 = (1 << 3) | 2;

        if self.remaining == 0 {
            return None;
        }
        // This read is part of the cursor transition, not a diagnostic. Keep
        // it outside `debug_assert!`: assertions are erased in release builds
        // and must never own parser state changes.
        let key = read_canonical_varint(message, &mut self.at)
            .expect("validated ConnectiveBody child key remains present");
        assert_eq!(
            key, CHILD_KEY,
            "validated ConnectiveBody cursor remains aligned to a child key"
        );
        let len = usize::try_from(read_canonical_varint(message, &mut self.at)?)
            .expect("validated ConnectiveBody child length fits usize");
        let end = self
            .at
            .checked_add(len)
            .expect("validated ConnectiveBody child end fits usize");
        let child = message
            .get(self.at..end)
            .expect("validated ConnectiveBody child range remains in bounds");
        self.at = end;
        self.remaining -= 1;
        Some(child)
    }
}

impl<'a> Iterator for CanonicalConnectiveChildren<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> { self.cursor.next_in(self.message) }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.cursor.remaining, Some(self.cursor.remaining))
    }
}

impl ExactSizeIterator for CanonicalConnectiveChildren<'_> {}

/// Canonical wire-level connective shape used by byte-native candidate-plan
/// compilation.
///
/// This is a routing proof, not a second semantic authority. `None` from
/// [`canonical_connective_parts`] means the complete generated decoder must be
/// used. In particular, no malformed or non-canonical message is interpreted
/// as `Empty` or `All` merely because its first tag resembles a connective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalConnectiveParts<'a> {
    And(CanonicalConnectiveChildren<'a>),
    Or(CanonicalConnectiveChildren<'a>),
    Not(&'a [u8]),
    VarRef,
    Predicate { tag: u32, value: bool },
    Absent,
}

/// Split one complete canonical `Connective` protobuf message without
/// materializing its recursive child `Par`s.
pub fn canonical_connective_parts(message: &[u8]) -> Option<CanonicalConnectiveParts<'_>> {
    if message.is_empty() {
        return Some(CanonicalConnectiveParts::Absent);
    }

    let mut at = 0usize;
    let key = read_canonical_varint(message, &mut at)?;
    let tag = u32::try_from(key >> 3).ok()?;
    let wire = key & 0x07;
    match tag {
        1..=4 if wire == 2 => {
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            let end = at.checked_add(len)?;
            let payload = message.get(at..end)?;
            (end == message.len()).then_some(())?;
            match tag {
                1 => CanonicalConnectiveChildren::new(payload).map(CanonicalConnectiveParts::And),
                2 => CanonicalConnectiveChildren::new(payload).map(CanonicalConnectiveParts::Or),
                3 => Some(CanonicalConnectiveParts::Not(payload)),
                4 => canonical_var_ref_is_canonical(payload)
                    .then_some(CanonicalConnectiveParts::VarRef),
                _ => unreachable!("the connective message tag is in 1..=4"),
            }
        }
        5..=9 if wire == 0 => {
            let value = read_canonical_varint(message, &mut at)?;
            (at == message.len() && value <= 1).then_some(CanonicalConnectiveParts::Predicate {
                tag,
                value: value != 0,
            })
        }
        _ => None,
    }
}

fn canonical_var_ref_is_canonical(message: &[u8]) -> bool {
    const INDEX_KEY: u64 = 1 << 3;
    const DEPTH_KEY: u64 = 2 << 3;

    let mut at = 0usize;
    let mut last_key = 0u64;
    while at < message.len() {
        let Some(key) = read_canonical_varint(message, &mut at) else {
            return false;
        };
        if !matches!(key, INDEX_KEY | DEPTH_KEY) || key <= last_key {
            return false;
        }
        let Some(encoded) = read_canonical_varint(message, &mut at) else {
            return false;
        };
        // Both fields are sint32. Values outside the zig-zag u32 image cannot
        // have been emitted by the generated canonical encoder.
        if u32::try_from(encoded).is_err() {
            return false;
        }
        last_key = key;
    }
    true
}

/// Borrowed child-message regions from one canonical `If` protobuf body.
///
/// These are the three semantic children only. `locally_free` and
/// `connective_used` are derived summaries and are deliberately validated but
/// not returned as matching obligations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalIfParts<'a> {
    pub condition: &'a [u8],
    pub if_true: &'a [u8],
    pub if_false: &'a [u8],
}

/// Borrowed semantic fields from one canonical `New` protobuf body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalNewParts<'a> {
    /// Canonical protobuf zig-zag representation of `bind_count`.
    pub bind_count: u64,
    pub body: &'a [u8],
}

/// Borrowed semantic fields from one canonical `Send` protobuf body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalSendParts<'a> {
    pub channel: &'a [u8],
    pub data: Vec<&'a [u8]>,
    pub persistent: bool,
}

/// Borrowed semantic fields from one canonical `Receive` protobuf body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalReceiveParts<'a> {
    pub binds: Vec<&'a [u8]>,
    pub body: &'a [u8],
    pub persistent: bool,
}

/// Borrowed semantic fields from one canonical `Match` protobuf body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalMatchParts<'a> {
    pub target: &'a [u8],
    pub cases: Vec<&'a [u8]>,
}

/// Recursive semantic children of one canonical `Expr` protobuf body.
///
/// Scalar expressions and expression families whose matcher requires another
/// semantic authority are intentionally not represented. Ordered collections
/// expose their child messages, while unordered sets/maps expose borrowed
/// members for the matcher's existing Bag/PathMap relation. Returning `None`
/// for every other shape makes the complete generated decoder the explicit
/// fallback instead of silently changing its equations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalExprParts<'a> {
    Unary {
        tag: u32,
        child: &'a [u8],
    },
    Binary {
        tag: u32,
        left: &'a [u8],
        right: &'a [u8],
    },
    Sequence {
        tag: u32,
        children: Vec<&'a [u8]>,
        remainder: Option<&'a [u8]>,
    },
    Map {
        entries: Vec<(&'a [u8], &'a [u8])>,
        remainder: Option<&'a [u8]>,
    },
    Method {
        method_name: &'a [u8],
        children: Vec<&'a [u8]>,
    },
    Matches {
        target: &'a [u8],
        pattern: &'a [u8],
    },
}

impl CanonicalExprParts<'_> {
    pub const fn tag(&self) -> u32 {
        match self {
            Self::Unary { tag, .. } | Self::Binary { tag, .. } | Self::Sequence { tag, .. } => *tag,
            Self::Map { .. } => 23,
            Self::Method { .. } => 24,
            Self::Matches { .. } => 27,
        }
    }
}

#[inline]
fn take_canonical_delimited_field<'a>(
    message: &'a [u8],
    at: &mut usize,
    expected_key: u64,
) -> Option<&'a [u8]> {
    (read_canonical_varint(message, at)? == expected_key).then_some(())?;
    let len = usize::try_from(read_canonical_varint(message, at)?).ok()?;
    let end = at.checked_add(len)?;
    let body = message.get(*at..end)?;
    *at = end;
    Some(body)
}

/// Split a canonical `If` protobuf body into its three canonical `Par`
/// messages without materializing any AST node.
///
/// The accepted language is exactly the field order emitted by the canonical
/// protobuf encoder: three required child messages followed by optional
/// derived metadata. Any missing, duplicate, reordered, unknown, non-minimal,
/// or trailing field is rejected so callers can conservatively fall back to
/// the complete generated decoder.
pub fn canonical_if_parts(message: &[u8]) -> Option<CanonicalIfParts<'_>> {
    const CONDITION_KEY: u64 = (1 << 3) | 2;
    const IF_TRUE_KEY: u64 = (2 << 3) | 2;
    const IF_FALSE_KEY: u64 = (3 << 3) | 2;
    const LOCALLY_FREE_KEY: u64 = (4 << 3) | 2;
    const CONNECTIVE_USED_KEY: u64 = 5 << 3;

    let mut at = 0usize;
    let condition = take_canonical_delimited_field(message, &mut at, CONDITION_KEY)?;
    let if_true = take_canonical_delimited_field(message, &mut at, IF_TRUE_KEY)?;
    let if_false = take_canonical_delimited_field(message, &mut at, IF_FALSE_KEY)?;

    if at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        if key == LOCALLY_FREE_KEY {
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
            if at < message.len() && read_canonical_varint(message, &mut at)? != CONNECTIVE_USED_KEY
            {
                return None;
            }
            if at < message.len() && read_canonical_varint(message, &mut at)? != 1 {
                return None;
            }
        } else if key == CONNECTIVE_USED_KEY {
            if read_canonical_varint(message, &mut at)? != 1 {
                return None;
            }
        } else {
            return None;
        }
    }

    (at == message.len()).then_some(CanonicalIfParts {
        condition,
        if_true,
        if_false,
    })
}

/// Extract the sole `If` component body from a canonical singleton-`If`
/// `Par`. The root summaries precede field 12 in canonical schema order.
pub fn canonical_single_if_from_par(message: &[u8]) -> Option<&[u8]> {
    const LOCALLY_FREE_KEY: u64 = (9 << 3) | 2;
    const CONNECTIVE_USED_KEY: u64 = 10 << 3;
    const IF_KEY: u64 = (12 << 3) | 2;

    let mut at = 0usize;
    if at < message.len() && message[at] == LOCALLY_FREE_KEY as u8 {
        let _ = read_canonical_varint(message, &mut at)?;
        let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
        at = at.checked_add(len).filter(|end| *end <= message.len())?;
    }
    if at < message.len() && message[at] == CONNECTIVE_USED_KEY as u8 {
        (read_canonical_varint(message, &mut at)? == CONNECTIVE_USED_KEY).then_some(())?;
        (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
    }
    let body = take_canonical_delimited_field(message, &mut at, IF_KEY)?;
    (at == message.len()).then_some(body)
}

/// Split a canonical `New` protobuf body without materializing its recursively
/// nested process. URI and injection fields are validated and skipped because
/// the spatial equation, like the historical matcher, constrains only
/// `bind_count` and `p`.
pub fn canonical_new_parts(message: &[u8]) -> Option<CanonicalNewParts<'_>> {
    let mut at = 0usize;
    let mut bind_count = 0u64;
    let mut body = None;
    let mut last_field = 0u64;
    let mut saw_bind_count = false;
    let mut saw_locally_free = false;

    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 0) if !saw_bind_count => {
                bind_count = read_canonical_varint(message, &mut at)?;
                (bind_count != 0).then_some(())?;
                saw_bind_count = true;
            }
            (2, 2) if body.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                body = Some(message.get(at..end)?);
                at = end;
            }
            (3 | 4, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
            }
            (5, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            _ => return None,
        }
    }

    Some(CanonicalNewParts {
        bind_count,
        body: body?,
    })
}

/// Split a canonical `Send` protobuf body into its ordered child messages.
pub fn canonical_send_parts(message: &[u8]) -> Option<CanonicalSendParts<'_>> {
    let mut at = 0usize;
    let mut channel = None;
    let mut data = Vec::new();
    let mut persistent = false;
    let mut last_field = 0u64;
    let mut saw_locally_free = false;
    let mut saw_connective = false;

    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) if channel.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                channel = Some(message.get(at..end)?);
                at = end;
            }
            (2, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                data.push(message.get(at..end)?);
                at = end;
            }
            (3, 0) if !persistent => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                persistent = true;
            }
            (5, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            (6, 0) if !saw_connective => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            _ => return None,
        }
    }

    Some(CanonicalSendParts {
        channel: channel?,
        data,
        persistent,
    })
}

/// Split a canonical `Receive` protobuf body without decoding its recursive
/// body. Bind messages remain separately borrowed so callers can decode or
/// cursor-match each unordered relation element without ever decoding the
/// complete body suffix again.
pub fn canonical_receive_parts(message: &[u8]) -> Option<CanonicalReceiveParts<'_>> {
    let mut at = 0usize;
    let mut binds = Vec::new();
    let mut body = None;
    let mut persistent = false;
    let mut last_field = 0u64;
    let mut saw_peek = false;
    let mut saw_bind_count = false;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    let mut saw_condition = false;

    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                binds.push(message.get(at..end)?);
                at = end;
            }
            (2, 2) if body.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                body = Some(message.get(at..end)?);
                at = end;
            }
            (3, 0) if !persistent => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                persistent = true;
            }
            (4, 0) if !saw_peek => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_peek = true;
            }
            (5, 0) if !saw_bind_count => {
                (read_canonical_varint(message, &mut at)? != 0).then_some(())?;
                saw_bind_count = true;
            }
            (6, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            (7, 0) if !saw_connective => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            (8, 2) if !saw_condition => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_condition = true;
            }
            _ => return None,
        }
    }

    Some(CanonicalReceiveParts {
        binds,
        body: body?,
        persistent,
    })
}

/// Split a canonical `Match` protobuf body without decoding its recursive
/// target. Cases remain individually borrowed bounded messages.
pub fn canonical_match_parts(message: &[u8]) -> Option<CanonicalMatchParts<'_>> {
    let mut at = 0usize;
    let mut target = None;
    let mut cases = Vec::new();
    let mut last_field = 0u64;
    let mut saw_locally_free = false;
    let mut saw_connective = false;

    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) if target.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                target = Some(message.get(at..end)?);
                at = end;
            }
            (2, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                cases.push(message.get(at..end)?);
                at = end;
            }
            (4, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            (5, 0) if !saw_connective => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            _ => return None,
        }
    }

    Some(CanonicalMatchParts {
        target: target?,
        cases,
    })
}

fn canonical_binary_par_parts(message: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut at = 0usize;
    let left = take_canonical_delimited_field(message, &mut at, (1 << 3) | 2)?;
    let right = take_canonical_delimited_field(message, &mut at, (2 << 3) | 2)?;
    (at == message.len()).then_some((left, right))
}

fn canonical_par_sequence_parts(
    message: &[u8],
    allow_remainder: bool,
) -> Option<(Vec<&[u8]>, Option<&[u8]>)> {
    let mut at = 0usize;
    let mut children = Vec::new();
    let mut last_field = 0u64;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    let mut remainder = None;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                children.push(message.get(at..end)?);
                at = end;
            }
            (3, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            (4, 0) if !saw_connective => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            (5, 2) if allow_remainder && remainder.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                remainder = Some(message.get(at..end)?);
                at = end;
            }
            _ => return None,
        }
    }
    Some((children, remainder))
}

fn canonical_map_parts(message: &[u8]) -> Option<(Vec<(&[u8], &[u8])>, Option<&[u8]>)> {
    let (entries, remainder) = canonical_par_sequence_parts(message, true)?;
    Some((
        entries
            .into_iter()
            .map(canonical_binary_par_parts)
            .collect::<Option<Vec<_>>>()?,
        remainder,
    ))
}

fn canonical_method_parts(message: &[u8]) -> Option<CanonicalExprParts<'_>> {
    let mut at = 0usize;
    let mut method_name = &message[0..0];
    let mut target = None;
    let mut arguments = Vec::new();
    let mut last_field = 0u64;
    let mut saw_name = false;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (1, 2) if !saw_name => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                let end = at.checked_add(len)?;
                method_name = message.get(at..end)?;
                std::str::from_utf8(method_name).ok()?;
                at = end;
                saw_name = true;
            }
            (2, 2) if target.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                target = Some(message.get(at..end)?);
                at = end;
            }
            (3, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                arguments.push(message.get(at..end)?);
                at = end;
            }
            (5, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
                saw_locally_free = true;
            }
            (6, 0) if !saw_connective => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            _ => return None,
        }
    }
    let mut children = Vec::with_capacity(arguments.len() + 1);
    children.push(target?);
    children.extend(arguments);
    Some(CanonicalExprParts::Method {
        method_name,
        children,
    })
}

/// Parse only recursive expression families whose spatial equation can be
/// continued directly over borrowed canonical child messages.
pub fn canonical_expr_parts(message: &[u8]) -> Option<CanonicalExprParts<'_>> {
    let CanonicalExprEnvelope { tag, payload: body } = canonical_expr_envelope(message)?;

    match tag {
        5 | 6 => Some(CanonicalExprParts::Unary {
            tag,
            child: canonical_single_delimited_field(body, (1 << 3) | 2)?,
        }),
        7..=18 | 28..=31 => {
            let (left, right) = canonical_binary_par_parts(body)?;
            Some(CanonicalExprParts::Binary { tag, left, right })
        }
        20..=22 => {
            let (children, remainder) = canonical_par_sequence_parts(body, matches!(tag, 20 | 22))?;
            Some(CanonicalExprParts::Sequence {
                tag,
                children,
                remainder,
            })
        }
        23 => {
            let (entries, remainder) = canonical_map_parts(body)?;
            Some(CanonicalExprParts::Map { entries, remainder })
        }
        24 => canonical_method_parts(body),
        27 => {
            let (target, pattern) = canonical_binary_par_parts(body)?;
            Some(CanonicalExprParts::Matches { target, pattern })
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct CanonicalSingletonParComponent<'a> {
    body: &'a [u8],
    locally_free: &'a [u8],
    connective_used: bool,
}

fn canonical_single_par_component_parts(
    message: &[u8],
    expected_field: u64,
) -> Option<CanonicalSingletonParComponent<'_>> {
    const LOCALLY_FREE_FIELD: u64 = 9;
    const CONNECTIVE_USED_FIELD: u64 = 10;

    let mut at = 0usize;
    let mut body = None;
    let mut locally_free = None;
    let mut last_field = 0u64;
    let mut connective_used = false;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (field, 2) if field == expected_field && body.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                body = Some(message.get(at..end)?);
                at = end;
            }
            (LOCALLY_FREE_FIELD, 2) if locally_free.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                let end = at.checked_add(len)?;
                locally_free = Some(message.get(at..end)?);
                at = end;
            }
            (CONNECTIVE_USED_FIELD, 0) if !connective_used => {
                (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
                connective_used = true;
            }
            _ => return None,
        }
    }
    Some(CanonicalSingletonParComponent {
        body: body?,
        locally_free: locally_free.unwrap_or_default(),
        connective_used,
    })
}

fn canonical_single_par_component(message: &[u8], expected_field: u64) -> Option<&[u8]> {
    canonical_single_par_component_parts(message, expected_field).map(|parts| parts.body)
}

/// Extract the sole `New` component body from a canonical singleton carrier.
pub fn canonical_single_new_from_par(message: &[u8]) -> Option<&[u8]> {
    canonical_single_par_component(message, ParComponentTag::New.byte().into())
}

/// Extract the sole `Send` component body from a canonical singleton carrier.
pub fn canonical_single_send_from_par(message: &[u8]) -> Option<&[u8]> {
    canonical_single_par_component(message, ParComponentTag::Send.byte().into())
}

/// Extract the sole `Receive` component body from a canonical singleton carrier.
pub fn canonical_single_receive_from_par(message: &[u8]) -> Option<&[u8]> {
    canonical_single_par_component(message, ParComponentTag::Receive.byte().into())
}

/// Extract the sole `Match` component body from a canonical singleton carrier.
pub fn canonical_single_match_from_par(message: &[u8]) -> Option<&[u8]> {
    canonical_single_par_component(message, ParComponentTag::Match.byte().into())
}

/// Extract the sole `Expr` component body from a canonical singleton carrier.
pub fn canonical_single_expr_from_par(message: &[u8]) -> Option<&[u8]> {
    canonical_single_par_component(message, ParComponentTag::Expr.byte().into())
}

/// Prove that a canonical protobuf `Par` contains exactly one
/// `Expr(EVar(FreeVar(level)))` occurrence and no other component family.
///
/// `None` is lack of proof, never a mismatch: callers must retain their full
/// generated-decoder path for any byte shape this exact recognizer rejects.
pub fn canonical_exact_free_var_par(message: &[u8]) -> Option<CanonicalExactFreeVarPar> {
    let expr = canonical_single_expr_from_par(message)?;
    Some(CanonicalExactFreeVarPar {
        level: canonical_exact_free_var_expr_level(expr)?,
        connective_used: canonical_par_connective_used(message)?,
    })
}

/// A canonical singleton-Expr `Par` whose expression is a fully validated
/// ground scalar.
///
/// The borrowed Expr message is an injective equality image for the admitted
/// scalar variants. Root `locally_free` is intentionally absent from this
/// view because concrete spatial equality ignores that derived summary;
/// `connective_used` remains explicit because it changes matcher dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalExactScalarPar<'a> {
    expr: &'a [u8],
    connective_used: bool,
}

impl<'a> CanonicalExactScalarPar<'a> {
    pub const fn expr(&self) -> &'a [u8] { self.expr }

    pub const fn connective_used(&self) -> bool { self.connective_used }
}

/// Prove that a canonical protobuf `Par` has exactly one occurrence of one
/// fully validated scalar `Expr` and no other component family.
///
/// `None` is deliberately only lack of proof. Callers must delegate it to the
/// complete generated decoder and semantic matcher rather than interpreting
/// it as inequality.
pub fn canonical_exact_scalar_par(message: &[u8]) -> Option<CanonicalExactScalarPar<'_>> {
    let expr = canonical_single_expr_from_par(message)?;
    canonical_exact_scalar_expr(expr)?;
    Some(CanonicalExactScalarPar {
        expr,
        connective_used: canonical_par_connective_used(message)?,
    })
}

/// Prove that a canonical protobuf `Expr` is exactly one fully validated
/// ground scalar and return its injective equality image.
///
/// This is the component-level counterpart of
/// [`canonical_exact_scalar_par`]. `None` remains lack of proof rather than a
/// semantic mismatch, so byte-native callers can conservatively delegate to
/// the complete decoder.
pub fn canonical_exact_scalar_expr(message: &[u8]) -> Option<&[u8]> {
    canonical_scalar_expr_is_exact(message).then_some(message)
}

fn canonical_scalar_expr_is_exact(message: &[u8]) -> bool {
    let Some(CanonicalExprEnvelope { tag, payload }) = canonical_expr_envelope(message) else {
        return false;
    };
    match tag {
        // Bool is the only scalar varint whose decoded domain is narrower
        // than its protobuf carrier. GInt's canonical zig-zag image is every
        // u64, so the envelope's minimal-varint proof is complete for tag 2.
        1 => {
            let mut at = 0usize;
            matches!(read_canonical_varint(payload, &mut at), Some(value) if value <= 1)
                && at == payload.len()
        }
        2 => {
            let mut at = 0usize;
            read_canonical_varint(payload, &mut at).is_some() && at == payload.len()
        }
        3 | 4 => std::str::from_utf8(payload).is_ok(),
        25 | 35 => true,
        34 => payload.len() == 8,
        36 => canonical_big_rational_is_exact(payload),
        37 => canonical_fixed_point_is_exact(payload),
        _ => false,
    }
}

fn canonical_big_rational_is_exact(message: &[u8]) -> bool {
    let mut at = 0usize;
    let mut last_key = 0u64;
    while at < message.len() {
        let Some(key) = read_canonical_varint(message, &mut at) else {
            return false;
        };
        if !matches!(key, 10 | 18) || key <= last_key {
            return false;
        }
        let Some(len) =
            read_canonical_varint(message, &mut at).and_then(|len| usize::try_from(len).ok())
        else {
            return false;
        };
        // Ordinary proto3 bytes fields omit their empty default. The entire
        // message may still be empty when both fields have default values.
        if len == 0 {
            return false;
        }
        let Some(end) = at.checked_add(len).filter(|end| *end <= message.len()) else {
            return false;
        };
        at = end;
        last_key = key;
    }
    true
}

fn canonical_fixed_point_is_exact(message: &[u8]) -> bool {
    let mut at = 0usize;
    let mut last_key = 0u64;
    while at < message.len() {
        let Some(key) = read_canonical_varint(message, &mut at) else {
            return false;
        };
        if !matches!(key, 10 | 16) || key <= last_key {
            return false;
        }
        match key {
            10 => {
                let Some(len) = read_canonical_varint(message, &mut at)
                    .and_then(|len| usize::try_from(len).ok())
                else {
                    return false;
                };
                if len == 0 {
                    return false;
                }
                let Some(end) = at.checked_add(len).filter(|end| *end <= message.len()) else {
                    return false;
                };
                at = end;
            }
            16 => {
                let Some(scale) = read_canonical_varint(message, &mut at) else {
                    return false;
                };
                if scale == 0 || u32::try_from(scale).is_err() {
                    return false;
                }
            }
            _ => unreachable!("the fixed-point key was proved above"),
        }
        last_key = key;
    }
    true
}

/// Read `Par.connective_used` from a canonical protobuf message without
/// decoding any component body.
///
/// Every non-summary `Par` field is length-delimited; field 10 is the sole
/// varint. Resident ParBag messages have already passed full canonical
/// validation, but this cursor remains total so external callers can fall back
/// conservatively when handed another byte language.
pub fn canonical_par_connective_used(message: &[u8]) -> Option<bool> {
    const CONNECTIVE_USED_FIELD: u64 = 10;

    let mut at = 0usize;
    let mut connective_used = false;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        if field == CONNECTIVE_USED_FIELD {
            (wire == 0 && !connective_used).then_some(())?;
            (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
            connective_used = true;
        } else {
            (wire == 2).then_some(())?;
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
        }
    }
    Some(connective_used)
}

/// Test the root `Par.locally_free` summary without decoding any component.
/// Canonical nonempty bytes mean the summary is nonempty; omission means the
/// empty bitset. The complete field stream is validated while it is skipped.
pub fn canonical_par_locally_free_is_empty(message: &[u8]) -> Option<bool> {
    const LOCALLY_FREE_FIELD: u64 = 9;
    const CONNECTIVE_USED_FIELD: u64 = 10;

    let mut at = 0usize;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    let mut last_field = 0u64;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        if field == LOCALLY_FREE_FIELD {
            (wire == 2 && !saw_locally_free).then_some(())?;
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            (len != 0).then_some(())?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
            saw_locally_free = true;
        } else if field == CONNECTIVE_USED_FIELD {
            (wire == 0 && !saw_connective).then_some(())?;
            (read_canonical_varint(message, &mut at)? == 1).then_some(())?;
            saw_connective = true;
        } else {
            (wire == 2).then_some(())?;
            let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
            at = at.checked_add(len).filter(|end| *end <= message.len())?;
        }
    }
    Some(!saw_locally_free)
}

/// Borrow one singular length-delimited field from a canonical protobuf
/// message while skipping every other field by its wire extent.
///
/// This is a shallow cursor: nested messages are never decoded or scanned.
/// The resident ParBag key validator is the authority for the surrounding
/// message language; this helper additionally rejects reordered fields, a
/// duplicate sought field, unsupported wire types, truncation, and non-minimal
/// framing before returning a proof used by a derived cache. Equal adjacent
/// field numbers remain valid for schema-declared repeated fields.
fn canonical_singular_delimited_field(message: &[u8], sought_field: u64) -> Option<Option<&[u8]>> {
    let mut at = 0usize;
    let mut last_field = 0u64;
    let mut result = None;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field != 0 && field >= last_field).then_some(())?;
        last_field = field;
        let body = match wire {
            0 => {
                let _ = read_canonical_varint(message, &mut at)?;
                None
            }
            1 => {
                at = at.checked_add(8).filter(|end| *end <= message.len())?;
                None
            }
            2 => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                let body = message.get(at..end)?;
                at = end;
                Some(body)
            }
            5 => {
                at = at.checked_add(4).filter(|end| *end <= message.len())?;
                None
            }
            _ => return None,
        };
        if field == sought_field {
            (wire == 2 && result.is_none()).then_some(())?;
            result = body;
        }
    }
    Some(result)
}

fn canonical_expr_locally_free_is_empty(message: &[u8]) -> Option<bool> {
    if message.is_empty() {
        return Some(true);
    }
    let envelope = canonical_expr_envelope(message)?;
    let par_is_closed = |par: &[u8]| canonical_par_locally_free_is_empty(par) == Some(true);
    match envelope.tag {
        // Ground scalar alternatives carry no locally-free summary.
        1..=4 | 25 | 34..=37 => canonical_exact_scalar_expr(message).map(|_| true),
        5 | 6 => {
            let child = canonical_single_delimited_field(envelope.payload, (1 << 3) | 2)?;
            Some(par_is_closed(child))
        }
        7..=18 | 28..=31 => {
            let (left, right) = canonical_binary_par_parts(envelope.payload)?;
            Some(par_is_closed(left) && par_is_closed(right))
        }
        19 => {
            const EVAR_VAR_KEY: u64 = (1 << 3) | 2;
            let variable = canonical_single_delimited_field(envelope.payload, EVAR_VAR_KEY)?;
            match canonical_var(variable)? {
                CanonicalVar::Bound(_) => Some(false),
                CanonicalVar::Free(_) | CanonicalVar::Wildcard => Some(true),
                CanonicalVar::Absent => None,
            }
        }
        // Collection and method bodies carry the exact cached fold directly.
        20..=23 | 32 => canonical_singular_delimited_field(envelope.payload, 3)
            .map(|field| field.is_none_or(<[u8]>::is_empty)),
        24 => canonical_singular_delimited_field(envelope.payload, 5)
            .map(|field| field.is_none_or(<[u8]>::is_empty)),
        27 => {
            let (target, pattern) = canonical_binary_par_parts(envelope.payload)?;
            Some(par_is_closed(target) && par_is_closed(pattern))
        }
        33 => canonical_singular_delimited_field(envelope.payload, 4)
            .map(|field| field.is_none_or(<[u8]>::is_empty)),
        _ => None,
    }
}

fn canonical_component_locally_free_is_empty(tag: ParComponentTag, message: &[u8]) -> Option<bool> {
    match tag {
        ParComponentTag::Send | ParComponentTag::New => {
            canonical_singular_delimited_field(message, 5)
                .map(|field| field.is_none_or(<[u8]>::is_empty))
        }
        ParComponentTag::Receive => canonical_singular_delimited_field(message, 6)
            .map(|field| field.is_none_or(<[u8]>::is_empty)),
        ParComponentTag::Expr => canonical_expr_locally_free_is_empty(message),
        ParComponentTag::Match | ParComponentTag::If => {
            canonical_singular_delimited_field(message, 4)
                .map(|field| field.is_none_or(<[u8]>::is_empty))
        }
        ParComponentTag::Bundle => {
            let body = canonical_singular_delimited_field(message, 1)??;
            canonical_par_locally_free_is_empty(body)
        }
        ParComponentTag::GUnforgeable | ParComponentTag::Connective => Some(true),
    }
}

/// Interpret the only two collection-remainder forms accepted by the spatial
/// matcher without constructing a generated `Var` value.
///
/// The first component is a free-variable level; the second marks a wildcard.
/// Bound, absent, and malformed variables return `None`, which makes the
/// matcher use its complete generated decoder rather than widening the wire
/// language accepted by this fast path.
pub fn canonical_collection_remainder(message: &[u8]) -> Option<(Option<i32>, bool)> {
    match canonical_var(message)? {
        CanonicalVar::Free(level) => Some((Some(level), false)),
        CanonicalVar::Wildcard => Some((None, true)),
        CanonicalVar::Absent | CanonicalVar::Bound(_) => None,
    }
}

/// Follow an exact non-empty chain of singleton AND/OR/NOT connectives and
/// decode only its terminal child. AND and OR are unary identities; NOT
/// contributes one parity bit. This is an algebraically exact normalization,
/// including the binding-isolation semantics applied by the matcher.
fn decode_unary_connective_spine(mut connective: &[u8]) -> Option<(Par, bool, bool)> {
    const AND_KEY: u64 = (1 << 3) | 2;
    const OR_KEY: u64 = (2 << 3) | 2;
    const NOT_KEY: u64 = (3 << 3) | 2;
    const CHILD_KEY: u64 = (1 << 3) | 2;

    let mut negated = false;
    let mut has_not = false;
    loop {
        let child = if let Some(body) = canonical_single_delimited_field(connective, AND_KEY)
            .or_else(|| canonical_single_delimited_field(connective, OR_KEY))
        {
            canonical_single_delimited_field(body, CHILD_KEY)?
        } else {
            negated = !negated;
            has_not = true;
            canonical_single_delimited_field(connective, NOT_KEY)?
        };
        if canonical_par_connective_used(child) == Some(true) {
            connective = canonical_single_connective_from_par(child)?;
        } else {
            return crate::rust::rholang::protobuf_decoder::decode_par(child)
                .ok()
                .map(|terminal| (terminal, negated, has_not));
        }
    }
}

fn decode_unary_connective_terminal(connective: &[u8]) -> Option<Par> {
    decode_unary_connective_spine(connective)
        .filter(|(_terminal, negated, _has_not)| !negated)
        .map(|(terminal, _negated, _has_not)| terminal)
}

fn canonical_single_bundle_from_par(message: &[u8]) -> Option<&[u8]> {
    let mut at = 0usize;
    let mut bundle = None;
    while at < message.len() {
        let key = read_canonical_varint(message, &mut at)?;
        match (key >> 3, key & 0x07) {
            (9, 2) => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                at = at.checked_add(len).filter(|end| *end <= message.len())?;
            }
            (10, 0) => {
                let _ = read_canonical_varint(message, &mut at)?;
            }
            (11, 2) if bundle.is_none() => {
                let len = usize::try_from(read_canonical_varint(message, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                bundle = Some(message.get(at..end)?);
                at = end;
            }
            _ => return None,
        }
    }
    bundle
}

fn canonical_bundle_body(bundle: &[u8]) -> Option<&[u8]> {
    const BODY_KEY: u64 = (1 << 3) | 2;
    let mut at = 0usize;
    (read_canonical_varint(bundle, &mut at)? == BODY_KEY).then_some(())?;
    let len = usize::try_from(read_canonical_varint(bundle, &mut at)?).ok()?;
    let end = at.checked_add(len)?;
    let body = bundle.get(at..end)?;
    at = end;
    while at < bundle.len() {
        let key = read_canonical_varint(bundle, &mut at)?;
        if !matches!((key >> 3, key & 0x07), (2 | 3, 0)) {
            return None;
        }
        let _ = read_canonical_varint(bundle, &mut at)?;
    }
    Some(body)
}

/// Follow a non-empty exact unary Bundle spine and decode only its terminal
/// body. Bundle capability flags do not change process evaluation; callers
/// that expose per-bundle observation must retain the ordinary semantic path.
fn decode_unary_bundle_terminal(mut bundle: &[u8]) -> Option<Par> {
    loop {
        let body = canonical_bundle_body(bundle)?;
        match canonical_single_bundle_from_par(body) {
            Some(next) => bundle = next,
            None => return crate::rust::rholang::protobuf_decoder::decode_par(body).ok(),
        }
    }
}

#[inline]
fn score_segment_is(actual: &[u8], at: &mut usize, expected: &[u8]) -> bool {
    let Some(end) = at.checked_add(expected.len()) else {
        return false;
    };
    if actual.get(*at..end) != Some(expected) {
        return false;
    }
    *at = end;
    true
}

#[inline]
fn score_int_is(actual: &[u8], at: &mut usize, value: i64) -> bool {
    let mut expected = [0u8; 10];
    expected[..2].copy_from_slice(&[1, 1]);
    expected[2..].copy_from_slice(&((value as u64) ^ (1u64 << 63)).to_be_bytes());
    score_segment_is(actual, at, &expected)
}

#[inline]
fn score_node_start_is(actual: &[u8], at: &mut usize, constructor: i32) -> bool {
    score_segment_is(actual, at, &[2]) && score_int_is(actual, at, i64::from(constructor))
}

/// Recognize and validate the canonical unary `EList` spine without
/// materializing its recursively nested `Par`s.
///
/// The resident result of importing a ParBag is the already-parsed outer
/// PathMap; constructing a temporary PathMap for every protobuf `Par` inside
/// one component is pure validation work. For a depth-`d` spine that copied
/// `1 + ... + d` score/body bytes and repeatedly hashed/dropped long PathMap
/// lines. This recognizer instead:
///
/// 1. follows exact canonical `Expr(EList(Par(Expr(...))))` framing by slice;
/// 2. proves the terminal is one canonical `GInt` oneof;
/// 3. compares the Rholang score key directly as repeated prefix/suffix
///    segments.
///
/// Every step consumes input, native stack is constant, heap allocation is
/// zero, and a non-matching shape falls through to the complete generated
/// validator. `Some(false)` means the protobuf is this exact canonical spine
/// but its claimed score key is noncanonical.
#[derive(Clone, Copy)]
enum CanonicalUnaryOrderedKind {
    EList,
    ETuple,
}

impl CanonicalUnaryOrderedKind {
    const fn expr_key(self) -> u64 {
        let tag = match self {
            Self::EList => 20,
            Self::ETuple => 21,
        };
        (tag << 3) | 2
    }

    const fn expr_tag(self) -> u32 {
        match self {
            Self::EList => 20,
            Self::ETuple => 21,
        }
    }

    const fn score(self) -> i32 {
        match self {
            Self::EList => Score::ELIST,
            Self::ETuple => Score::ETUPLE,
        }
    }
}

fn canonical_unary_ordered_expr(
    payload: &[u8],
    kind: CanonicalUnaryOrderedKind,
) -> Option<(usize, i64)> {
    // EList.ps and ETuple.ps are both field 1, wire type 2; Par.exprs is
    // field 5, wire type 2. The exact single-field parser also proves that
    // collection metadata and the EList remainder are absent.
    const ELIST_PAR_KEY: u64 = (1 << 3) | 2;
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;
    const EXPR_GINT_KEY: u64 = 2 << 3;

    let mut current = payload;
    let mut depth = 0usize;
    loop {
        if let Some(zigzag) = canonical_single_varint_field(current, EXPR_GINT_KEY) {
            let value = ((zigzag >> 1) as i64) ^ -((zigzag & 1) as i64);
            return Some((depth, value));
        }
        let list = canonical_single_delimited_field(current, kind.expr_key())?;
        let par = canonical_single_delimited_field(list, ELIST_PAR_KEY)?;
        current = canonical_single_delimited_field(par, PAR_EXPR_KEY)?;
        depth = depth.checked_add(1)?;
    }
}

fn canonical_unary_elist(payload: &[u8]) -> Option<(usize, i64)> {
    canonical_unary_ordered_expr(payload, CanonicalUnaryOrderedKind::EList)
}

fn canonical_unary_etuple(payload: &[u8]) -> Option<(usize, i64)> {
    canonical_unary_ordered_expr(payload, CanonicalUnaryOrderedKind::ETuple)
}

fn canonical_enot_child(payload: &[u8]) -> Option<&[u8]> {
    let CanonicalExprParts::Unary { tag: 5, child } = canonical_expr_parts(payload)? else {
        return None;
    };
    Some(child)
}

fn canonical_unary_enot_parts(payload: &[u8]) -> Option<(usize, &[u8])> {
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;

    let mut terminal = canonical_enot_child(payload)?;
    let mut depth = 1usize;
    loop {
        let Some(next_expr) = canonical_single_delimited_field(terminal, PAR_EXPR_KEY) else {
            return Some((depth, terminal));
        };
        let Some(next_child) = canonical_enot_child(next_expr) else {
            return Some((depth, terminal));
        };
        terminal = next_child;
        depth = depth.checked_add(1)?;
    }
}

/// Recognize an exact canonical `ENot(Par(ENot(...)))` spine ending in the
/// empty `Par`, directly in the protobuf body of its outer `Expr`.
///
/// `ENot` cannot be used as a canonical PathMap path, so this recognizer does
/// not claim stability. Its purpose is narrower: the resident `ParBag` stores
/// the complete outer Expr as one endpoint, and need not construct, hash, and
/// dismantle every recursively nested suffix merely to reach that state.
fn canonical_unary_enot(payload: &[u8]) -> Option<usize> {
    let (depth, terminal) = canonical_unary_enot_parts(payload)?;
    terminal.is_empty().then_some(depth)
}

/// Adopt one already-proved canonical unary `EList` expression as the sole
/// endpoint of a `ParBag`.
///
/// The score and protobuf body are written directly into the final trie key.
/// This avoids the quadratic sequence of constructing, hashing, and dropping
/// every nested suffix trie while preserving the exact `RPB\x03` key language.
fn import_canonical_unary_ordered_expr(
    expr_body: &[u8],
    kind: CanonicalUnaryOrderedKind,
    depth: usize,
    terminal: i64,
) -> Option<Par> {
    let score_capacity = 32usize.checked_add(depth.checked_mul(44)?)?;
    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(score_capacity)?
            .checked_add(expr_body.len())?
            .checked_add(PAR_BAG_SCORE_LEN_BYTES)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();
    for _ in 0..depth {
        begin_score_node(&mut key, kind.score());
        if matches!(kind, CanonicalUnaryOrderedKind::EList) {
            append_int_score_atom(&mut key, -1);
        }
        begin_score_node(&mut key, Score::PAR);
    }
    begin_score_node(&mut key, Score::INT);
    append_int_score_atom(&mut key, terminal);
    end_score_node(&mut key);
    for _ in 0..depth {
        append_int_score_atom(&mut key, 0);
        end_score_node(&mut key);
        append_int_score_atom(&mut key, 0);
        end_score_node(&mut key);
    }
    let score_len = u64::try_from(key.len().checked_sub(score_start)?)
        .expect("one allocated canonical score length fits in u64");
    key.extend_from_slice(expr_body);
    key.extend_from_slice(&score_len.to_be_bytes());

    let mut components = ParBag::new();
    components
        .insert_bytes(ParComponentTag::Expr, &key, Multiplicity::ONE, Some(true))
        .expect("one validated singleton component cannot overflow multiplicity");
    Some(Par::from_parts(components, Vec::new(), false))
}

fn import_canonical_unary_elist_expr(expr_body: &[u8], depth: usize, terminal: i64) -> Option<Par> {
    import_canonical_unary_ordered_expr(
        expr_body,
        CanonicalUnaryOrderedKind::EList,
        depth,
        terminal,
    )
}

fn import_canonical_unary_etuple_expr(
    expr_body: &[u8],
    depth: usize,
    terminal: i64,
) -> Option<Par> {
    import_canonical_unary_ordered_expr(
        expr_body,
        CanonicalUnaryOrderedKind::ETuple,
        depth,
        terminal,
    )
}

/// Wrap an arbitrary canonical terminal `Par` in `depth` unary EList or
/// ETuple expressions and construct the final authoritative ParBag endpoint in
/// one linear pass.
///
/// Repeatedly calling `Par::with_exprs` is a persistent update at every level:
/// each update must encode the complete previous protobuf suffix, so building
/// a depth-`d` spine that way copies `1 + ... + d` bytes. This routine plans
/// every protobuf length once, emits the final outer expression once, and
/// writes its canonical score directly into the one final PathMap key.
fn wrap_canonical_unary_ordered_par(
    kind: CanonicalUnaryOrderedKind,
    depth: usize,
    terminal: Par,
) -> Option<Par> {
    if depth == 0 {
        return Some(terminal);
    }

    const COLLECTION_PAR_TAG: u32 = 1;
    const PAR_EXPR_TAG: u32 = 5;

    let terminal_stable = crate::rust::pathmap_crate_type_mapper::eval_stable_par(&terminal);
    let terminal_body = terminal.protobuf_bytes().ok()?;

    // Frames are recorded inner-to-outer. `child_par_len` is either the
    // terminal's complete Par body or the field-5 Par wrapper around the next
    // inner collection expression.
    let mut child_par_len = terminal_body.len();
    let mut frames = Vec::with_capacity(depth);
    for _ in 0..depth {
        let list_len = key_len(COLLECTION_PAR_TAG)
            .checked_add(encoded_len_varint(child_par_len as u64))?
            .checked_add(child_par_len)?;
        let expr_len = key_len(kind.expr_tag())
            .checked_add(encoded_len_varint(list_len as u64))?
            .checked_add(list_len)?;
        frames.push((expr_len, list_len, child_par_len));
        child_par_len = key_len(PAR_EXPR_TAG)
            .checked_add(encoded_len_varint(expr_len as u64))?
            .checked_add(expr_len)?;
    }

    let outer_expr_len = frames.last()?.0;
    let mut expr_body = Vec::with_capacity(outer_expr_len);
    for index in (0..frames.len()).rev() {
        let (expr_len, list_len, planned_child_par_len) = frames[index];
        let _ = expr_len;
        encode_key(kind.expr_tag(), WireType::LengthDelimited, &mut expr_body);
        prost::encoding::encode_varint(list_len as u64, &mut expr_body);
        encode_key(
            COLLECTION_PAR_TAG,
            WireType::LengthDelimited,
            &mut expr_body,
        );
        prost::encoding::encode_varint(planned_child_par_len as u64, &mut expr_body);
        if index != 0 {
            let child_expr_len = frames[index - 1].0;
            encode_key(PAR_EXPR_TAG, WireType::LengthDelimited, &mut expr_body);
            prost::encoding::encode_varint(child_expr_len as u64, &mut expr_body);
        }
    }
    expr_body.extend_from_slice(&terminal_body);
    debug_assert_eq!(expr_body.len(), outer_expr_len);

    let score_capacity = terminal
        .components()
        .score_components_snapshot()
        .len()
        .checked_add(depth.checked_mul(44)?)?
        .checked_add(64)?;
    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(score_capacity)?
            .checked_add(expr_body.len())?
            .checked_add(PAR_BAG_SCORE_LEN_BYTES)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();
    for level in 0..depth {
        begin_score_node(&mut key, kind.score());
        if matches!(kind, CanonicalUnaryOrderedKind::EList) {
            append_int_score_atom(&mut key, -1);
        }
        if level + 1 < depth {
            begin_score_node(&mut key, Score::PAR);
        }
    }
    terminal
        .components()
        .append_par_score_key(terminal.connective_used(), &mut key);
    for level in (0..depth).rev() {
        append_int_score_atom(&mut key, 0);
        end_score_node(&mut key);
        if level != 0 {
            append_int_score_atom(&mut key, 0);
            end_score_node(&mut key);
        }
    }
    let score_len = u64::try_from(key.len().checked_sub(score_start)?)
        .expect("one allocated canonical score length fits in u64");
    key.extend_from_slice(&expr_body);
    key.extend_from_slice(&score_len.to_be_bytes());

    let mut components = ParBag::new();
    components
        .insert_bytes(
            ParComponentTag::Expr,
            &key,
            Multiplicity::ONE,
            Some(terminal_stable),
        )
        .expect("one validated singleton component cannot overflow multiplicity");
    Some(Par::from_parts(components, Vec::new(), false))
}

pub(crate) fn wrap_canonical_unary_elist_par(depth: usize, terminal: Par) -> Option<Par> {
    wrap_canonical_unary_ordered_par(CanonicalUnaryOrderedKind::EList, depth, terminal)
}

pub(crate) fn wrap_canonical_unary_etuple_par(depth: usize, terminal: Par) -> Option<Par> {
    wrap_canonical_unary_ordered_par(CanonicalUnaryOrderedKind::ETuple, depth, terminal)
}

/// Write the complete authoritative ParBag key for one already-proved
/// canonical unary `ENot` expression, without constructing its suffix Pars.
fn canonical_unary_enot_key(expr_body: &[u8], depth: usize) -> Option<Vec<u8>> {
    (depth != 0).then_some(())?;
    let score_capacity = 24usize.checked_add(depth.checked_mul(34)?)?;
    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(score_capacity)?
            .checked_add(expr_body.len())?
            .checked_add(PAR_BAG_SCORE_LEN_BYTES)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();

    // Each ENot score contains its child Par score. The deepest child is the
    // empty Par; every outer child Par additionally contains the next ENot
    // occurrence before its connective-used flag.
    for _ in 0..depth {
        begin_score_node(&mut key, Score::ENOT);
        begin_score_node(&mut key, Score::PAR);
    }
    append_int_score_atom(&mut key, 0);
    end_score_node(&mut key);
    for level in 0..depth {
        end_score_node(&mut key);
        if level + 1 < depth {
            append_int_score_atom(&mut key, 0);
            end_score_node(&mut key);
        }
    }

    let score_len = u64::try_from(key.len().checked_sub(score_start)?)
        .expect("one allocated canonical score length fits in u64");
    key.extend_from_slice(expr_body);
    key.extend_from_slice(&score_len.to_be_bytes());
    Some(key)
}

/// Adopt one already-proved canonical unary `ENot` expression as the sole
/// endpoint of a `ParBag`, without constructing its nested suffix Pars.
fn import_canonical_unary_enot_expr(expr_body: &[u8], depth: usize) -> Option<Par> {
    let key = canonical_unary_enot_key(expr_body, depth)?;
    let mut components = ParBag::new();
    components
        .insert_bytes(ParComponentTag::Expr, &key, Multiplicity::ONE, Some(false))
        .expect("one validated singleton component cannot overflow multiplicity");
    Some(Par::from_parts(components, Vec::new(), false))
}

/// Construct the resident canonical unary-list `Par` directly from its two
/// scalar parameters.  Canonical-path decoding uses this after its byte cursor
/// has proved the structural path image.
pub(crate) fn import_canonical_unary_elist_par(depth: usize, terminal: i64) -> Option<Par> {
    wrap_canonical_unary_elist_par(
        depth,
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(terminal)),
        }]),
    )
}

pub(crate) fn import_canonical_unary_etuple_par(depth: usize, terminal: i64) -> Option<Par> {
    wrap_canonical_unary_etuple_par(
        depth,
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(terminal)),
        }]),
    )
}

fn validate_unary_elist_key(score: &[u8], payload: &[u8]) -> Option<bool> {
    validate_unary_ordered_key(
        score,
        payload,
        CanonicalUnaryOrderedKind::EList,
        canonical_unary_elist,
    )
}

fn validate_unary_etuple_key(score: &[u8], payload: &[u8]) -> Option<bool> {
    validate_unary_ordered_key(
        score,
        payload,
        CanonicalUnaryOrderedKind::ETuple,
        canonical_unary_etuple,
    )
}

fn validate_unary_enot_key(score: &[u8], payload: &[u8]) -> Option<bool> {
    let depth = canonical_unary_enot(payload)?;
    let mut at = 0usize;
    for _ in 0..depth {
        if !score_node_start_is(score, &mut at, Score::ENOT)
            || !score_node_start_is(score, &mut at, Score::PAR)
        {
            return Some(false);
        }
    }
    if !score_int_is(score, &mut at, 0) || !score_segment_is(score, &mut at, &[0]) {
        return Some(false);
    }
    for level in 0..depth {
        if !score_segment_is(score, &mut at, &[0]) {
            return Some(false);
        }
        if level + 1 < depth
            && (!score_int_is(score, &mut at, 0) || !score_segment_is(score, &mut at, &[0]))
        {
            return Some(false);
        }
    }
    Some(at == score.len())
}

fn validate_unary_ordered_key(
    score: &[u8],
    payload: &[u8],
    kind: CanonicalUnaryOrderedKind,
    recognize: fn(&[u8]) -> Option<(usize, i64)>,
) -> Option<bool> {
    let (depth, terminal) = recognize(payload)?;

    let mut at = 0usize;
    for _ in 0..depth {
        if !score_node_start_is(score, &mut at, kind.score()) {
            return Some(false);
        }
        if matches!(kind, CanonicalUnaryOrderedKind::EList) && !score_int_is(score, &mut at, -1) {
            return Some(false);
        }
        if !score_node_start_is(score, &mut at, Score::PAR) {
            return Some(false);
        }
    }
    if !score_node_start_is(score, &mut at, Score::INT)
        || !score_int_is(score, &mut at, terminal)
        || !score_segment_is(score, &mut at, &[0])
    {
        return Some(false);
    }
    for _ in 0..depth {
        if !score_int_is(score, &mut at, 0)
            || !score_segment_is(score, &mut at, &[0])
            || !score_int_is(score, &mut at, 0)
            || !score_segment_is(score, &mut at, &[0])
        {
            return Some(false);
        }
    }
    Some(at == score.len())
}

/// Import the canonical protobuf image of a singleton `ESet` containing an
/// exact unary `EList` spine as one authoritative ParBag endpoint.
///
/// This is a schema-directed zero-copy validator followed by one linear key
/// construction.  It exists at the protobuf/ParBag boundary because decoding
/// every nested `Par` first would build and hash all `1 + ... + depth` suffix
/// tries, even though the resident representation retains only the outer
/// canonical Expr bytes.  Any extra field, metadata bit, non-minimal varint,
/// non-singleton set, or different expression shape returns `None` and is
/// handled by the complete generated decoder.
pub(crate) fn import_canonical_singleton_eset_unary_elist_par(protobuf: &[u8]) -> Option<Par> {
    // Par.exprs = 5, Expr.e_set_body = 22, ESet.ps = 1.
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;
    const EXPR_ESET_KEY: u64 = (22 << 3) | 2;
    const ESET_PAR_KEY: u64 = (1 << 3) | 2;

    let expr_body = canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)?;
    let set_body = canonical_single_delimited_field(expr_body, EXPR_ESET_KEY)?;
    let element_par = canonical_single_delimited_field(set_body, ESET_PAR_KEY)?;
    let element_expr = canonical_single_delimited_field(element_par, PAR_EXPR_KEY)?;
    let (depth, terminal) = canonical_unary_elist(element_expr)?;

    let score_capacity = 64usize.checked_add(depth.checked_mul(44)?)?;
    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(score_capacity)?
            .checked_add(expr_body.len())?
            .checked_add(PAR_BAG_SCORE_LEN_BYTES)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();

    // ESet(remainder = absent, [element], connective_used = false).
    begin_score_node(&mut key, Score::ESET);
    append_int_score_atom(&mut key, -1);

    // The set element is one metadata-free Par whose sole Expr is the unary
    // EList spine.  Each EList similarly contains one metadata-free child Par.
    begin_score_node(&mut key, Score::PAR);
    for _ in 0..depth {
        begin_score_node(&mut key, Score::ELIST);
        append_int_score_atom(&mut key, -1);
        begin_score_node(&mut key, Score::PAR);
    }
    begin_score_node(&mut key, Score::INT);
    append_int_score_atom(&mut key, terminal);
    end_score_node(&mut key);
    for _ in 0..depth {
        append_int_score_atom(&mut key, 0);
        end_score_node(&mut key);
        append_int_score_atom(&mut key, 0);
        end_score_node(&mut key);
    }
    append_int_score_atom(&mut key, 0);
    end_score_node(&mut key);
    append_int_score_atom(&mut key, 0);
    end_score_node(&mut key);

    let score_len = u64::try_from(key.len().checked_sub(score_start)?)
        .expect("one allocated canonical score length fits in u64");
    key.extend_from_slice(expr_body);
    key.extend_from_slice(&score_len.to_be_bytes());

    let mut components = ParBag::new();
    components
        .insert_bytes(ParComponentTag::Expr, &key, Multiplicity::ONE, Some(false))
        .expect("one validated singleton component cannot overflow multiplicity");
    Some(Par::from_parts(components, Vec::new(), false))
}

/// Prove the exact direct-import shape without constructing its ParBag key.
/// Canonical-path validation uses this to avoid materializing a semantic term
/// that it would immediately dismantle merely to discover that this schema
/// shape cannot contain an EPathMap.
pub(crate) fn is_canonical_singleton_eset_unary_elist_par(protobuf: &[u8]) -> bool {
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;
    const EXPR_ESET_KEY: u64 = (22 << 3) | 2;
    const ESET_PAR_KEY: u64 = (1 << 3) | 2;

    canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)
        .and_then(|expr| canonical_single_delimited_field(expr, EXPR_ESET_KEY))
        .and_then(|set| canonical_single_delimited_field(set, ESET_PAR_KEY))
        .and_then(|par| canonical_single_delimited_field(par, PAR_EXPR_KEY))
        .and_then(canonical_unary_elist)
        .is_some()
}

#[derive(Clone, Copy)]
struct CanonicalSingletonEPathMapParts<'a> {
    expr_body: &'a [u8],
    snapshot: &'a [u8],
    epathmap_connective_used: bool,
    epathmap_locally_free_is_empty: bool,
    epathmap_has_remainder: bool,
    locally_free: &'a [u8],
    connective_used: bool,
}

fn canonical_singleton_epathmap_parts(
    protobuf: &[u8],
) -> Option<CanonicalSingletonEPathMapParts<'_>> {
    // The carrier cursor admits canonical root metadata while still requiring
    // exactly one Expr occurrence. The Expr/EPathMap cursor below likewise
    // admits canonical EPathMap metadata before trie_snapshot = 9.
    let carrier =
        canonical_single_par_component_parts(protobuf, ParComponentTag::Expr.byte().into())?;
    let epathmap = canonical_epathmap_parts_from_expr(carrier.body)?;
    Some(CanonicalSingletonEPathMapParts {
        expr_body: carrier.body,
        snapshot: epathmap.snapshot,
        epathmap_connective_used: epathmap.connective_used,
        epathmap_locally_free_is_empty: epathmap.locally_free_is_empty,
        epathmap_has_remainder: epathmap.remainder.is_some(),
        locally_free: carrier.locally_free,
        connective_used: carrier.connective_used,
    })
}

#[derive(Clone, Copy)]
struct CanonicalEPathMapEnvelope<'a> {
    snapshot: &'a [u8],
    connective_used: bool,
    locally_free_is_empty: bool,
    remainder: Option<&'a [u8]>,
}

/// Width-independent routing information for one canonical EPathMap Expr.
///
/// The outer protobuf envelope is checked exactly, but the EPM1 result is only
/// a route hint: its ACT topology and value table have not been validated.
/// Consumers selecting a byte-region specialization must still invoke the
/// complete canonical snapshot parser before trusting any key or value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalEPathMapRootRouteHint {
    mode: crate::rust::epathmap_trie_codec::EPathMapMode,
    map_value_count: usize,
    has_remainder: bool,
}

impl CanonicalEPathMapRootRouteHint {
    #[inline]
    pub const fn mode(self) -> crate::rust::epathmap_trie_codec::EPathMapMode { self.mode }

    #[inline]
    pub const fn map_value_count(self) -> usize { self.map_value_count }

    #[inline]
    pub const fn has_remainder(self) -> bool { self.has_remainder }
}

/// Borrowed canonical envelope for one `Expr(EPathMap)` endpoint.
///
/// The protobuf body and EPM1 snapshot remain slices of the caller's existing
/// authoritative bytes.  The route hint is intentionally shallow: consumers
/// must still pass [`Self::protobuf_body`] to the complete generated decoder,
/// or pass [`Self::snapshot`] to the complete canonical snapshot inspector,
/// before trusting trie topology, keys, or map values.
#[derive(Clone, Copy, Debug)]
pub struct CanonicalEPathMapRootView<'a> {
    protobuf_body: &'a [u8],
    snapshot: &'a [u8],
    route_hint: CanonicalEPathMapRootRouteHint,
}

impl<'a> CanonicalEPathMapRootView<'a> {
    /// The complete protobuf body of the `EPathMap`, excluding its enclosing
    /// `Expr` oneof key and length.
    #[inline]
    pub const fn protobuf_body(self) -> &'a [u8] { self.protobuf_body }

    /// The borrowed EPM1 snapshot stored in protobuf field 9.
    #[inline]
    pub const fn snapshot(self) -> &'a [u8] { self.snapshot }

    /// Width-independent dispatch information from the canonical envelope and
    /// fixed EPM1 prefix. This is not a validation result.
    #[inline]
    pub const fn route_hint(self) -> CanonicalEPathMapRootRouteHint { self.route_hint }
}

/// Borrow one canonical `Expr(EPathMap)` envelope while inspecting only its
/// outer protobuf framing and fixed EPM1 prefix.
///
/// The compact ACT arena is skipped by length. The bounded `Var` remainder,
/// when present, is checked against its exact canonical protobuf grammar. No
/// trie node, canonical key, map value, `Par`, or proportional collection is
/// traversed or materialized.
pub fn canonical_epathmap_root_view(expr: &[u8]) -> Option<CanonicalEPathMapRootView<'_>> {
    let (protobuf_body, envelope) = canonical_epathmap_body_and_parts(expr)?;
    let snapshot = crate::rust::epathmap_trie_codec::snapshot_route_hint(envelope.snapshot).ok()?;
    Some(CanonicalEPathMapRootView {
        protobuf_body,
        snapshot: envelope.snapshot,
        route_hint: CanonicalEPathMapRootRouteHint {
            mode: snapshot.mode(),
            map_value_count: snapshot.map_value_count(),
            has_remainder: envelope.remainder.is_some(),
        },
    })
}

/// Inspect only the canonical Expr/EPathMap envelope and fixed EPM1 prefix.
///
/// The compact ACT arena is skipped by length. The bounded `Var` remainder,
/// when present, is checked against its exact canonical protobuf grammar. No
/// trie node, canonical key, map value, `Par`, or proportional collection is
/// traversed or materialized.
pub fn canonical_epathmap_root_route_hint(expr: &[u8]) -> Option<CanonicalEPathMapRootRouteHint> {
    canonical_epathmap_root_view(expr).map(CanonicalEPathMapRootView::route_hint)
}

/// Borrow field 9 from one canonical EPathMap Expr without decoding its trie or
/// recursively nested map values. Scalar metadata and the bounded remainder
/// envelope are validated in place.
fn canonical_epathmap_parts_from_expr(expr: &[u8]) -> Option<CanonicalEPathMapEnvelope<'_>> {
    canonical_epathmap_body_and_parts(expr).map(|(_, envelope)| envelope)
}

fn canonical_epathmap_body_and_parts(
    expr: &[u8],
) -> Option<(&[u8], CanonicalEPathMapEnvelope<'_>)> {
    let CanonicalExprEnvelope {
        tag: 32,
        payload: pathmap,
    } = canonical_expr_envelope(expr)?
    else {
        return None;
    };

    Some((pathmap, canonical_epathmap_parts(pathmap)?))
}

fn canonical_epathmap_parts(pathmap: &[u8]) -> Option<CanonicalEPathMapEnvelope<'_>> {
    let mut at = 0usize;
    let mut last_field = 0u64;
    let mut saw_locally_free = false;
    let mut saw_connective = false;
    let mut remainder = None;
    let mut snapshot = None;
    while at < pathmap.len() {
        let key = read_canonical_varint(pathmap, &mut at)?;
        let field = key >> 3;
        let wire = key & 0x07;
        (field >= last_field).then_some(())?;
        last_field = field;
        match (field, wire) {
            (3, 2) if !saw_locally_free => {
                let len = usize::try_from(read_canonical_varint(pathmap, &mut at)?).ok()?;
                (len != 0).then_some(())?;
                at = at.checked_add(len).filter(|end| *end <= pathmap.len())?;
                saw_locally_free = true;
            }
            (4, 0) if !saw_connective => {
                (read_canonical_varint(pathmap, &mut at)? == 1).then_some(())?;
                saw_connective = true;
            }
            (5, 2) if remainder.is_none() => {
                let len = usize::try_from(read_canonical_varint(pathmap, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                let body = pathmap.get(at..end)?;
                canonical_var(body)?;
                remainder = Some(body);
                at = end;
            }
            (9, 2) if snapshot.is_none() => {
                let len = usize::try_from(read_canonical_varint(pathmap, &mut at)?).ok()?;
                let end = at.checked_add(len)?;
                snapshot = Some(pathmap.get(at..end)?);
                at = end;
            }
            _ => return None,
        }
    }
    Some(CanonicalEPathMapEnvelope {
        snapshot: snapshot?,
        connective_used: saw_connective,
        locally_free_is_empty: !saw_locally_free,
        remainder,
    })
}

fn canonical_epathmap_snapshot_from_expr(expr: &[u8]) -> Option<&[u8]> {
    canonical_epathmap_parts_from_expr(expr).map(|parts| parts.snapshot)
}

fn canonical_singleton_gint_par(protobuf: &[u8]) -> bool {
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;
    const EXPR_GINT_KEY: u64 = 2 << 3;

    canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)
        .and_then(|expr| canonical_single_varint_field(expr, EXPR_GINT_KEY))
        .is_some()
}

/// Prove an exact one-entry map chain directly in its nested EPM1/protobuf
/// image. Every loop iteration consumes one strictly smaller value body, so
/// work and auxiliary state are linear in the final byte string rather than in
/// the sum of all decoded suffix graphs.
struct CanonicalSingletonEPathMapChainProof<'a> {
    depth: usize,
    terminal: &'a [u8],
    nested_values_stable: bool,
}

fn canonical_singleton_epathmap_chain_proof(
    mut snapshot: &[u8],
) -> Option<CanonicalSingletonEPathMapChainProof<'_>> {
    use crate::rust::epathmap_trie_codec::{CanonicalSnapshotKey, EPathMapMode};

    let mut depth = 0usize;
    let mut nested_values_stable = true;
    loop {
        let inspection =
            crate::rust::epathmap_trie_codec::inspect_canonical_map_snapshot(snapshot).ok()?;
        if inspection.mode != EPathMapMode::Map
            || inspection.keys.len() != 1
            || inspection.value_bodies.len() != 1
        {
            return None;
        }

        let key_is_stable = match &inspection.keys[0] {
            CanonicalSnapshotKey::Relative(range) => snapshot
                .get(range.clone())
                .and_then(|key| crate::rust::canonical_path::validate_canonical_path(key).ok())
                .is_some_and(|metadata| metadata.stable),
            CanonicalSnapshotKey::Owned(key) => {
                crate::rust::canonical_path::validate_canonical_path(key)
                    .is_ok_and(|metadata| metadata.stable)
            }
        };
        if !key_is_stable {
            return None;
        }

        let value_body = snapshot.get(inspection.value_bodies[0].clone())?;
        depth = depth.checked_add(1)?;
        let Some(nested_parts) = canonical_singleton_epathmap_parts(value_body) else {
            return Some(CanonicalSingletonEPathMapChainProof {
                depth,
                terminal: value_body,
                nested_values_stable,
            });
        };
        nested_values_stable &= nested_parts.locally_free.is_empty()
            && !nested_parts.connective_used
            && nested_parts.epathmap_locally_free_is_empty
            && !nested_parts.epathmap_connective_used
            && !nested_parts.epathmap_has_remainder;
        let nested = nested_parts.snapshot;
        if nested.len() >= snapshot.len() {
            return None;
        }
        snapshot = nested;
    }
}

fn canonical_singleton_epathmap_chain_parts(snapshot: &[u8]) -> Option<(usize, &[u8])> {
    canonical_singleton_epathmap_chain_proof(snapshot).map(|proof| (proof.depth, proof.terminal))
}

/// Import a canonical, stable singleton EPathMap chain as one raw Expr
/// endpoint. The nested maps remain in their prefix-compressed EPM1 image until
/// a semantic consumer actually asks to decode that Expr.
fn import_canonical_singleton_epathmap_par(protobuf: &[u8]) -> Option<Par> {
    let parts = canonical_singleton_epathmap_parts(protobuf)?;
    let chain = canonical_singleton_epathmap_chain_proof(parts.snapshot)?;
    canonical_singleton_gint_par(chain.terminal).then_some(())?;
    let component_stable = parts.epathmap_locally_free_is_empty
        && !parts.epathmap_connective_used
        && !parts.epathmap_has_remainder
        && chain.nested_values_stable;

    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(parts.snapshot.len().checked_mul(2)?)?
            .checked_add(parts.expr_body.len())?
            .checked_add(64)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();
    begin_score_node(&mut key, Score::EPATHMAP);
    append_bytes_score_atom(&mut key, parts.snapshot);
    append_int_score_atom(&mut key, i64::from(parts.epathmap_connective_used));
    end_score_node(&mut key);
    let score_len = u64::try_from(key.len().checked_sub(score_start)?)
        .expect("one allocated canonical score length fits in u64");
    key.extend_from_slice(parts.expr_body);
    key.extend_from_slice(&score_len.to_be_bytes());

    let mut components = ParBag::new();
    components
        .insert_bytes(
            ParComponentTag::Expr,
            &key,
            Multiplicity::ONE,
            Some(component_stable),
        )
        .expect("one validated singleton component cannot overflow multiplicity");
    Some(Par::from_parts(
        components,
        parts.locally_free.to_vec(),
        parts.connective_used,
    ))
}

/// Exact, non-consuming fast imports available to both top-level and nested
/// generated protobuf PDA entry points. `None` always delegates to the full
/// generated schema machine.
pub(crate) fn import_canonical_parbag_fast_path(protobuf: &[u8]) -> Option<Par> {
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;

    let unary = || {
        let expr_body = canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)?;
        let (depth, terminal) = canonical_unary_elist(expr_body)?;
        import_canonical_unary_elist_expr(expr_body, depth, terminal)
    };
    let unary_tuple = || {
        let expr_body = canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)?;
        let (depth, terminal) = canonical_unary_etuple(expr_body)?;
        import_canonical_unary_etuple_expr(expr_body, depth, terminal)
    };
    let unary_not = || {
        let expr_body = canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)?;
        let depth = canonical_unary_enot(expr_body)?;
        import_canonical_unary_enot_expr(expr_body, depth)
    };

    import_canonical_singleton_epathmap_par(protobuf)
        .or_else(unary)
        .or_else(unary_tuple)
        .or_else(unary_not)
        .or_else(|| import_canonical_singleton_eset_unary_elist_par(protobuf))
}

fn validate_typed_key<T: ParBagComponent>(
    path: &[u8],
    payload: &[u8],
) -> Result<bool, ParBagCodecError> {
    let value =
        T::decode_protobuf(payload).map_err(|source| ParBagCodecError::ComponentDecode {
            tag: T::TAG,
            reason: source.to_string(),
        })?;
    if !with_component_key(&value, |canonical| canonical == path) {
        return Err(ParBagCodecError::NonCanonicalComponent { tag: T::TAG });
    }
    Ok(T::canonical_path_stable(&value))
}

fn validate_key(path: &[u8]) -> Result<(ParComponentTag, usize, bool), ParBagCodecError> {
    let (tag, score, payload) = split_key_parts(path)?;
    if order_key_extent(score) != Some(score.len()) {
        return Err(ParBagCodecError::InvalidOrderKey { tag });
    }
    if tag == ParComponentTag::Expr {
        if let Some(matches) = validate_unary_elist_key(score, payload) {
            if matches {
                return Ok((tag, payload.len(), true));
            }
            return Err(ParBagCodecError::NonCanonicalComponent { tag });
        }
        if let Some(matches) = validate_unary_etuple_key(score, payload) {
            if matches {
                return Ok((tag, payload.len(), true));
            }
            return Err(ParBagCodecError::NonCanonicalComponent { tag });
        }
        if let Some(matches) = validate_unary_enot_key(score, payload) {
            if matches {
                return Ok((tag, payload.len(), false));
            }
            return Err(ParBagCodecError::NonCanonicalComponent { tag });
        }
    }
    let stable = match tag {
        ParComponentTag::Send => validate_typed_key::<Send>(path, payload)?,
        ParComponentTag::Receive => validate_typed_key::<Receive>(path, payload)?,
        ParComponentTag::New => validate_typed_key::<New>(path, payload)?,
        ParComponentTag::Expr => validate_typed_key::<Expr>(path, payload)?,
        ParComponentTag::Match => validate_typed_key::<Match>(path, payload)?,
        ParComponentTag::GUnforgeable => validate_typed_key::<GUnforgeable>(path, payload)?,
        ParComponentTag::Connective => validate_typed_key::<Connective>(path, payload)?,
        ParComponentTag::Bundle => validate_typed_key::<Bundle>(path, payload)?,
        ParComponentTag::If => validate_typed_key::<If>(path, payload)?,
    };
    Ok((tag, payload.len(), stable))
}

/// A borrowed, byte-native typed occurrence visited directly from the trie.
/// Decode only when an operation actually needs the Rust AST value.
#[derive(Clone, Copy)]
pub struct EncodedParComponent<'a, T> {
    score: &'a [u8],
    payload: &'a [u8],
    multiplicity: Multiplicity,
    marker: PhantomData<fn() -> T>,
}

/// Owned canonical protobuf body copied from one validated RPB3 endpoint.
///
/// Construction is restricted to [`EncodedParComponent`], whose bytes came
/// from an invariant-checked resident [`ParBag`] key. The type therefore
/// carries the admission proof required by the deferred EPathMap decoder after
/// the visiting zipper releases its borrowed path buffer. It deliberately owns
/// only the protobuf body: copying the proportional score prefix would add no
/// decoding evidence and would duplicate potentially large data.
pub struct OwnedCanonicalParComponent<T> {
    payload: Vec<u8>,
    multiplicity: Multiplicity,
    marker: PhantomData<fn() -> T>,
}

impl<T: ParBagComponent> OwnedCanonicalParComponent<T> {
    pub fn payload(&self) -> &[u8] { &self.payload }

    pub const fn multiplicity(&self) -> Multiplicity { self.multiplicity }

    /// Decode through the proof-carrying resident-key path. Full canonical
    /// validation already occurred at ParBag admission; the decoder still
    /// validates protobuf/EPM1 framing and ACT structure without rebuilding
    /// every canonical EPathMap key a second time.
    pub fn decode(self) -> Result<T, ParBagCodecError> { decode_exact::<T>(&self.payload) }

    pub fn into_payload(self) -> Vec<u8> { self.payload }
}

/// An owned proof that one authoritative RPB3 endpoint is an `EZipper`.
///
/// The score/body pair can only be created by the typed [`ParBag`] visitor,
/// after the resident trie key has passed its complete invariant checks.  The
/// private fields deliberately prevent downstream code from pairing arbitrary
/// score bytes with a different protobuf body.
pub struct OwnedCanonicalEzipper {
    score: Box<[u8]>,
    protobuf: Box<[u8]>,
    locally_free: Box<[u8]>,
    connective_used: bool,
}

impl OwnedCanonicalEzipper {
    pub fn locally_free(&self) -> &[u8] { &self.locally_free }

    pub const fn connective_used(&self) -> bool { self.connective_used }

    pub(crate) fn into_parts(self) -> (Box<[u8]>, Box<[u8]>) { (self.score, self.protobuf) }
}

/// One typed component yielded to a representation-independent worklist.
///
/// Generated protobuf `Par` can lend its already-decoded repeated-field
/// elements. A PathMap-backed `Par` decodes one distinct trie endpoint and
/// transfers ownership to the worklist. Keeping that distinction explicit
/// prevents the transitional generated representation from deep-cloning a
/// recursive component merely to feed an iterative consumer.
pub enum ParComponentItem<'a, T> {
    Borrowed(&'a T),
    Owned(T),
}

impl<T> AsRef<T> for ParComponentItem<'_, T> {
    fn as_ref(&self) -> &T {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(value) => value,
        }
    }
}

impl<'a, T: ParBagComponent> EncodedParComponent<'a, T> {
    pub const fn score(&self) -> &'a [u8] { self.score }

    pub const fn payload(&self) -> &'a [u8] { self.payload }

    pub const fn multiplicity(&self) -> Multiplicity { self.multiplicity }

    pub fn decode(self) -> Result<T, ParBagCodecError> { decode_exact::<T>(self.payload) }

    pub fn to_owned_canonical(self) -> OwnedCanonicalParComponent<T> {
        OwnedCanonicalParComponent {
            payload: self.payload.to_vec(),
            multiplicity: self.multiplicity,
            marker: PhantomData,
        }
    }
}

impl EncodedParComponent<'_, Connective> {
    /// Decode only the terminal below an exact non-empty spine of unary AND/OR
    /// connectives. Every intermediate `Par` remains a borrowed protobuf
    /// region inside this authoritative trie endpoint.
    pub(crate) fn canonical_unary_connective_terminal(self) -> Option<Par> {
        decode_unary_connective_terminal(self.payload)
    }

    pub(crate) fn canonical_unary_connective_spine(self) -> Option<(Par, bool, bool)> {
        decode_unary_connective_spine(self.payload)
    }
}

impl EncodedParComponent<'_, Bundle> {
    pub(crate) fn canonical_unary_bundle_terminal(self) -> Option<Par> {
        decode_unary_bundle_terminal(self.payload)
    }
}

impl<'a> EncodedParComponent<'a, Expr> {
    /// Retain an opaque runtime zipper as its authoritative RPB3 score/body
    /// pair.  This performs only canonical field-envelope scans: the inner
    /// `EPathMap` snapshot is skipped by length and is never decoded or
    /// traversed.
    pub fn to_owned_canonical_ezipper(&self) -> Option<OwnedCanonicalEzipper> {
        let parts = canonical_ezipper_region_parts(self.payload)?;
        Some(OwnedCanonicalEzipper {
            score: self.score.into(),
            protobuf: self.payload.into(),
            locally_free: parts.locally_free.into(),
            connective_used: parts.connective_used,
        })
    }

    /// Borrow the depth and terminal `Par` body of an exact one-entry
    /// `PathMap<Par>` value chain.
    ///
    /// Each step inspects the stored EPM1 snapshot in place. Keys are required
    /// to be valid stable canonical paths, map mode and singleton arity are
    /// proved at every level, and the terminal remains a borrowed protobuf
    /// region. No `EPathMap`, nested `Par`, entry vector, or trie shadow is
    /// materialized.
    pub fn canonical_singleton_epathmap_map_parts(&self) -> Option<(usize, &'a [u8])> {
        let snapshot = canonical_epathmap_snapshot_from_expr(self.payload)?;
        canonical_singleton_epathmap_chain_parts(snapshot)
    }

    /// Return the depth and borrowed terminal `Par` protobuf body of an exact
    /// non-empty canonical unary `ENot` spine.
    ///
    /// Intermediate `Par`s must be metadata-free singleton expressions. The
    /// terminal itself may have any shape and remains borrowed from the
    /// authoritative component key.
    pub fn canonical_unary_enot_parts(&self) -> Option<(usize, &'a [u8])> {
        canonical_unary_enot_parts(self.payload)
    }

    /// Return the depth of an exact canonical
    /// `ENot(Par(ENot(.. Par::default() ..)))` component.
    ///
    /// Like the ordered-collection recognizers below, this walks the stored
    /// protobuf body directly and allocates no nested `Par` values.
    pub fn canonical_unary_enot_depth(self) -> Option<usize> { canonical_unary_enot(self.payload) }

    /// Return the depth of an exact canonical
    /// `EList([EList([.. EList([GInt(_)]) ..])])` component.
    ///
    /// This is a schema-directed byte cursor over the stored canonical
    /// protobuf body. It allocates nothing and never materializes the nested
    /// `Par`s, so shape probes and non-vacuity gates remain linear in the
    /// encoded body instead of repeatedly decoding every remaining suffix.
    pub fn canonical_unary_elist_depth(self) -> Option<usize> {
        canonical_unary_elist(self.payload).map(|(depth, _terminal)| depth)
    }

    /// Return both scalar parameters of the exact canonical unary-list shape.
    /// Canonical-path encoding can consequently stream its structural trie
    /// image without decoding any suffix `ParBag`.
    pub(crate) fn canonical_unary_elist_parts(self) -> Option<(usize, i64)> {
        canonical_unary_elist(self.payload)
    }

    /// Return both scalar parameters of the exact canonical unary-tuple shape
    /// without decoding any nested ParBag suffix.
    pub(crate) fn canonical_unary_etuple_parts(self) -> Option<(usize, i64)> {
        canonical_unary_etuple(self.payload)
    }
}

/// One owned, canonical ParBag trie key compiled from a typed component.
///
/// RSpace pattern/channel plans are long-lived relative to individual trie
/// probes. They should construct this value once and reuse its bytes, avoiding
/// repeated canonical sorting and serialization in the matching loop. The
/// phantom component type makes it impossible to pass an `Expr` key to a
/// `Send`-typed API; the key owns one boxed byte slice and carries no shadow AST
/// or architecture-dependent identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalParBagKey<T: ParBagComponent> {
    bytes: Box<[u8]>,
    canonical_path_stable: bool,
    marker: PhantomData<fn() -> T>,
}

impl<T: ParBagComponent> CanonicalParBagKey<T> {
    pub fn compile(value: &T) -> Self {
        with_component_key(value, |bytes| Self {
            bytes: bytes.into(),
            canonical_path_stable: T::canonical_path_stable(value),
            marker: PhantomData,
        })
    }

    pub fn as_bytes(&self) -> &[u8] { &self.bytes }

    pub(crate) fn parts(&self) -> (&[u8], &[u8]) {
        let (tag, score, protobuf) = split_key_parts(&self.bytes)
            .expect("a compiled canonical component key has valid framing");
        debug_assert_eq!(tag, T::TAG);
        (score, protobuf)
    }

    pub(crate) const fn canonical_path_stable(&self) -> bool { self.canonical_path_stable }
}

impl<T: ParBagComponent> fmt::Debug for CanonicalParBagKey<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CanonicalParBagKey")
            .field("tag", &T::TAG)
            .field("bytes", &self.bytes)
            .finish()
    }
}

/// One homogeneous PathMap-backed multiset containing every top-level `Par`
/// component category. Empty `Par` is exactly an empty `ParBag`.
#[derive(Clone, Copy)]
struct ParFamilyMetrics {
    exact_cardinalities: [u128; PAR_COMPONENT_FAMILY_COUNT],
    distinct_lengths: [usize; PAR_COMPONENT_FAMILY_COUNT],
    /// Exact repeated-message protobuf byte totals by constructor family.
    ///
    /// `None` is a real arithmetic-overflow state, not an uninitialized cache.
    /// Keeping the total beside the multiplicity metrics makes `Par::encoded_len`
    /// O(1): it never performs nine independent prefix walks over the trie.
    exact_protobuf_lengths: [Option<u128>; PAR_COMPONENT_FAMILY_COUNT],
}

impl Default for ParFamilyMetrics {
    fn default() -> Self {
        Self {
            exact_cardinalities: [0; PAR_COMPONENT_FAMILY_COUNT],
            distinct_lengths: [0; PAR_COMPONENT_FAMILY_COUNT],
            exact_protobuf_lengths: [Some(0); PAR_COMPONENT_FAMILY_COUNT],
        }
    }
}

impl ParFamilyMetrics {
    fn framed_occurrence_len(tag: ParComponentTag, body_len: usize) -> u128 {
        key_len(tag.byte().into()) as u128
            + encoded_len_varint(body_len as u64) as u128
            + body_len as u128
    }

    fn add_decoded(
        &mut self,
        tag: ParComponentTag,
        multiplicity: Multiplicity,
        body_len: usize,
    ) -> Result<(), ParBagCodecError> {
        let index = tag.index();
        self.exact_cardinalities[index] = self.exact_cardinalities[index]
            .checked_add(u128::from(multiplicity.get()))
            .ok_or(ParBagCodecError::FamilyCardinalityOverflow { tag })?;
        self.distinct_lengths[index] = self.distinct_lengths[index]
            .checked_add(1)
            .expect("Bag distinct length already fits usize");
        let contribution =
            Self::framed_occurrence_len(tag, body_len).checked_mul(u128::from(multiplicity.get()));
        self.exact_protobuf_lengths[index] = self.exact_protobuf_lengths[index]
            .and_then(|current| contribution.and_then(|add| current.checked_add(add)));
        Ok(())
    }

    fn from_trusted_components(components: &Bag) -> Self {
        let mut metrics = Self::default();
        for (path, multiplicity) in components.iter() {
            let (tag, payload) = split_key(&path)
                .expect("ParBag invariant: every internally produced key has a valid header");
            metrics
                .add_decoded(tag, multiplicity, payload.len())
                .expect("a resident Bag family cardinality fits u128");
        }
        metrics
    }
}

struct ParBagAux {
    metrics: ParFamilyMetrics,
    compact_snapshot: OnceLock<Arc<[u8]>>,
    /// RMP2's BLAKE2b-256 digest of the canonical component transcript.
    ///
    /// Counts and token framing stay outside this cell: they are O(1) metrics,
    /// while this is the one derived value that otherwise reopens every
    /// PathMap family prefix. The cell lives in the existing CoW auxiliary root
    /// so untouched clones share it and a successful component mutation drops
    /// it with the other component-derived projections.
    rmp2_component_digest: OnceLock<[u8; 32]>,
    /// Concatenated component score keys in canonical Par family order.
    score_components: OnceLock<Arc<[u8]>>,
    /// Canonical protobuf component bytes on either side of root fields 9/10.
    protobuf_before_root: OnceLock<Arc<[u8]>>,
    protobuf_after_root: OnceLock<Arc<[u8]>>,
    /// Stability of the sole component, when cardinality is exactly one.
    /// Multi-component and empty bags are known unstable without consulting
    /// this cell. The cell is shared by CoW clones and invalidated on mutation.
    canonical_path_singleton_stable: OnceLock<bool>,
    /// Exact locally-free fold over concrete component endpoints. The fold is
    /// derived from shallow canonical protobuf metadata, never from decoded
    /// recursive values, and is shared by CoW clones.
    concrete_components_locally_free_empty: OnceLock<Option<bool>>,
}

impl ParBagAux {
    fn new(metrics: ParFamilyMetrics) -> Self {
        Self {
            metrics,
            compact_snapshot: OnceLock::new(),
            rmp2_component_digest: OnceLock::new(),
            score_components: OnceLock::new(),
            protobuf_before_root: OnceLock::new(),
            protobuf_after_root: OnceLock::new(),
            canonical_path_singleton_stable: OnceLock::new(),
            concrete_components_locally_free_empty: OnceLock::new(),
        }
    }
}

pub struct ParBag {
    components: Bag,
    /// Cardinalities, distinct counts, and component-derived memoized images
    /// are shared by CoW clones. A successful mutation detaches this compact
    /// metadata/cache block exactly once.
    aux: Arc<ParBagAux>,
}

/// Lazy unique partitions of one Par component family.
///
/// One counter is retained per distinct PathMap endpoint, not per occurrence.
/// Counter zero varies fastest, which is the order in which the legacy
/// positional bit-subset enumerator first encountered each distinct Bag
/// partition. Positional duplicates are observationally identical to the
/// matcher and are deliberately emitted once.
#[derive(Clone)]
pub struct FamilyPartitions {
    entries: Arc<[(Arc<[u8]>, Multiplicity)]>,
    selected: Box<[u64]>,
    selected_total: u128,
    minimum: u128,
    maximum: u128,
    started: bool,
    exhausted: bool,
}

impl FamilyPartitions {
    fn new(entries: Vec<(Arc<[u8]>, Multiplicity)>, minimum: u64, maximum: u64) -> Self {
        let entries: Arc<[(Arc<[u8]>, Multiplicity)]> = entries.into();
        Self {
            // Mixed-radix multiplicity counters, not locally-free bit-vector
            // bytes. Keep the element type explicit so the source-level
            // bitset-producer census cannot misclassify this scratch state.
            selected: vec![0u64; entries.len()].into_boxed_slice(),
            entries,
            selected_total: 0,
            minimum: u128::from(minimum),
            maximum: u128::from(maximum),
            started: false,
            exhausted: minimum > maximum,
        }
    }

    /// Advance the mixed-radix counter at or above `start`, resetting every
    /// less-significant digit. Returns the digit that changed.
    fn advance_from(&mut self, start: usize) -> Option<usize> {
        for index in 0..start {
            self.selected_total -= u128::from(self.selected[index]);
            self.selected[index] = 0;
        }
        for index in start..self.entries.len() {
            let available = self.entries[index].1.get();
            if self.selected[index] < available {
                self.selected[index] += 1;
                self.selected_total += 1;
                return Some(index);
            }
            self.selected_total -= u128::from(self.selected[index]);
            self.selected[index] = 0;
        }
        None
    }

    /// Raise only digits less significant than `end` to the smallest value
    /// whose total reaches `minimum`.
    fn fill_minimum_below(&mut self, end: usize) -> bool {
        let mut needed = self.minimum.saturating_sub(self.selected_total);
        for index in 0..end {
            if needed == 0 {
                break;
            }
            let room = self.entries[index].1.get() - self.selected[index];
            let added = u128::from(room).min(needed) as u64;
            self.selected[index] += added;
            self.selected_total += u128::from(added);
            needed -= u128::from(added);
        }
        needed == 0
    }

    fn initialize(&mut self) -> bool {
        self.fill_minimum_below(self.entries.len()) && self.selected_total <= self.maximum
    }

    fn advance_to_feasible(&mut self) -> bool {
        let Some(mut changed) = self.advance_from(0) else {
            return false;
        };
        loop {
            if self.selected_total > self.maximum {
                let Some(next_changed) = self.advance_from(changed + 1) else {
                    return false;
                };
                changed = next_changed;
                continue;
            }
            if self.selected_total < self.minimum {
                if self.fill_minimum_below(changed) {
                    return true;
                }
                let Some(next_changed) = self.advance_from(changed + 1) else {
                    return false;
                };
                changed = next_changed;
                continue;
            }
            return true;
        }
    }

    fn current_partition(&self) -> (ParBag, ParBag) {
        let mut selected = Bag::new();
        let mut complement = Bag::new();
        for ((path, available), selected_count) in self.entries.iter().zip(self.selected.iter()) {
            if *selected_count != 0 {
                selected.set_count(
                    path,
                    Some(
                        Multiplicity::new(*selected_count)
                            .expect("a selected positive count is a multiplicity"),
                    ),
                );
            }
            let complement_count = available.get() - *selected_count;
            if complement_count != 0 {
                complement.set_count(
                    path,
                    Some(
                        Multiplicity::new(complement_count)
                            .expect("a complementary positive count is a multiplicity"),
                    ),
                );
            }
        }
        (
            ParBag::from_components(selected),
            ParBag::from_components(complement),
        )
    }
}

impl Iterator for FamilyPartitions {
    type Item = (ParBag, ParBag);

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        if self.started {
            if !self.advance_to_feasible() {
                self.exhausted = true;
                return None;
            }
        } else {
            self.started = true;
            if !self.initialize() {
                self.exhausted = true;
                return None;
            }
        }
        Some(self.current_partition())
    }
}

impl Default for ParBag {
    fn default() -> Self {
        Self {
            components: Bag::new(),
            aux: Arc::new(ParBagAux::new(ParFamilyMetrics::default())),
        }
    }
}

impl Clone for ParBag {
    fn clone(&self) -> Self {
        Self {
            components: self.components.clone(),
            aux: Arc::clone(&self.aux),
        }
    }
}

impl PartialEq for ParBag {
    fn eq(&self, other: &Self) -> bool { self.components == other.components }
}

impl Eq for ParBag {}

impl PartialOrd for ParBag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for ParBag {
    fn cmp(&self, other: &Self) -> Ordering { self.components.cmp(&other.components) }
}

impl Hash for ParBag {
    fn hash<H: Hasher>(&self, state: &mut H) { self.components.hash(state) }
}

impl fmt::Debug for ParBag {
    /// Constant-space structural diagnostics.
    ///
    /// PathMap keys contain complete canonical protobuf component bodies.
    /// Printing those byte arrays would make a diagnostic proportional to the
    /// encoded term (quadratic for a unary nested-protobuf ladder) and would
    /// expose implementation bytes rather than useful Bag state.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParBag")
            .field("distinct_len", &self.components.distinct_len())
            .field("cardinality", &self.components.cardinality())
            .field(
                "cardinality_is_saturated",
                &self.components.cardinality_is_saturated(),
            )
            .field(
                "family_cardinalities",
                &self
                    .aux
                    .metrics
                    .exact_cardinalities
                    .map(project_cardinality),
            )
            .finish()
    }
}

impl ParBag {
    pub fn new() -> Self { Self::default() }

    /// Whether two bags still share the immutable auxiliary root installed by
    /// [`Clone`]. Every successful mutation detaches that root before changing
    /// either the authoritative [`Bag`] or one of its byte projections, so a
    /// `true` result is a conservative, allocation-free proof that the two
    /// compact component images are byte-identical.
    ///
    /// This is an optimization witness, not semantic equality: independently
    /// constructed equal bags deliberately return `false`.
    pub fn shares_storage_with(&self, other: &Self) -> bool { Arc::ptr_eq(&self.aux, &other.aux) }

    #[inline]
    fn aux_for_mutation(&mut self) -> &mut ParBagAux {
        if Arc::get_mut(&mut self.aux).is_none() {
            self.aux = Arc::new(ParBagAux::new(self.aux.metrics));
        }
        let aux = Arc::get_mut(&mut self.aux).expect("ParBag owns its detached auxiliary state");
        let _ = aux.compact_snapshot.take();
        let _ = aux.rmp2_component_digest.take();
        let _ = aux.score_components.take();
        let _ = aux.protobuf_before_root.take();
        let _ = aux.protobuf_after_root.take();
        let _ = aux.canonical_path_singleton_stable.take();
        let _ = aux.concrete_components_locally_free_empty.take();
        aux
    }

    /// A newly-created one-endpoint Par is the dominant recursive construction
    /// shape. The complete key is already in hand, so seed all byte projections
    /// now instead of opening three independent PathMap zippers when its parent
    /// immediately consumes it.
    fn seed_singleton_caches(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        canonical_path_stable: Option<bool>,
    ) {
        let (actual_tag, score, body) = split_key_parts(key)
            .expect("ParBag invariant: a typed singleton key has a valid header");
        debug_assert_eq!(actual_tag, tag);
        let score: Arc<[u8]> = Arc::from(score);
        let mut protobuf = Vec::with_capacity(
            key_len(tag.byte().into()) + encoded_len_varint(body.len() as u64) + body.len(),
        );
        encode_key(tag.byte().into(), WireType::LengthDelimited, &mut protobuf);
        prost::encoding::encode_varint(body.len() as u64, &mut protobuf);
        protobuf.extend_from_slice(body);
        let protobuf: Arc<[u8]> = Arc::from(protobuf);
        let empty: Arc<[u8]> = Arc::from([]);

        let aux = Arc::get_mut(&mut self.aux).expect("singleton insertion owns auxiliary state");
        if let Some(stable) = canonical_path_stable {
            aux.canonical_path_singleton_stable
                .set(stable)
                .expect("mutation cleared the singleton stability cache");
        }
        aux.score_components
            .set(score)
            .expect("mutation cleared the singleton score cache");
        if matches!(tag, ParComponentTag::Bundle | ParComponentTag::If) {
            aux.protobuf_before_root
                .set(Arc::clone(&empty))
                .expect("mutation cleared the before-root protobuf cache");
            aux.protobuf_after_root
                .set(protobuf)
                .expect("mutation cleared the after-root protobuf cache");
        } else {
            aux.protobuf_before_root
                .set(protobuf)
                .expect("mutation cleared the before-root protobuf cache");
            aux.protobuf_after_root
                .set(empty)
                .expect("mutation cleared the after-root protobuf cache");
        }
    }

    #[inline]
    fn record_insert(
        &mut self,
        tag: ParComponentTag,
        amount: Multiplicity,
        body_len: usize,
        distinct_added: bool,
    ) {
        let metrics = &mut self.aux_for_mutation().metrics;
        let index = tag.index();
        metrics.exact_cardinalities[index] = metrics.exact_cardinalities[index]
            .checked_add(u128::from(amount.get()))
            .expect("a resident Bag family cardinality fits u128");
        if distinct_added {
            metrics.distinct_lengths[index] = metrics.distinct_lengths[index]
                .checked_add(1)
                .expect("Bag distinct length already fits usize");
        }
        let contribution = ParFamilyMetrics::framed_occurrence_len(tag, body_len)
            .checked_mul(u128::from(amount.get()));
        metrics.exact_protobuf_lengths[index] = metrics.exact_protobuf_lengths[index]
            .and_then(|current| contribution.and_then(|add| current.checked_add(add)));
    }

    #[inline]
    fn record_remove(
        &mut self,
        tag: ParComponentTag,
        removed: u64,
        body_len: usize,
        distinct_removed: bool,
    ) {
        let index = tag.index();
        // A u128 overflow is possible in the abstract PathMap language even
        // though it is unreachable for ordinary resident terms. If it did
        // occur, subtraction cannot recover the exact value from a sentinel;
        // rebuild once from the already-mutated trie instead.
        if self.aux.metrics.exact_protobuf_lengths[index].is_none() {
            let rebuilt = ParFamilyMetrics::from_trusted_components(&self.components);
            let metrics = &mut self.aux_for_mutation().metrics;
            metrics.exact_cardinalities[index] = rebuilt.exact_cardinalities[index];
            metrics.distinct_lengths[index] = rebuilt.distinct_lengths[index];
            metrics.exact_protobuf_lengths[index] = rebuilt.exact_protobuf_lengths[index];
            return;
        }
        let metrics = &mut self.aux_for_mutation().metrics;
        metrics.exact_cardinalities[index] -= u128::from(removed);
        if distinct_removed {
            metrics.distinct_lengths[index] -= 1;
        }
        let contribution = ParFamilyMetrics::framed_occurrence_len(tag, body_len)
            .checked_mul(u128::from(removed))
            .expect("one resident component's protobuf expansion fits u128");
        let exact = metrics.exact_protobuf_lengths[index]
            .as_mut()
            .expect("the overflow case rebuilt and returned above");
        *exact = exact
            .checked_sub(contribution)
            .expect("ParBag protobuf metrics cannot remove absent multiplicity");
    }

    fn insert_bytes(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
        canonical_path_stable: Option<bool>,
    ) -> Result<Multiplicity, BagError> {
        let was_empty = self.components.is_empty();
        let distinct_added = !self.components.contains(key);
        let next = self.components.insert_many(key, amount)?;
        let (_, payload) = split_key(key)
            .expect("ParBag invariant: typed insertion produced a valid component key");
        self.record_insert(tag, amount, payload.len(), distinct_added);
        if was_empty && amount == Multiplicity::ONE {
            self.seed_singleton_caches(tag, key, canonical_path_stable);
        }
        Ok(next)
    }

    /// Insert a key emitted by the schema-driven [`ParConstruction`] PDA.
    ///
    /// The planner constructs the same canonical RPB3 key language as
    /// [`ParBagComponent::append_key_suffix`] and has already proved its score
    /// and protobuf lengths.  Revalidating it here would decode the just-built
    /// recursive body and restore the construction cost this path removes.
    pub(crate) fn insert_planned_key(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
        canonical_path_stable: bool,
    ) -> Result<Multiplicity, BagError> {
        self.insert_bytes(tag, key, amount, Some(canonical_path_stable))
    }

    fn remove_bytes(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
    ) -> Option<Multiplicity> {
        let current = self.components.count(key);
        if current == 0 {
            return None;
        }
        let removed = current.min(amount.get());
        let next = self.components.remove_many(key, amount);
        let (_, payload) =
            split_key(key).expect("ParBag invariant: typed removal used a valid component key");
        self.record_remove(tag, removed, payload.len(), removed == current);
        next
    }

    fn try_remove_bytes(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        let next = self.components.try_remove_many(key, amount)?;
        let (_, payload) =
            split_key(key).expect("ParBag invariant: typed removal used a valid component key");
        self.record_remove(tag, amount.get(), payload.len(), next.is_none());
        Ok(next)
    }

    pub fn try_from_bag(components: Bag) -> Result<Self, ParBagCodecError> {
        let mut metrics = ParFamilyMetrics::default();
        let is_singleton = components.cardinality() == 1;
        let mut singleton_stable = None;
        for (path, multiplicity) in components.iter() {
            let (tag, body_len, stable) = validate_key(&path)?;
            metrics.add_decoded(tag, multiplicity, body_len)?;
            if is_singleton {
                debug_assert_eq!(multiplicity, Multiplicity::ONE);
                singleton_stable = Some(stable);
            }
        }
        let bag = Self {
            components,
            aux: Arc::new(ParBagAux::new(metrics)),
        };
        if let Some(stable) = singleton_stable {
            bag.aux
                .canonical_path_singleton_stable
                .set(stable)
                .expect("newly imported ParBag stability cell is empty");
        }
        Ok(bag)
    }

    pub fn as_bag(&self) -> &Bag { &self.components }

    pub fn into_bag(self) -> Bag { self.components }

    pub const fn distinct_len(&self) -> usize { self.components.distinct_len() }

    pub const fn cardinality(&self) -> u64 { self.components.cardinality() }

    pub const fn cardinality_is_saturated(&self) -> bool {
        self.components.cardinality_is_saturated()
    }

    pub fn is_empty(&self) -> bool { self.components.is_empty() }

    /// Return RMP2's memoized component-transcript digest.
    ///
    /// The caller owns the versioned transcript algorithm. This narrow closure
    /// boundary keeps `ParBag` independent of `rholang::match_path` while the
    /// cache remains attached to the authoritative CoW component storage.
    /// Only the 32-byte digest is retained; counts, token bytes, transcript
    /// bytes, compact ACT images, and region-route summaries are not cached.
    pub(crate) fn memoized_rmp2_component_digest(
        &self,
        initialize: impl FnOnce(&Self) -> [u8; 32],
    ) -> [u8; 32] {
        *self
            .aux
            .rmp2_component_digest
            .get_or_init(|| initialize(self))
    }

    /// Cached stability of the complete component bag under the assumption
    /// that the owning `Par` has empty root metadata. Empty and multi-element
    /// bags are structurally unable to inhabit the stable canonical-path
    /// alphabet; singleton bags use the exact fold seeded at typed insertion or
    /// canonical ACT import.
    pub(crate) fn cached_canonical_path_stability(&self) -> Option<bool> {
        if self.cardinality() != 1 {
            Some(false)
        } else {
            self.aux.canonical_path_singleton_stable.get().copied()
        }
    }

    /// Memoize the exact result of a fallback semantic traversal. This is
    /// interior cache state shared by CoW clones; it never enters equality,
    /// hashing, ordering, protobuf, bincode, or the compact PathMap image.
    pub(crate) fn cache_canonical_path_stability(&self, stable: bool) {
        if self.cardinality() == 1 {
            let _ = self.aux.canonical_path_singleton_stable.set(stable);
        }
    }

    /// Prove that every concrete component's own locally-free fold is empty.
    ///
    /// The first call walks each distinct prefix-compressed endpoint once and
    /// only scans its top-level canonical framing. Recursive child bodies are
    /// skipped by length, so a unary depth ladder remains linear across all
    /// enclosing Pars. Later calls and CoW clones reuse the derived result in
    /// O(1). `None` is a conservative lack of proof for an unrecognized
    /// canonical shape and must make a caller use its complete semantic path.
    pub fn concrete_components_locally_free_are_empty(&self) -> Option<bool> {
        *self
            .aux
            .concrete_components_locally_free_empty
            .get_or_init(|| {
                let mut result = Some(true);
                self.visit_canonical_key_parts(|tag, _score, protobuf, _multiplicity| {
                    if result == Some(true) {
                        result = canonical_component_locally_free_is_empty(tag, protobuf);
                    }
                });
                result
            })
    }

    pub fn contains<T: ParBagComponent>(&self, value: &T) -> bool {
        with_component_key(value, |key| self.components.contains(key))
    }

    /// Exact membership for a key compiled once by the matcher/channel plan.
    #[inline]
    pub fn contains_key<T: ParBagComponent>(&self, key: &CanonicalParBagKey<T>) -> bool {
        self.components.contains(key.as_bytes())
    }

    pub fn count<T: ParBagComponent>(&self, value: &T) -> u64 {
        with_component_key(value, |key| self.components.count(key))
    }

    #[inline]
    pub fn count_key<T: ParBagComponent>(&self, key: &CanonicalParBagKey<T>) -> u64 {
        self.components.count(key.as_bytes())
    }

    /// Merge an exact canonical Expr protobuf body through a byte-native
    /// specialization when one is available. `None` means the caller must use
    /// the complete generated semantic decoder; `Some` has consumed no input
    /// but reports the authoritative Bag insertion result.
    pub(crate) fn insert_canonical_expr_protobuf_fast_path(
        &mut self,
        expr_body: &[u8],
    ) -> Option<Result<(), BagError>> {
        let depth = canonical_unary_enot(expr_body)?;
        let key = canonical_unary_enot_key(expr_body, depth)?;
        Some(
            self.insert_bytes(ParComponentTag::Expr, &key, Multiplicity::ONE, Some(false))
                .map(|_| ()),
        )
    }

    pub fn insert<T: ParBagComponent>(&mut self, value: &T) -> Result<Multiplicity, BagError> {
        with_component_key(value, |key| {
            self.insert_bytes(
                T::TAG,
                key,
                Multiplicity::ONE,
                Some(T::canonical_path_stable(value)),
            )
        })
    }

    #[inline]
    pub fn insert_key<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
    ) -> Result<Multiplicity, BagError> {
        self.insert_bytes(
            T::TAG,
            key.as_bytes(),
            Multiplicity::ONE,
            Some(key.canonical_path_stable),
        )
    }

    pub fn insert_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        with_component_key(value, |key| {
            self.insert_bytes(T::TAG, key, amount, Some(T::canonical_path_stable(value)))
        })
    }

    #[inline]
    pub fn insert_key_many<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        self.insert_bytes(
            T::TAG,
            key.as_bytes(),
            amount,
            Some(key.canonical_path_stable),
        )
    }

    pub fn remove<T: ParBagComponent>(&mut self, value: &T) -> Option<Multiplicity> {
        with_component_key(value, |key| {
            self.remove_bytes(T::TAG, key, Multiplicity::ONE)
        })
    }

    #[inline]
    pub fn remove_key<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
    ) -> Option<Multiplicity> {
        self.remove_bytes(T::TAG, key.as_bytes(), Multiplicity::ONE)
    }

    pub fn remove_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Option<Multiplicity> {
        with_component_key(value, |key| self.remove_bytes(T::TAG, key, amount))
    }

    #[inline]
    pub fn remove_key_many<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
        amount: Multiplicity,
    ) -> Option<Multiplicity> {
        self.remove_bytes(T::TAG, key.as_bytes(), amount)
    }

    pub fn try_remove_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        with_component_key(value, |key| self.try_remove_bytes(T::TAG, key, amount))
    }

    #[inline]
    pub fn try_remove_key_many<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        self.try_remove_bytes(T::TAG, key.as_bytes(), amount)
    }

    /// Remove one entire constructor family with one prefix operation.
    pub fn remove_family<T: ParBagComponent>(&mut self) -> (usize, u64) {
        let index = T::TAG.index();
        let expected_distinct = self.aux.metrics.distinct_lengths[index];
        let expected_cardinality = self.aux.metrics.exact_cardinalities[index];
        let removed = self.components.remove_prefix(&component_prefix(T::TAG));
        if removed.0 != 0 {
            debug_assert_eq!(removed.0, expected_distinct);
            debug_assert_eq!(removed.1, project_cardinality(expected_cardinality));
            let metrics = &mut self.aux_for_mutation().metrics;
            metrics.distinct_lengths[index] = 0;
            metrics.exact_cardinalities[index] = 0;
            metrics.exact_protobuf_lengths[index] = Some(0);
        }
        removed
    }

    pub fn family_distinct_len<T: ParBagComponent>(&self) -> usize {
        self.aux.metrics.distinct_lengths[T::TAG.index()]
    }

    pub fn family_cardinality<T: ParBagComponent>(&self) -> u64 {
        project_cardinality(self.aux.metrics.exact_cardinalities[T::TAG.index()])
    }

    pub fn family_cardinality_is_saturated<T: ParBagComponent>(&self) -> bool {
        self.aux.metrics.exact_cardinalities[T::TAG.index()] > u64::MAX as u128
    }

    /// Lazily enumerate the unique Bag partitions of one component family.
    ///
    /// Canonical RPB3 keys stay encoded and one counter is carried per distinct
    /// PathMap endpoint. A key with multiplicity N therefore consumes O(1)
    /// iterator state rather than N `Arc` slots. Counter zero varies fastest,
    /// preserving the first-occurrence order of the legacy positional subset
    /// enumeration while suppressing duplicate identical Bag candidates.
    pub fn family_partitions<T: ParBagComponent>(
        &self,
        min_size: u64,
        max_size: u64,
    ) -> FamilyPartitions {
        let mut entries = Vec::with_capacity(self.family_distinct_len::<T>());
        self.components
            .visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                entries.push((Arc::from(path), multiplicity));
            });
        FamilyPartitions::new(entries, min_size, max_size)
    }

    /// Visit one constructor family without decoding its payloads.
    pub fn visit_encoded<T: ParBagComponent>(
        &self,
        mut visitor: impl FnMut(EncodedParComponent<'_, T>),
    ) {
        self.components
            .visit_relative_prefix(&component_prefix(T::TAG), |suffix, multiplicity| {
                let (score, payload) = split_key_suffix(T::TAG, suffix)
                    .expect("ParBag invariant: every stored component key was validated");
                visitor(EncodedParComponent {
                    score,
                    payload,
                    multiplicity,
                    marker: PhantomData,
                });
            });
    }

    /// Visit every authoritative RPB3 endpoint as its already-separated score
    /// and protobuf slices.
    ///
    /// This is the zero-decode import boundary for transient construction
    /// plans. The callback borrows the zipper's reused path buffer; callers
    /// that retain a fragment must copy that fragment before returning.
    pub(crate) fn visit_canonical_key_parts(
        &self,
        mut visitor: impl FnMut(ParComponentTag, &[u8], &[u8], Multiplicity),
    ) {
        for tag in [
            ParComponentTag::Send,
            ParComponentTag::Receive,
            ParComponentTag::New,
            ParComponentTag::Expr,
            ParComponentTag::Match,
            ParComponentTag::GUnforgeable,
            ParComponentTag::Connective,
            ParComponentTag::Bundle,
            ParComponentTag::If,
        ] {
            self.components.visit_relative_prefix(
                &component_prefix(tag),
                |suffix, multiplicity| {
                    let (score, protobuf) = split_key_suffix(tag, suffix)
                        .expect("ParBag invariant: every component has a valid RPB3 key");
                    visitor(tag, score, protobuf, multiplicity);
                },
            );
        }
    }

    /// Append one family's canonical score trees in the exact order encoded by
    /// its PathMap keys. Keys are
    /// `score-prefix || canonical-protobuf-body || u64-BE(score-length)`, so
    /// zipper order is already the sorter's `(score, emitted bytes)` order.
    /// No component body is decoded and repeated multiplicities reuse one
    /// decoded score tree.
    pub(crate) fn append_family_scores<T: ParBagComponent>(&self, out: &mut Vec<Tree<ScoreAtom>>) {
        self.components
            .visit_relative_prefix(&component_prefix(T::TAG), |suffix, multiplicity| {
                let (score_bytes, _payload) = split_key_suffix(T::TAG, suffix)
                    .expect("ParBag invariant: every component has a valid key header");
                let (score, extent) = decode_order_key(score_bytes)
                    .expect("ParBag invariant: every component has a canonical score prefix");
                debug_assert_eq!(
                    extent,
                    score_bytes.len(),
                    "score decoder must consume the declared complete prefix"
                );
                for _ in 1..multiplicity.get() {
                    out.push(score.clone());
                }
                out.push(score);
            });
    }

    /// Append one family's already-encoded score prefixes in canonical trie
    /// order. Multiplicity is expanded exactly as the legacy score vector was,
    /// but no component protobuf body is decoded and no score tree is built.
    fn append_family_score_keys<T: ParBagComponent>(&self, out: &mut Vec<u8>) {
        self.components
            .visit_relative_prefix(&component_prefix(T::TAG), |suffix, multiplicity| {
                let (score, _payload) = split_key_suffix(T::TAG, suffix)
                    .expect("ParBag invariant: every component has a valid key header");
                for _ in 0..multiplicity.get() {
                    out.extend_from_slice(score);
                }
            });
    }

    fn score_components_snapshot(&self) -> &[u8] {
        self.aux
            .score_components
            .get_or_init(|| {
                let mut out = Vec::new();
                self.append_family_score_keys::<Send>(&mut out);
                self.append_family_score_keys::<Receive>(&mut out);
                self.append_family_score_keys::<Expr>(&mut out);
                self.append_family_score_keys::<New>(&mut out);
                self.append_family_score_keys::<Match>(&mut out);
                self.append_family_score_keys::<Bundle>(&mut out);
                self.append_family_score_keys::<Connective>(&mut out);
                self.append_family_score_keys::<GUnforgeable>(&mut out);
                self.append_family_score_keys::<If>(&mut out);
                Arc::from(out)
            })
            .as_ref()
    }

    /// Append the complete canonical score key of the owning `Par` directly
    /// from its authoritative component paths. This is byte-identical to
    /// `encode_order_key(Par::canonical_score())`, without its recursive score
    /// tree and per-node vector allocations.
    pub(crate) fn append_par_score_key(&self, connective_used: bool, out: &mut Vec<u8>) {
        begin_score_node(out, Score::PAR);
        out.extend_from_slice(self.score_components_snapshot());
        append_int_score_atom(out, i64::from(connective_used));
        end_score_node(out);
    }

    /// Decode a constructor family lazily, one trie endpoint at a time.
    pub fn try_visit_decoded<T, E>(
        &self,
        mut visitor: impl FnMut(T, Multiplicity) -> Result<(), E>,
    ) -> Result<(), ParBagVisitError<E>>
    where
        T: ParBagComponent,
    {
        self.components.try_visit_relative_prefix(
            &component_prefix(T::TAG),
            |suffix, multiplicity| {
                let (_score, payload) =
                    split_key_suffix(T::TAG, suffix).map_err(ParBagVisitError::Codec)?;
                let value = decode_exact::<T>(payload).map_err(ParBagVisitError::Codec)?;
                visitor(value, multiplicity).map_err(ParBagVisitError::Visitor)
            },
        )
    }

    /// Visit every semantic occurrence in one constructor family without
    /// constructing a repeated-field collection. Each distinct trie endpoint
    /// is decoded once and borrowed for exactly its stored multiplicity.
    pub fn try_visit_occurrences<T, E>(
        &self,
        mut visitor: impl FnMut(&T) -> Result<(), E>,
    ) -> Result<(), ParBagVisitError<E>>
    where
        T: ParBagComponent,
    {
        self.try_visit_decoded::<T, E>(|value, multiplicity| {
            for _ in 0..multiplicity.get() {
                visitor(&value)?;
            }
            Ok(())
        })
    }

    /// Decode only the lexicographically first endpoint in one constructor
    /// family. The zipper traversal stops immediately after that endpoint;
    /// it does not scan or collect the remaining family.
    pub fn first<T: ParBagComponent>(&self) -> Result<Option<(T, Multiplicity)>, ParBagCodecError> {
        enum Stop {
            Found,
            Codec(ParBagCodecError),
        }

        let mut first = None;
        let result = self.components.try_visit_relative_prefix(
            &component_prefix(T::TAG),
            |suffix, multiplicity| {
                let (_score, payload) = split_key_suffix(T::TAG, suffix).map_err(Stop::Codec)?;
                let value = decode_exact::<T>(payload).map_err(Stop::Codec)?;
                first = Some((value, multiplicity));
                Err(Stop::Found)
            },
        );
        match result {
            Ok(()) | Err(Stop::Found) => Ok(first),
            Err(Stop::Codec(error)) => Err(error),
        }
    }

    pub fn sum(&self, other: &Self) -> Result<Self, BagError> {
        self.components
            .sum(&other.components)
            .map(Self::from_components)
    }

    pub fn scale(&self, factor: u64) -> Result<Self, BagError> {
        match factor {
            0 => return Ok(Self::new()),
            1 => return Ok(self.clone()),
            _ => {}
        }
        let components = self.components.scale(factor)?;
        let mut metrics = self.aux.metrics;
        for exact in &mut metrics.exact_cardinalities {
            *exact = exact
                .checked_mul(u128::from(factor))
                .expect("scaled resident endpoint multiplicities fit exact u128 cardinality");
        }
        for exact in &mut metrics.exact_protobuf_lengths {
            *exact = exact.and_then(|length| length.checked_mul(u128::from(factor)));
        }
        Ok(Self {
            components,
            aux: Arc::new(ParBagAux::new(metrics)),
        })
    }

    pub fn join(&self, other: &Self) -> Self {
        Self::from_components(self.components.join(&other.components))
    }

    pub fn meet(&self, other: &Self) -> Self {
        Self::from_components(self.components.meet(&other.components))
    }

    pub fn subtract(&self, other: &Self) -> Self {
        Self::from_components(self.components.subtract(&other.components))
    }

    pub fn is_subbag_of(&self, other: &Self) -> bool {
        self.components.is_subbag_of(&other.components)
    }

    fn from_components(components: Bag) -> Self {
        let metrics = ParFamilyMetrics::from_trusted_components(&components);
        Self {
            components,
            aux: Arc::new(ParBagAux::new(metrics)),
        }
    }

    /// Borrow the memoized canonical `ACTree03` image directly. The first
    /// access materializes PathMap's compact arena once; subsequent encoders
    /// and CoW clones share the immutable byte array.
    pub fn compact_snapshot(&self) -> &[u8] {
        self.aux
            .compact_snapshot
            .get_or_init(|| Arc::from(self.components.compact_bytes()))
            .as_ref()
    }

    /// Owned convenience image: no entry vector or decoded shadow.
    pub fn compact_bytes(&self) -> Vec<u8> { self.compact_snapshot().to_vec() }

    pub fn from_compact_bytes(bytes: &[u8]) -> Result<Self, ParBagCodecError> {
        let components = Bag::from_compact_bytes(bytes)?;
        let mut bag = Self::try_from_bag(components)?;
        Arc::get_mut(&mut bag.aux)
            .expect("newly decoded ParBag uniquely owns its auxiliary state")
            .compact_snapshot
            .set(Arc::from(bytes))
            .expect("newly decoded ParBag has no compact snapshot");
        Ok(bag)
    }

    /// Exact length of the legacy repeated-message protobuf component fields.
    /// Root metadata fields 9 and 10 are owned by the eventual repository
    /// `Par` wrapper and are deliberately outside this component Bag.
    pub fn protobuf_components_encoded_len(&self) -> Result<usize, ParBagProtobufError> {
        let mut total = 0usize;
        self.add_protobuf_components_before_root_metadata_len(&mut total)?;
        self.add_protobuf_components_after_root_metadata_len(&mut total)?;
        Ok(total)
    }

    pub(crate) fn add_protobuf_components_before_root_metadata_len(
        &self,
        total: &mut usize,
    ) -> Result<(), ParBagProtobufError> {
        self.add_protobuf_family_len::<Send>(total)?;
        self.add_protobuf_family_len::<Receive>(total)?;
        self.add_protobuf_family_len::<New>(total)?;
        self.add_protobuf_family_len::<Expr>(total)?;
        self.add_protobuf_family_len::<Match>(total)?;
        self.add_protobuf_family_len::<GUnforgeable>(total)?;
        self.add_protobuf_family_len::<Connective>(total)?;
        Ok(())
    }

    pub(crate) fn add_protobuf_components_after_root_metadata_len(
        &self,
        total: &mut usize,
    ) -> Result<(), ParBagProtobufError> {
        self.add_protobuf_family_len::<Bundle>(total)?;
        self.add_protobuf_family_len::<If>(total)?;
        Ok(())
    }

    /// Stream protobuf fields from trie endpoints directly into a caller's
    /// output buffer in canonical tag/key order.
    ///
    /// The canonical protobuf body is copied directly `Multiplicity` times;
    /// no component decode, repeated-field `Vec`, legacy `Par`, intermediate
    /// byte buffer, or element sort is involved.
    pub fn encode_protobuf_components(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError> {
        self.encode_protobuf_components_before_root_metadata(output)?;
        self.encode_protobuf_components_after_root_metadata(output)?;
        Ok(())
    }

    pub(crate) fn encode_protobuf_components_before_root_metadata(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError> {
        output.put_slice(self.protobuf_before_root_snapshot());
        Ok(())
    }

    pub(crate) fn encode_protobuf_components_after_root_metadata(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError> {
        output.put_slice(self.protobuf_after_root_snapshot());
        Ok(())
    }

    fn protobuf_before_root_snapshot(&self) -> &[u8] {
        self.aux
            .protobuf_before_root
            .get_or_init(|| {
                let exact = [
                    ParComponentTag::Send,
                    ParComponentTag::Receive,
                    ParComponentTag::New,
                    ParComponentTag::Expr,
                    ParComponentTag::Match,
                    ParComponentTag::GUnforgeable,
                    ParComponentTag::Connective,
                ]
                .into_iter()
                .try_fold(0u128, |total, tag| {
                    self.aux.metrics.exact_protobuf_lengths[tag.index()]?.checked_add(total)
                });
                let capacity = exact
                    .and_then(|length| usize::try_from(length).ok())
                    .unwrap_or(0);
                let mut out = Vec::with_capacity(capacity);
                self.encode_protobuf_family::<Send>(&mut out)
                    .expect("resident Send protobuf lengths are representable");
                self.encode_protobuf_family::<Receive>(&mut out)
                    .expect("resident Receive protobuf lengths are representable");
                self.encode_protobuf_family::<New>(&mut out)
                    .expect("resident New protobuf lengths are representable");
                self.encode_protobuf_family::<Expr>(&mut out)
                    .expect("resident Expr protobuf lengths are representable");
                self.encode_protobuf_family::<Match>(&mut out)
                    .expect("resident Match protobuf lengths are representable");
                self.encode_protobuf_family::<GUnforgeable>(&mut out)
                    .expect("resident GUnforgeable protobuf lengths are representable");
                self.encode_protobuf_family::<Connective>(&mut out)
                    .expect("resident Connective protobuf lengths are representable");
                Arc::from(out)
            })
            .as_ref()
    }

    fn protobuf_after_root_snapshot(&self) -> &[u8] {
        self.aux
            .protobuf_after_root
            .get_or_init(|| {
                let exact = [ParComponentTag::Bundle, ParComponentTag::If]
                    .into_iter()
                    .try_fold(0u128, |total, tag| {
                        self.aux.metrics.exact_protobuf_lengths[tag.index()]?.checked_add(total)
                    });
                let capacity = exact
                    .and_then(|length| usize::try_from(length).ok())
                    .unwrap_or(0);
                let mut out = Vec::with_capacity(capacity);
                self.encode_protobuf_family::<Bundle>(&mut out)
                    .expect("resident Bundle protobuf lengths are representable");
                self.encode_protobuf_family::<If>(&mut out)
                    .expect("resident If protobuf lengths are representable");
                Arc::from(out)
            })
            .as_ref()
    }

    /// Collect the direct protobuf stream into its final `Vec<u8>` payload.
    ///
    /// This convenience is intended for complete-message boundaries and
    /// tests. Streaming serializers should call
    /// [`Self::encode_protobuf_components`] so no intermediate output buffer is
    /// allocated.
    pub fn protobuf_components_bytes(&self) -> Result<Vec<u8>, ParBagProtobufError> {
        let encoded_len = self.protobuf_components_encoded_len()?;
        let mut output = Vec::with_capacity(encoded_len);
        self.encode_protobuf_components(&mut output)?;
        debug_assert_eq!(output.len(), encoded_len);
        Ok(output)
    }

    fn add_protobuf_family_len<T>(&self, total: &mut usize) -> Result<(), ParBagProtobufError>
    where T: ParBagComponent {
        let index = T::TAG.index();
        let exact = self.aux.metrics.exact_protobuf_lengths[index];
        let exact_cardinality = self.aux.metrics.exact_cardinalities[index];
        let one_occurrence_bytes = match (
            exact,
            self.aux.metrics.distinct_lengths[index],
            exact_cardinality,
        ) {
            (Some(length), 1, cardinality) if cardinality != 0 => {
                usize::try_from(length / cardinality).unwrap_or(usize::MAX)
            }
            _ => 0,
        };
        let overflow = || ParBagProtobufError::EncodedLengthOverflow {
            tag: T::TAG,
            multiplicity: project_cardinality(exact_cardinality),
            one_occurrence_bytes,
        };
        let family_len = usize::try_from(exact.ok_or_else(overflow)?).map_err(|_| overflow())?;
        *total = total.checked_add(family_len).ok_or_else(overflow)?;
        Ok(())
    }

    fn encode_protobuf_family<T>(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError>
    where
        T: ParBagComponent,
    {
        self.visit_encoded::<T>(|entry| {
            let payload = entry.payload();
            let multiplicity = entry.multiplicity();
            for _ in 0..multiplicity.get() {
                encode_key(T::TAG.byte().into(), WireType::LengthDelimited, output);
                prost::encoding::encode_varint(payload.len() as u64, output);
                output.put_slice(payload);
            }
        });
        Ok(())
    }
}

impl Serialize for ParBag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_bytes(self.compact_snapshot())
    }
}

impl<'de> Deserialize<'de> for ParBag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let components = Bag::deserialize(deserializer)?;
        Self::try_from_bag(components).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod fast_key_tests {
    use super::*;
    use crate::rhoapi::{
        Bundle, Connective, ConnectiveBody, EAnd, EDiv, EEq, EGt, EGte, EList, ELt, ELte, EMap,
        EMatches, EMethod, EMinus, EMinusMinus, EMod, EMult, ENeg, ENeq, ENot, EOr, EPathMap,
        EPercentPercent, EPlus, EPlusPlus, ESet, ETuple, EVar, EZipper, GBigRational, GDeployId,
        GDeployerId, GFixedPoint, GPrivate, GSysAuthToken, If, Match, New, Par, Receive, Send, Var,
        VarRef,
    };

    fn generic_key<T: ParBagComponent>(value: &T) -> Vec<u8> {
        let scored = T::canonicalize(value);
        let mut key = component_prefix(T::TAG).to_vec();
        let score_start = key.len();
        encode_order_key(&scored.score, &mut key);
        let score_len = u64::try_from(key.len() - score_start).expect("test score fits u64");
        protobuf_encoder::encode_into(&scored.term, &mut key);
        key.extend_from_slice(&score_len.to_be_bytes());
        key
    }

    #[test]
    fn par_bag_aux_size_including_the_rmp2_digest_has_an_explicit_budget() {
        let measured = std::mem::size_of::<ParBagAux>();
        assert!(
            measured <= 672,
            "ParBagAux measured {measured} bytes, exceeding the reviewed per-CoW-root cache budget"
        );
    }

    #[test]
    fn every_leaf_expr_fast_key_is_byte_identical_to_the_general_sorter() {
        let values = [
            Expr::default(),
            Expr {
                expr_instance: Some(ExprInstance::GBool(true)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GBool(false)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(i64::MIN)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("a\0b".into())),
            },
            Expr {
                expr_instance: Some(ExprInstance::GUri("rho:id".into())),
            },
            Expr {
                expr_instance: Some(ExprInstance::GByteArray(vec![0, 1, 0xff])),
            },
            Expr {
                expr_instance: Some(ExprInstance::GDouble((-0.0f64).to_bits())),
            },
            Expr {
                expr_instance: Some(ExprInstance::GBigInt(vec![0x80, 0])),
            },
            Expr {
                expr_instance: Some(ExprInstance::GBigRat(GBigRational {
                    numerator: vec![0xff],
                    denominator: vec![0, 2],
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::GFixedPoint(GFixedPoint {
                    unscaled: vec![1, 0],
                    scale: u32::MAX,
                })),
            },
        ];

        for value in values {
            assert_eq!(
                with_component_key(&value, <[u8]>::to_vec),
                generic_key(&value),
                "{value:?}"
            );
        }
    }

    #[test]
    fn every_unforgeable_fast_key_is_byte_identical_to_the_general_sorter() {
        let values = [
            GUnforgeable::default(),
            GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![0, 1] })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: vec![2, 0] })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GDeployerIdBody(GDeployerId {
                    public_key: vec![3, 0],
                })),
            },
            GUnforgeable {
                unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
            },
        ];

        for value in values {
            assert_eq!(
                with_component_key(&value, <[u8]>::to_vec),
                generic_key(&value),
                "{value:?}"
            );
        }
    }

    #[test]
    fn ordered_collection_fast_keys_are_byte_identical_to_the_general_sorter() {
        let child = |value| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }])
        };
        let values = [
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![child(3), child(1), child(2)],
                    locally_free: vec![0x81],
                    connective_used: true,
                    remainder: None,
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![child(5)],
                    locally_free: vec![],
                    connective_used: false,
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(7)),
                    }),
                })),
            },
            Expr {
                expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                    ps: vec![child(9), child(-1)],
                    locally_free: vec![0x82],
                    connective_used: true,
                })),
            },
        ];

        for value in values {
            assert_eq!(
                with_component_key(&value, <[u8]>::to_vec),
                generic_key(&value),
                "{value:?}"
            );
        }
    }

    #[test]
    fn score_length_trailer_preserves_legacy_component_order() {
        let mut values = vec![
            Expr::default(),
            Expr {
                expr_instance: Some(ExprInstance::GInt(-1)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("short".into())),
            },
            Expr {
                expr_instance: Some(ExprInstance::GString("a\0much-longer-value".into())),
            },
            Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![Par::default().with_exprs(vec![Expr {
                        expr_instance: Some(ExprInstance::GInt(7)),
                    }])],
                    ..Default::default()
                })),
            },
        ];
        let mut expected: Vec<_> = values
            .iter()
            .map(<Expr as ParBagComponent>::canonicalize)
            .collect();
        ScoredTerm::sort_vec(&mut expected);
        let expected: Vec<_> = expected.into_iter().map(|scored| scored.term).collect();

        values.sort_by(|left, right| {
            with_component_key(left, |left_key| {
                with_component_key(right, |right_key| left_key.cmp(right_key))
            })
        });
        assert_eq!(values, expected);
    }

    #[test]
    fn par_score_key_is_byte_identical_to_the_general_score_tree() {
        let mut value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: vec![Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::GString("nested".into())),
                }])],
                ..Default::default()
            })),
        }]);
        value.replace_connective_used(true);

        let mut direct = Vec::new();
        value
            .components()
            .append_par_score_key(value.connective_used(), &mut direct);
        let mut expected = Vec::new();
        encode_order_key(&value.canonical_score(), &mut expected);
        assert_eq!(direct, expected);
    }

    fn unary_elist_expr(depth: usize, terminal: i64) -> Expr {
        let mut current = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(terminal)),
        }]);
        for _ in 0..depth {
            current = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![current],
                    ..EList::default()
                })),
            }]);
        }
        current
            .decode_occurrences::<Expr>()
            .into_iter()
            .next()
            .expect("the outer Par contains exactly one Expr")
    }

    fn unary_etuple_expr(depth: usize, terminal: i64) -> Expr {
        let mut current = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(terminal)),
        }]);
        for _ in 0..depth {
            current = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ETupleBody(crate::rhoapi::ETuple {
                    ps: vec![current],
                    ..crate::rhoapi::ETuple::default()
                })),
            }]);
        }
        current
            .decode_occurrences::<Expr>()
            .into_iter()
            .next()
            .expect("the outer Par contains exactly one Expr")
    }

    fn unary_enot_expr_with_terminal(depth: usize, terminal: Par) -> Expr {
        assert_ne!(depth, 0, "a unary ENot spine contains at least one arm");
        let mut current = terminal;
        for _ in 0..depth {
            current = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(crate::rhoapi::ENot {
                    p: Some(current),
                })),
            }]);
        }
        current
            .decode_occurrences::<Expr>()
            .into_iter()
            .next()
            .expect("the outer Par contains exactly one Expr")
    }

    fn unary_enot_expr(depth: usize) -> Expr {
        unary_enot_expr_with_terminal(depth, Par::default())
    }

    #[test]
    fn byte_native_unary_elist_validator_accepts_its_exact_canonical_key() {
        for terminal in [i64::MIN, -1, 0, 1, i64::MAX] {
            let value = unary_elist_expr(64, terminal);
            with_component_key(&value, |key| {
                let (tag, score, payload) = split_key_parts(key).expect("canonical key");
                assert_eq!(tag, ParComponentTag::Expr);
                assert_eq!(validate_unary_elist_key(score, payload), Some(true));
            });
        }
    }

    #[test]
    fn byte_native_unary_elist_validator_rejects_a_false_score_claim() {
        let value = unary_elist_expr(8, 0);
        with_component_key(&value, |key| {
            let (_tag, score, payload) = split_key_parts(key).expect("canonical key");
            let mut corrupt_score = score.to_vec();
            corrupt_score[2] ^= 1;
            assert_eq!(
                validate_unary_elist_key(&corrupt_score, payload),
                Some(false)
            );
        });
    }

    #[test]
    fn byte_native_unary_elist_validator_delegates_every_other_expr_shape() {
        let value = Expr {
            expr_instance: Some(ExprInstance::GString("not-an-integer-spine".to_owned())),
        };
        with_component_key(&value, |key| {
            let (_tag, score, payload) = split_key_parts(key).expect("canonical key");
            assert_eq!(validate_unary_elist_key(score, payload), None);
        });
    }

    #[test]
    fn direct_unary_elist_import_is_byte_score_and_term_identical() {
        for depth in [0, 1, 32, 256] {
            for terminal in [i64::MIN, -1, 0, 1, i64::MAX] {
                let expected = Par::default().with_exprs(vec![unary_elist_expr(depth, terminal)]);
                let protobuf = protobuf_encoder::encode_to_vec(&expected);

                let from_protobuf = import_canonical_parbag_fast_path(&protobuf)
                    .expect("the canonical unary-list protobuf shape is imported");
                let direct = import_canonical_unary_elist_par(depth, terminal)
                    .expect("the scalar unary-list parameters are representable");

                assert_eq!(from_protobuf, expected);
                assert_eq!(direct, expected);
                assert_eq!(protobuf_encoder::encode_to_vec(&from_protobuf), protobuf);
                assert_eq!(protobuf_encoder::encode_to_vec(&direct), protobuf);
                assert_eq!(
                    direct.components().cached_canonical_path_stability(),
                    Some(true)
                );
            }
        }
    }

    #[test]
    fn bulk_unary_elist_wraps_an_arbitrary_terminal_byte_identically() {
        for depth in [0, 1, 4, 32] {
            let terminal = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("terminal".to_owned())),
            }]);
            let mut expected = terminal.clone();
            for _ in 0..depth {
                expected = Par::default().with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EListBody(EList {
                        ps: vec![expected],
                        ..EList::default()
                    })),
                }]);
            }

            let direct = Par::wrap_unary_elist(depth, terminal)
                .expect("the requested unary-list depth is representable");
            assert_eq!(direct, expected);
            assert_eq!(
                protobuf_encoder::encode_to_vec(&direct),
                protobuf_encoder::encode_to_vec(&expected)
            );
        }
    }

    #[test]
    fn direct_unary_etuple_import_is_byte_score_and_term_identical() {
        for depth in [1, 32, 256] {
            for terminal in [i64::MIN, -1, 0, 1, i64::MAX] {
                let value = unary_etuple_expr(depth, terminal);
                with_component_key(&value, |key| {
                    let (tag, score, payload) = split_key_parts(key).expect("canonical tuple key");
                    assert_eq!(tag, ParComponentTag::Expr);
                    assert_eq!(validate_unary_etuple_key(score, payload), Some(true));
                });
                let expected = Par::default().with_exprs(vec![value]);
                let protobuf = protobuf_encoder::encode_to_vec(&expected);

                let from_protobuf = import_canonical_parbag_fast_path(&protobuf)
                    .expect("the canonical unary-tuple protobuf shape is imported");
                let direct = import_canonical_unary_etuple_par(depth, terminal)
                    .expect("the scalar unary-tuple parameters are representable");

                assert_eq!(from_protobuf, expected);
                assert_eq!(direct, expected);
                assert_eq!(protobuf_encoder::encode_to_vec(&from_protobuf), protobuf);
                assert_eq!(protobuf_encoder::encode_to_vec(&direct), protobuf);
                assert_eq!(
                    direct.components().cached_canonical_path_stability(),
                    Some(true)
                );
            }
        }
    }

    #[test]
    fn direct_unary_enot_import_is_byte_score_and_term_identical() {
        for depth in [1, 32, 256] {
            let value = unary_enot_expr(depth);
            with_component_key(&value, |key| {
                let (tag, score, payload) = split_key_parts(key).expect("canonical ENot key");
                assert_eq!(tag, ParComponentTag::Expr);
                assert_eq!(validate_unary_enot_key(score, payload), Some(true));
            });
            let expected = Par::default().with_exprs(vec![value]);
            let protobuf = protobuf_encoder::encode_to_vec(&expected);
            let imported = import_canonical_parbag_fast_path(&protobuf)
                .expect("the canonical unary-ENot protobuf shape is imported");

            assert_eq!(imported, expected);
            assert_eq!(protobuf_encoder::encode_to_vec(&imported), protobuf);
            assert_eq!(
                imported.components().cached_canonical_path_stability(),
                Some(false)
            );
        }
    }

    #[test]
    fn byte_native_unary_enot_parts_preserve_arbitrary_terminals() {
        use crate::rhoapi::EMethod;

        let terminals = [
            Par::default(),
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GBool(true)),
            }]),
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            }]),
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EMethodBody(EMethod {
                    method_name: "nth".to_owned(),
                    target: Some(Par::default()),
                    arguments: Vec::new(),
                    locally_free: Vec::new(),
                    connective_used: false,
                })),
            }]),
            Par::default().with_exprs(vec![
                Expr {
                    expr_instance: Some(ExprInstance::GInt(1)),
                },
                Expr {
                    expr_instance: Some(ExprInstance::GString("two".to_owned())),
                },
            ]),
        ];

        for depth in [1, 2, 32] {
            for terminal in &terminals {
                let value = unary_enot_expr_with_terminal(depth, terminal.clone());
                let terminal_bytes = protobuf_encoder::encode_to_vec(terminal);
                with_component_key(&value, |key| {
                    let (tag, _score, payload) = split_key_parts(key).expect("canonical ENot key");
                    assert_eq!(tag, ParComponentTag::Expr);
                    let (observed_depth, observed_terminal) =
                        canonical_unary_enot_parts(payload).expect("exact ENot spine");
                    assert_eq!(observed_depth, depth);
                    assert_eq!(observed_terminal, terminal_bytes);
                    assert_eq!(
                        canonical_unary_enot(payload),
                        terminal.is_empty().then_some(depth)
                    );
                });
            }
        }
    }

    #[test]
    fn byte_native_unary_enot_parts_reject_malformed_outer_frames() {
        for malformed in [
            vec![0x10, 0x02],
            vec![0x2a, 0x00],
            vec![0x28, 0x00],
            vec![0x2a, 0x02, 0x0a],
            vec![0xaa, 0x00, 0x00],
            vec![0x2a, 0x80, 0x00],
            vec![0x2a, 0x02, 0x0a, 0x00, 0x2a, 0x00],
        ] {
            assert!(canonical_unary_enot_parts(&malformed).is_none());
        }
    }

    #[test]
    fn byte_native_unary_enot_parts_stop_before_a_malformed_nested_enot() {
        let malformed_nested_expr = [0x2a, 0x00];
        let terminal_par = [0x2a, 0x02, 0x2a, 0x00];
        let outer = [0x2a, 0x06, 0x0a, 0x04, 0x2a, 0x02, 0x2a, 0x00];
        let (depth, terminal) = canonical_unary_enot_parts(&outer).expect("valid outer ENot");
        assert_eq!(depth, 1);
        assert_eq!(terminal, terminal_par);
        assert_eq!(&terminal[2..], malformed_nested_expr);
        assert_eq!(canonical_unary_enot(&outer), None);
    }

    #[test]
    fn canonical_connective_cursor_lends_every_child_without_projection() {
        use crate::rhoapi::connective::ConnectiveInstance::{ConnAndBody, ConnOrBody};

        let children = vec![
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(1)),
            }]),
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("two".to_owned())),
            }]),
            Par::default(),
        ];
        let expected = children
            .iter()
            .map(protobuf_encoder::encode_to_vec)
            .collect::<Vec<_>>();

        for instance in [
            ConnAndBody(ConnectiveBody {
                ps: children.clone(),
            }),
            ConnOrBody(ConnectiveBody {
                ps: children.clone(),
            }),
        ] {
            let bytes = protobuf_encoder::encode_to_vec(&Connective {
                connective_instance: Some(instance),
            });
            let cursor = match canonical_connective_parts(&bytes).expect("canonical connective") {
                CanonicalConnectiveParts::And(children)
                | CanonicalConnectiveParts::Or(children) => children,
                other => panic!("expected AND/OR cursor, got {other:?}"),
            };
            assert_eq!(cursor.len(), expected.len());
            assert_eq!(cursor.collect::<Vec<_>>(), expected);
        }

        for instance in [
            ConnAndBody(ConnectiveBody { ps: Vec::new() }),
            ConnOrBody(ConnectiveBody { ps: Vec::new() }),
        ] {
            let bytes = protobuf_encoder::encode_to_vec(&Connective {
                connective_instance: Some(instance),
            });
            let cursor = match canonical_connective_parts(&bytes).expect("empty connective body") {
                CanonicalConnectiveParts::And(children)
                | CanonicalConnectiveParts::Or(children) => children,
                other => panic!("expected empty AND/OR cursor, got {other:?}"),
            };
            assert!(cursor.is_empty());
        }
    }

    #[test]
    fn canonical_connective_cursor_covers_nonbranching_arms_and_rejects_bad_frames() {
        use crate::rhoapi::connective::ConnectiveInstance::{ConnBool, ConnNotBody, VarRefBody};

        let terminal = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let terminal_bytes = protobuf_encoder::encode_to_vec(&terminal);
        let not = protobuf_encoder::encode_to_vec(&Connective {
            connective_instance: Some(ConnNotBody(terminal)),
        });
        assert_eq!(
            canonical_connective_parts(&not),
            Some(CanonicalConnectiveParts::Not(&terminal_bytes))
        );

        for value in [false, true] {
            let bytes = protobuf_encoder::encode_to_vec(&Connective {
                connective_instance: Some(ConnBool(value)),
            });
            assert_eq!(
                canonical_connective_parts(&bytes),
                Some(CanonicalConnectiveParts::Predicate { tag: 5, value })
            );
        }

        let var_ref = protobuf_encoder::encode_to_vec(&Connective {
            connective_instance: Some(VarRefBody(VarRef {
                index: -3,
                depth: 5,
            })),
        });
        assert_eq!(
            canonical_connective_parts(&var_ref),
            Some(CanonicalConnectiveParts::VarRef)
        );
        assert_eq!(
            canonical_connective_parts(&[]),
            Some(CanonicalConnectiveParts::Absent)
        );

        for malformed in [
            vec![0x0a, 0x02, 0x0a],
            vec![0x0a, 0x02, 0x12, 0x00],
            vec![0x0a, 0x80, 0x00],
            vec![0x28, 0x02],
            vec![0x22, 0x02, 0x08],
            vec![0x28, 0x01, 0x30, 0x01],
        ] {
            assert!(
                canonical_connective_parts(&malformed).is_none(),
                "malformed connective must delegate: {malformed:02x?}"
            );
        }
    }

    #[test]
    fn canonical_exact_scalar_cursor_covers_every_ground_scalar_equality_image() {
        let scalars = [
            ExprInstance::GBool(false),
            ExprInstance::GBool(true),
            ExprInstance::GInt(i64::MIN),
            ExprInstance::GInt(i64::MAX),
            ExprInstance::GString(String::new()),
            ExprInstance::GString("lambda λ\0".to_owned()),
            ExprInstance::GUri(String::new()),
            ExprInstance::GUri("rho:id:λ".to_owned()),
            ExprInstance::GByteArray(Vec::new()),
            ExprInstance::GByteArray(vec![0, 1, 0xff]),
            ExprInstance::GDouble(0.0f64.to_bits()),
            ExprInstance::GDouble((-0.0f64).to_bits()),
            ExprInstance::GDouble(0x7ff8_0000_0000_0042),
            ExprInstance::GBigInt(Vec::new()),
            ExprInstance::GBigInt(vec![0x80, 0, 0xff]),
            ExprInstance::GBigRat(GBigRational::default()),
            ExprInstance::GBigRat(GBigRational {
                numerator: vec![0xff],
                denominator: vec![0, 2],
            }),
            ExprInstance::GFixedPoint(GFixedPoint::default()),
            ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: vec![1, 0],
                scale: u32::MAX,
            }),
        ];

        for scalar in scalars {
            let expr = Expr {
                expr_instance: Some(scalar),
            };
            let expr_bytes = protobuf_encoder::encode_to_vec(&expr);
            let par = Par::default()
                .with_exprs(vec![expr])
                .with_locally_free(vec![0x80, 0x01]);
            let par_bytes = protobuf_encoder::encode_to_vec(&par);
            let proven = canonical_exact_scalar_par(&par_bytes).expect("exact scalar Par");
            assert_eq!(proven.expr(), expr_bytes);
            assert!(!proven.connective_used());
            assert_eq!(
                canonical_exact_scalar_expr(&expr_bytes),
                Some(expr_bytes.as_slice())
            );

            let marked = par.with_connective_used(true);
            let marked_bytes = protobuf_encoder::encode_to_vec(&marked);
            assert!(canonical_exact_scalar_par(&marked_bytes)
                .expect("connective-used is an explicit proven root summary")
                .connective_used());
        }
    }

    #[test]
    fn canonical_exact_scalar_cursor_rejects_non_scalar_shape_and_malformed_payloads() {
        let scalar = Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        };
        for rejected in [
            Par::default(),
            Par::default().with_exprs(vec![Expr::default()]),
            Par::default().with_exprs(vec![scalar.clone(), scalar.clone()]),
            Par::default().with_sends(vec![Send::default()]),
        ] {
            assert!(
                canonical_exact_scalar_par(&protobuf_encoder::encode_to_vec(&rejected)).is_none()
            );
        }

        // These are exact outer oneof frames whose scalar payload is either
        // non-canonical or cannot inhabit the corresponding Rust value.
        let malformed = [
            vec![0x08, 0x02],                               // bool outside {0,1}
            vec![0x08, 0x80, 0x00],                         // overlong bool varint
            vec![0x1a, 0x01, 0xff],                         // invalid UTF-8 string
            vec![0x91, 0x02, 0, 0, 0, 0, 0, 0, 0],          // truncated fixed64
            vec![0xa2, 0x02, 0x02, 0x0a, 0x00],             // empty BigRat bytes field
            vec![0xa2, 0x02, 0x06, 0x0a, 1, 1, 0x0a, 1, 2], // duplicate BigRat field
            vec![0xa2, 0x02, 0x06, 0x12, 1, 2, 0x0a, 1, 1], // reordered BigRat fields
            vec![0xaa, 0x02, 0x02, 0x0a, 0x00],             // empty FixedPoint bytes field
            vec![0xaa, 0x02, 0x02, 0x10, 0x00],             // explicitly encoded default scale
            vec![0xaa, 0x02, 0x04, 0x10, 1, 0x10, 2],       // duplicate FixedPoint scale
        ];
        for bytes in malformed {
            assert!(
                canonical_exact_scalar_expr(&bytes).is_none(),
                "malformed scalar must delegate: {bytes:02x?}"
            );
        }
    }

    #[test]
    fn canonical_free_var_cursor_is_exact_and_constant_depth() {
        use crate::rhoapi::var::VarInstance;

        for level in [i32::MIN, -17, 0, 23, i32::MAX] {
            let expr = Expr {
                expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(level)),
                    }),
                })),
            };
            assert_eq!(
                canonical_exact_free_var_expr_level(&protobuf_encoder::encode_to_vec(&expr)),
                Some(level)
            );
            let data = Par::default().with_exprs(vec![expr.clone()]);
            let data_proof = canonical_exact_free_var_par(&protobuf_encoder::encode_to_vec(&data))
                .expect("a singleton free-variable Par is exact");
            assert_eq!(data_proof.level(), level);
            assert!(!data_proof.connective_used());

            let capture = data.with_connective_used(true);
            let capture_proof =
                canonical_exact_free_var_par(&protobuf_encoder::encode_to_vec(&capture))
                    .expect("connective-used is retained by the exact Par proof");
            assert_eq!(capture_proof.level(), level);
            assert!(capture_proof.connective_used());
        }

        for rejected in [
            VarInstance::BoundVar(7),
            VarInstance::Wildcard(crate::rhoapi::var::WildcardMsg {}),
        ] {
            let expr = Expr {
                expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                    v: Some(Var {
                        var_instance: Some(rejected),
                    }),
                })),
            };
            assert_eq!(
                canonical_exact_free_var_expr_level(&protobuf_encoder::encode_to_vec(&expr)),
                None
            );
            assert_eq!(
                canonical_exact_free_var_par(&protobuf_encoder::encode_to_vec(
                    &Par::default().with_exprs(vec![expr])
                )),
                None
            );
        }

        let free = Expr {
            expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(7)),
                }),
            })),
        };
        for rejected in [
            Par::default(),
            Par::default().with_exprs(vec![free.clone(), free.clone()]),
            Par::default()
                .with_exprs(vec![free])
                .with_sends(vec![Send::default()]),
        ] {
            assert_eq!(
                canonical_exact_free_var_par(&protobuf_encoder::encode_to_vec(&rejected)),
                None
            );
        }

        for malformed in [
            vec![],
            vec![0x9a, 0x01, 0x00],
            vec![0x9a, 0x01, 0x02, 0x0a],
            vec![0x9a, 0x01, 0x04, 0x0a, 0x02, 0x10, 0x80],
        ] {
            assert_eq!(canonical_exact_free_var_expr_level(&malformed), None);
        }
    }

    #[test]
    fn canonical_var_cursor_accepts_exact_encoder_images_only() {
        use crate::rhoapi::var::{VarInstance, WildcardMsg};

        let cases = [
            (Var::default(), CanonicalVar::Absent),
            (
                Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                },
                CanonicalVar::Bound(0),
            ),
            (
                Var {
                    var_instance: Some(VarInstance::BoundVar(i32::MIN)),
                },
                CanonicalVar::Bound(i32::MIN),
            ),
            (
                Var {
                    var_instance: Some(VarInstance::BoundVar(i32::MAX)),
                },
                CanonicalVar::Bound(i32::MAX),
            ),
            (
                Var {
                    var_instance: Some(VarInstance::FreeVar(-1)),
                },
                CanonicalVar::Free(-1),
            ),
            (
                Var {
                    var_instance: Some(VarInstance::FreeVar(i32::MAX)),
                },
                CanonicalVar::Free(i32::MAX),
            ),
            (
                Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                },
                CanonicalVar::Wildcard,
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                canonical_var(&protobuf_encoder::encode_to_vec(&value)),
                Some(expected)
            );
        }

        let mut overflowing_sint32 = vec![0x08];
        prost::encoding::encode_varint(u64::from(u32::MAX) + 1, &mut overflowing_sint32);
        for malformed in [
            vec![0x08],                   // truncated scalar
            vec![0x08, 0x80, 0x00],       // non-minimal scalar
            vec![0x88, 0x00, 0x00],       // non-minimal key
            vec![0x08, 0x00, 0x10, 0x00], // duplicate oneof arms
            vec![0x20, 0x00],             // unknown field
            vec![0x1a, 0x01],             // truncated wildcard
            vec![0x1a, 0x01, 0x00],       // non-empty wildcard
            vec![0x1a, 0x00, 0x20, 0x00], // trailing field
            overflowing_sint32,
        ] {
            assert_eq!(canonical_var(&malformed), None, "accepted {malformed:02x?}");
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(4_096))]

        #[test]
        fn every_byte_string_accepted_by_the_free_var_cursor_is_its_exact_canonical_message(
            bytes in proptest::collection::vec(proptest::arbitrary::any::<u8>(), 0..64),
        ) {
            use crate::rhoapi::var::VarInstance;

            if let Some(level) = canonical_exact_free_var_expr_level(&bytes) {
                let expected = protobuf_encoder::encode_to_vec(&Expr {
                    expr_instance: Some(ExprInstance::EVarBody(EVar {
                        v: Some(Var {
                            var_instance: Some(VarInstance::FreeVar(level)),
                        }),
                    })),
                });
                proptest::prop_assert_eq!(bytes, expected);
            }
        }
    }

    #[test]
    fn concrete_locally_free_byte_proof_covers_every_expr_schema_arm() {
        use crate::rhoapi::var::{VarInstance, WildcardMsg};

        let proof = |expr_instance| {
            Par::default()
                .with_exprs(vec![Expr {
                    expr_instance: Some(expr_instance),
                }])
                .concrete_components_locally_free_are_empty()
        };
        let closed = Par::default();
        let open = closed.clone().with_locally_free(vec![1]);

        for scalar in [
            ExprInstance::GBool(true),
            ExprInstance::GInt(7),
            ExprInstance::GString("value".into()),
            ExprInstance::GUri("rho:test".into()),
            ExprInstance::GByteArray(vec![1, 2, 3]),
            ExprInstance::GDouble(7),
            ExprInstance::GBigInt(vec![1]),
            ExprInstance::GBigRat(GBigRational::default()),
            ExprInstance::GFixedPoint(GFixedPoint::default()),
        ] {
            assert_eq!(proof(scalar), Some(true));
        }

        for unary in [
            ExprInstance::ENotBody(ENot {
                p: Some(closed.clone()),
            }),
            ExprInstance::ENegBody(ENeg {
                p: Some(closed.clone()),
            }),
        ] {
            assert_eq!(proof(unary), Some(true));
        }
        for unary in [
            ExprInstance::ENotBody(ENot {
                p: Some(open.clone()),
            }),
            ExprInstance::ENegBody(ENeg {
                p: Some(open.clone()),
            }),
        ] {
            assert_eq!(proof(unary), Some(false));
        }

        let closed_binary = |instance| assert_eq!(proof(instance), Some(true));
        closed_binary(ExprInstance::EMultBody(EMult {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EDivBody(EDiv {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EPlusBody(EPlus {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EMinusBody(EMinus {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::ELtBody(ELt {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::ELteBody(ELte {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EGtBody(EGt {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EGteBody(EGte {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EEqBody(EEq {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::ENeqBody(ENeq {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EAndBody(EAnd {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EOrBody(EOr {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EPercentPercentBody(EPercentPercent {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EPlusPlusBody(EPlusPlus {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EMinusMinusBody(EMinusMinus {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));
        closed_binary(ExprInstance::EModBody(EMod {
            p1: Some(closed.clone()),
            p2: Some(closed.clone()),
        }));

        for binary in [
            ExprInstance::EMultBody(EMult {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EDivBody(EDiv {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EPlusBody(EPlus {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EMinusBody(EMinus {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::ELtBody(ELt {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::ELteBody(ELte {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EGtBody(EGt {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EGteBody(EGte {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EEqBody(EEq {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::ENeqBody(ENeq {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EAndBody(EAnd {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EOrBody(EOr {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EPercentPercentBody(EPercentPercent {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EPlusPlusBody(EPlusPlus {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EMinusMinusBody(EMinusMinus {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
            ExprInstance::EModBody(EMod {
                p1: Some(open.clone()),
                p2: Some(closed.clone()),
            }),
        ] {
            assert_eq!(proof(binary), Some(false));
        }

        assert_eq!(
            proof(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(0)),
                }),
            })),
            Some(false)
        );
        assert_eq!(
            proof(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(0)),
                }),
            })),
            Some(true)
        );
        assert_eq!(
            proof(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
                }),
            })),
            Some(true)
        );
        assert_eq!(proof(ExprInstance::EVarBody(EVar::default())), None);

        let mut pathmap = EPathMap::default();
        pathmap.locally_free = vec![1];
        for collection in [
            ExprInstance::EListBody(EList {
                locally_free: vec![1],
                ..EList::default()
            }),
            ExprInstance::ETupleBody(ETuple {
                locally_free: vec![1],
                ..ETuple::default()
            }),
            ExprInstance::ESetBody(ESet {
                locally_free: vec![1],
                ..ESet::default()
            }),
            ExprInstance::EMapBody(EMap {
                locally_free: vec![1],
                ..EMap::default()
            }),
            ExprInstance::EPathmapBody(pathmap),
            ExprInstance::EMethodBody(EMethod {
                locally_free: vec![1],
                ..EMethod::default()
            }),
            ExprInstance::EZipperBody(EZipper {
                locally_free: vec![1],
                ..EZipper::default()
            }),
        ] {
            assert_eq!(proof(collection), Some(false));
        }
        assert_eq!(proof(ExprInstance::EListBody(EList::default())), Some(true));
        assert_eq!(
            proof(ExprInstance::ETupleBody(ETuple::default())),
            Some(true)
        );
        assert_eq!(proof(ExprInstance::ESetBody(ESet::default())), Some(true));
        assert_eq!(proof(ExprInstance::EMapBody(EMap::default())), Some(true));
        assert_eq!(
            proof(ExprInstance::EPathmapBody(EPathMap::default())),
            Some(true)
        );
        assert_eq!(
            proof(ExprInstance::EMethodBody(EMethod::default())),
            Some(true)
        );
        assert_eq!(
            proof(ExprInstance::EZipperBody(EZipper::default())),
            Some(true)
        );
        assert_eq!(
            proof(ExprInstance::EMatchesBody(EMatches {
                target: Some(closed.clone()),
                pattern: Some(closed.clone()),
            })),
            Some(true)
        );
        assert_eq!(
            proof(ExprInstance::EMatchesBody(EMatches {
                target: Some(open),
                pattern: Some(closed),
            })),
            Some(false)
        );
    }

    #[test]
    fn concrete_locally_free_cache_uses_component_metadata_and_cow_invalidation() {
        use crate::rhoapi::var::VarInstance;

        let scalar = Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        };
        let closed = Par::default().with_exprs(vec![scalar.clone()]);
        assert_eq!(
            closed.concrete_components_locally_free_are_empty(),
            Some(true)
        );
        let closed_clone = closed.clone();
        assert!(
            closed
                .components()
                .shares_storage_with(closed_clone.components()),
            "the derived proof cache is shared by CoW clones"
        );
        assert_eq!(
            closed_clone.concrete_components_locally_free_are_empty(),
            Some(true)
        );

        let free = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(3)),
                }),
            })),
        }]);
        let bound = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(crate::rhoapi::EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(3)),
                }),
            })),
        }]);
        assert_eq!(
            free.concrete_components_locally_free_are_empty(),
            Some(true)
        );
        assert_eq!(
            bound.concrete_components_locally_free_are_empty(),
            Some(false)
        );

        let open_child = closed.clone().with_locally_free(vec![1]);
        let closed_enot = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(closed.clone()),
            })),
        }]);
        let open_enot = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(open_child.clone()),
            })),
        }]);
        assert_eq!(
            closed_enot.concrete_components_locally_free_are_empty(),
            Some(true)
        );
        assert_eq!(
            open_enot.concrete_components_locally_free_are_empty(),
            Some(false)
        );

        for open_component in [
            Par::default().with_sends(vec![Send {
                locally_free: vec![1],
                ..Send::default()
            }]),
            Par::default().with_receives(vec![Receive {
                locally_free: vec![1],
                ..Receive::default()
            }]),
            Par::default().with_news(vec![New {
                locally_free: vec![1],
                ..New::default()
            }]),
            Par::default().with_matches(vec![Match {
                locally_free: vec![1],
                ..Match::default()
            }]),
            Par::default().with_bundles(vec![Bundle {
                body: Some(open_child.clone()),
                ..Bundle::default()
            }]),
            Par::default().with_conditionals(vec![If {
                locally_free: vec![1],
                ..If::default()
            }]),
        ] {
            assert_eq!(
                open_component.concrete_components_locally_free_are_empty(),
                Some(false)
            );
        }

        let mut detached = closed.into_components();
        assert_eq!(
            detached.concrete_components_locally_free_are_empty(),
            Some(true)
        );
        detached
            .insert(&If {
                locally_free: vec![1],
                ..If::default()
            })
            .expect("insert invalidates the cached concrete fold");
        assert_eq!(
            detached.concrete_components_locally_free_are_empty(),
            Some(false)
        );

        let open_if = If {
            locally_free: vec![1],
            ..If::default()
        };
        let mut removed = Par::default()
            .with_conditionals(vec![open_if.clone()])
            .into_components();
        assert_eq!(
            removed.concrete_components_locally_free_are_empty(),
            Some(false)
        );
        assert!(removed.remove(&open_if).is_none());
        assert_eq!(
            removed.concrete_components_locally_free_are_empty(),
            Some(true)
        );

        let mut family_removed = Par::default()
            .with_conditionals(vec![open_if])
            .into_components();
        assert_eq!(
            family_removed.concrete_components_locally_free_are_empty(),
            Some(false)
        );
        assert_eq!(family_removed.remove_family::<If>(), (1, 1));
        assert_eq!(
            family_removed.concrete_components_locally_free_are_empty(),
            Some(true)
        );
    }

    #[test]
    fn concrete_locally_free_cache_is_concurrent_cow_shared_and_observationally_neutral() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let value = Par::default()
            .with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            }])
            .with_sends(vec![Send::default()]);
        let protobuf_before = protobuf_encoder::encode_to_vec(&value);
        let bincode_before = bincode::serialize(&value).expect("serialize uncached Par");
        let compact_before = value.components().compact_snapshot().to_vec();
        let mut hash_before = DefaultHasher::new();
        value.hash(&mut hash_before);
        let hash_before = hash_before.finish();
        let equal = value.clone();

        let shared = Arc::new(value.clone());
        let workers = (0..16)
            .map(|_| {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || {
                    assert_eq!(
                        shared.concrete_components_locally_free_are_empty(),
                        Some(true)
                    );
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("concurrent cache reader completes");
        }

        assert_eq!(
            protobuf_encoder::encode_to_vec(shared.as_ref()),
            protobuf_before
        );
        assert_eq!(
            bincode::serialize(shared.as_ref()).expect("serialize cached Par"),
            bincode_before
        );
        assert_eq!(shared.components().compact_snapshot(), compact_before);
        assert_eq!(shared.as_ref(), &equal);
        assert_eq!(shared.as_ref().cmp(&equal), Ordering::Equal);
        let mut hash_after = DefaultHasher::new();
        shared.as_ref().hash(&mut hash_after);
        assert_eq!(hash_after.finish(), hash_before);

        let detached = shared.as_ref().clone().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                locally_free: vec![1],
                ..EList::default()
            })),
        }]);
        assert_eq!(
            detached.concrete_components_locally_free_are_empty(),
            Some(false)
        );
        assert_eq!(
            shared.concrete_components_locally_free_are_empty(),
            Some(true)
        );
    }

    #[test]
    fn direct_singleton_eset_import_is_byte_and_term_identical() {
        use crate::rhoapi::ESet;

        for terminal in [i64::MIN, -1, 0, 1, i64::MAX] {
            let expected = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ESetBody(ESet {
                    ps: vec![Par::default().with_exprs(vec![unary_elist_expr(64, terminal)])],
                    ..ESet::default()
                })),
            }]);
            let protobuf = protobuf_encoder::encode_to_vec(&expected);
            let imported = import_canonical_singleton_eset_unary_elist_par(&protobuf)
                .expect("the exact canonical shape is imported");
            assert_eq!(imported, expected);
            assert_eq!(protobuf_encoder::encode_to_vec(&imported), protobuf);
            assert_eq!(
                imported.components().cached_canonical_path_stability(),
                Some(false)
            );
        }
    }

    #[test]
    fn direct_singleton_eset_import_rejects_noncanonical_or_other_shapes() {
        use crate::rhoapi::ESet;

        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![Par::default().with_exprs(vec![unary_elist_expr(8, 1)])],
                ..ESet::default()
            })),
        }]);
        let canonical = protobuf_encoder::encode_to_vec(&value);

        let mut trailing_unknown = canonical.clone();
        trailing_unknown.extend_from_slice(&[0x68, 0x00]);
        assert!(import_canonical_singleton_eset_unary_elist_par(&trailing_unknown).is_none());

        let scalar = protobuf_encoder::encode_to_vec(&Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }]));
        assert!(import_canonical_singleton_eset_unary_elist_par(&scalar).is_none());
    }

    #[test]
    fn direct_singleton_epathmap_chain_import_is_byte_and_term_identical() {
        use crate::rhoapi::EPathMap;

        let key = || {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("key".to_owned())),
            }])
        };
        let mut expected = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        for _ in 0..32 {
            expected = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    [(key(), expected)],
                    Vec::new(),
                    false,
                    None,
                ))),
            }]);
        }

        let protobuf = protobuf_encoder::encode_to_vec(&expected);
        let imported = import_canonical_parbag_fast_path(&protobuf)
            .expect("the exact stable EPathMap chain is imported");
        assert_eq!(imported, expected);
        assert_eq!(protobuf_encoder::encode_to_vec(&imported), protobuf);
        assert_eq!(
            imported.components().cached_canonical_path_stability(),
            Some(true)
        );
    }

    #[test]
    fn owned_canonical_epathmap_component_is_infallible_from_every_admission_lane() {
        let key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("key".to_owned())),
        }]);
        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let programmatic = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(key, value)],
                Vec::new(),
                false,
                None,
            ))),
        }]);
        let protobuf = protobuf_encoder::encode_to_vec(&programmatic);

        let compact = Par::from_parts(
            ParBag::from_compact_bytes(&programmatic.components().compact_bytes())
                .expect("compact ACT admission"),
            programmatic.locally_free().to_vec(),
            programmatic.connective_used(),
        );
        let mut construction = crate::rust::par_construction::ParConstruction::new();
        let planned = construction
            .import_par(&programmatic)
            .expect("planned admission");
        let planned = construction.finish(planned).expect("planned finish");
        let direct = import_canonical_parbag_fast_path(&protobuf).expect("fast protobuf admission");

        for (label, admitted) in [
            ("typed insertion", programmatic),
            ("compact ACT", compact),
            ("planned construction", planned),
            ("direct protobuf", direct),
        ] {
            let mut owned = None;
            admitted.visit_encoded::<Expr>(|entry| owned = Some(entry.to_owned_canonical()));
            let decoded = owned
                .unwrap_or_else(|| panic!("{label}: singleton Expr endpoint missing"))
                .decode()
                .unwrap_or_else(|error| panic!("{label}: deferred decode failed: {error}"));
            assert_eq!(
                protobuf_encoder::encode_to_vec(&decoded),
                protobuf_encoder::encode_to_vec(
                    &admitted
                        .first::<Expr>()
                        .expect("resident Expr key remains canonical")
                        .expect("one Expr")
                        .0
                ),
                "{label}"
            );
        }
    }

    #[test]
    fn epathmap_root_route_hint_distinguishes_mode_value_count_and_remainder() {
        let par = |value| {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }])
        };
        let encode_expr = |map| {
            protobuf_encoder::encode_to_vec(&Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(map)),
            })
        };

        let mut set = EPathMap::default();
        set.insert_entry(par(1));
        set.insert_entry(par(2));
        let encoded_set = encode_expr(set);
        let set_view =
            canonical_epathmap_root_view(&encoded_set).expect("canonical set Expr root view");
        let set_hint = set_view.route_hint();
        assert_eq!(
            set_hint.mode(),
            crate::rust::epathmap_trie_codec::EPathMapMode::Set
        );
        assert_eq!(set_hint.map_value_count(), 0);
        assert!(!set_hint.has_remainder());
        assert_eq!(
            set_view.protobuf_body(),
            canonical_expr_envelope(&encoded_set)
                .expect("canonical set Expr envelope")
                .payload
        );
        assert_eq!(
            set_view.snapshot(),
            canonical_epathmap_snapshot_from_expr(&encoded_set)
                .expect("canonical set EPM1 snapshot")
        );

        let map = EPathMap::new_map(
            [(par(1), par(11)), (par(2), par(22))],
            Vec::new(),
            true,
            Some(Var::default()),
        );
        let map_hint = canonical_epathmap_root_route_hint(&encode_expr(map))
            .expect("canonical map Expr route hint");
        assert_eq!(
            map_hint.mode(),
            crate::rust::epathmap_trie_codec::EPathMapMode::Map
        );
        assert_eq!(map_hint.map_value_count(), 2);
        assert!(map_hint.has_remainder());

        assert!(
            canonical_epathmap_root_route_hint(&protobuf_encoder::encode_to_vec(&Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            }))
            .is_none()
        );
    }

    #[test]
    fn direct_singleton_epathmap_chain_import_preserves_root_and_map_metadata() {
        use crate::rhoapi::{EPathMap, Var};

        let key = || {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("key".to_owned())),
            }])
        };
        let mut expected = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        for level in 0..8u8 {
            expected = Par::default()
                .with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                        [(key(), expected)],
                        vec![level + 1],
                        true,
                        Some(Var::default()),
                    ))),
                }])
                .with_locally_free(vec![level + 17])
                .with_connective_used(true);
        }

        let protobuf = protobuf_encoder::encode_to_vec(&expected);
        let imported = import_canonical_parbag_fast_path(&protobuf)
            .expect("the metadata-bearing stable EPathMap chain is imported");
        assert_eq!(imported, expected);
        assert_eq!(protobuf_encoder::encode_to_vec(&imported), protobuf);
        assert_eq!(imported.locally_free(), expected.locally_free());
        assert_eq!(imported.connective_used(), expected.connective_used());
        assert_eq!(
            imported.components().cached_canonical_path_stability(),
            Some(false),
            "component stability must include nested value-Par and EPathMap metadata"
        );
        assert_eq!(
            crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&imported),
            crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&expected)
        );
        assert_eq!(
            crate::rust::canonical_path::encode_trie_path(&imported),
            crate::rust::canonical_path::encode_trie_path(&expected),
            "fast import cannot make canonical paths construction-dependent"
        );

        let mut owned = None;
        imported.visit_encoded::<Expr>(|entry| {
            owned = Some(entry.to_owned_canonical());
        });
        let owned = owned.expect("the singleton Expr endpoint is retained after its zipper exits");
        assert_eq!(owned.multiplicity(), Multiplicity::ONE);
        let canonical_payload = owned.payload().to_vec();
        let decoded = owned
            .decode()
            .expect("the proof-carrying component uses trusted deferred decoding");
        assert_eq!(protobuf_encoder::encode_to_vec(&decoded), canonical_payload);

        let mut measured = None;
        imported.visit_encoded::<Expr>(|entry| {
            measured = entry
                .canonical_singleton_epathmap_map_parts()
                .map(|(depth, terminal)| (depth, terminal.to_vec()));
        });
        let (depth, terminal) = measured.expect("the borrowed chain cursor accepts metadata");
        assert_eq!(depth, 8);
        assert_eq!(
            canonical_exact_scalar_par(&terminal)
                .and_then(|scalar| canonical_expr_oneof_tag(scalar.expr())),
            Some(2)
        );
    }

    #[test]
    fn singleton_epathmap_fast_import_stability_fold_matches_semantic_authority() {
        use crate::rhoapi::Var;

        let key = || {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("key".to_owned())),
            }])
        };
        let wrap = |value: Par,
                    root_locally_free: Vec<u8>,
                    root_connective: bool,
                    map_locally_free: Vec<u8>,
                    map_connective: bool,
                    remainder: Option<Var>| {
            Par::default()
                .with_exprs(vec![Expr {
                    expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                        [(key(), value)],
                        map_locally_free,
                        map_connective,
                        remainder,
                    ))),
                }])
                .with_locally_free(root_locally_free)
                .with_connective_used(root_connective)
        };
        let terminal = || {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(7)),
            }])
        };
        let clean_inner = || wrap(terminal(), Vec::new(), false, Vec::new(), false, None);
        let cases = [
            (
                "clean",
                wrap(clean_inner(), Vec::new(), false, Vec::new(), false, None),
                true,
            ),
            (
                "top root locally-free does not belong to the component cache",
                wrap(clean_inner(), vec![1], false, Vec::new(), false, None),
                true,
            ),
            (
                "top root connective does not belong to the component cache",
                wrap(clean_inner(), Vec::new(), true, Vec::new(), false, None),
                true,
            ),
            (
                "top map locally-free",
                wrap(clean_inner(), Vec::new(), false, vec![1], false, None),
                false,
            ),
            (
                "top map connective",
                wrap(clean_inner(), Vec::new(), false, Vec::new(), true, None),
                false,
            ),
            (
                "top map remainder",
                wrap(
                    clean_inner(),
                    Vec::new(),
                    false,
                    Vec::new(),
                    false,
                    Some(Var::default()),
                ),
                false,
            ),
            (
                "nested value root metadata",
                wrap(
                    wrap(terminal(), vec![1], false, Vec::new(), false, None),
                    Vec::new(),
                    false,
                    Vec::new(),
                    false,
                    None,
                ),
                false,
            ),
            (
                "nested map metadata",
                wrap(
                    wrap(terminal(), Vec::new(), false, vec![1], false, None),
                    Vec::new(),
                    false,
                    Vec::new(),
                    false,
                    None,
                ),
                false,
            ),
        ];

        for (label, programmatic, component_stable) in cases {
            let protobuf = protobuf_encoder::encode_to_vec(&programmatic);
            let imported = import_canonical_parbag_fast_path(&protobuf)
                .unwrap_or_else(|| panic!("{label}: supported canonical chain was not imported"));
            assert_eq!(imported, programmatic, "{label}");
            assert_eq!(
                imported.components().cached_canonical_path_stability(),
                Some(component_stable),
                "{label}"
            );
            assert_eq!(
                crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&imported),
                crate::rust::pathmap_crate_type_mapper::eval_stable_par_for_test(&programmatic),
                "{label}"
            );
            assert_eq!(
                crate::rust::canonical_path::encode_trie_path(&imported),
                crate::rust::canonical_path::encode_trie_path(&programmatic),
                "{label}"
            );
        }
    }

    #[test]
    fn singleton_epathmap_cursors_reject_duplicate_fields_and_malformed_snapshots() {
        use crate::rhoapi::EPathMap;

        let key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("key".to_owned())),
        }]);
        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(key, value)],
                vec![1],
                true,
                None,
            ))),
        };
        let canonical_expr = protobuf_encoder::encode_to_vec(&expr);
        let snapshot = canonical_epathmap_snapshot_from_expr(&canonical_expr)
            .expect("canonical EPathMap snapshot")
            .to_vec();

        let envelope = canonical_expr_envelope(&canonical_expr).expect("canonical Expr envelope");
        let mut duplicate_snapshot_body = envelope.payload.to_vec();
        duplicate_snapshot_body.push(0x4a); // EPathMap.trie_snapshot = 9, length-delimited.
        prost::encoding::encode_varint(snapshot.len() as u64, &mut duplicate_snapshot_body);
        duplicate_snapshot_body.extend_from_slice(&snapshot);
        let mut duplicate_snapshot_expr = vec![0x82, 0x02]; // Expr.e_pathmap_body = 32.
        prost::encoding::encode_varint(
            duplicate_snapshot_body.len() as u64,
            &mut duplicate_snapshot_expr,
        );
        duplicate_snapshot_expr.extend_from_slice(&duplicate_snapshot_body);
        assert!(canonical_epathmap_snapshot_from_expr(&duplicate_snapshot_expr).is_none());
        assert!(canonical_epathmap_root_view(&duplicate_snapshot_expr).is_none());

        let mut malformed_snapshot = snapshot;
        malformed_snapshot[0] ^= 0xff;
        assert!(canonical_singleton_epathmap_chain_parts(&malformed_snapshot).is_none());
    }

    #[test]
    fn singleton_epathmap_fast_import_rejects_malformed_outer_and_nested_remainders() {
        use crate::rhoapi::var::VarInstance;

        let key = || {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GString("key".to_owned())),
            }])
        };
        let remainder = || Var {
            var_instance: Some(VarInstance::FreeVar(0)),
        };
        let mut chain = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        for _ in 0..3 {
            chain = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    [(key(), chain)],
                    Vec::new(),
                    false,
                    Some(remainder()),
                ))),
            }]);
        }

        let canonical = protobuf_encoder::encode_to_vec(&chain);
        // EPathMap.remainder = field 5; FreeVar(0) is the exact two-byte
        // oneof image [0x10, 0x00]. Each chain level contributes one frame.
        let needle = [0x2a, 0x02, 0x10, 0x00];
        let offsets = canonical
            .windows(needle.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == needle).then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 3, "one remainder frame per map level");

        for offset in offsets {
            let mut malformed = canonical.clone();
            malformed[offset + 2] = 0x20; // same-length unknown Var field
            assert!(
                import_canonical_parbag_fast_path(&malformed).is_none(),
                "a malformed remainder at byte {offset} entered the trusted fast importer"
            );
            if let Ok(decoded) =
                crate::rust::rholang::protobuf_decoder::decode_par(malformed.as_slice())
            {
                assert_ne!(
                    protobuf_encoder::encode_to_vec(&decoded),
                    malformed,
                    "a permissive full decode must canonicalize, not preserve, unknown Var bytes"
                );
            }
        }
    }

    #[test]
    fn epathmap_root_view_does_not_promote_a_route_hint_to_validation() {
        use crate::rhoapi::EPathMap;

        let key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("key".to_owned())),
        }]);
        let value = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }]);
        let expr = Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                [(key, value)],
                Vec::new(),
                false,
                None,
            ))),
        };
        let canonical_expr = protobuf_encoder::encode_to_vec(&expr);
        let canonical_view =
            canonical_epathmap_root_view(&canonical_expr).expect("canonical root view");
        let canonical_hint = canonical_view.route_hint();
        let body_start =
            canonical_view.protobuf_body().as_ptr() as usize - canonical_expr.as_ptr() as usize;
        let snapshot_start = canonical_view.snapshot().as_ptr() as usize
            - canonical_view.protobuf_body().as_ptr() as usize;
        let mut snapshot_at = 6usize; // EPM1 magic, version, and mode.
        let arena_len = usize::try_from(
            read_canonical_varint(canonical_view.snapshot(), &mut snapshot_at)
                .expect("canonical ACT arena length"),
        )
        .expect("ACT arena length fits usize");
        assert_ne!(arena_len, 0);

        let mut malformed_expr = canonical_expr.clone();
        malformed_expr[body_start + snapshot_start + snapshot_at] = 0xff;
        let malformed_view = canonical_epathmap_root_view(&malformed_expr)
            .expect("the shallow route still accepts the fixed EPM1 prefix");
        assert_eq!(
            malformed_view.route_hint(),
            canonical_hint,
            "ACT contents cannot affect shallow route selection"
        );
        assert!(
            crate::rust::rholang::protobuf_decoder::decode_epath_map(
                malformed_view.protobuf_body()
            )
            .is_err(),
            "the complete generated decoder must reject the malformed ACT"
        );
    }
}
