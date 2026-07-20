// See rspace/src/main/scala/coop/rchain/rspace/serializers/ScodecSerialize.
// scala

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::trace::event::{Consume, Produce};

// ─────────────────────────────────────────────────────────────────────────────
// EPathMap fix P4.1 — the serializer MATERIALIZATION boundary.
//
// `Datum`/`WaitingContinuation` are Arc-shaped and deliberately non-serde
// (fail-closed: no serde "rc" feature, so an Arc field reaching
// `bincode::serialize` cannot compile). This module is where the storage
// shape materializes back into the historical wire layout, through borrowed
// SERIALIZE TWINS and owned DESERIALIZE TWINS whose field order replicates
// the pre-P4.1 struct declarations exactly. bincode 1.3.3 (legacy
// fixint-LE) is positional — struct/field names never reach the wire and
// `serialize(&T) == serialize(T)` — so the twin encodings are byte-identical
// to the old derived impls BY CONSTRUCTION. Pinned by
// `tests/serializer_byte_goldens.rs` (literal bytes captured at 4e422b6b)
// and transitively by every history checkpoint-root test: these bytes are
// cold-store leaves, a consensus surface.
// ─────────────────────────────────────────────────────────────────────────────

/// Borrowed serialize twin of the pre-P4.1 `Datum<A>` layout
/// (`a`, `persist`, `source` — declaration order).
#[derive(Serialize)]
struct DatumSer<'a, A> {
    a: &'a A,
    persist: bool,
    source: &'a Produce,
}

/// Owned deserialize twin of the pre-P4.1 `Datum<A>` layout.
#[derive(Deserialize)]
struct DatumDe<A> {
    a: A,
    persist: bool,
    source: Produce,
}

impl<A: Clone> From<DatumDe<A>> for Datum<A> {
    fn from(de: DatumDe<A>) -> Self {
        Datum {
            a: Arc::new(de.a),
            persist: de.persist,
            source: de.source,
        }
    }
}

/// Borrowed serialize twin of the pre-P4.1 `WaitingContinuation<P, K>` layout
/// (`patterns`, `continuation`, `persist`, `peeks`, `source`).
#[derive(Serialize)]
struct WaitingContinuationSer<'a, P, K> {
    patterns: &'a Vec<P>,
    continuation: &'a K,
    persist: bool,
    peeks: &'a BTreeSet<i32>,
    source: &'a Consume,
}

/// Owned deserialize twin of the pre-P4.1 `WaitingContinuation<P, K>` layout.
#[derive(Deserialize)]
struct WaitingContinuationDe<P, K> {
    patterns: Vec<P>,
    continuation: K,
    persist: bool,
    peeks: BTreeSet<i32>,
    source: Consume,
}

impl<P: Clone, K: Clone> From<WaitingContinuationDe<P, K>> for WaitingContinuation<P, K> {
    fn from(de: WaitingContinuationDe<P, K>) -> Self {
        WaitingContinuation {
            patterns: Arc::new(de.patterns),
            continuation: Arc::new(de.continuation),
            persist: de.persist,
            peeks: de.peeks,
            source: de.source,
        }
    }
}

fn datum_ser<'a, A: Clone>(datum: &'a Datum<A>) -> DatumSer<'a, A> {
    DatumSer {
        a: &datum.a,
        persist: datum.persist,
        source: &datum.source,
    }
}

fn continuation_ser<'a, P: Clone, K: Clone>(
    wk: &'a WaitingContinuation<P, K>,
) -> WaitingContinuationSer<'a, P, K> {
    WaitingContinuationSer {
        patterns: &wk.patterns,
        continuation: &wk.continuation,
        persist: wk.persist,
        peeks: &wk.peeks,
        source: &wk.source,
    }
}

/// P4.1: the deterministic candidate-ordering bytes (`rspace.rs`
/// `shuffle_with_index` → `deterministic_candidate_hash`). Candidate ordering
/// participates in replay-visible COMM selection, so these bytes must equal
/// the pre-P4.1 `bincode::serialize(candidate)` exactly — they go through
/// the same twins as the cold-store encoding (one layout, one pin).
pub trait CandidateOrderingBytes {
    fn candidate_ordering_bytes(&self) -> Vec<u8>;
}

impl<A: Clone + Serialize> CandidateOrderingBytes for Datum<A> {
    fn candidate_ordering_bytes(&self) -> Vec<u8> {
        bincode::serialize(&datum_ser(self))
            .expect("Serializers: Unable to serialize datum for candidate ordering")
    }
}

impl<P: Clone + Serialize, K: Clone + Serialize> CandidateOrderingBytes
    for WaitingContinuation<P, K>
{
    fn candidate_ordering_bytes(&self) -> Vec<u8> {
        bincode::serialize(&continuation_ser(self))
            .expect("Serializers: Unable to serialize continuation for candidate ordering")
    }
}

pub fn encode_datum<A: Clone + Serialize>(datum: &Datum<A>) -> Vec<u8> {
    bincode::serialize(&datum_ser(datum)).expect("Serializers: Unable to serialize datum")
}

pub fn decode_datum<A: Clone + for<'a> Deserialize<'a>>(vec_bytes: &Vec<u8>) -> Datum<A> {
    let de: DatumDe<A> =
        bincode::deserialize(vec_bytes).expect("Serializers: Unable to deserialize datum");
    de.into()
}

pub fn encode_datums<A: Clone + Serialize>(datums: &Vec<Datum<A>>) -> Vec<u8> {
    let mut serialized_datums: Vec<Vec<u8>> = datums
        .iter()
        .map(|datum| {
            bincode::serialize(&datum_ser(datum)).expect("Serializers: Unable to serialize datum")
        })
        .collect();

    serialized_datums.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_datums).expect("Serializers: Unable to serialize datums")
}

pub fn decode_datums<A: Clone + for<'a> Deserialize<'a>>(vec_bytes: &Vec<u8>) -> Vec<Datum<A>> {
    let encoded_datums: Vec<Vec<u8>> =
        bincode::deserialize(vec_bytes).expect("Serializers: Unable to deserialize datums");

    let datums = encoded_datums
        .iter()
        .map(|bytes| {
            let de: DatumDe<A> =
                bincode::deserialize(bytes).expect("Serializers: Unable to deserialize datum");
            de.into()
        })
        .collect();
    datums
}

pub fn encode_continuations<P: Clone + Serialize, K: Clone + Serialize>(
    conts: &Vec<WaitingContinuation<P, K>>,
) -> Vec<u8> {
    let mut serialized_continuations: Vec<Vec<u8>> = conts
        .iter()
        .map(|wk| {
            bincode::serialize(&continuation_ser(wk))
                .expect("Serializers: Unable to serialize continuation")
        })
        .collect();

    serialized_continuations.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_continuations)
        .expect("Serializers: Unable to serialize continuations")
}

pub fn decode_continuations<P, K>(vec_bytes: &Vec<u8>) -> Vec<WaitingContinuation<P, K>>
where
    P: Clone + for<'a> Deserialize<'a>,
    K: Clone + for<'a> Deserialize<'a>,
{
    let encoded_continuations: Vec<Vec<u8>> =
        bincode::deserialize(vec_bytes).expect("Serializers: Unable to deserialize continuations");

    let continuations = encoded_continuations
        .iter()
        .map(|bytes| {
            let de: WaitingContinuationDe<P, K> = bincode::deserialize(bytes)
                .expect("Serializers: Unable to deserialize continuation");
            de.into()
        })
        .collect();
    continuations
}

pub fn encode_joins<C: Clone + Serialize>(joins: &Vec<Vec<C>>) -> Vec<u8> {
    let mut serialized_joins: Vec<Vec<u8>> = joins
        .iter()
        .map(|datum| bincode::serialize(datum).expect("Serializers: Unable to serialize join"))
        .collect();

    serialized_joins.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_joins).expect("Serializers: Unable to serialize joins")
}

pub fn decode_joins<C: Clone + for<'a> Deserialize<'a>>(vec_bytes: &Vec<u8>) -> Vec<Vec<C>> {
    let encoded_joins: Vec<Vec<u8>> =
        bincode::deserialize(vec_bytes).expect("Serializers: Unable to deserialize joins");

    let joins = encoded_joins
        .iter()
        .map(|bytes| bincode::deserialize(bytes).expect("Serializers: Unable to deserialize join"))
        .collect();
    joins
}

pub fn encode_binary(binary: &Vec<Vec<u8>>) -> Vec<u8> {
    let mut serialized_binary: Vec<Vec<u8>> = binary
        .iter()
        .map(|datum| bincode::serialize(datum).expect("Serializers: Unable to serialize binary"))
        .collect();

    serialized_binary.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_binary).expect("Serializers: Unable to serialize binaries")
}

// See rspace/src/main/scala/coop/rchain/rspace/util/package.scala
fn compare_byte_vectors(a: &Vec<u8>, b: &Vec<u8>) -> Ordering {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.cmp(y))
        .find(|&ord| ord != Ordering::Equal)
        .unwrap_or(a.len().cmp(&b.len()))
}
