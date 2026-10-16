//! Stack-safe, byte-identical event-hash serialization.
//!
//! Event hashes cover the legacy bincode bytes of channels, datums, patterns,
//! and continuations. The former intern-store splice path and its `contains_par`
//! dispatch scan were deleted with the intern store: EPathMap now stores PathMap
//! directly and bincode emits its canonical EPM1 snapshot through the generated
//! schema. Every rhoapi root below therefore has one unconditional explicit-
//! worklist encoder and no recursive serde fallback.

use std::mem::size_of;

use rspace_plus_plus::rspace::errors::{HistoryError, RSpaceError};
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{
    BincodeByteSink, StableHashSerialize,
};

use crate::rhoapi::cost_signature::Value as CostValue;
use crate::rhoapi::tagged_continuation::TaggedCont;
use crate::rhoapi::{
    BindPattern, CostAuthority, CostSignature, CostStack, ListParWithRandom, Par, ParWithRandom,
    TaggedContinuation,
};
use crate::rust::canonical_path::CodecError;
use crate::rust::rholang::bincode_encoder::{self, ColdStoreEncode};
use crate::rust::rholang::par_children::{
    visit_canonical_par_tree, visit_canonical_par_tree_charged, CanonicalVisitFailure,
    CanonicalVisitStep,
};

fn invalid_canonical_channel(error: CodecError) -> RSpaceError {
    RSpaceError::HistoryError(HistoryError::DecodeError(error.to_string()))
}

fn invalid_cost_v1_branch() -> RSpaceError {
    RSpaceError::InterpreterError(
        "CostStack.parallel_tail requires the CostV2 event codec epoch".into(),
    )
}

fn charge_canonical_visit(
    step: CanonicalVisitStep,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    match step {
        CanonicalVisitStep::BorrowedPar => Ok(()),
        CanonicalVisitStep::DecodedKeyPar => meter.reserve(1, size_of::<Par>(), 0),
        CanonicalVisitStep::EncodedKey(bytes) => {
            let backing = bytes
                .checked_mul(size_of::<Par>())
                .ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(1, bytes, backing)
        }
    }
}

/// Inspect every canonical Par reached through ordinary fields, cost
/// signatures, and EPathMap keys/values. A branch at any depth makes the
/// protocol-v6 positional layout lossy and must be rejected before writing.
fn validate_par_cost_v1(root: &Par) -> Result<(), RSpaceError> {
    let mut branched = false;
    visit_canonical_par_tree(root, |par| {
        branched |= par
            .cost_stacks
            .iter()
            .any(|stack| !stack.parallel_tail.is_empty());
    })
    .map_err(invalid_canonical_channel)?;
    if branched {
        Err(invalid_cost_v1_branch())
    } else {
        Ok(())
    }
}

fn validate_par_cost_v1_metered(root: &Par, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
    let mut branched = false;
    visit_canonical_par_tree_charged(
        root,
        |par| {
            branched |= par
                .cost_stacks
                .iter()
                .any(|stack| !stack.parallel_tail.is_empty());
        },
        |step| charge_canonical_visit(step, meter),
    )
    .map_err(|error| match error {
        CanonicalVisitFailure::Codec(error) => invalid_canonical_channel(error),
        CanonicalVisitFailure::Charge(error) => error,
    })?;
    if branched {
        Err(invalid_cost_v1_branch())
    } else {
        Ok(())
    }
}

/// A signature may quote a Par containing a CostStack. Traverse arbitrarily
/// nested compound signatures iteratively, then use the canonical Par visitor
/// for the quote; this also covers signature roots held directly by a datum
/// stack or by CostAuthority.
fn validate_signatures_cost_v1<'a>(
    roots: impl IntoIterator<Item = &'a CostSignature>,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    let mut work: Vec<&CostSignature> = Vec::new();
    for signature in roots {
        if let Some(meter) = meter {
            meter.reserve(1, size_of::<&CostSignature>(), size_of::<&CostSignature>())?;
        }
        work.try_reserve_exact(1)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        work.push(signature);
    }
    while let Some(signature) = work.pop() {
        match &signature.value {
            Some(CostValue::Quote(par) | CostValue::Name(par)) => {
                if let Some(meter) = meter {
                    validate_par_cost_v1_metered(par, meter)?;
                } else {
                    validate_par_cost_v1(par)?;
                }
            }
            Some(CostValue::Compound(compound)) => {
                for element in &compound.elements {
                    if let Some(meter) = meter {
                        meter.reserve(
                            1,
                            size_of::<&CostSignature>(),
                            size_of::<&CostSignature>(),
                        )?;
                    }
                    work.try_reserve_exact(1)
                        .map_err(|_| RSpaceError::HostWorkRejected)?;
                    work.push(element);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_authority_cost_v1(
    authority: &CostAuthority,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    validate_signatures_cost_v1(
        authority
            .regions
            .iter()
            .filter_map(|region| region.signature.as_ref()),
        meter,
    )
}

fn validate_direct_stack_cost_v1(
    stack: &CostStack,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    if !stack.parallel_tail.is_empty() {
        return Err(invalid_cost_v1_branch());
    }
    validate_signatures_cost_v1(stack.cells.iter(), meter)
}

fn validate_datum_cost_v1(
    datum: &ListParWithRandom,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    if let Some(stack) = &datum.cost_stack {
        validate_direct_stack_cost_v1(stack, meter)?;
    }
    if let Some(authority) = &datum.cost_authority {
        validate_authority_cost_v1(authority, meter)?;
    }
    for par in &datum.pars {
        if let Some(meter) = meter {
            validate_par_cost_v1_metered(par, meter)?;
        } else {
            validate_par_cost_v1(par)?;
        }
    }
    Ok(())
}

fn validate_pattern_cost_v1(
    pattern: &BindPattern,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    for par in &pattern.patterns {
        if let Some(meter) = meter {
            validate_par_cost_v1_metered(par, meter)?;
        } else {
            validate_par_cost_v1(par)?;
        }
    }
    Ok(())
}

fn validate_continuation_cost_v1(
    continuation: &TaggedContinuation,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    if let Some(authority) = &continuation.cost_authority {
        validate_authority_cost_v1(authority, meter)?;
    }
    if let Some(par) = &continuation.guard {
        if let Some(meter) = meter {
            validate_par_cost_v1_metered(par, meter)?;
        } else {
            validate_par_cost_v1(par)?;
        }
    }
    if let Some(TaggedCont::ParBody(body)) = &continuation.tagged_cont {
        if let Some(par) = &body.body {
            if let Some(meter) = meter {
                validate_par_cost_v1_metered(par, meter)?;
            } else {
                validate_par_cost_v1(par)?;
            }
        }
    }
    Ok(())
}

/// Whether the complete canonical channel contains any cost-accounting field.
///
/// This deliberately visits decoded EPathMap *set and map keys* as well as map
/// values. A top-level-only check, or a walk over borrowed `Par` children,
/// would miss key-only cost fields and alias a cost-bearing channel with an
/// ordinary historical channel. The shared visitor keeps the traversal and
/// key decoding on an explicit worklist; malformed keys fail closed.
pub fn channel_contains_cost(channel: &Par) -> Result<bool, CodecError> {
    let mut found = false;
    visit_canonical_par_tree(channel, |par| {
        found |= !par.cost_signed_terms.is_empty()
            || !par.cost_stacks.is_empty()
            || par
                .receives
                .iter()
                .flat_map(|receive| receive.binds.iter())
                .any(|bind| bind.cost_signature.is_some());
    })?;
    Ok(found)
}

/// Native Route-A validation reuses the same canonical visitor, but charges
/// before decoding every raw trie key and before visiting its newly allocated
/// Par tree. Original borrowed Par nodes are covered by the caller's charged
/// `prepare_*` backing inspection and add no new Legacy meter operations.
pub fn channel_contains_cost_metered(
    channel: &Par,
    meter: &dyn SourceMeter,
) -> Result<bool, RSpaceError> {
    let mut found = false;
    visit_canonical_par_tree_charged(
        channel,
        |par| {
            found |= !par.cost_signed_terms.is_empty()
                || !par.cost_stacks.is_empty()
                || par
                    .receives
                    .iter()
                    .flat_map(|receive| receive.binds.iter())
                    .any(|bind| bind.cost_signature.is_some());
        },
        |step| match step {
            CanonicalVisitStep::BorrowedPar => Ok(()),
            CanonicalVisitStep::DecodedKeyPar => meter.reserve(1, size_of::<Par>(), 0),
            CanonicalVisitStep::EncodedKey(bytes) => {
                let backing = bytes
                    .checked_mul(size_of::<Par>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                meter.reserve(1, bytes, backing)
            }
        },
    )
    .map_err(|error| match error {
        CanonicalVisitFailure::Codec(error) => invalid_canonical_channel(error),
        CanonicalVisitFailure::Charge(error) => error,
    })?;
    Ok(found)
}

/// Direct and nested cost fields in the three other RSpace event legs. Every
/// Par is checked even after a direct hit so malformed canonical map keys are
/// never hidden behind an earlier positive result.
pub fn datum_contains_cost(datum: &ListParWithRandom) -> Result<bool, CodecError> {
    let mut found = datum.cost_authority.is_some() || datum.cost_stack.is_some();
    for par in &datum.pars {
        found |= channel_contains_cost(par)?;
    }
    Ok(found)
}

fn datum_contains_cost_metered(
    datum: &ListParWithRandom,
    meter: &dyn SourceMeter,
) -> Result<bool, RSpaceError> {
    let mut found = datum.cost_authority.is_some() || datum.cost_stack.is_some();
    for par in &datum.pars {
        found |= channel_contains_cost_metered(par, meter)?;
    }
    Ok(found)
}

pub fn pattern_contains_cost(pattern: &BindPattern) -> Result<bool, CodecError> {
    let mut found = false;
    for par in &pattern.patterns {
        found |= channel_contains_cost(par)?;
    }
    Ok(found)
}

fn pattern_contains_cost_metered(
    pattern: &BindPattern,
    meter: &dyn SourceMeter,
) -> Result<bool, RSpaceError> {
    let mut found = false;
    for par in &pattern.patterns {
        found |= channel_contains_cost_metered(par, meter)?;
    }
    Ok(found)
}

pub fn continuation_contains_cost(continuation: &TaggedContinuation) -> Result<bool, CodecError> {
    let mut found = continuation.cost_authority.is_some();
    if let Some(guard) = &continuation.guard {
        found |= channel_contains_cost(guard)?;
    }
    if let Some(TaggedCont::ParBody(body)) = &continuation.tagged_cont {
        if let Some(par) = &body.body {
            found |= channel_contains_cost(par)?;
        }
    }
    Ok(found)
}

fn continuation_contains_cost_metered(
    continuation: &TaggedContinuation,
    meter: &dyn SourceMeter,
) -> Result<bool, RSpaceError> {
    let mut found = continuation.cost_authority.is_some();
    if let Some(guard) = &continuation.guard {
        found |= channel_contains_cost_metered(guard, meter)?;
    }
    if let Some(TaggedCont::ParBody(body)) = &continuation.tagged_cont {
        if let Some(par) = &body.body {
            found |= channel_contains_cost_metered(par, meter)?;
        }
    }
    Ok(found)
}

pub fn event_hash_bytes_list_par_with_random(datum: &ListParWithRandom) -> Vec<u8> {
    datum.cold_encode()
}

pub fn event_hash_bytes_bind_pattern(pattern: &BindPattern) -> Vec<u8> { pattern.cold_encode() }

pub fn event_hash_bytes_tagged_continuation(continuation: &TaggedContinuation) -> Vec<u8> {
    continuation.cold_encode()
}

impl StableHashSerialize for ListParWithRandom {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_list_par_with_random(self) }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { bincode_encoder::encode_legacy(self) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { bincode_encoder::encode_cost_v1(self) }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> {
        validate_datum_cost_v1(self, None)
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        validate_datum_cost_v1(self, Some(meter))
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_cost_v1_metered_into(self, sink)
    }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        datum_contains_cost(self).map_err(invalid_canonical_channel)
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        datum_contains_cost_metered(self, meter)
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, false, sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, true, sink)
    }
}

impl StableHashSerialize for BindPattern {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_bind_pattern(self) }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { bincode_encoder::encode_legacy(self) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { bincode_encoder::encode_cost_v1(self) }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> {
        validate_pattern_cost_v1(self, None)
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        validate_pattern_cost_v1(self, Some(meter))
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_cost_v1_metered_into(self, sink)
    }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        pattern_contains_cost(self).map_err(invalid_canonical_channel)
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        pattern_contains_cost_metered(self, meter)
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, false, sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, true, sink)
    }
}

impl StableHashSerialize for TaggedContinuation {
    fn stable_hash_bytes(&self) -> Vec<u8> { event_hash_bytes_tagged_continuation(self) }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { bincode_encoder::encode_legacy(self) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { bincode_encoder::encode_cost_v1(self) }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> {
        validate_continuation_cost_v1(self, None)
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        validate_continuation_cost_v1(self, Some(meter))
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_cost_v1_metered_into(self, sink)
    }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        continuation_contains_cost(self).map_err(invalid_canonical_channel)
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        continuation_contains_cost_metered(self, meter)
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, false, sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, true, sink)
    }
}

impl StableHashSerialize for Par {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        #[cfg(feature = "phase7-depth-histograms")]
        crate::rust::rholang::phase7_depth_histogram::record_par("bincode_encoder", self);
        bincode_encoder::encode(self)
    }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { bincode_encoder::encode_legacy(self) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { bincode_encoder::encode_cost_v1(self) }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> { validate_par_cost_v1(self) }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        validate_par_cost_v1_metered(self, meter)
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_cost_v1_metered_into(self, sink)
    }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        channel_contains_cost(self).map_err(invalid_canonical_channel)
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        channel_contains_cost_metered(self, meter)
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, false, sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, true, sink)
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

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { bincode_encoder::encode_legacy(self) }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { bincode_encoder::encode_cost_v1(self) }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> {
        match &self.body {
            Some(par) => validate_par_cost_v1(par),
            None => Ok(()),
        }
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        match &self.body {
            Some(par) => validate_par_cost_v1_metered(par, meter),
            None => Ok(()),
        }
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_cost_v1_metered_into(self, sink)
    }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        self.body
            .as_ref()
            .map(channel_contains_cost)
            .transpose()
            .map(|present| present.unwrap_or(false))
            .map_err(invalid_canonical_channel)
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        self.body
            .as_ref()
            .map(|body| channel_contains_cost_metered(body, meter))
            .transpose()
            .map(|present| present.unwrap_or(false))
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, false, sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        bincode_encoder::encode_metered_into(self, true, sink)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;

    use pathmap::PathMap;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::hashing::native_source;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider::{
        channel_hash_for_epoch, channel_tuple_hash_for_epoch, hash_consume_for_epoch,
        hash_from_hashes, hash_produce_for_epoch, EventCodecEpoch,
    };
    use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};

    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{CostSignature, CostStack, EPathMap, Expr, Receive, ReceiveBind, Send};
    use crate::rust::epathmap_trie_codec::{self, EPathMapRepr};

    fn cost_par() -> Par {
        let mut par = Par::default();
        par.cost_stacks = vec![CostStack {
            cells: Vec::new(),
            parallel_tail: Vec::new(),
        }];
        par
    }

    fn deferred_invalid_utf8_set_snapshot() -> Vec<u8> {
        let mut raw = PathMap::<()>::new();
        // Structurally declared as GString, but the payload is not UTF-8.
        // Deferred construction may cache topology; the charged canonical
        // visitor must reject the payload before using the channel key.
        raw.insert(&[0x04, 0x01, 0xff], ());
        epathmap_trie_codec::encode(&EPathMapRepr::Set(raw))
    }

    fn legacy_tuple_hash(channels: &[Par]) -> Blake2b256Hash {
        let hashes = channels
            .iter()
            .map(|channel| Blake2b256Hash::new(&bincode_encoder::encode_legacy(channel)))
            .collect();
        hash_from_hashes(&hashes)
    }

    fn pathmap_par(map: EPathMap) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }])
    }

    #[test]
    fn channel_cost_fold_sees_nested_receive_bind_and_plain_siblings() {
        let ordinary = Par::default();
        assert_eq!(channel_contains_cost(&ordinary), Ok(false));
        let nested = Par::default().with_receives(vec![Receive {
            binds: vec![ReceiveBind {
                cost_signature: Some(CostSignature::default()),
                ..ReceiveBind::default()
            }],
            ..Receive::default()
        }]);
        assert_eq!(channel_contains_cost(&nested), Ok(true));
    }

    #[test]
    fn channel_cost_fold_decodes_set_and_map_keys_and_map_values() {
        let ordinary = Par::default();
        let set_key = pathmap_par(EPathMap::from_set_iter(
            [ordinary.clone(), cost_par()],
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(channel_contains_cost(&set_key), Ok(true));

        let map_key = pathmap_par(EPathMap::new_map(
            [(cost_par(), ordinary.clone())],
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(channel_contains_cost(&map_key), Ok(true));

        let map_value = pathmap_par(EPathMap::new_map(
            [(ordinary.clone(), cost_par())],
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(channel_contains_cost(&map_value), Ok(true));

        let ordinary_map = pathmap_par(EPathMap::new_map(
            [(ordinary.clone(), ordinary)],
            Vec::new(),
            false,
            None,
        ));
        assert_eq!(channel_contains_cost(&ordinary_map), Ok(false));
    }

    #[test]
    fn channel_cost_fold_rejects_malformed_canonical_key() {
        let snapshot = deferred_invalid_utf8_set_snapshot();
        let mut map = EPathMap::default();
        map.replace_trie_snapshot_deferred_key_validation(&snapshot)
            .expect("deferred key validation is intentionally available to the decoder");
        assert!(channel_contains_cost(&pathmap_par(map)).is_err());
    }

    #[test]
    fn native_validation_charges_each_raw_key_before_decode_and_fails_closed() {
        let ordinary = Par::default();
        let cost = cost_par();
        let expected_lengths = {
            let mut lengths = vec![
                crate::rust::canonical_path::encode_trie_path(&ordinary).len(),
                crate::rust::canonical_path::encode_trie_path(&cost).len(),
            ];
            lengths.sort();
            lengths
        };
        let channel = pathmap_par(EPathMap::from_set_iter(
            [ordinary, cost],
            Vec::new(),
            false,
            None,
        ));
        let charges = RefCell::new(Vec::new());
        let meter = |operations, scanned, backing| {
            charges.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        assert!(channel_contains_cost_metered(&channel, &meter).unwrap());
        let trace = charges.into_inner();
        let mut key_lengths = trace
            .iter()
            .filter(|(_, _, backing)| *backing > 0)
            .map(|(_, scanned, _)| *scanned)
            .collect::<Vec<_>>();
        key_lengths.sort();
        assert_eq!(key_lengths, expected_lengths);
        assert_eq!(
            trace.iter().filter(|(_, _, backing)| *backing > 0).count(),
            2,
            "each raw key must be charged exactly once"
        );
        let route_trace = RefCell::new(Vec::new());
        let route_meter = |operations, scanned, backing| {
            route_trace
                .borrow_mut()
                .push((operations, scanned, backing));
            Ok(())
        };
        native_source::channel_hash_for_epoch(EventCodecEpoch::CostV1, &channel, &route_meter)
            .unwrap();
        let route_trace = route_trace.into_inner();
        // CostV1 admission first classifies the complete canonical channel,
        // then checks that no nested CostStack branch would be lost by its
        // cells-only positional layout. Both are charged key walks. Encoding
        // after those walks must not introduce a third key decode.
        assert_eq!(&route_trace[..trace.len()], trace.as_slice());
        assert_eq!(&route_trace[trace.len()..2 * trace.len()], trace.as_slice());
        let key_charges = route_trace
            .iter()
            .filter(|(operations, scanned, backing)| {
                *operations == 1
                    && *backing > 0
                    && *backing == scanned.saturating_mul(size_of::<Par>())
            })
            .count();
        assert_eq!(
            key_charges,
            2 * expected_lengths.len(),
            "encoding must not add a third raw-key validation walk"
        );
        for reject_at in 0..trace.len() {
            let calls = Cell::new(0);
            let rejecting = |_: usize, _: usize, _: usize| {
                let call = calls.get();
                calls.set(call + 1);
                if call == reject_at {
                    Err(RSpaceError::HostWorkRejected)
                } else {
                    Ok(())
                }
            };
            assert!(matches!(
                channel_contains_cost_metered(&channel, &rejecting),
                Err(RSpaceError::HostWorkRejected)
            ));
            assert_eq!(calls.get(), reject_at + 1);
        }

        let snapshot = deferred_invalid_utf8_set_snapshot();
        let mut map = EPathMap::default();
        map.replace_trie_snapshot_deferred_key_validation(&snapshot)
            .unwrap();
        let malformed = pathmap_par(map);
        let charges = RefCell::new(Vec::new());
        let meter = |operations, scanned, backing| {
            charges.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        assert!(channel_contains_cost_metered(&malformed, &meter).is_err());
        assert_eq!(charges.into_inner(), vec![(1, 3, 3 * size_of::<Par>())]);
    }

    #[test]
    fn legacy_event_leg_gate_detects_nested_cost_without_payload_sniffing() {
        let cost = cost_par();
        let datum = ListParWithRandom {
            pars: vec![cost.clone()],
            ..ListParWithRandom::default()
        };
        assert_eq!(datum_contains_cost(&datum), Ok(true));
        let pattern = BindPattern {
            patterns: vec![cost.clone()],
            ..BindPattern::default()
        };
        assert_eq!(pattern_contains_cost(&pattern), Ok(true));
        let continuation = TaggedContinuation {
            guard: Some(cost),
            ..TaggedContinuation::default()
        };
        assert_eq!(continuation_contains_cost(&continuation), Ok(true));
        assert_eq!(
            datum_contains_cost(&ListParWithRandom::default()),
            Ok(false)
        );
        assert_eq!(pattern_contains_cost(&BindPattern::default()), Ok(false));
        assert_eq!(
            continuation_contains_cost(&TaggedContinuation::default()),
            Ok(false)
        );
    }

    #[test]
    fn channel_cost_fold_is_stack_safe_through_deep_plain_children() {
        std::thread::Builder::new()
            .stack_size(96 * 1024)
            .spawn(|| {
                let mut deep = cost_par();
                for _ in 0..4096 {
                    deep = Par::default().with_sends(vec![Send {
                        chan: Some(deep),
                        ..Send::default()
                    }]);
                }
                assert_eq!(channel_contains_cost(&deep), Ok(true));
                assert_eq!(
                    channel_hash_for_epoch(EventCodecEpoch::CostV1, &deep)
                        .expect("deep cost channel key")
                        .bytes()
                        .len(),
                    32,
                );
            })
            .expect("small-stack channel detector worker")
            .join()
            .expect("small-stack channel detector must not overflow");
    }

    #[test]
    fn route_a_keeps_ordinary_keys_and_separates_cost_keys() {
        let ordinary = Par::default();
        let legacy = channel_hash_for_epoch(EventCodecEpoch::Legacy, &ordinary).unwrap();
        assert_eq!(
            legacy,
            Blake2b256Hash::new(&bincode_encoder::encode_legacy(&ordinary))
        );
        assert_eq!(
            channel_hash_for_epoch(EventCodecEpoch::CostV1, &ordinary).unwrap(),
            legacy,
        );

        let nested_cost = Par::default().with_sends(vec![Send {
            chan: Some(cost_par()),
            ..Send::default()
        }]);
        assert!(channel_hash_for_epoch(EventCodecEpoch::Legacy, &nested_cost).is_err());
        assert_ne!(
            channel_hash_for_epoch(EventCodecEpoch::CostV1, &nested_cost).unwrap(),
            Blake2b256Hash::new(&bincode_encoder::encode_legacy(&nested_cost)),
        );
        let key_only_cost = pathmap_par(EPathMap::new_map(
            [(cost_par(), Par::default())],
            Vec::new(),
            false,
            None,
        ));
        assert!(channel_hash_for_epoch(EventCodecEpoch::Legacy, &key_only_cost).is_err());
        assert_ne!(
            channel_hash_for_epoch(EventCodecEpoch::CostV1, &key_only_cost).unwrap(),
            Blake2b256Hash::new(&bincode_encoder::encode_legacy(&key_only_cost)),
        );

        let ordinary_tuple = vec![ordinary.clone(), Par::default()];
        assert_eq!(
            channel_tuple_hash_for_epoch(EventCodecEpoch::Legacy, &ordinary_tuple).unwrap(),
            legacy_tuple_hash(&ordinary_tuple),
        );
        assert_eq!(
            channel_tuple_hash_for_epoch(EventCodecEpoch::CostV1, &ordinary_tuple).unwrap(),
            legacy_tuple_hash(&ordinary_tuple),
        );
        let cost_tuple = vec![ordinary, key_only_cost];
        assert!(channel_tuple_hash_for_epoch(EventCodecEpoch::Legacy, &cost_tuple).is_err());
        assert_ne!(
            channel_tuple_hash_for_epoch(EventCodecEpoch::CostV1, &cost_tuple).unwrap(),
            legacy_tuple_hash(&cost_tuple),
        );
    }

    #[test]
    fn route_a_stored_records_bind_the_same_epoch_as_their_event_sources() {
        let ordinary = Par::default();
        let datum = ListParWithRandom::default();
        let stored =
            Datum::create_for_epoch(EventCodecEpoch::Legacy, &ordinary, datum.clone(), false)
                .unwrap();
        assert_eq!(
            stored.source,
            rspace_plus_plus::rspace::trace::event::Produce::create_for_epoch(
                EventCodecEpoch::Legacy,
                &ordinary,
                &datum,
                false,
            )
            .unwrap(),
        );
        assert!(
            Datum::create_for_epoch(EventCodecEpoch::Legacy, &cost_par(), datum, false,).is_err()
        );

        let channels = vec![ordinary];
        let patterns = vec![BindPattern::default()];
        let continuation = TaggedContinuation::default();
        let waiting = WaitingContinuation::create_for_epoch(
            EventCodecEpoch::CostV1,
            &channels,
            &patterns,
            &continuation,
            false,
            BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            waiting.source,
            rspace_plus_plus::rspace::trace::event::Consume::create_for_epoch(
                EventCodecEpoch::CostV1,
                &channels,
                &patterns,
                &continuation,
                false,
            )
            .unwrap(),
        );
        let cost_channels = vec![cost_par()];
        assert!(WaitingContinuation::create_for_epoch(
            EventCodecEpoch::Legacy,
            &cost_channels,
            &patterns,
            &continuation,
            false,
            BTreeSet::new(),
        )
        .is_err());
    }

    #[test]
    fn route_a_preserves_old_produce_preimage_and_tags_cost_v1_events() {
        let channel = Par::default();
        let channel_hash = channel_hash_for_epoch(EventCodecEpoch::Legacy, &channel).unwrap();
        let datum = ListParWithRandom::default();
        let expected_legacy = Blake2b256Hash::new(
            &bincode::serialize(&vec![
                channel_hash.bytes(),
                bincode_encoder::encode_legacy(&datum),
                bincode::serialize(&false).unwrap(),
            ])
            .unwrap(),
        );
        assert_eq!(
            hash_produce_for_epoch(EventCodecEpoch::Legacy, &channel_hash, &datum, false).unwrap(),
            expected_legacy,
        );
        assert_ne!(
            hash_produce_for_epoch(EventCodecEpoch::CostV1, &channel_hash, &datum, false).unwrap(),
            expected_legacy,
        );
        let signed_datum = ListParWithRandom {
            pars: vec![cost_par()],
            ..ListParWithRandom::default()
        };
        assert!(hash_produce_for_epoch(
            EventCodecEpoch::Legacy,
            &channel_hash,
            &signed_datum,
            false
        )
        .is_err());
        let snapshot = deferred_invalid_utf8_set_snapshot();
        let mut malformed_map = EPathMap::default();
        malformed_map
            .replace_trie_snapshot_deferred_key_validation(&snapshot)
            .unwrap();
        let malformed_datum = ListParWithRandom {
            pars: vec![pathmap_par(malformed_map)],
            ..ListParWithRandom::default()
        };
        assert!(hash_produce_for_epoch(
            EventCodecEpoch::CostV1,
            &channel_hash,
            &malformed_datum,
            false
        )
        .is_err());
    }

    #[test]
    fn route_a_consume_rejects_legacy_signed_pattern_and_domains_cost_v1() {
        let channel_hash =
            channel_hash_for_epoch(EventCodecEpoch::Legacy, &Par::default()).unwrap();
        let ordinary = BindPattern::default();
        let continuation = TaggedContinuation::default();
        let old = hash_consume_for_epoch(
            EventCodecEpoch::Legacy,
            &[channel_hash.clone()],
            &[ordinary.clone()],
            &continuation,
            false,
        )
        .unwrap();
        let expected_old = Blake2b256Hash::new(
            &bincode::serialize(&vec![
                channel_hash.bytes(),
                bincode_encoder::encode_legacy(&ordinary),
                bincode_encoder::encode_legacy(&continuation),
                bincode::serialize(&false).unwrap(),
            ])
            .unwrap(),
        );
        assert_eq!(old, expected_old);
        let other_hash = Blake2b256Hash::new(b"second ordinary channel");
        assert_eq!(
            hash_consume_for_epoch(
                EventCodecEpoch::Legacy,
                &[channel_hash.clone(), other_hash.clone()],
                &[ordinary.clone()],
                &continuation,
                false,
            )
            .unwrap(),
            hash_consume_for_epoch(
                EventCodecEpoch::Legacy,
                &[other_hash, channel_hash.clone()],
                &[ordinary.clone()],
                &continuation,
                false,
            )
            .unwrap(),
        );
        assert_ne!(
            hash_consume_for_epoch(
                EventCodecEpoch::CostV1,
                &[channel_hash.clone()],
                &[ordinary],
                &continuation,
                false,
            )
            .unwrap(),
            old,
        );
        let signed = BindPattern {
            patterns: vec![cost_par()],
            ..BindPattern::default()
        };
        assert!(hash_consume_for_epoch(
            EventCodecEpoch::Legacy,
            &[channel_hash],
            &[signed],
            &continuation,
            false,
        )
        .is_err());
    }
}
