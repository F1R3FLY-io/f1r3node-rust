//! EPathMap fix P4.1 — serializer BYTE GOLDENS for the storage-shape change.
//!
//! `Datum`/`WaitingContinuation` become Arc-shaped (non-Serialize) in P4.1;
//! the history/cold-store boundary then serializes through borrowed twins in
//! `serializers.rs`. These tests pin the twin encodings against literal bytes
//! captured from the PRE-P4.1 derived `Serialize` impls (4e422b6b), so any
//! layout drift in the twins is a hard failure — the cold-store leaf bytes
//! and therefore the checkpoint roots are consensus surfaces.
//!
//! Capture procedure: `EPM_P4_BLESS=1 cargo test -p rspace_plus_plus --test
//! serializer_byte_goldens -- --nocapture` at 4e422b6b printed the constants
//! below; the assert mode then pins them forever.

use std::collections::BTreeSet;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_datums, decode_continuations, encode_datum, encode_datums, encode_continuations,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

/// Deterministic fixture datum (String payload — the rspace++ test-double
/// instantiation; the models-typed path is pinned by the P0 event-hash
/// goldens and the history checkpoint-root tests).
fn fixture_datum() -> Datum<String> {
    Datum {
        a: "data0".to_string().into(),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(&[1, 2, 3]),
            Blake2b256Hash::new(&[4, 5, 6]),
            false,
        ),
    }
}

fn fixture_datum_persist() -> Datum<String> {
    Datum {
        a: "data-persist".to_string().into(),
        persist: true,
        source: Produce::new(
            Blake2b256Hash::new(&[7, 8, 9]),
            Blake2b256Hash::new(&[10, 11, 12]),
            true,
        ),
    }
}

fn fixture_continuation() -> WaitingContinuation<String, String> {
    WaitingContinuation {
        patterns: vec!["pat0".to_string(), "pat1".to_string()].into(),
        continuation: "continuation".to_string().into(),
        persist: true,
        peeks: BTreeSet::from([0, 2]),
        source: Consume {
            channel_hashes: vec![Blake2b256Hash::new(&[13, 14, 15])],
            hash: Blake2b256Hash::new(&[16, 17, 18]),
            persistent: true,
        },
    }
}

/// Blake2b256 of the encoding — a compact stand-in for embedding the full
/// byte vectors (lengths are asserted exactly too).
fn digest_hex(bytes: &[u8]) -> String { hex::encode(Blake2b256Hash::new(bytes).bytes()) }

mod pinned {
    /// `encode_datum(fixture_datum())` — length + Blake2b256, captured at
    /// 4e422b6b from the derived `Serialize`.
    pub const DATUM_LEN: usize = 105;
    pub const DATUM_DIGEST_HEX: &str =
        "da6fd6ff637b8b5c4e824f7640cd4ef2f207ab280cc72b943abcc00c3236fef6";

    /// `encode_datums([fixture_datum(), fixture_datum_persist()])`.
    pub const DATUMS_LEN: usize = 241;
    pub const DATUMS_DIGEST_HEX: &str =
        "cbc7cda85ea8211d1773ac94dab49d98975a52169f056fb9304da0e502b3ad51";

    /// `encode_continuations([fixture_continuation()])`.
    pub const CONTS_LEN: usize = 174;
    pub const CONTS_DIGEST_HEX: &str =
        "eb9661583df8275f3e919df4237c55837d41650dad3449afc1378f0b2e6ea18d";
}

fn bless() -> bool { std::env::var_os("EPM_P4_BLESS").is_some() }

fn check(label: &str, pinned_len: usize, pinned_digest: &str, bytes: &[u8]) {
    if bless() {
        println!(
            "PIN {label}_LEN = {}; {label}_DIGEST_HEX = \"{}\";",
            bytes.len(),
            digest_hex(bytes)
        );
    } else {
        assert_eq!(bytes.len(), pinned_len, "{label}: encoded length drifted");
        assert_eq!(digest_hex(bytes), pinned_digest, "{label}: encoded bytes drifted");
    }
}

#[test]
fn datum_encoding_pinned() {
    check(
        "DATUM",
        pinned::DATUM_LEN,
        pinned::DATUM_DIGEST_HEX,
        &encode_datum(&fixture_datum()),
    );
}

#[test]
fn datums_encoding_pinned() {
    check(
        "DATUMS",
        pinned::DATUMS_LEN,
        pinned::DATUMS_DIGEST_HEX,
        &encode_datums(&vec![fixture_datum(), fixture_datum_persist()]),
    );
}

#[test]
fn continuations_encoding_pinned() {
    check(
        "CONTS",
        pinned::CONTS_LEN,
        pinned::CONTS_DIGEST_HEX,
        &encode_continuations(&vec![fixture_continuation()]),
    );
}

/// Round-trip: decode(encode(x)) == x (the cold-store readback path).
#[test]
fn datum_roundtrip() {
    let datums = vec![fixture_datum(), fixture_datum_persist()];
    let decoded: Vec<Datum<String>> = decode_datums(&encode_datums(&datums));
    // encode_datums sorts by encoded bytes; compare as sets.
    assert_eq!(decoded.len(), datums.len());
    for d in &datums {
        assert!(decoded.contains(d), "decoded datums must contain {:?}", d);
    }
}

#[test]
fn continuation_roundtrip() {
    let conts = vec![fixture_continuation()];
    let decoded: Vec<WaitingContinuation<String, String>> =
        decode_continuations(&encode_continuations(&conts));
    assert_eq!(decoded, conts);
}
