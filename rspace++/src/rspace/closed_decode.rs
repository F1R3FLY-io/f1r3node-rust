//! D-S2 (DR-95): the RSpace history record types are closed history types:
//! plain serde derives over vectors, booleans and hashes. The peek set of a
//! waiting continuation decodes through the tree-set tag (DR-95 part 1).

use shared::rust::closed_decode::ClosedDecode;

use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::internal::{Datum, WaitingContinuation};
use super::trace::event::{Consume, Produce};

// SAFETY: a newtype over a byte vector.
unsafe impl ClosedDecode for Blake2b256Hash {}
// SAFETY: plain derives over hashes, booleans and byte vectors.
unsafe impl ClosedDecode for Produce {}
// SAFETY: plain derives over hashes and a boolean.
unsafe impl ClosedDecode for Consume {}
// SAFETY: plain derives over a closed payload, a boolean and a produce.
unsafe impl<A: Clone + ClosedDecode> ClosedDecode for Datum<A> {}
// SAFETY: plain derives over closed patterns and continuation, a boolean, a
// consume and a peek set that decodes through the tree-set tag.
unsafe impl<P: Clone + ClosedDecode, K: Clone + ClosedDecode> ClosedDecode
    for WaitingContinuation<P, K>
{
}
