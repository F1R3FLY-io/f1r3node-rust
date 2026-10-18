//! Repository-owned, PathMap-backed representation of Rholang parallel composition.
//!
//! [`Par`] is the target of the `.rhoapi.Par` external-schema cutover. Its one
//! component authority is [`ParBag`], a prefix-compressed
//! `PathMap<Multiplicity>`. Root `locally_free` and `connective_used` values are
//! retained only as normalization summaries; there is no repeated-field or
//! decoded component shadow.

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Range;

use prost::bytes::{Buf, BufMut};
use prost::encoding::wire_type::WireType;
use prost::encoding::{self, DecodeContext};
use prost::{DecodeError, Message};
use rspace_plus_plus::rspace::bag::{BagError, Multiplicity};
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::par_bag::{
    encode_protobuf_varint_chunk, EncodedParComponent, ParBag, ParBagComponent, ParBagOwnerKey,
    ParBagProtobufError, ParBagVisitError, ParComponentItem, ParProtobufProjectionPreparation,
};
use crate::rhoapi::{
    Bundle, Connective, CostTokenStack, Expr, GUnforgeable, If, Match, New, Receive, Send,
};
use crate::rust::rholang::sorter::score_tree::{Score, ScoreAtom, Tree};

#[derive(Debug)]
pub enum ParProtobufError {
    Components(ParBagProtobufError),
    RootMetadataLengthOverflow,
    ComponentRangeOwnerMismatch,
    InvalidComponentRanges,
    EncodedLengthMismatch { expected: usize, actual: usize },
}

impl fmt::Display for ParProtobufError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Components(error) => error.fmt(formatter),
            Self::RootMetadataLengthOverflow => {
                formatter.write_str("Par root metadata overflows the protobuf output length")
            }
            Self::ComponentRangeOwnerMismatch => formatter.write_str(
                "protobuf component ranges belong to a different ParBag copy-on-write owner",
            ),
            Self::InvalidComponentRanges => formatter.write_str(
                "protobuf component ranges do not name two ordered regions of the existing output",
            ),
            Self::EncodedLengthMismatch { expected, actual } => write!(
                formatter,
                "Par protobuf encoded_len predicted {expected} bytes but emission wrote {actual}"
            ),
        }
    }
}

impl std::error::Error for ParProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Components(error) => Some(error),
            Self::RootMetadataLengthOverflow
            | Self::ComponentRangeOwnerMismatch
            | Self::InvalidComponentRanges
            | Self::EncodedLengthMismatch { .. } => None,
        }
    }
}

/// Exact protobuf lengths obtained from one ordered ParBag metric pass.
///
/// The component length excludes root fields 9 (`locally_free`) and 10
/// (`connective_used`). Certified outer-message preflight needs both values;
/// returning them together avoids rereading all nine family metrics while
/// preserving the established before-root → root → after-root error order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct ParProtobufMeasurement {
    pub encoded_len: usize,
    pub component_fields_encoded_len: usize,
}

/// Two component-only regions already resident in a private protobuf frame.
///
/// Root fields 9 and 10 are deliberately excluded: two [`Par`] values may
/// share the same component owner while carrying different root metadata.
/// Later occurrences therefore copy `before_root`, emit their own metadata,
/// and then copy `after_root`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct ParProtobufComponentRanges {
    owner: ParBagOwnerKey,
    before_root: Range<usize>,
    after_root: Range<usize>,
}

impl ParProtobufComponentRanges {
    #[doc(hidden)]
    pub fn owner_key(&self) -> &ParBagOwnerKey { &self.owner }

    #[doc(hidden)]
    pub fn component_bytes(&self) -> usize {
        (self.before_root.end - self.before_root.start)
            + (self.after_root.end - self.after_root.start)
    }
}

impl From<ParBagProtobufError> for ParProtobufError {
    fn from(error: ParBagProtobufError) -> Self { Self::Components(error) }
}

/// Error from streaming canonical protobuf chunks to a caller-owned sink.
#[derive(Debug)]
pub enum ParProtobufSinkError<E> {
    Protobuf(ParProtobufError),
    Sink(E),
}

impl<E: fmt::Display> fmt::Display for ParProtobufSinkError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protobuf(error) => error.fmt(formatter),
            Self::Sink(error) => write!(formatter, "protobuf sink rejected a chunk: {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ParProtobufSinkError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Protobuf(error) => Some(error),
            Self::Sink(error) => Some(error),
        }
    }
}

#[repr(C)]
#[derive(Clone, Default, utoipa::ToSchema)]
pub struct Par {
    #[schema(value_type = Vec<u8>)]
    components: ParBag,
    /// Wire-authoritative cached root summary. This remains public for source
    /// compatibility with the generated protobuf `Par`; unlike component
    /// families, exposing it does not create a decoded shadow of the Bag.
    pub locally_free: Vec<u8>,
    /// Wire-authoritative cached root summary. Callers may construct
    /// non-canonical protobuf messages, so this retains the generated API's
    /// public mutability while component storage remains encapsulated.
    pub connective_used: bool,
}

impl Par {
    pub fn from_parts(components: ParBag, locally_free: Vec<u8>, connective_used: bool) -> Self {
        Self {
            components,
            locally_free,
            connective_used,
        }
    }

    pub const fn components(&self) -> &ParBag { &self.components }

    pub fn into_components(self) -> ParBag { self.components }

    /// Borrow the canonical prefix-compressed Bag image. The image is derived
    /// once, memoized by `ParBag`, and shared by CoW clones.
    pub fn compact_snapshot(&self) -> &[u8] { self.components.compact_snapshot() }

    pub fn locally_free(&self) -> &[u8] { &self.locally_free }

    pub const fn connective_used(&self) -> bool { self.connective_used }

    /// Merge one already-decoded protobuf component into the authoritative
    /// component Bag without re-deriving root metadata.  This deliberately
    /// preserves Prost's wire contract: tags 9 and 10 remain the authority for
    /// the serialized root summaries, including on non-canonical input.
    pub(crate) fn merge_protobuf_component<T: ParBagComponent>(
        &mut self,
        value: &T,
    ) -> Result<(), DecodeError> {
        insert_component(&mut self.components, value)
    }

    fn merge_canonical_expr_protobuf_fast_path(
        &mut self,
        expr_body: &[u8],
    ) -> Option<Result<(), DecodeError>> {
        self.components
            .insert_canonical_expr_protobuf_fast_path(expr_body)
            .map(|result| result.map_err(|error| par_decode_error(error.to_string())))
    }

    pub(crate) fn merge_protobuf_locally_free<B: Buf>(
        &mut self,
        wire_type: WireType,
        buf: &mut B,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        encoding::bytes::merge(wire_type, &mut self.locally_free, buf, ctx)
    }

    pub(crate) fn merge_protobuf_connective_used<B: Buf>(
        &mut self,
        wire_type: WireType,
        buf: &mut B,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        encoding::bool::merge(wire_type, &mut self.connective_used, buf, ctx)
    }

    /// Whether the complete authoritative component Bag has no endpoints.
    /// This is distinct from historical [`Self::is_empty`], whose Rholang
    /// contract intentionally examines only process-form families.
    #[inline]
    pub fn components_is_empty(&self) -> bool { self.components.is_empty() }

    pub const fn distinct_len(&self) -> usize { self.components.distinct_len() }

    pub const fn cardinality(&self) -> u64 { self.components.cardinality() }

    pub const fn cardinality_is_saturated(&self) -> bool {
        self.components.cardinality_is_saturated()
    }

    pub fn contains<T: ParBagComponent>(&self, value: &T) -> bool {
        self.components.contains(value)
    }

    pub fn count<T: ParBagComponent>(&self, value: &T) -> u64 { self.components.count(value) }

    pub fn family_distinct_len<T: ParBagComponent>(&self) -> usize {
        self.components.family_distinct_len::<T>()
    }

    /// Whether every authoritative component belongs to family `T`.
    ///
    /// This is an exact O(1) endpoint-domain test. It deliberately compares
    /// distinct endpoint counts rather than projected cardinalities: logical
    /// multiplicity counters saturate at `u64::MAX`, while the presence of one
    /// endpoint from another family must remain observable even when both
    /// projected cardinalities are saturated. The empty `Par` satisfies this
    /// predicate vacuously; callers that require one or more `T` occurrences
    /// must check that arity separately.
    pub fn all_components_are<T: ParBagComponent>(&self) -> bool {
        self.distinct_len() == self.family_distinct_len::<T>()
    }

    pub fn family_cardinality<T: ParBagComponent>(&self) -> u64 {
        self.components.family_cardinality::<T>()
    }

    pub fn family_cardinality_exact<T: ParBagComponent>(&self) -> u128 {
        self.components.family_cardinality_exact::<T>()
    }

    pub fn family_cardinality_is_saturated<T: ParBagComponent>(&self) -> bool {
        self.components.family_cardinality_is_saturated::<T>()
    }

    pub fn visit_encoded<T: ParBagComponent>(
        &self,
        visitor: impl FnMut(EncodedParComponent<'_, T>),
    ) {
        self.components.visit_encoded(visitor);
    }

    /// Cached, byte-native proof that every concrete component endpoint has an
    /// empty locally-free fold. See [`ParBag::concrete_components_locally_free_are_empty`].
    pub fn concrete_components_locally_free_are_empty(&self) -> Option<bool> {
        self.components.concrete_components_locally_free_are_empty()
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

    /// Feed an iterative consumer one owned value per distinct trie endpoint.
    /// Multiplicity remains separate, so the consumer can apply it
    /// algebraically instead of repeating decode or traversal work.
    pub fn try_visit_components<'a, T, E>(
        &'a self,
        mut visitor: impl FnMut(ParComponentItem<'a, T>, Multiplicity) -> Result<(), E>,
    ) -> Result<(), ParBagVisitError<E>>
    where
        T: ParBagComponent,
    {
        self.components.try_visit_decoded(|value, multiplicity| {
            visitor(ParComponentItem::Owned(value), multiplicity)
        })
    }

    /// Consuming worklist feed. PathMap endpoints remain canonical bytes, so
    /// each distinct value is decoded exactly once and transferred directly
    /// to the caller with its multiplicity.
    pub fn try_into_components<T, E>(
        self,
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

    /// Materialize one typed family for a semantic consumer that genuinely
    /// needs owned decoded values (principally compatibility boundaries and
    /// independent test oracles). This is an explicit projection, never a
    /// resident shadow of the authoritative prefix-compressed Bag.
    pub fn decode_occurrences<T: ParBagComponent + Clone>(&self) -> Vec<T> {
        let mut values = Vec::new();
        self.try_visit_occurrences::<T, std::convert::Infallible>(|value| {
            values.push(value.clone());
            Ok(())
        })
        .expect("canonical resident ParBag endpoints decode");
        values
    }

    /// Remove and materialize one typed component family.
    ///
    /// This is the PathMap-backed equivalent of `mem::take` on one of the
    /// historical generated `Vec<T>` fields.  It is deliberately an explicit
    /// compatibility/ownership boundary: resident `Par` values retain no
    /// decoded shadow, while worklist machines that need owned children decode
    /// every distinct trie endpoint once and expand only its multiplicity.
    pub fn take_occurrences<T: ParBagComponent + Clone>(&mut self) -> Vec<T> {
        let values = self.decode_occurrences::<T>();
        self.components.remove_family::<T>();
        values
    }

    /// Move the root locally-free summary out without exposing mutable access
    /// to the authoritative representation.
    pub fn take_locally_free(&mut self) -> Vec<u8> { std::mem::take(&mut self.locally_free) }

    /// Move the root connective summary out, leaving the neutral value.
    pub fn take_connective_used(&mut self) -> bool { std::mem::take(&mut self.connective_used) }

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

    pub fn scale_occurrences(self, factor: u64) -> Result<Self, BagError> {
        match factor {
            0 => Ok(Self::default()),
            1 => Ok(self),
            _ => Ok(Self {
                components: self.components.scale(factor)?,
                locally_free: self.locally_free,
                connective_used: self.connective_used,
            }),
        }
    }

    fn replace_family<T: ParBagComponent>(&mut self, values: Vec<T>) {
        self.components.remove_family::<T>();
        for value in values {
            self.components
                .insert(&value)
                .expect("a finite component vector cannot overflow u64 multiplicity");
        }
    }

    pub fn replace_sends(&mut self, values: Vec<Send>) { self.replace_family(values) }

    pub fn replace_receives(&mut self, values: Vec<Receive>) { self.replace_family(values) }

    pub fn replace_news(&mut self, values: Vec<New>) { self.replace_family(values) }

    pub fn replace_exprs(&mut self, values: Vec<Expr>) { self.replace_family(values) }

    pub fn replace_matches(&mut self, values: Vec<Match>) { self.replace_family(values) }

    pub fn replace_unforgeables(&mut self, values: Vec<GUnforgeable>) {
        self.replace_family(values)
    }

    pub fn replace_bundles(&mut self, values: Vec<Bundle>) { self.replace_family(values) }

    pub fn replace_connectives(&mut self, values: Vec<Connective>) { self.replace_family(values) }

    pub fn replace_conditionals(&mut self, values: Vec<If>) { self.replace_family(values) }

    pub fn replace_cost_token_stacks(&mut self, values: Vec<CostTokenStack>) {
        self.replace_family(values)
    }

    pub fn replace_locally_free(&mut self, locally_free: Vec<u8>) {
        self.locally_free = locally_free;
    }

    pub const fn replace_connective_used(&mut self, connective_used: bool) {
        self.connective_used = connective_used;
    }

    fn with_replaced<T: ParBagComponent>(&self, values: Vec<T>) -> Self {
        let mut result = self.clone();
        result.replace_family(values);
        result
    }

    pub fn with_sends(&self, values: Vec<Send>) -> Self { self.with_replaced(values) }

    pub fn with_receives(&self, values: Vec<Receive>) -> Self { self.with_replaced(values) }

    pub fn with_news(&self, values: Vec<New>) -> Self { self.with_replaced(values) }

    pub fn with_exprs(&self, values: Vec<Expr>) -> Self { self.with_replaced(values) }

    pub fn with_matches(&self, values: Vec<Match>) -> Self { self.with_replaced(values) }

    pub fn with_unforgeables(&self, values: Vec<GUnforgeable>) -> Self {
        self.with_replaced(values)
    }

    pub fn with_bundles(&self, values: Vec<Bundle>) -> Self { self.with_replaced(values) }

    pub fn with_connectives(&self, values: Vec<Connective>) -> Self { self.with_replaced(values) }

    pub fn with_conditionals(&self, values: Vec<If>) -> Self { self.with_replaced(values) }

    pub fn with_cost_token_stacks(&self, values: Vec<CostTokenStack>) -> Self {
        self.with_replaced(values)
    }

    pub fn with_locally_free(&self, locally_free: Vec<u8>) -> Self {
        let mut result = self.clone();
        result.replace_locally_free(locally_free);
        result
    }

    pub fn with_connective_used(&self, connective_used: bool) -> Self {
        let mut result = self.clone();
        result.replace_connective_used(connective_used);
        result
    }

    pub fn prepend_send(mut self, value: Send) -> Self {
        let locally_free = value.locally_free.clone();
        let connective_used = value.connective_used;
        self.insert_component(&value, locally_free, connective_used)
            .expect("one owned Send cannot overflow u64 multiplicity");
        self
    }

    pub fn prepend_receive(mut self, value: Receive) -> Self {
        let locally_free = value.locally_free.clone();
        let connective_used = value.connective_used;
        self.insert_component(&value, locally_free, connective_used)
            .expect("one owned Receive cannot overflow u64 multiplicity");
        self
    }

    pub fn prepend_match(mut self, value: Match) -> Self {
        let locally_free = value.locally_free.clone();
        let connective_used = value.connective_used;
        self.insert_component(&value, locally_free, connective_used)
            .expect("one owned Match cannot overflow u64 multiplicity");
        self
    }

    pub fn prepend_if(mut self, value: If) -> Self {
        let locally_free = value.locally_free.clone();
        let connective_used = value.connective_used;
        self.insert_component(&value, locally_free, connective_used)
            .expect("one owned If cannot overflow u64 multiplicity");
        self
    }

    /// Historical Rholang process-emptiness predicate. Unforgeables,
    /// connectives, and conditionals are deliberately outside this contract.
    pub fn is_empty(&self) -> bool {
        self.family_cardinality::<Send>() == 0
            && self.family_cardinality::<Receive>() == 0
            && self.family_cardinality::<New>() == 0
            && self.family_cardinality::<Match>() == 0
            && self.family_cardinality::<Bundle>() == 0
            && self.family_cardinality::<Expr>() == 0
    }

    pub fn is_nil(&self) -> bool { self.components_is_empty() }

    pub fn single_connective(&self) -> Option<Connective> {
        (self.cardinality() == 1 && self.family_cardinality::<Connective>() == 1).then(|| {
            self.first::<Connective>()
                .expect("Par invariant: stored Connective keys are canonical")
                .expect("the cardinality check proved one Connective")
                .0
        })
    }

    pub fn single_bundle(&self) -> Option<Bundle> {
        (self.cardinality() == 1 && self.family_cardinality::<Bundle>() == 1).then(|| {
            self.first::<Bundle>()
                .expect("Par invariant: stored Bundle keys are canonical")
                .expect("the cardinality check proved one Bundle")
                .0
        })
    }

    pub fn append(&self, other: Self) -> Self {
        self.clone()
            .parallel(other)
            .expect("two finite Par values cannot overflow u64 multiplicity")
    }

    /// Wrap `terminal` in `depth` singleton ELists in one byte-native pass.
    ///
    /// This is the bulk-construction counterpart to repeated `with_exprs`
    /// calls. It preserves the exact protobuf and canonical-sort image while
    /// avoiding repeated copying of every already-built suffix.
    pub fn wrap_unary_elist(depth: usize, terminal: Self) -> Option<Self> {
        super::par_bag::wrap_canonical_unary_elist_par(depth, terminal)
    }

    /// The ETuple specialization of [`Par::wrap_unary_elist`].
    pub fn wrap_unary_etuple(depth: usize, terminal: Self) -> Option<Self> {
        super::par_bag::wrap_canonical_unary_etuple_par(depth, terminal)
    }

    /// Recognize an exact singleton `EList([..EList([GInt])..])` directly in
    /// the authoritative component key. No nested `Par` is decoded.
    pub fn canonical_unary_elist_gint_parts(&self) -> Option<(usize, i64)> {
        if !self.locally_free.is_empty()
            || self.connective_used
            || self.cardinality() != 1
            || self.family_cardinality::<Expr>() != 1
        {
            return None;
        }
        let mut result = None;
        self.visit_encoded::<Expr>(|entry| {
            if entry.multiplicity().get() == 1 {
                result = entry.canonical_unary_elist_parts();
            }
        });
        result
    }

    /// Recognize an exact singleton `ETuple(..ETuple(GInt)..)` directly in
    /// the authoritative component key. No nested `Par` is decoded.
    pub fn canonical_unary_etuple_gint_parts(&self) -> Option<(usize, i64)> {
        if !self.locally_free.is_empty()
            || self.connective_used
            || self.cardinality() != 1
            || self.family_cardinality::<Expr>() != 1
        {
            return None;
        }
        let mut result = None;
        self.visit_encoded::<Expr>(|entry| {
            if entry.multiplicity().get() == 1 {
                result = entry.canonical_unary_etuple_parts();
            }
        });
        result
    }

    /// Peel an exact non-empty spine of unary AND/OR connectives directly from
    /// the canonical component bytes and decode only its terminal `Par`.
    ///
    /// Singleton conjunction and disjunction are both logical identities.
    /// Candidate-plan compilation and spatial matching can therefore consume
    /// the terminal without allocating any intermediate connective or `Par`.
    pub fn canonical_unary_connective_terminal(&self) -> Option<Self> {
        if !self.connective_used()
            || self.cardinality() != 1
            || self.family_cardinality::<Connective>() != 1
            || !self.all_components_are::<Connective>()
        {
            return None;
        }
        let mut result = None;
        self.visit_encoded::<Connective>(|entry| {
            if entry.multiplicity().get() == 1 {
                result = entry.canonical_unary_connective_terminal();
            }
        });
        result
    }

    /// Peel an exact non-empty unary AND/OR/NOT spine. The returned boolean is
    /// the parity of NOT nodes; a true value also tells the spatial matcher to
    /// restore the pre-spine binding map after matching the terminal.
    pub fn canonical_unary_connective_spine(&self) -> Option<(Self, bool, bool)> {
        if !self.connective_used()
            || self.cardinality() != 1
            || self.family_cardinality::<Connective>() != 1
            || !self.all_components_are::<Connective>()
        {
            return None;
        }
        let mut result = None;
        self.visit_encoded::<Connective>(|entry| {
            if entry.multiplicity().get() == 1 {
                result = entry.canonical_unary_connective_spine();
            }
        });
        result
    }

    /// Peel an exact non-empty unary Bundle spine and decode only its terminal
    /// process. Bundle flags constrain channel use when a bundle is a value;
    /// they do not alter evaluation of its body. Reducer callers must preserve
    /// the ordinary path when per-bundle observation is active.
    pub fn canonical_unary_bundle_terminal(&self) -> Option<Self> {
        if self.cardinality() != 1
            || self.family_cardinality::<Bundle>() != 1
            || !self.all_components_are::<Bundle>()
        {
            return None;
        }
        let mut result = None;
        self.visit_encoded::<Bundle>(|entry| {
            if entry.multiplicity().get() == 1 {
                result = entry.canonical_unary_bundle_terminal();
            }
        });
        result
    }

    #[doc(hidden)]
    #[inline]
    pub fn protobuf_egress_measurement(&self) -> Result<ParProtobufMeasurement, ParProtobufError> {
        let mut total = 0usize;
        self.components
            .add_protobuf_components_before_root_metadata_len(&mut total)?;
        let root_metadata_start = total;
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
        let root_metadata_len = total - root_metadata_start;
        self.components
            .add_protobuf_components_after_root_metadata_len(&mut total)?;
        // Every component contribution was added to `total` before or after
        // the contiguous root-metadata region, so subtracting that region is
        // exact and cannot underflow.
        Ok(ParProtobufMeasurement {
            encoded_len: total,
            component_fields_encoded_len: total - root_metadata_len,
        })
    }

    pub fn protobuf_encoded_len(&self) -> Result<usize, ParProtobufError> {
        self.protobuf_egress_measurement()
            .map(|measurement| measurement.encoded_len)
    }

    /// Exact protobuf bytes contributed by the nine component families,
    /// excluding root `locally_free` and `connective_used` metadata.
    ///
    /// Certified outer-message preflight calls [`Self::protobuf_encoded_len`]
    /// first to retain its established before-root → root → after-root error
    /// precedence. This component-only query is then an O(1) read of the
    /// authoritative ParBag metrics used solely to decide whether owner-local
    /// component reuse can save work.
    #[doc(hidden)]
    pub fn protobuf_component_fields_encoded_len(&self) -> Result<usize, ParProtobufError> {
        self.components
            .protobuf_components_encoded_len()
            .map_err(Into::into)
    }

    pub fn encode_protobuf(&self, output: &mut impl BufMut) -> Result<(), ParProtobufError> {
        // Validate the complete expanded length before publishing any prefix
        // to the caller. This keeps impossible Bag multiplicities atomic even
        // when callers invoke `encode_raw` without first asking `encoded_len`.
        let _ = self.protobuf_encoded_len()?;
        let mut sink = |chunk: &[u8]| {
            output.put_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        };
        match self.try_visit_protobuf_chunks_prevalidated(&mut sink) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        Ok(())
    }

    fn try_visit_root_metadata_chunks<E>(
        &self,
        visitor: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        if !self.locally_free.is_empty() {
            const LOCALLY_FREE_KEY: [u8; 1] = [(9 << 3) | 2];
            let mut length = [0u8; 10];
            let length_bytes =
                encode_protobuf_varint_chunk(self.locally_free.len() as u64, &mut length);
            visitor(&LOCALLY_FREE_KEY)?;
            visitor(&length[..length_bytes])?;
            visitor(&self.locally_free)?;
        }
        if self.connective_used {
            const CONNECTIVE_USED_TRUE: [u8; 2] = [(10 << 3), 1];
            visitor(&CONNECTIVE_USED_TRUE)?;
        }
        Ok(())
    }

    fn try_visit_protobuf_chunks_prevalidated<E>(
        &self,
        visitor: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        match self.components.protobuf_projection_if_prepared() {
            Some((before_root, after_root)) => {
                if !before_root.is_empty() {
                    visitor(before_root)?;
                }
                self.try_visit_root_metadata_chunks(visitor)?;
                if !after_root.is_empty() {
                    visitor(after_root)?;
                }
            }
            None => self
                .components
                .try_visit_protobuf_chunks_direct_around_root_metadata(visitor, |visitor| {
                    self.try_visit_root_metadata_chunks(visitor)
                })?,
        }
        Ok(())
    }

    fn try_visit_protobuf_chunks_direct_prevalidated<E>(
        &self,
        visitor: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        self.components
            .try_visit_protobuf_chunks_direct_around_root_metadata(visitor, |visitor| {
                self.try_visit_root_metadata_chunks(visitor)
            })
    }

    /// Opaque identity of the component Bag's copy-on-write auxiliary owner.
    ///
    /// This is a response-local optimization witness.  It is neither semantic
    /// equality nor a serialized identifier: independently constructed equal
    /// Pars intentionally have different keys.
    #[doc(hidden)]
    #[inline]
    pub fn protobuf_owner_key(&self) -> ParBagOwnerKey { self.components.owner_key() }

    /// Emit one complete `Par` directly into a private frame and remember the
    /// two component-only ranges that were written.
    ///
    /// The authoritative RPB3 PathMap is traversed exactly once. Root metadata
    /// is emitted between the ranges because it is not part of the shared
    /// component owner. No projection allocation or live-owner cache is made.
    #[doc(hidden)]
    pub fn encode_protobuf_recording_component_ranges(
        &self,
        output: &mut Vec<u8>,
    ) -> Result<ParProtobufComponentRanges, ParProtobufError> {
        let expected = self.protobuf_encoded_len()?;
        self.encode_protobuf_recording_component_ranges_prevalidated(output, expected)
    }

    /// Emit one complete `Par` after an enclosing encoder has already proved
    /// its exact body length.
    ///
    /// This is the cross-crate seam used by the generated certified egress
    /// traversal. The caller supplies the length captured from this same
    /// immutable `Par` during the enclosing message's mandatory preflight.
    /// The method still verifies the emitted delta and rolls the private frame
    /// back on disagreement; it merely avoids repeating the component-length
    /// calculation for every occurrence during pass two.
    #[doc(hidden)]
    pub fn encode_protobuf_recording_component_ranges_prevalidated(
        &self,
        output: &mut Vec<u8>,
        expected: usize,
    ) -> Result<ParProtobufComponentRanges, ParProtobufError> {
        let start = output.len();
        let written = Cell::new(start);
        let mut before_root_end = None;
        let mut after_root_start = None;
        let mut append = |chunk: &[u8]| {
            output.extend_from_slice(chunk);
            written.set(
                written
                    .get()
                    .checked_add(chunk.len())
                    .expect("private protobuf frame length already fits addressable memory"),
            );
            Ok::<_, std::convert::Infallible>(())
        };
        let result = self
            .components
            .try_visit_protobuf_chunks_direct_around_root_metadata(&mut append, |append| {
                before_root_end = Some(written.get());
                self.try_visit_root_metadata_chunks(append)?;
                after_root_start = Some(written.get());
                Ok::<_, std::convert::Infallible>(())
            });
        match result {
            Ok(()) => {}
            Err(never) => match never {},
        }

        let actual = output.len() - start;
        if actual != expected {
            output.truncate(start);
            return Err(ParProtobufError::EncodedLengthMismatch { expected, actual });
        }
        let before_root_end = before_root_end
            .expect("the component chunk driver must place root metadata exactly once");
        let after_root_start = after_root_start
            .expect("the component chunk driver must place root metadata exactly once");
        Ok(ParProtobufComponentRanges {
            owner: self.protobuf_owner_key(),
            before_root: start..before_root_end,
            after_root: after_root_start..output.len(),
        })
    }

    /// Emit this `Par` by copying component bytes already present in `output`.
    ///
    /// The ranges must have been recorded for the same copy-on-write owner and
    /// must name bytes that predate this append. Invalid or mismatched ranges
    /// leave `output` unchanged. The current value's root metadata is always
    /// emitted freshly, so metadata-only clones remain byte-exact.
    #[doc(hidden)]
    pub fn encode_protobuf_from_component_ranges(
        &self,
        output: &mut Vec<u8>,
        ranges: &ParProtobufComponentRanges,
    ) -> Result<(), ParProtobufError> {
        let expected = self.protobuf_encoded_len()?;
        self.encode_protobuf_from_component_ranges_prevalidated(output, ranges, expected)
    }

    /// Reuse component ranges after an enclosing encoder has already proved
    /// this occurrence's exact protobuf body length.
    ///
    /// Owner/range validation happens before publication. The final byte delta
    /// is compared with `expected`, and any disagreement truncates the private
    /// frame back to its entry length. Consequently the optimization cannot
    /// publish a partially or incorrectly framed message.
    #[doc(hidden)]
    pub fn encode_protobuf_from_component_ranges_prevalidated(
        &self,
        output: &mut Vec<u8>,
        ranges: &ParProtobufComponentRanges,
        expected: usize,
    ) -> Result<(), ParProtobufError> {
        if self.protobuf_owner_key() != ranges.owner {
            return Err(ParProtobufError::ComponentRangeOwnerMismatch);
        }
        let start = output.len();
        let valid = ranges.before_root.start <= ranges.before_root.end
            && ranges.before_root.end <= ranges.after_root.start
            && ranges.after_root.start <= ranges.after_root.end
            && ranges.after_root.end <= start;
        if !valid {
            return Err(ParProtobufError::InvalidComponentRanges);
        }

        output.extend_from_within(ranges.before_root.clone());
        let mut append = |chunk: &[u8]| {
            output.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        };
        match self.try_visit_root_metadata_chunks(&mut append) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        output.extend_from_within(ranges.after_root.clone());

        let actual = output.len() - start;
        if actual != expected {
            output.truncate(start);
            return Err(ParProtobufError::EncodedLengthMismatch { expected, actual });
        }
        Ok(())
    }

    /// Stream the canonical protobuf image as callback-scoped borrowed chunks.
    ///
    /// A prepared component projection is reused when present; otherwise the
    /// authoritative RPB3 PathMap is traversed once and remains cold. Complete
    /// output length is validated before the first callback, so an invalid
    /// multiplicity publishes no prefix. A callback error can follow earlier
    /// successful chunks: durable callers must write into an uncommitted WAL
    /// transaction or equivalent private staging area and publish only after
    /// this method returns `Ok(())`.
    pub fn try_visit_protobuf_chunks<E>(
        &self,
        mut visitor: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), ParProtobufSinkError<E>> {
        let _ = self
            .protobuf_encoded_len()
            .map_err(ParProtobufSinkError::Protobuf)?;
        self.try_visit_protobuf_chunks_prevalidated(&mut visitor)
            .map_err(ParProtobufSinkError::Sink)
    }

    /// Strict cold-control form of [`Self::try_visit_protobuf_chunks`].
    /// Existing prepared bytes are ignored and no projection is retained.
    pub fn try_visit_protobuf_chunks_direct<E>(
        &self,
        mut visitor: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), ParProtobufSinkError<E>> {
        let _ = self
            .protobuf_encoded_len()
            .map_err(ParProtobufSinkError::Protobuf)?;
        self.try_visit_protobuf_chunks_direct_prevalidated(&mut visitor)
            .map_err(ParProtobufSinkError::Sink)
    }

    /// Explicitly retain a CoW-shared component projection for a measured
    /// repeated-use owner, subject to that owner's byte budget.
    pub fn prepare_protobuf_projection(
        &self,
        max_retained_bytes: usize,
    ) -> Result<ParProtobufProjectionPreparation, ParProtobufError> {
        self.components
            .prepare_protobuf_projection(max_retained_bytes)
            .map_err(Into::into)
    }

    /// Emit one final protobuf image without retaining a second component
    /// projection in the shared `ParBag` auxiliary root.
    ///
    /// This is the appropriate route for one-shot hash, transport, and owned
    /// `Vec` boundaries. [`Self::encode_protobuf`] remains the reusable route:
    /// it can share memoized projections across CoW clones and repeated
    /// occurrences in a larger generated message.
    pub fn encode_protobuf_direct(&self, output: &mut impl BufMut) -> Result<(), ParProtobufError> {
        let _ = self.protobuf_encoded_len()?;
        let mut sink = |chunk: &[u8]| {
            output.put_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        };
        match self.try_visit_protobuf_chunks_direct_prevalidated(&mut sink) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        Ok(())
    }

    /// Stream a preflighted `Par` directly into a caller-private `Vec`.
    ///
    /// Certified outer egress uses this for component-empty values and owners
    /// that its exact census proved occur only once. It avoids both a repeated
    /// length calculation and the owner/range bookkeeping needed only by a
    /// reusable owner. A byte-count disagreement restores the exact entry
    /// prefix before returning an error.
    #[doc(hidden)]
    pub fn encode_protobuf_direct_to_vec_prevalidated(
        &self,
        output: &mut Vec<u8>,
        expected: usize,
    ) -> Result<(), ParProtobufError> {
        let start = output.len();
        let mut sink = |chunk: &[u8]| {
            output.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        };
        match self.try_visit_protobuf_chunks_direct_prevalidated(&mut sink) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        let actual = output.len() - start;
        if actual != expected {
            output.truncate(start);
            return Err(ParProtobufError::EncodedLengthMismatch { expected, actual });
        }
        Ok(())
    }

    pub fn protobuf_bytes(&self) -> Result<Vec<u8>, ParProtobufError> {
        let mut output = Vec::with_capacity(self.protobuf_encoded_len()?);
        let mut sink = |chunk: &[u8]| {
            output.put_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        };
        match self.try_visit_protobuf_chunks_direct_prevalidated(&mut sink) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        Ok(output)
    }

    /// Reconstruct the canonical sorter score directly from the score prefixes
    /// embedded in the authoritative component trie keys.  Component bodies
    /// remain opaque canonical protobuf bytes; the term itself is already in
    /// canonical Bag form and therefore needs only an O(1) CoW clone.
    pub(crate) fn canonical_score(&self) -> Tree<ScoreAtom> {
        let mut children = Vec::new();
        for tag in crate::rust::rholang::canonical_semantics::PAR_SCORE_TAGS {
            match tag {
                super::par_bag::ParComponentTag::Send => {
                    self.components.append_family_scores::<Send>(&mut children)
                }
                super::par_bag::ParComponentTag::Receive => self
                    .components
                    .append_family_scores::<Receive>(&mut children),
                super::par_bag::ParComponentTag::Expr => {
                    self.components.append_family_scores::<Expr>(&mut children)
                }
                super::par_bag::ParComponentTag::New => {
                    self.components.append_family_scores::<New>(&mut children)
                }
                super::par_bag::ParComponentTag::Match => {
                    self.components.append_family_scores::<Match>(&mut children)
                }
                super::par_bag::ParComponentTag::Bundle => self
                    .components
                    .append_family_scores::<Bundle>(&mut children),
                super::par_bag::ParComponentTag::Connective => self
                    .components
                    .append_family_scores::<Connective>(&mut children),
                super::par_bag::ParComponentTag::GUnforgeable => self
                    .components
                    .append_family_scores::<GUnforgeable>(&mut children),
                super::par_bag::ParComponentTag::If => {
                    self.components.append_family_scores::<If>(&mut children)
                }
                super::par_bag::ParComponentTag::CostTokenStack => self
                    .components
                    .append_family_scores::<CostTokenStack>(&mut children),
            }
        }
        children.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(
            self.connective_used,
        )));
        Tree::<ScoreAtom>::create_node_from_i32(Score::PAR, children)
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

impl fmt::Debug for Par {
    /// Trie-native diagnostics for the trie-native representation.
    ///
    /// Expanding every canonical component body here would reconstruct the
    /// recursive AST shadow that `ParBag` deliberately eliminated. Besides
    /// making `Debug` O(total decoded tree), nested components would re-enter
    /// `Par::fmt` through prost's generated message formatters and consume one
    /// native frame per term level. Keep diagnostics on the authoritative
    /// PathMap root instead; the Rholang pretty-printer is the stack-safe API
    /// for a complete source-like rendering.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Par")
            .field("components", &self.components)
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

/// Human-readable compatibility view of one component family. JSON and other
/// diagnostic formats retain the established structured Rholang schema, but
/// the authoritative Bag remains compressed and no repeated-field shadow is
/// kept resident. Each distinct endpoint is decoded once and emitted for its
/// stored multiplicity directly into the serializer.
struct HumanReadableFamily<'a, T> {
    components: &'a ParBag,
    marker: PhantomData<T>,
}

impl<'a, T> HumanReadableFamily<'a, T> {
    const fn new(components: &'a ParBag) -> Self {
        Self {
            components,
            marker: PhantomData,
        }
    }
}

impl<T> Serialize for HumanReadableFamily<'_, T>
where T: ParBagComponent + Serialize
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let length = usize::try_from(self.components.family_cardinality::<T>()).ok();
        let mut sequence = serializer.serialize_seq(length)?;
        match self
            .components
            .try_visit_occurrences::<T, S::Error>(|value| sequence.serialize_element(value))
        {
            Ok(()) => sequence.end(),
            Err(ParBagVisitError::Codec(error)) => Err(serde::ser::Error::custom(error)),
            Err(ParBagVisitError::Visitor(error)) => Err(error),
        }
    }
}

impl Serialize for Par {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        if serializer.is_human_readable() {
            let mut state = serializer.serialize_struct("Par", 11)?;
            state.serialize_field("sends", &HumanReadableFamily::<Send>::new(&self.components))?;
            state.serialize_field(
                "receives",
                &HumanReadableFamily::<Receive>::new(&self.components),
            )?;
            state.serialize_field("news", &HumanReadableFamily::<New>::new(&self.components))?;
            state.serialize_field("exprs", &HumanReadableFamily::<Expr>::new(&self.components))?;
            state.serialize_field(
                "matches",
                &HumanReadableFamily::<Match>::new(&self.components),
            )?;
            state.serialize_field(
                "unforgeables",
                &HumanReadableFamily::<GUnforgeable>::new(&self.components),
            )?;
            state.serialize_field(
                "bundles",
                &HumanReadableFamily::<Bundle>::new(&self.components),
            )?;
            state.serialize_field(
                "connectives",
                &HumanReadableFamily::<Connective>::new(&self.components),
            )?;
            state.serialize_field(
                "conditionals",
                &HumanReadableFamily::<If>::new(&self.components),
            )?;
            state.serialize_field("locally_free", &self.locally_free)?;
            state.serialize_field("connective_used", &self.connective_used)?;
            return state.end();
        }

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

#[derive(Deserialize)]
struct HumanReadableParImage {
    #[serde(default)]
    sends: Vec<Send>,
    #[serde(default)]
    receives: Vec<Receive>,
    #[serde(default)]
    news: Vec<New>,
    #[serde(default)]
    exprs: Vec<Expr>,
    #[serde(default)]
    matches: Vec<Match>,
    #[serde(default)]
    unforgeables: Vec<GUnforgeable>,
    #[serde(default)]
    bundles: Vec<Bundle>,
    #[serde(default)]
    connectives: Vec<Connective>,
    #[serde(default)]
    conditionals: Vec<If>,
    #[serde(default)]
    locally_free: Vec<u8>,
    #[serde(default)]
    connective_used: bool,
}

impl HumanReadableParImage {
    fn into_par(self) -> Result<Par, BagError> {
        let mut components = ParBag::new();
        macro_rules! insert_family {
            ($values:expr) => {
                for value in &$values {
                    components.insert(value)?;
                }
            };
        }
        insert_family!(self.sends);
        insert_family!(self.receives);
        insert_family!(self.news);
        insert_family!(self.exprs);
        insert_family!(self.matches);
        insert_family!(self.unforgeables);
        insert_family!(self.bundles);
        insert_family!(self.connectives);
        insert_family!(self.conditionals);
        Ok(Par::from_parts(
            components,
            self.locally_free,
            self.connective_used,
        ))
    }
}

impl<'de> Deserialize<'de> for Par {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: Deserializer<'de> {
        if deserializer.is_human_readable() {
            return HumanReadableParImage::deserialize(deserializer)?
                .into_par()
                .map_err(serde::de::Error::custom);
        }
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

#[cfg(test)]
#[path = "../../tests/support/protobuf_par_frozen.rs"]
mod protobuf_field_oracle;

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
                self.merge_protobuf_component(&value)
            }};
        }

        #[cfg(test)]
        if crate::protobuf_decoder_oracle::enter_decoder() {
            let mut erased: &mut dyn Buf = buf;
            return protobuf_field_oracle::merge_field(self, tag, wire_type, &mut erased, ctx);
        }
        #[cfg(test)]
        crate::protobuf_decoder_oracle::assert_successor_body();

        match tag {
            1 => merge_component!(Send, crate::rust::rholang::protobuf_decoder::decode_send),
            2 => merge_component!(
                Receive,
                crate::rust::rholang::protobuf_decoder::decode_receive
            ),
            4 => merge_component!(New, crate::rust::rholang::protobuf_decoder::decode_new),
            5 => {
                let mut body = take_length_delimited_component(wire_type, buf)?;
                let direct = (body.chunk().len() == body.remaining())
                    .then(|| self.merge_canonical_expr_protobuf_fast_path(body.chunk()))
                    .flatten();
                if let Some(result) = direct {
                    let remaining = body.remaining();
                    body.advance(remaining);
                    return result;
                }
                let value = crate::rust::rholang::protobuf_decoder::decode_expr(body)?;
                self.merge_protobuf_component(&value)
            }
            6 => merge_component!(Match, crate::rust::rholang::protobuf_decoder::decode_match),
            7 => merge_component!(
                GUnforgeable,
                crate::rust::rholang::protobuf_decoder::decode_g_unforgeable
            ),
            8 => merge_component!(
                Connective,
                crate::rust::rholang::protobuf_decoder::decode_connective
            ),
            9 => self.merge_protobuf_locally_free(wire_type, buf, ctx),
            10 => self.merge_protobuf_connective_used(wire_type, buf, ctx),
            11 => merge_component!(
                Bundle,
                crate::rust::rholang::protobuf_decoder::decode_bundle
            ),
            12 => merge_component!(If, crate::rust::rholang::protobuf_decoder::decode_if),
            13 => merge_component!(
                CostTokenStack,
                crate::rust::rholang::protobuf_decoder::decode_cost_token_stack
            ),
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
