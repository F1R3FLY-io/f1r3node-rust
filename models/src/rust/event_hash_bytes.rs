//! Stack-safe, byte-identical event-hash serialization.
//!
//! Event hashes cover the legacy bincode bytes of channels, datums, patterns,
//! and continuations. The former intern-store splice path and its `contains_par`
//! dispatch scan were deleted with the intern store: EPathMap now stores PathMap
//! directly and bincode emits its canonical EPM1 snapshot through the generated
//! schema. Every rhoapi root below therefore has one unconditional explicit-
//! worklist encoder and no recursive serde fallback.

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;

use crate::rhoapi::{BindPattern, ListParWithRandom, Par, ParWithRandom, TaggedContinuation};
use crate::rust::rholang::bincode_encoder::{self, ColdStoreEncode};

pub fn event_hash_bytes_list_par_with_random(datum: &ListParWithRandom) -> Vec<u8> {
    datum.cold_encode()
}

pub fn event_hash_bytes_bind_pattern(pattern: &BindPattern) -> Vec<u8> { pattern.cold_encode() }

pub fn event_hash_bytes_tagged_continuation(continuation: &TaggedContinuation) -> Vec<u8> {
    continuation.cold_encode()
}

impl StableHashSerialize for ListParWithRandom {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_list_par_with_random(self) }
}

impl StableHashSerialize for BindPattern {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_bind_pattern(self) }
}

impl StableHashSerialize for TaggedContinuation {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_tagged_continuation(self) }
}

impl StableHashSerialize for Par {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_par("bincode_encoder", self);
        bincode_encoder::encode(self)
    }
}

impl StableHashSerialize for ParWithRandom {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_par_with_random(
            "bincode_encoder",
            self,
        );
        bincode_encoder::encode(self)
    }
}
