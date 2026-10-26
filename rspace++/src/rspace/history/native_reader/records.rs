//! DR-131 P2a: the decoding of one native history row through the cold-store
//! decode boundary (`ColdStoreDecode`).
//!
//! The bodies follow `decode_datum`, `decode_continuation` and `decode_join`
//! of `feature/mettail`
//! (`5108d8543:rspace++/src/rspace/serializers/serializers.rs` :287-296,
//! :348-367 and :397-401). They build this branch's `Datum` and
//! `WaitingContinuation`, which hold their values directly instead of in an
//! `Arc`. Bincode is positional, so a record is the concatenation of its
//! fields. The `Par`-bearing prefix decodes through `ColdStoreDecode`, which
//! reports how many bytes it consumed, and the bounded tail decodes as one
//! bincode tuple (`legacy_tail`).
//!
//! S6 of the host-work removal replaced the typed decoder (`typed.rs`, now
//! deleted) with these functions at every call site.

use std::collections::BTreeSet;

use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, cold_decode_vec_prefix, legacy_tail,
};
use crate::rspace::trace::event::{Consume, Produce};

/// Decodes one data row: `Datum { a: A, persist: bool, source: Produce }`.
pub fn decode_datum<A: Clone + ColdStoreDecode>(
    bytes: &[u8],
) -> Result<Datum<A>, ColdStoreDecodeError> {
    let (a, consumed) = A::cold_decode_prefix(bytes)?;
    let (persist, source): (bool, Produce) = legacy_tail(&bytes[consumed..])?;
    Ok(Datum { a, persist, source })
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
    let (patterns, mut consumed) = cold_decode_vec_prefix::<P>(bytes)?;
    let (continuation, used) = K::cold_decode_prefix(&bytes[consumed..])?;
    consumed += used;
    let (persist, peeks, source): (bool, BTreeSet<i32>, Consume) = legacy_tail(&bytes[consumed..])?;
    Ok(WaitingContinuation {
        patterns,
        continuation,
        persist,
        peeks,
        source,
    })
}

/// Decodes one join row: the channels of one join, `Vec<C>`.
pub fn decode_join<C: Clone + ColdStoreDecode>(
    bytes: &[u8],
) -> Result<Vec<C>, ColdStoreDecodeError> {
    cold_decode_vec_prefix::<C>(bytes).map(|(join, _consumed)| join)
}

#[cfg(test)]
mod tests;
