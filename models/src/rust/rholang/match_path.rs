//! Stack-safe structural paths for RSpace matching.
//!
//! This format is a derived in-memory search key, not a protobuf, bincode, or
//! consensus identity encoding. It follows the generated matching cursor and
//! therefore covers the complete rhoapi descriptor without recursive calls.
//! `EPathMap` stays native: its token is a domain-separated BLAKE2b-256 digest
//! streamed from a logical zipper walk. It never materializes entries or
//! forces the EPM1 snapshot cache.

use crypto::rust::hash::blake2b256::Blake2b256;
use pathmap::utils::ByteMaskIter;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyValues};
use pathmap::PathMap;

use super::match_schema::{RhoPathCursor, RhoPathEvent, RhoScalarRef};
use super::match_schema_tables::ParCursor;
use crate::rhoapi::Par;
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::rhoapi_ext::EPathMap;

pub const RHO_MATCH_PATH_VERSION: [u8; 4] = *b"RMP\x01";

const NODE_START: u8 = 0x01;
const NODE_END: u8 = 0x02;
const FIELD_START: u8 = 0x03;
const FIELD_END: u8 = 0x04;
const BOOL: u8 = 0x10;
const I32: u8 = 0x11;
const U32: u8 = 0x12;
const I64: u8 = 0x13;
const U64: u8 = 0x14;
const BYTES: u8 = 0x15;
const STRING: u8 = 0x16;
const STRING_SEQUENCE: u8 = 0x17;
const BYTES_SEQUENCE: u8 = 0x18;
const ABSENT: u8 = 0x20;
const SEQUENCE_START: u8 = 0x21;
const SEQUENCE_END: u8 = 0x22;
const MAP_START: u8 = 0x23;
const MAP_KEY: u8 = 0x24;
const MAP_END: u8 = 0x25;
const ONEOF_START: u8 = 0x26;
const ONEOF_END: u8 = 0x27;
const PATHMAP: u8 = 0x28;

/// Encode one `Par` as a complete-schema structural RSpace search path.
pub fn encode_par_match_path(value: &Par) -> Vec<u8> { encode_match_path(ParCursor::from(value)) }

/// Encode any generated rhoapi cursor as a complete-schema structural path.
pub fn encode_match_path(root: ParCursor<'_>) -> Vec<u8> {
    let mut output = Vec::with_capacity(256);
    output.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    for event in RhoPathCursor::new(root) {
        match event {
            RhoPathEvent::EnterNode(program) => {
                output.push(NODE_START);
                output.extend_from_slice(&program.type_id.to_be_bytes());
            }
            RhoPathEvent::LeaveNode => output.push(NODE_END),
            RhoPathEvent::EnterField(field) => {
                output.push(FIELD_START);
                output.extend_from_slice(&field.tag.to_be_bytes());
            }
            RhoPathEvent::LeaveField => output.push(FIELD_END),
            RhoPathEvent::Scalar(value) => append_scalar(&mut output, value),
            RhoPathEvent::Absent => output.push(ABSENT),
            RhoPathEvent::EnterSequence(len) => {
                output.push(SEQUENCE_START);
                append_len(&mut output, len);
            }
            RhoPathEvent::LeaveSequence => output.push(SEQUENCE_END),
            RhoPathEvent::EnterMap(len) => {
                output.push(MAP_START);
                append_len(&mut output, len);
            }
            RhoPathEvent::MapKey(key) => {
                output.push(MAP_KEY);
                append_bytes(&mut output, key.as_bytes());
            }
            RhoPathEvent::LeaveMap => output.push(MAP_END),
            RhoPathEvent::EnterOneof(oneof) => {
                output.push(ONEOF_START);
                output.extend_from_slice(&oneof.protobuf_tag.to_be_bytes());
            }
            RhoPathEvent::LeaveOneof => output.push(ONEOF_END),
            RhoPathEvent::PathMap(map) => append_pathmap(&mut output, map),
        }
    }
    output
}

fn append_scalar(output: &mut Vec<u8>, value: RhoScalarRef<'_>) {
    match value {
        RhoScalarRef::Bool(value) => {
            output.extend_from_slice(&[BOOL, u8::from(value)]);
        }
        RhoScalarRef::I32(value) => {
            output.push(I32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::U32(value) => {
            output.push(U32);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::I64(value) => {
            output.push(I64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::U64(value) => {
            output.push(U64);
            output.extend_from_slice(&value.to_be_bytes());
        }
        RhoScalarRef::Bytes(value) => {
            output.push(BYTES);
            append_bytes(output, value);
        }
        RhoScalarRef::String(value) => {
            output.push(STRING);
            append_bytes(output, value.as_bytes());
        }
        RhoScalarRef::StringSequence(values) => {
            output.push(STRING_SEQUENCE);
            append_len(output, values.len());
            for value in values {
                append_bytes(output, value.as_bytes());
            }
        }
        RhoScalarRef::BytesSequence(values) => {
            output.push(BYTES_SEQUENCE);
            append_len(output, values.len());
            for value in values {
                append_bytes(output, value);
            }
        }
    }
}

fn append_pathmap(output: &mut Vec<u8>, map: &EPathMap) {
    output.push(PATHMAP);
    output.push(map.mode() as u8);
    append_len(output, map.len());
    output.extend_from_slice(&epathmap_match_digest(map));
}

/// Deterministic EPathMap key-topology digest for the derived match index.
///
/// Values in map mode are deliberately absent: the native EPathMap matcher is
/// the acceptance authority, so equal key topology may admit a conservative
/// false positive but can never accept one. Mode and cardinality are encoded
/// beside this digest by [`encode_match_path`].
pub fn epathmap_match_digest(map: &EPathMap) -> [u8; 32] {
    let digest = match map.representation() {
        EPathMapRepr::Empty => logical_pathmap_digest::<()>(None),
        EPathMapRepr::Set(entries) => logical_pathmap_digest(Some(entries)),
        EPathMapRepr::Map(entries) => logical_pathmap_digest(Some(entries)),
    };
    digest
        .try_into()
        .expect("BLAKE2b-256 always returns exactly 32 bytes")
}

fn logical_pathmap_digest<V>(entries: Option<&PathMap<V>>) -> Vec<u8>
where V: Clone + Send + Sync + Unpin {
    const DOMAIN: &[u8] = b"RhoMatchPath/EPathMap/key-topology/v1";
    const EDGE: u8 = 1;
    const LEAVE: u8 = 2;

    Blake2b256::hash_stream(|feed| {
        feed(DOMAIN);
        let Some(entries) = entries else {
            feed(&[0, LEAVE]);
            return;
        };
        let mut zipper = entries.read_zipper();
        feed(&[u8::from(zipper.get_val().is_some())]);
        let mut levels: Vec<ByteMaskIter> = vec![zipper.child_mask().iter()];
        while let Some(children) = levels.last_mut() {
            match children.next() {
                Some(byte) => {
                    feed(&[EDGE, byte]);
                    zipper.descend_to_byte(byte);
                    feed(&[u8::from(zipper.get_val().is_some())]);
                    levels.push(zipper.child_mask().iter());
                }
                None => {
                    feed(&[LEAVE]);
                    levels.pop();
                    if !levels.is_empty() {
                        assert!(zipper.ascend_byte());
                    }
                }
            }
        }
    })
}

fn append_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    append_len(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn append_len(output: &mut Vec<u8>, len: usize) {
    let len = u64::try_from(len).expect("an allocated rhoapi collection length fits in u64");
    output.extend_from_slice(&len.to_be_bytes());
}
