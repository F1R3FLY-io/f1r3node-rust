//! DR-131 P2a: composition tests of the per-record decoding.
//!
//! The payloads are bounded stand-in types (`String`, `Vec<u8>`), which
//! implement `ColdStoreDecode` through bincode. Each record function must read
//! the record as the derived decoder reads it, accept trailing bytes as
//! bincode does, and fail on exactly the inputs on which the derived decoder
//! fails. `Produce` compares only its identity fields, so every success also
//! compares the re-encoded bytes, which covers every field.

use std::collections::BTreeSet;
use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

const TRAILING: [u8; 3] = [0xFF, 0x00, 0x7F];

fn produce() -> Produce {
    Produce {
        channel_hash: Blake2b256Hash::new(b"channel"),
        hash: Blake2b256Hash::new(b"produce"),
        persistent: true,
        is_deterministic: true,
        output_value: vec![vec![1, 2], Vec::new()],
        failed: false,
    }
}

fn consume() -> Consume {
    Consume {
        channel_hashes: vec![Blake2b256Hash::new(b"left"), Blake2b256Hash::new(b"right")],
        hash: Blake2b256Hash::new(b"consume"),
        persistent: false,
    }
}

fn datum() -> Datum<String> {
    Datum {
        a: "payload λ".to_string(),
        persist: true,
        source: produce(),
    }
}

fn continuation() -> WaitingContinuation<String, Vec<u8>> {
    WaitingContinuation {
        patterns: vec!["first".to_string(), String::new()],
        continuation: vec![7, 8, 9],
        persist: false,
        peeks: BTreeSet::from([0, 2]),
        source: consume(),
    }
}

fn join() -> Vec<String> { vec!["x".to_string(), "y".to_string()] }

/// Checks one record function against the derived decoder on the encoding of
/// `value`, on that encoding with trailing bytes, on every truncation and on
/// every single-byte flip.
fn agree<T, E>(value: &T, decode: impl Fn(&[u8]) -> Result<T, E>)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
    E: Debug,
{
    let bytes = bincode::serialize(value).expect("serialize the record");
    let decoded = decode(&bytes).expect("decode the record");
    assert_eq!(&decoded, value);
    assert_eq!(bincode::serialize(&decoded).expect("re-encode the record"), bytes);
    let mut padded = bytes.clone();
    padded.extend_from_slice(&TRAILING);
    let decoded = decode(&padded).expect("decode the record with trailing bytes");
    assert_eq!(bincode::serialize(&decoded).expect("re-encode the record"), bytes);
    for end in 0..bytes.len() {
        assert_eq!(
            decode(&bytes[..end]).is_ok(),
            bincode::deserialize::<T>(&bytes[..end]).is_ok(),
            "a prefix of {end} bytes"
        );
    }
    for position in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[position] ^= 0xFF;
        match (decode(&flipped), bincode::deserialize::<T>(&flipped)) {
            (Ok(record), Ok(oracle)) => assert_eq!(
                bincode::serialize(&record).expect("re-encode the record"),
                bincode::serialize(&oracle).expect("re-encode the oracle"),
                "a flip at byte {position}"
            ),
            (Err(_), Err(_)) => {}
            (record, oracle) => panic!(
                "a flip at byte {position}: the record function gives {:?}, the derived decoder \
                 {:?}",
                record.map(|_| ()),
                oracle.map(|_| ())
            ),
        }
    }
}

#[test]
fn a_datum_row_decodes_as_the_derived_decoder_reads_it() {
    agree(&datum(), decode_datum::<String>);
}

#[test]
fn a_continuation_row_decodes_as_the_derived_decoder_reads_it() {
    agree(&continuation(), decode_continuation::<String, Vec<u8>>);
}

#[test]
fn a_join_row_decodes_as_the_derived_decoder_reads_it() { agree(&join(), decode_join::<String>); }

#[test]
fn empty_collections_decode_as_the_derived_decoder_reads_them() {
    agree(
        &WaitingContinuation::<String, Vec<u8>> {
            patterns: Vec::new(),
            continuation: Vec::new(),
            persist: true,
            peeks: BTreeSet::new(),
            source: Consume::default(),
        },
        decode_continuation::<String, Vec<u8>>,
    );
    agree(&Vec::<String>::new(), decode_join::<String>);
}
