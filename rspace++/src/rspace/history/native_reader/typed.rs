use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;

use bincode::Options;
use serde::de::{
    DeserializeOwned, DeserializeSeed, EnumAccess, Error, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::{Deserialize, Deserializer};
use shared::rust::closed_decode::ClosedDecode;
use shared::rust::collection_backing::{hash_backing, tree_backing, tree_growth};

use super::{NativeReadCharge, NativeReadError, NativeReadFault, NativeReadMeter};

const MAX_DEPTH: usize = 128;

/// D-S2 (DR-95): the newtype name that marks a B-tree set for the metered
/// history decoder. Bincode ignores newtype names, so the tag does not change
/// the wire format.
pub const NATIVE_TREE_SET: &str = "NativeTreeSet";

/// D-S2 (DR-95): deserializes a B-tree set through the tree-set tag, so that
/// the metered history decoder can recognize the set and charge its node
/// allocations. Use it with `#[serde(deserialize_with = ...)]`.
pub fn tree_set<'de, D, T>(deserializer: D) -> Result<BTreeSet<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Ord,
{
    struct Tagged<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de> + Ord> Visitor<'de> for Tagged<T> {
        type Value = BTreeSet<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a tagged tree set")
        }

        fn visit_newtype_struct<D: Deserializer<'de>>(
            self,
            inner: D,
        ) -> Result<Self::Value, D::Error> {
            BTreeSet::deserialize(inner)
        }
    }

    deserializer.deserialize_newtype_struct(NATIVE_TREE_SET, Tagged(PhantomData))
}

/// D-S2 (DR-95): the charge rule of a decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The generic rule for records of any type: every node reserves a worst
    /// case for whatever its type might allocate.
    Legacy,
    /// The exact rule for closed history types (`ClosedDecode`): one charge
    /// per node, and backing only for vector growth, tagged B-tree sets,
    /// B-tree maps and strings.
    History,
}

/// D-S2 (DR-95): what a node's visitor allocates while it decodes the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Nothing: a struct, tuple, option, enum or scalar.
    Inline,
    /// A vector that grows by pushes from capacity zero.
    Seq,
    /// A B-tree set (behind the tree-set tag).
    TreeSet,
    /// A B-tree map.
    Map,
}

/// D-S2 (DR-95): the capacity of a vector after one push at length `len`
/// with capacity `cap` (Rust's `RawVec::grow_amortized`;
/// `NativeDecodeBacking.grown`).
fn grown(element: usize, cap: usize, len: usize) -> Option<usize> {
    if len < cap {
        return Some(cap);
    }
    let minimum = if element == 1 {
        8
    } else if element <= 1024 {
        4
    } else {
        1
    };
    Some(cap.checked_mul(2)?.max(len.checked_add(1)?).max(minimum))
}

struct Budget<'a, M: NativeReadMeter + ?Sized> {
    meter: &'a M,
    failure: RefCell<Option<NativeReadError<M::Error>>>,
    mode: Mode,
}

impl<M: NativeReadMeter + ?Sized> Budget<'_, M> {
    fn reserve<E: Error>(&self, backing: usize, scanned: usize) -> Result<(), E> {
        if self.failure.borrow().is_some() {
            return Err(E::custom("native record reservation failed"));
        }
        match self.meter.reserve(NativeReadCharge {
            operations: 1,
            scanned_bytes: scanned,
            backing_bytes: backing,
        }) {
            Ok(()) => Ok(()),
            Err(error) => {
                *self.failure.borrow_mut() = Some(NativeReadError::Host(error));
                Err(E::custom("native record reservation failed"))
            }
        }
    }

    fn charge<E: Error>(&self, operations: usize, scanned: usize, backing: usize) -> Result<(), E> {
        if self.failure.borrow().is_some() {
            return Err(E::custom("native record reservation failed"));
        }
        match self.meter.reserve(NativeReadCharge {
            operations,
            scanned_bytes: scanned,
            backing_bytes: backing,
        }) {
            Ok(()) => Ok(()),
            Err(error) => {
                *self.failure.borrow_mut() = Some(NativeReadError::Host(error));
                Err(E::custom("native record reservation failed"))
            }
        }
    }

    fn overflow<E: Error>(&self) -> E {
        *self.failure.borrow_mut() = Some(NativeReadError::Invalid(NativeReadFault::Overflow));
        E::custom("native record backing overflow")
    }

    /// D-S2 (DR-95): the History charge of one node: one operation and the
    /// node's size, no backing. The legacy rule reserves a worst case.
    fn value<T, E: Error>(&self, depth: usize) -> Result<(), E> {
        if self.mode == Mode::History {
            if depth >= MAX_DEPTH {
                *self.failure.borrow_mut() = Some(NativeReadError::Invalid(NativeReadFault::Depth));
                return Err(E::custom("native record depth or backing overflow"));
            }
            return self.reserve(0, size_of::<T>());
        }
        let backing = size_of::<T>()
            .checked_mul(8)
            .and_then(|n| n.checked_add(tree_backing::<T, ()>(1)?.1))
            .and_then(|n| n.checked_add(hash_backing::<T, ()>(1)?.1));
        if depth >= MAX_DEPTH || backing.is_none() {
            let fault = if depth >= MAX_DEPTH {
                NativeReadFault::Depth
            } else {
                NativeReadFault::Overflow
            };
            *self.failure.borrow_mut() = Some(NativeReadError::Invalid(fault));
            return Err(E::custom("native record depth or backing overflow"));
        }
        self.reserve(backing.unwrap(), size_of::<T>())
    }

    /// A charge that the legacy rule repeats for a node that its deserializer
    /// call already charged: the charge at the seed hook and the charge of a
    /// tuple or struct variant's body. Disabled in History mode by D-S2
    /// (DR-95): a node is charged once, at its deserializer call.
    fn repeat<T, E: Error>(&self, depth: usize) -> Result<(), E> {
        match self.mode {
            Mode::Legacy => self.value::<T, E>(depth),
            Mode::History => Ok(()),
        }
    }

    /// D-S2 (DR-95): before element `len` of a vector at capacity `cap`, the
    /// growth that the push of that element makes: the new buffer, and the
    /// copy of the old elements. Returns the new capacity.
    fn seq_growth<T, E: Error>(&self, cap: usize, len: usize) -> Result<usize, E> {
        let element = size_of::<T>();
        let next = grown(element, cap, len).ok_or_else(|| self.overflow())?;
        if next != cap {
            let backing = next.checked_mul(element).ok_or_else(|| self.overflow())?;
            let copied = len.checked_mul(element).ok_or_else(|| self.overflow())?;
            self.charge(1, copied, backing)?;
        }
        Ok(next)
    }

    /// D-S2 (DR-95): before insert `index` of a B-tree, the increment of its
    /// tree backing (`IncrementalTreeBacking`).
    fn tree_insert<K, V, E: Error>(&self, index: usize) -> Result<(), E> {
        let (operations, backing) = tree_growth::<K, V>(index, 1).ok_or_else(|| self.overflow())?;
        self.charge(operations, 0, backing)
    }
}

struct Decoder<'a, 'm, D, M: NativeReadMeter + ?Sized> {
    inner: D,
    budget: &'a Budget<'m, M>,
    depth: usize,
    /// D-S2 (DR-95): the value is the set inside the tree-set tag.
    tree_set: bool,
}

struct Guard<'a, 'm, V, M: NativeReadMeter + ?Sized> {
    inner: V,
    budget: &'a Budget<'m, M>,
    depth: usize,
    /// D-S2 (DR-95): what the guarded visitor allocates, the tag flag, and the
    /// length and capacity of a growing vector.
    kind: Kind,
    tree_set: bool,
    len: usize,
    cap: usize,
}

impl<'a, 'm, V, M: NativeReadMeter + ?Sized> Guard<'a, 'm, V, M> {
    fn new(inner: V, budget: &'a Budget<'m, M>, depth: usize) -> Self {
        Self::with_kind(inner, budget, depth, Kind::Inline)
    }

    fn with_kind(inner: V, budget: &'a Budget<'m, M>, depth: usize, kind: Kind) -> Self {
        Self {
            inner,
            budget,
            depth,
            kind,
            tree_set: false,
            len: 0,
            cap: 0,
        }
    }
}

impl<'a, 'm, D, M: NativeReadMeter + ?Sized> Decoder<'a, 'm, D, M> {
    fn new(inner: D, budget: &'a Budget<'m, M>, depth: usize) -> Self {
        Self {
            inner,
            budget,
            depth,
            tree_set: false,
        }
    }
}

macro_rules! decode {
    ($($method:ident $(($($arg:ident: $ty:ty),*))?),* $(,)?) => {
        $(fn $method<V: Visitor<'de>>(self, $($($arg: $ty,)*)? visitor: V)
            -> Result<V::Value, D::Error>
        {
            self.budget.value::<V::Value, D::Error>(self.depth)?;
            self.inner.$method($($($arg,)*)? Guard::new(visitor, self.budget, self.depth + 1))
        })*
    };
}

impl<'de, D: Deserializer<'de>, M: NativeReadMeter + ?Sized> Deserializer<'de>
    for Decoder<'_, '_, D, M>
{
    type Error = D::Error;

    decode!(
        deserialize_any, deserialize_bool, deserialize_i8, deserialize_i16, deserialize_i32,
        deserialize_i64, deserialize_i128, deserialize_u8, deserialize_u16, deserialize_u32,
        deserialize_u64, deserialize_u128, deserialize_f32, deserialize_f64, deserialize_char,
        deserialize_str, deserialize_bytes, deserialize_option, deserialize_unit,
        deserialize_unit_struct(name: &'static str),
        deserialize_tuple(len: usize), deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier, deserialize_ignored_any,
    );

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name == NATIVE_TREE_SET {
            // D-S2 (DR-95): the tag is not a node of its own; the tagged set is
            // charged when it is decoded.
            let mut guard = Guard::new(visitor, self.budget, self.depth);
            guard.tree_set = true;
            return self.inner.deserialize_newtype_struct(name, guard);
        }
        self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.inner
            .deserialize_newtype_struct(name, Guard::new(visitor, self.budget, self.depth + 1))
    }

    /// D-S2 (DR-95): a sequence is a vector unless it is the set inside the
    /// tree-set tag.
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.budget.value::<V::Value, Self::Error>(self.depth)?;
        let kind = if self.tree_set {
            Kind::TreeSet
        } else {
            Kind::Seq
        };
        self.inner
            .deserialize_seq(Guard::with_kind(visitor, self.budget, self.depth + 1, kind))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.inner.deserialize_map(Guard::with_kind(
            visitor,
            self.budget,
            self.depth + 1,
            Kind::Map,
        ))
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_bytes(visitor)
    }

    fn is_human_readable(&self) -> bool { self.inner.is_human_readable() }
}

macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(fn $method<E: Error>(self, value: $ty) -> Result<Self::Value, E> {
            self.inner.$method(value)
        })*
    };
}

impl<'de, V: Visitor<'de>, M: NativeReadMeter + ?Sized> Visitor<'de> for Guard<'_, '_, V, M> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(formatter)
    }

    scalar!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_f32(f32),
        visit_f64(f64),
        visit_char(char),
    );

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        self.budget.reserve(value.len(), value.len())?;
        self.inner.visit_str(value)
    }

    fn visit_borrowed_str<E: Error>(self, value: &'de str) -> Result<Self::Value, E> {
        self.budget.reserve(value.len(), value.len())?;
        self.inner.visit_borrowed_str(value)
    }

    fn visit_bytes<E: Error>(self, value: &[u8]) -> Result<Self::Value, E> {
        self.budget.reserve(value.len(), value.len())?;
        self.inner.visit_bytes(value)
    }

    fn visit_borrowed_bytes<E: Error>(self, value: &'de [u8]) -> Result<Self::Value, E> {
        self.budget.reserve(value.len(), value.len())?;
        self.inner.visit_borrowed_bytes(value)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> { self.inner.visit_none() }
    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> { self.inner.visit_unit() }

    fn visit_some<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner
            .visit_some(Decoder::new(inner, self.budget, self.depth))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        let mut decoder = Decoder::new(inner, self.budget, self.depth);
        decoder.tree_set = self.tree_set;
        self.inner.visit_newtype_struct(decoder)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_seq(Guard::with_kind(inner, self.budget, self.depth, self.kind))
    }

    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_map(Guard::with_kind(inner, self.budget, self.depth, self.kind))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner
            .visit_enum(Guard::new(inner, self.budget, self.depth))
    }
}

impl<'de, S: DeserializeSeed<'de>, M: NativeReadMeter + ?Sized> DeserializeSeed<'de>
    for Guard<'_, '_, S, M>
{
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        // Changed by D-S2 (DR-95): in History mode a node is charged once, at
        // its deserializer call.
        // self.budget.value::<S::Value, D::Error>(self.depth)?;
        self.budget.repeat::<S::Value, D::Error>(self.depth)?;
        self.inner
            .deserialize(Decoder::new(inner, self.budget, self.depth))
    }
}

impl<'de, A: SeqAccess<'de>, M: NativeReadMeter + ?Sized> SeqAccess<'de> for Guard<'_, '_, A, M> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        // D-S2 (DR-95): in History mode, reserve what the push of this element
        // allocates before decoding it, unless no element remains.
        if self.budget.mode == Mode::History && self.inner.size_hint() != Some(0) {
            match self.kind {
                Kind::Seq => {
                    self.cap = self
                        .budget
                        .seq_growth::<S::Value, Self::Error>(self.cap, self.len)?;
                }
                Kind::TreeSet => {
                    self.budget
                        .tree_insert::<S::Value, (), Self::Error>(self.len)?;
                }
                Kind::Inline | Kind::Map => {}
            }
        }
        let value = self
            .inner
            .next_element_seed(Guard::new(inner, self.budget, self.depth))?;
        if value.is_some() {
            self.len += 1;
        }
        Ok(value)
    }

    fn size_hint(&self) -> Option<usize> { None }
}

impl<'de, A: MapAccess<'de>, M: NativeReadMeter + ?Sized> MapAccess<'de> for Guard<'_, '_, A, M> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        // D-S2 (DR-95): in History mode, the key's share of the insert's tree
        // backing, before the key.
        if self.budget.mode == Mode::History && self.inner.size_hint() != Some(0) {
            self.budget
                .tree_insert::<S::Value, (), Self::Error>(self.len)?;
        }
        self.inner
            .next_key_seed(Guard::new(inner, self.budget, self.depth))
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        // D-S2 (DR-95): in History mode, the value's share of the insert's
        // tree backing, before the value.
        if self.budget.mode == Mode::History {
            self.budget
                .tree_insert::<(), S::Value, Self::Error>(self.len)?;
        }
        let value = self
            .inner
            .next_value_seed(Guard::new(inner, self.budget, self.depth))?;
        self.len += 1;
        Ok(value)
    }

    fn size_hint(&self) -> Option<usize> { None }
}

impl<'de, 'a, 'm, A: EnumAccess<'de>, M: NativeReadMeter + ?Sized> EnumAccess<'de>
    for Guard<'a, 'm, A, M>
{
    type Error = A::Error;
    type Variant = Guard<'a, 'm, A::Variant, M>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        inner: S,
    ) -> Result<(S::Value, Self::Variant), Self::Error> {
        let (value, inner) = self
            .inner
            .variant_seed(Guard::new(inner, self.budget, self.depth))?;
        Ok((value, Guard::new(inner, self.budget, self.depth)))
    }
}

impl<'de, A: VariantAccess<'de>, M: NativeReadMeter + ?Sized> VariantAccess<'de>
    for Guard<'_, '_, A, M>
{
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), Self::Error> { self.inner.unit_variant() }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(
        self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner
            .newtype_variant_seed(Guard::new(inner, self.budget, self.depth))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, inner: V) -> Result<V::Value, Self::Error> {
        // Changed by D-S2 (DR-95): deserialize_enum already charged the enum
        // node, so History mode does not charge it again.
        // self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.budget.repeat::<V::Value, Self::Error>(self.depth)?;
        self.inner
            .tuple_variant(len, Guard::new(inner, self.budget, self.depth))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        inner: V,
    ) -> Result<V::Value, Self::Error> {
        // Changed by D-S2 (DR-95): deserialize_enum already charged the enum
        // node, so History mode does not charge it again.
        // self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.budget.repeat::<V::Value, Self::Error>(self.depth)?;
        self.inner
            .struct_variant(fields, Guard::new(inner, self.budget, self.depth))
    }
}

/// The generic rule: decodes a record of any type and reserves a worst case
/// for every node.
pub fn decode_record<T: DeserializeOwned, M: NativeReadMeter + ?Sized>(
    bytes: &[u8],
    meter: &M,
) -> Result<T, NativeReadError<M::Error>> {
    decode_with_mode(bytes, meter, Mode::Legacy)
}

/// D-S2 (DR-95): the exact rule for closed history types: one charge per
/// node, and backing only for the allocations the decode makes, each reserved
/// before it happens (`NativeDecodeBacking.v`).
pub fn decode_history_record<T: DeserializeOwned + ClosedDecode, M: NativeReadMeter + ?Sized>(
    bytes: &[u8],
    meter: &M,
) -> Result<T, NativeReadError<M::Error>> {
    decode_with_mode(bytes, meter, Mode::History)
}

fn decode_with_mode<T: DeserializeOwned, M: NativeReadMeter + ?Sized>(
    bytes: &[u8],
    meter: &M,
    mode: Mode,
) -> Result<T, NativeReadError<M::Error>> {
    super::reserve(
        meter,
        1,
        0,
        size_of::<bincode::ErrorKind>()
            .checked_add(64)
            .ok_or(NativeReadError::Invalid(NativeReadFault::Overflow))?,
    )?;
    let budget = Budget {
        meter,
        failure: RefCell::new(None),
        mode,
    };
    let options = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(
            u64::try_from(bytes.len())
                .map_err(|_| NativeReadError::Invalid(NativeReadFault::Overflow))?,
        );
    let mut decoder = bincode::de::Deserializer::from_slice(bytes, options);
    let value = T::deserialize(Decoder::new(&mut decoder, &budget, 0));
    if let Some(error) = budget.failure.into_inner() {
        return Err(error);
    }
    value.map_err(|_| NativeReadError::Invalid(NativeReadFault::TypedRecord))
}

#[cfg(test)]
mod tests;
