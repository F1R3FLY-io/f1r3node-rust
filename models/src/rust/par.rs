//! Repository-owned, PathMap-backed representation of Rholang parallel composition.
//!
//! [`Par`] is the target of the `.rhoapi.Par` external-schema cutover. Its one
//! component authority is [`ParBag`], a prefix-compressed
//! `PathMap<Multiplicity>`. Root `locally_free` and `connective_used` values are
//! retained only as normalization summaries; there is no repeated-field or
//! decoded component shadow.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::{DecodeError, Message};
use rspace_plus_plus::rspace::bag::{BagError, Multiplicity};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::par_bag::{
    EncodedParComponent, ParBag, ParBagComponent, ParBagProtobufError, ParBagVisitError,
};
use crate::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Receive, Send};

#[derive(Debug)]
pub enum ParProtobufError {
    Components(ParBagProtobufError),
    RootMetadataLengthOverflow,
}

impl fmt::Display for ParProtobufError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Components(error) => error.fmt(formatter),
            Self::RootMetadataLengthOverflow => {
                formatter.write_str("Par root metadata overflows the protobuf output length")
            }
        }
    }
}

impl std::error::Error for ParProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Components(error) => Some(error),
            Self::RootMetadataLengthOverflow => None,
        }
    }
}

impl From<ParBagProtobufError> for ParProtobufError {
    fn from(error: ParBagProtobufError) -> Self { Self::Components(error) }
}

#[repr(C)]
#[derive(Clone, Default)]
pub struct Par {
    components: ParBag,
    locally_free: Vec<u8>,
    connective_used: bool,
}

impl Par {
    pub fn from_parts(components: ParBag, locally_free: Vec<u8>, connective_used: bool) -> Self {
        Self {
            components,
            locally_free,
            connective_used,
        }
    }

    pub fn import_generated(value: &crate::rhoapi::Par) -> Result<Self, BagError> {
        Ok(Self::from_parts(
            ParBag::import_legacy(value)?,
            value.locally_free.clone(),
            value.connective_used,
        ))
    }

    pub const fn components(&self) -> &ParBag { &self.components }

    pub fn into_components(self) -> ParBag { self.components }

    /// Borrow the canonical prefix-compressed Bag image. The image is derived
    /// once, memoized by `ParBag`, and shared by CoW clones.
    pub fn compact_snapshot(&self) -> &[u8] { self.components.compact_snapshot() }

    pub fn locally_free(&self) -> &[u8] { &self.locally_free }

    pub const fn connective_used(&self) -> bool { self.connective_used }

    pub fn is_empty(&self) -> bool { self.components.is_empty() }

    pub const fn distinct_len(&self) -> usize { self.components.distinct_len() }

    pub const fn cardinality(&self) -> u128 { self.components.cardinality() }

    pub fn contains<T: ParBagComponent>(&self, value: &T) -> bool {
        self.components.contains(value)
    }

    pub fn count<T: ParBagComponent>(&self, value: &T) -> u64 { self.components.count(value) }

    pub fn family_distinct_len<T: ParBagComponent>(&self) -> usize {
        self.components.family_distinct_len::<T>()
    }

    pub fn family_cardinality<T: ParBagComponent>(&self) -> u128 {
        self.components.family_cardinality::<T>()
    }

    pub fn visit_encoded<T: ParBagComponent>(
        &self,
        visitor: impl FnMut(EncodedParComponent<'_, T>),
    ) {
        self.components.visit_encoded(visitor);
    }

    pub fn try_visit_decoded<T, E>(
        &self,
        visitor: impl FnMut(T, Multiplicity) -> Result<(), E>,
    ) -> Result<(), ParBagVisitError<E>>
    where
        T: ParBagComponent,
    {
        self.components.try_visit_decoded(visitor)
    }

    pub fn try_visit_occurrences<T, E>(
        &self,
        visitor: impl FnMut(&T) -> Result<(), E>,
    ) -> Result<(), ParBagVisitError<E>>
    where
        T: ParBagComponent,
    {
        self.components.try_visit_occurrences(visitor)
    }

    pub fn first<T: ParBagComponent>(
        &self,
    ) -> Result<Option<(T, Multiplicity)>, super::par_bag::ParBagCodecError> {
        self.components.first::<T>()
    }

    /// Insert one occurrence and join its independently-derived root metadata.
    /// The caller supplies the same cached summary it computed while building
    /// the component, so no second AST traversal is introduced here.
    pub fn insert_component<T: ParBagComponent>(
        &mut self,
        value: &T,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<Multiplicity, BagError> {
        let multiplicity = self.components.insert(value)?;
        self.locally_free =
            super::utils::union(std::mem::take(&mut self.locally_free), locally_free);
        self.connective_used |= connective_used;
        Ok(multiplicity)
    }

    /// Insert `amount` identical occurrences with one canonical-key build.
    pub fn insert_component_many<T: ParBagComponent>(
        &mut self,
        value: &T,
        amount: Multiplicity,
        locally_free: Vec<u8>,
        connective_used: bool,
    ) -> Result<Multiplicity, BagError> {
        let multiplicity = self.components.insert_many(value, amount)?;
        self.locally_free =
            super::utils::union(std::mem::take(&mut self.locally_free), locally_free);
        self.connective_used |= connective_used;
        Ok(multiplicity)
    }

    pub fn parallel(self, other: Self) -> Result<Self, BagError> {
        Ok(Self {
            components: self.components.sum(&other.components)?,
            locally_free: super::utils::union(self.locally_free, other.locally_free),
            connective_used: self.connective_used || other.connective_used,
        })
    }

    pub fn protobuf_encoded_len(&self) -> Result<usize, ParProtobufError> {
        let mut total = 0usize;
        self.components
            .add_protobuf_components_before_root_metadata_len(&mut total)?;
        if !self.locally_free.is_empty() {
            total = total
                .checked_add(prost::encoding::bytes::encoded_len(9, &self.locally_free))
                .ok_or(ParProtobufError::RootMetadataLengthOverflow)?;
        }
        if self.connective_used {
            total = total
                .checked_add(prost::encoding::bool::encoded_len(
                    10,
                    &self.connective_used,
                ))
                .ok_or(ParProtobufError::RootMetadataLengthOverflow)?;
        }
        self.components
            .add_protobuf_components_after_root_metadata_len(&mut total)?;
        Ok(total)
    }

    pub fn encode_protobuf(&self, output: &mut impl BufMut) -> Result<(), ParProtobufError> {
        self.components
            .encode_protobuf_components_before_root_metadata(output)?;
        if !self.locally_free.is_empty() {
            prost::encoding::bytes::encode(9, &self.locally_free, output);
        }
        if self.connective_used {
            prost::encoding::bool::encode(10, &self.connective_used, output);
        }
        self.components
            .encode_protobuf_components_after_root_metadata(output)?;
        Ok(())
    }

    pub fn protobuf_bytes(&self) -> Result<Vec<u8>, ParProtobufError> {
        let mut output = Vec::with_capacity(self.protobuf_encoded_len()?);
        self.encode_protobuf(&mut output)?;
        Ok(output)
    }
}

impl PartialEq for Par {
    fn eq(&self, other: &Self) -> bool {
        self.components == other.components && self.connective_used == other.connective_used
    }
}

impl Eq for Par {}

impl PartialOrd for Par {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Par {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components
            .cmp(&other.components)
            .then_with(|| self.connective_used.cmp(&other.connective_used))
    }
}

impl Hash for Par {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.components.hash(state);
        self.connective_used.hash(state);
    }
}

struct ParFamilyDebug<'a, T> {
    components: &'a ParBag,
    marker: PhantomData<T>,
}

impl<'a, T> ParFamilyDebug<'a, T> {
    fn new(components: &'a ParBag) -> Self {
        Self {
            components,
            marker: PhantomData,
        }
    }
}

impl<T> fmt::Debug for ParFamilyDebug<'_, T>
where T: ParBagComponent + fmt::Debug
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = formatter.debug_list();
        self.components.visit_encoded::<T>(|entry| {
            let multiplicity = entry.multiplicity();
            let value = entry
                .decode()
                .expect("Par invariant: every stored component body was validated");
            for _ in 0..multiplicity.get() {
                list.entry(&value);
            }
        });
        list.finish()
    }
}

impl fmt::Debug for Par {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Par")
            .field("sends", &ParFamilyDebug::<Send>::new(&self.components))
            .field(
                "receives",
                &ParFamilyDebug::<Receive>::new(&self.components),
            )
            .field("news", &ParFamilyDebug::<New>::new(&self.components))
            .field("exprs", &ParFamilyDebug::<Expr>::new(&self.components))
            .field("matches", &ParFamilyDebug::<Match>::new(&self.components))
            .field(
                "unforgeables",
                &ParFamilyDebug::<GUnforgeable>::new(&self.components),
            )
            .field("bundles", &ParFamilyDebug::<Bundle>::new(&self.components))
            .field(
                "connectives",
                &ParFamilyDebug::<Connective>::new(&self.components),
            )
            .field("conditionals", &ParFamilyDebug::<If>::new(&self.components))
            .field("locally_free", &self.locally_free)
            .field("connective_used", &self.connective_used)
            .finish()
    }
}

struct BorrowedBytes<'a>(&'a [u8]);

impl Serialize for BorrowedBytes<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        serializer.serialize_bytes(self.0)
    }
}

impl Serialize for Par {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let mut state = serializer.serialize_struct("Par", 3)?;
        state.serialize_field("components", &BorrowedBytes(self.compact_snapshot()))?;
        state.serialize_field("locally_free", &BorrowedBytes(&[]))?;
        state.serialize_field("connective_used", &self.connective_used)?;
        state.end()
    }
}

#[derive(Deserialize)]
struct ParImage {
    components: ParBag,
    locally_free: Vec<u8>,
    connective_used: bool,
}

impl<'de> Deserialize<'de> for Par {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        let image = ParImage::deserialize(deserializer)?;
        Ok(Self::from_parts(
            image.components,
            image.locally_free,
            image.connective_used,
        ))
    }
}

fn take_length_delimited_component<B>(
    wire_type: WireType,
    buf: &mut B,
) -> Result<prost::bytes::buf::Take<&mut B>, DecodeError>
where
    B: Buf,
{
    encoding::check_wire_type(WireType::LengthDelimited, wire_type)?;
    let length = encoding::decode_varint(buf)?;
    if length > buf.remaining() as u64 {
        return Err(par_decode_error("buffer underflow"));
    }
    Ok((&mut *buf).take(length as usize))
}

fn insert_component<T: ParBagComponent>(
    components: &mut ParBag,
    value: &T,
) -> Result<(), DecodeError> {
    components
        .insert(value)
        .map(|_| ())
        .map_err(|error| par_decode_error(error.to_string()))
}

/// `DecodeError::new` is the only constructor Prost 0.14 exposes to external
/// `Message` implementations for implementation-specific invariant failures.
/// Keeping that dependency in one place makes the required Prost 0.15 API
/// adaptation local if its announced removal occurs.
#[allow(deprecated)]
fn par_decode_error(description: impl Into<std::borrow::Cow<'static, str>>) -> DecodeError {
    DecodeError::new(description)
}

impl Message for Par {
    fn encode_raw(&self, buf: &mut impl BufMut) {
        self.encode_protobuf(buf)
            .expect("PathMap-backed Par invariant: protobuf output length is representable");
    }

    fn merge_field(
        &mut self,
        tag: u32,
        wire_type: WireType,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        macro_rules! merge_component {
            ($component:ty, $decode:path) => {{
                let body = take_length_delimited_component(wire_type, buf)?;
                let value: $component = $decode(body)?;
                insert_component(&mut self.components, &value)
            }};
        }

        match tag {
            1 => merge_component!(Send, crate::rust::rholang::protobuf_decoder::decode_send),
            2 => merge_component!(
                Receive,
                crate::rust::rholang::protobuf_decoder::decode_receive
            ),
            4 => merge_component!(New, crate::rust::rholang::protobuf_decoder::decode_new),
            5 => merge_component!(Expr, crate::rust::rholang::protobuf_decoder::decode_expr),
            6 => merge_component!(Match, crate::rust::rholang::protobuf_decoder::decode_match),
            7 => merge_component!(
                GUnforgeable,
                crate::rust::rholang::protobuf_decoder::decode_g_unforgeable
            ),
            8 => merge_component!(
                Connective,
                crate::rust::rholang::protobuf_decoder::decode_connective
            ),
            9 => encoding::bytes::merge(wire_type, &mut self.locally_free, buf, ctx),
            10 => encoding::bool::merge(wire_type, &mut self.connective_used, buf, ctx),
            11 => merge_component!(
                Bundle,
                crate::rust::rholang::protobuf_decoder::decode_bundle
            ),
            12 => merge_component!(If, crate::rust::rholang::protobuf_decoder::decode_if),
            _ => crate::rust::rholang::protobuf_decoder::skip_unknown_field(wire_type, tag, buf),
        }
    }

    fn encoded_len(&self) -> usize {
        self.protobuf_encoded_len()
            .expect("PathMap-backed Par invariant: protobuf output length is representable")
    }

    fn clear(&mut self) {
        self.components = ParBag::default();
        self.locally_free.clear();
        self.connective_used = false;
    }
}
