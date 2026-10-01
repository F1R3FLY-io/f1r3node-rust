use std::cell::RefCell;
use std::fmt;
use std::mem::size_of;

use bincode::Options;
use serde::Deserializer;
use serde::de::{
    DeserializeOwned, DeserializeSeed, EnumAccess, Error, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use shared::rust::collection_backing::{hash_backing, tree_backing};

use super::{NativeReadCharge, NativeReadError, NativeReadFault, NativeReadMeter};

const MAX_DEPTH: usize = 128;

struct Budget<'a, M: NativeReadMeter + ?Sized> {
    meter: &'a M,
    failure: RefCell<Option<NativeReadError<M::Error>>>,
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

    fn value<T, E: Error>(&self, depth: usize) -> Result<(), E> {
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
}

struct Decoder<'a, 'm, D, M: NativeReadMeter + ?Sized> {
    inner: D,
    budget: &'a Budget<'m, M>,
    depth: usize,
}

struct Guard<'a, 'm, V, M: NativeReadMeter + ?Sized> {
    inner: V,
    budget: &'a Budget<'m, M>,
    depth: usize,
}

macro_rules! decode {
    ($($method:ident $(($($arg:ident: $ty:ty),*))?),* $(,)?) => {
        $(fn $method<V: Visitor<'de>>(self, $($($arg: $ty,)*)? visitor: V)
            -> Result<V::Value, D::Error>
        {
            self.budget.value::<V::Value, D::Error>(self.depth)?;
            self.inner.$method($($($arg,)*)? Guard {
                inner: visitor,
                budget: self.budget,
                depth: self.depth + 1,
            })
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
        deserialize_newtype_struct(name: &'static str), deserialize_seq,
        deserialize_tuple(len: usize), deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_map, deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier, deserialize_ignored_any,
    );

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
        self.inner.visit_some(Decoder {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(Decoder {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
}

impl<'de, S: DeserializeSeed<'de>, M: NativeReadMeter + ?Sized> DeserializeSeed<'de>
    for Guard<'_, '_, S, M>
{
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.budget.value::<S::Value, D::Error>(self.depth)?;
        self.inner.deserialize(Decoder {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
}

impl<'de, A: SeqAccess<'de>, M: NativeReadMeter + ?Sized> SeqAccess<'de> for Guard<'_, '_, A, M> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_element_seed(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn size_hint(&self) -> Option<usize> { None }
}

impl<'de, A: MapAccess<'de>, M: NativeReadMeter + ?Sized> MapAccess<'de> for Guard<'_, '_, A, M> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_key_seed(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
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
        let (value, inner) = self.inner.variant_seed(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })?;
        Ok((value, Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        }))
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
        self.inner.newtype_variant_seed(Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, inner: V) -> Result<V::Value, Self::Error> {
        self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.inner.tuple_variant(len, Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        inner: V,
    ) -> Result<V::Value, Self::Error> {
        self.budget.value::<V::Value, Self::Error>(self.depth)?;
        self.inner.struct_variant(fields, Guard {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
}

pub fn decode_record<T: DeserializeOwned, M: NativeReadMeter + ?Sized>(
    bytes: &[u8],
    meter: &M,
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
    };
    let options = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(
            u64::try_from(bytes.len())
                .map_err(|_| NativeReadError::Invalid(NativeReadFault::Overflow))?,
        );
    let mut decoder = bincode::de::Deserializer::from_slice(bytes, options);
    let value = T::deserialize(Decoder {
        inner: &mut decoder,
        budget: &budget,
        depth: 0,
    });
    if let Some(error) = budget.failure.into_inner() {
        return Err(error);
    }
    value.map_err(|_| NativeReadError::Invalid(NativeReadFault::TypedRecord))
}

#[cfg(test)]
mod tests;
