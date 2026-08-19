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
pub use rspace_plus_plus::rspace::rho_path::RHO_MATCH_PATH_VERSION;
pub(super) use rspace_plus_plus::rspace::rho_path::{
    RHO_MATCH_ABSENT as ABSENT, RHO_MATCH_BOOL as BOOL, RHO_MATCH_BYTES as BYTES,
    RHO_MATCH_BYTES_SEQUENCE as BYTES_SEQUENCE, RHO_MATCH_FIELD_END as FIELD_END,
    RHO_MATCH_FIELD_START as FIELD_START, RHO_MATCH_I32 as I32, RHO_MATCH_I64 as I64,
    RHO_MATCH_MAP_END as MAP_END, RHO_MATCH_MAP_KEY as MAP_KEY, RHO_MATCH_MAP_START as MAP_START,
    RHO_MATCH_NODE_END as NODE_END, RHO_MATCH_NODE_START as NODE_START,
    RHO_MATCH_ONEOF_END as ONEOF_END, RHO_MATCH_ONEOF_START as ONEOF_START,
    RHO_MATCH_PAR_BAG as PAR_BAG, RHO_MATCH_PATHMAP as PATHMAP,
    RHO_MATCH_SEQUENCE_END as SEQUENCE_END, RHO_MATCH_SEQUENCE_START as SEQUENCE_START,
    RHO_MATCH_STRING as STRING, RHO_MATCH_STRING_SEQUENCE as STRING_SEQUENCE, RHO_MATCH_U32 as U32,
    RHO_MATCH_U64 as U64,
};

use super::match_schema::{RhoPathCursor, RhoPathEvent, RhoScalarRef};
use super::match_schema_tables::ParCursor;
use crate::rhoapi::{
    Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send as RhoSend,
};
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::par_bag::ParBag;
use crate::rust::rhoapi_ext::EPathMap;

/// Encode one `Par` as a complete-schema structural RSpace search path.
pub fn encode_par_match_path(value: &Par) -> Vec<u8> { encode_match_path(ParCursor::from(value)) }

/// Decode the sole expression of a `Par` whose authoritative component Bag
/// contains exactly that one occurrence and no other component family.
///
/// This projection is intentionally limited to the candidate-index boundary:
/// it exposes ordered structure beneath the compact ParBag token without
/// restoring a retained `Vec<Par>` representation.
pub fn is_single_expr_match_carrier(value: &Par) -> bool {
    value.family_cardinality::<RhoSend>() == 0
        && value.family_cardinality::<Receive>() == 0
        && value.family_cardinality::<New>() == 0
        && value.family_cardinality::<Expr>() == 1
        && value.family_cardinality::<Match>() == 0
        && value.family_cardinality::<GUnforgeable>() == 0
        && value.family_cardinality::<Bundle>() == 0
        && value.family_cardinality::<Connective>() == 0
        && value.family_cardinality::<If>() == 0
}

pub fn single_expr_match_component(value: &Par) -> Option<Expr> {
    if !is_single_expr_match_carrier(value) {
        return None;
    }
    value
        .first::<Expr>()
        .expect("Par invariant: stored Expr keys are canonical")
        .map(|(expr, _)| expr)
}

/// Candidate-index projection for a `Par` atom.
///
/// A sole expression is encoded from its generated `Expr` cursor so ordered
/// holes nested below EList/ETuple can retain a useful trie prefix. Every other
/// Par remains the compact, byte-native ParBag token. The authoritative matcher
/// and full bincode identity still operate on the complete Par.
pub fn encode_par_match_atom(value: &Par) -> Vec<u8> {
    match single_expr_match_component(value) {
        Some(expr) => encode_match_path(ParCursor::from(&expr)),
        None => encode_par_match_path(value),
    }
}

/// Encode any generated rhoapi cursor as a complete-schema structural path.
pub fn encode_match_path(root: ParCursor<'_>) -> Vec<u8> {
    let mut output = Vec::with_capacity(256);
    output.extend_from_slice(&RHO_MATCH_PATH_VERSION);
    for event in RhoPathCursor::new(root) {
        append_match_path_event(&mut output, event);
    }
    output
}

/// Prove that two generated-schema values have the same canonical bincode
/// image without constructing that image.
///
/// The lockstep walk compares every scalar, field/sequence/map boundary, and
/// oneof discriminator. Opaque PathMap-backed fields are accepted only when
/// they retain the same immutable CoW root. A `true` result is therefore a
/// sound byte-identity witness; `false` merely means that callers must use the
/// ordinary encoder/key lookup. The walk is iterative and constant-stack.
pub fn shares_cow_bincode_identity(left: ParCursor<'_>, right: ParCursor<'_>) -> bool {
    let mut left = RhoPathCursor::new(left);
    let mut right = RhoPathCursor::new(right);

    loop {
        let equal = match (left.next(), right.next()) {
            (None, None) => return true,
            (
                Some(RhoPathEvent::EnterNode { program: left, .. }),
                Some(RhoPathEvent::EnterNode { program: right, .. }),
            ) => left.type_id == right.type_id,
            (Some(RhoPathEvent::LeaveNode), Some(RhoPathEvent::LeaveNode)) => true,
            (Some(RhoPathEvent::EnterField(left)), Some(RhoPathEvent::EnterField(right))) => {
                left.tag == right.tag
            }
            (Some(RhoPathEvent::LeaveField), Some(RhoPathEvent::LeaveField)) => true,
            (Some(RhoPathEvent::Scalar(left)), Some(RhoPathEvent::Scalar(right))) => left == right,
            (Some(RhoPathEvent::Absent), Some(RhoPathEvent::Absent)) => true,
            (Some(RhoPathEvent::EnterSequence(left)), Some(RhoPathEvent::EnterSequence(right))) => {
                left == right
            }
            (Some(RhoPathEvent::LeaveSequence), Some(RhoPathEvent::LeaveSequence)) => true,
            (Some(RhoPathEvent::EnterMap(left)), Some(RhoPathEvent::EnterMap(right))) => {
                left == right
            }
            (Some(RhoPathEvent::MapKey(left)), Some(RhoPathEvent::MapKey(right))) => left == right,
            (Some(RhoPathEvent::LeaveMap), Some(RhoPathEvent::LeaveMap)) => true,
            (Some(RhoPathEvent::EnterOneof(left)), Some(RhoPathEvent::EnterOneof(right))) => {
                left.protobuf_tag == right.protobuf_tag && left.serde_index == right.serde_index
            }
            (Some(RhoPathEvent::LeaveOneof), Some(RhoPathEvent::LeaveOneof)) => true,
            (Some(RhoPathEvent::ParBag(left)), Some(RhoPathEvent::ParBag(right))) => {
                left.shares_storage_with(right)
            }
            (Some(RhoPathEvent::PathMap(left)), Some(RhoPathEvent::PathMap(right))) => {
                left.shares_storage_with(right)
            }
            _ => false,
        };
        if !equal {
            return false;
        }
    }
}

/// Append one generated cursor event to an `RMP1` byte stream.
///
/// Pattern compilers use the same primitive as the exact encoder so literal
/// instruction runs cannot drift from datum encoding.
pub fn append_match_path_event(output: &mut Vec<u8>, event: RhoPathEvent<'_>) {
    match event {
        RhoPathEvent::EnterNode { program, .. } => {
            output.push(NODE_START);
            output.extend_from_slice(&program.type_id.to_be_bytes());
        }
        RhoPathEvent::LeaveNode => output.push(NODE_END),
        RhoPathEvent::EnterField(field) => {
            output.push(FIELD_START);
            output.extend_from_slice(&field.tag.to_be_bytes());
        }
        RhoPathEvent::LeaveField => output.push(FIELD_END),
        RhoPathEvent::Scalar(value) => append_scalar(output, value),
        RhoPathEvent::Absent => output.push(ABSENT),
        RhoPathEvent::EnterSequence(len) => {
            output.push(SEQUENCE_START);
            append_len(output, len);
        }
        RhoPathEvent::LeaveSequence => output.push(SEQUENCE_END),
        RhoPathEvent::EnterMap(len) => {
            output.push(MAP_START);
            append_len(output, len);
        }
        RhoPathEvent::MapKey(key) => {
            output.push(MAP_KEY);
            append_bytes(output, key.as_bytes());
        }
        RhoPathEvent::LeaveMap => output.push(MAP_END),
        RhoPathEvent::EnterOneof(oneof) => {
            output.push(ONEOF_START);
            output.extend_from_slice(&oneof.protobuf_tag.to_be_bytes());
        }
        RhoPathEvent::LeaveOneof => output.push(ONEOF_END),
        RhoPathEvent::ParBag(bag) => append_par_bag(output, bag),
        RhoPathEvent::PathMap(map) => append_pathmap(output, map),
    }
}

fn append_par_bag(output: &mut Vec<u8>, bag: &ParBag) {
    const DOMAIN: &[u8] = b"RhoMatchPath/ParBag/compact/v1";

    output.push(PAR_BAG);
    append_len(output, bag.distinct_len());
    output.extend_from_slice(&bag.cardinality().to_be_bytes());
    let digest = Blake2b256::hash_stream(|feed| {
        feed(DOMAIN);
        feed(bag.compact_snapshot());
    });
    output.extend_from_slice(&digest);
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
