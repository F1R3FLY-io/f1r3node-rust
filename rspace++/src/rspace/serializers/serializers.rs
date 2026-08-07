// See rspace/src/main/scala/coop/rchain/rspace/serializers/ScodecSerialize.
// scala

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Only the `#[cfg(test)]` ORACLE twins (`DatumOracleDe`,
/// `WaitingContinuationOracleDe`) derive `Deserialize` now — production decode
/// goes through `ColdStoreDecode`.
#[cfg(test)]
use serde::Deserialize;
use serde::Serialize;

use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, cold_decode_vec_prefix, legacy_tail,
};
use crate::rspace::trace::event::{Consume, Produce};

// ─────────────────────────────────────────────────────────────────────────────
// The EPathMap serializer materialization boundary.
//
// `Datum`/`WaitingContinuation` are Arc-shaped and deliberately non-serde
// (fail-closed: no serde "rc" feature, so an Arc field reaching
// `bincode::serialize` cannot compile). This module is where the storage
// shape materializes back into the historical wire layout, through borrowed
// SERIALIZE TWINS and owned DESERIALIZE TWINS whose field order replicates
// the earlier value-shaped struct declarations exactly. bincode 1.3.3 (legacy
// fixint-LE) is positional — struct/field names never reach the wire and
// `serialize(&T) == serialize(T)` — so the twin encodings are byte-identical
// to the old derived impls BY CONSTRUCTION. Pinned by
// `tests/serializer_byte_goldens.rs` (literal bytes captured at 4e422b6b)
// and transitively by every history checkpoint-root test: these bytes are
// cold-store leaves, a consensus surface.
// ─────────────────────────────────────────────────────────────────────────────

/// Borrowed serialize twin of the value-shaped `Datum<A>` layout
/// (`a`, `persist`, `source` — declaration order).
#[derive(Serialize)]
struct DatumSer<'a, A> {
    a: &'a A,
    persist: bool,
    source: &'a Produce,
}

/// Owned deserialize twin of the value-shaped `Datum<A>` layout.
///
/// ⚠ **Retained as a `#[cfg(test)]` ORACLE only.** Production decoding goes
/// through [`decode_datum`], which reads `a` with the O(1)-native-stack machine
/// (`models/src/rust/rholang/bincode_decoder.rs`) and the bounded tail with
/// bincode. This twin is what the differential in this file's `mod tests`
/// compares against; the `Par`-typed half lives in
/// `models/tests/cold_store_records.rs`, on the far side of the `models ->
/// rspace_plus_plus` dependency edge.
///
/// It is a better oracle than the serialize twins were: those are
/// hand-maintained and could drift from the struct they mirror, whereas this
/// one's *body* is compiler-generated from the field list, so the only thing
/// that can drift is the field list itself — which is the same list
/// [`decode_datum`] reads, three lines below it.
#[cfg(test)]
#[derive(Deserialize)]
pub(crate) struct DatumOracleDe<A> {
    pub a: A,
    pub persist: bool,
    pub source: Produce,
}

#[cfg(test)]
impl<A: Clone> From<DatumOracleDe<A>> for Datum<A> {
    fn from(de: DatumOracleDe<A>) -> Self {
        Datum {
            a: Arc::new(de.a),
            persist: de.persist,
            source: de.source,
        }
    }
}

/// Borrowed serialize twin of the value-shaped `WaitingContinuation<P, K>`
/// layout (`patterns`, `continuation`, `persist`, `peeks`, `source`).
#[derive(Serialize)]
struct WaitingContinuationSer<'a, P, K> {
    patterns: &'a Vec<P>,
    continuation: &'a K,
    persist: bool,
    peeks: &'a BTreeSet<i32>,
    source: &'a Consume,
}

/// Owned deserialize twin of the value-shaped `WaitingContinuation<P, K>`
/// layout. Retained as a `#[cfg(test)]` ORACLE only — see [`DatumOracleDe`].
#[cfg(test)]
#[derive(Deserialize)]
pub(crate) struct WaitingContinuationOracleDe<P, K> {
    pub patterns: Vec<P>,
    pub continuation: K,
    pub persist: bool,
    pub peeks: BTreeSet<i32>,
    pub source: Consume,
}

#[cfg(test)]
impl<P: Clone, K: Clone> From<WaitingContinuationOracleDe<P, K>> for WaitingContinuation<P, K> {
    fn from(de: WaitingContinuationOracleDe<P, K>) -> Self {
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

/// The deterministic candidate-ordering bytes (`rspace.rs`
/// `shuffle_with_index` → `deterministic_candidate_hash`). Candidate ordering
/// participates in replay-visible COMM selection, so these bytes must equal
/// the earlier value-shaped `bincode::serialize(candidate)` exactly — they go
/// through the same twins as the cold-store encoding (one layout, one pin).
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

// ─────────────────────────────────────────────────────────────────────────────
// THE DECODE BOUNDARY
//
// The four `decode_*` below are the node's cold-store READ path: every datum,
// continuation and join comes back through them, and the bytes they read were
// not necessarily produced by this node — `rspace_importer` writes peer bytes
// to LMDB without ever deep-decoding them. Their bound is therefore
// `ColdStoreDecode` (the O(1)-native-stack machine) rather than
// `for<'a> Deserialize<'a>` (bincode's unbounded recursive descent), and they
// return `Result` rather than `.expect(..)`, because a too-deep or malformed
// datum must fail ONE read rather than abort the process.
//
// Their `Serialize` twins above are untouched, so the bytes are unchanged.
// ─────────────────────────────────────────────────────────────────────────────

/// Decode a `Datum<A>` — `{ a: A, persist: bool, source: Produce }`.
///
/// `a` is a **prefix**: bincode is positional and not self-delimiting, so the
/// only way to know where `persist` starts is to have parsed `a`. The machine
/// reports that extent; the remaining two fields are bounded-depth and decode
/// as a bincode tuple, which is byte-identical to the derived struct read (see
/// [`legacy_tail`]).
pub fn decode_datum<A: Clone + ColdStoreDecode>(
    vec_bytes: &[u8],
) -> Result<Datum<A>, ColdStoreDecodeError> {
    let (a, consumed) = A::cold_decode_prefix(vec_bytes)?;
    let (persist, source): (bool, Produce) = legacy_tail(&vec_bytes[consumed..])?;
    Ok(Datum {
        a: Arc::new(a),
        persist,
        source,
    })
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

/// The cold-store DATA leaf: a `Vec<Vec<u8>>` of per-datum encodings.
///
/// The outer `Vec<Vec<u8>>` is two levels deep by construction, so it goes
/// through bincode; each inner element is a whole `Datum<A>` encoding and goes
/// through [`decode_datum`].
pub fn decode_datums<A: Clone + ColdStoreDecode>(
    vec_bytes: &[u8],
) -> Result<Vec<Datum<A>>, ColdStoreDecodeError> {
    let encoded_datums: Vec<Vec<u8>> = legacy_tail(vec_bytes)?;
    let mut datums = Vec::with_capacity(encoded_datums.len());
    for bytes in &encoded_datums {
        datums.push(decode_datum(bytes)?);
    }
    Ok(datums)
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

/// Decode one `WaitingContinuation<P, K>` — `{ patterns: Vec<P>,
/// continuation: K, persist: bool, peeks: BTreeSet<i32>, source: Consume }`.
///
/// `patterns` and `continuation` are both `Par`-bearing prefixes and go through
/// the machine; the remaining three fields are bounded and decode as one
/// bincode tuple.
fn decode_continuation<P, K>(
    vec_bytes: &[u8],
) -> Result<WaitingContinuation<P, K>, ColdStoreDecodeError>
where
    P: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    let (patterns, mut consumed) = cold_decode_vec_prefix::<P>(vec_bytes)?;
    let (continuation, used) = K::cold_decode_prefix(&vec_bytes[consumed..])?;
    consumed += used;
    let (persist, peeks, source): (bool, BTreeSet<i32>, Consume) =
        legacy_tail(&vec_bytes[consumed..])?;
    Ok(WaitingContinuation {
        patterns: Arc::new(patterns),
        continuation: Arc::new(continuation),
        persist,
        peeks,
        source,
    })
}

/// The cold-store CONTINUATION leaf: a `Vec<Vec<u8>>` of per-continuation
/// encodings. See [`decode_datums`] for the outer/inner split.
pub fn decode_continuations<P, K>(
    vec_bytes: &[u8],
) -> Result<Vec<WaitingContinuation<P, K>>, ColdStoreDecodeError>
where
    P: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    let encoded_continuations: Vec<Vec<u8>> = legacy_tail(vec_bytes)?;
    let mut continuations = Vec::with_capacity(encoded_continuations.len());
    for bytes in &encoded_continuations {
        continuations.push(decode_continuation(bytes)?);
    }
    Ok(continuations)
}

pub fn encode_joins<C: Clone + Serialize>(joins: &Vec<Vec<C>>) -> Vec<u8> {
    let mut serialized_joins: Vec<Vec<u8>> = joins
        .iter()
        .map(|datum| bincode::serialize(datum).expect("Serializers: Unable to serialize join"))
        .collect();

    serialized_joins.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_joins).expect("Serializers: Unable to serialize joins")
}

/// The cold-store JOINS leaf: a `Vec<Vec<u8>>` whose inner elements are each a
/// `Vec<C>` encoding (a channel list). `C` is `Par` in production.
pub fn decode_joins<C: Clone + ColdStoreDecode>(
    vec_bytes: &[u8],
) -> Result<Vec<Vec<C>>, ColdStoreDecodeError> {
    let encoded_joins: Vec<Vec<u8>> = legacy_tail(vec_bytes)?;
    let mut joins = Vec::with_capacity(encoded_joins.len());
    for bytes in &encoded_joins {
        let (join, _consumed) = cold_decode_vec_prefix::<C>(bytes)?;
        joins.push(join);
    }
    Ok(joins)
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

// ═════════════════════════════════════════════════════════════════════════════
// THE RECORD-LEVEL DIFFERENTIAL
//
// `models/tests/bincode_decoder_differential.rs` proves the machine and the
// derive agree on `Par`, `ListParWithRandom`, `BindPattern` and
// `TaggedContinuation`. What it cannot reach from over there is the COMPOSITION
// performed here: a `Datum<A>` decode is "machine prefix, then bincode tuple
// tail", and the claim that this equals the derived whole-struct read rests on
// bincode tuples being pure concatenation. That claim is checked below against
// the retained compiler-generated oracles, on the rspace++ test-double
// instantiation.
//
// The `Par`-typed half of the same claim is in
// `models/tests/cold_store_records.rs` — it has to be, because `models` depends
// on `rspace_plus_plus` and not the other way round.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    fn datum() -> Datum<String> {
        Datum {
            a: Arc::new("payload".to_string()),
            persist: true,
            source: Produce::new(
                Blake2b256Hash::new(&[1, 2, 3]),
                Blake2b256Hash::new(&[4, 5, 6]),
                true,
            ),
        }
    }

    fn continuation() -> WaitingContinuation<String, String> {
        WaitingContinuation {
            patterns: Arc::new(vec!["p0".to_string(), "p1".to_string()]),
            continuation: Arc::new("k".to_string()),
            persist: false,
            peeks: BTreeSet::from([-1, 0, 4]),
            source: Consume {
                channel_hashes: vec![Blake2b256Hash::new(&[7])],
                hash: Blake2b256Hash::new(&[8]),
                persistent: true,
            },
        }
    }

    /// The composition claim for `Datum<A>`: prefix + tuple tail == the derived
    /// whole-struct read.
    #[test]
    fn decode_datum_matches_the_derived_oracle() {
        let bytes = encode_datum(&datum());
        let oracle: Datum<String> = bincode::deserialize::<DatumOracleDe<String>>(&bytes)
            .expect("oracle")
            .into();
        let machine = decode_datum::<String>(&bytes).expect("machine");
        assert_eq!(machine, oracle);
        assert_eq!(machine, datum());
    }

    /// The same for `WaitingContinuation<P, K>`, which has TWO machine-decoded
    /// prefixes (`patterns: Vec<P>` and `continuation: K`) before its tail.
    #[test]
    fn decode_continuation_matches_the_derived_oracle() {
        let bytes = bincode::serialize(&continuation_ser(&continuation())).expect("encode");
        let oracle: WaitingContinuation<String, String> =
            bincode::deserialize::<WaitingContinuationOracleDe<String, String>>(&bytes)
                .expect("oracle")
                .into();
        let machine = decode_continuation::<String, String>(&bytes).expect("machine");
        assert_eq!(machine, oracle);
        assert_eq!(machine, continuation());
    }

    /// The REJECTION SET at the record level. A `Datum` that decodes its `a`
    /// prefix successfully but then runs out of tail must be rejected by both —
    /// a decoder that reported the wrong extent would read `persist` and
    /// `source` out of the middle of `a` and cheerfully return a wrong value.
    #[test]
    fn truncated_records_are_rejected_by_both() {
        let datum_bytes = encode_datum(&datum());
        let cont_bytes = bincode::serialize(&continuation_ser(&continuation())).expect("encode");
        let mut checked = 0usize;

        for cut in 0..datum_bytes.len() {
            let slice = &datum_bytes[..cut];
            let oracle = bincode::deserialize::<DatumOracleDe<String>>(slice).is_ok();
            let machine = decode_datum::<String>(slice).is_ok();
            assert_eq!(
                oracle, machine,
                "Datum truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
        }
        for cut in 0..cont_bytes.len() {
            let slice = &cont_bytes[..cut];
            let oracle =
                bincode::deserialize::<WaitingContinuationOracleDe<String, String>>(slice).is_ok();
            let machine = decode_continuation::<String, String>(slice).is_ok();
            assert_eq!(
                oracle, machine,
                "WaitingContinuation truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
        }
        assert!(checked > 100, "ANTI-VACUITY: only {checked} truncations tried");
    }

    /// The leaf-level round trips (`Vec<Vec<u8>>` outer, per-record inner).
    #[test]
    fn leaf_round_trips() {
        let datums = vec![datum()];
        let decoded = decode_datums::<String>(&encode_datums(&datums)).expect("datums");
        assert_eq!(decoded, datums);

        let conts = vec![continuation()];
        let decoded = decode_continuations::<String, String>(&encode_continuations(&conts))
            .expect("continuations");
        assert_eq!(decoded, conts);

        let joins: Vec<Vec<String>> = vec![vec!["a".into(), "b".into()], vec![]];
        let decoded = decode_joins::<String>(&encode_joins(&joins)).expect("joins");
        // `encode_joins` sorts by encoded bytes, so compare as sets.
        assert_eq!(decoded.len(), joins.len());
        for j in &joins {
            assert!(decoded.contains(j), "decoded joins must contain {:?}", j);
        }
    }

    /// `legacy_prefix` must report the exact extent, or the tail of a `Datum`
    /// lands in the wrong place. Checked directly, because every test-double
    /// instantiation depends on it.
    #[test]
    fn legacy_prefix_reports_the_exact_extent() {
        use crate::rspace::serializers::cold_store_decode::{ColdStoreDecode, legacy_prefix};

        let value = "a string with a λ in it".to_string();
        let mut bytes = bincode::serialize(&value).expect("encode");
        let exact = bytes.len();
        bytes.extend_from_slice(&[0xAB, 0xCD]);

        let (decoded, consumed) = legacy_prefix::<String>(&bytes).expect("legacy_prefix");
        assert_eq!(decoded, value);
        assert_eq!(consumed, exact);
        assert_eq!(String::cold_decode(&bytes).expect("cold_decode"), value);
    }

    /// ⚠ The reason `legacy_prefix` carries a byte limit.
    /// `IoReader::fill_buffer` does `temp_buffer.resize(length, 0)` BEFORE
    /// reading, so without the limit a `String` prefixed with `[0xFF; 8]`
    /// would try to allocate ~16 EiB and abort — while
    /// `bincode::deserialize` (SliceReader) returns a clean `Err`.
    /// If this test ever OOMs instead of failing, the limit has been dropped.
    #[test]
    fn legacy_prefix_rejects_a_hostile_length_without_allocating() {
        use crate::rspace::serializers::cold_store_decode::legacy_prefix;

        let hostile = vec![0xFFu8; 8];
        assert!(bincode::deserialize::<String>(&hostile).is_err());
        assert!(legacy_prefix::<String>(&hostile).is_err());
    }
}
