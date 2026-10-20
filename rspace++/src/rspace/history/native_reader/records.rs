use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, cold_decode_vec_prefix,
};

/// Decodes one data row: `Datum { a: A, persist: bool, source: Produce }`.
pub fn decode_datum<A: Clone + ColdStoreDecode>(
    bytes: &[u8],
) -> Result<Datum<A>, ColdStoreDecodeError> {
    crate::rspace::serializers::serializers::decode_datum(bytes)
}

/// Decodes one continuation row: `WaitingContinuation { patterns: Vec<P>,
/// continuation: K, persist: bool, peeks: BTreeSet<i32>, source: Consume }`.
pub fn decode_continuation<P, K>(
    bytes: &[u8],
) -> Result<WaitingContinuation<P, K>, ColdStoreDecodeError>
where
    P: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    crate::rspace::serializers::serializers::decode_continuation(bytes)
}

/// Decodes one join row: the channels of one join, `Vec<C>`.
pub fn decode_join<C: Clone + ColdStoreDecode>(
    bytes: &[u8],
) -> Result<Vec<C>, ColdStoreDecodeError> {
    cold_decode_vec_prefix::<C>(bytes).map(|(join, _consumed)| join)
}

#[cfg(test)]
mod tests;
