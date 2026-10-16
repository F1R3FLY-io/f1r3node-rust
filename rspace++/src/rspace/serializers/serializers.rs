// See rspace/src/main/scala/coop/rchain/rspace/serializers/ScodecSerialize.
// scala

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use shared::rust::closed_decode::ClosedDecode;

use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::{self, SourceMeter};
use crate::rspace::hashing::stable_hash_provider::{
    EventCodecEpoch, StableHashSerialize, event_leg_bytes_for_epoch,
};
use crate::rspace::history::native_reader::{
    NativeReadError, NativeReadMeter, decode_history_record,
};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, cold_decode_vec_prefix,
    cold_decode_vec_prefix_for_epoch, legacy_tail,
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
/// Ordinary cold-store reads use [`decode_datum`] and the iterative
/// `ColdStoreDecode` machine. Native-replay reads use this owned twin only
/// through the separately bounded `native_reader::decode_record` visitor:
/// that visitor charges each deserialize operation before allocation and
/// rejects depth 128, both consensus-visible properties of the committed
/// cost-accounting branch. The twin also remains the cold-decoder oracle.
///
/// It is a better oracle than the serialize twins were: those are
/// hand-maintained and could drift from the struct they mirror, whereas this
/// one's *body* is compiler-generated from the field list, so the only thing
/// that can drift is the field list itself — which is the same list
/// [`decode_datum`] reads, three lines below it.
#[derive(Deserialize)]
pub(crate) struct DatumOracleDe<A> {
    pub a: A,
    pub persist: bool,
    pub source: Produce,
}

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
/// layout. Native replay uses it through the bounded metered visitor; see
/// [`DatumOracleDe`].
#[derive(Deserialize)]
pub(crate) struct WaitingContinuationOracleDe<P, K> {
    pub patterns: Vec<P>,
    pub continuation: K,
    pub persist: bool,
    #[serde(deserialize_with = "crate::rspace::history::native_reader::tree_set")]
    pub peeks: BTreeSet<i32>,
    pub source: Consume,
}

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

    /// Hash the same borrowed wire layout while reserving each serializer
    /// write before it reads or allocates. Native replay uses this for ties.
    fn metered_candidate_hash(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError>;
}

impl<A: Clone + Serialize> CandidateOrderingBytes for Datum<A> {
    fn candidate_ordering_bytes(&self) -> Vec<u8> {
        bincode::serialize(&datum_ser(self))
            .expect("Serializers: Unable to serialize datum for candidate ordering")
    }

    fn metered_candidate_hash(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError> {
        native_source::hash(&datum_ser(self), &|operations, scanned, backing| {
            meter.reserve(operations, scanned, backing)
        })
    }
}

impl<P: Clone + Serialize, K: Clone + Serialize> CandidateOrderingBytes
    for WaitingContinuation<P, K>
{
    fn candidate_ordering_bytes(&self) -> Vec<u8> {
        bincode::serialize(&continuation_ser(self))
            .expect("Serializers: Unable to serialize continuation for candidate ordering")
    }

    fn metered_candidate_hash(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError> {
        native_source::hash(&continuation_ser(self), &|operations, scanned, backing| {
            meter.reserve(operations, scanned, backing)
        })
    }
}

impl<T: CandidateOrderingBytes + ?Sized> CandidateOrderingBytes for Arc<T> {
    fn candidate_ordering_bytes(&self) -> Vec<u8> { (**self).candidate_ordering_bytes() }
    fn metered_candidate_hash(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError> {
        (**self).metered_candidate_hash(meter)
    }
}

impl<T: CandidateOrderingBytes + Clone> CandidateOrderingBytes for Cow<'_, T> {
    fn candidate_ordering_bytes(&self) -> Vec<u8> { self.as_ref().candidate_ordering_bytes() }
    fn metered_candidate_hash(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<Blake2b256Hash, RSpaceError> {
        self.as_ref().metered_candidate_hash(meter)
    }
}

pub fn encode_datum<A: Clone + Serialize>(datum: &Datum<A>) -> Vec<u8> {
    bincode::serialize(&datum_ser(datum)).expect("Serializers: Unable to serialize datum")
}

/// Metered form of the same borrowed wire twin used by cold-store writes.
pub(crate) fn encode_datum_metered<A: Clone + Serialize>(
    datum: &Datum<A>,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>, RSpaceError> {
    native_source::encode(&datum_ser(datum), meter)
}

/// Metered continuation encoding preserves the historical positional layout.
pub(crate) fn encode_continuation_metered<P: Clone + Serialize, K: Clone + Serialize>(
    continuation: &WaitingContinuation<P, K>,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>, RSpaceError> {
    native_source::encode(&continuation_ser(continuation), meter)
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

pub fn decode_datum_for_epoch<A: Clone + ColdStoreDecode>(
    epoch: EventCodecEpoch,
    bytes: &[u8],
) -> Result<Datum<A>, ColdStoreDecodeError> {
    let (a, consumed) = match epoch {
        EventCodecEpoch::Legacy => A::cold_decode_legacy_prefix(bytes)?,
        EventCodecEpoch::CostV1 => A::cold_decode_cost_v1_prefix(bytes)?,
        EventCodecEpoch::CostV2 => A::cold_decode_prefix(bytes)?,
    };
    let (persist, source): (bool, Produce) = legacy_tail(&bytes[consumed..])?;
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

/// Epoch-selected positional cold leaf. A stored datum has no channel value,
/// so its `source` must have been constructed under this same authenticated
/// epoch before insertion; `Datum::create_for_epoch` is that admission API.
pub fn encode_datums_for_epoch<A: Clone + StableHashSerialize>(
    epoch: EventCodecEpoch,
    datums: &[Datum<A>],
) -> Result<Vec<u8>, RSpaceError> {
    let mut encoded = Vec::with_capacity(datums.len());
    for datum in datums {
        let mut bytes = event_leg_bytes_for_epoch(epoch, datum.a.as_ref())?;
        bytes.extend(
            bincode::serialize(&(datum.persist, &datum.source))
                .expect("bounded datum tail serialization"),
        );
        encoded.push(bytes);
    }
    encoded.sort_by(compare_byte_vectors);
    Ok(bincode::serialize(&encoded).expect("datum leaf serialization"))
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

pub fn decode_datums_for_epoch<A: Clone + ColdStoreDecode>(
    epoch: EventCodecEpoch,
    bytes: &[u8],
) -> Result<Vec<Datum<A>>, ColdStoreDecodeError> {
    let encoded: Vec<Vec<u8>> = legacy_tail(bytes)?;
    encoded
        .iter()
        .map(|item| decode_datum_for_epoch(epoch, item))
        .collect()
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

pub fn encode_continuations_for_epoch<P, K>(
    epoch: EventCodecEpoch,
    continuations: &[WaitingContinuation<P, K>],
) -> Result<Vec<u8>, RSpaceError>
where
    P: Clone + StableHashSerialize,
    K: Clone + StableHashSerialize,
{
    let mut encoded = Vec::with_capacity(continuations.len());
    for waiting in continuations {
        let count =
            u64::try_from(waiting.patterns.len()).map_err(|_| RSpaceError::HostWorkRejected)?;
        let mut bytes = count.to_le_bytes().to_vec();
        for pattern in waiting.patterns.iter() {
            bytes.extend(event_leg_bytes_for_epoch(epoch, pattern)?);
        }
        bytes.extend(event_leg_bytes_for_epoch(epoch, waiting.continuation.as_ref())?);
        bytes.extend(
            bincode::serialize(&(waiting.persist, &waiting.peeks, &waiting.source))
                .expect("bounded continuation tail serialization"),
        );
        encoded.push(bytes);
    }
    encoded.sort_by(compare_byte_vectors);
    Ok(bincode::serialize(&encoded).expect("continuation leaf serialization"))
}

/// Decode one `WaitingContinuation<P, K>` — `{ patterns: Vec<P>,
/// continuation: K, persist: bool, peeks: BTreeSet<i32>, source: Consume }`.
///
/// `patterns` and `continuation` are both `Par`-bearing prefixes and go through
/// the machine; the remaining three fields are bounded and decode as one
/// bincode tuple.
pub(crate) fn decode_continuation<P, K>(
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

pub fn decode_continuation_for_epoch<P, K>(
    epoch: EventCodecEpoch,
    bytes: &[u8],
) -> Result<WaitingContinuation<P, K>, ColdStoreDecodeError>
where
    P: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    let (patterns, mut consumed) = cold_decode_vec_prefix_for_epoch::<P>(epoch, bytes)?;
    let (continuation, used) = match epoch {
        EventCodecEpoch::Legacy => K::cold_decode_legacy_prefix(&bytes[consumed..])?,
        EventCodecEpoch::CostV1 => K::cold_decode_cost_v1_prefix(&bytes[consumed..])?,
        EventCodecEpoch::CostV2 => K::cold_decode_prefix(&bytes[consumed..])?,
    };
    consumed += used;
    let (persist, peeks, source): (bool, BTreeSet<i32>, Consume) = legacy_tail(&bytes[consumed..])?;
    Ok(WaitingContinuation {
        patterns: Arc::new(patterns),
        continuation: Arc::new(continuation),
        persist,
        peeks,
        source,
    })
}

/// Native replay deliberately retains the committed branch's bounded visitor:
/// its exact charge trace and depth-128 rejection are consensus-visible. The
/// value-shaped twins preserve the visitor's field layout and `size_of` charges
/// while materializing the target's Arc-backed hot-store records afterward.
pub(crate) trait NativeRecordDecode: Sized {
    type Wire: DeserializeOwned + ClosedDecode;

    fn decode_native<M: NativeReadMeter + ?Sized>(
        bytes: &[u8],
        meter: &M,
    ) -> Result<Self, NativeReadError<M::Error>>;
}

impl<A: Clone + DeserializeOwned + ClosedDecode> NativeRecordDecode for Datum<A> {
    type Wire = DatumOracleDe<A>;

    fn decode_native<M: NativeReadMeter + ?Sized>(
        bytes: &[u8],
        meter: &M,
    ) -> Result<Self, NativeReadError<M::Error>> {
        let decoded = decode_history_record::<DatumOracleDe<A>, M>(bytes, meter)?;
        let backing = crate::rspace::native_backing::arc_allocation_bytes::<A>().ok_or(
            NativeReadError::Invalid(
                crate::rspace::history::native_reader::NativeReadFault::Overflow,
            ),
        )?;
        meter
            .reserve(crate::rspace::history::native_reader::NativeReadCharge {
                operations: 1,
                scanned_bytes: 0,
                backing_bytes: backing,
            })
            .map_err(NativeReadError::Host)?;
        Ok(decoded.into())
    }
}

impl<P: Clone + DeserializeOwned + ClosedDecode, K: Clone + DeserializeOwned + ClosedDecode>
    NativeRecordDecode for WaitingContinuation<P, K>
{
    type Wire = WaitingContinuationOracleDe<P, K>;

    fn decode_native<M: NativeReadMeter + ?Sized>(
        bytes: &[u8],
        meter: &M,
    ) -> Result<Self, NativeReadError<M::Error>> {
        let decoded = decode_history_record::<WaitingContinuationOracleDe<P, K>, M>(bytes, meter)?;
        let backing = crate::rspace::native_backing::arc_allocation_bytes::<Vec<P>>()
            .and_then(|bytes| {
                bytes.checked_add(crate::rspace::native_backing::arc_allocation_bytes::<K>()?)
            })
            .ok_or(NativeReadError::Invalid(
                crate::rspace::history::native_reader::NativeReadFault::Overflow,
            ))?;
        meter
            .reserve(crate::rspace::history::native_reader::NativeReadCharge {
                operations: 2,
                scanned_bytes: 0,
                backing_bytes: backing,
            })
            .map_err(NativeReadError::Host)?;
        Ok(decoded.into())
    }
}

impl<C: DeserializeOwned + ClosedDecode> NativeRecordDecode for Vec<C> {
    type Wire = Vec<C>;

    fn decode_native<M: NativeReadMeter + ?Sized>(
        bytes: &[u8],
        meter: &M,
    ) -> Result<Self, NativeReadError<M::Error>> {
        decode_history_record(bytes, meter)
    }
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

pub fn decode_continuations_for_epoch<P, K>(
    epoch: EventCodecEpoch,
    bytes: &[u8],
) -> Result<Vec<WaitingContinuation<P, K>>, ColdStoreDecodeError>
where
    P: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    let encoded: Vec<Vec<u8>> = legacy_tail(bytes)?;
    encoded
        .iter()
        .map(|item| decode_continuation_for_epoch(epoch, item))
        .collect()
}

pub fn encode_joins<C: Clone + Serialize>(joins: &Vec<Vec<C>>) -> Vec<u8> {
    let mut serialized_joins: Vec<Vec<u8>> = joins
        .iter()
        .map(|datum| bincode::serialize(datum).expect("Serializers: Unable to serialize join"))
        .collect();

    serialized_joins.sort_by(compare_byte_vectors);
    bincode::serialize(&serialized_joins).expect("Serializers: Unable to serialize joins")
}

pub fn encode_joins_for_epoch<C: Clone + StableHashSerialize>(
    epoch: EventCodecEpoch,
    joins: &[Vec<C>],
) -> Result<Vec<u8>, RSpaceError> {
    let mut encoded = Vec::with_capacity(joins.len());
    for join in joins {
        let count = u64::try_from(join.len()).map_err(|_| RSpaceError::HostWorkRejected)?;
        let mut bytes = count.to_le_bytes().to_vec();
        for channel in join {
            bytes.extend(event_leg_bytes_for_epoch(epoch, channel)?);
        }
        encoded.push(bytes);
    }
    encoded.sort_by(compare_byte_vectors);
    Ok(bincode::serialize(&encoded).expect("join leaf serialization"))
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

pub fn decode_joins_for_epoch<C: Clone + ColdStoreDecode>(
    epoch: EventCodecEpoch,
    bytes: &[u8],
) -> Result<Vec<Vec<C>>, ColdStoreDecodeError> {
    let encoded: Vec<Vec<u8>> = legacy_tail(bytes)?;
    encoded
        .iter()
        .map(|item| cold_decode_vec_prefix_for_epoch::<C>(epoch, item).map(|(join, _)| join))
        .collect()
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
    use std::cell::RefCell;

    use super::*;
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use crate::rspace::history::native_reader::{NativeReadCharge, NativeReadFault};
    use crate::rspace::native_backing::arc_allocation_bytes;

    // These are the committed source branch's value-shaped record layouts.
    // Their derived visitor is the independent native-meter oracle.
    #[derive(Serialize, Deserialize)]
    struct LegacyDatum<A> {
        a: A,
        persist: bool,
        source: Produce,
    }

    unsafe impl<A: ClosedDecode> ClosedDecode for LegacyDatum<A> {}

    #[derive(Serialize, Deserialize)]
    struct LegacyContinuation<P, K> {
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        #[serde(deserialize_with = "crate::rspace::history::native_reader::tree_set")]
        peeks: BTreeSet<i32>,
        source: Consume,
    }

    unsafe impl<P: ClosedDecode, K: ClosedDecode> ClosedDecode for LegacyContinuation<P, K> {}

    struct TraceMeter {
        charges: RefCell<Vec<NativeReadCharge>>,
        limit: Option<usize>,
    }

    impl TraceMeter {
        fn new(limit: Option<usize>) -> Self {
            Self {
                charges: RefCell::new(Vec::new()),
                limit,
            }
        }
    }

    impl NativeReadMeter for TraceMeter {
        type Error = &'static str;

        fn reserve(&self, charge: NativeReadCharge) -> Result<(), Self::Error> {
            let mut charges = self.charges.borrow_mut();
            if self.limit.is_some_and(|limit| charges.len() >= limit) {
                return Err("budget");
            }
            charges.push(charge);
            Ok(())
        }
    }

    struct SourceTraceMeter(RefCell<Vec<(usize, usize, usize)>>);

    impl SourceTraceMeter {
        fn new() -> Self { Self(RefCell::new(Vec::new())) }
    }

    impl SourceMeter for SourceTraceMeter {
        fn reserve(
            &self,
            operations: usize,
            scanned: usize,
            backing: usize,
        ) -> Result<(), RSpaceError> {
            self.0.borrow_mut().push((operations, scanned, backing));
            Ok(())
        }
    }

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

    #[test]
    fn native_record_adapter_preserves_source_visitor_charges_and_budget_order() {
        let datum_bytes = encode_datum(&datum());
        let continuation_bytes =
            bincode::serialize(&continuation_ser(&continuation())).expect("encode");
        assert_eq!(
            std::mem::size_of::<DatumOracleDe<String>>(),
            std::mem::size_of::<LegacyDatum<String>>()
        );
        assert_eq!(
            std::mem::size_of::<WaitingContinuationOracleDe<String, String>>(),
            std::mem::size_of::<LegacyContinuation<String, String>>()
        );

        let source = TraceMeter::new(None);
        let source_datum: LegacyDatum<String> =
            decode_history_record(&datum_bytes, &source).expect("source datum");
        let integrated = TraceMeter::new(None);
        let integrated_datum =
            Datum::<String>::decode_native(&datum_bytes, &integrated).expect("integrated datum");
        assert_eq!(integrated_datum.a.as_ref(), &source_datum.a);
        assert_eq!(integrated_datum.persist, source_datum.persist);
        assert_eq!(integrated_datum.source, source_datum.source);
        let integrated_charges = integrated.charges.borrow();
        let source_charges = source.charges.borrow();
        assert_eq!(&integrated_charges[..source_charges.len()], source_charges.as_slice());
        assert_eq!(integrated_charges[source_charges.len()], NativeReadCharge {
            operations: 1,
            scanned_bytes: 0,
            backing_bytes: arc_allocation_bytes::<String>().unwrap(),
        });
        assert_eq!(integrated_charges.len(), source_charges.len() + 1);

        let source_cont = TraceMeter::new(None);
        let source_wire: LegacyContinuation<String, String> =
            decode_history_record(&continuation_bytes, &source_cont).expect("source continuation");
        let integrated_cont = TraceMeter::new(None);
        let integrated_wire = WaitingContinuation::<String, String>::decode_native(
            &continuation_bytes,
            &integrated_cont,
        )
        .expect("integrated continuation");
        assert_eq!(integrated_wire.patterns.as_ref(), &source_wire.patterns);
        assert_eq!(integrated_wire.continuation.as_ref(), &source_wire.continuation);
        assert_eq!(integrated_wire.persist, source_wire.persist);
        assert_eq!(integrated_wire.peeks, source_wire.peeks);
        assert_eq!(integrated_wire.source, source_wire.source);
        let integrated_charges = integrated_cont.charges.borrow();
        let source_charges = source_cont.charges.borrow();
        assert_eq!(&integrated_charges[..source_charges.len()], source_charges.as_slice());
        assert_eq!(integrated_charges[source_charges.len()], NativeReadCharge {
            operations: 2,
            scanned_bytes: 0,
            backing_bytes: arc_allocation_bytes::<Vec<String>>().unwrap() +
                arc_allocation_bytes::<String>().unwrap(),
        });
        assert_eq!(integrated_charges.len(), source_charges.len() + 1);

        let limit = source.charges.borrow().len() / 2;
        let source_limited = TraceMeter::new(Some(limit));
        let integrated_limited = TraceMeter::new(Some(limit));
        assert!(matches!(
            decode_history_record::<LegacyDatum<String>, _>(&datum_bytes, &source_limited),
            Err(NativeReadError::Host("budget"))
        ));
        assert!(matches!(
            Datum::<String>::decode_native(&datum_bytes, &integrated_limited),
            Err(NativeReadError::Host("budget"))
        ));
        assert_eq!(*integrated_limited.charges.borrow(), *source_limited.charges.borrow());
    }

    #[test]
    fn native_record_encoding_preserves_source_bytes_and_reservation_order() {
        let value = datum();
        let old = LegacyDatum {
            a: value.a.as_ref().clone(),
            persist: value.persist,
            source: value.source.clone(),
        };
        let old_meter = SourceTraceMeter::new();
        let new_meter = SourceTraceMeter::new();
        assert_eq!(
            native_source::encode(&old, &old_meter).expect("source encoding"),
            encode_datum_metered(&value, &new_meter).expect("integrated encoding"),
        );
        assert_eq!(*old_meter.0.borrow(), *new_meter.0.borrow());

        let value = continuation();
        let old = LegacyContinuation {
            patterns: value.patterns.as_ref().clone(),
            continuation: value.continuation.as_ref().clone(),
            persist: value.persist,
            peeks: value.peeks.clone(),
            source: value.source.clone(),
        };
        let old_meter = SourceTraceMeter::new();
        let new_meter = SourceTraceMeter::new();
        assert_eq!(
            native_source::encode(&old, &old_meter).expect("source encoding"),
            encode_continuation_metered(&value, &new_meter).expect("integrated encoding"),
        );
        assert_eq!(*old_meter.0.borrow(), *new_meter.0.borrow());
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    enum Nested {
        End,
        Next(Vec<Nested>),
    }

    unsafe impl ClosedDecode for Nested {}

    #[test]
    fn native_record_adapter_preserves_source_depth_rejection() {
        let mut shallow = Nested::End;
        for _ in 0..8 {
            shallow = Nested::Next(vec![shallow]);
        }
        let shallow_datum = Datum {
            a: Arc::new(shallow),
            persist: false,
            source: datum().source,
        };
        let shallow_bytes = encode_datum(&shallow_datum);
        let shallow_source = TraceMeter::new(None);
        let source_value: LegacyDatum<Nested> =
            decode_history_record(&shallow_bytes, &shallow_source).expect("shallow source datum");
        let shallow_integrated = TraceMeter::new(None);
        let integrated_value = Datum::<Nested>::decode_native(&shallow_bytes, &shallow_integrated)
            .expect("shallow integrated datum");
        assert_eq!(integrated_value.a.as_ref(), &source_value.a);
        let integrated_charges = shallow_integrated.charges.borrow();
        let source_charges = shallow_source.charges.borrow();
        assert_eq!(&integrated_charges[..source_charges.len()], source_charges.as_slice());
        assert_eq!(integrated_charges.len(), source_charges.len() + 1);

        let mut nested = Nested::End;
        for _ in 0..256 {
            nested = Nested::Next(vec![nested]);
        }
        let datum = Datum {
            a: Arc::new(nested),
            persist: false,
            source: datum().source,
        };
        let bytes = encode_datum(&datum);
        let source = TraceMeter::new(None);
        let integrated = TraceMeter::new(None);
        assert!(matches!(
            decode_history_record::<LegacyDatum<Nested>, _>(&bytes, &source),
            Err(NativeReadError::Invalid(NativeReadFault::Depth))
        ));
        assert!(matches!(
            Datum::<Nested>::decode_native(&bytes, &integrated),
            Err(NativeReadError::Invalid(NativeReadFault::Depth))
        ));
        assert_eq!(*integrated.charges.borrow(), *source.charges.borrow());
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
