//! DR-131 P2a: the cold-store decode boundary (`ColdStoreDecode`) for the four
//! roots of the native history records: `Par` (the channels of a join),
//! `ListParWithRandom` (the payload of a datum), `BindPattern` (the patterns
//! of a continuation) and `TaggedContinuation` (its body).
//!
//! In P2a every root decodes through `legacy_prefix`. That is the derived
//! bincode decoder with the options of dev's reader, so it still recurses
//! once for each nesting level. P2b replaces these bodies with an
//! explicit-stack machine adapted from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/src/rust/rholang/bincode_decoder.rs`).
//!
//! `feature/mettail` implements the same trait for the same types in its own
//! machine (`5108d8543:models/src/rust/rholang/bincode_decoder.rs:2024-2116`).
//! At the merge of that branch the two sets of implementations collide
//! (E0119), and the collision marks this file for reconciliation.

use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    legacy_prefix, ColdStoreDecode, ColdStoreDecodeError,
};

use crate::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};

impl ColdStoreDecode for Par {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

impl ColdStoreDecode for ListParWithRandom {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

impl ColdStoreDecode for BindPattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

impl ColdStoreDecode for TaggedContinuation {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}
