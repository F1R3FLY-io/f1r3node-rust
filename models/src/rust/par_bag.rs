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
use crate::rust::rholang::protobuf_schema::{ProtobufDescent, ProtobufNode};
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
    let payload_end = path.len() - PAR_BAG_SCORE_LEN_BYTES;
    let declared = u64::from_be_bytes(
        path[payload_end..]
            .try_into()
            .expect("the checked key trailer contains one u64 score length"),
    );
    let score_len =
        usize::try_from(declared).map_err(|_| ParBagCodecError::InvalidOrderKey { tag })?;
    let score_end = PAR_BAG_KEY_HEADER_LEN
        .checked_add(score_len)
        .filter(|&end| end <= payload_end)
        .ok_or(ParBagCodecError::InvalidOrderKey { tag })?;
    Ok((
        tag,
        &path[PAR_BAG_KEY_HEADER_LEN..score_end],
        &path[score_end..payload_end],
    ))
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
fn canonical_unary_elist(payload: &[u8]) -> Option<(usize, i64)> {
    // Expr.e_list_body = field 20, wire type 2; EList.ps = field 1, wire type
    // 2; Par.exprs = field 5, wire type 2.
    const EXPR_ELIST_KEY: u64 = (20 << 3) | 2;
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
        let list = canonical_single_delimited_field(current, EXPR_ELIST_KEY)?;
        let par = canonical_single_delimited_field(list, ELIST_PAR_KEY)?;
        current = canonical_single_delimited_field(par, PAR_EXPR_KEY)?;
        depth = depth.checked_add(1)?;
    }
}

/// Build the canonical protobuf body of one `Expr` whose shape is an exact
/// unary `EList` spine ending in `GInt`.
///
/// Lengths are computed from the leaf outward and emitted from the root
/// inward.  This is the same two-pass discipline as the generated protobuf
/// encoder, specialized to the three-field recurrence in this shape; no
/// intermediate nested `Par` or component trie is constructed.
fn encode_canonical_unary_elist_expr(depth: usize, terminal: i64) -> Option<Vec<u8>> {
    const EXPR_ELIST_TAG: u32 = 20;
    const ELIST_PAR_TAG: u32 = 1;
    const PAR_EXPR_TAG: u32 = 5;
    const EXPR_GINT_TAG: u32 = 2;

    let zigzag = ((terminal << 1) ^ (terminal >> 63)) as u64;
    let mut expr_len = key_len(EXPR_GINT_TAG).checked_add(encoded_len_varint(zigzag))?;
    let mut frames = Vec::with_capacity(depth);
    for _ in 0..depth {
        let child_expr_len = expr_len;
        let par_len = key_len(PAR_EXPR_TAG)
            .checked_add(encoded_len_varint(child_expr_len as u64))?
            .checked_add(child_expr_len)?;
        let list_len = key_len(ELIST_PAR_TAG)
            .checked_add(encoded_len_varint(par_len as u64))?
            .checked_add(par_len)?;
        expr_len = key_len(EXPR_ELIST_TAG)
            .checked_add(encoded_len_varint(list_len as u64))?
            .checked_add(list_len)?;
        frames.push((list_len, par_len, child_expr_len));
    }

    let mut out = Vec::with_capacity(expr_len);
    for &(list_len, par_len, child_expr_len) in frames.iter().rev() {
        encode_key(EXPR_ELIST_TAG, WireType::LengthDelimited, &mut out);
        prost::encoding::encode_varint(list_len as u64, &mut out);
        encode_key(ELIST_PAR_TAG, WireType::LengthDelimited, &mut out);
        prost::encoding::encode_varint(par_len as u64, &mut out);
        encode_key(PAR_EXPR_TAG, WireType::LengthDelimited, &mut out);
        prost::encoding::encode_varint(child_expr_len as u64, &mut out);
    }
    encode_key(EXPR_GINT_TAG, WireType::Varint, &mut out);
    prost::encoding::encode_varint(zigzag, &mut out);
    debug_assert_eq!(out.len(), expr_len);
    Some(out)
}

/// Adopt one already-proved canonical unary `EList` expression as the sole
/// endpoint of a `ParBag`.
///
/// The score and protobuf body are written directly into the final trie key.
/// This avoids the quadratic sequence of constructing, hashing, and dropping
/// every nested suffix trie while preserving the exact `RPB\x03` key language.
fn import_canonical_unary_elist_expr(expr_body: &[u8], depth: usize, terminal: i64) -> Option<Par> {
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

/// Construct the resident canonical unary-list `Par` directly from its two
/// scalar parameters.  Canonical-path decoding uses this after its byte cursor
/// has proved the structural path image.
pub(crate) fn import_canonical_unary_elist_par(depth: usize, terminal: i64) -> Option<Par> {
    let expr_body = encode_canonical_unary_elist_expr(depth, terminal)?;
    import_canonical_unary_elist_expr(&expr_body, depth, terminal)
}

fn validate_unary_elist_key(score: &[u8], payload: &[u8]) -> Option<bool> {
    let (depth, terminal) = canonical_unary_elist(payload)?;

    let mut at = 0usize;
    for _ in 0..depth {
        if !score_node_start_is(score, &mut at, Score::ELIST)
            || !score_int_is(score, &mut at, -1)
            || !score_node_start_is(score, &mut at, Score::PAR)
        {
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

fn canonical_singleton_epathmap_parts(protobuf: &[u8]) -> Option<(&[u8], &[u8])> {
    // Par.exprs = 5, Expr.e_pathmap_body = 32,
    // EPathMap.trie_snapshot = 9.
    const PAR_EXPR_KEY: u64 = (5 << 3) | 2;
    const EXPR_EPATHMAP_KEY: u64 = (32 << 3) | 2;
    const EPATHMAP_SNAPSHOT_KEY: u64 = (9 << 3) | 2;

    let expr_body = canonical_single_delimited_field(protobuf, PAR_EXPR_KEY)?;
    let pathmap_body = canonical_single_delimited_field(expr_body, EXPR_EPATHMAP_KEY)?;
    let snapshot = canonical_single_delimited_field(pathmap_body, EPATHMAP_SNAPSHOT_KEY)?;
    Some((expr_body, snapshot))
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
fn canonical_singleton_epathmap_chain_is_stable(mut snapshot: &[u8]) -> bool {
    use crate::rust::epathmap_trie_codec::{CanonicalSnapshotKey, EPathMapMode};

    loop {
        let Ok(inspection) =
            crate::rust::epathmap_trie_codec::inspect_canonical_map_snapshot(snapshot)
        else {
            return false;
        };
        if inspection.mode != EPathMapMode::Map
            || inspection.keys.len() != 1
            || inspection.value_bodies.len() != 1
        {
            return false;
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
            return false;
        }

        let Some(value_body) = snapshot.get(inspection.value_bodies[0].clone()) else {
            return false;
        };
        if canonical_singleton_gint_par(value_body) {
            return true;
        }
        let Some((_expr_body, nested)) = canonical_singleton_epathmap_parts(value_body) else {
            return false;
        };
        if nested.len() >= snapshot.len() {
            return false;
        }
        snapshot = nested;
    }
}

/// Import a canonical, stable singleton EPathMap chain as one raw Expr
/// endpoint. The nested maps remain in their prefix-compressed EPM1 image until
/// a semantic consumer actually asks to decode that Expr.
fn import_canonical_singleton_epathmap_par(protobuf: &[u8]) -> Option<Par> {
    let (expr_body, snapshot) = canonical_singleton_epathmap_parts(protobuf)?;
    canonical_singleton_epathmap_chain_is_stable(snapshot).then_some(())?;

    let mut key = Vec::with_capacity(
        PAR_BAG_KEY_HEADER_LEN
            .checked_add(snapshot.len().checked_mul(2)?)?
            .checked_add(expr_body.len())?
            .checked_add(64)?,
    );
    key.extend_from_slice(&component_prefix(ParComponentTag::Expr));
    let score_start = key.len();
    begin_score_node(&mut key, Score::EPATHMAP);
    append_bytes_score_atom(&mut key, snapshot);
    append_int_score_atom(&mut key, 0);
    end_score_node(&mut key);
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

    import_canonical_singleton_epathmap_par(protobuf)
        .or_else(unary)
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
    payload: &'a [u8],
    multiplicity: Multiplicity,
    marker: PhantomData<fn() -> T>,
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
    pub const fn payload(&self) -> &'a [u8] { self.payload }

    pub const fn multiplicity(&self) -> Multiplicity { self.multiplicity }

    pub fn decode(self) -> Result<T, ParBagCodecError> { decode_exact::<T>(self.payload) }
}

impl EncodedParComponent<'_, Expr> {
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
    /// Concatenated component score keys in canonical Par family order.
    score_components: OnceLock<Arc<[u8]>>,
    /// Canonical protobuf component bytes on either side of root fields 9/10.
    protobuf_before_root: OnceLock<Arc<[u8]>>,
    protobuf_after_root: OnceLock<Arc<[u8]>>,
    /// Stability of the sole component, when cardinality is exactly one.
    /// Multi-component and empty bags are known unstable without consulting
    /// this cell. The cell is shared by CoW clones and invalidated on mutation.
    canonical_path_singleton_stable: OnceLock<bool>,
}

impl ParBagAux {
    fn new(metrics: ParFamilyMetrics) -> Self {
        Self {
            metrics,
            compact_snapshot: OnceLock::new(),
            score_components: OnceLock::new(),
            protobuf_before_root: OnceLock::new(),
            protobuf_after_root: OnceLock::new(),
            canonical_path_singleton_stable: OnceLock::new(),
        }
    }
}

pub struct ParBag {
    components: Bag,
    /// Cardinalities, distinct counts, and the canonical ACTree03 image are
    /// shared by CoW clones. A successful mutation detaches this compact
    /// metadata/cache block exactly once.
    aux: Arc<ParBagAux>,
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
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParBag")
            .field("components", &self.components)
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

    #[inline]
    fn aux_for_mutation(&mut self) -> &mut ParBagAux {
        if Arc::get_mut(&mut self.aux).is_none() {
            self.aux = Arc::new(ParBagAux::new(self.aux.metrics));
        }
        let aux = Arc::get_mut(&mut self.aux).expect("ParBag owns its detached auxiliary state");
        let _ = aux.compact_snapshot.take();
        let _ = aux.score_components.take();
        let _ = aux.protobuf_before_root.take();
        let _ = aux.protobuf_after_root.take();
        let _ = aux.canonical_path_singleton_stable.take();
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

    /// Visit one constructor family without decoding its payloads.
    pub fn visit_encoded<T: ParBagComponent>(
        &self,
        mut visitor: impl FnMut(EncodedParComponent<'_, T>),
    ) {
        self.components
            .visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                let (tag, payload) = split_key(path)
                    .expect("ParBag invariant: every stored component key was validated");
                debug_assert_eq!(tag, T::TAG);
                visitor(EncodedParComponent {
                    payload,
                    multiplicity,
                    marker: PhantomData,
                });
            });
    }

    /// Append one family's canonical score trees in the exact order encoded by
    /// its PathMap keys. Keys are
    /// `score-prefix || canonical-protobuf-body || u64-BE(score-length)`, so
    /// zipper order is already the sorter's `(score, emitted bytes)` order.
    /// No component body is decoded and repeated multiplicities reuse one
    /// decoded score tree.
    pub(crate) fn append_family_scores<T: ParBagComponent>(&self, out: &mut Vec<Tree<ScoreAtom>>) {
        self.components
            .visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                let (tag, score_bytes, _payload) = split_key_parts(path)
                    .expect("ParBag invariant: every component has a valid key header");
                debug_assert_eq!(tag, T::TAG);
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
            .visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                let (tag, score, _payload) = split_key_parts(path)
                    .expect("ParBag invariant: every component has a valid key header");
                debug_assert_eq!(tag, T::TAG);
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
        self.components
            .try_visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                let (tag, payload) = split_key(path).map_err(ParBagVisitError::Codec)?;
                debug_assert_eq!(tag, T::TAG);
                let value = decode_exact::<T>(payload).map_err(ParBagVisitError::Codec)?;
                visitor(value, multiplicity).map_err(ParBagVisitError::Visitor)
            })
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
        let result =
            self.components
                .try_visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                    let (tag, payload) = split_key(path).map_err(Stop::Codec)?;
                    debug_assert_eq!(tag, T::TAG);
                    let value = decode_exact::<T>(payload).map_err(Stop::Codec)?;
                    first = Some((value, multiplicity));
                    Err(Stop::Found)
                });
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
        EList, ETuple, GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken,
        Par, Var,
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
}
