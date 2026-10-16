//! D-S2 (DR-95): the RSpace history record types are closed history types:
//! plain serde derives over vectors, booleans and hashes. The peek set of a
//! waiting continuation decodes through the tree-set tag (DR-95 part 1).

use shared::rust::closed_decode::ClosedDecode;

use super::hashing::blake2b256_hash::Blake2b256Hash;
use super::serializers::serializers::{DatumOracleDe, WaitingContinuationOracleDe};
use super::trace::event::{Consume, Produce};

// SAFETY: a newtype over a byte vector.
unsafe impl ClosedDecode for Blake2b256Hash {}
// SAFETY: plain derives over hashes, booleans and byte vectors.
unsafe impl ClosedDecode for Produce {}
// SAFETY: plain derives over hashes and a boolean.
unsafe impl ClosedDecode for Consume {}
unsafe impl<A: ClosedDecode> ClosedDecode for DatumOracleDe<A> {}
unsafe impl<P: ClosedDecode, K: ClosedDecode> ClosedDecode for WaitingContinuationOracleDe<P, K> {}
