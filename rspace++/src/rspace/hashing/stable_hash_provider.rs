use std::io::Write;

use bincode;
use serde::Serialize;

use super::blake2b256_hash::Blake2b256Hash;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source::SourceMeter;

/// Authenticated consensus context supplied by the caller. This is never
/// selected by inspecting a payload or by trying one decoder after another.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventCodecEpoch {
    Legacy,
    CostV1,
    CostV2,
}

pub const COST_EVENT_DOMAIN_V1: &[u8] = b"f1r3node:rspace-event-cost:v1";
pub const COST_CHANNEL_KEY_DOMAIN_V1: &[u8] = b"f1r3node:rspace-channel-key-cost:v1";

/// One bincode output chunk, charged by the native source meter before it is
/// observed by a hash or stored in a vector. The generated models trampoline
/// and bounded serde test-double fallback both write through this interface;
/// neither may build an unmetered whole-leg buffer first.
pub trait BincodeByteSink {
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), RSpaceError>;
}

struct SerdeSinkAdapter<'a> {
    sink: &'a mut dyn BincodeByteSink,
    error: Option<RSpaceError>,
}

impl Write for SerdeSinkAdapter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.error.is_some() {
            return Err(std::io::ErrorKind::Other.into());
        }
        match self.sink.write_bytes(bytes) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => {
                self.error = Some(error);
                Err(std::io::ErrorKind::Other.into())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

/// The serialization contract of EPathMap-bearing event hashing.
///
/// `hash_produce`/`hash_consume` hash the bincode 1.3.3 default-config
/// (legacy fixint-LE) encoding of the datum / each pattern / the
/// continuation. The default body IS the historical path —
/// `bincode::serialize` — so every implementor is byte-identical by
/// definition unless it overrides. An override may ONLY be a byte-identical
/// faster construction: models overrides the three rhoapi event types
/// (`ListParWithRandom`, `BindPattern`, `TaggedContinuation`) with the
/// generated stack-safe bincode encoder. EPathMap nodes copy their canonical
/// EPM1 snapshot directly; no intern-store dispatch scan or recursive serde
/// fallback remains. Byte identity is gated by the canonical event-hash
/// goldens and generated-vs-derived differential tests.
///
/// The trait lives HERE (rspace++ is generic and models depends on rspace++,
/// so the orphan rule places the rhoapi impls in models); rspace++ provides
/// default-body impls for the primitive types its own tests instantiate.
pub trait StableHashSerialize: Serialize {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("StableHashSerialize: bincode serialization must not fail")
    }

    /// Exact pre-cost positional bytes. Schema extensions must override this;
    /// the default is correct only for types whose layout never changed.
    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { self.stable_hash_bytes() }

    /// Protocol-v6 cost layout. The default is correct for types unchanged by
    /// protocol 7; schema-extended models roots override it to omit only the
    /// v7-only fields while retaining all original cost fields.
    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { self.stable_hash_bytes() }

    /// Cost fields anywhere in this canonical event leg. An invalid encoded
    /// key is an error, not evidence that the value is ordinary.
    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> { Ok(false) }

    /// CostV1 cannot encode v7-only structure. Schema-extended roots override
    /// this to refuse a nonempty parallel tail before the v6 layout is emitted.
    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> { Ok(()) }

    /// Native validation hook. Bounded historical/test values need no extra
    /// traversal; models overrides for canonical Par and its event wrappers.
    fn stable_hash_contains_cost_metered(
        &self,
        _meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        self.stable_hash_contains_cost()
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        _meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        self.stable_hash_validate_cost_v1()
    }

    /// Emit bincode's exact write chunks into a charge-before-write sink.
    /// Bounded test doubles keep serde's native chunk trace; recursive rhoapi
    /// roots override this with the generated explicit-stack machine.
    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        let mut adapter = SerdeSinkAdapter { sink, error: None };
        let result = bincode::serialize_into(&mut adapter, self);
        if let Some(error) = adapter.error {
            return Err(error);
        }
        result.map_err(|error| RSpaceError::InterpreterError(error.to_string()))
    }

    /// Pre-cost positional layout through the same sink. Schema-extended
    /// models roots override this; unchanged bounded types use the default.
    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        self.stable_hash_write_metered(sink)
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        self.stable_hash_write_metered(sink)
    }
}

// Reference forwarding: `&T` hashes as `T` (serde serializes through
// references transparently, so the bytes are identical; forwarding — not
// the default body — ensures an override like models' spliced emitter is
// reached through references too).
impl<T: StableHashSerialize + ?Sized> StableHashSerialize for &T {
    fn stable_hash_bytes(&self) -> Vec<u8> { (**self).stable_hash_bytes() }

    fn stable_hash_legacy_bytes(&self) -> Vec<u8> { (**self).stable_hash_legacy_bytes() }

    fn stable_hash_cost_v1_bytes(&self) -> Vec<u8> { (**self).stable_hash_cost_v1_bytes() }

    fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> {
        (**self).stable_hash_contains_cost()
    }

    fn stable_hash_validate_cost_v1(&self) -> Result<(), RSpaceError> {
        (**self).stable_hash_validate_cost_v1()
    }

    fn stable_hash_contains_cost_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<bool, RSpaceError> {
        (**self).stable_hash_contains_cost_metered(meter)
    }

    fn stable_hash_validate_cost_v1_metered(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<(), RSpaceError> {
        (**self).stable_hash_validate_cost_v1_metered(meter)
    }

    fn stable_hash_write_metered(&self, sink: &mut dyn BincodeByteSink) -> Result<(), RSpaceError> {
        (**self).stable_hash_write_metered(sink)
    }

    fn stable_hash_write_legacy_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        (**self).stable_hash_write_legacy_metered(sink)
    }

    fn stable_hash_write_cost_v1_metered(
        &self,
        sink: &mut dyn BincodeByteSink,
    ) -> Result<(), RSpaceError> {
        (**self).stable_hash_write_cost_v1_metered(sink)
    }
}

/// Selected bytes for one event leg. Legacy is fail-closed even if the caller
/// hands in a populated cost field; the omission codec may never be lossy.
pub fn event_leg_bytes_for_epoch<T: StableHashSerialize>(
    epoch: EventCodecEpoch,
    value: &T,
) -> Result<Vec<u8>, RSpaceError> {
    // Validate the complete canonical tree in both epochs. A malformed raw
    // EPathMap key must not become an apparently valid CostV1 event leg.
    let cost_present = value.stable_hash_contains_cost()?;
    match epoch {
        EventCodecEpoch::Legacy => {
            if cost_present {
                return Err(RSpaceError::InterpreterError(
                    "cost-bearing event leg is invalid in the Legacy codec epoch".into(),
                ));
            }
            Ok(value.stable_hash_legacy_bytes())
        }
        EventCodecEpoch::CostV1 => {
            value.stable_hash_validate_cost_v1()?;
            Ok(value.stable_hash_cost_v1_bytes())
        }
        EventCodecEpoch::CostV2 => Ok(value.stable_hash_bytes()),
    }
}

/// Route A channel identity. Ordinary channels retain the exact historical
/// key in both epochs; any cost-bearing channel gets a disjoint domain-tagged
/// preimage only in CostV1. The caller provides the authenticated epoch.
pub fn channel_hash_for_epoch<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channel: &C,
) -> Result<Blake2b256Hash, RSpaceError> {
    let cost_present = channel.stable_hash_contains_cost()?;
    if cost_present && epoch == EventCodecEpoch::Legacy {
        return Err(RSpaceError::InterpreterError(
            "cost-bearing channel is invalid in the Legacy codec epoch".into(),
        ));
    }
    if !cost_present {
        return Ok(Blake2b256Hash::new(&channel.stable_hash_legacy_bytes()));
    }
    let tagged =
        bincode::serialize(&vec![COST_CHANNEL_KEY_DOMAIN_V1.to_vec(), event_leg_bytes_for_epoch(epoch, channel)?])
            .expect("channel key domain preimage serialization");
    Ok(Blake2b256Hash::new(&tagged))
}

pub fn channel_hashes_for_epoch<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channels: &[C],
) -> Result<Vec<Blake2b256Hash>, RSpaceError> {
    let mut hashes = channels
        .iter()
        .map(|channel| channel_hash_for_epoch(epoch, channel))
        .collect::<Result<Vec<_>, _>>()?;
    hashes.sort();
    Ok(hashes)
}

/// The channel-tuple key used for continuations. It is built from precisely
/// the same routed channel keys as produce and consume event identities.
/// Keeping this operation here prevents history readers and checkpoint writers
/// from independently choosing a different channel-key namespace.
pub fn channel_tuple_hash_for_epoch<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channels: &[C],
) -> Result<Blake2b256Hash, RSpaceError> {
    let hashes = channel_hashes_for_epoch(epoch, channels)?;
    Ok(hash_from_hashes(&hashes))
}

fn event_preimage_for_epoch(epoch: EventCodecEpoch, mut legs: Vec<Vec<u8>>) -> Vec<u8> {
    if epoch != EventCodecEpoch::Legacy {
        legs.insert(0, COST_EVENT_DOMAIN_V1.to_vec());
    }
    bincode::serialize(&legs).expect("event-domain preimage serialization")
}

/// Epoch-aware counterpart of `hash_produce`; not used by the unversioned
/// production path until live, native replay, and history all pass one context.
pub fn hash_produce_for_epoch<A: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channel_hash: &Blake2b256Hash,
    datum: &A,
    persist: bool,
) -> Result<Blake2b256Hash, RSpaceError> {
    let legs = vec![
        channel_hash.bytes(),
        event_leg_bytes_for_epoch(epoch, datum)?,
        bincode::serialize(&persist).expect("bool serialization"),
    ];
    Ok(Blake2b256Hash::new(&event_preimage_for_epoch(epoch, legs)))
}

/// Epoch-aware counterpart of `hash_consume`, preserving the historical sort
/// over encoded patterns before the optional CostV1 outer domain member.
pub fn hash_consume_for_epoch<P: StableHashSerialize, K: StableHashSerialize>(
    epoch: EventCodecEpoch,
    channels: &[Blake2b256Hash],
    patterns: &[P],
    continuation: &K,
    persist: bool,
) -> Result<Blake2b256Hash, RSpaceError> {
    let mut encoded_patterns = patterns
        .iter()
        .map(|pattern| event_leg_bytes_for_epoch(epoch, pattern))
        .collect::<Result<Vec<_>, _>>()?;
    encoded_patterns.sort();
    let mut ordered_channels = channels.to_vec();
    ordered_channels.sort();
    let mut legs = ordered_channels
        .iter()
        .map(Blake2b256Hash::bytes)
        .collect::<Vec<_>>();
    legs.extend(encoded_patterns);
    legs.push(event_leg_bytes_for_epoch(epoch, continuation)?);
    legs.push(bincode::serialize(&persist).expect("bool serialization"));
    Ok(Blake2b256Hash::new(&event_preimage_for_epoch(epoch, legs)))
}

// Default-body impls for the rspace++ test-double instantiations (no
// blanket impl: coherence must leave room for models' overriding impls).
impl StableHashSerialize for String {}
impl StableHashSerialize for str {}
impl StableHashSerialize for () {}
impl StableHashSerialize for u8 {}
impl StableHashSerialize for u32 {}
impl StableHashSerialize for i32 {}
impl StableHashSerialize for u64 {}
impl StableHashSerialize for bool {}
impl StableHashSerialize for Vec<u8> {}
// Two-level byte vectors are used only by the native-source hash corpus.
impl StableHashSerialize for Vec<Vec<u8>> {}
// The state-root key type, hashed as a channel by the exporter/importer paths.
impl StableHashSerialize for Blake2b256Hash {}

// See rspace/src/main/scala/coop/rchain/rspace/hashing/StableHashProvider.scala
//
// The CHANNEL leg of every event hash. Routed through [`StableHashSerialize`]
// for the same reason the datum and pattern legs already are: the default body
// IS `bincode::serialize`, so every implementor is byte-identical by
// definition unless it overrides, and an override may ONLY be a byte-identical
// faster construction. `models` overrides `Par` with the single-walk
// trampolined encoder (`models::rust::rholang::bincode_encoder`), which is
// gated byte-identical against the derived `Serialize` over an exhaustive
// structural corpus plus proptest, with an executed mutation proof.
pub fn hash<C: StableHashSerialize>(channel: &C) -> Blake2b256Hash {
    let bytes = channel.stable_hash_bytes();
    Blake2b256Hash::new(&bytes)
}

pub fn hash_vec<C: StableHashSerialize>(channels: &Vec<C>) -> Vec<Blake2b256Hash> {
    let mut hashes: Vec<Blake2b256Hash> = channels
        .iter()
        .map(|channel| Blake2b256Hash::new(&channel.stable_hash_bytes()))
        .collect();
    hashes.sort();
    hashes
}

pub fn hash_from_vec<C: StableHashSerialize>(channels: &Vec<C>) -> Blake2b256Hash {
    let hashes = hash_vec(channels);
    hash_from_hashes(&hashes)
}

pub fn hash_from_hashes(channels_hashes: &Vec<Blake2b256Hash>) -> Blake2b256Hash {
    let mut ord_refs: Vec<&Blake2b256Hash> = channels_hashes.iter().collect();
    ord_refs.sort();
    let mut concatenated: Vec<u8> = Vec::with_capacity(ord_refs.len() * 32);
    for h in ord_refs.iter() {
        concatenated.extend(h.0.iter().copied());
    }
    Blake2b256Hash::new(&concatenated)
}

// See rspace/src/main/scala/coop/rchain/rspace/hashing/StableHashProvider.scala
//
// Pattern/continuation bytes come from `StableHashSerialize` —
// byte-identical to the previous direct `bincode::serialize` (default body;
// models' spliced override is gated byte-identical). The sort over encoded
// patterns is order-stable because the bytes are unchanged.
pub fn hash_consume<P: StableHashSerialize, K: StableHashSerialize>(
    mut encoded_channels: Vec<Vec<u8>>,
    patterns: &[P],
    continuation: &K,
    persist: bool,
) -> Blake2b256Hash {
    let mut encoded_patterns = patterns
        .iter()
        .map(|pattern| pattern.stable_hash_bytes())
        .collect::<Vec<_>>();
    encoded_patterns.sort();

    let encoded_continuation = continuation.stable_hash_bytes();
    let encoded_persist = bincode::serialize(&persist).unwrap();

    encoded_channels.extend(encoded_patterns);
    encoded_channels.push(encoded_continuation);
    encoded_channels.push(encoded_persist);

    let encoded = bincode::serialize(&encoded_channels).unwrap();
    Blake2b256Hash::new(&encoded)
}

// The hashed unit is bincode(vec![channel_hash_bytes, bincode(datum),
// bincode(persist)]) — `datum.stable_hash_bytes()` supplies the middle leg
// byte-identically (the models override splices
// cached per-EPathMap serde bytes into exactly this layout).
pub fn hash_produce<A: StableHashSerialize>(
    encoded_channel: Vec<u8>,
    datum: &A,
    persist: bool,
) -> Blake2b256Hash {
    let encoded_datum = datum.stable_hash_bytes();
    let encoded_persist = bincode::serialize(&persist).unwrap();

    let encoded_vec = vec![encoded_channel, encoded_datum, encoded_persist];

    let encoded = bincode::serialize(&encoded_vec).unwrap();
    Blake2b256Hash::new(&encoded)
}
