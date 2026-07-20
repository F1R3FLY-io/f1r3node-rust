use bincode;
use serde::Serialize;

use super::blake2b256_hash::Blake2b256Hash;

/// EPathMap fix P4.3 (plan v1 §1-P4 sub-commit 3, amendment PM-1): the
/// serialization CONTRACT of event hashing.
///
/// `hash_produce`/`hash_consume` hash the bincode 1.3.3 default-config
/// (legacy fixint-LE) encoding of the datum / each pattern / the
/// continuation. The default body IS the historical path —
/// `bincode::serialize` — so every implementor is byte-identical by
/// definition unless it overrides. An override may ONLY be a byte-identical
/// faster construction: models overrides the three rhoapi event types
/// (`ListParWithRandom`, `BindPattern`, `TaggedContinuation`) with the
/// intern-aware spliced emitter, which reuses `InternedEPathMap.serde_bytes`
/// at filled-cell EPathMap nodes and black-box `bincode::serialize` for
/// interned-map-free subtrees (bincode's encoding is compositional). Byte
/// identity is gated by the P0 event-hash goldens (re-asserted with filled
/// cells) and the spliced-vs-direct proptest.
///
/// The trait lives HERE (rspace++ is generic and models depends on rspace++,
/// so the orphan rule places the rhoapi impls in models); rspace++ provides
/// default-body impls for the primitive types its own tests instantiate.
pub trait StableHashSerialize: Serialize {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        bincode::serialize(self)
            .expect("StableHashSerialize: bincode serialization must not fail")
    }
}

// Reference forwarding: `&T` hashes as `T` (serde serializes through
// references transparently, so the bytes are identical; forwarding — not
// the default body — ensures an override like models' spliced emitter is
// reached through references too).
impl<T: StableHashSerialize + ?Sized> StableHashSerialize for &T {
    fn stable_hash_bytes(&self) -> Vec<u8> { (**self).stable_hash_bytes() }
}

// Default-body impls for the rspace++ test-double instantiations (no
// blanket impl: coherence must leave room for models' overriding impls).
impl StableHashSerialize for String {}
impl StableHashSerialize for i32 {}
impl StableHashSerialize for u64 {}
impl StableHashSerialize for bool {}
impl StableHashSerialize for Vec<u8> {}

// See rspace/src/main/scala/coop/rchain/rspace/hashing/StableHashProvider.scala
pub fn hash<C: Serialize>(channel: &C) -> Blake2b256Hash {
    let bytes = bincode::serialize(channel).unwrap();
    Blake2b256Hash::new(&bytes)
}

pub fn hash_vec<C: Serialize>(channels: &Vec<C>) -> Vec<Blake2b256Hash> {
    let mut hashes: Vec<Blake2b256Hash> = channels
        .iter()
        .map(|channel| {
            let bytes = bincode::serialize(&channel).unwrap();
            Blake2b256Hash::new(&bytes)
        })
        .collect();
    hashes.sort();
    hashes
}

pub fn hash_from_vec<C: Serialize>(channels: &Vec<C>) -> Blake2b256Hash {
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
// P4.3: the pattern/continuation bytes come from `StableHashSerialize` —
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

// P4.3: the hashed unit is bincode(vec![channel_hash_bytes, bincode(datum),
// bincode(persist)]) — `datum.stable_hash_bytes()` supplies the middle leg
// byte-identically (amendment PM-1: the intern-aware models override splices
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
