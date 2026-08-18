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
//!     "RPB"  0x02  constructor-tag  order-key  canonical protobuf body
//!      └domain┘     └── type split ─┘ └──── one complete occurrence ────┘
//! ```
//!
//! `order-key` is the prefix-free, lexicographically order-preserving image of
//! the component's score tree.  The protobuf body is consequently both the
//! exact tie-break used by canonical Rholang sorting and the bytes emitted on
//! the legacy protobuf surface.  Serialization walks the trie and copies those
//! bytes directly: it never decodes a component or constructs `Vec<Par>`.
//!
//! Canonical ACT import decodes the body with the generated stack-safe protobuf
//! PDA, sorts it, regenerates the complete key, and requires byte equality.
//! Identical typed occurrences therefore share one prefix-compressed path whose
//! value is their checked `u64` multiplicity.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};

use prost::bytes::BufMut;
use prost::encoding::{encode_key, encoded_len_varint, key_len, WireType};
use rspace_plus_plus::rspace::bag::{Bag, BagCodecError, BagError, Multiplicity};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::g_unforgeable::UnfInstance;
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
    encode_order_key, order_key_extent, Score, ScoredTerm,
};
use crate::rust::rholang::sorter::send_sort_matcher::SendSortMatcher;
use crate::rust::rholang::sorter::sortable::Sortable;
use crate::rust::rholang::sorter::unforgeable_sort_matcher::UnforgeableSortMatcher;

/// Domain and version prefix for every authoritative ParBag component key.
///
/// The version byte is part of the trie path, so a future encoding cannot
/// silently collide with or reinterpret this key language.
pub const PAR_BAG_KEY_DOMAIN: [u8; 4] = *b"RPB\x02";
const PAR_BAG_KEY_HEADER_LEN: usize = PAR_BAG_KEY_DOMAIN.len() + 1;
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

    /// Append the score-order key and canonical protobuf body for one value.
    ///
    /// The default is the complete stack-safe canonical sorter. Components
    /// whose entire constructor family is already a canonical leaf override
    /// this to avoid allocating a traversal, score tree, and cloned term on
    /// every exact lookup.
    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) {
        let scored = Self::canonicalize(value);
        encode_order_key(&scored.score, out);
        protobuf_encoder::encode_into(&scored.term, out);
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError>;
}

macro_rules! impl_par_bag_component {
    ($ty:ty, $tag:ident, $sorter:ty, $decoder:ident) => {
        impl sealed::Sealed for $ty {}
        impl ParBagComponent for $ty {
            const TAG: ParComponentTag = ParComponentTag::$tag;

            fn canonicalize(value: &Self) -> ScoredTerm<Self> {
                <$sorter as Sortable<Self>>::sort_match(value)
            }

            fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
                crate::rust::rholang::protobuf_decoder::$decoder(payload)
            }
        }
    };
}

impl_par_bag_component!(Send, Send, SendSortMatcher, decode_send);
impl_par_bag_component!(Receive, Receive, ReceiveSortMatcher, decode_receive);
impl_par_bag_component!(New, New, NewSortMatcher, decode_new);
impl_par_bag_component!(Match, Match, MatchSortMatcher, decode_match);
impl_par_bag_component!(
    Connective,
    Connective,
    ConnectiveSortMatcher,
    decode_connective
);
impl_par_bag_component!(Bundle, Bundle, BundleSortMatcher, decode_bundle);
impl_par_bag_component!(If, If, IfSortMatcher, decode_if);

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

impl sealed::Sealed for Expr {}
impl ParBagComponent for Expr {
    const TAG: ParComponentTag = ParComponentTag::Expr;

    fn canonicalize(value: &Self) -> ScoredTerm<Self> {
        <ExprSortMatcher as Sortable<Expr>>::sort_match(value)
    }

    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) {
        if !append_leaf_expr_score(value, out) {
            let scored = Self::canonicalize(value);
            encode_order_key(&scored.score, out);
            protobuf_encoder::encode_into(&scored.term, out);
            return;
        }
        assert!(
            append_shallow_protobuf(value, out),
            "ground Expr has at most one non-recursive protobuf message edge"
        );
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_expr(payload)
    }
}

impl sealed::Sealed for GUnforgeable {}
impl ParBagComponent for GUnforgeable {
    const TAG: ParComponentTag = ParComponentTag::GUnforgeable;

    fn canonicalize(value: &Self) -> ScoredTerm<Self> {
        <UnforgeableSortMatcher as Sortable<GUnforgeable>>::sort_match(value)
    }

    fn append_key_suffix(value: &Self, out: &mut Vec<u8>) {
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
        assert!(
            append_shallow_protobuf(value, out),
            "GUnforgeable has exactly one non-recursive protobuf message edge"
        );
    }

    fn decode_protobuf(payload: &[u8]) -> Result<Self, prost::DecodeError> {
        crate::rust::rholang::protobuf_decoder::decode_g_unforgeable(payload)
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
                "ParBag key is {length} byte(s), shorter than its {PAR_BAG_KEY_HEADER_LEN}-byte header"
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

fn component_prefix(tag: ParComponentTag) -> [u8; PAR_BAG_KEY_HEADER_LEN] {
    let mut prefix = [0u8; PAR_BAG_KEY_HEADER_LEN];
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
    KEY_BUFFER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut key) => {
            key.clear();
            key.extend_from_slice(&component_prefix(T::TAG));
            T::append_key_suffix(value, &mut key);
            let result = callback(&key);
            if key.capacity() > KEY_BUFFER_SHRINK_THRESHOLD {
                key.shrink_to(KEY_BUFFER_CAPACITY);
            }
            result
        }
        Err(_) => {
            let mut key = Vec::with_capacity(KEY_BUFFER_CAPACITY);
            key.extend_from_slice(&component_prefix(T::TAG));
            T::append_key_suffix(value, &mut key);
            callback(&key)
        }
    })
}

fn decode_exact<T: ParBagComponent>(payload: &[u8]) -> Result<T, ParBagCodecError> {
    T::decode_protobuf(payload).map_err(|source| ParBagCodecError::ComponentDecode {
        tag: T::TAG,
        reason: source.to_string(),
    })
}

fn split_key(path: &[u8]) -> Result<(ParComponentTag, &[u8]), ParBagCodecError> {
    if path.len() < PAR_BAG_KEY_HEADER_LEN {
        return Err(ParBagCodecError::KeyTooShort { length: path.len() });
    }
    let mut found = [0u8; PAR_BAG_KEY_DOMAIN.len()];
    found.copy_from_slice(&path[..PAR_BAG_KEY_DOMAIN.len()]);
    if found != PAR_BAG_KEY_DOMAIN {
        return Err(ParBagCodecError::InvalidDomain { found });
    }
    let tag_byte = path[PAR_BAG_KEY_DOMAIN.len()];
    let tag = ParComponentTag::from_byte(tag_byte).ok_or(ParBagCodecError::InvalidTag(tag_byte))?;
    let suffix = &path[PAR_BAG_KEY_HEADER_LEN..];
    let order_len = order_key_extent(suffix).ok_or(ParBagCodecError::InvalidOrderKey { tag })?;
    Ok((tag, &suffix[order_len..]))
}

fn validate_typed_key<T: ParBagComponent>(
    path: &[u8],
    payload: &[u8],
) -> Result<(), ParBagCodecError> {
    let value = decode_exact::<T>(payload)?;
    let canonical = with_component_key(&value, <[u8]>::to_vec);
    if canonical != path {
        return Err(ParBagCodecError::NonCanonicalComponent { tag: T::TAG });
    }
    Ok(())
}

fn validate_key(path: &[u8]) -> Result<ParComponentTag, ParBagCodecError> {
    let (tag, payload) = split_key(path)?;
    match tag {
        ParComponentTag::Send => validate_typed_key::<Send>(path, payload)?,
        ParComponentTag::Receive => validate_typed_key::<Receive>(path, payload)?,
        ParComponentTag::New => validate_typed_key::<New>(path, payload)?,
        ParComponentTag::Expr => validate_typed_key::<Expr>(path, payload)?,
        ParComponentTag::Match => validate_typed_key::<Match>(path, payload)?,
        ParComponentTag::GUnforgeable => validate_typed_key::<GUnforgeable>(path, payload)?,
        ParComponentTag::Connective => validate_typed_key::<Connective>(path, payload)?,
        ParComponentTag::Bundle => validate_typed_key::<Bundle>(path, payload)?,
        ParComponentTag::If => validate_typed_key::<If>(path, payload)?,
    }
    Ok(tag)
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
    marker: PhantomData<fn() -> T>,
}

impl<T: ParBagComponent> CanonicalParBagKey<T> {
    pub fn compile(value: &T) -> Self {
        with_component_key(value, |bytes| Self {
            bytes: bytes.into(),
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
#[derive(Clone, Copy, Default)]
struct ParFamilyMetrics {
    exact_cardinalities: [u128; PAR_COMPONENT_FAMILY_COUNT],
    distinct_lengths: [usize; PAR_COMPONENT_FAMILY_COUNT],
}

impl ParFamilyMetrics {
    fn add_decoded(
        &mut self,
        tag: ParComponentTag,
        multiplicity: Multiplicity,
    ) -> Result<(), ParBagCodecError> {
        let index = tag.index();
        self.exact_cardinalities[index] = self.exact_cardinalities[index]
            .checked_add(u128::from(multiplicity.get()))
            .ok_or(ParBagCodecError::FamilyCardinalityOverflow { tag })?;
        self.distinct_lengths[index] = self.distinct_lengths[index]
            .checked_add(1)
            .expect("Bag distinct length already fits usize");
        Ok(())
    }

    fn from_trusted_components(components: &Bag) -> Self {
        let mut metrics = Self::default();
        for (path, multiplicity) in components.iter() {
            let (tag, _) = split_key(&path)
                .expect("ParBag invariant: every internally produced key has a valid header");
            metrics
                .add_decoded(tag, multiplicity)
                .expect("a resident Bag family cardinality fits u128");
        }
        metrics
    }
}

struct ParBagAux {
    metrics: ParFamilyMetrics,
    compact_snapshot: OnceLock<Arc<[u8]>>,
}

impl ParBagAux {
    fn new(metrics: ParFamilyMetrics) -> Self {
        Self {
            metrics,
            compact_snapshot: OnceLock::new(),
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
        aux
    }

    #[inline]
    fn record_insert(&mut self, tag: ParComponentTag, amount: Multiplicity, distinct_added: bool) {
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
    }

    #[inline]
    fn record_remove(&mut self, tag: ParComponentTag, removed: u64, distinct_removed: bool) {
        let metrics = &mut self.aux_for_mutation().metrics;
        let index = tag.index();
        metrics.exact_cardinalities[index] -= u128::from(removed);
        if distinct_removed {
            metrics.distinct_lengths[index] -= 1;
        }
    }

    fn insert_bytes(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        let distinct_added = !self.components.contains(key);
        let next = self.components.insert_many(key, amount)?;
        self.record_insert(tag, amount, distinct_added);
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
        self.record_remove(tag, removed, removed == current);
        next
    }

    fn try_remove_bytes(
        &mut self,
        tag: ParComponentTag,
        key: &[u8],
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        let next = self.components.try_remove_many(key, amount)?;
        self.record_remove(tag, amount.get(), next.is_none());
        Ok(next)
    }

    /// Import the legacy repeated fields once. Root metadata is deliberately
    /// not duplicated here: it remains a derived concern of the eventual
    /// repository-owned `Par`, outside its sole component authority.
    pub fn import_legacy(par: &Par) -> Result<Self, BagError> {
        let mut bag = Self::new();
        for value in &par.sends {
            bag.insert(value)?;
        }
        for value in &par.receives {
            bag.insert(value)?;
        }
        for value in &par.news {
            bag.insert(value)?;
        }
        for value in &par.exprs {
            bag.insert(value)?;
        }
        for value in &par.matches {
            bag.insert(value)?;
        }
        for value in &par.unforgeables {
            bag.insert(value)?;
        }
        for value in &par.connectives {
            bag.insert(value)?;
        }
        for value in &par.bundles {
            bag.insert(value)?;
        }
        for value in &par.conditionals {
            bag.insert(value)?;
        }
        Ok(bag)
    }

    pub fn try_from_bag(components: Bag) -> Result<Self, ParBagCodecError> {
        let mut metrics = ParFamilyMetrics::default();
        for (path, multiplicity) in components.iter() {
            let tag = validate_key(&path)?;
            metrics.add_decoded(tag, multiplicity)?;
        }
        Ok(Self {
            components,
            aux: Arc::new(ParBagAux::new(metrics)),
        })
    }

    pub fn as_bag(&self) -> &Bag { &self.components }

    pub fn into_bag(self) -> Bag { self.components }

    pub const fn distinct_len(&self) -> usize { self.components.distinct_len() }

    pub const fn cardinality(&self) -> u64 { self.components.cardinality() }

    pub const fn cardinality_is_saturated(&self) -> bool {
        self.components.cardinality_is_saturated()
    }

    pub fn is_empty(&self) -> bool { self.components.is_empty() }

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
            self.insert_bytes(T::TAG, key, Multiplicity::ONE)
        })
    }

    #[inline]
    pub fn insert_key<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
    ) -> Result<Multiplicity, BagError> {
        self.insert_bytes(T::TAG, key.as_bytes(), Multiplicity::ONE)
    }

    pub fn insert_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        with_component_key(value, |key| self.insert_bytes(T::TAG, key, amount))
    }

    #[inline]
    pub fn insert_key_many<T: ParBagComponent>(
        &mut self,
        key: &CanonicalParBagKey<T>,
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        self.insert_bytes(T::TAG, key.as_bytes(), amount)
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
        self.encode_protobuf_family::<Send>(output)?;
        self.encode_protobuf_family::<Receive>(output)?;
        self.encode_protobuf_family::<New>(output)?;
        self.encode_protobuf_family::<Expr>(output)?;
        self.encode_protobuf_family::<Match>(output)?;
        self.encode_protobuf_family::<GUnforgeable>(output)?;
        self.encode_protobuf_family::<Connective>(output)?;
        Ok(())
    }

    pub(crate) fn encode_protobuf_components_after_root_metadata(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError> {
        self.encode_protobuf_family::<Bundle>(output)?;
        self.encode_protobuf_family::<If>(output)?;
        Ok(())
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
        let mut result = Ok(());
        self.visit_encoded::<T>(|entry| {
            if result.is_err() {
                return;
            }
            result = (|| {
                let multiplicity = entry.multiplicity();
                let body_len = entry.payload().len();
                let body_len_u64 = u64::try_from(body_len).map_err(|_| {
                    ParBagProtobufError::EncodedLengthOverflow {
                        tag: T::TAG,
                        multiplicity: multiplicity.get(),
                        one_occurrence_bytes: body_len,
                    }
                })?;
                let one_occurrence = key_len(T::TAG.byte().into())
                    .checked_add(encoded_len_varint(body_len_u64))
                    .and_then(|framing| framing.checked_add(body_len))
                    .ok_or(ParBagProtobufError::EncodedLengthOverflow {
                        tag: T::TAG,
                        multiplicity: multiplicity.get(),
                        one_occurrence_bytes: body_len,
                    })?;
                let repetitions = usize::try_from(multiplicity.get()).map_err(|_| {
                    ParBagProtobufError::EncodedLengthOverflow {
                        tag: T::TAG,
                        multiplicity: multiplicity.get(),
                        one_occurrence_bytes: one_occurrence,
                    }
                })?;
                let family_len = one_occurrence.checked_mul(repetitions).ok_or(
                    ParBagProtobufError::EncodedLengthOverflow {
                        tag: T::TAG,
                        multiplicity: multiplicity.get(),
                        one_occurrence_bytes: one_occurrence,
                    },
                )?;
                *total = total.checked_add(family_len).ok_or(
                    ParBagProtobufError::EncodedLengthOverflow {
                        tag: T::TAG,
                        multiplicity: multiplicity.get(),
                        one_occurrence_bytes: one_occurrence,
                    },
                )?;
                Ok(())
            })();
        });
        result
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

impl TryFrom<&Par> for ParBag {
    type Error = BagError;

    fn try_from(par: &Par) -> Result<Self, Self::Error> { Self::import_legacy(par) }
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
        GBigRational, GDeployId, GDeployerId, GFixedPoint, GPrivate, GSysAuthToken,
    };

    fn generic_key<T: ParBagComponent>(value: &T) -> Vec<u8> {
        let scored = T::canonicalize(value);
        let mut key = component_prefix(T::TAG).to_vec();
        encode_order_key(&scored.score, &mut key);
        protobuf_encoder::encode_into(&scored.term, &mut key);
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
}
