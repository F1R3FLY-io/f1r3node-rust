//! Stack-safe structural paths for RSpace matching.
//!
//! This format is a derived in-memory search key, not a protobuf, bincode, or
//! consensus identity encoding. It follows the generated matching cursor and
//! therefore covers the complete rhoapi descriptor without recursive calls.
//! `EPathMap` stays native: its token is computed from PathMap's logical trie
//! hash and never materializes entries or forces the EPM1 snapshot cache.

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
    let logical_hash = match map.representation() {
        EPathMapRepr::Empty => PathMap::<()>::INVIS_HASH,
        EPathMapRepr::Set(entries) => entries.hash(|_| PathMap::<()>::INVIS_HASH),
        EPathMapRepr::Map(entries) => entries.hash(|_| PathMap::<Par>::INVIS_HASH),
    };
    output.extend_from_slice(&logical_hash.to_be_bytes());
}

fn append_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    append_len(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn append_len(output: &mut Vec<u8>, len: usize) {
    let len = u64::try_from(len).expect("an allocated rhoapi collection length fits in u64");
    output.extend_from_slice(&len.to_be_bytes());
}
