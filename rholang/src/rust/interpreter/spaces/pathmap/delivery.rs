//! The delivery planner of a PathMap space (P5e: plan "PathMap delivery", decisions 9.3, 37, 40,
//! 41, 42 and 45, F4, P5 design v2 3.4 and v3 A.11).
//!
//! The planner holds the pure parts of a PathMap delivery. It reads a staged view that the caller
//! passes in ([`PrefixView`]), and it stages nothing. The engine of the space (the prefix source
//! of the join engine) matches, guards and stages with the plans of this module.
//!
//! ```text
//! CONSUME-CANDIDATES(q):                     ▷ the candidates of a receive on the prefix q
//!   for each marker m on I(q):               ▷ one marker for each stored datum under q
//!     (src, s) ← decode(m); key ← path_key(s)      ▷ path_key(q ++ s) = prefix_key(q) ++ key
//!   group the markers by key, in byte order        ▷ the exact path (s = ε, key 0x00) first
//!   for each group with the suffix s, p = q ++ s:
//!     pair each record of D(p) with its marker by src      ▷ marker invariant, or Inconsistent
//!     order the records of D(p) by the discipline          ▷ bag, set: DR-75. a head shape: heads
//!   return the paths in key order, each with its records in discipline order
//!
//! PRODUCE-WAITERS(p):                         ▷ the waiters that a send on the path p meets
//!   for q in [p, parent(p), …, ε]:            ▷ the most specific prefix first
//!     the waiters of I(q) in the waiter order of the discipline, with the suffix p∖q
//!
//! REROOT(s, d):  R(s, v) for each argument v of d, with the random state, the cost authority
//!                and the cost stack of d       ▷ a pattern matches the re-rooted datum
//!
//! STORE-MARKERS(d at p):  for k in 0..=|p|: the marker of d on the prefix of depth k
//!                                              ▷ in depth order, ε first (plan, "Events")
//!
//! RETIRE-PLAN(d at p, delivered at depth k):   ▷ a removal of d after a delivery at depth k
//!   the datum record D(p):r, then the marker of each depth j ≠ k in depth order
//! ```
//!
//! **Peeks (decision 45, Rule E).** A consume-side peek on the prefix `q` fires one COMM on
//! `I(q)` with the stored marker `m_q`, and the COMM removes `m_q`, as the host COMM removes a
//! peeked datum. A persistent datum is never removed, so it has no second send. For a datum `d`
//! that is not persistent, the same transaction sends `d` again exactly as the host's second
//! send. The second send of a datum on `p` is a send on `p`, so it meets the waiters of
//! [`produce_waiters`] in their order, re-rooted, with the identity and the authority of `d`. A
//! fire of a waiter that does not peek spends `d`, and [`retire_plan`] with `delivered = k`
//! retires the record of `d` and its other markers. A peek waiter keeps `d` in flight
//! (decision 30). When no waiter fires, the engine stores `m_q` again with its identity, so `d` is
//! stored again with all of its markers. The peek bumps no counter for the second send. This is
//! the analogue of RESTORE-SCAN and RETIRE-CLASS of the scan driver: the record of `d` and its
//! markers form the class of `d`.
//!
//! **The marker invariant.** Every stored datum `d` on `p` has exactly one marker on `I(q)` for
//! each `q ⪯ p`, with the suffix `p∖q` and the source hash of `d`, and every marker belongs to a
//! stored datum. A store adds [`store_markers`], and a removal retires the plan of
//! [`retire_plan`] with the marker that the delivery COMM removed. The planner checks the
//! invariant on each path that it reads, and a break is `Inconsistent`
//! (`PathMapPrefix.v`, `marker_invariant_preserved`).
//!
//! **Determinism.** Every order of the planner is a function of the entry sets of the channels:
//! the path keys, the source hashes, the view indices only as the last tie-break, and the order
//! of the discipline, which DR-75 or the orders of the envelopes give. So play and replay plan the
//! same delivery.
//!
//! **Metering (decision 42).** The planner reserves each marker decode, each path-key encode,
//! each key comparison, each suffix copy and each buffer on the meter of the job before it makes
//! it. The orders of the disciplines reserve their own work.

use std::cmp::Ordering;
use std::mem::size_of;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostAuthority, ListParWithRandom, Par};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::space_store_handle::{
    canonical_continuation_order, canonical_data_order,
};
use serde::Serialize;
use shared::rust::fallible_sort;

use super::adapter::EPathMapAdapter;
use super::markers::{
    decode_marker_metered, marker_authority, marker_random, marker_record_metered, SOURCE_LEN,
};
use super::path_codec::{compare_keys, path_key, EncodedPath};
use super::reroot::reroot_payload;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{Discipline, Ordered, ParDatum, ParWaiter, Side};

/// The staged view of a PathMap space, as the planner reads it. The engine implements it over
/// the staged view of its transaction, and a key names its channel in the space of the
/// operation. Each method returns the entries of one channel in the list order of the store.
pub trait PrefixView {
    /// The records of the index channel `I(sid, prefix_key)`: the markers of the prefix.
    fn markers(&mut self, prefix_key: &[u8]) -> Result<&[ParDatum], SpaceError>;

    /// The records of the data channel `D(sid, path_key)`: the datums of the path.
    fn records(&mut self, path_key: &[u8]) -> Result<&[ParDatum], SpaceError>;

    /// The waiting continuations of the index channel `I(sid, prefix_key)`: the waiters on the
    /// prefix.
    fn waiters(&mut self, prefix_key: &[u8]) -> Result<&[ParWaiter], SpaceError>;
}

/// One stored datum of a path, as a receive on a prefix of the path sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PathEntry {
    /// The index of the datum record in the staged view of `D(p)`.
    pub record_index: usize,
    /// The index of the marker of the datum in the staged view of `I(q)`.
    pub marker_index: usize,
    /// The source hash of the datum: the hash of its source produce on `D(p)`.
    pub source: Blake2b256Hash,
}

/// The stored datums of one path `p ⪰ q` that a receive on `q` can meet, in the order of the
/// discipline.
#[derive(Clone, Debug, PartialEq)]
pub struct PathCandidates {
    /// The path key of `p`: the prefix key of `q` and the path key of the suffix.
    pub path_key: Vec<u8>,
    /// The suffix `p∖q`, which re-roots the payloads of the datums.
    pub suffix: Vec<Par>,
    /// The datums that the discipline offers, in its order.
    pub entries: Vec<PathEntry>,
}

/// The waiters of one prefix `q ⪯ p` that a send on `p` meets.
#[derive(Clone, Debug, PartialEq)]
pub struct PrefixWaiters {
    /// The number of elements of `q`.
    pub depth: usize,
    /// The prefix key of `q`.
    pub prefix_key: Vec<u8>,
    /// The suffix `p∖q`, which re-roots the payload of the datum for these waiters.
    pub suffix: Vec<Par>,
    /// The indices of the waiters in the staged view of `I(q)`, in the waiter order of the
    /// discipline.
    pub waiters: Vec<usize>,
}

/// A marker that the store of a datum adds, with the depth of its prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedMarker {
    /// The number of elements of the prefix. The marker lives on the index channel of the prefix
    /// key of this depth.
    pub depth: usize,
    /// The marker record.
    pub record: ListParWithRandom,
}

/// A record that the removal of a datum retires with a `K_retire` COMM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Retired {
    /// The datum record on `D(p)`.
    Datum,
    /// The marker of the datum on the prefix of this depth.
    Marker(usize),
}

/// The order of the records of one path or of the waiters of one prefix: a function from the
/// entries of one channel to the indices that the discipline offers, in its order.
pub type EntryOrder<'o, T> = dyn FnMut(&[T], &ScanMeter) -> Result<Vec<usize>, SpaceError> + 'o;

/// A decoded marker of one index channel with its key: the path key of its suffix.
struct KeyedMarker {
    key: Vec<u8>,
    index: usize,
    source: Blake2b256Hash,
}

/// The candidates of a receive on the prefix whose key is `prefix_key` (CONSUME-CANDIDATES): the
/// paths `p ⪰ q` with stored datums in path-key order, the exact path first, and the datums of
/// each path in the order that `data_order` gives (`PathMapPrefix.v`, `candidate_order_total`).
///
/// The markers of `I(q)` name the paths. Each record of `D(p)` pairs with the marker of `q` that
/// carries its source hash. A record without a marker, or a marker without a record, breaks the
/// marker invariant and fails with `Inconsistent`. `data_order` sees every record of the path and
/// returns the indices that the discipline offers: all of them in DR-75 order for a bag or a set,
/// and the heads for a head-only discipline.
pub fn consume_candidates<V: PrefixView>(
    view: &mut V,
    prefix_key: &[u8],
    data_order: &mut EntryOrder<'_, ParDatum>,
    meter: &ScanMeter,
) -> Result<Vec<PathCandidates>, SpaceError> {
    consume_candidates_excluding(view, prefix_key, &[], data_order, meter)
}

/// [`consume_candidates`] without the datums whose source hash is one of `excluded`: the datums
/// in flight of the transaction (seams arbitration, section 5). A second send or a lifted
/// element keeps its record and some of its markers in the store while it is in flight, and a
/// peek or a lift removed one of its markers. So the planner leaves out every record and every
/// marker of such a datum, and the marker invariant holds for the other datums. Each comparison
/// with an excluded hash is reserved first.
pub fn consume_candidates_excluding<V: PrefixView>(
    view: &mut V,
    prefix_key: &[u8],
    excluded: &[Blake2b256Hash],
    data_order: &mut EntryOrder<'_, ParDatum>,
    meter: &ScanMeter,
) -> Result<Vec<PathCandidates>, SpaceError> {
    let mut keyed = keyed_markers(view.markers(prefix_key)?, meter)?;
    if !excluded.is_empty() {
        let mut kept = meter.buffer::<KeyedMarker>(keyed.len())?;
        for marker in keyed {
            if !is_excluded(&marker.source, excluded, meter)? {
                kept.push(marker);
            }
        }
        keyed = kept;
    }
    sort_by_key(&mut keyed, meter)?;
    let groups = group_bounds(&keyed, meter)?;
    let mut paths = meter.buffer::<PathCandidates>(groups.len())?;
    for (start, end) in groups {
        let group = keyed.get(start..end).ok_or(SpaceError::Inconsistent(
            "a marker group lies outside the markers of a prefix",
        ))?;
        let first = group.first().ok_or(SpaceError::Inconsistent(
            "a marker group of a prefix is empty",
        ))?;
        let suffix = {
            let markers = view.markers(prefix_key)?;
            let record = markers.get(first.index).ok_or(SpaceError::Inconsistent(
                "a marker index lies outside the markers of a prefix",
            ))?;
            meter.copy_slice(decode_marker_metered(&record.a, meter)?.suffix)?
        };
        let path_key = joined_key(prefix_key, &first.key, meter)?;
        let records = view.records(&path_key)?;
        let pairs = pair_records(group, records, excluded, meter)?;
        let order = data_order(records, meter)?;
        let mut entries = meter.buffer::<PathEntry>(order.len())?;
        for record_index in order {
            let paired = pairs.get(record_index).ok_or(SpaceError::Inconsistent(
                "a discipline order names a record outside its path",
            ))?;
            let Some(marker) = paired else {
                continue;
            };
            entries.push(PathEntry {
                record_index,
                marker_index: marker.index,
                source: meter.copy(&marker.source)?,
            });
        }
        paths.push(PathCandidates {
            path_key,
            suffix,
            entries,
        });
    }
    Ok(paths)
}

/// The decoded markers of one index channel with the path keys of their suffixes. Each decode and
/// each encode is reserved first.
fn keyed_markers(markers: &[ParDatum], meter: &ScanMeter) -> Result<Vec<KeyedMarker>, SpaceError> {
    let mut keyed = meter.buffer::<KeyedMarker>(markers.len())?;
    for (index, record) in markers.iter().enumerate() {
        let view = decode_marker_metered(&record.a, meter)?;
        let key = path_key(view.suffix, meter)?;
        meter.reserve_buffer::<u8>(SOURCE_LEN)?;
        keyed.push(KeyedMarker {
            key,
            index,
            source: view.source_hash(),
        });
    }
    Ok(keyed)
}

/// Sorts the markers of one index channel by the key of their suffix, then by their view index.
/// The moves and each comparison are reserved, as the adapter reserves its sort.
fn sort_by_key(keyed: &mut [KeyedMarker], meter: &ScanMeter) -> Result<(), SpaceError> {
    let moves = size_of::<KeyedMarker>()
        .checked_mul(2)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.charge(
        keyed.len(),
        keyed
            .len()
            .checked_mul(moves)
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
    )?;
    fallible_sort::sort(keyed, |left, right| {
        Ok::<_, SpaceError>(
            compare_keys(&left.key, &right.key, meter)?.then(left.index.cmp(&right.index)),
        )
    })
}

/// The bounds of the runs of equal keys in the sorted markers `keyed`. Each comparison of two
/// neighbors is reserved first.
fn group_bounds(
    keyed: &[KeyedMarker],
    meter: &ScanMeter,
) -> Result<Vec<(usize, usize)>, SpaceError> {
    let mut bounds = meter.buffer::<(usize, usize)>(keyed.len())?;
    let mut start = 0;
    for index in 1..=keyed.len() {
        let boundary = match (keyed.get(index - 1), keyed.get(index)) {
            (Some(left), Some(right)) => compare_keys(&left.key, &right.key, meter)?.is_ne(),
            _ => true,
        };
        if boundary {
            bounds.push((start, index));
            start = index;
        }
    }
    Ok(bounds)
}

/// The key `left ++ right` in one reserved buffer: the path key of `p = q ++ s` from the prefix
/// key of `q` and the path key of `s` (`PathMapPrefix.v`, `path_key_app`).
fn joined_key(left: &[u8], right: &[u8], meter: &ScanMeter) -> Result<Vec<u8>, SpaceError> {
    let length = left
        .len()
        .checked_add(right.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut key = meter.buffer::<u8>(length)?;
    meter.charge(1, length, 0)?;
    key.extend_from_slice(left);
    key.extend_from_slice(right);
    Ok(key)
}

/// The marker of each record of one path, by the position of the record in `records`, or `None`
/// for a record whose source hash is one of `excluded`. The markers of the group carry the source
/// hashes of the datums of the path, and the group holds no marker of an excluded datum. Each
/// other record pairs with an unused marker of its source hash, in view order, so a repeated
/// identity pairs once for each record. A record without a marker, or a marker left over, is
/// `Inconsistent`. Each comparison is reserved first.
fn pair_records<'g>(
    group: &'g [KeyedMarker],
    records: &[ParDatum],
    excluded: &[Blake2b256Hash],
    meter: &ScanMeter,
) -> Result<Vec<Option<&'g KeyedMarker>>, SpaceError> {
    let unpaired =
        SpaceError::Inconsistent("the markers of a prefix differ from the datums of a path");
    let mut used = meter.buffer::<bool>(group.len())?;
    used.resize(group.len(), false);
    let mut pairs = meter.buffer::<Option<&KeyedMarker>>(records.len())?;
    for record in records {
        if is_excluded(&record.source.hash, excluded, meter)? {
            pairs.push(None);
            continue;
        }
        let mut found = None;
        for (position, marker) in group.iter().enumerate() {
            meter.compare(SOURCE_LEN, SOURCE_LEN)?;
            let free = !used.get(position).copied().unwrap_or(true);
            if free && marker.source == record.source.hash {
                found = Some(position);
                break;
            }
        }
        let position = found.ok_or(unpaired.clone())?;
        if let Some(slot) = used.get_mut(position) {
            *slot = true;
        }
        pairs.push(Some(group.get(position).ok_or(unpaired.clone())?));
    }
    match used.iter().all(|taken| *taken) {
        true => Ok(pairs),
        false => Err(unpaired),
    }
}

/// True when `source` is one of `excluded`. Each comparison is reserved first.
pub fn is_excluded(
    source: &Blake2b256Hash,
    excluded: &[Blake2b256Hash],
    meter: &ScanMeter,
) -> Result<bool, SpaceError> {
    for other in excluded {
        meter.compare(source.0.len(), other.0.len())?;
        if other == source {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The waiters that a send on the path `path` meets (PRODUCE-WAITERS): the waiters of `I(p)`,
/// `I(parent(p))`, …, `I(ε)`, the most specific prefix first, each with the suffix `p∖q` and in
/// the order that `waiter_order` gives (`PathMapPrefix.v`, `offer_list_complete` and
/// `send_prefixes_depths`). `encoded` is the encoded path of `path`. A prefix without waiters is
/// left out. `waiter_order` sees every waiter of the prefix and returns the indices that the
/// discipline offers: all of them in DR-75 order for a bag or a set, and the head for a head-only
/// discipline.
pub fn produce_waiters<V: PrefixView>(
    view: &mut V,
    path: &[Par],
    encoded: &EncodedPath,
    waiter_order: &mut EntryOrder<'_, ParWaiter>,
    meter: &ScanMeter,
) -> Result<Vec<PrefixWaiters>, SpaceError> {
    if encoded.depth() != path.len() {
        return Err(SpaceError::Inconsistent(
            "an encoded path has another depth than its path",
        ));
    }
    let count = path
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut prefixes = meter.buffer::<PrefixWaiters>(count)?;
    for depth in (0..=path.len()).rev() {
        let prefix_key = encoded.prefix_key(depth).ok_or(SpaceError::Inconsistent(
            "an encoded path has no prefix key of a depth",
        ))?;
        let waiters = view.waiters(prefix_key)?;
        if waiters.is_empty() {
            continue;
        }
        let order = waiter_order(waiters, meter)?;
        if order.iter().any(|index| *index >= waiters.len()) {
            return Err(SpaceError::Inconsistent(
                "a discipline order names a waiter outside its prefix",
            ));
        }
        let suffix = path
            .get(depth..)
            .ok_or(SpaceError::Inconsistent("a prefix is deeper than its path"))?;
        prefixes.push(PrefixWaiters {
            depth,
            prefix_key: meter.copy_slice(prefix_key)?,
            suffix: meter.copy_slice(suffix)?,
            waiters: order,
        });
    }
    Ok(prefixes)
}

/// The datum `datum` as a receive on a prefix sees it (REROOT): `R(suffix, v)` for each argument
/// `v`, with the random state, the cost authority and the cost stack of `datum`. The payload moves
/// into the re-rooting, and the adapter reserves its work (`reroot_payload`).
pub fn rerooted<A: EPathMapAdapter>(
    suffix: &[Par],
    datum: ListParWithRandom,
    adapter: &A,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    let ListParWithRandom {
        pars,
        random_state,
        cost_authority,
        cost_stack,
    } = datum;
    Ok(ListParWithRandom {
        pars: reroot_payload(suffix, pars, adapter, meter)?,
        random_state,
        cost_authority,
        cost_stack,
    })
}

/// The markers that the store of a datum on the path `path` adds (STORE-MARKERS), in depth
/// order: the marker of the prefix of depth `k` holds the suffix `path[k..]`, the random state
/// `marker_random(datum_random, k)` and the cost authority `cost_authority`. `source` is the
/// source hash of the datum on `D(p)`. Each suffix copy and each marker is reserved first.
pub fn store_markers(
    path: &[Par],
    source: &Blake2b256Hash,
    datum_random: &Blake2b512Random,
    cost_authority: Option<&CostAuthority>,
    meter: &ScanMeter,
) -> Result<Vec<PlannedMarker>, SpaceError> {
    let count = path
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let mut planned = meter.buffer::<PlannedMarker>(count)?;
    for depth in 0..=path.len() {
        let suffix = path
            .get(depth..)
            .ok_or(SpaceError::Inconsistent("a prefix is deeper than its path"))?;
        let random = marker_random(datum_random, depth)?;
        let record = marker_record_metered(
            source,
            meter.copy_slice(suffix)?,
            &random,
            cost_authority,
            meter,
        )?;
        planned.push(PlannedMarker { depth, record });
    }
    Ok(planned)
}

/// The markers of the datum whose record on `D(p)` is `datum_record` and whose source hash is
/// `source`, for its store on the path `path` (STORE-MARKERS): [`store_markers`] with the random
/// state of the record and the authority of [`marker_authority`]. The restore of a marker uses
/// [`datum_marker`], which builds the marker of one depth from the same parts, so a restored
/// marker has the bytes of the stored one.
pub fn datum_markers(
    path: &[Par],
    source: &Blake2b256Hash,
    datum_record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<Vec<PlannedMarker>, SpaceError> {
    let datum_random = Blake2b512Random::from_bytes(&datum_record.random_state);
    store_markers(
        path,
        source,
        &datum_random,
        marker_authority(datum_record),
        meter,
    )
}

/// The marker of depth `depth` of the datum whose record on `D(p)` is `datum_record` and whose
/// source hash is `source`, on the path `path`: the marker that [`datum_markers`] stores at that
/// depth, built again. A peek or a lift removed it, and the second send restores it with these
/// bytes, so the restored marker keeps its produce identity. The suffix copy and the marker are
/// reserved first. A depth below the path is `Inconsistent`.
pub fn datum_marker(
    path: &[Par],
    source: &Blake2b256Hash,
    datum_record: &ListParWithRandom,
    depth: usize,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, SpaceError> {
    let suffix = path
        .get(depth..)
        .ok_or(SpaceError::Inconsistent("a prefix is deeper than its path"))?;
    let datum_random = Blake2b512Random::from_bytes(&datum_record.random_state);
    let random = marker_random(&datum_random, depth)?;
    marker_record_metered(
        source,
        meter.copy_slice(suffix)?,
        &random,
        marker_authority(datum_record),
        meter,
    )
}

/// The records that the removal of a datum on a path of `depth` elements retires (RETIRE-PLAN),
/// in staging order: the datum record, then the marker of each prefix in depth order. The marker
/// of the depth `delivered` is left out, because the delivery COMM on that prefix removed it. A
/// removal with no delivery COMM, for example after a produce-side fire of a second send whose
/// own marker a peek removed, leaves out that depth too (`PathMapPrefix.v`,
/// `marker_invariant_preserved`).
pub fn retire_plan(depth: usize, delivered: Option<usize>) -> Vec<Retired> {
    let mut plan = Vec::with_capacity(depth.checked_add(2).unwrap_or(depth));
    plan.push(Retired::Datum);
    plan.extend(
        (0..=depth)
            .filter(|marker| Some(*marker) != delivered)
            .map(Retired::Marker),
    );
    plan
}

/// The view index of the marker of the datum `source` among the markers `markers` of one index
/// channel, for its retirement. Each decode and each comparison is reserved first. A missing
/// marker breaks the marker invariant and is `Inconsistent`.
pub fn marker_index(
    markers: &[ParDatum],
    source: &Blake2b256Hash,
    meter: &ScanMeter,
) -> Result<usize, SpaceError> {
    for (index, record) in markers.iter().enumerate() {
        let view = decode_marker_metered(&record.a, meter)?;
        meter.compare(SOURCE_LEN, SOURCE_LEN)?;
        if view.is_of(source) {
            return Ok(index);
        }
    }
    Err(SpaceError::Inconsistent(
        "a stored datum has no marker on a prefix of its path",
    ))
}

/// The first `need` entries of `entries` in the head order of `discipline` on `side`: the head,
/// then the head of the other entries, and so on. It is the order within one path or one prefix
/// of a head-only discipline. `ord_of` decodes the order of an entry and reserves the decode. The
/// buffers and the head scans reserve their work, and the order depends on the entry set only.
pub fn head_order<D: Discipline, T: Serialize>(
    discipline: &D,
    side: Side,
    entries: &[T],
    ord_of: impl Fn(&T, &ScanMeter) -> Result<i64, SpaceError>,
    need: usize,
    meter: &ScanMeter,
) -> Result<Vec<usize>, SpaceError> {
    let mut ords = meter.buffer::<i64>(entries.len())?;
    for entry in entries {
        ords.push(ord_of(entry, meter)?);
    }
    let taken = need.min(entries.len());
    let mut order = meter.buffer::<usize>(taken)?;
    let mut remaining = meter.buffer::<usize>(entries.len())?;
    remaining.extend(0..entries.len());
    while order.len() < taken {
        let mut ordered = meter.buffer::<Ordered<'_, T>>(remaining.len())?;
        for index in &remaining {
            let (Some(ord), Some(entry)) = (ords.get(*index), entries.get(*index)) else {
                return Err(SpaceError::Inconsistent("a head scan lost an entry"));
            };
            ordered.push(Ordered { ord: *ord, entry });
        }
        let Some(head) = discipline.head_metered(side, &ordered, meter)? else {
            break;
        };
        if head >= remaining.len() {
            return Err(SpaceError::Inconsistent("a head lies outside its side"));
        }
        order.push(remaining.remove(head));
    }
    Ok(order)
}

/// The order of the records of one path for a bag or a set: every record in the canonical order
/// of DR-75 (decision 41), with the work reserved as the substrate reserves it.
pub fn scan_data_order(records: &[ParDatum], meter: &ScanMeter) -> Result<Vec<usize>, SpaceError> {
    Ok(canonical_data_order(records, meter)?)
}

/// The order of the waiters of one prefix for a bag or a set: every waiter in the canonical order
/// of DR-75 (decision 41), with the work reserved as the substrate reserves it.
pub fn scan_waiter_order(
    waiters: &[ParWaiter],
    meter: &ScanMeter,
) -> Result<Vec<usize>, SpaceError> {
    Ok(canonical_continuation_order(waiters, meter)?)
}

/// The byte order of two candidates of a receive: by the path key, then by the rank of the
/// discipline within the path. It is the order `cand_lt` of `PathMapPrefix.v`, which
/// [`consume_candidates`] produces. The comparison of the keys is reserved first.
pub fn candidate_cmp(
    left: (&[u8], usize),
    right: (&[u8], usize),
    meter: &ScanMeter,
) -> Result<Ordering, SpaceError> {
    Ok(compare_keys(left.0, right.0, meter)?.then(left.1.cmp(&right.1)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{BindPattern, ParWithRandom, TaggedContinuation};
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;
    use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

    use super::super::adapter::HeadEPathMapAdapter;
    use super::super::markers::decode_marker;
    use super::super::path_codec::{encode_path, prefix_key};
    use super::super::reroot::reroot;
    use super::super::test_support::*;
    use super::*;
    use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
    use crate::rust::interpreter::spaces::disciplines::stack::StackDiscipline;
    use crate::rust::interpreter::spaces::generic::OuterStore;
    use crate::rust::interpreter::spaces::namespace::{
        self, CollectionKey, Durability, NativeSpace, SID_LEN,
    };
    use crate::rust::interpreter::spaces::outer::pathmap::PathMapOuter;
    use crate::rust::interpreter::spaces::pathmap::PATH_DEPTH_CAP;
    use crate::rust::interpreter::spaces::record;
    use crate::rust::interpreter::spaces::types::config::SpaceConfig;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    const SPACE: NativeSpace = NativeSpace {
        sid: [5; SID_LEN],
        durability: Durability::Durable,
    };

    /// A staged view in memory: the entries of the data and index channels of one space, by key.
    #[derive(Clone, Debug, Default)]
    struct MemoryView {
        data: BTreeMap<Vec<u8>, Vec<ParDatum>>,
        index: BTreeMap<Vec<u8>, Vec<ParDatum>>,
        waiters: BTreeMap<Vec<u8>, Vec<ParWaiter>>,
    }

    impl PrefixView for MemoryView {
        fn markers(&mut self, prefix_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
            Ok(self.index.get(prefix_key).map(Vec::as_slice).unwrap_or(&[]))
        }

        fn records(&mut self, path_key: &[u8]) -> Result<&[ParDatum], SpaceError> {
            Ok(self.data.get(path_key).map(Vec::as_slice).unwrap_or(&[]))
        }

        fn waiters(&mut self, prefix_key: &[u8]) -> Result<&[ParWaiter], SpaceError> {
            Ok(self
                .waiters
                .get(prefix_key)
                .map(Vec::as_slice)
                .unwrap_or(&[]))
        }
    }

    /// A stored datum of the model: its path, its record and its source.
    #[derive(Clone, Debug)]
    struct Stored {
        path: Vec<Par>,
        record: ListParWithRandom,
        source: Produce,
    }

    fn key_of(path: &[Par]) -> Vec<u8> { path_key(path, &unmetered()).expect("a path key") }

    fn prefix_of(path: &[Par], depth: usize) -> Vec<u8> {
        prefix_key(&path[..depth], &unmetered()).expect("a prefix key")
    }

    fn datum(value: i64, seed: u64) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![gint(value)],
            random_state: Blake2b512Random::create_from_bytes(&seed.to_be_bytes()).to_bytes(),
            cost_authority: None,
            cost_stack: None,
        }
    }

    impl MemoryView {
        /// Stores the datum `sent` on `path` with its markers, as the engine stages a store.
        fn store(&mut self, path: &[Par], sent: &ListParWithRandom, ord: i64) -> Stored {
            let record = record::wrap_datum(ord, sent);
            let channel = PathMapOuter::path_data(SPACE, &key_of(path));
            let source = Produce::create(&channel, &record, false);
            self.data.entry(key_of(path)).or_default().insert(0, Datum {
                a: record.clone(),
                persist: false,
                source: source.clone(),
            });
            let random = Blake2b512Random::from_bytes(&sent.random_state);
            for planned in
                store_markers(path, &source.hash, &random, None, &unmetered()).expect("markers")
            {
                let key = prefix_of(path, planned.depth);
                let index = PathMapOuter::index(SPACE, &key);
                let marker_source = Produce::create(&index, &planned.record, false);
                self.index.entry(key).or_default().insert(0, Datum {
                    a: planned.record,
                    persist: false,
                    source: marker_source,
                });
            }
            Stored {
                path: path.to_vec(),
                record,
                source,
            }
        }

        /// Removes the stored datum `stored` after a delivery on the prefix of depth
        /// `delivered`: the delivery COMM removes that marker, and the retire plan removes the
        /// record and the other markers.
        fn remove(&mut self, stored: &Stored, delivered: Option<usize>) {
            if let Some(depth) = delivered {
                self.take_marker(stored, depth);
            }
            for retired in retire_plan(stored.path.len(), delivered) {
                match retired {
                    Retired::Datum => {
                        let records = self
                            .data
                            .get_mut(&key_of(&stored.path))
                            .expect("the path holds the datum");
                        let position = records
                            .iter()
                            .position(|record| record.source == stored.source)
                            .expect("the datum is stored");
                        records.remove(position);
                    }
                    Retired::Marker(depth) => {
                        self.take_marker(stored, depth);
                    }
                }
            }
        }

        /// Removes the marker of `stored` on the prefix of depth `depth` and returns it.
        fn take_marker(&mut self, stored: &Stored, depth: usize) -> ParDatum {
            let markers = self
                .index
                .get_mut(&prefix_of(&stored.path, depth))
                .expect("the prefix holds markers");
            let position = marker_index(markers, &stored.source.hash, &unmetered())
                .expect("the marker is stored");
            markers.remove(position)
        }

        /// A consume-side peek on the prefix of depth `depth` whose second send meets no waiter:
        /// the COMM removes the marker, and the engine stores it again with its identity.
        fn peek_restore(&mut self, stored: &Stored, depth: usize) {
            let marker = self.take_marker(stored, depth);
            self.index
                .entry(prefix_of(&stored.path, depth))
                .or_default()
                .insert(0, marker);
        }

        /// Parks a waiter on the prefix `prefix`.
        fn park(&mut self, prefix: &[Par], tag: i64) {
            let continuation = TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(gint(tag)),
                    random_state: Vec::new(),
                })),
                guard: None,
                cost_authority: None,
            };
            let channel = PathMapOuter::index(SPACE, &prefix_of(prefix, prefix.len()));
            let pattern = BindPattern::default();
            let source =
                Consume::create(&vec![channel], &vec![pattern.clone()], &continuation, false);
            self.waiters
                .entry(prefix_of(prefix, prefix.len()))
                .or_default()
                .insert(0, WaitingContinuation {
                    patterns: vec![pattern],
                    continuation,
                    persist: false,
                    peeks: Default::default(),
                    source,
                });
        }
    }

    /// Checks the marker invariant: every stored datum has exactly one marker on each prefix of
    /// its path, with its suffix and its source, and no other marker exists.
    fn marker_invariant(view: &MemoryView, stored: &[Stored]) -> Result<(), String> {
        let mut expected: BTreeMap<Vec<u8>, Vec<(Vec<u8>, Vec<Par>)>> = BTreeMap::new();
        for datum in stored {
            for depth in 0..=datum.path.len() {
                expected
                    .entry(prefix_of(&datum.path, depth))
                    .or_default()
                    .push((datum.source.hash.bytes(), datum.path[depth..].to_vec()));
            }
        }
        for (key, markers) in &view.index {
            let mut actual: Vec<(Vec<u8>, Vec<Par>)> = markers
                .iter()
                .map(|marker| {
                    let view = decode_marker(&marker.a).expect("a stored marker decodes");
                    (view.source.to_vec(), view.suffix.to_vec())
                })
                .collect();
            let mut wanted = expected.remove(key).unwrap_or_default();
            actual.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            wanted.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            if actual != wanted {
                return Err(format!(
                    "the markers of {key:?} differ: {actual:?} != {wanted:?}"
                ));
            }
        }
        match expected.values().all(Vec::is_empty) {
            true => Ok(()),
            false => Err(format!("markers are missing: {expected:?}")),
        }
    }

    fn all_data(records: &[ParDatum], meter: &ScanMeter) -> Result<Vec<usize>, SpaceError> {
        scan_data_order(records, meter)
    }

    fn ints(values: &[i64]) -> Vec<Par> { values.iter().map(|value| gint(*value)).collect() }

    #[test]
    fn the_fips_paths_deliver_the_exact_path_first_and_rerooted() {
        let mut view = MemoryView::default();
        let hi = view.store(&ints(&[0, 1, 2]), &datum(42, 1), 0);
        let there = view.store(&ints(&[0, 1, 3]), &datum(7, 2), 0);
        let exact = view.store(&ints(&[0, 1]), &datum(5, 3), 0);
        let elsewhere = view.store(&ints(&[0, 2]), &datum(9, 4), 0);
        let paths = consume_candidates(
            &mut view,
            &prefix_of(&ints(&[0, 1]), 2),
            &mut all_data,
            &unmetered(),
        )
        .expect("the candidates of [0,1]");
        let suffixes: Vec<Vec<Par>> = paths.iter().map(|path| path.suffix.clone()).collect();
        assert_eq!(suffixes, vec![Vec::new(), ints(&[2]), ints(&[3])]);
        let sources: Vec<Blake2b256Hash> = paths
            .iter()
            .flat_map(|path| path.entries.iter().map(|entry| entry.source.clone()))
            .collect();
        assert_eq!(sources, vec![
            exact.source.hash.clone(),
            hi.source.hash.clone(),
            there.source.hash.clone()
        ]);
        assert!(!sources.contains(&elsewhere.source.hash));
        assert_eq!(paths[1].path_key, key_of(&ints(&[0, 1, 2])));
        let payload = record::unwrap_datum(&hi.record)
            .expect("a datum record")
            .datum;
        let seen = rerooted(
            &paths[1].suffix,
            payload,
            &HeadEPathMapAdapter,
            &unmetered(),
        )
        .expect("the datum re-roots");
        assert_eq!(seen.pars, vec![glist(ints(&[2, 42]))]);
        assert_eq!(seen.random_state, datum(42, 1).random_state);
        let root = consume_candidates(&mut view, &[], &mut all_data, &unmetered())
            .expect("the candidates of ε");
        assert_eq!(root.len(), 4);
        assert_eq!(root[0].suffix, ints(&[0, 1]));
    }

    #[test]
    fn the_waiters_of_a_send_are_the_most_specific_first() {
        let mut view = MemoryView::default();
        let path = ints(&[0, 1, 2]);
        view.park(&ints(&[]), 1);
        view.park(&ints(&[0, 1]), 2);
        view.park(&ints(&[0, 1, 2]), 3);
        view.park(&ints(&[0, 2]), 4);
        let encoded = encode_path(&path, &unmetered()).expect("the path encodes");
        let prefixes = produce_waiters(
            &mut view,
            &path,
            &encoded,
            &mut scan_waiter_order,
            &unmetered(),
        )
        .expect("the waiters of the send");
        let depths: Vec<usize> = prefixes.iter().map(|prefix| prefix.depth).collect();
        assert_eq!(depths, vec![3, 2, 0]);
        assert_eq!(prefixes[0].suffix, Vec::<Par>::new());
        assert_eq!(prefixes[1].suffix, ints(&[2]));
        assert_eq!(prefixes[2].suffix, ints(&[0, 1, 2]));
        assert_eq!(prefixes[2].prefix_key, Vec::<u8>::new());
    }

    #[test]
    fn a_removal_retires_the_record_and_every_other_marker() {
        assert_eq!(retire_plan(2, Some(1)), vec![
            Retired::Datum,
            Retired::Marker(0),
            Retired::Marker(2)
        ]);
        assert_eq!(retire_plan(0, None), vec![
            Retired::Datum,
            Retired::Marker(0)
        ]);
        assert_eq!(retire_plan(0, Some(0)), vec![Retired::Datum]);
        let mut view = MemoryView::default();
        let kept = view.store(&ints(&[0, 1]), &datum(1, 1), 0);
        let gone = view.store(&ints(&[0, 1]), &datum(2, 2), 0);
        view.remove(&gone, Some(1));
        marker_invariant(&view, std::slice::from_ref(&kept)).expect("the invariant holds");
        view.peek_restore(&kept, 0);
        marker_invariant(&view, std::slice::from_ref(&kept)).expect("a peek restores the marker");
    }

    #[test]
    fn a_broken_marker_invariant_is_inconsistent() {
        let mut view = MemoryView::default();
        let stored = view.store(&ints(&[0, 1]), &datum(1, 1), 0);
        view.store(&ints(&[0, 1]), &datum(2, 2), 0);
        // The other datum of the path names the path on I([0]), so the planner reads D([0,1])
        // and finds a record without its marker.
        let mut missing = view.clone();
        missing.take_marker(&stored, 1);
        assert_eq!(
            consume_candidates(
                &mut missing,
                &prefix_of(&ints(&[0, 1]), 1),
                &mut all_data,
                &unmetered()
            ),
            Err(SpaceError::Inconsistent(
                "the markers of a prefix differ from the datums of a path"
            ))
        );
        let mut orphan = view.clone();
        orphan.data.clear();
        assert_eq!(
            consume_candidates(&mut orphan, &[], &mut all_data, &unmetered()),
            Err(SpaceError::Inconsistent(
                "the markers of a prefix differ from the datums of a path"
            ))
        );
        assert_eq!(
            marker_index(&[], &stored.source.hash, &unmetered()),
            Err(SpaceError::Inconsistent(
                "a stored datum has no marker on a prefix of its path"
            ))
        );
    }

    #[test]
    fn head_orders_take_the_heads_of_a_path() {
        let mut view = MemoryView::default();
        let path = ints(&[4]);
        let older = view.store(&path, &datum(1, 1), 3);
        let newer = view.store(&path, &datum(2, 2), 8);
        let middle = view.store(&path, &datum(3, 3), 5);
        let ord_of =
            |datum: &ParDatum, meter: &ScanMeter| record::datum_ord_metered(&datum.a, meter);
        let records = view.data.get(&key_of(&path)).expect("the path").clone();
        let sources = |order: Vec<usize>| -> Vec<Produce> {
            order
                .into_iter()
                .map(|index| records[index].source.clone())
                .collect()
        };
        let queue = head_order(
            &QueueDiscipline,
            Side::Data,
            &records,
            ord_of,
            2,
            &unmetered(),
        )
        .expect("a queue order");
        assert_eq!(sources(queue), vec![
            older.source.clone(),
            middle.source.clone()
        ]);
        let stack = head_order(
            &StackDiscipline,
            Side::Data,
            &records,
            ord_of,
            5,
            &unmetered(),
        )
        .expect("a stack order");
        assert_eq!(sources(stack), vec![
            newer.source.clone(),
            middle.source.clone(),
            older.source.clone()
        ]);
        let mut queue_heads = |records: &[ParDatum], meter: &ScanMeter| {
            head_order(&QueueDiscipline, Side::Data, records, ord_of, 1, meter)
        };
        let paths = consume_candidates(&mut view, &[], &mut queue_heads, &unmetered())
            .expect("the heads of ε");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].entries.len(), 1);
        assert_eq!(paths[0].entries[0].source, older.source.hash);
    }

    #[test]
    fn the_planner_reserves_its_work() {
        let mut view = MemoryView::default();
        view.store(&ints(&[0, 1, 2]), &datum(42, 1), 0);
        view.store(&ints(&[0, 1]), &datum(5, 3), 0);
        view.park(&ints(&[0, 1]), 2);
        let path = ints(&[0, 1, 2]);
        assert_cut(|meter| {
            let mut copy = view.clone();
            consume_candidates(&mut copy, &prefix_of(&path, 1), &mut all_data, meter)
        });
        assert_cut(|meter| {
            let mut copy = view.clone();
            let encoded = encode_path(&path, meter)?;
            produce_waiters(&mut copy, &path, &encoded, &mut scan_waiter_order, meter)
        });
        let random = Blake2b512Random::create_from_bytes(b"datum");
        let hash = Blake2b256Hash::from_bytes(vec![1; SOURCE_LEN]);
        assert_cut(|meter| store_markers(&path, &hash, &random, None, meter));
        assert_cut(|meter| rerooted(&ints(&[2]), datum(42, 1), &HeadEPathMapAdapter, meter));
        let records = view
            .data
            .get(&key_of(&ints(&[0, 1])))
            .expect("a path")
            .clone();
        let ord_of =
            |datum: &ParDatum, meter: &ScanMeter| record::datum_ord_metered(&datum.a, meter);
        assert_cut(|meter| head_order(&QueueDiscipline, Side::Data, &records, ord_of, 1, meter));
    }

    /// One step of a random history of a PathMap space.
    #[derive(Clone, Debug)]
    enum Step {
        /// Store a datum on the path of these elements.
        Store(Vec<i64>),
        /// Remove the stored datum with this position after a delivery on this depth, or with no
        /// delivery COMM (a second send that a waiter took after a peek).
        Remove(usize, Option<usize>),
        /// A peek on this depth of the stored datum with this position whose second send meets
        /// no waiter.
        Peek(usize, usize),
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            3 => prop::collection::vec(0i64..3, 0..4).prop_map(Step::Store),
            2 => (0usize..8, prop::option::of(0usize..5)).prop_map(|(at, depth)| Step::Remove(at, depth)),
            1 => (0usize..8, 0usize..5).prop_map(|(at, depth)| Step::Peek(at, depth)),
        ]
    }

    // `marker_invariant_preserved` of `PathMapPrefix.v`: a store adds one marker for each prefix,
    // a removal retires the record and the other markers, and a peek whose second send meets no
    // waiter restores the marker of its COMM. The invariant holds after each step.
    #[test]
    fn prop_marker_invariant_preserved() {
        check(
            pure_config(),
            prop::collection::vec(step(), 0..12),
            |steps| {
                let mut view = MemoryView::default();
                let mut stored: Vec<Stored> = Vec::new();
                for (seed, step) in steps.into_iter().enumerate() {
                    match step {
                        Step::Store(elements) => stored.push(view.store(
                            &ints(&elements),
                            &datum(seed as i64, seed as u64),
                            0,
                        )),
                        Step::Remove(at, delivered) if !stored.is_empty() => {
                            let datum = stored.remove(at % stored.len());
                            let delivered = delivered.map(|depth| depth % (datum.path.len() + 1));
                            view.remove(&datum, delivered);
                        }
                        Step::Peek(at, depth) if !stored.is_empty() => {
                            let datum = stored[at % stored.len()].clone();
                            view.peek_restore(&datum, depth % (datum.path.len() + 1));
                        }
                        Step::Remove(..) | Step::Peek(..) => {}
                    }
                    marker_invariant(&view, &stored).map_err(TestCaseError::fail)?;
                }
                Ok(())
            },
        );
    }

    // `candidate_order_total` and `prefix_contact_anchored` of `PathMapPrefix.v`: the candidates of
    // a receive on q are exactly the stored datums under q, each with its marker on I(q), in
    // strictly ascending order of (path key, rank), with the exact path first.
    #[test]
    fn prop_candidates_cover_the_prefix_in_key_order() {
        check(
            pure_config(),
            (
                prop::collection::vec(prop::collection::vec(0i64..3, 0..4), 0..8),
                prop::collection::vec(0i64..3, 0..3),
            ),
            |(paths, prefix)| {
                let mut view = MemoryView::default();
                let stored: Vec<Stored> = paths
                    .iter()
                    .enumerate()
                    .map(|(seed, path)| {
                        view.store(&ints(path), &datum(seed as i64, seed as u64), 0)
                    })
                    .collect();
                let q = ints(&prefix);
                let q_key = prefix_of(&q, q.len());
                let candidates = consume_candidates(&mut view, &q_key, &mut all_data, &unmetered())
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let mut seen = Vec::new();
                let mut previous: Option<(Vec<u8>, usize)> = None;
                for path in &candidates {
                    ensure(
                        path.path_key.starts_with(&q_key),
                        "a candidate path extends q",
                    )?;
                    let full: Vec<Par> = [q.clone(), path.suffix.clone()].concat();
                    ensure(
                        path.path_key == key_of(&full),
                        "the key is the key of q ++ s",
                    )?;
                    for (rank, entry) in path.entries.iter().enumerate() {
                        let here = (path.path_key.clone(), rank);
                        if let Some(before) = &previous {
                            ensure(
                                candidate_cmp(
                                    (&before.0, before.1),
                                    (&here.0, here.1),
                                    &unmetered(),
                                )
                                .map_err(|error| TestCaseError::fail(error.to_string()))?
                                    == Ordering::Less,
                                "the candidates ascend",
                            )?;
                        }
                        let marker = &view.index[&q_key][entry.marker_index];
                        ensure(
                            decode_marker(&marker.a)
                                .map_err(|error| TestCaseError::fail(error.to_string()))?
                                .is_of(&entry.source),
                            "each candidate pairs with its marker on I(q)",
                        )?;
                        seen.push(entry.source.clone());
                        previous = Some(here);
                    }
                }
                let mut wanted: Vec<Blake2b256Hash> = stored
                    .iter()
                    .filter(|datum| datum.path.starts_with(&q))
                    .map(|datum| datum.source.hash.clone())
                    .collect();
                wanted.sort();
                seen.sort();
                ensure(seen == wanted, "the candidates are the datums under q")?;
                if let Some(first) = candidates.first() {
                    let exact = stored.iter().any(|datum| datum.path == q);
                    ensure(
                        !exact || first.suffix.is_empty(),
                        "the exact path comes first",
                    )?;
                }
                Ok(())
            },
        );
    }

    // `send_prefixes_spec` and `send_prefixes_depths` of `PathMapPrefix.v`: a send on p meets
    // the prefixes of p with waiters from the most specific to ε, each prefix once, and no
    // other prefix.
    #[test]
    fn prop_waiter_prefixes_are_most_specific_first() {
        check(
            pure_config(),
            (
                prop::collection::vec(prop::collection::vec(0i64..3, 0..4), 0..6),
                prop::collection::vec(0i64..3, 0..4),
            ),
            |(parked, path)| {
                let mut view = MemoryView::default();
                for (tag, prefix) in parked.iter().enumerate() {
                    view.park(&ints(prefix), tag as i64);
                }
                let p = ints(&path);
                let encoded = encode_path(&p, &unmetered())
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let prefixes = produce_waiters(
                    &mut view,
                    &p,
                    &encoded,
                    &mut scan_waiter_order,
                    &unmetered(),
                )
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let depths: Vec<usize> = prefixes.iter().map(|prefix| prefix.depth).collect();
                let wanted: Vec<usize> = (0..=p.len())
                    .rev()
                    .filter(|depth| parked.iter().any(|prefix| ints(prefix) == p[..*depth]))
                    .collect();
                ensure(
                    depths == wanted,
                    "the prefixes with waiters, most specific first",
                )?;
                for prefix in &prefixes {
                    ensure(
                        prefix.suffix == p[prefix.depth..],
                        "the suffix is p after q",
                    )?;
                    let count = parked
                        .iter()
                        .filter(|parked| ints(parked) == p[..prefix.depth])
                        .count();
                    ensure(
                        prefix.waiters.len() == count,
                        "every waiter of the prefix is offered",
                    )?;
                }
                Ok(())
            },
        );
    }

    // REROOT: the datum that a receive on q sees is R(p∖q, v) for each argument, with the
    // random state, the authority and the cost stack of the datum.
    #[test]
    fn prop_rerooted_datum_is_reroot_of_each_argument() {
        check(
            pure_config(),
            (
                path(),
                prop::collection::vec(prop_oneof![element(), map_value()], 0..3),
            ),
            |(suffix, payload)| {
                let sent = ListParWithRandom {
                    pars: payload.clone(),
                    random_state: vec![1, 2, 3],
                    cost_authority: None,
                    cost_stack: None,
                };
                let seen = rerooted(&suffix, sent.clone(), &HeadEPathMapAdapter, &unmetered())
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let wanted: Result<Vec<Par>, SpaceError> = payload
                    .into_iter()
                    .map(|value| reroot(&suffix, value, &HeadEPathMapAdapter, &unmetered()))
                    .collect();
                ensure(
                    seen.pars == wanted.map_err(|error| TestCaseError::fail(error.to_string()))?,
                    "each argument re-roots",
                )?;
                ensure(
                    seen.random_state == sent.random_state,
                    "the random state stays",
                )
            },
        );
    }

    /// A path element for the rule property: a scalar, a nested tuple, and sometimes a PathMap or
    /// a zipper at some depth, with a flag that tells whether the element holds one.
    fn rule_element() -> impl Strategy<Value = (Par, bool)> {
        let leaf = prop_oneof![
            4 => (0i64..5).prop_map(|value| (gint(value), false)),
            1 => Just((pathmap(Vec::new()), true)),
            1 => Just((zipper(Vec::new()), true)),
        ];
        leaf.prop_recursive(2, 8, 3, |inner| {
            prop::collection::vec(inner, 0..3).prop_map(|parts| {
                let holds = parts.iter().any(|(_, holds)| *holds);
                let elements = parts.into_iter().map(|(element, _)| element).collect();
                (gtuple(elements), holds)
            })
        })
    }

    // `path_element_rules` of `PathMapPrefix.v`: the store accepts a channel exactly when its
    // path has at most PATH_DEPTH_CAP elements and no element holds a PathMap or a zipper. Every
    // prefix of an accepted path is accepted too, so every index channel of a stored datum exists.
    #[test]
    fn prop_path_element_rules() {
        let config = SpaceConfig::new(
            SPACE.sid,
            PathMapOuter::NAME,
            "bag",
            SpaceQualifier::Default,
        );
        let checked = |path: &[Par]| -> Result<(), SpaceError> {
            let channel = glist(path.to_vec());
            let key = CollectionKey::of_channel(&channel)?;
            PathMapOuter.check_key(&config, &channel, &key, &unmetered())
        };
        check(
            pure_config(),
            (prop::collection::vec(rule_element(), 0..4), prop_oneof![
                Just(0usize),
                Just(PATH_DEPTH_CAP - 3),
                Just(PATH_DEPTH_CAP)
            ]),
            |(elements, padding)| {
                let holds = elements.iter().any(|(_, holds)| *holds);
                let mut path: Vec<Par> = (0..padding as i64).map(gint).collect();
                path.extend(elements.into_iter().map(|(element, _)| element));
                let expected = match (path.len() > PATH_DEPTH_CAP, holds) {
                    (true, _) => Err(SpaceError::PathTooDeep),
                    (false, true) => Err(SpaceError::UnsupportedPathElement),
                    (false, false) => Ok(()),
                };
                let result = checked(&path);
                ensure(result == expected, "the check follows the path rules")?;
                match result {
                    Ok(()) => {
                        for depth in 0..=path.len() {
                            ensure(
                                checked(&path[..depth]).is_ok(),
                                "every prefix of an accepted path is accepted",
                            )?;
                        }
                        Ok(())
                    }
                    Err(_) => Ok(()),
                }
            },
        );
    }

    #[test]
    fn the_index_channels_of_the_model_are_the_store_channels() {
        let path = ints(&[0, 1]);
        let encoded = encode_path(&path, &unmetered()).expect("the path encodes");
        let channels =
            PathMapOuter::path_channels(SPACE, &encoded, &unmetered()).expect("the channels");
        for depth in 0..=path.len() {
            assert_eq!(
                channels.index(depth),
                Some(&namespace::gprivate(namespace::path_index_channel(
                    SPACE,
                    &prefix_of(&path, depth)
                )))
            );
        }
    }
}
