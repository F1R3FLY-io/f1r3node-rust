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
//!     "RPB"  0x01  constructor-tag  stack-safe canonical bincode payload
//!      └domain┘     └── type split ─┘ └──── one complete occurrence ─────┘
//! ```
//!
//! The PathMap endpoint delimits the payload. Given the constructor tag, the
//! generated bincode PDA determines the exact payload extent; canonical ACT
//! import rejects a key unless that extent is the whole suffix. Identical typed
//! occurrences therefore share one prefix-compressed path whose value is their
//! checked `u64` multiplicity. Ordered children remain ordered *inside* their
//! component payload.

use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

use prost::bytes::BufMut;
use prost::encoding::{encode_key, encoded_len_varint, key_len, WireType};
use rspace_plus_plus::rspace::bag::{Bag, BagCodecError, BagError, Multiplicity};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use crate::rust::rholang::bincode_encoder;
use crate::rust::rholang::bincode_schema::BincodeNode;

/// Domain and version prefix for every authoritative ParBag component key.
///
/// The version byte is part of the trie path, so a future encoding cannot
/// silently collide with or reinterpret this key language.
pub const PAR_BAG_KEY_DOMAIN: [u8; 4] = *b"RPB\x01";
const PAR_BAG_KEY_HEADER_LEN: usize = PAR_BAG_KEY_DOMAIN.len() + 1;

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
pub trait ParBagComponent:
    sealed::Sealed + BincodeNode + ColdStoreDecode + Sized + 'static
{
    const TAG: ParComponentTag;
}

macro_rules! impl_par_bag_component {
    ($ty:ty, $tag:ident) => {
        impl sealed::Sealed for $ty {}
        impl ParBagComponent for $ty {
            const TAG: ParComponentTag = ParComponentTag::$tag;
        }
    };
}

impl_par_bag_component!(Send, Send);
impl_par_bag_component!(Receive, Receive);
impl_par_bag_component!(New, New);
impl_par_bag_component!(Expr, Expr);
impl_par_bag_component!(Match, Match);
impl_par_bag_component!(GUnforgeable, GUnforgeable);
impl_par_bag_component!(Connective, Connective);
impl_par_bag_component!(Bundle, Bundle);
impl_par_bag_component!(If, If);

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
    ComponentDecode {
        tag: ParComponentTag,
        source: ColdStoreDecodeError,
    },
    TrailingComponentBytes {
        tag: ParComponentTag,
        consumed: usize,
        available: usize,
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
            Self::ComponentDecode { tag, source } => {
                write!(formatter, "invalid {tag:?} component payload: {source}")
            }
            Self::TrailingComponentBytes {
                tag,
                consumed,
                available,
            } => write!(
                formatter,
                "{tag:?} component consumed {consumed} of {available} payload bytes"
            ),
        }
    }
}

impl std::error::Error for ParBagCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bag(error) => Some(error),
            Self::ComponentDecode { source, .. } => Some(source),
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
    /// Reused complete-key buffer. `bincode_encoder::encode_into` appends the
    /// payload directly after the domain and tag, avoiding both a payload copy
    /// and a second temporary byte vector in the steady state.
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
            bincode_encoder::encode_into(value, &mut key);
            let result = callback(&key);
            if key.capacity() > KEY_BUFFER_SHRINK_THRESHOLD {
                key.shrink_to(KEY_BUFFER_CAPACITY);
            }
            result
        }
        Err(_) => {
            let mut key = Vec::with_capacity(KEY_BUFFER_CAPACITY);
            key.extend_from_slice(&component_prefix(T::TAG));
            bincode_encoder::encode_into(value, &mut key);
            callback(&key)
        }
    })
}

fn decode_exact<T: ParBagComponent>(payload: &[u8]) -> Result<T, ParBagCodecError> {
    let (value, consumed) =
        T::cold_decode_prefix(payload).map_err(|source| ParBagCodecError::ComponentDecode {
            tag: T::TAG,
            source,
        })?;
    if consumed != payload.len() {
        return Err(ParBagCodecError::TrailingComponentBytes {
            tag: T::TAG,
            consumed,
            available: payload.len(),
        });
    }
    Ok(value)
}

fn validate_key(path: &[u8]) -> Result<(), ParBagCodecError> {
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
    let payload = &path[PAR_BAG_KEY_HEADER_LEN..];
    match tag {
        ParComponentTag::Send => drop(decode_exact::<Send>(payload)?),
        ParComponentTag::Receive => drop(decode_exact::<Receive>(payload)?),
        ParComponentTag::New => drop(decode_exact::<New>(payload)?),
        ParComponentTag::Expr => drop(decode_exact::<Expr>(payload)?),
        ParComponentTag::Match => drop(decode_exact::<Match>(payload)?),
        ParComponentTag::GUnforgeable => drop(decode_exact::<GUnforgeable>(payload)?),
        ParComponentTag::Connective => drop(decode_exact::<Connective>(payload)?),
        ParComponentTag::Bundle => drop(decode_exact::<Bundle>(payload)?),
        ParComponentTag::If => drop(decode_exact::<If>(payload)?),
    }
    Ok(())
}

/// A borrowed, byte-native typed occurrence visited directly from the trie.
/// Decode only when an operation actually needs the Rust AST value.
#[derive(Clone, Copy)]
pub struct EncodedParComponent<'a, T> {
    payload: &'a [u8],
    multiplicity: Multiplicity,
    marker: PhantomData<fn() -> T>,
}

impl<'a, T: ParBagComponent> EncodedParComponent<'a, T> {
    pub const fn payload(&self) -> &'a [u8] { self.payload }

    pub const fn multiplicity(&self) -> Multiplicity { self.multiplicity }

    pub fn decode(self) -> Result<T, ParBagCodecError> { decode_exact::<T>(self.payload) }
}

/// One homogeneous PathMap-backed multiset containing every top-level `Par`
/// component category. Empty `Par` is exactly an empty `ParBag`.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ParBag {
    components: Bag,
}

impl ParBag {
    pub fn new() -> Self { Self::default() }

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
        for (path, _) in components.iter() {
            validate_key(&path)?;
        }
        Ok(Self { components })
    }

    pub fn as_bag(&self) -> &Bag { &self.components }

    pub fn into_bag(self) -> Bag { self.components }

    pub const fn distinct_len(&self) -> usize { self.components.distinct_len() }

    pub const fn cardinality(&self) -> u128 { self.components.cardinality() }

    pub fn is_empty(&self) -> bool { self.components.is_empty() }

    pub fn contains<T: ParBagComponent>(&self, value: &T) -> bool {
        with_component_key(value, |key| self.components.contains(key))
    }

    pub fn count<T: ParBagComponent>(&self, value: &T) -> u64 {
        with_component_key(value, |key| self.components.count(key))
    }

    pub fn insert<T: ParBagComponent>(&mut self, value: &T) -> Result<Multiplicity, BagError> {
        with_component_key(value, |key| self.components.insert(key))
    }

    pub fn insert_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Result<Multiplicity, BagError> {
        with_component_key(value, |key| self.components.insert_many(key, amount))
    }

    pub fn remove<T: ParBagComponent>(&mut self, value: &T) -> Option<Multiplicity> {
        with_component_key(value, |key| self.components.remove(key))
    }

    pub fn remove_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Option<Multiplicity> {
        with_component_key(value, |key| self.components.remove_many(key, amount))
    }

    pub fn try_remove_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
    ) -> Result<Option<Multiplicity>, BagError> {
        with_component_key(value, |key| self.components.try_remove_many(key, amount))
    }

    /// Remove one entire constructor family with one prefix operation.
    pub fn remove_family<T: ParBagComponent>(&mut self) -> (usize, u128) {
        self.components.remove_prefix(&component_prefix(T::TAG))
    }

    pub fn family_distinct_len<T: ParBagComponent>(&self) -> usize {
        let mut count = 0usize;
        self.components
            .visit_prefix(&component_prefix(T::TAG), |_path, _multiplicity| {
                count += 1;
            });
        count
    }

    pub fn family_cardinality<T: ParBagComponent>(&self) -> u128 {
        let mut cardinality = 0u128;
        self.components
            .visit_prefix(&component_prefix(T::TAG), |_path, multiplicity| {
                cardinality += u128::from(multiplicity.get());
            });
        cardinality
    }

    /// Visit one constructor family without decoding its payloads.
    pub fn visit_encoded<T: ParBagComponent>(
        &self,
        mut visitor: impl FnMut(EncodedParComponent<'_, T>),
    ) {
        self.components
            .visit_prefix(&component_prefix(T::TAG), |path, multiplicity| {
                debug_assert!(path.len() >= PAR_BAG_KEY_HEADER_LEN);
                visitor(EncodedParComponent {
                    payload: &path[PAR_BAG_KEY_HEADER_LEN..],
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
                let value = decode_exact::<T>(&path[PAR_BAG_KEY_HEADER_LEN..])
                    .map_err(ParBagVisitError::Codec)?;
                visitor(value, multiplicity).map_err(ParBagVisitError::Visitor)
            })
    }

    pub fn sum(&self, other: &Self) -> Result<Self, BagError> {
        self.components
            .sum(&other.components)
            .map(|components| Self { components })
    }

    pub fn join(&self, other: &Self) -> Self {
        Self {
            components: self.components.join(&other.components),
        }
    }

    pub fn meet(&self, other: &Self) -> Self {
        Self {
            components: self.components.meet(&other.components),
        }
    }

    pub fn subtract(&self, other: &Self) -> Self {
        Self {
            components: self.components.subtract(&other.components),
        }
    }

    pub fn is_subbag_of(&self, other: &Self) -> bool {
        self.components.is_subbag_of(&other.components)
    }

    /// Direct canonical `ACTree03` image: no entry vector or decoded shadow.
    pub fn compact_bytes(&self) -> Vec<u8> { self.components.compact_bytes() }

    pub fn from_compact_bytes(bytes: &[u8]) -> Result<Self, ParBagCodecError> {
        Self::try_from_bag(Bag::from_compact_bytes(bytes)?)
    }

    /// Exact length of the legacy repeated-message protobuf component fields.
    /// Root metadata fields 9 and 10 are owned by the eventual repository
    /// `Par` wrapper and are deliberately outside this component Bag.
    pub fn protobuf_components_encoded_len(&self) -> Result<usize, ParBagProtobufError> {
        let mut total = 0usize;
        self.add_protobuf_family_len::<Send>(&mut total)?;
        self.add_protobuf_family_len::<Receive>(&mut total)?;
        self.add_protobuf_family_len::<New>(&mut total)?;
        self.add_protobuf_family_len::<Expr>(&mut total)?;
        self.add_protobuf_family_len::<Match>(&mut total)?;
        self.add_protobuf_family_len::<GUnforgeable>(&mut total)?;
        self.add_protobuf_family_len::<Connective>(&mut total)?;
        self.add_protobuf_family_len::<Bundle>(&mut total)?;
        self.add_protobuf_family_len::<If>(&mut total)?;
        Ok(total)
    }

    /// Stream protobuf fields from trie endpoints in canonical tag/key order.
    /// Each distinct payload is decoded once and emitted `Multiplicity` times;
    /// no repeated-field `Vec` or legacy `Par` is materialized.
    pub fn protobuf_components_to_vec(&self) -> Result<Vec<u8>, ParBagProtobufError> {
        let encoded_len = self.protobuf_components_encoded_len()?;
        let mut output = Vec::with_capacity(encoded_len);
        self.encode_protobuf_family::<Send>(&mut output)?;
        self.encode_protobuf_family::<Receive>(&mut output)?;
        self.encode_protobuf_family::<New>(&mut output)?;
        self.encode_protobuf_family::<Expr>(&mut output)?;
        self.encode_protobuf_family::<Match>(&mut output)?;
        self.encode_protobuf_family::<GUnforgeable>(&mut output)?;
        self.encode_protobuf_family::<Connective>(&mut output)?;
        self.encode_protobuf_family::<Bundle>(&mut output)?;
        self.encode_protobuf_family::<If>(&mut output)?;
        debug_assert_eq!(output.len(), encoded_len);
        Ok(output)
    }

    fn add_protobuf_family_len<T>(&self, total: &mut usize) -> Result<(), ParBagProtobufError>
    where T: ParBagComponent + crate::rust::rholang::protobuf_schema::ProtobufNode {
        self.try_visit_decoded::<T, ParBagProtobufError>(|value, multiplicity| {
            let body_len = crate::rust::rholang::protobuf_encoder::encoded_len(&value);
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
        })
        .map_err(|error| match error {
            ParBagVisitError::Codec(error) => ParBagProtobufError::Codec(error),
            ParBagVisitError::Visitor(error) => error,
        })
    }

    fn encode_protobuf_family<T>(
        &self,
        output: &mut impl BufMut,
    ) -> Result<(), ParBagProtobufError>
    where
        T: ParBagComponent + crate::rust::rholang::protobuf_schema::ProtobufNode,
    {
        self.try_visit_decoded::<T, ParBagProtobufError>(|value, multiplicity| {
            for _ in 0..multiplicity.get() {
                encode_key(T::TAG.byte().into(), WireType::LengthDelimited, output);
                crate::rust::rholang::protobuf_encoder::encode_length_delimited_body_into(
                    &value, output,
                );
            }
            Ok(())
        })
        .map_err(|error| match error {
            ParBagVisitError::Codec(error) => ParBagProtobufError::Codec(error),
            ParBagVisitError::Visitor(error) => error,
        })
    }
}

impl TryFrom<&Par> for ParBag {
    type Error = BagError;

    fn try_from(par: &Par) -> Result<Self, Self::Error> { Self::import_legacy(par) }
}

impl Serialize for ParBag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        self.components.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ParBag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let components = Bag::deserialize(deserializer)?;
        Self::try_from_bag(components).map_err(serde::de::Error::custom)
    }
}
